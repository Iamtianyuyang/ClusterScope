use anyhow::Result;
use chrono::Utc;
use common::config::AgentConfig;
use protocol::NodeMetricsReport;
use std::collections::HashMap;
use std::process::Command;
use nvml_wrapper::Nvml;
use nvml_wrapper::enums::device::UsedGpuMemory;
use tracing::warn;

/// Lazily-initialized NVML handle (once per process). `None` when the
/// driver/NVML is unavailable — callers must fall back gracefully.
static NVML: std::sync::OnceLock<Option<Nvml>> = std::sync::OnceLock::new();

fn nvml() -> Option<&'static Nvml> {
    NVML.get_or_init(|| {
        match Nvml::init() {
            Ok(nvml) => {
                warn!("NVML initialized");
                Some(nvml)
            }
            Err(e) => {
                warn!(error = %e, "NVML init failed — falling back to nvidia-smi");
                None
            }
        }
    }).as_ref()
}

pub struct MetricsCollector {
    prev_network: Option<HashMap<String, NetworkCounter>>,
    prev_disk: Option<HashMap<String, DiskCounter>>,
}

struct NetworkCounter {
    bytes_sent: u64,
    bytes_recv: u64,
}

struct DiskCounter {
    total_bytes: u64,
    free_bytes: u64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            prev_network: None,
            prev_disk: None,
        }
    }
    
    pub fn collect(&mut self, config: &AgentConfig) -> Result<NodeMetricsReport> {
        let now_ms = Utc::now().timestamp_millis() as u64;
        
        let mut report = NodeMetricsReport {
            node_id: config.node_id.clone().unwrap_or_default(),
            sequence: 0, // Will be set by client
            timestamp_ms: now_ms,
            monotonic_clock_ms: monotonic_clock_ms(),
            cpu_usage_percent: 0.0,
            cpu_cores: vec![],
            load_1: 0.0,
            load_5: 0.0,
            load_15: 0.0,
            memory_total_bytes: 0,
            memory_used_bytes: 0,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            uptime_seconds: 0,
            boot_time_seconds: 0,
            disks: vec![],
            networks: vec![],
            gpus: vec![],
            gpu_processes: vec![],
        };
        
        // Collect each category independently so one failure doesn't affect others
        if let Err(e) = self.collect_system(&mut report) {
            warn!(error = %e, "Failed to collect system metrics");
        }
        
        if let Err(e) = self.collect_gpu(&mut report) {
            warn!(error = %e, "Failed to collect GPU metrics");
        }
        
        if let Err(e) = self.collect_gpu_processes(config, &mut report) {
            warn!(error = %e, "Failed to collect GPU processes");
        }
        
        Ok(report)
    }
    
    fn collect_system(&mut self, report: &mut NodeMetricsReport) -> Result<()> {
        let sys = sysinfo::System::new_all();
        
        // CPU usage
        let total_usage = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32;
        report.cpu_usage_percent = total_usage as f64 / 100.0;
        
        // CPU cores
        for (i, cpu) in sys.cpus().iter().enumerate() {
            report.cpu_cores.push(protocol::CpuCoreMetrics {
                core_id: i as u32,
                usage_percent: (cpu.cpu_usage() / 100.0) as f32,
            });
        }
        
        // Load average
        if let Ok(loads) = get_load_average() {
            report.load_1 = loads.0;
            report.load_5 = loads.1;
            report.load_15 = loads.2;
        }
        
        // Memory
        let total = sys.total_memory();
        let used = sys.used_memory();
        report.memory_total_bytes = total as u64;
        report.memory_used_bytes = used as u64;
        report.swap_total_bytes = sys.total_swap() as u64;
        report.swap_used_bytes = sys.used_swap() as u64;
        
        // Uptime
        report.uptime_seconds = sysinfo::System::uptime();
        report.boot_time_seconds = sysinfo::System::boot_time();
        
        // Disks
        self.collect_disks(report);
        
        // Network
        self.collect_network(report);
        
        Ok(())
    }
    
    fn collect_disks(&mut self, report: &mut NodeMetricsReport) {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        
        let mut current = HashMap::new();
        
        for disk in &disks {
            let usage = disk.total_space();
            let available = disk.available_space();
            let used = usage - available;
            let usage_pct = if usage > 0 { (used as f64 / usage as f64) * 100.0 } else { 0.0 };
            
            let disk_metrics = protocol::DiskMetrics {
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                total_bytes: usage,
                used_bytes: used,
                free_bytes: available,
                usage_percent: usage_pct as f32,
            };
            
            if let Some(prev) = self.prev_disk.as_ref().and_then(|m| m.get(&disk_metrics.mount_point)) {
                let time_diff = (Utc::now().timestamp() - (prev.total_bytes / 1_000_000) as i64) as f64;
                if time_diff > 0.0 {
                    // Disk usage is cumulative, don't compute rate
                }
            }
            
            current.insert(
                disk_metrics.mount_point.clone(),
                DiskCounter { total_bytes: usage, free_bytes: available },
            );
            report.disks.push(disk_metrics);
        }
        
        self.prev_disk = Some(current);
    }
    
    fn collect_network(&mut self, report: &mut NodeMetricsReport) {
        let network_data = read_network_stats();
        
        let mut current = HashMap::new();
        
        for (iface, stats) in network_data.iter() {
            let mut net = protocol::NetworkMetrics {
                interface_name: iface.clone(),
                bytes_sent: stats.bytes_sent,
                bytes_recv: stats.bytes_recv,
                packets_sent: stats.packets_sent,
                packets_recv: stats.packets_recv,
                errors_in: 0,
                errors_out: 0,
                drops_in: 0,
                drops_out: 0,
                tx_rate_bytes_per_sec: None,
                rx_rate_bytes_per_sec: None,
            };
            
            // Calculate rates
            if let Some(prev) = self.prev_network.as_ref().and_then(|m| m.get(iface)) {
                let now = Utc::now().timestamp();
                let diff = now - (prev.bytes_sent / 1_000_000) as i64;
                if diff > 0 {
                    let tx_rate = (stats.bytes_sent as i64 - prev.bytes_sent as i64) / diff;
                    let rx_rate = (stats.bytes_recv as i64 - prev.bytes_recv as i64) / diff;
                    net.tx_rate_bytes_per_sec = Some(tx_rate.max(0) as f32);
                    net.rx_rate_bytes_per_sec = Some(rx_rate.max(0) as f32);
                }
            }
            
            report.networks.push(net);
            current.insert(iface.clone(), NetworkCounter {
                bytes_sent: stats.bytes_sent,
                bytes_recv: stats.bytes_recv,
            });
        }
        
        self.prev_network = Some(current);
    }
    
    fn collect_gpu(&self, report: &mut NodeMetricsReport) -> Result<()> {
        // Parse nvidia-smi output
        let output = Command::new("nvidia-smi")
            .args(["--query-gpu=index,uuid,name,utilization.gpu,utilization.memory,memory.total,memory.used,temperature.gpu,power.draw,power.limit,fan.speed,pcie.link.gen.current,pcie.link.width.current",
                   "--format=csv,noheader,nounits"])
            .output();
        
        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if fields.len() < 10 {
                        continue;
                    }
                    
                    let mut gpu = protocol::GpuMetrics {
                        index: fields[0].parse().unwrap_or(0),
                        uuid: fields[1].to_string(),
                        name: fields[2].to_string(),
                        utilization_gpu: fields[3].parse().unwrap_or(0.0),
                        utilization_memory: fields[4].parse().unwrap_or(0.0),
                        memory_total_bytes: parse_bytes(fields[5]),
                        memory_used_bytes: parse_bytes(fields[6]),
                        temperature_celsius: fields[7].parse().unwrap_or(0.0),
                        power_watts: fields[8].parse().unwrap_or(0.0),
                        power_limit_watts: fields[9].parse().unwrap_or(0.0),
                        fan_speed_percent: fields.get(10).and_then(|s| s.parse().ok()),
                        pcie_tx_bytes_per_second: None,
                        pcie_rx_bytes_per_second: None,
                        mig_enabled: false,
                        mig_instances: vec![],
                    };
                    
                    // Read PCIe counters from /sys
                    self.read_pcie_stats(&mut gpu);
                    
                    report.gpus.push(gpu);
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    fn read_pcie_stats(&self, gpu: &mut protocol::GpuMetrics) {
        let pci_path = format!(
            "/sys/bus/pci/devices/0000:{:02x}:00.0/",
            gpu.index
        );
        
        let tx_bytes = std::fs::read_to_string(format!("{}rx_bytes", pci_path)).ok();
        let rx_bytes = std::fs::read_to_string(format!("{}tx_byte", pci_path)).ok();
        
        if let Some(tx_str) = tx_bytes {
            if let Ok(tx) = tx_str.trim().parse::<u64>() {
                gpu.pcie_tx_bytes_per_second = Some(tx);
            }
        }
        if let Some(rx_str) = rx_bytes {
            if let Ok(rx) = rx_str.trim().parse::<u64>() {
                gpu.pcie_rx_bytes_per_second = Some(rx);
            }
        }
    }
    
    fn collect_gpu_processes(&self, _config: &AgentConfig, report: &mut NodeMetricsReport) -> Result<()> {
        // Primary path: NVML (no root, no nvidia-smi text parsing).
        if let Some(nvml) = nvml() {
            if self.collect_gpu_processes_nvml(nvml, report).is_ok() {
                return Ok(());
            }
            warn!("NVML process collection failed — falling back to nvidia-smi");
        }
        self.collect_gpu_processes_fallback(report)
    }

    /// NVML-based GPU process collection: pid + used VRAM per device, plus
    /// per-process SM/memory utilization samples when the driver supports it.
    fn collect_gpu_processes_nvml(
        &self,
        nvml: &Nvml,
        report: &mut NodeMetricsReport,
    ) -> Result<()> {
        let count = nvml.device_count()?;
        for idx in 0..count {
            let device = nvml.device_by_index(idx)?;
            let uuid = device.uuid().unwrap_or_default();
            // Utilization samples (last sample per PID; unsupported -> empty).
            let samples = device.process_utilization_stats(None).unwrap_or_default();

            let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
            for p in device.running_compute_processes().unwrap_or_default() {
                if !seen.insert(p.pid) {
                    continue; // same PID may appear in graphics + compute lists
                }
                let (username, command) = process_user_command(p.pid);
                let sample = samples.iter().find(|s| s.pid == p.pid);
                report.gpu_processes.push(protocol::GpuProcess {
                    pid: p.pid,
                    username,
                    command,
                    gpu_uuid: uuid.clone(),
                    gpu_memory_bytes: match p.used_gpu_memory {
                        UsedGpuMemory::Used(b) => b,
                        UsedGpuMemory::Unavailable => 0,
                    },
                    cpu_percent: 0.0,
                    system_memory_bytes: 0,
                    started_at: 0,
                    container_name: String::new(),
                    gpu_indices: vec![],
                    sm_utilization: sample.map(|s| s.sm_util as f32),
                    memory_utilization: sample.map(|s| s.mem_util as f32),
                    encoder_utilization: sample.map(|s| s.enc_util as f32),
                    decoder_utilization: sample.map(|s| s.dec_util as f32),
                });
            }
        }
        Ok(())
    }

    /// Fallback: nvidia-smi compute-apps query (kept for drivers without NVML).
    fn collect_gpu_processes_fallback(&self, report: &mut NodeMetricsReport) -> Result<()> {
        // Use nvidia-smi to get GPU processes
        let output = Command::new("nvidia-smi")
            .args(["--query-gpu=index,uuid", "--format=csv,noheader,nounits"])
            .output();
        
        let gpu_uuids = match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut uuids = vec![];
                for line in stdout.lines() {
                    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if fields.len() >= 2 {
                        uuids.push((fields[0].parse::<u32>().unwrap_or(0), fields[1].to_string()));
                    }
                }
                uuids
            }
            _ => return Ok(()),
        };
        
        let mut seen_pids = std::collections::HashSet::new();
        
        for (_gpu_idx, gpu_uuid) in &gpu_uuids {
            // Query per-GPU processes
            let output = Command::new("nvidia-smi")
                .args([
                    "-i", gpu_uuid,
                    "--query-compute-apps=pid,used_memory,gpu_uuid",
                    "--format=csv,noheader,nounits",
                ])
                .output();
            
            if let Ok(output) = output {
                if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
                    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if fields.len() < 2 {
                        continue;
                    }
                    
                    let pid = fields[0].parse::<u32>().ok().filter(|&p| p > 0);
                    if let Some(pid) = pid {
                        if seen_pids.contains(&pid) {
                            continue;
                        }
                        seen_pids.insert(pid);
                        
                        let (username, command) = process_user_command(pid);

                        report.gpu_processes.push(protocol::GpuProcess {
                            pid,
                            username,
                            command,
                            gpu_uuid: gpu_uuid.clone(),
                            gpu_memory_bytes: fields[1].trim().parse().unwrap_or(0) * 1024 * 1024,
                            cpu_percent: 0.0,
                            system_memory_bytes: 0,
                            started_at: 0,
                            container_name: String::new(),
                            gpu_indices: vec![],
                            sm_utilization: None,
                            memory_utilization: None,
                            encoder_utilization: None,
                            decoder_utilization: None,
                        });
                    }
                }
                }
            }
        }
        
        Ok(())
    }
}

fn parse_bytes(field: &str) -> u64 {
    // nvidia-smi returns values in MiB for memory
    field.trim().parse::<u64>().unwrap_or(0) * 1024 * 1024
}

fn get_load_average() -> Result<(f64, f64, f64)> {
    let contents = std::fs::read_to_string("/proc/loadavg")?;
    let parts: Vec<&str> = contents.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok((0.0, 0.0, 0.0));
    }
    Ok((
        parts[0].parse()?,
        parts[1].parse()?,
        parts[2].parse()?,
    ))
}

struct NetworkStats {
    bytes_sent: u64,
    bytes_recv: u64,
    packets_sent: u64,
    packets_recv: u64,
}

fn read_network_stats() -> HashMap<String, NetworkStats> {
    let mut stats = HashMap::new();
    
    if let Ok(contents) = std::fs::read_to_string("/proc/net/dev") {
        for line in contents.lines().skip(2) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 17 {
                continue;
            }
            
            let iface = parts[0].trim_end_matches(':').to_string();
            stats.insert(iface, NetworkStats {
                bytes_sent: parts[9].parse().unwrap_or(0),
                bytes_recv: parts[1].parse().unwrap_or(0),
                packets_sent: parts[10].parse().unwrap_or(0),
                packets_recv: parts[2].parse().unwrap_or(0),
            });
        }
    }
    
    stats
}

/// Best-effort lookup of USER and COMMAND for a PID via /proc.
/// Never fails: on permission errors (hidepid, other users) or if the
/// process has already exited, returns ("unknown"/"?" , "unknown").
fn process_user_command(pid: u32) -> (String, String) {
    let mut username = "unknown".to_string();
    if let Ok(status) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
        for line in status.lines() {
            if line.starts_with("Uid:") {
                let uid: u32 = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                username = uid_to_username(uid).unwrap_or_else(|| "?".to_string());
                break;
            }
        }
    }

    let mut command = "unknown".to_string();
    if let Ok(bytes) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
        let text = String::from_utf8_lossy(&bytes);
        let parts: Vec<String> = text
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if let Some(first) = parts.first() {
            let base = std::path::Path::new(first)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| first.to_string());
            // training scripts often look like: python train.py --config ...
            let script = parts
                .iter()
                .skip(1)
                .find(|p| p.ends_with(".py"))
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|s| s.to_string_lossy().to_string());
            command = match script {
                Some(s) => format!("{} {}", base, s),
                None => base,
            };
        }
    }

    (username, command)
}

struct ProcessInfo {
    username: String,
    command: String,
    cpu_percent: f32,
    system_memory_bytes: u64,
    started_at: i64,
}

impl ProcessInfo {
    /// Placeholder used when process details are not collected (no root needed).
    fn unknown() -> Self {
        Self {
            username: "unknown".to_string(),
            command: "unknown".to_string(),
            cpu_percent: 0.0,
            system_memory_bytes: 0,
            started_at: 0,
        }
    }
}

fn get_process_info(pid: u32) -> ProcessInfo {
    let mut info = ProcessInfo {
        username: "unknown".to_string(),
        command: "unknown".to_string(),
        cpu_percent: 0.0,
        system_memory_bytes: 0,
        started_at: 0,
    };
    
    let proc_path = format!("/proc/{}", pid);
    
    // Get command
    if let Ok(cmd) = std::fs::read(format!("{}/cmdline", proc_path)) {
        let cmd_str = cmd.into_iter()
            .filter(|&b| b != 0)
            .collect::<Vec<u8>>();
        info.command = String::from_utf8_lossy(&cmd_str)
            .to_string()
            .replace("\0", " ")
            .trim()
            .to_string();
    }
    
    // Get status for username and memory
    if let Ok(status) = std::fs::read_to_string(format!("{}/status", proc_path)) {
        for line in status.lines() {
            if line.starts_with("Uid:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() > 1 {
                    if let Ok(uid) = parts[1].parse::<u32>() {
                        info.username = uid_to_username(uid).unwrap_or_else(|| "unknown".to_string());
                    }
                }
            } else if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        info.system_memory_bytes = kb * 1024;
                    }
                }
            } else if line.starts_with("Time:") {
                // utime + stime in clock ticks
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(utime) = parts[1].parse::<i64>() {
                        let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                        if clk_tck > 0 {
                            info.started_at = Utc::now().timestamp() - (utime / clk_tck as i64);
                        }
                    }
                }
            }
        }
    }
    
    // Get CPU percent from /proc/[pid]/stat
    if let Ok(stat) = std::fs::read_to_string(format!("{}/stat", proc_path)) {
        if let Some(last_paren) = stat.rfind(')') {
            let rest = &stat[last_paren + 2..];
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 11 {
                // utime (index 11 in the rest, 14 overall)
                if let Ok(utime) = parts[11].parse::<i64>() {
                    let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                    if clk_tck > 0 {
                        info.cpu_percent = ((utime as f64 / clk_tck as f64) * 100.0) as f32;
                    }
                }
            }
        }
    }
    
    info
}

fn uid_to_username(uid: u32) -> Option<String> {
    unsafe {
        let passwd = libc::getpwuid(uid);
        if passwd.is_null() {
            return None;
        }
        let name = (*passwd).pw_name;
        if name.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(name).to_string_lossy().to_string())
    }
}

#[cfg(test)]

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn smoke_collect_gpus() {
        let mut collector = MetricsCollector::new();
        let config = common::config::AgentConfig::default();
        let report = collector.collect(&config).unwrap();
        eprintln!("SMOKE gpus={} gpu_procs={} disks={} nets={}",
            report.gpus.len(), report.gpu_processes.len(),
            report.disks.len(), report.networks.len());
        for g in &report.gpus {
            eprintln!("SMOKE gpu[{}] {} util={} mem={}MB temp={} power={}W",
                g.index, g.name, g.utilization_gpu,
                g.memory_used_bytes / 1024 / 1024,
                g.temperature_celsius, g.power_watts);
        }
    }
}

fn monotonic_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
