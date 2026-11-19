//! Every user-invokable action. Keybindings and the command palette share this list.

use mao_ui::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CommandPalette,
    NewTab,
    ClosePane,
    CloseTab,
    NextTab,
    PrevTab,
    /// 0-based tab index.
    GoToTab(usize),
    SplitRight,
    SplitDown,
    Focus(Direction),
    ToggleZoomPane,
    Copy,
    /// Copy when there is a selection, otherwise send Ctrl+C.
    CopyOrInterrupt,
    Paste,
    FontBigger,
    FontSmaller,
    FontReset,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    Quit,
}

impl Action {
    /// Human-readable name, shown in the command palette.
    pub fn name(self) -> &'static str {
        match self {
            Self::CommandPalette => "Show Command Palette",
            Self::NewTab => "New Tab",
            Self::ClosePane => "Close Pane",
            Self::CloseTab => "Close Tab",
            Self::NextTab => "Next Tab",
            Self::PrevTab => "Previous Tab",
            Self::GoToTab(_) => "Go to Tab",
            Self::SplitRight => "Split Pane Right",
            Self::SplitDown => "Split Pane Down",
            Self::Focus(Direction::Left) => "Focus Pane Left",
            Self::Focus(Direction::Right) => "Focus Pane Right",
            Self::Focus(Direction::Up) => "Focus Pane Up",
            Self::Focus(Direction::Down) => "Focus Pane Down",
            Self::ToggleZoomPane => "Toggle Maximize Pane",
            Self::Copy => "Copy",
            Self::CopyOrInterrupt => "Copy or Interrupt",
            Self::Paste => "Paste",
            Self::FontBigger => "Increase Font Size",
            Self::FontSmaller => "Decrease Font Size",
            Self::FontReset => "Reset Font Size",
            Self::ScrollPageUp => "Scroll Page Up",
            Self::ScrollPageDown => "Scroll Page Down",
            Self::ScrollToTop => "Scroll to Top",
            Self::ScrollToBottom => "Scroll to Bottom",
            Self::Quit => "Quit Mao-Term",
        }
    }

    /// Default shortcut, for display.
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::CommandPalette => "Ctrl+Shift+P",
            Self::NewTab => "Ctrl+Shift+T",
            Self::ClosePane => "Ctrl+Shift+W",
            Self::NextTab => "Ctrl+Tab",
            Self::PrevTab => "Ctrl+Shift+Tab",
            Self::GoToTab(_) => "Alt+1…9",
            Self::SplitRight => "Ctrl+Shift+D",
            Self::SplitDown => "Ctrl+Shift+E",
            Self::Focus(Direction::Left) => "Alt+Left",
            Self::Focus(Direction::Right) => "Alt+Right",
            Self::Focus(Direction::Up) => "Alt+Up",
            Self::Focus(Direction::Down) => "Alt+Down",
            Self::ToggleZoomPane => "Ctrl+Shift+Enter",
            Self::Copy => "Ctrl+Shift+C",
            Self::CopyOrInterrupt => "Ctrl+C",
            Self::Paste => "Ctrl+Shift+V",
            Self::FontBigger => "Ctrl+=",
            Self::FontSmaller => "Ctrl+-",
            Self::FontReset => "Ctrl+0",
            Self::ScrollPageUp => "Shift+PgUp",
            Self::ScrollPageDown => "Shift+PgDn",
            Self::ScrollToTop => "Shift+Home",
            Self::ScrollToBottom => "Shift+End",
            Self::CloseTab | Self::Quit => "",
        }
    }
}

/// Actions offered in the command palette, in display order when the query is empty.
pub const PALETTE: &[Action] = &[
    Action::NewTab,
    Action::SplitRight,
    Action::SplitDown,
    Action::ClosePane,
    Action::CloseTab,
    Action::NextTab,
    Action::PrevTab,
    Action::Focus(Direction::Left),
    Action::Focus(Direction::Right),
    Action::Focus(Direction::Up),
    Action::Focus(Direction::Down),
    Action::ToggleZoomPane,
    Action::Copy,
    Action::Paste,
    Action::FontBigger,
    Action::FontSmaller,
    Action::FontReset,
    Action::ScrollPageUp,
    Action::ScrollPageDown,
    Action::ScrollToTop,
    Action::ScrollToBottom,
    Action::Quit,
];
