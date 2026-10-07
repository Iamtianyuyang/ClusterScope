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

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tonic::transport::Server;
use tracing::{info, warn};

mod auth_middleware;
mod grpc;
mod handlers;
mod ws_handler;

use anyhow::Context;

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
    /// Per-source-IP login attempt log (sliding window) for rate limiting.
    login_attempts: std::sync::Arc<std::sync::Mutex<HashMap<String, VecDeque<Instant>>>>,
    /// Global login attempt log (all IPs, successes AND failures) for the
    /// global sliding-window budget — bounds total Argon2 CPU when many
    /// distinct IPs attack at once.
    global_login_attempts: std::sync::Arc<std::sync::Mutex<VecDeque<Instant>>>,
    /// Caps concurrent Argon2 verifications/hashes so a synchronized login
    /// burst cannot spike CPU across all cores.
    login_concurrency: std::sync::Arc<tokio::sync::Semaphore>,
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
        login_attempts: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        global_login_attempts: std::sync::Arc::new(std::sync::Mutex::new(VecDeque::new())),
        login_concurrency: std::sync::Arc::new(tokio::sync::Semaphore::new(
            handlers::LOGIN_MAX_CONCURRENCY,
        )),
    });

    // Background tasks
    let state_bg = state.clone();
    tokio::spawn(async move { run_background_tasks(state_bg).await });

    // gRPC server (shared-token auth - see grpc_auth_interceptor). When
    // tls_enabled the listener speaks TLS so the shared token, job
    // environments and logs are not sent in clear text.
    let grpc_addr = config.grpc_addr.parse::<SocketAddr>()?;
    let agent_service = grpc::AgentServiceImpl::new(state.clone());
    let grpc_token = state.config.agent_token.clone();
    let grpc_identity = load_tls_identity(&config)?;
    let mut grpc_handle = tokio::spawn(async move {
        serve_grpc(grpc_addr, grpc_identity, agent_service, grpc_token).await
    });

    info!(addr = %config.grpc_addr, tls = config.tls_enabled, "gRPC server started");
    if config.agent_token.is_empty() {
        warn!(
            "agent_token is empty — gRPC accepts any caller; set AGENT_TOKEN for untrusted networks"
        );
    }

    // HTTP server (REST + WebSocket, both on http_addr)
    let http_state = state.clone();
    let http_router = build_http_router(http_state);

    let http_addr = config.http_addr.parse::<SocketAddr>()?;
    // Load the PEM pair up front (outside the async task) so a missing/bad
    // file fails startup instead of silently serving plaintext.
    let (http_cert_pem, http_key_pem) = if config.tls_enabled {
        let cert = std::fs::read(
            config
                .tls_cert_path
                .as_ref()
                .context("tls_cert_path required when tls_enabled")?,
        )
        .context("failed to read tls_cert_path")?;
        let key = std::fs::read(
            config
                .tls_key_path
                .as_ref()
                .context("tls_key_path required when tls_enabled")?,
        )
        .context("failed to read tls_key_path")?;
        (Some(cert), Some(key))
    } else {
        (None, None)
    };

    // B's TLS listener combined with C's error propagation: the bind/serve
    // error must reach the select! below (a detached task used to swallow it
    // and the process kept running with half the services dead).
    let mut http_handle = tokio::spawn(async move {
        if let (Some(cert), Some(key)) = (http_cert_pem, http_key_pem) {
            serve_http_tls(http_router, http_addr, cert, key).await
        } else {
            axum::serve(
                TcpListener::bind(http_addr).await?,
                http_router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await?;
            Ok(())
        }
    });

    info!(addr = %config.http_addr, tls = config.tls_enabled, "HTTP server started");

    // Optional standalone Prometheus listener (no auth - scrapers cannot
    // carry JWTs; bind it to a trusted network via prometheus_addr).
    let prom_handle = if config.prometheus_enabled {
        let prom_state = state.clone();
        let prom_addr = config.prometheus_addr.parse::<SocketAddr>()?;
        info!(addr = %config.prometheus_addr, "Prometheus metrics listener started");
        Some(tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/metrics",
                axum::routing::get(move || {
                    let state = prom_state.clone();
                    async move { handlers::render_prometheus_metrics(&state) }
                }),
            );
            match TcpListener::bind(prom_addr).await {
                Ok(listener) => {
                    if let Err(e) = axum::serve(listener, app).await {
                        warn!(error = %e, "Prometheus listener stopped with error");
                    }
                }
                Err(e) => warn!(error = %e, "Prometheus bind failed"),
            }
        }))
    } else {
        None
    };

    // Wait for Ctrl-C or a server failure. A bind/serve error inside a
    // detached task used to be silently swallowed - the process would keep
    // running with half the services dead and nobody noticing.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
            grpc_handle.abort();
            http_handle.abort();
            if let Some(h) = &prom_handle {
                h.abort();
            }
        }
        result = &mut grpc_handle => {
            http_handle.abort();
            if let Some(h) = &prom_handle {
                h.abort();
            }
            match result {
                Ok(Ok(())) => warn!("gRPC server stopped unexpectedly"),
                Ok(Err(e)) => anyhow::bail!("gRPC server failed: {e}"),
                Err(e) => anyhow::bail!("gRPC server task panicked: {e}"),
            }
        }
        result = &mut http_handle => {
            grpc_handle.abort();
            if let Some(h) = &prom_handle {
                h.abort();
            }
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
    if let Ok(v) = std::env::var("CLUSTERSCOPE_TRUST_PROXY_HEADERS")
        .or_else(|_| std::env::var("TRUST_PROXY_HEADERS"))
    {
        config.trust_proxy_headers = v.eq_ignore_ascii_case("true") || v == "1";
    }
    if let Ok(v) = std::env::var("CLUSTERSCOPE_PROMETHEUS_ENABLED")
        .or_else(|_| std::env::var("PROMETHEUS_ENABLED"))
    {
        config.prometheus_enabled = v.eq_ignore_ascii_case("true") || v == "1";
    }
    if let Ok(v) = std::env::var("CLUSTERSCOPE_DEFAULT_ADMIN_PASSWORD")
        .or_else(|_| std::env::var("DEFAULT_ADMIN_PASSWORD"))
    {
        config.default_admin_password = v;
    }
    config.agent_token = env("AGENT_TOKEN", config.agent_token);
    // Alias kept from the B round (its config key was called grpc_token).
    if let Ok(v) =
        std::env::var("CLUSTERSCOPE_AGENT_TOKEN").or_else(|_| std::env::var("GRPC_TOKEN"))
    {
        config.agent_token = v;
    }

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

/// Reject every gRPC call unless it carries `Authorization: Bearer <token>`.
/// Applies to unary and streaming RPCs alike (tonic interceptors wrap both).
/// The interceptor's signature is fixed by tonic (`Result<Request<()>, Status>`)
/// and `Status` is a large type, so the clippy lint is not actionable here.
#[allow(clippy::result_large_err)]
fn grpc_auth_interceptor(
    token: String,
) -> impl Fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> + Clone + Send + 'static
{
    use subtle::ConstantTimeEq;

    move |req: tonic::Request<()>| {
        if token.is_empty() {
            // No token configured: accept any caller (trusted network only),
            // matching the documented `agent_token` semantics of this tree.
            return Ok(req);
        }
        let expected = format!("Bearer {}", token);
        match req.metadata().get("authorization") {
            // Constant-time comparison: a timing side channel on the shared
            // token would let a LAN attacker refine guesses byte by byte.
            // (Length is not secret, and ct_eq handles the mismatch case.)
            Some(v) => {
                // Constant-time comparison via subtle (slice ct_eq returns
                // Choice; convert once here).
                let ok = v
                    .to_str()
                    .map(|s| bool::from(s.as_bytes().ct_eq(expected.as_bytes())))
                    .unwrap_or(false);
                if ok {
                    Ok(req)
                } else {
                    Err(tonic::Status::unauthenticated(
                        "missing or invalid gRPC token",
                    ))
                }
            }
            None => Err(tonic::Status::unauthenticated(
                "missing or invalid gRPC token",
            )),
        }
    }
}

/// Load the PEM certificate+key pair into a tonic `Identity` when TLS is
/// enabled. Missing/invalid files fail startup loudly rather than silently
/// falling back to plaintext.
fn load_tls_identity(config: &ServerConfig) -> anyhow::Result<Option<tonic::transport::Identity>> {
    if !config.tls_enabled {
        return Ok(None);
    }
    let cert = std::fs::read(
        config
            .tls_cert_path
            .as_ref()
            .context("tls_cert_path required when tls_enabled")?,
    )
    .context("failed to read tls_cert_path")?;
    let key = std::fs::read(
        config
            .tls_key_path
            .as_ref()
            .context("tls_key_path required when tls_enabled")?,
    )
    .context("failed to read tls_key_path")?;
    Ok(Some(tonic::transport::Identity::from_pem(cert, key)))
}

/// Run the tonic gRPC server, with TLS when an identity is provided.
/// Serves until the server errors (it never returns Ok on its own).
async fn serve_grpc(
    addr: SocketAddr,
    identity: Option<tonic::transport::Identity>,
    service: grpc::AgentServiceImpl,
    token: String,
) -> Result<(), tonic::transport::Error> {
    let mut builder = Server::builder();
    if let Some(identity) = identity {
        builder =
            builder.tls_config(tonic::transport::ServerTlsConfig::new().identity(identity))?;
    }
    builder
        .add_service(AgentServiceServer::with_interceptor(
            service,
            grpc_auth_interceptor(token),
        ))
        .serve(addr)
        .await
}

/// Parse a PEM cert+key pair into a rustls server config (http/1.1 ALPN).
fn build_rustls_server_config(
    cert_pem: &[u8],
    key_pem: &[u8],
) -> anyhow::Result<rustls::ServerConfig> {
    use rustls_pki_types::CertificateDer;

    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::BufReader::new(cert_pem))
            .collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(key_pem))?
        .ok_or_else(|| anyhow::anyhow!("no private key found in tls_key_path"))?;

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("failed to build TLS server config")?;
    // The web frontend runs over http/1.1 (WebSocket upgrade); keep h2 for
    // clients that negotiate it.
    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(tls_config)
}

/// Serve the axum router over TLS (tokio-rustls + hyper http/1.1).
/// Accepts connections forever; returns on bind/accept errors.
async fn serve_http_tls(
    router: axum::Router,
    addr: SocketAddr,
    cert_pem: Vec<u8>,
    key_pem: Vec<u8>,
) -> anyhow::Result<()> {
    use hyper_util::rt::TokioIo;
    use tokio_rustls::TlsAcceptor;

    let tls_config = build_rustls_server_config(&cert_pem, &key_pem)?;
    let acceptor = TlsAcceptor::from(std::sync::Arc::new(tls_config));

    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let router = router.clone();
        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    let io = TokioIo::new(tls_stream);
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, hyper_util::service::TowerToHyperService::new(router))
                        .await;
                }
                Err(e) => warn!(error = %e, "TLS handshake failed"),
            }
        });
    }
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
        .route("/audit-logs", get(handlers::list_audit_logs))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::require_admin_middleware,
        ));

    // Operator+: job submission and cancellation.
    let operator_routes = Router::new()
        .route("/jobs", post(handlers::create_job))
        .route("/jobs/{job_id}", delete(handlers::stop_job))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::require_operator_middleware,
        ));

    // Any authenticated user (no role gate): self-service password change.
    let self_routes = Router::new().route("/me/password", post(handlers::change_my_password));

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

    let authed_routes = admin_routes
        .merge(operator_routes)
        .merge(self_routes)
        .merge(read_routes);

    let authed_routes = if state.config.auth_required {
        authed_routes.route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::auth_middleware,
        ))
    } else {
        // Read-only mode: allow GET without a token, still require auth for writes.
        authed_routes.route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
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

        // Drop rate-limit entries whose newest attempt fell out of the
        // window, so the login limiter map cannot grow unboundedly with
        // distinct source IPs (F-08).
        handlers::prune_login_attempts(&state);

        if let Err(e) = storage::queries::prune_old_metrics(state.database.pool()).await {
            warn!(error = %e, "Failed to prune old metrics");
        }

        run_scheduler_cycle(&state).await;

        refresh_alert_rules(&state).await;

        // Self-heal stale alert instances (server restarts lose engine state).
        // Also reset the in-memory engine for the expired keys so the DB
        // trail and the engine cannot diverge (a node coming back above
        // threshold must re-fire, not stay silently "resolved").
        if cycle.is_multiple_of(12) {
            // every 2 minutes
            match storage::alert_queries::expire_stale_alerts(
                state.database.pool(),
                Utc::now() - ChronoDuration::minutes(10),
            )
            .await
            {
                Ok(expired) => {
                    for (rule_id, node_id, gpu_uuid) in expired {
                        state
                            .alert_engine
                            .reset_state(&common::alert::AlertKey::new(rule_id, node_id, gpu_uuid));
                    }
                }
                Err(e) => warn!(error = %e, "Failed to expire stale alerts"),
            }
        }

        // Reap jobs stuck in `stopping` (agent died / process ignores
        // SIGTERM+SIGKILL): mark them `lost` and free scheduler capacity.
        match storage::job_queries::reset_stale_stopping_jobs(
            state.database.pool(),
            Utc::now() - ChronoDuration::minutes(10),
        )
        .await
        {
            Ok(lost_ids) => {
                for id in &lost_ids {
                    state
                        .scheduler
                        .complete_job(id, common::job::JobStatus::Lost)
                        .await;
                    state.ws_manager.push_job_update(id).await;
                }
                if !lost_ids.is_empty() {
                    info!(count = lost_ids.len(), "Marked stale stopping jobs as lost");
                }
            }
            Err(e) => warn!(error = %e, "Failed to reap stale stopping jobs"),
        }

        // Reap jobs stuck in `running` on nodes that have been offline past
        // the reap window (agent died / network gone): nothing else ever
        // re-reads a `running` row, so without this the job would hold its
        // GPU capacity forever. The window is far longer than the
        // node-offline threshold (60s), so a short partition does not kill
        // healthy jobs. The registry is memory-only and entries only appear
        // after a fresh heartbeat, so a server restart cannot spuriously
        // reap jobs of nodes that are about to reconnect.
        let dead_cutoff = Utc::now() - ChronoDuration::minutes(10);
        let dead_nodes: Vec<String> = state
            .node_registry
            .list()
            .into_iter()
            .filter(|n| n.last_seen < dead_cutoff)
            .map(|n| n.node_id)
            .collect();
        if !dead_nodes.is_empty() {
            match storage::job_queries::mark_running_jobs_lost(state.database.pool(), &dead_nodes)
                .await
            {
                Ok(lost_ids) => {
                    for id in &lost_ids {
                        state
                            .scheduler
                            .complete_job(id, common::job::JobStatus::Lost)
                            .await;
                        state.ws_manager.push_job_update(id).await;
                    }
                    if !lost_ids.is_empty() {
                        info!(
                            count = lost_ids.len(),
                            "Marked running jobs lost (node offline)"
                        );
                    }
                }
                Err(e) => warn!(error = %e, "Failed to reap running jobs on offline nodes"),
            }
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
            if let Err(e) = storage::queries::prune_old_job_logs(
                state.database.pool(),
                Utc::now() - ChronoDuration::days(30),
            )
            .await
            {
                warn!(error = %e, "Failed to prune old job logs");
            }
            if let Err(e) =
                storage::alert_queries::prune_old_alert_events(state.database.pool()).await
            {
                warn!(error = %e, "Failed to prune old alert events");
            }
            if let Err(e) =
                storage::audit_queries::prune_old_audit_logs(state.database.pool()).await
            {
                warn!(error = %e, "Failed to prune old audit logs");
            }
            if let Err(e) = storage::user_queries::prune_refresh_tokens(state.database.pool()).await
            {
                warn!(error = %e, "Failed to prune refresh tokens");
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

    // Re-queue jobs stuck in 'starting' (agent died / server restarted).
    // Drop the matching in-memory scheduler slots too, otherwise the stale
    // entry keeps counting against the node's GPU capacity.
    //
    // Only requeue when the node is NOT online: an online agent re-receives
    // `starting` jobs on its 5s poll, so requeueing a live node's job would
    // let a run whose terminal report was lost (gRPC blip) be executed a
    // second time — the agent-side dedup guard is released when the first
    // run finishes, so nothing would stop the double run.
    let online_node_ids: Vec<String> = state
        .node_registry
        .list_online()
        .into_iter()
        .map(|n| n.node_id)
        .collect();
    match storage::job_queries::reset_stale_starting_jobs(
        state.database.pool(),
        Utc::now() - ChronoDuration::minutes(10),
        &online_node_ids,
    )
    .await
    {
        Ok(reset_ids) => {
            for id in &reset_ids {
                // The job went back to `queued`: drop the in-memory slots it
                // held, otherwise the stale entries keep counting against the
                // node's GPU capacity until the next cycle re-enqueues it.
                state.scheduler.remove_running(id).await;
                state.scheduler.drop_job(id).await;
            }
            if !reset_ids.is_empty() {
                info!(count = reset_ids.len(), "Requeued stale starting jobs");
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to reset stale starting jobs");
        }
    }

    // Recycle jobs whose node died: running/stopping jobs on a node that has
    // been offline past the dead-node cutoff are marked 'lost' so their
    // scheduler capacity is freed and the state surfaces in the UI. Only
    // truly-dead nodes qualify (offline > 10 min) — a brief partition must
    // not orphan a live process that would later report a rejected status.
    let dead_cutoff = Utc::now() - ChronoDuration::minutes(10);
    let dead_nodes: Vec<String> = state
        .node_registry
        .list()
        .into_iter()
        .filter(|n| {
            n.status == common::node_registry::NodeStatus::Offline && n.last_seen < dead_cutoff
        })
        .map(|n| n.node_id)
        .collect();
    if !dead_nodes.is_empty()
        && let Ok(lost_ids) = storage::job_queries::mark_lost_jobs_on_nodes(
            state.database.pool(),
            &dead_nodes,
            "node offline — job presumed lost",
        )
        .await
    {
        for job_id in lost_ids {
            state.scheduler.complete_job(&job_id, JobStatus::Lost).await;
            state.ws_manager.push_job_update(&job_id).await;
            info!(job_id = %job_id, "Job marked lost (node offline)");
        }
    }

    // Load queued jobs (oldest first) and hand them to the capacity-aware
    // scheduler. Newest-first with a page cap would starve the oldest jobs
    // whenever more than one page is queued. All queued jobs are loaded —
    // the in-memory queue dedups on job_id, and capping the scan would
    // head-of-line-block newer jobs behind the oldest unschedulable ones.
    if let Ok(rows) =
        storage::job_queries::list_queued_jobs_for_scheduling(state.database.pool()).await
    {
        for row in rows {
            if let Some(job) = job_row_to_job(&row) {
                state.scheduler.enqueue(job).await;
            }
        }
    }

    let scheduled = state.scheduler.schedule().await;
    for job in scheduled {
        match storage::job_queries::assign_job_to_node(
            state.database.pool(),
            &job.job_id,
            &job.node_id,
        )
        .await
        {
            Ok(true) => {
                info!(job_id = %job.job_id, node_id = %job.node_id, "Job dispatched");
                state.ws_manager.push_job_update(&job.job_id).await;
            }
            Ok(false) => {
                // The job left `queued` between the in-memory schedule pass
                // and this write (typically cancelled by an operator). Drop
                // the in-memory slot so GPU capacity is not leaked and the
                // job is not dispatched again.
                state.scheduler.drop_job(&job.job_id).await;
                warn!(
                    job_id = %job.job_id,
                    "Dispatch lost a race (job cancelled?); dropping in-memory slot"
                );
            }
            Err(e) => {
                // Persisting the dispatch failed (transient DB error). The DB
                // row is still `queued` and will be re-scanned next cycle, but
                // the in-memory `running_jobs` slot would make `enqueue`
                // reject that copy — the job would wedge forever while
                // leaking GPU capacity. Drop the slot so the next cycle can
                // dispatch it again.
                state.scheduler.drop_job(&job.job_id).await;
                warn!(
                    error = %e,
                    job_id = %job.job_id,
                    "Failed to persist job dispatch; dropped in-memory slot, will retry next cycle"
                );
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with_auth(token: &str) -> tonic::Request<()> {
        let mut req = tonic::Request::new(());
        req.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        req
    }

    #[test]
    fn grpc_interceptor_accepts_matching_token() {
        let interceptor = grpc_auth_interceptor("s3cret-token-123".to_string());
        assert!(interceptor(req_with_auth("s3cret-token-123")).is_ok());
    }

    #[test]
    fn grpc_interceptor_rejects_missing_and_wrong_tokens() {
        let interceptor = grpc_auth_interceptor("s3cret-token-123".to_string());
        // Missing header.
        assert!(interceptor(tonic::Request::new(())).is_err());
        // Wrong token.
        assert!(interceptor(req_with_auth("wrong")).is_err());
        // Not a Bearer scheme.
        let mut req = tonic::Request::new(());
        req.metadata_mut()
            .insert("authorization", "Basic abc".parse().unwrap());
        assert!(interceptor(req).is_err());
    }

    /// Self-signed cert/key round-trip through the real parsing + rustls
    /// builder paths used by the TLS listeners (rcgen generates PEM exactly
    /// like `openssl req -x509` would).
    #[test]
    fn test_tls_identity_and_server_config() {
        // Same provider selection as main() (both rustls providers are
        // enabled in the workspace dependency tree).
        let _ = rustls::crypto::ring::default_provider().install_default();
        use rcgen::{CertifiedKey, generate_simple_self_signed};
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(vec!["clusterscope.test".to_string()]).unwrap();
        let cert_pem = cert.pem().into_bytes();
        let key_pem = key_pair.serialize_pem().into_bytes();

        // tonic identity (gRPC listener).
        let identity = tonic::transport::Identity::from_pem(cert_pem.clone(), key_pem.clone());
        let tls = tonic::transport::ServerTlsConfig::new().identity(identity);
        let _ = tls;

        // rustls server config (HTTP listener).
        let config = build_rustls_server_config(&cert_pem, &key_pem).unwrap();
        assert_eq!(config.alpn_protocols, vec![b"http/1.1".to_vec()]);

        // load_tls_identity with a real config file path.
        let dir =
            std::env::temp_dir().join(format!("clusterscope-tls-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        std::fs::write(&cert_path, &cert_pem).unwrap();
        std::fs::write(&key_path, &key_pem).unwrap();

        let mut cfg = ServerConfig {
            tls_enabled: false,
            ..Default::default()
        };
        assert!(load_tls_identity(&cfg).unwrap().is_none());

        cfg.tls_enabled = true;
        cfg.tls_cert_path = Some(cert_path.clone());
        cfg.tls_key_path = Some(key_path.clone());
        assert!(load_tls_identity(&cfg).unwrap().is_some());

        // Missing file fails loudly (no silent plaintext fallback).
        cfg.tls_cert_path = Some(dir.join("missing.pem"));
        assert!(load_tls_identity(&cfg).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Full-stack TLS check: serve the real TLS accept path on a loopback
    /// port and complete a TLS handshake + HTTP/1.1 request with a rustls
    /// client trusting the self-signed cert. Guards against regressions in
    /// cert parsing, ALPN, the hyper bridge and the router behind TLS.
    #[tokio::test]
    async fn test_http_tls_end_to_end() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _ = rustls::crypto::ring::default_provider().install_default();
        use rcgen::{CertifiedKey, generate_simple_self_signed};
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(vec!["clusterscope.test".to_string()]).unwrap();
        let cert_pem = cert.pem().into_bytes();
        let key_pem = key_pair.serialize_pem().into_bytes();

        // Pick a free loopback port.
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let router =
            axum::Router::new().route("/api/health", axum::routing::get(|| async { "OK" }));
        let server = tokio::spawn(async move {
            let _ = serve_http_tls(router, addr, cert_pem, key_pem).await;
        });

        // Client trusts the self-signed cert.
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let client_cfg = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_cfg));

        // The server task binds asynchronously; retry until it accepts.
        let mut tls = None;
        for _ in 0..50 {
            if let Ok(stream) = tokio::net::TcpStream::connect(addr).await
                && let Ok(conn) = connector
                    .connect(
                        rustls_pki_types::ServerName::try_from("clusterscope.test").unwrap(),
                        stream,
                    )
                    .await
            {
                tls = Some(conn);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let mut tls = tls.expect("TLS handshake must succeed");
        tls.write_all(
            b"GET /api/health HTTP/1.1\r\nHost: clusterscope.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        tls.read_to_end(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf);
        assert!(
            resp.starts_with("HTTP/1.1 200"),
            "unexpected response: {}",
            resp
        );
        assert!(resp.contains("OK"));

        server.abort();
    }
}
