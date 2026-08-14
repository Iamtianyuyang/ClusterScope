use crate::models::{AlertEventRow, AlertRuleRow};
use anyhow::{Context, Result};
use sqlx::PgPool;

#[allow(clippy::too_many_arguments)]
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
    sqlx::query_as::<_, AlertRuleRow>("SELECT * FROM alert_rules WHERE rule_id = $1")
        .bind(rule_id)
        .fetch_optional(pool)
        .await
        .context("Failed to get alert rule")
}

pub async fn list_alert_rules(pool: &PgPool) -> Result<Vec<AlertRuleRow>> {
    sqlx::query_as::<_, AlertRuleRow>("SELECT * FROM alert_rules ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
        .context("Failed to list alert rules")
}

#[allow(clippy::too_many_arguments)]
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

/// Delete an alert rule together with its events (the events table has a
/// RESTRICT FK on rule_id, so a bare rule delete fails with 500 once the
/// rule has fired). One transaction keeps both deletes atomic.
pub async fn delete_alert_rule_cascade(pool: &PgPool, rule_id: &str) -> Result<u64> {
    let mut tx = pool
        .begin()
        .await
        .context("Failed to begin delete_alert_rule transaction")?;
    sqlx::query("DELETE FROM alert_events WHERE rule_id = $1")
        .bind(rule_id)
        .execute(&mut *tx)
        .await
        .context("Failed to delete alert events")?;
    let result = sqlx::query("DELETE FROM alert_rules WHERE rule_id = $1")
        .bind(rule_id)
        .execute(&mut *tx)
        .await
        .context("Failed to delete alert rule")?;
    tx.commit()
        .await
        .context("Failed to commit delete_alert_rule transaction")?;
    Ok(result.rows_affected())
}

#[allow(clippy::too_many_arguments)]
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
    // Take the LATEST event per (node_id, rule_id, gpu_uuid) first, then
    // filter — otherwise a stale firing row shadows a newer resolved one.
    sqlx::query_as::<_, AlertEventRow>(
        r#"
        SELECT * FROM (
            SELECT DISTINCT ON (node_id, rule_id, gpu_uuid) *
            FROM alert_events
            ORDER BY node_id, rule_id, gpu_uuid, timestamp DESC
        ) latest
        WHERE new_state IN ('pending', 'firing')
        ORDER BY timestamp DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to get active alert events")
}

/// Self-heal: alert instances whose latest event is still pending/firing but
/// older than `cutoff` get a synthetic `resolved` event (e.g. after a server
/// restart the in-memory engine lost their state).
pub async fn expire_stale_alerts(
    pool: &PgPool,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Result<usize> {
    let result = sqlx::query(
        r#"
        INSERT INTO alert_events (
            event_id, rule_id, node_id, gpu_uuid, old_state, new_state,
            current_value, threshold, timestamp
        )
        SELECT md5(random()::text || clock_timestamp()::text),
               t.rule_id, t.node_id, t.gpu_uuid, t.new_state, 'resolved',
               t.current_value, t.threshold, NOW()
        FROM (
            SELECT DISTINCT ON (rule_id, node_id, gpu_uuid) *
            FROM alert_events
            ORDER BY rule_id, node_id, gpu_uuid, timestamp DESC
        ) t
        WHERE t.new_state IN ('pending', 'firing') AND t.timestamp < $1
        "#,
    )
    .bind(cutoff)
    .execute(pool)
    .await
    .context("Failed to expire stale alerts")?;
    Ok(result.rows_affected() as usize)
}

/// Number of targets whose latest alert event is still pending/firing.
pub async fn count_active_alerts(pool: &PgPool) -> Result<i64> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM (
            SELECT DISTINCT ON (node_id, rule_id, gpu_uuid) *
            FROM alert_events
            ORDER BY node_id, rule_id, gpu_uuid, timestamp DESC
        ) latest
        WHERE latest.new_state IN ('pending', 'firing')
        "#,
    )
    .fetch_one(pool)
    .await
    .context("Failed to count active alerts")?;
    Ok(count)
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
