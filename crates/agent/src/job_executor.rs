use anyhow::{Context, Result};
use common::config::AgentConfig;
use protocol::{AgentServiceClient, Job, JobLogEntry, JobStatus, JobStatusUpdate};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Shared runtime state for jobs executed by this agent: process group leader
/// PIDs (for cancellation), a set of job ids that were cancelled, and a set of
/// job ids currently executing (duplicate-dispatch guard).
pub struct JobRuntime {
    /// job_id -> (process group leader pid, leader starttime). The starttime
    /// lets cancellation refuse to signal a pid that was recycled by the
    /// kernel to an unrelated process group after the leader exited.
    pub pids: Arc<Mutex<HashMap<String, (i32, u64)>>>,
    /// job ids cancelled by the server (kill was requested or will be)
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    /// job ids currently executing on this agent
    running: Arc<Mutex<HashSet<String>>>,
}

impl JobRuntime {
    pub fn new() -> Self {
        Self {
            pids: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            running: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Try to claim a job for execution. Returns `false` when the job is
    /// already running here (duplicate dispatch: the server re-sends
    /// `starting` jobs every few seconds and the scheduler may duplicate
    /// queue entries) — the duplicate is dropped, never re-executed.
    pub async fn try_claim(&self, job_id: &str) -> bool {
        self.running.lock().await.insert(job_id.to_string())
    }

    /// Release the execution claim (job finished / failed / was dropped).
    pub async fn release(&self, job_id: &str) {
        self.running.lock().await.remove(job_id);
    }

    /// Send SIGTERM to the process group of a running job (if any).
    /// Returns true when the job was running and got the signal.
    ///
    /// Idempotent per job: the server re-sends `stopping` jobs on every 5s
    /// poll, and a repeat call must not re-signal the group or pile up
    /// escalation tasks — the first call's SIGTERM + 5s SIGKILL escalation
    /// already cover the process.
    pub async fn request_cancel(&self, job_id: &str) -> bool {
        // Lock ordering: pids before cancelled. `prune()` takes running →
        // pids → cancelled and the executor takes pids (register) before
        // cancelled (is_cancelled); the previous cancelled → pids order
        // could deadlock against prune().
        let pids = self.pids.lock().await;
        let mut cancelled = self.cancelled.lock().await;
        // Already cancelled: the kill is in flight (or the job is already
        // gone) — nothing new to do.
        if cancelled.contains(job_id) {
            return pids.contains_key(job_id);
        }
        cancelled.insert(job_id.to_string());
        if let Some(&(pid, expected_start)) = pids.get(job_id) {
            // Never signal a pid that was recycled to an unrelated process
            // group: the group leader may have exited while its children
            // live on, and the kernel can hand the freed pid to a new
            // session/process-group leader. When the recorded leader is alive at
            // that pid with a different starttime, it is not ours.
            if let Some(actual) = process_starttime(pid)
                && expected_start > 0
                && actual != expected_start
            {
                warn!(
                    job_id = %job_id, pid,
                    "Job pid recycled — cannot signal the original process group"
                );
                return true; // still "running" as far as the runtime knows
            }
            // C-side guard kept: pid 0 is the placeholder the poll loop
            // inserts while a spawn is in flight. `kill(-0, SIG)` would
            // signal the caller's own process group (the agent itself).
            if pid <= 0 {
                return true;
            }
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            // Escalate to SIGKILL after 5s unless the process already exited
            // (the pid entry is removed right after `wait()` returns, so a
            // live entry here means the process group is still around).
            let pids = self.pids.clone();
            let job_id = job_id.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let pids = pids.lock().await;
                if let Some(&(pid, expected_start)) = pids.get(&job_id) {
                    // Same recycled-pid guard as above.
                    if let Some(actual) = process_starttime(pid)
                        && expected_start > 0
                        && actual != expected_start
                    {
                        warn!(job_id = %job_id, pid, "SIGKILL escalation skipped: pid recycled");
                        return;
                    }
                    // Same C-side placeholder guard: never signal pid 0.
                    if pid <= 0 {
                        return;
                    }
                    warn!(job_id = %job_id, pid, "SIGTERM ignored, escalating to SIGKILL");
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
            });
            true
        } else {
            false
        }
    }

    /// Forced cancellation (the server asked for it explicitly): SIGKILL the
    /// process group immediately instead of waiting out the SIGTERM grace
    /// period. Records the cancel marker as well, so a request that raced the
    /// spawn still stops the job from starting.
    pub async fn force_cancel(&self, job_id: &str) -> bool {
        let pids = self.pids.lock().await;
        let mut cancelled = self.cancelled.lock().await;
        cancelled.insert(job_id.to_string());
        match pids.get(job_id).copied() {
            Some((pid, expected_start)) if pid > 0 => {
                if let Some(actual) = process_starttime(pid)
                    && expected_start > 0
                    && actual != expected_start
                {
                    warn!(
                        job_id = %job_id, pid,
                        "Job pid recycled — cannot force-kill the original process group"
                    );
                    return true;
                }
                warn!(job_id = %job_id, pid, "Forced cancellation — sending SIGKILL");
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    pub async fn is_cancelled(&self, job_id: &str) -> bool {
        self.cancelled.lock().await.contains(job_id)
    }

    /// Drop cancellation markers for jobs that are no longer executing here
    /// and no longer have a process group — they will never be checked again.
    /// Prevents the cancelled set from growing without bound on a long-lived
    /// agent (one entry per cancelled job, forever).
    pub async fn prune(&self) {
        let running = self.running.lock().await;
        let pids = self.pids.lock().await;
        let mut cancelled = self.cancelled.lock().await;
        cancelled.retain(|id| running.contains(id) || pids.contains_key(id));
    }
}

/// Report a job status transition to the server (best effort). `pid` is the
/// process id of the executed job (0 when nothing was spawned); `exit_code`
/// follows Unix convention (negative = killed by signal) and is only
/// meaningful for terminal states.
async fn report_status(
    client: &mut AgentServiceClient<tonic::transport::Channel>,
    token: &str,
    job_id: &str,
    status: JobStatus,
    message: &str,
    pid: u32,
    exit_code: i32,
) {
    // M6 adaptation: this tree's `JobStatusUpdate` message has no `pid` /
    // `exit_code` fields (the proto is outside this round's change set), so
    // the two values are recorded in the agent log -- which is where the
    // F-03 evidence ("the agent log has the real pid") lives anyway. The
    // follow-up round that extends the proto can lift them onto the wire.
    tracing::debug!(job_id = %job_id, pid, exit_code, "reporting job status");
    let _ = client
        .update_job_status(crate::grpc_client::authed(
            token,
            tonic::Request::new(JobStatusUpdate {
                job_id: job_id.to_string(),
                status: status as i32,
                message: message.to_string(),
            }),
        ))
        .await;
}

/// Execute a job on the agent node and report status transitions to the server.
///
/// Entry point: claims the job in the runtime (dedup) and wraps the real
/// execution so the claim is released on every exit path.
pub async fn execute_job(
    config: &AgentConfig,
    job: Job,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
    runtime: &JobRuntime,
) -> Result<()> {
    let job_id = job.job_id.clone();

    // Duplicate dispatch guard: if we are already running this job (the
    // server re-sends 'starting' jobs every 5s and the scheduler may
    // duplicate entries), ignore the second copy instead of executing it
    // concurrently and clobbering the pid / log offsets.
    if !runtime.try_claim(&job_id).await {
        info!(job_id = %job_id, "Duplicate dispatch ignored — job already running on this agent");
        return Ok(());
    }

    let result = execute_job_inner(config, job, client, runtime).await;
    runtime.release(&job_id).await;
    result
}

/// The actual job execution (see [`execute_job`] for the claim wrapper).
async fn execute_job_inner(
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
            &config.agent_token,
            &job_id,
            JobStatus::Cancelled,
            "cancelled before start",
            0,
            0,
        )
        .await;
        return Ok(());
    }

    // Create log directory
    let log_dir = config.log_dir.join(&job_id);

    // Double-run guard: a previous run of this job may have finished while
    // the server never learned about it (both status RPCs lost to a gRPC
    // outage, node went offline, stale-`starting` reaper requeued the job).
    // Re-executing would run the job twice; instead replay the recorded
    // terminal outcome and skip execution. The marker is keyed on
    // retry_count, so a legitimate server-side retry (counter bumped)
    // still executes a fresh run.
    match read_completed_marker(&log_dir, job.retry_count).await {
        MarkerRead::Valid((status, exit_code, message)) => {
            info!(job_id = %job_id, status = ?status, "Replaying completed run (terminal report was lost)");
            report_status(
                client,
                &config.agent_token,
                &job_id,
                status,
                &message,
                0,
                exit_code,
            )
            .await;
            return Ok(());
        }
        // A marker exists but is unreadable / carries an unknown status:
        // do NOT execute. The conservative choice is to fail loudly — an
        // unknown marker usually means a bug (or version skew), and
        // re-executing could run the job a second time.
        MarkerRead::Corrupt => {
            error!(job_id = %job_id, "Refusing to execute job with corrupt completion marker");
            report_status(
                client,
                &config.agent_token,
                &job_id,
                JobStatus::Failed,
                "refusing to execute: corrupt completion marker",
                0,
                -1,
            )
            .await;
            return Ok(());
        }
        MarkerRead::Missing => {}
    }

    tokio::fs::create_dir_all(&log_dir)
        .await
        .with_context(|| format!("Failed to create log dir for job {}", job_id))?;

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
            write_completed_marker(
                &log_dir,
                job.retry_count,
                JobStatus::Failed,
                -1,
                &format!("spawn failed: {}", e),
            )
            .await;
            report_status(
                client,
                &config.agent_token,
                &job_id,
                JobStatus::Failed,
                &format!("spawn failed: {}", e),
                0,
                -1,
            )
            .await;
            return Ok(());
        }
    };

    let pid = process.id().unwrap_or(0);
    let mut process_stdout = process.stdout.take().expect("stdout piped");
    let mut process_stderr = process.stderr.take().expect("stderr piped");
    info!(job_id = %job_id, pid = pid, "Process spawned");

    // Register the process group so cancellation can reach it. The starttime
    // is captured BEFORE inserting so request_cancel can verify the pid was
    // not recycled when it later signals the group.
    let starttime = process_starttime(pid as i32).unwrap_or(0);
    runtime
        .pids
        .lock()
        .await
        .insert(job_id.clone(), (pid as i32, starttime));

    // Persist the pid + starttime so a restarted agent can reap this process
    // group (orphan cleanup on startup — without it, a crash/restart followed
    // by a re-dispatch of the same `starting` job would run it a second time
    // while the original process group is still alive). The starttime lets
    // the cleanup skip pids that were recycled to unrelated processes.
    let pid_file = log_dir.join("pid");
    if let Err(e) = tokio::fs::write(&pid_file, format!("{}\n{}", pid, starttime)).await {
        warn!(job_id = %job_id, error = %e, "Failed to persist job pid file");
    }

    // If a cancel arrived between the early check and now, kill immediately.
    if runtime.is_cancelled(&job_id).await {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        warn!(job_id = %job_id, "Cancel raced spawn — killing process group");
    }

    // Report running (with the pid so the server can surface it).
    report_status(
        client,
        &config.agent_token,
        &job_id,
        JobStatus::Running,
        "",
        pid,
        0,
    )
    .await;

    // Global, monotonically increasing offset shared by stdout+stderr so the
    // server-side UNIQUE(job_id, log_offset) never collides between streams.
    let log_offset = Arc::new(AtomicI64::new(0));

    // Stream stdout and stderr to files and gRPC
    let token = config.agent_token.clone();
    let stdout_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let offset = log_offset.clone();
        let token = token.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(
                &mut process_stdout,
                &log_dir,
                &job_id,
                false,
                &offset,
                &token,
                &mut client,
            )
            .await
        })
    };

    let stderr_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let offset = log_offset.clone();
        let token = token.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(
                &mut process_stderr,
                &log_dir,
                &job_id,
                true,
                &offset,
                &token,
                &mut client,
            )
            .await
        })
    };

    // Wait for process to finish
    let wait_result = process.wait().await;

    // The group leader exited, but processes it spawned (same process
    // group) may still be alive — e.g. a launcher that forked workers and
    // exited. Without this they would keep running detached (and keep
    // using GPU memory) while the job is reported finished and its GPU
    // capacity is re-dispatched. Terminate the remainder of the group
    // before reporting the terminal outcome.
    kill_remaining_process_group(pid as i32, starttime).await;

    runtime.pids.lock().await.remove(&job_id);
    // Drop the persisted pid marker (the process group is gone now).
    let _ = tokio::fs::remove_file(&pid_file).await;

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
            // Record the terminal outcome before reporting it: if the
            // report is lost, a re-dispatched copy of this run replays the
            // marker instead of executing a second time.
            write_completed_marker(&log_dir, job.retry_count, final_status, exit_code, &message)
                .await;
            let _ = report_status(
                client,
                &config.agent_token,
                &job_id,
                final_status,
                &message,
                pid,
                exit_code,
            )
            .await;
        }
        Err(e) => {
            error!(job_id = %job_id, pid = pid, error = %e, "Process wait failed");
            write_completed_marker(
                &log_dir,
                job.retry_count,
                JobStatus::Failed,
                -1,
                &format!("wait failed: {}", e),
            )
            .await;
            report_status(
                client,
                &config.agent_token,
                &job_id,
                JobStatus::Failed,
                &format!("wait failed: {}", e),
                pid,
                -1,
            )
            .await;
        }
    }

    Ok(())
}

/// Hard cap on a single log line sent to the server. Lines longer than this
/// are truncated (with a visible marker) and the remainder is drained to the
/// next newline, so a job emitting endless/binary output cannot grow the
/// agent's memory (or a single gRPC message / DB row) without bound.
const MAX_LOG_LINE_BYTES: usize = 64 * 1024;

/// Read one line from a buffered reader, capped at `cap` bytes. Returns
/// `None` at EOF. Over-long lines are truncated (a marker is appended) and
/// the rest of the line is consumed so the next read starts at a newline
/// boundary (keeping server-side log offsets aligned with lines).
async fn read_line_capped<R>(reader: &mut R, cap: usize) -> anyhow::Result<Option<String>>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut line: Vec<u8> = Vec::with_capacity(cap.min(1024));
    loop {
        // Copy the pending buffer out so the borrow ends before consume().
        let pending: Vec<u8> = {
            let buf = reader.fill_buf().await?;
            buf.to_vec()
        };
        if pending.is_empty() {
            // EOF.
            if line.is_empty() {
                return Ok(None);
            }
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
        match pending.iter().position(|&b| b == b'\n') {
            Some(idx) => {
                let take = idx + 1;
                if line.len() + take > cap {
                    line.extend_from_slice(&pending[..cap - line.len()]);
                    reader.consume(take);
                    return Ok(Some(truncated_line(&line)));
                }
                line.extend_from_slice(&pending[..take]);
                reader.consume(take);
                // Strip the trailing newline (matches the previous read_line
                // behaviour for the stored log content).
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            None => {
                let room = cap - line.len();
                if pending.len() > room {
                    line.extend_from_slice(&pending[..room]);
                    reader.consume(pending.len());
                    // Drain the remainder of this over-long line.
                    loop {
                        let b: Vec<u8> = {
                            let buf = reader.fill_buf().await?;
                            buf.to_vec()
                        };
                        if b.is_empty() {
                            return Ok(Some(truncated_line(&line)));
                        }
                        match b.iter().position(|&x| x == b'\n') {
                            Some(idx) => {
                                reader.consume(idx + 1);
                                return Ok(Some(truncated_line(&line)));
                            }
                            None => reader.consume(b.len()),
                        }
                    }
                }
                line.extend_from_slice(&pending);
                reader.consume(pending.len());
            }
        }
    }
}

fn truncated_line(line: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(line).into_owned();
    s.push_str(" ...[truncated]");
    s
}

/// Persist the terminal outcome of a job run in `log_dir/done` (lines:
/// retry_count / status int / exit_code / message). Read back by
/// [`read_completed_marker`] when the same run is re-dispatched after the
/// terminal status report was lost — the outcome is replayed instead of
/// re-executing the job. The file lives in the job's log dir, so the
/// existing 7-day log-dir cleanup bounds it.
async fn write_completed_marker(
    job_dir: &Path,
    retry_count: u32,
    status: JobStatus,
    exit_code: i32,
    message: &str,
) {
    // `status` is the proto enum (JOB_STATUS_SUCCEEDED=5, FAILED=6,
    // CANCELLED=7 — see proto/clusterscope.proto). Store and read back with
    // the SAME numbering: mixing in common::job::JobStatus values
    // (Succeeded=4, Failed=5, Cancelled=6) would silently shift every
    // replayed status.
    let content = format!(
        "{}\n{}\n{}\n{}",
        retry_count, status as i32, exit_code, message
    );
    let _ = tokio::fs::write(job_dir.join("done"), content).await;
}

/// Outcome of reading a [`write_completed_marker`] file.
#[derive(Debug, PartialEq)]
pub enum MarkerRead {
    /// No marker for this run (first execution, or the marker belongs to a
    /// different retry_count) — execute normally.
    Missing,
    /// A valid terminal outcome to replay instead of executing.
    Valid((JobStatus, i32, String)),
    /// A marker exists but cannot be parsed or carries an unknown status.
    /// Callers must NOT execute the job (double-run risk).
    Corrupt,
}

/// Read the [`write_completed_marker`] outcome for the given run.
async fn read_completed_marker(job_dir: &Path, retry_count: u32) -> MarkerRead {
    let content = match tokio::fs::read_to_string(job_dir.join("done")).await {
        Ok(c) => c,
        // No marker (or unreadable, treated as missing): first execution.
        Err(_) => return MarkerRead::Missing,
    };
    let mut lines = content.lines();
    let Some(stored_retry) = lines
        .next()
        .map(str::trim)
        .and_then(|s| s.parse::<u32>().ok())
    else {
        return MarkerRead::Corrupt;
    };
    if stored_retry != retry_count {
        // Marker belongs to a different run (the server bumped the counter
        // for a real retry, so a fresh execution is expected).
        return MarkerRead::Missing;
    }
    let Some(status_int) = lines
        .next()
        .map(str::trim)
        .and_then(|s| s.parse::<i32>().ok())
    else {
        return MarkerRead::Corrupt;
    };
    let Some(exit_code) = lines
        .next()
        .map(str::trim)
        .and_then(|s| s.parse::<i32>().ok())
    else {
        return MarkerRead::Corrupt;
    };
    let message = lines.collect::<Vec<_>>().join("\n");
    // The marker only ever stores proto terminal states:
    // JOB_STATUS_SUCCEEDED=5, JOB_STATUS_FAILED=6, JOB_STATUS_CANCELLED=7.
    let status = match status_int {
        5 => JobStatus::Succeeded,
        6 => JobStatus::Failed,
        7 => JobStatus::Cancelled,
        _ => return MarkerRead::Corrupt,
    };
    MarkerRead::Valid((status, exit_code, message))
}

async fn stream_output<R>(
    reader: &mut R,
    log_dir: &std::path::Path,
    job_id: &str,
    is_stderr: bool,
    log_offset: &AtomicI64,
    token: &str,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf_reader = tokio::io::BufReader::new(reader);
    // Truncate instead of append: a retried job re-runs with the same
    // job_id/log_dir, and the two runs' output must not interleave in the
    // local mirror (the server-side log rows are cleared on retry too).
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(log_dir.join(if is_stderr {
            "stderr.log"
        } else {
            "stdout.log"
        }))
        .await?;

    loop {
        let Some(line) = read_line_capped(&mut buf_reader, MAX_LOG_LINE_BYTES).await? else {
            break; // EOF
        };
        let offset = log_offset.fetch_add(1, Ordering::SeqCst);
        let entry = JobLogEntry {
            job_id: job_id.to_string(),
            log_data: line.clone(),
            is_stderr,
            timestamp: chrono::Utc::now().timestamp_millis(),
            log_offset: offset,
        };

        // Write to file
        use tokio::io::AsyncWriteExt;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;

        // Send to server (best effort)
        let _ = client
            .report_job_logs(crate::grpc_client::authed(
                token,
                tonic::Request::new(tokio_stream::iter(vec![entry])),
            ))
            .await;
    }

    Ok(())
}

/// Read the process start time (jiffies since boot) from `/proc/<pid>/stat`.
/// Returns `None` when the process does not exist or is not readable.
///
/// `/proc/<pid>/stat` looks like `123 (comm) S ppid ... `; the comm field
/// may contain spaces and parentheses, so parsing starts after the LAST `)`.
/// `starttime` is the 22nd field overall, i.e. index 19 after the comm
/// (index 0 of the tail is field 3, `state`).
fn process_starttime(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let after_comm = stat.rsplit(')').next()?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // Index 0 of the tail is field 3 of /proc/pid/stat (state), so
    // starttime (field 22) sits at index 19. B-wip read index 20, i.e.
    // vsize, which changes from reading to reading -- every cancellation
    // then took the "pid recycled" branch and never signalled the group.
    fields.get(19)?.parse().ok()
}

/// After the process-group leader has exited, kill any members that are
/// still alive in its group (children it spawned that did not die with it).
/// Sends SIGTERM, polls `kill(-pgid, 0)` until the group is empty (ESRCH),
/// then escalates to SIGKILL after ~5s. EPERM means the group still exists
/// (just not ours to signal), so it counts as alive and gets the escalation.
///
/// `expected_start` guards against pid recycling: the leader has just
/// exited, and a NEW process group can claim the freed pid. When a process
/// is alive at that pid with a DIFFERENT starttime, it is not ours and the
/// group must not be signalled. When no process is alive at the pid we
/// cannot distinguish "our group still alive" from "pid recycled" and fall
/// back to the (pre-existing) best-effort group signal.
async fn kill_remaining_process_group(pgid: i32, expected_start: u64) {
    if pgid <= 1 {
        return;
    }
    if expected_start > 0
        && let Some(actual) = process_starttime(pgid)
        && actual != expected_start
    {
        warn!(
            pgid,
            "Process group leader pid recycled — skipping group kill"
        );
        return;
    }
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let alive = unsafe { libc::kill(-pgid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if !alive {
            return;
        }
    }
    warn!(
        pgid,
        "Process group still alive after SIGTERM, escalating to SIGKILL"
    );
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

/// Delete job log directories (`log_dir/<job_id>/`) whose content is older
/// than `max_age` (by directory mtime). The server prunes its job_logs copy
/// after 7 days; without the local counterpart a long-lived agent would
/// accumulate job stdout/stderr files forever.
pub async fn cleanup_old_job_dirs(log_dir: &std::path::Path, max_age: chrono::Duration) {
    let Ok(mut entries) = tokio::fs::read_dir(log_dir).await else {
        return;
    };
    let max_age = max_age
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(7 * 86400));
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        match modified.elapsed() {
            Ok(age) if age > max_age => {
                if let Err(e) = tokio::fs::remove_dir_all(entry.path()).await {
                    warn!(error = %e, path = %entry.path().display(), "Failed to remove old job log dir");
                }
            }
            _ => {}
        }
    }
}

/// Kill process groups left behind by a previous agent instance (crash or
/// restart) and remove their pid markers. Each job writes
/// `log_dir/<job_id>/pid` (pid + starttime) while executing; on startup we
/// reap any that are still around so a re-dispatched `starting` job is never
/// executed twice on this node.
///
/// The recorded starttime is compared against the live process before
/// killing: a pid that was recycled by the kernel to an unrelated process
/// group has a different starttime and is left alone (only the stale marker
/// is removed). Legacy single-line pid files (no starttime) keep the old
/// best-effort behaviour.
pub async fn cleanup_orphaned_process_groups(log_dir: &std::path::Path) {
    let Ok(mut entries) = tokio::fs::read_dir(log_dir).await else {
        return; // no log dir yet — nothing to clean
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let pid_file = entry.path().join("pid");
        let Ok(contents) = tokio::fs::read_to_string(&pid_file).await else {
            continue; // not a job dir with a pid marker
        };
        let mut lines = contents.trim().lines();
        let Ok(pid) = lines.next().map(str::trim).unwrap_or("").parse::<i32>() else {
            continue;
        };
        if pid <= 1 {
            continue;
        }

        // Newer marker: pid + starttime. Only kill when the pid still refers
        // to the exact process group we left behind.
        if let Some(expected_start) = lines.next().and_then(|l| l.trim().parse::<u64>().ok()) {
            match process_starttime(pid) {
                Some(actual) if actual == expected_start => {}
                Some(_) => {
                    warn!(pid, path = %entry.path().display(), "Stale pid file points at a recycled pid — skipping kill");
                    let _ = tokio::fs::remove_file(&pid_file).await;
                    continue;
                }
                None => {
                    // Process group already gone; drop the marker.
                    let _ = tokio::fs::remove_file(&pid_file).await;
                    continue;
                }
            }
        }

        warn!(pid, path = %entry.path().display(), "Reaping orphaned process group from previous agent instance");
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        // Escalate to SIGKILL after a grace period, then drop the marker so
        // a future restart does not try to kill a recycled pid.
        let pid_file = pid_file.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = tokio::fs::remove_file(&pid_file).await;
        });
    }
}

#[cfg(test)]
mod marker_tests {
    use super::*;
    use tokio::runtime::Runtime;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cs-marker-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn marker_roundtrip_preserves_terminal_status() {
        let rt = Runtime::new().unwrap();
        let dir = temp_dir("roundtrip");
        rt.block_on(async {
            // Proto enum numbering: Succeeded=5, Failed=6, Cancelled=7.
            for (status, status_int) in [
                (JobStatus::Succeeded, 5),
                (JobStatus::Failed, 6),
                (JobStatus::Cancelled, 7),
            ] {
                write_completed_marker(&dir, 0, status, 42, "msg").await;
                let read = read_completed_marker(&dir, 0).await;
                assert_eq!(
                    read,
                    MarkerRead::Valid((status, 42, "msg".to_string())),
                    "status_int {} must round-trip to the same status",
                    status_int
                );
            }
        });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn marker_missing_and_retry_mismatch_are_missing() {
        let rt = Runtime::new().unwrap();
        let dir = temp_dir("missing");
        rt.block_on(async {
            // No file at all.
            assert!(matches!(
                read_completed_marker(&dir, 0).await,
                MarkerRead::Missing
            ));
            // A marker for a different run (retry_count bumped) is not ours.
            write_completed_marker(&dir, 1, JobStatus::Succeeded, 0, "").await;
            assert!(matches!(
                read_completed_marker(&dir, 0).await,
                MarkerRead::Missing
            ));
            // Same run after a bump is valid again.
            assert!(matches!(
                read_completed_marker(&dir, 1).await,
                MarkerRead::Valid(_)
            ));
        });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_marker_is_never_valid() {
        let rt = Runtime::new().unwrap();
        let dir = temp_dir("corrupt");
        rt.block_on(async {
            // Unknown status int (e.g. common::job numbering leaked in, or a
            // future enum value): must be Corrupt, never re-executed.
            write_completed_marker(&dir, 0, JobStatus::Succeeded, 0, "").await;
            // Overwrite with a raw unknown value (the pre-fix bug wrote 5/6/7
            // for common 4/5/6 — 4 and 6 were valid "shifted" reads; now the
            // shifted legacy reads must not be trusted for 4/6 either).
            tokio::fs::write(dir.join("done"), "0\n4\n0\nlegacy-common-succeeded\n")
                .await
                .unwrap();
            assert!(matches!(
                read_completed_marker(&dir, 0).await,
                MarkerRead::Corrupt
            ));
            // Garbage content.
            tokio::fs::write(dir.join("done"), "not-a-marker\n")
                .await
                .unwrap();
            assert!(matches!(
                read_completed_marker(&dir, 0).await,
                MarkerRead::Corrupt
            ));
        });
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Acceptance tests for `features/merge_m6_job_safety.feature` (cancellation).
///
/// The scenario names are the test function names verbatim. Both tests drive
/// the real `execute_job` path with real processes in their own process group
/// (a session of its own), so the SIGTERM -> SIGKILL escalation of `JobRuntime` is
/// exercised end to end, including the pid bookkeeping around `wait()`.
#[cfg(test)]
mod cancel_acceptance_tests {
    use super::*;
    use std::time::Duration as StdDuration;
    use std::time::Instant;

    const SIGKILL_SCENARIO_JOB: &str = "m6-cancel-sigkill";
    const SIGTERM_SCENARIO_JOB: &str = "m6-cancel-sigterm";

    fn test_config(name: &str) -> AgentConfig {
        let config = AgentConfig {
            log_dir: std::env::temp_dir().join(format!(
                "cs-cancel-{}-{}",
                name,
                uuid::Uuid::new_v4()
            )),
            agent_token: String::new(),
            ..AgentConfig::default()
        };
        std::fs::create_dir_all(&config.log_dir).expect("temp log dir");
        config
    }

    fn test_job(job_id: &str, script: &str) -> Job {
        Job {
            job_id: job_id.to_string(),
            executable: "/bin/sh".to_string(),
            arguments: vec!["-c".to_string(), script.to_string()],
            working_directory: "/tmp".to_string(),
            ..Default::default()
        }
    }

    /// A lazy channel to a port nobody listens on: every status report fails
    /// fast and is swallowed by `report_status` (best effort), so the test
    /// needs no server.
    fn lazy_client() -> AgentServiceClient<tonic::transport::Channel> {
        // Short request timeout: tonic keeps re-dialling a lazy channel
        // forever, and a hanging status report would keep the executor away
        // from  (the child would stay an unreaped zombie).
        let endpoint = tonic::transport::Channel::from_static("http://127.0.0.1:9")
            .timeout(StdDuration::from_millis(100))
            .connect_timeout(StdDuration::from_millis(100));
        AgentServiceClient::new(endpoint.connect_lazy())
    }

    fn process_alive(pid: i32) -> bool {
        pid > 0 && unsafe { libc::kill(pid, 0) } == 0
    }

    async fn wait_for_pid(runtime: &JobRuntime, job_id: &str) -> i32 {
        let deadline = Instant::now() + StdDuration::from_secs(10);
        loop {
            if let Some(&(pid, _)) = runtime.pids.lock().await.get(job_id)
                && pid > 0
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "the executor never registered a pid for {job_id}"
            );
            tokio::time::sleep(StdDuration::from_millis(50)).await;
        }
    }

    async fn wait_until_process_gone(pid: i32, within: StdDuration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if !process_alive(pid) {
                return true;
            }
            tokio::time::sleep(StdDuration::from_millis(20)).await;
        }
        !process_alive(pid)
    }

    async fn wait_until_pid_entry_gone(
        runtime: &JobRuntime,
        job_id: &str,
        within: StdDuration,
    ) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if !runtime.pids.lock().await.contains_key(job_id) {
                return true;
            }
            tokio::time::sleep(StdDuration::from_millis(20)).await;
        }
        !runtime.pids.lock().await.contains_key(job_id)
    }

    #[tokio::test]
    async fn cancelling_a_job_whose_process_ignores_sigterm_escalates_to_sigkill() {
        let config = test_config("sigkill");
        let runtime = Arc::new(JobRuntime::new());
        let job = test_job(SIGKILL_SCENARIO_JOB, "trap '' TERM; sleep 60");
        let mut client = lazy_client();

        let executor = {
            let config = config.clone();
            let runtime = runtime.clone();
            tokio::spawn(async move { execute_job(&config, job, &mut client, &runtime).await })
        };

        let pid = wait_for_pid(&runtime, SIGKILL_SCENARIO_JOB).await;
        assert!(process_alive(pid), "the fixture process must be running");

        let requested = runtime.request_cancel(SIGKILL_SCENARIO_JOB).await;
        assert!(requested, "a running job must accept the cancellation");

        // The grace period is ~5s: the process ignores SIGTERM, so it is
        // still alive well inside the window ...
        tokio::time::sleep(StdDuration::from_secs(2)).await;
        assert!(
            process_alive(pid),
            "the fixture process traps SIGTERM, so it must still be alive after 2s"
        );

        // ... and gone once the escalation fired.
        assert!(
            wait_until_process_gone(pid, StdDuration::from_secs(10)).await,
            "the process group must be SIGKILLed after the ~5s grace period"
        );
        let _ = executor.await;
        std::fs::remove_dir_all(&config.log_dir).ok();
    }

    #[tokio::test]
    async fn cancelling_a_job_that_exits_on_sigterm_does_not_wait_for_the_escalation() {
        let config = test_config("sigterm");
        let runtime = Arc::new(JobRuntime::new());
        let job = test_job(SIGTERM_SCENARIO_JOB, "sleep 60");
        let mut client = lazy_client();

        let executor = {
            let config = config.clone();
            let runtime = runtime.clone();
            tokio::spawn(async move { execute_job(&config, job, &mut client, &runtime).await })
        };

        let pid = wait_for_pid(&runtime, SIGTERM_SCENARIO_JOB).await;
        let started = Instant::now();
        let requested = runtime.request_cancel(SIGTERM_SCENARIO_JOB).await;
        assert!(requested, "a running job must accept the cancellation");

        assert!(
            wait_until_process_gone(pid, StdDuration::from_secs(1)).await,
            "a process that honours SIGTERM must exit within 1s, long before the 5s escalation"
        );
        assert!(
            started.elapsed() < StdDuration::from_secs(5),
            "the process must not have waited for the SIGKILL escalation"
        );

        assert!(
            wait_until_pid_entry_gone(&runtime, SIGTERM_SCENARIO_JOB, StdDuration::from_secs(5))
                .await,
            "the runtime must drop the process-group record once the process is reaped"
        );
        let _ = executor.await;
        std::fs::remove_dir_all(&config.log_dir).ok();
    }
}
