use common::job::{Job, JobStatus};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use tracing::{info, warn};

/// GPU job scheduler for ClusterScope.
///
/// Design:
/// - `enqueue` adds jobs to a FIFO queue.
/// - `schedule` walks the queue and dispatches each job to a node that has
///   enough free GPU capacity.  Un-schedulable jobs stay in the queue and are
///   retried on the next cycle.
/// - GPU requirements are parsed from `Job::resource_quota`
///   (e.g. "gpu:2", "2", "gpus:4"); the default is 1 GPU.
pub struct Scheduler {
    job_queue: Arc<Mutex<Vec<Job>>>,
    running_jobs: Arc<Mutex<BTreeMap<String, Job>>>,
    node_gpu_capacity: Arc<Mutex<HashMap<String, u32>>>,
}

/// Parse the number of GPUs a job requires from its `resource_quota`.
///
/// Supported formats: `"2"`, `"gpu:2"`, `"gpus:4"`, `"GPU=2"`; anything
/// unparseable falls back to 1.
pub fn parse_gpu_requirement(quota: &str) -> u32 {
    let s = quota.trim().to_ascii_lowercase();
    if s.is_empty() {
        return 1;
    }
    // Extract the first integer from the string ("gpu:2" -> 2, "3" -> 3).
    let digits: String = s.chars().skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return 1;
    }
    digits.parse::<u32>().unwrap_or(1).max(1)
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            job_queue: Arc::new(Mutex::new(Vec::new())),
            running_jobs: Arc::new(Mutex::new(BTreeMap::new())),
            node_gpu_capacity: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn enqueue(&self, job: Job) {
        let mut queue = self.job_queue.lock().await;
        queue.push(job);
    }

    /// Try to dispatch queued jobs to nodes with free GPU capacity.
    /// Returns the jobs that were scheduled in this cycle.
    pub async fn schedule(&self) -> Vec<Job> {
        let mut queue = self.job_queue.lock().await;
        let mut scheduled = Vec::new();
        let mut remaining = Vec::new();

        for job in queue.drain(..) {
            let gpu_required = parse_gpu_requirement(&job.resource_quota);
            match self.find_node_for_job(gpu_required).await {
                Some(node_id) => {
                    let mut job = job;
                    job.node_id = node_id.clone();
                    job.status = JobStatus::Starting;
                    let mut running = self.running_jobs.lock().await;
                    running.insert(job.job_id.clone(), job.clone());
                    info!(
                        job_id = %job.job_id,
                        node_id = %node_id,
                        gpus = gpu_required,
                        "Job scheduled"
                    );
                    scheduled.push(job);
                }
                None => {
                    // Not enough capacity anywhere: keep for next cycle.
                    remaining.push(job);
                }
            }
        }

        *queue = remaining;
        scheduled
    }

    pub async fn get_running_jobs(&self) -> Vec<Job> {
        self.running_jobs.lock().await.values().cloned().collect()
    }

    pub async fn complete_job(&self, job_id: &str, status: JobStatus) {
        let mut running = self.running_jobs.lock().await;
        if let Some(mut job) = running.remove(job_id) {
            job.transition_to(status).ok();
            info!(job_id = %job_id, status = ?status, "Job completed");
        }
    }

    pub async fn set_node_gpu_capacity(&self, node_id: &str, capacity: u32) {
        self.node_gpu_capacity.lock().await.insert(node_id.to_string(), capacity);
    }

    /// GPUs currently in use on `node_id` (sum of running jobs' requirements).
    async fn gpus_in_use(&self, node_id: &str) -> u32 {
        let running = self.running_jobs.lock().await;
        running.values()
            .filter(|j| j.node_id == node_id)
            .map(|j| parse_gpu_requirement(&j.resource_quota))
            .sum()
    }

    pub async fn get_available_gpu_count(&self, node_id: &str) -> u32 {
        let capacity = self.node_gpu_capacity.lock().await.get(node_id).copied().unwrap_or(0);
        let used = self.gpus_in_use(node_id).await;
        capacity.saturating_sub(used)
    }

    /// Pick the alphabetically-first node with enough free capacity for a job
    /// requiring `gpu_required` GPUs. Returns `None` if no node qualifies.
    pub async fn find_node_for_job(&self, gpu_required: u32) -> Option<String> {
        let running = self.running_jobs.lock().await;
        let capacity = self.node_gpu_capacity.lock().await;

        let mut candidates: Vec<&String> = capacity.iter()
            .filter(|(node, cap)| {
                let used: u32 = running.values()
                    .filter(|j| j.node_id == **node)
                    .map(|j| parse_gpu_requirement(&j.resource_quota))
                    .sum();
                cap.saturating_sub(used) >= gpu_required
            })
            .map(|(n, _)| n)
            .collect();

        candidates.sort();
        candidates.into_iter().next().cloned()
    }
}

/// Run the scheduler loop.
pub async fn run_scheduler(scheduler: Arc<Scheduler>, database: storage::DatabasePool) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        // Load pending jobs from database
        if let Ok((jobs, _)) = storage::job_queries::list_jobs(
            database.pool(),
            None,
            Some("queued"),
            None,
            0,
            100,
        ).await {
            for job in jobs {
                let job = Job {
                    job_id: job.job_id.clone(),
                    node_id: job.node_id.clone(),
                    name: job.name.clone(),
                    executable: job.executable.clone(),
                    arguments: serde_json::from_value(job.arguments).unwrap_or_default(),
                    working_directory: job.working_directory.clone(),
                    environment: serde_json::from_value(job.environment).unwrap_or_default(),
                    status: JobStatus::Queued,
                    pid: None,
                    exit_code: None,
                    error_message: None,
                    created_at: job.created_at,
                    started_at: job.started_at,
                    finished_at: job.finished_at,
                    created_by: job.created_by.clone(),
                    resource_quota: job.resource_quota.unwrap_or_default(),
                    retry_count: job.retry_count as u32,
                    max_retries: job.max_retries as u32,
                };
                scheduler.enqueue(job).await;
            }
        }

        // Try to schedule jobs
        let scheduled = scheduler.schedule().await;
        if scheduled.is_empty() {
            warn!("No jobs scheduled in this cycle");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_job(id: &str, node: &str) -> Job {
        Job {
            job_id: id.to_string(),
            node_id: node.to_string(),
            name: format!("test-job-{}", id),
            executable: "/bin/test".to_string(),
            arguments: vec![],
            working_directory: "/tmp".to_string(),
            environment: HashMap::new(),
            status: JobStatus::Queued,
            pid: None,
            exit_code: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: None,
            created_by: "test".to_string(),
            resource_quota: String::new(),
            retry_count: 0,
            max_retries: 0,
        }
    }

    fn make_test_job_with_quota(id: &str, node: &str, quota: &str) -> Job {
        let mut job = make_test_job(id, node);
        job.resource_quota = quota.to_string();
        job
    }

    #[test]
    fn test_parse_gpu_requirement() {
        assert_eq!(parse_gpu_requirement(""), 1);
        assert_eq!(parse_gpu_requirement("gpu:2"), 2);
        assert_eq!(parse_gpu_requirement("gpus:4"), 4);
        assert_eq!(parse_gpu_requirement("GPU=8"), 8);
        assert_eq!(parse_gpu_requirement("3"), 3);
        assert_eq!(parse_gpu_requirement("bogus"), 1);
        assert_eq!(parse_gpu_requirement("0"), 1);
    }

    #[tokio::test]
    async fn test_enqueue_and_schedule() {
        let scheduler = Scheduler::new();
        scheduler.set_node_gpu_capacity("node-1", 4).await;
        scheduler.set_node_gpu_capacity("node-2", 4).await;

        scheduler.enqueue(make_test_job("job-1", "node-1")).await;
        scheduler.enqueue(make_test_job("job-2", "node-2")).await;

        let scheduled = scheduler.schedule().await;
        assert_eq!(scheduled.len(), 2);

        let running = scheduler.get_running_jobs().await;
        assert_eq!(running.len(), 2);
    }

    #[tokio::test]
    async fn test_complete_job() {
        let scheduler = Scheduler::new();
        scheduler.set_node_gpu_capacity("node-1", 4).await;
        scheduler.enqueue(make_test_job("job-1", "node-1")).await;
        scheduler.schedule().await;

        scheduler.complete_job("job-1", JobStatus::Succeeded).await;
        let running = scheduler.get_running_jobs().await;
        assert_eq!(running.len(), 0);
    }

    #[tokio::test]
    async fn test_schedule_respects_capacity() {
        let scheduler = Scheduler::new();
        scheduler.set_node_gpu_capacity("node-1", 2).await;

        // Two 2-GPU jobs on a 2-GPU node: only the first can run.
        scheduler.enqueue(make_test_job_with_quota("job-1", "", "gpu:2")).await;
        scheduler.enqueue(make_test_job_with_quota("job-2", "", "gpu:2")).await;

        let scheduled = scheduler.schedule().await;
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].job_id, "job-1");
        assert_eq!(scheduled[0].node_id, "node-1");

        // Second job stays queued.
        let running = scheduler.get_running_jobs().await;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].job_id, "job-1");
    }

    #[tokio::test]
    async fn test_schedule_picks_node_with_capacity() {
        let scheduler = Scheduler::new();
        scheduler.set_node_gpu_capacity("node-a", 1).await;
        scheduler.set_node_gpu_capacity("node-b", 4).await;

        // node-a is alphabetically first but too small for a 2-GPU job.
        scheduler.enqueue(make_test_job_with_quota("job-1", "", "gpu:2")).await;
        let scheduled = scheduler.schedule().await;

        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].node_id, "node-b");
    }

    #[tokio::test]
    async fn test_gpu_capacity_freed_after_completion() {
        let scheduler = Scheduler::new();
        scheduler.set_node_gpu_capacity("node-1", 2).await;

        scheduler.enqueue(make_test_job_with_quota("job-1", "", "gpu:2")).await;
        scheduler.schedule().await;
        assert_eq!(scheduler.get_available_gpu_count("node-1").await, 0);

        scheduler.complete_job("job-1", JobStatus::Succeeded).await;
        assert_eq!(scheduler.get_available_gpu_count("node-1").await, 2);

        // The queued second job can now run.
        scheduler.enqueue(make_test_job_with_quota("job-2", "", "gpu:2")).await;
        let scheduled = scheduler.schedule().await;
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].job_id, "job-2");
    }
}
