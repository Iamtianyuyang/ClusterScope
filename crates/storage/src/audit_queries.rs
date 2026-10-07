use crate::models::AuditLogRow;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn insert_audit_log(
    pool: &PgPool,
    user: &str,
    action: &str,
    target: Option<&str>,
    target_type: Option<&str>,
    details: Option<&str>,
    result: &str,
    source_ip: Option<&str>,
) -> Result<String> {
    let log_id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO audit_logs (log_id, username, action, target, target_type, details, result, source_ip)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(&log_id)
    .bind(user)
    .bind(action)
    .bind(target)
    .bind(target_type)
    .bind(details)
    .bind(result)
    .bind(source_ip)
    .execute(pool)
    .await
    .context("Failed to insert audit log")?;

    Ok(log_id)
}

pub async fn list_audit_logs(
    pool: &PgPool,
    user: Option<&str>,
    action: Option<&str>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<AuditLogRow>, i64)> {
    let offset = page * page_size;

    let mut conditions = vec!["1=1".to_string()];

    // Bind placeholder index = position in `conditions` (index 0 is the
    // constant "1=1", so the first filter gets $1). The same bound values
    // are used for BOTH the count and the page query — forgetting the count
    // query (as before) makes every filtered request fail with a missing
    // bind parameter.
    if user.is_some() {
        conditions.push(format!("username = ${}", conditions.len()));
    }
    if action.is_some() {
        conditions.push(format!("action = ${}", conditions.len()));
    }
    if start_time.is_some() {
        conditions.push(format!("timestamp >= ${}", conditions.len()));
    }
    if end_time.is_some() {
        conditions.push(format!("timestamp <= ${}", conditions.len()));
    }

    let where_clause = conditions.join(" AND ");

    let total_query = format!("SELECT COUNT(*) FROM audit_logs WHERE {}", where_clause);
    let query = format!(
        r#"
        SELECT * FROM audit_logs WHERE {}
        ORDER BY timestamp DESC
        LIMIT ${} OFFSET ${}
        "#,
        where_clause,
        conditions.len(),
        conditions.len() + 1,
    );

    let mut total_q = sqlx::query_as::<_, (i64,)>(&total_query);
    if user.is_some() {
        total_q = total_q.bind(user);
    }
    if action.is_some() {
        total_q = total_q.bind(action);
    }
    if start_time.is_some() {
        total_q = total_q.bind(start_time);
    }
    if end_time.is_some() {
        total_q = total_q.bind(end_time);
    }
    let total: Option<(i64,)> = total_q.fetch_optional(pool).await?;
    let total = total.map(|(t,)| t).unwrap_or(0);

    let mut q = sqlx::query_as::<_, AuditLogRow>(&query);
    if user.is_some() {
        q = q.bind(user);
    }
    if action.is_some() {
        q = q.bind(action);
    }
    if start_time.is_some() {
        q = q.bind(start_time);
    }
    if end_time.is_some() {
        q = q.bind(end_time);
    }
    let logs: Vec<AuditLogRow> = q
        .bind(page_size)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("Failed to list audit logs")?;

    Ok((logs, total))
}

/// Drop audit-log rows older than the 90-day retention window.
pub async fn prune_old_audit_logs(pool: &PgPool) -> Result<()> {
    let cutoff = Utc::now() - chrono::Duration::days(90);
    sqlx::query("DELETE FROM audit_logs WHERE timestamp < $1")
        .bind(cutoff)
        .execute(pool)
        .await
        .context("Failed to prune audit logs")?;
    Ok(())
}
