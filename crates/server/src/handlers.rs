use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use common::auth::{self, Claims, UserRole};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;

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
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let user = storage::user_queries::get_user_by_username(
        state.database.pool(),
        &req.username,
    ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = user.ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.enabled {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if let Some(locked_until) = user.locked_until {
        if locked_until > Utc::now() {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    // Verify password
    if auth::verify_password(&req.password, &user.password_hash).is_err() {
        storage::user_queries::record_failed_login(
            state.database.pool(),
            &req.username,
            state.config.max_login_attempts as i32,
            state.config.lockout_duration_secs as i64,
        ).await.ok();
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Record successful login
    storage::user_queries::record_login(
        state.database.pool(),
        &user.user_id,
    ).await.ok();

    // Generate tokens
    let access_token = auth::generate_jwt(
        &user.user_id,
        &user.role,
        &state.jwt_secret,
        state.config.jwt_access_expiry_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let refresh_token = auth::generate_refresh_token();
    let expires_at = Utc::now()
        .timestamp()
        + state.config.jwt_refresh_expiry_secs as i64;

    storage::user_queries::add_refresh_token(
        state.database.pool(),
        &refresh_token,
        &user.user_id,
        chrono::DateTime::from_timestamp(expires_at, 0).unwrap_or(Utc::now()),
    ).await.ok();

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        expires_at,
    }))
}

pub async fn refresh_token(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let refresh_token = req.get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let user_id = storage::user_queries::validate_refresh_token(
        state.database.pool(),
        refresh_token,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    let user = storage::user_queries::get_user_by_id(
        state.database.pool(),
        &user_id,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    let access_token = auth::generate_jwt(
        &user.user_id,
        &user.role,
        &state.jwt_secret,
        state.config.jwt_access_expiry_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let new_refresh = auth::generate_refresh_token();
    let expires_at = Utc::now()
        .timestamp()
        + state.config.jwt_refresh_expiry_secs as i64;

    storage::user_queries::add_refresh_token(
        state.database.pool(),
        &new_refresh,
        &user_id,
        chrono::DateTime::from_timestamp(expires_at, 0).unwrap_or(Utc::now()),
    ).await.ok();

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
    let result = nodes.into_iter().map(|n| {
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
    }).collect();
    Ok(Json(result))
}

pub async fn get_node_status(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let node = state.node_registry.get(&node_id)
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
    let metrics = storage::queries::get_latest_metrics(
        state.database.pool(),
        &node_id,
    ).await
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
    let node_id = params.get("node_id")
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let start_time: i64 = params.get("start_time_ms")
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let end_time: i64 = params.get("end_time_ms")
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Raw rows cover the last 24h; older ranges come from the hourly (7d)
    // and daily (90d) aggregate tables, merged and sorted by timestamp.
    const RAW_RETENTION_MS: i64 = 24 * 3600 * 1000;
    const HOURLY_RETENTION_MS: i64 = 7 * 24 * 3600 * 1000;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let raw_cutoff = now_ms - RAW_RETENTION_MS;
    let hourly_cutoff = now_ms - HOURLY_RETENTION_MS;

    let mut rows: Vec<serde_json::Value> = Vec::new();

    // 1) Raw reports within the retention window.
    if end_time >= raw_cutoff {
        let raw_start = start_time.max(raw_cutoff);
        if let Ok(reports) = storage::queries::get_metrics_history(
            state.database.pool(),
            node_id,
            raw_start,
            end_time,
            10000,
        ).await {
            rows.extend(reports.into_iter().map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null)));
        }
    }

    // 2) Hourly buckets for the part older than the raw window.
    if start_time < raw_cutoff && end_time > hourly_cutoff {
        let agg_start = start_time.max(hourly_cutoff);
        let agg_end = raw_cutoff - 1;
        if let Ok(buckets) = storage::queries::get_aggregated_history(
            state.database.pool(), node_id, agg_start, agg_end, true,
        ).await {
            rows.extend(aggregate_rows_to_json(node_id, buckets, "hourly"));
        }
    }

    // 3) Daily buckets for ranges older than 7 days.
    if start_time < hourly_cutoff {
        let agg_end = end_time.min(hourly_cutoff - 1);
        if let Ok(buckets) = storage::queries::get_aggregated_history(
            state.database.pool(), node_id, start_time, agg_end, false,
        ).await {
            rows.extend(aggregate_rows_to_json(node_id, buckets, "daily"));
        }
    }

    // Sort merged rows by timestamp, oldest first.
    rows.sort_by_key(|r| r.get("timestamp_ms").and_then(|v| v.as_i64()).unwrap_or(0));

    Ok(Json(serde_json::json!(rows)))
}

/// Convert aggregate buckets into the same row shape as raw reports, using
/// the shared metric keys so clients can plot them uniformly:
/// `cpu_usage_percent` / `memory_usage_percent` / `gpu_utilization_percent`.
fn aggregate_rows_to_json(
    node_id: &str,
    buckets: Vec<(chrono::DateTime<chrono::Utc>, String, f64)>,
    source: &'static str,
) -> Vec<serde_json::Value> {
    buckets.into_iter().filter_map(|(bucket, metric, avg)| {
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
    }).collect()
}

// ===== Job Handlers =====

pub async fn create_job(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<JobCreateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {


    let job_id = Uuid::new_v4().to_string();
    let env: HashMap<String, String> = req.environment.into_iter()
        .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string()))
        .collect();

    let job_row = storage::models::JobRow {
        job_id: job_id.clone(),
        node_id: req.node_id.clone(),
        name: req.name.clone(),
        executable: req.executable.clone(),
        arguments: serde_json::to_value(&req.arguments).unwrap_or(serde_json::Value::Array(vec![])),
        working_directory: req.working_directory.clone(),
        environment: serde_json::to_value(&env).unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
        status: "queued".to_string(),
        pid: None,
        exit_code: None,
        error_message: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        created_by: claims.sub.clone(),
        resource_quota: None,
        retry_count: 0,
        max_retries: 0,
    };

    storage::job_queries::insert_job(state.database.pool(), &job_row)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Audit log
    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &claims.sub,
        "create_job",
        Some(&job_id),
        Some("job"),
        Some(&format!("Created job '{}' on node {}", req.name, req.node_id)),
        "success",
        None,
    ).await.ok();

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
        params.get("page").and_then(|s| s.parse().ok()).unwrap_or(0),
        params.get("page_size").and_then(|s| s.parse().ok()).unwrap_or(20),
    ).await
    .map_err(|e| { warn!(error = %e, "list_jobs failed"); StatusCode::INTERNAL_SERVER_ERROR })?;

    Ok(Json(serde_json::json!({
        "jobs": jobs,
        "total": total,
    })))
}

pub async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<storage::models::JobRow>, StatusCode> {
    let job = storage::job_queries::get_job(
        state.database.pool(),
        &job_id,
    ).await
    .map_err(|e| { warn!(error = %e, "get_job failed"); StatusCode::INTERNAL_SERVER_ERROR })?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(job))
}

pub async fn stop_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {


    storage::job_queries::update_job_status(
        state.database.pool(),
        &job_id,
        "stopping",
        None,
        None,
        None,
        None,
        None,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    storage::audit_queries::insert_audit_log(
        state.database.pool(),
        &claims.sub,
        "stop_job",
        Some(&job_id),
        Some("job"),
        Some("Stop job requested"),
        "success",
        None,
    ).await.ok();

    Ok(Json(serde_json::json!({
        "job_id": job_id,
        "status": "stopping",
    })))
}

pub async fn get_job_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let offset: i64 = params.get("offset").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: i64 = params.get("limit").and_then(|s| s.parse().ok()).unwrap_or(100);

    let logs = storage::queries::get_job_logs(
        state.database.pool(),
        &job_id,
        offset,
        limit,
        false,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!(logs)))
}

// ===== Alert Handlers =====

pub async fn create_alert_rule(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rule_id = Uuid::new_v4().to_string();


    let name = req.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let metric = req.get("metric").and_then(|v| v.as_str()).unwrap_or("");
    let operator = req.get("operator").and_then(|v| v.as_str()).unwrap_or("gt");
    let threshold: f64 = req.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let duration: i32 = req.get("duration_seconds").and_then(|v| v.as_i64()).unwrap_or(30) as i32;
    let severity = req.get("severity").and_then(|v| v.as_str()).unwrap_or("warning");
    let node_id = req.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
    let gpu_uuids = req.get("gpu_uuids").cloned().unwrap_or(serde_json::Value::Array(vec![]));
    let labels = req.get("labels").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let description = req.get("description").and_then(|v| v.as_str()).unwrap_or("");

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
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "rule_id": rule_id,
    })))
}

pub async fn list_alert_rules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<storage::models::AlertRuleRow>>, StatusCode> {
    let rules = storage::alert_queries::list_alert_rules(
        state.database.pool(),
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rules))
}

pub async fn delete_alert_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,

) -> Result<StatusCode, StatusCode> {

    storage::alert_queries::delete_alert_rule(
        state.database.pool(),
        &rule_id,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

pub async fn get_alert_state(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let states = state.alert_engine.get_all_states();
    let filtered: Vec<_> = states.into_iter()
        .filter(|s| s.key.rule_id == rule_id)
        .collect();

    Ok(Json(serde_json::json!(filtered)))
}

pub async fn list_alert_events(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<storage::models::AlertEventRow>>, StatusCode> {
    let events = storage::alert_queries::get_active_alert_events(
        state.database.pool(),
    ).await
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

    let key = common::alert::AlertKey::new(rule_id, node_id.to_string(), gpu_uuid.to_string());
    state.alert_engine.reset_state(&key);

    Ok(StatusCode::OK)
}

// ===== User Handlers =====

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,

) -> Result<Json<serde_json::Value>, StatusCode> {

    let username = req.get("username").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;
    let password = req.get("password").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;
    let role = req.get("role").and_then(|v| v.as_str()).unwrap_or("viewer");
    let email = req.get("email").and_then(|v| v.as_str());

    let hash = auth::hash_password(password)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_id = storage::user_queries::create_user(
        state.database.pool(),
        username,
        email,
        role,
        &hash,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "user_id": user_id,
        "username": username,
        "role": role,
    })))
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,

) -> Result<Json<Vec<storage::models::UserRow>>, StatusCode> {

    let users = storage::user_queries::list_users(
        state.database.pool(),
    ).await
    .map_err(|e| { warn!(error = %e, "list_users failed"); StatusCode::INTERNAL_SERVER_ERROR })?;

    Ok(Json(users))
}

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,

) -> Result<Json<serde_json::Value>, StatusCode> {

    let user = storage::user_queries::get_user_by_id(
        state.database.pool(),
        &id,
    ).await
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
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,

) -> Result<StatusCode, StatusCode> {

    let role = req.get("role").and_then(|v| v.as_str());
    let enabled = req.get("enabled").and_then(|v| v.as_bool());

    storage::user_queries::update_user(
        state.database.pool(),
        &id,
        role,
        enabled,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,

) -> Result<StatusCode, StatusCode> {

    storage::user_queries::delete_user(
        state.database.pool(),
        &id,
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

// ===== Cluster Info =====

pub async fn get_cluster_info(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let nodes = state.node_registry.list();
    let online = nodes.iter().filter(|n| n.status == common::node_registry::NodeStatus::Online).count();
    let degraded = nodes.iter().filter(|n| n.status == common::node_registry::NodeStatus::Degraded).count();
    let offline = nodes.iter().filter(|n| n.status == common::node_registry::NodeStatus::Offline).count();

    let total_gpus: u32 = nodes.iter().map(|n| n.gpu_count).sum();

    // Real values from the latest metrics report per node. `None` when no
    // metrics have arrived yet — reported as JSON null, never as fake 0s.
    // Busy = GPU with utilization >= 1% in its latest report.
    let gpu_summary = storage::queries::get_gpu_utilization_summary(
        state.database.pool(),
    ).await.ok().flatten();
    let (avg_gpu_utilization, idle_gpus) = match gpu_summary {
        Some((avg, busy, total)) => (Some(avg), Some((total - busy).max(0))),
        None => (None, None),
    };

    // Count running jobs
    let running_jobs = storage::job_queries::list_jobs(
        state.database.pool(),
        None,
        Some("running"),
        None,
        0,
        1,
    ).await.map(|(jobs, _)| jobs.len()).unwrap_or(0);

    // Active (pending/firing) alerts from the latest event per target.
    let active_alerts = storage::alert_queries::count_active_alerts(
        state.database.pool(),
    ).await.unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_nodes": nodes.len(),
        "online_nodes": online,
        "degraded_nodes": degraded,
        "offline_nodes": offline,
        "total_gpus": total_gpus,
        "idle_gpus": idle_gpus,
        "avg_gpu_utilization": avg_gpu_utilization,
        "running_jobs": running_jobs,
        "active_alerts": active_alerts,
    })))
}

// ===== Audit Log =====

pub async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (logs, total) = storage::audit_queries::list_audit_logs(
        state.database.pool(),
        params.get("user").map(|s| s.as_str()),
        params.get("action").map(|s| s.as_str()),
        params.get("start_time_ms").and_then(|s| s.parse().ok()).map(|t: i64| chrono::DateTime::from_timestamp_millis(t).unwrap_or(Utc::now())),
        params.get("end_time_ms").and_then(|s| s.parse().ok()).map(|t: i64| chrono::DateTime::from_timestamp_millis(t).unwrap_or(Utc::now())),
        params.get("page").and_then(|s| s.parse().ok()).unwrap_or(0),
        params.get("page_size").and_then(|s| s.parse().ok()).unwrap_or(50),
    ).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "logs": logs,
        "total": total,
    })))
}

// ===== Prometheus Metrics =====

pub async fn get_prometheus_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<String, StatusCode> {
    use prometheus_client::registry::Registry;
    use prometheus_client::metrics::counter::Counter;
    use prometheus_client::metrics::gauge::Gauge;

    let mut registry = Registry::default();

    let nodes_counter: Counter = Counter::default();
    registry.register("nodes_total", "Total nodes", nodes_counter);

    let online_gauge: Gauge = Gauge::default();
    online_gauge.set(state.node_registry.list_online().len() as i64);
    registry.register("nodes_online", "Online nodes", online_gauge);

    let mut buffer = String::new();
    let _ = prometheus_client::encoding::text::encode(&mut buffer, &registry);
    let metrics_text = buffer;
    Ok(metrics_text)
}

// ===== Helpers =====

/// Role-based authorization helper used by the auth middleware.
pub fn check_role(claims: &Claims, allowed: &[&str]) -> Result<(), StatusCode> {
    let role = UserRole::from_str(&claims.role).unwrap_or(UserRole::Viewer);
    if allowed.contains(&role.to_str()) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
