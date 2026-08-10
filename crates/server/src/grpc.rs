use crate::AppState;
use protocol::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};
use tracing::{info, warn};

pub struct AgentServiceImpl {
    state: Arc<AppState>,
}

impl AgentServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl AgentService for AgentServiceImpl {
    type ReportMetricsStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<MetricsAck, Status>> + Send>
    >;

    async fn register(&self, request: Request<NodeInfo>) -> Result<Response<NodeInfo>, Status> {
        let node_info = request.into_inner();
        info!(node_id = %node_info.node_id, "Agent registered");

        let entry = common::node_registry::NodeEntry {
            node_id: node_info.node_id.clone(),
            hostname: node_info.hostname.clone(),
            ip_address: node_info.ip_address.clone(),
            agent_version: node_info.agent_version.clone(),
            os_info: node_info.os_info.clone(),
            kernel_version: node_info.kernel_version.clone(),
            cpu_model: node_info.cpu_model.clone(),
            cpu_cores: node_info.cpu_cores,
            memory_total_bytes: node_info.memory_total_bytes,
            registered_at: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            status: common::node_registry::NodeStatus::Online,
            labels: node_info.labels.clone(),
            gpu_count: 0,
        };

        self.state.node_registry.register(entry.clone());

        // Persist node info (upsert) so metrics can reference it via FK.
        if let Err(e) = storage::queries::upsert_node_info(
            self.state.database.pool(),
            &node_info.node_id,
            &node_info.hostname,
            &node_info.ip_address,
            &node_info.agent_version,
            &node_info.os_info,
            &node_info.kernel_version,
            &node_info.cpu_model,
            node_info.cpu_cores as i32,
            node_info.memory_total_bytes as i64,
            serde_json::to_value(&node_info.labels).unwrap_or_else(|_| serde_json::json!({})),
            0, // gpu_count is learned from metrics after registration
        ).await {
            warn!(error = %e, "Failed to persist node info");
        }

        Ok(Response::new(node_info))
    }

    async fn report_metrics(&self, request: Request<Streaming<NodeMetricsReport>>) -> Result<Response<Self::ReportMetricsStream>, Status> {
        let mut stream = request.into_inner();
        let state = self.state.clone();

        let (tx, rx) = mpsc::channel(10);

        tokio::spawn(async move {
            while let Some(report_result) = stream.next().await {
                let report = match report_result {
                    Ok(r) => r,
                    Err(e) => { warn!(error = %e, "Error receiving metrics report"); break; }
                };

                let node_id = report.node_id.clone();
                let sequence = report.sequence;
                state.node_registry.update_last_seen(&node_id, chrono::Utc::now());

                let cache_key = format!("{}:{}", node_id, sequence);
                let is_new = {
                    let mut seen = state.seen_reports.write();
                    if seen.contains(&cache_key) { false } else { seen.put(cache_key, ()); true }
                };

                if !is_new {
                    let _ = tx.send(Ok(MetricsAck { node_id: node_id.clone(), sequence, received_at: None })).await;
                    continue;
                }

                if let Err(e) = save_metrics_to_db(&state, &report).await {
                    warn!(error = %e, "Failed to save metrics");
                }

                let json = serde_json::json!({
                    "type": "metrics_update",
                    "node_id": node_id,
                    "payload": {
                        "sequence": report.sequence,
                        "timestamp_ms": report.timestamp_ms,
                        "cpu_usage_percent": report.cpu_usage_percent,
                        "load_1": report.load_1,
                        "memory_total_bytes": report.memory_total_bytes,
                        "memory_used_bytes": report.memory_used_bytes,
                        "gpu_count": report.gpus.len(),
                    }
                }).to_string();
                state.ws_manager.push_metrics(&node_id, json).await;

                let _ = tx.send(Ok(MetricsAck {
                    node_id, sequence,
                    received_at: Some(prost_types::Timestamp { seconds: chrono::Utc::now().timestamp(), nanos: 0 }),
                })).await;
            }
        });

        let stream: Self::ReportMetricsStream =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }

    async fn submit_job(&self, request: Request<JobDefinition>) -> Result<Response<Job>, Status> {
        let job_def = request.into_inner();
        info!(job_id = %job_def.job_id, "Job submitted");

        let job_row = storage::models::JobRow {
            job_id: job_def.job_id.clone(), node_id: job_def.node_id.clone(),
            name: job_def.name.clone(), executable: job_def.executable.clone(),
            arguments: serde_json::to_value(&job_def.arguments).unwrap_or(serde_json::Value::Array(vec![])),
            working_directory: job_def.working_directory.clone(),
            environment: serde_json::to_value(&job_def.environment).unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
            status: "queued".to_string(), pid: None, exit_code: None, error_message: None,
            created_at: job_def.created_at
                .map(|t| chrono::DateTime::<chrono::Utc>::from_timestamp(t.seconds, t.nanos as u32).unwrap_or(chrono::Utc::now()))
                .unwrap_or(chrono::Utc::now()),
            started_at: None, finished_at: None, created_by: job_def.created_by.clone(),
            resource_quota: None, retry_count: 0, max_retries: 0,
        };

        if let Err(e) = storage::job_queries::insert_job(self.state.database.pool(), &job_row).await {
            warn!(error = %e, "Failed to save job");
        }

        Ok(Response::new(Job {
            job_id: job_def.job_id, node_id: job_def.node_id, name: job_def.name,
            executable: job_def.executable, arguments: job_def.arguments,
            working_directory: job_def.working_directory, environment: job_def.environment,
            status: 1, pid: 0, exit_code: 0, error_message: String::new(),
            created_at: job_def.created_at, started_at: None, finished_at: None,
            created_by: job_def.created_by, log_offset: 0, resource_quota: String::new(),
            retry_count: 0, max_retries: 0,
        }))
    }

    type GetPendingJobsStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<Job, Status>> + Send>>;

    async fn get_pending_jobs(&self, request: Request<NodeInfo>) -> Result<Response<Self::GetPendingJobsStream>, Status> {
        let node_id = request.into_inner().node_id;
        let state = self.state.clone();
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                if let Ok(jobs) = storage::job_queries::get_running_jobs(state.database.pool(), &node_id).await {
                    for job in jobs {
                        let _ = tx.send(Ok(job_to_proto(&job))).await;
                    }
                }
            }
        });

        let stream: Self::GetPendingJobsStream =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }

    type ReportJobLogsStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<LogAck, Status>> + Send>>;

    async fn report_job_logs(&self, request: Request<Streaming<JobLogEntry>>) -> Result<Response<Self::ReportJobLogsStream>, Status> {
        let mut stream = request.into_inner();
        let state = self.state.clone();
        let (tx, rx) = mpsc::channel(10);

        tokio::spawn(async move {
            while let Some(entry_result) = stream.next().await {
                let entry = match entry_result { Ok(e) => e, Err(e) => { warn!(error = %e, "Error receiving job log"); break; } };
                if let Err(e) = save_job_log(&state, &entry).await {
                    warn!(error = %e, "Failed to save job log");
                }
                let _ = tx.send(Ok(LogAck { job_id: entry.job_id.clone(), acknowledged_offset: entry.log_offset })).await;
            }
        });

        let stream: Self::ReportJobLogsStream =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }

    type WatchJobStatusStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<JobStatusUpdate, Status>> + Send>>;

    async fn watch_job_status(&self, request: Request<Streaming<JobStatusUpdate>>) -> Result<Response<Self::WatchJobStatusStream>, Status> {
        let mut stream = request.into_inner();
        let state = self.state.clone();
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            while let Some(update_result) = stream.next().await {
                let update = match update_result { Ok(u) => u, Err(e) => { warn!(error = %e, "Error receiving job status update"); break; } };
                let status_str = match update.status {
                    1 => "queued", 2 => "starting", 3 => "running", 4 => "stopping",
                    5 => "succeeded", 6 => "failed", 7 => "cancelled", 8 => "lost", _ => "unknown",
                };
                if let Err(e) = storage::job_queries::update_job_status(
                    state.database.pool(), &update.job_id, status_str, None, None, Some(&update.message), None, None,
                ).await {
                    warn!(error = %e, "Failed to update job status");
                }
                let _ = tx.send(Ok(JobStatusUpdate {
                    job_id: update.job_id.clone(),
                    status: update.status,
                    message: String::new(),
                })).await;
            }
        });

        let stream: Self::WatchJobStatusStream =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }

    type HeartbeatStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<HeartbeatAck, Status>> + Send>>;

    async fn heartbeat(&self, request: Request<Streaming<NodeHeartbeat>>) -> Result<Response<Self::HeartbeatStream>, Status> {
        let mut stream = request.into_inner();
        let state = self.state.clone();
        let (tx, rx) = mpsc::channel(10);

        tokio::spawn(async move {
            while let Some(heartbeat_result) = stream.next().await {
                let heartbeat = match heartbeat_result { Ok(h) => h, Err(e) => { warn!(error = %e, "Error receiving heartbeat"); break; } };
                state.node_registry.update_last_seen(&heartbeat.node_id, chrono::Utc::now());
                let _ = tx.send(Ok(HeartbeatAck { node_id: heartbeat.node_id.clone(), timestamp_ms: heartbeat.timestamp_ms, pending_job_count: 0 })).await;
            }
        });

        let stream: Self::HeartbeatStream =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }

    async fn cancel_job(&self, request: Request<CancelJobRequest>) -> Result<Response<Job>, Status> {
        let req = request.into_inner();
        info!(job_id = %req.job_id, node_id = %req.node_id, "Job cancellation requested");
        Ok(Response::new(Job { job_id: req.job_id, node_id: req.node_id, status: 4, ..Default::default() }))
    }
}

// Helper functions for gRPC
async fn save_metrics_to_db(state: &AppState, report: &NodeMetricsReport) -> anyhow::Result<()> {
    let gpu_metrics: Vec<_> = report.gpus.iter().map(|g| serde_json::json!({
        "index": g.index, "uuid": g.uuid, "name": g.name,
        "utilization_gpu": g.utilization_gpu, "utilization_memory": g.utilization_memory,
        "memory_total_bytes": g.memory_total_bytes, "memory_used_bytes": g.memory_used_bytes,
        "temperature_celsius": g.temperature_celsius, "power_watts": g.power_watts,
        "fan_speed_percent": g.fan_speed_percent, "mig_enabled": g.mig_enabled,
    })).collect();
    let gpu_metrics = serde_json::to_value(gpu_metrics).ok();
    let gpu_processes: Vec<_> = report.gpu_processes.iter().map(|p| serde_json::json!({
        "pid": p.pid, "username": p.username, "command": p.command, "gpu_uuid": p.gpu_uuid,
        "gpu_memory_bytes": p.gpu_memory_bytes, "cpu_percent": p.cpu_percent,
        "system_memory_bytes": p.system_memory_bytes, "started_at": p.started_at,
        "sm_utilization": p.sm_utilization, "memory_utilization": p.memory_utilization,
        "encoder_utilization": p.encoder_utilization, "decoder_utilization": p.decoder_utilization,
    })).collect();
    let gpu_processes = serde_json::to_value(gpu_processes).ok();
    let cpu_core_metrics: Vec<_> = report.cpu_cores.iter().map(|c| serde_json::json!({
        "core_id": c.core_id, "usage_percent": c.usage_percent,
    })).collect();
    let cpu_core_metrics = serde_json::to_value(cpu_core_metrics).ok();
    let network_metrics: Vec<_> = report.networks.iter().map(|n| {
        serde_json::json!({"interface_name": n.interface_name, "bytes_sent": n.bytes_sent, "bytes_recv": n.bytes_recv})
    }).collect();
    let network_metrics = serde_json::to_value(network_metrics).ok();
    let disk_metrics: Vec<_> = report.disks.iter().map(|d| {
        serde_json::json!({"mount_point": d.mount_point, "total_bytes": d.total_bytes, "used_bytes": d.used_bytes, "free_bytes": d.free_bytes})
    }).collect();
    let disk_metrics = serde_json::to_value(disk_metrics).ok();

    storage::queries::insert_node_metrics(
        state.database.pool(),
        &report.node_id,
        report.sequence as i64,
        report.timestamp_ms as i64,
        Some(report.monotonic_clock_ms as i64),
        Some(report.cpu_usage_percent),
        Some(report.load_1),
        Some(report.load_5),
        Some(report.load_15),
        Some(report.memory_total_bytes as i64),
        Some(report.memory_used_bytes as i64),
        Some(0_i64), // swap_total
        Some(0_i64), // swap_used
        Some(report.uptime_seconds as i64),
        Some(report.boot_time_seconds as i64),
        gpu_metrics,
        gpu_processes,
        network_metrics,
        disk_metrics,
        cpu_core_metrics,
    )
    .await?;
    Ok(())
}

fn ts_from_datetime(dt: chrono::DateTime<chrono::Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

fn job_to_proto(job: &storage::models::JobRow) -> Job {
    Job {
        job_id: job.job_id.clone(),
        node_id: job.node_id.clone(),
        name: job.name.clone(),
        executable: job.executable.clone(),
        arguments: serde_json::from_value(job.arguments.clone()).unwrap_or_default(),
        working_directory: job.working_directory.clone(),
        environment: serde_json::from_value(job.environment.clone()).unwrap_or_default(),
        status: match job.status.as_str() {
            "queued" => 1, "starting" => 2, "running" => 3, "stopping" => 4,
            "succeeded" => 5, "failed" => 6, "cancelled" => 7, "lost" => 8, _ => 3,
        },
        pid: job.pid.map(|p| p as u32).unwrap_or(0),
        exit_code: job.exit_code.unwrap_or(0) as u32,
        error_message: job.error_message.clone().unwrap_or_default(),
        created_at: Some(ts_from_datetime(job.created_at)),
        started_at: job.started_at.map(ts_from_datetime),
        finished_at: job.finished_at.map(ts_from_datetime),
        created_by: job.created_by.clone(),
        log_offset: 0,
        resource_quota: job.resource_quota.clone().unwrap_or_default(),
        retry_count: job.retry_count as u32,
        max_retries: job.max_retries as u32,
    }
}

fn db_metrics_to_proto(metrics: &storage::models::NodeMetricsRow) -> NodeMetricsReport {
    NodeMetricsReport {
        node_id: metrics.node_id.clone(),
        sequence: metrics.sequence as u64,
        timestamp_ms: metrics.timestamp_ms as u64,
        monotonic_clock_ms: metrics.monotonic_clock_ms.map(|v| v as u64).unwrap_or(0),
        cpu_usage_percent: metrics.cpu_usage_percent.unwrap_or(0.0),
        cpu_cores: vec![],
        load_1: metrics.load_1.unwrap_or(0.0),
        load_5: metrics.load_5.unwrap_or(0.0),
        load_15: metrics.load_15.unwrap_or(0.0),
        memory_total_bytes: metrics.memory_total_bytes.unwrap_or(0) as u64,
        memory_used_bytes: metrics.memory_used_bytes.unwrap_or(0) as u64,
        swap_total_bytes: metrics.swap_total_bytes.unwrap_or(0) as u64,
        swap_used_bytes: metrics.swap_used_bytes.unwrap_or(0) as u64,
        uptime_seconds: metrics.uptime_seconds.unwrap_or(0) as u64,
        boot_time_seconds: metrics.boot_time_seconds.unwrap_or(0) as u64,
        disks: vec![],
        networks: vec![],
        gpus: vec![],
        gpu_processes: vec![],
    }
}

async fn save_job_log(state: &AppState, entry: &JobLogEntry) -> anyhow::Result<()> {
    
    let pool = state.database.pool();
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(entry.timestamp)
        .unwrap_or(chrono::Utc::now());

    sqlx::query(
        "INSERT INTO job_logs (job_id, log_offset, log_data, is_stderr, timestamp) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING"
    )
    .bind(&entry.job_id)
    .bind(entry.log_offset)
    .bind(&entry.log_data)
    .bind(entry.is_stderr)
    .bind(timestamp)
    .execute(pool)
    .await
    .ok();

    Ok(())
}


