use crate::models::NodeMetricsRow;
use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::PgPool;

/// Insert or update a node's static info (called on agent registration).
#[allow(clippy::too_many_arguments)]
pub async fn upsert_node_info(
    pool: &PgPool,
    node_id: &str,
    hostname: &str,
    ip_address: &str,
    agent_version: &str,
    os_info: &str,
    kernel_version: &str,
    cpu_model: &str,
    cpu_cores: i32,
    memory_total_bytes: i64,
    labels: serde_json::Value,
    gpu_count: i32,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_info (
            node_id, hostname, ip_address, agent_version, os_info,
            kernel_version, cpu_model, cpu_cores, memory_total_bytes,
            labels, gpu_count, last_seen
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())
        ON CONFLICT (node_id) DO UPDATE SET
            hostname = EXCLUDED.hostname,
            ip_address = EXCLUDED.ip_address,
            agent_version = EXCLUDED.agent_version,
            os_info = EXCLUDED.os_info,
            kernel_version = EXCLUDED.kernel_version,
            cpu_model = EXCLUDED.cpu_model,
            cpu_cores = EXCLUDED.cpu_cores,
            memory_total_bytes = EXCLUDED.memory_total_bytes,
            labels = EXCLUDED.labels,
            gpu_count = EXCLUDED.gpu_count,
            last_seen = NOW()
        "#,
    )
    .bind(node_id)
    .bind(hostname)
    .bind(ip_address)
    .bind(agent_version)
    .bind(os_info)
    .bind(kernel_version)
    .bind(cpu_model)
    .bind(cpu_cores)
    .bind(memory_total_bytes)
    .bind(labels)
    .bind(gpu_count)
    .execute(pool)
    .await
    .context("Failed to upsert node info")?;
    Ok(())
}

pub async fn insert_node_metrics(
    pool: &PgPool,
    node_id: &str,
    sequence: i64,
    timestamp_ms: i64,
    monotonic_clock_ms: Option<i64>,
    cpu_usage_percent: Option<f64>,
    load_1: Option<f64>,
    load_5: Option<f64>,
    load_15: Option<f64>,
    memory_total_bytes: Option<i64>,
    memory_used_bytes: Option<i64>,
    swap_total_bytes: Option<i64>,
    swap_used_bytes: Option<i64>,
    uptime_seconds: Option<i64>,
    boot_time_seconds: Option<i64>,
    gpu_metrics: Option<serde_json::Value>,
    gpu_processes: Option<serde_json::Value>,
    network_metrics: Option<serde_json::Value>,
    disk_metrics: Option<serde_json::Value>,
    cpu_core_metrics: Option<serde_json::Value>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_metrics (
            node_id, sequence, timestamp_ms, monotonic_clock_ms,
            cpu_usage_percent, load_1, load_5, load_15,
            memory_total_bytes, memory_used_bytes,
            swap_total_bytes, swap_used_bytes,
            uptime_seconds, boot_time_seconds,
            gpu_metrics, gpu_processes, network_metrics, disk_metrics, cpu_core_metrics
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
        "#,
    )
    .bind(node_id)
    .bind(sequence)
    .bind(timestamp_ms)
    .bind(monotonic_clock_ms)
    .bind(cpu_usage_percent)
    .bind(load_1)
    .bind(load_5)
    .bind(load_15)
    .bind(memory_total_bytes)
    .bind(memory_used_bytes)
    .bind(swap_total_bytes)
    .bind(swap_used_bytes)
    .bind(uptime_seconds)
    .bind(boot_time_seconds)
    .bind(gpu_metrics)
    .bind(gpu_processes)
    .bind(network_metrics)
    .bind(disk_metrics)
    .bind(cpu_core_metrics)
    .execute(pool)
    .await
    .context("Failed to insert node metrics")?;
    
    Ok(())
}

pub async fn get_latest_metrics(
    pool: &PgPool,
    node_id: &str,
) -> Result<Option<NodeMetricsRow>> {
    sqlx::query_as::<_, NodeMetricsRow>(
        r#"
        SELECT * FROM node_metrics
        WHERE node_id = $1
        ORDER BY timestamp_ms DESC
        LIMIT 1
        "#,
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get latest metrics")
}

pub async fn get_metrics_history(
    pool: &PgPool,
    node_id: &str,
    start_time_ms: i64,
    end_time_ms: i64,
    limit: i64,
) -> Result<Vec<NodeMetricsRow>> {
    sqlx::query_as::<_, NodeMetricsRow>(
        r#"
        SELECT * FROM node_metrics
        WHERE node_id = $1
          AND timestamp_ms >= $2
          AND timestamp_ms <= $3
        ORDER BY timestamp_ms ASC
        LIMIT $4
        "#,
    )
    .bind(node_id)
    .bind(start_time_ms)
    .bind(end_time_ms)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to get metrics history")
}

pub async fn get_metrics_for_time_range(
    pool: &PgPool,
    start_time_ms: i64,
    end_time_ms: i64,
) -> Result<Vec<NodeMetricsRow>> {
    sqlx::query_as::<_, NodeMetricsRow>(
        r#"
        SELECT * FROM node_metrics
        WHERE timestamp_ms >= $1
          AND timestamp_ms <= $2
        ORDER BY timestamp_ms ASC
        LIMIT 10000
        "#,
    )
    .bind(start_time_ms)
    .bind(end_time_ms)
    .fetch_all(pool)
    .await
    .context("Failed to get metrics for time range")
}

pub async fn prune_old_metrics(pool: &PgPool) -> Result<()> {
    // Delete raw metrics older than 24 hours
    let twenty_four_ago = Utc::now() - chrono::Duration::hours(24);
    sqlx::query("DELETE FROM node_metrics WHERE timestamp_ms < $1")
        .bind(twenty_four_ago.timestamp_millis())
        .execute(pool)
        .await?;
    
    Ok(())
}

pub async fn get_job_logs(
    pool: &PgPool,
    job_id: &str,
    offset: i64,
    limit: i64,
    stderr_only: bool,
) -> Result<Vec<crate::models::JobLogRow>> {
    let query = if stderr_only {
        r#"
        SELECT id, job_id, log_offset, log_data, is_stderr, timestamp, created_at
        FROM job_logs WHERE job_id = $1 AND is_stderr = TRUE
        ORDER BY log_offset ASC LIMIT $2 OFFSET $3
        "#
    } else {
        r#"
        SELECT id, job_id, log_offset, log_data, is_stderr, timestamp, created_at
        FROM job_logs WHERE job_id = $1
        ORDER BY log_offset ASC LIMIT $2 OFFSET $3
        "#
    };

    sqlx::query_as::<_, crate::models::JobLogRow>(query)
        .bind(job_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("Failed to get job logs")
}
