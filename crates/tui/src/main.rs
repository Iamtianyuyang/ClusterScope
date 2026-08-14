mod api;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

/// ClusterScope terminal dashboard
#[derive(Parser)]
#[command(
    name = "clusterscope-tui",
    about = "ClusterScope GPU cluster monitor (TUI)"
)]
struct Cli {
    /// Server base URL
    #[arg(short, long, default_value = "http://127.0.0.1:8080")]
    server: String,

    /// Username (optional when the server runs in read-only mode)
    #[arg(short, long)]
    username: Option<String>,

    /// Password (optional when the server runs in read-only mode)
    #[arg(short, long)]
    password: Option<String>,

    /// Refresh interval in seconds
    #[arg(short, long, default_value_t = 3)]
    interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let api = api::Api::connect(
        &cli.server,
        cli.username.as_deref(),
        cli.password.as_deref(),
    )
    .await?;
    eprintln!("connected to {}", cli.server);

    let mut state = ui::AppState::new();
    state.refresh(&api).await;

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let refresh_interval = Duration::from_secs(cli.interval.max(1));
    let result = run(&mut terminal, &mut state, &api, refresh_interval).await;

    // Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ui::AppState,
    api: &api::Api,
    refresh_interval: Duration,
) -> Result<()> {
    let mut last_refresh = std::time::Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, state))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {
                        state.refresh(api).await;
                        last_refresh = std::time::Instant::now();
                    }
                    KeyCode::Char('?') => state.help = !state.help,
                    KeyCode::Esc => {
                        if state.help {
                            state.help = false;
                        }
                    }
                    KeyCode::Char('1') => state.tab = ui::Tab::from_index(0),
                    KeyCode::Char('2') => state.tab = ui::Tab::from_index(1),
                    KeyCode::Char('3') => state.tab = ui::Tab::from_index(2),
                    KeyCode::Char('4') => state.tab = ui::Tab::from_index(3),
                    KeyCode::Tab => {
                        state.tab = ui::Tab::from_index(state.tab.index() + 1);
                    }
                    KeyCode::BackTab => {
                        state.tab = ui::Tab::from_index(state.tab.index() + 2);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        let gpu_count = state
                            .nodes
                            .get(state.selected_node)
                            .and_then(|n| state.metrics.get(&n.node_id))
                            .map(|m| m.gpus.len())
                            .unwrap_or(0);
                        if state.selected_gpu + 1 < gpu_count.max(1) {
                            state.selected_gpu += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if state.selected_gpu > 0 {
                            state.selected_gpu -= 1;
                        }
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        if state.selected_node > 0 {
                            state.selected_node -= 1;
                            state.selected_gpu = 0;
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        if state.selected_node + 1 < state.nodes.len() {
                            state.selected_node += 1;
                            state.selected_gpu = 0;
                        }
                    }
                    KeyCode::Enter => {
                        // reserved: node/process details
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last_refresh.elapsed() >= refresh_interval {
            state.refresh(api).await;
            last_refresh = std::time::Instant::now();
        }
    }

    Ok(())
}
