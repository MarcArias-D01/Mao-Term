//! Terminal sessions: PTY + VT emulation (`alacritty_terminal`).
//!
//! `alacritty_terminal` types never leak out of this crate: the UI gets plain
//! RGB cells through [`Terminal::render`]. See `docs/specs/terminal-core.md`.

pub mod integration;
pub mod keys;
pub mod mouse;
mod osc;
mod palette;
mod platform;
mod pty;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, Term, TermDamage, TermMode};
use alacritty_terminal::tty::{self, Shell};

use crate::pty::ScanningPty;
use alacritty_terminal::vte::ansi::{CursorShape as VtCursorShape, Processor};

pub use integration::ShellKind;
pub use osc::ShellEvent;
pub use palette::{Palette, Rgb8};

/// Errors raised when creating a terminal session.
#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("failed to create the PTY: {0}")]
    Pty(#[source] std::io::Error),
    #[error("failed to start the PTY I/O thread: {0}")]
    EventLoop(#[source] std::io::Error),
}

/// Events sent from the I/O thread to the UI. The callback runs on the I/O
/// thread: keep it cheap (typically forward to the UI event loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// New content is available. Coalesced: no new wakeup is sent until
    /// [`Terminal::render`] runs.
    Wakeup,
    Title(String),
    ResetTitle,
    Bell,
    /// The program asked to copy text to the clipboard (OSC 52).
    ClipboardStore(String),
    /// The shell exited, with its exit code when known.
    Exited(Option<i32>),
    /// Shell-integration mark (prompt, command, cwd).
    Shell(ShellEvent),
}

/// Grid size in cells plus cell size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl GridSize {
    fn window_size(self) -> WindowSize {
        WindowSize {
            num_lines: self.rows,
            num_cols: self.cols,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// How to start the shell.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Program to run; `None` uses the OS default shell.
    pub program: Option<String>,
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub scrollback: usize,
    pub palette: Palette,
    /// Folder for the shell-integration scripts; `None` disables integration.
    pub integration_dir: Option<PathBuf>,
}

/// Visual style of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// The glyph spans two cells.
    pub wide: bool,
    /// Second half of a wide glyph: draw its background only.
    pub spacer: bool,
}

/// A resolved cell, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellView {
    pub ch: char,
    pub fg: Rgb8,
    pub bg: Rgb8,
    pub style: CellStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// Drawn by inverting the cell colors (already applied to the cells).
    Block,
    HollowBlock,
    Beam,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorView {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub color: Rgb8,
    pub wide: bool,
}

/// Result of [`Terminal::render`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderInfo {
    pub cursor: Option<CursorView>,
    /// Lines scrolled back into history (0 = at the bottom).
    pub display_offset: usize,
}

/// Options for [`Terminal::render`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    /// The pane has keyboard focus (solid vs hollow block cursor).
    pub focused: bool,
    /// Visit every row, not just the changed ones.
    pub force_full: bool,
    /// Don't draw the terminal cursor (e.g. the input editor draws its own).
    pub hide_cursor: bool,
}

/// Terminal modes the UI cares about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    pub alternate_scroll: bool,
    pub mouse: mouse::MouseModes,
    /// The program wants focus in/out reports (`CSI I` / `CSI O`).
    pub focus_reporting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Character-wise (single click + drag).
    Simple,
    /// Word-wise (double click).
    Word,
    /// Line-wise (triple click).
    Line,
    /// Rectangular (Alt + drag).
    Block,
}

/// State shared between the UI and the I/O thread.
struct Shared {
    sender: OnceLock<EventLoopSender>,
    size: Mutex<WindowSize>,
    wakeup_pending: AtomicBool,
    notify: Box<dyn Fn(TerminalEvent) + Send + Sync>,
}

#[derive(Clone)]
struct EventProxy(Arc<Shared>);

impl EventProxy {
    fn write(&self, bytes: Cow<'static, [u8]>) {
        if let Some(sender) = self.0.sender.get()
            && let Err(e) = sender.send(Msg::Input(bytes))
        {
            tracing::warn!("failed to write to the PTY: {e:?}");
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let notify = &self.0.notify;
        match event {
            Event::Wakeup => {
                if !self.0.wakeup_pending.swap(true, Ordering::AcqRel) {
                    notify(TerminalEvent::Wakeup);
                }
            }
            Event::Title(title) => notify(TerminalEvent::Title(title)),
            Event::ResetTitle => notify(TerminalEvent::ResetTitle),
            Event::Bell => notify(TerminalEvent::Bell),
            Event::ClipboardStore(_, text) => notify(TerminalEvent::ClipboardStore(text)),
            Event::PtyWrite(text) => self.write(Cow::Owned(text.into_bytes())),
            Event::TextAreaSizeRequest(format) => {
                let size = *self.0.size.lock().unwrap_or_else(|e| e.into_inner());
                self.write(Cow::Owned(format(size).into_bytes()));
            }
            Event::ChildExit(status) => notify(TerminalEvent::Exited(status.code())),
            Event::Exit => notify(TerminalEvent::Exited(None)),
            // Color queries (OSC 4/10/11) and OSC 52 reads are not supported yet.
            Event::ColorRequest(..)
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange => {}
        }
    }
}

/// A running terminal session.
pub struct Terminal {
    term: Arc<FairMutex<Term<EventProxy>>>,
    proxy: EventProxy,
    kind: ShellKind,
    /// Process id of the shell (if the platform reports it).
    pid: Option<u32>,
    size: GridSize,
    palette: Palette,
    // Render state, reused across frames to avoid allocations.
    dirty: Vec<bool>,
    row: Vec<CellView>,
    last_cursor_row: Option<usize>,
    last_offset: usize,
    last_selection: Option<SelectionRange>,
    /// Rows to visit on the next render regardless of damage (UI overlays).
    extra_dirty: Vec<usize>,
}

impl Terminal {
    /// Spawns the shell in a PTY and starts its I/O thread.
    pub fn spawn(
        options: SpawnOptions,
        size: GridSize,
        notify: impl Fn(TerminalEvent) + Send + Sync + 'static,
    ) -> Result<Self, TerminalError> {
        let (program, mut args) = match options.program {
            Some(program) => (program, options.args),
            None => platform::default_shell(),
        };
        let mut env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "mao-term".to_owned()),
        ]);
        let kind = ShellKind::detect(&program);
        if let Some(dir) = &options.integration_dir {
            integration::inject(&program, &mut args, &mut env, dir);
        }
        tracing::debug!(%program, ?args, ?kind, "spawning shell");
        let pty_options = tty::Options {
            shell: Some(Shell::new(program, args)),
            working_directory: options.working_directory,
            drain_on_exit: false,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let pty = tty::new(&pty_options, size.window_size(), 0).map_err(TerminalError::Pty)?;
        #[cfg(windows)]
        let pid = pty.child_watcher().pid().map(std::num::NonZeroU32::get);
        #[cfg(not(windows))]
        let pid = Some(pty.child().id());

        let notify: Arc<dyn Fn(TerminalEvent) + Send + Sync> = Arc::new(notify);
        let shell_notify = Arc::clone(&notify);
        let pty = ScanningPty::new(
            pty,
            Arc::new(move |e| shell_notify(TerminalEvent::Shell(e))),
        );

        let proxy = EventProxy(Arc::new(Shared {
            sender: OnceLock::new(),
            size: Mutex::new(size.window_size()),
            wakeup_pending: AtomicBool::new(false),
            notify: Box::new(move |e| notify(e)),
        }));
        let config = term::Config {
            scrolling_history: options.scrollback,
            ..Default::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, proxy.clone())));

        let event_loop = EventLoop::new(Arc::clone(&term), proxy.clone(), pty, false, false)
            .map_err(TerminalError::EventLoop)?;
        // `set` only fails if already set, which cannot happen here.
        let _ = proxy.0.sender.set(event_loop.channel());
        // The I/O thread runs until it receives `Msg::Shutdown` (sent on drop) or the shell exits.
        drop(event_loop.spawn());

        Ok(Self {
            term,
            proxy,
            kind,
            pid,
            size,
            palette: options.palette,
            dirty: Vec::new(),
            row: Vec::new(),
            last_cursor_row: None,
            last_offset: 0,
            last_selection: None,
            extra_dirty: Vec::new(),
        })
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    /// Changes the colors (theme switch). The next render must be a full one.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Process id of the shell.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The kind of shell running (for quoting and completion rules).
    pub fn shell_kind(&self) -> ShellKind {
        self.kind
    }

    /// Sends bytes to the shell. Never blocks: the write happens on the I/O thread.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.proxy.write(bytes.into());
    }

    /// Pastes text, using bracketed paste when the program enabled it.
    pub fn paste(&self, text: &str) {
        let bracketed = self.modes().bracketed_paste;
        let mut out = String::with_capacity(text.len() + 12);
        if bracketed {
            out.push_str("\x1b[200~");
            // A paste must not be able to end the bracket early.
            out.push_str(&text.replace("\x1b[201~", ""));
            out.push_str("\x1b[201~");
        } else {
            out.push_str(&text.replace("\r\n", "\r").replace('\n', "\r"));
        }
        self.write(out.into_bytes());
    }

    pub fn resize(&mut self, size: GridSize) {
        if size == self.size {
            return;
        }
        self.size = size;
        *self.proxy.0.size.lock().unwrap_or_else(|e| e.into_inner()) = size.window_size();
        self.term.lock().resize(size);
        if let Some(sender) = self.proxy.0.sender.get() {
            let _ = sender.send(Msg::Resize(size.window_size()));
        }
    }

    pub fn modes(&self) -> Modes {
        let mode = *self.term.lock().mode();
        Modes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            mouse: mouse::MouseModes {
                report: mode.intersects(TermMode::MOUSE_MODE),
                drag: mode.contains(TermMode::MOUSE_DRAG),
                motion: mode.contains(TermMode::MOUSE_MOTION),
                sgr: mode.contains(TermMode::SGR_MOUSE),
            },
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
        }
    }

    /// Reports a focus change to the program, if it asked for it.
    pub fn report_focus(&self, focused: bool) {
        if self.modes().focus_reporting {
            self.write(if focused {
                &b"\x1b[I"[..]
            } else {
                &b"\x1b[O"[..]
            });
        }
    }

    /// Prints text on the terminal screen locally, without involving the shell
    /// (e.g. a "process exited" notice once the PTY is gone).
    pub fn print_local(&self, text: &str) {
        let mut term = self.term.lock();
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *term, text.as_bytes());
        drop(term);
        // Make the UI redraw even though no PTY output arrived.
        self.proxy.0.wakeup_pending.store(true, Ordering::Release);
        (self.proxy.0.notify)(TerminalEvent::Wakeup);
    }

    /// Scrolls the viewport through history. Positive = up (older lines).
    pub fn scroll_lines(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_page(&self, up: bool) {
        self.term
            .lock()
            .scroll_display(if up { Scroll::PageUp } else { Scroll::PageDown });
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Starts a selection at a viewport cell. `right_half` is whether the
    /// pointer is on the right half of the cell.
    pub fn start_selection(&self, kind: SelectionKind, row: usize, col: usize, right_half: bool) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        term.selection = Some(Selection::new(ty, point, side(right_half)));
    }

    pub fn update_selection(&self, row: usize, col: usize, right_half: bool) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side(right_half));
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn has_selection(&self) -> bool {
        self.term
            .lock()
            .selection
            .as_ref()
            .is_some_and(|s| !s.is_empty())
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    /// Forces these viewport rows to be visited by the next [`Self::render`]
    /// (e.g. rows an overlay drew over).
    pub fn invalidate_rows(&mut self, rows: impl IntoIterator<Item = usize>) {
        self.extra_dirty.extend(rows);
    }

    /// Viewport position (row, col) of the terminal cursor, if visible.
    pub fn cursor_position(&self) -> Option<(usize, usize)> {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let point = term.grid().cursor.point;
        let row = usize::try_from(point.line.0 + offset).ok()?;
        (row < usize::from(self.size.rows)).then_some((row, point.column.0))
    }

    /// Visits the rows that changed since the last call (all rows if `force_full`),
    /// with colors fully resolved (selection and block cursor included).
    pub fn render(
        &mut self,
        opts: RenderOptions,
        mut on_row: impl FnMut(usize, &[CellView]),
    ) -> RenderInfo {
        let RenderOptions {
            focused,
            force_full,
            hide_cursor,
        } = opts;
        // New output after this point must trigger a new wakeup.
        self.proxy.0.wakeup_pending.store(false, Ordering::Release);

        let rows = usize::from(self.size.rows);
        let cols = usize::from(self.size.cols);
        let mut term = self.term.lock();

        self.dirty.clear();
        self.dirty.resize(rows, force_full);
        match term.damage() {
            TermDamage::Full => self.dirty.fill(true),
            TermDamage::Partial(lines) => {
                for damage in lines {
                    if let Some(d) = self.dirty.get_mut(damage.line) {
                        *d = true;
                    }
                }
            }
        }
        term.reset_damage();
        for row in self.extra_dirty.drain(..) {
            if let Some(d) = self.dirty.get_mut(row) {
                *d = true;
            }
        }

        let content = term.renderable_content();
        let offset = content.display_offset;
        let selection = content.selection;
        if offset != self.last_offset || selection != self.last_selection {
            self.dirty.fill(true);
        }
        self.last_offset = offset;
        self.last_selection = selection;

        let cursor_point = content.cursor.point;
        let cursor_row = usize::try_from(cursor_point.line.0 + offset as i32)
            .ok()
            .filter(|&r| r < rows);
        let cursor_shape = match content.cursor.shape {
            _ if hide_cursor => None,
            VtCursorShape::Hidden => None,
            VtCursorShape::Block if focused => Some(CursorShape::Block),
            VtCursorShape::Block | VtCursorShape::HollowBlock => Some(CursorShape::HollowBlock),
            VtCursorShape::Beam => Some(CursorShape::Beam),
            VtCursorShape::Underline => Some(CursorShape::Underline),
        };
        for row in [self.last_cursor_row, cursor_row].into_iter().flatten() {
            if let Some(d) = self.dirty.get_mut(row) {
                *d = true;
            }
        }
        self.last_cursor_row = cursor_row;

        let colors = content.colors;
        let palette = &self.palette;
        let grid = term.grid();
        let cursor_color = palette.indexed(
            colors,
            alacritty_terminal::vte::ansi::NamedColor::Cursor as usize,
        );

        for row in 0..rows {
            if !self.dirty[row] {
                continue;
            }
            let line = Line(row as i32 - offset as i32);
            let grid_row = &grid[line];
            self.row.clear();
            for col in 0..cols {
                let cell = &grid_row[Column(col)];
                let flags = cell.flags;
                let mut fg = palette.resolve(colors, cell.fg);
                let mut bg = palette.resolve(colors, cell.bg);
                if flags.contains(Flags::DIM) {
                    fg = palette::dim(fg);
                }
                if flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if flags.contains(Flags::HIDDEN) {
                    fg = bg;
                }
                if selection.is_some_and(|s| s.contains(Point::new(line, Column(col)))) {
                    bg = palette.selection();
                }
                if cursor_shape == Some(CursorShape::Block)
                    && cursor_row == Some(row)
                    && cursor_point.column.0 == col
                {
                    fg = bg;
                    bg = cursor_color;
                }
                let spacer =
                    flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                self.row.push(CellView {
                    ch: if flags.contains(Flags::HIDDEN) || spacer {
                        ' '
                    } else {
                        cell.c
                    },
                    fg,
                    bg,
                    style: CellStyle {
                        bold: flags.contains(Flags::BOLD),
                        italic: flags.contains(Flags::ITALIC),
                        underline: flags.intersects(Flags::ALL_UNDERLINES),
                        strikeout: flags.contains(Flags::STRIKEOUT),
                        wide: flags.contains(Flags::WIDE_CHAR),
                        spacer,
                    },
                });
            }
            on_row(row, &self.row);
        }

        let cursor = cursor_row.zip(cursor_shape).map(|(row, shape)| CursorView {
            row,
            col: cursor_point.column.0,
            shape,
            color: cursor_color,
            wide: grid[cursor_point].flags.contains(Flags::WIDE_CHAR),
        });
        RenderInfo {
            cursor,
            display_offset: offset,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(sender) = self.proxy.0.sender.get() {
            let _ = sender.send(Msg::Shutdown);
        }
    }
}

fn side(right_half: bool) -> Side {
    if right_half { Side::Right } else { Side::Left }
}

fn viewport_point<T>(term: &Term<T>, row: usize, col: usize) -> Point {
    let offset = term.grid().display_offset() as i32;
    let rows = term.screen_lines();
    let cols = term.columns();
    let row = row.min(rows.saturating_sub(1)) as i32;
    Point::new(Line(row - offset), Column(col.min(cols.saturating_sub(1))))
}
