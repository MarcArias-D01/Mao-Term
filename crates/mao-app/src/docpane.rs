//! A document pane: shows a file loaded by `mao-docs` (Markdown, code, text,
//! images) next to the terminals. See `docs/specs/document-viewer.md`.

use std::path::PathBuf;
use std::sync::Arc;

use mao_docs::layout::{self, Row};
use mao_docs::model::char_width;
use mao_docs::{Content, Document, Kind, Style, TextDoc};
use mao_render::{CellMetrics, CellStyle, GridCell, ImageData, ImageDraw, Renderer, Rgb8};
use mao_ui::{PaneId, Rect};

/// Colors of the text view.
#[derive(Debug, Clone, Copy)]
pub struct DocColors {
    pub fg: Rgb8,
    pub bg: Rgb8,
    pub muted: Rgb8,
    pub selection: Rgb8,
    pub matched: Rgb8,
    pub current: Rgb8,
}

/// Buttons in a document pane's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocButton {
    /// Markdown: the rendered view (segmented with `Source`).
    Preview,
    /// Markdown: the source.
    Source,
    Wrap,
    /// Image: fit to the pane.
    Fit,
    /// Open with the system's default app.
    External,
    /// Close the document pane.
    Close,
}

/// How a header button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Part of a segmented choice (selected = filled).
    Segment,
    /// On/off switch (shows a check when on).
    Toggle,
    /// Plain action.
    Action,
}

/// A position in the text: line and byte offset in its content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub byte: usize,
}

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    /// (line, start byte, end byte), in document order.
    pub matches: Vec<(usize, usize, usize)>,
    pub current: usize,
}

const ZOOM_STEP: f32 = 1.25;
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 64.0;

pub struct DocPane {
    pub id: PaneId,
    pub path: PathBuf,
    pub doc: Option<Arc<Document>>,
    pub rect: Rect,
    pub header: Rect,
    pub body: Rect,
    /// Grid origin and size inside the body.
    pub origin: (f32, f32),
    pub cols: usize,
    pub rows: usize,
    cell: CellMetrics,
    /// Markdown: show the source instead of the rendered view.
    pub source: bool,
    pub wrap: bool,
    layout: Vec<Row>,
    layout_key: Option<(usize, bool, bool)>,
    /// First visible row, and first visible column when not wrapping.
    pub scroll: usize,
    pub hscroll: usize,
    /// Image zoom (`None` = fit to the pane) and pan offset in pixels.
    pub zoom: Option<f32>,
    pub pan: (f32, f32),
    /// The image is on the GPU.
    image_ready: bool,
    /// (anchor, head) of the selection.
    pub selection: Option<(Pos, Pos)>,
    selecting: bool,
    /// Image drag: pointer and pan at press time.
    panning: Option<((f32, f32), (f32, f32))>,
    pub search: Option<Search>,
    /// The grid must be redrawn.
    pub dirty: bool,
}

impl DocPane {
    pub fn new(id: PaneId, path: PathBuf) -> Self {
        Self {
            id,
            path,
            doc: None,
            rect: Rect::default(),
            header: Rect::default(),
            body: Rect::default(),
            origin: (0.0, 0.0),
            cols: 1,
            rows: 1,
            cell: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
            source: false,
            wrap: true,
            layout: Vec::new(),
            layout_key: None,
            scroll: 0,
            hscroll: 0,
            zoom: None,
            pan: (0.0, 0.0),
            image_ready: false,
            selection: None,
            selecting: false,
            panning: None,
            search: None,
            dirty: true,
        }
    }

    /// Shows another file in this pane (state is reset).
    pub fn replace_path(&mut self, path: PathBuf) {
        let (id, rect) = (self.id, self.rect);
        let (header, body, origin, cols, rows, cell) = (
            self.header,
            self.body,
            self.origin,
            self.cols,
            self.rows,
            self.cell,
        );
        *self = Self::new(id, path);
        self.rect = rect;
        self.header = header;
        self.body = body;
        self.origin = origin;
        self.cols = cols;
        self.rows = rows;
        self.cell = cell;
    }

    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// A (re)loaded version of the document arrived.
    pub fn set_document(&mut self, doc: Arc<Document>) {
        let at_bottom = self.doc.is_some() && self.scroll > 0 && self.scroll >= self.max_scroll();
        self.doc = Some(doc);
        self.layout_key = None;
        self.image_ready = false;
        self.selection = None;
        self.ensure_layout();
        self.scroll = if at_bottom {
            // Following the end of a growing file (logs).
            self.max_scroll()
        } else {
            self.scroll.min(self.max_scroll())
        };
        self.refresh_search();
        self.dirty = true;
    }

    pub fn is_markdown(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.kind == Kind::Markdown)
    }

    pub fn text(&self) -> Option<&TextDoc> {
        match &self.doc.as_ref()?.content {
            Content::Markdown { rendered, source } => {
                Some(if self.source { source } else { rendered })
            }
            Content::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn image(&self) -> Option<&mao_docs::Image> {
        match &self.doc.as_ref()?.content {
            Content::Image(img) => Some(img),
            _ => None,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match &self.doc.as_ref()?.content {
            Content::Message(m) => Some(m),
            _ => None,
        }
    }

    /// Places the pane: a header bar of `header_h` pixels, then the body.
    pub fn set_rect(&mut self, rect: Rect, header_h: f32, pad: f32, cell: CellMetrics) {
        self.rect = rect;
        self.header = Rect::new(rect.x, rect.y, rect.w, header_h.min(rect.h));
        self.body = Rect::new(
            rect.x,
            rect.y + self.header.h,
            rect.w,
            (rect.h - self.header.h).max(0.0),
        );
        self.origin = (
            (self.body.x + pad).round(),
            (self.body.y + pad / 2.0).round(),
        );
        self.cols = (((self.body.w - 2.0 * pad) / cell.width).floor().max(2.0)) as usize;
        self.rows = (((self.body.h - pad) / cell.height).floor().max(1.0)) as usize;
        self.cell = cell;
        self.ensure_layout();
        self.scroll = self.scroll.min(self.max_scroll());
        self.dirty = true;
    }

    /// Cells used by line numbers (or the left margin).
    fn gutter(&self) -> usize {
        match self.text() {
            Some(t) if t.gutter => t.lines.len().max(1).to_string().len() + 2,
            _ => 1,
        }
    }

    fn text_cols(&self) -> usize {
        self.cols.saturating_sub(self.gutter()).max(1)
    }

    fn ensure_layout(&mut self) {
        let key = (self.text_cols(), self.wrap, self.source);
        if self.layout_key == Some(key) {
            return;
        }
        self.layout = match self.text() {
            Some(t) => layout::layout(t, key.0, self.wrap),
            None => Vec::new(),
        };
        self.layout_key = Some(key);
    }

    pub fn total_rows(&self) -> usize {
        self.layout.len()
    }

    pub fn max_scroll(&self) -> usize {
        self.layout.len().saturating_sub(self.rows)
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let next = (self.scroll as isize + delta).clamp(0, self.max_scroll() as isize) as usize;
        if next != self.scroll {
            self.scroll = next;
            self.dirty = true;
        }
    }

    pub fn scroll_columns(&mut self, delta: isize) {
        if self.wrap {
            return;
        }
        let next = (self.hscroll as isize + delta).max(0) as usize;
        if next != self.hscroll {
            self.hscroll = next;
            self.dirty = true;
        }
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.hscroll = 0;
        self.relayout_keeping_top();
    }

    pub fn toggle_source(&mut self) {
        // (Allowed while loading: applies once the Markdown arrives.)
        if self.doc.is_none() || self.is_markdown() {
            self.source = !self.source;
            self.selection = None;
            self.scroll = 0;
            self.layout_key = None;
            self.ensure_layout();
            self.refresh_search();
            self.dirty = true;
        }
    }

    /// Re-wraps, keeping the first visible line at the top.
    fn relayout_keeping_top(&mut self) {
        let top = self.layout.get(self.scroll).map(|r| Pos {
            line: r.line as usize,
            byte: r.start as usize,
        });
        self.ensure_layout();
        if let Some(top) = top {
            self.scroll = self.row_of(top).min(self.max_scroll());
        }
        self.dirty = true;
    }

    /// Index of the row showing `pos`.
    fn row_of(&self, pos: Pos) -> usize {
        let i = self.layout.partition_point(|r| {
            (r.line as usize) < pos.line
                || (r.line as usize == pos.line && (r.end as usize) <= pos.byte)
        });
        i.min(self.layout.len().saturating_sub(1))
    }

    // ---- drawing ----

    /// Uploads a newly loaded image to the GPU.
    pub fn upload_image(&mut self, renderer: &mut Renderer) {
        if self.image_ready {
            return;
        }
        if let Some(img) = self.image() {
            let levels: Vec<ImageData<'_>> = img
                .levels
                .iter()
                .map(|l| ImageData {
                    width: l.width,
                    height: l.height,
                    rgba: &l.rgba,
                })
                .collect();
            renderer.set_image(self.id, &levels);
            self.image_ready = true;
        }
    }

    /// Scale that fits the image in the body (never enlarging bitmaps).
    fn fit_zoom(&self) -> f32 {
        let Some(img) = self.image() else {
            return 1.0;
        };
        let (w, h) = (img.natural.0.max(1) as f32, img.natural.1.max(1) as f32);
        let pad = 16.0;
        let s = ((self.body.w - 2.0 * pad) / w).min((self.body.h - 2.0 * pad) / h);
        let svg = self.doc.as_ref().is_some_and(|d| d.kind == Kind::Svg);
        if svg { s } else { s.min(1.0) }.max(MIN_ZOOM)
    }

    pub fn effective_zoom(&self) -> f32 {
        self.zoom.unwrap_or_else(|| self.fit_zoom())
    }

    pub fn zoom_by(&mut self, factor: f32) {
        let z = (self.effective_zoom() * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = (self.pan.0 * factor, self.pan.1 * factor);
        self.zoom = Some(z);
    }

    pub fn zoom_in(&mut self) {
        self.zoom_by(ZOOM_STEP);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_by(1.0 / ZOOM_STEP);
    }

    pub fn zoom_fit(&mut self) {
        self.zoom = None;
        self.pan = (0.0, 0.0);
    }

    pub fn zoom_actual(&mut self) {
        self.zoom = Some(1.0);
        self.pan = (0.0, 0.0);
    }

    pub fn image_draw(&self) -> Option<ImageDraw> {
        let img = self.image()?;
        if !self.image_ready {
            return None;
        }
        let z = self.effective_zoom();
        let (w, h) = (img.natural.0 as f32 * z, img.natural.1 as f32 * z);
        let x = self.body.x + (self.body.w - w) / 2.0 + self.pan.0;
        let y = self.body.y + (self.body.h - h) / 2.0 + self.pan.1;
        Some(ImageDraw {
            id: self.id,
            dest: [x.round(), y.round(), w.round().max(1.0), h.round().max(1.0)],
            clip: [self.body.x, self.body.y, self.body.w, self.body.h],
        })
    }

    /// Draws the visible rows into grid `self.id` (only when something changed).
    pub fn render(&mut self, renderer: &mut Renderer, colors: &DocColors, buf: &mut Vec<GridCell>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.ensure_layout();
        for r in 0..self.rows {
            self.row_cells(self.scroll + r, colors, buf);
            renderer.set_row(self.id, r, buf.drain(..));
        }
        renderer.set_cursor(self.id, None);
    }

    fn row_cells(&self, index: usize, colors: &DocColors, out: &mut Vec<GridCell>) {
        out.clear();
        let cols = self.cols;
        let blank = GridCell {
            ch: ' ',
            fg: colors.fg,
            bg: colors.bg,
            style: CellStyle::default(),
        };
        let (Some(text), Some(row)) = (self.text(), self.layout.get(index)) else {
            out.resize(cols, blank);
            return;
        };
        let line_no = row.line as usize;
        let line = &text.lines[line_no];
        let fill = line.fill.unwrap_or(colors.bg);

        // Gutter: line numbers or a margin.
        let gutter = self.gutter();
        let label = if text.gutter && !row.cont {
            format!("{:>w$} ", line_no + 1, w = gutter - 1)
        } else {
            " ".repeat(gutter)
        };
        out.extend(label.chars().map(|ch| GridCell {
            ch,
            fg: colors.muted,
            ..blank
        }));

        let (sel_start, sel_end) = match self.selection {
            Some((a, b)) if a != b => (a.min(b), a.max(b)),
            _ => (Pos::default(), Pos::default()),
        };
        let matches: &[(usize, usize, usize)] = self.search.as_ref().map_or(&[], |s| {
            let from = s.matches.partition_point(|m| m.0 < line_no);
            let to = s.matches.partition_point(|m| m.0 <= line_no);
            &s.matches[from..to]
        });
        let current = self
            .search
            .as_ref()
            .and_then(|s| s.matches.get(s.current))
            .copied();

        let skip = if self.wrap { 0 } else { self.hscroll };
        let mut vx = 0usize;
        let mut emit = |out: &mut Vec<GridCell>, ch: char, style: Style, bg: Option<Rgb8>| {
            let w = char_width(ch);
            if w == 0 {
                return;
            }
            if vx < skip {
                vx += w;
                return;
            }
            vx += w;
            if out.len() + w > cols {
                return;
            }
            let cell = GridCell {
                ch,
                fg: style.fg.unwrap_or(colors.fg),
                bg: bg.or(style.bg).unwrap_or(fill),
                style: CellStyle {
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline,
                    strikeout: style.strike,
                    spacer: false,
                },
            };
            out.push(cell);
            if w == 2 {
                out.push(GridCell {
                    ch: ' ',
                    style: CellStyle {
                        spacer: true,
                        ..CellStyle::default()
                    },
                    ..cell
                });
            }
        };

        if row.cont {
            for (b, ch) in line.cont.text.char_indices() {
                emit(out, ch, line.cont.style_at(b), None);
            }
        }
        let content = &line.content;
        let mut last = (' ', Style::default());
        for (off, ch) in content.text[row.start as usize..row.end as usize].char_indices() {
            let byte = row.start as usize + off;
            let style = content.style_at(byte);
            let pos = Pos {
                line: line_no,
                byte,
            };
            let bg = if pos >= sel_start && pos < sel_end {
                Some(colors.selection)
            } else {
                matches.iter().find(|m| byte >= m.1 && byte < m.2).map(|m| {
                    if Some(*m) == current {
                        colors.current
                    } else {
                        colors.matched
                    }
                })
            };
            emit(out, ch, style, bg);
            last = (ch, style);
        }
        if line.rule {
            while out.len() < cols {
                let before = out.len();
                emit(out, last.0, last.1, None);
                if out.len() == before {
                    break;
                }
            }
        }
        let filler = GridCell { bg: fill, ..blank };
        out.resize(cols, filler);
        out.truncate(cols);
    }

    // ---- header ----

    /// Header buttons with their labels and whether they are "on".
    pub fn buttons(
        &self,
        px: impl Fn(f32) -> f32,
        text_size: f32,
    ) -> Vec<(DocButton, Rect, &'static str, bool, ButtonKind)> {
        use ButtonKind::*;
        let mut list: Vec<(DocButton, &'static str, bool, ButtonKind)> = Vec::new();
        if self.is_markdown() {
            list.push((DocButton::Preview, "Preview", !self.source, Segment));
            list.push((DocButton::Source, "Source", self.source, Segment));
        }
        if self.text().is_some() {
            list.push((
                DocButton::Wrap,
                if self.wrap { "✓ Wrap" } else { "Wrap" },
                self.wrap,
                Toggle,
            ));
        }
        if self.image().is_some() {
            list.push((
                DocButton::Fit,
                if self.zoom.is_none() {
                    "✓ Fit"
                } else {
                    "Fit"
                },
                self.zoom.is_none(),
                Toggle,
            ));
        }
        list.push((DocButton::External, "Default app ↗", false, Action));
        list.push((DocButton::Close, "×", false, Action));
        let h = (self.header.h - px(8.0)).max(1.0);
        let mut right = self.header.right() - px(6.0);
        let mut out = Vec::new();
        let mut prev: Option<ButtonKind> = None;
        for (b, label, on, kind) in list.into_iter().rev() {
            // Segments touch each other; other buttons are spaced.
            if let Some(p) = prev {
                right -= if p == Segment && kind == Segment {
                    0.0
                } else {
                    px(8.0)
                };
            }
            let w = if b == DocButton::Close {
                h
            } else {
                (label.chars().count() as f32 * text_size * 0.58 + px(16.0)).round()
            };
            let r = Rect::new(right - w, self.header.y + (self.header.h - h) / 2.0, w, h);
            if r.x < self.header.x + self.header.w * 0.3 && b != DocButton::Close {
                break;
            }
            out.push((b, r, label, on, kind));
            right -= w;
            prev = Some(kind);
        }
        out.reverse();
        out
    }

    /// One-line description: kind · size · lines / zoom.
    pub fn info(&self) -> String {
        let Some(doc) = &self.doc else {
            return "Loading…".into();
        };
        let mut parts = vec![doc.kind.label().to_owned(), mao_docs::human_size(doc.size)];
        if let Some(t) = self.text() {
            parts.push(format!("{} lines", t.lines.len()));
        }
        if let Some(img) = self.image() {
            parts.push(format!("{}×{}", img.natural.0, img.natural.1));
            parts.push(format!("{:.0}%", self.effective_zoom() * 100.0));
        }
        if doc.truncated {
            parts.push("truncated".into());
        }
        if doc.pending_highlight {
            parts.push("highlighting…".into());
        }
        parts.join("  ·  ")
    }

    // ---- mouse & selection ----

    /// Text position under a pixel, clamped into the visible text.
    fn pos_at(&self, x: f32, y: f32) -> Option<Pos> {
        let text = self.text()?;
        if self.layout.is_empty() {
            return None;
        }
        let r = ((y - self.origin.1) / self.cell.height).floor().max(0.0) as usize;
        let index = (self.scroll + r).min(self.layout.len() - 1);
        let row = self.layout[index];
        let line = &text.lines[row.line as usize];
        let mut col = ((x - self.origin.0) / self.cell.width).floor().max(0.0) as usize;
        col = col.saturating_sub(self.gutter());
        if row.cont {
            col = col.saturating_sub(line.cont.width());
        }
        if !self.wrap {
            col += self.hscroll;
        }
        let byte = layout::byte_at_col(
            &line.content.text,
            row.start as usize,
            row.end as usize,
            col,
        );
        Some(Pos {
            line: row.line as usize,
            byte,
        })
    }

    /// Left button pressed in the body.
    pub fn press(&mut self, x: f32, y: f32, extend: bool) {
        if self.image().is_some() {
            self.panning = Some(((x, y), self.pan));
            return;
        }
        let Some(pos) = self.pos_at(x, y) else {
            return;
        };
        self.selection = match (self.selection, extend) {
            (Some((anchor, _)), true) => Some((anchor, pos)),
            _ => Some((pos, pos)),
        };
        self.selecting = true;
        self.dirty = true;
    }

    /// Pointer moved with the button held. Returns whether to redraw.
    pub fn drag(&mut self, x: f32, y: f32) -> bool {
        if let Some(((sx, sy), (px, py))) = self.panning {
            self.pan = (px + x - sx, py + y - sy);
            return true;
        }
        if !self.selecting {
            return false;
        }
        // Dragging past the edges scrolls.
        if y < self.body.y {
            self.scroll_by(-1);
        } else if y > self.body.bottom() {
            self.scroll_by(1);
        }
        if let (Some(pos), Some((anchor, _))) = (self.pos_at(x, y), self.selection) {
            self.selection = Some((anchor, pos));
            self.dirty = true;
            return true;
        }
        false
    }

    pub fn release(&mut self) {
        self.selecting = false;
        self.panning = None;
    }

    pub fn has_capture(&self) -> bool {
        self.selecting || self.panning.is_some()
    }

    pub fn select_all(&mut self) {
        if let Some(t) = self.text() {
            let last = t.lines.len().saturating_sub(1);
            let end = t.lines.last().map_or(0, |l| l.content.text.len());
            self.selection = Some((
                Pos::default(),
                Pos {
                    line: last,
                    byte: end,
                },
            ));
            self.dirty = true;
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        let (start, end) = (a.min(b), a.max(b));
        if start == end {
            return None;
        }
        let text = self.text()?;
        let mut out = String::new();
        let last = end.line.min(text.lines.len().saturating_sub(1));
        for (line_no, line) in text
            .lines
            .iter()
            .enumerate()
            .take(last + 1)
            .skip(start.line)
        {
            let s = &line.content.text;
            let from = if line_no == start.line {
                start.byte.min(s.len())
            } else {
                0
            };
            let to = if line_no == end.line {
                end.byte.min(s.len())
            } else {
                s.len()
            };
            out.push_str(s.get(from..to).unwrap_or(""));
            if line_no != end.line {
                out.push('\n');
            }
        }
        Some(out)
    }

    pub fn clear_selection(&mut self) -> bool {
        if self.selection.take().is_some() {
            self.dirty = true;
            return true;
        }
        false
    }

    // ---- search ----

    pub fn start_search(&mut self) {
        if self.image().is_none() && self.search.is_none() {
            self.search = Some(Search::default());
        }
    }

    pub fn close_search(&mut self) {
        if self.search.take().is_some() {
            self.dirty = true;
        }
    }

    pub fn search_edit(&mut self, f: impl FnOnce(&mut String)) {
        if let Some(s) = self.search.as_mut() {
            f(&mut s.query);
            s.current = 0;
        }
        self.refresh_search();
        self.reveal_current();
    }

    /// Moves to the next (or previous) match.
    pub fn search_step(&mut self, forward: bool) {
        if let Some(s) = self.search.as_mut()
            && !s.matches.is_empty()
        {
            let n = s.matches.len();
            s.current = if forward {
                (s.current + 1) % n
            } else {
                (s.current + n - 1) % n
            };
        }
        self.reveal_current();
    }

    fn refresh_search(&mut self) {
        let Some(query) = self.search.as_ref().map(|s| s.query.to_ascii_lowercase()) else {
            return;
        };
        let mut matches = Vec::new();
        if !query.is_empty()
            && let Some(text) = self.text()
        {
            for (i, line) in text.lines.iter().enumerate() {
                // ASCII case folding keeps byte offsets valid.
                let hay = line.content.text.to_ascii_lowercase();
                let mut from = 0;
                while let Some(at) = hay[from..].find(&query) {
                    let start = from + at;
                    matches.push((i, start, start + query.len()));
                    from = start + query.len().max(1);
                }
            }
        }
        if let Some(s) = self.search.as_mut() {
            s.current = s.current.min(matches.len().saturating_sub(1));
            s.matches = matches;
        }
        self.dirty = true;
    }

    fn reveal_current(&mut self) {
        let Some(&(line, byte, _)) = self.search.as_ref().and_then(|s| s.matches.get(s.current))
        else {
            return;
        };
        let row = self.row_of(Pos { line, byte });
        if row < self.scroll || row >= self.scroll + self.rows {
            self.scroll = row.saturating_sub(self.rows / 3).min(self.max_scroll());
        }
        if !self.wrap {
            let col = self.text().map_or(0, |t| {
                t.lines[line].content.text[..byte]
                    .chars()
                    .map(char_width)
                    .sum::<usize>()
            });
            let visible = self.text_cols();
            if col < self.hscroll || col >= self.hscroll + visible {
                self.hscroll = col.saturating_sub(visible / 3);
            }
        }
        self.dirty = true;
    }

    /// "3/12"-style search status.
    pub fn search_status(&self) -> Option<String> {
        let s = self.search.as_ref()?;
        Some(if s.query.is_empty() {
            String::new()
        } else if s.matches.is_empty() {
            "no results".into()
        } else {
            format!("{}/{}", s.current + 1, s.matches.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mao_docs::{Line, StyledText};

    fn pane(lines: &[&str]) -> DocPane {
        let mut p = DocPane::new(1, PathBuf::from("a.txt"));
        p.set_document(Arc::new(Document {
            path: PathBuf::from("a.txt"),
            kind: Kind::Text,
            content: Content::Text(TextDoc {
                lines: lines
                    .iter()
                    .map(|l| Line::new(StyledText::plain(*l)))
                    .collect(),
                gutter: true,
            }),
            size: 0,
            truncated: false,
            pending_highlight: false,
        }));
        p.set_rect(
            Rect::new(0.0, 0.0, 100.0, 60.0),
            10.0,
            0.0,
            CellMetrics {
                width: 10.0,
                height: 10.0,
            },
        );
        p
    }

    fn colors() -> DocColors {
        DocColors {
            fg: [200; 3],
            bg: [0; 3],
            muted: [100; 3],
            selection: [50; 3],
            matched: [60; 3],
            current: [70; 3],
        }
    }

    #[test]
    fn rows_have_line_numbers_and_wrap() {
        let p = pane(&["hello world", "x"]);
        // 10 cols, gutter 3 → 7 text columns.
        let mut buf = Vec::new();
        let text = |p: &DocPane, i, buf: &mut Vec<GridCell>| {
            p.row_cells(i, &colors(), buf);
            buf.iter().map(|c| c.ch).collect::<String>()
        };
        assert_eq!(text(&p, 0, &mut buf), " 1 hello  ");
        assert_eq!(text(&p, 1, &mut buf), "   world  ");
        assert_eq!(text(&p, 2, &mut buf), " 2 x      ");
        assert_eq!(buf.len(), 10);
    }

    #[test]
    fn selection_and_copy() {
        let mut p = pane(&["abc", "def"]);
        p.selection = Some((Pos { line: 0, byte: 1 }, Pos { line: 1, byte: 2 }));
        assert_eq!(p.selected_text().as_deref(), Some("bc\nde"));
        p.select_all();
        assert_eq!(p.selected_text().as_deref(), Some("abc\ndef"));
    }

    #[test]
    fn search_finds_and_steps() {
        let mut p = pane(&["Foo foo", "bar", "FOO"]);
        p.start_search();
        p.search_edit(|q| q.push_str("foo"));
        let s = p.search.as_ref().unwrap();
        assert_eq!(s.matches, [(0, 0, 3), (0, 4, 7), (2, 0, 3)]);
        p.search_step(false);
        assert_eq!(p.search.as_ref().unwrap().current, 2);
        assert_eq!(p.search_status().as_deref(), Some("3/3"));
    }

    #[test]
    fn scrolling_is_clamped() {
        let mut p = pane(&["1", "2", "3", "4", "5", "6", "7", "8"]);
        // 5 rows visible.
        p.scroll_by(100);
        assert_eq!(p.scroll, 3);
        p.scroll_by(-100);
        assert_eq!(p.scroll, 0);
    }
}
