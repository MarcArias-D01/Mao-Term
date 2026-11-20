//! Documents for Mao-Term's viewer: loading (background thread), Markdown
//! rendering, syntax highlighting, images, and wrapping text into rows.
//! See `docs/specs/document-viewer.md`.

pub mod highlight;
mod image;
pub mod layout;
mod load;
pub mod markdown;
pub mod model;
mod service;
pub mod text;

pub use load::human_size;
pub use model::{
    Content, Document, Image, ImageLevel, Kind, Line, Rgb, Style, StyledText, TextDoc,
};
pub use service::DocService;

/// Colors used to render documents (derived from the terminal theme).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub fg: Rgb,
    pub bg: Rgb,
    /// Secondary text, borders, bullets of quotes.
    pub muted: Rgb,
    pub accent: Rgb,
    pub heading: Rgb,
    pub link: Rgb,
    pub code_fg: Rgb,
    pub code_bg: Rgb,
    /// Dark background (selects the syntax theme).
    pub dark: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            fg: [0xd4, 0xd4, 0xd4],
            bg: [0x1e, 0x1e, 0x1e],
            muted: [0x80, 0x80, 0x80],
            accent: [0x56, 0x9c, 0xd6],
            heading: [0x6c, 0xb6, 0xff],
            link: [0x4e, 0xc9, 0xb0],
            code_fg: [0xce, 0x91, 0x78],
            code_bg: [0x2a, 0x2a, 0x2a],
            dark: true,
        }
    }
}
