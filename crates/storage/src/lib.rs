use common::config::ServerConfig;
use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

pub mod models;
pub mod conversions;
pub mod queries;
pub mod migrations;
pub mod aggregation;
pub mod alert_queries;
pub mod job_queries;
pub mod user_queries;
pub mod audit_queries;

pub struct DatabasePool {
    pub pool: Arc<sqlx::Pool<sqlx::Postgres>>,
}

impl DatabasePool {
    pub async fn new(config: &ServerConfig) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(50)
            .connect(&config.postgres_url)
            .await
            .context("Failed to connect to PostgreSQL")?;
        
        // Run migrations
        Self::run_migrations(&pool, &config.default_admin_password).await?;
        
        Ok(Self { pool: Arc::new(pool) })
    }
    
    async fn run_migrations(pool: &sqlx::Pool<sqlx::Postgres>, admin_password: &str) -> Result<()> {
        
        
        // Create tables (multi-statement script: raw execution, not prepared)
        sqlx::raw_sql(
            r#"
            CREATE TABLE IF NOT EXISTS node_info (
                node_id VARCHAR(255) PRIMARY KEY,
                hostname VARCHAR(255) NOT NULL,
                ip_address VARCHAR(45) NOT NULL,
                agent_version VARCHAR(50),
                os_info TEXT,
                kernel_version VARCHAR(100),
                cpu_model VARCHAR(255),
                cpu_cores INTEGER NOT NULL DEFAULT 0,
                memory_total_bytes BIGINT NOT NULL DEFAULT 0,
                agent_platform VARCHAR(100),
                registered_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                last_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                labels JSONB DEFAULT '{}'::jsonb,
                gpu_count INTEGER NOT NULL DEFAULT 0
            );
            
            CREATE TABLE IF NOT EXISTS node_metrics (
                id BIGSERIAL PRIMARY KEY,
                node_id VARCHAR(255) NOT NULL REFERENCES node_info(node_id),
                sequence BIGINT NOT NULL,
                timestamp_ms BIGINT NOT NULL,
                monotonic_clock_ms BIGINT,
                cpu_usage_percent DOUBLE PRECISION,
                load_1 DOUBLE PRECISION,
                load_5 DOUBLE PRECISION,
                load_15 DOUBLE PRECISION,
                memory_total_bytes BIGINT,
                memory_used_bytes BIGINT,
                swap_total_bytes BIGINT,
                swap_used_bytes BIGINT,
                uptime_seconds BIGINT,
                boot_time_seconds BIGINT,
                gpu_metrics JSONB,
                gpu_processes JSONB,
                network_metrics JSONB,
                disk_metrics JSONB,
                cpu_core_metrics JSONB,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_node_metrics_node_time ON node_metrics(node_id, timestamp_ms);
            CREATE INDEX IF NOT EXISTS idx_node_metrics_timestamp ON node_metrics(timestamp_ms);
            
            CREATE TABLE IF NOT EXISTS metrics_hourly (
                id BIGSERIAL PRIMARY KEY,
                node_id VARCHAR(255) NOT NULL REFERENCES node_info(node_id),
                metric_name VARCHAR(255) NOT NULL,
                hour_bucket TIMESTAMP WITH TIME ZONE NOT NULL,
                avg_value DOUBLE PRECISION,
                max_value DOUBLE PRECISION,
                min_value DOUBLE PRECISION,
                p95_value DOUBLE PRECISION,
                sample_count BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                UNIQUE(node_id, metric_name, hour_bucket)
            );
            
            CREATE INDEX IF NOT EXISTS idx_metrics_hourly_lookup ON metrics_hourly(node_id, metric_name, hour_bucket);
            
            CREATE TABLE IF NOT EXISTS metrics_daily (
                id BIGSERIAL PRIMARY KEY,
                node_id VARCHAR(255) NOT NULL REFERENCES node_info(node_id),
                metric_name VARCHAR(255) NOT NULL,
                day_bucket DATE NOT NULL,
                avg_value DOUBLE PRECISION,
                max_value DOUBLE PRECISION,
                min_value DOUBLE PRECISION,
                p95_value DOUBLE PRECISION,
                sample_count BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                UNIQUE(node_id, metric_name, day_bucket)
            );
            
            CREATE INDEX IF NOT EXISTS idx_metrics_daily_lookup ON metrics_daily(node_id, metric_name, day_bucket);
            
            CREATE TABLE IF NOT EXISTS jobs (
                job_id VARCHAR(255) PRIMARY KEY,
                node_id VARCHAR(255) NOT NULL REFERENCES node_info(node_id),
                name VARCHAR(255) NOT NULL,
                executable TEXT NOT NULL,
                arguments JSONB NOT NULL DEFAULT '[]'::jsonb,
                working_directory TEXT NOT NULL DEFAULT '/',
                environment JSONB NOT NULL DEFAULT '{}'::jsonb,
                status VARCHAR(50) NOT NULL DEFAULT 'queued',
                pid INTEGER,
                exit_code INTEGER,
                error_message TEXT,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                started_at TIMESTAMP WITH TIME ZONE,
                finished_at TIMESTAMP WITH TIME ZONE,
                created_by VARCHAR(255) NOT NULL,
                resource_quota TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 0
            );
            
            CREATE INDEX IF NOT EXISTS idx_jobs_node ON jobs(node_id);
            CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
            CREATE INDEX IF NOT EXISTS idx_jobs_created_at ON jobs(created_at);
            
            CREATE TABLE IF NOT EXISTS job_logs (
                id BIGSERIAL PRIMARY KEY,
                job_id VARCHAR(255) NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
                log_offset BIGINT NOT NULL,
                log_data TEXT NOT NULL,
                is_stderr BOOLEAN NOT NULL DEFAULT FALSE,
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                UNIQUE(job_id, log_offset)
            );
            
            CREATE INDEX IF NOT EXISTS idx_job_logs_job_offset ON job_logs(job_id, log_offset);
            
            CREATE TABLE IF NOT EXISTS alert_rules (
                rule_id VARCHAR(255) PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                description TEXT,
                metric VARCHAR(255) NOT NULL,
                operator VARCHAR(20) NOT NULL,
                threshold DOUBLE PRECISION NOT NULL,
                duration_seconds INTEGER NOT NULL DEFAULT 30,
                severity VARCHAR(20) NOT NULL DEFAULT 'warning',
                node_id VARCHAR(255) NOT NULL DEFAULT '',
                gpu_uuids JSONB NOT NULL DEFAULT '[]'::jsonb,
                labels JSONB DEFAULT '{}'::jsonb,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                created_by VARCHAR(255) NOT NULL DEFAULT 'system'
            );
            
            CREATE TABLE IF NOT EXISTS alert_events (
                event_id VARCHAR(255) PRIMARY KEY,
                rule_id VARCHAR(255) NOT NULL REFERENCES alert_rules(rule_id),
                node_id VARCHAR(255) NOT NULL,
                gpu_uuid VARCHAR(255) NOT NULL DEFAULT '',
                old_state VARCHAR(20),
                new_state VARCHAR(20) NOT NULL,
                current_value DOUBLE PRECISION,
                threshold DOUBLE PRECISION NOT NULL,
                notification_sent VARCHAR(255),
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_alert_events_rule ON alert_events(rule_id);
            CREATE INDEX IF NOT EXISTS idx_alert_events_node ON alert_events(node_id);
            CREATE INDEX IF NOT EXISTS idx_alert_events_timestamp ON alert_events(timestamp);
            
            CREATE TABLE IF NOT EXISTS users (
                user_id VARCHAR(255) PRIMARY KEY,
                username VARCHAR(100) UNIQUE NOT NULL,
                email VARCHAR(255),
                role VARCHAR(20) NOT NULL DEFAULT 'viewer',
                password_hash TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                last_login_at TIMESTAMP WITH TIME ZONE,
                failed_login_attempts INTEGER NOT NULL DEFAULT 0,
                locked_until TIMESTAMP WITH TIME ZONE
            );
            
            CREATE TABLE IF NOT EXISTS refresh_tokens (
                token VARCHAR(255) PRIMARY KEY,
                user_id VARCHAR(255) NOT NULL REFERENCES users(user_id),
                expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                revoked BOOLEAN NOT NULL DEFAULT FALSE
            );
            
            CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens(user_id);
            
            CREATE TABLE IF NOT EXISTS audit_logs (
                log_id VARCHAR(255) PRIMARY KEY,
                username VARCHAR(255) NOT NULL,
                action VARCHAR(255) NOT NULL,
                target VARCHAR(255),
                target_type VARCHAR(50),
                details TEXT,
                result VARCHAR(50) NOT NULL,
                source_ip VARCHAR(45),
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_audit_logs_user ON audit_logs(username);
            CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
            CREATE INDEX IF NOT EXISTS idx_audit_logs_timestamp ON audit_logs(timestamp);
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
                // Unique constraint violation, table likely exists
                sqlx::migrate::MigrateError::VersionMissing(0)
            }
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("42P07") => {
                // Relation already exists
                sqlx::migrate::MigrateError::VersionMissing(0)
            }
            e => e.into(),
        })?;
        
        // Create initial admin user if not exists
        use argon2::password_hash::{PasswordHasher, SaltString};
        use argon2::password_hash::rand_core::OsRng;
        
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT username FROM users WHERE username = 'admin' LIMIT 1"
        )
        .fetch_one(pool)
        .await;
        
        if existing.is_err() {
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = argon2::Argon2::default();
            let password_hash = argon2
                .hash_password(admin_password.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!("Failed to hash admin password: {}", e))?
                .to_string();
            
            sqlx::query(
                "INSERT INTO users (user_id, username, role, password_hash, enabled) VALUES ($1, $2, $3, $4, TRUE)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind("admin")
            .bind("admin")
            .bind(password_hash)
            .execute(pool)
            .await?;
        }
        
        Ok(())
    }
    
    pub fn pool(&self) -> &sqlx::Pool<sqlx::Postgres> {
        &self.pool
    }
}
