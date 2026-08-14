use axum::{
    Router,
    routing::{delete, get, post},
};
use chrono::{Duration as ChronoDuration, Utc};
use common::alert::AlertRule;
use common::config::ServerConfig;
use common::job::{Job, JobStatus, status_from_str};
use protocol::AgentServiceServer;
use scheduler::Scheduler;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tonic::transport::Server;
use tracing::{info, warn};

mod auth_middleware;
mod grpc;
mod handlers;
mod ws_handler;

struct AppState {
    config: ServerConfig,
    database: storage::DatabasePool,
    ws_manager: ws_handler::WsManager,
    alert_engine: common::alert::AlertEngine,
    /// Cached enabled alert rules (refreshed periodically from the DB).
    alert_rules: parking_lot::RwLock<Vec<AlertRule>>,
    node_registry: common::node_registry::RegistryManager,
    scheduler: Arc<Scheduler>,
    jwt_secret: String,
    seen_reports: std::sync::Arc<parking_lot::RwLock<lru::LruCache<String, ()>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config()?;

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("ClusterScope Server starting");

    let database = storage::DatabasePool::new(&config).await?;
    info!("Connected to database");

    let node_registry = common::node_registry::RegistryManager::with_thresholds(
        common::node_registry::NodeThresholds {
            online_secs: config.node_online_threshold_secs,
            degraded_secs: config.node_degraded_threshold_secs,
            offline_secs: config.node_offline_threshold_secs,
        },
    );

    let ws_manager = ws_handler::WsManager::new();
    let alert_engine = common::alert::AlertEngine::new();

    // Rebuild the scheduler's in-memory running set from the DB so GPU
    // capacity accounting survives a server restart.
    let scheduler = Arc::new(Scheduler::new());
    if let Ok(rows) = storage::job_queries::list_active_jobs(database.pool()).await {
        let jobs: Vec<common::job::Job> = rows.iter().filter_map(job_row_to_job).collect();
        scheduler.restore_running(jobs).await;
    }

    let state = Arc::new(AppState {
        config: config.clone(),
        database,
        ws_manager,
        alert_engine,
        alert_rules: parking_lot::RwLock::new(Vec::new()),
        node_registry,
        scheduler,
        jwt_secret: config.jwt_secret.clone(),
        seen_reports: std::sync::Arc::new(parking_lot::RwLock::new(lru::LruCache::new(
            std::num::NonZeroUsize::new(100000).unwrap(),
        ))),
    });

    // Background tasks
    let state_bg = state.clone();
    tokio::spawn(async move { run_background_tasks(state_bg).await });

    // gRPC server
    let grpc_addr = config.grpc_addr.parse::<SocketAddr>()?;
    let agent_service = grpc::AgentServiceImpl::new(state.clone());
    let agent_token = config.agent_token.clone();
    let interceptor = move |req: tonic::Request<()>| {
        if agent_token.is_empty() {
            // No token configured: accept any caller (trusted network only).
            return Ok(req);
        }
        let authorized = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t == agent_token)
            .unwrap_or(false);
        if authorized {
            Ok(req)
        } else {
            Err(tonic::Status::unauthenticated("invalid agent token"))
        }
    };

    let mut grpc_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(AgentServiceServer::with_interceptor(
                agent_service,
                interceptor,
            ))
            .serve(grpc_addr)
            .await
    });

    info!(addr = %config.grpc_addr, "gRPC server started");
    if config.agent_token.is_empty() {
        warn!(
            "agent_token is empty — gRPC accepts any caller; set AGENT_TOKEN for untrusted networks"
        );
    }

    // HTTP server (REST + WebSocket, both on http_addr)
    let http_state = state.clone();
    let http_router = build_http_router(http_state);

    let http_addr = config.http_addr.parse::<SocketAddr>()?;
    let mut http_handle =
        tokio::spawn(
            async move { axum::serve(TcpListener::bind(http_addr).await?, http_router).await },
        );

    info!(addr = %config.http_addr, "HTTP server started");

    // Wait for Ctrl-C or a server failure. A bind/serve error inside a
    // detached task used to be silently swallowed — the process would keep
    // running with half the services dead and nobody noticing.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
            grpc_handle.abort();
            http_handle.abort();
        }
        result = &mut grpc_handle => {
            http_handle.abort();
            match result {
                Ok(Ok(())) => warn!("gRPC server stopped unexpectedly"),
                Ok(Err(e)) => anyhow::bail!("gRPC server failed: {e}"),
                Err(e) => anyhow::bail!("gRPC server task panicked: {e}"),
            }
        }
        result = &mut http_handle => {
            grpc_handle.abort();
            match result {
                Ok(Ok(())) => warn!("HTTP server stopped unexpectedly"),
                Ok(Err(e)) => anyhow::bail!("HTTP server failed: {e}"),
                Err(e) => anyhow::bail!("HTTP server task panicked: {e}"),
            }
        }
    }

    Ok(())
}

/// Load configuration from a YAML file (argv[1]) plus environment overrides.
///
/// Env vars (also without the `CLUSTERSCOPE_` prefix, for docker-compose):
///   CLUSTERSCOPE_POSTGRES_URL / POSTGRES_URL
///   CLUSTERSCOPE_JWT_SECRET / JWT_SECRET
///   CLUSTERSCOPE_HTTP_ADDR / HTTP_ADDR
///   CLUSTERSCOPE_GRPC_ADDR / GRPC_ADDR
///   CLUSTERSCOPE_AUTH_REQUIRED / AUTH_REQUIRED
///   CLUSTERSCOPE_AGENT_TOKEN / AGENT_TOKEN
fn load_config() -> anyhow::Result<ServerConfig> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("/etc/clusterscope/server.yaml");

    let mut config = if std::path::Path::new(config_path).exists() {
        let content = std::fs::read_to_string(config_path)
            .map_err(|e| anyhow::anyhow!("Failed to read config {}: {}", config_path, e))?;
        serde_yaml::from_str::<ServerConfig>(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config {}: {}", config_path, e))?
    } else if args.len() > 1 {
        anyhow::bail!("Config file not found: {}", config_path);
    } else {
        info!("No config file given — using defaults + environment overrides");
        ServerConfig::default()
    };

    let env = |name: &str, current: String| -> String {
        std::env::var(format!("CLUSTERSCOPE_{}", name))
            .or_else(|_| std::env::var(name))
            .unwrap_or(current)
    };
    config.postgres_url = env("POSTGRES_URL", config.postgres_url);
    config.jwt_secret = env("JWT_SECRET", config.jwt_secret);
    config.http_addr = env("HTTP_ADDR", config.http_addr);
    config.grpc_addr = env("GRPC_ADDR", config.grpc_addr);
    if let Ok(v) =
        std::env::var("CLUSTERSCOPE_AUTH_REQUIRED").or_else(|_| std::env::var("AUTH_REQUIRED"))
    {
        config.auth_required = v.eq_ignore_ascii_case("true") || v == "1";
    }
    if let Ok(v) = std::env::var("CLUSTERSCOPE_DEFAULT_ADMIN_PASSWORD")
        .or_else(|_| std::env::var("DEFAULT_ADMIN_PASSWORD"))
    {
        config.default_admin_password = v;
    }
    config.agent_token = env("AGENT_TOKEN", config.agent_token);

    // Refuse obviously insecure configurations when auth is enforced.
    if config.auth_required
        && (config.jwt_secret == "default-secret-change-me" || config.jwt_secret.len() < 16)
    {
        anyhow::bail!(
            "refusing to start: jwt_secret is missing/too weak with auth_required: true. \
             Set a strong jwt_secret in server.yaml (or JWT_SECRET env), or set auth_required: false for trusted LANs."
        );
    }

    Ok(config)
}

fn build_http_router(state: Arc<AppState>) -> Router {
    let public_routes = Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/login", post(handlers::login))
        .route("/api/refresh-token", post(handlers::refresh_token))
        .route("/ws", get(ws_handler::ws_upgrade));

    // Admin-only: user management + alert rule management (writes).
    let admin_routes = Router::new()
        .route(
            "/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route(
            "/users/{id}",
            get(handlers::get_user)
                .patch(handlers::update_user)
                .delete(handlers::delete_user),
        )
        .route("/alerts/rules", post(handlers::create_alert_rule))
        .route(
            "/alerts/rules/{rule_id}",
            delete(handlers::delete_alert_rule),
        )
        .route(
            "/alerts/rules/{rule_id}/ack",
            post(handlers::acknowledge_alert),
        )
        .route_layer(axum::middleware::from_fn(
            auth_middleware::require_admin_middleware,
        ));

    // Operator+: job submission and cancellation.
    let operator_routes = Router::new()
        .route("/jobs", post(handlers::create_job))
        .route("/jobs/{job_id}", delete(handlers::stop_job))
        .route_layer(axum::middleware::from_fn(
            auth_middleware::require_operator_middleware,
        ));

    // Read-only monitoring routes (any authenticated user).
    let read_routes = Router::new()
        .route("/nodes", get(handlers::list_nodes))
        .route("/nodes/{node_id}", get(handlers::get_node_status))
        .route("/nodes/{node_id}/metrics", get(handlers::get_node_metrics))
        .route("/metrics/history", get(handlers::get_metrics_history))
        .route("/jobs", get(handlers::list_jobs))
        .route("/jobs/{job_id}", get(handlers::get_job))
        .route("/jobs/{job_id}/logs", get(handlers::get_job_logs))
        .route("/alerts/rules", get(handlers::list_alert_rules))
        .route(
            "/alerts/rules/{rule_id}/state",
            get(handlers::get_alert_state),
        )
        .route("/alerts/events", get(handlers::list_alert_events))
        .route("/cluster/info", get(handlers::get_cluster_info))
        .route("/audit-logs", get(handlers::list_audit_logs))
        .route("/prometheus/metrics", get(handlers::get_prometheus_metrics));

    let authed_routes = admin_routes.merge(operator_routes).merge(read_routes);

    let authed_routes = if state.config.auth_required {
        authed_routes.route_layer(axum::middleware::from_fn_with_state(
            std::sync::Arc::new(state.jwt_secret.clone()),
            auth_middleware::auth_middleware,
        ))
    } else {
        // Read-only mode: allow GET without a token, still require auth for writes.
        authed_routes.route_layer(axum::middleware::from_fn_with_state(
            std::sync::Arc::new(state.jwt_secret.clone()),
            auth_middleware::readonly_middleware,
        ))
    };

    Router::new()
        .merge(public_routes)
        .nest("/api", authed_routes)
        .with_state(state)
}

async fn run_background_tasks(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    // Counters for slower-period tasks (10s tick).
    let mut cycle: u64 = 0;

    loop {
        interval.tick().await;
        cycle += 1;

        state.node_registry.check_node_status(Utc::now());

        if let Err(e) = storage::queries::prune_old_metrics(state.database.pool()).await {
            warn!(error = %e, "Failed to prune old metrics");
        }

        run_scheduler_cycle(&state).await;

        refresh_alert_rules(&state).await;

        // Self-heal stale alert instances (server restarts lose engine state).
        // every 2 minutes
        if cycle.is_multiple_of(12)
            && let Err(e) = storage::alert_queries::expire_stale_alerts(
                state.database.pool(),
                Utc::now() - ChronoDuration::minutes(10),
            )
            .await
        {
            warn!(error = %e, "Failed to expire stale alerts");
        }

        // Job log retention: keep the last 30 days of task output (every 10
        // minutes).
        if cycle.is_multiple_of(60)
            && let Err(e) = storage::queries::prune_old_job_logs(
                state.database.pool(),
                Utc::now() - ChronoDuration::days(30),
            )
            .await
        {
            warn!(error = %e, "Failed to prune old job logs");
        }

        // Hourly rollups every 10 minutes; daily every hour.
        if cycle.is_multiple_of(60) {
            if let Err(e) = storage::aggregation::aggregate_to_hourly(state.database.pool()).await {
                warn!(error = %e, "Failed to aggregate hourly metrics");
            }
            if let Err(e) =
                storage::aggregation::cleanup_hourly_data(state.database.pool(), 7).await
            {
                warn!(error = %e, "Failed to clean hourly metrics");
            }
        }
        if cycle.is_multiple_of(360) {
            if let Err(e) = storage::aggregation::aggregate_to_daily(state.database.pool()).await {
                warn!(error = %e, "Failed to aggregate daily metrics");
            }
            if let Err(e) =
                storage::aggregation::cleanup_daily_data(state.database.pool(), 90).await
            {
                warn!(error = %e, "Failed to clean daily metrics");
            }
        }
    }
}

/// Dispatch queued jobs to nodes with free GPU capacity and persist the
/// assignment (status -> 'starting') so the target agent picks it up.
async fn run_scheduler_cycle(state: &Arc<AppState>) {
    // Refresh per-node GPU capacity from the registry (learned from metrics).
    // Only online nodes are schedulable; offline/degraded nodes get zero
    // capacity so the scheduler never dispatches to a dead node.
    for node in state.node_registry.list() {
        let capacity = if node.status == common::node_registry::NodeStatus::Online {
            node.gpu_count
        } else {
            0
        };
        state
            .scheduler
            .set_node_gpu_capacity(&node.node_id, capacity)
            .await;
    }

    // Re-queue jobs stuck in 'starting' (agent died / server restarted) and
    // free the scheduler's in-memory capacity accounting for them.
    if let Ok(requeued) = storage::job_queries::reset_stale_starting_jobs(
        state.database.pool(),
        Utc::now() - ChronoDuration::minutes(10),
    )
    .await
    {
        for job_id in requeued {
            state.scheduler.remove_running(&job_id).await;
        }
    }

    // Load queued jobs (oldest first) and hand them to the capacity-aware
    // scheduler. The scheduler dedups by job_id, so reloading every cycle is
    // safe and never grows the queue while jobs wait for capacity.
    if let Ok(rows) = storage::job_queries::list_queued_jobs(state.database.pool(), 1000).await {
        for row in rows {
            if let Some(job) = job_row_to_job(&row) {
                state.scheduler.enqueue(job).await;
            }
        }
    }

    let scheduled = state.scheduler.schedule().await;
    for job in scheduled {
        info!(job_id = %job.job_id, node_id = %job.node_id, "Job dispatched");
        if let Err(e) = storage::job_queries::assign_job_to_node(
            state.database.pool(),
            &job.job_id,
            &job.node_id,
        )
        .await
        {
            warn!(error = %e, job_id = %job.job_id, "Failed to persist job dispatch");
        }
        state.ws_manager.push_job_update(&job.job_id).await;
    }
}

fn job_row_to_job(row: &storage::models::JobRow) -> Option<Job> {
    Some(Job {
        job_id: row.job_id.clone(),
        node_id: row.node_id.clone(),
        name: row.name.clone(),
        executable: row.executable.clone(),
        arguments: serde_json::from_value(row.arguments.clone()).unwrap_or_default(),
        working_directory: row.working_directory.clone(),
        environment: serde_json::from_value(row.environment.clone()).unwrap_or_default(),
        status: status_from_str(&row.status).unwrap_or(JobStatus::Queued),
        pid: row.pid.map(|p| p as u32),
        exit_code: row.exit_code,
        error_message: row.error_message.clone(),
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        created_by: row.created_by.clone(),
        resource_quota: row.resource_quota.clone().unwrap_or_default(),
        retry_count: row.retry_count as u32,
        max_retries: row.max_retries as u32,
    })
}

/// Load enabled alert rules from the DB into the shared cache.
async fn refresh_alert_rules(state: &Arc<AppState>) {
    let Ok(rows) = storage::alert_queries::list_alert_rules(state.database.pool()).await else {
        return;
    };
    let rules: Vec<AlertRule> = rows.iter().filter_map(grpc::alert_rule_from_row).collect();
    *state.alert_rules.write() = rules;
}
