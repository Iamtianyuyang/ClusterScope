use anyhow::Result;
use chrono::Utc;
use common::config::AgentConfig;
use nvml_wrapper::Nvml;
use nvml_wrapper::enums::device::UsedGpuMemory;
use protocol::NodeMetricsReport;
use std::collections::HashMap;
use std::process::Command;
use tracing::warn;

/// Lazily-initialized NVML handle (once per process). `None` when the
/// driver/NVML is unavailable — callers must fall back gracefully.
static NVML: std::sync::OnceLock<Option<Nvml>> = std::sync::OnceLock::new();

fn nvml() -> Option<&'static Nvml> {
    NVML.get_or_init(|| match Nvml::init() {
        Ok(nvml) => {
            warn!("NVML initialized");
            Some(nvml)
        }
        Err(e) => {
            warn!(error = %e, "NVML init failed — falling back to nvidia-smi");
            None
        }
    })
    .as_ref()
}

pub struct MetricsCollector {
    /// Persistent sysinfo handle: CPU usage is computed from the delta
    /// between two refreshes, so a fresh instance per tick would report
    /// 0 forever on Linux. Reused across ticks instead.
    sys: Option<sysinfo::System>,
    prev_network: Option<HashMap<String, NetworkCounter>>,
}

struct NetworkCounter {
    bytes_sent: u64,
    bytes_recv: u64,
    sampled_at_ms: i64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            sys: None,
            prev_network: None,
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
        // Reuse one System across ticks (CPU usage needs a previous sample).
        let sys = self.sys.get_or_insert_with(sysinfo::System::new_all);
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        // CPU usage (sysinfo returns 0-100; stored as-is)
        let total_usage =
            sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32;
        report.cpu_usage_percent = total_usage as f64;

        // CPU cores
        for (i, cpu) in sys.cpus().iter().enumerate() {
            report.cpu_cores.push(protocol::CpuCoreMetrics {
                core_id: i as u32,
                usage_percent: cpu.cpu_usage(),
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
        report.memory_total_bytes = total;
        report.memory_used_bytes = used;
        report.swap_total_bytes = sys.total_swap();
        report.swap_used_bytes = sys.used_swap();

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

        for disk in &disks {
            let usage = disk.total_space();
            let available = disk.available_space();
            // saturating: quota/ZFS/overlay filesystems can report
            // available > total; a plain subtract would underflow (panic in
            // debug, >100% usage in release).
            let used = usage.saturating_sub(available);
            let usage_pct = if usage > 0 {
                (used as f64 / usage as f64) * 100.0
            } else {
                0.0
            };

            report.disks.push(protocol::DiskMetrics {
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                total_bytes: usage,
                used_bytes: used,
                free_bytes: available,
                usage_percent: usage_pct as f32,
            });
        }
    }

    fn collect_network(&mut self, report: &mut NodeMetricsReport) {
        let now_ms = Utc::now().timestamp_millis();
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

            // Rate = (current - previous) bytes / elapsed seconds
            if let Some(prev) = self.prev_network.as_ref().and_then(|m| m.get(iface)) {
                let elapsed_secs = (now_ms - prev.sampled_at_ms) as f64 / 1000.0;
                if elapsed_secs > 0.0 {
                    let tx_rate =
                        (stats.bytes_sent as i128 - prev.bytes_sent as i128) as f64 / elapsed_secs;
                    let rx_rate =
                        (stats.bytes_recv as i128 - prev.bytes_recv as i128) as f64 / elapsed_secs;
                    net.tx_rate_bytes_per_sec = Some(tx_rate.max(0.0) as f32);
                    net.rx_rate_bytes_per_sec = Some(rx_rate.max(0.0) as f32);
                }
            }

            report.networks.push(net);
            current.insert(
                iface.clone(),
                NetworkCounter {
                    bytes_sent: stats.bytes_sent,
                    bytes_recv: stats.bytes_recv,
                    sampled_at_ms: now_ms,
                },
            );
        }

        self.prev_network = Some(current);
    }

    /// GPU metrics via NVML when available (no subprocess spawn per tick);
    /// falls back to parsing `nvidia-smi` output.
    fn collect_gpu(&self, report: &mut NodeMetricsReport) -> Result<()> {
        if let Some(nvml) = nvml() {
            if self.collect_gpu_nvml(nvml, report).is_ok() {
                return Ok(());
            }
            warn!("NVML GPU metrics failed — falling back to nvidia-smi");
        }
        self.collect_gpu_fallback(report)
    }

    /// NVML-based GPU metrics: utilization, memory, temperature, power, fan.
    /// A failing device is skipped (degraded) without failing the others;
    /// unsupported optional fields (fan, power limit) degrade to None/0.
    fn collect_gpu_nvml(&self, nvml: &Nvml, report: &mut NodeMetricsReport) -> Result<()> {
        let count = nvml.device_count()?;
        for idx in 0..count {
            let device = match nvml.device_by_index(idx) {
                Ok(d) => d,
                Err(e) => {
                    warn!(error = %e, gpu_index = idx, "NVML device open failed — skipping GPU");
                    continue;
                }
            };
            // Utilization is the core metric: when it cannot be read the GPU
            // is reported unavailable rather than guessing 0.
            let utilization = match device.utilization_rates() {
                Ok(u) => (u.gpu as f32, u.memory as f32),
                Err(e) => {
                    warn!(error = %e, gpu_index = idx, "NVML utilization read failed — skipping GPU");
                    continue;
                }
            };
            let memory = device.memory_info().ok().map(|m| (m.total, m.used));
            let temperature = device
                .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                .ok()
                .map(|t| t as f32);
            let power_watts = device.power_usage().ok().map(|mw| mw as f32 / 1000.0);
            let power_limit_watts = device
                .power_management_limit()
                .ok()
                .map(|mw| mw as f32 / 1000.0);
            let fan_speed = device.fan_speed(0).ok().map(|f| f as f32);
            let name = device.name().unwrap_or_else(|_| "unknown".to_string());
            let uuid = device.uuid().unwrap_or_default();

            report.gpus.push(protocol::GpuMetrics {
                index: idx,
                uuid,
                name,
                utilization_gpu: utilization.0,
                utilization_memory: utilization.1,
                memory_total_bytes: memory.map(|m| m.0),
                memory_used_bytes: memory.map(|m| m.1),
                temperature_celsius: temperature,
                power_watts,
                power_limit_watts,
                fan_speed_percent: fan_speed,
                pcie_tx_bytes_per_second: None,
                pcie_rx_bytes_per_second: None,
                mig_enabled: false,
                mig_instances: vec![],
            });
        }
        Ok(())
    }

    /// Fallback: parse `nvidia-smi --query-gpu=...` (kept for hosts without
    /// NVML). Unparseable fields stay 0; a missing/failed nvidia-smi is
    /// logged so the gap is visible.
    fn collect_gpu_fallback(&self, report: &mut NodeMetricsReport) -> Result<()> {
        // Parse nvidia-smi output
        let output = Command::new("nvidia-smi")
            .args(["--query-gpu=index,uuid,name,utilization.gpu,utilization.memory,memory.total,memory.used,temperature.gpu,power.draw,power.limit,fan.speed",
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

                    // Utilization is the core metric: when it cannot be read
                    // the GPU is reported unavailable rather than guessing 0
                    // (mirrors the NVML path).
                    let Some(utilization_gpu) = parse_opt::<f32>(fields[3]) else {
                        warn!(gpu_index = %fields[0], "nvidia-smi utilization unavailable — skipping GPU");
                        continue;
                    };
                    let Some(utilization_memory) = parse_opt::<f32>(fields[4]) else {
                        warn!(gpu_index = %fields[0], "nvidia-smi memory utilization unavailable — skipping GPU");
                        continue;
                    };

                    let gpu = protocol::GpuMetrics {
                        index: fields[0].parse().unwrap_or(0),
                        uuid: fields[1].to_string(),
                        name: fields[2].to_string(),
                        utilization_gpu,
                        utilization_memory,
                        memory_total_bytes: parse_opt::<u64>(fields[5]).map(|v| v * 1024 * 1024),
                        memory_used_bytes: parse_opt::<u64>(fields[6]).map(|v| v * 1024 * 1024),
                        temperature_celsius: parse_opt::<f32>(fields[7]),
                        power_watts: parse_opt::<f32>(fields[8]),
                        power_limit_watts: parse_opt::<f32>(fields[9]),
                        fan_speed_percent: fields.get(10).and_then(|s| parse_opt::<f32>(s)),
                        pcie_tx_bytes_per_second: None,
                        pcie_rx_bytes_per_second: None,
                        mig_enabled: false,
                        mig_instances: vec![],
                    };

                    report.gpus.push(gpu);
                }
            }
            Ok(_) => {
                warn!("nvidia-smi exited non-zero — GPU metrics unavailable");
            }
            Err(e) => {
                warn!(error = %e, "Failed to run nvidia-smi — GPU metrics unavailable");
            }
        }

        Ok(())
    }

    fn collect_gpu_processes(
        &self,
        _config: &AgentConfig,
        report: &mut NodeMetricsReport,
    ) -> Result<()> {
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
                    // Unavailable must stay unset — a fabricated 0 looks like
                    // an idle process to consumers.
                    gpu_memory_bytes: match p.used_gpu_memory {
                        UsedGpuMemory::Used(b) => Some(b),
                        UsedGpuMemory::Unavailable => None,
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
                    "-i",
                    gpu_uuid,
                    "--query-compute-apps=pid,used_memory,gpu_uuid",
                    "--format=csv,noheader,nounits",
                ])
                .output();

            if let Ok(output) = output
                && output.status.success()
            {
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
                                gpu_memory_bytes: parse_opt::<u64>(fields[1])
                                    .map(|mib| mib * 1024 * 1024),
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

/// Parse a numeric field, treating empty / "[N/A]" / garbage as unset.
fn parse_opt<T: std::str::FromStr>(field: &str) -> Option<T> {
    let f = field.trim();
    if f.is_empty() || f.eq_ignore_ascii_case("[N/A]") {
        None
    } else {
        f.parse().ok()
    }
}

fn get_load_average() -> Result<(f64, f64, f64)> {
    let contents = std::fs::read_to_string("/proc/loadavg")?;
    let parts: Vec<&str> = contents.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok((0.0, 0.0, 0.0));
    }
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
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
            stats.insert(
                iface,
                NetworkStats {
                    bytes_sent: parts[9].parse().unwrap_or(0),
                    bytes_recv: parts[1].parse().unwrap_or(0),
                    packets_sent: parts[10].parse().unwrap_or(0),
                    packets_recv: parts[2].parse().unwrap_or(0),
                },
            );
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

fn monotonic_clock_ms() -> u64 {
    // Real CLOCK_MONOTONIC (never jumps with wall-clock changes).
    #[cfg(target_os = "linux")]
    {
        let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) } == 0 {
            let ts = unsafe { ts.assume_init() };
            return (ts.tv_sec as u64) * 1_000 + (ts.tv_nsec as u64) / 1_000_000;
        }
    }
    // Fallback (non-Linux build): wall clock since epoch.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn smoke_collect_gpus() {
        let mut collector = MetricsCollector::new();
        let config = common::config::AgentConfig::default();
        let report = collector.collect(&config).unwrap();
        eprintln!(
            "SMOKE gpus={} gpu_procs={} disks={} nets={}",
            report.gpus.len(),
            report.gpu_processes.len(),
            report.disks.len(),
            report.networks.len()
        );
        for g in &report.gpus {
            eprintln!(
                "SMOKE gpu[{}] {} util={} mem={}MB temp={} power={}W",
                g.index,
                g.name,
                g.utilization_gpu,
                g.memory_used_bytes.unwrap_or(0) / 1024 / 1024,
                g.temperature_celsius.unwrap_or(f32::NAN),
                g.power_watts.unwrap_or(f32::NAN)
            );
        }
    }
}
