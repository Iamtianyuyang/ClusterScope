use crate::models::UserRow;
use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

/// Refresh tokens are stored as SHA-256 digests of the raw token, so a
/// database leak does not expose usable session credentials. The raw token
/// (a 128-bit UUID) is only ever handed to the client.
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn create_user(
    pool: &PgPool,
    username: &str,
    email: Option<&str>,
    role: &str,
    password_hash: &str,
) -> Result<String> {
    let user_id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO users (user_id, username, email, role, password_hash, enabled)
        VALUES ($1, $2, $3, $4, $5, TRUE)
        "#,
    )
    .bind(&user_id)
    .bind(username)
    .bind(email)
    .bind(role)
    .bind(password_hash)
    .execute(pool)
    .await
    .context("Failed to create user")?;

    Ok(user_id)
}

pub async fn get_user_by_username(pool: &PgPool, username: &str) -> Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
        .context("Failed to get user by username")
}

pub async fn get_user_by_id(pool: &PgPool, user_id: &str) -> Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context("Failed to get user by id")
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT user_id, username, email, role, enabled, created_at, last_login_at, failed_login_attempts, locked_until FROM users ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list users")
}

pub async fn update_user(
    pool: &PgPool,
    user_id: &str,
    role: Option<&str>,
    enabled: Option<bool>,
    password_hash: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users SET
            role = COALESCE($2, role),
            enabled = COALESCE($3, enabled),
            password_hash = COALESCE($4, password_hash),
            -- A fresh password clears lockout state (admin reset or self-change).
            failed_login_attempts = CASE WHEN $4 IS NOT NULL THEN 0 ELSE failed_login_attempts END,
            locked_until = CASE WHEN $4 IS NOT NULL THEN NULL ELSE locked_until END
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .bind(role)
    .bind(enabled)
    .bind(password_hash)
    .execute(pool)
    .await
    .context("Failed to update user")?;

    Ok(())
}

/// Update a user with an ATOMIC last-enabled-admin guard.
///
/// Unlike the plain [`update_user`] (used by self-service password change,
/// which cannot remove the last admin), this runs the admin count and the
/// UPDATE in one transaction and locks every enabled-admin row with
/// `FOR UPDATE`: two concurrent demotions/disables serialize — the second
/// transaction's count query re-evaluates after the lock wait (READ
/// COMMITTED) and sees one fewer admin, so the cluster can never be left
/// with zero enabled admins.
///
/// Returns `false` when the change was refused (last-admin guard) or the
/// user does not exist; `true` when the update was applied.
pub async fn update_user_guarded(
    pool: &PgPool,
    user_id: &str,
    role: Option<&str>,
    enabled: Option<bool>,
    password_hash: Option<&str>,
) -> Result<bool> {
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;

    // NOTE (M6 graft): B's original statement was
    // `SELECT COUNT(*) ... FOR UPDATE`, which PostgreSQL rejects outright
    // ("FOR UPDATE is not allowed with aggregate functions"), so the guard
    // never ran. Locking the rows and counting them here keeps the intended
    // semantics: the lock serializes concurrent demotions, and the count is
    // taken inside the same transaction.
    let locked_admins: Vec<(String,)> = sqlx::query_as(
        "SELECT user_id FROM users WHERE role = 'admin' AND enabled = TRUE FOR UPDATE",
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to lock the enabled admins")?;
    let admin_count = locked_admins.len() as i64;

    let target: Option<(String, bool)> =
        sqlx::query_as("SELECT role, enabled FROM users WHERE user_id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .context("Failed to read target user")?;

    let Some((target_role, target_enabled)) = target else {
        tx.rollback().await.ok();
        return Ok(false);
    };

    let new_role = role.unwrap_or(target_role.as_str());
    let new_enabled = enabled.unwrap_or(target_enabled);
    let removes_last_admin = target_role == "admin"
        && target_enabled
        && (new_role != "admin" || !new_enabled)
        && admin_count <= 1;
    if removes_last_admin {
        tx.rollback().await.ok();
        return Ok(false);
    }

    sqlx::query(
        r#"
        UPDATE users SET
            role = COALESCE($2, role),
            enabled = COALESCE($3, enabled),
            password_hash = COALESCE($4, password_hash),
            failed_login_attempts = CASE WHEN $4 IS NOT NULL THEN 0 ELSE failed_login_attempts END,
            locked_until = CASE WHEN $4 IS NOT NULL THEN NULL ELSE locked_until END
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .bind(role)
    .bind(enabled)
    .bind(password_hash)
    .execute(&mut *tx)
    .await
    .context("Failed to update user")?;

    tx.commit().await.context("Failed to commit user update")?;
    Ok(true)
}

/// Delete a user with an ATOMIC last-enabled-admin guard (same transaction
/// and `FOR UPDATE` serialization as [`update_user_guarded`]). Also removes
/// the user's refresh tokens inside the same transaction (the FK has no ON
/// DELETE CASCADE). Returns `false` when refused (last admin) or when the
/// user does not exist.
pub async fn delete_user_guarded(pool: &PgPool, user_id: &str) -> Result<bool> {
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;

    // NOTE (M6 graft): B's original statement was
    // `SELECT COUNT(*) ... FOR UPDATE`, which PostgreSQL rejects outright
    // ("FOR UPDATE is not allowed with aggregate functions"), so the guard
    // never ran. Locking the rows and counting them here keeps the intended
    // semantics: the lock serializes concurrent demotions, and the count is
    // taken inside the same transaction.
    let locked_admins: Vec<(String,)> = sqlx::query_as(
        "SELECT user_id FROM users WHERE role = 'admin' AND enabled = TRUE FOR UPDATE",
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to lock the enabled admins")?;
    let admin_count = locked_admins.len() as i64;

    let target: Option<(String, bool)> =
        sqlx::query_as("SELECT role, enabled FROM users WHERE user_id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .context("Failed to read target user")?;

    let Some((target_role, target_enabled)) = target else {
        tx.rollback().await.ok();
        return Ok(false);
    };
    if target_role == "admin" && target_enabled && admin_count <= 1 {
        tx.rollback().await.ok();
        return Ok(false);
    }

    // refresh_tokens reference the user with no ON DELETE CASCADE — remove
    // them first or the DELETE fails with a FK violation (23503 -> 500).
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("Failed to delete refresh tokens")?;

    sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("Failed to delete user")?;

    tx.commit().await.context("Failed to commit user delete")?;
    Ok(true)
}

/// Drop revoked or expired refresh tokens so the table cannot grow without
/// bound (every login/refresh inserts a row, most are single-use).
pub async fn prune_refresh_tokens(pool: &PgPool) -> Result<()> {
    sqlx::query("DELETE FROM refresh_tokens WHERE revoked = TRUE OR expires_at < NOW()")
        .execute(pool)
        .await
        .context("Failed to prune refresh tokens")?;
    Ok(())
}

pub async fn record_login(pool: &PgPool, user_id: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users SET
            last_login_at = NOW(),
            failed_login_attempts = 0,
            locked_until = NULL
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await
    .context("Failed to record login")?;

    Ok(())
}

pub async fn record_failed_login(
    pool: &PgPool,
    username: &str,
    max_attempts: i32,
    lockout_secs: i64,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users SET
            failed_login_attempts = failed_login_attempts + 1,
            -- Lock at the threshold, but NEVER extend an in-progress lock:
            -- otherwise a determined attacker who knows a username could
            -- keep re-locking the account forever by failing a few attempts
            -- right before the window expires (lockout DoS). The lock stays
            -- for its original duration; after it expires the account is
            -- usable again until the next full batch of failures.
            locked_until = CASE
                WHEN failed_login_attempts + 1 >= $2
                     THEN CASE WHEN locked_until > NOW() THEN locked_until
                               ELSE NOW() + make_interval(secs => $3) END
                ELSE locked_until
            END
        WHERE username = $1
        "#,
    )
    .bind(username)
    .bind(max_attempts)
    .bind(lockout_secs)
    .execute(pool)
    .await
    .context("Failed to record failed login")?;

    Ok(())
}

/// Validate AND revoke a refresh token in one atomic statement: the UPDATE
/// only matches non-revoked, unexpired tokens, so two concurrent refreshes
/// with the same token cannot both succeed (a stolen token racing the
/// legitimate client gets exactly one pair minted). Returns the user_id when
/// the token was consumed, `None` when it was already used/revoked/expired.
pub async fn consume_refresh_token(pool: &PgPool, token: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        UPDATE refresh_tokens SET revoked = TRUE
        WHERE token = $1 AND revoked = FALSE AND expires_at > NOW()
        RETURNING user_id
        "#,
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("Failed to consume refresh token")?;
    Ok(row.map(|(uid,)| uid))
}

pub async fn add_refresh_token(
    pool: &PgPool,
    token: &str,
    user_id: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (token, user_id, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(hash_token(token))
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .context("Failed to add refresh token")?;

    Ok(())
}

pub async fn revoke_all_refresh_tokens(pool: &PgPool, user_id: &str) -> Result<()> {
    sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to revoke user's refresh tokens")?;

    Ok(())
}

/// C-side helper kept alongside B's digest scheme: mark a single refresh
/// token as revoked.
///
/// The token is matched through the same SHA-256 digest used by
/// [`add_refresh_token`], so the raw credential is never compared against (or
/// stored in) the database.
pub async fn revoke_refresh_token(pool: &PgPool, token: &str) -> Result<()> {
    sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token = $1")
        .bind(hash_token(token))
        .execute(pool)
        .await
        .context("Failed to revoke refresh token")?;

    Ok(())
}

/// C-side helper kept: read the user of a still-valid refresh token without
/// consuming it (the handler uses it to answer "who is this token for").
///
/// New call sites should prefer the atomic [`consume_refresh_token`], which
/// cannot be raced by a second use of the same token.
pub async fn validate_refresh_token(pool: &PgPool, token: &str) -> Result<Option<String>> {
    sqlx::query_as::<_, (String,)>(
        r#"
        SELECT user_id FROM refresh_tokens
        WHERE token = $1 AND revoked = FALSE AND expires_at > NOW()
        "#,
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("Failed to validate refresh token")
    .map(|r| r.map(|(uid,)| uid))
}

/// C-side helper kept: plain delete without the last-enabled-admin guard.
/// The admin API goes through [`delete_user_guarded`], which also removes the
/// user's refresh tokens in the same transaction.
pub async fn delete_user(pool: &PgPool, user_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to delete user")?;

    Ok(())
}
