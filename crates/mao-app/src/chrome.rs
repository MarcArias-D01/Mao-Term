//! Window chrome: activity bar, sidebar, tab bar, status bar, pane dividers,
//! context menu and the command palette.
//! Geometry (for hit-testing) and drawing (into a `UiFrame`).

use mao_config::ThemeConfig;
use mao_render::{Align, Rgb8, UiFrame, UiText};
use mao_ui::{Divider, PaneId, Rect};

use crate::docpane::{ButtonKind, DocButton, DocPane};
use crate::editor::{MENU_ROWS, Menu};
use crate::palette::{MAX_VISIBLE, Palette};
use crate::panels::{self, PanelHits};
use crate::sidebar::{Panel, Sidebar, SortBy};
use crate::workspace::Workspace;

/// Pixel sizes derived from the display scale.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub scale: f32,
    pub ui_text: f32,
    pub tab_bar: f32,
    pub status_bar: f32,
    /// Divider thickness between panes.
    pub gap: f32,
    /// Padding inside each pane around the grid.
    pub pad: f32,
    pub activity_bar: f32,
    /// Height of a sidebar tree row.
    pub row: f32,
    /// We draw the title bar: minimize/maximize/close live in the tab bar.
    pub window_buttons: bool,
    /// Width of the invisible resize border (0 when maximized / system frame).
    pub resize_border: f32,
}

impl Metrics {
    pub fn new(scale: f32, padding: f32) -> Self {
        let px = |v: f32| (v * scale).round();
        Self {
            scale,
            ui_text: 13.0 * scale,
            tab_bar: px(36.0),
            status_bar: px(24.0),
            gap: px(1.0).max(1.0),
            pad: px(padding),
            activity_bar: px(46.0),
            row: px(24.0),
            window_buttons: false,
            resize_border: 0.0,
        }
    }

    pub fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }
}

/// Window buttons of the custom title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// Buttons of the activity bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Panel(Panel),
    Settings,
}

/// Where things are in the window.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub activity: Rect,
    pub activity_buttons: Vec<(Activity, Rect)>,
    /// The sidebar, when open.
    pub sidebar: Option<Rect>,
    /// Part of the sidebar holding tree rows.
    pub sidebar_rows: Rect,
    pub tab_bar: Rect,
    pub content: Rect,
    pub status: Rect,
    pub tabs: Vec<Rect>,
    pub closes: Vec<Rect>,
    pub plus: Rect,
    /// Minimize/maximize/close (custom title bar).
    pub window_buttons: Vec<(WindowButton, Rect)>,
}

/// Height of the sidebar header (panel title + folder name).
fn sidebar_header(m: &Metrics) -> f32 {
    m.px(62.0)
}

impl Layout {
    /// `sidebar_width` in physical pixels, `None` when closed.
    pub fn new(
        window: (u32, u32),
        m: &Metrics,
        tab_count: usize,
        sidebar_width: Option<f32>,
    ) -> Self {
        let full = Rect::new(0.0, 0.0, window.0 as f32, window.1 as f32);
        let (body, status) = full.split_bottom(m.status_bar);

        let activity = Rect::new(body.x, body.y, m.activity_bar.min(body.w), body.h);
        let b = m.activity_bar;
        let mut activity_buttons: Vec<(Activity, Rect)> =
            [Panel::Files, Panel::Agents, Panel::Monitor]
                .into_iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        Activity::Panel(p),
                        Rect::new(activity.x, activity.y + m.px(4.0) + i as f32 * b, b, b),
                    )
                })
                .collect();
        activity_buttons.push((
            Activity::Settings,
            Rect::new(activity.x, activity.bottom() - b - m.px(4.0), b, b),
        ));

        let mut rest = Rect::new(
            activity.right(),
            body.y,
            (body.w - activity.w).max(0.0),
            body.h,
        );
        let sidebar = sidebar_width.map(|w| {
            let w = w.min((rest.w - m.px(200.0)).max(0.0)).round();
            let r = Rect::new(rest.x, rest.y, w, rest.h);
            rest = Rect::new(r.right(), rest.y, (rest.w - w).max(0.0), rest.h);
            r
        });
        let sidebar_rows = sidebar.map_or_else(Rect::default, |r| {
            let (_, rows) = r.split_top(sidebar_header(m));
            rows
        });

        let (tab_bar, content) = rest.split_top(m.tab_bar);
        let plus_w = m.tab_bar;
        let mut window_buttons = Vec::new();
        let mut buttons_w = 0.0;
        if m.window_buttons {
            let w = m.px(46.0);
            for (i, b) in [
                WindowButton::Minimize,
                WindowButton::Maximize,
                WindowButton::Close,
            ]
            .into_iter()
            .enumerate()
            {
                let x = tab_bar.right() - (3 - i) as f32 * w;
                window_buttons.push((b, Rect::new(x, tab_bar.y, w, tab_bar.h)));
            }
            // Leave some empty bar to grab the window by.
            buttons_w = 3.0 * w + m.px(48.0);
        }
        let available = (tab_bar.w - plus_w - m.px(8.0) - buttons_w).max(0.0);
        let tab_w = if tab_count == 0 {
            0.0
        } else {
            // Shrink as needed so tabs never overflow the bar.
            (available / tab_count as f32).min(m.px(220.0)).floor()
        };
        let close_w = m.px(28.0);
        let mut tabs = Vec::with_capacity(tab_count);
        let mut closes = Vec::with_capacity(tab_count);
        for i in 0..tab_count {
            let r = Rect::new(tab_bar.x + i as f32 * tab_w, tab_bar.y, tab_w, tab_bar.h);
            closes.push(Rect::new(r.right() - close_w, r.y, close_w, r.h));
            tabs.push(r);
        }
        let plus = Rect::new(
            tab_bar.x + tab_count as f32 * tab_w,
            tab_bar.y,
            plus_w,
            tab_bar.h,
        );
        Self {
            activity,
            activity_buttons,
            sidebar,
            sidebar_rows,
            tab_bar,
            content,
            status,
            tabs,
            closes,
            plus,
            window_buttons,
        }
    }

    /// Tree rows that fit in the sidebar.
    pub fn visible_rows(&self, m: &Metrics) -> usize {
        (self.sidebar_rows.h / m.row).floor().max(0.0) as usize
    }
}

/// Something to do with a file or folder from the context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// In the document viewer.
    Open,
    OpenExternally,
    Cd,
    OpenInNewTab,
    InsertPath,
    CopyPath,
    CopyRelativePath,
    Reveal,
    /// Agents and processes.
    GoToPane,
    OpenSessions,
    CopyPid,
    KillProcess,
    ConfirmKill,
    Cancel,
}

impl FileAction {
    fn label(self) -> &'static str {
        match self {
            Self::GoToPane => "Go to Pane",
            Self::OpenSessions => "Open Sessions Folder",
            Self::CopyPid => "Copy PID",
            Self::KillProcess => "Kill Process…",
            Self::ConfirmKill => "Kill",
            Self::Cancel => "Cancel",
            Self::Open => "Open",
            Self::OpenExternally => "Open with Default App",
            Self::Cd => "cd Here",
            Self::OpenInNewTab => "Open in New Tab",
            Self::InsertPath => "Insert Path in Terminal",
            Self::CopyPath => "Copy Path",
            Self::CopyRelativePath => "Copy Relative Path",
            Self::Reveal => {
                if cfg!(windows) {
                    "Reveal in File Explorer"
                } else {
                    "Open Containing Folder"
                }
            }
        }
    }
}

/// Right-click menu on a tree entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    pub target: std::path::PathBuf,
    pub items: Vec<FileAction>,
    /// Process the menu is about (agents and monitor), with its name.
    pub process: Option<(u32, String)>,
    pub sessions: Option<std::path::PathBuf>,
    pub pane: Option<PaneId>,
}

impl ContextMenu {
    /// Menu of an agent session.
    pub fn agent(x: f32, y: f32, e: &crate::sidebar::AgentEntry) -> Self {
        use FileAction::*;
        let a = &e.agent;
        let mut items = Vec::new();
        if e.pane.is_some() {
            items.push(GoToPane);
        }
        if a.cwd.is_some() {
            items.extend([OpenInNewTab, Reveal, CopyPath]);
        }
        if a.sessions_dir.is_some() {
            items.push(OpenSessions);
        }
        items.extend([CopyPid, KillProcess]);
        Self {
            x,
            y,
            target: a.cwd.clone().unwrap_or_default(),
            items,
            process: Some((a.pid, a.name.clone())),
            sessions: a.sessions_dir.clone(),
            pane: e.pane.map(|(p, _)| p),
        }
    }

    /// Menu of a process in the monitor.
    pub fn process(x: f32, y: f32, pid: u32, name: String) -> Self {
        Self {
            x,
            y,
            target: std::path::PathBuf::new(),
            items: vec![FileAction::CopyPid, FileAction::KillProcess],
            process: Some((pid, name)),
            sessions: None,
            pane: None,
        }
    }

    /// The same menu turned into a kill confirmation.
    pub fn confirm_kill(&self) -> Self {
        Self {
            items: vec![FileAction::ConfirmKill, FileAction::Cancel],
            ..self.clone()
        }
    }

    /// Label of an item (some depend on the target).
    pub fn label(&self, action: FileAction) -> String {
        match (action, &self.process) {
            (FileAction::ConfirmKill, Some((pid, name))) => format!("Kill {name} (pid {pid})"),
            _ => action.label().to_owned(),
        }
    }

    pub fn new(x: f32, y: f32, target: std::path::PathBuf, is_dir: bool) -> Self {
        use FileAction::*;
        let items = if is_dir {
            vec![
                Cd,
                OpenInNewTab,
                InsertPath,
                CopyPath,
                CopyRelativePath,
                Reveal,
            ]
        } else {
            vec![
                Open,
                OpenExternally,
                InsertPath,
                CopyPath,
                CopyRelativePath,
                Reveal,
            ]
        };
        Self {
            x,
            y,
            target,
            items,
            process: None,
            sessions: None,
            pane: None,
        }
    }

    fn geometry(&self, l: &Layout, m: &Metrics) -> (Rect, Vec<Rect>) {
        let row_h = m.px(28.0);
        let pad = m.px(4.0);
        let w = m.px(230.0);
        let h = self.items.len() as f32 * row_h + 2.0 * pad;
        let win_w = l.status.right();
        let win_h = l.status.y;
        let x = self.x.min(win_w - w - m.px(4.0)).max(0.0);
        let y = if self.y + h > win_h {
            (self.y - h).max(0.0)
        } else {
            self.y
        };
        let rows = (0..self.items.len())
            .map(|i| Rect::new(x + pad, y + pad + i as f32 * row_h, w - 2.0 * pad, row_h))
            .collect();
        (Rect::new(x, y, w, h), rows)
    }
}

/// What is under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum Hit {
    None,
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Divider(Divider),
    Pane(PaneId),
    PaletteRow(usize),
    /// Inside the palette box but not on a row.
    Palette,
    Activity(Activity),
    /// Tree row, as an index into the visible rows (scroll applied).
    SidebarRow(usize),
    /// Elsewhere in the sidebar.
    Sidebar,
    /// The sidebar's right edge (drag to resize).
    SidebarEdge,
    ContextItem(usize),
    /// Inside the context menu but not on an item.
    Context,
    /// A button in a document pane's header.
    DocButton(PaneId, DocButton),
    /// Custom title bar: a window button, the empty bar (drag), a resize edge.
    WindowButton(WindowButton),
    TitleBar,
    Resize(winit::window::ResizeDirection),
    /// A column header of the monitor's process list.
    MonitorSort(SortBy),
    /// A process row in the monitor.
    Process(u32),
}

/// What hit-testing needs to know about the window.
pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub metrics: &'a Metrics,
    pub workspace: &'a Workspace,
    pub palette: Option<&'a Palette>,
    pub context: Option<&'a ContextMenu>,
    pub sidebar: &'a Sidebar,
    /// Panel geometry from the last frame.
    pub panel_hits: &'a PanelHits,
}

pub fn hit_test(x: f32, y: f32, scene: &Scene<'_>) -> Hit {
    let Scene {
        layout,
        metrics: m,
        workspace: ws,
        palette,
        context,
        sidebar,
        panel_hits,
    } = *scene;
    // Invisible resize border around an undecorated window.
    let b = m.resize_border;
    if b > 0.0 {
        use winit::window::ResizeDirection as D;
        let (w, h) = (layout.status.right(), layout.status.bottom());
        let (left, right, top, bottom) = (x < b, x >= w - b, y < b, y >= h - b);
        let corner = 2.0 * b;
        let dir = match (
            left || x < corner && (top || bottom),
            right || x >= w - corner && (top || bottom),
            top || y < corner && (left || right),
            bottom || y >= h - corner && (left || right),
        ) {
            (true, _, true, _) => Some(D::NorthWest),
            (_, true, true, _) => Some(D::NorthEast),
            (true, _, _, true) => Some(D::SouthWest),
            (_, true, _, true) => Some(D::SouthEast),
            (true, ..) => Some(D::West),
            (_, true, ..) => Some(D::East),
            (_, _, true, _) => Some(D::North),
            (.., true) => Some(D::South),
            _ => None,
        };
        if let Some(d) = dir {
            return Hit::Resize(d);
        }
    }
    if let Some(menu) = context {
        let (frame, rows) = menu.geometry(layout, m);
        if frame.contains(x, y) {
            return rows
                .iter()
                .position(|r| r.contains(x, y))
                .map_or(Hit::Context, Hit::ContextItem);
        }
    }
    if let Some(p) = palette {
        let geo = PaletteGeometry::new(layout, m, p);
        if geo.frame.contains(x, y) {
            return geo
                .rows
                .iter()
                .position(|r| r.contains(x, y))
                .map_or(Hit::Palette, |r| Hit::PaletteRow(p.scroll + r));
        }
    }
    if layout.activity.contains(x, y) {
        return layout
            .activity_buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map_or(Hit::None, |(a, _)| Hit::Activity(*a));
    }
    if let Some(sb) = layout.sidebar {
        let grab = m.px(4.0);
        if (x - sb.right()).abs() <= grab && y >= sb.y && y < sb.bottom() {
            return Hit::SidebarEdge;
        }
        if sb.contains(x, y) {
            if layout.sidebar_rows.contains(x, y) {
                let row = ((y - layout.sidebar_rows.y) / m.row) as usize;
                match sidebar.panel {
                    Panel::Files => {
                        let i = row + sidebar.files.scroll;
                        if i < sidebar.files.tree.rows().len() {
                            return Hit::SidebarRow(i);
                        }
                    }
                    Panel::Agents => {
                        let i = row + sidebar.agents.scroll;
                        if sidebar.agents.entry_at_row(i).is_some() {
                            return Hit::SidebarRow(i);
                        }
                    }
                    Panel::Monitor => {
                        if let Some(hit) = panel_hits.hit(x, y) {
                            return hit;
                        }
                    }
                }
            }
            return Hit::Sidebar;
        }
    }
    if layout.tab_bar.contains(x, y) {
        for (i, tab) in layout.tabs.iter().enumerate() {
            if tab.contains(x, y) {
                return if layout.closes[i].contains(x, y) {
                    Hit::CloseTab(i)
                } else {
                    Hit::Tab(i)
                };
            }
        }
        if let Some((b, _)) = layout.window_buttons.iter().find(|(_, r)| r.contains(x, y)) {
            return Hit::WindowButton(*b);
        }
        return if layout.plus.contains(x, y) {
            Hit::NewTab
        } else if m.window_buttons {
            Hit::TitleBar
        } else {
            Hit::None
        };
    }
    let Some(tab) = ws.active_tab() else {
        return Hit::None;
    };
    if tab.zoomed.is_none() {
        // Dividers are 1px; give them a comfortable grab zone.
        let grab = m.px(4.0);
        for d in tab.tree.dividers(layout.content, m.gap) {
            if d.rect.inset(-grab).contains(x, y) {
                return Hit::Divider(d);
            }
        }
    }
    let Some(id) = tab
        .layout(layout.content, m.gap)
        .into_iter()
        .find(|(_, r)| r.contains(x, y))
        .map(|(id, _)| id)
    else {
        return Hit::None;
    };
    if let Some(doc) = ws.docs.get(&id)
        && doc.header.contains(x, y)
    {
        return doc_buttons(doc, m)
            .into_iter()
            .find(|(_, r, ..)| r.contains(x, y))
            .map_or(Hit::Pane(id), |(b, ..)| Hit::DocButton(id, b));
    }
    Hit::Pane(id)
}

/// Height of a document pane's header bar.
pub fn doc_header(m: &Metrics) -> f32 {
    m.px(30.0)
}

fn doc_buttons(
    doc: &DocPane,
    m: &Metrics,
) -> Vec<(DocButton, Rect, &'static str, bool, ButtonKind)> {
    doc.buttons(|v| m.px(v), m.ui_text * 0.92)
}

/// Colors derived from the terminal theme.
pub struct Colors {
    pub(crate) bg: Rgb8,
    pub(crate) fg: Rgb8,
    pub(crate) dim: Rgb8,
    pub(crate) accent: Rgb8,
    pub(crate) selection: Rgb8,
    pub(crate) chrome: Rgb8,
    pub(crate) hover: Rgb8,
    pub(crate) divider: Rgb8,
    pub(crate) raised: Rgb8,
    pub(crate) branch: Rgb8,
    pub(crate) option: Rgb8,
    pub(crate) warn: Rgb8,
    pub(crate) error: Rgb8,
}

impl Colors {
    pub fn new(theme: &ThemeConfig) -> Self {
        let bg = theme.background.to_array();
        let fg = theme.foreground.to_array();
        let chrome = mix(bg, [0, 0, 0], 0.28);
        Self {
            bg,
            fg,
            dim: mix(fg, bg, 0.45),
            accent: theme.cursor.to_array(),
            selection: theme.selection.to_array(),
            chrome,
            hover: mix(chrome, fg, 0.07),
            divider: mix(bg, fg, 0.16),
            raised: mix(bg, fg, 0.06),
            branch: theme.ansi[10].to_array(),
            option: theme.ansi[14].to_array(),
            warn: theme.ansi[11].to_array(),
            error: theme.ansi[9].to_array(),
        }
    }
}

pub fn mix(a: Rgb8, b: Rgb8, t: f32) -> Rgb8 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

// Text cache keys (stable across frames).
const KEY_TAB_TITLE: u64 = 1 << 48;
const KEY_TAB_CLOSE: u64 = 2 << 48;
const KEY_PLUS: u64 = 3 << 48;
const KEY_STATUS: u64 = 4 << 48;
const KEY_PALETTE: u64 = 5 << 48;
const KEY_PALETTE_ROW: u64 = 6 << 48;
const KEY_PALETTE_SHORTCUT: u64 = 7 << 48;
const KEY_MENU_LABEL: u64 = 8 << 48;
const KEY_MENU_DETAIL: u64 = 9 << 48;
const KEY_MENU_COUNT: u64 = 10 << 48;
const KEY_ACTIVITY: u64 = 11 << 48;
const KEY_SIDEBAR: u64 = 12 << 48;
const KEY_TREE_NAME: u64 = 13 << 48;
const KEY_TREE_CHEVRON: u64 = 14 << 48;
const KEY_TREE_ICON: u64 = 15 << 48;
const KEY_CONTEXT: u64 = 16 << 48;
const KEY_DRAG: u64 = 17 << 48;
const KEY_DOC: u64 = 18 << 48;
const KEY_WINDOW_BUTTON: u64 = 23 << 48;
pub(crate) const KEY_AGENTS: u64 = 19 << 48;
pub(crate) const KEY_MONITOR: u64 = 20 << 48;
const KEY_BADGE: u64 = 21 << 48;

/// Icon glyphs: Segoe Fluent/MDL2 code points, or plain fallbacks.
struct Icons {
    files: &'static str,
    agents: &'static str,
    monitor: &'static str,
    settings: &'static str,
    chevron_right: &'static str,
    chevron_down: &'static str,
    folder: &'static str,
    file: &'static str,
}

const FONT_ICONS: Icons = Icons {
    files: "\u{EC50}",
    agents: "\u{E99A}",
    monitor: "\u{E9D9}",
    settings: "\u{E713}",
    chevron_right: "\u{E76C}",
    chevron_down: "\u{E70D}",
    folder: "\u{E8B7}",
    file: "\u{E8A5}",
};

const TEXT_ICONS: Icons = Icons {
    files: "≡",
    agents: "◎",
    monitor: "◔",
    settings: "⚙",
    chevron_right: "▸",
    chevron_down: "▾",
    folder: "",
    file: "",
};

/// The completion menu of the focused pane and where to put it.
pub struct MenuView<'a> {
    pub menu: &'a Menu,
    /// Left edge (the completed token's first cell).
    pub x: f32,
    /// Top of the row below the caret.
    pub below: f32,
    /// Top of the caret row (the menu goes above it if there's no room below).
    pub above: f32,
}

/// The bottom input box of a terminal pane.
pub struct InputView {
    /// The whole box (grown upward for long lines).
    pub rect: Rect,
    /// Where the editor grid is drawn (`None` while a command runs).
    pub grid: Option<Rect>,
    pub cwd: String,
    pub branch: Option<String>,
    pub exit: Option<i32>,
    pub running: Option<String>,
    pub focused: bool,
}

pub struct DrawState<'a> {
    pub inputs: &'a [InputView],
    /// The window is maximized (custom title bar shows "restore").
    pub maximized: bool,
    pub sidebar: &'a Sidebar,
    /// An icon font is available.
    pub icons: bool,
    pub context: Option<&'a ContextMenu>,
    /// Entry being dragged from the tree, and the pointer position.
    pub drag: Option<(&'a str, f32, f32)>,
    pub layout: &'a Layout,
    pub metrics: &'a Metrics,
    pub colors: &'a Colors,
    pub workspace: &'a Workspace,
    pub palette: Option<&'a Palette>,
    pub hover: &'a Hit,
    pub window_focused: bool,
    pub menu: Option<MenuView<'a>>,
}

/// Draws the chrome; returns the panels' clickable geometry.
pub fn draw(ui: &mut UiFrame, s: &DrawState<'_>) -> PanelHits {
    ui.clear();
    draw_activity_bar(ui, s);
    let mut hits = PanelHits::default();
    if let Some(r) = s.layout.sidebar {
        hits = draw_sidebar(ui, s, r);
    }
    draw_tab_bar(ui, s);
    draw_docs(ui, s);
    draw_inputs(ui, s);
    draw_panes_decor(ui, s);
    draw_status(ui, s);
    // Floating things go on the top layer so they cover the text below them.
    let top = ui.top();
    if let Some(menu) = &s.menu {
        draw_menu(top, s, menu);
    }
    if let Some(menu) = s.context {
        draw_context(top, s, menu);
    }
    if let Some((label, x, y)) = s.drag {
        draw_drag(top, s, label, x, y);
    }
    if let Some(p) = s.palette {
        draw_palette(top, s, p);
    }
    hits
}

pub(crate) fn icon(key: u64, glyph: &str, r: Rect, size: f32, color: Rgb8) -> UiText {
    let mut t = text(key, glyph, r, size, color, Align::Center);
    t.icon = true;
    t
}

fn draw_activity_bar(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let a = l.activity;
    ui.rect(a.x, a.y, a.w, a.h, mix(c.chrome, [0, 0, 0], 0.18));
    let icons = if s.icons { &FONT_ICONS } else { &TEXT_ICONS };
    for (i, (act, r)) in l.activity_buttons.iter().enumerate() {
        let active =
            matches!(act, Activity::Panel(p) if s.sidebar.visible && s.sidebar.panel == *p);
        let hovered = *s.hover == Hit::Activity(*act);
        if active {
            ui.rect(r.x, r.y + m.px(8.0), m.px(2.0), r.h - m.px(16.0), c.accent);
        }
        let glyph = match act {
            Activity::Panel(Panel::Files) => icons.files,
            Activity::Panel(Panel::Agents) => icons.agents,
            Activity::Panel(Panel::Monitor) => icons.monitor,
            Activity::Settings => icons.settings,
        };
        let color = if active || hovered { c.fg } else { c.dim };
        ui.texts
            .push(icon(KEY_ACTIVITY | i as u64, glyph, *r, m.px(20.0), color));
        // Number of agents running (known while the agents/monitor panel samples).
        let agents = s.sidebar.agents.entries.len();
        if *act == Activity::Panel(Panel::Agents) && agents > 0 && s.sidebar.interest().processes {
            let d = m.px(14.0);
            let b = Rect::new(r.right() - d - m.px(4.0), r.y + m.px(5.0), d, d);
            ui.rect(b.x, b.y, b.w, b.h, c.accent);
            ui.texts.push(text(
                KEY_BADGE,
                agents.min(99).to_string(),
                b,
                m.ui_text * 0.72,
                c.bg,
                Align::Center,
            ));
        }
    }
}

fn draw_sidebar(ui: &mut UiFrame, s: &DrawState<'_>, r: Rect) -> PanelHits {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    ui.rect(r.x, r.y, r.w, r.h, c.chrome);
    let edge = if *s.hover == Hit::SidebarEdge {
        c.accent
    } else {
        c.divider
    };
    ui.rect(r.right() - m.gap, r.y, m.gap, r.h, edge);

    let pad = m.px(14.0);
    let title = match s.sidebar.panel {
        Panel::Files => "EXPLORER",
        Panel::Agents => "AGENTS",
        Panel::Monitor => "SYSTEM MONITOR",
    };
    let mut t = text(
        KEY_SIDEBAR,
        title,
        Rect::new(r.x + pad, r.y + m.px(4.0), r.w - 2.0 * pad, m.px(28.0)),
        m.ui_text * 0.85,
        c.dim,
        Align::Left,
    );
    t.bold = true;
    ui.texts.push(t);

    let subtitle = |ui: &mut UiFrame, label: String| {
        let mut t = text(
            KEY_SIDEBAR | 2,
            label,
            Rect::new(r.x + pad, r.y + m.px(32.0), r.w - 2.0 * pad, m.px(26.0)),
            m.ui_text,
            c.fg,
            Align::Left,
        );
        t.bold = true;
        ui.texts.push(t);
    };
    let _ = l;
    match s.sidebar.panel {
        Panel::Files => draw_files(ui, s, r),
        Panel::Agents => {
            subtitle(ui, panels::agents_summary(s));
            panels::draw_agents(ui, s);
        }
        Panel::Monitor => {
            let host = s
                .sidebar
                .monitor
                .snap
                .as_ref()
                .map_or_else(String::new, |snap| snap.host.clone());
            subtitle(ui, host);
            return panels::draw_monitor(ui, s);
        }
    }
    PanelHits::default()
}

fn draw_files(ui: &mut UiFrame, s: &DrawState<'_>, r: Rect) {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let files = &s.sidebar.files;
    let icons = if s.icons { &FONT_ICONS } else { &TEXT_ICONS };
    let pad = m.px(14.0);

    // Folder name line.
    let root_name = files.tree.root().map_or_else(String::new, |p| {
        p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    });
    let suffix = if files.show_ignored {
        "  (all files)"
    } else {
        ""
    };
    let mut t = text(
        KEY_SIDEBAR | 2,
        format!("{root_name}{suffix}"),
        Rect::new(r.x + pad, r.y + m.px(32.0), r.w - 2.0 * pad, m.px(26.0)),
        m.ui_text,
        c.fg,
        Align::Left,
    );
    t.bold = true;
    ui.texts.push(t);

    let area = l.sidebar_rows;
    let rows = files.tree.rows();
    if rows.is_empty() {
        let msg = match files.tree.root_error() {
            Some(e) => format!("Cannot read folder: {e}"),
            None if files.tree.root().is_none() => {
                "No folder (the shell hasn't reported one yet)".to_owned()
            }
            None => "Empty or loading…".to_owned(),
        };
        ui.texts.push(text(
            KEY_SIDEBAR | 3,
            msg,
            Rect::new(area.x + pad, area.y, area.w - 2.0 * pad, m.row),
            m.ui_text * 0.95,
            c.dim,
            Align::Left,
        ));
        return;
    }

    let visible = l.visible_rows(m);
    for (slot, row) in rows.iter().enumerate().skip(files.scroll).take(visible) {
        let y = area.y + (slot - files.scroll) as f32 * m.row;
        let rr = Rect::new(area.x, y, area.w - m.gap, m.row);
        let selected = files.selected.as_deref() == Some(row.entry.path.as_path());
        let hovered = *s.hover == Hit::SidebarRow(slot);
        if selected {
            ui.rect(
                rr.x,
                rr.y,
                rr.w,
                rr.h,
                if files.focused { c.selection } else { c.hover },
            );
        } else if hovered {
            ui.rect(rr.x, rr.y, rr.w, rr.h, c.hover);
        }
        let k = (slot - files.scroll) as u64;
        let mut x = rr.x + m.px(8.0) + row.depth as f32 * m.px(14.0);
        let chevron_w = m.px(16.0);
        if row.entry.is_dir {
            let glyph = if row.expanded {
                icons.chevron_down
            } else {
                icons.chevron_right
            };
            ui.texts.push(icon(
                KEY_TREE_CHEVRON | k,
                glyph,
                Rect::new(x, rr.y, chevron_w, rr.h),
                m.px(10.0),
                c.dim,
            ));
        }
        x += chevron_w;
        let glyph = if row.entry.is_dir {
            icons.folder
        } else {
            icons.file
        };
        if !glyph.is_empty() {
            let color = if row.entry.is_dir { c.accent } else { c.dim };
            ui.texts.push(icon(
                KEY_TREE_ICON | k,
                glyph,
                Rect::new(x, rr.y, m.px(18.0), rr.h),
                m.px(13.0),
                color,
            ));
            x += m.px(22.0);
        }
        let name = if row.loading {
            format!("{} …", row.entry.name)
        } else {
            row.entry.name.clone()
        };
        ui.texts.push(text(
            KEY_TREE_NAME | k,
            name,
            Rect::new(x, rr.y, (rr.right() - x - m.px(6.0)).max(0.0), rr.h),
            m.ui_text,
            c.fg,
            Align::Left,
        ));
    }

    // Scrollbar.
    if rows.len() > visible && visible > 0 {
        let track = area.h;
        let thumb = (track * visible as f32 / rows.len() as f32).max(m.px(20.0));
        let y = area.y + (track - thumb) * files.scroll as f32 / (rows.len() - visible) as f32;
        ui.rect(area.right() - m.px(6.0), y, m.px(4.0), thumb, c.divider);
    }
}

fn draw_context(ui: &mut UiFrame, s: &DrawState<'_>, menu: &ContextMenu) {
    let (m, c) = (s.metrics, s.colors);
    let (frame, rows) = menu.geometry(s.layout, m);
    let b = m.gap;
    ui.rect(
        frame.x - b,
        frame.y - b,
        frame.w + 2.0 * b,
        frame.h + 2.0 * b,
        c.divider,
    );
    ui.rect(frame.x, frame.y, frame.w, frame.h, c.raised);
    for (i, (row, action)) in rows.iter().zip(&menu.items).enumerate() {
        if *s.hover == Hit::ContextItem(i) {
            ui.rect(row.x, row.y, row.w, row.h, c.selection);
        }
        let inner = Rect::new(row.x + m.px(10.0), row.y, row.w - m.px(20.0), row.h);
        ui.texts.push(text(
            KEY_CONTEXT | i as u64,
            menu.label(*action),
            inner,
            m.ui_text,
            c.fg,
            Align::Left,
        ));
    }
}

fn draw_drag(ui: &mut UiFrame, s: &DrawState<'_>, label: &str, x: f32, y: f32) {
    let (m, c) = (s.metrics, s.colors);
    let w = m.px(220.0);
    let h = m.px(26.0);
    let (x, y) = (x + m.px(12.0), y + m.px(8.0));
    ui.rect(x, y, w, h, c.selection);
    ui.texts.push(text(
        KEY_DRAG,
        label,
        Rect::new(x + m.px(8.0), y, w - m.px(16.0), h),
        m.ui_text,
        c.fg,
        Align::Left,
    ));
}

pub(crate) fn text(
    key: u64,
    text: impl Into<String>,
    r: Rect,
    size: f32,
    color: Rgb8,
    align: Align,
) -> UiText {
    UiText {
        key,
        text: text.into(),
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        size,
        color,
        bold: false,
        mono: false,
        icon: false,
        align,
    }
}

fn draw_tab_bar(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c, ws) = (s.layout, s.metrics, s.colors, s.workspace);
    let bar = l.tab_bar;
    ui.rect(bar.x, bar.y, bar.w, bar.h, c.chrome);

    for (i, tab) in ws.tabs.iter().enumerate() {
        let r = l.tabs[i];
        let active = i == ws.active;
        let hovered = matches!(s.hover, Hit::Tab(h) | Hit::CloseTab(h) if *h == i);
        if active {
            ui.rect(r.x, r.y, r.w, r.h, c.bg);
            ui.rect(r.x, r.y, r.w, m.px(2.0), c.accent);
        } else if hovered {
            ui.rect(r.x, r.y, r.w, r.h, c.hover);
        } else if i + 1 != ws.active && i + 1 < ws.tabs.len() {
            // Thin separator between two inactive tabs.
            let h = (r.h * 0.45).round();
            ui.rect(
                r.right() - m.gap,
                r.y + (r.h - h) / 2.0,
                m.gap,
                h,
                c.divider,
            );
        }

        let title = ws.pane_name(tab.focused);
        let pad = m.px(14.0);
        let mut text_x = r.x + pad;
        if tab.activity && !active {
            let d = m.px(6.0);
            ui.rect(r.x + pad, r.y + (r.h - d) / 2.0, d, d, c.accent);
            text_x += d + m.px(8.0);
        }
        let label = if tab.zoomed.is_some() {
            format!("{title} (max)")
        } else if tab.tree.len() > 1 {
            format!("{title} +{}", tab.tree.len() - 1)
        } else {
            title.clone()
        };
        let text_w = (l.closes[i].x - text_x).max(0.0);
        let color = if active { c.fg } else { c.dim };
        ui.texts.push(text(
            KEY_TAB_TITLE | tab.id,
            label,
            Rect::new(text_x, r.y, text_w, r.h),
            m.ui_text,
            color,
            Align::Left,
        ));

        if active || hovered {
            let close = l.closes[i];
            let close_hovered = *s.hover == Hit::CloseTab(i);
            if close_hovered {
                let d = m.px(20.0);
                ui.rect(
                    close.x + (close.w - d) / 2.0,
                    close.y + (close.h - d) / 2.0,
                    d,
                    d,
                    c.hover,
                );
            }
            ui.texts.push(text(
                KEY_TAB_CLOSE | i as u64,
                "×",
                close,
                m.ui_text * 1.2,
                if close_hovered { c.fg } else { c.dim },
                Align::Center,
            ));
        }
    }

    if *s.hover == Hit::NewTab {
        let p = l.plus;
        ui.rect(p.x, p.y, p.w, p.h, c.hover);
    }
    // Custom title bar buttons.
    for (i, (b, r)) in l.window_buttons.iter().enumerate() {
        let hovered = *s.hover == Hit::WindowButton(*b);
        let close = *b == WindowButton::Close;
        if hovered {
            ui.top().rect(
                r.x,
                r.y,
                r.w,
                r.h,
                if close { [0xc4, 0x2b, 0x1c] } else { c.hover },
            );
        }
        let glyph = match (b, s.icons, s.maximized) {
            (WindowButton::Minimize, true, _) => "\u{E921}",
            (WindowButton::Maximize, true, false) => "\u{E922}",
            (WindowButton::Maximize, true, true) => "\u{E923}",
            (WindowButton::Close, true, _) => "\u{E8BB}",
            (WindowButton::Minimize, false, _) => "–",
            (WindowButton::Maximize, false, _) => "□",
            (WindowButton::Close, false, _) => "×",
        };
        let color = if hovered && close {
            [0xff, 0xff, 0xff]
        } else if s.window_focused {
            c.fg
        } else {
            c.dim
        };
        let t = icon(KEY_WINDOW_BUTTON | i as u64, glyph, *r, m.px(10.0), color);
        // On the top layer so the hover fill (also on top) doesn't hide it.
        ui.top().texts.push(t);
    }
    ui.texts.push(text(
        KEY_PLUS,
        "+",
        l.plus,
        m.ui_text * 1.35,
        c.dim,
        Align::Center,
    ));
}

/// Document panes: header (name, info, buttons, search box) and messages.
fn draw_docs(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c, ws) = (s.layout, s.metrics, s.colors, s.workspace);
    let Some(tab) = ws.active_tab() else {
        return;
    };
    for (id, _) in tab.layout(l.content, m.gap) {
        let Some(doc) = ws.docs.get(&id) else {
            continue;
        };
        let key = |slot: u64| KEY_DOC | (id << 8) | slot;
        let h = doc.header;
        ui.rect(h.x, h.y, h.w, h.h, c.raised);
        ui.rect(h.x, h.bottom() - m.gap, h.w, m.gap, c.divider);
        let pad = m.px(10.0);
        let size = m.ui_text * 0.92;
        let buttons = doc_buttons(doc, m);
        let buttons_x = buttons.first().map_or(h.right() - pad, |(_, r, ..)| r.x);

        // File name, then either the info line or the search box.
        let name = doc.name();
        let name_w = (name.chars().count() as f32 * m.ui_text * 0.6 + pad).min(h.w * 0.45);
        let mut t = text(
            key(0),
            name,
            Rect::new(h.x + pad, h.y, name_w, h.h),
            m.ui_text,
            c.fg,
            Align::Left,
        );
        t.bold = true;
        ui.texts.push(t);
        let info_x = h.x + pad + name_w + m.px(8.0);
        let info_w = (buttons_x - info_x - m.px(8.0)).max(0.0);
        if let Some(search) = &doc.search {
            let r = Rect::new(info_x, h.y + m.px(4.0), info_w, h.h - m.px(8.0));
            ui.rect(r.x, r.y, r.w, r.h, c.bg);
            ui.rect(r.x, r.bottom() - m.px(2.0), r.w, m.px(2.0), c.accent);
            let status = doc.search_status().unwrap_or_default();
            ui.texts.push(text(
                key(1),
                format!("Find: {}", search.query),
                r.inset(m.px(6.0)),
                size,
                c.fg,
                Align::Left,
            ));
            ui.texts.push(text(
                key(2),
                status,
                r.inset(m.px(6.0)),
                size,
                c.dim,
                Align::Right,
            ));
        } else {
            ui.texts.push(text(
                key(1),
                doc.info(),
                Rect::new(info_x, h.y, info_w, h.h),
                size,
                c.dim,
                Align::Left,
            ));
        }

        for (i, (b, r, label, on, kind)) in buttons.iter().enumerate() {
            let hovered = *s.hover == Hit::DocButton(id, *b);
            let color = match kind {
                ButtonKind::Segment => {
                    // A framed group; the chosen part is filled with the accent.
                    ui.rect(r.x, r.y, r.w, r.h, c.divider);
                    let fill = if *on {
                        c.accent
                    } else if hovered {
                        c.hover
                    } else {
                        c.raised
                    };
                    ui.rect(
                        r.x + m.gap,
                        r.y + m.gap,
                        r.w - 2.0 * m.gap,
                        r.h - 2.0 * m.gap,
                        fill,
                    );
                    if *on { c.bg } else { c.fg }
                }
                ButtonKind::Toggle => {
                    if hovered {
                        ui.rect(r.x, r.y, r.w, r.h, c.hover);
                    }
                    if *on { c.accent } else { c.dim }
                }
                ButtonKind::Action => {
                    if hovered {
                        ui.rect(r.x, r.y, r.w, r.h, c.hover);
                    }
                    c.fg
                }
            };
            let mut t = text(
                key(8 + i as u64),
                *label,
                *r,
                if *b == DocButton::Close {
                    size * 1.3
                } else {
                    size
                },
                color,
                Align::Center,
            );
            t.bold = *kind == ButtonKind::Segment && *on;
            ui.texts.push(t);
        }

        // Loading / can't show.
        let message = match (&doc.doc, doc.message()) {
            (None, _) => Some("Loading…"),
            (_, Some(msg)) => Some(msg),
            _ => None,
        };
        if let Some(msg) = message {
            ui.texts.push(text(
                key(3),
                msg,
                doc.body.inset(m.px(16.0)),
                m.ui_text,
                c.dim,
                Align::Center,
            ));
        }
    }
}

const KEY_INPUT: u64 = 22 << 48;

/// Input boxes at the bottom of terminal panes: context line + frame around
/// the editor grid (the grid itself draws the text).
fn draw_inputs(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (m, c) = (s.metrics, s.colors);
    for (n, v) in s.inputs.iter().enumerate() {
        let r = v.rect;
        let key = KEY_INPUT | (n as u64) << 8;
        // Background everywhere except over the editor grid (UI rects are drawn
        // after grid text and would hide it).
        match v.grid {
            Some(g) => {
                ui.rect(r.x, r.y, r.w, g.y - r.y, c.raised);
                ui.rect(r.x, g.y, g.x - r.x, g.h, c.raised);
                ui.rect(g.right(), g.y, r.right() - g.right(), g.h, c.raised);
                ui.rect(r.x, g.bottom(), r.w, r.bottom() - g.bottom(), c.raised);
            }
            None => ui.rect(r.x, r.y, r.w, r.h, c.raised),
        }
        ui.rect(
            r.x,
            r.y,
            r.w,
            m.gap,
            if v.focused {
                mix(c.accent, c.raised, 0.4)
            } else {
                c.divider
            },
        );

        // Context line: folder, git branch, last exit code / running command.
        let pad = m.px(12.0);
        let line = Rect::new(r.x + pad, r.y + m.px(4.0), r.w - 2.0 * pad, m.px(22.0));
        let size = m.ui_text * 0.9;
        let cwd_w = (v.cwd.chars().count() as f32 * size * 0.56 + m.px(6.0)).min(line.w * 0.6);
        ui.texts.push(text(
            key,
            v.cwd.clone(),
            Rect::new(line.x, line.y, cwd_w, line.h),
            size,
            c.dim,
            Align::Left,
        ));
        let mut x = line.x + cwd_w + m.px(10.0);
        if let Some(b) = &v.branch {
            let w = (b.chars().count() as f32 + 2.0) * size * 0.58 + m.px(10.0);
            let chip = Rect::new(x, line.y + m.px(2.0), w, line.h - m.px(4.0));
            ui.rect(
                chip.x,
                chip.y,
                chip.w,
                chip.h,
                mix(c.raised, c.branch, 0.15),
            );
            ui.texts.push(text(
                key | 1,
                format!("⎇ {b}"),
                chip,
                size * 0.95,
                c.branch,
                Align::Center,
            ));
            x += w + m.px(8.0);
        }
        if let Some(code) = v.exit.filter(|c| *c != 0) {
            let label = format!("✗ {code}");
            let w = (label.chars().count() as f32) * size * 0.6 + m.px(12.0);
            let chip = Rect::new(x, line.y + m.px(2.0), w, line.h - m.px(4.0));
            ui.rect(chip.x, chip.y, chip.w, chip.h, mix(c.raised, c.error, 0.18));
            ui.texts.push(text(
                key | 2,
                label,
                chip,
                size * 0.95,
                c.error,
                Align::Center,
            ));
        }
        if let Some(cmd) = &v.running {
            ui.texts.push(text(
                key | 3,
                format!("running  {cmd}"),
                line,
                size,
                c.accent,
                Align::Right,
            ));
        }
        if v.grid.is_none() {
            let body = Rect::new(
                r.x + pad,
                line.bottom(),
                r.w - 2.0 * pad,
                r.bottom() - line.bottom(),
            );
            ui.texts.push(text(
                key | 4,
                "Keys go to the running program",
                body,
                size,
                mix(c.dim, c.raised, 0.3),
                Align::Left,
            ));
        }
    }
}

fn draw_panes_decor(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let Some(tab) = s.workspace.active_tab() else {
        return;
    };
    if tab.zoomed.is_some() {
        return;
    }
    let dividers = tab.tree.dividers(l.content, m.gap);
    for d in &dividers {
        let color = if matches!(s.hover, Hit::Divider(h) if h.path == d.path) {
            c.accent
        } else {
            c.divider
        };
        ui.rect(d.rect.x, d.rect.y, d.rect.w, d.rect.h, color);
    }
    if !dividers.is_empty() && s.window_focused {
        // Mark the focused pane with an accent line on top.
        if let Some((_, r)) = tab
            .layout(l.content, m.gap)
            .into_iter()
            .find(|(id, _)| *id == tab.focused)
        {
            ui.rect(r.x, r.y, r.w, m.px(2.0), c.accent);
        }
    }
}

fn draw_status(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c, ws) = (s.layout, s.metrics, s.colors, s.workspace);
    let r = l.status;
    ui.rect(r.x, r.y, r.w, r.h, c.chrome);
    ui.rect(r.x, r.y, r.w, m.gap, c.divider);
    let pad = m.px(12.0);
    let inner = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad).max(0.0), r.h);
    let size = m.ui_text * 0.92;

    let doc = ws.focused_doc();
    let left = if let Some(d) = doc {
        d.path.display().to_string()
    } else {
        ws.focused_pane().map_or(String::new(), |p| {
            let place = p
                .cwd
                .as_ref()
                .map_or_else(|| p.title.clone(), |c| c.display().to_string());
            match p.last_exit {
                Some(code) if code != 0 => format!("{place}   ✗ exit {code}"),
                _ => place,
            }
        })
    };
    ui.texts.push(text(
        KEY_STATUS,
        left,
        Rect::new(inner.x, inner.y, inner.w * 0.6, inner.h),
        size,
        c.dim,
        Align::Left,
    ));

    let right = match (ws.active_tab(), ws.focused_pane()) {
        (Some(tab), _) if doc.is_some() => {
            let d = doc.map_or(String::new(), |d| {
                let total = d.total_rows();
                if total == 0 {
                    return String::new();
                }
                let pct = if d.max_scroll() == 0 {
                    100
                } else {
                    d.scroll * 100 / d.max_scroll()
                };
                format!("  ·  row {}/{}  ({pct}%)", d.scroll + 1, total)
            });
            format!(
                "tab {}/{}  ·  pane {}/{}{d}",
                ws.active + 1,
                ws.tabs.len(),
                tab.tree
                    .panes()
                    .iter()
                    .position(|&id| id == tab.focused)
                    .unwrap_or(0)
                    + 1,
                tab.tree.len()
            )
        }
        (Some(tab), Some(pane)) => {
            let grid = pane.terminal.size();
            let panes = tab.tree.len();
            let index = tab
                .tree
                .panes()
                .iter()
                .position(|&id| id == tab.focused)
                .unwrap_or(0);
            format!(
                "tab {}/{}  ·  pane {}/{}  ·  {}×{}",
                ws.active + 1,
                ws.tabs.len(),
                index + 1,
                panes,
                grid.cols,
                grid.rows
            )
        }
        _ => String::new(),
    };
    // Live metrics while a sampling panel is open.
    let right = match s.sidebar.monitor.snap.as_deref() {
        Some(snap) if s.sidebar.interest().any() => {
            let agents = match s.sidebar.agents.entries.len() {
                0 => String::new(),
                1 => "1 agent  ·  ".into(),
                n => format!("{n} agents  ·  "),
            };
            format!(
                "{agents}CPU {:.0}%  ·  RAM {} / {}  ·  {right}",
                snap.cpu,
                mao_sysmon::human_bytes(snap.mem_used),
                mao_sysmon::human_bytes(snap.mem_total)
            )
        }
        _ => right,
    };
    ui.texts.push(text(
        KEY_STATUS | 1,
        right,
        inner,
        size,
        c.dim,
        Align::Right,
    ));
}

fn draw_menu(ui: &mut UiFrame, s: &DrawState<'_>, v: &MenuView<'_>) {
    let (m, c, l) = (s.metrics, s.colors, s.layout);
    let items = &v.menu.completions.items;
    if items.is_empty() {
        return;
    }
    let rows = items.len().min(MENU_ROWS);
    let row_h = m.px(26.0);
    let footer = if items.len() > MENU_ROWS {
        m.px(22.0)
    } else {
        0.0
    };
    let pad = m.px(4.0);
    let h = rows as f32 * row_h + 2.0 * pad + footer;
    let w = m.px(440.0).min(l.content.w - m.px(16.0)).max(m.px(120.0));
    // Align the labels (inset by `pad` + 8px) with the token being completed.
    let x = (v.x - pad - m.px(8.0))
        .min(l.content.right() - w - m.px(8.0))
        .max(l.content.x + m.px(4.0));
    let y = if v.below + h <= l.content.bottom() {
        v.below + m.px(2.0)
    } else {
        (v.above - h - m.px(2.0)).max(l.content.y)
    };
    let b = m.gap;
    ui.rect(x - b, y - b, w + 2.0 * b, h + 2.0 * b, c.divider);
    ui.rect(x, y, w, h, c.raised);

    let start = v.menu.scroll.min(items.len().saturating_sub(rows));
    for (i, item) in items.iter().enumerate().skip(start).take(rows) {
        let ry = y + pad + (i - start) as f32 * row_h;
        let row = Rect::new(x + pad, ry, w - 2.0 * pad, row_h);
        if i == v.menu.selected {
            ui.rect(row.x, row.y, row.w, row.h, c.selection);
        }
        let color = match item.kind {
            mao_complete::ItemKind::Directory => c.accent,
            mao_complete::ItemKind::Branch => c.branch,
            mao_complete::ItemKind::Option => c.option,
            _ => c.fg,
        };
        let inner = Rect::new(row.x + m.px(8.0), row.y, row.w - m.px(16.0), row.h);
        let label_w = inner.w * 0.62;
        let mut label = text(
            KEY_MENU_LABEL | (i - start) as u64,
            item.label.clone(),
            Rect::new(inner.x, inner.y, label_w, inner.h),
            m.ui_text,
            color,
            Align::Left,
        );
        label.mono = true;
        ui.texts.push(label);
        ui.texts.push(text(
            KEY_MENU_DETAIL | (i - start) as u64,
            item.detail.clone(),
            Rect::new(inner.x + label_w, inner.y, inner.w - label_w, inner.h),
            m.ui_text * 0.88,
            c.dim,
            Align::Right,
        ));
    }
    if footer > 0.0 {
        let fy = y + pad + rows as f32 * row_h;
        ui.texts.push(text(
            KEY_MENU_COUNT,
            format!("{}/{}", v.menu.selected + 1, items.len()),
            Rect::new(x + m.px(12.0), fy, w - m.px(24.0), footer),
            m.ui_text * 0.85,
            c.dim,
            Align::Right,
        ));
    }
}

/// Palette box geometry, shared by drawing and hit-testing.
struct PaletteGeometry {
    frame: Rect,
    input: Rect,
    rows: Vec<Rect>,
}

impl PaletteGeometry {
    fn new(l: &Layout, m: &Metrics, p: &Palette) -> Self {
        let w = m.px(620.0).min(l.content.w - m.px(32.0)).max(0.0);
        let x = (l.content.x + (l.content.w - w) / 2.0).round();
        let y = l.content.y + m.px(12.0);
        let input_h = m.px(42.0);
        let row_h = m.px(32.0);
        let n = p.matches.len().min(MAX_VISIBLE);
        let pad = m.px(6.0);
        let input = Rect::new(x, y, w, input_h);
        let rows = (0..n)
            .map(|i| {
                Rect::new(
                    x + pad,
                    y + input_h + pad + i as f32 * row_h,
                    w - 2.0 * pad,
                    row_h,
                )
            })
            .collect::<Vec<_>>();
        let h = input_h
            + if n > 0 {
                2.0 * pad + n as f32 * row_h
            } else {
                0.0
            };
        Self {
            frame: Rect::new(x, y, w, h),
            input,
            rows,
        }
    }
}

fn draw_palette(ui: &mut UiFrame, s: &DrawState<'_>, p: &Palette) {
    let (m, c) = (s.metrics, s.colors);
    let g = PaletteGeometry::new(s.layout, m, p);
    let f = g.frame;
    let b = m.gap;
    // Border, then body.
    ui.rect(f.x - b, f.y - b, f.w + 2.0 * b, f.h + 2.0 * b, c.divider);
    ui.rect(f.x, f.y, f.w, f.h, c.raised);
    ui.rect(g.input.x, g.input.bottom() - b, g.input.w, b, c.divider);

    let pad = m.px(14.0);
    let input_text = Rect::new(g.input.x + pad, g.input.y, g.input.w - 2.0 * pad, g.input.h);
    if p.query.is_empty() {
        ui.texts.push(text(
            KEY_PALETTE,
            if p.is_themes() {
                "Select a theme (↑↓ to preview, Enter to keep, Esc to cancel)"
            } else {
                "Type a command…"
            },
            input_text,
            m.ui_text * 1.05,
            c.dim,
            Align::Left,
        ));
    } else {
        ui.texts.push(text(
            KEY_PALETTE,
            format!("{}▏", p.query),
            input_text,
            m.ui_text * 1.05,
            c.fg,
            Align::Left,
        ));
    }

    for (slot, (row, (i, item))) in g.rows.iter().zip(p.visible()).enumerate() {
        let selected = i == p.selected;
        let hovered = *s.hover == Hit::PaletteRow(i);
        if selected {
            ui.rect(row.x, row.y, row.w, row.h, c.selection);
        } else if hovered {
            ui.rect(row.x, row.y, row.w, row.h, c.hover);
        }
        let mut inner = Rect::new(row.x + m.px(10.0), row.y, row.w - m.px(20.0), row.h);
        // Themes: a swatch of the theme's background and normal colors.
        if let Some(theme) = p.theme(item) {
            let t = &theme.colors;
            let sw = m.px(14.0);
            let n = 8.0;
            let x0 = inner.right() - n * sw - m.px(6.0);
            let y0 = row.y + (row.h - sw) / 2.0;
            ui.rect(
                x0 - m.px(4.0),
                y0 - m.px(3.0),
                n * sw + m.px(8.0),
                sw + m.px(6.0),
                t.background.to_array(),
            );
            for (k, color) in t.ansi[..8].iter().enumerate() {
                ui.rect(x0 + k as f32 * sw + 1.0, y0, sw - 2.0, sw, color.to_array());
            }
            inner.w = (x0 - inner.x - m.px(12.0)).max(0.0);
        }
        ui.texts.push(text(
            KEY_PALETTE_ROW | slot as u64,
            p.label(item),
            inner,
            m.ui_text,
            c.fg,
            Align::Left,
        ));
        ui.texts.push(text(
            KEY_PALETTE_SHORTCUT | slot as u64,
            p.detail(item),
            inner,
            m.ui_text * 0.9,
            c.dim,
            Align::Right,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_window() {
        let m = Metrics::new(1.0, 8.0);
        let l = Layout::new((1000, 600), &m, 3, None);
        assert_eq!(l.activity, Rect::new(0.0, 0.0, 46.0, 576.0));
        assert_eq!(l.tab_bar, Rect::new(46.0, 0.0, 954.0, 36.0));
        assert_eq!(l.status, Rect::new(0.0, 576.0, 1000.0, 24.0));
        assert_eq!(l.content, Rect::new(46.0, 36.0, 954.0, 540.0));
        assert_eq!(l.tabs.len(), 3);
        assert_eq!(l.tabs[0].x, 46.0);
        assert_eq!(l.plus.x, 46.0 + 660.0);
        let with_sidebar = Layout::new((1000, 600), &m, 1, Some(250.0));
        assert_eq!(
            with_sidebar.sidebar,
            Some(Rect::new(46.0, 0.0, 250.0, 576.0))
        );
        assert_eq!(with_sidebar.content.x, 296.0);
    }

    #[test]
    fn tabs_shrink_when_many() {
        let m = Metrics::new(1.0, 8.0);
        let l = Layout::new((800, 600), &m, 20, None);
        assert!(l.tabs[19].right() <= 800.0 - 36.0);
    }
}
