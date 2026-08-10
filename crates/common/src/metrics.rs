use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f64,
    pub cores: Vec<CpuCoreMetrics>,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCoreMetrics {
    pub core_id: u32,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub interface_name: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub packets_sent: u64,
    pub packets_recv: u64,
    pub errors_in: u64,
    pub errors_out: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuMetrics {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    pub utilization_gpu: f32,
    pub utilization_memory: f32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub temperature_celsius: f32,
    pub power_watts: f32,
    pub power_limit_watts: f32,
    pub fan_speed_percent: Option<f32>,
    pub pcie_tx_bytes_per_second: Option<u64>,
    pub pcie_rx_bytes_per_second: Option<u64>,
    pub mig_enabled: bool,
    pub mig_instances: Vec<MigInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigInstance {
    pub gpu_index: u32,
    pub uuid: String,
    pub name: String,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub utilization_gpu: f32,
    pub utilization_memory: f32,
    pub temperature_celsius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProcess {
    pub pid: u32,
    pub username: String,
    pub command: String,
    pub gpu_uuid: String,
    pub gpu_memory_bytes: u64,
    pub cpu_percent: f32,
    pub system_memory_bytes: u64,
    pub started_at: i64,
    pub container_name: Option<String>,
    pub gpu_indices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub hostname: String,
    pub os_info: String,
    pub kernel_version: String,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub disks: Vec<DiskMetrics>,
    pub networks: Vec<NetworkMetrics>,
    pub uptime_seconds: u64,
    pub boot_time_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetricsSnapshot {
    pub node_id: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub monotonic_clock_ms: u64,
    pub system: SystemMetrics,
    pub gpus: Vec<GpuMetrics>,
    pub gpu_processes: Vec<GpuProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsAggregation {
    pub metric_name: String,
    pub avg: f64,
    pub max: f64,
    pub min: f64,
    pub p95: f64,
    pub count: u64,
    pub start_time_ms: u64,
    pub end_time_ms: u64,
}

impl MetricsAggregation {
    pub fn new(metric_name: String, values: &[f64], start_time_ms: u64, end_time_ms: u64) -> Self {
        if values.is_empty() {
            return Self {
                metric_name,
                avg: 0.0,
                max: 0.0,
                min: 0.0,
                p95: 0.0,
                count: 0,
                start_time_ms,
                end_time_ms,
            };
        }
        
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let sum: f64 = values.iter().sum();
        let avg = sum / values.len() as f64;
        let max = sorted.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = sorted.iter().cloned().fold(f64::INFINITY, f64::min);
        
        let p95_idx = ((sorted.len() as f64) * 0.95) as usize;
        let p95 = sorted.get(p95_idx.clamp(0, sorted.len() - 1)).copied().unwrap_or(0.0);
        
        Self {
            metric_name,
            avg,
            max,
            min,
            p95,
            count: values.len() as u64,
            start_time_ms,
            end_time_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_aggregation() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
        let agg = MetricsAggregation::new("gpu_utilization".to_string(), &values, 0, 1000);
        
        assert_eq!(agg.count, 10);
        assert!((agg.avg - 55.0).abs() < f64::EPSILON);
        assert_eq!(agg.max, 100.0);
        assert_eq!(agg.min, 10.0);
    }
    
    #[test]
    fn test_empty_aggregation() {
        let agg = MetricsAggregation::new("test".to_string(), &[], 0, 1000);
        assert_eq!(agg.count, 0);
        assert_eq!(agg.avg, 0.0);
        assert_eq!(agg.max, 0.0);
    }
}
