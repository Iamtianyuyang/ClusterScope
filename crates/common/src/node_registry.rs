use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum NodeStatus {
    Online = 0,
    Degraded = 1,
    #[default]
    Offline = 2,
}

impl NodeStatus {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Online),
            1 => Some(Self::Degraded),
            2 => Some(Self::Offline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeEntry {
    pub node_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub agent_version: String,
    pub os_info: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub memory_total_bytes: u64,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub status: NodeStatus,
    pub labels: HashMap<String, String>,
    pub gpu_count: u32,
}

#[derive(Debug, Clone)]
pub struct NodeThresholds {
    pub online_secs: u64,
    pub degraded_secs: u64,
    pub offline_secs: u64,
}

impl Default for NodeThresholds {
    fn default() -> Self {
        Self {
            online_secs: 10,
            degraded_secs: 30,
            offline_secs: 60,
        }
    }
}

pub type NodeRegistry = Arc<RwLock<HashMap<String, NodeEntry>>>;

pub struct RegistryManager {
    nodes: NodeRegistry,
    thresholds: RwLock<NodeThresholds>,
}

impl Default for RegistryManager {
    fn default() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            thresholds: RwLock::new(NodeThresholds::default()),
        }
    }
}

impl RegistryManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_thresholds(thresholds: NodeThresholds) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            thresholds: RwLock::new(thresholds),
        }
    }

    pub fn register(&self, node: NodeEntry) {
        let mut nodes = self.nodes.write();
        nodes.insert(node.node_id.clone(), node);
    }

    pub fn update_last_seen(&self, node_id: &str, now: DateTime<Utc>) -> bool {
        let mut nodes = self.nodes.write();
        if let Some(entry) = nodes.get_mut(node_id) {
            entry.last_seen = now;
            entry.status = NodeStatus::Online;
            true
        } else {
            false
        }
    }

    pub fn update_status(&self, node_id: &str, status: NodeStatus) {
        let mut nodes = self.nodes.write();
        if let Some(entry) = nodes.get_mut(node_id) {
            entry.status = status;
        }
    }

    /// Update the GPU count learned from the latest metrics report.
    pub fn update_gpu_count(&self, node_id: &str, gpu_count: u32) {
        let mut nodes = self.nodes.write();
        if let Some(entry) = nodes.get_mut(node_id) {
            entry.gpu_count = gpu_count;
        }
    }

    pub fn get(&self, node_id: &str) -> Option<NodeEntry> {
        self.nodes.read().get(node_id).cloned()
    }

    pub fn list(&self) -> Vec<NodeEntry> {
        self.nodes.read().values().cloned().collect()
    }

    pub fn list_online(&self) -> Vec<NodeEntry> {
        self.nodes
            .read()
            .values()
            .filter(|n| n.status == NodeStatus::Online)
            .cloned()
            .collect()
    }

    pub fn exists(&self, node_id: &str) -> bool {
        self.nodes.read().contains_key(node_id)
    }

    pub fn remove(&self, node_id: &str) {
        self.nodes.write().remove(node_id);
    }

    pub fn count(&self) -> usize {
        self.nodes.read().len()
    }

    pub fn set_thresholds(&self, thresholds: NodeThresholds) {
        *self.thresholds.write() = thresholds;
    }

    pub fn get_thresholds(&self) -> NodeThresholds {
        self.thresholds.read().clone()
    }

    pub fn check_node_status(&self, now: DateTime<Utc>) {
        let thresholds = self.thresholds.read();
        let mut nodes = self.nodes.write();

        for entry in nodes.values_mut() {
            let elapsed = (now - entry.last_seen).num_seconds() as u64;

            if elapsed <= thresholds.online_secs {
                entry.status = NodeStatus::Online;
            } else if elapsed <= thresholds.degraded_secs {
                entry.status = NodeStatus::Degraded;
            } else {
                entry.status = NodeStatus::Offline;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn make_node(id: &str) -> NodeEntry {
        let now = Utc::now();
        NodeEntry {
            node_id: id.to_string(),
            hostname: format!("host-{}", id),
            ip_address: format!("192.168.1.{}", id.parse::<u8>().unwrap_or(1)),
            agent_version: "0.1.0".to_string(),
            os_info: "Ubuntu 24.04".to_string(),
            kernel_version: "6.8.0".to_string(),
            cpu_model: "Test CPU".to_string(),
            cpu_cores: 8,
            memory_total_bytes: 64_000_000_000,
            registered_at: now,
            last_seen: now,
            status: NodeStatus::Online,
            labels: HashMap::new(),
            gpu_count: 4,
        }
    }

    #[test]
    fn test_register_and_get() {
        let mgr = RegistryManager::new();
        let node = make_node("node-1");
        mgr.register(node);

        assert_eq!(mgr.count(), 1);
        assert!(mgr.get("node-1").is_some());
        assert!(mgr.get("node-2").is_none());
    }

    #[test]
    fn test_list_online() {
        let mgr = RegistryManager::new();
        mgr.register(make_node("node-1"));
        mgr.register(make_node("node-2"));

        mgr.update_status("node-2", NodeStatus::Offline);

        let online = mgr.list_online();
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].node_id, "node-1");
    }

    #[test]
    fn test_node_status_check() {
        let mgr = RegistryManager::new();
        let mut node = make_node("node-1");
        let now = Utc::now();

        node.last_seen = now - ChronoDuration::seconds(5);
        mgr.register(node);
        mgr.check_node_status(now);
        assert_eq!(mgr.get("node-1").unwrap().status, NodeStatus::Online);

        let now = now + ChronoDuration::seconds(20);
        mgr.check_node_status(now);
        assert_eq!(mgr.get("node-1").unwrap().status, NodeStatus::Degraded);

        let now = now + ChronoDuration::seconds(50);
        mgr.check_node_status(now);
        assert_eq!(mgr.get("node-1").unwrap().status, NodeStatus::Offline);
    }

    #[test]
    fn test_update_last_seen() {
        let mgr = RegistryManager::new();
        mgr.register(make_node("node-1"));

        let now = Utc::now();
        assert!(mgr.update_last_seen("node-1", now));
        assert_eq!(mgr.get("node-1").unwrap().last_seen, now);
        assert_eq!(mgr.get("node-1").unwrap().status, NodeStatus::Online);

        assert!(!mgr.update_last_seen("node-999", now));
    }

    #[test]
    fn test_remove() {
        let mgr = RegistryManager::new();
        mgr.register(make_node("node-1"));
        assert_eq!(mgr.count(), 1);

        mgr.remove("node-1");
        assert_eq!(mgr.count(), 0);
    }
}
