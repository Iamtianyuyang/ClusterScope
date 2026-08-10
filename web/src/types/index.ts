export interface NodeStatus {
  node_id: string
  hostname: string
  ip_address: string
  status: 'online' | 'degraded' | 'offline'
  last_seen: string
  gpu_count: number
  labels: Record<string, string>
  cpu_model?: string
  cpu_cores?: number
  memory_total_bytes?: number
}

export interface ClusterInfo {
  total_nodes: number
  online_nodes: number
  degraded_nodes: number
  offline_nodes: number
  total_gpus: number
  idle_gpus: number
  avg_gpu_utilization: number
  running_jobs: number
  active_alerts: number
}

export interface GpuMetric {
  gpu_index: number
  gpu_name: string
  utilization_gpu: number
  memory_total_gb: number
  memory_used_gb: number
  temperature: number
  power_watts: number
  power_limit_watts: number
  fan_speed: number | null
}

export interface SystemMetric {
  cpu_usage: number
  memory_total_gb: number
  memory_used_gb: number
  swap_total_gb: number
  swap_used_gb: number
  load_1: number
  load_5: number
  load_15: number
}

export interface Job {
  job_id: string
  node_id: string
  name: string
  executable: string
  arguments: string[]
  working_directory: string
  status: string
  pid: number
  exit_code: number | null
  error_message: string
  created_at: string
  started_at: string | null
  finished_at: string | null
  created_by: string
}

export interface JobLogEntry {
  log_offset: number
  log_data: string
  is_stderr: boolean
  timestamp: number
}

export interface AlertRule {
  rule_id: string
  name: string
  metric: string
  operator: string
  threshold: number
  duration_seconds: number
  severity: 'info' | 'warning' | 'critical'
  node_id: string
  enabled: boolean
  created_at: string
}

export interface AlertEvent {
  event_id: string
  rule_id: string
  node_id: string
  old_state: string
  new_state: string
  current_value: number
  threshold: number
  timestamp: string
}

export interface User {
  user_id: string
  username: string
  email: string
  role: 'viewer' | 'operator' | 'admin'
  enabled: boolean
  created_at: string
}

export interface MetricPoint {
  timestamp: number
  value: number
}

export interface MetricsHistory {
  node_id: string
  metric_name: string
  points: MetricPoint[]
}
