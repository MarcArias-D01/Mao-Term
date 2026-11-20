//! Command palette state: query, filtered items, selection. It lists either
//! the commands or, in theme mode, the color themes (previewed live).

use mao_config::themes::Theme;

use crate::actions::{Action, PALETTE};

pub const MAX_VISIBLE: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Action(Action),
    /// Index into the theme list.
    Theme(usize),
    /// Index into the font list.
    Font(usize),
}

#[derive(Debug, Default)]
pub enum Mode {
    #[default]
    Commands,
    Themes {
        themes: Vec<Theme>,
        /// Theme active when the picker opened (restored on Esc).
        original: String,
    },
    Fonts {
        families: Vec<String>,
        current: String,
    },
}

#[derive(Debug, Default)]
pub struct Palette {
    pub query: String,
    pub matches: Vec<Item>,
    pub selected: usize,
    /// First visible match (the list scrolls with the selection).
    pub scroll: usize,
    pub mode: Mode,
}

impl Palette {
    pub fn new() -> Self {
        let mut p = Self::default();
        p.refilter();
        p
    }

    /// Theme picker, with the current theme selected.
    pub fn themes(themes: Vec<Theme>, current: &str) -> Self {
        let mut p = Self {
            mode: Mode::Themes {
                themes,
                original: current.to_owned(),
            },
            ..Self::default()
        };
        p.refilter();
        if let Some(i) = p
            .matches
            .iter()
            .position(|m| p.theme(*m).is_some_and(|t| t.name == current))
        {
            p.select(i);
        }
        p
    }

    /// Font picker, with the current family selected.
    pub fn fonts(families: Vec<String>, current: &str) -> Self {
        let mut p = Self {
            mode: Mode::Fonts {
                families,
                current: current.to_owned(),
            },
            ..Self::default()
        };
        p.refilter();
        if let Some(i) = p.matches.iter().position(|m| p.label(*m) == current) {
            p.select(i);
        }
        p
    }

    /// The font family of a font item.
    pub fn font(&self, item: Item) -> Option<&str> {
        match (&self.mode, item) {
            (Mode::Fonts { families, .. }, Item::Font(i)) => families.get(i).map(String::as_str),
            _ => None,
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self.mode {
            Mode::Commands => "Type a command…",
            Mode::Themes { .. } => "Select a theme (↑↓ to preview, Enter to keep, Esc to cancel)",
            Mode::Fonts { .. } => "Select a monospace font (Enter to apply)",
        }
    }

    pub fn theme(&self, item: Item) -> Option<&Theme> {
        match (&self.mode, item) {
            (Mode::Themes { themes, .. }, Item::Theme(i)) => themes.get(i),
            _ => None,
        }
    }

    pub fn original_theme(&self) -> Option<&str> {
        match &self.mode {
            Mode::Themes { original, .. } => Some(original),
            Mode::Commands | Mode::Fonts { .. } => None,
        }
    }

    pub fn label(&self, item: Item) -> &str {
        match item {
            Item::Action(a) => a.name(),
            Item::Theme(_) => self.theme(item).map_or("", |t| t.name.as_str()),
            Item::Font(_) => self.font(item).unwrap_or(""),
        }
    }

    /// Right-aligned detail: the shortcut, or where a theme comes from.
    pub fn detail(&self, item: Item) -> &str {
        match item {
            Item::Action(a) => a.shortcut(),
            Item::Theme(_) => match self.theme(item) {
                Some(t) if t.path.is_some() => "custom",
                _ => "",
            },
            Item::Font(_) => match &self.mode {
                Mode::Fonts { current, .. } if self.font(item) == Some(current.as_str()) => {
                    "current"
                }
                _ => "",
            },
        }
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
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.select((self.selected as isize + delta).rem_euclid(n as isize) as usize);
    }

    fn select(&mut self, i: usize) {
        self.selected = i;
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + MAX_VISIBLE {
            self.scroll = i + 1 - MAX_VISIBLE;
        }
    }

    /// Visible matches with their index in `matches`.
    pub fn visible(&self) -> impl Iterator<Item = (usize, Item)> + '_ {
        self.matches
            .iter()
            .copied()
            .enumerate()
            .skip(self.scroll)
            .take(MAX_VISIBLE)
    }

    pub fn selected_item(&self) -> Option<Item> {
        self.matches.get(self.selected).copied()
    }

    pub fn selected_action(&self) -> Option<Action> {
        match self.selected_item()? {
            Item::Action(a) => Some(a),
            Item::Theme(_) | Item::Font(_) => None,
        }
    }

    fn refilter(&mut self) {
        let names: Vec<(Item, String)> = match &self.mode {
            Mode::Commands => PALETTE
                .iter()
                .map(|&a| (Item::Action(a), a.name().to_owned()))
                .collect(),
            Mode::Themes { themes, .. } => themes
                .iter()
                .enumerate()
                .map(|(i, t)| (Item::Theme(i), t.name.clone()))
                .collect(),
            Mode::Fonts { families, .. } => families
                .iter()
                .enumerate()
                .map(|(i, f)| (Item::Font(i), f.clone()))
                .collect(),
        };
        let mut scored: Vec<(i32, usize, Item)> = names
            .into_iter()
            .enumerate()
            .filter_map(|(i, (item, name))| {
                mao_ui::fuzzy::score(&self.query, &name).map(|s| (s, i, item))
            })
            .collect();
        // Best score first; keep the list order for ties (and for an empty query).
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.matches = scored.into_iter().map(|(_, _, a)| a).collect();
        self.selected = 0;
        self.scroll = 0;
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
    fn selection_wraps_and_scrolls() {
        let mut p = Palette::new();
        p.move_selection(-1);
        assert_eq!(p.selected, PALETTE.len() - 1);
        assert_eq!(p.scroll, PALETTE.len() - MAX_VISIBLE);
        p.move_selection(1);
        assert_eq!((p.selected, p.scroll), (0, 0));
    }

    #[test]
    fn theme_mode_starts_on_current() {
        let themes = mao_config::themes::builtin();
        let p = Palette::themes(themes, "Nord");
        assert_eq!(p.selected_item().map(|i| p.label(i)), Some("Nord"));
        assert_eq!(p.original_theme(), Some("Nord"));
    }
}
