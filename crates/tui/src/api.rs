#![allow(dead_code)] // API data models: fields kept for wire compatibility
use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub node_id: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub ip_address: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_seen: String,
    #[serde(default)]
    pub gpu_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GpuInfo {
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub utilization_gpu: f64,
    #[serde(default)]
    pub utilization_memory: f64,
    #[serde(default)]
    pub memory_total_bytes: Option<u64>,
    #[serde(default)]
    pub memory_used_bytes: Option<u64>,
    #[serde(default)]
    pub temperature_celsius: Option<f64>,
    #[serde(default)]
    pub power_watts: Option<f64>,
    #[serde(default)]
    pub fan_speed_percent: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GpuProcess {
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub gpu_uuid: String,
    #[serde(default)]
    pub gpu_memory_bytes: Option<u64>,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub sm_utilization: Option<f32>,
    #[serde(default)]
    pub memory_utilization: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeMetrics {
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub cpu_usage_percent: f64,
    #[serde(default)]
    pub load_1: f64,
    #[serde(default)]
    pub load_5: f64,
    #[serde(default)]
    pub load_15: f64,
    #[serde(default)]
    pub memory_total_bytes: u64,
    #[serde(default)]
    pub memory_used_bytes: u64,
    #[serde(default)]
    pub uptime_seconds: u64,
    #[serde(default, alias = "gpu_metrics")]
    pub gpus: Vec<GpuInfo>,
    #[serde(default, alias = "gpu_processes")]
    pub gpu_processes: Vec<GpuProcess>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub job_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertRule {
    pub rule_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub metric: String,
    #[serde(default)]
    pub operator: String,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertEvent {
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub gpu_uuid: String,
    /// DB column is `new_state`; keep `state` for compatibility.
    #[serde(default, alias = "new_state")]
    pub state: String,
    #[serde(default)]
    pub current_value: f64,
    #[serde(default)]
    pub timestamp: String,
}

#[derive(Clone)]
pub struct Api {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl Api {
    pub async fn login(base: &str, username: &str, password: &str) -> Result<Self> {
        let base = base.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client
            .post(format!("{}/api/login", base))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("login failed: HTTP {}", resp.status());
        }
        let body: Value = resp.json().await?;
        let token = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no access_token in login response"))?
            .to_string();
        Ok(Self {
            base,
            token,
            client,
        })
    }

    /// Connect without credentials. Works when the server runs in read-only
    /// mode (`auth_required: false`); otherwise the caller must log in.
    pub async fn connect(
        base: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self> {
        match (username, password) {
            (Some(u), Some(p)) => Self::login(base, u, p).await,
            _ => {
                let base = base.trim_end_matches('/').to_string();
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()?;
                let api = Self {
                    base,
                    token: String::new(),
                    client,
                };
                match api.get("/api/nodes").await {
                    Ok(_) => Ok(api),
                    Err(e) => anyhow::bail!(
                        "server requires authentication ({});\n  pass --username/--password, or set auth_required: false in server.yaml",
                        e
                    ),
                }
            }
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let mut req = self.client.get(format!("{}{}", self.base, path));
        // Only attach the header when a real token exists: an empty
        // `Bearer ` header makes the server reject the request.
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("GET {}: HTTP {}", path, resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn nodes(&self) -> Result<Vec<Node>> {
        self.get("/api/nodes").await?.serde_json::<Vec<Node>>()
    }

    pub async fn node_metrics(&self, node_id: &str) -> Result<Option<NodeMetrics>> {
        let v = self.get(&format!("/api/nodes/{}/metrics", node_id)).await?;
        if v.is_null() {
            return Ok(None);
        }
        Ok(serde_json::from_value(v).ok())
    }

    pub async fn jobs(&self) -> Result<Vec<Job>> {
        let v = self.get("/api/jobs").await?;
        Ok(serde_json::from_value(v["jobs"].clone()).unwrap_or_default())
    }

    pub async fn alert_rules(&self) -> Result<Vec<AlertRule>> {
        self.get("/api/alerts/rules").await?.serde_json()
    }

    pub async fn alert_events(&self) -> Result<Vec<AlertEvent>> {
        self.get("/api/alerts/events").await?.serde_json()
    }
}

pub trait SerdeJson {
    fn serde_json<T: serde::de::DeserializeOwned>(self) -> Result<T>;
}

impl SerdeJson for Value {
    fn serde_json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        Ok(serde_json::from_value(self)?)
    }
}

/// Human-friendly uptime string.
pub fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{}d{}h{}m", d, h, m)
    } else if h > 0 {
        format!("{}h{}m", h, m)
    } else {
        format!("{}m", m)
    }
}

/// Human-friendly bytes.
pub fn fmt_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", b, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// RFC3339 -> HH:MM:SS (UTC), or "-".
pub fn fmt_time(s: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(dt) => dt.with_timezone(&Utc).format("%H:%M:%S").to_string(),
        Err(_) => "-".to_string(),
    }
}
