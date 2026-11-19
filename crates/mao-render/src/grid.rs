//! Cell grid rendering: per-row text buffers and background quads, rebuilt only for dirty rows.

use std::ops::Range;
use std::thread::JoinHandle;

use glyphon::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, Weight, Wrap};

use crate::color::to_target;
use crate::quad::push_quad;

/// sRGB color.
pub type Rgb8 = [u8; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// Second half of a wide glyph: background only.
    pub spacer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    pub ch: char,
    pub fg: Rgb8,
    pub bg: Rgb8,
    pub style: CellStyle,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: [255; 3],
            bg: [0; 3],
            style: CellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// Already drawn through the cell colors; nothing extra is drawn.
    Block,
    HollowBlock,
    Beam,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub color: Rgb8,
    pub wide: bool,
}

/// Cell size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Loads the system fonts on a background thread (it can take tens of ms),
/// overlapping with window and GPU creation.
pub struct FontLoader(JoinHandle<FontSystem>);

impl FontLoader {
    pub fn start() -> Self {
        Self(
            std::thread::Builder::new()
                .name("font-loader".into())
                .spawn(FontSystem::new)
                .unwrap_or_else(|_| std::thread::spawn(FontSystem::new)),
        )
    }

    pub(crate) fn finish(self) -> FontSystem {
        self.0.join().unwrap_or_else(|_| {
            tracing::error!("font loader thread panicked; loading fonts synchronously");
            FontSystem::new()
        })
    }
}

/// Monospace families tried, in order, when the configured one is missing.
const FALLBACK_FAMILIES: &[&str] = &[
    "JetBrains Mono",
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Ubuntu Mono",
];

pub(crate) struct Font {
    /// Resolved family name; `None` = generic monospace.
    family: Option<String>,
    size_pt: f32,
    scale: f32,
    metrics: Metrics,
    pub(crate) cell: CellMetrics,
}

impl Font {
    pub(crate) fn new(
        font_system: &mut FontSystem,
        family: &str,
        size_pt: f32,
        scale: f32,
    ) -> Self {
        let family = resolve_family(font_system, family);
        let mut font = Self {
            family,
            size_pt,
            scale,
            metrics: Metrics::new(1.0, 1.0),
            cell: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
        };
        font.measure(font_system);
        font
    }

    pub(crate) fn set_size(&mut self, font_system: &mut FontSystem, size_pt: f32, scale: f32) {
        self.size_pt = size_pt;
        self.scale = scale;
        self.measure(font_system);
    }

    pub(crate) fn size_pt(&self) -> f32 {
        self.size_pt
    }

    /// Resolved family name (`None` = generic monospace).
    pub(crate) fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    fn attrs(&self) -> Attrs<'_> {
        let family = match &self.family {
            Some(name) => Family::Name(name),
            None => Family::Monospace,
        };
        Attrs::new().family(family)
    }

    fn measure(&mut self, font_system: &mut FontSystem) {
        // Points → pixels at 96 DPI, times the display scale.
        let px = (self.size_pt * 96.0 / 72.0 * self.scale).max(1.0);
        let height = (px * 1.25).ceil();
        self.metrics = Metrics::new(px, height);

        let mut probe = Buffer::new(font_system, self.metrics);
        probe.set_text("0", &self.attrs(), Shaping::Advanced, None);
        probe.shape_until_scroll(font_system, false);
        let advance = probe
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|g| g.w))
            .unwrap_or(px * 0.6);
        // Use the font's exact advance, not a rounded one: glyphs are laid out with
        // their natural advance, so a rounded cell width drifts away from the text
        // (≈0.4 px per column with a 12.6 px advance). cosmic-text 0.19's
        // `monospace_width` snapping doesn't help: it rounds pixels to em units.
        self.cell = CellMetrics {
            width: advance.max(1.0),
            height,
        };
        tracing::debug!(family = ?self.family, px, cell = ?self.cell, "font measured");
    }
}

fn resolve_family(font_system: &FontSystem, wanted: &str) -> Option<String> {
    let installed = |name: &str| {
        font_system.db().faces().any(|face| {
            face.families
                .iter()
                .any(|(f, _)| f.eq_ignore_ascii_case(name))
        })
    };
    let found = std::iter::once(wanted)
        .chain(FALLBACK_FAMILIES.iter().copied())
        .find(|name| installed(name))
        .map(str::to_owned);
    match &found {
        Some(name) if !name.eq_ignore_ascii_case(wanted) => {
            tracing::warn!(wanted, using = %name, "font not found, using fallback");
        }
        None => tracing::warn!(
            wanted,
            "no known monospace font found, using generic monospace"
        ),
        Some(_) => {}
    }
    found
}

/// A run of text anchored at a grid column.
///
/// Glyphs are laid out with their natural advance, so text drifts off the cell
/// grid wherever a glyph's advance isn't exactly one cell (wide CJK, emoji,
/// symbols from a fallback font). Each such glyph gets its own segment pinned
/// to its column; ordinary text stays in one segment per row.
struct Segment {
    col: usize,
    buffer: Buffer,
}

struct Row {
    cells: Vec<GridCell>,
    /// Segment pool; only the first `used` are current.
    segments: Vec<Segment>,
    used: usize,
    /// Background/decoration quads for this row, in target color space.
    quads: Vec<f32>,
    dirty: bool,
}

impl Row {
    fn new(cols: usize) -> Self {
        Self {
            cells: vec![GridCell::default(); cols],
            segments: Vec::new(),
            used: 0,
            quads: Vec::new(),
            dirty: true,
        }
    }

    /// Next segment from the pool, starting at `col`.
    fn next_segment(&mut self, col: usize, font: &Font) -> &mut Segment {
        if self.used == self.segments.len() {
            let mut buffer = Buffer::new_empty(font.metrics);
            buffer.set_wrap(Wrap::None);
            self.segments.push(Segment { col, buffer });
        }
        let seg = &mut self.segments[self.used];
        seg.col = col;
        self.used += 1;
        seg
    }
}

/// Glyphs whose advance is unlikely to be exactly one cell of the primary font.
fn needs_own_segment(c: char) -> bool {
    use unicode_width::UnicodeWidthChar;
    let cp = c as u32;
    c.width() == Some(2)
        // Misc symbols and dingbats (✓ ★ …) usually come from a fallback font.
        || (0x2600..=0x27BF).contains(&cp)
        || cp >= 0x1F000
}

/// Key identifying a run of cells that share text attributes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SpanKey {
    fg: Rgb8,
    bold: bool,
    italic: bool,
}

pub(crate) struct Grid {
    rows: Vec<Row>,
    cols: usize,
    origin: (f32, f32),
    /// Text is clipped to this rectangle (the pane), in pixels.
    clip: glyphon::TextBounds,
    cursor: Option<Cursor>,
    // Scratch buffers reused across row rebuilds.
    text: String,
    spans: Vec<(Range<usize>, SpanKey)>,
}

impl Grid {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            cols: 0,
            origin: (0.0, 0.0),
            clip: glyphon::TextBounds::default(),
            cursor: None,
            text: String::new(),
            spans: Vec::new(),
        }
    }

    pub(crate) fn set_layout(
        &mut self,
        origin: (f32, f32),
        cols: usize,
        rows: usize,
        clip: [f32; 4],
    ) {
        if origin != self.origin {
            // Quads hold absolute positions.
            self.mark_all_dirty();
        }
        self.origin = origin;
        self.clip = glyphon::TextBounds {
            left: clip[0] as i32,
            top: clip[1] as i32,
            right: (clip[0] + clip[2]) as i32,
            bottom: (clip[1] + clip[3]) as i32,
        };
        if cols != self.cols {
            self.cols = cols;
            for row in &mut self.rows {
                row.cells.resize(cols, GridCell::default());
                row.dirty = true;
            }
        }
        self.rows.truncate(rows);
        while self.rows.len() < rows {
            self.rows.push(Row::new(cols));
        }
    }

    /// Applies new font metrics to every row.
    pub(crate) fn set_font(&mut self, font: &Font) {
        for row in &mut self.rows {
            for seg in &mut row.segments {
                seg.buffer.set_metrics(font.metrics);
            }
            row.dirty = true;
        }
    }

    pub(crate) fn mark_all_dirty(&mut self) {
        for row in &mut self.rows {
            row.dirty = true;
        }
    }

    pub(crate) fn set_row(&mut self, index: usize, cells: impl IntoIterator<Item = GridCell>) {
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };
        row.cells.clear();
        row.cells.extend(cells.into_iter().take(self.cols));
        row.cells.resize(self.cols, GridCell::default());
        row.dirty = true;
    }

    pub(crate) fn set_cursor(&mut self, cursor: Option<Cursor>) {
        self.cursor = cursor;
    }

    /// Reshapes dirty rows. `background` cells are not drawn (the clear covers them).
    pub(crate) fn update(
        &mut self,
        font_system: &mut FontSystem,
        font: &Font,
        background: Rgb8,
        srgb: bool,
    ) {
        let cell = font.cell;
        for (index, row) in self.rows.iter_mut().enumerate() {
            if !row.dirty {
                continue;
            }
            row.dirty = false;
            let y = self.origin.1 + index as f32 * cell.height;

            // Text, in segments anchored to their columns, each grouped into
            // spans of equal attributes.
            row.used = 0;
            let base = font.attrs();
            let mut col = 0;
            while col < row.cells.len() {
                let seg_start = col;
                self.text.clear();
                self.spans.clear();
                while col < row.cells.len() {
                    let c = &row.cells[col];
                    let own = needs_own_segment(c.ch);
                    if own && col > seg_start {
                        break; // Close the current run; this glyph gets its own.
                    }
                    col += 1;
                    if c.style.spacer {
                        continue;
                    }
                    let key = SpanKey {
                        fg: c.fg,
                        bold: c.style.bold,
                        italic: c.style.italic,
                    };
                    let start = self.text.len();
                    self.text.push(if c.ch == '\0' { ' ' } else { c.ch });
                    match self.spans.last_mut() {
                        Some((range, k)) if *k == key => range.end = self.text.len(),
                        _ => self.spans.push((start..self.text.len(), key)),
                    }
                    if own {
                        break;
                    }
                }
                // Blank runs need no text.
                if self.text.trim().is_empty() {
                    continue;
                }
                let text = &self.text;
                let seg = row.next_segment(seg_start, font);
                seg.buffer.set_rich_text(
                    self.spans.iter().map(|(range, key)| {
                        let mut attrs = base
                            .clone()
                            .color(Color::rgb(key.fg[0], key.fg[1], key.fg[2]));
                        if key.bold {
                            attrs = attrs.weight(Weight::BOLD);
                        }
                        if key.italic {
                            attrs = attrs.style(Style::Italic);
                        }
                        (&text[range.clone()], attrs)
                    }),
                    &base,
                    Shaping::Advanced,
                    None,
                );
                seg.buffer.shape_until_scroll(font_system, false);
            }

            // Backgrounds (merged runs) and decorations.
            row.quads.clear();
            let mut col = 0;
            while col < row.cells.len() {
                let bg = row.cells[col].bg;
                let start = col;
                while col < row.cells.len() && row.cells[col].bg == bg {
                    col += 1;
                }
                if bg != background {
                    let x = self.origin.0 + start as f32 * cell.width;
                    let w = (col - start) as f32 * cell.width;
                    push_quad(&mut row.quads, x, y, w, cell.height, to_target(bg, srgb));
                }
            }
            let thickness = (cell.height / 16.0).round().max(1.0);
            for (col, c) in row.cells.iter().enumerate() {
                let x = self.origin.0 + col as f32 * cell.width;
                if c.style.underline {
                    let uy = y + cell.height - thickness * 2.0;
                    push_quad(
                        &mut row.quads,
                        x,
                        uy,
                        cell.width,
                        thickness,
                        to_target(c.fg, srgb),
                    );
                }
                if c.style.strikeout {
                    let sy = y + (cell.height / 2.0).round();
                    push_quad(
                        &mut row.quads,
                        x,
                        sy,
                        cell.width,
                        thickness,
                        to_target(c.fg, srgb),
                    );
                }
            }
        }
    }

    /// Appends all quads (rows + cursor) to `out`.
    pub(crate) fn collect_quads(&self, font: &Font, srgb: bool, out: &mut Vec<f32>) {
        for row in &self.rows {
            out.extend_from_slice(&row.quads);
        }
        let Some(cursor) = self.cursor else {
            return;
        };
        let cell = font.cell;
        let x = self.origin.0 + cursor.col as f32 * cell.width;
        let y = self.origin.1 + cursor.row as f32 * cell.height;
        let w = if cursor.wide {
            cell.width * 2.0
        } else {
            cell.width
        };
        let t = (cell.height / 12.0).round().max(1.0);
        let color = to_target(cursor.color, srgb);
        match cursor.shape {
            CursorShape::Block => {}
            CursorShape::Beam => push_quad(out, x, y, t.max(2.0), cell.height, color),
            CursorShape::Underline => {
                push_quad(out, x, y + cell.height - t * 2.0, w, t * 2.0, color)
            }
            CursorShape::HollowBlock => {
                push_quad(out, x, y, w, t, color);
                push_quad(out, x, y + cell.height - t, w, t, color);
                push_quad(out, x, y, t, cell.height, color);
                push_quad(out, x + w - t, y, t, cell.height, color);
            }
        }
    }

    /// Text areas for every row, clipped to the grid's rectangle.
    pub(crate) fn text_areas(&self, font: &Font) -> impl Iterator<Item = glyphon::TextArea<'_>> {
        let bounds = self.clip;
        let (ox, oy) = self.origin;
        let height = font.cell.height;
        let width = font.cell.width;
        self.rows.iter().enumerate().flat_map(move |(i, row)| {
            row.segments[..row.used]
                .iter()
                .map(move |seg| glyphon::TextArea {
                    buffer: &seg.buffer,
                    left: ox + seg.col as f32 * width,
                    top: oy + i as f32 * height,
                    scale: 1.0,
                    bounds,
                    default_color: Color::rgb(255, 255, 255),
                    custom_glyphs: &[],
                })
        })
    }
}
