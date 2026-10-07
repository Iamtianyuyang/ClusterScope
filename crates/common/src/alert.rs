use chrono::{DateTime, Duration as ChronoDuration, Utc};
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
            Self::Eq => approx_eq(actual, threshold),
            Self::Neq => !approx_eq(actual, threshold),
        }
    }
}

/// Float equality with a relative tolerance. An absolute `f64::EPSILON`
/// comparison is useless for large-magnitude metrics (e.g. `gpu_power_watts`
/// around 300.0): the representable gap between two readings is far larger
/// than EPSILON, so `eq` would never fire. The tolerance scales with the
/// magnitude of the values involved (and degrades to exact-equality for
/// both-zero).
fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * a.abs().max(b.abs())
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

    /// Evaluate a rule against one metrics reading at time `now`.
    ///
    /// The `now` parameter (instead of reading the clock internally) keeps
    /// the state machine testable and makes the duration semantics explicit:
    /// Pending -> Firing happens once the condition has held continuously
    /// for `rule.duration_seconds`, independent of the reporting interval.
    pub fn evaluate(
        &self,
        rule: &AlertRule,
        node_id: &str,
        gpu_uuid: &str,
        value: f64,
        now: DateTime<Utc>,
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

                    // Time-based: fire once the condition has held for the
                    // rule's duration. (The previous count-based heuristic
                    // assumed a fixed 2s report interval and silently drifted
                    // when report_interval_secs was configured differently.)
                    let held_for = instance
                        .triggered_at
                        .map(|t| now.signed_duration_since(t))
                        .unwrap_or_default();
                    let required =
                        ChronoDuration::seconds(rule.duration_seconds.min(i64::MAX as u64) as i64);
                    if held_for >= required {
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
                    // Restart the duration timer for the new pending window.
                    instance.triggered_at = Some(now);
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

    /// Drop all in-memory instances for a rule (called when the rule is
    /// deleted; otherwise instances accumulate forever — one entry per
    /// rule × node × GPU).
    pub fn remove_rule_instances(&self, rule_id: &str) {
        self.instances
            .write()
            .retain(|key, _| key.rule_id != rule_id);
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

        // Relative tolerance: `eq` must work on large-magnitude metrics too
        // (a bare f64::EPSILON comparison would never match at ~300.0).
        assert!(AlertOperator::Eq.evaluate(300.0, 300.0));
        // One ulp apart at 300.0 (≈5.7e-14) is within the relative tolerance.
        assert!(AlertOperator::Eq.evaluate(300.0_f64.next_up(), 300.0));
        assert!(!AlertOperator::Eq.evaluate(300.1, 300.0));
        assert!(AlertOperator::Neq.evaluate(300.1, 300.0));
        assert!(!AlertOperator::Neq.evaluate(300.0, 300.0));
        assert!(AlertOperator::Eq.evaluate(0.0, 0.0));
        assert!(!AlertOperator::Eq.evaluate(0.0, 1e-9));
    }

    #[test]
    fn test_alert_state_machine() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);
        let t0 = Utc::now();

        // Normal -> Pending (first reading above threshold)
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 90.0, t0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Pending);

        // Still pending before the duration has elapsed.
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            88.0,
            t0 + ChronoDuration::seconds(10),
        );
        assert!(event.is_none());
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

        // Pending -> Firing once the condition has held for `duration_seconds`.
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            88.0,
            t0 + ChronoDuration::seconds(31),
        );
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Firing);

        // Firing -> Resolved (reading below threshold)
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            70.0,
            t0 + ChronoDuration::seconds(40),
        );
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Resolved);

        // Resolved -> Pending (above again); the duration timer restarts.
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            92.0,
            t0 + ChronoDuration::seconds(50),
        );
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Pending);

        // Not yet 30s since the second pending window opened.
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            88.0,
            t0 + ChronoDuration::seconds(70),
        );
        assert!(event.is_none());
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

        // Fires again after the full duration.
        let event = engine.evaluate(
            &rule,
            "node-1",
            "gpu-0",
            88.0,
            t0 + ChronoDuration::seconds(81),
        );
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, AlertState::Firing);
    }

    #[test]
    fn test_alert_deduplication() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);
        let t0 = Utc::now();

        // Same node+gpu should not re-fire
        for _ in 0..50 {
            engine.evaluate(&rule, "node-1", "gpu-0", 90.0, t0);
        }

        let event = engine.evaluate(&rule, "node-1", "gpu-0", 90.0, t0);
        assert!(event.is_none());
    }

    #[test]
    fn test_remove_rule_drops_instances() {
        // C-side regression test, kept: deleting a rule must drop every
        // in-memory instance of it (ported to B's evaluate/now signature and
        // B's remove_rule_instances name).
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);
        let t0 = Utc::now();
        engine.evaluate(&rule, "node-1", "gpu-0", 90.0, t0);
        engine.evaluate(&rule, "node-1", "gpu-1", 90.0, t0);
        assert_eq!(engine.get_all_states().len(), 2);

        // Deleting the rule must drop every instance of it.
        engine.remove_rule_instances("rule-1");
        assert!(engine.get_all_states().is_empty());

        // Other rules are untouched.
        let other = make_rule("rule-2", "gpu_temperature", 85.0, 30);
        engine.evaluate(&other, "node-1", "gpu-0", 90.0, t0);
        engine.remove_rule_instances("rule-1");
        assert_eq!(engine.get_all_states().len(), 1);
        engine.remove_rule_instances("rule-2");
        assert!(engine.get_all_states().is_empty());
    }

    #[test]
    fn test_different_gpus() {
        let engine = AlertEngine::new();
        let rule = make_rule("rule-1", "gpu_temperature", 85.0, 30);
        let t0 = Utc::now();

        engine.evaluate(&rule, "node-1", "gpu-0", 90.0, t0);
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

        engine.evaluate(&rule, "node-1", "gpu-1", 90.0, t0);
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
