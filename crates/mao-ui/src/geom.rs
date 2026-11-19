/// Axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Shrinks the rectangle by `d` on every side (never below zero size).
    pub fn inset(&self, d: f32) -> Self {
        Self {
            x: self.x + d,
            y: self.y + d,
            w: (self.w - 2.0 * d).max(0.0),
            h: (self.h - 2.0 * d).max(0.0),
        }
    }

    /// Splits off `size` pixels from the top; returns (top, rest).
    pub fn split_top(&self, size: f32) -> (Self, Self) {
        let size = size.min(self.h);
        (
            Self::new(self.x, self.y, self.w, size),
            Self::new(self.x, self.y + size, self.w, self.h - size),
        )
    }

    /// Splits off `size` pixels from the bottom; returns (rest, bottom).
    pub fn split_bottom(&self, size: f32) -> (Self, Self) {
        let size = size.min(self.h);
        (
            Self::new(self.x, self.y, self.w, self.h - size),
            Self::new(self.x, self.bottom() - size, self.w, size),
        )
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}
