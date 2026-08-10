use crate::models::NodeMetricsRow;
use protocol::NodeMetricsReport;

pub fn node_metrics_to_proto(m: &NodeMetricsRow) -> NodeMetricsReport {
    NodeMetricsReport {
        node_id: m.node_id.clone(),
        sequence: m.sequence as u64,
        timestamp_ms: m.timestamp_ms as u64,
        monotonic_clock_ms: m.monotonic_clock_ms.map(|v| v as u64).unwrap_or(0),
        cpu_usage_percent: m.cpu_usage_percent.unwrap_or(0.0),
        cpu_cores: vec![],
        load_1: m.load_1.unwrap_or(0.0),
        load_5: m.load_5.unwrap_or(0.0),
        load_15: m.load_15.unwrap_or(0.0),
        memory_total_bytes: m.memory_total_bytes.unwrap_or(0) as u64,
        memory_used_bytes: m.memory_used_bytes.unwrap_or(0) as u64,
        swap_total_bytes: m.swap_total_bytes.unwrap_or(0) as u64,
        swap_used_bytes: m.swap_used_bytes.unwrap_or(0) as u64,
        uptime_seconds: m.uptime_seconds.unwrap_or(0) as u64,
        boot_time_seconds: m.boot_time_seconds.unwrap_or(0) as u64,
        disks: vec![],
        networks: vec![],
        gpus: vec![],
        gpu_processes: vec![],
    }
}
