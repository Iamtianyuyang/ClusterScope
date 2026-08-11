use crate::api::{AlertEvent, AlertRule, GpuInfo, Job, Node, NodeMetrics};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table,
    },
    Frame,
};
use std::collections::{HashMap, VecDeque};

// ===== palette (single source of truth) =====
// fg: normal data · dim: auxiliary · teal: selection/focus · green: online ·
// yellow: warning/busy/temp-high · red: critical/offline/error
const TEAL: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const NORMAL: Color = Color::White;

// ===== thresholds =====
const TEMP_WARN_C: f64 = 65.0;
const TEMP_CRIT_C: f64 = 80.0;
const MEM_WARN_PCT: f64 = 70.0;
const MEM_CRIT_PCT: f64 = 90.0;
const BUSY_PCT: f64 = 1.0; // GPU considered "busy" above this

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Nodes,
    Jobs,
    Alerts,
    Trend,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Nodes, Tab::Jobs, Tab::Alerts, Tab::Trend];
    pub fn title(self) -> &'static str {
        match self {
            Tab::Nodes => "Overview",
            Tab::Jobs => "Jobs",
            Tab::Alerts => "Alerts",
            Tab::Trend => "Trend",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Tab::Nodes => 0,
            Tab::Jobs => 1,
            Tab::Alerts => 2,
            Tab::Trend => 3,
        }
    }
    pub fn from_index(i: usize) -> Self {
        Tab::ALL[i % Tab::ALL.len()]
    }
}

/// Rolling history per node (last ~2 min at 3s refresh).
/// `gpus[i]` = utilization curve of GPU i; `gpus_mem[i]` = its memory used %.
pub struct NodeHistory {
    pub cpu: VecDeque<f64>,
    pub gpus: Vec<VecDeque<f64>>,
    pub gpus_mem: Vec<VecDeque<f64>>,
}

impl NodeHistory {
    fn new() -> Self {
        Self { cpu: VecDeque::new(), gpus: Vec::new(), gpus_mem: Vec::new() }
    }
    fn push(&mut self, cpu: f64, gpus: &[(f64, f64)]) {
        self.cpu.push_back(cpu);
        if self.cpu.len() > HISTORY_LEN {
            self.cpu.pop_front();
        }
        for (i, &(util, mem)) in gpus.iter().enumerate() {
            if i >= self.gpus.len() {
                self.gpus.push(VecDeque::new());
                self.gpus_mem.push(VecDeque::new());
            }
            self.gpus[i].push_back(util);
            if self.gpus[i].len() > HISTORY_LEN {
                self.gpus[i].pop_front();
            }
            self.gpus_mem[i].push_back(mem);
            if self.gpus_mem[i].len() > HISTORY_LEN {
                self.gpus_mem[i].pop_front();
            }
        }
    }
}

pub const HISTORY_LEN: usize = 60;

pub struct AppState {
    pub tab: Tab,
    pub nodes: Vec<Node>,
    pub metrics: HashMap<String, NodeMetrics>,
    pub history: HashMap<String, NodeHistory>,
    pub jobs: Vec<Job>,
    pub rules: Vec<AlertRule>,
    pub events: Vec<AlertEvent>,
    pub selected_node: usize,
    pub selected_gpu: usize, // within selected node
    pub help: bool,
    pub error: Option<String>,
    pub last_refresh_at: std::time::Instant,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            tab: Tab::Nodes,
            nodes: vec![],
            metrics: HashMap::new(),
            history: HashMap::new(),
            jobs: vec![],
            rules: vec![],
            events: vec![],
            selected_node: 0,
            selected_gpu: 0,
            help: false,
            error: None,
            last_refresh_at: std::time::Instant::now(),
        }
    }

    pub async fn refresh(&mut self, api: &crate::api::Api) {
        match self.refresh_inner(api).await {
            Ok(()) => {
                self.error = None;
                self.last_refresh_at = std::time::Instant::now();
            }
            Err(e) => self.error = Some(format!("{}", e)),
        }
    }

    async fn refresh_inner(&mut self, api: &crate::api::Api) -> anyhow::Result<()> {
        let nodes = api.nodes().await?;
        let mut metrics = HashMap::new();
        for n in &nodes {
            if let Some(m) = api.node_metrics(&n.node_id).await? {
                metrics.insert(n.node_id.clone(), m);
            }
        }
        let jobs = api.jobs().await?;
        let rules = api.alert_rules().await?;
        let events = api.alert_events().await?;

        for (id, m) in &metrics {
            let gpu_pairs: Vec<(f64, f64)> = m.gpus.iter().map(|g| {
                let mem_pct = if g.memory_total_bytes > 0 {
                    g.memory_used_bytes as f64 / g.memory_total_bytes as f64 * 100.0
                } else {
                    0.0
                };
                (g.utilization_gpu, mem_pct)
            }).collect();
            self.history
                .entry(id.clone())
                .or_insert_with(NodeHistory::new)
                .push(m.cpu_usage_percent, &gpu_pairs);
        }

        if self.selected_node >= nodes.len() {
            self.selected_node = 0;
            self.selected_gpu = 0;
        }
        self.nodes = nodes;
        self.metrics = metrics;
        self.jobs = jobs;
        self.rules = rules;
        self.events = events;
        Ok(())
    }

    /// Processes running on the selected GPU of the selected node.
    pub fn selected_processes(&self) -> Vec<&crate::api::GpuProcess> {
        let Some(node) = self.nodes.get(self.selected_node) else { return vec![] };
        let Some(m) = self.metrics.get(&node.node_id) else { return vec![] };
        let Some(gpu) = m.gpus.get(self.selected_gpu) else { return vec![] };
        m.gpu_processes
            .iter()
            .filter(|p| p.gpu_uuid == gpu.uuid)
            .collect()
    }

    /// All GPU processes on the selected node (Trend tab: every user's
    /// programs across all GPUs, not just the selected card).
    pub fn node_processes(&self) -> Vec<&crate::api::GpuProcess> {
        let Some(node) = self.nodes.get(self.selected_node) else { return vec![] };
        let Some(m) = self.metrics.get(&node.node_id) else { return vec![] };
        m.gpu_processes.iter().collect()
    }
}

// ===== small helpers =====

fn temp_color(c: f64) -> Color {
    if c >= TEMP_CRIT_C {
        Color::Red
    } else if c >= TEMP_WARN_C {
        Color::Yellow
    } else {
        NORMAL
    }
}

fn mem_color(pct: f64) -> Color {
    if pct >= MEM_CRIT_PCT {
        Color::Red
    } else if pct >= MEM_WARN_PCT {
        Color::Yellow
    } else {
        NORMAL
    }
}

/// Busy indicator color: busy -> yellow/white, idle -> plain.
fn util_color(pct: f64) -> Color {
    if pct >= 95.0 {
        Color::Yellow
    } else if pct >= BUSY_PCT {
        NORMAL
    } else {
        DIM
    }
}

/// Compact VRAM: 38.2G · 612M · 0
fn fmt_vram(b: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if b >= GB {
        format!("{:.1}G", b as f64 / GB as f64)
    } else if b >= MB {
        format!("{:.0}M", b / MB)
    } else {
        "0".to_string()
    }
}

/// Compact total VRAM: 46G
fn fmt_total(b: u64) -> String {
    const GB: u64 = 1_073_741_824;
    format!("{:.0}G", b as f64 / GB as f64)
}

/// Mini activity bar: only filled when busy (`████`), empty when idle.
/// Width 6. Returns string of fixed display width.
fn mini_bar(pct: f64, width: usize) -> String {
    let filled = if pct >= BUSY_PCT {
        (pct.clamp(0.0, 100.0) / 100.0 * width as f64).round() as usize
    } else {
        0
    };
    format!("{:<width$}", "█".repeat(filled), width = width)
}

/// Cell density for the GPU table (adapts to panel height).
#[derive(Clone, Copy, PartialEq)]
enum CellMode {
    /// One GPU per row: bar + util + vram + temp (original layout).
    Full,
    /// Two per row: util + vram + temp.
    Medium,
    /// Three per row: util + vram.
    Short,
    /// Six per row: util + vram (used only).
    Tiny,
}

/// One GPU cell. Formats:
/// Full:   ` >0 ██████ 100% 10.0G/45G 42°`
/// Medium: ` >0 100% 10.0G/45G 42°`
/// Short:  ` >0 100% 10.0G/45G`
/// Tiny:   ` >0 100% 10.0G`
fn gpu_cell(g: &crate::api::GpuInfo, sel: bool, mode: CellMode) -> Vec<Span<'static>> {
    let idle = g.utilization_gpu < BUSY_PCT;
    let vram = format!("{}/{}", fmt_vram(g.memory_used_bytes), fmt_total(g.memory_total_bytes));
    let vram_used = fmt_vram(g.memory_used_bytes);
    let mem_pct = if g.memory_total_bytes > 0 {
        g.memory_used_bytes as f64 / g.memory_total_bytes as f64 * 100.0
    } else {
        0.0
    };

    let idx = Span::styled(
        format!(" {}{:<2}", if sel { ">" } else { " " }, g.index),
        Style::default().fg(if sel { TEAL } else { DIM }).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
    );
    let util = Span::styled(
        format!("{:>3.0}%", g.utilization_gpu),
        Style::default().fg(if idle { DIM } else { util_color(g.utilization_gpu) }).add_modifier(if !idle { Modifier::BOLD } else { Modifier::empty() }),
    );

    match mode {
        CellMode::Full => vec![
            idx,
            Span::styled(
                format!(" {:<6}", mini_bar(g.utilization_gpu, 6)),
                Style::default().fg(if idle { DIM } else { util_color(g.utilization_gpu) }),
            ),
            util,
            Span::styled(format!(" {:<7}", vram), Style::default().fg(mem_color(mem_pct))),
            Span::styled(format!(" {:>2}°", g.temperature_celsius as u64), Style::default().fg(temp_color(g.temperature_celsius))),
        ],
        CellMode::Medium => vec![
            idx,
            util,
            Span::styled(format!(" {:<8}", vram), Style::default().fg(mem_color(mem_pct))),
            Span::styled(format!(" {:>2}°", g.temperature_celsius as u64), Style::default().fg(temp_color(g.temperature_celsius))),
        ],
        CellMode::Short => vec![
            idx,
            util,
            Span::styled(format!(" {:<8}", vram), Style::default().fg(mem_color(mem_pct))),
        ],
        CellMode::Tiny => vec![
            idx,
            util,
            Span::styled(format!(" {:<5}", vram_used), Style::default().fg(mem_color(mem_pct))),
        ],
    }
}

// ===== draw =====

pub fn draw(f: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // topbar
            Constraint::Length(1), // nav
            Constraint::Length(1), // divider
            Constraint::Min(8),    // nodes
            Constraint::Length(2), // process header (title + divider)
            Constraint::Min(3),    // process table
            Constraint::Length(1), // footer
        ])
        .split(f.area());

    draw_topbar(f, app, chunks[0]);
    draw_nav(f, app, chunks[1]);
    draw_divider(f, chunks[2]);
    match app.tab {
        Tab::Nodes => draw_nodes(f, app, chunks[3]),
        Tab::Jobs => draw_jobs(f, app, chunks[3]),
        Tab::Alerts => draw_alerts(f, app, chunks[3]),
        Tab::Trend => draw_trend_full(f, app, chunks[3]),
    }
    draw_process(f, app, chunks[4], chunks[5]);
    draw_footer(f, app, chunks[6]);
}

// ---- TopBar ----

fn draw_topbar(f: &mut Frame, app: &AppState, area: Rect) {
    let total_gpus: usize = app.metrics.values().map(|m| m.gpus.len()).sum();
    let busy: usize = app
        .metrics
        .values()
        .flat_map(|m| m.gpus.iter())
        .filter(|g| g.utilization_gpu >= BUSY_PCT)
        .count();
    let ago = app.last_refresh_at.elapsed().as_secs();

    let mut spans: Vec<Span> = vec![
        Span::styled(
            " ClusterScope",
            Style::default().fg(NORMAL).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("     {} nodes · {} GPUs · {} busy · {} jobs", app.nodes.len(), total_gpus, busy, app.jobs.len()),
            Style::default().fg(DIM),
        ),
    ];
    if app.rules.is_empty() && app.events.is_empty() {
        spans.push(Span::styled("     0 alerts", Style::default().fg(DIM)));
    } else {
        // `events` holds the active (pending/firing) alerts from the server.
        let active = app.events.len();
        spans.push(Span::styled(
            format!("     ! {} alert{}", active, if active == 1 { "" } else { "s" }),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!("     LIVE · {}s", ago),
        Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---- Navigation ----

fn draw_nav(f: &mut Frame, app: &AppState, area: Rect) {
    let mut spans = Vec::new();
    for (i, t) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        let selected = app.tab.index() == i;
        spans.push(Span::styled(
            t.title().to_string(),
            if selected {
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(DIM)
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(DIM),
        ))),
        area,
    );
}

// ---- Node Panels (horizontal) ----

fn draw_nodes(f: &mut Frame, app: &AppState, area: Rect) {
    if app.nodes.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " no servers registered — install agents on your nodes",
                Style::default().fg(DIM),
            ))),
            area,
        );
        return;
    }

    let w = area.width;
    let cols = if w >= 150 { 3 } else if w >= 102 { 2 } else { 1 };
    let rows = app.nodes.len().div_ceil(cols).max(1);
    let col_w = area.width / cols as u16;
    let row_h = area.height / rows as u16;

    for (i, node) in app.nodes.iter().enumerate() {
        let c = i % cols;
        let r = i / cols;
        let panel_area = Rect {
            x: area.x + c as u16 * col_w,
            y: area.y + r as u16 * row_h,
            width: col_w,
            height: row_h,
        };
        let selected = i == app.selected_node;
        node_panel(f, app, node, panel_area, selected);
    }
}

fn node_panel(f: &mut Frame, app: &AppState, node: &Node, area: Rect, selected: bool) {
    let m = app.metrics.get(&node.node_id);
    let gpus: Vec<&GpuInfo> = m.map(|m| m.gpus.iter().collect()).unwrap_or_default();

    let host = if node.hostname.is_empty() { &node.node_id } else { &node.hostname };
    let border_style = if selected {
        Style::default().fg(TEAL)
    } else {
        Style::default().fg(DIM)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            format!(" {} ", host),
            if selected {
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let avail = inner.height as usize; // content rows available

    let mut lines: Vec<Line<'static>> = Vec::new();

    // ---- priority allocation for limited heights ----
    // status (1) -> GPU table (header + rows, selection-first) -> summary -> meta
    let status = node.status.to_uppercase();
    let status_span = match node.status.as_str() {
        "online" => Span::styled(
            format!("● {}", status),
            Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
        ),
        "offline" => Span::styled(
            format!("○ {}", status),
            Style::default().fg(DIM).add_modifier(Modifier::DIM),
        ),
        s => Span::styled(format!("● {}", s.to_uppercase()), Style::default().fg(Color::Yellow)),
    };
    lines.push(Line::from(status_span));
    let mut used = 1;

    let cpu = m.map(|m| m.cpu_usage_percent).unwrap_or(0.0);
    let mem_used = m.map(|m| m.memory_used_bytes).unwrap_or(0);
    let mem_total = m.map(|m| m.memory_total_bytes).unwrap_or(0);
    let mem_pct = if mem_total > 0 { mem_used as f64 / mem_total as f64 * 100.0 } else { 0.0 };
    let gpu_avg = if gpus.is_empty() { 0.0 } else { gpus.iter().map(|g| g.utilization_gpu).sum::<f64>() / gpus.len() as f64 };
    let load = m.map(|m| m.load_1).unwrap_or(0.0);

    // GPU table: density adapts to the panel height so every GPU stays
    // visible — 1 per row (full detail), else 2/3/6 per row (compact).
    if gpus.is_empty() {
        if avail > used {
            lines.push(Line::from(Span::styled(" no GPU data", Style::default().fg(DIM))));
            used += 1;
        }
    } else if avail >= 2 {
        let n = gpus.len();
        let rows_avail = (avail - used).max(1) as usize;
        let per_row = n.div_ceil(rows_avail).clamp(1, n.max(1));
        let mode = match per_row {
            1 => CellMode::Full,
            2 => CellMode::Medium,
            3 => CellMode::Short,
            _ => CellMode::Tiny,
        };

        // Roomier panels keep the original single-column table with header.
        if mode == CellMode::Full && avail - used >= n + 1 {
            lines.push(Line::from(vec![
                Span::styled(format!(" GPU  UTIL      VRAM   TEMP"), Style::default().fg(DIM)),
            ]));
            used += 1;
        }

        let mut gi = 0usize;
        while gi < n {
            let mut row: Vec<Span> = Vec::new();
            for c in 0..per_row {
                if gi >= n {
                    break;
                }
                let sel = selected && gi == app.selected_gpu;
                row.extend(gpu_cell(&gpus[gi], sel, mode));
                if c + 1 < per_row && gi + 1 < n {
                    row.push(Span::raw("  "));
                }
                gi += 1;
            }
            lines.push(Line::from(row));
            used += 1;
        }
    }

    // summary (CPU/MEM/GPU) if space remains
    if avail > used {
        let mut sum = Vec::new();
        for (label, pct) in [("CPU", cpu), ("MEM", mem_pct), ("GPU", gpu_avg)] {
            sum.push(Span::styled(format!(" {:<3}", label), Style::default().fg(NORMAL)));
            sum.push(Span::styled(format!("{:>3.0}%", pct), Style::default().fg(if pct >= 90.0 { Color::Yellow } else { NORMAL })));
            if w >= 34 {
                sum.push(Span::styled(
                    format!(" {}", mini_bar(pct, 3)),
                    Style::default().fg(if pct >= BUSY_PCT { Color::Yellow } else { DIM }),
                ));
            }
        }
        lines.push(Line::from(sum));
        used += 1;
    }

    // meta (IP · GPU · load) last — lowest priority
    if avail > used {
        lines.push(Line::from(Span::styled(
            format!(
                " {} · {} GPU · load {:.1}",
                if node.ip_address.is_empty() { "-" } else { &node.ip_address },
                gpus.len(),
                load
            ),
            Style::default().fg(DIM),
        )));
    }

    let p = Paragraph::new(lines);
    f.render_widget(p, inner);
}

// ---- Trend (per-GPU real-time charts) ----

const UTIL_COLOR: Color = Color::Yellow;
const MEM_COLOR: Color = Color::Cyan;
const CPU_COLOR: Color = Color::LightBlue;

/// Trend tab: one mini chart per GPU — two lines (utilization + memory used %)
/// — plus a CPU chart. `h`/`l` switches the node.
pub fn draw_trend_full(f: &mut Frame, app: &AppState, area: Rect) {
    let sel = app.nodes.get(app.selected_node);
    let host = sel
        .map(|n| if n.hostname.is_empty() { n.node_id.clone() } else { n.hostname.clone() })
        .unwrap_or_else(|| "-".to_string());

    // History is keyed by node_id; fall back to the cluster average.
    let (cpu, gpus_util, gpus_mem) = match sel.and_then(|n| app.history.get(&n.node_id)) {
        Some(h) if !h.cpu.is_empty() => (h.cpu.clone(), h.gpus.clone(), h.gpus_mem.clone()),
        _ => cluster_average_history(app),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);

    // Header: node + line legend.
    let header = Line::from(vec![
        Span::styled(format!(" Trend: {} ", host), Style::default().fg(NORMAL).add_modifier(Modifier::BOLD)),
        Span::styled("  U ── ", Style::default().fg(UTIL_COLOR).add_modifier(Modifier::BOLD)),
        Span::styled("util   ", Style::default().fg(DIM)),
        Span::styled("M ── ", Style::default().fg(MEM_COLOR).add_modifier(Modifier::BOLD)),
        Span::styled("mem   ", Style::default().fg(DIM)),
        Span::styled(format!("· {} samples · 3s · h/l node", cpu.len()), Style::default().fg(DIM)),
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    let grid = chunks[1];
    if cpu.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " collecting live metrics…",
                Style::default().fg(DIM),
            ))),
            grid,
        );
        return;
    }

    let cpu_pts: Vec<(f64, f64)> = cpu.iter().enumerate().map(|(i, v)| (i as f64, *v)).collect();
    let mut items: Vec<(String, Vec<(f64, f64)>, Vec<(f64, f64)>)> =
        vec![("CPU".to_string(), cpu_pts, vec![])];
    for i in 0..gpus_util.len() {
        let util_pts: Vec<(f64, f64)> =
            gpus_util[i].iter().enumerate().map(|(x, v)| (x as f64, *v)).collect();
        let mem_pts: Vec<(f64, f64)> = gpus_mem
            .get(i)
            .map(|m| m.iter().enumerate().map(|(x, v)| (x as f64, *v)).collect())
            .unwrap_or_default();
        items.push((format!("GPU{}", i), util_pts, mem_pts));
    }

    let cols = ((grid.width as usize) / 26).clamp(2, 4);
    let rows = items.len().div_ceil(cols).max(1);
    let cell_w = (grid.width / cols as u16).max(1);
    let cell_h = (grid.height / rows as u16).max(3);

    for (i, (name, util, mem)) in items.iter().enumerate() {
        let c = i % cols;
        let r = i / cols;
        let cell = Rect {
            x: grid.x + c as u16 * cell_w,
            y: grid.y + r as u16 * cell_h,
            width: cell_w,
            height: cell_h,
        };
        mini_chart(f, name, util, mem, cell);
    }
}

/// One chart per GPU: bordered block, title shows current values, two braille
/// lines — utilization (yellow) and memory used % (cyan). The CPU chart uses a
/// single LightBlue line.
fn mini_chart(f: &mut Frame, name: &str, util: &[(f64, f64)], mem: &[(f64, f64)], area: Rect) {
    let last_util = util.last().map(|p| p.1).unwrap_or(0.0);
    let last_mem = mem.last().map(|p| p.1).unwrap_or(0.0);

    let mut title = vec![Span::styled(
        format!(" {} ", name),
        Style::default().fg(NORMAL).add_modifier(Modifier::BOLD),
    )];
    if name == "CPU" {
        title.push(Span::styled(format!(" {:>3.0}%", last_util), Style::default().fg(CPU_COLOR)));
    } else {
        title.push(Span::styled(format!(" U {:>3.0}%", last_util), Style::default().fg(UTIL_COLOR)));
        title.push(Span::styled(format!(" M {:>3.0}%", last_mem), Style::default().fg(MEM_COLOR)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(title);

    let n = util.len().max(1) as f64;
    let line_color = if name == "CPU" { CPU_COLOR } else { UTIL_COLOR };
    let mut datasets = vec![
        Dataset::default()
            .name("util")
            .marker(Marker::Braille)
            .style(Style::default().fg(line_color))
            .graph_type(GraphType::Line)
            .data(util),
    ];
    if !mem.is_empty() {
        datasets.push(
            Dataset::default()
                .name("mem")
                .marker(Marker::Braille)
                .style(Style::default().fg(MEM_COLOR))
                .graph_type(GraphType::Line)
                .data(mem),
        );
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds([0.0, (n - 1.0).max(1.0)]))
        .y_axis(Axis::default().bounds([0.0, 100.0]));
    f.render_widget(chart, area);
}

/// Element-wise cluster average of all node histories (used when the selected
/// node has no history yet, e.g. right after startup). Per-GPU curves are
/// aligned by GPU index; a node without that GPU index simply contributes 0.
fn cluster_average_history(
    app: &AppState,
) -> (
    std::collections::VecDeque<f64>,
    Vec<std::collections::VecDeque<f64>>,
    Vec<std::collections::VecDeque<f64>>,
) {
    let mut cpu = std::collections::VecDeque::new();
    let mut gpus: Vec<std::collections::VecDeque<f64>> = Vec::new();
    let mut gpus_mem: Vec<std::collections::VecDeque<f64>> = Vec::new();
    let mut count = 0usize;
    for h in app.history.values() {
        if h.cpu.is_empty() {
            continue;
        }
        count += 1;
        for (i, v) in h.cpu.iter().enumerate() {
            if i >= cpu.len() {
                cpu.push_back(0.0);
            }
            cpu[i] += v;
        }
        for (gi, g) in h.gpus.iter().enumerate() {
            if gi >= gpus.len() {
                gpus.push(std::collections::VecDeque::new());
            }
            for (i, v) in g.iter().enumerate() {
                if i >= gpus[gi].len() {
                    gpus[gi].push_back(0.0);
                }
                gpus[gi][i] += v;
            }
        }
        for (gi, g) in h.gpus_mem.iter().enumerate() {
            if gi >= gpus_mem.len() {
                gpus_mem.push(std::collections::VecDeque::new());
            }
            for (i, v) in g.iter().enumerate() {
                if i >= gpus_mem[gi].len() {
                    gpus_mem[gi].push_back(0.0);
                }
                gpus_mem[gi][i] += v;
            }
        }
    }
    if count > 0 {
        for v in cpu.iter_mut() {
            *v /= count as f64;
        }
        for g in gpus.iter_mut() {
            for v in g.iter_mut() {
                *v /= count as f64;
            }
        }
        for g in gpus_mem.iter_mut() {
            for v in g.iter_mut() {
                *v /= count as f64;
            }
        }
    }
    (cpu, gpus, gpus_mem)
}

// ---- Process panel ----

fn draw_process(f: &mut Frame, app: &AppState, header_area: Rect, table_area: Rect) {
    // On the Trend tab show every user's processes across all GPUs; on the
    // other tabs keep the selected GPU's processes.
    let all_gpus = app.tab == Tab::Trend;

    // header: "Processes" + selected node/gpu, then a divider line
    let sel_node = app.nodes.get(app.selected_node);
    let (node_label, gpu_label) = match sel_node {
        Some(n) => {
            let host = if n.hostname.is_empty() { &n.node_id } else { &n.hostname };
            (
                host.clone(),
                if all_gpus { "all GPUs".to_string() } else { app.selected_gpu.to_string() },
            )
        }
        None => ("-".to_string(), "-".to_string()),
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" Processes", Style::default().fg(NORMAL).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("      {} / GPU {}", node_label, gpu_label),
                    Style::default().fg(DIM),
                ),
            ]),
            Line::from(Span::styled(
                "─".repeat(header_area.width as usize),
                Style::default().fg(DIM),
            )),
        ]),
        header_area,
    );

    let mut procs: Vec<&crate::api::GpuProcess> = if all_gpus {
        app.node_processes()
    } else {
        app.selected_processes()
    };
    let node_online = sel_node.map(|n| n.status == "online").unwrap_or(false);

    if !node_online {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " node offline",
                Style::default().fg(DIM),
            ))),
            table_area,
        );
        return;
    }
    if procs.is_empty() {
        // Distinguish: no processes vs unknown. Agent reports processes it could
        // see; when details are restricted the fields carry '?' / '<restricted>'.
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No active GPU processes.",
                Style::default().fg(DIM),
            ))),
            table_area,
        );
        return;
    }

    // GPU index lookup (uuid → index) for the all-GPUs view.
    let gpu_idx: std::collections::HashMap<String, String> = sel_node
        .and_then(|n| app.metrics.get(&n.node_id))
        .map(|m| m.gpus.iter().map(|g| (g.uuid.clone(), g.index.to_string())).collect())
        .unwrap_or_default();
    if all_gpus {
        procs.sort_by_key(|p| {
            gpu_idx
                .get(&p.gpu_uuid)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(i64::MAX)
        });
    }

    // table columns: [GPU] PID USER SM VRAM CPU COMMAND
    let mut rows: Vec<Row> = Vec::new();
    for p in &procs {
        let sm = p.sm_utilization.map(|v| format!("{:.0}%", v)).unwrap_or_else(|| "—".into());
        let user = if p.username.is_empty() || p.username == "unknown" { "—" } else { &p.username };
        let cmd = if p.command.is_empty() || p.command == "unknown" { "<restricted>" } else { &p.command };
        let mut cells: Vec<Cell> = Vec::new();
        if all_gpus {
            cells.push(Cell::from(Span::styled(
                format!("{:>3}", gpu_idx.get(&p.gpu_uuid).map(|s| s.as_str()).unwrap_or("?")),
                Style::default().fg(DIM),
            )));
        }
        cells.push(Cell::from(Span::styled(format!("{:>7}", p.pid), Style::default().fg(NORMAL))));
        cells.push(Cell::from(Span::styled(format!("{:<8}", truncate(user, 8)), Style::default().fg(NORMAL))));
        cells.push(Cell::from(Span::styled(format!("{:>5}", sm), Style::default().fg(if p.sm_utilization.map(|v| v >= 50.0).unwrap_or(false) { Color::Yellow } else { NORMAL }))));
        cells.push(Cell::from(Span::styled(format!("{:>7}", fmt_vram(p.gpu_memory_bytes)), Style::default().fg(NORMAL))));
        cells.push(Cell::from(Span::styled(format!("{:>5}", format!("{:.0}%", p.cpu_percent)), Style::default().fg(DIM))));
        cells.push(Cell::from(Span::styled(
            truncate(cmd, table_area.width.saturating_sub(if all_gpus { 46 } else { 41 }) as usize),
            Style::default().fg(NORMAL),
        )));
        rows.push(Row::new(cells));
    }

    let widths: Vec<Constraint> = if all_gpus {
        vec![
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Min(5),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Min(5),
        ]
    };
    let header_names: Vec<&str> = if all_gpus {
        vec!["GPU", "PID", "USER", "SM", "VRAM", "CPU", "COMMAND"]
    } else {
        vec!["PID", "USER", "SM", "VRAM", "CPU", "COMMAND"]
    };
    let table = Table::new(rows, widths).header(
        Row::new(header_names).style(Style::default().fg(DIM)),
    );
    f.render_widget(table, table_area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

// ---- Footer ----

fn draw_footer(f: &mut Frame, app: &AppState, area: Rect) {
    let text = if app.help {
        " j/k GPU    h/l node    Tab tabs    Enter details    r refresh    1/2/3/4 views    q quit "
    } else {
        " j/k GPU    h/l node    Tab tabs    r refresh    ? help    q quit "
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM)))),
        area,
    );

    // Surface refresh errors (e.g. server unreachable, auth issues).
    if let Some(err) = &app.error {
        let line = format!(" error: {}\n", err);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                line,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ))),
            Rect {
                x: 0,
                y: area.y.saturating_sub(1),
                width: area.width,
                height: 1,
            },
        );
    }
}

// ---- Jobs / Alerts (unchanged content, restyled) ----

fn draw_jobs(f: &mut Frame, app: &AppState, area: Rect) {
    let rows: Vec<Row> = app
        .jobs
        .iter()
        .map(|j| {
            let status_color = match j.status.as_str() {
                "running" | "starting" => Color::Green,
                "queued" => Color::Yellow,
                "failed" | "lost" => Color::Red,
                _ => DIM,
            };
            Row::new(vec![
                Cell::from(Span::raw(j.job_id.clone())),
                Cell::from(Span::raw(j.name.clone())),
                Cell::from(Span::raw(j.node_id.clone())),
                Cell::from(Span::styled(j.status.clone(), Style::default().fg(status_color))),
                Cell::from(Span::styled(j.executable.clone(), Style::default().fg(NORMAL))),
                Cell::from(Span::styled(crate::api::fmt_time(&j.created_at), Style::default().fg(DIM))),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(38),
            Constraint::Length(20),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(24),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["JOB ID", "NAME", "NODE", "STATUS", "EXECUTABLE", "CREATED"])
            .style(Style::default().fg(DIM)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Jobs "));
    f.render_widget(table, area);
}

fn draw_alerts(f: &mut Frame, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let rule_rows: Vec<Row> = app
        .rules
        .iter()
        .map(|r| {
            let sev_color = match r.severity.as_str() {
                "critical" => Color::Red,
                "warning" => Color::Yellow,
                "info" => TEAL,
                _ => DIM,
            };
            Row::new(vec![
                Cell::from(Span::raw(r.name.clone())),
                Cell::from(Span::raw(r.metric.clone())),
                Cell::from(Span::raw(r.operator.clone())),
                Cell::from(Span::raw(format!("{}", r.threshold))),
                Cell::from(Span::styled(r.severity.clone(), Style::default().fg(sev_color))),
                Cell::from(Span::styled(if r.enabled { "on" } else { "off" }, Style::default().fg(if r.enabled { Color::Green } else { DIM }))),
            ])
        })
        .collect();
    let rules_table = Table::new(
        rule_rows,
        [
            Constraint::Length(20),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new(vec!["NAME", "METRIC", "OP", "THRESHOLD", "SEVERITY", "ENABLED"])
            .style(Style::default().fg(DIM)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Rules "));
    f.render_widget(rules_table, chunks[0]);

    let event_rows: Vec<Row> = app
        .events
        .iter()
        .map(|e| {
            let state_color = match e.state.as_str() {
                "firing" => Color::Red,
                "pending" => Color::Yellow,
                "resolved" | "normal" => Color::Green,
                _ => DIM,
            };
            Row::new(vec![
                Cell::from(Span::raw(e.node_id.clone())),
                Cell::from(Span::raw(e.gpu_uuid.chars().take(12).collect::<String>())),
                Cell::from(Span::styled(e.state.clone(), Style::default().fg(state_color))),
                Cell::from(Span::raw(format!("{:.1}", e.current_value))),
                Cell::from(Span::styled(crate::api::fmt_time(&e.timestamp), Style::default().fg(DIM))),
            ])
        })
        .collect();
    let events_table = Table::new(
        event_rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["NODE", "GPU", "STATE", "VALUE", "TIME"])
            .style(Style::default().fg(DIM)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Events "));
    f.render_widget(events_table, chunks[1]);
}
