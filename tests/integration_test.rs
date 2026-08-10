#[cfg(test)]
mod tests {
    use common::alert::{AlertEngine, AlertRule, AlertSeverity, AlertOperator};
    use common::job::{Job, JobDefinition, JobStatus, can_transition, validate_transition};
    use common::node_registry::{NodeRegistry, RegistryManager, NodeStatus, NodeEntry};
    use common::metrics::MetricsAggregation;
    use common::dedup::{SequenceDeduplicator, SequenceTracker};
    use common::auth::{hash_password, verify_password, generate_jwt, verify_jwt, UserRole};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_test_rule(id: &str, threshold: f64, duration: u64) -> AlertRule {
        AlertRule {
            rule_id: id.to_string(),
            name: format!("Test rule {}", id),
            description: String::new(),
            metric: "gpu_temperature".to_string(),
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

    fn make_test_node(id: &str) -> NodeEntry {
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
            registered_at: Utc::now(),
            last_seen: Utc::now(),
            status: NodeStatus::Online,
            labels: HashMap::new(),
            gpu_count: 4,
        }
    }

    #[test]
    fn test_node_registry() {
        let mgr = RegistryManager::new();
        mgr.register(make_test_node("node-1"));
        assert_eq!(mgr.count(), 1);
        assert!(mgr.exists("node-1"));
        assert!(!mgr.exists("node-2"));

        mgr.update_status("node-1", NodeStatus::Offline);
        assert_eq!(mgr.get("node-1").unwrap().status, NodeStatus::Offline);

        let online = mgr.list_online();
        assert_eq!(online.len(), 0);
    }

    #[test]
    fn test_alert_state_machine() {
        let engine = AlertEngine::new();
        let rule = make_test_rule("rule-1", 85.0, 30);

        // Normal -> Pending
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 90.0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, common::alert::AlertState::Pending);

        // Pending -> Firing after enough readings
        for _ in 0..20 {
            engine.evaluate(&rule, "node-1", "gpu-0", 88.0);
        }
        assert_eq!(
            engine.get_state(&common::alert::AlertKey::new(
                "rule-1".to_string(), "node-1".to_string(), "gpu-0".to_string()
            )).unwrap(),
            common::alert::AlertState::Firing
        );

        // Firing -> Resolved
        let event = engine.evaluate(&rule, "node-1", "gpu-0", 70.0);
        assert!(event.is_some());
        assert_eq!(event.unwrap().new_state, common::alert::AlertState::Resolved);
    }

    #[test]
    fn test_job_state_transitions() {
        let mut job = Job::from_definition(&JobDefinition {
            job_id: "job-1".to_string(),
            node_id: "node-1".to_string(),
            name: "test".to_string(),
            executable: "/bin/test".to_string(),
            arguments: vec![],
            working_directory: "/tmp".to_string(),
            environment: HashMap::new(),
            max_concurrent: 1,
            created_at: Utc::now(),
            created_by: "test".to_string(),
            description: String::new(),
            max_retries: 0,
        });

        assert!(can_transition(JobStatus::Queued, JobStatus::Starting));
        assert!(can_transition(JobStatus::Running, JobStatus::Stopping));
        assert!(!can_transition(JobStatus::Succeeded, JobStatus::Running));

        job.transition_to(JobStatus::Starting).unwrap();
        job.transition_to(JobStatus::Running).unwrap();
        job.transition_to(JobStatus::Stopping).unwrap();
        job.transition_to(JobStatus::Succeeded).unwrap();

        assert_eq!(job.status, JobStatus::Succeeded);
    }

    #[test]
    fn test_metrics_aggregation() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
        let agg = MetricsAggregation::new("gpu_util".to_string(), &values, 0, 1000);

        assert_eq!(agg.count, 10);
        assert!((agg.avg - 55.0).abs() < f64::EPSILON);
        assert_eq!(agg.max, 100.0);
        assert_eq!(agg.min, 10.0);
    }

    #[test]
    fn test_password_auth() {
        let hash = hash_password("test-password").unwrap();
        assert!(verify_password("test-password", &hash).is_ok());
        assert!(verify_password("wrong-password", &hash).is_err());
    }

    #[test]
    fn test_jwt() {
        let token = generate_jwt("user-1", "admin", "secret", 3600).unwrap();
        let claims = verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn test_role_permissions() {
        assert!(UserRole::Viewer.can_read_metrics());
        assert!(!UserRole::Viewer.can_manage_jobs());

        assert!(UserRole::Operator.can_manage_jobs());
        assert!(UserRole::Operator.can_stop_jobs());
        assert!(!UserRole::Operator.can_manage_nodes());

        assert!(UserRole::Admin.can_manage_nodes());
        assert!(UserRole::Admin.can_manage_users());
        assert!(UserRole::Admin.can_manage_rules());
    }

    #[test]
    fn test_deduplication() {
        let dedup = SequenceDeduplicator::new(1000);
        assert!(dedup.try_insert(1));
        assert!(!dedup.try_insert(1));
        assert!(dedup.try_insert(2));
        assert!(!dedup.try_insert(2));
    }

    #[test]
    fn test_sequence_tracker() {
        let tracker = SequenceTracker::new();
        assert!(tracker.check("node-1", 1));
        assert!(!tracker.check("node-1", 1));
        assert!(tracker.check("node-2", 1));
        assert!(!tracker.check("node-2", 1));
    }
}
