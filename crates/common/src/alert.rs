use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AlertState {
    Normal = 0,
    Pending = 1,
    Firing = 2,
    Resolved = 3,
}

impl AlertState {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Pending),
            2 => Some(Self::Firing),
            3 => Some(Self::Resolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertOperator {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Neq,
}

impl AlertOperator {
    pub fn evaluate(&self, actual: f64, threshold: f64) -> bool {
        match self {
            Self::Gt => actual > threshold,
            Self::Gte => actual >= threshold,
            Self::Lt => actual < threshold,
            Self::Lte => actual <= threshold,
            Self::Eq => (actual - threshold).abs() < f64::EPSILON,
            Self::Neq => (actual - threshold).abs() >= f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub rule_id: String,
    pub name: String,
    pub description: String,
    pub metric: String,
    pub operator: AlertOperator,
    pub threshold: f64,
    pub duration_seconds: u64,
    pub severity: AlertSeverity,
    pub node_id: String,
    pub gpu_uuids: Vec<String>,
    pub labels: HashMap<String, String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInstance {
    pub key: AlertKey,
    pub state: AlertState,
    pub current_value: Option<f64>,
    pub threshold: f64,
    pub consecutive_count: u32,
    pub triggered_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertKey {
    pub rule_id: String,
    pub node_id: String,
    pub gpu_uuid: String,
}

impl AlertKey {
    pub fn new(rule_id: String, node_id: String, gpu_uuid: String) -> Self {
        Self {
            rule_id,
            node_id,
            gpu_uuid,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub event_id: String,
    pub rule_id: String,
    pub node_id: String,
    pub gpu_uuid: String,
    pub old_state: AlertState,
    pub new_state: AlertState,
    pub current_value: Option<f64>,
    pub threshold: f64,
    pub timestamp: DateTime<Utc>,
}

pub struct AlertEngine {
    instances: Arc<parking_lot::RwLock<HashMap<AlertKey, AlertInstance>>>,
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertEngine {
    pub fn new() -> Self {
        Self {
            instances: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    pub fn evaluate(
        &self,
        rule: &AlertRule,
        node_id: &str,
        gpu_uuid: &str,
        value: f64,
    ) -> Option<AlertEvent> {
        if !rule.enabled {
            return None;
        }

        if !rule.node_id.is_empty() && rule.node_id != node_id {
            return None;
        }

        if !rule.gpu_uuids.is_empty() && !rule.gpu_uuids.contains(&gpu_uuid.to_string()) {
            return None;
        }

        let key = AlertKey::new(
            rule.rule_id.clone(),
            node_id.to_string(),
            gpu_uuid.to_string(),
        );

        let mut instances = self.instances.write();
        let now = Utc::now();

        let instance = instances.entry(key.clone()).or_insert(AlertInstance {
            key: key.clone(),
            state: AlertState::Normal,
            current_value: None,
            threshold: rule.threshold,
            consecutive_count: 0,
            triggered_at: None,
            resolved_at: None,
            last_updated: now,
        });

        let condition_met = rule.operator.evaluate(value, rule.threshold);
        let mut event: Option<AlertEvent> = None;

        match instance.state {
            AlertState::Normal => {
                if condition_met {
                    instance.consecutive_count += 1;
                    instance.current_value = Some(value);

                    if instance.consecutive_count == 1 {
                        instance.state = AlertState::Pending;
                        instance.triggered_at = Some(now);
                        event = Some(AlertEvent {
                            event_id: Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            node_id: node_id.to_string(),
                            gpu_uuid: gpu_uuid.to_string(),
                            old_state: AlertState::Normal,
                            new_state: AlertState::Pending,
                            current_value: Some(value),
                            threshold: rule.threshold,
                            timestamp: now,
                        });
                    }
                }
            }
            AlertState::Pending => {
                if condition_met {
                    instance.consecutive_count += 1;
                    instance.current_value = Some(value);

                    let required_count = rule.duration_seconds.div_ceil(2);
                    if instance.consecutive_count >= required_count as u32 {
                        instance.state = AlertState::Firing;
                        event = Some(AlertEvent {
                            event_id: Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            node_id: node_id.to_string(),
                            gpu_uuid: gpu_uuid.to_string(),
                            old_state: AlertState::Pending,
                            new_state: AlertState::Firing,
                            current_value: Some(value),
                            threshold: rule.threshold,
                            timestamp: now,
                        });
                    }
                } else {
                    instance.consecutive_count = 0;
                    instance.state = AlertState::Normal;
                    instance.triggered_at = None;
                }
            }
            AlertState::Firing => {
                if !condition_met {
                    instance.state = AlertState::Resolved;
                    instance.resolved_at = Some(now);
                    instance.current_value = Some(value);
                    event = Some(AlertEvent {
                        event_id: Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        node_id: node_id.to_string(),
                        gpu_uuid: gpu_uuid.to_string(),
                        old_state: AlertState::Firing,
                        new_state: AlertState::Resolved,
                        current_value: Some(value),
                        threshold: rule.threshold,
                        timestamp: now,
                    });
                } else {
                    instance.current_value = Some(value);
                }
            }
            AlertState::Resolved => {
                if condition_met {
                    instance.state = AlertState::Pending;
                    instance.resolved_at = None;
                    instance.consecutive_count = 1;
                    instance.current_value = Some(value);
                    event = Some(AlertEvent {
                        event_id: Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        node_id: node_id.to_string(),
                        gpu_uuid: gpu_uuid.to_string(),
                        old_state: AlertState::Resolved,
                        new_state: AlertState::Pending,
                        current_value: Some(value),
                        threshold: rule.threshold,
                        timestamp: now,
                    });
                }
            }
        }

        instance.last_updated = now;
        event
    }

    pub fn get_state(&self, key: &AlertKey) -> Option<AlertState> {
        self.instances.read().get(key).map(|i| i.state)
    }

    pub fn get_all_states(&self) -> Vec<AlertInstance> {
        self.instances.read().values().cloned().collect()
    }

    pub fn reset_state(&self, key: &AlertKey) {
        if let Some(instance) = self.instances.write().get_mut(key) {
            instance.state = AlertState::Normal;
            instance.consecutive_count = 0;
            instance.triggered_at = None;
            instance.resolved_at = None;
            instance.current_value = None;
        }
    }

    pub fn clear(&self) {
        self.instances.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rule(id: &str, metric: &str, threshold: f64, duration: u64) -> AlertRule {
        AlertRule {
            rule_id: id.to_string(),
            name: format!("Test rule {}", id),
            description: String::new(),
            metric: metric.to_string(),
            operator: AlertOperator::Gt,
            threshold,
            duration_seconds: duration,
            severity: AlertSeverity::Critical,
            node_id: String::new(),
            gpu_uuids: vec![],
            labels: HashMap::new(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "test".to_string(),
        }
    }

    #[test]
    fn test_operator_evaluate() {
        assert!(AlertOperator::Gt.evaluate(90.0, 85.0));
        assert!(!AlertOperator::Gt.evaluate(80.0, 85.0));

        assert!(AlertOperator::Gte.evaluate(85.0, 85.0));
        assert!(!AlertOperator::Gte.evaluate(84.9, 85.0));

        assert!(AlertOperator::Lt.evaluate(30.0, 50.0));
        assert!(!AlertOperator::Lt.evaluate(60.0, 50.0));

        assert!(AlertOperator::Eq.evaluate(50.0, 50.0));
        assert!(!AlertOperator::Eq.evaluate(50.1, 50.0));
    }

    #[test]
    fn test_alert_state_machine() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);

        // Normal -> Pending (first reading above threshold)
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 90.0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Pending);

        // Pending -> Firing (enough readings)
        for _ in 0..20 {
            engine.evaluate(&rule, "node-1", "gpu-0", 88.0);
        }
        assert_eq!(
            engine
                .get_state(&AlertKey::new(
                    "rule-1".to_string(),
                    "node-1".to_string(),
                    "gpu-0".to_string()
                ))
                .unwrap(),
            AlertState::Firing
        );

        // Firing -> Resolved (reading below threshold)
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 70.0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Resolved);

        // Resolved -> Pending (above again)
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 92.0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Pending);

        // Pending -> Firing again
        for _ in 0..20 {
            engine.evaluate(&rule, "node-1", "gpu-0", 88.0);
        }
        assert_eq!(
            engine
                .get_state(&AlertKey::new(
                    "rule-1".to_string(),
                    "node-1".to_string(),
                    "gpu-0".to_string()
                ))
                .unwrap(),
            AlertState::Firing
        );
    }

    #[test]
    fn test_alert_deduplication() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);

        // Same node+gpu should not re-fire
        for _ in 0..50 {
            engine.evaluate(&rule, "node-1", "gpu-0", 90.0);
        }

        let event = engine.evaluate(&rule, "node-1", "gpu-0", 90.0);
        assert!(event.is_none());
    }

    #[test]
    fn test_different_gpus() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);

        engine.evaluate(&rule, "node-1", "gpu-0", 90.0);
        assert_eq!(
            engine
                .get_state(&AlertKey::new(
                    "rule-1".to_string(),
                    "node-1".to_string(),
                    "gpu-0".to_string()
                ))
                .unwrap(),
            AlertState::Pending
        );

        engine.evaluate(&rule, "node-1", "gpu-1", 90.0);
        assert_eq!(
            engine
                .get_state(&AlertKey::new(
                    "rule-1".to_string(),
                    "node-1".to_string(),
                    "gpu-1".to_string()
                ))
                .unwrap(),
            AlertState::Pending
        );

        // gpu-2 was never evaluated, so no alert state is tracked yet
        assert!(
            engine
                .get_state(&AlertKey::new(
                    "rule-1".to_string(),
                    "node-1".to_string(),
                    "gpu-2".to_string()
                ))
                .is_none()
        );
    }
}
