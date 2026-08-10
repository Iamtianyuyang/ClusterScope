use crate::models::{AlertEventRow, AlertRuleRow};
use anyhow::{Context, Result};
use sqlx::PgPool;

pub async fn insert_alert_rule(
    pool: &PgPool,
    rule_id: &str,
    name: &str,
    description: &str,
    metric: &str,
    operator: &str,
    threshold: f64,
    duration_seconds: i32,
    severity: &str,
    node_id: &str,
    gpu_uuids: &serde_json::Value,
    labels: &serde_json::Value,
    enabled: bool,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO alert_rules (
            rule_id, name, description, metric, operator, threshold,
            duration_seconds, severity, node_id, gpu_uuids, labels, enabled, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
    )
    .bind(rule_id)
    .bind(name)
    .bind(description)
    .bind(metric)
    .bind(operator)
    .bind(threshold)
    .bind(duration_seconds)
    .bind(severity)
    .bind(node_id)
    .bind(gpu_uuids)
    .bind(labels)
    .bind(enabled)
    .bind(created_by)
    .execute(pool)
    .await
    .context("Failed to insert alert rule")?;
    
    Ok(())
}

pub async fn get_alert_rule(pool: &PgPool, rule_id: &str) -> Result<Option<AlertRuleRow>> {
    sqlx::query_as::<_, AlertRuleRow>(
        "SELECT * FROM alert_rules WHERE rule_id = $1",
    )
    .bind(rule_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get alert rule")
}

pub async fn list_alert_rules(pool: &PgPool) -> Result<Vec<AlertRuleRow>> {
    sqlx::query_as::<_, AlertRuleRow>(
        "SELECT * FROM alert_rules ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list alert rules")
}

pub async fn update_alert_rule(
    pool: &PgPool,
    rule_id: &str,
    name: &str,
    description: &str,
    metric: &str,
    operator: &str,
    threshold: f64,
    duration_seconds: i32,
    severity: &str,
    node_id: &str,
    gpu_uuids: &serde_json::Value,
    labels: &serde_json::Value,
    enabled: bool,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE alert_rules SET
            name = $2, description = $3, metric = $4, operator = $5,
            threshold = $6, duration_seconds = $7, severity = $8,
            node_id = $9, gpu_uuids = $10, labels = $11, enabled = $12,
            updated_at = NOW()
        WHERE rule_id = $1
        "#,
    )
    .bind(rule_id)
    .bind(name)
    .bind(description)
    .bind(metric)
    .bind(operator)
    .bind(threshold)
    .bind(duration_seconds)
    .bind(severity)
    .bind(node_id)
    .bind(gpu_uuids)
    .bind(labels)
    .bind(enabled)
    .execute(pool)
    .await
    .context("Failed to update alert rule")?;
    
    Ok(())
}

pub async fn delete_alert_rule(pool: &PgPool, rule_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM alert_rules WHERE rule_id = $1")
        .bind(rule_id)
        .execute(pool)
        .await
        .context("Failed to delete alert rule")?;
    
    Ok(())
}

pub async fn insert_alert_event(
    pool: &PgPool,
    event_id: &str,
    rule_id: &str,
    node_id: &str,
    gpu_uuid: &str,
    old_state: &str,
    new_state: &str,
    current_value: Option<f64>,
    threshold: f64,
    notification_sent: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO alert_events (
            event_id, rule_id, node_id, gpu_uuid, old_state, new_state,
            current_value, threshold, notification_sent
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(event_id)
    .bind(rule_id)
    .bind(node_id)
    .bind(gpu_uuid)
    .bind(old_state)
    .bind(new_state)
    .bind(current_value)
    .bind(threshold)
    .bind(notification_sent)
    .execute(pool)
    .await
    .context("Failed to insert alert event")?;
    
    Ok(())
}

pub async fn get_active_alert_events(pool: &PgPool) -> Result<Vec<AlertEventRow>> {
    sqlx::query_as::<_, AlertEventRow>(
        r#"
        SELECT DISTINCT ON (node_id, rule_id) *
        FROM alert_events
        WHERE new_state IN ('pending', 'firing')
        ORDER BY node_id, rule_id, timestamp DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to get active alert events")
}

pub async fn get_alert_events_by_rule(
    pool: &PgPool,
    rule_id: &str,
    limit: i64,
) -> Result<Vec<AlertEventRow>> {
    sqlx::query_as::<_, AlertEventRow>(
        r#"
        SELECT * FROM alert_events
        WHERE rule_id = $1
        ORDER BY timestamp DESC
        LIMIT $2
        "#,
    )
    .bind(rule_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to get alert events")
}
