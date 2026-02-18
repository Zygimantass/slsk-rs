mod app;
mod client;
mod spotify;
mod ui;

use std::io;
use std::time::Duration;

use app::{App, AppEvent};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let username = std::env::var("SOULSEEK_ACCOUNT").expect("SOULSEEK_ACCOUNT not set");
    let password = std::env::var("SOULSEEK_PASSWORD").expect("SOULSEEK_PASSWORD not set");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let mut app = App::new(cmd_tx);

    let client_handle = tokio::spawn(async move {
        if let Err(e) = client::run_client(&username, &password, event_tx, cmd_rx).await {
            eprintln!("Client error: {e}");
        }
    });

    let result = run_app(&mut terminal, &mut app, event_rx).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    client_handle.abort();

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mut event_rx: mpsc::UnboundedReceiver<AppEvent>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        tokio::select! {
            Some(app_event) = event_rx.recv() => {
                app.handle_app_event(app_event);
            }
            _ = tokio::time::sleep(Duration::from_millis(5)) => {
                while event::poll(Duration::from_millis(0))? {
                    if let Event::Key(key) = event::read()? {
                        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                            return Ok(());
                        }
                        if key.code == KeyCode::Char('q') && !app.is_input_mode() {
                            return Ok(());
                        }
                        app.handle_key(key);
                    }
                }
            }
        }
    }
}
