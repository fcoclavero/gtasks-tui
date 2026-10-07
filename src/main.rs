mod app;
mod backend;
mod browser;
mod cli;
mod dates;
mod details;
mod model;
mod tree;
mod ui;
mod worker;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, UnboundedReceiver};

use app::App;
use backend::TasksBackend;
use backend::gws::GwsBackend;
use backend::mock::MockBackend;
use cli::Args;
use worker::Msg;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.mock {
        let backend = MockBackend::seeded(
            Duration::from_millis(args.mock_latency_ms),
            args.mock_fail_every,
        );
        run(Arc::new(backend), &args).await
    } else {
        let backend = GwsBackend::new(args.gws_bin.clone(), Duration::from_secs(args.timeout_secs));
        run(Arc::new(backend), &args).await
    }
}

async fn run<B: TasksBackend>(backend: Arc<B>, args: &Args) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut app = App::new(worker::dispatcher(backend, tx.clone()));
    worker::spawn_input_thread(tx.clone());
    worker::spawn_ticker(tx, args.refresh_secs);
    app.start_refresh();

    let mut terminal = ratatui::init();
    let result = async {
        execute!(terminal.backend_mut(), EnableMouseCapture)?;
        event_loop(&mut terminal, &mut app, &mut rx).await
    }
    .await;
    let mouse_result = execute!(terminal.backend_mut(), DisableMouseCapture);
    ratatui::restore();
    result.and(mouse_result.map_err(Into::into))
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    rx: &mut UnboundedReceiver<Msg>,
) -> Result<()> {
    terminal.draw(|frame| ui::draw(frame, app))?;
    loop {
        // While quitting, poll so the pending-op deadline is honoured even if nothing arrives.
        let msg = if app.is_quitting() {
            tokio::time::timeout(Duration::from_millis(100), rx.recv())
                .await
                .ok()
                .flatten()
        } else {
            rx.recv().await
        };
        match msg {
            Some(msg) => app.handle(msg),
            None if app.is_quitting() => {}
            None => break,
        }
        while let Ok(msg) = rx.try_recv() {
            app.handle(msg);
        }
        app.tick_quit();
        if app.should_quit {
            break;
        }
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
    Ok(())
}
