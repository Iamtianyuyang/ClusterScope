use crate::models::UserRow;
use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

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
    sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .context("Failed to get user by username")
}

pub async fn get_user_by_id(pool: &PgPool, user_id: &str) -> Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE user_id = $1",
    )
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
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users SET
            role = COALESCE($2, role),
            enabled = COALESCE($3, enabled),
            updated_at = NOW()
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .bind(role)
    .bind(enabled)
    .execute(pool)
    .await
    .context("Failed to update user")?;
    
    Ok(())
}

pub async fn delete_user(pool: &PgPool, user_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to delete user")?;
    
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

pub async fn record_failed_login(pool: &PgPool, username: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users SET
            failed_login_attempts = failed_login_attempts + 1,
            locked_until = CASE
                WHEN failed_login_attempts + 1 >= 5 THEN NOW() + INTERVAL '5 minutes'
                ELSE locked_until
            END
        WHERE username = $1
        "#,
    )
    .bind(username)
    .execute(pool)
    .await
    .context("Failed to record failed login")?;
    
    Ok(())
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
    .bind(token)
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .context("Failed to add refresh token")?;
    
    Ok(())
}

pub async fn revoke_refresh_token(pool: &PgPool, token: &str) -> Result<()> {
    sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token = $1")
        .bind(token)
        .execute(pool)
        .await
        .context("Failed to revoke refresh token")?;
    
    Ok(())
}

pub async fn validate_refresh_token(pool: &PgPool, token: &str) -> Result<Option<String>> {
    sqlx::query_as::<_, (String,)>(
        r#"
        SELECT user_id FROM refresh_tokens
        WHERE token = $1 AND revoked = FALSE AND expires_at > NOW()
        "#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await
    .context("Failed to validate refresh token")
    .map(|r| r.map(|(uid,)| uid))
}
