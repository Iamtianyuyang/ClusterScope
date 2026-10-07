//! Acceptance tests for the storage half of
//! `features/merge_m6_auth_hardening.feature` (M6 step 2, `user_queries.rs`:
//! F-09 session revocation, single-use refresh tokens, digest storage,
//! last-enabled-admin guard).
//!
//! Scenario names become the test function names verbatim (the `commands`
//! adapter matches scenarios to JUnit test names by lowercased substring).
//!
//! The tests drive the real PostgreSQL instance (`POSTGRES_URL`, defaulting to
//! the local shared database). Fixture hygiene: only the six `m6-` usernames
//! listed in [`FIXTURE_USERS`] and their refresh tokens are ever inserted or
//! deleted -- never a TRUNCATE / DELETE of a whole table.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Mutex;
use storage::user_queries::{
    add_refresh_token, consume_refresh_token, create_user, delete_user_guarded,
    revoke_all_refresh_tokens, update_user_guarded,
};

const DEFAULT_POSTGRES_URL: &str =
    "postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope";

const USER_REVOKE: &str = "m6-revoke";
const USER_ONCE: &str = "m6-once";
const USER_DIGEST: &str = "m6-digest";
const USER_CASCADE: &str = "m6-cascade";
const USER_ADMIN_EXTRA: &str = "m6-admin-extra";
/// The account the shared database already ships with; it must never be
/// touched by these tests (the demote/delete scenario only checks that it is
/// left alone).
const USER_ADMIN_EXISTING: &str = "admin";

const FIXTURE_USERS: [&str; 5] = [
    USER_REVOKE,
    USER_ONCE,
    USER_DIGEST,
    USER_CASCADE,
    USER_ADMIN_EXTRA,
];

/// All scenarios share the `users` / `refresh_tokens` tables, so they run
/// serialized (a parallel run would let one test's admin demotion change the
/// enabled-admin count another test depends on).
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn lock_fixture() -> std::sync::MutexGuard<'static, ()> {
    FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn accept_pool() -> PgPool {
    let url = std::env::var("POSTGRES_URL").unwrap_or_else(|_| DEFAULT_POSTGRES_URL.to_string());
    PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("PostgreSQL at {url} must be reachable: {e}"))
}

/// Removes the fixture users and their refresh tokens (FK first), nothing else.
async fn cleanup(pool: &PgPool) {
    for user in FIXTURE_USERS {
        sqlx::query(
            "DELETE FROM refresh_tokens WHERE user_id IN (SELECT user_id FROM users WHERE username = $1)",
        )
        .bind(user)
        .execute(pool)
        .await
        .expect("removing fixture refresh tokens must succeed");
    }
    for user in FIXTURE_USERS {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(user)
            .execute(pool)
            .await
            .expect("removing fixture users must succeed");
    }
}

async fn new_fixture_user(pool: &PgPool, username: &str, role: &str) -> String {
    create_user(pool, username, None, role, "m6-fixture-hash")
        .await
        .expect("creating a fixture user must succeed")
}

async fn issue_token(pool: &PgPool, raw: &str, user_id: &str) {
    add_refresh_token(pool, raw, user_id, Utc::now() + Duration::hours(1))
        .await
        .expect("issuing a refresh token must succeed");
}

#[tokio::test]
async fn revoking_all_sessions_invalidates_every_outstanding_refresh_token_of_the_user() {
    let _guard = lock_fixture();
    let pool = accept_pool().await;
    cleanup(&pool).await;

    let user_id = new_fixture_user(&pool, USER_REVOKE, "viewer").await;
    let raw_token = "m6-raw-token-revoke";
    issue_token(&pool, raw_token, &user_id).await;
    let second_raw = "m6-raw-token-revoke-2";
    issue_token(&pool, second_raw, &user_id).await;

    let (outstanding,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1 AND revoked = FALSE",
    )
    .bind(&user_id)
    .fetch_one(&pool)
    .await
    .expect("counting the outstanding tokens must work");
    assert_eq!(
        outstanding, 2,
        "both fixture tokens must be outstanding before the admin revokes them"
    );

    revoke_all_refresh_tokens(&pool, &user_id)
        .await
        .expect("revoking all sessions must succeed");

    assert_eq!(
        consume_refresh_token(&pool, raw_token)
            .await
            .expect("consume must not fail"),
        None,
        "a revoked refresh token must no longer produce a session"
    );
    assert_eq!(
        consume_refresh_token(&pool, second_raw)
            .await
            .expect("consume must not fail"),
        None,
        "revocation covers every outstanding token of the user, not just one"
    );

    cleanup(&pool).await;
}

#[tokio::test]
async fn a_refresh_token_can_only_be_consumed_once() {
    let _guard = lock_fixture();
    let pool = accept_pool().await;
    cleanup(&pool).await;

    let user_id = new_fixture_user(&pool, USER_ONCE, "viewer").await;
    let raw_token = "m6-raw-token-once";
    issue_token(&pool, raw_token, &user_id).await;

    let first = consume_refresh_token(&pool, raw_token)
        .await
        .expect("the first consume must not fail");
    assert_eq!(
        first.as_deref(),
        Some(user_id.as_str()),
        "the first consumer gets the identity behind the token"
    );

    let second = consume_refresh_token(&pool, raw_token)
        .await
        .expect("a second consume is a normal outcome, not an error");
    assert_eq!(second, None, "the token is single use");

    cleanup(&pool).await;
}

#[tokio::test]
async fn refresh_tokens_are_stored_as_digests_so_the_raw_value_is_not_in_the_database() {
    let _guard = lock_fixture();
    let pool = accept_pool().await;
    cleanup(&pool).await;

    let user_id = new_fixture_user(&pool, USER_DIGEST, "viewer").await;
    let raw_token = "m6-raw-token-digest";
    issue_token(&pool, raw_token, &user_id).await;

    let (stored,): (String,) =
        sqlx::query_as("SELECT token FROM refresh_tokens WHERE user_id = $1")
            .bind(&user_id)
            .fetch_one(&pool)
            .await
            .expect("the token row must exist");

    assert_ne!(stored, raw_token, "the raw token must not be persisted");
    assert_eq!(
        stored.len(),
        64,
        "the stored digest is a fixed-length digest"
    );
    assert!(
        stored.chars().all(|c| c.is_ascii_hexdigit()),
        "the stored digest must be hexadecimal, got {stored}"
    );
    assert!(
        !stored.contains(raw_token),
        "the stored value must not embed the raw token"
    );

    // The digest must still be usable: the raw token authenticates.
    assert_eq!(
        consume_refresh_token(&pool, raw_token)
            .await
            .expect("consume must not fail"),
        Some(user_id.clone())
    );

    cleanup(&pool).await;
}

#[tokio::test]
async fn deleting_a_user_also_deletes_its_refresh_tokens() {
    let _guard = lock_fixture();
    let pool = accept_pool().await;
    cleanup(&pool).await;

    let user_id = new_fixture_user(&pool, USER_CASCADE, "viewer").await;
    issue_token(&pool, "m6-raw-token-cascade", &user_id).await;

    let deleted = delete_user_guarded(&pool, &user_id)
        .await
        .expect("deleting a non-admin user must not hit a foreign-key error");
    assert!(deleted, "the delete must report success");

    let (user_rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE user_id = $1")
        .bind(&user_id)
        .fetch_one(&pool)
        .await
        .expect("counting the deleted user must work");
    assert_eq!(user_rows, 0, "the user row is gone");

    let (token_rows,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1")
            .bind(&user_id)
            .fetch_one(&pool)
            .await
            .expect("counting the deleted tokens must work");
    assert_eq!(token_rows, 0, "no refresh token of the user may survive");

    cleanup(&pool).await;
}

#[tokio::test]
async fn demoting_or_deleting_an_administrator_that_is_not_the_last_one_succeeds() {
    let _guard = lock_fixture();
    let pool = accept_pool().await;
    cleanup(&pool).await;

    let existing: Option<(String, bool)> =
        sqlx::query_as("SELECT role, enabled FROM users WHERE username = $1")
            .bind(USER_ADMIN_EXISTING)
            .fetch_optional(&pool)
            .await
            .expect("reading the existing admin must work");
    let existing_before = existing.expect(
        "the shared database must keep its `admin` account (it is never touched by this test)",
    );
    assert_eq!(existing_before.0, "admin");

    let extra_id = new_fixture_user(&pool, USER_ADMIN_EXTRA, "admin").await;

    let demoted = update_user_guarded(&pool, &extra_id, Some("viewer"), None, None)
        .await
        .expect("demoting a non-last administrator must not error");
    assert!(
        demoted,
        "an administrator that is not the last one can be demoted"
    );

    let (role, enabled): (String, bool) =
        sqlx::query_as("SELECT role, enabled FROM users WHERE user_id = $1")
            .bind(&extra_id)
            .fetch_one(&pool)
            .await
            .expect("the demoted user must still exist");
    assert_eq!(role, "viewer");
    assert!(enabled);

    let deleted = delete_user_guarded(&pool, &extra_id)
        .await
        .expect("deleting a non-last administrator must not error");
    assert!(
        deleted,
        "the viewer can then be deleted like any other account"
    );

    let existing_after: (String, bool) =
        sqlx::query_as("SELECT role, enabled FROM users WHERE username = $1")
            .bind(USER_ADMIN_EXISTING)
            .fetch_one(&pool)
            .await
            .expect("the existing admin must still be there");
    assert_eq!(
        existing_after, existing_before,
        "the pre-existing administrator must be untouched by either step"
    );

    cleanup(&pool).await;
}
