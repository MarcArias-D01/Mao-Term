//! Window chrome: tab bar, status bar, pane dividers and the command palette.
//! Geometry (for hit-testing) and drawing (into a `UiFrame`).

use mao_config::ThemeConfig;
use mao_render::{Align, Rgb8, UiFrame, UiText};
use mao_ui::{Divider, PaneId, Rect};

use crate::editor::{MENU_ROWS, Menu};
use crate::palette::{MAX_VISIBLE, Palette};
use crate::workspace::Workspace;

/// Pixel sizes derived from the display scale.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub scale: f32,
    pub ui_text: f32,
    pub tab_bar: f32,
    pub status_bar: f32,
    /// Divider thickness between panes.
    pub gap: f32,
    /// Padding inside each pane around the grid.
    pub pad: f32,
}

impl Metrics {
    pub fn new(scale: f32, padding: f32) -> Self {
        let px = |v: f32| (v * scale).round();
        Self {
            scale,
            ui_text: 13.0 * scale,
            tab_bar: px(36.0),
            status_bar: px(24.0),
            gap: px(1.0).max(1.0),
            pad: px(padding),
        }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }
}

/// Where things are in the window.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub tab_bar: Rect,
    pub content: Rect,
    pub status: Rect,
    pub tabs: Vec<Rect>,
    pub closes: Vec<Rect>,
    pub plus: Rect,
}

impl Layout {
    pub fn new(window: (u32, u32), m: &Metrics, tab_count: usize) -> Self {
        let full = Rect::new(0.0, 0.0, window.0 as f32, window.1 as f32);
        let (tab_bar, rest) = full.split_top(m.tab_bar);
        let (content, status) = rest.split_bottom(m.status_bar);

        let plus_w = m.tab_bar;
        let available = (tab_bar.w - plus_w - m.px(8.0)).max(0.0);
        let tab_w = if tab_count == 0 {
            0.0
        } else {
            // Shrink as needed so tabs never overflow the bar.
            (available / tab_count as f32).min(m.px(220.0)).floor()
        };
        let close_w = m.px(28.0);
        let mut tabs = Vec::with_capacity(tab_count);
        let mut closes = Vec::with_capacity(tab_count);
        for i in 0..tab_count {
            let r = Rect::new(i as f32 * tab_w, 0.0, tab_w, tab_bar.h);
            closes.push(Rect::new(r.right() - close_w, r.y, close_w, r.h));
            tabs.push(r);
        }
        let plus = Rect::new(tab_count as f32 * tab_w, 0.0, plus_w, tab_bar.h);
        Self {
            tab_bar,
            content,
            status,
            tabs,
            closes,
            plus,
        }
    }
}

/// What is under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum Hit {
    None,
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Divider(Divider),
    Pane(PaneId),
    PaletteRow(usize),
    /// Inside the palette box but not on a row.
    Palette,
}

pub fn hit_test(
    x: f32,
    y: f32,
    layout: &Layout,
    m: &Metrics,
    ws: &Workspace,
    palette: Option<&Palette>,
) -> Hit {
    if let Some(p) = palette {
        let geo = PaletteGeometry::new(layout, m, p);
        if geo.frame.contains(x, y) {
            return geo
                .rows
                .iter()
                .position(|r| r.contains(x, y))
                .map_or(Hit::Palette, Hit::PaletteRow);
        }
    }
    if layout.tab_bar.contains(x, y) {
        for (i, tab) in layout.tabs.iter().enumerate() {
            if tab.contains(x, y) {
                return if layout.closes[i].contains(x, y) {
                    Hit::CloseTab(i)
                } else {
                    Hit::Tab(i)
                };
            }
        }
        return if layout.plus.contains(x, y) {
            Hit::NewTab
        } else {
            Hit::None
        };
    }
    let Some(tab) = ws.active_tab() else {
        return Hit::None;
    };
    if tab.zoomed.is_none() {
        // Dividers are 1px; give them a comfortable grab zone.
        let grab = m.px(4.0);
        for d in tab.tree.dividers(layout.content, m.gap) {
            if d.rect.inset(-grab).contains(x, y) {
                return Hit::Divider(d);
            }
        }
    }
    tab.layout(layout.content, m.gap)
        .into_iter()
        .find(|(_, r)| r.contains(x, y))
        .map_or(Hit::None, |(id, _)| Hit::Pane(id))
}

/// Colors derived from the terminal theme.
pub struct Colors {
    bg: Rgb8,
    fg: Rgb8,
    dim: Rgb8,
    accent: Rgb8,
    selection: Rgb8,
    chrome: Rgb8,
    hover: Rgb8,
    divider: Rgb8,
    raised: Rgb8,
    branch: Rgb8,
    option: Rgb8,
}

impl Colors {
    pub fn new(theme: &ThemeConfig) -> Self {
        let bg = theme.background.to_array();
        let fg = theme.foreground.to_array();
        let chrome = mix(bg, [0, 0, 0], 0.28);
        Self {
            bg,
            fg,
            dim: mix(fg, bg, 0.45),
            accent: theme.cursor.to_array(),
            selection: theme.selection.to_array(),
            chrome,
            hover: mix(chrome, fg, 0.07),
            divider: mix(bg, fg, 0.16),
            raised: mix(bg, fg, 0.06),
            branch: theme.ansi[10].to_array(),
            option: theme.ansi[14].to_array(),
        }
    }
}

fn mix(a: Rgb8, b: Rgb8, t: f32) -> Rgb8 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

// Text cache keys (stable across frames).
const KEY_TAB_TITLE: u64 = 1 << 48;
const KEY_TAB_CLOSE: u64 = 2 << 48;
const KEY_PLUS: u64 = 3 << 48;
const KEY_STATUS: u64 = 4 << 48;
const KEY_PALETTE: u64 = 5 << 48;
const KEY_PALETTE_ROW: u64 = 6 << 48;
const KEY_PALETTE_SHORTCUT: u64 = 7 << 48;
const KEY_MENU_LABEL: u64 = 8 << 48;
const KEY_MENU_DETAIL: u64 = 9 << 48;
const KEY_MENU_COUNT: u64 = 10 << 48;

/// The completion menu of the focused pane and where to put it.
pub struct MenuView<'a> {
    pub menu: &'a Menu,
    /// Left edge (the completed token's first cell).
    pub x: f32,
    /// Top of the row below the caret.
    pub below: f32,
    /// Top of the caret row (the menu goes above it if there's no room below).
    pub above: f32,
}

pub struct DrawState<'a> {
    pub layout: &'a Layout,
    pub metrics: &'a Metrics,
    pub colors: &'a Colors,
    pub workspace: &'a Workspace,
    pub palette: Option<&'a Palette>,
    pub hover: &'a Hit,
    pub window_focused: bool,
    pub menu: Option<MenuView<'a>>,
}

pub fn draw(ui: &mut UiFrame, s: &DrawState<'_>) {
    ui.clear();
    draw_tab_bar(ui, s);
    draw_panes_decor(ui, s);
    draw_status(ui, s);
    if let Some(menu) = &s.menu {
        draw_menu(ui, s, menu);
    }
    if let Some(p) = s.palette {
        draw_palette(ui, s, p);
    }
}

fn text(
    key: u64,
    text: impl Into<String>,
    r: Rect,
    size: f32,
    color: Rgb8,
    align: Align,
) -> UiText {
    UiText {
        key,
        text: text.into(),
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        size,
        color,
        bold: false,
        mono: false,
        align,
    }
}

fn draw_tab_bar(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c, ws) = (s.layout, s.metrics, s.colors, s.workspace);
    let bar = l.tab_bar;
    ui.rect(bar.x, bar.y, bar.w, bar.h, c.chrome);

    for (i, tab) in ws.tabs.iter().enumerate() {
        let r = l.tabs[i];
        let active = i == ws.active;
        let hovered = matches!(s.hover, Hit::Tab(h) | Hit::CloseTab(h) if *h == i);
        if active {
            ui.rect(r.x, r.y, r.w, r.h, c.bg);
            ui.rect(r.x, r.y, r.w, m.px(2.0), c.accent);
        } else if hovered {
            ui.rect(r.x, r.y, r.w, r.h, c.hover);
        } else if i + 1 != ws.active && i + 1 < ws.tabs.len() {
            // Thin separator between two inactive tabs.
            let h = (r.h * 0.45).round();
            ui.rect(
                r.right() - m.gap,
                r.y + (r.h - h) / 2.0,
                m.gap,
                h,
                c.divider,
            );
        }

        let title = ws
            .panes
            .get(&tab.focused)
            .map_or_else(|| "shell".to_owned(), |p| p.display_name());
        let pad = m.px(14.0);
        let mut text_x = r.x + pad;
        if tab.activity && !active {
            let d = m.px(6.0);
            ui.rect(r.x + pad, r.y + (r.h - d) / 2.0, d, d, c.accent);
            text_x += d + m.px(8.0);
        }
        let label = if tab.zoomed.is_some() {
            format!("{title} (max)")
        } else if tab.tree.len() > 1 {
            format!("{title} +{}", tab.tree.len() - 1)
        } else {
            title.clone()
        };
        let text_w = (l.closes[i].x - text_x).max(0.0);
        let color = if active { c.fg } else { c.dim };
        ui.texts.push(text(
            KEY_TAB_TITLE | tab.id,
            label,
            Rect::new(text_x, r.y, text_w, r.h),
            m.ui_text,
            color,
            Align::Left,
        ));

        if active || hovered {
            let close = l.closes[i];
            let close_hovered = *s.hover == Hit::CloseTab(i);
            if close_hovered {
                let d = m.px(20.0);
                ui.rect(
                    close.x + (close.w - d) / 2.0,
                    close.y + (close.h - d) / 2.0,
                    d,
                    d,
                    c.hover,
                );
            }
            ui.texts.push(text(
                KEY_TAB_CLOSE | i as u64,
                "×",
                close,
                m.ui_text * 1.2,
                if close_hovered { c.fg } else { c.dim },
                Align::Center,
            ));
        }
    }

    if *s.hover == Hit::NewTab {
        let p = l.plus;
        ui.rect(p.x, p.y, p.w, p.h, c.hover);
    }
    ui.texts.push(text(
        KEY_PLUS,
        "+",
        l.plus,
        m.ui_text * 1.35,
        c.dim,
        Align::Center,
    ));
}

fn draw_panes_decor(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let Some(tab) = s.workspace.active_tab() else {
        return;
    };
    if tab.zoomed.is_some() {
        return;
    }
    let dividers = tab.tree.dividers(l.content, m.gap);
    for d in &dividers {
        let color = if matches!(s.hover, Hit::Divider(h) if h.path == d.path) {
            c.accent
        } else {
            c.divider
        };
        ui.rect(d.rect.x, d.rect.y, d.rect.w, d.rect.h, color);
    }
    if !dividers.is_empty() && s.window_focused {
        // Mark the focused pane with an accent line on top.
        if let Some((_, r)) = tab
            .layout(l.content, m.gap)
            .into_iter()
            .find(|(id, _)| *id == tab.focused)
        {
            ui.rect(r.x, r.y, r.w, m.px(2.0), c.accent);
        }
    }
}

fn draw_status(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c, ws) = (s.layout, s.metrics, s.colors, s.workspace);
    let r = l.status;
    ui.rect(r.x, r.y, r.w, r.h, c.chrome);
    ui.rect(r.x, r.y, r.w, m.gap, c.divider);
    let pad = m.px(12.0);
    let inner = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad).max(0.0), r.h);
    let size = m.ui_text * 0.92;

    let left = ws.focused_pane().map_or(String::new(), |p| {
        let place = p
            .cwd
            .as_ref()
            .map_or_else(|| p.title.clone(), |c| c.display().to_string());
        match p.last_exit {
            Some(code) if code != 0 => format!("{place}   ✗ exit {code}"),
            _ => place,
        }
    });
    ui.texts.push(text(
        KEY_STATUS,
        left,
        Rect::new(inner.x, inner.y, inner.w * 0.6, inner.h),
        size,
        c.dim,
        Align::Left,
    ));

    let right = match (ws.active_tab(), ws.focused_pane()) {
        (Some(tab), Some(pane)) => {
            let grid = pane.terminal.size();
            let panes = tab.tree.len();
            let index = tab
                .tree
                .panes()
                .iter()
                .position(|&id| id == tab.focused)
                .unwrap_or(0);
            format!(
                "tab {}/{}  ·  pane {}/{}  ·  {}×{}",
                ws.active + 1,
                ws.tabs.len(),
                index + 1,
                panes,
                grid.cols,
                grid.rows
            )
        }
        _ => String::new(),
    };
    ui.texts.push(text(
        KEY_STATUS | 1,
        right,
        inner,
        size,
        c.dim,
        Align::Right,
    ));
}

fn draw_menu(ui: &mut UiFrame, s: &DrawState<'_>, v: &MenuView<'_>) {
    let (m, c, l) = (s.metrics, s.colors, s.layout);
    let items = &v.menu.completions.items;
    if items.is_empty() {
        return;
    }
    let rows = items.len().min(MENU_ROWS);
    let row_h = m.px(26.0);
    let footer = if items.len() > MENU_ROWS {
        m.px(22.0)
    } else {
        0.0
    };
    let pad = m.px(4.0);
    let h = rows as f32 * row_h + 2.0 * pad + footer;
    let w = m.px(440.0).min(l.content.w - m.px(16.0)).max(m.px(120.0));
    // Align the labels (inset by `pad` + 8px) with the token being completed.
    let x = (v.x - pad - m.px(8.0))
        .min(l.content.right() - w - m.px(8.0))
        .max(l.content.x + m.px(4.0));
    let y = if v.below + h <= l.content.bottom() {
        v.below + m.px(2.0)
    } else {
        (v.above - h - m.px(2.0)).max(l.content.y)
    };
    let b = m.gap;
    ui.rect(x - b, y - b, w + 2.0 * b, h + 2.0 * b, c.divider);
    ui.rect(x, y, w, h, c.raised);

    let start = v.menu.scroll.min(items.len().saturating_sub(rows));
    for (i, item) in items.iter().enumerate().skip(start).take(rows) {
        let ry = y + pad + (i - start) as f32 * row_h;
        let row = Rect::new(x + pad, ry, w - 2.0 * pad, row_h);
        if i == v.menu.selected {
            ui.rect(row.x, row.y, row.w, row.h, c.selection);
        }
        let color = match item.kind {
            mao_complete::ItemKind::Directory => c.accent,
            mao_complete::ItemKind::Branch => c.branch,
            mao_complete::ItemKind::Option => c.option,
            _ => c.fg,
        };
        let inner = Rect::new(row.x + m.px(8.0), row.y, row.w - m.px(16.0), row.h);
        let label_w = inner.w * 0.62;
        let mut label = text(
            KEY_MENU_LABEL | (i - start) as u64,
            item.label.clone(),
            Rect::new(inner.x, inner.y, label_w, inner.h),
            m.ui_text,
            color,
            Align::Left,
        );
        label.mono = true;
        ui.texts.push(label);
        ui.texts.push(text(
            KEY_MENU_DETAIL | (i - start) as u64,
            item.detail.clone(),
            Rect::new(inner.x + label_w, inner.y, inner.w - label_w, inner.h),
            m.ui_text * 0.88,
            c.dim,
            Align::Right,
        ));
    }
    if footer > 0.0 {
        let fy = y + pad + rows as f32 * row_h;
        ui.texts.push(text(
            KEY_MENU_COUNT,
            format!("{}/{}", v.menu.selected + 1, items.len()),
            Rect::new(x + m.px(12.0), fy, w - m.px(24.0), footer),
            m.ui_text * 0.85,
            c.dim,
            Align::Right,
        ));
    }
}

/// Palette box geometry, shared by drawing and hit-testing.
struct PaletteGeometry {
    frame: Rect,
    input: Rect,
    rows: Vec<Rect>,
}

impl PaletteGeometry {
    fn new(l: &Layout, m: &Metrics, p: &Palette) -> Self {
        let w = m.px(620.0).min(l.content.w - m.px(32.0)).max(0.0);
        let x = (l.content.x + (l.content.w - w) / 2.0).round();
        let y = l.content.y + m.px(12.0);
        let input_h = m.px(42.0);
        let row_h = m.px(32.0);
        let n = p.matches.len().min(MAX_VISIBLE);
        let pad = m.px(6.0);
        let input = Rect::new(x, y, w, input_h);
        let rows = (0..n)
            .map(|i| {
                Rect::new(
                    x + pad,
                    y + input_h + pad + i as f32 * row_h,
                    w - 2.0 * pad,
                    row_h,
                )
            })
            .collect::<Vec<_>>();
        let h = input_h
            + if n > 0 {
                2.0 * pad + n as f32 * row_h
            } else {
                0.0
            };
        Self {
            frame: Rect::new(x, y, w, h),
            input,
            rows,
        }
    }
}

fn draw_palette(ui: &mut UiFrame, s: &DrawState<'_>, p: &Palette) {
    let (m, c) = (s.metrics, s.colors);
    let g = PaletteGeometry::new(s.layout, m, p);
    let f = g.frame;
    let b = m.gap;
    // Border, then body.
    ui.rect(f.x - b, f.y - b, f.w + 2.0 * b, f.h + 2.0 * b, c.divider);
    ui.rect(f.x, f.y, f.w, f.h, c.raised);
    ui.rect(g.input.x, g.input.bottom() - b, g.input.w, b, c.divider);

    let pad = m.px(14.0);
    let input_text = Rect::new(g.input.x + pad, g.input.y, g.input.w - 2.0 * pad, g.input.h);
    if p.query.is_empty() {
        ui.texts.push(text(
            KEY_PALETTE,
            "Type a command…",
            input_text,
            m.ui_text * 1.05,
            c.dim,
            Align::Left,
        ));
    } else {
        ui.texts.push(text(
            KEY_PALETTE,
            format!("{}▏", p.query),
            input_text,
            m.ui_text * 1.05,
            c.fg,
            Align::Left,
        ));
    }

    for (i, (row, action)) in g.rows.iter().zip(&p.matches).enumerate() {
        let selected = i == p.selected;
        let hovered = *s.hover == Hit::PaletteRow(i);
        if selected {
            ui.rect(row.x, row.y, row.w, row.h, c.selection);
        } else if hovered {
            ui.rect(row.x, row.y, row.w, row.h, c.hover);
        }
        let inner = Rect::new(row.x + m.px(10.0), row.y, row.w - m.px(20.0), row.h);
        ui.texts.push(text(
            KEY_PALETTE_ROW | i as u64,
            action.name(),
            inner,
            m.ui_text,
            c.fg,
            Align::Left,
        ));
        ui.texts.push(text(
            KEY_PALETTE_SHORTCUT | i as u64,
            action.shortcut(),
            inner,
            m.ui_text * 0.9,
            c.dim,
            Align::Right,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_window() {
        let m = Metrics::new(1.0, 8.0);
        let l = Layout::new((1000, 600), &m, 3);
        assert_eq!(l.tab_bar, Rect::new(0.0, 0.0, 1000.0, 36.0));
        assert_eq!(l.status, Rect::new(0.0, 576.0, 1000.0, 24.0));
        assert_eq!(l.content, Rect::new(0.0, 36.0, 1000.0, 540.0));
        assert_eq!(l.tabs.len(), 3);
        assert_eq!(l.tabs[0].w, 220.0);
        assert_eq!(l.plus.x, 660.0);
    }

    #[test]
    fn tabs_shrink_when_many() {
        let m = Metrics::new(1.0, 8.0);
        let l = Layout::new((800, 600), &m, 20);
        assert!(l.tabs[19].right() <= 800.0 - 36.0);
    }
}
