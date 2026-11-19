//! Tabs and panes. See `docs/specs/workspace-layout.md`.

use std::collections::HashMap;

use mao_ui::{PaneId, Rect, SplitTree};

use crate::pane::Pane;

pub struct Tab {
    pub id: u64,
    pub tree: SplitTree,
    pub focused: PaneId,
    /// Pane temporarily maximized to the whole tab.
    pub zoomed: Option<PaneId>,
    /// A background pane produced output since the tab was last shown.
    pub activity: bool,
}

impl Tab {
    /// Visible panes and their rectangles inside `content`.
    pub fn layout(&self, content: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        match self.zoomed {
            Some(id) => vec![(id, content)],
            None => self.tree.layout(content, gap),
        }
    }
}

/// What closing a pane did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    Pane,
    Tab,
    /// That was the last pane of the last tab.
    Everything,
}

#[derive(Default)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub panes: HashMap<PaneId, Pane>,
    next_id: u64,
}

impl Workspace {
    /// Allocates an id for a new pane or tab.
    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn focused_id(&self) -> Option<PaneId> {
        self.active_tab().map(|t| t.focused)
    }

    pub fn focused_pane(&self) -> Option<&Pane> {
        self.panes.get(&self.focused_id()?)
    }

    pub fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        let id = self.focused_id()?;
        self.panes.get_mut(&id)
    }

    pub fn tab_of(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|t| t.tree.contains(pane))
    }

    /// Adds a tab holding `pane` and makes it active.
    pub fn add_tab(&mut self, pane: Pane) {
        let id = self.next_id();
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        self.tabs.push(Tab {
            id,
            tree: SplitTree::new(pane_id),
            focused: pane_id,
            zoomed: None,
            activity: false,
        });
        self.set_active(self.tabs.len() - 1);
    }

    /// Switches tabs; the newly shown panes must redraw fully.
    pub fn set_active(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        let tab = &mut self.tabs[index];
        tab.activity = false;
        for id in tab.tree.panes() {
            if let Some(p) = self.panes.get_mut(&id) {
                p.full_redraw = true;
            }
        }
    }

    pub fn focus(&mut self, pane: PaneId) {
        if let Some(tab) = self.active_tab_mut()
            && tab.tree.contains(pane)
        {
            tab.focused = pane;
        }
    }

    /// Removes and drops a pane (its shell is terminated with it).
    pub fn close_pane(&mut self, pane: PaneId) -> Closed {
        let Some(index) = self.tab_of(pane) else {
            return Closed::Pane;
        };
        self.panes.remove(&pane);
        let tab = &mut self.tabs[index];
        if tab.tree.remove(pane) {
            if tab.zoomed == Some(pane) {
                tab.zoomed = None;
            }
            if tab.focused == pane {
                tab.focused = tab.tree.panes()[0];
            }
            for id in tab.tree.panes() {
                if let Some(p) = self.panes.get_mut(&id) {
                    p.full_redraw = true;
                }
            }
            return Closed::Pane;
        }
        // It was the last pane of the tab.
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            return Closed::Everything;
        }
        let active = if self.active > index || self.active >= self.tabs.len() {
            self.active.saturating_sub(1)
        } else {
            self.active
        };
        self.set_active(active.min(self.tabs.len() - 1));
        Closed::Tab
    }
}
