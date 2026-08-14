use anyhow::Result;
use common::config::AgentConfig;
use protocol::{AgentServiceClient, Job, JobLogEntry, JobStatus, JobStatusUpdate};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Shared runtime state for jobs executed by this agent: process group leader
/// PIDs (for cancellation) and a set of job ids that were cancelled.
pub struct JobRuntime {
    /// job_id -> process group leader pid
    pub pids: Arc<Mutex<HashMap<String, i32>>>,
    /// job ids cancelled by the server (kill was requested or will be)
    pub cancelled: Arc<Mutex<HashSet<String>>>,
}

impl JobRuntime {
    pub fn new() -> Self {
        Self {
            pids: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Send SIGTERM to the process group of a running job (if any).
    /// Returns true when the runtime owns the job (killed, or a spawn is in
    /// flight and will observe the cancel); false when the job was never
    /// seen — the caller reports it as cancelled-before-start.
    pub async fn request_cancel(&self, job_id: &str) -> bool {
        self.cancelled.lock().await.insert(job_id.to_string());
        let pids = self.pids.lock().await;
        match pids.get(job_id).copied() {
            Some(pid) if pid > 0 => {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
                true
            }
            // Placeholder (0) = spawn in flight: the executor's
            // is_cancelled checks terminate and report it. Kill nothing.
            Some(_) => true,
            None => false,
        }
    }

    pub async fn is_cancelled(&self, job_id: &str) -> bool {
        self.cancelled.lock().await.contains(job_id)
    }
}

/// Report a job status transition to the server (best effort).
async fn report_status(
    client: &mut AgentServiceClient<tonic::transport::Channel>,
    job_id: &str,
    status: JobStatus,
    message: &str,
) {
    let _ = client
        .update_job_status(JobStatusUpdate {
            job_id: job_id.to_string(),
            status: status as i32,
            message: message.to_string(),
        })
        .await;
}

/// Execute a job on the agent node and report status transitions to the server.
pub async fn execute_job(
    config: &AgentConfig,
    job: Job,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
    runtime: &JobRuntime,
) -> Result<()> {
    let job_id = job.job_id.clone();
    let executable = job.executable.clone();
    let arguments = job.arguments.clone();
    let working_dir = job.working_directory.clone();
    let env: HashMap<String, String> = job.environment.clone();

    info!(job_id = %job_id, executable = %executable, "Starting job");

    // Abort early when the job was cancelled before we spawned it.
    if runtime.is_cancelled(&job_id).await {
        info!(job_id = %job_id, "Job cancelled before start");
        report_status(
            client,
            &job_id,
            JobStatus::Cancelled,
            "cancelled before start",
        )
        .await;
        runtime.cancelled.lock().await.remove(&job_id);
        // Drop the poll-loop placeholder so the job id is free for a
        // future dispatch.
        runtime.pids.lock().await.remove(&job_id);
        return Ok(());
    }

    // Create log directory
    let log_dir = config.log_dir.join(&job_id);
    if let Err(e) = tokio::fs::create_dir_all(&log_dir).await {
        // The poll loop inserted a placeholder pid (0) for this job: drop it
        // so a later re-send is not skipped forever (job would stay stuck in
        // `starting`). The job itself is left for the server to requeue.
        runtime.pids.lock().await.remove(&job_id);
        return Err(anyhow::anyhow!(
            "Failed to create log dir for job {}: {}",
            job_id,
            e
        ));
    }

    // Start process
    let mut cmd = Command::new(&executable);
    cmd.args(&arguments)
        .current_dir(&working_dir)
        .envs(&env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Set process group
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut process = match cmd.spawn() {
        Ok(p) => p,
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to spawn process");
            report_status(
                client,
                &job_id,
                JobStatus::Failed,
                &format!("spawn failed: {}", e),
            )
            .await;
            runtime.cancelled.lock().await.remove(&job_id);
            // Drop the poll-loop placeholder (spawn never happened).
            runtime.pids.lock().await.remove(&job_id);
            return Ok(());
        }
    };

    let pid = process.id().unwrap_or(0);
    let mut process_stdout = process.stdout.take().expect("stdout piped");
    let mut process_stderr = process.stderr.take().expect("stderr piped");
    info!(job_id = %job_id, pid = pid, "Process spawned");

    // Register the process group so cancellation can reach it.
    runtime.pids.lock().await.insert(job_id.clone(), pid as i32);

    // If a cancel arrived between the early check and now, kill immediately.
    if runtime.is_cancelled(&job_id).await {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        warn!(job_id = %job_id, "Cancel raced spawn — killing process group");
    }

    // Report running.
    report_status(client, &job_id, JobStatus::Running, "").await;

    // Global, monotonically increasing offset shared by stdout+stderr so the
    // server-side UNIQUE(job_id, log_offset) never collides between streams.
    let log_offset = Arc::new(AtomicI64::new(0));

    // Stream stdout and stderr to files and gRPC
    let stdout_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let offset = log_offset.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(
                &mut process_stdout,
                &log_dir,
                &job_id,
                false,
                &offset,
                &mut client,
            )
            .await
        })
    };

    let stderr_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let offset = log_offset.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(
                &mut process_stderr,
                &log_dir,
                &job_id,
                true,
                &offset,
                &mut client,
            )
            .await
        })
    };

    // Wait for process to finish
    let wait_result = process.wait().await;

    runtime.pids.lock().await.remove(&job_id);

    // Stop streaming
    stdout_task.abort();
    stderr_task.abort();
    let _ = tokio::join!(stdout_task, stderr_task);

    match wait_result {
        Ok(status) => {
            let exit_code = status.code().unwrap_or(-1);
            info!(job_id = %job_id, pid = pid, exit_code, "Job finished");

            let (final_status, message) = if runtime.is_cancelled(&job_id).await {
                (
                    JobStatus::Cancelled,
                    format!("cancelled (exit {})", exit_code),
                )
            } else if exit_code == 0 {
                (JobStatus::Succeeded, String::new())
            } else {
                (JobStatus::Failed, format!("exit code {}", exit_code))
            };
            let _ = report_status(client, &job_id, final_status, &message).await;
        }
        Err(e) => {
            error!(job_id = %job_id, pid = pid, error = %e, "Process wait failed");
            report_status(
                client,
                &job_id,
                JobStatus::Failed,
                &format!("wait failed: {}", e),
            )
            .await;
        }
    }

    // The job is finished: drop cancel bookkeeping so the set does not grow
    // without bound on long-lived agents.
    runtime.cancelled.lock().await.remove(&job_id);

    Ok(())
}

async fn stream_output<R>(
    reader: &mut R,
    log_dir: &std::path::Path,
    job_id: &str,
    is_stderr: bool,
    log_offset: &AtomicI64,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(if is_stderr {
            "stderr.log"
        } else {
            "stdout.log"
        }))
        .await?;

    loop {
        let mut line = String::new();
        match buf_reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let offset = log_offset.fetch_add(1, Ordering::SeqCst);
                let entry = JobLogEntry {
                    job_id: job_id.to_string(),
                    log_data: line.clone(),
                    is_stderr,
                    timestamp: chrono::Utc::now().timestamp_millis() as i64,
                    log_offset: offset,
                };

                // Write to file
                use tokio::io::AsyncWriteExt;
                file.write_all(line.as_bytes()).await?;

                // Send to server (best effort)
                let _ = client
                    .report_job_logs(tokio_stream::iter(vec![entry]))
                    .await;
            }
            Err(e) => {
                warn!(error = %e, "Error reading job output");
                break;
            }
        }
    }

    Ok(())
}
