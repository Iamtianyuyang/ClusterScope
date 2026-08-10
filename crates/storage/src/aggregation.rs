use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::PgPool;

/// Aggregation window for the hourly pass: re-aggregate the last 25 hours so
/// the currently-open hour bucket stays fresh.
const HOURLY_WINDOW_HOURS: i64 = 25;

/// Aggregate raw metrics into hourly buckets (cpu / memory / gpu utilization).
/// Idempotent: upserts on (node_id, metric_name, hour_bucket).
pub async fn aggregate_to_hourly(pool: &PgPool) -> Result<()> {
    let since = (Utc::now() - chrono::Duration::hours(HOURLY_WINDOW_HOURS)).timestamp_millis();
    sqlx::query(
        r#"
        INSERT INTO metrics_hourly (node_id, metric_name, hour_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        SELECT node_id, metric_name, hour_bucket,
               AVG(v) AS avg_value, MAX(v) AS max_value, MIN(v) AS min_value,
               percentile_cont(0.95) WITHIN GROUP (ORDER BY v) AS p95_value,
               COUNT(*) AS sample_count
        FROM (
            SELECT node_id, hour_bucket, metric_name, v
            FROM (
                SELECT node_id,
                       date_trunc('hour', to_timestamp(timestamp_ms / 1000.0)) AS hour_bucket,
                       'cpu_usage_percent' AS metric_name,
                       cpu_usage_percent AS v
                FROM node_metrics
                WHERE cpu_usage_percent IS NOT NULL AND timestamp_ms >= $1
                UNION ALL
                SELECT node_id,
                       date_trunc('hour', to_timestamp(timestamp_ms / 1000.0)),
                       'memory_used_percent',
                       CASE WHEN memory_total_bytes > 0
                            THEN memory_used_bytes::float / memory_total_bytes::float * 100
                            ELSE NULL END
                FROM node_metrics
                WHERE memory_total_bytes > 0 AND timestamp_ms >= $1
                UNION ALL
                SELECT nm.node_id,
                       date_trunc('hour', to_timestamp(nm.timestamp_ms / 1000.0)),
                       'gpu_utilization',
                       (g->>'utilization_gpu')::float
                FROM node_metrics nm
                CROSS JOIN LATERAL jsonb_array_elements(nm.gpu_metrics) AS g
                WHERE nm.gpu_metrics IS NOT NULL AND nm.timestamp_ms >= $1
            ) x
            WHERE v IS NOT NULL
        ) y
        GROUP BY node_id, metric_name, hour_bucket
        ON CONFLICT (node_id, metric_name, hour_bucket) DO UPDATE SET
            avg_value = EXCLUDED.avg_value,
            max_value = EXCLUDED.max_value,
            min_value = EXCLUDED.min_value,
            p95_value = EXCLUDED.p95_value,
            sample_count = EXCLUDED.sample_count
        "#,
    )
    .bind(since)
    .execute(pool)
    .await
    .context("Failed to aggregate hourly metrics")?;
    Ok(())
}

/// Roll hourly buckets up into daily buckets. Idempotent.
pub async fn aggregate_to_daily(pool: &PgPool) -> Result<()> {
    let since = (Utc::now() - chrono::Duration::days(2)).timestamp();
    sqlx::query(
        r#"
        INSERT INTO metrics_daily (node_id, metric_name, day_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        SELECT node_id, metric_name, hour_bucket::date AS day_bucket,
               AVG(avg_value) AS avg_value,
               MAX(max_value) AS max_value,
               MIN(min_value) AS min_value,
               percentile_cont(0.95) WITHIN GROUP (ORDER BY avg_value) AS p95_value,
               SUM(sample_count) AS sample_count
        FROM metrics_hourly
        WHERE hour_bucket >= $1
        GROUP BY node_id, metric_name, hour_bucket::date
        ON CONFLICT (node_id, metric_name, day_bucket) DO UPDATE SET
            avg_value = EXCLUDED.avg_value,
            max_value = EXCLUDED.max_value,
            min_value = EXCLUDED.min_value,
            p95_value = EXCLUDED.p95_value,
            sample_count = EXCLUDED.sample_count
        "#,
    )
    .bind(since)
    .execute(pool)
    .await
    .context("Failed to aggregate daily metrics")?;
    Ok(())
}

pub async fn cleanup_hourly_data(pool: &PgPool, retention_days: u64) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let result = sqlx::query("DELETE FROM metrics_hourly WHERE hour_bucket < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() as usize)
}

pub async fn cleanup_daily_data(pool: &PgPool, retention_days: u64) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let result = sqlx::query("DELETE FROM metrics_daily WHERE day_bucket < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() as usize)
}
