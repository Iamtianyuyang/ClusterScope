use anyhow::{Context, Result};
use common::config::AgentConfig;
use protocol::{
    AgentServiceClient, NodeInfo, NodeMetricsReport, NodeHeartbeat, Job, JobDefinition,
    JobLogEntry,
};
use tokio::time::Duration;
use tokio_stream::StreamExt;
use tracing::{info, warn};

/// Best-effort local IP detection (UDP connect trick; no packets sent).
fn local_ip() -> String {
    use std::net::UdpSocket;
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(sock) => match sock.connect("192.0.2.1:80") {
            Ok(()) => sock.local_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
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
    sequence: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl AgentClient {
    pub async fn new(
        server_addr: String,
        node_id: String,
        config: AgentConfig,
    ) -> Result<Self> {
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
        
        Ok(Self {
            client,
            node_id,
            config,
            sequence: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        })
    }
    
    pub async fn register(&mut self) -> Result<()> {
        let node_info = self.build_node_info();
        let request = tonic::Request::new(node_info);
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
        let cpu_model = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
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
        let seq = self.sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        report.node_id = self.node_id.clone();
        report.sequence = seq;
        
        let request = tonic::Request::new(tokio_stream::iter(vec![report]));
        
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
        
        let request = tonic::Request::new(tokio_stream::iter(vec![heartbeat]));
        
        let _response = self.client.heartbeat(request).await?;
        Ok(())
    }
    
    pub async fn submit_job(&mut self, job: JobDefinition) -> Result<Job> {
        let request = tonic::Request::new(job);
        let response = self.client.submit_job(request).await?;
        Ok(response.into_inner())
    }
    
    pub async fn report_job_logs(&mut self, _job_id: &str, logs: Vec<JobLogEntry>) -> Result<()> {
        let request = tonic::Request::new(tokio_stream::iter(logs));
        let _response = self.client.report_job_logs(request).await?;
        Ok(())
    }
    
    pub async fn cancel_job(&mut self, job_id: &str, force: bool) -> Result<Job> {
        let request = tonic::Request::new(protocol::CancelJobRequest {
            node_id: self.node_id.clone(),
            job_id: job_id.to_string(),
            force,
        });
        let response = self.client.cancel_job(request).await?;
        Ok(response.into_inner())
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
        
        let request = tonic::Request::new(node_info);
        let mut stream = self.client.get_pending_jobs(request).await?
            .into_inner();
        
        while let Some(job_result) = stream.next().await {
            match job_result {
                Ok(job) => {
                    info!(job_id = %job.job_id, name = %job.name, "Received pending job");
                    // Execute job
                    if let Err(e) = crate::job_executor::execute_job(
                        &self.config,
                        job,
                        &mut self.client,
                    ).await {
                        warn!(error = %e, "Job execution failed");
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
