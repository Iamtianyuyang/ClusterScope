//! Acceptance tests for `features/merge_m6_audit_queries.feature`
//! (M6 step 1 -- F-01 the audit endpoint always returned 500, F-16 the COUNT
//! statement never bound its parameters).
//!
//! Scenario names become the test function names verbatim: the `commands`
//! adapter matches every Gherkin scenario to a passing JUnit test name by
//! lowercased, whitespace-folded substring match.
//!
//! The tests drive the real PostgreSQL instance of this machine
//! (`POSTGRES_URL`, defaulting to the local shared database) through the
//! `storage::audit_queries` seam.
//!
//! Fixture hygiene: only rows for the two fixture usernames below are ever
//! inserted or deleted -- never TRUNCATE / DELETE of the whole table, because
//! the database is shared with the QA harness.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Mutex;
use storage::audit_queries::{insert_audit_log, list_audit_logs};

const DEFAULT_POSTGRES_URL: &str =
    "postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope";

/// The three records of the scenario background belong to this user.
const USER_AUDIT: &str = "m6-audit";
/// The one record that must never leak into a filtered listing.
const USER_OTHER: &str = "m6-other";
const ACTION_LOGIN: &str = "m6-login";
const ACTION_CREATE: &str = "m6-create";

/// All four scenarios share one fixture table, so they are serialized: a
/// parallel run would let one test's fixtures show up in another test's count.
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

async fn remove_fixture_rows(pool: &PgPool) {
    sqlx::query("DELETE FROM audit_logs WHERE username = $1 OR username = $2")
        .bind(USER_AUDIT)
        .bind(USER_OTHER)
        .execute(pool)
        .await
        .expect("cleaning the m6- fixture rows must succeed");
}

/// Background of the feature: three `m6-audit` rows (two `m6-login`, one
/// `m6-create`, timestamps increasing in that order) plus one `m6-other` row.
///
/// Returns the log ids in ascending timestamp order together with the oldest
/// timestamp, so a scenario can build its own time windows.
struct Fixture {
    pool: PgPool,
    oldest: DateTime<Utc>,
    audit_ids: Vec<String>,
}

async fn seed_fixture() -> Fixture {
    let pool = accept_pool().await;
    remove_fixture_rows(&pool).await;

    let oldest = Utc::now() - Duration::hours(1);
    let rows = [
        (USER_AUDIT, ACTION_LOGIN, 0_i64),
        (USER_AUDIT, ACTION_LOGIN, 10),
        (USER_AUDIT, ACTION_CREATE, 20),
    ];
    let mut audit_ids = Vec::new();
    for (user, action, offset_min) in rows {
        let id = insert_audit_log(
            &pool,
            user,
            action,
            Some("m6-target"),
            Some("job"),
            None,
            "success",
            Some("127.0.0.1"),
        )
        .await
        .expect("insert_audit_log must succeed for the fixture");
        set_timestamp(&pool, &id, oldest + Duration::minutes(offset_min)).await;
        audit_ids.push(id);
    }

    let other_id = insert_audit_log(
        &pool,
        USER_OTHER,
        ACTION_LOGIN,
        Some("m6-target"),
        Some("job"),
        None,
        "success",
        Some("127.0.0.1"),
    )
    .await
    .expect("insert_audit_log must succeed for the fixture");
    set_timestamp(&pool, &other_id, oldest + Duration::minutes(30)).await;

    Fixture {
        pool,
        oldest,
        audit_ids,
    }
}

/// `insert_audit_log` leaves the timestamp to the column default, so scenarios
/// that need a deterministic ordering pin it explicitly.
async fn set_timestamp(pool: &PgPool, log_id: &str, at: DateTime<Utc>) {
    sqlx::query("UPDATE audit_logs SET timestamp = $1 WHERE log_id = $2")
        .bind(at)
        .bind(log_id)
        .execute(pool)
        .await
        .expect("pinning the fixture timestamp must succeed");
}

#[tokio::test]
async fn audit_listing_filters_rows_and_returns_a_matching_total() {
    let _guard = lock_fixture();
    let fixture = seed_fixture().await;

    let (rows, total) = list_audit_logs(&fixture.pool, Some(USER_AUDIT), None, None, None, 0, 50)
        .await
        .expect("filtering by user must not fail (F-01/F-16 regression)");

    assert_eq!(rows.len(), 3, "the three m6-audit rows must be returned");
    assert_eq!(total, 3, "total must match the filtered row count");
    assert!(
        rows.iter().all(|row| row.username == USER_AUDIT),
        "no row of another user may leak into a user-filtered listing"
    );
    assert!(
        rows.iter().any(|row| row.action == ACTION_CREATE),
        "the m6-create row must be part of the result"
    );

    remove_fixture_rows(&fixture.pool).await;
}

#[tokio::test]
async fn audit_listing_returns_zero_total_instead_of_an_error_when_nothing_matches() {
    let _guard = lock_fixture();
    let fixture = seed_fixture().await;

    let result = list_audit_logs(&fixture.pool, Some("m6-nobody"), None, None, None, 0, 50).await;

    let (rows, total) = result.expect("an empty result set is not an error");
    assert_eq!(rows.len(), 0);
    assert_eq!(total, 0, "the count query must bind and return 0, not fail");

    remove_fixture_rows(&fixture.pool).await;
}

#[tokio::test]
async fn audit_listing_is_ordered_newest_first_and_pages_by_offset() {
    let _guard = lock_fixture();
    let fixture = seed_fixture().await;
    let newest = &fixture.audit_ids[2];
    let middle = &fixture.audit_ids[1];
    let oldest = &fixture.audit_ids[0];

    let (page_one, total_one) =
        list_audit_logs(&fixture.pool, Some(USER_AUDIT), None, None, None, 0, 2)
            .await
            .expect("the first page must be read without error");
    assert_eq!(page_one.len(), 2, "the first page holds page_size rows");
    assert_eq!(page_one[0].log_id, *newest, "newest row comes first");
    assert_eq!(page_one[1].log_id, *middle);
    assert_eq!(
        total_one, 3,
        "total is the whole filtered set, not the page"
    );

    let (page_two, total_two) =
        list_audit_logs(&fixture.pool, Some(USER_AUDIT), None, None, None, 1, 2)
            .await
            .expect("the second page must be read without error");
    assert_eq!(page_two.len(), 1, "only the oldest row is left on page two");
    assert_eq!(page_two[0].log_id, *oldest);
    assert_eq!(total_two, 3);

    let mut seen: Vec<&str> = page_one
        .iter()
        .chain(page_two.iter())
        .map(|row| row.log_id.as_str())
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen.len(),
        3,
        "both pages together are exactly the three rows"
    );

    remove_fixture_rows(&fixture.pool).await;
}

#[tokio::test]
async fn audit_listing_combines_user_action_and_time_filters() {
    let _guard = lock_fixture();
    let fixture = seed_fixture().await;
    let base = fixture.oldest;

    let (rows, total) = list_audit_logs(
        &fixture.pool,
        Some(USER_AUDIT),
        Some(ACTION_LOGIN),
        Some(base - Duration::hours(1)),
        Some(base + Duration::hours(1)),
        0,
        50,
    )
    .await
    .expect("the combined filter must not fail");
    assert_eq!(rows.len(), 2, "only the two m6-login rows match");
    assert_eq!(total, 2);
    assert!(
        rows.iter()
            .all(|row| row.username == USER_AUDIT && row.action == ACTION_LOGIN),
        "every row must satisfy both filters"
    );

    let (earlier_rows, earlier_total) = list_audit_logs(
        &fixture.pool,
        Some(USER_AUDIT),
        Some(ACTION_LOGIN),
        Some(base - Duration::hours(2)),
        Some(base - Duration::minutes(1)),
        0,
        50,
    )
    .await
    .expect("a window before the oldest row is still a valid query");
    assert_eq!(earlier_rows.len(), 0);
    assert_eq!(earlier_total, 0);

    remove_fixture_rows(&fixture.pool).await;
}
