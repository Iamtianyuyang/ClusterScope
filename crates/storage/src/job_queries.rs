use crate::models::JobRow;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

pub async fn insert_job(pool: &PgPool, job: &JobRow) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO jobs (
            job_id, node_id, name, executable, arguments, working_directory,
            environment, status, pid, exit_code, error_message,
            created_at, started_at, finished_at, created_by,
            resource_quota, retry_count, max_retries
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        "#,
    )
    .bind(&job.job_id)
    .bind(&job.node_id)
    .bind(&job.name)
    .bind(&job.executable)
    .bind(&job.arguments)
    .bind(&job.working_directory)
    .bind(&job.environment)
    .bind(&job.status)
    .bind(job.pid)
    .bind(job.exit_code)
    .bind(&job.error_message)
    .bind(&job.created_at)
    .bind(&job.started_at)
    .bind(&job.finished_at)
    .bind(&job.created_by)
    .bind(&job.resource_quota)
    .bind(job.retry_count)
    .bind(job.max_retries)
    .execute(pool)
    .await
    .context("Failed to insert job")?;
    
    Ok(())
}

pub async fn get_job(pool: &PgPool, job_id: &str) -> Result<Option<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        "SELECT * FROM jobs WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get job")
}

pub async fn list_jobs(
    pool: &PgPool,
    node_id: Option<&str>,
    status: Option<&str>,
    created_by: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<JobRow>, i64)> {
    let offset = page * page_size;
    
    let (query, total_query) = if let Some(_node_id) = node_id {
        (
            format!(
                r#"
                SELECT * FROM jobs
                WHERE node_id = $1
                {}
                {}
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
                status.map(|_s| "AND status = $4".to_string()).unwrap_or_default(),
                created_by.map(|_c| "AND created_by = $5".to_string()).unwrap_or_default(),
            ),
            format!(
                r#"
                SELECT COUNT(*) FROM jobs
                WHERE node_id = $1
                {}
                "#,
                status.map(|_| "AND status = $2".to_string()).unwrap_or_default(),
            ),
        )
    } else {
        (
            format!(
                r#"
                SELECT * FROM jobs
                {}
                {}
                {}
                ORDER BY created_at DESC
                LIMIT $1 OFFSET $2
                "#,
                status.map(|_s| "AND status = $3".to_string()).unwrap_or_default(),
                created_by.map(|_c| "AND created_by = $4".to_string()).unwrap_or_default(),
                "1=1",
            ),
            format!(
                r#"
                SELECT COUNT(*) FROM jobs
                {}
                {}
                "#,
                status.map(|_| "AND status = $2".to_string()).unwrap_or_default(),
                created_by.map(|_| "AND created_by = $3".to_string()).unwrap_or_default(),
            ),
        )
    };
    
    // Simple approach: get total count first
    let total: Option<(i64,)> = sqlx::query_as(&total_query)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    let total = total.map(|(t,)| t).unwrap_or(0);
    
    let jobs = sqlx::query_as::<_, JobRow>(&query)
        .fetch_all(pool)
        .await;
    
    let jobs = match jobs {
        Ok(j) => j,
        Err(_) => {
            // Try simpler query without conditions
            sqlx::query_as::<_, JobRow>(
                r#"
                SELECT * FROM jobs
                ORDER BY created_at DESC
                LIMIT $1 OFFSET $2
                "#,
            )
            .bind(page_size)
            .bind(offset)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        }
    };
    
    Ok((jobs, total))
}

pub async fn update_job_status(
    pool: &PgPool,
    job_id: &str,
    status: &str,
    pid: Option<i32>,
    exit_code: Option<i32>,
    error_message: Option<&str>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE jobs SET
            status = $2,
            pid = $3,
            exit_code = $4,
            error_message = $5,
            started_at = COALESCE($6, started_at),
            finished_at = COALESCE($7, finished_at)
        WHERE job_id = $1
        "#,
    )
    .bind(job_id)
    .bind(status)
    .bind(pid)
    .bind(exit_code)
    .bind(error_message)
    .bind(started_at)
    .bind(finished_at)
    .execute(pool)
    .await
    .context("Failed to update job status")?;
    
    Ok(())
}

pub async fn get_running_jobs(pool: &PgPool, node_id: &str) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT * FROM jobs
        WHERE node_id = $1 AND status IN ('running', 'starting')
        "#,
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .context("Failed to get running jobs")
}
