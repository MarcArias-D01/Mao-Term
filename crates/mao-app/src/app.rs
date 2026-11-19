use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use mao_config::Config;
use mao_render::{FontLoader, FontSpec, FrameOutcome, Renderer, RendererOptions};
use mao_terminal::keys::{self, KeyModes};
use mao_terminal::mouse::{self as tmouse, MouseAction};
use mao_terminal::{GridSize, Palette, SelectionKind, SpawnOptions, Terminal, TerminalEvent};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::input::{self, Shortcut};

const MIN_FONT_SIZE: f32 = 6.0;
const MAX_FONT_SIZE: f32 = 72.0;
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(400);
const WHEEL_LINES: f32 = 3.0;

/// Events sent to the UI thread from other threads.
#[derive(Debug)]
pub enum UserEvent {
    Terminal(TerminalEvent),
}

/// Mouse state for selection and reporting.
#[derive(Default)]
struct Mouse {
    position: PhysicalPosition<f64>,
    selecting: bool,
    /// Button held while the application receives mouse reports.
    reported_button: Option<tmouse::MouseButton>,
    /// Last cell reported to the application (motion is only sent on cell change).
    last_reported_cell: Option<(usize, usize)>,
    last_click: Option<(Instant, usize, usize)>,
    click_count: u8,
    /// Sub-line wheel/touchpad scroll accumulated in pixels.
    scroll_accum: f32,
}

/// Window, renderer and terminal, created once the event loop is running.
struct Session {
    window: Arc<Window>,
    renderer: Renderer,
    terminal: Terminal,
    clipboard: Option<arboard::Clipboard>,
    modifiers: ModifiersState,
    mouse: Mouse,
    focused: bool,
    scale: f32,
    /// Next frame must redraw every row (layout/font/focus change).
    full_redraw: bool,
    /// The window starts hidden and is shown after the first frame.
    shown: bool,
    /// The shell exited; waiting for Enter (restart) or Esc (close).
    exited: bool,
    proxy: EventLoopProxy<UserEvent>,
    key_buf: Vec<u8>,
}

/// What the app should do after a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyOutcome {
    Handled,
    CloseWindow,
}

pub struct App {
    config: Config,
    fonts: Option<FontLoader>,
    proxy: EventLoopProxy<UserEvent>,
    started: Instant,
    session: Option<Session>,
    fatal: Option<anyhow::Error>,
}

impl App {
    pub fn new(
        config: Config,
        fonts: FontLoader,
        proxy: EventLoopProxy<UserEvent>,
        started: Instant,
    ) -> Self {
        Self {
            config,
            fonts: Some(fonts),
            proxy,
            started,
            session: None,
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

    fn create_session(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<Session> {
        let fonts = self.fonts.take().context("session already created")?;
        let attributes = Window::default_attributes()
            .with_title("Mao-Term")
            .with_inner_size(LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ))
            .with_visible(false);
        let t = Instant::now();
        let window = Arc::new(event_loop.create_window(attributes)?);
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: window created");
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let renderer = Renderer::new(
            Arc::clone(&window),
            event_loop.owned_display_handle(),
            (size.width, size.height),
            fonts,
            &FontSpec {
                family: self.config.font.family.clone(),
                size: self.config.font.size,
                scale,
            },
            RendererOptions::default(),
        )?;
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: renderer ready");

        let grid = grid_size(&renderer, self.config.terminal.padding * scale);
        let terminal = spawn_terminal(&self.config, grid.0, &self.proxy)?;
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: shell spawned");

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| tracing::warn!("clipboard unavailable: {e}"))
            .ok();
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: clipboard ready");

        // Testing aid: type this text into the shell at startup, with `\n`
        // meaning Enter. Goes straight to our own PTY (never to other windows).
        if let Ok(script) = std::env::var("MAO_DEBUG_INPUT") {
            terminal.write(script.replace("\\n", "\r").into_bytes());
        }

        let mut session = Session {
            window,
            renderer,
            terminal,
            clipboard,
            modifiers: ModifiersState::empty(),
            mouse: Mouse::default(),
            focused: true,
            scale,
            full_redraw: true,
            shown: false,
            exited: false,
            proxy: self.proxy.clone(),
            key_buf: Vec::with_capacity(16),
        };
        session.relayout(self.config.terminal.padding);
        Ok(session)
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let background = self.config.theme.background.to_array();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let full = std::mem::take(&mut s.full_redraw);
        let renderer = &mut s.renderer;
        let info = s.terminal.render(s.focused, full, |row, cells| {
            renderer.set_row(row, cells.iter().map(to_grid_cell));
        });
        renderer.set_cursor(info.cursor.map(to_cursor));

        match renderer.render(background) {
            Ok(FrameOutcome::Presented) => {
                if !s.shown {
                    s.window.set_visible(true);
                    s.shown = true;
                    tracing::info!(
                        elapsed_ms = self.started.elapsed().as_millis(),
                        "first frame presented"
                    );
                }
            }
            Ok(FrameOutcome::Skipped) => {}
            Ok(FrameOutcome::Retry) => s.window.request_redraw(),
            Err(e) => self.fail(event_loop, e.into()),
        }
    }
}

impl Session {
    /// Recomputes the grid from the window size and font, and resizes the PTY.
    fn relayout(&mut self, padding: f32) {
        let pad = padding * self.scale;
        let (grid, origin) = grid_size(&self.renderer, pad);
        self.renderer
            .set_grid_layout(origin, usize::from(grid.cols), usize::from(grid.rows));
        self.terminal.resize(grid);
        self.full_redraw = true;
        self.window.request_redraw();
    }

    fn set_font_size(&mut self, size: f32, padding: f32) {
        self.renderer
            .set_font_size(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE), self.scale);
        self.relayout(padding);
    }

    /// Viewport cell under the mouse, and whether it's the right half of the cell.
    fn mouse_cell(&self, padding: f32) -> (usize, usize, bool) {
        let cell = self.renderer.cell_metrics();
        let pad = f64::from(padding * self.scale);
        let x = ((self.mouse.position.x - pad) / f64::from(cell.width)).max(0.0);
        let y = ((self.mouse.position.y - pad) / f64::from(cell.height)).max(0.0);
        let size = self.terminal.size();
        let col = (x as usize).min(usize::from(size.cols).saturating_sub(1));
        let row = (y as usize).min(usize::from(size.rows).saturating_sub(1));
        (row, col, x.fract() > 0.5)
    }

    fn copy_selection(&mut self) -> bool {
        let Some(text) = self.terminal.selection_text() else {
            return false;
        };
        if let Some(clipboard) = self.clipboard.as_mut()
            && let Err(e) = clipboard.set_text(text)
        {
            tracing::warn!("copy failed: {e}");
        }
        true
    }

    fn paste(&mut self) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        match clipboard.get_text() {
            Ok(text) if !text.is_empty() => {
                self.terminal.scroll_to_bottom();
                self.terminal.paste(&text);
            }
            Ok(_) => {}
            Err(e) => tracing::debug!("paste: clipboard has no text: {e}"),
        }
    }

    fn on_key(&mut self, event: &KeyEvent, config: &Config) -> KeyOutcome {
        if event.state != ElementState::Pressed {
            return KeyOutcome::Handled;
        }
        if self.exited {
            return self.on_key_after_exit(event, config);
        }
        let mods = input::modifiers(self.modifiers);
        if let Some(shortcut) = input::shortcut(event, mods) {
            let padding = config.terminal.padding;
            let size = self.renderer.font_size();
            match shortcut {
                Shortcut::Copy => {
                    self.copy_selection();
                }
                Shortcut::CopyOrInterrupt => {
                    if self.copy_selection() {
                        self.terminal.clear_selection();
                        self.window.request_redraw();
                    } else {
                        self.terminal.scroll_to_bottom();
                        self.terminal.write(&[0x03][..]);
                    }
                }
                Shortcut::Paste => self.paste(),
                Shortcut::ZoomIn => self.set_font_size(size + 1.0, padding),
                Shortcut::ZoomOut => self.set_font_size(size - 1.0, padding),
                Shortcut::ZoomReset => self.set_font_size(config.font.size, padding),
                Shortcut::ScrollPageUp => {
                    self.terminal.scroll_page(true);
                    self.window.request_redraw();
                }
                Shortcut::ScrollPageDown => {
                    self.terminal.scroll_page(false);
                    self.window.request_redraw();
                }
            }
            return KeyOutcome::Handled;
        }

        let Some(key) = input::terminal_key(event) else {
            return KeyOutcome::Handled;
        };
        let modes = KeyModes {
            app_cursor: self.terminal.modes().app_cursor,
        };
        if keys::encode(key, mods, modes, &mut self.key_buf) {
            self.terminal.scroll_to_bottom();
            self.terminal.clear_selection();
            self.terminal.write(self.key_buf.clone());
        }
        KeyOutcome::Handled
    }

    /// After the shell exited: Enter restarts it, Esc closes the window.
    fn on_key_after_exit(&mut self, event: &KeyEvent, config: &Config) -> KeyOutcome {
        use winit::keyboard::{Key, NamedKey};
        match event.logical_key {
            Key::Named(NamedKey::Enter) => {
                let grid = self.terminal.size();
                match spawn_terminal(config, grid, &self.proxy) {
                    Ok(terminal) => {
                        self.terminal = terminal;
                        self.exited = false;
                        self.window.set_title("Mao-Term");
                        self.full_redraw = true;
                        self.window.request_redraw();
                    }
                    Err(e) => {
                        tracing::error!("failed to restart the shell: {e:#}");
                        self.terminal
                            .print_local(&format!("\r\n\x1b[31mfailed to restart: {e}\x1b[0m\r\n"));
                    }
                }
                KeyOutcome::Handled
            }
            Key::Named(NamedKey::Escape) => KeyOutcome::CloseWindow,
            _ => KeyOutcome::Handled,
        }
    }

    fn on_shell_exit(&mut self, code: Option<i32>) {
        if self.exited {
            // Both "child exited" and "PTY closed" arrive; report once.
            return;
        }
        self.exited = true;
        tracing::info!(?code, "shell exited");
        let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
        self.terminal.scroll_to_bottom();
        self.terminal.print_local(&format!(
            "\r\n\x1b[0;2m[process exited with code {code} · Enter: restart · Esc: close]\x1b[0m"
        ));
    }

    /// Sends a mouse event to the application if it enabled mouse reporting.
    /// Shift bypasses reporting so the user can still select text.
    /// Returns whether the event was consumed.
    fn report_mouse(
        &mut self,
        button: Option<tmouse::MouseButton>,
        action: MouseAction,
        padding: f32,
    ) -> bool {
        let modes = self.terminal.modes().mouse;
        if !modes.report || self.modifiers.shift_key() || self.exited {
            return false;
        }
        let (row, col, _) = self.mouse_cell(padding);
        if action == MouseAction::Motion
            && (!modes.wants_motion(button.is_some())
                || self.mouse.last_reported_cell == Some((row, col)))
        {
            return true;
        }
        self.mouse.last_reported_cell = Some((row, col));
        let mods = input::modifiers(self.modifiers);
        if tmouse::encode(button, action, mods, row, col, modes, &mut self.key_buf) {
            self.terminal.write(self.key_buf.clone());
        }
        true
    }

    fn on_mouse_button(&mut self, state: ElementState, button: MouseButton, padding: f32) {
        let reported = match button {
            MouseButton::Left => Some(tmouse::MouseButton::Left),
            MouseButton::Middle => Some(tmouse::MouseButton::Middle),
            MouseButton::Right => Some(tmouse::MouseButton::Right),
            _ => None,
        };
        if let Some(b) = reported {
            let action = match state {
                ElementState::Pressed => MouseAction::Press,
                ElementState::Released => MouseAction::Release,
            };
            if action == MouseAction::Release && self.mouse.reported_button == Some(b) {
                // Always pair a reported press with its release, even if Shift is now held.
                self.mouse.reported_button = None;
                let (row, col, _) = self.mouse_cell(padding);
                let modes = self.terminal.modes().mouse;
                let mods = input::modifiers(self.modifiers);
                if tmouse::encode(Some(b), action, mods, row, col, modes, &mut self.key_buf) {
                    self.terminal.write(self.key_buf.clone());
                }
                return;
            }
            if self.report_mouse(Some(b), action, padding) {
                if action == MouseAction::Press {
                    self.mouse.reported_button = Some(b);
                }
                return;
            }
        }
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                let (row, col, right) = self.mouse_cell(padding);
                let now = Instant::now();
                let repeat = self.mouse.last_click.is_some_and(|(t, r, c)| {
                    now.duration_since(t) < MULTI_CLICK_INTERVAL && r == row && c == col
                });
                self.mouse.click_count = if repeat {
                    self.mouse.click_count % 3 + 1
                } else {
                    1
                };
                self.mouse.last_click = Some((now, row, col));
                let kind = match (self.mouse.click_count, self.modifiers.alt_key()) {
                    (_, true) => SelectionKind::Block,
                    (2, _) => SelectionKind::Word,
                    (3, _) => SelectionKind::Line,
                    _ => SelectionKind::Simple,
                };
                self.terminal.start_selection(kind, row, col, right);
                self.mouse.selecting = true;
                self.window.request_redraw();
            }
            (MouseButton::Left, ElementState::Released) => self.mouse.selecting = false,
            (MouseButton::Right, ElementState::Pressed) if cfg!(windows) => {
                // Windows Terminal convention: right click copies a selection, or pastes.
                if self.copy_selection() {
                    self.terminal.clear_selection();
                    self.window.request_redraw();
                } else {
                    self.paste();
                }
            }
            (MouseButton::Middle, ElementState::Pressed) if !cfg!(windows) => self.paste(),
            _ => {}
        }
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta, padding: f32) {
        let cell_height = self.renderer.cell_metrics().height;
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * WHEEL_LINES,
            MouseScrollDelta::PixelDelta(pos) => {
                self.mouse.scroll_accum += pos.y as f32;
                let lines = (self.mouse.scroll_accum / cell_height).trunc();
                self.mouse.scroll_accum -= lines * cell_height;
                lines
            }
        };
        let lines = lines as i32;
        if lines == 0 {
            return;
        }
        let modes = self.terminal.modes();
        if modes.mouse.report && !self.modifiers.shift_key() {
            let button = if lines > 0 {
                tmouse::MouseButton::WheelUp
            } else {
                tmouse::MouseButton::WheelDown
            };
            for _ in 0..lines.unsigned_abs().min(10) {
                self.report_mouse(Some(button), MouseAction::Press, padding);
            }
            return;
        }
        if modes.alt_screen && modes.alternate_scroll {
            // Full-screen apps (less, vim…): scroll by sending arrow keys.
            let arrow: &[u8] = match (lines > 0, modes.app_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.terminal
                .write(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            self.terminal.scroll_lines(lines);
            self.window.request_redraw();
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match self.create_session(event_loop) {
            Ok(session) => {
                self.session = Some(session);
                // Draw the first frame right away: hidden windows may never
                // receive `RedrawRequested` on some platforms.
                self.redraw(event_loop);
            }
            Err(e) => self.fail(event_loop, e.context("failed to start")),
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let UserEvent::Terminal(event) = event;
        match event {
            TerminalEvent::Wakeup => s.window.request_redraw(),
            TerminalEvent::Title(title) => s.window.set_title(&title),
            TerminalEvent::ResetTitle => s.window.set_title("Mao-Term"),
            TerminalEvent::Bell => s.window.request_user_attention(None),
            TerminalEvent::ClipboardStore(text) => {
                if let Some(clipboard) = s.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            TerminalEvent::Exited(code) => s.on_shell_exit(code),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let padding = self.config.terminal.padding;
        if let WindowEvent::RedrawRequested = event {
            self.redraw(event_loop);
            return;
        }
        let Some(s) = self.session.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                s.renderer.resize(size.width, size.height);
                s.relayout(padding);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                s.scale = scale_factor as f32;
                let size = s.renderer.font_size();
                s.set_font_size(size, padding);
            }
            WindowEvent::Focused(focused) => {
                s.focused = focused;
                s.terminal.report_focus(focused);
                s.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(mods) => s.modifiers = mods.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if s.on_key(&event, &self.config) == KeyOutcome::CloseWindow {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                s.on_mouse_button(state, button, padding)
            }
            WindowEvent::CursorMoved { position, .. } => {
                s.mouse.position = position;
                let held = s.mouse.reported_button;
                if (held.is_some() || s.terminal.modes().mouse.motion)
                    && s.report_mouse(held, MouseAction::Motion, padding)
                {
                    return;
                }
                if s.mouse.selecting {
                    let (row, col, right) = s.mouse_cell(padding);
                    s.terminal.update_selection(row, col, right);
                    s.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => s.on_wheel(delta, padding),
            _ => {}
        }
    }
}

/// Spawns the configured shell, forwarding its events to the UI thread.
fn spawn_terminal(
    config: &Config,
    grid: GridSize,
    proxy: &EventLoopProxy<UserEvent>,
) -> anyhow::Result<Terminal> {
    let theme = &config.theme;
    let palette = Palette::new(
        theme.ansi.map(|c| c.to_array()),
        theme.foreground.to_array(),
        theme.background.to_array(),
        theme.cursor.to_array(),
        theme.selection.to_array(),
    );
    let proxy = proxy.clone();
    let terminal = Terminal::spawn(
        SpawnOptions {
            program: config.shell.program.clone(),
            args: config.shell.args.clone(),
            working_directory: None,
            scrollback: config.terminal.scrollback,
            palette,
        },
        grid,
        move |event| {
            // Fails only if the event loop is gone (shutting down).
            let _ = proxy.send_event(UserEvent::Terminal(event));
        },
    )?;
    Ok(terminal)
}

/// Grid size that fits the surface, and the grid origin in pixels.
fn grid_size(renderer: &Renderer, pad: f32) -> (GridSize, (f32, f32)) {
    let (width, height) = renderer.size();
    let cell = renderer.cell_metrics();
    let cols = ((width as f32 - 2.0 * pad) / cell.width).floor().max(2.0);
    let rows = ((height as f32 - 2.0 * pad) / cell.height).floor().max(1.0);
    let grid = GridSize {
        cols: cols as u16,
        rows: rows as u16,
        cell_width: cell.width as u16,
        cell_height: cell.height as u16,
    };
    (grid, (pad, pad))
}

fn to_grid_cell(c: &mao_terminal::CellView) -> mao_render::GridCell {
    mao_render::GridCell {
        ch: c.ch,
        fg: c.fg,
        bg: c.bg,
        style: mao_render::CellStyle {
            bold: c.style.bold,
            italic: c.style.italic,
            underline: c.style.underline,
            strikeout: c.style.strikeout,
            spacer: c.style.spacer,
        },
    }
}

fn to_cursor(c: mao_terminal::CursorView) -> mao_render::Cursor {
    mao_render::Cursor {
        row: c.row,
        col: c.col,
        shape: match c.shape {
            mao_terminal::CursorShape::Block => mao_render::CursorShape::Block,
            mao_terminal::CursorShape::HollowBlock => mao_render::CursorShape::HollowBlock,
            mao_terminal::CursorShape::Beam => mao_render::CursorShape::Beam,
            mao_terminal::CursorShape::Underline => mao_render::CursorShape::Underline,
        },
        color: c.color,
        wide: c.wide,
    }
}
