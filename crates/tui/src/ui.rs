use crate::api::{AlertEvent, AlertRule, GpuInfo, Job, Node, NodeMetrics};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
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
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Nodes, Tab::Jobs, Tab::Alerts];
    pub fn title(self) -> &'static str {
        match self {
            Tab::Nodes => "Overview",
            Tab::Jobs => "Jobs",
            Tab::Alerts => "Alerts",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Tab::Nodes => 0,
            Tab::Jobs => 1,
            Tab::Alerts => 2,
        }
    }
    pub fn from_index(i: usize) -> Self {
        Tab::ALL[i % Tab::ALL.len()]
    }
}

/// Rolling history per node (last ~2 min at 3s refresh).
pub struct NodeHistory {
    pub cpu: VecDeque<f64>,
    pub gpu: VecDeque<f64>,
}

impl NodeHistory {
    fn new() -> Self {
        Self { cpu: VecDeque::new(), gpu: VecDeque::new() }
    }
    fn push(&mut self, cpu: f64, gpu: f64) {
        self.cpu.push_back(cpu);
        self.gpu.push_back(gpu);
        if self.cpu.len() > HISTORY_LEN {
            self.cpu.pop_front();
        }
        if self.gpu.len() > HISTORY_LEN {
            self.gpu.pop_front();
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
            let gpu_avg = if m.gpus.is_empty() {
                0.0
            } else {
                m.gpus.iter().map(|g| g.utilization_gpu).sum::<f64>() / m.gpus.len() as f64
            };
            self.history
                .entry(id.clone())
                .or_insert_with(NodeHistory::new)
                .push(m.cpu_usage_percent, gpu_avg);
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

    // GPU table first (core info). Selection stays visible via windowing.
    if gpus.is_empty() {
        if avail > used {
            lines.push(Line::from(Span::styled(" no GPU data", Style::default().fg(DIM))));
            used += 1;
        }
    } else if avail >= 3 {
        lines.push(Line::from(vec![
            Span::styled(format!(" GPU  UTIL      VRAM   TEMP"), Style::default().fg(DIM)),
        ]));
        used += 1;
        let gpu_space = (avail - used).min(6).max(1);
        let start = if gpus.len() > gpu_space {
            (app.selected_gpu + 1).saturating_sub(gpu_space).min(gpus.len() - gpu_space)
        } else {
            0
        };
        for (gi, g) in gpus.iter().enumerate().skip(start).take(gpu_space) {
            let sel = selected && gi == app.selected_gpu;
            let idle = g.utilization_gpu < BUSY_PCT;
            let vram = format!("{}/{}", fmt_vram(g.memory_used_bytes), fmt_total(g.memory_total_bytes));
            let bar = mini_bar(g.utilization_gpu, 6);
            let row: Vec<Span> = vec![
                Span::styled(
                    format!(" {}{:<2}", if sel { ">" } else { " " }, g.index),
                    Style::default().fg(if sel { TEAL } else { DIM }).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(
                    format!(" {:<6}", bar),
                    Style::default().fg(if idle { DIM } else { util_color(g.utilization_gpu) }),
                ),
                Span::styled(
                    format!("{:>3.0}%", g.utilization_gpu),
                    Style::default().fg(if idle { DIM } else { util_color(g.utilization_gpu) }).add_modifier(if !idle { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(
                    format!(" {:<7}", vram),
                    Style::default().fg(mem_color(if g.memory_total_bytes > 0 { g.memory_used_bytes as f64 / g.memory_total_bytes as f64 * 100.0 } else { 0.0 })),
                ),
                Span::styled(
                    format!(" {:>2}°", g.temperature_celsius as u64),
                    Style::default().fg(temp_color(g.temperature_celsius)),
                ),
            ];
            lines.push(Line::from(row));
            used += 1;
        }
        if gpus.len() > gpu_space {
            lines.push(Line::from(Span::styled(
                format!(" {} more GPU… (j/k)", gpus.len() - gpu_space),
                Style::default().fg(DIM),
            )));
        }
    } else if avail == 2 {
        // very short panel: show only the selected GPU row (no header)
        if let Some(g) = gpus.get(app.selected_gpu.min(gpus.len() - 1)) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" >{:<2} {:>3.0}% {}/{} {:>2}°",
                        g.index,
                        g.utilization_gpu,
                        fmt_vram(g.memory_used_bytes),
                        fmt_total(g.memory_total_bytes),
                        g.temperature_celsius as u64),
                    Style::default().fg(TEAL),
                ),
            ]));
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

// ---- Process panel ----

fn draw_process(f: &mut Frame, app: &AppState, header_area: Rect, table_area: Rect) {
    // header: "Processes" + selected node/gpu, then a divider line
    let sel_node = app.nodes.get(app.selected_node);
    let (node_label, gpu_label) = match sel_node {
        Some(n) => {
            let host = if n.hostname.is_empty() { &n.node_id } else { &n.hostname };
            (host.clone(), app.selected_gpu.to_string())
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

    let procs = app.selected_processes();
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

    // table columns: PID USER SM VRAM CPU COMMAND
    let mut rows: Vec<Row> = Vec::new();
    for p in &procs {
        let sm = p.sm_utilization.map(|v| format!("{:.0}%", v)).unwrap_or_else(|| "—".into());
        let user = if p.username.is_empty() || p.username == "unknown" { "—" } else { &p.username };
        let cmd = if p.command.is_empty() || p.command == "unknown" { "<restricted>" } else { &p.command };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(format!("{:>7}", p.pid), Style::default().fg(NORMAL))),
            Cell::from(Span::styled(format!("{:<8}", truncate(user, 8)), Style::default().fg(NORMAL))),
            Cell::from(Span::styled(format!("{:>5}", sm), Style::default().fg(if p.sm_utilization.map(|v| v >= 50.0).unwrap_or(false) { Color::Yellow } else { NORMAL }))),
            Cell::from(Span::styled(format!("{:>7}", fmt_vram(p.gpu_memory_bytes)), Style::default().fg(NORMAL))),
            Cell::from(Span::styled(format!("{:>5}", format!("{:.0}%", p.cpu_percent)), Style::default().fg(DIM))),
            Cell::from(Span::styled(truncate(cmd, table_area.width.saturating_sub(45) as usize), Style::default().fg(NORMAL))),
        ]));
    }

    let widths = [
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Min(5),
    ];
    let table = Table::new(rows, widths).header(
        Row::new(vec!["PID", "USER", "SM", "VRAM", "CPU", "COMMAND"])
            .style(Style::default().fg(DIM)),
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
        " j/k GPU    h/l node    Tab tabs    Enter details    r refresh    1/2/3 views    q quit "
    } else {
        " j/k GPU    h/l node    Enter details    r refresh    ? help    q quit "
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM)))),
        area,
    );
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
