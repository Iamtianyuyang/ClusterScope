use clap::Parser;
use std::path::PathBuf;
use tokio::signal;
use tracing::{error, info, warn};

mod config_loader;
mod grpc_client;
mod job_executor;
mod metrics;
mod node_identity;

#[derive(Parser)]
#[command(name = "clusterscope-agent", about = "ClusterScope GPU Node Agent")]
struct Cli {
    #[arg(short, long, default_value = "/etc/clusterscope/agent.yaml")]
    config: PathBuf,

    #[arg(long)]
    config_dir: Option<PathBuf>,

    #[arg(long)]
    server_addr: Option<String>,

    #[arg(long)]
    node_id: Option<String>,

    #[arg(long)]
    agent_token: Option<String>,
}

/// Connect to the server with exponential backoff (never gives up).
async fn connect_with_retry(
    server_addr: String,
    node_id: String,
    config: common::config::AgentConfig,
) -> anyhow::Result<grpc_client::AgentClient> {
    let mut delay_secs: u64 = 1;
    loop {
        match grpc_client::AgentClient::new(server_addr.clone(), node_id.clone(), config.clone())
            .await
        {
            Ok(client) => return Ok(client),
            Err(e) => {
                warn!(error = %e, delay_secs, "Failed to connect to server, retrying");
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                delay_secs = (delay_secs * 2).min(60);
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = config_loader::load_config(&cli)?;

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!(
        server_addr = %config.server_addr,
        node_id = ?config.node_id,
        "ClusterScope Agent starting"
    );

    // Get or generate node identity
    let node_identity = node_identity::NodeIdentity::load_or_create(&config.node_id_file)?;
    // node_id resolution order:
    //   1. explicit config value
    //   2. local hostname (works on shared-HOME clusters where one config
    //      file is visible to many machines)
    //   3. persisted node identity file
    let node_id = config
        .node_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            sysinfo::System::host_name()
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| node_identity.id.clone())
        });

    info!(node_id = %node_id, "Node identity resolved");

    // Create metrics collector
    let collector = metrics::MetricsCollector::new();

    // Create gRPC client (shared across tasks via mutex), retrying until the
    // server is reachable so a temporary outage doesn't kill the agent.
    let client = std::sync::Arc::new(tokio::sync::Mutex::new(
        connect_with_retry(config.server_addr.clone(), node_id.clone(), config.clone()).await?,
    ));

    // Register node identity with the server (persists node_info row)
    {
        let mut guard = client.lock().await;
        let mut attempt: u32 = 0;
        loop {
            match guard.register().await {
                Ok(()) => break,
                Err(e) => {
                    attempt += 1;
                    if attempt >= 10 {
                        return Err(anyhow::anyhow!(
                            "Failed to register with server after retries: {}",
                            e
                        ));
                    }
                    warn!(error = %e, attempt, "Register failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    // Start reporting loop
    let report_interval = std::time::Duration::from_secs(config.report_interval_secs);
    let collector_handle = {
        let client = client.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(report_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut collector = collector;

            loop {
                interval.tick().await;

                // Collect metrics
                let report = match collector.collect(&config) {
                    Ok(report) => report,
                    Err(e) => {
                        warn!(error = %e, "Failed to collect metrics, will retry next cycle");
                        continue;
                    }
                };

                // Report to server
                if let Err(e) = client.lock().await.report_metrics(report).await {
                    warn!(error = %e, "Failed to report metrics");
                }
            }
        })
    };

    // Start heartbeat loop
    let heartbeat_handle = {
        let client = client.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut beats: u64 = 0;

            loop {
                interval.tick().await;
                beats += 1;

                if let Err(e) = client.lock().await.send_heartbeat().await {
                    warn!(error = %e, "Failed to send heartbeat");
                }

                // Re-register every ~60s so node info survives server restarts
                // (the server keeps node state in memory + upserts on register).
                if beats % 12 == 0 {
                    if let Err(e) = client.lock().await.register().await {
                        warn!(error = %e, "Failed to re-register with server");
                    }
                }
            }
        })
    };

    // Watch for job changes
    let job_watcher_handle = {
        // Standalone client so the long-lived job stream does not block
        // metrics/heartbeat tasks on the shared mutex.
        let standalone = client.lock().await.clone();
        tokio::spawn(async move {
            let mut standalone = standalone;
            standalone.watch_jobs().await;
        })
    };

    // Wait for signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal");
        }
        Err(err) => {
            error!(error = %err, "Unable to listen for shutdown signal");
        }
    }

    // Cancel tasks
    collector_handle.abort();
    heartbeat_handle.abort();
    job_watcher_handle.abort();

    info!("Agent stopped");
    Ok(())
}
