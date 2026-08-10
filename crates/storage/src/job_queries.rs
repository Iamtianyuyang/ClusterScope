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

    // Build a parameterized query with proper bind order.
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<serde_json::Value> = Vec::new();
    let mut n = 0usize;
    if let Some(nid) = node_id {
        n += 1;
        bind_values.push(serde_json::Value::String(nid.to_string()));
        conditions.push(format!("node_id = ${}", n));
    }
    if let Some(st) = status {
        n += 1;
        bind_values.push(serde_json::Value::String(st.to_string()));
        conditions.push(format!("status = ${}", n));
    }
    if let Some(cb) = created_by {
        n += 1;
        bind_values.push(serde_json::Value::String(cb.to_string()));
        conditions.push(format!("created_by = ${}", n));
    }
    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM jobs {}", where_sql);
    let list_sql = format!(
        "SELECT * FROM jobs {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        where_sql,
        n + 1,
        n + 2
    );

    // Count first, then rows — both with the same bound values.
    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_sql);
    for v in &bind_values {
        count_q = count_q.bind(v);
    }
    let total: i64 = count_q.fetch_optional(pool).await.ok().flatten().map(|(t,)| t).unwrap_or(0);

    let mut q = sqlx::query_as::<_, JobRow>(&list_sql);
    for v in &bind_values {
        q = q.bind(v);
    }
    let jobs = q
        .bind(page_size)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("Failed to list jobs")?;

    Ok((jobs, total))
}

/// Fetch jobs for a node that need agent attention: assigned (`starting`)
/// and cancellation requests (`stopping`).
pub async fn get_jobs_for_node(pool: &PgPool, node_id: &str) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT * FROM jobs
        WHERE node_id = $1 AND status IN ('starting', 'stopping')
        ORDER BY created_at ASC
        "#,
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .context("Failed to get jobs for node")
}

/// Assign a queued job to a node (scheduler dispatch) and mark it starting.
pub async fn assign_job_to_node(
    pool: &PgPool,
    job_id: &str,
    node_id: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE jobs SET node_id = $2, status = 'starting', started_at = NOW()
        WHERE job_id = $1
        "#,
    )
    .bind(job_id)
    .bind(node_id)
    .execute(pool)
    .await
    .context("Failed to assign job to node")?;
    Ok(())
}

/// Re-queue jobs stuck in `starting` past the cutoff (e.g. the server
/// restarted before their agent picked them up, or the agent died).
pub async fn reset_stale_starting_jobs(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<usize> {
    let result = sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'queued', started_at = NULL, pid = NULL, error_message = NULL
        WHERE status = 'starting' AND started_at < $1
        "#,
    )
    .bind(cutoff)
    .execute(pool)
    .await
    .context("Failed to reset stale starting jobs")?;
    Ok(result.rows_affected() as usize)
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
