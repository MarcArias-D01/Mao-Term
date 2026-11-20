//! Markdown rendered to styled monospace lines (headings, lists, quotes, code
//! blocks, tables…), in the spirit of `glow`.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::Theme;
use crate::highlight::Highlighter;
use crate::model::{Line, Style, StyledText, TextDoc};

pub fn render(source: &str, theme: &Theme, hl: Option<&Highlighter>) -> TextDoc {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut md = Md {
        theme,
        hl,
        lines: Vec::new(),
        stack: Vec::new(),
        inline: StyledText::default(),
        bold: 0,
        italic: 0,
        strike: 0,
        link: 0,
        image: 0,
        heading: None,
        code: None,
        table: None,
        last_blank: true,
    };
    for event in Parser::new_ext(source, options) {
        md.event(event);
    }
    md.flush();
    while md
        .lines
        .last()
        .is_some_and(|l| l.content.text.trim().is_empty() && l.fill.is_none())
    {
        md.lines.pop();
    }
    TextDoc {
        lines: md.lines,
        gutter: false,
    }
}

enum Container {
    Quote,
    /// A list and the number of its next item (ordered lists).
    List(Option<u64>),
    /// A list item: its bullet, and whether a line already showed it.
    Item {
        bullet: String,
        shown: bool,
    },
}

struct Table {
    align: Vec<Alignment>,
    rows: Vec<Vec<StyledText>>,
    header_rows: usize,
    in_head: bool,
}

struct Md<'a> {
    theme: &'a Theme,
    hl: Option<&'a Highlighter>,
    lines: Vec<Line>,
    stack: Vec<Container>,
    /// Inline content of the paragraph/heading/cell being built.
    inline: StyledText,
    bold: u32,
    italic: u32,
    strike: u32,
    link: u32,
    image: u32,
    heading: Option<HeadingLevel>,
    /// Inside a code block: (language, text).
    code: Option<(String, String)>,
    table: Option<Table>,
    /// The last emitted line is blank (avoid doubles).
    last_blank: bool,
}

const BULLETS: [&str; 3] = ["• ", "◦ ", "▪ "];

impl Md<'_> {
    fn style(&self) -> Style {
        let t = self.theme;
        let mut s = Style::default();
        if self.heading.is_some() {
            s.bold = true;
            s.fg = Some(t.heading);
        }
        if self.bold > 0 {
            s.bold = true;
        }
        if self.italic > 0 {
            s.italic = true;
        }
        if self.strike > 0 {
            s.strike = true;
        }
        if self.link > 0 {
            s.fg = Some(t.link);
            s.underline = true;
        }
        if self.image > 0 {
            s.fg = Some(t.muted);
            s.italic = true;
        }
        s
    }

    /// Prefix of the next line: quote bars and the pending list bullet.
    fn prefix(&mut self, first: bool) -> StyledText {
        let muted = Style::fg(self.theme.muted);
        let mut p = StyledText::default();
        for c in &mut self.stack {
            match c {
                Container::Quote => p.push("│ ", muted),
                Container::List(_) => {}
                Container::Item { bullet, shown } => {
                    if first && !*shown {
                        p.push(bullet, Style::fg(self.theme.accent));
                        *shown = true;
                    } else {
                        p.push(&" ".repeat(bullet.chars().count()), Style::default());
                    }
                }
            }
        }
        p
    }

    fn push_line(&mut self, line: Line) {
        self.last_blank = line.content.text.trim().is_empty() && line.fill.is_none();
        self.lines.push(line);
    }

    fn blank(&mut self) {
        if self.last_blank {
            return;
        }
        self.push_line(Line::default());
    }

    /// Emits the pending inline text as one (wrappable) line.
    fn flush(&mut self) {
        if self.inline.text.trim().is_empty() {
            self.inline = StyledText::default();
            return;
        }
        let inline = std::mem::take(&mut self.inline);
        // Hard breaks were stored as '\n': each part is its own line.
        let mut start = 0;
        let parts: Vec<(usize, usize)> = inline
            .text
            .split('\n')
            .map(|part| {
                let r = (start, start + part.len());
                start += part.len() + 1;
                r
            })
            .collect();
        for (i, (a, b)) in parts.into_iter().enumerate() {
            let mut content = self.prefix(i == 0);
            let cont = self.prefix(false);
            content.append(&slice(&inline, a, b));
            let mut line = Line::new(content);
            line.cont = cont;
            self.push_line(line);
        }
    }

    fn event(&mut self, event: Event<'_>) {
        if let Some((_, buf)) = self.code.as_mut() {
            match event {
                Event::Text(t) => {
                    buf.push_str(&t);
                    return;
                }
                Event::End(TagEnd::CodeBlock) => {}
                _ => return,
            }
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let style = self.style();
                self.inline.push(&t, style);
            }
            Event::Code(t) => {
                let style = Style {
                    fg: Some(self.theme.code_fg),
                    bg: Some(self.theme.code_bg),
                    ..self.style()
                };
                self.inline.push(&t, style);
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let style = Style::fg(self.theme.code_fg);
                self.inline.push(&t, style);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let t = t.trim_end_matches('\n');
                if !t.trim_start().starts_with("<!--") {
                    let style = Style::fg(self.theme.muted);
                    if self.table.is_none() && t.contains('\n') {
                        self.flush();
                        for part in t.split('\n') {
                            self.inline.push(part, style);
                            self.flush();
                        }
                    } else {
                        self.inline.push(t, style);
                    }
                }
            }
            Event::FootnoteReference(label) => {
                let style = Style::fg(self.theme.link);
                self.inline.push(&format!("[^{label}]"), style);
            }
            Event::SoftBreak => {
                let style = self.style();
                self.inline.push(" ", style);
            }
            Event::HardBreak => self.inline.push("\n", Style::default()),
            Event::Rule => {
                self.flush();
                self.blank();
                let mut content = self.prefix(false);
                content.push("─", Style::fg(self.theme.muted));
                let line = Line {
                    rule: true,
                    ..Line::new(content)
                };
                self.push_line(line);
                self.blank();
            }
            Event::TaskListMarker(done) => {
                let (mark, color) = if done {
                    ("☑ ", self.theme.accent)
                } else {
                    ("☐ ", self.theme.muted)
                };
                self.inline.push(mark, Style::fg(color));
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.flush(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.blank();
                self.heading = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.blank();
                self.stack.push(Container::Quote);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split([',', ' ']).next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                if !self
                    .stack
                    .iter()
                    .any(|c| matches!(c, Container::Item { .. }))
                {
                    self.blank();
                }
                self.stack.push(Container::List(start));
            }
            Tag::Item => {
                self.flush();
                let depth = self
                    .stack
                    .iter()
                    .filter(|c| matches!(c, Container::List(_)))
                    .count();
                let bullet = match self.stack.last_mut() {
                    Some(Container::List(Some(n))) => {
                        let b = format!("{n}. ");
                        *n += 1;
                        b
                    }
                    _ => BULLETS[depth.saturating_sub(1) % BULLETS.len()].to_owned(),
                };
                self.stack.push(Container::Item {
                    bullet,
                    shown: false,
                });
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                let style = Style::fg(self.theme.link);
                self.inline.push(&format!("[^{label}]: "), style);
            }
            Tag::Table(align) => {
                self.flush();
                self.blank();
                self.table = Some(Table {
                    align,
                    rows: Vec::new(),
                    header_rows: 0,
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.inline = StyledText::default(),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { .. } => self.link += 1,
            Tag::Image { .. } => {
                self.image += 1;
                let style = self.style();
                self.inline.push("[image: ", style);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.blank();
            }
            TagEnd::Heading(level) => {
                self.flush();
                self.heading = None;
                let rule = match level {
                    HeadingLevel::H1 => Some(("━", self.theme.accent)),
                    HeadingLevel::H2 => Some(("─", self.theme.muted)),
                    _ => None,
                };
                if let Some((ch, color)) = rule {
                    let mut content = self.prefix(false);
                    content.push(ch, Style::fg(color));
                    let line = Line {
                        rule: true,
                        ..Line::new(content)
                    };
                    self.push_line(line);
                }
                self.blank();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.stack.pop();
                self.blank();
            }
            TagEnd::CodeBlock => {
                if let Some((lang, text)) = self.code.take() {
                    self.code_block(&lang, &text);
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.stack.pop();
                if !self
                    .stack
                    .iter()
                    .any(|c| matches!(c, Container::Item { .. }))
                {
                    self.blank();
                }
            }
            TagEnd::Item => {
                self.flush();
                self.stack.pop();
            }
            TagEnd::FootnoteDefinition => {
                self.flush();
                self.blank();
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = false;
                    t.header_rows = t.rows.len();
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(t) = self.table.as_mut()
                    && let Some(row) = t.rows.last_mut()
                {
                    row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table_lines(&t);
                }
                self.blank();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = self.link.saturating_sub(1),
            TagEnd::Image => {
                let style = self.style();
                self.inline.push("]", style);
                self.image = self.image.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn code_block(&mut self, lang: &str, text: &str) {
        let t = self.theme;
        let fill = Some(t.code_bg);
        let text = text.strip_suffix('\n').unwrap_or(text);
        let source: Vec<String> = text.lines().map(crate::text::clean_line).collect();
        let highlighted = self
            .hl
            .and_then(|hl| Some((hl, hl.syntax_for_token(lang)?)))
            .map(|(hl, syntax)| hl.highlight(syntax, source.iter().map(String::as_str)));
        let cont = {
            let mut p = self.prefix(false);
            p.push("  ", Style::default());
            p
        };

        // Header row with the language name.
        let mut header = self.prefix(true);
        header.push(" ", Style::default());
        header.push(lang, Style::fg(t.muted));
        self.push_line(Line {
            fill,
            ..Line::new(header)
        });
        for (i, src) in source.iter().enumerate() {
            let mut content = cont.clone();
            match highlighted.as_ref().and_then(|h| h.get(i)) {
                Some(h) => content.append(h),
                None => content.push(src, Style::fg(t.code_fg)),
            }
            self.push_line(Line {
                content,
                cont: cont.clone(),
                fill,
                rule: false,
                nowrap: false,
            });
        }
        let bottom = self.prefix(false);
        self.push_line(Line {
            fill,
            ..Line::new(bottom)
        });
        self.last_blank = false;
        self.blank();
    }

    fn table_lines(&mut self, t: &Table) {
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let mut widths = vec![1; cols];
        for row in &t.rows {
            for (c, cell) in row.iter().enumerate() {
                widths[c] = widths[c].max(cell.width());
            }
        }
        let border = Style::fg(self.theme.muted);
        let rule = |l: &str, m: &str, r: &str| {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    s.push_str(m);
                }
                s.push_str(&"─".repeat(w + 2));
            }
            s.push_str(r);
            s
        };
        let mut out: Vec<StyledText> = Vec::new();
        let mut line = StyledText::default();
        line.push(&rule("┌", "┬", "┐"), border);
        out.push(line);
        for (ri, row) in t.rows.iter().enumerate() {
            let mut line = StyledText::default();
            for (c, w) in widths.iter().enumerate() {
                line.push(if c == 0 { "│ " } else { " │ " }, border);
                let empty = StyledText::default();
                let cell = row.get(c).unwrap_or(&empty);
                let pad = w - cell.width();
                let (left, right) = match t.align.get(c) {
                    Some(Alignment::Right) => (pad, 0),
                    Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                line.push(&" ".repeat(left), Style::default());
                if ri < t.header_rows {
                    let mut bold = StyledText::default();
                    let mut start = 0;
                    for run in &cell.runs {
                        let mut s = run.style;
                        s.bold = true;
                        bold.push(&cell.text[start..run.end as usize], s);
                        start = run.end as usize;
                    }
                    line.append(&bold);
                } else {
                    line.append(cell);
                }
                line.push(&" ".repeat(right), Style::default());
            }
            line.push(" │", border);
            out.push(line);
            if ri + 1 == t.header_rows && t.rows.len() > t.header_rows {
                let mut sep = StyledText::default();
                sep.push(&rule("├", "┼", "┤"), border);
                out.push(sep);
            }
        }
        let mut bottom = StyledText::default();
        bottom.push(&rule("└", "┴", "┘"), border);
        out.push(bottom);

        for text in out {
            let mut content = self.prefix(true);
            content.append(&text);
            self.push_line(Line {
                nowrap: true,
                ..Line::new(content)
            });
        }
    }
}

/// Bytes `a..b` of a styled text, keeping styles.
fn slice(t: &StyledText, a: usize, b: usize) -> StyledText {
    let mut out = StyledText::default();
    let mut start = 0;
    for run in &t.runs {
        let end = run.end as usize;
        let (s, e) = (start.max(a), end.min(b));
        if s < e {
            out.push(&t.text[s..e], run.style);
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        render(src, &Theme::default(), None)
            .lines
            .into_iter()
            .map(|l| l.content.text)
            .collect()
    }

    #[test]
    fn headings_paragraphs_and_rules() {
        let t = texts("# Title\n\nSome *text*\nmore.\n\n---\n\n## Sub");
        assert_eq!(
            t,
            ["Title", "━", "", "Some text more.", "", "─", "", "Sub", "─"]
        );
    }

    #[test]
    fn nested_lists_with_bullets_and_numbers() {
        let t = texts("- a\n- b\n  1. c\n  2. d\n- [x] done\n");
        assert_eq!(t, ["• a", "• b", "  1. c", "  2. d", "• ☑ done"]);
    }

    #[test]
    fn list_continuation_is_indented() {
        let doc = render("- item text", &Theme::default(), None);
        assert_eq!(doc.lines[0].cont.text, "  ");
    }

    #[test]
    fn quotes_get_bars() {
        let t = texts("> quoted\n> text");
        assert_eq!(t, ["│ quoted text"]);
    }

    #[test]
    fn code_blocks_are_filled() {
        let doc = render("```rust\nfn x() {}\n```\n", &Theme::default(), None);
        let lines: Vec<_> = doc.lines.iter().map(|l| l.content.text.as_str()).collect();
        assert_eq!(lines, [" rust", "  fn x() {}", ""]);
        assert!(doc.lines.iter().all(|l| l.fill.is_some()));
    }

    #[test]
    fn tables_are_boxed_and_aligned() {
        let t = texts("| a | bb |\n|---|---:|\n| ccc | d |\n");
        assert_eq!(
            t,
            [
                "┌─────┬────┐",
                "│ a   │ bb │",
                "├─────┼────┤",
                "│ ccc │  d │",
                "└─────┴────┘"
            ]
        );
    }

    #[test]
    fn inline_styles() {
        let doc = render("a **b** `c` [d](http://x)", &Theme::default(), None);
        let line = &doc.lines[0].content;
        assert_eq!(line.text, "a b c d");
        assert!(line.style_at(2).bold);
        assert_eq!(line.style_at(4).bg, Some(Theme::default().code_bg));
        assert!(line.style_at(6).underline);
    }
}
