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
use mao_ui::{Divider, PaneId, Rect, SplitDir};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Window, WindowId};

use crate::actions::Action;
use crate::chrome::{
    self, Activity, Colors, ContextMenu, DrawState, FileAction, Hit, InputView, Layout, MenuView,
    Metrics,
};
use crate::docpane::{DocButton, DocColors, DocPane};
use crate::editor::{Ctx, Outcome};
use crate::input;
use crate::overlay::{self, OverlayColors};
use crate::palette::Palette;
use crate::pane::{Pane, ShellState, grid_for};
use crate::sidebar::{Panel, RowAction, Sidebar};
use crate::workspace::{Closed, Workspace};

const MIN_FONT_SIZE: f32 = 6.0;
/// Grid ids of the input boxes: the pane id with this bit set.
const INPUT_GRID: u64 = 1 << 60;
/// Most rows the input box grows to.
const MAX_INPUT_ROWS: usize = 8;
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
    /// File explorer listings.
    Fs(mao_fs::FsEvent),
    /// A document was loaded (or reloaded after a change on disk).
    Doc(Arc<mao_docs::Document>),
    /// A system sample (monitor and agents panels).
    Sys(Arc<mao_sysmon::Snapshot>),
}

/// A tree entry being dragged (to drop its path on a terminal).
struct TreeDrag {
    path: PathBuf,
    is_dir: bool,
    start: PhysicalPosition<f64>,
    /// Moved far enough to count as a drag.
    active: bool,
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
    /// Editor in a box at the bottom of panes (Warp-style) instead of inline.
    input_bottom: bool,
    overlay_colors: OverlayColors,
    /// Input boxes drawn this frame.
    inputs: Vec<InputView>,
    /// Scratch row for compositing the editor overlay.
    row_buf: Vec<mao_render::GridCell>,
    sidebar: Sidebar,
    context: Option<ContextMenu>,
    /// Dragging the sidebar's edge.
    sidebar_resize: bool,
    tree_drag: Option<TreeDrag>,
    /// Loads documents; created when the first one is opened.
    docs: Option<mao_docs::DocService>,
    doc_theme: mao_docs::Theme,
    doc_colors: DocColors,
    /// Images drawn this frame.
    images: Vec<mao_render::ImageDraw>,
    /// A shell asked to show this file (`mao open`).
    pending_open: Option<PathBuf>,
    /// We draw the title bar (window has no OS decorations).
    custom_frame: bool,
    /// Last click on the empty title bar (double click = maximize).
    title_click: Option<Instant>,
    /// Theme to apply (previewed, or kept when `true`), handled by `App`
    /// because the config changes.
    theme_request: Option<(String, bool)>,
    /// System sampler; created when a panel first needs it.
    sampler: Option<mao_sysmon::Sampler>,
    detectors: Arc<Vec<mao_agents::Detector>>,
    /// Clickable geometry of the sidebar panels (last frame).
    panel_hits: crate::panels::PanelHits,
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
        let win_cfg = &self.config.window;
        let custom_frame = win_cfg.decorations == mao_config::Decorations::Custom;
        let translucent = win_cfg.opacity < 1.0;
        let mut attributes = Window::default_attributes()
            .with_title("Mao-Term")
            .with_inner_size(LogicalSize::new(win_cfg.width, win_cfg.height))
            .with_decorations(!custom_frame)
            .with_transparent(translucent);
        #[cfg(windows)]
        {
            use winit::platform::windows::{
                BackdropType, CornerPreference, WindowAttributesExtWindows,
            };
            attributes = attributes
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round);
            if translucent {
                // The swapchain lives on a composition visual (see the renderer).
                attributes = attributes
                    .with_no_redirection_bitmap(true)
                    .with_system_backdrop(match win_cfg.backdrop {
                        mao_config::Backdrop::Acrylic => BackdropType::TransientWindow,
                        mao_config::Backdrop::Mica => BackdropType::MainWindow,
                        mao_config::Backdrop::None => BackdropType::None,
                    });
            }
        }
        let attributes = attributes
            // Testing aid: open without taking keyboard focus, so automated checks
            // never capture what the user is typing elsewhere. Showing a hidden
            // window later would activate it, so in that mode it starts visible.
            .with_visible(background)
            .with_active(!background);
        let t = Instant::now();
        let window = Arc::new(event_loop.create_window(attributes)?);
        if translucent {
            crate::platform::extend_backdrop(&window);
        }
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
            RendererOptions {
                opacity: self.config.window.opacity,
                ..RendererOptions::default()
            },
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
            layout: Layout::new((size.width, size.height), &metrics, 1, None),
            metrics,
            colors: Colors::new(&self.config.palette),
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
            input_bottom: self.config.input.editor
                && self.config.input.position == mao_config::InputPosition::Bottom,
            inputs: Vec::new(),
            overlay_colors: OverlayColors::new(&self.config.palette),
            row_buf: Vec::new(),
            sidebar: Sidebar::default(),
            context: None,
            sidebar_resize: false,
            tree_drag: None,
            docs: None,
            doc_theme: doc_theme(&self.config.palette),
            doc_colors: doc_colors(&self.config.palette),
            images: Vec::new(),
            pending_open: None,
            custom_frame,
            title_click: None,
            theme_request: None,
            sampler: None,
            detectors: Arc::new(detectors(&self.config)),
            panel_hits: crate::panels::PanelHits::default(),
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
        let background = self.config.palette.background.to_array();
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

    /// Sidebar width in physical pixels, if open.
    fn sidebar_px(&self) -> Option<f32> {
        self.sidebar
            .visible
            .then_some(self.sidebar.width * self.scale)
    }

    /// Keeps the file explorer pointed at the focused pane's folder, and its
    /// service running only while it is on screen.
    fn sync_sidebar(&mut self) {
        // The sampler runs only while the agents or monitor panel is shown.
        let interest = self.sidebar.interest();
        if interest.any() && self.sampler.is_none() {
            let proxy = self.proxy.clone();
            let sampler = mao_sysmon::Sampler::new(move |snap| {
                let _ = proxy.send_event(UserEvent::Sys(snap));
            });
            sampler.set_detail_filter(Some(mao_agents::detail_filter(Arc::clone(&self.detectors))));
            self.sampler = Some(sampler);
        }
        if let Some(sampler) = self.sampler.as_mut() {
            sampler.set_interest(interest);
        }
        if !self.sidebar.files_active() {
            self.sidebar.files.deactivate();
            return;
        }
        let root = self.current_cwd().or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        });
        if let Some(root) = root {
            let proxy = self.proxy.clone();
            self.sidebar.files.activate(&root, move || {
                mao_fs::FsService::new(move |e| {
                    let _ = proxy.send_event(UserEvent::Fs(e));
                })
            });
        }
    }

    fn set_sidebar(&mut self, config: &Config, visible: bool, panel: Panel) {
        self.sidebar.visible = visible;
        self.sidebar.panel = panel;
        if !visible {
            self.sidebar.files.focused = false;
        }
        self.relayout(config.terminal.padding);
    }

    /// Writes `path` (quoted for the focused shell) where the user is typing.
    fn insert_path(&mut self, config: &Config, path: &std::path::Path) {
        let Some(id) = self.ws.focused_id() else {
            return;
        };
        let engine = &self.engine;
        let Some(pane) = self.ws.panes.get_mut(&id) else {
            return;
        };
        // Relative to the shell's folder when inside it: shorter and what you'd type.
        let shown = pane
            .cwd
            .as_deref()
            .and_then(|cwd| path.strip_prefix(cwd).ok())
            .filter(|rel| !rel.as_os_str().is_empty())
            .unwrap_or(path);
        let text = pane.quoting().quote(&shown.display().to_string());
        if pane.editor_active(config.input.editor) {
            let ctx = Ctx {
                engine,
                cwd: pane.cwd.as_deref(),
                quoting: pane.quoting(),
                suggestions: config.input.suggestions,
            };
            pane.editor.insert(&format!("{text} "), ctx);
        } else {
            pane.terminal.paste(&format!("{text} "));
        }
        self.sidebar.files.focused = false;
        self.window.request_redraw();
    }

    /// Runs `cd <dir>` in the focused pane, if its shell is at the prompt.
    fn cd_into(&mut self, config: &Config, dir: &std::path::Path) {
        let Some(id) = self.ws.focused_id() else {
            return;
        };
        let Some(pane) = self.ws.panes.get(&id) else {
            return;
        };
        if !pane.editor_active(config.input.editor) {
            tracing::info!("not at a prompt; not running cd");
            return;
        }
        let line = format!("cd {}", pane.quoting().quote(&dir.display().to_string()));
        self.submit(id, &line);
    }

    fn file_action(&mut self, config: &Config, action: FileAction, path: &std::path::Path) {
        let relative = || {
            self.sidebar
                .files
                .tree
                .root()
                .and_then(|r| path.strip_prefix(r).ok())
                .map_or_else(|| path.display().to_string(), |p| p.display().to_string())
        };
        match action {
            FileAction::Open => self.open_document(config, path, true),
            FileAction::OpenExternally => {
                if let Err(e) = mao_fs::open_with_default_app(path) {
                    tracing::warn!("cannot open {}: {e}", path.display());
                }
            }
            FileAction::Cd => self.cd_into(config, path),
            FileAction::OpenInNewTab => {
                let id = self.ws.next_id();
                let layout = Layout::new(
                    self.renderer.size(),
                    &self.metrics,
                    self.ws.tabs.len() + 1,
                    self.sidebar_px(),
                );
                let grid = grid_for(layout.content, self.metrics.pad, self.cell());
                match self.spawn(config, id, grid, Some(path.to_path_buf())) {
                    Ok(terminal) => {
                        self.ws.add_tab(Pane::new(id, terminal));
                        self.relayout(config.terminal.padding);
                        self.update_title();
                    }
                    Err(e) => tracing::error!("failed to open a tab: {e:#}"),
                }
            }
            FileAction::InsertPath => self.insert_path(config, path),
            FileAction::CopyPath | FileAction::CopyRelativePath => {
                let text = if action == FileAction::CopyPath {
                    path.display().to_string()
                } else {
                    relative()
                };
                if let Some(clipboard) = self.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            FileAction::Reveal => {
                if let Err(e) = mao_fs::reveal_in_file_manager(path) {
                    tracing::warn!("cannot reveal {}: {e}", path.display());
                }
            }
            // Process actions are handled by the context menu itself.
            FileAction::GoToPane
            | FileAction::OpenSessions
            | FileAction::CopyPid
            | FileAction::KillProcess
            | FileAction::ConfirmKill
            | FileAction::Cancel => {}
        }
    }

    /// Keys while the file tree has focus. Returns `false` to fall through.
    fn on_tree_key(&mut self, config: &Config, event: &KeyEvent) -> bool {
        let visible = self.layout.visible_rows(&self.metrics);
        let files = &mut self.sidebar.files;
        match &event.logical_key {
            Key::Named(NamedKey::ArrowDown) => {
                files.move_selection(1, visible);
            }
            Key::Named(NamedKey::ArrowUp) => {
                files.move_selection(-1, visible);
            }
            Key::Named(NamedKey::PageDown) => {
                files.move_selection(visible as isize, visible);
            }
            Key::Named(NamedKey::PageUp) => {
                files.move_selection(-(visible as isize), visible);
            }
            Key::Named(NamedKey::ArrowRight) => files.expand_selected(true, visible),
            Key::Named(NamedKey::ArrowLeft) => files.expand_selected(false, visible),
            Key::Named(NamedKey::Escape) => files.focused = false,
            Key::Named(NamedKey::Enter) => {
                let Some(sel) = files.selected.clone() else {
                    return true;
                };
                let is_dir = files
                    .row_paths()
                    .iter()
                    .find(|(p, _)| *p == sel)
                    .is_some_and(|(_, d)| *d);
                let ctrl = self.modifiers.control_key();
                match (is_dir, ctrl) {
                    (true, true) => self.cd_into(config, &sel),
                    (true, false) => self.sidebar.files.toggle(&sel),
                    (false, true) => self.insert_path(config, &sel),
                    // Preview it, keeping the tree focused for more browsing.
                    (false, false) => self.open_document(config, &sel, false),
                }
            }
            _ => {
                let mods = input::modifiers(self.modifiers);
                match event.text.as_deref() {
                    Some(t) if !mods.ctrl && !t.chars().any(char::is_control) => {
                        files.type_ahead(t, visible);
                    }
                    _ => return false,
                }
            }
        }
        self.window.request_redraw();
        true
    }

    /// Recomputes chrome and pane geometry for every tab and resizes the PTYs.
    fn relayout(&mut self, padding: f32) {
        self.metrics = self.make_metrics(padding);
        self.layout = Layout::new(
            self.renderer.size(),
            &self.metrics,
            self.ws.tabs.len(),
            self.sidebar_px(),
        );
        let cell = self.cell();
        let (content, gap, pad) = (self.layout.content, self.metrics.gap, self.metrics.pad);
        let header = chrome::doc_header(&self.metrics);
        let input_h = self.input_height();
        let bottom = self.input_bottom;
        for tab in &self.ws.tabs {
            for (id, rect) in tab.layout(content, gap) {
                if let Some(doc) = self.ws.docs.get_mut(&id) {
                    doc.set_rect(rect, header, pad, cell);
                    let b = doc.body;
                    self.renderer.set_grid_layout(
                        id,
                        doc.origin,
                        doc.cols,
                        doc.rows,
                        [b.x, b.y, b.w, b.h],
                    );
                }
                if let Some(pane) = self.ws.panes.get_mut(&id) {
                    let boxed = pane.wants_box(bottom).then_some(input_h);
                    let grid = pane.layout(rect, pad, cell, boxed);
                    let clip_h = pane.input_rect.map_or(rect.h, |r| r.y - rect.y);
                    self.renderer.set_grid_layout(
                        id,
                        pane.origin,
                        usize::from(grid.cols),
                        usize::from(grid.rows),
                        [rect.x, rect.y, rect.w, clip_h],
                    );
                }
            }
        }
        self.window.request_redraw();
    }

    /// Metrics, with the custom title bar's needs.
    fn make_metrics(&self, padding: f32) -> Metrics {
        let mut m = Metrics::new(self.scale, padding);
        m.window_buttons = self.custom_frame;
        m.resize_border = if self.custom_frame && !self.window.is_maximized() {
            m.px(5.0)
        } else {
            0.0
        };
        m
    }

    /// Height of the bottom input box with one line of text.
    fn input_height(&self) -> f32 {
        self.metrics.px(30.0) + self.cell().height + self.metrics.px(10.0)
    }

    /// Directory new shells start in: the focused pane's.
    fn current_cwd(&self) -> Option<PathBuf> {
        if let Some(doc) = self.ws.focused_doc() {
            return doc.path.parent().map(std::path::Path::to_path_buf);
        }
        self.ws.focused_pane().and_then(|p| p.cwd.clone())
    }

    /// Shows `path` in a document pane: the active tab's (reused), or a new
    /// one split to the right of the focused pane.
    fn open_document(&mut self, config: &Config, path: &std::path::Path, focus: bool) {
        let path = path.to_path_buf();
        if path.is_dir() {
            return;
        }
        let max_texture = self.renderer.max_texture_size();
        let proxy = self.proxy.clone();
        let theme = self.doc_theme;
        let service = self.docs.get_or_insert_with(|| {
            mao_docs::DocService::new(theme, max_texture, move |doc| {
                let _ = proxy.send_event(UserEvent::Doc(doc));
            })
        });
        let existing = self.ws.active_tab().and_then(|tab| {
            tab.tree
                .panes()
                .into_iter()
                .find(|id| self.ws.docs.contains_key(id))
        });
        let id = if let Some(id) = existing {
            if let Some(doc) = self.ws.docs.get_mut(&id) {
                if doc.path != path {
                    service.close(&doc.path);
                    service.open(&path);
                    doc.replace_path(path);
                    self.renderer.remove_image(id);
                } else {
                    service.reload(&path);
                }
            }
            id
        } else {
            let Some(target) = self.ws.focused_id() else {
                return;
            };
            let id = self.ws.next_id();
            let Some(tab) = self.ws.active_tab_mut() else {
                return;
            };
            tab.zoomed = None;
            tab.tree.split(target, SplitDir::Right, id);
            service.open(&path);
            self.ws.docs.insert(id, DocPane::new(id, path));
            id
        };
        if focus {
            self.ws.focus(id);
            self.sidebar.files.focused = false;
        }
        self.relayout(config.terminal.padding);
        self.update_title();
    }

    /// A document finished loading: show it in every pane that has it open.
    fn on_document(&mut self, doc: Arc<mao_docs::Document>) {
        for pane in self.ws.docs.values_mut() {
            if pane.path == doc.path {
                pane.set_document(Arc::clone(&doc));
                self.renderer.remove_image(pane.id);
            }
        }
        self.window.request_redraw();
    }

    /// Keys while a document pane has focus. Returns `true` if handled.
    fn on_doc_key(&mut self, config: &Config, event: &KeyEvent) -> bool {
        let mods = input::modifiers(self.modifiers);
        let Some(id) = self.ws.focused_id() else {
            return false;
        };
        let Some(doc) = self.ws.docs.get_mut(&id) else {
            return false;
        };
        let page = doc.rows.saturating_sub(1).max(1) as isize;
        let image = doc.image().is_some();
        let key = event.key_without_modifiers();

        // Typing into the search box.
        if doc.search.is_some() {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => doc.close_search(),
                Key::Named(NamedKey::Enter) => doc.search_step(!mods.shift),
                Key::Named(NamedKey::Backspace) => doc.search_edit(|q| {
                    q.pop();
                }),
                _ => match event.text.as_deref() {
                    Some(t) if !mods.ctrl && !t.chars().any(char::is_control) => {
                        doc.search_edit(|q| q.push_str(t));
                    }
                    _ => return self.doc_shortcut(config, event, id),
                },
            }
            self.window.request_redraw();
            return true;
        }

        match (&key, mods.ctrl, mods.shift, mods.alt) {
            (Key::Named(NamedKey::ArrowDown), false, _, false) if image => doc.pan.1 -= 40.0,
            (Key::Named(NamedKey::ArrowUp), false, _, false) if image => doc.pan.1 += 40.0,
            (Key::Named(NamedKey::ArrowLeft), false, _, false) if image => doc.pan.0 += 40.0,
            (Key::Named(NamedKey::ArrowRight), false, _, false) if image => doc.pan.0 -= 40.0,
            (Key::Named(NamedKey::ArrowDown), false, false, false) => doc.scroll_by(1),
            (Key::Named(NamedKey::ArrowUp), false, false, false) => doc.scroll_by(-1),
            (Key::Named(NamedKey::ArrowRight), false, false, false) => doc.scroll_columns(4),
            (Key::Named(NamedKey::ArrowLeft), false, false, false) => doc.scroll_columns(-4),
            (Key::Named(NamedKey::PageDown), _, false, false)
            | (Key::Named(NamedKey::Space), false, false, false) => {
                doc.scroll_by(page);
            }
            (Key::Named(NamedKey::PageUp), _, false, false)
            | (Key::Named(NamedKey::Space), false, true, false) => {
                doc.scroll_by(-page);
            }
            (Key::Named(NamedKey::Home), _, false, false) => {
                doc.scroll_by(isize::MIN / 2);
                doc.hscroll = 0;
            }
            (Key::Named(NamedKey::End), _, false, false) => doc.scroll_by(isize::MAX / 2),
            (Key::Named(NamedKey::Escape), false, false, false) => {
                if !doc.clear_selection() {
                    return false;
                }
            }
            (Key::Character(c), false, _, false) if image && (c == "+" || c == "=") => {
                doc.zoom_in()
            }
            (Key::Character(c), false, false, false) if image && c == "-" => doc.zoom_out(),
            (Key::Character(c), false, false, false) if image && c == "0" => doc.zoom_fit(),
            (Key::Character(c), false, false, false) if image && c == "1" => doc.zoom_actual(),
            (Key::Character(c), true, false, false) if c == "a" => doc.select_all(),
            _ => return self.doc_shortcut(config, event, id),
        }
        self.window.request_redraw();
        true
    }

    /// Document shortcuts that map to actions (Ctrl+F, Alt+Z, F5, F3, Ctrl+C).
    fn doc_shortcut(&mut self, config: &Config, event: &KeyEvent, id: PaneId) -> bool {
        let mods = input::modifiers(self.modifiers);
        let action = match (
            event.key_without_modifiers(),
            mods.ctrl,
            mods.shift,
            mods.alt,
        ) {
            (Key::Character(c), true, false, false) if c == "f" => Action::FindInDocument,
            (Key::Character(c), false, false, true) if c == "z" => Action::ToggleWordWrap,
            (Key::Named(NamedKey::F5), false, false, false) => Action::ReloadDocument,
            (Key::Named(NamedKey::F3), false, shift, false) => {
                if let Some(doc) = self.ws.docs.get_mut(&id) {
                    doc.search_step(!shift);
                    self.window.request_redraw();
                }
                return true;
            }
            (Key::Character(c), true, _, false) if c == "c" => Action::Copy,
            _ => return false,
        };
        let _ = self.run(config, action);
        true
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
        let layout = Layout::new(
            self.renderer.size(),
            &self.metrics,
            self.ws.tabs.len() + 1,
            self.sidebar_px(),
        );
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
        self.renderer.remove_grid(id | INPUT_GRID);
        if let Some(doc) = self.ws.docs.get(&id) {
            self.renderer.remove_image(id);
            if let Some(service) = self.docs.as_mut() {
                service.close(&doc.path);
                if service.is_idle() {
                    // Frees the loader thread, its watcher and the grammars.
                    self.docs = None;
                }
            }
        }
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
        if let Some(doc) = self.ws.focused_doc() {
            self.window.set_title(&format!("{} — Mao-Term", doc.name()));
            return;
        }
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
            Action::SelectTheme => {
                let current = config
                    .theme_name()
                    .unwrap_or(mao_config::themes::DEFAULT_THEME);
                // The canonical name of the current theme (config may say "nord").
                let current = mao_config::themes::find(current)
                    .map_or_else(|| current.to_owned(), |t| t.name);
                self.palette = Some(Palette::themes(mao_config::themes::list(), &current));
                self.window.request_redraw();
            }
            Action::OpenThemesFolder => {
                if let Some(dir) = mao_config::themes::themes_dir() {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        tracing::warn!("cannot create {}: {e}", dir.display());
                    }
                    if let Err(e) = mao_fs::open_with_default_app(&dir) {
                        tracing::warn!("cannot open {}: {e}", dir.display());
                    }
                }
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
                if let Some(doc) = self.ws.focused_doc() {
                    if let (Some(text), Some(clipboard)) =
                        (doc.selected_text(), self.clipboard.as_mut())
                    {
                        let _ = clipboard.set_text(text);
                    }
                } else if let Some(pane) = self.ws.focused_pane() {
                    pane.copy_selection(self.clipboard.as_mut());
                }
            }
            Action::ToggleMarkdownSource
            | Action::ToggleWordWrap
            | Action::FindInDocument
            | Action::ReloadDocument
            | Action::OpenDocumentExternally => self.doc_action(action),
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
            Action::ToggleSidebar => {
                let panel = self.sidebar.panel;
                self.set_sidebar(config, !self.sidebar.visible, panel);
            }
            Action::ShowPanel(panel) => {
                // Shown and focused → hide; otherwise show it and focus the tree.
                let showing = self.sidebar.visible && self.sidebar.panel == panel;
                if showing && (panel != Panel::Files || self.sidebar.files.focused) {
                    self.set_sidebar(config, false, panel);
                } else {
                    self.set_sidebar(config, true, panel);
                    self.sidebar.files.focused = panel == Panel::Files;
                }
            }
            Action::ToggleIgnoredFiles => {
                self.sidebar.files.toggle_ignored();
                self.window.request_redraw();
            }
            Action::CollapseFolders => {
                self.sidebar.files.collapse_all();
                self.window.request_redraw();
            }
            Action::OpenSettings => match mao_config::ensure_config_file() {
                Ok(path) => {
                    if let Err(e) = mao_fs::open_with_default_app(&path) {
                        tracing::warn!("cannot open {}: {e}", path.display());
                    }
                }
                Err(e) => tracing::error!("cannot create the config file: {e}"),
            },
            Action::Quit => return Flow::Quit,
        }
        Flow::Continue
    }

    /// Runs a document action on the focused document pane (or the active tab's).
    fn doc_action(&mut self, action: Action) {
        let id = self
            .ws
            .focused_id()
            .filter(|id| self.ws.docs.contains_key(id))
            .or_else(|| {
                self.ws
                    .active_tab()?
                    .tree
                    .panes()
                    .into_iter()
                    .find(|id| self.ws.docs.contains_key(id))
            });
        let Some(doc) = id.and_then(|id| self.ws.docs.get_mut(&id)) else {
            return;
        };
        match action {
            Action::ToggleMarkdownSource => doc.toggle_source(),
            Action::ToggleWordWrap => doc.toggle_wrap(),
            Action::FindInDocument => doc.start_search(),
            Action::ReloadDocument => {
                if let Some(service) = self.docs.as_ref() {
                    service.reload(&doc.path);
                }
            }
            Action::OpenDocumentExternally => {
                if let Err(e) = mao_fs::open_with_default_app(&doc.path) {
                    tracing::warn!("cannot open {}: {e}", doc.path.display());
                }
            }
            _ => {}
        }
        self.window.request_redraw();
    }

    fn on_key(&mut self, config: &Config, event: &KeyEvent) -> Flow {
        if event.state != ElementState::Pressed {
            return Flow::Continue;
        }
        if self.palette.is_some() {
            return self.on_palette_key(config, event);
        }
        if self.context.is_some() {
            if event.logical_key == Key::Named(NamedKey::Escape) {
                self.context = None;
                self.window.request_redraw();
            }
            return Flow::Continue;
        }
        if self.sidebar.files_active()
            && self.sidebar.files.focused
            && input::action_for(event, input::modifiers(self.modifiers)).is_none()
            && self.on_tree_key(config, event)
        {
            return Flow::Continue;
        }
        if self.ws.focused_doc().is_some() && self.on_doc_key(config, event) {
            return Flow::Continue;
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
        pane.running = (!line.trim().is_empty()).then(|| line.trim().to_owned());
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
            Key::Named(NamedKey::Escape) => {
                // Cancelling the theme picker restores the theme it started with.
                if let Some(original) = palette.original_theme() {
                    self.theme_request = Some((original.to_owned(), false));
                }
                self.palette = None;
            }
            Key::Named(NamedKey::Enter) => {
                let item = palette.selected_item();
                let theme = item.and_then(|i| palette.theme(i)).map(|t| t.name.clone());
                let action = palette.selected_action();
                self.palette = None;
                self.window.request_redraw();
                if let Some(name) = theme {
                    self.theme_request = Some((name, true));
                } else if let Some(action) = action {
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
        // Live preview of the highlighted theme.
        if let Some(p) = self.palette.as_ref()
            && let Some(theme) = p.selected_item().and_then(|i| p.theme(i))
        {
            self.theme_request = Some((theme.name.clone(), false));
        }
        self.window.request_redraw();
        Flow::Continue
    }

    /// Applies new colors everywhere (config already updated by the caller).
    fn apply_theme(&mut self, config: &Config) {
        let theme = &config.palette;
        self.colors = Colors::new(theme);
        self.overlay_colors = OverlayColors::new(theme);
        self.doc_theme = doc_theme(theme);
        self.doc_colors = doc_colors(theme);
        let term = term_palette(config);
        for pane in self.ws.panes.values_mut() {
            pane.terminal.set_palette(term.clone());
            pane.full_redraw = true;
        }
        // Rendered documents have their colors baked in: load them again.
        if let Some(service) = self.docs.take() {
            drop(service);
            let paths: Vec<PathBuf> = self.ws.docs.values().map(|d| d.path.clone()).collect();
            let proxy = self.proxy.clone();
            let max_texture = self.renderer.max_texture_size();
            let mut service = mao_docs::DocService::new(self.doc_theme, max_texture, move |doc| {
                let _ = proxy.send_event(UserEvent::Doc(doc));
            });
            for path in &paths {
                service.open(path);
            }
            self.docs = Some(service);
        }
        for doc in self.ws.docs.values_mut() {
            doc.dirty = true;
        }
        self.window.request_redraw();
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
            &chrome::Scene {
                layout: &self.layout,
                metrics: &self.metrics,
                workspace: &self.ws,
                palette: self.palette.as_ref(),
                context: self.context.as_ref(),
                sidebar: &self.sidebar,
                panel_hits: &self.panel_hits,
            },
        )
    }

    /// A system sample arrived: refresh the monitor and the agents list.
    fn on_sample(&mut self, snap: Arc<mao_sysmon::Snapshot>) {
        if let Some(procs) = snap.processes.as_ref() {
            // Shell pid → (pane, tab), to tell which agents run inside Mao-Term.
            let mut shells: HashMap<u32, (PaneId, usize)> = HashMap::new();
            for (t, tab) in self.ws.tabs.iter().enumerate() {
                for id in tab.tree.panes() {
                    if let Some(pid) = self.ws.panes.get(&id).and_then(|p| p.terminal.pid()) {
                        shells.insert(pid, (id, t));
                    }
                }
            }
            let entries = mao_agents::detect(procs, &self.detectors)
                .into_iter()
                .map(|agent| {
                    let pane = agent.ancestors.iter().find_map(|a| shells.get(a).copied());
                    crate::sidebar::AgentEntry { agent, pane }
                })
                .collect();
            self.sidebar.agents.set(entries);
        }
        self.sidebar.monitor.snap = Some(snap);
        if self.sidebar.visible {
            self.window.request_redraw();
        }
    }

    /// Focuses a pane wherever it is.
    fn go_to_pane(&mut self, id: PaneId) {
        if let Some(t) = self.ws.tab_of(id) {
            self.activate_tab(t);
            self.ws.focus(id);
            self.sidebar.files.focused = false;
            self.update_title();
            self.window.request_redraw();
        }
    }

    /// Opens a terminal tab in `dir`.
    fn open_tab_in(&mut self, config: &Config, dir: &std::path::Path) {
        self.file_action(config, FileAction::OpenInNewTab, dir);
    }

    fn update_hover(&mut self) {
        let hit = self.hit();
        let icon = match &hit {
            Hit::Resize(d) => {
                use winit::window::ResizeDirection as D;
                match d {
                    D::East | D::West => CursorIcon::EwResize,
                    D::North | D::South => CursorIcon::NsResize,
                    D::NorthEast | D::SouthWest => CursorIcon::NeswResize,
                    D::NorthWest | D::SouthEast => CursorIcon::NwseResize,
                }
            }
            Hit::WindowButton(_) => CursorIcon::Default,
            Hit::TitleBar => CursorIcon::Default,
            Hit::Divider(d) if d.dir == SplitDir::Right => CursorIcon::ColResize,
            Hit::Divider(_) => CursorIcon::RowResize,
            Hit::Pane(_) => CursorIcon::Text,
            Hit::SidebarEdge => CursorIcon::ColResize,
            Hit::Tab(_)
            | Hit::CloseTab(_)
            | Hit::NewTab
            | Hit::PaletteRow(_)
            | Hit::Activity(_)
            | Hit::ContextItem(_)
            | Hit::DocButton(..)
            | Hit::MonitorSort(_)
            | Hit::Process(_)
            | Hit::SidebarRow(_) => CursorIcon::Pointer,
            Hit::None | Hit::Palette | Hit::Sidebar | Hit::Context => CursorIcon::Default,
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
        if self.sidebar_resize {
            let x = pos.x as f32 - self.layout.activity.right();
            self.sidebar.width =
                (x / self.scale).clamp(crate::sidebar::MIN_WIDTH, crate::sidebar::MAX_WIDTH);
            self.relayout(config.terminal.padding);
            return;
        }
        if let Some(drag) = self.tree_drag.as_mut() {
            let moved = (pos.x - drag.start.x).hypot(pos.y - drag.start.y);
            if moved > f64::from(6.0 * self.scale) {
                drag.active = true;
            }
            if drag.active {
                self.window.request_redraw();
            }
            return;
        }
        if let Some(d) = self.drag.clone() {
            if let Some(tab) = self.ws.active_tab_mut() {
                tab.tree.drag_divider(&d, pos.x as f32, pos.y as f32);
            }
            self.relayout(config.terminal.padding);
            return;
        }
        let (cell, mods) = (self.cell(), input::modifiers(self.modifiers));
        if let Some(id) = self.capture
            && let Some(doc) = self.ws.docs.get_mut(&id)
        {
            if doc.drag(pos.x as f32, pos.y as f32) {
                self.window.request_redraw();
            }
            return;
        }
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
            self.sidebar_resize = false;
            if let Some(drag) = self.tree_drag.take() {
                if drag.active {
                    // Dropped on a terminal: insert the path there.
                    if let Hit::Pane(id) = self.hit() {
                        self.ws.focus(id);
                        self.insert_path(config, &drag.path);
                    }
                } else if button == MouseButton::Left {
                    match self.sidebar.files.click(&drag.path, drag.is_dir) {
                        // A single click previews files; the tree keeps focus.
                        RowAction::None if !drag.is_dir => {
                            self.open_document(config, &drag.path, false);
                            self.sidebar.files.focused = true;
                        }
                        RowAction::None => {}
                        RowAction::Open(path) => self.file_action(config, FileAction::Open, &path),
                        RowAction::Cd(path) => self.cd_into(config, &path),
                    }
                }
                self.window.request_redraw();
                return Flow::Continue;
            }
            if let Some(id) = self.capture
                && let Some(doc) = self.ws.docs.get_mut(&id)
            {
                doc.release();
                self.capture = None;
                return Flow::Continue;
            }
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
        if let Some(menu) = self.context.take() {
            if let Hit::ContextItem(i) = hit
                && let Some(&action) = menu.items.get(i)
            {
                match action {
                    FileAction::GoToPane => {
                        if let Some(id) = menu.pane {
                            self.go_to_pane(id);
                        }
                    }
                    FileAction::OpenSessions => {
                        if let Some(dir) = &menu.sessions
                            && let Err(e) = mao_fs::open_with_default_app(dir)
                        {
                            tracing::warn!("cannot open {}: {e}", dir.display());
                        }
                    }
                    FileAction::CopyPid => {
                        if let (Some((pid, _)), Some(clipboard)) =
                            (&menu.process, self.clipboard.as_mut())
                        {
                            let _ = clipboard.set_text(pid.to_string());
                        }
                    }
                    // Ask again before killing.
                    FileAction::KillProcess => self.context = Some(menu.confirm_kill()),
                    FileAction::ConfirmKill => {
                        if let (Some((pid, _)), Some(sampler)) = (&menu.process, &self.sampler) {
                            sampler.kill(*pid);
                        }
                    }
                    FileAction::Cancel => {}
                    _ => self.file_action(config, action, &menu.target),
                }
            }
            self.window.request_redraw();
            return Flow::Continue;
        }
        if self.palette.is_some() {
            match hit {
                Hit::PaletteRow(i) => {
                    let item = self
                        .palette
                        .as_ref()
                        .and_then(|p| p.matches.get(i).copied());
                    let theme = self
                        .palette
                        .as_ref()
                        .zip(item)
                        .and_then(|(p, i)| p.theme(i))
                        .map(|t| t.name.clone());
                    self.palette = None;
                    self.window.request_redraw();
                    match (theme, item) {
                        (Some(name), _) => self.theme_request = Some((name, true)),
                        (None, Some(crate::palette::Item::Action(action))) => {
                            return self.run(config, action);
                        }
                        _ => {}
                    }
                }
                Hit::Palette => {}
                _ => {
                    if let Some(original) = self.palette.as_ref().and_then(Palette::original_theme)
                    {
                        self.theme_request = Some((original.to_owned(), false));
                    }
                    self.palette = None;
                    self.window.request_redraw();
                }
            }
            return Flow::Continue;
        }

        match (hit, button) {
            (Hit::Resize(dir), MouseButton::Left) => {
                if let Err(e) = self.window.drag_resize_window(dir) {
                    tracing::debug!("resize drag failed: {e}");
                }
            }
            (Hit::WindowButton(b), MouseButton::Left) => match b {
                chrome::WindowButton::Minimize => self.window.set_minimized(true),
                chrome::WindowButton::Maximize => {
                    self.window.set_maximized(!self.window.is_maximized());
                }
                chrome::WindowButton::Close => return Flow::Quit,
            },
            (Hit::TitleBar, MouseButton::Left) => {
                let now = Instant::now();
                let double = self
                    .title_click
                    .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(400));
                self.title_click = (!double).then_some(now);
                if double {
                    self.window.set_maximized(!self.window.is_maximized());
                } else if let Err(e) = self.window.drag_window() {
                    tracing::debug!("window drag failed: {e}");
                }
            }
            (Hit::Tab(i), MouseButton::Left) => self.activate_tab(i),
            (Hit::Tab(i) | Hit::CloseTab(i), MouseButton::Middle)
            | (Hit::CloseTab(i), MouseButton::Left) => return self.close_tab(config, i),
            (Hit::NewTab, MouseButton::Left) => return self.run(config, Action::NewTab),
            (Hit::Divider(d), MouseButton::Left) => self.drag = Some(d),
            (Hit::Activity(Activity::Panel(panel)), MouseButton::Left) => {
                let show = !(self.sidebar.visible && self.sidebar.panel == panel);
                self.set_sidebar(config, show, panel);
            }
            (Hit::Activity(Activity::Settings), MouseButton::Left) => {
                return self.run(config, Action::OpenSettings);
            }
            (Hit::SidebarEdge, MouseButton::Left) => self.sidebar_resize = true,
            (Hit::DocButton(id, button), MouseButton::Left) => {
                self.ws.focus(id);
                self.update_title();
                let source = self.ws.docs.get(&id).is_some_and(|d| d.source);
                let action = match button {
                    DocButton::Close => return self.close_pane(config, id),
                    // Segmented: only a change of view toggles.
                    DocButton::Preview | DocButton::Source
                        if source == (button == DocButton::Source) =>
                    {
                        return Flow::Continue;
                    }
                    DocButton::Preview | DocButton::Source => Action::ToggleMarkdownSource,
                    DocButton::Wrap => Action::ToggleWordWrap,
                    DocButton::External => Action::OpenDocumentExternally,
                    DocButton::Fit => {
                        if let Some(doc) = self.ws.docs.get_mut(&id) {
                            if doc.zoom.is_none() {
                                doc.zoom_actual();
                            } else {
                                doc.zoom_fit();
                            }
                        }
                        self.window.request_redraw();
                        return Flow::Continue;
                    }
                };
                self.doc_action(action);
            }
            (Hit::Pane(id), _) if self.ws.docs.contains_key(&id) => {
                self.sidebar.files.focused = false;
                if self.ws.focused_id() != Some(id) {
                    self.ws.focus(id);
                    self.update_title();
                }
                let shift = self.modifiers.shift_key();
                let (x, y) = (self.cursor.x as f32, self.cursor.y as f32);
                if let Some(doc) = self.ws.docs.get_mut(&id)
                    && button == MouseButton::Left
                    && doc.body.contains(x, y)
                {
                    doc.press(x, y, shift);
                    if doc.has_capture() {
                        self.capture = Some(id);
                    }
                }
                self.window.request_redraw();
            }
            (Hit::SidebarRow(i), MouseButton::Left | MouseButton::Right)
                if self.sidebar.panel == Panel::Agents =>
            {
                if let Some(entry) = self.sidebar.agents.entry_at_row(i).cloned() {
                    self.sidebar.agents.selected = Some(entry.agent.pid);
                    if button == MouseButton::Right {
                        self.context = Some(ContextMenu::agent(
                            self.cursor.x as f32,
                            self.cursor.y as f32,
                            &entry,
                        ));
                    } else if let Some((pane, _)) = entry.pane {
                        self.go_to_pane(pane);
                    } else if let Some(cwd) = &entry.agent.cwd {
                        self.open_tab_in(config, cwd);
                    }
                    self.window.request_redraw();
                }
            }
            (Hit::MonitorSort(sort), MouseButton::Left) => {
                self.sidebar.monitor.sort = sort;
                self.window.request_redraw();
            }
            (Hit::Process(pid), MouseButton::Left | MouseButton::Right) => {
                self.sidebar.monitor.selected = Some(pid);
                if button == MouseButton::Right {
                    let name = self
                        .sidebar
                        .monitor
                        .snap
                        .as_ref()
                        .and_then(|s| s.processes.as_ref())
                        .and_then(|ps| ps.iter().find(|p| p.pid == pid))
                        .map_or_else(|| "process".to_owned(), |p| p.name.clone());
                    self.context = Some(ContextMenu::process(
                        self.cursor.x as f32,
                        self.cursor.y as f32,
                        pid,
                        name,
                    ));
                }
                self.window.request_redraw();
            }
            (Hit::SidebarRow(i), MouseButton::Left | MouseButton::Right) => {
                let rows = self.sidebar.files.row_paths();
                if let Some((path, is_dir)) = rows.get(i).cloned() {
                    self.sidebar.files.focused = true;
                    self.sidebar.files.selected = Some(path.clone());
                    if button == MouseButton::Right {
                        self.context = Some(ContextMenu::new(
                            self.cursor.x as f32,
                            self.cursor.y as f32,
                            path,
                            is_dir,
                        ));
                    } else {
                        // Resolved on release: a click, or a drag onto a terminal.
                        self.tree_drag = Some(TreeDrag {
                            path,
                            is_dir,
                            start: self.cursor,
                            active: false,
                        });
                    }
                    self.window.request_redraw();
                }
            }
            (Hit::Sidebar, _) => {
                self.sidebar.files.focused = self.sidebar.panel == Panel::Files;
                self.window.request_redraw();
            }
            (Hit::Pane(id), _)
                if self
                    .ws
                    .panes
                    .get(&id)
                    .and_then(|p| p.input_drawn)
                    .is_some_and(|r| r.contains(self.cursor.x as f32, self.cursor.y as f32)) =>
            {
                self.sidebar.files.focused = false;
                if self.ws.focused_id() != Some(id) {
                    self.ws.focus(id);
                    self.update_title();
                }
                self.window.request_redraw();
            }
            (Hit::Pane(id), _) => {
                self.sidebar.files.focused = false;
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
        let hit = self.hit();
        if matches!(
            hit,
            Hit::SidebarRow(_) | Hit::Sidebar | Hit::Process(_) | Hit::MonitorSort(_)
        ) {
            let rows = match delta {
                winit::event::MouseScrollDelta::LineDelta(_, y) => -(y * 3.0) as isize,
                winit::event::MouseScrollDelta::PixelDelta(p) => {
                    -(p.y as f32 / self.metrics.row) as isize
                }
            };
            let visible = self.layout.visible_rows(&self.metrics);
            match self.sidebar.panel {
                Panel::Files => self.sidebar.files.scroll_by(rows, visible),
                Panel::Agents => self.sidebar.agents.scroll_by(rows, visible),
                Panel::Monitor => {
                    let max = (self.panel_hits.content_h - self.layout.sidebar_rows.h).max(0.0);
                    let m = &mut self.sidebar.monitor;
                    m.scroll = (m.scroll + rows as f32 * self.metrics.row).clamp(0.0, max);
                }
            }
            self.update_hover();
            self.window.request_redraw();
            return;
        }
        if let Hit::Pane(id) | Hit::DocButton(id, _) = hit
            && let Some(doc) = self.ws.docs.get_mut(&id)
        {
            let lines = match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => (x, y),
                winit::event::MouseScrollDelta::PixelDelta(p) => {
                    (p.x as f32 / cell.height, p.y as f32 / cell.height)
                }
            };
            if doc.image().is_some() {
                if lines.1 != 0.0 {
                    let factor = 1.15f32.powf(lines.1);
                    doc.zoom_by(factor);
                }
            } else if mods.shift || lines.0 != 0.0 {
                let dx = if lines.0 != 0.0 { -lines.0 } else { -lines.1 };
                doc.scroll_columns((dx * 6.0).round() as isize);
            } else {
                doc.scroll_by((-lines.1 * 3.0).round() as isize);
            }
            self.window.request_redraw();
            return;
        }
        if let Hit::Pane(id) = hit
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
            TerminalEvent::Shell(mao_terminal::ShellEvent::Open(path)) => {
                tracing::debug!(path = %path.display(), "mao open");
                // Opened by the caller, which has the config.
                self.pending_open = Some(path);
                self.window.request_redraw();
            }
            TerminalEvent::Shell(event) => {
                tracing::trace!(pane = pane_id, ?event, "shell event");
                pane.on_shell_event(event);
                self.update_title();
                self.window.request_redraw();
            }
        }
    }

    fn draw(&mut self, background: [u8; 3]) -> Result<FrameOutcome, mao_render::RenderError> {
        self.sync_sidebar();
        // The input box appears/disappears (integration seen, full-screen apps):
        // that changes the terminal's size.
        let bottom = self.input_bottom;
        if self
            .ws
            .panes
            .values()
            .any(|p| p.wants_box(bottom) != p.input_rect.is_some())
        {
            self.relayout(self.metrics.pad / self.scale);
        }
        self.visible.clear();
        self.inputs.clear();
        let cell = self.cell();
        let editor_enabled = self.editor_enabled;
        let input_colors = OverlayColors {
            bg: self.colors.raised,
            ..self.overlay_colors
        };
        if let Some(tab) = self.ws.active_tab() {
            let focused_pane = tab.focused;
            self.images.clear();
            for (id, _) in tab.layout(self.layout.content, self.metrics.gap) {
                if let Some(doc) = self.ws.docs.get_mut(&id) {
                    doc.upload_image(&mut self.renderer);
                    if let Some(draw) = doc.image_draw() {
                        self.images.push(draw);
                    } else {
                        doc.render(&mut self.renderer, &self.doc_colors, &mut self.row_buf);
                        self.visible.push(id);
                    }
                    continue;
                }
                self.visible.push(id);
                let Some(pane) = self.ws.panes.get_mut(&id) else {
                    continue;
                };
                let full = std::mem::take(&mut pane.full_redraw);
                if full {
                    self.renderer.invalidate_grid(id);
                }
                let focused = self.focused && id == focused_pane && self.palette.is_none();

                // Bottom mode: the editor lives in the input box; the live prompt
                // rows are hidden from the output above.
                if let Some(base) = pane.input_rect {
                    let editing = pane.editor_active(editor_enabled);
                    let (m, pad) = (&self.metrics, self.metrics.pad);
                    let grid_x = (base.x + pad).round();
                    let cols = (((base.w - 2.0 * pad) / cell.width).floor().max(4.0)) as usize;
                    let input_id = id | INPUT_GRID;
                    let mut hidden: Vec<usize> = Vec::new();
                    let mut rect = base;
                    let mut grid_rect = None;
                    // Rows of output (the rest of the screen is empty).
                    let mut used = pane.terminal.cursor_position().map_or(0, |(r, _)| r + 1);
                    if editing {
                        if let Some((row, _)) = pane.terminal.cursor_position() {
                            let start = pane.terminal.wrapped_start(row);
                            hidden = (start..=row).collect();
                            used = start;
                        }
                        let o = overlay::layout(
                            &pane.editor,
                            (0, 2),
                            cols,
                            MAX_INPUT_ROWS,
                            &input_colors,
                        );
                        let n = o.rows.len().max(1);
                        let extra = (n - 1) as f32 * cell.height;
                        rect = Rect::new(
                            base.x,
                            (base.y - extra).max(pane.rect.y),
                            base.w,
                            base.h + extra,
                        );
                        let gy = (rect.y + m.px(30.0)).round();
                        let g =
                            Rect::new(grid_x, gy, cols as f32 * cell.width, n as f32 * cell.height);
                        self.renderer.set_grid_layout(
                            input_id,
                            (g.x, g.y),
                            cols,
                            n,
                            [g.x, g.y, g.w, g.h],
                        );
                        let blank = mao_render::GridCell {
                            ch: ' ',
                            fg: input_colors.fg,
                            bg: input_colors.bg,
                            style: mao_render::CellStyle::default(),
                        };
                        for r in 0..n {
                            self.row_buf.clear();
                            self.row_buf.resize(cols, blank);
                            if r == 0 {
                                self.row_buf[0] = mao_render::GridCell {
                                    ch: '❯',
                                    fg: self.overlay_colors.cursor,
                                    ..blank
                                };
                            }
                            for (rr, c, cellv) in &o.cells {
                                if *rr == r
                                    && let Some(slot) = self.row_buf.get_mut(*c)
                                {
                                    *slot = *cellv;
                                }
                            }
                            self.renderer.set_row(input_id, r, self.row_buf.drain(..));
                        }
                        self.renderer.set_cursor(
                            input_id,
                            Some(mao_render::Cursor {
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
                        );
                        self.visible.push(input_id);
                        let (_, mc) = o.menu_anchor;
                        pane.menu_anchor = Some((
                            g.x + mc as f32 * cell.width,
                            self.layout.content.bottom() + 10_000.0,
                            rect.y,
                        ));
                        grid_rect = Some(g);
                    } else {
                        pane.menu_anchor = None;
                    }
                    // Warp-style: short output sits right above the input box.
                    let size = pane.terminal.size();
                    let shift = usize::from(size.rows).saturating_sub(used) as f32 * cell.height;
                    pane.origin = (pane.base_origin.0, pane.base_origin.1 + shift);
                    // Keep the output's text out of a grown box.
                    self.renderer.set_grid_layout(
                        id,
                        pane.origin,
                        usize::from(size.cols),
                        usize::from(size.rows),
                        [pane.rect.x, pane.rect.y, pane.rect.w, rect.y - pane.rect.y],
                    );
                    pane.input_drawn = Some(rect);
                    let cwd = pane
                        .cwd
                        .as_ref()
                        .map_or_else(String::new, |p| short_path(p));
                    let branch = pane.git_branch().map(str::to_owned);
                    self.inputs.push(InputView {
                        rect,
                        grid: grid_rect,
                        cwd,
                        branch,
                        exit: pane.last_exit,
                        running: pane.running.clone().filter(|_| !editing),
                        focused,
                    });

                    let stale = std::mem::replace(&mut pane.overlay_rows, hidden);
                    pane.terminal.invalidate_rows(
                        stale.into_iter().chain(pane.overlay_rows.iter().copied()),
                    );
                    let renderer = &mut self.renderer;
                    let row_buf = &mut self.row_buf;
                    let hidden = &pane.overlay_rows;
                    let bg = self.overlay_colors.bg;
                    let info = pane.terminal.render(
                        RenderOptions {
                            focused,
                            force_full: full,
                            hide_cursor: editing,
                        },
                        |row, cells| {
                            if hidden.contains(&row) {
                                row_buf.clear();
                                row_buf.extend(cells.iter().map(|_| mao_render::GridCell {
                                    ch: ' ',
                                    fg: bg,
                                    bg,
                                    style: mao_render::CellStyle::default(),
                                }));
                                renderer.set_row(id, row, row_buf.drain(..));
                            } else {
                                renderer.set_row(id, row, cells.iter().map(to_grid_cell));
                            }
                        },
                    );
                    renderer.set_cursor(
                        id,
                        if editing {
                            None
                        } else {
                            info.cursor.map(to_cursor)
                        },
                    );
                    continue;
                }
                pane.input_drawn = None;

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
        let drag_label = self.tree_drag.as_ref().filter(|d| d.active).map(|d| {
            d.path.file_name().map_or_else(
                || d.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
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
        self.panel_hits = chrome::draw(
            &mut self.ui,
            &DrawState {
                inputs: &self.inputs,
                maximized: self.custom_frame && self.window.is_maximized(),
                layout: &self.layout,
                metrics: &self.metrics,
                colors: &self.colors,
                workspace: &self.ws,
                palette: self.palette.as_ref(),
                hover: &self.hover,
                window_focused: self.focused,
                menu,
                sidebar: &self.sidebar,
                icons: self.renderer.has_icon_font(),
                context: self.context.as_ref(),
                drag: drag_label
                    .as_deref()
                    .map(|l| (l, self.cursor.x as f32, self.cursor.y as f32)),
            },
        );
        self.renderer
            .render(background, &self.visible, &self.images, &self.ui)
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        match self.create_window(event_loop) {
            Ok(mut win) => {
                // Testing aid: open this file in a document pane at startup.
                if let Ok(path) = std::env::var("MAO_DEBUG_OPEN") {
                    win.open_document(&self.config, std::path::Path::new(&path), true);
                }
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
            } => {
                w.on_terminal_event(pane, session, event);
                if let Some(path) = w.pending_open.take() {
                    w.open_document(&self.config, &path, false);
                }
            }
            UserEvent::Fs(event) => {
                w.sidebar.files.on_event(event);
                w.window.request_redraw();
            }
            UserEvent::Doc(doc) => w.on_document(doc),
            UserEvent::Sys(snap) => w.on_sample(snap),
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
        self.handle_theme_request();
    }
}

impl App {
    /// Applies a theme the window asked for (preview or keep).
    fn handle_theme_request(&mut self) {
        let Some(w) = self.win.as_mut() else {
            return;
        };
        let Some((name, keep)) = w.theme_request.take() else {
            return;
        };
        let Some(theme) = mao_config::themes::find(&name) else {
            tracing::warn!(name, "theme not found");
            return;
        };
        if self.config.palette != theme_with_overrides(&self.config, &theme) {
            self.config.set_theme(&theme);
            w.apply_theme(&self.config);
        } else {
            self.config.set_theme(&theme);
        }
        if keep {
            match mao_config::config_file_path() {
                Some(path) => match mao_config::themes::save_choice(&path, &theme.name) {
                    Ok(()) => tracing::info!(theme = theme.name, "theme saved"),
                    Err(e) => tracing::error!("cannot save the theme choice: {e}"),
                },
                None => tracing::warn!("no config directory; theme not saved"),
            }
        }
    }
}

/// The colors `theme` would give with the config's overrides.
fn theme_with_overrides(
    config: &Config,
    theme: &mao_config::themes::Theme,
) -> mao_config::ThemeConfig {
    let mut colors = theme.colors.clone();
    config.colors.apply(&mut colors);
    colors
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
    let palette = term_palette(config);
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

/// A folder for display, with the home folder as `~`.
fn short_path(p: &std::path::Path) -> String {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if let Some(home) = home
        && let Ok(rest) = p.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".to_owned();
        }
        let sep = std::path::MAIN_SEPARATOR;
        return format!("~{sep}{}", rest.display());
    }
    p.display().to_string()
}

/// Built-in agent detectors plus the user's (same name = replaced).
fn detectors(config: &Config) -> Vec<mao_agents::Detector> {
    let mut list = mao_agents::default_detectors();
    for d in &config.agents.detectors {
        let detector = mao_agents::Detector {
            name: d.name.clone(),
            exe: d.exe.clone(),
            cmdline: d.cmdline.clone(),
            exclude: d.exclude.clone(),
            sessions_dir: d.sessions_dir.clone(),
        };
        match list
            .iter_mut()
            .find(|x| x.name.eq_ignore_ascii_case(&d.name))
        {
            Some(slot) => *slot = detector,
            None => list.push(detector),
        }
    }
    list
}

/// Terminal colors from the theme.
fn term_palette(config: &Config) -> TermPalette {
    let theme = &config.palette;
    TermPalette::new(
        theme.ansi.map(|c| c.to_array()),
        theme.foreground.to_array(),
        theme.background.to_array(),
        theme.cursor.to_array(),
        theme.selection.to_array(),
    )
}

/// Document colors from the terminal theme.
fn doc_theme(t: &mao_config::ThemeConfig) -> mao_docs::Theme {
    let (bg, fg) = (t.background.to_array(), t.foreground.to_array());
    let luma =
        |c: [u8; 3]| 0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]);
    mao_docs::Theme {
        fg,
        bg,
        muted: chrome::mix(fg, bg, 0.5),
        accent: t.cursor.to_array(),
        heading: t.ansi[12].to_array(),
        link: t.ansi[14].to_array(),
        code_fg: t.ansi[11].to_array(),
        code_bg: chrome::mix(bg, fg, 0.07),
        dark: luma(bg) < 128.0,
    }
}

fn doc_colors(t: &mao_config::ThemeConfig) -> DocColors {
    let (bg, fg) = (t.background.to_array(), t.foreground.to_array());
    let yellow = t.ansi[3].to_array();
    DocColors {
        fg,
        bg,
        muted: chrome::mix(fg, bg, 0.55),
        selection: t.selection.to_array(),
        matched: chrome::mix(bg, yellow, 0.35),
        current: chrome::mix(bg, yellow, 0.7),
    }
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
