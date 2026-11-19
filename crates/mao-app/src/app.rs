use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use mao_config::Config;
use mao_render::{FrameOutcome, RenderError, Renderer, RendererOptions};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

/// Window + renderer, created once the event loop is running.
struct WindowState {
    window: Arc<Window>,
    renderer: Renderer,
    /// The window starts hidden and is shown after the first frame,
    /// avoiding a white flash on startup.
    shown: bool,
}

pub struct App {
    config: Config,
    started: Instant,
    state: Option<WindowState>,
    fatal: Option<anyhow::Error>,
}

impl App {
    pub fn new(config: Config, started: Instant) -> Self {
        Self {
            config,
            started,
            state: None,
            fatal: None,
        }
    }

    /// Result to return from `main` once the event loop exits.
    pub fn exit_result(self) -> anyhow::Result<()> {
        self.fatal.map_or(Ok(()), Err)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        tracing::error!("{error:#}");
        self.fatal = Some(error);
        event_loop.exit();
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<WindowState> {
        let attributes = Window::default_attributes()
            .with_title("Mao-Term")
            .with_inner_size(LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ))
            .with_visible(false);
        let window = Arc::new(event_loop.create_window(attributes)?);
        let size = window.inner_size();
        let renderer = Renderer::new(
            Arc::clone(&window),
            event_loop.owned_display_handle(),
            size.width,
            size.height,
            RendererOptions::default(),
        )?;
        Ok(WindowState {
            window,
            renderer,
            shown: false,
        })
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let background = self.config.theme.background.to_array();
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match state.renderer.render(background) {
            Ok(FrameOutcome::Presented) => {
                if !state.shown {
                    state.window.set_visible(true);
                    state.shown = true;
                    tracing::info!(
                        elapsed_ms = self.started.elapsed().as_millis(),
                        "first frame presented"
                    );
                }
            }
            Ok(FrameOutcome::Skipped) => {}
            Err(RenderError::SurfaceLost) => {
                // TODO(M1): recreate the surface instead of exiting.
                self.fail(event_loop, anyhow!("GPU surface lost"));
            }
            Err(e) => self.fail(event_loop, e.into()),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match self.create_window(event_loop) {
            Ok(state) => {
                self.state = Some(state);
                // Draw the first frame right away: hidden windows may never
                // receive `RedrawRequested` on some platforms.
                self.redraw(event_loop);
            }
            Err(e) => self.fail(event_loop, e.context("failed to create the window")),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    state.renderer.resize(size.width, size.height);
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }
}
