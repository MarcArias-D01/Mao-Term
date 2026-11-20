//! Wrapping logical lines into screen rows, and mapping between the two.

use crate::model::{Line, TextDoc, char_width};

/// One screen row: bytes `start..end` of line `line`'s content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub line: u32,
    pub start: u32,
    pub end: u32,
    /// A continuation of the previous row (wrapped).
    pub cont: bool,
}

/// Splits every line of `doc` into rows at most `width` cells wide (word
/// wrapping). With `wrap` off each line is one row (except nothing changes).
pub fn layout(doc: &TextDoc, width: usize, wrap: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(doc.lines.len());
    for (i, line) in doc.lines.iter().enumerate() {
        if !wrap || line.nowrap || line.rule {
            rows.push(Row {
                line: i as u32,
                start: 0,
                end: line.content.text.len() as u32,
                cont: false,
            });
        } else {
            wrap_line(i as u32, line, width, &mut rows);
        }
    }
    rows
}

fn wrap_line(index: u32, line: &Line, width: usize, out: &mut Vec<Row>) {
    let text = &line.content.text;
    let indent = line.cont.width();
    let width = width.max(1);
    let mut start = 0usize;
    let mut cont = false;
    loop {
        let avail = if cont {
            width.saturating_sub(indent).max(1)
        } else {
            width
        };
        let mut col = 0;
        let mut end = text.len();
        // Byte offset just after the last space seen on this row.
        let mut last_break = None;
        for (off, c) in text[start..].char_indices() {
            let at = start + off;
            let w = char_width(c);
            if col + w > avail {
                end = match last_break {
                    // A space doesn't need room at the end of a row.
                    _ if c == ' ' => at + 1,
                    Some(b) if b > start => b,
                    // No space on the row: break mid-word (at least one char).
                    _ if at > start => at,
                    _ => at + c.len_utf8(),
                };
                break;
            }
            col += w;
            if c == ' ' {
                last_break = Some(at + 1);
            }
        }
        out.push(Row {
            line: index,
            start: start as u32,
            end: end as u32,
            cont,
        });
        if end >= text.len() {
            break;
        }
        start = end;
        cont = true;
    }
}

/// Byte offset in `text[start..end]` at display column `col` (clamped to `end`).
pub fn byte_at_col(text: &str, start: usize, end: usize, col: usize) -> usize {
    let mut x = 0;
    for (off, c) in text[start..end].char_indices() {
        let w = char_width(c);
        if x + w > col {
            // Past the middle of a wide char → after it.
            return start + off;
        }
        x += w;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::StyledText;

    fn doc(lines: &[&str]) -> TextDoc {
        TextDoc {
            lines: lines
                .iter()
                .map(|l| Line::new(StyledText::plain(*l)))
                .collect(),
            gutter: false,
        }
    }

    fn texts(d: &TextDoc, rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                d.lines[r.line as usize].content.text[r.start as usize..r.end as usize].to_owned()
            })
            .collect()
    }

    #[test]
    fn wraps_at_spaces() {
        let d = doc(&["hello brave new world"]);
        let rows = layout(&d, 11, true);
        assert_eq!(texts(&d, &rows), ["hello brave ", "new world"]);
        assert!(rows[1].cont);
    }

    #[test]
    fn breaks_long_words_and_keeps_empty_lines() {
        let d = doc(&["abcdefghij", ""]);
        let rows = layout(&d, 4, true);
        assert_eq!(texts(&d, &rows), ["abcd", "efgh", "ij", ""]);
    }

    #[test]
    fn hanging_indent_reduces_width() {
        let mut d = doc(&["- aaa bbb ccc"]);
        d.lines[0].cont = StyledText::plain("  ");
        let rows = layout(&d, 8, true);
        assert_eq!(texts(&d, &rows), ["- aaa ", "bbb ", "ccc"]);
    }

    #[test]
    fn wide_chars_count_double() {
        let d = doc(&["日本語の文"]);
        let rows = layout(&d, 4, true);
        assert_eq!(texts(&d, &rows), ["日本", "語の", "文"]);
    }

    #[test]
    fn no_wrap_is_one_row_per_line() {
        let d = doc(&["a very long line indeed", "b"]);
        assert_eq!(layout(&d, 3, false).len(), 2);
    }

    #[test]
    fn column_to_byte() {
        assert_eq!(byte_at_col("a日b", 0, 5, 0), 0);
        assert_eq!(byte_at_col("a日b", 0, 5, 1), 1);
        assert_eq!(byte_at_col("a日b", 0, 5, 2), 1);
        assert_eq!(byte_at_col("a日b", 0, 5, 3), 4);
        assert_eq!(byte_at_col("a日b", 0, 5, 9), 5);
    }
}
