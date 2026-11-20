// No console window for release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod chrome;
mod docpane;
mod editor;
mod input;
mod overlay;
mod palette;
mod pane;
mod panels;
mod platform;
mod sidebar;
mod workspace;

use std::time::Instant;

use anyhow::Context;
use tracing_subscriber::EnvFilter;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    init_logging();

    // Font discovery is slow; overlap it with config, window and GPU setup.
    let fonts = mao_render::FontLoader::start();
    let config = mao_config::Config::load_or_default();
    // History (ours + the shells') loads while the window and GPU start.
    let history = std::thread::Builder::new()
        .name("history-loader".into())
        .spawn(|| {
            let path = mao_config::data_dir().map(|d| d.join("history.tsv"));
            if let Some(dir) = path.as_ref().and_then(|p| p.parent()) {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut history = path
                .as_deref()
                .map(mao_complete::History::load)
                .unwrap_or_default();
            history.import_shell_histories();
            history
        })
        .context("failed to start the history loader")?;

    let event_loop = EventLoop::<app::UserEvent>::with_user_event()
        .build()
        .context("failed to create the event loop")?;
    // Render on demand: sleep until an event arrives.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = app::App::new(config, fonts, history, event_loop.create_proxy(), started);
    event_loop.run_app(&mut app).context("event loop error")?;
    app.exit_result()
}

/// Logs to stderr, or to the file in `MAO_LOG_FILE`. Level controlled with
/// `MAO_LOG` (e.g. `MAO_LOG=debug`).
fn init_logging() {
    let filter = EnvFilter::try_from_env("MAO_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,naga=warn"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    match std::env::var_os("MAO_LOG_FILE").map(std::fs::File::create) {
        Some(Ok(file)) => builder
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        _ => builder.init(),
    }
}
