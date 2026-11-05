import { accessToken, clearSession, connectEvents, login, request } from '/api.js';
import {
  $, areaPath, escapeHtml, fmtBytes, fmtClock, fmtPercent, fmtTime, fmtUptime,
  gpuTemp, gpuTone, gpuUtil, historySample, linePath, memoryPercent,
  normalizeMetrics, statusLabel,
} from '/lib.js';
import { createCountdown, flipCell, pageTransition, spinRefresh } from '/motion.js';

const VIEWS = ['overview', 'trend'];
const RANGES = [['live', '实时'], ['1h', '1 小时'], ['24h', '24 小时'], ['7d', '7 天']];
const REFRESH_MS = 3000;

const state = {
  view: VIEWS.includes(location.hash.slice(1)) ? location.hash.slice(1) : 'overview',
  nodes: [],
  metrics: new Map(),
  histories: new Map(),
  selectedNodeId: '',
  selectedGpu: 0,
  processMode: 'gpu',
  loading: true,
  refreshing: false,
  error: '',
  nextRefreshAt: Date.now() + REFRESH_MS,
  lastUpdated: 0,
  connected: null,
  boardSignature: '',
  trendSignature: '',
  historyRange: '1h',
  historySamples: [],
  historyLoading: false,
  historyRequest: 0,
  chartCache: new Map(),
  trendDataSignature: '',
  nodeStatuses: new Map(),
  statusesInitialized: false,
  impactNodeId: '',
  pendingServers: [],
  addServerAfterLogin: false,
  addServerInstalling: false,
};

let disconnectEvents = () => {};
let toastTimer;
let countdown;

const clamp = value => Math.max(0, Math.min(100, Number(value) || 0));

function toast(message) {
  const element = $('#toast');
  if (!element) return;
  clearTimeout(toastTimer);
  element.textContent = message;
  element.hidden = false;
  toastTimer = setTimeout(() => { element.hidden = true; }, 3200);
}

function selectedNode() {
  return state.nodes.find(node => node.node_id === state.selectedNodeId) || state.nodes[0] || null;
}

function metricFor(node) {
  return node ? state.metrics.get(node.node_id) || null : null;
}

function gpusFor(node) {
  return metricFor(node)?.gpus || [];
}

function gpuCountFor(node) {
  return Math.max(Number(node?.gpu_count) || 0, gpusFor(node).length);
}

function gpuAt(metric, index) {
  const gpus = metric?.gpus || [];
  return gpus.find(gpu => Number(gpu.index) === index) || gpus[index] || null;
}

function nodeMemPercent(metric) {
  const total = Number(metric?.memory_total_bytes);
  const used = Number(metric?.memory_used_bytes);
  return Number.isFinite(total) && total > 0 && Number.isFinite(used) ? used / total * 100 : null;
}

function nodeCpu(node) {
  const value = Number(metricFor(node)?.cpu_usage_percent);
  return Number.isFinite(value) ? value : null;
}

function nodeLoad(node) {
  const value = Number(metricFor(node)?.load_1);
  return Number.isFinite(value) ? value.toFixed(2) : '—';
}

function nodeSub(node) {
  const ip = node.ip_address || node.node_id;
  if (node.status === 'offline') {
    const lastSeen = Date.parse(node.last_seen || '');
    return `${ip} · 最后在线 ${Number.isFinite(lastSeen) ? fmtTime(lastSeen, true) : '—'}`;
  }
  return `${gpuCountFor(node)} GPU · ${ip}`;
}

function gpuText(gpu) {
  const util = gpuUtil(gpu);
  return util === null ? '--' : String(Math.round(util)).padStart(2, '0');
}

function gpuTempText(gpu) {
  const temp = gpuTemp(gpu);
  return temp === null ? '--' : `${Math.round(temp)}°`;
}

function cpuTone(value) {
  return value !== null && value >= 90 ? 'hot' : '';
}

function memTone(value) {
  if (value === null) return '';
  return value >= 90 ? 'critical' : value >= 75 ? 'hot' : '';
}

function summaryData() {
  const online = state.nodes.filter(node => node.status === 'online').length;
  const offline = state.nodes.filter(node => node.status === 'offline').length;
  const degraded = state.nodes.filter(node => node.status === 'degraded').length;
  const totalGpus = state.nodes.reduce((sum, node) => sum + gpuCountFor(node), 0);
  const activeNodes = state.nodes.filter(node => node.status !== 'offline');
  const metrics = activeNodes.map(metricFor).filter(Boolean);
  const allGpus = metrics.flatMap(metric => metric.gpus);
  const busy = allGpus.filter(gpu => (gpuUtil(gpu) ?? 0) >= 10).length;
  const cpus = metrics.map(metric => Number(metric.cpu_usage_percent))
    .filter(value => Number.isFinite(value));
  const avgCpu = cpus.length ? cpus.reduce((sum, value) => sum + value, 0) / cpus.length : null;
  let hottest = null;
  for (const [nodeId, metric] of state.metrics) {
    if (state.nodes.find(node => node.node_id === nodeId)?.status === 'offline') continue;
    for (const gpu of metric.gpus) {
      const temp = gpuTemp(gpu);
      if (temp !== null && (!hottest || temp > hottest.temp)) {
        hottest = { temp, nodeId, index: gpu.index };
      }
    }
  }
  return { online, offline, degraded, totalGpus, busy, avgCpu, cpuNodes: cpus.length, hottest };
}

function summaryCells() {
  const { online, offline, degraded, totalGpus, busy, avgCpu, cpuNodes, hottest } = summaryData();
  const busyPercent = totalGpus ? Math.round(busy / totalGpus * 100) : 0;
  const availabilityText = offline || degraded
    ? `${offline ? `${offline} 离线` : ''}${offline && degraded ? ' · ' : ''}${degraded ? `${degraded} 不稳定` : ''}`
    : '全部在线';
  const availability = `<span class="sum-side ${offline ? 'bad' : degraded ? 'hot' : 'good'}" title="${availabilityText}">${availabilityText}</span>`;
  return `<div class="sum">
    <div><span class="sum-label">在线节点</span><span class="sum-value"><span class="sum-num">${online}</span><small> / ${state.nodes.length}</small></span></div>
    ${availability}
  </div>
  <div class="sum">
    <div><span class="sum-label">GPU 忙碌</span><span class="sum-value"><span class="sum-num">${busy}</span><small> / ${totalGpus}</small></span></div>
    <span class="sum-side">${busyPercent}% 占用</span>
  </div>
  <div class="sum">
    <div><span class="sum-label">平均 CPU</span><span class="sum-value"><span class="sum-num">${avgCpu === null ? '—' : Math.round(avgCpu)}</span><small>${avgCpu === null ? '' : '%'}</small></span></div>
    <span class="sum-side">${cpuNodes} 个节点平均</span>
  </div>
  <div class="sum">
    <div><span class="sum-label">最高温度</span><span class="sum-value"><span class="sum-num">${hottest ? Math.round(hottest.temp) : '—'}</span><small>${hottest ? '°C' : ''}</small></span></div>
    <span class="sum-side ${hottest && hottest.temp >= 80 ? 'bad' : hottest && hottest.temp >= 70 ? 'hot' : ''}" title="${hottest ? escapeHtml(`${hottest.nodeId} · GPU ${hottest.index}`) : ''}">${hottest ? `${escapeHtml(hottest.nodeId)} · GPU ${hottest.index}` : '—'}</span>
  </div>`;
}

function renderError() {
  return `<div class="error-banner" id="error-banner" role="alert" ${state.error ? '' : 'hidden'}><span>${escapeHtml(state.error)}</span><button type="button" data-retry>重试</button></div>`;
}

function patchError() {
  const banner = $('#error-banner');
  if (!banner) return;
  banner.hidden = !state.error;
  const text = banner.querySelector('span');
  if (text) text.textContent = state.error;
}

function viewMeta() {
  const updated = state.lastUpdated ? fmtClock(new Date(state.lastUpdated)) : '—';
  return `最后更新 <b>${updated}</b><br>采样 <b>3s</b> · ${state.connected ? 'WS' : 'REST'}`;
}

function emptyHtml(trend = false) {
  const failed = Boolean(state.error);
  const cells = Array.from({ length: 24 }, (_, index) => `<i class="empty-cell" style="--i:${index}"></i>`).join('');
  const title = trend
    ? (failed ? '暂时读不到趋势' : '还没有趋势数据')
    : (failed ? '暂时读不到集群' : '还没有节点');
  const description = trend
    ? (failed ? '检查 Server 是否运行，或稍后重试。' : '节点上线并开始上报后，趋势会显示在这里。')
    : (failed ? '检查 Server 是否运行，或稍后重试。' : '在计算节点启动 Agent 后，节点会显示在这里。');
  return `<div class="view-head"><div><h1>${trend ? '趋势' : '集群状态'}</h1><p>ClusterScope · ${state.nodes.length} 节点 · ${state.nodes.reduce((sum, node) => sum + gpuCountFor(node), 0)} GPU</p></div><div class="view-actions">${trend ? '' : addServerButton()}<div class="view-meta" data-view-meta>${viewMeta()}</div></div></div>
  ${renderError()}
  <section class="empty-board" aria-label="${escapeHtml(title)}">
    <div class="empty-grid" aria-hidden="true">${cells}</div>
    <div class="empty-message"><b>${escapeHtml(title)}</b><p>${escapeHtml(description)}</p></div>
  </section>`;
}

function loadingHtml() {
  return `${renderError()}<div class="loading-board" aria-label="正在读取集群状态"><div class="loading-head"></div><div class="loading-row"></div><div class="loading-row"></div><div class="loading-row"></div></div>`;
}

function visibleGpuSlots() {
  return state.nodes.reduce((max, node) => Math.max(max, Math.min(8, gpuCountFor(node))), 0);
}

function boardHead() {
  const slots = visibleGpuSlots();
  const gpuHeads = slots
    ? Array.from({ length: 8 }, (_, index) => `<span role="columnheader">GPU ${index}</span>`).join('')
    : '<span role="columnheader" class="head-gpu-empty" aria-colspan="8">GPU</span>';
  return `<div class="board-head" role="row">
    <span role="columnheader">节点 / 地址</span>
    <span role="columnheader">状态</span>
    <span role="columnheader">CPU</span>
    <span role="columnheader">内存</span>
    ${gpuHeads}
    <span role="columnheader" class="head-right">负载</span>
    <span role="columnheader" class="head-right">运行</span>
  </div>`;
}

function renderGpuButton(node, gpu, index, nodeSelected) {
  const util = gpuUtil(gpu);
  const temp = gpuTemp(gpu);
  const tone = gpu ? gpuTone(gpu) : 'idle';
  const selected = nodeSelected && index === state.selectedGpu;
  const text = util === null ? '--' : `${gpuText(gpu)}%`;
  const sub = `${index} · ${gpuTempText(gpu)}`;
  const label = `${node.hostname || node.node_id} GPU ${index}，利用率 ${util === null ? '未知' : `${Math.round(util)}%`}，温度 ${temp === null ? '未知' : `${Math.round(temp)} 度`}`;
  const title = `${gpu?.name || 'GPU'}${gpu ? ` · ${fmtBytes(gpu.memory_used_bytes, true)} / ${fmtBytes(gpu.memory_total_bytes, true)}` : ''}`;
  return `<button class="gpu-button ${selected ? 'selected' : ''}" type="button" data-node="${escapeHtml(node.node_id)}" data-gpu="${index}" data-focus-key="board-gpu-${index}" aria-pressed="${selected}" aria-label="${escapeHtml(label)}" title="${escapeHtml(title)}">
    <span class="flap ${tone}" data-value="${text}"><span class="flap-face">${text}</span></span>
    <span class="gpu-sub">${sub}</span>
  </button>`;
}

function renderBoardRow(node) {
  const metric = metricFor(node);
  const selected = selectedNode()?.node_id === node.node_id;
  const offline = node.status === 'offline';
  const cpu = offline ? null : nodeCpu(node);
  const memory = offline ? null : nodeMemPercent(metric);
  const cpuText = cpu === null ? '—' : `${Math.round(cpu)}%`;
  const memText = memory === null ? '—' : `${Math.round(memory)}%`;
  const count = Math.min(8, gpuCountFor(node));
  const gpus = Array.from({ length: count }, (_, index) => offline ? null : gpuAt(metric, index));
  return `<div class="node-row ${selected ? 'selected' : ''} ${offline ? 'is-offline' : ''}" data-node-row="${escapeHtml(node.node_id)}" role="row">
    <div class="node-cell" role="cell"><i class="row-lamp"></i><button class="node-button" type="button" data-node="${escapeHtml(node.node_id)}" aria-pressed="${selected}" aria-label="选择节点 ${escapeHtml(node.hostname || node.node_id)}" title="${escapeHtml(`${node.hostname || node.node_id} · ${nodeSub(node)}`)}"><b>${escapeHtml(node.hostname || node.node_id)}</b><span data-node-sub>${escapeHtml(nodeSub(node))}</span></button></div>
    <div class="status ${escapeHtml(node.status)}" role="cell" data-status><i></i><span class="status-text">${statusLabel(node.status)}</span></div>
    <div class="flap-cell measure cpu ${cpuTone(cpu)}" role="cell" data-cell="cpu"><span class="measure-label">CPU</span><div class="flap ${cpuTone(cpu)}" data-value="${escapeHtml(cpuText)}"><span class="flap-face">${escapeHtml(cpuText)}</span><i class="bar" style="width:${clamp(cpu)}%"></i></div></div>
    <div class="flap-cell measure mem ${memTone(memory)}" role="cell" data-cell="mem"><span class="measure-label">内存</span><div class="flap ${memTone(memory)}" data-value="${escapeHtml(memText)}"><span class="flap-face">${escapeHtml(memText)}</span><i class="bar" style="width:${clamp(memory)}%"></i></div></div>
    <div class="gpu-strip" role="cell" aria-colspan="8">${count ? gpus.map((gpu, index) => renderGpuButton(node, gpu, index, selected)).join('') : '<span class="no-gpu">仅 CPU · 无 GPU</span>'}</div>
    <div class="num-cell load-cell" role="cell" data-load>${offline ? '—' : nodeLoad(node)}<span>LOAD</span></div>
    <div class="num-cell up-cell" role="cell" data-uptime>${offline ? '—' : fmtUptime(metric?.uptime_seconds)}<span>UPTIME</span></div>
  </div>`;
}

function coreBand(metric, stale = false) {
  const cores = metric?.cpu_cores || [];
  if (!cores.length) return '';
  const values = cores.map(core => Number(core.usage_percent)).filter(Number.isFinite);
  const avg = values.length ? Math.round(values.reduce((sum, value) => sum + value, 0) / values.length) : null;
  const max = values.length ? Math.round(Math.max(...values)) : null;
  const blocks = cores.map(core => {
    const value = Number(core.usage_percent);
    const tone = value >= 90 ? 'hot' : value >= 1 ? 'busy' : '';
    return `<i class="core-block ${tone}" aria-hidden="true" title="核心 ${core.core_id} · ${fmtPercent(core.usage_percent)}"></i>`;
  }).join('');
  return `<div class="core-band"><span class="core-label">CPU 核心 · ${cores.length}${stale ? ' · 最后数据' : ''}</span><div class="core-strip" role="img" aria-label="每核 CPU 使用率">${blocks}</div><span class="core-note">平均 ${avg === null ? '—' : `${avg}%`} · 最高 ${max === null ? '—' : `${max}%`}</span></div>`;
}

function detailPanes() {
  const node = selectedNode();
  if (!node) return '<div class="pane wide"><div class="pane-empty"><b>等待节点接入</b><span>Agent 启动后，这里会显示 GPU、进程和每核 CPU。</span></div></div>';
  const metric = metricFor(node);
  const count = gpuCountFor(node);
  const offline = node.status === 'offline';
  if (!count) {
    const head = `<div class="pane-head"><h2>CPU 进程</h2><p>${escapeHtml(node.hostname || node.node_id)} · 占用最高</p></div>`;
    const body = offline
      ? '<div class="pane-empty"><b>节点已离线</b><span>显示最后一次上报的数据，等待 Agent 重新连接。</span></div>'
      : metric
        ? cpuProcessTable(metric)
        : '<div class="pane-empty"><b>正在采样</b><span>Agent 每隔几秒读取一次进程，请稍等。</span></div>';
    return `<div class="pane wide">${head}${body}${coreBand(metric, offline)}</div>`;
  }
  const gpus = Array.from({ length: count }, (_, index) => gpuAt(metric, index));
  const selectedGpu = gpus[state.selectedGpu] || null;
  const gpuRows = gpus.map((gpu, index) => {
    const util = gpuUtil(gpu);
    const mem = memoryPercent(gpu);
    const temp = gpuTemp(gpu);
    const tone = gpu ? gpuTone(gpu) : 'idle';
    const selected = index === state.selectedGpu;
    return `<button class="gpu-row ${selected ? 'selected' : ''}" type="button" data-gpu-row="${index}" data-focus-key="detail-gpu-${index}" aria-pressed="${selected}" aria-label="选择 GPU ${index}">
      <b>GPU ${index}</b>
      <span class="gpu-track ${tone === 'critical' ? 'critical' : tone === 'hot' ? 'hot' : ''}"><i style="width:${clamp(util)}%"></i></span>
      <span class="r">${util === null ? '—' : `${Math.round(util)}%`}</span>
      <span class="r">${mem === null ? '—' : fmtBytes(gpu?.memory_used_bytes, true)}</span>
      <em>${temp === null ? '—' : `${Math.round(temp)}°C`}</em>
    </button>`;
  }).join('');
  const subline = count === 0
    ? `${escapeHtml(node.hostname || node.node_id)} · CPU 进程`
    : state.processMode === 'cpu'
      ? `${escapeHtml(node.hostname || node.node_id)} · CPU 占用最高`
      : `${escapeHtml(node.hostname || node.node_id)} · GPU ${state.selectedGpu}${selectedGpu?.name ? ` · ${escapeHtml(selectedGpu.name)}` : ''}`;
  return `<div class="pane">
    <div class="pane-head"><h2>GPU 明细</h2><p>${escapeHtml(node.hostname || node.node_id)} · ${count} 块${offline ? ' · 最后数据' : ''}</p></div>
    <div class="gpu-list">${gpuRows || (count ? '<div class="pane-empty"><b>正在采样</b><span>Agent 还没有上报 GPU 数据。</span></div>' : '<div class="pane-empty"><b>没有 GPU</b><span>这台节点只上报 CPU 数据。</span></div>')}</div>
  </div>
  <div class="pane">
    <div class="pane-head"><h2>进程</h2><div class="segmented" role="group" aria-label="进程类型"><button type="button" data-process-mode="gpu" data-focus-key="mode-gpu" aria-pressed="${state.processMode === 'gpu'}">GPU 进程</button><button type="button" data-process-mode="cpu" data-focus-key="mode-cpu" aria-pressed="${state.processMode === 'cpu'}">Top CPU</button></div></div>
    <p class="pane-sub" title="${subline}">${subline}</p>
    ${processTable(node, metric)}
    ${coreBand(metric, offline)}
  </div>`;
}

function cpuProcessTable(metric) {
  const processes = [...(metric?.cpu_processes || [])]
    .sort((a, b) => Number(b.cpu_percent) - Number(a.cpu_percent)).slice(0, 12);
  if (!processes.length) return '<div class="pane-empty"><b>没有 CPU 进程</b><span>Agent 还没有读到进程。</span></div>';
  const table = `<div class="table-wrap" data-scroll><table class="process-table"><thead><tr><th>PID</th><th>用户</th><th>CPU</th><th>内存</th><th>命令</th></tr></thead><tbody>${processes.map(process => `<tr><td class="pid">${process.pid}</td><td class="muted">${escapeHtml(process.username || '—')}</td><td>${fmtPercent(process.cpu_percent)}</td><td>${fmtBytes(process.memory_bytes, true)}</td><td class="command" title="${escapeHtml(process.command || '')}">${escapeHtml(process.command || '—')}</td></tr>`).join('')}</tbody></table></div>`;
  const cards = `<div class="process-list" data-scroll><ul class="process-entries">${processes.map(process => `<li class="process-entry"><div class="pe-top"><b>${process.pid}</b><span>${escapeHtml(process.username || '—')}</span><span>CPU ${fmtPercent(process.cpu_percent)}</span><span>内存 ${fmtBytes(process.memory_bytes, true)}</span></div><p class="pe-command">${escapeHtml(process.command || '—')}</p></li>`).join('')}</ul></div>`;
  return `<div class="process-view">${table}${cards}</div>`;
}

function processTable(node, metric) {
  if (node.status === 'offline') {
    return '<div class="pane-empty"><b>节点已离线</b><span>显示最后一次上报的数据，等待 Agent 重新连接。</span></div>';
  }
  if (!gpuCountFor(node) && state.processMode === 'gpu') {
    return '<div class="pane-empty"><b>没有 GPU</b><span>这台节点只上报 CPU 数据。切到 Top CPU 查看进程。</span></div>';
  }
  if (!metric) {
    return '<div class="pane-empty"><b>正在采样</b><span>Agent 每隔几秒读取一次进程，请稍等。</span></div>';
  }
  if (state.processMode === 'cpu') {
    return cpuProcessTable(metric);
  }
  const gpu = gpuAt(metric, state.selectedGpu);
  let processes = metric.gpu_processes || [];
  if (gpu?.uuid) processes = processes.filter(process => process.gpu_uuid === gpu.uuid);
  processes = [...processes].sort((a, b) => Number(b.gpu_memory_bytes) - Number(a.gpu_memory_bytes)).slice(0, 12);
  if (!processes.length) {
    return `<div class="pane-empty"><b>GPU ${state.selectedGpu} 空闲</b><span>当前没有正在运行的进程。</span></div>`;
  }
  const table = `<div class="table-wrap" data-scroll><table class="process-table"><thead><tr><th>PID</th><th>用户</th><th>SM</th><th>显存</th><th>CPU</th><th>命令</th></tr></thead><tbody>${processes.map(process => `<tr><td class="pid">${process.pid}</td><td class="muted">${escapeHtml(process.username || '—')}</td><td>${process.sm_utilization === null || process.sm_utilization === undefined ? '—' : fmtPercent(process.sm_utilization)}</td><td>${fmtBytes(process.gpu_memory_bytes, true)}</td><td>${fmtPercent(process.cpu_percent)}</td><td class="command" title="${escapeHtml(process.command || '')}">${escapeHtml(process.command || '—')}</td></tr>`).join('')}</tbody></table></div>`;
  const cards = `<div class="process-list" data-scroll><ul class="process-entries">${processes.map(process => `<li class="process-entry"><div class="pe-top"><b>${process.pid}</b><span>${escapeHtml(process.username || '—')}</span><span>SM ${process.sm_utilization === null || process.sm_utilization === undefined ? '—' : fmtPercent(process.sm_utilization)}</span><span>显存 ${fmtBytes(process.gpu_memory_bytes, true)}</span><span>CPU ${fmtPercent(process.cpu_percent)}</span></div><p class="pe-command">${escapeHtml(process.command || '—')}</p></li>`).join('')}</ul></div>`;
  return `<div class="process-view">${table}${cards}</div>`;
}

const PENDING_KEY = 'cs_pending_servers';

function addServerButton() {
  return `<button class="text-button add-server-button" type="button" data-add-server aria-haspopup="dialog"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>添加服务器</button>`;
}

function loadPendingServers() {
  try {
    const rows = JSON.parse(localStorage.getItem(PENDING_KEY) || '[]');
    if (!Array.isArray(rows)) return [];
    return rows.filter(row => row && typeof row.id === 'string' && typeof row.username === 'string' && typeof row.host === 'string')
      .map(row => ({
        id: row.id,
        username: row.username,
        host: row.host,
        port: Number(row.port) || 22,
        nodeId: typeof row.nodeId === 'string' ? row.nodeId : '',
        serverAddr: typeof row.serverAddr === 'string' ? row.serverAddr : '',
        // A request cannot survive a reload; treat persisted rows as waiting so
        // a closed tab never leaves a permanent "installing" receipt.
        status: 'waiting',
        createdAt: Number(row.createdAt) || Date.now(),
      }));
  } catch {
    return [];
  }
}

function savePendingServers() {
  try {
    localStorage.setItem(PENDING_KEY, JSON.stringify(state.pendingServers));
  } catch {
    // Private mode / quota: the in-memory list still works for this page.
  }
}

function pendingDisplayName(pending) {
  return pending.nodeId || `${pending.username}@${pending.host}`;
}

function renderPendingRow(pending) {
  const installing = pending.status === 'installing';
  const name = pendingDisplayName(pending);
  const sub = `${pending.username}@${pending.host}:${pending.port} · ${installing ? '正在安装' : '等待 Agent 上报'}`;
  const status = installing ? 'installing' : 'pending';
  const statusText = installing ? '安装中' : '待接入';
  const note = installing ? '正在通过 SSH 上传并启动 Agent…' : 'Agent 启动后自动出现在这里';
  const remove = installing ? '' : `<button class="text-button" type="button" data-pending-remove="${escapeHtml(pending.id)}" aria-label="移除待接入服务器 ${escapeHtml(name)}">移除</button>`;
  return `<div class="node-row pending ${installing ? 'installing' : ''}" data-pending-row="${escapeHtml(pending.id)}" role="row">
    <div class="node-cell" role="cell"><i class="row-lamp"></i><div class="node-button"><b>${escapeHtml(name)}</b><span>${escapeHtml(sub)}</span></div></div>
    <div class="status ${status}" role="cell"><i></i><span class="status-text">${statusText}</span></div>
    <div class="flap-cell measure cpu" role="cell"><span class="measure-label">CPU</span><div class="flap" data-value="—"><span class="flap-face">—</span></div></div>
    <div class="flap-cell measure mem" role="cell"><span class="measure-label">内存</span><div class="flap" data-value="—"><span class="flap-face">—</span></div></div>
    <div class="gpu-strip" role="cell" aria-colspan="8"><div class="pending-note"><span>${note}</span>${remove}</div></div>
    <div class="num-cell load-cell" role="cell">—<span>LOAD</span></div>
    <div class="num-cell up-cell" role="cell">—<span>UPTIME</span></div>
  </div>`;
}

function reconcilePendingServers() {
  if (!state.pendingServers.length) return;
  const removed = [];
  state.pendingServers = state.pendingServers.filter(pending => {
    const target = pending.nodeId || pending.host;
    const found = state.nodes.some(node => node.node_id === target || node.hostname === target
      || node.ip_address === pending.host || node.node_id === pending.host);
    if (found) removed.push(target);
    return !found;
  });
  if (removed.length) {
    savePendingServers();
    toast(`节点 ${removed[0]} 已接入。`);
  }
}

function removePendingServer(id) {
  state.pendingServers = state.pendingServers.filter(pending => pending.id !== id);
  savePendingServers();
  render();
  toast('已移除待接入服务器。');
}

function defaultServerAddr() {
  const host = location.hostname;
  const publicNode = state.nodes.map(node => node.ip_address)
    .find(address => address && !address.startsWith('127.') && address !== '::1');
  const base = host && host !== 'localhost' && host !== '127.0.0.1' && host !== '::1'
    ? host
    : publicNode || '127.0.0.1';
  return `${base}:10203`;
}

function openAddServer() {
  if (!accessToken()) {
    state.addServerAfterLogin = true;
    openLogin('添加服务器需要 ClusterScope 管理员账号（应用内角色），不是系统管理员。');
    return;
  }
  const dialog = $('#add-server-dialog');
  const form = $('#add-server-form');
  if (!dialog || !form) return;
  formError(form, '');
  const address = form.querySelector('[name="server_addr"]');
  if (address && !address.value) address.value = defaultServerAddr();
  const httpWarning = form.querySelector('[data-http-warning]');
  if (httpWarning) httpWarning.hidden = location.protocol === 'https:';
  dialog.showModal();
  requestAnimationFrame(() => form.querySelector('[name="username"]')?.focus());
}

function setAddServerInstalling(installing) {
  state.addServerInstalling = installing;
  const dialog = $('#add-server-dialog');
  const form = $('#add-server-form');
  if (form) {
    form.querySelectorAll('input, button').forEach(element => { element.disabled = installing; });
    const submit = form.querySelector('[type="submit"]');
    if (submit) submit.textContent = installing ? '正在登录并安装…' : '登录并安装';
  }
  if (dialog) dialog.setAttribute('aria-busy', String(installing));
}

async function submitAddServer(event) {
  event.preventDefault();
  if (state.addServerInstalling) return;
  const form = event.currentTarget;
  formError(form, '');
  const data = new FormData(form);
  const username = String(data.get('username') || '').trim();
  const host = String(data.get('host') || '').trim();
  const password = String(data.get('password') || '');
  const port = Number(data.get('port')) || 22;
  const nodeId = String(data.get('node_id') || '').trim();
  const serverAddr = String(data.get('server_addr') || '').trim();
  if (!username || !host || !password || !serverAddr) {
    formError(form, '用户名、IP、密码和 Server 地址都需要填写。');
    return;
  }

  const pending = {
    id: globalThis.crypto?.randomUUID?.() || `pending-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    username, host, port, nodeId, serverAddr,
    status: 'installing',
    createdAt: Date.now(),
  };
  state.pendingServers.push(pending);
  savePendingServers();
  render();
  setAddServerInstalling(true);

  try {
    const result = await request('/api/servers/install', {
      method: 'POST',
      body: JSON.stringify({ username, host, password, port, node_id: nodeId, server_addr: serverAddr }),
    });
    pending.status = 'waiting';
    pending.nodeId = result?.node_id || pending.nodeId;
    savePendingServers();
    render();
    toast(`已安装 ${result?.node_id || host}，等待 Agent 上报。`);
    form.reset();
    $('#add-server-dialog')?.close();
    await refresh();
  } catch (error) {
    state.pendingServers = state.pendingServers.filter(item => item.id !== pending.id);
    savePendingServers();
    render();
    if (error.status === 401) {
      formError(form, '安装需要管理员登录。');
      openLogin('登录已失效。请重新登录管理员账号后继续安装。');
    } else if (error.status === 403) {
      formError(form, '当前账号不是 ClusterScope 管理员，不能添加服务器。');
    } else {
      formError(form, error.message);
    }
  } finally {
    setAddServerInstalling(false);
  }
}

function overviewHtml() {
  if (!state.nodes.length && !state.pendingServers.length) return emptyHtml(false);
  const { online, totalGpus } = summaryData();
  const pendingCount = state.pendingServers.length;
  return `<div class="overview">
    ${renderError()}
    <div class="view-head">
      <div><h1>集群状态</h1><p>${state.nodes.length} 节点 · ${totalGpus} GPU · ${online} 在线${pendingCount ? ` · ${pendingCount} 待接入` : ''}</p></div>
      <div class="view-actions">${addServerButton()}<div class="view-meta" data-view-meta>${viewMeta()}</div></div>
    </div>
    <section class="summary" id="summary-slot" aria-label="集群摘要">${summaryCells()}</section>
    <section class="board-shell">
      <section class="board" role="table" aria-label="节点与 GPU 状态">
        ${boardHead()}
        ${state.nodes.map(renderBoardRow).join('')}${state.pendingServers.map(renderPendingRow).join('')}
      </section>
      <div class="board-foot" role="presentation"><div class="legend"><span><i class="ok"></i>在线</span><span><i class="warn"></i>不稳定</span><span><i class="bad"></i>离线</span></div><span>点选 GPU 查看进程 · 列宽固定，数据实时更新</span></div>
    </section>
    <section class="detail" id="detail-band" aria-label="选中节点详情">${detailPanes()}</section>
  </div>`;
}

function boardSignature() {
  const nodes = state.nodes.map(node => `${node.node_id}:${gpuCountFor(node)}`).join('|');
  const pending = state.pendingServers.map(item => `${item.id}:${item.status}:${item.nodeId}`).join('|');
  return `${nodes}||${pending}`;
}

function patchBoard() {
  const selected = selectedNode();
  document.querySelectorAll('[data-node-row]').forEach(row => {
    const node = state.nodes.find(item => item.node_id === row.dataset.nodeRow);
    if (!node) return;
    const metric = metricFor(node);
    const isSelected = selected?.node_id === node.node_id;
    const offline = node.status === 'offline';
    row.classList.toggle('selected', isSelected);
    row.classList.toggle('is-offline', offline);
    const nodeButton = row.querySelector('.node-button');
    nodeButton?.setAttribute('aria-pressed', String(isSelected));
    if (nodeButton) nodeButton.title = `${node.hostname || node.node_id} · ${nodeSub(node)}`;
    const sub = row.querySelector('[data-node-sub]');
    if (sub) sub.textContent = nodeSub(node);
    const status = row.querySelector('[data-status]');
    if (status) {
      status.className = `status ${node.status}`;
      const text = status.querySelector('.status-text');
      if (text) text.textContent = statusLabel(node.status);
    }
    const cpu = offline ? null : nodeCpu(node);
    const memory = offline ? null : nodeMemPercent(metric);
    const cpuText = cpu === null ? '—' : `${Math.round(cpu)}%`;
    const memText = memory === null ? '—' : `${Math.round(memory)}%`;
    const cpuCell = row.querySelector('[data-cell="cpu"]');
    const memCell = row.querySelector('[data-cell="mem"]');
    if (cpuCell) {
      const flap = cpuCell.querySelector('.flap');
      const before = Number.parseFloat(String(flap?.dataset.value || '').replace('%', ''));
      const moved = !Number.isFinite(before) || cpu === null || Math.abs(cpu - before) >= 3;
      flipCell(flap, cpuText, cpuTone(cpu), isSelected && moved);
      const bar = cpuCell.querySelector('.bar');
      if (bar) bar.style.width = `${clamp(cpu)}%`;
      cpuCell.classList.remove('hot', 'critical', 'idle');
      if (cpuTone(cpu)) cpuCell.classList.add(cpuTone(cpu));
    }
    if (memCell) {
      const flap = memCell.querySelector('.flap');
      const before = Number.parseFloat(String(flap?.dataset.value || '').replace('%', ''));
      const moved = !Number.isFinite(before) || memory === null || Math.abs(memory - before) >= 2;
      flipCell(flap, memText, memTone(memory), isSelected && moved);
      const bar = memCell.querySelector('.bar');
      if (bar) bar.style.width = `${clamp(memory)}%`;
      memCell.classList.remove('hot', 'critical', 'idle');
      if (memTone(memory)) memCell.classList.add(memTone(memory));
    }
    row.querySelectorAll('[data-gpu]').forEach(button => {
      const index = Number(button.dataset.gpu);
      const gpu = offline ? null : gpuAt(metric, index);
      const util = gpuUtil(gpu);
      const temp = gpuTemp(gpu);
      const tone = gpu ? gpuTone(gpu) : 'idle';
      const text = util === null ? '--' : `${gpuText(gpu)}%`;
      const gpuSelected = isSelected && index === state.selectedGpu;
      const flap = button.querySelector('.flap');
      const before = Number.parseFloat(flap?.dataset.value);
      const moved = !Number.isFinite(before) || util === null ? true : Math.abs(util - before) >= 5;
      flipCell(flap, text, tone, isSelected && (gpuSelected || moved));
      button.classList.toggle('selected', gpuSelected);
      button.setAttribute('aria-pressed', String(gpuSelected));
      button.setAttribute('aria-label', `${node.hostname || node.node_id} GPU ${index}，利用率 ${util === null ? '未知' : `${Math.round(util)}%`}，温度 ${temp === null ? '未知' : `${Math.round(temp)} 度`}`);
      button.title = `${gpu?.name || 'GPU'}${gpu ? ` · ${fmtBytes(gpu.memory_used_bytes, true)} / ${fmtBytes(gpu.memory_total_bytes, true)}` : ''}`;
      const sub = button.querySelector('.gpu-sub');
      if (sub) sub.textContent = `${index} · ${gpuTempText(gpu)}`;
    });
    const load = row.querySelector('[data-load]');
    if (load) load.innerHTML = `${offline ? '—' : nodeLoad(node)}<span>LOAD</span>`;
    const uptime = row.querySelector('[data-uptime]');
    if (uptime) uptime.innerHTML = `${offline ? '—' : fmtUptime(metric?.uptime_seconds)}<span>UPTIME</span>`;
  });
  if (state.impactNodeId) {
    const row = [...document.querySelectorAll('[data-node-row]')]
      .find(item => item.dataset.nodeRow === state.impactNodeId);
    if (row) {
      row.classList.add('impact');
      setTimeout(() => row.classList.remove('impact'), 1200);
    }
    state.impactNodeId = '';
  }
}

function patchDetail() {
  const band = $('#detail-band');
  if (!band) return;
  const active = document.activeElement;
  const activeKey = active && band.contains(active) ? active.dataset.focusKey : '';
  const visibleWrap = () => [...band.querySelectorAll('[data-scroll]')].find(element => element.offsetParent !== null);
  const wrap = visibleWrap();
  const scrollLeft = wrap?.scrollLeft || 0;
  band.innerHTML = detailPanes();
  if (activeKey) {
    const selector = `[data-focus-key="${window.CSS?.escape ? CSS.escape(activeKey) : activeKey}"]`;
    band.querySelector(selector)?.focus({ preventScroll: true });
  }
  const nextWrap = visibleWrap();
  if (nextWrap) nextWrap.scrollLeft = scrollLeft;
}

function patchOverview() {
  const summary = $('#summary-slot');
  if (summary) summary.innerHTML = summaryCells();
  const meta = $('[data-view-meta]');
  if (meta) meta.innerHTML = viewMeta();
  patchBoard();
  patchDetail();
  patchError();
}

function currentTrendSamples() {
  const node = selectedNode();
  if (!node) return [];
  const samples = state.historyRange === 'live'
    ? state.histories.get(node.node_id) || []
    : state.historySamples;
  if (state.historyRange === 'live' || node.status === 'offline') return samples;
  const metric = metricFor(node);
  if (!metric) return samples;
  const current = historySample(metric);
  if (!samples.length) return [current];
  const last = samples[samples.length - 1];
  if (last && Math.abs(current.timestamp - last.timestamp) < 5000) {
    return [...samples.slice(0, -1), current];
  }
  return [...samples, current];
}

function buildCharts(samples) {
  const long = ['24h', '7d'].includes(state.historyRange);
  const timestamps = samples.map(sample => sample.timestamp);
  const charts = [{
    id: 'cpu', name: 'CPU', type: 'cpu',
    values: samples.map(sample => sample.cpu),
    mem: samples.map(sample => sample.systemMem),
    timestamps, long,
  }];
  const gpuCount = Math.max(0, ...samples.map(sample => sample.gpus.length));
  if (gpuCount) {
    for (let index = 0; index < gpuCount; index += 1) {
      charts.push({
        id: `gpu-${index}`, name: `GPU ${index}`, type: 'gpu',
        values: samples.map(sample => sample.gpus[index]?.util ?? null),
        mem: samples.map(sample => sample.gpus[index]?.mem ?? null),
        timestamps, long,
      });
    }
  } else if (samples.some(sample => Number.isFinite(sample.gpuAverage))) {
    charts.push({
      id: 'gpu-avg', name: 'GPU 平均', type: 'gpu',
      values: samples.map(sample => sample.gpuAverage),
      mem: [], timestamps, long,
    });
  }
  return charts;
}

function renderChart(chart) {
  const validCount = chart.values.filter(Number.isFinite).length;
  const latest = [...chart.values].reverse().find(Number.isFinite);
  const latestMem = [...chart.mem].reverse().find(Number.isFinite);
  const utilPath = validCount >= 2 ? linePath(chart.values) : '';
  const memPath = validCount >= 2 ? linePath(chart.mem) : '';
  const area = validCount >= 2 ? areaPath(chart.values) : '';
  const latestText = latest === undefined ? '—' : `${Math.round(latest)}%`;
  const memText = latestMem === undefined ? '—' : `${Math.round(latestMem)}%`;
  const first = chart.timestamps[0];
  const last = chart.timestamps[chart.timestamps.length - 1];
  const valuesLabel = chart.type === 'cpu'
    ? `<span>占用 <b>${latestText}</b></span><span class="mem">内存 <b>${memText}</b></span>`
    : `<span>利用率 <b>${latestText}</b></span><span class="mem">显存 <b>${memText}</b></span>`;
  const aria = `${chart.name} ${chart.type === 'cpu' ? 'CPU 与内存' : '利用率与显存'}趋势`;
  return `<figure class="chart-card ${chart.type}" data-chart="${chart.id}" aria-label="${escapeHtml(aria)}">
    <div class="chart-head"><h3>${chart.name}</h3><div class="chart-values">${valuesLabel}</div></div>
    ${utilPath ? `<div class="chart-plot"><span class="y-label y100">100</span><span class="y-label y50">50</span><span class="y-label y0">0</span>
      <svg class="chart-svg" viewBox="0 0 600 180" preserveAspectRatio="none" role="img" aria-label="${escapeHtml(aria)}">
        <path class="chart-grid-line" d="M 10 45 H 590 M 10 90 H 590 M 10 135 H 590"/>
        <path class="chart-axis" d="M 10 8 V 172 H 590"/>
        ${area ? `<path class="chart-area ${chart.type}" d="${area}"/>` : ''}
        ${utilPath ? `<path class="chart-line ${chart.type === 'gpu' ? 'gpu' : ''}" d="${utilPath}"/>` : ''}
        ${memPath ? `<path class="chart-line mem" d="${memPath}"/>` : ''}
        <line class="chart-cross" x1="10" y1="8" x2="10" y2="172"/>
        <circle class="chart-dot" cx="10" cy="90" r="2.6"/>
        <rect class="chart-hit" x="10" y="8" width="580" height="164"/>
      </svg>
    </div>
    <div class="chart-x"><span>${fmtTime(first, chart.long)}</span><span>${fmtTime(last, chart.long)}</span></div>
    <div class="chart-tip" hidden></div>` : '<div class="chart-empty">等待采样</div>'}
  </figure>`;
}

function bindCharts() {
  document.querySelectorAll('[data-chart]').forEach(card => {
    const chart = state.chartCache.get(card.dataset.chart);
    if (!chart || !chart.values.some(Number.isFinite)) return;
    const svg = card.querySelector('.chart-svg');
    const hit = card.querySelector('.chart-hit');
    const cross = card.querySelector('.chart-cross');
    const dot = card.querySelector('.chart-dot');
    const tip = card.querySelector('.chart-tip');
    if (!svg || !hit || !cross || !dot || !tip) return;
    const show = event => {
      const rect = svg.getBoundingClientRect();
      if (!rect.width) return;
      const ratio = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
      const index = Math.round(ratio * (chart.values.length - 1));
      const value = chart.values[index];
      const x = 10 + (index / Math.max(1, chart.values.length - 1)) * 580;
      cross.setAttribute('x1', x);
      cross.setAttribute('x2', x);
      cross.style.opacity = value === null || value === undefined ? '0' : '1';
      if (value === null || value === undefined) {
        dot.style.opacity = '0';
      } else {
        const y = 170 - clamp(value) / 100 * 160;
        dot.setAttribute('cx', x);
        dot.setAttribute('cy', y);
        dot.style.opacity = '1';
      }
      const mem = chart.mem[index];
      const memLabel = chart.type === 'cpu' ? '内存' : '显存';
      tip.innerHTML = `${escapeHtml(fmtTime(chart.timestamps[index], chart.long))}<br><b>${value === null || value === undefined ? '—' : `${Math.round(value)}%`}</b>${Number.isFinite(mem) ? ` · ${memLabel} ${Math.round(mem)}%` : ''}`;
      const cardRect = card.getBoundingClientRect();
      const left = event.clientX - cardRect.left + 12;
      const top = event.clientY - cardRect.top - 12;
      tip.style.left = `${Math.min(cardRect.width - 108, Math.max(4, left))}px`;
      tip.style.top = `${Math.max(4, top)}px`;
      tip.hidden = false;
    };
    const hide = () => {
      cross.style.opacity = '0';
      dot.style.opacity = '0';
      tip.hidden = true;
    };
    hit.addEventListener('pointermove', show, { passive: true });
    hit.addEventListener('pointerleave', hide);
    hit.addEventListener('pointercancel', hide);
  });
}

function renderTrendBody() {
  const body = $('#trend-body');
  if (!body) return;
  const node = selectedNode();
  const meta = $('#trend-meta');
  if (!node) {
    body.innerHTML = '';
    if (meta) meta.textContent = '';
    return;
  }
  const samples = currentTrendSamples();
  const rangeLabel = RANGES.find(([value]) => value === state.historyRange)?.[1] || '实时';
  const countText = state.historyRange === 'live'
    ? '最近 60 次采样 · 约 3 分钟'
    : `${rangeLabel} · ${samples.length} 个采样点`;
  if (meta) {
    meta.innerHTML = `<span>节点 <b>${escapeHtml(node.hostname || node.node_id)}</b></span><span>${statusLabel(node.status)} · 运行 ${fmtUptime(metricFor(node)?.uptime_seconds)}</span><span>${countText}</span>`;
  }
  const coreHost = $('#trend-core');
  if (coreHost) coreHost.innerHTML = coreBand(metricFor(node), node.status === 'offline');
  const signature = `${state.historyRange}|${node.node_id}|${state.historyLoading}|${samples.length}|${samples.at(-1)?.timestamp || ''}`;
  if (state.trendDataSignature === signature) return;
  state.trendDataSignature = signature;
  if (state.historyLoading) {
    body.innerHTML = `<div class="chart-grid">${Array.from({ length: 4 }, () => '<div class="chart-card"><div class="chart-loading"><i></i><i></i><i></i></div></div>').join('')}</div>`;
    return;
  }
  if (!samples.length) {
    const live = state.historyRange === 'live';
    body.innerHTML = `<section class="empty-board" aria-label="还没有趋势数据"><div class="empty-grid" aria-hidden="true">${Array.from({ length: 24 }, (_, index) => `<i class="empty-cell" style="--i:${index}"></i>`).join('')}</div><div class="empty-message"><b>${live ? '正在等待第一组数据' : '这个时间段没有数据'}</b><p>${live ? '采样开始后，曲线会显示在这里。' : '换一个时间范围，或等 Agent 再次上报。'}</p></div></section>`;
    return;
  }
  const charts = buildCharts(samples);
  state.chartCache = new Map(charts.map(chart => [chart.id, chart]));
  body.innerHTML = `<div class="chart-grid">${charts.map(renderChart).join('')}</div>`;
  bindCharts();
}

function trendShellHtml() {
  if (!state.nodes.length) return emptyHtml(true);
  const node = selectedNode();
  const options = state.nodes.map(item => `<option value="${escapeHtml(item.node_id)}" ${item.node_id === node?.node_id ? 'selected' : ''}>${escapeHtml(item.hostname || item.node_id)}</option>`).join('');
  return `<div class="trend">
    ${renderError()}
    <div class="view-head">
      <div><h1>趋势</h1><p>CPU 与每块 GPU 的利用率、显存。</p></div>
      <div class="trend-controls">
        <label>节点<select id="trend-node" class="select" aria-label="趋势节点">${options}</select></label>
        <div class="segmented" role="group" aria-label="时间范围">${RANGES.map(([value, label]) => `<button type="button" data-range="${value}" aria-pressed="${state.historyRange === value}">${label}</button>`).join('')}</div>
      </div>
    </div>
    <div class="trend-meta" id="trend-meta"></div>
    <div class="trend-legend"><span><i></i>实线 · CPU 占用 / GPU 利用率</span><span><i class="dashed"></i>虚线 · 内存 / 显存</span></div>
    <div id="trend-core"></div>
    <div id="trend-body"></div>
  </div>`;
}

function patchTrendControls() {
  const select = $('#trend-node');
  const node = selectedNode();
  if (select && node && select.value !== node.node_id) select.value = node.node_id;
  document.querySelectorAll('[data-range]').forEach(button => {
    button.setAttribute('aria-pressed', String(button.dataset.range === state.historyRange));
  });
}

function renderTrendView() {
  const view = $('#app-view');
  const signature = state.nodes.map(node => node.node_id).join('|');
  if (!view.querySelector('.trend') || state.trendSignature !== signature) {
    view.innerHTML = trendShellHtml();
    state.trendSignature = signature;
    state.trendDataSignature = '';
  }
  patchTrendControls();
  patchError();
  renderTrendBody();
}

function renderShell() {
  document.querySelectorAll('.view-tab').forEach(button => {
    button.setAttribute('aria-selected', String(button.dataset.view === state.view));
  });
  $('#account-button').textContent = accessToken() ? '退出' : '登录';
}

function render() {
  renderShell();
  const view = $('#app-view');
  if (state.loading && !state.nodes.length) {
    view.innerHTML = loadingHtml();
    return;
  }
  if (state.view === 'overview') {
    const signature = boardSignature();
    if (!view.querySelector('.overview') || state.boardSignature !== signature) {
      view.innerHTML = overviewHtml();
      state.boardSignature = signature;
    } else {
      patchOverview();
    }
  } else {
    renderTrendView();
  }
}

function recordHistories() {
  for (const [nodeId, metric] of state.metrics) {
    const history = state.histories.get(nodeId) || [];
    const sample = historySample(metric);
    if (history.at(-1)?.timestamp !== sample.timestamp) history.push(sample);
    state.histories.set(nodeId, history.slice(-60));
  }
}

function detectOfflineNode() {
  for (const node of state.nodes) {
    if (state.statusesInitialized && node.status === 'offline' && state.nodeStatuses.get(node.node_id) !== 'offline') {
      state.impactNodeId = node.node_id;
    }
    state.nodeStatuses.set(node.node_id, node.status);
  }
  state.statusesInitialized = true;
}

function setConnection(online) {
  state.connected = online;
  const dot = $('#live-dot');
  if (dot) dot.className = `lamp ${online ? 'online' : 'polling'}`;
  const label = $('#live-label');
  const short = window.matchMedia('(max-width: 640px)').matches;
  if (label) label.textContent = online
    ? (short ? '实时' : '实时 · WS')
    : (short ? '轮询' : '轮询 · 3s');
  const source = $('#footer-source');
  if (source) source.textContent = online ? 'WS · 3s' : 'REST · 3s';
  const sr = $('#sr-status');
  if (sr) sr.textContent = online ? '实时连接已建立。' : '实时连接不可用，已切换到轮询。';
}

async function refresh({ manual = false } = {}) {
  if (state.refreshing) return;
  state.refreshing = true;
  state.nextRefreshAt = Date.now() + REFRESH_MS;
  countdown?.reset();
  const button = $('#refresh-button');
  if (button) button.disabled = true;
  if (manual) spinRefresh(button);
  try {
    const nodes = await request('/api/nodes');
    const metricRows = await Promise.all(nodes.map(node => request(`/api/nodes/${encodeURIComponent(node.node_id)}/metrics`)));
    state.nodes = nodes;
    state.metrics = new Map();
    nodes.forEach((node, index) => {
      const metric = normalizeMetrics(metricRows[index]);
      if (metric) state.metrics.set(node.node_id, metric);
    });
    if (!state.nodes.some(node => node.node_id === state.selectedNodeId)) {
      state.selectedNodeId = state.nodes[0]?.node_id || '';
      state.selectedGpu = 0;
    }
    const node = selectedNode();
    if (node && state.selectedGpu >= gpuCountFor(node)) state.selectedGpu = 0;
    recordHistories();
    detectOfflineNode();
    reconcilePendingServers();
    state.error = '';
    state.lastUpdated = Date.now();
  } catch (error) {
    state.error = error.message;
    if (error.status === 401) openLogin();
  } finally {
    state.loading = false;
    state.refreshing = false;
    if (button) button.disabled = false;
    render();
  }
}

function selectNode(nodeId, gpuIndex = null) {
  const node = state.nodes.find(item => item.node_id === nodeId);
  if (!node) return;
  const changed = state.selectedNodeId !== nodeId;
  if (changed) {
    state.selectedGpu = 0;
    if (state.historyRange !== 'live') state.historySamples = [];
    state.trendDataSignature = '';
  }
  state.selectedNodeId = nodeId;
  if (gpuIndex !== null && Number.isFinite(Number(gpuIndex))) {
    state.selectedGpu = Math.max(0, Math.min(gpuCountFor(node) - 1, Number(gpuIndex)));
  }
  render();
}

function navigate(view, animate = true) {
  if (!VIEWS.includes(view)) return;
  if (view === state.view) { render(); return; }
  const oldIndex = VIEWS.indexOf(state.view);
  const newIndex = VIEWS.indexOf(view);
  state.view = view;
  state.chartCache = new Map();
  state.trendDataSignature = '';
  history.pushState(null, '', `#${view}`);
  render();
  if (view === 'trend' && state.historyRange !== 'live' && !state.historySamples.length && !state.historyLoading) {
    loadHistory();
  }
  if (animate) pageTransition($('#app-view'), newIndex < oldIndex ? -1 : 1);
  window.scrollTo({ top: 0, behavior: 'instant' });
}

async function loadHistory() {
  const node = selectedNode();
  if (!node || state.historyRange === 'live') return;
  const durations = { '1h': 3600000, '24h': 86400000, '7d': 604800000 };
  const end = Date.now();
  const start = end - durations[state.historyRange];
  const requestId = ++state.historyRequest;
  const nodeId = node.node_id;
  state.historyLoading = true;
  renderTrendBody();
  try {
    const rows = await request(`/api/metrics/history?node_id=${encodeURIComponent(nodeId)}&start_time_ms=${start}&end_time_ms=${end}`);
    if (requestId !== state.historyRequest) return;
    state.historySamples = rows.map(historySample);
    state.error = '';
  } catch (error) {
    if (requestId === state.historyRequest) { state.error = error.message; toast(error.message); }
  } finally {
    if (requestId === state.historyRequest) {
      state.historyLoading = false;
      render();
    }
  }
}

function changeRange(range) {
  if (!RANGES.some(([value]) => value === range)) return;
  state.historyRange = range;
  state.historyRequest += 1;
  state.historyLoading = false;
  state.trendDataSignature = '';
  if (range === 'live') {
    state.historySamples = [];
    patchTrendControls();
    renderTrendBody();
  } else {
    patchTrendControls();
    loadHistory();
  }
}

function handleViewClick(event) {
  const target = event.target.closest('button');
  if (!target) return;
  if (target.dataset.pendingRemove !== undefined) {
    removePendingServer(target.dataset.pendingRemove);
    return;
  }
  if (target.dataset.addServer !== undefined) {
    openAddServer();
    return;
  }
  if (target.dataset.gpu !== undefined) {
    selectNode(target.dataset.node, Number(target.dataset.gpu));
    return;
  }
  if (target.dataset.node) {
    selectNode(target.dataset.node);
    return;
  }
  if (target.dataset.gpuRow !== undefined) {
    const node = selectedNode();
    if (node) selectNode(node.node_id, Number(target.dataset.gpuRow));
    return;
  }
  if (target.dataset.processMode) {
    state.processMode = target.dataset.processMode;
    patchDetail();
    return;
  }
  if (target.dataset.range) {
    changeRange(target.dataset.range);
    return;
  }
  if (target.dataset.retry !== undefined) {
    refresh({ manual: true });
  }
}

function formError(form, message) {
  const error = form.querySelector('.form-error');
  if (!error) return;
  error.hidden = !message;
  error.textContent = message || '';
}

function openLogin(message = '') {
  const dialog = $('#login-dialog');
  if (!dialog || dialog.open) return;
  const subtitle = $('#login-subtitle');
  if (subtitle) {
    subtitle.textContent = message || 'Server 开启认证后，需要登录才能查看集群。';
  }
  dialog.showModal();
  dialog.querySelector('input[name="username"]')?.focus();
}

function setupEvents() {
  disconnectEvents();
  let lastPush = 0;
  disconnectEvents = connectEvents({
    onStatus(online) { setConnection(online); },
    onMessage(message) {
      if (message.type === 'metrics_update' && Date.now() - lastPush > 800) {
        lastPush = Date.now();
        refresh();
      }
    },
  });
}

function bindEvents() {
  document.querySelectorAll('.view-tab').forEach(button => {
    button.addEventListener('click', () => navigate(button.dataset.view));
  });
  $('#app-view').addEventListener('click', handleViewClick);
  $('#app-view').addEventListener('change', event => {
    if (event.target.id === 'trend-node') {
      selectNode(event.target.value);
      if (state.historyRange !== 'live') loadHistory();
      else renderTrendBody();
    }
  });
  $('#add-server-form')?.addEventListener('submit', submitAddServer);
  $('#add-server-dialog')?.addEventListener('cancel', event => {
    if (state.addServerInstalling) event.preventDefault();
  });
  $('#refresh-button').addEventListener('click', () => refresh({ manual: true }));
  $('#account-button').addEventListener('click', async () => {
    if (!accessToken()) {
      openLogin();
      return;
    }
    clearSession();
    disconnectEvents();
    setupEvents();
    toast('已退出登录。');
    await refresh();
  });
  document.querySelectorAll('.close-dialog').forEach(button => {
    button.addEventListener('click', () => button.closest('dialog')?.close());
  });
  // Cancelling the login prompt must not leave "open the add-server dialog"
  // armed for the next, unrelated login.
  $('#login-dialog')?.addEventListener('close', () => {
    if (!accessToken()) state.addServerAfterLogin = false;
  });
  $('#login-form').addEventListener('submit', async event => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const submit = form.querySelector('[type="submit"]');
    submit.disabled = true;
    submit.textContent = '正在登录…';
    formError(form, '');
    try {
      await login(data.get('username'), data.get('password'));
      form.reset();
      $('#login-dialog')?.close();
      disconnectEvents();
      setupEvents();
      toast('登录成功。');
      await refresh();
      if (state.addServerAfterLogin) {
        state.addServerAfterLogin = false;
        openAddServer();
      }
    } catch (error) {
      formError(form, error.status === 401 ? '用户名或密码不正确，请重试。' : error.message);
    } finally {
      submit.disabled = false;
      submit.textContent = '登录';
    }
  });
  window.addEventListener('popstate', () => {
    const view = VIEWS.includes(location.hash.slice(1)) ? location.hash.slice(1) : 'overview';
    navigate(view, false);
  });
  window.addEventListener('resize', () => {
    if (state.connected !== null) setConnection(state.connected);
  });
  document.addEventListener('keydown', event => {
    if (event.ctrlKey || event.metaKey || event.altKey || /INPUT|TEXTAREA|SELECT/.test(event.target.tagName) || document.querySelector('dialog[open]')) return;
    const key = event.key.toLowerCase();
    if (event.target.closest('.view-tabs') && ['arrowleft', 'arrowright'].includes(key)) {
      const tabs = [...document.querySelectorAll('.view-tab')];
      const index = tabs.indexOf(document.activeElement);
      const next = tabs[(index + (key === 'arrowleft' ? -1 : 1) + tabs.length) % tabs.length];
      if (next) {
        event.preventDefault();
        next.focus();
        navigate(next.dataset.view, false);
      }
      return;
    }
    if (key === '1' || key === '2') { navigate(VIEWS[Number(key) - 1], false); return; }
    if (key === 'r') { refresh({ manual: true }); return; }
    if (key === 'p' && state.view === 'overview') {
      state.processMode = state.processMode === 'gpu' ? 'cpu' : 'gpu';
      patchDetail();
      return;
    }
    if (['h', 'l', 'arrowleft', 'arrowright'].includes(key)) {
      const index = state.nodes.findIndex(node => node.node_id === selectedNode()?.node_id);
      const direction = ['h', 'arrowleft'].includes(key) ? -1 : 1;
      const next = state.nodes[index + direction];
      if (next) {
        event.preventDefault();
        selectNode(next.node_id);
        if (state.view === 'trend' && state.historyRange !== 'live') loadHistory();
      }
      return;
    }
    if (['j', 'k', 'arrowdown', 'arrowup'].includes(key) && state.view === 'overview') {
      const count = gpuCountFor(selectedNode());
      const direction = ['j', 'arrowdown'].includes(key) ? 1 : -1;
      state.selectedGpu = Math.max(0, Math.min(count - 1, state.selectedGpu + direction));
      event.preventDefault();
      render();
    }
  });
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden && Date.now() >= state.nextRefreshAt) refresh();
  });
}

function tickClock() {
  const element = $('#clock');
  if (!element) return;
  const now = new Date();
  element.textContent = fmtClock(now);
  element.dateTime = now.toISOString();
}

async function init() {
  if (!location.hash) history.replaceState(null, '', '#overview');
  state.pendingServers = loadPendingServers();
  bindEvents();
  setupEvents();
  tickClock();
  setInterval(tickClock, 1000);
  countdown = createCountdown($('#countdown'), () => (state.nextRefreshAt - Date.now()) / 1000);
  await refresh();
  if (state.view === 'trend' && state.historyRange !== 'live') await loadHistory();
  setInterval(() => {
    if (!document.hidden && Date.now() >= state.nextRefreshAt) refresh();
  }, 250);
}

init();
