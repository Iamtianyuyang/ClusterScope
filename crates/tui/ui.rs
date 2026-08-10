use crate::api::{Api, AlertEvent, AlertRule, Job, Node, NodeMetrics};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Gauge, Paragraph, Row, Table, Tabs,
    },
    Frame,
};

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
            Tab::Nodes => " Nodes ",
            Tab::Jobs => " Jobs ",
            Tab::Alerts => " Alerts ",
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

pub struct AppState {
    pub tab: Tab,
    pub nodes: Vec<Node>,
    pub metrics: std::collections::HashMap<String, NodeMetrics>,
    pub jobs: Vec<Job>,
    pub rules: Vec<AlertRule>,
    pub events: Vec<AlertEvent>,
    pub selected: usize,
    pub error: Option<String>,
    pub last_refresh: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            tab: Tab::Nodes,
            nodes: vec![],
            metrics: std::collections::HashMap::new(),
            jobs: vec![],
            rules: vec![],
            events: vec![],
            selected: 0,
            error: None,
            last_refresh: String::new(),
        }
    }

    pub async fn refresh(&mut self, api: &Api) {
        match self.refresh_inner(api).await {
            Ok(()) => {
                self.error = None;
                self.last_refresh = chrono::Utc::now().format("%H:%M:%S").to_string();
            }
            Err(e) => self.error = Some(format!("{}", e)),
        }
    }

    async fn refresh_inner(&mut self, api: &Api) -> anyhow::Result<()> {
        let nodes = api.nodes().await?;
        let mut metrics = std::collections::HashMap::new();
        for n in &nodes {
            if let Some(m) = api.node_metrics(&n.node_id).await? {
                metrics.insert(n.node_id.clone(), m);
            }
        }
        let jobs = api.jobs().await?;
        let rules = api.alert_rules().await?;
        let events = api.alert_events().await?;

        if self.selected >= nodes.len() {
            self.selected = 0;
        }
        self.nodes = nodes;
        self.metrics = metrics;
        self.jobs = jobs;
        self.rules = rules;
        self.events = events;
        Ok(())
    }
}

pub fn draw(f: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Title bar
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " ClusterScope ",
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(format!(
            "{} nodes | {} jobs | refresh {}",
            app.nodes.len(),
            app.jobs.len(),
            app.last_refresh
        )),
    ]));
    f.render_widget(title, chunks[0]);

    // Tabs
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| t.title().to_string()).collect())
        .select(app.tab.index())
        .style(Style::default().fg(Color::Cyan))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    f.render_widget(tabs, chunks[1]);

    // Error line
    if let Some(err) = &app.error {
        let p = Paragraph::new(Line::from(vec![
            Span::styled(" error: ", Style::default().fg(Color::White).bg(Color::Red)),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
        f.render_widget(p, chunks[3]);
    } else {
        let p = Paragraph::new(Line::from(vec![
            Span::styled(
                " Tab: switch  ↑↓: select  q: quit ",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        f.render_widget(p, chunks[3]);
    }

    match app.tab {
        Tab::Nodes => draw_nodes(f, app, chunks[2]),
        Tab::Jobs => draw_jobs(f, app, chunks[2]),
        Tab::Alerts => draw_alerts(f, app, chunks[2]),
    }
}

fn status_style(status: &str) -> Style {
    match status {
        "online" => Style::default().fg(Color::Green),
        "degraded" => Style::default().fg(Color::Yellow),
        "offline" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::DarkGray),
    }
}

fn draw_nodes(f: &mut Frame, app: &AppState, area: Rect) {
    let rows: Vec<Row> = app
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let m = app.metrics.get(&n.node_id);
            let cpu = m.map(|m| m.cpu_usage_percent).unwrap_or(0.0);
            let mem_used = m.map(|m| m.memory_used_bytes).unwrap_or(0);
            let mem_total = m.map(|m| m.memory_total_bytes).unwrap_or(0);
            let mem_pct = if mem_total > 0 { mem_used as f64 / mem_total as f64 * 100.0 } else { 0.0 };
            let gpu_str = m
                .map(|m| {
                    if m.gpus.is_empty() {
                        format!("{}", n.gpu_count)
                    } else {
                        let active = m.gpus.iter().filter(|g| g.utilization_gpu > 1.0).count();
                        format!("{}/{}", active, m.gpus.len())
                    }
                })
                .unwrap_or_else(|| format!("{}", n.gpu_count));
            let style = if i == app.selected {
                Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(Span::styled(n.node_id.clone(), style)),
                Cell::from(Span::styled(n.hostname.clone(), style)),
                Cell::from(Span::styled(n.status.clone(), status_style(&n.status).patch(style))),
                Cell::from(Span::styled(gpu_str, style)),
                Cell::from(Span::styled(format!("{:.0}%", cpu), style)),
                Cell::from(Span::styled(format!("{:.0}%", mem_pct), style)),
                Cell::from(Span::styled(
                    crate::api::fmt_time(&n.last_seen),
                    style,
                )),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(18),
        Constraint::Length(20),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    let table = Table::new(
        rows,
        widths,
    )
    .header(
        Row::new(vec!["NODE", "HOSTNAME", "STATUS", "GPU", "CPU", "MEM", "LAST SEEN"])
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Nodes "));

    let (table_area, detail_area) = if area.height > 14 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    f.render_widget(table, table_area);

    // Detail panel for selected node
    if let Some(detail) = detail_area {
        if let Some(node) = app.nodes.get(app.selected) {
            draw_node_detail(f, app, node, detail);
        }
    }
}

fn draw_node_detail(f: &mut Frame, app: &AppState, node: &Node, area: Rect) {
    let m = app.metrics.get(&node.node_id);
    let mut lines: Vec<Line> = vec![];

    if let Some(m) = m {
        let mem_pct = if m.memory_total_bytes > 0 {
            m.memory_used_bytes as f64 / m.memory_total_bytes as f64 * 100.0
        } else {
            0.0
        };
        lines.push(Line::from(vec![
            Span::styled("CPU ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{:.1}%", m.cpu_usage_percent)),
            Span::raw("   "),
            Span::styled("LOAD ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{:.2} {:.2} {:.2}", m.load_1, m.load_5, m.load_15)),
            Span::raw("   "),
            Span::styled("MEM ", Style::default().fg(Color::Cyan)),
            Span::raw(format!(
                "{} / {} ({:.1}%)",
                crate::api::fmt_bytes(m.memory_used_bytes),
                crate::api::fmt_bytes(m.memory_total_bytes),
                mem_pct
            )),
            Span::raw("   "),
            Span::styled("UPTIME ", Style::default().fg(Color::Cyan)),
            Span::raw(crate::api::fmt_uptime(m.uptime_seconds)),
        ]));
        lines.push(Line::from(""));

        if m.gpus.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no GPU metrics",
                Style::default().fg(Color::DarkGray),
            )));
        }
        for g in &m.gpus {
            let util_color = if g.utilization_gpu > 90.0 {
                Color::Red
            } else if g.utilization_gpu > 60.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            let mem_pct = if g.memory_total_bytes > 0 {
                g.memory_used_bytes as f64 / g.memory_total_bytes as f64 * 100.0
            } else {
                0.0
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" GPU {} ", g.index),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ),
                Span::raw(format!(" {}  ", g.name)),
                Span::styled(
                    format!("util {:.0}%", g.utilization_gpu),
                    Style::default().fg(util_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    "  mem {:.0}% ({}/{})",
                    mem_pct,
                    crate::api::fmt_bytes(g.memory_used_bytes),
                    crate::api::fmt_bytes(g.memory_total_bytes)
                )),
                Span::raw(format!(
                    "  temp {:.0}C  power {:.0}W",
                    g.temperature_celsius, g.power_watts
                )),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "no metrics yet",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(format!(" {} ", node.node_id)));
    f.render_widget(p, area);
}

fn draw_jobs(f: &mut Frame, app: &AppState, area: Rect) {
    let rows: Vec<Row> = app
        .jobs
        .iter()
        .map(|j| {
            let status_color = match j.status.as_str() {
                "running" | "starting" => Color::Green,
                "queued" => Color::Yellow,
                "failed" | "lost" => Color::Red,
                "succeeded" => Color::Blue,
                _ => Color::DarkGray,
            };
            Row::new(vec![
                Cell::from(Span::raw(j.job_id.clone())),
                Cell::from(Span::raw(j.name.clone())),
                Cell::from(Span::raw(j.node_id.clone())),
                Cell::from(Span::styled(j.status.clone(), Style::default().fg(status_color))),
                Cell::from(Span::raw(j.executable.clone())),
                Cell::from(Span::raw(crate::api::fmt_time(&j.created_at))),
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
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Jobs "));

    f.render_widget(table, area);
}

fn draw_alerts(f: &mut Frame, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Rules table
    let rule_rows: Vec<Row> = app
        .rules
        .iter()
        .map(|r| {
            let sev_color = match r.severity.as_str() {
                "critical" => Color::Red,
                "warning" => Color::Yellow,
                "info" => Color::Blue,
                _ => Color::DarkGray,
            };
            Row::new(vec![
                Cell::from(Span::raw(r.name.clone())),
                Cell::from(Span::raw(r.metric.clone())),
                Cell::from(Span::raw(r.operator.clone())),
                Cell::from(Span::raw(format!("{}", r.threshold))),
                Cell::from(Span::styled(r.severity.clone(), Style::default().fg(sev_color))),
                Cell::from(Span::styled(
                    if r.enabled { "on" } else { "off" },
                    Style::default().fg(if r.enabled { Color::Green } else { Color::DarkGray }),
                )),
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
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Rules "));
    f.render_widget(rules_table, chunks[0]);

    // Events table
    let event_rows: Vec<Row> = app
        .events
        .iter()
        .map(|e| {
            let state_color = match e.state.as_str() {
                "firing" => Color::Red,
                "pending" => Color::Yellow,
                "resolved" | "normal" => Color::Green,
                _ => Color::DarkGray,
            };
            Row::new(vec![
                Cell::from(Span::raw(e.node_id.clone())),
                Cell::from(Span::raw(e.gpu_uuid.chars().take(12).collect::<String>())),
                Cell::from(Span::styled(e.state.clone(), Style::default().fg(state_color))),
                Cell::from(Span::raw(format!("{:.1}", e.current_value))),
                Cell::from(Span::raw(crate::api::fmt_time(&e.timestamp))),
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
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Events "));
    f.render_widget(events_table, chunks[1]);
}
