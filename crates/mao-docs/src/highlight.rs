//! Syntax highlighting with syntect (+ bat's extra grammars from `two-face`).

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme as SynTheme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::model::{Style, StyledText};

/// Loaded grammars and color theme. Building it takes tens of milliseconds:
/// create it lazily, on a background thread.
pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: SynTheme,
}

impl Highlighter {
    pub fn new(dark: bool) -> Self {
        let mut themes = ThemeSet::load_defaults();
        let name = if dark {
            "base16-ocean.dark"
        } else {
            "InspiredGitHub"
        };
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
            theme: themes.themes.remove(name).unwrap_or_default(),
        }
    }

    /// Grammar for a file, by extension, file name or first line (shebang).
    pub fn syntax_for(&self, path: &Path, first_line: &str) -> Option<&SyntaxReference> {
        let by_name = || {
            let name = path.file_name()?.to_str()?;
            self.syntaxes.find_syntax_by_extension(name)
        };
        let by_ext = || {
            let ext = path.extension()?.to_str()?;
            self.syntaxes
                .find_syntax_by_extension(ext)
                .or_else(|| self.syntaxes.find_syntax_by_token(ext))
        };
        by_name()
            .or_else(by_ext)
            .or_else(|| self.syntaxes.find_syntax_by_first_line(first_line))
            .filter(|s| s.name != "Plain Text")
    }

    /// Grammar for a Markdown fence language (`rust`, `sh`, `json`…).
    pub fn syntax_for_token(&self, token: &str) -> Option<&SyntaxReference> {
        self.syntaxes
            .find_syntax_by_token(token)
            .filter(|s| s.name != "Plain Text")
    }

    pub fn markdown(&self) -> Option<&SyntaxReference> {
        self.syntaxes.find_syntax_by_name("Markdown")
    }

    /// Highlights consecutive lines (without their newline) of one file.
    pub fn highlight<'a>(
        &self,
        syntax: &SyntaxReference,
        lines: impl IntoIterator<Item = &'a str>,
    ) -> Vec<StyledText> {
        let mut h = HighlightLines::new(syntax, &self.theme);
        let mut buf = String::new();
        lines
            .into_iter()
            .map(|line| {
                buf.clear();
                buf.push_str(line);
                buf.push('\n');
                let mut out = StyledText::default();
                match h.highlight_line(&buf, &self.syntaxes) {
                    Ok(spans) => {
                        for (style, text) in spans {
                            out.push(text.trim_end_matches('\n'), convert(style));
                        }
                    }
                    Err(e) => {
                        tracing::debug!("highlighting failed: {e}");
                        out.push(line, Style::default());
                    }
                }
                out
            })
            .collect()
    }
}

fn convert(s: syntect::highlighting::Style) -> Style {
    let c = s.foreground;
    Style {
        fg: Some([c.r, c.g, c.b]),
        bg: None,
        bold: s.font_style.contains(FontStyle::BOLD),
        italic: s.font_style.contains(FontStyle::ITALIC),
        underline: s.font_style.contains(FontStyle::UNDERLINE),
        strike: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_common_grammars() {
        let h = Highlighter::new(true);
        for (file, name) in [
            ("main.rs", "Rust"),
            ("Cargo.toml", "TOML"),
            ("a.json", "JSON"),
            ("x.yaml", "YAML"),
            ("app.ts", "TypeScript"),
            ("README.md", "Markdown"),
        ] {
            let s = h.syntax_for(Path::new(file), "").map(|s| s.name.as_str());
            assert_eq!(s, Some(name), "{file}");
        }
        assert_eq!(
            h.syntax_for(Path::new("script"), "#!/bin/bash")
                .map(|s| s.name.as_str()),
            Some("Bourne Again Shell (bash)")
        );
        assert!(h.syntax_for(Path::new("notes.txt"), "hello").is_none());
    }

    #[test]
    fn highlights_keywords() {
        let h = Highlighter::new(true);
        let syntax = h.syntax_for(Path::new("a.rs"), "").unwrap();
        let lines = h.highlight(syntax, ["fn main() {}"]);
        assert_eq!(lines[0].text, "fn main() {}");
        assert!(lines[0].runs.len() > 1, "{:?}", lines[0].runs);
    }
}
