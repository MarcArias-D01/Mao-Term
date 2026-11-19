//! Lays the prompt editor out as grid cells drawn over the terminal, right
//! where the shell's prompt ends.

use mao_config::ThemeConfig;
use mao_render::{CellStyle, GridCell, Rgb8};
use unicode_width::UnicodeWidthChar;

use crate::editor::{Highlight, PromptEditor, highlight};

/// Colors for the editor overlay, derived from the theme.
#[derive(Debug, Clone, Copy)]
pub struct OverlayColors {
    pub fg: Rgb8,
    pub bg: Rgb8,
    pub command: Rgb8,
    pub option: Rgb8,
    pub string: Rgb8,
    pub variable: Rgb8,
    pub operator: Rgb8,
    pub ghost: Rgb8,
    pub selection: Rgb8,
    pub cursor: Rgb8,
}

impl OverlayColors {
    pub fn new(theme: &ThemeConfig) -> Self {
        let a = |i: usize| theme.ansi[i].to_array();
        let fg = theme.foreground.to_array();
        let bg = theme.background.to_array();
        let mix = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
        Self {
            fg,
            bg,
            command: a(11),
            option: a(8),
            string: a(6),
            variable: a(10),
            operator: a(8),
            ghost: [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2])],
            selection: theme.selection.to_array(),
            cursor: theme.cursor.to_array(),
        }
    }
}

/// The editor laid out on the grid.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    /// (row, col, cell) to draw over the terminal.
    pub cells: Vec<(usize, usize, GridCell)>,
    /// Rows touched (to redraw them when the overlay moves or disappears).
    pub rows: Vec<usize>,
    /// Caret cell.
    pub caret: (usize, usize),
    /// Cell where the completion menu anchors (start of the completed token).
    pub menu_anchor: (usize, usize),
}

/// Lays out `editor` starting at `start` on a `cols` × `rows` grid.
pub fn layout(
    editor: &PromptEditor,
    start: (usize, usize),
    cols: usize,
    rows: usize,
    c: &OverlayColors,
) -> Overlay {
    let text = editor.line.text();
    let classes = highlight(text);
    let selection = editor.line.selection();
    let cursor = editor.line.cursor();
    let menu_byte = editor.menu.as_ref().map(|m| m.completions.range.start);

    let mut out = Overlay {
        caret: start,
        menu_anchor: start,
        ..Overlay::default()
    };
    let (mut row, mut col) = start;
    let cols = cols.max(1);

    let place = |out: &mut Overlay,
                 ch: char,
                 fg: Rgb8,
                 bg: Rgb8,
                 row: &mut usize,
                 col: &mut usize|
     -> bool {
        let w = ch.width().unwrap_or(0).max(1);
        if *col + w > cols {
            *row += 1;
            *col = 0;
        }
        if *row >= rows {
            return false;
        }
        out.cells.push((*row, *col, cell(ch, fg, bg, false)));
        if w == 2 && *col + 1 < cols {
            out.cells.push((*row, *col + 1, cell(' ', fg, bg, true)));
        }
        *col += w;
        true
    };

    for (i, ch) in text.char_indices() {
        if i == cursor {
            out.caret = wrap_point(row, col, cols);
        }
        if menu_byte == Some(i) {
            out.menu_anchor = wrap_point(row, col, cols);
        }
        let fg = match classes[i] {
            Highlight::Command => c.command,
            Highlight::Option => c.option,
            Highlight::String => c.string,
            Highlight::Variable => c.variable,
            Highlight::Operator => c.operator,
            Highlight::Plain => c.fg,
        };
        let bg = if selection.as_ref().is_some_and(|s| s.contains(&i)) {
            c.selection
        } else {
            c.bg
        };
        if !place(&mut out, ch, fg, bg, &mut row, &mut col) {
            break;
        }
    }
    if cursor >= text.len() {
        out.caret = wrap_point(row, col, cols);
    }
    if menu_byte.is_some_and(|b| b >= text.len()) {
        out.menu_anchor = wrap_point(row, col, cols);
    }
    if let Some(ghost) = &editor.ghost {
        for ch in ghost.chars() {
            if !place(&mut out, ch, c.ghost, c.bg, &mut row, &mut col) {
                break;
            }
        }
    }

    out.caret.0 = out.caret.0.min(rows.saturating_sub(1));
    let last = row.min(rows.saturating_sub(1)).max(out.caret.0);
    out.rows = (start.0..=last).collect();
    out
}

/// A caret at the end of a full line sits at the start of the next one.
fn wrap_point(row: usize, col: usize, cols: usize) -> (usize, usize) {
    if col >= cols {
        (row + 1, 0)
    } else {
        (row, col)
    }
}

fn cell(ch: char, fg: Rgb8, bg: Rgb8, spacer: bool) -> GridCell {
    GridCell {
        ch,
        fg,
        bg,
        style: CellStyle {
            spacer,
            ..CellStyle::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> OverlayColors {
        OverlayColors::new(&ThemeConfig::default())
    }

    fn editor(text: &str) -> PromptEditor {
        let mut e = PromptEditor::default();
        e.line.set(text);
        e
    }

    #[test]
    fn starts_after_prompt_and_tracks_caret() {
        let o = layout(&editor("ls"), (3, 10), 80, 24, &colors());
        assert_eq!(o.cells[0].0, 3);
        assert_eq!(o.cells[0].1, 10);
        assert_eq!(o.caret, (3, 12));
        assert_eq!(o.rows, vec![3]);
    }

    #[test]
    fn wraps_and_handles_wide_chars() {
        let o = layout(&editor("ab你"), (0, 2), 5, 24, &colors());
        // "a" at 2, "b" at 3, "你" doesn't fit at 4 → next row.
        assert_eq!((o.cells[2].0, o.cells[2].1), (1, 0));
        assert!(o.cells[3].2.style.spacer);
        assert_eq!(o.caret, (1, 2));
        assert_eq!(o.rows, vec![0, 1]);
    }

    #[test]
    fn ghost_text_is_dim_and_after_caret() {
        let mut e = editor("git pu");
        e.ghost = Some("sh".into());
        let c = colors();
        let o = layout(&e, (0, 0), 80, 24, &c);
        assert_eq!(o.caret, (0, 6));
        assert_eq!(o.cells.last().map(|x| x.2.fg), Some(c.ghost));
        assert_eq!(o.cells.len(), 8);
    }
}
