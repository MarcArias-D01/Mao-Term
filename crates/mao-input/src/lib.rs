//! Native line editor used while the shell waits at its prompt (ADR 0002).
//! Pure text editing: no rendering, no terminal. See `docs/specs/input-autocomplete.md`.

use std::ops::Range;

/// A single-line editor with selection and undo.
#[derive(Debug, Clone, Default)]
pub struct LineEditor {
    text: String,
    /// Caret position, a byte index on a char boundary.
    cursor: usize,
    /// Other end of the selection, if any.
    anchor: Option<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn at_end(&self) -> bool {
        self.cursor == self.text.len()
    }

    /// Selected byte range, if any (never empty).
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        let (s, e) = if a < self.cursor {
            (a, self.cursor)
        } else {
            (self.cursor, a)
        };
        (s < e).then_some(s..e)
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    /// Replaces the whole content, caret at the end.
    pub fn set(&mut self, text: &str) {
        self.snapshot();
        self.text = text.to_owned();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    /// Clears and returns the content (e.g. on submit).
    pub fn take(&mut self) -> String {
        self.undo.clear();
        self.redo.clear();
        self.anchor = None;
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn clear(&mut self) {
        if !self.text.is_empty() {
            self.snapshot();
        }
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    /// Inserts text at the caret, replacing the selection.
    pub fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.snapshot();
        self.delete_selection_raw();
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Replaces `range` with `s` and puts the caret after it.
    pub fn replace_range(&mut self, range: Range<usize>, s: &str) {
        let range = range.start.min(self.text.len())..range.end.min(self.text.len());
        self.snapshot();
        self.text.replace_range(range.clone(), s);
        self.cursor = range.start + s.len();
        self.anchor = None;
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.snapshot();
            self.delete_selection_raw();
        } else if let Some(prev) = self.prev_boundary(self.cursor) {
            self.snapshot();
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    pub fn delete(&mut self) {
        if self.selection().is_some() {
            self.snapshot();
            self.delete_selection_raw();
        } else if let Some(next) = self.next_boundary(self.cursor) {
            self.snapshot();
            self.text.replace_range(self.cursor..next, "");
        }
    }

    pub fn delete_word_back(&mut self) {
        if self.selection().is_some() {
            return self.backspace();
        }
        let start = self.word_start_before(self.cursor);
        if start < self.cursor {
            self.snapshot();
            self.text.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    pub fn delete_word_forward(&mut self) {
        if self.selection().is_some() {
            return self.delete();
        }
        let end = self.word_end_after(self.cursor);
        if end > self.cursor {
            self.snapshot();
            self.text.replace_range(self.cursor..end, "");
        }
    }

    /// Deletes from the caret to the start of the line (Ctrl+U).
    pub fn delete_to_start(&mut self) {
        if self.cursor > 0 {
            self.snapshot();
            self.text.replace_range(..self.cursor, "");
            self.cursor = 0;
            self.anchor = None;
        }
    }

    /// Deletes from the caret to the end of the line (Ctrl+K).
    pub fn delete_to_end(&mut self) {
        if !self.at_end() {
            self.snapshot();
            self.text.truncate(self.cursor);
            self.anchor = None;
        }
    }

    pub fn move_left(&mut self, select: bool) {
        if !select && let Some(sel) = self.selection() {
            self.cursor = sel.start;
            self.anchor = None;
            return;
        }
        let to = self.prev_boundary(self.cursor).unwrap_or(0);
        self.move_to(to, select);
    }

    pub fn move_right(&mut self, select: bool) {
        if !select && let Some(sel) = self.selection() {
            self.cursor = sel.end;
            self.anchor = None;
            return;
        }
        let to = self.next_boundary(self.cursor).unwrap_or(self.text.len());
        self.move_to(to, select);
    }

    pub fn word_left(&mut self, select: bool) {
        let to = self.word_start_before(self.cursor);
        self.move_to(to, select);
    }

    pub fn word_right(&mut self, select: bool) {
        let to = self.word_end_after(self.cursor);
        self.move_to(to, select);
    }

    pub fn home(&mut self, select: bool) {
        self.move_to(0, select);
    }

    pub fn end(&mut self, select: bool) {
        self.move_to(self.text.len(), select);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    pub fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo.pop() {
            self.redo
                .push((std::mem::replace(&mut self.text, text), self.cursor));
            self.cursor = cursor;
            self.anchor = None;
        }
    }

    pub fn redo(&mut self) {
        if let Some((text, cursor)) = self.redo.pop() {
            self.undo
                .push((std::mem::replace(&mut self.text, text), self.cursor));
            self.cursor = cursor;
            self.anchor = None;
        }
    }

    fn move_to(&mut self, to: usize, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = to;
    }

    fn snapshot(&mut self) {
        const MAX_UNDO: usize = 200;
        self.undo.push((self.text.clone(), self.cursor));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn delete_selection_raw(&mut self) {
        if let Some(sel) = self.selection() {
            self.text.replace_range(sel.clone(), "");
            self.cursor = sel.start;
        }
        self.anchor = None;
    }

    fn prev_boundary(&self, i: usize) -> Option<usize> {
        self.text[..i].char_indices().next_back().map(|(p, _)| p)
    }

    fn next_boundary(&self, i: usize) -> Option<usize> {
        self.text[i..].chars().next().map(|c| i + c.len_utf8())
    }

    /// Start of the "word" before `i`: skips whitespace, then a run of the same
    /// class (word characters or punctuation), like most editors' Ctrl+Left.
    fn word_start_before(&self, i: usize) -> usize {
        let mut chars = self.text[..i].char_indices().rev().peekable();
        let mut start = i;
        while let Some((p, _)) = chars.next_if(|(_, c)| class(*c) == Class::Space) {
            start = p;
        }
        let Some(&(_, first)) = chars.peek() else {
            return start;
        };
        let cls = class(first);
        while let Some((p, _)) = chars.next_if(|(_, c)| class(*c) == cls) {
            start = p;
        }
        start
    }

    /// End of the "word" after `i` (mirror of [`Self::word_start_before`]).
    fn word_end_after(&self, i: usize) -> usize {
        let mut chars = self.text[i..].char_indices().peekable();
        let mut end = i;
        while let Some((p, c)) = chars.next_if(|(_, c)| class(*c) == Class::Space) {
            end = i + p + c.len_utf8();
        }
        let Some(&(_, first)) = chars.peek() else {
            return end;
        };
        let cls = class(first);
        while let Some((p, c)) = chars.next_if(|(_, c)| class(*c) == cls) {
            end = i + p + c.len_utf8();
        }
        end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Punct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str) -> LineEditor {
        let mut e = LineEditor::new();
        e.set(text);
        e
    }

    #[test]
    fn insert_and_backspace_unicode() {
        let mut e = LineEditor::new();
        e.insert("añ");
        e.insert("你");
        assert_eq!(e.text(), "añ你");
        e.backspace();
        assert_eq!(e.text(), "añ");
        e.move_left(false);
        e.backspace();
        assert_eq!(e.text(), "ñ");
        assert_eq!(e.cursor(), 0);
    }

    #[test]
    fn word_movement() {
        let mut e = ed("git checkout --force main");
        e.word_left(false);
        assert_eq!(&e.text()[e.cursor()..], "main");
        e.word_left(false);
        assert_eq!(&e.text()[e.cursor()..], "force main");
        e.home(false);
        e.word_right(false);
        assert_eq!(e.cursor(), 3);
    }

    #[test]
    fn delete_word_back() {
        let mut e = ed("cargo build --release");
        e.delete_word_back();
        assert_eq!(e.text(), "cargo build --");
        e.delete_word_back();
        assert_eq!(e.text(), "cargo build ");
    }

    #[test]
    fn selection_replace() {
        let mut e = ed("hello world");
        e.word_left(true);
        assert_eq!(e.selected_text(), Some("world"));
        e.insert("there");
        assert_eq!(e.text(), "hello there");
    }

    #[test]
    fn undo_redo() {
        let mut e = LineEditor::new();
        e.insert("a");
        e.insert("b");
        e.undo();
        assert_eq!(e.text(), "a");
        e.redo();
        assert_eq!(e.text(), "ab");
    }

    #[test]
    fn kill_line() {
        let mut e = ed("abc def");
        e.word_left(false);
        e.delete_to_end();
        assert_eq!(e.text(), "abc ");
        e.delete_to_start();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn replace_range_moves_caret() {
        let mut e = ed("git ch");
        e.replace_range(4..6, "checkout ");
        assert_eq!(e.text(), "git checkout ");
        assert!(e.at_end());
    }
}
