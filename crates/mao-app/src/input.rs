//! Maps winit input to terminal input and app shortcuts.

use mao_terminal::keys::{Key, Modifiers, NamedKey};
use winit::event::KeyEvent;
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey as WNamed};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        alt: state.alt_key(),
        ctrl: state.control_key(),
    }
}

/// App-level actions bound to keys. Resolved before anything reaches the PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Copy,
    /// Copy if there is a selection, otherwise send Ctrl+C to the shell.
    CopyOrInterrupt,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollPageUp,
    ScrollPageDown,
}

pub fn shortcut(event: &KeyEvent, mods: Modifiers) -> Option<Shortcut> {
    let base = event.key_without_modifiers();
    let ctrl_shift = mods.ctrl && mods.shift && !mods.alt;
    let ctrl_only = mods.ctrl && !mods.shift && !mods.alt;
    match &base {
        WKey::Character(c) => {
            let c = c.as_str();
            match () {
                _ if ctrl_shift && c.eq_ignore_ascii_case("c") => Some(Shortcut::Copy),
                _ if ctrl_shift && c.eq_ignore_ascii_case("v") => Some(Shortcut::Paste),
                _ if ctrl_only && c.eq_ignore_ascii_case("c") => Some(Shortcut::CopyOrInterrupt),
                _ if mods.ctrl && !mods.alt && (c == "=" || c == "+") => Some(Shortcut::ZoomIn),
                _ if ctrl_only && c == "-" => Some(Shortcut::ZoomOut),
                _ if ctrl_only && c == "0" => Some(Shortcut::ZoomReset),
                _ => None,
            }
        }
        WKey::Named(WNamed::Insert) if mods.shift && !mods.ctrl => Some(Shortcut::Paste),
        WKey::Named(WNamed::PageUp) if mods.shift && !mods.ctrl => Some(Shortcut::ScrollPageUp),
        WKey::Named(WNamed::PageDown) if mods.shift && !mods.ctrl => Some(Shortcut::ScrollPageDown),
        _ => None,
    }
}

/// Maps a key press to a terminal key, if it produces input.
pub fn terminal_key(event: &KeyEvent) -> Option<Key<'_>> {
    let base = match event.key_without_modifiers() {
        WKey::Character(c) => c.chars().next(),
        WKey::Named(WNamed::Space) => Some(' '),
        _ => None,
    };
    let named = match &event.logical_key {
        WKey::Named(named) => named,
        WKey::Character(c) => {
            let text = event.text.as_deref().unwrap_or(c.as_str());
            return Some(Key::Text { text, base });
        }
        // Unidentified keys can still carry text: injected Unicode (VK_PACKET on
        // Windows), some IME/compose paths. Dead keys carry none until composed.
        _ => {
            let text = event.text.as_deref()?;
            return Some(Key::Text { text, base });
        }
    };
    let key = match named {
        WNamed::Space => {
            return Some(Key::Text {
                text: event.text.as_deref().unwrap_or(" "),
                base,
            });
        }
        WNamed::Enter => NamedKey::Enter,
        WNamed::Backspace => NamedKey::Backspace,
        WNamed::Tab => NamedKey::Tab,
        WNamed::Escape => NamedKey::Escape,
        WNamed::ArrowUp => NamedKey::Up,
        WNamed::ArrowDown => NamedKey::Down,
        WNamed::ArrowLeft => NamedKey::Left,
        WNamed::ArrowRight => NamedKey::Right,
        WNamed::Home => NamedKey::Home,
        WNamed::End => NamedKey::End,
        WNamed::Insert => NamedKey::Insert,
        WNamed::Delete => NamedKey::Delete,
        WNamed::PageUp => NamedKey::PageUp,
        WNamed::PageDown => NamedKey::PageDown,
        WNamed::F1 => NamedKey::F(1),
        WNamed::F2 => NamedKey::F(2),
        WNamed::F3 => NamedKey::F(3),
        WNamed::F4 => NamedKey::F(4),
        WNamed::F5 => NamedKey::F(5),
        WNamed::F6 => NamedKey::F(6),
        WNamed::F7 => NamedKey::F(7),
        WNamed::F8 => NamedKey::F(8),
        WNamed::F9 => NamedKey::F(9),
        WNamed::F10 => NamedKey::F(10),
        WNamed::F11 => NamedKey::F(11),
        WNamed::F12 => NamedKey::F(12),
        // Other named keys (media, modifiers alone…) may still carry text.
        _ => {
            let text = event.text.as_deref()?;
            return Some(Key::Text { text, base });
        }
    };
    Some(Key::Named(key))
}
