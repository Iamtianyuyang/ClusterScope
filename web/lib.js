export const $ = selector => document.querySelector(selector);

export function escapeHtml(value = '') {
  return String(value).replace(/[&<>'"]/g, char => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;',
  })[char]);
}

export function fmtBytes(value, compact = false) {
  if (value === null || value === undefined || !Number.isFinite(Number(value))) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let number = Math.max(0, Number(value));
  let unit = 0;
  while (number >= 1024 && unit < units.length - 1) { number /= 1024; unit += 1; }
  const digits = unit === 0 ? 0 : number >= 100 ? 0 : 1;
  return `${number.toFixed(digits)}${compact ? '' : ' '}${units[unit]}`;
}

export function fmtPercent(value) {
  return value === null || value === undefined || !Number.isFinite(Number(value))
    ? '—' : `${Math.round(Number(value))}%`;
}

export function fmtUptime(seconds) {
  if (seconds === null || seconds === undefined || !Number.isFinite(Number(seconds))) return '—';
  const value = Math.max(0, Number(seconds));
  const days = Math.floor(value / 86400);
  const hours = Math.floor(value % 86400 / 3600);
  const minutes = Math.floor(value % 3600 / 60);
  if (days) return `${days}d ${String(hours).padStart(2, '0')}h`;
  if (hours) return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  return `${minutes}m`;
}

export function fmtClock(date = new Date()) {
  return [date.getHours(), date.getMinutes(), date.getSeconds()]
    .map(value => String(value).padStart(2, '0')).join(':');
}

export function fmtTime(ms, long = false) {
  const date = new Date(Number(ms));
  if (!Number.isFinite(date.getTime())) return '—';
  if (long) {
    return `${String(date.getMonth() + 1).padStart(2, '0')}/${String(date.getDate()).padStart(2, '0')} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
  }
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function memoryPercent(gpu) {
  if (gpu?.memory_used_bytes === null || gpu?.memory_used_bytes === undefined
    || gpu?.memory_total_bytes === null || gpu?.memory_total_bytes === undefined) return null;
  const used = Number(gpu.memory_used_bytes);
  const total = Number(gpu.memory_total_bytes);
  return Number.isFinite(used) && Number.isFinite(total) && total > 0 ? used / total * 100 : null;
}

export function gpuUtil(gpu) {
  const value = Number(gpu?.utilization_gpu);
  return Number.isFinite(value) ? value : null;
}

export function gpuTemp(gpu) {
  const value = Number(gpu?.temperature_celsius);
  return Number.isFinite(value) ? value : null;
}

export function gpuTone(gpu) {
  const util = gpuUtil(gpu);
  const temp = gpuTemp(gpu);
  if (temp !== null && temp >= 80) return 'critical';
  if ((util !== null && util >= 85) || (temp !== null && temp >= 70)) return 'hot';
  if (util !== null && util < 5) return 'idle';
  return '';
}

export function normalizeMetrics(raw) {
  if (!raw || !raw.node_id) return null;
  return {
    ...raw,
    gpus: raw.gpus || raw.gpu_metrics || [],
    gpu_processes: raw.gpu_processes || [],
    cpu_cores: raw.cpu_cores || raw.cpu_core_metrics || [],
    cpu_processes: raw.cpu_processes || [],
  };
}

export function linePath(values) {
  const width = 600, height = 180, padding = 10;
  const valid = values.map((value, index) => ({ value, index }))
    .filter(point => Number.isFinite(point.value));
  if (!valid.length) return '';
  const denominator = Math.max(1, values.length - 1);
  const points = valid.map(({ value, index }) => [
    padding + index / denominator * (width - padding * 2),
    height - padding - Math.max(0, Math.min(100, value)) / 100 * (height - padding * 2),
  ]);
  if (points.length === 1) return `M ${points[0][0]} ${points[0][1]} L ${points[0][0] + .01} ${points[0][1]}`;
  let path = `M ${points[0][0]} ${points[0][1]}`;
  for (let i = 0; i < points.length - 1; i += 1) {
    const p0 = points[Math.max(0, i - 1)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(points.length - 1, i + 2)];
    const cp1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const cp2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    path += ` C ${cp1.join(' ')} ${cp2.join(' ')} ${p2.join(' ')}`;
  }
  return path;
}

export function areaPath(values) {
  const line = linePath(values);
  return line ? `${line} L 590 170 L 10 170 Z` : '';
}

export function historySample(raw) {
  const gpus = raw?.gpus || raw?.gpu_metrics || [];
  const memoryTotal = Number(raw?.memory_total_bytes);
  const memoryUsed = Number(raw?.memory_used_bytes);
  const systemMem = Number.isFinite(raw?.memory_usage_percent)
    ? raw.memory_usage_percent
    : Number.isFinite(memoryTotal) && memoryTotal > 0 && Number.isFinite(memoryUsed)
      ? memoryUsed / memoryTotal * 100 : null;
  return {
    cpu: Number.isFinite(raw?.cpu_usage_percent) ? raw.cpu_usage_percent : null,
    gpuAverage: Number.isFinite(raw?.gpu_utilization_percent) ? raw.gpu_utilization_percent : null,
    systemMem,
    gpus: gpus.map(gpu => ({ util: gpuUtil(gpu), mem: memoryPercent(gpu) })),
    timestamp: raw?.timestamp_ms || Date.now(),
  };
}

export function statusLabel(status) {
  return ({
    online: '在线', degraded: '不稳定', offline: '离线',
  })[status] || status || '—';
}
