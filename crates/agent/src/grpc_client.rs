use anyhow::{Context, Result};
use common::config::AgentConfig;
use protocol::{
    AgentServiceClient, JobLogEntry, JobStatusUpdate, NodeHeartbeat, NodeInfo, NodeMetricsReport,
};
use tokio::time::Duration;
use tokio_stream::StreamExt;
use tracing::{info, warn};

/// Best-effort local IP detection (UDP connect trick; no packets sent).
fn local_ip() -> String {
    use std::net::UdpSocket;
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(sock) => match sock.connect("192.0.2.1:80") {
            Ok(()) => sock
                .local_addr()
                .map(|a| a.ip().to_string())
                .unwrap_or_default(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

#[derive(Clone)]
pub struct AgentClient {
    client: AgentServiceClient<tonic::transport::Channel>,
    node_id: String,
    config: AgentConfig,
    /// Authorization metadata attached to every request when a token is set.
    auth: Option<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>,
    /// Per-process monotonic report sequence. Seeded with the wall-clock
    /// millis so that after a restart the sequence jumps forward — the
    /// server-side dedup cache (node:seq) never mistakes new reports for
    /// old duplicates.
    sequence: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub job_runtime: std::sync::Arc<crate::job_executor::JobRuntime>,
}

impl AgentClient {
    pub async fn new(server_addr: String, node_id: String, config: AgentConfig) -> Result<Self> {
        let channel = tonic::transport::Channel::from_shared(server_addr.clone())
            .context("Invalid server address")?
            .connect_timeout(Duration::from_secs(10))
            .connect()
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to connect to server at {}: {}", server_addr, e)
            })?;

        let client = AgentServiceClient::new(channel);

        info!("Connected to server at {}", server_addr);

        let seed = chrono::Utc::now().timestamp_millis() as u64;
        let auth = if config.agent_token.is_empty() {
            None
        } else {
            let value = format!("Bearer {}", config.agent_token)
                .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                .map_err(|e| anyhow::anyhow!("invalid agent_token: {}", e))?;
            Some(value)
        };
        Ok(Self {
            client,
            node_id,
            config,
            auth,
            sequence: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(seed)),
            job_runtime: std::sync::Arc::new(crate::job_executor::JobRuntime::new()),
        })
    }

    /// Attach the agent token to a request (when configured).
    fn authed<T>(&self, mut request: tonic::Request<T>) -> tonic::Request<T> {
        if let Some(token) = &self.auth {
            request
                .metadata_mut()
                .insert("authorization", token.clone());
        }
        request
    }

    pub async fn register(&mut self) -> Result<()> {
        let node_info = self.build_node_info();
        let request = self.authed(tonic::Request::new(node_info));
        let _ = self.client.register(request).await?;
        info!(node_id = %self.node_id, "Registered with server");
        Ok(())
    }

    /// Collect local machine info (hostname, IP, CPU, memory, OS) for
    /// registration so the dashboard shows real device details.
    fn build_node_info(&self) -> NodeInfo {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        let os = sysinfo::System::name().unwrap_or_default();
        let os_version = sysinfo::System::os_version().unwrap_or_default();
        let os_info = if os_version.is_empty() {
            os
        } else {
            format!("{} {}", os, os_version)
        };

        let hostname = sysinfo::System::host_name().unwrap_or_default();
        let cpu_model = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default();
        let cpu_cores = sys.cpus().len() as u32;
        let memory_total_bytes = sys.total_memory();

        NodeInfo {
            node_id: self.node_id.clone(),
            hostname,
            ip_address: local_ip(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            os_info,
            kernel_version: sysinfo::System::kernel_version().unwrap_or_default(),
            cpu_model,
            cpu_cores,
            memory_total_bytes,
            labels: Default::default(),
            ..Default::default()
        }
    }

    pub async fn report_metrics(&mut self, mut report: NodeMetricsReport) -> Result<()> {
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        report.node_id = self.node_id.clone();
        report.sequence = seq;

        let request = self.authed(tonic::Request::new(tokio_stream::iter(vec![report])));

        self.client.report_metrics(request).await?;
        Ok(())
    }

    pub async fn send_heartbeat(&mut self) -> Result<()> {
        let heartbeat = NodeHeartbeat {
            node_id: self.node_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            status: 1, // Online
            ..Default::default()
        };

        let request = self.authed(tonic::Request::new(tokio_stream::iter(vec![heartbeat])));

        let _response = self.client.heartbeat(request).await?;
        Ok(())
    }

    /// Report a job status transition (running/succeeded/failed/…).
    pub async fn update_job_status(
        &mut self,
        job_id: &str,
        status: i32,
        message: &str,
    ) -> Result<()> {
        let update = protocol::JobStatusUpdate {
            job_id: job_id.to_string(),
            status,
            message: message.to_string(),
        };
        let request = self.authed(tonic::Request::new(update));
        let _ = self.client.update_job_status(request).await?;
        Ok(())
    }

    pub async fn report_job_logs(&mut self, _job_id: &str, logs: Vec<JobLogEntry>) -> Result<()> {
        let request = self.authed(tonic::Request::new(tokio_stream::iter(logs)));
        let _response = self.client.report_job_logs(request).await?;
        Ok(())
    }

    pub async fn watch_jobs(&mut self) {
        loop {
            match self.poll_pending_jobs().await {
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "Failed to poll pending jobs, retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn poll_pending_jobs(&mut self) -> Result<()> {
        let node_info = NodeInfo {
            node_id: self.node_id.clone(),
            ..Default::default()
        };

        let request = self.authed(tonic::Request::new(node_info));
        let mut stream = self.client.get_pending_jobs(request).await?.into_inner();

        while let Some(job_result) = stream.next().await {
            match job_result {
                Ok(job) => {
                    info!(job_id = %job.job_id, status = job.status, "Received job message");
                    // `stopping` = the server asked us to cancel a running job.
                    if job.status == protocol::JobStatus::Stopping as i32 {
                        info!(job_id = %job.job_id, "Cancel requested by server");
                        let runtime = self.job_runtime.clone();
                        let mut client = self.client.clone();
                        let job_id = job.job_id.clone();
                        // Handle cancellation concurrently: the poll loop must
                        // keep consuming the stream.
                        tokio::spawn(async move {
                            let running = runtime.request_cancel(&job_id).await;
                            if !running {
                                // Nothing to kill — never started or already gone.
                                let _ = client
                                    .update_job_status(JobStatusUpdate {
                                        job_id,
                                        status: protocol::JobStatus::Cancelled as i32,
                                        message: "cancelled before start".to_string(),
                                    })
                                    .await;
                            }
                        });
                    } else {
                        info!(job_id = %job.job_id, name = %job.name, "Received pending job");
                        // Skip when this job is already running locally OR a
                        // spawn is in flight: the server re-sends `starting`
                        // jobs every 5s until our `running` report lands, so
                        // without this guard a lost status report would spawn
                        // a second process.
                        if self.job_runtime.pids.lock().await.contains_key(&job.job_id) {
                            tracing::debug!(job_id = %job.job_id, "Job already running locally — skipping duplicate");
                            continue;
                        }
                        // Skip jobs the server already asked us to cancel
                        // before we ever saw them (placeholder never inserted).
                        if self.job_runtime.is_cancelled(&job.job_id).await {
                            tracing::debug!(job_id = %job.job_id, "Job already cancelled — skipping");
                            continue;
                        }
                        // Reserve the job id *before* spawning: spawn can take
                        // longer than the 5s re-poll window (slow NFS, high
                        // load), and without a placeholder the next poll would
                        // start a second copy of the same job. The placeholder
                        // pid (0) is replaced with the real pid by
                        // execute_job once the process exists, and cleaned up
                        // on every exit path.
                        self.job_runtime
                            .pids
                            .lock()
                            .await
                            .insert(job.job_id.clone(), 0);
                        // Execute the job concurrently so long-running jobs do
                        // not block polling for new jobs / cancellations.
                        let config = self.config.clone();
                        let runtime = self.job_runtime.clone();
                        let mut client = self.client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = crate::job_executor::execute_job(
                                &config,
                                job,
                                &mut client,
                                &runtime,
                            )
                            .await
                            {
                                warn!(error = %e, "Job execution failed");
                            }
                        });
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Error receiving job");
                }
            }
        }

        Ok(())
    }
}
