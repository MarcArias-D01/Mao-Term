//! Command palette state: query, filtered actions, selection.

use crate::actions::{Action, PALETTE};

pub const MAX_VISIBLE: usize = 10;

#[derive(Debug, Default)]
pub struct Palette {
    pub query: String,
    pub matches: Vec<Action>,
    pub selected: usize,
}

impl Palette {
    pub fn new() -> Self {
        let mut p = Self::default();
        p.refilter();
        p
    }

    pub fn push(&mut self, text: &str) {
        self.query.push_str(text);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn move_selection(&mut self, delta: isize) {
        let n = self.matches.len().min(MAX_VISIBLE);
        if n == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
    }

    pub fn selected_action(&self) -> Option<Action> {
        self.matches.get(self.selected).copied()
    }

    fn refilter(&mut self) {
        let mut scored: Vec<(i32, usize, Action)> = PALETTE
            .iter()
            .enumerate()
            .filter_map(|(i, &a)| mao_ui::fuzzy::score(&self.query, a.name()).map(|s| (s, i, a)))
            .collect();
        // Best score first; keep the list order for ties (and for an empty query).
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.matches = scored.into_iter().map(|(_, _, a)| a).collect();
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_selects() {
        let mut p = Palette::new();
        assert_eq!(p.matches.len(), PALETTE.len());
        p.push("split r");
        assert_eq!(p.selected_action(), Some(Action::SplitRight));
        p.backspace();
        p.backspace();
        p.push("d");
        assert_eq!(p.selected_action(), Some(Action::SplitDown));
    }

    #[test]
    fn selection_wraps() {
        let mut p = Palette::new();
        p.move_selection(-1);
        assert_eq!(p.selected, PALETTE.len().min(MAX_VISIBLE) - 1);
        p.move_selection(1);
        assert_eq!(p.selected, 0);
    }
}
