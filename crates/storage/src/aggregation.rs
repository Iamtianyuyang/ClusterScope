use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct MetricAggregation {
    pub metric_name: String,
    pub avg: f64,
    pub max: f64,
    pub min: f64,
    pub p95: f64,
    pub sample_count: u64,
    pub hour_bucket: DateTime<Utc>,
    pub day_bucket: DateTime<Utc>,
}

pub struct RetentionPolicy {
    raw_retention_hours: u64,
    hourly_retention_days: u64,
    daily_retention_days: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            raw_retention_hours: 24,
            hourly_retention_days: 7,
            daily_retention_days: 90,
        }
    }
}

/// Aggregate raw metrics into hourly buckets
pub async fn aggregate_to_hourly(_pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let query = r#"
        WITH aggregated AS (
            SELECT
                node_id,
                EXTRACT(epoch FROM DATE_TRUNC('hour', TIMESTAMP WITH TIME ZONE '@epoch@'))::bigint * 1000 as hour_bucket,
                metric_name,
                AVG(value) as avg_value,
                MAX(value) as max_value,
                MIN(value) as min_value,
                PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY value) as p95_value,
                COUNT(*) as sample_count
            FROM (
                SELECT node_id, timestamp_ms as ts,
                       'cpu_usage_percent' as metric_name,
                       cpu_usage_percent as value
                FROM node_metrics WHERE cpu_usage_percent IS NOT NULL
                UNION ALL
                SELECT node_id, timestamp_ms, 'memory_used_percent',
                       CASE WHEN memory_total_bytes > 0 THEN memory_used_bytes::float / memory_total_bytes::float * 100 ELSE 0 END
                FROM node_metrics WHERE memory_total_bytes > 0
                UNION ALL
                SELECT node_id, timestamp_ms, 'gpu_utilization',
                       jsonb_array_length(gpu_metrics->'gpus')::float
                FROM node_metrics WHERE gpu_metrics IS NOT NULL
            ) metrics
            WHERE ts >= NOW() - INTERVAL '7 days'
            GROUP BY node_id, hour_bucket, metric_name
        )
        INSERT INTO metrics_hourly (node_id, hour_bucket, metric_name, avg_value, max_value, min_value, p95_value, sample_count)
        SELECT node_id, hour_bucket, metric_name, avg_value, max_value, min_value, p95_value, sample_count
        FROM aggregated
        ON CONFLICT (node_id, metric_name, DATE_TRUNC('hour', TIMESTAMP WITH TIME ZONE '@epoch@'::bigint))
        DO UPDATE SET
            avg_value = EXCLUDED.avg_value,
            max_value = EXCLUDED.max_value,
            min_value = EXCLUDED.min_value,
            p95_value = EXCLUDED.p95_value,
            sample_count = EXCLUDED.sample_count
    "#;
    
    let _ = query;
    Ok(())
}

pub async fn cleanup_hourly_data(pool: &PgPool, retention_days: u64) -> Result<usize, Box<dyn std::error::Error>> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let result = sqlx::query("DELETE FROM metrics_hourly WHERE hour_bucket < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    
    Ok(result.rows_affected() as usize)
}

pub async fn cleanup_daily_data(pool: &PgPool, retention_days: u64) -> Result<usize, Box<dyn std::error::Error>> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let result = sqlx::query("DELETE FROM metrics_daily WHERE day_bucket < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    
    Ok(result.rows_affected() as usize)
}
