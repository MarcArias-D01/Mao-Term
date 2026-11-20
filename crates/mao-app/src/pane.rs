//! A pane: one terminal session plus its per-pane input state.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mao_render::CellMetrics;
use mao_terminal::keys::{self, Key, KeyModes, Modifiers};
use mao_terminal::mouse::{self as tmouse, MouseAction};
use mao_terminal::{GridSize, SelectionKind, ShellEvent, ShellKind, Terminal};
use mao_ui::{PaneId, Rect};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::editor::PromptEditor;

/// What the shell is doing, from shell-integration marks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShellState {
    /// No integration marks seen (yet): classic terminal behavior.
    #[default]
    Unknown,
    /// Printing its prompt.
    Prompt,
    /// Waiting for a command: the prompt editor is active.
    Input,
    /// A command is running: keys go to it.
    Running,
}

const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(400);
const WHEEL_LINES: f32 = 3.0;

#[derive(Default)]
struct PaneMouse {
    selecting: bool,
    /// Button held while the application receives mouse reports.
    reported_button: Option<tmouse::MouseButton>,
    /// Last cell reported to the application (motion is only sent on cell change).
    last_reported_cell: Option<(usize, usize)>,
    last_click: Option<(Instant, usize, usize)>,
    click_count: u8,
    /// Sub-line touchpad scroll accumulated in pixels.
    scroll_accum: f32,
}

pub struct Pane {
    pub id: PaneId,
    pub terminal: Terminal,
    /// Title set by the program (OSC 0/2).
    pub title: String,
    /// The shell exited; waiting for Enter (restart) or Esc (close).
    pub exited: bool,
    /// Pane rectangle in window pixels, and the grid origin inside it.
    pub rect: Rect,
    pub origin: (f32, f32),
    /// Next frame must redraw every row of this pane.
    pub full_redraw: bool,
    pub shell: ShellState,
    /// Current directory reported by the shell (OSC 7).
    pub cwd: Option<PathBuf>,
    /// Exit code of the last command (OSC 133;D).
    pub last_exit: Option<i32>,
    pub editor: PromptEditor,
    /// Rows the editor overlay covered in the last frame.
    pub overlay_rows: Vec<usize>,
    /// Where the completion menu goes: (x, y below the caret row, y of the caret row top).
    pub menu_anchor: Option<(f32, f32, f32)>,
    mouse: PaneMouse,
    buf: Vec<u8>,
}

impl Pane {
    pub fn new(id: PaneId, terminal: Terminal) -> Self {
        Self {
            id,
            terminal,
            title: String::new(),
            exited: false,
            rect: Rect::default(),
            origin: (0.0, 0.0),
            full_redraw: true,
            shell: ShellState::Unknown,
            cwd: None,
            last_exit: None,
            editor: PromptEditor::default(),
            overlay_rows: Vec::new(),
            menu_anchor: None,
            mouse: PaneMouse::default(),
            buf: Vec::with_capacity(16),
        }
    }

    /// Whether keys go to the prompt editor instead of the shell.
    pub fn editor_active(&self, enabled: bool) -> bool {
        enabled
            && self.shell == ShellState::Input
            && !self.exited
            && !self.terminal.modes().alt_screen
    }

    pub fn quoting(&self) -> mao_complete::Quoting {
        match self.terminal.shell_kind() {
            ShellKind::PowerShell => mao_complete::Quoting::PowerShell,
            _ => mao_complete::Quoting::Posix,
        }
    }

    /// Applies a shell-integration mark.
    pub fn on_shell_event(&mut self, event: ShellEvent) {
        match event {
            ShellEvent::PromptStart => self.shell = ShellState::Prompt,
            ShellEvent::PromptEnd => self.shell = ShellState::Input,
            ShellEvent::CommandStart => self.shell = ShellState::Running,
            ShellEvent::CommandEnd(code) => self.last_exit = code,
            ShellEvent::Cwd(path) => self.cwd = Some(path),
            // Handled by the app (opens a document pane).
            ShellEvent::Open(_) => {}
        }
    }

    /// Display name: the cwd's folder if known, else the program's title.
    pub fn display_name(&self) -> String {
        match self.cwd.as_ref().and_then(|p| p.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => match &self.cwd {
                // A drive root like `C:\` has no file name.
                Some(p) => p.display().to_string(),
                None => self.short_title().to_owned(),
            },
        }
    }

    /// Swaps in a new terminal (shell restart), keeping the pane's place.
    pub fn replace_terminal(&mut self, terminal: Terminal) {
        self.terminal = terminal;
        self.title.clear();
        self.exited = false;
        self.full_redraw = true;
        self.shell = ShellState::Unknown;
        self.last_exit = None;
        self.editor = PromptEditor::default();
        self.mouse = PaneMouse::default();
    }

    /// Short title for tabs: the last path component without `.exe`.
    pub fn short_title(&self) -> &str {
        let title = self.title.trim();
        if title.is_empty() {
            return "shell";
        }
        let last = title.rsplit(['\\', '/']).next().unwrap_or(title);
        let last = last.strip_suffix(".exe").unwrap_or(last);
        if last.is_empty() { title } else { last }
    }

    /// Places the pane and resizes its terminal to fit `rect` minus `pad`.
    pub fn layout(&mut self, rect: Rect, pad: f32, cell: CellMetrics) -> GridSize {
        self.rect = rect;
        self.origin = ((rect.x + pad).round(), (rect.y + pad).round());
        let grid = grid_for(rect, pad, cell);
        if grid != self.terminal.size() {
            self.terminal.resize(grid);
            self.full_redraw = true;
        }
        grid
    }

    pub fn write_key(&mut self, key: Key<'_>, mods: Modifiers) {
        let modes = KeyModes {
            app_cursor: self.terminal.modes().app_cursor,
        };
        if keys::encode(key, mods, modes, &mut self.buf) {
            self.terminal.scroll_to_bottom();
            self.terminal.clear_selection();
            self.terminal.write(self.buf.clone());
        }
    }

    pub fn copy_selection(&self, clipboard: Option<&mut arboard::Clipboard>) -> bool {
        let Some(text) = self.terminal.selection_text() else {
            return false;
        };
        if let Some(clipboard) = clipboard
            && let Err(e) = clipboard.set_text(text)
        {
            tracing::warn!("copy failed: {e}");
        }
        true
    }

    pub fn paste(&self, clipboard: Option<&mut arboard::Clipboard>) {
        let Some(clipboard) = clipboard else {
            return;
        };
        match clipboard.get_text() {
            Ok(text) if !text.is_empty() => {
                self.terminal.scroll_to_bottom();
                self.terminal.paste(&text);
            }
            Ok(_) => {}
            Err(e) => tracing::debug!("paste: clipboard has no text: {e}"),
        }
    }

    pub fn on_shell_exit(&mut self, code: Option<i32>) {
        if self.exited {
            // Both "child exited" and "PTY closed" arrive; report once.
            return;
        }
        self.exited = true;
        tracing::info!(pane = self.id, ?code, "shell exited");
        let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
        self.terminal.scroll_to_bottom();
        self.terminal.print_local(&format!(
            "\r\n\x1b[0;2m[process exited with code {code} · Enter: restart · Esc: close]\x1b[0m"
        ));
    }

    /// Whether the pane holds the mouse (drag selection or reported button).
    pub fn has_mouse_capture(&self) -> bool {
        self.mouse.selecting || self.mouse.reported_button.is_some()
    }

    /// Viewport cell under `pos`, and whether it's the right half of the cell.
    fn cell_at(&self, pos: PhysicalPosition<f64>, cell: CellMetrics) -> (usize, usize, bool) {
        let x = ((pos.x as f32 - self.origin.0) / cell.width).max(0.0);
        let y = ((pos.y as f32 - self.origin.1) / cell.height).max(0.0);
        let size = self.terminal.size();
        let col = (x as usize).min(usize::from(size.cols).saturating_sub(1));
        let row = (y as usize).min(usize::from(size.rows).saturating_sub(1));
        (row, col, x.fract() > 0.5)
    }

    /// Sends a mouse event to the program if it enabled reporting. Shift bypasses
    /// reporting so the user can still select. Returns whether it was consumed.
    fn report_mouse(
        &mut self,
        button: Option<tmouse::MouseButton>,
        action: MouseAction,
        pos: PhysicalPosition<f64>,
        cell: CellMetrics,
        mods: Modifiers,
    ) -> bool {
        let modes = self.terminal.modes().mouse;
        if !modes.report || mods.shift || self.exited {
            return false;
        }
        let (row, col, _) = self.cell_at(pos, cell);
        if action == MouseAction::Motion
            && (!modes.wants_motion(button.is_some())
                || self.mouse.last_reported_cell == Some((row, col)))
        {
            return true;
        }
        self.mouse.last_reported_cell = Some((row, col));
        if tmouse::encode(button, action, mods, row, col, modes, &mut self.buf) {
            self.terminal.write(self.buf.clone());
        }
        true
    }

    /// Handles a mouse button inside the pane. Returns whether a redraw is needed.
    pub fn on_mouse_button(
        &mut self,
        state: ElementState,
        button: MouseButton,
        pos: PhysicalPosition<f64>,
        cell: CellMetrics,
        mods: Modifiers,
        clipboard: Option<&mut arboard::Clipboard>,
    ) -> bool {
        let reported = match button {
            MouseButton::Left => Some(tmouse::MouseButton::Left),
            MouseButton::Middle => Some(tmouse::MouseButton::Middle),
            MouseButton::Right => Some(tmouse::MouseButton::Right),
            _ => None,
        };
        if let Some(b) = reported {
            let action = match state {
                ElementState::Pressed => MouseAction::Press,
                ElementState::Released => MouseAction::Release,
            };
            if action == MouseAction::Release && self.mouse.reported_button == Some(b) {
                // Always pair a reported press with its release, even if Shift is now held.
                self.mouse.reported_button = None;
                let (row, col, _) = self.cell_at(pos, cell);
                let modes = self.terminal.modes().mouse;
                if tmouse::encode(Some(b), action, mods, row, col, modes, &mut self.buf) {
                    self.terminal.write(self.buf.clone());
                }
                return false;
            }
            if self.report_mouse(Some(b), action, pos, cell, mods) {
                if action == MouseAction::Press {
                    self.mouse.reported_button = Some(b);
                }
                return false;
            }
        }

        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                let (row, col, right) = self.cell_at(pos, cell);
                let now = Instant::now();
                let repeat = self.mouse.last_click.is_some_and(|(t, r, c)| {
                    now.duration_since(t) < MULTI_CLICK_INTERVAL && r == row && c == col
                });
                self.mouse.click_count = if repeat {
                    self.mouse.click_count % 3 + 1
                } else {
                    1
                };
                self.mouse.last_click = Some((now, row, col));
                let kind = match (self.mouse.click_count, mods.alt) {
                    (_, true) => SelectionKind::Block,
                    (2, _) => SelectionKind::Word,
                    (3, _) => SelectionKind::Line,
                    _ => SelectionKind::Simple,
                };
                self.terminal.start_selection(kind, row, col, right);
                self.mouse.selecting = true;
                true
            }
            (MouseButton::Left, ElementState::Released) => {
                self.mouse.selecting = false;
                false
            }
            (MouseButton::Right, ElementState::Pressed) if cfg!(windows) => {
                // Windows Terminal convention: right click copies a selection, or pastes.
                let mut clipboard = clipboard;
                if self.copy_selection(clipboard.as_deref_mut()) {
                    self.terminal.clear_selection();
                    true
                } else {
                    self.paste(clipboard);
                    false
                }
            }
            (MouseButton::Middle, ElementState::Pressed) if !cfg!(windows) => {
                self.paste(clipboard);
                false
            }
            _ => false,
        }
    }

    /// Handles pointer motion. Returns whether a redraw is needed.
    pub fn on_mouse_move(
        &mut self,
        pos: PhysicalPosition<f64>,
        cell: CellMetrics,
        mods: Modifiers,
    ) -> bool {
        let held = self.mouse.reported_button;
        if (held.is_some() || self.terminal.modes().mouse.motion)
            && self.report_mouse(held, MouseAction::Motion, pos, cell, mods)
        {
            return false;
        }
        if self.mouse.selecting {
            let (row, col, right) = self.cell_at(pos, cell);
            self.terminal.update_selection(row, col, right);
            return true;
        }
        false
    }

    /// Handles the mouse wheel. Returns whether a redraw is needed.
    pub fn on_wheel(
        &mut self,
        delta: MouseScrollDelta,
        pos: PhysicalPosition<f64>,
        cell: CellMetrics,
        mods: Modifiers,
    ) -> bool {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * WHEEL_LINES,
            MouseScrollDelta::PixelDelta(p) => {
                self.mouse.scroll_accum += p.y as f32;
                let lines = (self.mouse.scroll_accum / cell.height).trunc();
                self.mouse.scroll_accum -= lines * cell.height;
                lines
            }
        };
        let lines = lines as i32;
        if lines == 0 {
            return false;
        }
        let modes = self.terminal.modes();
        if modes.mouse.report && !mods.shift {
            let button = if lines > 0 {
                tmouse::MouseButton::WheelUp
            } else {
                tmouse::MouseButton::WheelDown
            };
            for _ in 0..lines.unsigned_abs().min(10) {
                self.report_mouse(Some(button), MouseAction::Press, pos, cell, mods);
            }
            return false;
        }
        if modes.alt_screen && modes.alternate_scroll {
            // Full-screen apps (less, vim…): scroll by sending arrow keys.
            let arrow: &[u8] = match (lines > 0, modes.app_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.terminal
                .write(arrow.repeat(lines.unsigned_abs() as usize));
            return false;
        }
        self.terminal.scroll_lines(lines);
        true
    }
}

/// Grid size that fits `rect` minus `pad` on every side.
pub fn grid_for(rect: Rect, pad: f32, cell: CellMetrics) -> GridSize {
    let cols = ((rect.w - 2.0 * pad) / cell.width).floor().max(2.0);
    let rows = ((rect.h - 2.0 * pad) / cell.height).floor().max(1.0);
    GridSize {
        cols: cols as u16,
        rows: rows as u16,
        cell_width: cell.width as u16,
        cell_height: cell.height as u16,
    }
}
