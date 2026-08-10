use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum JobStatus {
    Queued = 0,
    Starting = 1,
    Running = 2,
    Stopping = 3,
    Succeeded = 4,
    Failed = 5,
    Cancelled = 6,
    Lost = 7,
}

impl JobStatus {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
    
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Queued),
            1 => Some(Self::Starting),
            2 => Some(Self::Running),
            3 => Some(Self::Stopping),
            4 => Some(Self::Succeeded),
            5 => Some(Self::Failed),
            6 => Some(Self::Cancelled),
            7 => Some(Self::Lost),
            _ => None,
        }
    }
    
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Lost)
    }
    
    pub fn is_running(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

impl Default for JobStatus {
    fn default() -> Self {
        Self::Queued
    }
}

// Valid state transitions
const VALID_TRANSITIONS: &[(JobStatus, JobStatus)] = &[
    (JobStatus::Queued, JobStatus::Starting),
    (JobStatus::Queued, JobStatus::Cancelled),
    (JobStatus::Queued, JobStatus::Failed),
    (JobStatus::Starting, JobStatus::Running),
    (JobStatus::Starting, JobStatus::Failed),
    (JobStatus::Starting, JobStatus::Cancelled),
    (JobStatus::Running, JobStatus::Stopping),
    (JobStatus::Running, JobStatus::Succeeded),
    (JobStatus::Running, JobStatus::Failed),
    (JobStatus::Running, JobStatus::Cancelled),
    (JobStatus::Running, JobStatus::Lost),
    (JobStatus::Stopping, JobStatus::Succeeded),
    (JobStatus::Stopping, JobStatus::Failed),
    (JobStatus::Stopping, JobStatus::Cancelled),
    (JobStatus::Stopping, JobStatus::Lost),
    (JobStatus::Succeeded, JobStatus::Queued),  // retry
    (JobStatus::Failed, JobStatus::Queued),     // retry
    (JobStatus::Cancelled, JobStatus::Queued),  // retry
];

pub fn can_transition(from: JobStatus, to: JobStatus) -> bool {
    VALID_TRANSITIONS.contains(&(from, to))
}

pub fn validate_transition(from: JobStatus, to: JobStatus) -> Result<(), AppError> {
    if can_transition(from, to) {
        Ok(())
    } else {
        Err(AppError::bad_request(format!(
            "invalid job state transition: {:?} -> {:?}",
            from, to
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    pub job_id: String,
    pub node_id: String,
    pub name: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: HashMap<String, String>,
    pub max_concurrent: u32,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub description: String,
    pub max_retries: u32,
}

impl JobDefinition {
    pub fn new(
        node_id: String,
        name: String,
        executable: String,
        arguments: Vec<String>,
        working_directory: String,
        environment: HashMap<String, String>,
        created_by: String,
        description: String,
        max_retries: u32,
    ) -> Self {
        Self {
            job_id: Uuid::new_v4().to_string(),
            node_id,
            name,
            executable,
            arguments,
            working_directory,
            environment,
            max_concurrent: 1,
            created_at: Utc::now(),
            created_by,
            description,
            max_retries,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id: String,
    pub node_id: String,
    pub name: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: HashMap<String, String>,
    pub status: JobStatus,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub resource_quota: String,
    pub retry_count: u32,
    pub max_retries: u32,
}

impl Job {
    pub fn from_definition(def: &JobDefinition) -> Self {
        Self {
            job_id: def.job_id.clone(),
            node_id: def.node_id.clone(),
            name: def.name.clone(),
            executable: def.executable.clone(),
            arguments: def.arguments.clone(),
            working_directory: def.working_directory.clone(),
            environment: def.environment.clone(),
            status: JobStatus::Queued,
            pid: None,
            exit_code: None,
            error_message: None,
            created_at: def.created_at,
            started_at: None,
            finished_at: None,
            created_by: def.created_by.clone(),
            resource_quota: String::new(),
            retry_count: 0,
            max_retries: def.max_retries,
        }
    }
    
    pub fn transition_to(&mut self, new_status: JobStatus) -> Result<(), AppError> {
        validate_transition(self.status, new_status)?;
        self.status = new_status;
        
        match new_status {
            JobStatus::Starting | JobStatus::Running => {
                self.started_at = Some(Utc::now());
            }
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled | JobStatus::Lost => {
                self.finished_at = Some(Utc::now());
            }
            _ => {}
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobLogEntry {
    pub job_id: String,
    pub log_data: String,
    pub is_stderr: bool,
    pub timestamp: DateTime<Utc>,
    pub log_offset: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_transitions() {
        assert!(can_transition(JobStatus::Queued, JobStatus::Starting));
        assert!(can_transition(JobStatus::Starting, JobStatus::Running));
        assert!(can_transition(JobStatus::Running, JobStatus::Stopping));
        assert!(can_transition(JobStatus::Stopping, JobStatus::Succeeded));
        
        // Terminal states can't go anywhere except retry
        assert!(!can_transition(JobStatus::Succeeded, JobStatus::Running));
        assert!(can_transition(JobStatus::Succeeded, JobStatus::Queued));
    }
    
    #[test]
    fn test_invalid_transitions() {
        assert!(!can_transition(JobStatus::Running, JobStatus::Queued));
        assert!(!can_transition(JobStatus::Succeeded, JobStatus::Failed));
        assert!(!can_transition(JobStatus::Queued, JobStatus::Running));
    }
    
    #[test]
    fn test_transition_validation() {
        assert!(validate_transition(JobStatus::Queued, JobStatus::Starting).is_ok());
        assert!(validate_transition(JobStatus::Queued, JobStatus::Running).is_err());
    }
    
    #[test]
    fn test_job_status_is_terminal() {
        assert!(JobStatus::Succeeded.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(JobStatus::Lost.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(!JobStatus::Starting.is_terminal());
    }
    
    #[test]
    fn test_job_creation_from_definition() {
        let mut env = HashMap::new();
        env.insert("CUDA_VISIBLE_DEVICES".to_string(), "0".to_string());
        
        let def = JobDefinition {
            job_id: "job-1".to_string(),
            node_id: "node-1".to_string(),
            name: "test-job".to_string(),
            executable: "/usr/bin/test".to_string(),
            arguments: vec!["--flag".to_string()],
            working_directory: "/tmp".to_string(),
            environment: env,
            max_concurrent: 1,
            created_at: Utc::now(),
            created_by: "admin".to_string(),
            description: "test".to_string(),
            max_retries: 3,
        };
        
        let job = Job::from_definition(&def);
        assert_eq!(job.job_id, def.job_id);
        assert_eq!(job.status, JobStatus::Queued);
        assert!(job.pid.is_none());
        assert!(job.started_at.is_none());
    }
    
    #[test]
    fn test_job_transition_sets_timestamps() {
        let mut job = Job::from_definition(&JobDefinition {
            job_id: "job-1".to_string(),
            node_id: "node-1".to_string(),
            name: "test".to_string(),
            executable: "/bin/test".to_string(),
            arguments: vec![],
            working_directory: "/tmp".to_string(),
            environment: HashMap::new(),
            max_concurrent: 1,
            created_at: Utc::now(),
            created_by: "admin".to_string(),
            description: String::new(),
            max_retries: 0,
        });
        
        job.transition_to(JobStatus::Starting).unwrap();
        assert!(job.started_at.is_some());
        
        job.transition_to(JobStatus::Running).unwrap();
        assert!(job.started_at.is_some());
        
        job.transition_to(JobStatus::Succeeded).unwrap();
        assert!(job.started_at.is_some());
        assert!(job.finished_at.is_some());
    }
}
