//! Mouse reporting to applications (xterm protocols: normal/X10 and SGR 1006).

use crate::keys::Modifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// Pointer moved to a new cell; `button` is the one held, if any.
    Motion,
}

/// Which mouse events the application asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseModes {
    /// Any reporting enabled (clicks at least).
    pub report: bool,
    /// Report motion while a button is held (1002).
    pub drag: bool,
    /// Report all motion (1003).
    pub motion: bool,
    /// SGR extended coordinates (1006).
    pub sgr: bool,
}

impl MouseModes {
    /// Whether a motion event should be reported.
    pub fn wants_motion(self, button_held: bool) -> bool {
        self.motion || (self.drag && button_held)
    }
}

/// Encodes a mouse event at a 0-based cell into `out` (cleared first).
/// `button` is `None` only for motion with no button held.
/// Returns `false` if nothing should be sent.
pub fn encode(
    button: Option<MouseButton>,
    action: MouseAction,
    mods: Modifiers,
    row: usize,
    col: usize,
    modes: MouseModes,
    out: &mut Vec<u8>,
) -> bool {
    out.clear();
    if !modes.report {
        return false;
    }
    let mut code: u32 = match button {
        Some(MouseButton::Left) => 0,
        Some(MouseButton::Middle) => 1,
        Some(MouseButton::Right) => 2,
        Some(MouseButton::WheelUp) => 64,
        Some(MouseButton::WheelDown) => 65,
        None => 3,
    };
    if action == MouseAction::Motion {
        code += 32;
    }
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }
    let (x, y) = (col + 1, row + 1);

    if modes.sgr {
        let final_byte = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        out.extend_from_slice(format!("\x1b[<{code};{x};{y}{final_byte}").as_bytes());
        return true;
    }

    // Normal mode: release is button 3 (which button is unknown); coordinates
    // are single bytes offset by 32, so positions beyond 223 can't be reported.
    if action == MouseAction::Release {
        code = (code & !0b11) | 3;
    }
    let (Ok(cb), Ok(cx), Ok(cy)) = (
        u8::try_from(32 + code),
        u8::try_from(32 + x),
        u8::try_from(32 + y),
    ) else {
        return false;
    };
    out.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const SGR: MouseModes = MouseModes {
        report: true,
        drag: true,
        motion: false,
        sgr: true,
    };
    const NORMAL: MouseModes = MouseModes {
        report: true,
        drag: false,
        motion: false,
        sgr: false,
    };

    fn enc(
        button: Option<MouseButton>,
        action: MouseAction,
        mods: Modifiers,
        row: usize,
        col: usize,
        modes: MouseModes,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        encode(button, action, mods, row, col, modes, &mut out);
        out
    }

    #[test]
    fn sgr_press_release() {
        let none = Modifiers::default();
        assert_eq!(
            enc(Some(MouseButton::Left), MouseAction::Press, none, 0, 0, SGR),
            b"\x1b[<0;1;1M"
        );
        assert_eq!(
            enc(
                Some(MouseButton::Left),
                MouseAction::Release,
                none,
                4,
                9,
                SGR
            ),
            b"\x1b[<0;10;5m"
        );
        assert_eq!(
            enc(
                Some(MouseButton::WheelDown),
                MouseAction::Press,
                none,
                0,
                0,
                SGR
            ),
            b"\x1b[<65;1;1M"
        );
    }

    #[test]
    fn sgr_drag_with_ctrl() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert_eq!(
            enc(
                Some(MouseButton::Left),
                MouseAction::Motion,
                ctrl,
                1,
                2,
                SGR
            ),
            b"\x1b[<48;3;2M"
        );
    }

    #[test]
    fn normal_mode() {
        let none = Modifiers::default();
        assert_eq!(
            enc(
                Some(MouseButton::Right),
                MouseAction::Press,
                none,
                0,
                0,
                NORMAL
            ),
            [0x1b, b'[', b'M', 34, 33, 33]
        );
        assert_eq!(
            enc(
                Some(MouseButton::Right),
                MouseAction::Release,
                none,
                0,
                0,
                NORMAL
            ),
            [0x1b, b'[', b'M', 35, 33, 33]
        );
        // Out of range for the single-byte encoding.
        assert!(
            enc(
                Some(MouseButton::Left),
                MouseAction::Press,
                none,
                0,
                300,
                NORMAL
            )
            .is_empty()
        );
    }

    #[test]
    fn disabled_sends_nothing() {
        assert!(
            enc(
                Some(MouseButton::Left),
                MouseAction::Press,
                Modifiers::default(),
                0,
                0,
                MouseModes::default()
            )
            .is_empty()
        );
    }
}
