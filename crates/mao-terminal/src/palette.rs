use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// RGB color in sRGB space.
pub type Rgb8 = [u8; 3];

/// Resolved color table used to turn terminal colors into RGB.
///
/// Layout follows `alacritty_terminal`: 0..16 ANSI, 16..232 color cube,
/// 232..256 grayscale, then the named colors (foreground, background, cursor, dim…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    colors: [Rgb8; COUNT],
    selection: Rgb8,
}

impl Palette {
    pub fn new(
        ansi: [Rgb8; 16],
        foreground: Rgb8,
        background: Rgb8,
        cursor: Rgb8,
        selection: Rgb8,
    ) -> Self {
        let mut colors = [[0; 3]; COUNT];
        colors[..16].copy_from_slice(&ansi);

        // 6x6x6 color cube.
        let level = |v: usize| if v == 0 { 0 } else { (v * 40 + 55) as u8 };
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    colors[16 + r * 36 + g * 6 + b] = [level(r), level(g), level(b)];
                }
            }
        }
        // Grayscale ramp.
        for i in 0..24 {
            let v = (8 + i * 10) as u8;
            colors[232 + i] = [v, v, v];
        }

        colors[NamedColor::Foreground as usize] = foreground;
        colors[NamedColor::Background as usize] = background;
        colors[NamedColor::Cursor as usize] = cursor;
        colors[NamedColor::BrightForeground as usize] = foreground;
        colors[NamedColor::DimForeground as usize] = dim(foreground);
        for i in 0..8 {
            colors[NamedColor::DimBlack as usize + i] = dim(ansi[i]);
        }

        Self { colors, selection }
    }

    pub fn foreground(&self) -> Rgb8 {
        self.colors[NamedColor::Foreground as usize]
    }

    pub fn background(&self) -> Rgb8 {
        self.colors[NamedColor::Background as usize]
    }

    pub fn cursor(&self) -> Rgb8 {
        self.colors[NamedColor::Cursor as usize]
    }

    pub fn selection(&self) -> Rgb8 {
        self.selection
    }

    /// Color at a palette index, honoring runtime overrides (OSC 4/10/11/12).
    pub(crate) fn indexed(&self, overrides: &Colors, index: usize) -> Rgb8 {
        match overrides[index] {
            Some(rgb) => [rgb.r, rgb.g, rgb.b],
            None => self.colors[index],
        }
    }

    pub(crate) fn resolve(&self, overrides: &Colors, color: Color) -> Rgb8 {
        match color {
            Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
            Color::Indexed(i) => self.indexed(overrides, usize::from(i)),
            Color::Named(n) => self.indexed(overrides, n as usize),
        }
    }
}

pub(crate) fn dim([r, g, b]: Rgb8) -> Rgb8 {
    let f = |c: u8| (u16::from(c) * 2 / 3) as u8;
    [f(r), f(g), f(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let mut ansi = [[0; 3]; 16];
        for (i, c) in ansi.iter_mut().enumerate() {
            *c = [i as u8; 3];
        }
        Palette::new(ansi, [200; 3], [10; 3], [100; 3], [50; 3])
    }

    #[test]
    fn cube_and_grayscale() {
        let p = palette();
        let none = Colors::default();
        assert_eq!(p.indexed(&none, 16), [0, 0, 0]);
        assert_eq!(p.indexed(&none, 231), [255, 255, 255]);
        assert_eq!(p.indexed(&none, 196), [255, 0, 0]);
        assert_eq!(p.indexed(&none, 232), [8, 8, 8]);
        assert_eq!(p.indexed(&none, 255), [238, 238, 238]);
    }

    #[test]
    fn named_colors() {
        let p = palette();
        let none = Colors::default();
        assert_eq!(
            p.resolve(&none, Color::Named(NamedColor::Foreground)),
            [200; 3]
        );
        assert_eq!(p.resolve(&none, Color::Named(NamedColor::Red)), [1; 3]);
        assert_eq!(p.resolve(&none, Color::Indexed(9)), [9; 3]);
    }
}
