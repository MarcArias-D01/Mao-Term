use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::Context;
use mao_complete::{Engine, History};
use mao_config::Config;
use mao_render::{FontLoader, FontSpec, FrameOutcome, GridId, Renderer, RendererOptions, UiFrame};
use mao_terminal::{
    GridSize, Palette as TermPalette, RenderOptions, SpawnOptions, Terminal, TerminalEvent,
};
use mao_ui::{Divider, PaneId, SplitDir};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::actions::Action;
use crate::chrome::{self, Colors, DrawState, Hit, Layout, MenuView, Metrics};
use crate::editor::{Ctx, Outcome};
use crate::input;
use crate::overlay::{self, OverlayColors};
use crate::palette::Palette;
use crate::pane::{Pane, ShellState, grid_for};
use crate::workspace::{Closed, Workspace};

const MIN_FONT_SIZE: f32 = 6.0;
const MAX_FONT_SIZE: f32 = 72.0;

/// Events sent to the UI thread from other threads.
#[derive(Debug)]
pub enum UserEvent {
    /// `session` identifies the terminal instance, so events from a shell that
    /// was replaced (pane restarted) or closed are ignored.
    Terminal {
        pane: PaneId,
        session: u64,
        event: TerminalEvent,
    },
    /// Executables found on `PATH` (scanned in the background).
    Executables(Vec<String>),
}

/// Whether the app should keep running after handling something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

/// The window and everything in it, created once the event loop is running.
struct Win {
    window: Arc<Window>,
    renderer: Renderer,
    clipboard: Option<arboard::Clipboard>,
    proxy: EventLoopProxy<UserEvent>,
    ws: Workspace,
    /// Session id of each pane's current terminal.
    sessions: HashMap<PaneId, u64>,
    next_session: u64,
    palette: Option<Palette>,
    metrics: Metrics,
    layout: Layout,
    colors: Colors,
    ui: UiFrame,
    visible: Vec<GridId>,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    hover: Hit,
    cursor_icon: CursorIcon,
    /// Divider being dragged.
    drag: Option<Divider>,
    /// Pane that owns the mouse during a drag/press.
    capture: Option<PaneId>,
    focused: bool,
    scale: f32,
    shown: bool,
    engine: Engine,
    editor_enabled: bool,
    overlay_colors: OverlayColors,
    /// Scratch row for compositing the editor overlay.
    row_buf: Vec<mao_render::GridCell>,
}

pub struct App {
    config: Config,
    fonts: Option<FontLoader>,
    history: Option<JoinHandle<History>>,
    proxy: EventLoopProxy<UserEvent>,
    started: Instant,
    win: Option<Win>,
    fatal: Option<anyhow::Error>,
}

impl App {
    pub fn new(
        config: Config,
        fonts: FontLoader,
        history: JoinHandle<History>,
        proxy: EventLoopProxy<UserEvent>,
        started: Instant,
    ) -> Self {
        Self {
            config,
            fonts: Some(fonts),
            history: Some(history),
            proxy,
            started,
            win: None,
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

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<Win> {
        let fonts = self.fonts.take().context("window already created")?;
        let background = std::env::var_os("MAO_DEBUG_BACKGROUND").is_some();
        let attributes = Window::default_attributes()
            .with_title("Mao-Term")
            .with_inner_size(LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ))
            // Testing aid: open without taking keyboard focus, so automated checks
            // never capture what the user is typing elsewhere. Showing a hidden
            // window later would activate it, so in that mode it starts visible.
            .with_visible(background)
            .with_active(!background);
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

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| tracing::warn!("clipboard unavailable: {e}"))
            .ok();
        let metrics = Metrics::new(scale, self.config.terminal.padding);
        let history = self
            .history
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        tracing::debug!(entries = history.len(), "history loaded");
        // PATH scanning reads hundreds of directories: keep it off the UI thread.
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new()
            .name("path-scan".into())
            .spawn(move || {
                let names = mao_complete::paths::scan_path_executables();
                let _ = proxy.send_event(UserEvent::Executables(names));
            });
        let mut win = Win {
            layout: Layout::new((size.width, size.height), &metrics, 1),
            metrics,
            colors: Colors::new(&self.config.theme),
            window,
            renderer,
            clipboard,
            proxy: self.proxy.clone(),
            ws: Workspace::default(),
            sessions: HashMap::new(),
            next_session: 0,
            palette: None,
            ui: UiFrame::default(),
            visible: Vec::new(),
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            hover: Hit::None,
            cursor_icon: CursorIcon::Default,
            drag: None,
            capture: None,
            focused: true,
            scale,
            shown: background,
            engine: Engine::new(history),
            editor_enabled: self.config.input.editor,
            overlay_colors: OverlayColors::new(&self.config.theme),
            row_buf: Vec::new(),
        };
        win.new_tab(&self.config)?;
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: shell spawned");

        // Testing aid: type this text into the first shell at startup, with `\n`
        // meaning Enter. Goes straight to our own PTY (never to other windows).
        if let Ok(script) = std::env::var("MAO_DEBUG_INPUT")
            && let Some(pane) = win.ws.focused_pane()
        {
            pane.terminal
                .write(script.replace("\\n", "\r").into_bytes());
        }
        Ok(win)
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let background = self.config.theme.background.to_array();
        let Some(w) = self.win.as_mut() else {
            return;
        };
        match w.draw(background) {
            Ok(FrameOutcome::Presented) => {
                if !w.shown {
                    w.window.set_visible(true);
                    w.shown = true;
                    tracing::info!(
                        elapsed_ms = self.started.elapsed().as_millis(),
                        "first frame presented"
                    );
                }
            }
            Ok(FrameOutcome::Skipped) => {}
            Ok(FrameOutcome::Retry) => w.window.request_redraw(),
            Err(e) => self.fail(event_loop, e.into()),
        }
    }
}

impl Win {
    fn cell(&self) -> mao_render::CellMetrics {
        self.renderer.cell_metrics()
    }

    /// Recomputes chrome and pane geometry for every tab and resizes the PTYs.
    fn relayout(&mut self, padding: f32) {
        self.metrics = Metrics::new(self.scale, padding);
        self.layout = Layout::new(self.renderer.size(), &self.metrics, self.ws.tabs.len());
        let cell = self.cell();
        let (content, gap, pad) = (self.layout.content, self.metrics.gap, self.metrics.pad);
        for tab in &self.ws.tabs {
            for (id, rect) in tab.layout(content, gap) {
                if let Some(pane) = self.ws.panes.get_mut(&id) {
                    let grid = pane.layout(rect, pad, cell);
                    self.renderer.set_grid_layout(
                        id,
                        pane.origin,
                        usize::from(grid.cols),
                        usize::from(grid.rows),
                        [rect.x, rect.y, rect.w, rect.h],
                    );
                }
            }
        }
        self.window.request_redraw();
    }

    /// Directory new shells start in: the focused pane's.
    fn current_cwd(&self) -> Option<PathBuf> {
        self.ws.focused_pane().and_then(|p| p.cwd.clone())
    }

    /// Starts a shell for `pane` with the given grid size.
    fn spawn(
        &mut self,
        config: &Config,
        pane: PaneId,
        grid: GridSize,
        cwd: Option<PathBuf>,
    ) -> anyhow::Result<Terminal> {
        self.next_session += 1;
        let session = self.next_session;
        let terminal = spawn_terminal(config, grid, &self.proxy, pane, session, cwd)?;
        self.sessions.insert(pane, session);
        Ok(terminal)
    }

    fn new_tab(&mut self, config: &Config) -> anyhow::Result<()> {
        let id = self.ws.next_id();
        // Size the shell for the content area right away.
        let layout = Layout::new(self.renderer.size(), &self.metrics, self.ws.tabs.len() + 1);
        let grid = grid_for(layout.content, self.metrics.pad, self.cell());
        let cwd = self.current_cwd();
        let terminal = self.spawn(config, id, grid, cwd)?;
        self.ws.add_tab(Pane::new(id, terminal));
        self.relayout(config.terminal.padding);
        self.update_title();
        Ok(())
    }

    fn split(&mut self, config: &Config, dir: SplitDir) -> anyhow::Result<()> {
        let Some(target) = self.ws.focused_id() else {
            return Ok(());
        };
        let id = self.ws.next_id();
        let (content, gap, pad, cell) = (
            self.layout.content,
            self.metrics.gap,
            self.metrics.pad,
            self.cell(),
        );
        let Some(tab) = self.ws.active_tab_mut() else {
            return Ok(());
        };
        tab.zoomed = None;
        tab.tree.split(target, dir, id);
        let rect = tab
            .tree
            .layout(content, gap)
            .into_iter()
            .find(|(p, _)| *p == id)
            .map_or(content, |(_, r)| r);
        let cwd = self.current_cwd();
        match self.spawn(config, id, grid_for(rect, pad, cell), cwd) {
            Ok(terminal) => {
                self.ws.panes.insert(id, Pane::new(id, terminal));
                self.ws.focus(id);
                self.relayout(config.terminal.padding);
                self.update_title();
                Ok(())
            }
            Err(e) => {
                if let Some(tab) = self.ws.active_tab_mut() {
                    tab.tree.remove(id);
                }
                Err(e)
            }
        }
    }

    fn close_pane(&mut self, config: &Config, id: PaneId) -> Flow {
        self.renderer.remove_grid(id);
        self.sessions.remove(&id);
        if self.capture == Some(id) {
            self.capture = None;
        }
        if self.ws.close_pane(id) == Closed::Everything {
            return Flow::Quit;
        }
        self.relayout(config.terminal.padding);
        self.update_title();
        Flow::Continue
    }

    fn close_tab(&mut self, config: &Config, index: usize) -> Flow {
        let Some(tab) = self.ws.tabs.get(index) else {
            return Flow::Continue;
        };
        for id in tab.tree.panes() {
            if self.close_pane(config, id) == Flow::Quit {
                return Flow::Quit;
            }
        }
        Flow::Continue
    }

    fn set_font_size(&mut self, config: &Config, size: f32) {
        self.renderer
            .set_font_size(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE), self.scale);
        self.relayout(config.terminal.padding);
    }

    fn update_title(&self) {
        let title = self
            .ws
            .focused_pane()
            .map(|p| p.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or("Mao-Term");
        self.window.set_title(title);
    }

    fn activate_tab(&mut self, index: usize) {
        if index < self.ws.tabs.len() {
            self.ws.set_active(index);
            self.update_title();
            self.window.request_redraw();
        }
    }

    fn run(&mut self, config: &Config, action: Action) -> Flow {
        match action {
            Action::CommandPalette => {
                self.palette = Some(Palette::new());
                self.window.request_redraw();
            }
            Action::NewTab => {
                if let Err(e) = self.new_tab(config) {
                    tracing::error!("failed to open a tab: {e:#}");
                }
            }
            Action::SplitRight | Action::SplitDown => {
                let dir = if action == Action::SplitRight {
                    SplitDir::Right
                } else {
                    SplitDir::Down
                };
                if let Err(e) = self.split(config, dir) {
                    tracing::error!("failed to split: {e:#}");
                }
            }
            Action::ClosePane => {
                if let Some(id) = self.ws.focused_id() {
                    return self.close_pane(config, id);
                }
            }
            Action::CloseTab => return self.close_tab(config, self.ws.active),
            Action::NextTab | Action::PrevTab => {
                let n = self.ws.tabs.len();
                if n > 1 {
                    let next = if action == Action::NextTab {
                        (self.ws.active + 1) % n
                    } else {
                        (self.ws.active + n - 1) % n
                    };
                    self.activate_tab(next);
                }
            }
            Action::GoToTab(i) => self.activate_tab(i),
            Action::Focus(dir) => {
                let (content, gap) = (self.layout.content, self.metrics.gap);
                if let Some(tab) = self.ws.active_tab()
                    && tab.zoomed.is_none()
                    && let Some(next) = tab.tree.neighbor(content, gap, tab.focused, dir)
                {
                    self.ws.focus(next);
                    self.update_title();
                    self.window.request_redraw();
                }
            }
            Action::ToggleZoomPane => {
                if let Some(tab) = self.ws.active_tab_mut()
                    && tab.tree.len() > 1
                {
                    tab.zoomed = if tab.zoomed.is_some() {
                        None
                    } else {
                        Some(tab.focused)
                    };
                    for id in tab.tree.panes() {
                        if let Some(p) = self.ws.panes.get_mut(&id) {
                            p.full_redraw = true;
                        }
                    }
                    self.relayout(config.terminal.padding);
                }
            }
            Action::Copy => {
                if let Some(pane) = self.ws.focused_pane() {
                    pane.copy_selection(self.clipboard.as_mut());
                }
            }
            Action::CopyOrInterrupt => {
                if let Some(pane) = self.ws.focused_pane_mut() {
                    if pane.copy_selection(self.clipboard.as_mut()) {
                        pane.terminal.clear_selection();
                        self.window.request_redraw();
                    } else {
                        pane.terminal.scroll_to_bottom();
                        pane.terminal.write(&[0x03][..]);
                    }
                }
            }
            Action::Paste => {
                if let Some(pane) = self.ws.focused_pane() {
                    pane.paste(self.clipboard.as_mut());
                }
            }
            Action::FontBigger => self.set_font_size(config, self.renderer.font_size() + 1.0),
            Action::FontSmaller => self.set_font_size(config, self.renderer.font_size() - 1.0),
            Action::FontReset => self.set_font_size(config, config.font.size),
            Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToTop
            | Action::ScrollToBottom => {
                if let Some(pane) = self.ws.focused_pane() {
                    match action {
                        Action::ScrollPageUp => pane.terminal.scroll_page(true),
                        Action::ScrollPageDown => pane.terminal.scroll_page(false),
                        Action::ScrollToTop => pane.terminal.scroll_lines(i32::MAX / 2),
                        _ => pane.terminal.scroll_to_bottom(),
                    }
                    self.window.request_redraw();
                }
            }
            Action::Quit => return Flow::Quit,
        }
        Flow::Continue
    }

    fn on_key(&mut self, config: &Config, event: &KeyEvent) -> Flow {
        if event.state != ElementState::Pressed {
            return Flow::Continue;
        }
        if self.palette.is_some() {
            return self.on_palette_key(config, event);
        }
        let mods = input::modifiers(self.modifiers);
        let editor_on = self
            .ws
            .focused_pane()
            .is_some_and(|p| p.editor_active(config.input.editor));
        if editor_on {
            return self.on_editor_key(config, event, mods);
        }
        if let Some(action) = input::action_for(event, mods) {
            return self.run(config, action);
        }
        let Some(id) = self.ws.focused_id() else {
            return Flow::Continue;
        };
        if self.ws.panes.get(&id).is_some_and(|p| p.exited) {
            return match event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    self.restart(config, id);
                    Flow::Continue
                }
                Key::Named(NamedKey::Escape) => self.close_pane(config, id),
                _ => Flow::Continue,
            };
        }
        if let Some(key) = input::terminal_key(event)
            && let Some(pane) = self.ws.panes.get_mut(&id)
        {
            pane.write_key(key, mods);
        }
        Flow::Continue
    }

    /// Keys while the focused pane's shell waits at its prompt.
    fn on_editor_key(
        &mut self,
        config: &Config,
        event: &KeyEvent,
        mods: mao_terminal::keys::Modifiers,
    ) -> Flow {
        let Some(id) = self.ws.focused_id() else {
            return Flow::Continue;
        };
        match input::action_for(event, mods) {
            // Ctrl+C: copy a selection, else clear the line (like shells do).
            Some(Action::CopyOrInterrupt) => {
                let Some(pane) = self.ws.panes.get_mut(&id) else {
                    return Flow::Continue;
                };
                if pane.copy_selection(self.clipboard.as_mut()) {
                    pane.terminal.clear_selection();
                } else if let Some(text) = pane.editor.line.selected_text() {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        let _ = clipboard.set_text(text.to_owned());
                    }
                } else {
                    pane.editor = crate::editor::PromptEditor::default();
                }
                self.window.request_redraw();
                return Flow::Continue;
            }
            Some(Action::Paste) => {
                let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok());
                if let Some(text) = text {
                    self.paste_into_editor(config, id, &text);
                }
                return Flow::Continue;
            }
            // Shift+Home/End select in the editor instead of scrolling.
            Some(Action::ScrollToTop | Action::ScrollToBottom) => {}
            Some(action) => return self.run(config, action),
            None => {}
        }

        let engine = &self.engine;
        let Some(pane) = self.ws.panes.get_mut(&id) else {
            return Flow::Continue;
        };
        let ctx = Ctx {
            engine,
            cwd: pane.cwd.as_deref(),
            quoting: pane.quoting(),
            suggestions: config.input.suggestions,
        };
        let outcome = pane.editor.on_key(event, mods, ctx);
        tracing::trace!(key = ?event.logical_key, text = ?event.text, ?outcome, line = pane.editor.line.text(), "editor key");
        match outcome {
            Outcome::Ignored => {}
            Outcome::Changed => {
                pane.terminal.scroll_to_bottom();
                pane.terminal.clear_selection();
                self.window.request_redraw();
            }
            Outcome::Submit(line) => self.submit(id, &line),
            Outcome::PassThrough => {
                if let Some(key) = input::terminal_key(event) {
                    pane.write_key(key, mods);
                }
            }
        }
        Flow::Continue
    }

    /// Runs a command line from the editor in pane `id`.
    fn submit(&mut self, id: PaneId, line: &str) {
        let Some(pane) = self.ws.panes.get_mut(&id) else {
            return;
        };
        tracing::debug!(line, "submit");
        if !line.trim().is_empty() {
            self.engine.history.add(line, pane.cwd.as_deref());
        }
        pane.editor = crate::editor::PromptEditor::default();
        pane.shell = ShellState::Running;
        pane.terminal.scroll_to_bottom();
        pane.terminal.clear_selection();
        pane.terminal.write(format!("{line}\r").into_bytes());
        self.window.request_redraw();
    }

    fn paste_into_editor(&mut self, config: &Config, id: PaneId, text: &str) {
        let engine = &self.engine;
        let Some(pane) = self.ws.panes.get_mut(&id) else {
            return;
        };
        if text.contains(['\n', '\r']) {
            // Multi-line: hand everything to the shell as a regular paste.
            let typed = pane.editor.line.take();
            pane.editor = crate::editor::PromptEditor::default();
            pane.shell = ShellState::Running;
            pane.terminal.paste(&format!("{typed}{text}"));
        } else {
            let ctx = Ctx {
                engine,
                cwd: pane.cwd.as_deref(),
                quoting: pane.quoting(),
                suggestions: config.input.suggestions,
            };
            pane.editor.insert(text, ctx);
        }
        self.window.request_redraw();
    }

    fn on_palette_key(&mut self, config: &Config, event: &KeyEvent) -> Flow {
        let Some(palette) = self.palette.as_mut() else {
            return Flow::Continue;
        };
        tracing::trace!(key = ?event.logical_key, text = ?event.text, "palette key");
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.palette = None,
            Key::Named(NamedKey::Enter) => {
                let action = palette.selected_action();
                self.palette = None;
                self.window.request_redraw();
                if let Some(action) = action {
                    return self.run(config, action);
                }
            }
            Key::Named(NamedKey::ArrowUp) => palette.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => palette.move_selection(1),
            Key::Named(NamedKey::Backspace) => palette.backspace(),
            _ => {
                if let Some(text) = event.text.as_deref()
                    && !text.chars().any(char::is_control)
                {
                    palette.push(text);
                }
            }
        }
        self.window.request_redraw();
        Flow::Continue
    }

    /// Replaces an exited pane's shell with a fresh one.
    fn restart(&mut self, config: &Config, id: PaneId) {
        let Some((grid, cwd)) = self
            .ws
            .panes
            .get(&id)
            .map(|p| (p.terminal.size(), p.cwd.clone()))
        else {
            return;
        };
        match self.spawn(config, id, grid, cwd) {
            Ok(terminal) => {
                if let Some(pane) = self.ws.panes.get_mut(&id) {
                    pane.replace_terminal(terminal);
                }
                self.update_title();
                self.window.request_redraw();
            }
            Err(e) => {
                tracing::error!("failed to restart the shell: {e:#}");
                if let Some(pane) = self.ws.panes.get(&id) {
                    pane.terminal
                        .print_local(&format!("\r\n\x1b[31mfailed to restart: {e}\x1b[0m\r\n"));
                }
            }
        }
    }

    fn hit(&self) -> Hit {
        chrome::hit_test(
            self.cursor.x as f32,
            self.cursor.y as f32,
            &self.layout,
            &self.metrics,
            &self.ws,
            self.palette.as_ref(),
        )
    }

    fn update_hover(&mut self) {
        let hit = self.hit();
        let icon = match &hit {
            Hit::Divider(d) if d.dir == SplitDir::Right => CursorIcon::ColResize,
            Hit::Divider(_) => CursorIcon::RowResize,
            Hit::Pane(_) => CursorIcon::Text,
            Hit::Tab(_) | Hit::CloseTab(_) | Hit::NewTab | Hit::PaletteRow(_) => {
                CursorIcon::Pointer
            }
            Hit::None | Hit::Palette => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            self.window.set_cursor(icon);
        }
        // Only chrome hover changes need a redraw.
        let chrome_hover = |h: &Hit| !matches!(h, Hit::Pane(_) | Hit::None);
        if hit != self.hover && (chrome_hover(&hit) || chrome_hover(&self.hover)) {
            self.window.request_redraw();
        }
        self.hover = hit;
    }

    fn on_cursor_moved(&mut self, config: &Config, pos: PhysicalPosition<f64>) {
        self.cursor = pos;
        if let Some(d) = self.drag.clone() {
            if let Some(tab) = self.ws.active_tab_mut() {
                tab.tree.drag_divider(&d, pos.x as f32, pos.y as f32);
            }
            self.relayout(config.terminal.padding);
            return;
        }
        let (cell, mods) = (self.cell(), input::modifiers(self.modifiers));
        if let Some(id) = self.capture {
            if let Some(pane) = self.ws.panes.get_mut(&id)
                && pane.on_mouse_move(pos, cell, mods)
            {
                self.window.request_redraw();
            }
            return;
        }
        self.update_hover();
        if let Hit::Pane(id) = self.hover
            && let Some(pane) = self.ws.panes.get_mut(&id)
            && pane.on_mouse_move(pos, cell, mods)
        {
            self.window.request_redraw();
        }
    }

    fn on_mouse_input(
        &mut self,
        config: &Config,
        state: ElementState,
        button: MouseButton,
    ) -> Flow {
        let (cell, mods) = (self.cell(), input::modifiers(self.modifiers));
        if state == ElementState::Released {
            self.drag = None;
            if let Some(id) = self.capture
                && let Some(pane) = self.ws.panes.get_mut(&id)
            {
                if pane.on_mouse_button(
                    state,
                    button,
                    self.cursor,
                    cell,
                    mods,
                    self.clipboard.as_mut(),
                ) {
                    self.window.request_redraw();
                }
                if !pane.has_mouse_capture() {
                    self.capture = None;
                }
            }
            return Flow::Continue;
        }

        let hit = self.hit();
        if self.palette.is_some() {
            match hit {
                Hit::PaletteRow(i) => {
                    let action = self
                        .palette
                        .as_ref()
                        .and_then(|p| p.matches.get(i).copied());
                    self.palette = None;
                    self.window.request_redraw();
                    if let Some(action) = action {
                        return self.run(config, action);
                    }
                }
                Hit::Palette => {}
                _ => {
                    self.palette = None;
                    self.window.request_redraw();
                }
            }
            return Flow::Continue;
        }

        match (hit, button) {
            (Hit::Tab(i), MouseButton::Left) => self.activate_tab(i),
            (Hit::Tab(i) | Hit::CloseTab(i), MouseButton::Middle)
            | (Hit::CloseTab(i), MouseButton::Left) => return self.close_tab(config, i),
            (Hit::NewTab, MouseButton::Left) => return self.run(config, Action::NewTab),
            (Hit::Divider(d), MouseButton::Left) => self.drag = Some(d),
            (Hit::Pane(id), _) => {
                if self.ws.focused_id() != Some(id) {
                    self.ws.focus(id);
                    self.update_title();
                    self.window.request_redraw();
                }
                if let Some(pane) = self.ws.panes.get_mut(&id) {
                    if pane.on_mouse_button(
                        state,
                        button,
                        self.cursor,
                        cell,
                        mods,
                        self.clipboard.as_mut(),
                    ) {
                        self.window.request_redraw();
                    }
                    if pane.has_mouse_capture() {
                        self.capture = Some(id);
                    }
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    fn on_wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let (cell, mods, pos) = (self.cell(), input::modifiers(self.modifiers), self.cursor);
        if let Hit::Pane(id) = self.hit()
            && let Some(pane) = self.ws.panes.get_mut(&id)
            && pane.on_wheel(delta, pos, cell, mods)
        {
            self.window.request_redraw();
        }
    }

    fn on_terminal_event(&mut self, pane_id: PaneId, session: u64, event: TerminalEvent) {
        if self.sessions.get(&pane_id) != Some(&session) {
            return; // From a terminal that was closed or replaced.
        }
        let visible = self.ws.active_tab().is_some_and(|t| {
            t.layout(self.layout.content, self.metrics.gap)
                .iter()
                .any(|(id, _)| *id == pane_id)
        });
        let tab_index = self.ws.tab_of(pane_id);
        let Some(pane) = self.ws.panes.get_mut(&pane_id) else {
            return;
        };
        match event {
            TerminalEvent::Wakeup => {
                if visible {
                    self.window.request_redraw();
                } else if let Some(tab) = tab_index.and_then(|i| self.ws.tabs.get_mut(i))
                    && !tab.activity
                {
                    tab.activity = true;
                    self.window.request_redraw();
                }
            }
            TerminalEvent::Title(title) => {
                pane.title = title;
                self.update_title();
                self.window.request_redraw();
            }
            TerminalEvent::ResetTitle => {
                pane.title.clear();
                self.update_title();
                self.window.request_redraw();
            }
            TerminalEvent::Bell => {
                tracing::debug!(pane = pane_id, "bell");
                self.window.request_user_attention(None);
            }
            TerminalEvent::ClipboardStore(text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            TerminalEvent::Exited(code) => pane.on_shell_exit(code),
            TerminalEvent::Shell(event) => {
                tracing::trace!(pane = pane_id, ?event, "shell event");
                pane.on_shell_event(event);
                self.update_title();
                self.window.request_redraw();
            }
        }
    }

    fn draw(&mut self, background: [u8; 3]) -> Result<FrameOutcome, mao_render::RenderError> {
        self.visible.clear();
        let cell = self.cell();
        let editor_enabled = self.editor_enabled;
        if let Some(tab) = self.ws.active_tab() {
            let focused_pane = tab.focused;
            for (id, _) in tab.layout(self.layout.content, self.metrics.gap) {
                self.visible.push(id);
                let Some(pane) = self.ws.panes.get_mut(&id) else {
                    continue;
                };
                let full = std::mem::take(&mut pane.full_redraw);
                if full {
                    self.renderer.invalidate_grid(id);
                }
                let focused = self.focused && id == focused_pane && self.palette.is_none();

                // The prompt editor, drawn over the terminal where the prompt ends.
                let size = pane.terminal.size();
                let overlay = if pane.editor_active(editor_enabled) {
                    pane.terminal.cursor_position().map(|start| {
                        overlay::layout(
                            &pane.editor,
                            start,
                            usize::from(size.cols),
                            usize::from(size.rows),
                            &self.overlay_colors,
                        )
                    })
                } else {
                    None
                };
                let new_rows = overlay.as_ref().map(|o| o.rows.clone()).unwrap_or_default();
                let stale = std::mem::replace(&mut pane.overlay_rows, new_rows);
                pane.terminal
                    .invalidate_rows(stale.into_iter().chain(pane.overlay_rows.iter().copied()));

                let renderer = &mut self.renderer;
                let row_buf = &mut self.row_buf;
                let info = pane.terminal.render(
                    RenderOptions {
                        focused,
                        force_full: full,
                        hide_cursor: overlay.is_some(),
                    },
                    |row, cells| match &overlay {
                        Some(o) if o.rows.contains(&row) => {
                            row_buf.clear();
                            row_buf.extend(cells.iter().map(to_grid_cell));
                            for (r, c, cell) in &o.cells {
                                if *r == row
                                    && let Some(slot) = row_buf.get_mut(*c)
                                {
                                    *slot = *cell;
                                }
                            }
                            renderer.set_row(id, row, row_buf.drain(..));
                        }
                        _ => renderer.set_row(id, row, cells.iter().map(to_grid_cell)),
                    },
                );
                let cursor = match &overlay {
                    Some(o) => Some(mao_render::Cursor {
                        row: o.caret.0,
                        col: o.caret.1,
                        shape: if focused {
                            mao_render::CursorShape::Beam
                        } else {
                            mao_render::CursorShape::HollowBlock
                        },
                        color: self.overlay_colors.cursor,
                        wide: false,
                    }),
                    None => info.cursor.map(to_cursor),
                };
                renderer.set_cursor(id, cursor);
                pane.menu_anchor = overlay.as_ref().map(|o| {
                    let (r, c) = o.menu_anchor;
                    let x = pane.origin.0 + c as f32 * cell.width;
                    let top = pane.origin.1 + r as f32 * cell.height;
                    (x, top + cell.height, top)
                });
            }
        }
        let menu = self.ws.focused_pane().and_then(|p| {
            let menu = p.editor.menu.as_ref()?;
            let (x, below, above) = p.menu_anchor?;
            Some(MenuView {
                menu,
                x,
                below,
                above,
            })
        });
        chrome::draw(
            &mut self.ui,
            &DrawState {
                layout: &self.layout,
                metrics: &self.metrics,
                colors: &self.colors,
                workspace: &self.ws,
                palette: self.palette.as_ref(),
                hover: &self.hover,
                window_focused: self.focused,
                menu,
            },
        );
        self.renderer.render(background, &self.visible, &self.ui)
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        match self.create_window(event_loop) {
            Ok(mut win) => {
                // Testing aid: run actions by palette name at startup, separated by `|`
                // (e.g. `MAO_DEBUG_ACTIONS="Split Pane Right|New Tab"`).
                if let Ok(list) = std::env::var("MAO_DEBUG_ACTIONS") {
                    for name in list.split('|').map(str::trim).filter(|n| !n.is_empty()) {
                        match crate::actions::PALETTE
                            .iter()
                            .chain(&[Action::CommandPalette])
                            .find(|a| a.name().eq_ignore_ascii_case(name))
                        {
                            Some(&action) => {
                                win.run(&self.config, action);
                            }
                            None => tracing::warn!(name, "MAO_DEBUG_ACTIONS: unknown action"),
                        }
                    }
                }
                self.win = Some(win);
                // Draw the first frame right away: hidden windows may never
                // receive `RedrawRequested` on some platforms.
                self.redraw(event_loop);
            }
            Err(e) => self.fail(event_loop, e.context("failed to start")),
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(w) = self.win.as_mut() else {
            return;
        };
        match event {
            UserEvent::Terminal {
                pane,
                session,
                event,
            } => w.on_terminal_event(pane, session, event),
            UserEvent::Executables(names) => {
                tracing::debug!(count = names.len(), "PATH executables scanned");
                w.engine.set_executables(names);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            self.redraw(event_loop);
            return;
        }
        let config = &self.config;
        let Some(w) = self.win.as_mut() else {
            return;
        };
        let flow = match event {
            WindowEvent::CloseRequested => Flow::Quit,
            WindowEvent::Resized(size) => {
                w.renderer.resize(size.width, size.height);
                w.relayout(config.terminal.padding);
                Flow::Continue
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                w.scale = scale_factor as f32;
                let size = w.renderer.font_size();
                w.set_font_size(config, size);
                Flow::Continue
            }
            WindowEvent::Focused(focused) => {
                w.focused = focused;
                if let Some(pane) = w.ws.focused_pane() {
                    pane.terminal.report_focus(focused);
                }
                w.window.request_redraw();
                Flow::Continue
            }
            WindowEvent::ModifiersChanged(mods) => {
                w.modifiers = mods.state();
                Flow::Continue
            }
            WindowEvent::KeyboardInput { event, .. } => w.on_key(config, &event),
            WindowEvent::MouseInput { state, button, .. } => {
                w.on_mouse_input(config, state, button)
            }
            WindowEvent::CursorMoved { position, .. } => {
                w.on_cursor_moved(config, position);
                Flow::Continue
            }
            WindowEvent::CursorLeft { .. } => {
                if w.hover != Hit::None {
                    w.hover = Hit::None;
                    w.window.request_redraw();
                }
                Flow::Continue
            }
            WindowEvent::MouseWheel { delta, .. } => {
                w.on_wheel(delta);
                Flow::Continue
            }
            _ => Flow::Continue,
        };
        if flow == Flow::Quit {
            event_loop.exit();
        }
    }
}

/// Spawns the configured shell, forwarding its events to the UI thread.
fn spawn_terminal(
    config: &Config,
    grid: GridSize,
    proxy: &EventLoopProxy<UserEvent>,
    pane: PaneId,
    session: u64,
    cwd: Option<PathBuf>,
) -> anyhow::Result<Terminal> {
    let theme = &config.theme;
    let palette = TermPalette::new(
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
            working_directory: cwd.filter(|p| p.is_dir()),
            scrollback: config.terminal.scrollback,
            palette,
            integration_dir: config
                .shell
                .integration
                .then(|| mao_config::data_dir().map(|d| d.join("shell-integration")))
                .flatten(),
        },
        grid,
        move |event| {
            // Fails only if the event loop is gone (shutting down).
            let _ = proxy.send_event(UserEvent::Terminal {
                pane,
                session,
                event,
            });
        },
    )?;
    Ok(terminal)
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
