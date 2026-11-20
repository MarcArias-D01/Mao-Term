//! Sidebar state: which panel is shown and the file explorer.
//! See `docs/specs/file-tree.md` and `docs/specs/workspace-layout.md`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mao_fs::{FsEvent, FsService, Tree};

/// Default sidebar width, in logical pixels.
pub const DEFAULT_WIDTH: f32 = 260.0;
pub const MIN_WIDTH: f32 = 160.0;
pub const MAX_WIDTH: f32 = 700.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const TYPE_AHEAD_RESET: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Files,
    Agents,
    Monitor,
}

/// What a click on a tree row asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    None,
    /// Open a file with the default app (double click / Enter).
    Open(PathBuf),
    /// `cd` into a folder (double click on a folder).
    Cd(PathBuf),
}

pub struct Sidebar {
    pub visible: bool,
    pub panel: Panel,
    /// Width in logical pixels.
    pub width: f32,
    pub files: Files,
    pub agents: AgentsView,
    pub monitor: MonitorView,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            visible: false,
            panel: Panel::Files,
            width: DEFAULT_WIDTH,
            files: Files::default(),
            agents: AgentsView::default(),
            monitor: MonitorView::default(),
        }
    }
}

impl Sidebar {
    /// Whether the file explorer is on screen (and must keep its watchers).
    pub fn files_active(&self) -> bool {
        self.visible && self.panel == Panel::Files
    }

    /// What the system sampler must collect for the panel on screen.
    pub fn interest(&self) -> mao_sysmon::Interest {
        mao_sysmon::Interest {
            metrics: self.visible && self.panel == Panel::Monitor,
            processes: self.visible && matches!(self.panel, Panel::Agents | Panel::Monitor),
        }
    }
}

/// An agent session and where it runs.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub agent: mao_agents::Agent,
    /// The Mao-Term pane (and its tab index) running it.
    pub pane: Option<(mao_ui::PaneId, usize)>,
}

/// A line of the agents list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRow {
    /// Project header (index into `groups`).
    Group(usize),
    /// An agent (index into `entries`), first or second line.
    Agent(usize, u8),
}

#[derive(Debug, Default)]
pub struct AgentsView {
    pub entries: Vec<AgentEntry>,
    /// Project folder and the entries working in it.
    pub groups: Vec<(Option<PathBuf>, Vec<usize>)>,
    /// At least one scan finished.
    pub scanned: bool,
    pub selected: Option<u32>,
    pub scroll: usize,
}

impl AgentsView {
    pub fn set(&mut self, entries: Vec<AgentEntry>) {
        self.groups.clear();
        for (i, e) in entries.iter().enumerate() {
            match self.groups.last_mut() {
                Some((cwd, list)) if *cwd == e.agent.cwd => list.push(i),
                _ => self.groups.push((e.agent.cwd.clone(), vec![i])),
            }
        }
        self.entries = entries;
        self.scanned = true;
    }

    pub fn rows(&self) -> Vec<AgentRow> {
        let mut rows = Vec::new();
        for (g, (_, list)) in self.groups.iter().enumerate() {
            rows.push(AgentRow::Group(g));
            for &i in list {
                rows.push(AgentRow::Agent(i, 0));
                rows.push(AgentRow::Agent(i, 1));
            }
        }
        rows
    }

    pub fn entry_at_row(&self, row: usize) -> Option<&AgentEntry> {
        match self.rows().get(row)? {
            AgentRow::Agent(i, _) => self.entries.get(*i),
            AgentRow::Group(_) => None,
        }
    }

    pub fn scroll_by(&mut self, rows: isize, visible: usize) {
        let max = self.rows().len().saturating_sub(visible);
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Cpu,
    Memory,
}

#[derive(Debug, Default)]
pub struct MonitorView {
    pub snap: Option<std::sync::Arc<mao_sysmon::Snapshot>>,
    pub sort: SortBy,
    /// Scroll offset in pixels.
    pub scroll: f32,
    pub selected: Option<u32>,
}

/// Processes shown in the monitor's list.
pub const TOP_PROCESSES: usize = 12;

impl MonitorView {
    /// The busiest processes by the current sort key.
    pub fn top(&self) -> Vec<&mao_sysmon::ProcInfo> {
        let Some(procs) = self.snap.as_ref().and_then(|s| s.processes.as_ref()) else {
            return Vec::new();
        };
        let mut list: Vec<&mao_sysmon::ProcInfo> = procs.iter().filter(|p| p.pid != 0).collect();
        match self.sort {
            SortBy::Cpu => {
                list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)))
            }
            SortBy::Memory => list.sort_by_key(|p| std::cmp::Reverse(p.memory)),
        }
        list.truncate(TOP_PROCESSES);
        list
    }
}

#[derive(Default)]
pub struct Files {
    pub tree: Tree,
    fs: Option<FsService>,
    pub show_ignored: bool,
    /// Scroll offset in rows.
    pub scroll: usize,
    pub selected: Option<PathBuf>,
    /// The tree has keyboard focus.
    pub focused: bool,
    type_ahead: String,
    type_ahead_at: Option<Instant>,
    last_click: Option<(Instant, PathBuf)>,
}

impl Files {
    /// Starts the listing/watching service if needed and points the tree at `root`.
    pub fn activate(&mut self, root: &Path, notify: impl FnOnce() -> FsService) {
        if self.fs.is_none() {
            self.fs = Some(notify());
            // Re-list what we show: listings may be stale since we were closed.
            for dir in self.tree.invalidate() {
                self.list(&dir);
            }
        }
        if self.tree.set_root(root) {
            self.scroll = 0;
            self.selected = None;
            self.sync_watches();
        }
    }

    /// Stops listing and watching (sidebar closed: zero cost).
    pub fn deactivate(&mut self) {
        self.fs = None;
        self.focused = false;
    }

    pub fn on_event(&mut self, event: FsEvent) {
        match event {
            FsEvent::Listed { dir, entries } => self.tree.on_listed(&dir, entries),
            FsEvent::Failed { dir, error } => self.tree.on_failed(&dir, error),
        }
        // A listing may reveal expanded folders that now need loading.
        self.sync_watches();
    }

    pub fn toggle_ignored(&mut self) {
        self.show_ignored = !self.show_ignored;
        for dir in self.tree.invalidate() {
            self.list(&dir);
        }
    }

    pub fn toggle(&mut self, dir: &Path) {
        self.tree.toggle(dir);
        self.sync_watches();
    }

    pub fn collapse_all(&mut self) {
        self.tree.collapse_all();
        self.scroll = 0;
        self.sync_watches();
    }

    /// Lists and watches exactly the folders on screen.
    fn sync_watches(&mut self) {
        let wanted = self.tree.visible_dirs();
        let Some(fs) = self.fs.as_mut() else {
            return;
        };
        let stale: Vec<PathBuf> = fs
            .watched()
            .filter(|w| !wanted.iter().any(|d| d == w))
            .map(Path::to_path_buf)
            .collect();
        for dir in stale {
            fs.forget(&dir);
        }
        let new: Vec<PathBuf> = wanted
            .into_iter()
            .filter(|d| !fs.watched().any(|w| w == d))
            .collect();
        for dir in new {
            self.list(&dir);
        }
    }

    fn list(&mut self, dir: &Path) {
        let respect_ignore = !self.show_ignored;
        if let Some(fs) = self.fs.as_mut() {
            fs.list(dir, respect_ignore);
        }
    }

    /// Paths of the visible rows (cheap clone for hit-testing and keyboard).
    pub fn row_paths(&self) -> Vec<(PathBuf, bool)> {
        self.tree
            .rows()
            .into_iter()
            .map(|r| (r.entry.path.clone(), r.entry.is_dir))
            .collect()
    }

    /// Mouse click on the row showing `path`.
    pub fn click(&mut self, path: &Path, is_dir: bool) -> RowAction {
        self.focused = true;
        self.selected = Some(path.to_path_buf());
        let now = Instant::now();
        let double = self
            .last_click
            .as_ref()
            .is_some_and(|(t, p)| p == path && now.duration_since(*t) < DOUBLE_CLICK);
        self.last_click = (!double).then(|| (now, path.to_path_buf()));
        match (is_dir, double) {
            (true, false) => {
                self.toggle(path);
                RowAction::None
            }
            // The first click already toggled; restore it and cd instead.
            (true, true) => {
                self.toggle(path);
                RowAction::Cd(path.to_path_buf())
            }
            (false, true) => RowAction::Open(path.to_path_buf()),
            (false, false) => RowAction::None,
        }
    }

    /// Moves the selection by `delta` rows; returns the new index.
    pub fn move_selection(&mut self, delta: isize, visible_rows: usize) -> Option<usize> {
        let rows = self.row_paths();
        if rows.is_empty() {
            return None;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|(p, _)| p == s));
        let next = match current {
            Some(i) => (i as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(rows[next].0.clone());
        self.scroll_to(next, visible_rows);
        Some(next)
    }

    /// Right/Left: expand/collapse the selected folder, or go to its parent.
    pub fn expand_selected(&mut self, expand: bool, visible_rows: usize) {
        let Some(sel) = self.selected.clone() else {
            return;
        };
        let rows = self.row_paths();
        let is_dir = rows
            .iter()
            .find(|(p, _)| *p == sel)
            .is_some_and(|(_, d)| *d);
        let expanded = self.tree.is_expanded(&sel);
        match (expand, is_dir, expanded) {
            (true, true, false) | (false, true, true) => self.toggle(&sel),
            (true, true, true) => {
                self.move_selection(1, visible_rows);
            }
            (false, _, _) => {
                if let Some(parent) = sel.parent()
                    && let Some(i) = rows.iter().position(|(p, _)| p == parent)
                {
                    self.selected = Some(parent.to_path_buf());
                    self.scroll_to(i, visible_rows);
                }
            }
            _ => {}
        }
    }

    /// Selects the next row whose name starts with the typed letters.
    pub fn type_ahead(&mut self, text: &str, visible_rows: usize) {
        let now = Instant::now();
        if self
            .type_ahead_at
            .is_none_or(|t| now.duration_since(t) > TYPE_AHEAD_RESET)
        {
            self.type_ahead.clear();
        }
        self.type_ahead_at = Some(now);
        self.type_ahead.push_str(&text.to_lowercase());
        let rows = self.tree.rows();
        let start = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| r.entry.path == *s))
            .map_or(0, |i| {
                if self.type_ahead.chars().count() == 1 {
                    i + 1
                } else {
                    i
                }
            });
        let n = rows.len();
        let hit = (0..n).map(|k| (start + k) % n.max(1)).find(|&i| {
            rows[i]
                .entry
                .name
                .to_lowercase()
                .starts_with(&self.type_ahead)
        });
        if let Some(i) = hit {
            let path = rows[i].entry.path.clone();
            drop(rows);
            self.selected = Some(path);
            self.scroll_to(i, visible_rows);
        }
    }

    fn scroll_to(&mut self, index: usize, visible_rows: usize) {
        if index < self.scroll {
            self.scroll = index;
        } else if visible_rows > 0 && index >= self.scroll + visible_rows {
            self.scroll = index + 1 - visible_rows;
        }
    }

    pub fn scroll_by(&mut self, rows: isize, visible_rows: usize) {
        let total = self.tree.rows().len();
        let max = total.saturating_sub(visible_rows);
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }
}
