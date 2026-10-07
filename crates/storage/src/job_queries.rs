// sqlx's `bind` accepts both owned values and references; clippy's
// needless-borrow lint prefers owned, but &field keeps the row usable.
#![allow(clippy::needless_borrows_for_generic_args)]

use crate::models::JobRow;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// `node_id` may be `None` when the scheduler will pick the node later.
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
    .bind(if job.node_id.is_empty() {
        None::<String>
    } else {
        Some(job.node_id.clone())
    })
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
        "SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs WHERE job_id = $1",
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
    let mut bind_values: Vec<String> = Vec::new();
    let mut n = 0usize;
    if let Some(nid) = node_id {
        n += 1;
        bind_values.push(nid.to_string());
        conditions.push(format!("node_id = ${}", n));
    }
    if let Some(st) = status {
        n += 1;
        bind_values.push(st.to_string());
        conditions.push(format!("status = ${}", n));
    }
    if let Some(cb) = created_by {
        n += 1;
        bind_values.push(cb.to_string());
        conditions.push(format!("created_by = ${}", n));
    }
    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM jobs {}", where_sql);
    let list_sql = format!(
        "SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        where_sql,
        n + 1,
        n + 2
    );

    // Count first, then rows — both with the same bound values.
    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_sql);
    for v in &bind_values {
        count_q = count_q.bind(v);
    }
    let total: i64 = count_q
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(t,)| t)
        .unwrap_or(0);

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

/// Oldest-first scan of queued jobs for the scheduler.
///
/// The API-facing [`list_jobs`] is newest-first and capped at one page, so a
/// scheduler reusing it would starve the oldest jobs: whenever more than
/// `page_size` jobs are queued, every cycle would re-scan the same newest
/// 100 while older jobs never enter the in-memory queue. Oldest-first keeps
/// FIFO ordering (a job waits while the jobs ahead of it wait).
///
/// All queued jobs are loaded (no page cap): the in-memory scheduler queue
/// dedups on job_id, so a cap here would let the N oldest unschedulable jobs
/// (e.g. all pinned to one busy node) occupy the queue forever while newer
/// jobs — that could run on free nodes — never get a chance (head-of-line
/// blocking).
pub async fn list_queued_jobs_for_scheduling(pool: &PgPool) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs
        WHERE status = 'queued'
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to list queued jobs for scheduling")
}

/// Fetch jobs for a node that need agent attention: assigned (`starting`)
/// and cancellation requests (`stopping`).
pub async fn get_jobs_for_node(pool: &PgPool, node_id: &str) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs
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
/// Conditional dispatch: only a job that is still `queued` is moved to
/// `starting` (a job cancelled between the in-memory schedule pass and this
/// write must not be resurrected). Returns true when the row was updated.
pub async fn assign_job_to_node(pool: &PgPool, job_id: &str, node_id: &str) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE jobs SET node_id = $2, status = 'starting', started_at = NOW()
        WHERE job_id = $1 AND status = 'queued'
        "#,
    )
    .bind(job_id)
    .bind(node_id)
    .execute(pool)
    .await
    .context("Failed to assign job to node")?;
    Ok(result.rows_affected() > 0)
}

/// Re-queue jobs stuck in `starting` past the cutoff (e.g. the server
/// List jobs stuck in 'starting' past the cutoff (agent may be dead —
/// e.g. the server restarted before the agent picked them up, or the
/// agent died).
/// Returns (job_id, assigned node_id) so the caller can check whether the
/// node is still alive before requeueing — a slow-but-alive agent must not
/// be raced by a second dispatch (double-run).
pub async fn list_stale_starting_jobs(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT job_id, COALESCE(node_id, '') AS node_id
        FROM jobs
        WHERE status = 'starting' AND started_at < $1
        "#,
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .context("Failed to list stale starting jobs")?;
    Ok(rows)
}

/// Requeue a single stale starting job. Status-guarded so a job that
/// completed (or was cancelled) between the list and this update is never
/// rewritten. Returns the number of rows actually updated (0 when the job
/// left `starting` in the meantime — the caller must not free scheduler
/// capacity for a job that is still running elsewhere).
pub async fn requeue_stale_job(pool: &PgPool, job_id: &str) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'queued', started_at = NULL, pid = NULL, error_message = NULL
        WHERE job_id = $1 AND status = 'starting'
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await
    .context("Failed to requeue stale starting job")?;
    Ok(result.rows_affected())
}

/// Mark running/stopping jobs on dead nodes as 'lost' (agent death would
/// otherwise leave them occupying scheduler capacity forever). Returns the
/// affected job ids so the caller can free in-memory scheduler state.
pub async fn mark_lost_jobs_on_nodes(
    pool: &PgPool,
    node_ids: &[String],
    message: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        UPDATE jobs
        SET status = 'lost', finished_at = NOW(), error_message = $2
        WHERE node_id = ANY($1) AND status IN ('running', 'stopping')
        RETURNING job_id
        "#,
    )
    .bind(node_ids)
    .bind(message)
    .fetch_all(pool)
    .await
    .context("Failed to mark lost jobs")?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Queued jobs waiting for a node, oldest first (FIFO scheduling order).
pub async fn list_queued_jobs(pool: &PgPool, limit: i64) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs
        WHERE status = 'queued'
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to list queued jobs")
}

/// Jobs currently occupying capacity (used to rebuild the scheduler's
/// in-memory running set after a server restart).
pub async fn list_active_jobs(pool: &PgPool) -> Result<Vec<JobRow>> {
    sqlx::query_as::<_, JobRow>(
        r#"
        SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs
        WHERE status IN ('starting', 'running', 'stopping')
        "#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to list active jobs")
}

#[allow(clippy::too_many_arguments)]
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
        SELECT job_id, COALESCE(node_id, '') AS node_id, name, executable, arguments,
       working_directory, environment, status, pid, exit_code, error_message,
       created_at, started_at, finished_at, created_by, resource_quota,
       retry_count, max_retries FROM jobs
        WHERE node_id = $1 AND status IN ('running', 'starting')
        "#,
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .context("Failed to get running jobs")
}

/// Cancel a job that has not been dispatched yet. Conditional on the job
/// still being `queued` (a concurrent dispatch must win, not be clobbered),
/// returns `true` when this call performed the cancellation.
pub async fn cancel_queued_job(pool: &PgPool, job_id: &str) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE jobs SET status = 'cancelled',
               error_message = 'cancelled before start',
               finished_at = NOW()
        WHERE job_id = $1 AND status = 'queued'
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await
    .context("Failed to cancel queued job")?;
    Ok(result.rows_affected() > 0)
}

/// Record a forced-cancellation request on a `stopping` job so the agent
/// escalates to SIGKILL immediately instead of waiting out the grace period
/// (see `common::job::FORCE_CANCEL_MARKER`).
pub async fn mark_force_cancel(pool: &PgPool, job_id: &str) -> Result<()> {
    sqlx::query("UPDATE jobs SET error_message = $2 WHERE job_id = $1")
        .bind(job_id)
        .bind(common::job::FORCE_CANCEL_MARKER)
        .execute(pool)
        .await
        .context("Failed to record the forced cancellation")?;
    Ok(())
}

/// `starting`/`running` -> `stopping`; returns false when
/// the job is not in a state an agent could still be running (the caller then
/// re-reads the row instead of clobbering a terminal status).
pub async fn mark_stopping_if_active(pool: &PgPool, job_id: &str) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE jobs SET status = 'stopping'
        WHERE job_id = $1 AND status IN ('starting', 'running')
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await
    .context("Failed to mark job stopping")?;
    Ok(result.rows_affected() > 0)
}

/// Jobs stuck in `stopping` past `cutoff` are marked `lost` and returned: the
/// agent never reported the kill (crashed, or the process ignored
/// SIGTERM+SIGKILL), and nothing else re-reads a `stopping` row.
///
/// M6 adaptation: B keyed this on a `status_updated_at` column this tree does
/// not have (adding it would need a real migration on the shared database),
/// so the run start is the closest available "running for too long" marker.
pub async fn reset_stale_stopping_jobs(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        UPDATE jobs
        SET status = 'lost',
            error_message = COALESCE(error_message, 'stopping timed out'),
            finished_at = NOW()
        WHERE status = 'stopping'
          AND COALESCE(started_at, created_at) < $1
        RETURNING job_id
        "#,
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .context("Failed to reset stale stopping jobs")?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Mark `running` jobs of nodes that have been offline past the reap window as
/// `lost` (agent died / network gone). Returns the affected job ids.
pub async fn mark_running_jobs_lost(
    pool: &PgPool,
    dead_node_ids: &[String],
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        UPDATE jobs
        SET status = 'lost',
            error_message = COALESCE(error_message, 'node offline for extended period'),
            finished_at = NOW()
        WHERE node_id = ANY($1) AND status = 'running'
        RETURNING job_id
        "#,
    )
    .bind(dead_node_ids)
    .fetch_all(pool)
    .await
    .context("Failed to mark running jobs lost")?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Move jobs stuck in `starting` back to `queued` (agent died or the server
/// restarted mid-dispatch), unless their node is still online — a slow but
/// alive agent must not be raced by a second dispatch. Returns the job ids
/// that were reset.
pub async fn reset_stale_starting_jobs(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
    online_node_ids: &[String],
) -> Result<Vec<String>> {
    // NOT (node_id = ANY($2)): with an empty list every stale job qualifies
    // (e.g. right after a server restart, before agents re-register).
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        UPDATE jobs
        SET status = 'queued', started_at = NULL, pid = NULL, error_message = NULL
        WHERE status = 'starting' AND started_at < $1
          AND NOT (node_id = ANY($2))
        RETURNING job_id
        "#,
    )
    .bind(cutoff)
    .bind(online_node_ids)
    .fetch_all(pool)
    .await
    .context("Failed to reset stale starting jobs")?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
