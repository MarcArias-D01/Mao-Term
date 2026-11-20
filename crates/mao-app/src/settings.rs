//! The settings screen: a modal card with live controls. Every change is
//! applied at once and written to `config.toml` (comments preserved).

use mao_config::{Backdrop, Config, Decorations, InputPosition};
use mao_ui::Rect;

use crate::chrome::Metrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Theme,
    Font,
    FontSize,
    Opacity,
    Backdrop,
    TitleBar,
    InputPosition,
    Editor,
    Suggestions,
    Padding,
    ConfigFile,
    ThemesFolder,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Choice {
        options: &'static [&'static str],
        selected: usize,
    },
    Toggle(bool),
    Slider {
        min: f32,
        max: f32,
        step: f32,
        value: f32,
        label: String,
    },
    Button(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub key: Key,
    pub label: &'static str,
    pub help: String,
    pub control: Control,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Header(&'static str),
    Row(Row),
}

/// What the user asked for (applied by the app, which owns the config).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    PickTheme,
    PickFont,
    SetFont(String),
    FontSize(f32),
    Opacity(f32),
    Backdrop(Backdrop),
    TitleBar(Decorations),
    InputPosition(InputPosition),
    Editor(bool),
    Suggestions(bool),
    Padding(f32),
    OpenConfigFile,
    OpenThemesFolder,
}

const BACKDROPS: &[&str] = &["Acrylic", "Mica", "None"];
const TITLE_BARS: &[&str] = &["Custom", "System"];
const POSITIONS: &[&str] = &["Bottom", "Inline"];

/// What the screen shows for the current configuration.
pub fn entries(config: &Config, font_in_use: &str, translucency: bool) -> Vec<Entry> {
    let row = |key, label, help: &str, control| {
        Entry::Row(Row {
            key,
            label,
            help: help.to_owned(),
            control,
        })
    };
    let theme = config
        .theme_name()
        .map_or_else(|| "Custom colors".to_owned(), str::to_owned);
    let opacity_help = if translucency {
        "How much of the desktop shows through (1.0 = solid)."
    } else {
        "Started solid: restart Mao-Term after lowering it to see the glass."
    };
    vec![
        Entry::Header("APPEARANCE"),
        row(
            Key::Theme,
            "Theme",
            "Colors of the terminal and the interface.",
            Control::Button(theme),
        ),
        row(
            Key::Font,
            "Font",
            "Monospace font of the terminal.",
            Control::Button(font_in_use.to_owned()),
        ),
        row(
            Key::FontSize,
            "Font size",
            "In points.",
            Control::Slider {
                min: 8.0,
                max: 28.0,
                step: 0.5,
                value: config.font.size,
                label: format!("{:.1}", config.font.size),
            },
        ),
        Entry::Header("WINDOW"),
        row(
            Key::Opacity,
            "Opacity",
            opacity_help,
            Control::Slider {
                min: 0.3,
                max: 1.0,
                step: 0.02,
                value: config.window.opacity,
                label: format!("{:.0}%", config.window.opacity * 100.0),
            },
        ),
        row(
            Key::Backdrop,
            "Glass effect",
            "Windows 11 material behind the window: blur (Acrylic) or wallpaper tint (Mica).",
            Control::Choice {
                options: BACKDROPS,
                selected: match config.window.backdrop {
                    Backdrop::Acrylic => 0,
                    Backdrop::Mica => 1,
                    Backdrop::None => 2,
                },
            },
        ),
        row(
            Key::TitleBar,
            "Title bar",
            "Custom: tabs and window buttons in one bar. System: the OS title bar.",
            Control::Choice {
                options: TITLE_BARS,
                selected: usize::from(config.window.decorations == Decorations::System),
            },
        ),
        Entry::Header("TERMINAL"),
        row(
            Key::InputPosition,
            "Input",
            "Bottom: a box under the output (like Warp). Inline: at the shell prompt.",
            Control::Choice {
                options: POSITIONS,
                selected: usize::from(config.input.position == InputPosition::Inline),
            },
        ),
        row(
            Key::Editor,
            "Smart editor",
            "Mao-Term's editor at the prompt: highlighting, history, completion.",
            Control::Toggle(config.input.editor),
        ),
        row(
            Key::Suggestions,
            "Suggestions",
            "History suggestions as you type (→ to accept).",
            Control::Toggle(config.input.suggestions),
        ),
        row(
            Key::Padding,
            "Padding",
            "Space around the text, in pixels.",
            Control::Slider {
                min: 0.0,
                max: 24.0,
                step: 1.0,
                value: config.terminal.padding,
                label: format!("{:.0}", config.terminal.padding),
            },
        ),
        Entry::Header("FILES"),
        row(
            Key::ConfigFile,
            "Config file",
            "Everything else (shell, scrollback, agents…).",
            Control::Button("Open config.toml".into()),
        ),
        row(
            Key::ThemesFolder,
            "Themes folder",
            "Drop .toml theme files here.",
            Control::Button("Open folder".into()),
        ),
    ]
}

impl Row {
    fn change_for(&self, choice: usize) -> Option<Change> {
        Some(match self.key {
            Key::Backdrop => {
                Change::Backdrop([Backdrop::Acrylic, Backdrop::Mica, Backdrop::None][choice.min(2)])
            }
            Key::TitleBar => {
                Change::TitleBar([Decorations::Custom, Decorations::System][choice.min(1)])
            }
            Key::InputPosition => {
                Change::InputPosition([InputPosition::Bottom, InputPosition::Inline][choice.min(1)])
            }
            _ => return None,
        })
    }

    fn slider_change(&self, value: f32) -> Option<Change> {
        Some(match self.key {
            Key::FontSize => Change::FontSize(value),
            Key::Opacity => Change::Opacity(value),
            Key::Padding => Change::Padding(value),
            _ => return None,
        })
    }

    /// Left/right arrows.
    pub fn adjust(&self, dir: i32) -> Option<Change> {
        match &self.control {
            Control::Choice { options, selected } => {
                let n = options.len() as i32;
                self.change_for((*selected as i32 + dir).rem_euclid(n) as usize)
            }
            Control::Toggle(on) => self.toggle(!on),
            Control::Slider {
                min,
                max,
                step,
                value,
                ..
            } => self.slider_change(snap(*value + dir as f32 * step, *min, *max, *step)),
            Control::Button(_) => None,
        }
    }

    /// Enter / space / click on the control.
    pub fn activate(&self) -> Option<Change> {
        match &self.control {
            Control::Toggle(on) => self.toggle(!on),
            Control::Choice { .. } => self.adjust(1),
            Control::Button(_) => Some(match self.key {
                Key::Theme => Change::PickTheme,
                Key::Font => Change::PickFont,
                Key::ConfigFile => Change::OpenConfigFile,
                Key::ThemesFolder => Change::OpenThemesFolder,
                _ => return None,
            }),
            Control::Slider { .. } => None,
        }
    }

    pub fn choose(&self, option: usize) -> Option<Change> {
        self.change_for(option)
    }

    /// A click/drag on a slider at `frac` (0–1) of its track.
    pub fn slide_to(&self, frac: f32) -> Option<Change> {
        let Control::Slider { min, max, step, .. } = &self.control else {
            return None;
        };
        self.slider_change(snap(
            min + (max - min) * frac.clamp(0.0, 1.0),
            *min,
            *max,
            *step,
        ))
    }

    fn toggle(&self, on: bool) -> Option<Change> {
        Some(match self.key {
            Key::Editor => Change::Editor(on),
            Key::Suggestions => Change::Suggestions(on),
            _ => return None,
        })
    }
}

fn snap(v: f32, min: f32, max: f32, step: f32) -> f32 {
    let v = ((v - min) / step).round() * step + min;
    (v.clamp(min, max) * 1000.0).round() / 1000.0
}

/// Open settings screen: which row is selected and the list scroll.
#[derive(Debug, Default)]
pub struct SettingsView {
    /// Index into the rows (headers excluded).
    pub selected: usize,
    pub scroll: f32,
}

/// Where everything is on screen.
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub card: Rect,
    pub close: Rect,
    pub list: Rect,
    /// Per visible row: row index, row rect, control rect, segment rects.
    pub rows: Vec<(usize, Rect, Rect, Vec<Rect>)>,
    /// Visible section headers.
    pub headers: Vec<(&'static str, Rect)>,
    /// Height of all entries (for scrolling).
    pub content_h: f32,
}

pub fn geometry(content: Rect, m: &Metrics, entries: &[Entry], view: &SettingsView) -> Geometry {
    let w = m.px(760.0).min(content.w - m.px(32.0)).max(m.px(300.0));
    let h = (content.h - m.px(32.0)).max(m.px(200.0));
    let card = Rect::new(
        (content.x + (content.w - w) / 2.0).round(),
        (content.y + m.px(16.0)).round(),
        w,
        h,
    );
    let header_h = m.px(64.0);
    let close = Rect::new(
        card.right() - m.px(48.0),
        card.y + m.px(12.0),
        m.px(36.0),
        m.px(36.0),
    );
    let list = Rect::new(
        card.x,
        card.y + header_h,
        card.w,
        card.h - header_h - m.px(8.0),
    );
    let pad = m.px(24.0);
    let control_w = m.px(300.0).min(card.w * 0.45);
    let mut g = Geometry {
        card,
        close,
        list,
        ..Geometry::default()
    };
    let mut y = list.y - view.scroll;
    let mut row_i = 0;
    for e in entries {
        let eh = match e {
            Entry::Header(_) => m.px(40.0),
            Entry::Row(_) => m.px(56.0),
        };
        let visible = y >= list.y - 0.5 && y + eh <= list.bottom() + 0.5;
        match e {
            Entry::Header(t) => {
                if visible {
                    g.headers.push((
                        t,
                        Rect::new(card.x + pad, y + m.px(12.0), card.w - 2.0 * pad, m.px(24.0)),
                    ));
                }
            }
            Entry::Row(r) => {
                if visible {
                    let rr = Rect::new(card.x + m.px(8.0), y, card.w - m.px(16.0), eh);
                    let ch = match r.control {
                        Control::Toggle(_) => m.px(24.0),
                        _ => m.px(30.0),
                    };
                    let cw = match r.control {
                        Control::Toggle(_) => m.px(46.0),
                        _ => control_w,
                    };
                    let control = Rect::new(
                        rr.right() - pad + m.px(8.0) - cw,
                        y + (eh - ch) / 2.0,
                        cw,
                        ch,
                    );
                    let segments = match &r.control {
                        Control::Choice { options, .. } => {
                            let sw = control.w / options.len() as f32;
                            (0..options.len())
                                .map(|i| {
                                    Rect::new(control.x + i as f32 * sw, control.y, sw, control.h)
                                })
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    g.rows.push((row_i, rr, control, segments));
                }
                row_i += 1;
            }
        }
        y += eh;
    }
    g.content_h = y + view.scroll - list.y;
    g
}

/// The rows (headers skipped).
pub fn rows(entries: &[Entry]) -> Vec<&Row> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Row(r) => Some(r),
            Entry::Header(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_produce_changes() {
        let config = Config::default();
        let entries = entries(&config, "Cascadia Mono", true);
        let rows = rows(&entries);
        let size = rows.iter().find(|r| r.key == Key::FontSize).unwrap();
        assert_eq!(
            size.adjust(1),
            Some(Change::FontSize(config.font.size + 0.5))
        );
        assert_eq!(size.slide_to(0.0), Some(Change::FontSize(8.0)));
        assert_eq!(size.slide_to(1.0), Some(Change::FontSize(28.0)));
        let backdrop = rows.iter().find(|r| r.key == Key::Backdrop).unwrap();
        assert_eq!(backdrop.choose(1), Some(Change::Backdrop(Backdrop::Mica)));
        let editor = rows.iter().find(|r| r.key == Key::Editor).unwrap();
        assert_eq!(editor.activate(), Some(Change::Editor(false)));
        let theme = rows.iter().find(|r| r.key == Key::Theme).unwrap();
        assert_eq!(theme.activate(), Some(Change::PickTheme));
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(0.787, 0.3, 1.0, 0.02), 0.78);
        assert_eq!(snap(40.0, 8.0, 28.0, 0.5), 28.0);
    }
}
