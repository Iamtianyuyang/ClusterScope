use anyhow::{Context, Result};
use common::config::AgentConfig;
use protocol::{AgentServiceClient, Job, JobLogEntry};
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

/// Execute a job on the agent node
pub async fn execute_job(
    config: &AgentConfig,
    job: Job,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
) -> Result<()> {
    let job_id = job.job_id.clone();
    let executable = job.executable.clone();
    let arguments = job.arguments.clone();
    let working_dir = job.working_directory.clone();
    let env: HashMap<String, String> = job.environment.clone();
    
    info!(job_id = %job_id, executable = %executable, "Starting job");
    
    // Create log directory
    let log_dir = config.log_dir.join(&job_id);
    tokio::fs::create_dir_all(&log_dir)
        .await
        .with_context(|| format!("Failed to create log dir for job {}", job_id))?;
    
    let _stdout_path = log_dir.join("stdout.log");
    let _stderr_path = log_dir.join("stderr.log");
    
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
    
    let mut process = cmd.spawn()
        .with_context(|| format!("Failed to spawn process: {}", executable))?;
    
    let pid = process.id().unwrap_or(0);
    let mut process_stdout = process.stdout.take().expect("stdout piped");
    let mut process_stderr = process.stderr.take().expect("stderr piped");
    info!(job_id = %job_id, pid = pid, "Process spawned");
    
    // Stream stdout and stderr to files and gRPC
    let stdout_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(&mut process_stdout, &log_dir, &job_id, false, &mut client).await
        })
    };
    
    let stderr_task = {
        let log_dir = log_dir.clone();
        let job_id = job_id.clone();
        let mut client = client.clone();
        tokio::spawn(async move {
            stream_output(&mut process_stderr, &log_dir, &job_id, true, &mut client).await
        })
    };
    
    // Wait for process to finish
    let wait_result = process.wait().await;
    
    match wait_result {
        Ok(status) => {
            let exit_code = status.code().unwrap_or(-1);
            info!(job_id = %job_id, pid = pid, exit_code, "Job finished");
            
            // Stop streaming
            stdout_task.abort();
            stderr_task.abort();
            
            // Wait for stream tasks to finish
            let _ = tokio::join!(stdout_task, stderr_task);
            
            // Save final log offset
            Ok(())
        }
        Err(e) => {
            error!(job_id = %job_id, pid = pid, error = %e, "Process wait failed");
            Ok(())
        }
    }
}

async fn stream_output<R>(
    reader: &mut R,
    log_dir: &PathBuf,
    job_id: &str,
    is_stderr: bool,
    client: &mut AgentServiceClient<tonic::transport::Channel>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut log_offset = 0i64;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(if is_stderr { "stderr.log" } else { "stdout.log" }))
        .await?;
    
    loop {
        let mut line = String::new();
        match buf_reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let entry = JobLogEntry {
                    job_id: job_id.to_string(),
                    log_data: line.clone(),
                    is_stderr,
                    timestamp: chrono::Utc::now().timestamp_millis() as i64,
                    log_offset,
                };
                log_offset += 1;
                
                // Write to file
                use tokio::io::AsyncWriteExt;
                file.write_all(line.as_bytes()).await?;
                
                // Send to server (best effort)
                let _ = client.report_job_logs(tokio_stream::iter(vec![entry])).await;
            }
            Err(e) => {
                warn!(error = %e, "Error reading job output");
                break;
            }
        }
    }
    
    Ok(())
}
