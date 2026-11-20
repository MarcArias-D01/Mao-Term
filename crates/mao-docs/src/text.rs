//! Decoding file bytes into displayable lines.

use crate::model::{Line, StyledText, TextDoc, char_width};

const TAB_WIDTH: usize = 4;

/// Decodes text (UTF-8, or UTF-16 with a BOM). `None` if it looks binary.
pub fn decode(bytes: &[u8]) -> Option<String> {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if be {
                    u16::from_be_bytes(c)
                } else {
                    u16::from_le_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => return Some(utf16(false)),
        [0xFE, 0xFF, ..] => return Some(utf16(true)),
        _ => {}
    }
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Expands tabs and makes control characters visible (`␛`, `␀`…).
pub fn clean_line(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        match c {
            '\t' => {
                let n = TAB_WIDTH - col % TAB_WIDTH;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\u{0}'..='\u{1f}' => {
                out.push(char::from_u32(0x2400 + c as u32).unwrap_or('?'));
                col += 1;
            }
            '\u{7f}' => {
                out.push('␡');
                col += 1;
            }
            _ => {
                out.push(c);
                col += char_width(c);
            }
        }
    }
    out
}

/// Splits text into cleaned lines (a final newline doesn't add an empty line).
pub fn lines(text: &str) -> Vec<String> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.split('\n').map(clean_line).collect()
}

pub fn plain_doc(lines: &[String]) -> TextDoc {
    TextDoc {
        lines: lines
            .iter()
            .map(|l| Line::new(StyledText::plain(l.as_str())))
            .collect(),
        gutter: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_binary_and_decodes_utf16() {
        assert_eq!(decode(b"\x7fELF\x00\x01"), None);
        assert_eq!(decode(b"\xEF\xBB\xBFhi").as_deref(), Some("hi"));
        assert_eq!(decode(b"\xFF\xFEh\x00i\x00").as_deref(), Some("hi"));
        assert_eq!(decode(b"caf\xC3\xA9").as_deref(), Some("café"));
    }

    #[test]
    fn cleans_tabs_and_controls() {
        assert_eq!(clean_line("a\tb"), "a   b");
        assert_eq!(clean_line("\tx\r"), "    x");
        assert_eq!(clean_line("\x1b[0m"), "␛[0m");
    }

    #[test]
    fn splits_lines() {
        assert_eq!(lines("a\r\nb\n"), ["a", "b"]);
        assert_eq!(lines(""), [""]);
    }
}
