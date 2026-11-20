//! UI layer: solid rectangles and short text labels (tab bar, status bar, palette…),
//! drawn on top of the terminal grids. Text buffers are cached by key and only
//! reshaped when their content or style changes.

use std::collections::HashMap;

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, TextArea, TextBounds, Weight, Wrap,
};

use crate::color::to_target;
use crate::grid::Rgb8;
use crate::quad::push_quad;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Rgb8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiText {
    /// Stable identity used to cache the shaped text across frames.
    pub key: u64,
    pub text: String,
    /// Box the text is laid out and clipped in; vertically centered.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Font size in pixels.
    pub size: f32,
    pub color: Rgb8,
    pub bold: bool,
    /// Use the terminal (monospace) font instead of the UI font.
    pub mono: bool,
    /// Use the icon font (see [`crate::Renderer::has_icon_font`]).
    pub icon: bool,
    pub align: Align,
}

/// Layers rendered at most: base, floating (menus, settings), and a popup
/// over a floating one (a picker opened from the settings).
pub const MAX_LAYERS: usize = 3;

/// Everything the UI draws in one frame. Within a layer all rectangles are
/// drawn before all text; floating things (menus, palette) go in [`Self::top`]
/// so that they cover the layer below, text included (up to [`MAX_LAYERS`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiFrame {
    pub rects: Vec<UiRect>,
    pub texts: Vec<UiText>,
    /// The layer drawn above this one.
    top: Option<Box<UiFrame>>,
}

impl UiFrame {
    pub fn clear(&mut self) {
        self.rects.clear();
        self.texts.clear();
        if let Some(top) = self.top.as_mut() {
            top.clear();
        }
    }

    /// The overlay layer (menus, popups).
    pub fn top(&mut self) -> &mut UiFrame {
        self.top.get_or_insert_with(Box::default)
    }

    /// This layer and the ones above it, bottom first.
    pub(crate) fn layers(&self) -> Vec<&UiFrame> {
        let mut out = vec![self];
        while out.len() < MAX_LAYERS {
            match out[out.len() - 1].top.as_deref() {
                Some(t) => out.push(t),
                None => break,
            }
        }
        out
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb8) {
        self.rects.push(UiRect { x, y, w, h, color });
    }
}

struct Cached {
    buffer: Buffer,
    text: String,
    size: f32,
    color: Rgb8,
    bold: bool,
    mono: bool,
    icon: bool,
    width: f32,
    used: bool,
}

/// UI font families tried in order.
const UI_FAMILIES: &[&str] = &[
    "Segoe UI",
    "Inter",
    "Cantarell",
    "Noto Sans",
    "Ubuntu",
    "DejaVu Sans",
];

/// Icon fonts tried in order (Windows 10/11 ship them).
const ICON_FAMILIES: &[&str] = &["Segoe Fluent Icons", "Segoe MDL2 Assets"];

pub(crate) struct UiLayer {
    cache: HashMap<u64, Cached>,
    family: Option<String>,
    icon_family: Option<String>,
}

impl UiLayer {
    pub(crate) fn new(font_system: &FontSystem) -> Self {
        let find = |names: &[&str]| {
            names
                .iter()
                .find(|name| {
                    font_system
                        .db()
                        .faces()
                        .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
                })
                .map(|s| (*s).to_owned())
        };
        Self {
            cache: HashMap::new(),
            family: find(UI_FAMILIES),
            icon_family: find(ICON_FAMILIES),
        }
    }

    pub(crate) fn has_icon_font(&self) -> bool {
        self.icon_family.is_some()
    }

    /// Shapes changed labels and appends every layer's rectangles to `quads`,
    /// bottom layer first. Returns where each layer's quads start.
    pub(crate) fn update(
        &mut self,
        font_system: &mut FontSystem,
        mono_family: Option<&str>,
        frame: &UiFrame,
        srgb: bool,
        opacity: f32,
        quads: &mut Vec<f32>,
    ) -> Vec<usize> {
        let layers = frame.layers();
        let mut starts = Vec::with_capacity(layers.len());
        for (level, layer) in layers.iter().enumerate() {
            starts.push(quads.len());
            for r in &layer.rects {
                let mut color = to_target(r.color, srgb);
                // The base layer is translucent with the window; popups stay opaque.
                if level == 0 {
                    color[3] = opacity;
                }
                push_quad(quads, r.x, r.y, r.w, r.h, color);
            }
        }
        for c in self.cache.values_mut() {
            c.used = false;
        }
        let all_texts = layers.iter().flat_map(|l| l.texts.iter());
        for t in all_texts {
            let line_height = (t.size * 1.3).ceil();
            let entry = self.cache.entry(t.key).or_insert_with(|| Cached {
                buffer: Buffer::new_empty(Metrics::new(t.size, line_height)),
                text: String::new(),
                size: 0.0,
                color: [0; 3],
                bold: false,
                mono: false,
                icon: false,
                width: 0.0,
                used: false,
            });
            entry.used = true;
            let changed = entry.text != t.text
                || entry.size != t.size
                || entry.color != t.color
                || entry.bold != t.bold
                || entry.mono != t.mono
                || entry.icon != t.icon;
            if !changed {
                continue;
            }
            let family = if t.icon && self.icon_family.is_some() {
                self.icon_family
                    .as_deref()
                    .map_or(Family::SansSerif, Family::Name)
            } else if t.mono {
                mono_family.map_or(Family::Monospace, Family::Name)
            } else {
                self.family
                    .as_deref()
                    .map_or(Family::SansSerif, Family::Name)
            };
            let mut attrs = Attrs::new()
                .family(family)
                .color(Color::rgb(t.color[0], t.color[1], t.color[2]));
            if t.bold {
                attrs = attrs.weight(Weight::SEMIBOLD);
            }
            entry.buffer.set_metrics(Metrics::new(t.size, line_height));
            entry.buffer.set_wrap(Wrap::None);
            entry
                .buffer
                .set_text(&t.text, &attrs, Shaping::Advanced, None);
            entry.buffer.shape_until_scroll(font_system, false);
            entry.width = entry
                .buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0, f32::max);
            entry.text.clone_from(&t.text);
            entry.size = t.size;
            entry.color = t.color;
            entry.bold = t.bold;
            entry.mono = t.mono;
            entry.icon = t.icon;
        }
        self.cache.retain(|_, c| c.used);
        starts
    }

    pub(crate) fn text_areas<'a>(
        &'a self,
        frame: &'a UiFrame,
    ) -> impl Iterator<Item = TextArea<'a>> {
        frame.texts.iter().filter_map(|t| {
            let c = self.cache.get(&t.key)?;
            let left = match t.align {
                Align::Left => t.x,
                Align::Center => t.x + ((t.w - c.width) / 2.0).max(0.0),
                Align::Right => t.x + (t.w - c.width).max(0.0),
            };
            let line_height = (t.size * 1.3).ceil();
            let top = t.y + ((t.h - line_height) / 2.0).round();
            Some(TextArea {
                buffer: &c.buffer,
                left: left.round(),
                top,
                scale: 1.0,
                bounds: TextBounds {
                    left: t.x as i32,
                    top: t.y as i32,
                    right: (t.x + t.w) as i32,
                    bottom: (t.y + t.h) as i32,
                },
                default_color: Color::rgb(t.color[0], t.color[1], t.color[2]),
                custom_glyphs: &[],
            })
        })
    }
}
