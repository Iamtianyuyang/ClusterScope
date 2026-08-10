mod api;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

/// ClusterScope terminal dashboard
#[derive(Parser)]
#[command(name = "clusterscope-tui", about = "ClusterScope GPU cluster monitor (TUI)")]
struct Cli {
    /// Server base URL
    #[arg(short, long, default_value = "http://127.0.0.1:8080")]
    server: String,

    /// Username
    #[arg(short, long, default_value = "admin")]
    username: String,

    /// Password
    #[arg(short, long, default_value = "admin123")]
    password: String,

    /// Refresh interval in seconds
    #[arg(short, long, default_value_t = 3)]
    interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let api = api::Api::login(&cli.server, &cli.username, &cli.password).await?;
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
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab => {
                        state.tab = ui::Tab::from_index(state.tab.index() + 1);
                    }
                    KeyCode::Up => {
                        if state.selected > 0 {
                            state.selected -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if state.selected + 1 < state.nodes.len() {
                            state.selected += 1;
                        }
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
