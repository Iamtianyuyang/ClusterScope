use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NodeMetricsRow {
    pub id: i64,
    pub node_id: String,
    pub sequence: i64,
    pub timestamp_ms: i64,
    pub monotonic_clock_ms: Option<i64>,
    pub cpu_usage_percent: Option<f64>,
    pub load_1: Option<f64>,
    pub load_5: Option<f64>,
    pub load_15: Option<f64>,
    pub memory_total_bytes: Option<i64>,
    pub memory_used_bytes: Option<i64>,
    pub swap_total_bytes: Option<i64>,
    pub swap_used_bytes: Option<i64>,
    pub uptime_seconds: Option<i64>,
    pub boot_time_seconds: Option<i64>,
    pub gpu_metrics: Option<serde_json::Value>,
    pub gpu_processes: Option<serde_json::Value>,
    pub network_metrics: Option<serde_json::Value>,
    pub disk_metrics: Option<serde_json::Value>,
    pub cpu_core_metrics: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MetricsHourlyRow {
    pub id: i64,
    pub node_id: String,
    pub metric_name: String,
    pub hour_bucket: DateTime<Utc>,
    pub avg_value: f64,
    pub max_value: f64,
    pub min_value: f64,
    pub p95_value: f64,
    pub sample_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MetricsDailyRow {
    pub id: i64,
    pub node_id: String,
    pub metric_name: String,
    pub day_bucket: DateTime<Utc>,
    pub avg_value: f64,
    pub max_value: f64,
    pub min_value: f64,
    pub p95_value: f64,
    pub sample_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct JobRow {
    pub job_id: String,
    pub node_id: String,
    pub name: String,
    pub executable: String,
    pub arguments: serde_json::Value,
    pub working_directory: String,
    pub environment: serde_json::Value,
    pub status: String,
    pub pid: Option<i32>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub resource_quota: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct JobLogRow {
    pub id: i64,
    pub job_id: String,
    pub log_offset: i64,
    pub log_data: String,
    pub is_stderr: bool,
    pub timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlertRuleRow {
    pub rule_id: String,
    pub name: String,
    pub description: Option<String>,
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
    pub duration_seconds: i32,
    pub severity: String,
    pub node_id: String,
    pub gpu_uuids: serde_json::Value,
    pub labels: serde_json::Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlertEventRow {
    pub event_id: String,
    pub rule_id: String,
    pub node_id: String,
    pub gpu_uuid: String,
    pub old_state: Option<String>,
    pub new_state: String,
    pub current_value: Option<f64>,
    pub threshold: f64,
    pub notification_sent: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserRow {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub role: String,
    /// Missing in list queries (never expose hashes); defaults to empty.
    #[sqlx(default)]
    pub password_hash: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub failed_login_attempts: i32,
    pub locked_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLogRow {
    pub log_id: String,
    pub user: String,
    pub action: String,
    pub target: Option<String>,
    pub target_type: Option<String>,
    pub details: Option<String>,
    pub result: String,
    pub source_ip: Option<String>,
    pub timestamp: DateTime<Utc>,
}
