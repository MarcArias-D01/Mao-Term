// No console window for release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::time::Instant;

use anyhow::Context;
use tracing_subscriber::EnvFilter;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    init_logging();

    let config = mao_config::Config::load_or_default();

    let event_loop = EventLoop::new().context("failed to create the event loop")?;
    // Render on demand: sleep until an event arrives.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = app::App::new(config, started);
    event_loop.run_app(&mut app).context("event loop error")?;
    app.exit_result()
}

/// Logs to stderr. Level controlled with `MAO_LOG` (e.g. `MAO_LOG=debug`).
fn init_logging() {
    let filter = EnvFilter::try_from_env("MAO_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,naga=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
