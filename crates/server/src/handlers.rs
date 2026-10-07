use axum::{
    Json,
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use common::auth::{self, Claims};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration as StdDuration, Instant};
use tracing::warn;
use uuid::Uuid;

use crate::AppState;

/// Submission input limits (the jobs columns have no DB-side length caps).
/// `pub(crate)` so the gRPC `submit_job` path enforces the same caps.
pub(crate) const MAX_JOB_NAME_LEN: usize = 255;
pub(crate) const MAX_EXECUTABLE_LEN: usize = 4096;
pub(crate) const MAX_WORKDIR_LEN: usize = 4096;
pub(crate) const MAX_QUOTA_LEN: usize = 64;
pub(crate) const MAX_ARGS: usize = 256;
pub(crate) const MAX_ARG_LEN: usize = 4096;
pub(crate) const MAX_ENV_ENTRIES: usize = 128;
pub(crate) const MAX_ENV_KEY_LEN: usize = 255;
pub(crate) const MAX_ENV_VALUE_LEN: usize = 4096;

/// Login rate limit: at most 10 attempts per source IP per 60s window.
const LOGIN_WINDOW_SECS: u64 = 60;
const LOGIN_MAX_ATTEMPTS: usize = 10;

/// Global login budget: at most 300 attempts (successes AND failures) per
/// minute across ALL source IPs. The per-IP window cannot bound a distributed
/// brute force / Argon2 CPU DoS launched from many IPs; this one can.
const GLOBAL_LOGIN_WINDOW_SECS: u64 = 60;
const GLOBAL_LOGIN_MAX_ATTEMPTS: usize = 300;

/// Cap on concurrent Argon2 operations (each login attempt burns ~30-50ms):
/// a synchronized burst of logins must not spike CPU across all cores.
/// Referenced from `main.rs` when building the shared semaphore.
pub(crate) const LOGIN_MAX_CONCURRENCY: usize = 4;

/// users.username is VARCHAR(100) (characters, not bytes); email is
/// VARCHAR(255). Shared caps keep login/create/update consistent.
const MAX_USERNAME_CHARS: usize = 100;
const MAX_EMAIL_CHARS: usize = 255;
/// Argon2 cost scales with input length; cap passwords at 1 KiB so a huge
/// body cannot burn CPU per attempt.
const MAX_PASSWORD_BYTES: usize = 1024;

/// Dummy Argon2 hash verified against when a username does not exist, so the
/// response time does not reveal whether the account exists (timing side
/// channel). Argon2 cost is dominated by its parameters, not the salt, so a
/// fresh hash per process is fine.
fn dummy_password_hash() -> &'static str {
    use std::sync::OnceLock;
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        auth::hash_password("clusterscope-dummy-password-for-timing").unwrap_or_default()
    })
}

/// Per-IP sliding window plus the global attempt budget, in one place.
///
/// Extracted from the free functions below so the limits can be exercised
/// without building a whole `AppState`; the handler and the tests share this
/// single implementation. Caps/windows are unchanged:
/// `LOGIN_MAX_ATTEMPTS` failures per `LOGIN_WINDOW_SECS` per IP, and
/// `GLOBAL_LOGIN_MAX_ATTEMPTS` attempts per `GLOBAL_LOGIN_WINDOW_SECS`
/// across all IPs (every attempt counts, success or failure).
pub struct LoginLimiter {
    /// ip -> timestamps of FAILED attempts inside the window.
    attempts: StdMutex<HashMap<String, VecDeque<Instant>>>,
    /// Timestamps of every attempt (any IP) inside the global window.
    global: StdMutex<VecDeque<Instant>>,
}

impl Default for LoginLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self {
            attempts: StdMutex::new(HashMap::new()),
            global: StdMutex::new(VecDeque::new()),
        }
    }

    /// Sliding-window check for one source IP. Returns `true` when the attempt
    /// is allowed. Only FAILED attempts are recorded, so legitimate users are
    /// never throttled by their own successful logins.
    pub fn allowed(&self, ip: &str) -> bool {
        let now = Instant::now();
        let window = StdDuration::from_secs(LOGIN_WINDOW_SECS);
        let mut attempts = self.attempts.lock().unwrap();
        let queue = attempts.entry(ip.to_string()).or_default();
        while queue
            .front()
            .map(|t: &Instant| now.duration_since(*t) > window)
            .unwrap_or(false)
        {
            queue.pop_front();
        }
        queue.len() < LOGIN_MAX_ATTEMPTS
    }

    /// Record a failed attempt for an IP.
    pub fn record_failure(&self, ip: &str) {
        self.attempts
            .lock()
            .unwrap()
            .entry(ip.to_string())
            .or_default()
            .push_back(Instant::now());
    }

    /// Global budget check across all IPs.
    pub fn global_allowed(&self) -> bool {
        let now = Instant::now();
        let window = StdDuration::from_secs(GLOBAL_LOGIN_WINDOW_SECS);
        let mut attempts = self.global.lock().unwrap();
        while attempts
            .front()
            .map(|t: &Instant| now.duration_since(*t) > window)
            .unwrap_or(false)
        {
            attempts.pop_front();
        }
        attempts.len() < GLOBAL_LOGIN_MAX_ATTEMPTS
    }

    /// Record one attempt toward the global budget (successful or not).
    pub fn record_attempt(&self) {
        self.global.lock().unwrap().push_back(Instant::now());
    }

    /// Drop per-IP entries whose newest failure fell out of the window, so the
    /// map cannot grow unboundedly with distinct source IPs. The global queue
    /// is self-pruning (every check drops expired entries).
    pub fn prune(&self) {
        let now = Instant::now();
        let window = StdDuration::from_secs(LOGIN_WINDOW_SECS);
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, q| {
            q.back()
                .map(|t| now.duration_since(*t) <= window)
                .unwrap_or(false)
        });
    }
}

/// Sliding-window rate limiter keyed by client IP. Returns `true` when the
/// attempt is allowed. Only FAILED attempts are recorded (see
/// [`login_failed`]), so legitimate users are never throttled by their own
/// successful logins. Old entries are pruned opportunistically.
pub fn login_allowed(state: &AppState, ip: &str) -> bool {
    state.login_limiter.allowed(ip)
}

/// Record a failed login attempt for an IP (only failures count against the
/// per-IP sliding window).
pub fn login_failed(state: &AppState, ip: &str) {
    state.login_limiter.record_failure(ip);
}

/// Drop rate-limit entries whose newest attempt fell out of the window, so
/// the limiter map cannot grow unboundedly with distinct source IPs. Called
/// periodically from the server background task.
pub fn prune_login_attempts(state: &AppState) {
    state.login_limiter.prune();
}

/// Global sliding-window budget across all IPs: at most
/// `GLOBAL_LOGIN_MAX_ATTEMPTS` attempts per minute in total. Every login
/// attempt (success or failure) is recorded, so the budget bounds the total
/// Argon2 work an attacker can buy with many distinct source IPs.
pub fn global_login_allowed(state: &AppState) -> bool {
    state.login_limiter.global_allowed()
}

/// Record one attempt toward the global budget (called for every login
/// attempt, successful or not).
pub fn record_global_login_attempt(state: &AppState) {
    state.login_limiter.record_attempt();
}

/// Resolve the client IP for rate limiting. With `trust_proxy_headers` the
/// first `X-Forwarded-For` entry wins (the reverse proxy is expected to
/// overwrite the header, so it is the real client); otherwise the socket
/// address is used. Never panics and never returns an empty string.
pub fn effective_client_ip(
    headers: &axum::http::HeaderMap,
    addr: SocketAddr,
    trust_proxy: bool,
) -> String {
    if trust_proxy
        && let Some(xff) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        // X-Forwarded-For is a comma-separated list: client, proxy1, …
        && let Some(first) = xff.split(',').next()
    {
        let ip = first.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    addr.ip().to_string()
}

// ===== Auth Handlers =====

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // Global budget first: bounds the total Argon2 CPU an attacker can buy
    // with many distinct IPs (the per-IP window below cannot). Counts every
    // attempt, success or failure.
    if !global_login_allowed(&state) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    record_global_login_attempt(&state);

    // Concurrency cap: each attempt burns ~30-50ms of Argon2 (plus the dummy
    // hash on unknown users). A synchronized burst must not spike all cores.
    let _permit = state
        .login_concurrency
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // `trust_proxy_headers` is off by default so the limiter keys on the
    // socket address; behind a proxy that overwrites X-Forwarded-For it
    // keys on the real client (otherwise all users share the proxy IP and
    // one attacker can lock everyone out).
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    if !login_allowed(&state, &client_ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    // The username column is VARCHAR(100) (characters); reject oversized
    // inputs up front instead of binding a huge parameter (rate limiting is
    // per-IP, so this would otherwise be a cheap way to waste server/DB work).
    if req.username.chars().count() > MAX_USERNAME_CHARS || req.password.len() > MAX_PASSWORD_BYTES
    {
        login_failed(&state, &client_ip);
        return Err(StatusCode::BAD_REQUEST);
    }

    let user = storage::user_queries::get_user_by_username(state.database.pool(), &req.username)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Constant-time-ish path for unknown users: burn the same Argon2 cost so
    // the 401 latency does not leak whether the account exists.
    let user = match user {
        Some(u) => u,
        None => {
            let _ = auth::verify_password(&req.password, dummy_password_hash());
            storage::audit_queries::insert_audit_log(
                state.database.pool(),
                &req.username,
                "login",
                None,
                None,
                Some("unknown user"),
                "failed",
                Some(&client_ip),
            )
            .await
            .ok();
            login_failed(&state, &client_ip);
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if !user.enabled {
        let _ = auth::verify_password(&req.password, dummy_password_hash());
        storage::audit_queries::insert_audit_log(
            state.database.pool(),
            &req.username,
            "login",
            Some(&user.user_id),
            Some("user"),
            Some("disabled account"),
            "failed",
            Some(&client_ip),
        )
        .await
        .ok();
        login_failed(&state, &client_ip);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Locked accounts are reported as a plain 401 so the lock state itself
    // does not leak (and so the failed counter stops growing).
    if let Some(locked_until) = user.locked_until
        && locked_until > Utc::now()
    {
        storage::audit_queries::insert_audit_log(
            state.database.pool(),
            &req.username,
            "login",
            Some(&user.user_id),
            Some("user"),
            Some("locked account"),
            "failed",
            Some(&client_ip),
        )
        .await
        .ok();
        login_failed(&state, &client_ip);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Verify password
    if auth::verify_password(&req.password, &user.password_hash).is_err() {
        storage::user_queries::record_failed_login(
            state.database.pool(),
            &req.username,
            state.config.max_login_attempts as i32,
            state.config.lockout_duration_secs as i64,
        )
        .await
        .ok();
        storage::audit_queries::insert_audit_log(
            state.database.pool(),
            &req.username,
            "login",
            Some(&user.user_id),
            Some("user"),
            Some("wrong password"),
            "failed",
            Some(&client_ip),
        )
        .await
        .ok();
        login_failed(&state, &client_ip);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Record successful login
    if let Err(e) = storage::user_queries::record_login(state.database.pool(), &user.user_id).await
    {
        warn!(error = %e, username = %user.username, "Failed to record login");
    }

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &user.username,
        "login",
        Some(&user.user_id),
        Some("user"),
        None,
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    // Generate tokens
    let access_token = auth::generate_jwt(
        &user.user_id,
        &user.role,
        &state.jwt_secret,
        state.config.jwt_access_expiry_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let refresh_token = auth::generate_refresh_token();
    let expires_at = Utc::now().timestamp() + state.config.jwt_refresh_expiry_secs as i64;

    // Persisting the refresh token is part of a successful login: failing it
    // would hand the client an access token with no way to renew.
    storage::user_queries::add_refresh_token(
        state.database.pool(),
        &refresh_token,
        &user.user_id,
        chrono::DateTime::from_timestamp(expires_at, 0).unwrap_or(Utc::now()),
    )
    .await
    .map_err(|e| {
        warn!(error = %e, username = %user.username, "Failed to persist refresh token");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        expires_at,
    }))
}

pub async fn refresh_token(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // Same per-IP sliding window as login (failures only): a leaked/expired
    // refresh token must not enable unlimited attempts, and an attacker
    // must not be able to burn unbounded DB work on this public endpoint.
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    if !login_allowed(&state, &client_ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let refresh_token = req
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let user_id =
        storage::user_queries::consume_refresh_token(state.database.pool(), refresh_token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or_else(|| {
                login_failed(&state, &client_ip);
                StatusCode::UNAUTHORIZED
            })?;

    let user = storage::user_queries::get_user_by_id(state.database.pool(), &user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or_else(|| {
            login_failed(&state, &client_ip);
            StatusCode::UNAUTHORIZED
        })?;

    // Disabled or locked users must not be able to keep refreshing: the login
    // path checks these, the refresh path must too.
    if !user.enabled {
        login_failed(&state, &client_ip);
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(locked_until) = user.locked_until
        && locked_until > Utc::now()
    {
        login_failed(&state, &client_ip);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Rotate: revoke the presented refresh token so a stolen/leaked token
    // cannot be replayed for the whole expiry window. The consume is a single
    // conditional UPDATE (revoke + validity check in one statement), so two
    // concurrent refreshes with the same token cannot both mint new pairs
    // (validate-then-revoke had a TOCTOU window). If the consume failed after
    // returning a user_id, refuse to mint rather than leaving both tokens live.
    let access_token = auth::generate_jwt(
        &user.user_id,
        &user.role,
        &state.jwt_secret,
        state.config.jwt_access_expiry_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let new_refresh = auth::generate_refresh_token();
    let expires_at = Utc::now().timestamp() + state.config.jwt_refresh_expiry_secs as i64;

    storage::user_queries::add_refresh_token(
        state.database.pool(),
        &new_refresh,
        &user_id,
        chrono::DateTime::from_timestamp(expires_at, 0).unwrap_or(Utc::now()),
    )
    .await
    .map_err(|e| {
        warn!(error = %e, "Failed to persist new refresh token");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(LoginResponse {
        access_token,
        refresh_token: new_refresh,
        expires_at,
    }))
}

// ===== Node Handlers =====

pub async fn list_nodes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let nodes = state.node_registry.list();
    let result = nodes
        .into_iter()
        .map(|n| {
            serde_json::json!({
                "node_id": n.node_id,
                "hostname": n.hostname,
                "ip_address": n.ip_address,
                "status": match n.status {
                    common::node_registry::NodeStatus::Online => "online",
                    common::node_registry::NodeStatus::Degraded => "degraded",
                    common::node_registry::NodeStatus::Offline => "offline",
                },
                "last_seen": n.last_seen.to_rfc3339(),
                "gpu_count": n.gpu_count,
                "labels": n.labels,
            })
        })
        .collect();
    Ok(Json(result))
}

pub async fn get_node_status(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let node = state
        .node_registry
        .get(&node_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "node_id": node.node_id,
        "status": match node.status {
            common::node_registry::NodeStatus::Online => "online",
            common::node_registry::NodeStatus::Degraded => "degraded",
            common::node_registry::NodeStatus::Offline => "offline",
        },
        "last_seen": node.last_seen.to_rfc3339(),
    })))
}

pub async fn get_node_metrics(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let metrics = storage::queries::get_latest_metrics(state.database.pool(), &node_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match metrics {
        Some(m) => Ok(Json(serde_json::json!(m))),
        None => Ok(Json(serde_json::json!({}))),
    }
}

// ===== Metrics Handlers =====

pub async fn get_metrics_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let node_id = params
        .get("node_id")
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let start_time: i64 = params
        .get("start_time_ms")
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let end_time: i64 = params
        .get("end_time_ms")
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Raw rows cover the last 24h; older ranges come from the hourly (7d)
    // and daily (90d) aggregate tables, merged and sorted by timestamp.
    let now_ms = chrono::Utc::now().timestamp_millis();
    let (raw_range, hourly_range, daily_range) = history_tiers(start_time, end_time, now_ms);

    let mut rows: Vec<serde_json::Value> = Vec::new();

    // 1) Raw reports within the retention window.
    if let Some((raw_start, raw_end)) = raw_range
        && let Ok(reports) = storage::queries::get_metrics_history(
            state.database.pool(),
            node_id,
            raw_start,
            raw_end,
            10000,
        )
        .await
    {
        rows.extend(
            reports
                .into_iter()
                .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null)),
        );
    }

    // 2) Hourly buckets for the part older than the raw window.
    if let Some((agg_start, agg_end)) = hourly_range
        && let Ok(buckets) = storage::queries::get_aggregated_history(
            state.database.pool(),
            node_id,
            agg_start,
            agg_end,
            true,
        )
        .await
    {
        rows.extend(aggregate_rows_to_json(node_id, buckets, "hourly"));
    }

    // 3) Daily buckets for ranges older than 7 days.
    if let Some((agg_start, agg_end)) = daily_range
        && let Ok(buckets) = storage::queries::get_aggregated_history(
            state.database.pool(),
            node_id,
            agg_start,
            agg_end,
            false,
        )
        .await
    {
        rows.extend(aggregate_rows_to_json(node_id, buckets, "daily"));
    }

    // Sort merged rows by timestamp, oldest first.
    rows.sort_by_key(|r| r.get("timestamp_ms").and_then(|v| v.as_i64()).unwrap_or(0));

    Ok(Json(serde_json::json!(rows)))
}

/// One `[start, end]` sub-range served by a retention tier.
type TierRange = Option<(i64, i64)>;

/// Partition a `[start_ms, end_ms]` history request into the (raw, hourly,
/// daily) retention-tier ranges that must be queried, relative to `now_ms`.
/// Every returned sub-range is clipped to the caller's `[start_ms, end_ms]`.
///
/// Returns `(raw, hourly, daily)` where each is `Some((range_start, range_end))`
/// when that tier contributes data, `None` otherwise.
fn history_tiers(start_ms: i64, end_ms: i64, now_ms: i64) -> (TierRange, TierRange, TierRange) {
    const RAW_RETENTION_MS: i64 = 24 * 3600 * 1000;
    const HOURLY_RETENTION_MS: i64 = 7 * 24 * 3600 * 1000;
    let raw_cutoff = now_ms - RAW_RETENTION_MS;
    let hourly_cutoff = now_ms - HOURLY_RETENTION_MS;

    let raw = if end_ms >= raw_cutoff {
        Some((start_ms.max(raw_cutoff), end_ms))
    } else {
        None
    };
    let hourly = if start_ms < raw_cutoff && end_ms > hourly_cutoff {
        Some((start_ms.max(hourly_cutoff), end_ms.min(raw_cutoff - 1)))
    } else {
        None
    };
    let daily = if start_ms < hourly_cutoff {
        Some((start_ms, end_ms.min(hourly_cutoff - 1)))
    } else {
        None
    };
    (raw, hourly, daily)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 3600 * 1000;
    const DAY_MS: i64 = 24 * HOUR_MS;

    #[test]
    fn test_history_tiers_within_raw_window() {
        let now = 1_800_000_000_000;
        let (raw, hourly, daily) = history_tiers(now - 2 * HOUR_MS, now, now);
        assert_eq!(raw, Some((now - 2 * HOUR_MS, now)));
        assert_eq!(hourly, None);
        assert_eq!(daily, None);
    }

    #[test]
    fn test_history_tiers_recent_range_ends_before_raw_cutoff() {
        // 3 days ago, 1h window: entirely inside the hourly tier, and the
        // hourly range must NOT leak buckets beyond the requested end.
        let now = 1_800_000_000_000;
        let start = now - 3 * DAY_MS;
        let end = start + HOUR_MS;
        let (raw, hourly, daily) = history_tiers(start, end, now);
        assert_eq!(raw, None);
        let Some((hs, he)) = hourly else {
            panic!("expected hourly tier");
        };
        assert_eq!(hs, start);
        assert_eq!(he, end, "hourly range must respect the requested end");
        assert_eq!(daily, None);
    }

    #[test]
    fn test_history_tiers_spanning_all_tiers() {
        let now = 1_800_000_000_000;
        let start = now - 30 * DAY_MS;
        let end = now;
        let (raw, hourly, daily) = history_tiers(start, end, now);
        assert_eq!(raw, Some((now - DAY_MS, now)));
        let Some((hs, he)) = hourly else {
            panic!("expected hourly tier");
        };
        assert_eq!(hs, now - 7 * DAY_MS);
        assert_eq!(he, now - DAY_MS - 1);
        let Some((ds, de)) = daily else {
            panic!("expected daily tier");
        };
        assert_eq!(ds, start);
        assert_eq!(de, now - 7 * DAY_MS - 1);
    }

    #[test]
    fn test_history_tiers_older_than_hourly() {
        let now = 1_800_000_000_000;
        let start = now - 30 * DAY_MS;
        let end = start + HOUR_MS;
        let (raw, hourly, daily) = history_tiers(start, end, now);
        assert_eq!(raw, None);
        assert_eq!(hourly, None);
        assert_eq!(daily, Some((start, end)));
    }
}

/// Convert aggregate buckets into the same row shape as raw reports, using
/// the shared metric keys so clients can plot them uniformly:
/// `cpu_usage_percent` / `memory_usage_percent` / `gpu_utilization_percent`.
fn aggregate_rows_to_json(
    node_id: &str,
    buckets: Vec<(chrono::DateTime<chrono::Utc>, String, f64)>,
    source: &'static str,
) -> Vec<serde_json::Value> {
    buckets
        .into_iter()
        .filter_map(|(bucket, metric, avg)| {
            let key = match metric.as_str() {
                "cpu_usage_percent" => Some("cpu_usage_percent"),
                "memory_used_percent" => Some("memory_usage_percent"),
                "gpu_utilization" => Some("gpu_utilization_percent"),
                _ => None,
            };
            let key = key?;
            Some(serde_json::json!({
                "node_id": node_id,
                "timestamp_ms": bucket.timestamp_millis(),
                key: avg,
                "source": source,
            }))
        })
        .collect()
}

// ===== Job Handlers =====

/// Best-effort username for audit logs (JWT claims carry the user id, the
/// audit trail stores usernames). Falls back to the user id on lookup error.
async fn audit_username(state: &AppState, claims: &Claims) -> String {
    storage::user_queries::get_user_by_id(state.database.pool(), &claims.sub)
        .await
        .ok()
        .flatten()
        .map(|u| u.username)
        .unwrap_or_else(|| claims.sub.clone())
}

/// Submission caps shared by the HTTP handler and the gRPC path (the jobs
/// columns have no DB-side length limits, so without these a single call
/// could insert a gigantic row). Returns `BAD_REQUEST` plus a message naming
/// the violated limit.
pub(crate) fn validate_job_request(req: &JobCreateRequest) -> Result<(), (StatusCode, String)> {
    if req.arguments.len() > MAX_ARGS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("too many arguments: {} > {}", req.arguments.len(), MAX_ARGS),
        ));
    }
    if let Some(over) = req.arguments.iter().find(|a| a.len() > MAX_ARG_LEN) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("argument too long: {} > {} bytes", over.len(), MAX_ARG_LEN),
        ));
    }
    let oversize = req.name.len() > MAX_JOB_NAME_LEN
        || req.executable.len() > MAX_EXECUTABLE_LEN
        || req.working_directory.len() > MAX_WORKDIR_LEN
        || req.resource_quota.len() > MAX_QUOTA_LEN
        || req.environment.len() > MAX_ENV_ENTRIES
        || req.environment.keys().any(|k| k.len() > MAX_ENV_KEY_LEN)
        || req.environment.values().any(|v| {
            v.as_str()
                .map(|s| s.len() > MAX_ENV_VALUE_LEN)
                .unwrap_or(false)
        });
    if oversize {
        return Err((
            StatusCode::BAD_REQUEST,
            "submission exceeds the field length caps".to_string(),
        ));
    }
    Ok(())
}

pub async fn create_job(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Json(req): Json<JobCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Same client-IP resolution as login: honor trust_proxy_headers so the
    // audit trail records the real client behind a reverse proxy.
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    // Reject nonsense submissions early instead of queueing a job that can
    // never run: an executable is required, and a non-empty target node must
    // be known to the cluster (otherwise the row violates the node_info FK
    // and the job is silently un-runnable).
    if req.name.trim().is_empty() || req.executable.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !req.node_id.trim().is_empty() && !state.node_registry.exists(&req.node_id) {
        return Err(StatusCode::BAD_REQUEST);
    }

    validate_job_request(&req).map_err(|(_status, reason)| {
        warn!(reason = %reason, "Rejected job submission");
        StatusCode::BAD_REQUEST
    })?;

    let job_id = Uuid::new_v4().to_string();
    let env: HashMap<String, String> = req
        .environment
        .into_iter()
        .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string()))
        .collect();
    let max_retries = req.max_retries.min(10) as i32;

    let job_row = storage::models::JobRow {
        job_id: job_id.clone(),
        node_id: req.node_id.clone(),
        name: req.name.clone(),
        executable: req.executable.clone(),
        arguments: serde_json::to_value(&req.arguments).unwrap_or(serde_json::Value::Array(vec![])),
        working_directory: req.working_directory.clone(),
        environment: serde_json::to_value(&env)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
        status: "queued".to_string(),
        pid: None,
        exit_code: None,
        error_message: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        created_by: claims.sub.clone(),
        resource_quota: if req.resource_quota.trim().is_empty() {
            None
        } else {
            Some(req.resource_quota.trim().to_string())
        },
        retry_count: 0,
        max_retries,
    };

    storage::job_queries::insert_job(state.database.pool(), &job_row)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Audit log
    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "create_job",
        Some(&job_id),
        Some("job"),
        Some(&format!(
            "Created job '{}' on node {}",
            req.name, req.node_id
        )),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(Json(serde_json::json!({
        "job_id": job_id,
        "status": "queued",
    })))
}

#[derive(Deserialize)]
pub struct JobCreateRequest {
    pub node_id: String,
    pub name: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    #[serde(default)]
    pub environment: serde_json::Map<String, serde_json::Value>,
    /// GPU requirement, e.g. "2", "gpu:2", "gpus:4". Empty = 1 GPU.
    #[serde(default)]
    pub resource_quota: String,
    /// Number of automatic retries after a failed run (0 = no retry, max 10).
    #[serde(default)]
    pub max_retries: u32,
}

pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (jobs, total) = storage::job_queries::list_jobs(
        state.database.pool(),
        params.get("node_id").map(|s| s.as_str()),
        params.get("status").map(|s| s.as_str()),
        params.get("created_by").map(|s| s.as_str()),
        params
            .get("page")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
            .clamp(0, 1_000_000),
        params
            .get("page_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20)
            .clamp(1, 500),
    )
    .await
    .map_err(|e| {
        warn!(error = %e, "list_jobs failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "jobs": jobs,
        "total": total,
    })))
}

pub async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<storage::models::JobRow>, StatusCode> {
    let job = storage::job_queries::get_job(state.database.pool(), &job_id)
        .await
        .map_err(|e| {
            warn!(error = %e, "get_job failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(job))
}

pub async fn stop_job(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Path(job_id): Path<String>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let pool = state.database.pool();
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);

    // Unknown job: 404 instead of a fake "stopping" success.
    let Some(row) = storage::job_queries::get_job(pool, &job_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    let new_status: String = match row.status.as_str() {
        // Already terminal — nothing to stop.
        "succeeded" | "failed" | "cancelled" | "lost" => {
            return Ok(Json(serde_json::json!({
                "job_id": job_id,
                "status": row.status,
            })));
        }
        // Not dispatched yet: cancel atomically, no agent involved. Leaving
        // it 'stopping' would strand it forever (scheduler only picks
        // 'queued', and with no node_id no agent would ever see it). The
        // UPDATE is conditional on status='queued' and the in-memory
        // scheduler queue is drained too — otherwise the next schedule()
        // pass would dispatch the cancelled job anyway.
        "queued" => {
            let cancelled = storage::job_queries::cancel_queued_job(pool, &job_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if cancelled {
                state.scheduler.remove_queued(&job_id).await;
                state.scheduler.drop_job(&job_id).await;
                "cancelled".to_string()
            } else {
                // Raced with dispatch (status moved off 'queued'): the job
                // may be starting/running — ask the agent to kill it, but
                // only if it is still active: a concurrent stop may already
                // have cancelled it, and clobbering that with 'stopping'
                // would strand a terminal job until the reaper marks it lost.
                stop_active_job(pool, &job_id).await?
            }
        }
        // starting / running / stopping: ask the agent to kill it.
        _ => stop_active_job(pool, &job_id).await?,
    };

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "stop_job",
        Some(&job_id),
        Some("job"),
        Some("Stop job requested"),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    state.ws_manager.push_job_update(&job_id).await;

    Ok(Json(serde_json::json!({
        "job_id": job_id,
        "status": new_status,
    })))
}
pub async fn get_job_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let offset: i64 = params
        .get("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
        .max(0);
    let limit: i64 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .clamp(1, 1000);

    let logs = storage::queries::get_job_logs(state.database.pool(), &job_id, offset, limit, false)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!(logs)))
}

// ===== Alert Handlers =====

pub async fn create_alert_rule(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rule_id = Uuid::new_v4().to_string();
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);

    let name = req.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let metric = req.get("metric").and_then(|v| v.as_str()).unwrap_or("");
    let operator = req.get("operator").and_then(|v| v.as_str()).unwrap_or("gt");
    let threshold: f64 = req.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.0);
    // Parse duration as i64 first and range-check before narrowing: `as i32`
    // on an oversized value truncates (e.g. 2^40 -> 0), which would silently
    // turn a long duration into an instant-firing rule.
    let duration_secs: i64 = req
        .get("duration_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);
    let duration: i32 = i32::try_from(duration_secs)
        .ok()
        .filter(|d| *d >= 0)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let severity = req
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("warning");
    let node_id = req.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
    let gpu_uuids = req
        .get("gpu_uuids")
        .cloned()
        .unwrap_or(serde_json::Value::Array(vec![]));
    let labels = req
        .get("labels")
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let description = req
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Validate against the alert engine's supported inputs; an unknown
    // operator/severity would otherwise silently disable the rule.
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Length/size caps so a single rule cannot grow the DB row without bound.
    if name.len() > 255 || description.len() > 4096 {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(arr) = gpu_uuids.as_array()
        && arr.len() > 256
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(obj) = labels.as_object()
        && obj.len() > 64
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(operator, "gt" | "gte" | "lt" | "lte" | "eq" | "neq") {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Per-item caps: gpu_uuids entries and node_id feed VARCHAR columns and
    // end up in per-GPU alert keys, so an oversized string would bloat the
    // DB row and the in-memory engine without bound.
    if node_id.len() > 255 {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(arr) = gpu_uuids.as_array()
        && arr
            .iter()
            .any(|v| v.as_str().map(|s| s.len() > 255).unwrap_or(false))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(obj) = labels.as_object()
        && (obj.keys().any(|k| k.len() > 255)
            || obj
                .values()
                .any(|v| v.as_str().map(|s| s.len() > 4096).unwrap_or(false)))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(severity, "info" | "warning" | "critical") {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(
        metric,
        "cpu_usage_percent"
            | "memory_usage_percent"
            | "load_1"
            | "gpu_temperature"
            | "gpu_utilization"
            | "gpu_memory_used_percent"
            | "gpu_power_watts"
    ) {
        return Err(StatusCode::BAD_REQUEST);
    }

    storage::alert_queries::insert_alert_rule(
        state.database.pool(),
        &rule_id,
        name,
        description,
        metric,
        operator,
        threshold,
        duration,
        severity,
        node_id,
        &gpu_uuids,
        &labels,
        true,
        &claims.sub,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "create_alert_rule",
        Some(&rule_id),
        Some("alert_rule"),
        Some(&format!("Created alert rule '{}' on {}", name, metric)),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(Json(serde_json::json!({
        "rule_id": rule_id,
    })))
}

pub async fn list_alert_rules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<storage::models::AlertRuleRow>>, StatusCode> {
    let rules = storage::alert_queries::list_alert_rules(state.database.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rules))
}

pub async fn delete_alert_rule(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Path(rule_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    // alert_events reference the rule with no ON DELETE CASCADE; this tree's
    // cascade helper removes both inside one transaction (B used two calls).
    storage::alert_queries::delete_alert_rule_cascade(state.database.pool(), &rule_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Drop in-memory engine instances so deleted rules stop being tracked
    // (and stop consuming memory) immediately.
    state.alert_engine.remove_rule_instances(&rule_id);

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "delete_alert_rule",
        Some(&rule_id),
        Some("alert_rule"),
        Some("Alert rule deleted"),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(StatusCode::OK)
}

pub async fn get_alert_state(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let states = state.alert_engine.get_all_states();
    let filtered: Vec<_> = states
        .into_iter()
        .filter(|s| s.key.rule_id == rule_id)
        .collect();

    Ok(Json(serde_json::json!(filtered)))
}

pub async fn list_alert_events(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<storage::models::AlertEventRow>>, StatusCode> {
    let events = storage::alert_queries::get_active_alert_events(state.database.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(events))
}

pub async fn acknowledge_alert(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let node_id = req.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
    let gpu_uuid = req.get("gpu_uuid").and_then(|v| v.as_str()).unwrap_or("");
    // Caps mirror the alert-rule path (VARCHAR columns + engine keys).
    if node_id.len() > 255 || gpu_uuid.len() > 255 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let key =
        common::alert::AlertKey::new(rule_id.clone(), node_id.to_string(), gpu_uuid.to_string());

    // Snapshot the instance before resetting it so the ack event can carry
    // the value/threshold that was firing.
    let (current_value, threshold) = state
        .alert_engine
        .get_all_states()
        .into_iter()
        .find(|s| s.key == key)
        .map(|s| (s.current_value, s.threshold))
        .unwrap_or((None, 0.0));

    state.alert_engine.reset_state(&key);

    // Persist the acknowledgement as a resolved event: the active-alert view
    // (latest event per key) stops showing it immediately. The engine
    // re-evaluates on the next report and re-fires if the condition still
    // holds — ack silences the current firing, not the rule.
    let event_id = uuid::Uuid::new_v4().to_string();
    storage::alert_queries::insert_alert_event(
        state.database.pool(),
        &event_id,
        &rule_id,
        node_id,
        gpu_uuid,
        "firing",
        "resolved",
        current_value,
        threshold,
        Some("acknowledged"),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

// ===== User Handlers =====

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    let username = req
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let password = req
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let role = req.get("role").and_then(|v| v.as_str()).unwrap_or("viewer");
    if !matches!(role, "viewer" | "operator" | "admin") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let email = req.get("email").and_then(|v| v.as_str());

    // Column caps (VARCHAR(100)/VARCHAR(255)): reject up front so an
    // oversized value returns 400 instead of a DB error -> 500, and matches
    // the login-path checks.
    if username.chars().count() > MAX_USERNAME_CHARS || username.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(email) = email
        && (email.chars().count() > MAX_EMAIL_CHARS || email.is_empty())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Same strength policy as every other password-setting path.
    auth::validate_password_strength(password).map_err(|_| StatusCode::BAD_REQUEST)?;

    let hash = auth::hash_password(password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_id =
        storage::user_queries::create_user(state.database.pool(), username, email, role, &hash)
            .await
            .map_err(|e| {
                // Duplicate username: the users.username column is UNIQUE; report it
                // as a 409 Conflict instead of a misleading 500.
                if is_unique_violation(&e) {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            })?;

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "create_user",
        Some(&user_id),
        Some("user"),
        Some(&format!("Created user '{}' with role {}", username, role)),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(Json(serde_json::json!({
        "user_id": user_id,
        "username": username,
        "role": role,
    })))
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<storage::models::UserRow>>, StatusCode> {
    let users = storage::user_queries::list_users(state.database.pool())
        .await
        .map_err(|e| {
            warn!(error = %e, "list_users failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(users))
}

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user = storage::user_queries::get_user_by_id(state.database.pool(), &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Never expose the password hash.
    Ok(Json(serde_json::json!({
        "user_id": user.user_id,
        "username": user.username,
        "email": user.email,
        "role": user.role,
        "enabled": user.enabled,
        "created_at": user.created_at,
        "last_login_at": user.last_login_at,
        "failed_login_attempts": user.failed_login_attempts,
        "locked_until": user.locked_until,
    })))
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    let role = req.get("role").and_then(|v| v.as_str());
    let enabled = req.get("enabled").and_then(|v| v.as_bool());
    if let Some(r) = role
        && !matches!(r, "viewer" | "operator" | "admin")
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    // 404 up front (also lets the audit trail resolve the current role).
    storage::user_queries::get_user_by_id(state.database.pool(), &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Admin may reset a user's password (new hash, counter cleared).
    let password_hash = match req.get("password").and_then(|v| v.as_str()) {
        Some("") => return Err(StatusCode::BAD_REQUEST),
        Some(pw) if pw.len() > MAX_PASSWORD_BYTES => return Err(StatusCode::BAD_REQUEST),
        Some(pw) => {
            auth::validate_password_strength(pw).map_err(|_| StatusCode::BAD_REQUEST)?;
            Some(auth::hash_password(pw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
        }
        None => None,
    };

    // Atomic last-admin guard: the admin count and the UPDATE are one
    // transaction (FOR UPDATE), so two concurrent demotions can never both
    // pass and leave the cluster without any enabled admin.
    let applied = storage::user_queries::update_user_guarded(
        state.database.pool(),
        &id,
        role,
        enabled,
        password_hash.as_deref(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !applied {
        return Err(StatusCode::BAD_REQUEST);
    }

    // An admin password reset must invalidate the target's existing
    // sessions, exactly like a self-service password change: otherwise the
    // old refresh tokens keep minting access tokens for a user whose
    // password was rotated (e.g. a compromised account).
    if password_hash.is_some()
        && let Err(e) =
            storage::user_queries::revoke_all_refresh_tokens(state.database.pool(), &id).await
    {
        warn!(error = %e, user_id = %id, "Failed to revoke refresh tokens after admin password reset");
    }

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "update_user",
        Some(&id),
        Some("user"),
        Some("User updated (role/enabled/password)"),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(StatusCode::OK)
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);

    // 404 up front.
    storage::user_queries::get_user_by_id(state.database.pool(), &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Atomic last-admin guard (admin count + delete in one transaction, with
    // the refresh-token cleanup inside the same tx).
    let applied = storage::user_queries::delete_user_guarded(state.database.pool(), &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !applied {
        return Err(StatusCode::BAD_REQUEST);
    }

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &audit_username(&state, &claims).await,
        "delete_user",
        Some(&id),
        Some("user"),
        Some("User deleted"),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(StatusCode::OK)
}

/// Self-service password change: requires the current password, so a stolen
/// access token alone cannot be used to take over the account permanently.
pub async fn change_my_password(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(claims): Extension<Claims>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let client_ip = effective_client_ip(&headers, addr, state.config.trust_proxy_headers);
    let old_password = req
        .get("old_password")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_password = req
        .get("new_password")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if new_password.len() > MAX_PASSWORD_BYTES {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Same strength policy as every other password-setting path.
    auth::validate_password_strength(new_password).map_err(|_| StatusCode::BAD_REQUEST)?;

    let user = storage::user_queries::get_user_by_id(state.database.pool(), &claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // A disabled or locked account must not be able to use a still-valid
    // access token to change its own password (same gate as login/refresh).
    if !user.enabled {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(locked_until) = user.locked_until
        && locked_until > Utc::now()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if auth::verify_password(old_password, &user.password_hash).is_err() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let new_hash =
        auth::hash_password(new_password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    storage::user_queries::update_user(
        state.database.pool(),
        &user.user_id,
        None,
        None,
        Some(&new_hash),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Rotate all refresh tokens for this user: password change invalidates
    // every existing session except the current access token.
    if let Err(e) =
        storage::user_queries::revoke_all_refresh_tokens(state.database.pool(), &user.user_id).await
    {
        // The password itself was already rotated; failing the session
        // revoke only leaves old sessions live until expiry.
        warn!(error = %e, user_id = %user.user_id, "Failed to revoke refresh tokens after password change");
    }

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &user.username,
        "change_password",
        Some(&claims.sub),
        Some("user"),
        Some("User changed own password"),
        "success",
        Some(&client_ip),
    )
    .await
    .ok();

    Ok(StatusCode::OK)
}

// ===== Cluster Info =====

pub async fn get_cluster_info(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let nodes = state.node_registry.list();
    let online = nodes
        .iter()
        .filter(|n| n.status == common::node_registry::NodeStatus::Online)
        .count();
    let degraded = nodes
        .iter()
        .filter(|n| n.status == common::node_registry::NodeStatus::Degraded)
        .count();
    let offline = nodes
        .iter()
        .filter(|n| n.status == common::node_registry::NodeStatus::Offline)
        .count();

    // Aggregate GPU stats from the latest metrics snapshot per node instead
    // of returning hardcoded placeholders.
    let latest = storage::queries::get_latest_metrics_all(state.database.pool())
        .await
        .unwrap_or_default();
    let (mut total_gpus, mut busy_gpus, mut util_sum, mut util_count) = (0u32, 0u32, 0.0f64, 0u32);
    for m in &latest {
        if let Some(gpus) = m.gpu_metrics.as_ref().and_then(|v| v.as_array()) {
            for g in gpus {
                total_gpus += 1;
                let util = g
                    .get("utilization_gpu")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                util_sum += util;
                util_count += 1;
                if util >= 1.0 {
                    busy_gpus += 1;
                }
            }
        }
    }
    let avg_gpu_utilization = if util_count > 0 {
        util_sum / util_count as f64
    } else {
        0.0
    };

    // Count running jobs (the count comes from the query's total — the row
    // list is fetched with page_size=1, so the rows themselves would cap the
    // result at 0/1).
    let running_jobs =
        storage::job_queries::list_jobs(state.database.pool(), None, Some("running"), None, 0, 1)
            .await
            .map(|(_, total)| total)
            .unwrap_or(0);

    // Count currently active alerts (latest event per key is pending/firing).
    let active_alerts = storage::alert_queries::get_active_alert_events(state.database.pool())
        .await
        .map(|events| events.len())
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_nodes": nodes.len(),
        "online_nodes": online,
        "degraded_nodes": degraded,
        "offline_nodes": offline,
        "total_gpus": total_gpus,
        "idle_gpus": total_gpus.saturating_sub(busy_gpus),
        "avg_gpu_utilization": (avg_gpu_utilization * 10.0).round() / 10.0,
        "running_jobs": running_jobs,
        "active_alerts": active_alerts,
    })))
}

// ===== Audit Log =====

pub async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Parse timestamps strictly: an unparseable/out-of-range value is a
    // client error (400), not a silently wrong filter (the previous
    // `unwrap_or(Utc::now())` turned a bad end_time into "nothing after
    // now" and a bad start_time into "nothing before now").
    let start_time = match params.get("start_time_ms") {
        Some(s) => {
            let t: i64 = s.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            Some(chrono::DateTime::from_timestamp_millis(t).ok_or(StatusCode::BAD_REQUEST)?)
        }
        None => None,
    };
    let end_time = match params.get("end_time_ms") {
        Some(s) => {
            let t: i64 = s.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            Some(chrono::DateTime::from_timestamp_millis(t).ok_or(StatusCode::BAD_REQUEST)?)
        }
        None => None,
    };

    let (logs, total) = storage::audit_queries::list_audit_logs(
        state.database.pool(),
        params.get("user").map(|s| s.as_str()),
        params.get("action").map(|s| s.as_str()),
        start_time,
        end_time,
        params
            .get("page")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
            .clamp(0, 1_000_000),
        params
            .get("page_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50)
            .clamp(1, 500),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "logs": logs,
        "total": total,
    })))
}

// ===== Prometheus Metrics =====

/// Render the Prometheus text exposition of cluster metrics. Shared by the
/// authenticated `/api/prometheus/metrics` endpoint and the optional
/// standalone `prometheus_addr` listener (see `prometheus_enabled`).
pub fn render_prometheus_metrics(state: &AppState) -> String {
    use prometheus_client::metrics::gauge::Gauge;
    use prometheus_client::registry::Registry;

    let mut registry = Registry::default();

    let nodes_total: Gauge = Gauge::default();
    nodes_total.set(state.node_registry.list().len() as i64);
    registry.register(
        "nodes_total",
        "Total number of registered nodes (any status)",
        nodes_total,
    );

    let nodes_online: Gauge = Gauge::default();
    nodes_online.set(state.node_registry.list_online().len() as i64);
    registry.register("nodes_online", "Nodes currently online", nodes_online);

    let mut buffer = String::new();
    let _ = prometheus_client::encoding::text::encode(&mut buffer, &registry);
    buffer
}

pub async fn get_prometheus_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<String, StatusCode> {
    Ok(render_prometheus_metrics(&state))
}

// ===== Helpers =====

/// True when a sqlx error (wrapped in anyhow) is a Postgres unique-violation
/// (23505) — e.g. a duplicate username on user creation. Mapped to HTTP 409
/// by callers.
fn is_unique_violation(e: &anyhow::Error) -> bool {
    e.downcast_ref::<sqlx::Error>()
        .map(|db| matches!(db, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505")))
        .unwrap_or(false)
}
/// Set a job to `stopping` only while it is still active (starting/running).
/// Returns the status to report: "stopping" when the update applied,
/// otherwise the current persisted status (a concurrent cancellation or
/// completion won the race).
async fn stop_active_job(pool: &sqlx::PgPool, job_id: &str) -> Result<String, StatusCode> {
    match storage::job_queries::mark_stopping_if_active(pool, job_id).await {
        Ok(true) => Ok("stopping".to_string()),
        Ok(false) => {
            let status = storage::job_queries::get_job(pool, job_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .map(|r| r.status)
                .unwrap_or_default();
            Ok(status)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Acceptance tests for `features/merge_m6_auth_hardening.feature` (login
/// rate limiting / client-address resolution, F-08) and
/// `features/merge_m6_job_safety.feature` (submission caps, F-11).
///
/// The scenario names are the test function names verbatim. The limiter and
/// the submission validator are driven directly (the feature file allows
/// extracting them for exactly this reason): the handlers themselves only
/// forward to `LoginLimiter` / `validate_job_request`.
#[cfg(test)]
mod merge_m6_acceptance_tests {
    use super::*;
    use axum::http::HeaderMap;

    fn job_request(arguments: Vec<String>) -> JobCreateRequest {
        JobCreateRequest {
            node_id: "m6-node".to_string(),
            name: "m6-job".to_string(),
            executable: "/bin/true".to_string(),
            arguments,
            working_directory: "/tmp".to_string(),
            environment: serde_json::Map::new(),
            resource_quota: String::new(),
            max_retries: 0,
        }
    }

    #[test]
    fn repeated_failed_logins_from_one_client_address_are_capped() {
        let limiter = LoginLimiter::new();
        let ip = "m6-ip-a";

        // The first LOGIN_MAX_ATTEMPTS failures are allowed; the 11th attempt
        // from the same address is refused *before* any password work (the
        // handler checks the limiter first).
        for attempt in 1..=10 {
            assert!(
                limiter.allowed(ip),
                "failed attempt {attempt} must still be allowed"
            );
            limiter.record_failure(ip);
        }
        assert!(
            !limiter.allowed(ip),
            "the 11th attempt from the same address must be refused before the password check"
        );
    }

    #[test]
    fn the_login_attempt_cap_is_tracked_per_client_address() {
        let limiter = LoginLimiter::new();
        let capped = "m6-ip-a";
        for _ in 0..10 {
            limiter.record_failure(capped);
        }
        assert!(!limiter.allowed(capped), "the capped address stays refused");

        // A different source address has its own, untouched window.
        assert!(
            limiter.allowed("m6-ip-b"),
            "the budget must be tracked per client address"
        );
    }

    #[test]
    fn the_global_login_budget_bounds_attempts_across_all_client_addresses() {
        let limiter = LoginLimiter::new();

        // 300 attempts from 300 distinct addresses: every attempt counts
        // (success or failure) against the global budget.
        for attempt in 0..300 {
            let ip = format!("m6-global-{attempt}");
            assert!(
                limiter.global_allowed(),
                "attempt {attempt} must be inside the global budget"
            );
            assert!(limiter.allowed(&ip), "a fresh address is under its own cap");
            limiter.record_attempt();
        }

        assert!(
            !limiter.global_allowed(),
            "the 301st attempt must be refused even from a brand-new address"
        );
    }

    #[test]
    fn the_client_address_used_for_the_budget_follows_the_forwarded_header_when_the_proxy_is_trusted()
     {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.9, 10.0.0.1".parse().expect("header value"),
        );
        let addr: SocketAddr = "127.0.0.1:50000".parse().expect("socket addr");

        assert_eq!(
            effective_client_ip(&headers, addr, true),
            "203.0.113.9",
            "with a trusted proxy the first forwarded entry is the client"
        );
        assert_eq!(
            effective_client_ip(&headers, addr, false),
            "127.0.0.1",
            "without trust the header is ignored and the socket address is used"
        );
    }

    #[test]
    fn a_job_submission_with_too_many_arguments_is_rejected() {
        let too_many = job_request(vec!["m6-arg".to_string(); MAX_ARGS + 1]);
        let rejected = validate_job_request(&too_many);
        assert!(rejected.is_err(), "257 arguments must be rejected");
        assert!(
            rejected.unwrap_err().1.contains("too many arguments"),
            "the rejection names the argument-count cap"
        );

        let at_the_limit = job_request(vec!["m6-arg".to_string(); MAX_ARGS]);
        assert!(
            validate_job_request(&at_the_limit).is_ok(),
            "exactly {MAX_ARGS} arguments are still accepted"
        );
    }

    #[test]
    fn a_job_submission_with_an_argument_over_the_length_limit_is_rejected() {
        let too_long = job_request(vec!["x".repeat(MAX_ARG_LEN + 1)]);
        let rejected = validate_job_request(&too_long);
        assert!(rejected.is_err(), "a 4097-byte argument must be rejected");
        assert!(
            rejected.unwrap_err().1.contains("argument too long"),
            "the rejection names the per-argument limit"
        );

        let at_the_limit = job_request(vec!["x".repeat(MAX_ARG_LEN)]);
        assert!(
            validate_job_request(&at_the_limit).is_ok(),
            "exactly {MAX_ARG_LEN} bytes are still accepted"
        );
    }
}
