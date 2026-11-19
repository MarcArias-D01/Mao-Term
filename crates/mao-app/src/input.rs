//! Maps winit input to app actions and terminal input.

use mao_terminal::keys::{Key, Modifiers, NamedKey};
use mao_ui::Direction;
use winit::event::KeyEvent;
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey as WNamed};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use crate::actions::Action;
use crate::sidebar::Panel;

pub fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        alt: state.alt_key(),
        ctrl: state.control_key(),
    }
}

/// Default keybindings. Resolved before anything reaches the PTY.
pub fn action_for(event: &KeyEvent, mods: Modifiers) -> Option<Action> {
    let Modifiers { shift, alt, ctrl } = mods;
    let ctrl_shift = ctrl && shift && !alt;
    let ctrl_only = ctrl && !shift && !alt;
    let alt_only = alt && !ctrl && !shift;
    let shift_only = shift && !ctrl && !alt;

    match event.key_without_modifiers() {
        WKey::Character(c) => {
            let c = c.as_str().to_ascii_lowercase();
            let action = match c.as_str() {
                "p" if ctrl_shift => Action::CommandPalette,
                "t" if ctrl_shift => Action::NewTab,
                "w" if ctrl_shift => Action::ClosePane,
                "d" if ctrl_shift => Action::SplitRight,
                "e" if ctrl_shift => Action::SplitDown,
                "b" if ctrl_shift => Action::ToggleSidebar,
                "1" if ctrl_shift => Action::ShowPanel(Panel::Files),
                "2" if ctrl_shift => Action::ShowPanel(Panel::Agents),
                "3" if ctrl_shift => Action::ShowPanel(Panel::Monitor),
                "," if ctrl_only => Action::OpenSettings,
                "c" if ctrl_shift => Action::Copy,
                "v" if ctrl_shift => Action::Paste,
                "c" if ctrl_only => Action::CopyOrInterrupt,
                "=" | "+" if ctrl && !alt => Action::FontBigger,
                "-" if ctrl_only => Action::FontSmaller,
                "0" if ctrl_only => Action::FontReset,
                d if alt_only && d.len() == 1 && ("1"..="9").contains(&d) => {
                    Action::GoToTab(usize::from(d.as_bytes()[0] - b'1'))
                }
                _ => return None,
            };
            Some(action)
        }
        WKey::Named(named) => Some(match named {
            WNamed::Tab if ctrl_shift => Action::PrevTab,
            WNamed::Tab if ctrl_only => Action::NextTab,
            WNamed::Enter if ctrl_shift => Action::ToggleZoomPane,
            WNamed::ArrowLeft if alt_only => Action::Focus(Direction::Left),
            WNamed::ArrowRight if alt_only => Action::Focus(Direction::Right),
            WNamed::ArrowUp if alt_only => Action::Focus(Direction::Up),
            WNamed::ArrowDown if alt_only => Action::Focus(Direction::Down),
            WNamed::Insert if shift_only => Action::Paste,
            WNamed::PageUp if shift_only => Action::ScrollPageUp,
            WNamed::PageDown if shift_only => Action::ScrollPageDown,
            WNamed::Home if shift_only => Action::ScrollToTop,
            WNamed::End if shift_only => Action::ScrollToBottom,
            _ => return None,
        }),
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
