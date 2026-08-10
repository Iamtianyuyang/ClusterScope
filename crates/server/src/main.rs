use axum::{
    routing::{delete, get, post},
    Router,
};
use chrono::Utc;
use common::config::ServerConfig;
use protocol::AgentServiceServer;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tracing::{info, warn};


mod auth_middleware;
mod ws_handler;
mod handlers;
mod grpc;


struct AppState {
    config: ServerConfig,
    database: storage::DatabasePool,
    ws_manager: ws_handler::WsManager,
    alert_engine: common::alert::AlertEngine,
    node_registry: common::node_registry::RegistryManager,
    jwt_secret: String,
    seen_reports: std::sync::Arc<parking_lot::RwLock<lru::LruCache<String, ()>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config();

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

    let state = Arc::new(AppState {
        config: config.clone(),
        database,
        ws_manager,
        alert_engine,
        node_registry,
        jwt_secret: config.jwt_secret.clone(),
        seen_reports: std::sync::Arc::new(parking_lot::RwLock::new(
            lru::LruCache::new(std::num::NonZeroUsize::new(100000).unwrap())
        )),
    });

    // Background tasks
    let state_bg = state.clone();
    tokio::spawn(async move { run_background_tasks(state_bg).await });

    // gRPC server
    let grpc_addr = config.grpc_addr.parse::<SocketAddr>()?;
    let agent_service = grpc::AgentServiceImpl::new(state.clone());

    let grpc_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(AgentServiceServer::new(agent_service))
            .serve(grpc_addr)
            .await
    });

    info!(addr = %config.grpc_addr, "gRPC server started");

    // HTTP server
    let http_state = state.clone();
    let http_router = build_http_router(http_state);

    let http_addr = config.http_addr.parse::<SocketAddr>()?;
    let http_handle = tokio::spawn(async move {
        axum::serve(
            TcpListener::bind(http_addr).await?,
            http_router,
        )
        .await
    });

    info!(addr = %config.http_addr, "HTTP server started");

    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    grpc_handle.abort();
    http_handle.abort();

    Ok(())
}

fn load_config() -> ServerConfig {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).map(|s| s.as_str()).unwrap_or("/etc/clusterscope/server.yaml");

    if std::path::Path::new(config_path).exists() {
        let content = std::fs::read_to_string(config_path).ok();
        if let Some(content) = content {
            if let Ok(config) = serde_yaml::from_str::<ServerConfig>(&content) {
                return config;
            }
        }
    }

    ServerConfig::default()
}

fn build_http_router(state: Arc<AppState>) -> Router {
    let public_routes = Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/login", post(handlers::login))
        .route("/api/refresh-token", post(handlers::refresh_token))
        .route("/ws", get(ws_handler::ws_upgrade));

    let authed_routes = Router::new()
        .route("/users", get(handlers::list_users).post(handlers::create_user))
        .route("/users/{id}", get(handlers::get_user).patch(handlers::update_user).delete(handlers::delete_user))
        .route("/nodes", get(handlers::list_nodes))
        .route("/nodes/{node_id}", get(handlers::get_node_status))
        .route("/nodes/{node_id}/metrics", get(handlers::get_node_metrics))
        .route("/metrics/history", get(handlers::get_metrics_history))
        .route("/jobs", get(handlers::list_jobs).post(handlers::create_job))
        .route("/jobs/{job_id}", get(handlers::get_job).delete(handlers::stop_job))
        .route("/jobs/{job_id}/logs", get(handlers::get_job_logs))
        .route("/alerts/rules", get(handlers::list_alert_rules).post(handlers::create_alert_rule))
        .route("/alerts/rules/{rule_id}", delete(handlers::delete_alert_rule))
        .route("/alerts/rules/{rule_id}/state", get(handlers::get_alert_state))
        .route("/alerts/events", get(handlers::list_alert_events))
        .route("/alerts/rules/{rule_id}/ack", post(handlers::acknowledge_alert))
        .route("/cluster/info", get(handlers::get_cluster_info))
        .route("/audit-logs", get(handlers::list_audit_logs))
        .route("/prometheus/metrics", get(handlers::get_prometheus_metrics));

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

    loop {
        interval.tick().await;
        state.node_registry.check_node_status(Utc::now());

        if let Err(e) = storage::queries::prune_old_metrics(state.database.pool()).await {
            warn!(error = %e, "Failed to prune old metrics");
        }
    }
}
