//! The prompt editor: Mao-Term's own line editor shown at the shell prompt,
//! with history ghost text and a completion menu (ADR 0002).
//! See `docs/specs/input-autocomplete.md`.

use std::path::Path;

use mao_complete::{Completions, Engine, ItemKind, Quoting, Request};
use mao_input::LineEditor;
use mao_terminal::keys::Modifiers;
use winit::event::KeyEvent;
use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// Rows the completion menu shows at once.
pub const MENU_ROWS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub completions: Completions,
    pub selected: usize,
    /// First visible item.
    pub scroll: usize,
}

impl Menu {
    fn move_by(&mut self, delta: isize) {
        let n = self.completions.items.len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + MENU_ROWS {
            self.scroll = self.selected + 1 - MENU_ROWS;
        }
    }
}

/// Walking through history with Up/Down, filtered by what was typed.
#[derive(Debug, Clone)]
struct HistoryNav {
    typed: String,
    matches: Vec<String>,
    /// Index into `matches`; `None` = back at what was typed.
    index: Option<usize>,
}

/// What the app must do after the editor handled a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing visible changed.
    Ignored,
    /// The editor changed: redraw.
    Changed,
    /// Run this command line.
    Submit(String),
    /// The key isn't for the editor: send it to the shell as usual.
    PassThrough,
}

/// Context the editor needs from its pane.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub engine: &'a Engine,
    pub cwd: Option<&'a Path>,
    pub quoting: Quoting,
    pub suggestions: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PromptEditor {
    pub line: LineEditor,
    /// History suggestion shown after the caret.
    pub ghost: Option<String>,
    pub menu: Option<Menu>,
    nav: Option<HistoryNav>,
}

impl PromptEditor {
    pub fn on_key(&mut self, event: &KeyEvent, mods: Modifiers, ctx: Ctx<'_>) -> Outcome {
        let Modifiers { shift, alt, ctrl } = mods;
        let base = event.key_without_modifiers();
        let named = match &event.logical_key {
            Key::Named(n) => Some(*n),
            _ => None,
        };

        // Keys that drive the menu while it is open.
        if self.menu.is_some() {
            match named {
                Some(NamedKey::ArrowUp) => return self.menu_move(-1),
                Some(NamedKey::ArrowDown) => return self.menu_move(1),
                Some(NamedKey::Tab) if shift => return self.menu_move(-1),
                Some(NamedKey::Tab | NamedKey::Enter) => {
                    self.accept_menu(ctx);
                    return Outcome::Changed;
                }
                Some(NamedKey::Escape) => {
                    self.menu = None;
                    return Outcome::Changed;
                }
                Some(NamedKey::PageDown) => return self.menu_move(MENU_ROWS as isize),
                Some(NamedKey::PageUp) => return self.menu_move(-(MENU_ROWS as isize)),
                _ => {}
            }
        }

        let line = &mut self.line;
        let edited = match named {
            Some(NamedKey::Enter) if !shift => {
                self.ghost = None;
                self.menu = None;
                self.nav = None;
                return Outcome::Submit(self.line.take());
            }
            Some(NamedKey::Tab) if !ctrl => return self.complete(ctx),
            Some(NamedKey::Escape) => {
                if line.is_empty() {
                    return Outcome::Ignored;
                }
                line.clear();
                true
            }
            Some(NamedKey::ArrowUp) => return self.history_step(1, ctx),
            Some(NamedKey::ArrowDown) => return self.history_step(-1, ctx),
            Some(NamedKey::ArrowLeft) if ctrl => {
                line.word_left(shift);
                false
            }
            Some(NamedKey::ArrowLeft) => {
                line.move_left(shift);
                false
            }
            Some(NamedKey::ArrowRight | NamedKey::End)
                if line.at_end() && !shift && self.ghost.is_some() =>
            {
                // Accept the suggestion (Ctrl+Right: one word of it).
                let ghost = self.ghost.take().unwrap_or_default();
                let take = if ctrl && named == Some(NamedKey::ArrowRight) {
                    next_word_len(&ghost)
                } else {
                    ghost.len()
                };
                self.line.insert(&ghost[..take]);
                true
            }
            Some(NamedKey::ArrowRight) if ctrl => {
                line.word_right(shift);
                false
            }
            Some(NamedKey::ArrowRight) => {
                line.move_right(shift);
                false
            }
            Some(NamedKey::Home) => {
                line.home(shift);
                false
            }
            Some(NamedKey::End) => {
                line.end(shift);
                false
            }
            Some(NamedKey::Backspace) if ctrl || alt => {
                line.delete_word_back();
                true
            }
            Some(NamedKey::Backspace) => {
                line.backspace();
                true
            }
            Some(NamedKey::Delete) if ctrl => {
                line.delete_word_forward();
                true
            }
            Some(NamedKey::Delete) => {
                line.delete();
                true
            }
            _ => match &base {
                // Emacs-style shortcuts most shells offer.
                Key::Character(c) if ctrl && !alt => match c.to_ascii_lowercase().as_str() {
                    "a" => {
                        line.home(shift);
                        false
                    }
                    "e" => {
                        line.end(shift);
                        false
                    }
                    "k" => {
                        line.delete_to_end();
                        true
                    }
                    "u" => {
                        line.delete_to_start();
                        true
                    }
                    "w" => {
                        line.delete_word_back();
                        true
                    }
                    "z" => {
                        line.undo();
                        true
                    }
                    "y" => {
                        line.redo();
                        true
                    }
                    // Clear screen and friends: let the shell handle them.
                    "l" => return Outcome::PassThrough,
                    "d" if line.is_empty() => return Outcome::PassThrough,
                    "d" => {
                        line.delete();
                        true
                    }
                    _ => return Outcome::Ignored,
                },
                _ => {
                    // Printable text (AltGr arrives as Ctrl+Alt on Windows).
                    let Some(text) = event.text.as_deref() else {
                        return Outcome::Ignored;
                    };
                    if text.chars().any(char::is_control) || (ctrl && !alt) {
                        return Outcome::Ignored;
                    }
                    line.insert(text);
                    true
                }
            },
        };
        self.after_change(edited, ctx);
        Outcome::Changed
    }

    /// Inserts pasted text (single line).
    pub fn insert(&mut self, text: &str, ctx: Ctx<'_>) {
        self.line.insert(text);
        self.after_change(true, ctx);
    }

    /// Refreshes ghost text and the open menu after an edit or caret move.
    fn after_change(&mut self, edited: bool, ctx: Ctx<'_>) {
        if edited {
            self.nav = None;
        }
        self.ghost = if ctx.suggestions && self.line.at_end() && !self.line.is_empty() {
            ctx.engine.inline(self.line.text(), ctx.cwd)
        } else {
            None
        };
        if edited && self.menu.is_some() {
            let completions = self.completions(ctx);
            self.menu = (!completions.items.is_empty()).then_some(Menu {
                completions,
                selected: 0,
                scroll: 0,
            });
        }
    }

    fn completions(&self, ctx: Ctx<'_>) -> Completions {
        ctx.engine.complete(Request {
            line: self.line.text(),
            cursor: self.line.cursor(),
            cwd: ctx.cwd,
            quoting: ctx.quoting,
        })
    }

    /// Tab: complete the token under the caret.
    fn complete(&mut self, ctx: Ctx<'_>) -> Outcome {
        let completions = self.completions(ctx);
        match completions.items.len() {
            0 => Outcome::Ignored,
            1 => {
                let item = completions.items[0].clone();
                self.apply(&completions, &item.insert, item.kind, ctx);
                Outcome::Changed
            }
            _ => {
                // Extend the token with what all candidates share, then show them.
                let current = &self.line.text()[completions.range.clone()];
                if let Some(prefix) = completions.common_prefix()
                    && prefix.len() > current.len()
                    && prefix.starts_with(current)
                {
                    self.line.replace_range(completions.range.clone(), &prefix);
                }
                let completions = self.completions(ctx);
                self.ghost = None;
                self.menu = (!completions.items.is_empty()).then_some(Menu {
                    completions,
                    selected: 0,
                    scroll: 0,
                });
                Outcome::Changed
            }
        }
    }

    fn menu_move(&mut self, delta: isize) -> Outcome {
        if let Some(menu) = self.menu.as_mut() {
            menu.move_by(delta);
        }
        Outcome::Changed
    }

    fn accept_menu(&mut self, ctx: Ctx<'_>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if let Some(item) = menu.completions.items.get(menu.selected).cloned() {
            self.apply(&menu.completions, &item.insert, item.kind, ctx);
        }
    }

    /// Replaces the token with `insert`, adding a space unless more can follow.
    fn apply(&mut self, completions: &Completions, insert: &str, kind: ItemKind, ctx: Ctx<'_>) {
        let continues = kind == ItemKind::Directory || insert.ends_with(['/', '\\']);
        let mut text = insert.to_owned();
        let at_end = completions.range.end >= self.line.text().len();
        if !continues && at_end {
            text.push(' ');
        }
        self.line.replace_range(completions.range.clone(), &text);
        self.menu = None;
        self.after_change(true, ctx);
    }

    /// Up (`dir` = 1) / Down (-1) through history entries starting with what was typed.
    fn history_step(&mut self, dir: isize, ctx: Ctx<'_>) -> Outcome {
        let nav = self.nav.get_or_insert_with(|| {
            let typed = self.line.text().to_owned();
            let mut seen = std::collections::HashSet::new();
            let matches = ctx
                .engine
                .history
                .matching(&typed)
                .filter(|c| *c != typed && seen.insert(c.to_string()))
                .map(str::to_owned)
                .collect();
            HistoryNav {
                typed,
                matches,
                index: None,
            }
        });
        let next = match (nav.index, dir) {
            (None, 1) if !nav.matches.is_empty() => Some(0),
            (None, _) => return Outcome::Ignored,
            (Some(i), 1) => Some((i + 1).min(nav.matches.len() - 1)),
            (Some(0), _) => None,
            (Some(i), _) => Some(i - 1),
        };
        nav.index = next;
        let text = next.map_or_else(|| nav.typed.clone(), |i| nav.matches[i].clone());
        self.line.set(&text);
        self.ghost = None;
        Outcome::Changed
    }
}

fn next_word_len(s: &str) -> usize {
    let trimmed = s.trim_start();
    let lead = s.len() - trimmed.len();
    lead + trimmed.find(char::is_whitespace).unwrap_or(trimmed.len())
}

/// Syntax classes for the editor's own highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Command,
    Option,
    String,
    Variable,
    Operator,
    Plain,
}

/// Per-byte highlight classes for a command line (cheap, approximate).
pub fn highlight(line: &str) -> Vec<Highlight> {
    let mut out = vec![Highlight::Plain; line.len()];
    let mut command_next = true;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if matches!(c, '|' | ';' | '&' | '>' | '<') {
            out[i] = Highlight::Operator;
            command_next = matches!(c, '|' | ';' | '&');
            continue;
        }
        // A whole word (quotes kept together).
        let start = i;
        let mut end = i + c.len_utf8();
        let mut quote = matches!(c, '"' | '\'').then_some(c);
        while let Some(&(j, d)) = chars.peek() {
            match quote {
                Some(q) if d == q => {
                    quote = None;
                }
                Some(_) => {}
                None if d.is_whitespace() || matches!(d, '|' | ';' | '&' | '>' | '<') => break,
                None if matches!(d, '"' | '\'') => quote = Some(d),
                None => {}
            }
            end = j + d.len_utf8();
            chars.next();
        }
        let word = &line[start..end];
        let class = if command_next {
            Highlight::Command
        } else if word.starts_with('-') {
            Highlight::Option
        } else if word.starts_with(['"', '\'']) {
            Highlight::String
        } else if word.starts_with('$') {
            Highlight::Variable
        } else {
            Highlight::Plain
        };
        command_next = false;
        out[start..end].fill(class);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_words() {
        let line = "git commit -m \"a b\" | grep $x";
        let h = highlight(line);
        let at = |s: &str| h[line.find(s).unwrap()];
        assert_eq!(at("git"), Highlight::Command);
        assert_eq!(at("commit"), Highlight::Plain);
        assert_eq!(at("-m"), Highlight::Option);
        assert_eq!(at("\"a"), Highlight::String);
        assert_eq!(h[line.find("b\"").unwrap()], Highlight::String);
        assert_eq!(at("|"), Highlight::Operator);
        assert_eq!(at("grep"), Highlight::Command);
        assert_eq!(at("$x"), Highlight::Variable);
    }

    #[test]
    fn next_word() {
        assert_eq!(next_word_len("sh origin main"), 2);
        assert_eq!(next_word_len(" origin main"), 7);
    }
}
