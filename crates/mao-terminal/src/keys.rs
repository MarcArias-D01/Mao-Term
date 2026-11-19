//! Keyboard → bytes encoding (xterm conventions).
//!
//! Independent of the windowing library: the app maps its key events to [`Key`].
//! See `docs/skills/terminal-emulation/SKILL.md` → Keyboard.

/// Modifier state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Modifiers {
    fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }

    /// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// Keys that produce special sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    /// F1..=F12.
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key<'a> {
    Named(NamedKey),
    /// Text produced by the key, plus the unmodified character (used for Ctrl combos).
    Text {
        text: &'a str,
        base: Option<char>,
    },
}

/// Terminal modes that change key encoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM: arrows/home/end send `ESC O x` instead of `ESC [ x`.
    pub app_cursor: bool,
}

/// Encodes a key press into `out` (cleared first). Returns `false` if the key produces nothing.
pub fn encode(key: Key<'_>, mods: Modifiers, modes: KeyModes, out: &mut Vec<u8>) -> bool {
    out.clear();
    match key {
        Key::Named(named) => encode_named(named, mods, modes, out),
        Key::Text { text, base } => encode_text(text, base, mods, out),
    }
    !out.is_empty()
}

fn encode_text(text: &str, base: Option<char>, mods: Modifiers, out: &mut Vec<u8>) {
    // AltGr is reported as Ctrl+Alt on Windows. If the key produced a printable
    // character different from the unmodified one (e.g. AltGr+2 = '@' on a Spanish
    // layout), send that character as-is.
    let altgr_text = mods.ctrl
        && mods.alt
        && !text.is_empty()
        && !text.chars().any(char::is_control)
        && text.chars().next() != base;
    if altgr_text {
        out.extend_from_slice(text.as_bytes());
        return;
    }
    if mods.ctrl
        && let Some(byte) = base.and_then(|c| ctrl_byte(c, mods.shift))
    {
        if mods.alt {
            out.push(0x1b);
        }
        out.push(byte);
        return;
    }
    // Ignore control characters the platform may put in `text` for Ctrl combos we don't map.
    if text.chars().any(char::is_control) {
        return;
    }
    if mods.alt {
        out.push(0x1b);
    }
    out.extend_from_slice(text.as_bytes());
}

/// Control byte for `Ctrl+c`.
fn ctrl_byte(c: char, shift: bool) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' if !shift => c as u8 & 0x1f,
        ' ' | '2' | '@' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' | '-' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

fn encode_named(key: NamedKey, mods: Modifiers, modes: KeyModes, out: &mut Vec<u8>) {
    let esc_prefix = |out: &mut Vec<u8>| {
        if mods.alt {
            out.push(0x1b);
        }
    };
    match key {
        NamedKey::Enter => {
            esc_prefix(out);
            out.push(b'\r');
        }
        NamedKey::Backspace => {
            esc_prefix(out);
            out.push(if mods.ctrl { 0x08 } else { 0x7f });
        }
        NamedKey::Tab if mods.shift => out.extend_from_slice(b"\x1b[Z"),
        NamedKey::Tab => {
            esc_prefix(out);
            out.push(b'\t');
        }
        NamedKey::Escape => {
            esc_prefix(out);
            out.push(0x1b);
        }
        NamedKey::Up => cursor_key(b'A', mods, modes, out),
        NamedKey::Down => cursor_key(b'B', mods, modes, out),
        NamedKey::Right => cursor_key(b'C', mods, modes, out),
        NamedKey::Left => cursor_key(b'D', mods, modes, out),
        NamedKey::Home => cursor_key(b'H', mods, modes, out),
        NamedKey::End => cursor_key(b'F', mods, modes, out),
        NamedKey::Insert => tilde_key(2, mods, out),
        NamedKey::Delete => tilde_key(3, mods, out),
        NamedKey::PageUp => tilde_key(5, mods, out),
        NamedKey::PageDown => tilde_key(6, mods, out),
        NamedKey::F(n @ 1..=4) => {
            let final_byte = b'P' + (n - 1);
            if mods.any() {
                csi_mod(1, mods, final_byte, out);
            } else {
                out.extend_from_slice(&[0x1b, b'O', final_byte]);
            }
        }
        NamedKey::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(CODES[usize::from(n - 5)], mods, out);
        }
        NamedKey::F(_) => {}
    }
}

/// Arrows, Home, End: `ESC [ x`, `ESC O x` (app cursor) or `ESC [ 1 ; m x`.
fn cursor_key(final_byte: u8, mods: Modifiers, modes: KeyModes, out: &mut Vec<u8>) {
    if mods.any() {
        csi_mod(1, mods, final_byte, out);
    } else if modes.app_cursor {
        out.extend_from_slice(&[0x1b, b'O', final_byte]);
    } else {
        out.extend_from_slice(&[0x1b, b'[', final_byte]);
    }
}

/// `ESC [ n ~` or `ESC [ n ; m ~`.
fn tilde_key(n: u8, mods: Modifiers, out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[");
    push_number(n, out);
    if mods.any() {
        out.push(b';');
        push_number(mods.param(), out);
    }
    out.push(b'~');
}

/// `ESC [ n ; m x`.
fn csi_mod(n: u8, mods: Modifiers, final_byte: u8, out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[");
    push_number(n, out);
    out.push(b';');
    push_number(mods.param(), out);
    out.push(final_byte);
}

fn push_number(n: u8, out: &mut Vec<u8>) {
    if n >= 10 {
        out.push(b'0' + n / 10);
    }
    out.push(b'0' + n % 10);
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        alt: false,
        ctrl: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        alt: false,
        ctrl: true,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        alt: true,
        ctrl: false,
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        alt: false,
        ctrl: false,
    };

    fn enc(key: Key<'_>, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
        let mut out = Vec::new();
        encode(key, mods, KeyModes { app_cursor }, &mut out);
        out
    }

    fn named(k: NamedKey, mods: Modifiers) -> Vec<u8> {
        enc(Key::Named(k), mods, false)
    }

    fn text(t: &str, mods: Modifiers) -> Vec<u8> {
        enc(
            Key::Text {
                text: t,
                base: t.chars().next(),
            },
            mods,
            false,
        )
    }

    #[test]
    fn plain_text() {
        assert_eq!(text("a", NONE), b"a");
        assert_eq!(text("ñ", NONE), "ñ".as_bytes());
        assert_eq!(text("a", ALT), b"\x1ba");
    }

    #[test]
    fn ctrl_combos() {
        assert_eq!(text("c", CTRL), [0x03]);
        assert_eq!(text("C", CTRL), [0x03]);
        assert_eq!(text(" ", CTRL), [0x00]);
        assert_eq!(text("[", CTRL), [0x1b]);
        // Control chars from the platform are not echoed back.
        assert_eq!(
            enc(
                Key::Text {
                    text: "\u{3}",
                    base: None
                },
                CTRL,
                false
            ),
            b""
        );
    }

    #[test]
    fn altgr_produces_text() {
        let ctrl_alt = Modifiers {
            shift: false,
            alt: true,
            ctrl: true,
        };
        let altgr_2 = Key::Text {
            text: "@",
            base: Some('2'),
        };
        assert_eq!(enc(altgr_2, ctrl_alt, false), b"@");
        // Real Ctrl+Alt+letter still sends ESC + control byte.
        let ctrl_alt_a = Key::Text {
            text: "a",
            base: Some('a'),
        };
        assert_eq!(enc(ctrl_alt_a, ctrl_alt, false), [0x1b, 0x01]);
    }

    #[test]
    fn arrows() {
        assert_eq!(named(NamedKey::Up, NONE), b"\x1b[A");
        assert_eq!(enc(Key::Named(NamedKey::Up), NONE, true), b"\x1bOA");
        assert_eq!(named(NamedKey::Left, CTRL), b"\x1b[1;5D");
        assert_eq!(named(NamedKey::Right, SHIFT), b"\x1b[1;2C");
    }

    #[test]
    fn editing_keys() {
        assert_eq!(named(NamedKey::Enter, NONE), b"\r");
        assert_eq!(named(NamedKey::Backspace, NONE), [0x7f]);
        assert_eq!(named(NamedKey::Backspace, CTRL), [0x08]);
        assert_eq!(named(NamedKey::Tab, SHIFT), b"\x1b[Z");
        assert_eq!(named(NamedKey::Delete, NONE), b"\x1b[3~");
        assert_eq!(named(NamedKey::PageUp, CTRL), b"\x1b[5;5~");
    }

    #[test]
    fn function_keys() {
        assert_eq!(named(NamedKey::F(1), NONE), b"\x1bOP");
        assert_eq!(named(NamedKey::F(4), SHIFT), b"\x1b[1;2S");
        assert_eq!(named(NamedKey::F(5), NONE), b"\x1b[15~");
        assert_eq!(named(NamedKey::F(12), NONE), b"\x1b[24~");
    }
}
