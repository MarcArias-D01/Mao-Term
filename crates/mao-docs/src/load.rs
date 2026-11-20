//! Loading a file into a [`Document`] (runs on the service's worker thread).

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use crate::Theme;
use crate::highlight::Highlighter;
use crate::model::{Content, Document, Kind, Line, TextDoc};
use crate::{image, markdown, text};

/// Only this much of a text file is read (the rest is reported as truncated).
const TEXT_LIMIT: u64 = 64 * 1024 * 1024;
/// Larger files are shown in plain text first, then highlighted (also done
/// while the grammars are still loading: the first screen must be fast).
const QUICK_LIMIT: usize = 32 * 1024;
/// Larger files are never highlighted.
const HIGHLIGHT_LIMIT: usize = 4 * 1024 * 1024;

const RASTER: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];
const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd"];

/// Shared, lazily built state of the loader.
pub struct Loader {
    pub theme: Theme,
    pub max_texture: u32,
    highlighter: OnceLock<Highlighter>,
}

impl Loader {
    pub fn new(theme: Theme, max_texture: u32) -> Self {
        Self {
            theme,
            max_texture,
            highlighter: OnceLock::new(),
        }
    }

    fn hl(&self) -> &Highlighter {
        self.highlighter
            .get_or_init(|| Highlighter::new(self.theme.dark))
    }

    /// Loads `path`, calling `send` once, or twice when a quick plain version
    /// is sent before the highlighted one.
    pub fn load(&self, path: &Path, send: &mut dyn FnMut(Document)) {
        let started = std::time::Instant::now();
        let mut first = true;
        self.load_inner(path, &mut |doc| {
            tracing::debug!(
                ms = started.elapsed().as_millis(),
                path = %path.display(),
                pending = doc.pending_highlight,
                "{}",
                if first { "document loaded" } else { "document highlighted" }
            );
            first = false;
            send(doc);
        });
    }

    fn load_inner(&self, path: &Path, send: &mut dyn FnMut(Document)) {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let doc = |kind, content| Document {
            path: path.to_path_buf(),
            kind,
            content,
            size,
            truncated: false,
            pending_highlight: false,
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();

        if RASTER.contains(&ext.as_str()) || ext == "svg" {
            let (kind, result) = if ext == "svg" {
                (Kind::Svg, image::load_svg(path, self.max_texture))
            } else {
                (Kind::Image, image::load_raster(path, self.max_texture))
            };
            send(match result {
                Ok(img) => doc(kind, Content::Image(img)),
                Err(e) => doc(
                    kind,
                    Content::Message(format!("Cannot open this image: {e}")),
                ),
            });
            return;
        }

        let bytes = match read_limited(path, TEXT_LIMIT) {
            Ok(b) => b,
            Err(e) => {
                send(doc(
                    Kind::Text,
                    Content::Message(format!("Cannot read this file: {e}")),
                ));
                return;
            }
        };
        let truncated = (bytes.len() as u64) < size;
        let Some(source) = text::decode(&bytes) else {
            send(doc(
                Kind::Binary,
                Content::Message(format!(
                    "Binary file ({}) — it can't be shown as text.",
                    human_size(size)
                )),
            ));
            return;
        };
        let lines = text::lines(&source);
        let first = lines.first().map_or("", String::as_str);

        let highlight = source.len() <= HIGHLIGHT_LIMIT;
        let quick_first =
            highlight && (self.highlighter.get().is_none() || source.len() > QUICK_LIMIT);

        if MARKDOWN.contains(&ext.as_str()) {
            let markdown = |hl: Option<&Highlighter>, pending| {
                let rendered = markdown::render(&source, &self.theme, hl);
                let source_doc = match hl.and_then(|h| Some((h, h.markdown()?))) {
                    Some((h, syntax)) => highlighted(h, syntax, &lines),
                    None => text::plain_doc(&lines),
                };
                Document {
                    truncated,
                    pending_highlight: pending,
                    ..doc(
                        Kind::Markdown,
                        Content::Markdown {
                            rendered,
                            source: source_doc,
                        },
                    )
                }
            };
            if quick_first {
                send(markdown(None, true));
            }
            send(markdown(highlight.then(|| self.hl()), false));
            return;
        }

        if quick_first {
            send(Document {
                truncated,
                pending_highlight: true,
                ..doc(Kind::Text, Content::Text(text::plain_doc(&lines)))
            });
        }
        let hl = self.hl();
        let syntax = (source.len() <= HIGHLIGHT_LIMIT)
            .then(|| hl.syntax_for(path, first))
            .flatten();
        let Some(syntax) = syntax else {
            send(Document {
                truncated,
                ..doc(Kind::Text, Content::Text(text::plain_doc(&lines)))
            });
            return;
        };
        let kind = Kind::Code(syntax.name.clone());
        send(Document {
            truncated,
            ..doc(kind, Content::Text(highlighted(hl, syntax, &lines)))
        });
    }
}

fn highlighted(
    hl: &Highlighter,
    syntax: &syntect::parsing::SyntaxReference,
    lines: &[String],
) -> TextDoc {
    TextDoc {
        lines: hl
            .highlight(syntax, lines.iter().map(String::as_str))
            .into_iter()
            .map(Line::new)
            .collect(),
        gutter: true,
    }
}

fn read_limited(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    std::fs::File::open(path)?
        .take(limit)
        .read_to_end(&mut out)?;
    Ok(out)
}

/// `1.5 MB`-style size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_all(path: &Path) -> Vec<Document> {
        let loader = Loader::new(Theme::default(), 4096);
        let mut out = Vec::new();
        loader.load(path, &mut |d| out.push(d));
        out
    }

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mao-docs-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn loads_code_with_highlighting() {
        let docs = load_all(&temp("a.rs", b"fn main() {}\n"));
        // Plain first (the grammars weren't loaded yet), then highlighted.
        assert_eq!(docs.len(), 2);
        assert!(docs[0].pending_highlight);
        let last = docs.last().unwrap();
        assert_eq!(last.kind, Kind::Code("Rust".into()));
        assert!(!last.pending_highlight);
        let Content::Text(t) = &last.content else {
            panic!("{:?}", last.content)
        };
        assert!(t.gutter);
        assert!(t.lines[0].content.runs.len() > 1);
    }

    #[test]
    fn binary_files_get_a_message() {
        let docs = load_all(&temp("a.bin", b"\x00\x01\x02garbage"));
        assert_eq!(docs[0].kind, Kind::Binary);
        assert!(matches!(docs[0].content, Content::Message(_)));
    }

    #[test]
    fn markdown_has_both_views() {
        let docs = load_all(&temp("a.md", b"# Hi\n\n```json\n{\"a\": 1}\n```\n"));
        let Content::Markdown { rendered, source } = &docs.last().unwrap().content else {
            panic!()
        };
        assert_eq!(rendered.lines[0].content.text, "Hi");
        assert_eq!(source.lines[0].content.text, "# Hi");
    }

    #[test]
    fn sizes_are_human() {
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
