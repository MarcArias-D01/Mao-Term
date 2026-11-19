//! Command history: persistence, frecency and prefix search.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Most entries kept in memory.
const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub command: String,
    /// Where it was last run, if known.
    pub cwd: Option<PathBuf>,
    pub count: u32,
    /// Unix seconds of the last run (0 for imported entries).
    pub last: u64,
}

/// Command history, newest last.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    file: Option<PathBuf>,
}

impl History {
    /// Loads the history file (missing file = empty history). New commands are appended to it.
    pub fn load(path: &Path) -> Self {
        let mut h = Self {
            file: Some(path.to_owned()),
            ..Self::default()
        };
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let mut parts = line.splitn(3, '\t');
                let (Some(ts), Some(cwd), Some(cmd)) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let cwd = (!cwd.is_empty()).then(|| PathBuf::from(unescape(cwd)));
                h.record(&unescape(cmd), cwd, ts.parse().unwrap_or(0));
            }
        }
        h
    }

    /// Imports plain command lines (e.g. the shell's own history file) as older entries.
    pub fn import_lines<'a>(&mut self, lines: impl IntoIterator<Item = &'a str>) {
        let imported: Vec<Entry> = lines
            .into_iter()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| Entry {
                command: l.to_owned(),
                cwd: None,
                count: 1,
                last: 0,
            })
            .collect();
        // Older than everything we recorded ourselves: put them first.
        let mut merged = Vec::with_capacity(imported.len() + self.entries.len());
        let mut seen = HashMap::new();
        for e in imported
            .into_iter()
            .chain(std::mem::take(&mut self.entries))
        {
            match seen.get(&e.command) {
                Some(&i) => {
                    let existing: &mut Entry = &mut merged[i];
                    existing.count += e.count;
                    if e.last >= existing.last {
                        existing.last = e.last;
                        existing.cwd = e.cwd.or(existing.cwd.take());
                    }
                }
                None => {
                    seen.insert(e.command.clone(), merged.len());
                    merged.push(e);
                }
            }
        }
        self.entries = merged;
        self.trim();
        self.reindex();
    }

    /// Imports the most recent commands from the shells' own history files
    /// (PSReadLine on Windows; bash and zsh on Unix), so suggestions work from day one.
    pub fn import_shell_histories(&mut self) {
        const MAX_IMPORT: usize = 5_000;
        let mut lines: Vec<String> = Vec::new();
        for path in shell_history_files() {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let parsed: Vec<String> = text
                .lines()
                // zsh extended history: ": 1700000000:0;command"
                .map(|l| match l.strip_prefix(": ") {
                    Some(rest) => rest.split_once(';').map_or(l, |(_, c)| c),
                    None => l,
                })
                // Skip continuation lines of multi-line commands.
                .filter(|l| !l.ends_with('`') && !l.ends_with('\\'))
                .map(str::to_owned)
                .collect();
            let start = parsed.len().saturating_sub(MAX_IMPORT);
            lines.extend(parsed.into_iter().skip(start));
        }
        self.import_lines(lines.iter().map(String::as_str));
    }

    /// Records a command run in `cwd` and appends it to the history file.
    pub fn add(&mut self, command: &str, cwd: Option<&Path>) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.record(command, cwd.map(Path::to_path_buf), now);
        if let Some(path) = &self.file {
            let line = format!(
                "{now}\t{}\t{}\n",
                cwd.map_or(String::new(), |p| escape(&p.to_string_lossy())),
                escape(command)
            );
            let result = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(line.as_bytes()));
            if let Err(e) = result {
                tracing::warn!("cannot write history {}: {e}", path.display());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Best full command starting with `prefix` (longer than it): the most recent
    /// one run in `cwd`, else the most recent anywhere.
    pub fn suggest(&self, prefix: &str, cwd: Option<&Path>) -> Option<&str> {
        if prefix.trim().is_empty() {
            return None;
        }
        let mut fallback = None;
        for e in self.entries.iter().rev() {
            if e.command.len() > prefix.len() && e.command.starts_with(prefix) {
                if cwd.is_some() && e.cwd.as_deref() == cwd {
                    return Some(&e.command);
                }
                fallback.get_or_insert(e.command.as_str());
            }
        }
        fallback
    }

    /// Commands starting with `prefix`, newest first (for Up/Down navigation).
    pub fn matching(&self, prefix: &str) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .rev()
            .filter(move |e| e.command.starts_with(prefix))
            .map(|e| e.command.as_str())
    }

    /// How often `command` was run.
    pub fn count(&self, command: &str) -> u32 {
        self.index
            .get(command)
            .map_or(0, |&i| self.entries[i].count)
    }

    /// First words (commands) with how often they were used.
    pub fn command_words(&self) -> HashMap<&str, u32> {
        let mut words = HashMap::new();
        for e in &self.entries {
            if let Some(w) = e.command.split_whitespace().next() {
                *words.entry(w).or_insert(0) += e.count;
            }
        }
        words
    }

    fn record(&mut self, command: &str, cwd: Option<PathBuf>, ts: u64) {
        if let Some(&i) = self.index.get(command) {
            // Move to the end (most recent).
            let mut e = self.entries.remove(i);
            e.count += 1;
            e.last = e.last.max(ts);
            if cwd.is_some() {
                e.cwd = cwd;
            }
            self.entries.push(e);
            self.reindex_from(i);
        } else {
            self.index.insert(command.to_owned(), self.entries.len());
            self.entries.push(Entry {
                command: command.to_owned(),
                cwd,
                count: 1,
                last: ts,
            });
            if self.entries.len() > MAX_ENTRIES + MAX_ENTRIES / 10 {
                self.trim();
                self.reindex();
            }
        }
    }

    fn trim(&mut self) {
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
    }

    fn reindex(&mut self) {
        self.reindex_from(0);
    }

    fn reindex_from(&mut self, from: usize) {
        for (i, e) in self.entries.iter().enumerate().skip(from) {
            self.index.insert(e.command.clone(), i);
        }
    }
}

fn shell_history_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            files.push(
                PathBuf::from(appdata)
                    .join(r"Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"),
            );
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        files.push(home.join(".bash_history"));
        files.push(home.join(".zsh_history"));
    }
    files
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_prefers_cwd_then_recency() {
        let mut h = History::default();
        let a = Path::new("/a");
        let b = Path::new("/b");
        h.add("cargo build", Some(a));
        h.add("cargo test", Some(b));
        assert_eq!(h.suggest("cargo", Some(a)), Some("cargo build"));
        assert_eq!(
            h.suggest("cargo", Some(Path::new("/c"))),
            Some("cargo test")
        );
        assert_eq!(h.suggest("cargo test", Some(a)), None);
        assert_eq!(h.suggest("", Some(a)), None);
    }

    #[test]
    fn duplicates_move_to_front() {
        let mut h = History::default();
        h.add("ls", None);
        h.add("pwd", None);
        h.add("ls", None);
        assert_eq!(h.matching("").collect::<Vec<_>>(), ["ls", "pwd"]);
        assert_eq!(h.count("ls"), 2);
    }

    #[test]
    fn persists_and_reloads() {
        let path =
            std::env::temp_dir().join(format!("mao-history-test-{}.tsv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut h = History::load(&path);
            h.add("echo \"tab\there\"", Some(Path::new("/x")));
            h.add("git status", None);
        }
        let h = History::load(&path);
        assert_eq!(h.len(), 2);
        assert_eq!(
            h.suggest("echo", Some(Path::new("/x"))),
            Some("echo \"tab\there\"")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn imports_as_older() {
        let mut h = History::default();
        h.add("git push", None);
        h.import_lines(["git pull", "git push", "  "]);
        assert_eq!(
            h.matching("git").collect::<Vec<_>>(),
            ["git push", "git pull"]
        );
        assert_eq!(h.count("git push"), 2);
    }
}
