//! Shell-aware tokenizer: finds the command being typed and the token under the caret.

use std::ops::Range;

/// Quoting/escaping rules of the target shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quoting {
    /// PowerShell: `'…'` literal, `"…"`, backtick escapes.
    PowerShell,
    /// POSIX shells (bash, zsh, fish): `'…'`, `"…"`, backslash escapes.
    #[default]
    Posix,
}

impl Quoting {
    fn escape(self) -> char {
        match self {
            Self::PowerShell => '`',
            Self::Posix => '\\',
        }
    }

    /// Quotes `s` if it contains characters the shell would split or interpret.
    pub fn quote(self, s: &str) -> String {
        let needs = s.is_empty()
            || s.chars().any(|c| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '\'' | '"'
                            | '$'
                            | '`'
                            | '&'
                            | '|'
                            | ';'
                            | '('
                            | ')'
                            | '<'
                            | '>'
                            | '#'
                            | '{'
                            | '}'
                    )
            });
        if !needs {
            return s.to_owned();
        }
        match self {
            Self::PowerShell => format!("'{}'", s.replace('\'', "''")),
            Self::Posix => format!("'{}'", s.replace('\'', r"'\''")),
        }
    }
}

/// A word of the command line, with quotes removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    /// Byte range in the line, including quotes.
    pub range: Range<usize>,
}

/// The command under the caret: its previous tokens and the (possibly empty) current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    pub previous: Vec<Token>,
    pub current: Token,
}

/// Tokenizes `line[..cursor]`, keeping only the last command (after `|`, `;`, `&&`…).
pub fn command_at(line: &str, cursor: usize, quoting: Quoting) -> CommandContext {
    let cursor = cursor.min(line.len());
    let text = &line[..cursor];
    let escape = quoting.escape();

    let mut tokens: Vec<Token> = Vec::new();
    let mut cur = String::new();
    let mut start: Option<usize> = None;
    let mut quote: Option<char> = None;
    let mut chars = text.char_indices().peekable();

    let end_token =
        |tokens: &mut Vec<Token>, cur: &mut String, start: &mut Option<usize>, end: usize| {
            if let Some(s) = start.take() {
                tokens.push(Token {
                    text: std::mem::take(cur),
                    range: s..end,
                });
            }
        };

    while let Some((i, c)) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == escape && q == '"' {
                    if let Some((_, n)) = chars.next() {
                        cur.push(n);
                    }
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c.is_whitespace() {
                    end_token(&mut tokens, &mut cur, &mut start, i);
                } else if matches!(c, '|' | ';' | '&') || (c == '(' && start.is_none()) {
                    // Command separator: start over.
                    end_token(&mut tokens, &mut cur, &mut start, i);
                    tokens.clear();
                } else {
                    start.get_or_insert(i);
                    if c == '\'' || c == '"' {
                        quote = Some(c);
                    } else if c == escape {
                        if let Some((_, n)) = chars.next() {
                            cur.push(n);
                        }
                    } else {
                        cur.push(c);
                    }
                }
            }
        }
    }

    let current = match start {
        Some(s) => Token {
            text: cur,
            range: s..cursor,
        },
        None => Token {
            text: String::new(),
            range: cursor..cursor,
        },
    };
    CommandContext {
        previous: tokens,
        current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(line: &str) -> CommandContext {
        command_at(line, line.len(), Quoting::Posix)
    }

    fn texts(c: &CommandContext) -> Vec<&str> {
        c.previous.iter().map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn simple_words() {
        let c = ctx("git checkout ma");
        assert_eq!(texts(&c), ["git", "checkout"]);
        assert_eq!(c.current.text, "ma");
        assert_eq!(c.current.range, 13..15);
    }

    #[test]
    fn trailing_space_means_new_token() {
        let c = ctx("cargo ");
        assert_eq!(texts(&c), ["cargo"]);
        assert_eq!(c.current.text, "");
        assert_eq!(c.current.range, 6..6);
    }

    #[test]
    fn quotes_and_escapes() {
        let c = ctx(r#"cd "My Docs/a b" 'x y'"#);
        assert_eq!(texts(&c), ["cd", "My Docs/a b"]);
        assert_eq!(c.current.text, "x y");
        let c = ctx(r"ls My\ Do");
        assert_eq!(c.current.text, "My Do");
    }

    #[test]
    fn last_command_only() {
        let c = ctx("cargo build && git pu");
        assert_eq!(texts(&c), ["git"]);
        assert_eq!(c.current.text, "pu");
        let c = ctx("ls | gre");
        assert!(c.previous.is_empty());
        assert_eq!(c.current.text, "gre");
    }

    #[test]
    fn powershell_backtick() {
        let c = command_at("cd My` Do", 9, Quoting::PowerShell);
        assert_eq!(c.current.text, "My Do");
    }

    #[test]
    fn quoting() {
        assert_eq!(Quoting::Posix.quote("a b"), "'a b'");
        assert_eq!(Quoting::PowerShell.quote("it's"), "'it''s'");
        assert_eq!(Quoting::Posix.quote("plain.txt"), "plain.txt");
    }
}
