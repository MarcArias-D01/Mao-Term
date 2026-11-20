//! The document model the viewer draws: styled logical lines, or an image.

use std::path::PathBuf;

use unicode_width::UnicodeWidthChar;

/// An sRGB color.
pub type Rgb = [u8; 3];

/// How a run of text looks. `None` colors mean "the viewer's default".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

impl Style {
    pub fn fg(color: Rgb) -> Self {
        Self {
            fg: Some(color),
            ..Self::default()
        }
    }
}

/// A styled run ends at byte `end` of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub end: u32,
    pub style: Style,
}

/// Text with style runs covering it (sorted by `end`, the last ends at `text.len()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyledText {
    pub text: String,
    pub runs: Vec<Run>,
}

impl StyledText {
    pub fn plain(text: impl Into<String>) -> Self {
        let mut t = Self::default();
        t.push(&text.into(), Style::default());
        t
    }

    /// Appends `text` in `style`, merging with the previous run when equal.
    pub fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        let end = self.text.len() as u32;
        match self.runs.last_mut() {
            Some(last) if last.style == style => last.end = end,
            _ => self.runs.push(Run { end, style }),
        }
    }

    pub fn append(&mut self, other: &StyledText) {
        let mut start = 0;
        for run in &other.runs {
            self.push(&other.text[start..run.end as usize], run.style);
            start = run.end as usize;
        }
    }

    /// Style of the byte at `at`.
    pub fn style_at(&self, at: usize) -> Style {
        let i = self.runs.partition_point(|r| (r.end as usize) <= at);
        self.runs.get(i).map(|r| r.style).unwrap_or_default()
    }

    /// Display width in terminal cells.
    pub fn width(&self) -> usize {
        self.text.chars().map(char_width).sum()
    }
}

/// Cells a character occupies (0 for combining marks).
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// One logical line: wrapped into rows by [`crate::layout`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub content: StyledText,
    /// Drawn at the start of continuation rows (hanging indent, quote bars…).
    pub cont: StyledText,
    /// Background filling the whole row (code blocks).
    pub fill: Option<Rgb>,
    /// Repeat the content's first character across the row (horizontal rules).
    pub rule: bool,
    /// Never wrapped (tables), even with wrapping on.
    pub nowrap: bool,
}

impl Line {
    pub fn new(content: StyledText) -> Self {
        Self {
            content,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextDoc {
    pub lines: Vec<Line>,
    /// Show source line numbers.
    pub gutter: bool,
}

/// A decoded image with its mipmap chain (level 0 = full size), RGBA8 straight alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Size of the original (before any downscaling to the GPU limit).
    pub natural: (u32, u32),
    pub levels: Vec<ImageLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageLevel {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What kind of file this is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    /// Source code / structured text, with the syntax name if one was found.
    Code(String),
    Text,
    Image,
    Svg,
    Binary,
}

impl Kind {
    pub fn label(&self) -> &str {
        match self {
            Self::Markdown => "Markdown",
            Self::Code(name) => name,
            Self::Text => "Plain Text",
            Self::Image => "Image",
            Self::Svg => "SVG",
            Self::Binary => "Binary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Markdown: the rendered view and the source.
    Markdown {
        rendered: TextDoc,
        source: TextDoc,
    },
    Text(TextDoc),
    Image(Image),
    /// Cannot be shown (binary, unreadable…): a message for the user.
    Message(String),
}

/// A loaded document, delivered by [`crate::DocService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub path: PathBuf,
    pub kind: Kind,
    pub content: Content,
    /// Size on disk in bytes.
    pub size: u64,
    /// Only the beginning of the file was loaded.
    pub truncated: bool,
    /// Syntax highlighting is still being computed (a highlighted version follows).
    pub pending_highlight: bool,
}
