//! Autocomplete engine: inline suggestions from history and a completion menu
//! from command specs, paths, git branches and `PATH` executables.
//! See `docs/specs/input-autocomplete.md`.

mod git;
pub mod history;
pub mod paths;
pub mod spec;
pub mod tokenize;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

pub use history::History;
pub use tokenize::{Quoting, Token};

use spec::{ArgKind, Spec};

const MAX_ITEMS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Command,
    Subcommand,
    Option,
    Directory,
    File,
    Branch,
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Replacement text for the token (already quoted if needed).
    pub insert: String,
    /// What the menu shows.
    pub label: String,
    pub detail: String,
    pub kind: ItemKind,
}

/// Candidates for the token being typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completions {
    /// Byte range of the line the chosen item replaces.
    pub range: Range<usize>,
    pub items: Vec<Item>,
}

impl Completions {
    /// Longest text all items start with (to extend the token on Tab).
    pub fn common_prefix(&self) -> Option<String> {
        let first = &self.items.first()?.insert;
        let mut len = first.len();
        for item in &self.items[1..] {
            len = first
                .char_indices()
                .zip(item.insert.chars())
                .take_while(|((_, a), b)| a == b)
                .last()
                .map_or(0, |((i, a), _)| (i + a.len_utf8()).min(len));
        }
        Some(first[..len].to_owned())
    }
}

/// Everything the engine needs about the line being edited.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub line: &'a str,
    pub cursor: usize,
    pub cwd: Option<&'a Path>,
    pub quoting: Quoting,
}

pub struct Engine {
    pub history: History,
    specs: Vec<Spec>,
    executables: Vec<String>,
}

impl Engine {
    pub fn new(history: History) -> Self {
        Self {
            history,
            specs: spec::builtin(),
            executables: Vec::new(),
        }
    }

    /// Sets the `PATH` executables (scanned in the background).
    pub fn set_executables(&mut self, names: Vec<String>) {
        self.executables = names;
    }

    /// Ghost-text suggestion: what to append to `line` (from history).
    pub fn inline(&self, line: &str, cwd: Option<&Path>) -> Option<String> {
        self.history
            .suggest(line, cwd)
            .map(|full| full[line.len()..].to_owned())
    }

    /// Completion menu for the token under the caret.
    pub fn complete(&self, req: Request<'_>) -> Completions {
        let ctx = tokenize::command_at(req.line, req.cursor, req.quoting);
        let token = &ctx.current;
        let mut items = Vec::new();

        let looks_like_path =
            token.text.contains(['/', '\\']) || token.text.starts_with(['.', '~']);

        if ctx.previous.is_empty() {
            if looks_like_path {
                self.add_paths(&mut items, &token.text, req, false);
            } else {
                self.add_commands(&mut items, &token.text);
            }
        } else {
            let command = normalize_command(&ctx.previous[0].text);
            let spec = self.specs.iter().find(|s| s.matches(&command));
            match spec {
                Some(spec) => self.add_from_spec(&mut items, spec, &ctx.previous[1..], token, req),
                None => self.add_paths(&mut items, &token.text, req, false),
            }
        }

        items.truncate(MAX_ITEMS);
        Completions {
            range: token.range.clone(),
            items,
        }
    }

    fn add_commands(&self, items: &mut Vec<Item>, prefix: &str) {
        let used = self.history.command_words();
        let mut seen = HashSet::new();
        let mut cands: Vec<(u32, Item)> = Vec::new();
        for s in &self.specs {
            for name in std::iter::once(&s.name).chain(&s.aliases) {
                if paths::starts_with(name, prefix) && seen.insert(name.to_lowercase()) {
                    cands.push((
                        used.get(name.as_str()).copied().unwrap_or(0) + 1,
                        Item {
                            insert: name.clone(),
                            label: name.clone(),
                            detail: s.description.clone(),
                            kind: ItemKind::Command,
                        },
                    ));
                }
            }
        }
        for (word, count) in &used {
            if paths::starts_with(word, prefix) && seen.insert(word.to_lowercase()) {
                cands.push((
                    *count,
                    Item {
                        insert: (*word).to_owned(),
                        label: (*word).to_owned(),
                        detail: "history".into(),
                        kind: ItemKind::Command,
                    },
                ));
            }
        }
        // Executables only once something was typed: there are hundreds.
        if !prefix.is_empty() {
            for exe in &self.executables {
                if paths::starts_with(exe, prefix) && seen.insert(exe.to_lowercase()) {
                    cands.push((
                        0,
                        Item {
                            insert: exe.clone(),
                            label: exe.clone(),
                            detail: String::new(),
                            kind: ItemKind::Command,
                        },
                    ));
                }
            }
        }
        // Most used first, then alphabetical.
        cands.sort_by(|(ca, a), (cb, b)| {
            cb.cmp(ca)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        items.extend(cands.into_iter().map(|(_, i)| i));
    }

    fn add_from_spec(
        &self,
        items: &mut Vec<Item>,
        spec: &Spec,
        args: &[Token],
        token: &Token,
        req: Request<'_>,
    ) {
        // Walk subcommands; stop at the first positional argument.
        let mut node = spec;
        let mut positional = false;
        for t in args {
            if t.text.starts_with('-') {
                continue;
            }
            match node.subcommand(&t.text) {
                Some(sub) if !positional => node = sub,
                _ => positional = true,
            }
        }

        if token.text.starts_with('-') {
            for o in &node.options {
                for name in &o.names {
                    if name.starts_with(&token.text) {
                        items.push(Item {
                            insert: name.clone(),
                            label: name.clone(),
                            detail: o.description.clone(),
                            kind: ItemKind::Option,
                        });
                    }
                }
            }
            return;
        }

        if !positional {
            let mut subs: Vec<&Spec> = node
                .subcommands
                .iter()
                .filter(|s| s.name.starts_with(&token.text))
                .collect();
            subs.sort_by(|a, b| a.name.cmp(&b.name));
            items.extend(subs.into_iter().map(|s| Item {
                insert: s.name.clone(),
                label: s.name.clone(),
                detail: s.description.clone(),
                kind: ItemKind::Subcommand,
            }));
            if !node.subcommands.is_empty() && node.args == ArgKind::None {
                return;
            }
        }

        match node.args {
            ArgKind::Branch => {
                if let Some(cwd) = req.cwd {
                    items.extend(
                        git::branches(cwd)
                            .into_iter()
                            .filter(|b| b.starts_with(&token.text))
                            .map(|b| Item {
                                insert: req.quoting.quote(&b),
                                label: b,
                                detail: "branch".into(),
                                kind: ItemKind::Branch,
                            }),
                    );
                }
                // `git checkout` also restores files.
                if items.is_empty() {
                    self.add_paths(items, &token.text, req, false);
                }
            }
            ArgKind::Dir => self.add_paths(items, &token.text, req, true),
            ArgKind::Path | ArgKind::Any => self.add_paths(items, &token.text, req, false),
            ArgKind::None => {}
        }
    }

    fn add_paths(&self, items: &mut Vec<Item>, token: &str, req: Request<'_>, dirs_only: bool) {
        items.extend(
            paths::complete(token, req.cwd, dirs_only)
                .into_iter()
                .map(|p| {
                    let insert = if p.insert.contains(char::is_whitespace)
                        || p.insert.contains(['\'', '"', '$', '`', '&', '(', ')'])
                    {
                        req.quoting.quote(&p.insert)
                    } else {
                        p.insert
                    };
                    Item {
                        insert,
                        label: if p.is_dir {
                            format!("{}{}", p.name, std::path::MAIN_SEPARATOR)
                        } else {
                            p.name
                        },
                        detail: String::new(),
                        kind: if p.is_dir {
                            ItemKind::Directory
                        } else {
                            ItemKind::File
                        },
                    }
                }),
        );
    }
}

/// `C:\x\Git.EXE` → `git`.
fn normalize_command(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let lower = name.to_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".ps1", ".com"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_owned();
        }
    }
    // Spec names are matched case-sensitively except for this normalization.
    if cfg!(windows) {
        lower
    } else {
        name.to_owned()
    }
}

/// Frequency of each command word in the history (exposed for tests and UI).
pub fn command_frequencies(history: &History) -> HashMap<String, u32> {
    history
        .command_words()
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(History::default())
    }

    fn req(line: &str) -> Request<'_> {
        Request {
            line,
            cursor: line.len(),
            cwd: None,
            quoting: Quoting::Posix,
        }
    }

    fn labels(c: &Completions) -> Vec<&str> {
        c.items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn git_subcommands() {
        let c = engine().complete(req("git ch"));
        assert_eq!(labels(&c), ["checkout", "cherry-pick"]);
        assert_eq!(c.range, 4..6);
        assert_eq!(c.common_prefix().as_deref(), Some("che"));
    }

    #[test]
    fn nested_subcommands_and_options() {
        let e = engine();
        assert_eq!(labels(&e.complete(req("docker compose u"))), ["up"]);
        assert!(labels(&e.complete(req("cargo build --r"))).contains(&"--release"));
        assert_eq!(labels(&e.complete(req("git stash p"))), ["pop"]);
    }

    #[test]
    fn commands_from_specs_and_history() {
        let mut e = engine();
        e.history.add("cargo run", None);
        e.history.add("cargo test", None);
        e.set_executables(vec!["cargo-watch".into(), "cat".into()]);
        let c = e.complete(req("ca"));
        // Used twice in history → first; then the rest alphabetically.
        assert_eq!(labels(&c)[0], "cargo");
        assert!(labels(&c).contains(&"cargo-watch"));
        assert!(labels(&c).contains(&"cat"));
    }

    #[test]
    fn inline_from_history() {
        let mut e = engine();
        e.history.add("git push origin main", None);
        assert_eq!(e.inline("git pu", None).as_deref(), Some("sh origin main"));
        assert_eq!(e.inline("ls", None), None);
    }

    #[test]
    fn windows_exe_names_match_specs() {
        assert_eq!(
            normalize_command(r"C:\Program Files\Git\cmd\git.exe"),
            "git"
        );
    }

    #[test]
    fn common_prefix_handles_unicode() {
        let c = Completions {
            range: 0..0,
            items: ["ñandú", "ñandúes"]
                .iter()
                .map(|s| Item {
                    insert: (*s).into(),
                    label: (*s).into(),
                    detail: String::new(),
                    kind: ItemKind::File,
                })
                .collect(),
        };
        assert_eq!(c.common_prefix().as_deref(), Some("ñandú"));
    }
}
