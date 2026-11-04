import { accessToken, clearSession, connectEvents, login, request } from '/api.js';
import {
  $, escapeHtml, fmtBytes, fmtPercent, fmtUptime, historySample,
  linePath, memoryPercent, normalizeMetrics, statusLabel,
} from '/lib.js';
import {
  createCountdown, impactStatus, initMagneticButtons,
  pageTransition, startLineBoil,
} from '/motion.js';

const PAGES = ['overview', 'trend'];
const state = {
  page: PAGES.includes(location.hash.slice(1)) ? location.hash.slice(1) : 'overview',
  nodes: [], metrics: new Map(), histories: new Map(),
  selectedNodeId: '', selectedGpu: 0, processMode: 'gpu',
  loading: true, refreshing: false, error: '', nextRefreshAt: Date.now() + 3000,
  animateEntrance: true, drawTrend: true, historyRange: 'live', historySamples: [],
  historyLoading: false, historyRequest: 0, nodeStatuses: new Map(), statusesInitialized: false,
  impactNodeId: '',
};

let disconnectEvents = () => {};
let toastTimer;
let countdown;

function toast(message) {
  const element = $('#toast');
  clearTimeout(toastTimer);
  element.textContent = message;
  element.hidden = false;
  toastTimer = setTimeout(() => { element.hidden = true; }, 3200);
}

function selectedNode() {
  return state.nodes.find(node => node.node_id === state.selectedNodeId) || state.nodes[0] || null;
}

function selectedMetrics() {
  const node = selectedNode();
  return node ? state.metrics.get(node.node_id) : null;
}

function emptyState(title, description) {
  return `<div class="empty-state"><canvas class="line-boil" width="539" height="208" aria-hidden="true"></canvas><h3>${escapeHtml(title)}</h3><p>${escapeHtml(description)}</p></div>`;
}

function stat(value, label) {
  return `<div class="stat"><strong class="stat-value">${escapeHtml(value)}</strong><span class="stat-label">${escapeHtml(label)}</span></div>`;
}

function renderShell() {
  document.querySelectorAll('.nav-item').forEach(button => {
    button.setAttribute('aria-selected', String(button.dataset.page === state.page));
  });
  const metrics = [...state.metrics.values()];
  const gpus = metrics.flatMap(metric => metric.gpus);
  const busy = gpus.filter(gpu => Number(gpu.utilization_gpu) >= 1).length;
  const online = state.nodes.filter(node => node.status === 'online').length;
  const cpuValues = metrics.map(metric => metric.cpu_usage_percent)
    .filter(value => value !== null && value !== undefined).map(Number).filter(Number.isFinite);
  const cpu = cpuValues.length ? `${Math.round(cpuValues.reduce((a, b) => a + b, 0) / cpuValues.length)}%` : '—';
  $('#cluster-stats').innerHTML = stat(online, `在线节点 / 共 ${state.nodes.length} 台`)
    + stat(gpus.length, '块 GPU') + stat(busy, '块忙碌') + stat(cpu, '平均 CPU');
  $('#account-button').textContent = accessToken() ? '已登录 · 退出' : '登录';
  const banner = $('#error-banner');
  banner.hidden = !state.error;
  banner.textContent = state.error;
}

function render() {
  renderShell();
  const view = $('#app-view');
  if (state.loading) {
    view.innerHTML = '<div class="loading-state"><span class="spinner"></span><p>正在看清集群…</p></div>';
    $('#process-section').innerHTML = '';
    return;
  }
  view.innerHTML = state.page === 'overview' ? renderOverview() : renderTrend();
  state.animateEntrance = false;
  state.drawTrend = false;
  renderProcesses();
  view.querySelectorAll('.line-boil').forEach(startLineBoil);
  if (state.impactNodeId) {
    const card = [...view.querySelectorAll('[data-node-card]')]
      .find(element => element.dataset.nodeCard === state.impactNodeId);
    requestAnimationFrame(() => impactStatus(card));
    state.impactNodeId = '';
  }
}

function renderOverview() {
  if (!state.nodes.length) {
    return emptyState('集群还很安静', '在计算节点启动 Agent 后，这里会亮起第一块卡。');
  }
  return `<div class="section-heading"><div><h2>节点</h2><p>点选一块 GPU，下方只看与它有关的进程。</p></div></div>
    <div class="node-grid">${state.nodes.map(renderNode).join('')}</div>`;
}

function renderCores(cores) {
  return cores.map(core => `<i class="core-block ${core.usage_percent >= 90 ? 'hot' : core.usage_percent >= 1 ? 'busy' : ''}" title="核心 ${core.core_id}：${fmtPercent(core.usage_percent)}"></i>`).join('');
}

function renderNode(node, nodeIndex) {
  const metric = state.metrics.get(node.node_id);
  const gpus = metric?.gpus || [];
  const selected = selectedNode()?.node_id === node.node_id;
  const cpu = metric?.cpu_usage_percent === null || metric?.cpu_usage_percent === undefined
    ? null : Number(metric.cpu_usage_percent);
  const memory = Number(metric?.memory_total_bytes) > 0
    ? Number(metric.memory_used_bytes) / Number(metric.memory_total_bytes) * 100 : null;
  const gpuAverage = gpus.length ? gpus.reduce((sum, gpu) => sum + (Number(gpu.utilization_gpu) || 0), 0) / gpus.length : null;
  const cores = metric?.cpu_cores || [];
  return `<article class="node-card ${selected ? 'selected' : ''} ${node.status === 'offline' ? 'offline' : ''} ${state.animateEntrance ? 'stagger-item' : ''}" style="--stagger:${nodeIndex}" data-node-card="${escapeHtml(node.node_id)}">
    <header class="node-header">
      <button class="node-select" data-node="${escapeHtml(node.node_id)}" aria-label="选择节点 ${escapeHtml(node.node_id)}"><h3>${escapeHtml(node.hostname || node.node_id)}</h3><p>${escapeHtml(node.ip_address || node.node_id)} · ${gpus.length} GPU · 运行 ${fmtUptime(metric?.uptime_seconds)}</p></button>
      <span class="status ${escapeHtml(node.status)}">${statusLabel(node.status)}</span>
    </header>
    <div class="gpu-list">${gpus.length ? gpus.map((gpu, index) => renderGpu(gpu, index, selected)).join('') : '<p class="muted">暂时没有 GPU 数据。</p>'}</div>
    ${cores.length ? `<div class="core-strip" title="每核 CPU 使用率" aria-label="每核 CPU 使用率">${renderCores(cores)}</div>` : ''}
    <div class="node-summary"><span>CPU <strong>${fmtPercent(cpu)}</strong></span><span>内存 <strong>${fmtPercent(memory)}</strong></span><span>GPU <strong>${fmtPercent(gpuAverage)}</strong></span><span>负载 <strong>${Number.isFinite(Number(metric?.load_1)) ? Number(metric.load_1).toFixed(1) : '—'}</strong></span></div>
  </article>`;
}

function renderGpu(gpu, index, nodeSelected) {
  const utilization = Number(gpu.utilization_gpu);
  const memory = memoryPercent(gpu);
  const memoryText = gpu.memory_used_bytes == null || gpu.memory_total_bytes == null
    ? '—' : `${fmtBytes(gpu.memory_used_bytes, true)} / ${fmtBytes(gpu.memory_total_bytes, true)}`;
  const temperature = gpu.temperature_celsius;
  const selected = nodeSelected && state.page === 'overview' && index === state.selectedGpu;
  return `<button class="gpu-row ${selected ? 'selected' : ''}" data-gpu="${index}" aria-pressed="${selected}" title="${escapeHtml(gpu.name || `GPU ${index}`)}">
    <span class="gpu-index">GPU ${gpu.index ?? index}</span>
    <span class="util-track"><span class="util-fill ${utilization < 1 ? 'idle' : ''}" style="width:${Math.max(0, Math.min(100, utilization || 0))}%"></span></span>
    <span class="util-value ${utilization >= 95 ? 'hot' : ''}">${fmtPercent(utilization)}</span>
    <span class="vram ${memory >= 90 ? 'critical' : memory >= 70 ? 'hot' : ''}">${memoryText}</span>
    <span class="temperature ${temperature >= 80 ? 'critical' : temperature >= 65 ? 'hot' : ''}">${temperature === null || temperature === undefined ? '—' : `${Math.round(temperature)}°`}</span>
  </button>`;
}

function renderProcesses() {
  const section = $('#process-section');
  if (!selectedNode()) { section.innerHTML = ''; return; }
  const node = selectedNode();
  const metric = selectedMetrics();
  const allGpus = state.page === 'trend';
  let content = '';
  if (node.status === 'offline') {
    content = '<div class="empty-state"><h3>节点已离线</h3><p>保留最后一次数据，等待 Agent 重新连接。</p></div>';
  } else if (state.processMode === 'cpu') {
    const processes = [...(metric?.cpu_processes || [])].sort((a, b) => b.cpu_percent - a.cpu_percent);
    content = processes.length ? `<div class="table-wrap"><table><thead><tr><th>PID</th><th>用户</th><th>CPU</th><th>内存</th><th>命令</th></tr></thead><tbody>${processes.map(process => `<tr><td class="mono">${process.pid}</td><td>${escapeHtml(process.username || '—')}</td><td>${fmtPercent(process.cpu_percent)}</td><td>${fmtBytes(process.memory_bytes, true)}</td><td class="command-cell" title="${escapeHtml(process.command)}">${escapeHtml(process.command || '<受限>')}</td></tr>`).join('')}</tbody></table></div>`
      : '<div class="empty-state"><h3>正在采样</h3><p>Agent 每几秒读取一次 CPU 进程，请稍等。</p></div>';
  } else {
    let processes = metric?.gpu_processes || [];
    if (!allGpus) {
      const gpu = metric?.gpus?.[state.selectedGpu];
      processes = processes.filter(process => process.gpu_uuid === gpu?.uuid);
    }
    const gpuIndexes = new Map((metric?.gpus || []).map((gpu, index) => [gpu.uuid, gpu.index ?? index]));
    content = processes.length ? `<div class="table-wrap"><table><thead><tr>${allGpus ? '<th>GPU</th>' : ''}<th>PID</th><th>用户</th><th>SM</th><th>显存</th><th>CPU</th><th>命令</th></tr></thead><tbody>${processes.map(process => `<tr>${allGpus ? `<td>${gpuIndexes.get(process.gpu_uuid) ?? '—'}</td>` : ''}<td class="mono">${process.pid}</td><td>${escapeHtml(process.username || '—')}</td><td>${process.sm_utilization === null || process.sm_utilization === undefined ? '—' : fmtPercent(process.sm_utilization)}</td><td>${fmtBytes(process.gpu_memory_bytes, true)}</td><td>${fmtPercent(process.cpu_percent)}</td><td class="command-cell" title="${escapeHtml(process.command)}">${escapeHtml(process.command || '<受限>')}</td></tr>`).join('')}</tbody></table></div>`
      : '<div class="empty-state"><h3>这块 GPU 正在休息</h3><p>没有正在运行的 GPU 进程。</p></div>';
  }
  section.innerHTML = `<div class="process-panel"><div class="section-heading" style="margin-top:0"><div><h2>进程</h2><p>${escapeHtml(node.hostname || node.node_id)} · ${state.processMode === 'cpu' ? 'CPU 占用最高' : allGpus ? '全部 GPU' : `GPU ${state.selectedGpu}`}</p></div><div class="segmented" aria-label="进程类型"><button data-process-mode="gpu" aria-pressed="${state.processMode === 'gpu'}">GPU 进程</button><button data-process-mode="cpu" aria-pressed="${state.processMode === 'cpu'}">Top CPU</button></div></div>${content}</div>`;
}

function currentTrendSamples() {
  const node = selectedNode();
  if (!node) return [];
  return state.historyRange === 'live' ? state.histories.get(node.node_id) || [] : state.historySamples;
}

function renderTrend() {
  const nodeOptions = state.nodes.map(node => `<option value="${escapeHtml(node.node_id)}" ${node.node_id === selectedNode()?.node_id ? 'selected' : ''}>${escapeHtml(node.hostname || node.node_id)}</option>`).join('');
  const ranges = [['live', '实时'], ['1h', '1 小时'], ['24h', '24 小时'], ['7d', '7 天']];
  let content;
  if (!state.nodes.length) content = emptyState('还没有趋势', '节点上线后，每一次忙碌都会留下轨迹。');
  else if (state.historyLoading) content = '<div class="loading-state"><span class="spinner"></span><p>正在整理历史数据…</p></div>';
  else content = renderCharts(currentTrendSamples());
  return `<div class="page-heading"><div><h2>趋势</h2><p>利用率与显存，各走一条清晰的线。</p></div><div class="trend-controls"><select id="trend-node" class="select" aria-label="趋势节点">${nodeOptions}</select><div class="segmented">${ranges.map(([value, label]) => `<button data-range="${value}" aria-pressed="${state.historyRange === value}">${label}</button>`).join('')}</div></div></div>
    <div class="legend"><span><i></i>性能 / 利用率</span><span><i class="memory"></i>显存</span><span class="muted">${state.historyRange === 'live' ? '最近 60 次采样' : '历史保留策略自动合并'}</span></div>
    <div class="trend-cores core-strip">${renderCores(selectedMetrics()?.cpu_cores || [])}</div>${content}`;
}

function renderCharts(samples) {
  if (!samples.length) return emptyState('正在收集第一批样本', '过几秒再来，曲线会从这里生长。');
  const charts = [{ name: 'CPU', values: samples.map(sample => sample.cpu), memory: [], type: 'cpu' }];
  const gpuCount = Math.max(0, ...samples.map(sample => sample.gpus.length));
  for (let index = 0; index < gpuCount; index += 1) {
    charts.push({ name: `GPU ${index}`, values: samples.map(sample => sample.gpus[index]?.util), memory: samples.map(sample => sample.gpus[index]?.mem), type: 'gpu' });
  }
  if (!gpuCount && samples.some(sample => Number.isFinite(sample.gpuAverage))) {
    charts.push({ name: 'GPU 平均', values: samples.map(sample => sample.gpuAverage), memory: [], type: 'gpu' });
  }
  return `<div class="chart-grid ${state.drawTrend ? 'draw-once' : ''}">${charts.map(renderChart).join('')}</div>`;
}

function renderChart(chart) {
  const latest = [...chart.values].reverse().find(Number.isFinite);
  const latestMemory = [...chart.memory].reverse().find(Number.isFinite);
  const utilPath = linePath(chart.values);
  const memoryPath = linePath(chart.memory);
  return `<figure class="chart-card"><figcaption><strong>${chart.name}</strong><span>利用率 ${fmtPercent(latest)}</span>${chart.memory.length ? `<span>显存 ${fmtPercent(latestMemory)}</span>` : ''}</figcaption>
    <svg class="chart-svg" viewBox="0 0 600 180" role="img" aria-label="${chart.name} 利用率趋势">
      <path class="chart-grid-line" d="M 10 45 H 590 M 10 90 H 590 M 10 135 H 590"/>
      <path class="chart-axis" pathLength="1" d="M 10 10 V 170 H 590"/>
      ${utilPath ? `<path class="chart-line ${chart.type === 'cpu' ? 'cpu' : ''}" pathLength="1" d="${utilPath}"/>` : ''}
      ${memoryPath ? `<path class="chart-line memory" pathLength="1" d="${memoryPath}"/>` : ''}
    </svg></figure>`;
}

async function refresh({ manual = false } = {}) {
  if (state.refreshing) return;
  state.refreshing = true;
  state.nextRefreshAt = Date.now() + 3000;
  countdown?.reset();
  const button = $('#refresh-button');
  button.disabled = true;
  if (manual && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    button.querySelector('svg').animate([{ transform: 'rotate(0)' }, { transform: 'rotate(360deg)' }], { duration: 500, easing: 'ease-out' });
  }
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
    recordHistories();
    detectOfflineNode();
    state.error = '';
  } catch (error) {
    state.error = error.message;
    if (error.status === 401) $('#login-dialog').showModal();
  } finally {
    state.loading = false;
    state.refreshing = false;
    button.disabled = false;
    render();
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

function navigate(page) {
  if (!PAGES.includes(page)) return;
  if (page === state.page) { render(); return; }
  const oldIndex = PAGES.indexOf(state.page), newIndex = PAGES.indexOf(page);
  state.page = page;
  state.drawTrend = page === 'trend';
  history.pushState(null, '', `#${page}`);
  render();
  pageTransition($('#app-view'), newIndex < oldIndex ? -1 : 1);
  window.scrollTo({ top: 0, behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' });
}

async function loadHistory() {
  const node = selectedNode();
  if (!node || state.historyRange === 'live') return;
  const durations = { '1h': 3600000, '24h': 86400000, '7d': 604800000 };
  const end = Date.now(), start = end - durations[state.historyRange];
  const requestId = ++state.historyRequest;
  const nodeId = node.node_id;
  state.historyLoading = true;
  state.drawTrend = true;
  render();
  try {
    const rows = await request(`/api/metrics/history?node_id=${encodeURIComponent(nodeId)}&start_time_ms=${start}&end_time_ms=${end}`);
    if (requestId !== state.historyRequest) return;
    state.historySamples = rows.map(historySample);
    state.error = '';
  } catch (error) {
    if (requestId === state.historyRequest) { state.error = error.message; toast(error.message); }
  } finally {
    if (requestId === state.historyRequest) { state.historyLoading = false; state.drawTrend = true; render(); }
  }
}

async function handleViewClick(event) {
  const target = event.target.closest('button');
  if (!target) return;
  if (target.dataset.node) { state.selectedNodeId = target.dataset.node; state.selectedGpu = 0; render(); }
  if (target.dataset.gpu !== undefined) { state.selectedGpu = Number(target.dataset.gpu); render(); }
  if (target.dataset.range) {
    state.historyRange = target.dataset.range; state.drawTrend = true;
    state.historyRequest += 1; state.historyLoading = false;
    if (state.historyRange === 'live') render(); else await loadHistory();
  }
}

function bindEvents() {
  document.querySelectorAll('.nav-item').forEach(button => button.addEventListener('click', () => navigate(button.dataset.page)));
  $('#app-view').addEventListener('click', handleViewClick);
  $('#process-section').addEventListener('click', event => {
    const button = event.target.closest('[data-process-mode]');
    if (button) { state.processMode = button.dataset.processMode; renderProcesses(); }
  });
  $('#app-view').addEventListener('change', event => {
    if (event.target.id === 'trend-node') {
      state.selectedNodeId = event.target.value; state.selectedGpu = 0; state.drawTrend = true;
      state.historyRequest += 1; state.historyLoading = false;
      if (state.historyRange === 'live') render(); else loadHistory();
    }
  });
  $('#refresh-button').addEventListener('click', () => refresh({ manual: true }));
  $('#account-button').addEventListener('click', async () => {
    if (!accessToken()) { $('#login-dialog').showModal(); return; }
    clearSession(); disconnectEvents(); setupEvents(); toast('已退出登录。'); await refresh();
  });
  document.querySelectorAll('.close-dialog').forEach(button => button.addEventListener('click', () => button.closest('dialog').close()));
  $('#login-form').addEventListener('submit', async event => {
    event.preventDefault();
    const form = event.currentTarget, data = new FormData(form), submit = form.querySelector('[type="submit"]');
    submit.disabled = true; submit.textContent = '正在登录…'; formError(form, '');
    try {
      await login(data.get('username'), data.get('password'));
      form.reset(); $('#login-dialog').close(); disconnectEvents(); setupEvents(); toast('登录成功，欢迎回来。'); await refresh();
    } catch (error) { formError(form, error.status === 401 ? '用户名或密码不正确，请重试。' : error.message); }
    finally { submit.disabled = false; submit.textContent = '登录'; }
  });
  window.addEventListener('popstate', () => navigate(location.hash.slice(1) || 'overview'));
  document.addEventListener('keydown', event => {
    if (event.ctrlKey || event.metaKey || event.altKey || /INPUT|TEXTAREA|SELECT/.test(event.target.tagName) || document.querySelector('dialog[open]')) return;
    if (['1', '2'].includes(event.key)) navigate(PAGES[Number(event.key) - 1]);
    if (event.key.toLowerCase() === 'r') refresh({ manual: true });
    if (event.key.toLowerCase() === 'p') { state.processMode = state.processMode === 'gpu' ? 'cpu' : 'gpu'; renderProcesses(); }
    if (['h', 'l', 'ArrowLeft', 'ArrowRight'].includes(event.key)) {
      const index = state.nodes.findIndex(node => node.node_id === selectedNode()?.node_id);
      const direction = ['h', 'ArrowLeft'].includes(event.key) ? -1 : 1;
      const next = state.nodes[index + direction];
      if (next) { event.preventDefault(); state.selectedNodeId = next.node_id; state.selectedGpu = 0; state.drawTrend = true; render(); }
    }
    if (['j', 'k', 'ArrowDown', 'ArrowUp'].includes(event.key) && state.page === 'overview') {
      const count = selectedMetrics()?.gpus.length || 0;
      const direction = ['j', 'ArrowDown'].includes(event.key) ? 1 : -1;
      state.selectedGpu = Math.max(0, Math.min(count - 1, state.selectedGpu + direction));
      event.preventDefault(); render();
    }
  });
}

function formError(form, message) {
  const error = form.querySelector('.form-error');
  error.hidden = !message; error.textContent = message || '';
}

function setupEvents() {
  disconnectEvents();
  let lastPush = 0;
  disconnectEvents = connectEvents({
    onStatus(online) {
      $('#live-dot').className = `live-dot ${online ? 'online' : 'offline'}`;
      $('#live-label').textContent = online ? '实时连接' : '轮询连接';
    },
    onMessage(message) {
      if (message.type === 'metrics_update' && Date.now() - lastPush > 800) {
        lastPush = Date.now(); refresh();
      }
    },
  });
}

async function init() {
  if (!location.hash) history.replaceState(null, '', '#overview');
  bindEvents();
  initMagneticButtons();
  countdown = createCountdown($('#countdown'), () => (state.nextRefreshAt - Date.now()) / 1000);
  setupEvents();
  await refresh();
  setInterval(() => {
    if (!document.hidden && Date.now() >= state.nextRefreshAt) refresh();
  }, 250);
}

init();
