use crate::models::AuditLogRow;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

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
    
    if let Some(_user) = user {
        conditions.push(format!("username = ${}", conditions.len() + 1));
    }
    if let Some(_action) = action {
        conditions.push(format!("action = ${}", conditions.len() + 1));
    }
    if let Some(_start) = start_time {
        conditions.push(format!("timestamp >= ${}", conditions.len() + 1));
    }
    if let Some(_end) = end_time {
        conditions.push(format!("timestamp <= ${}", conditions.len() + 1));
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
        conditions.len() + 1,
        conditions.len() + 2,
    );
    
    let total: Option<(i64,)> = sqlx::query_as(&total_query).fetch_optional(pool).await?;
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
