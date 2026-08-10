use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub server_addr: String,
    pub node_id: Option<String>,
    pub node_id_file: PathBuf,
    pub report_interval_secs: u64,
    pub max_cached_reports: usize,
    pub reconnect_initial_delay_secs: u64,
    pub reconnect_max_delay_secs: u64,
    pub log_dir: PathBuf,
    pub log_level: String,
    pub node_labels: Vec<String>,
    pub disk_mounts: Vec<String>,
    /// Whether to read process details (`/proc/<pid>/...`) for GPU processes.
    /// Best-effort: unreadable processes degrade to "unknown" — the agent
    /// never requires root. Defaults to true.
    pub collect_process_details: bool,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
    pub tls_ca_path: Option<PathBuf>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server_addr: "http://localhost:50051".to_string(),
            node_id: None,
            node_id_file: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("/etc/clusterscope"))
                .join("node_id"),
            report_interval_secs: 2,
            max_cached_reports: 1000,
            reconnect_initial_delay_secs: 1,
            reconnect_max_delay_secs: 60,
            log_dir: dirs::state_dir()
                .unwrap_or_else(|| PathBuf::from("/var/log"))
                .join("clusterscope-agent"),
            log_level: "info".to_string(),
            node_labels: vec![],
            disk_mounts: vec!["/".to_string()],
            collect_process_details: true,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            tls_ca_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub grpc_addr: String,
    pub http_addr: String,
    pub postgres_url: String,
    pub redis_url: String,
    pub jwt_secret: String,
    pub jwt_access_expiry_secs: u64,
    pub jwt_refresh_expiry_secs: u64,
    pub max_login_attempts: u32,
    pub lockout_duration_secs: u64,
    pub node_online_threshold_secs: u64,
    pub node_degraded_threshold_secs: u64,
    pub node_offline_threshold_secs: u64,
    pub ws_heartbeat_interval_secs: u64,
    pub ws_slow_threshold_ms: u64,
    pub ws_max_backlog: usize,
    pub max_concurrent_ws_clients: usize,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
    pub prometheus_enabled: bool,
    pub prometheus_addr: String,
    pub default_admin_username: String,
    pub default_admin_password: String,
    /// When false, all GET (read-only) API endpoints skip JWT auth —
    /// convenient for LAN monitoring dashboards (TUI/web) without login.
    pub auth_required: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:50051".to_string(),
            http_addr: "0.0.0.0:8080".to_string(),
            postgres_url: "postgresql://clusterscope:clusterscope@localhost:5432/clusterscope".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "default-secret-change-me".to_string(),
            jwt_access_expiry_secs: 3600,
            jwt_refresh_expiry_secs: 604800,
            max_login_attempts: 5,
            lockout_duration_secs: 300,
            node_online_threshold_secs: 10,
            node_degraded_threshold_secs: 30,
            node_offline_threshold_secs: 60,
            ws_heartbeat_interval_secs: 30,
            ws_slow_threshold_ms: 5000,
            ws_max_backlog: 1000,
            max_concurrent_ws_clients: 1000,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            prometheus_enabled: false,
            prometheus_addr: "0.0.0.0:9090".to_string(),
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin".to_string(),
            auth_required: true,
        }
    }
}
