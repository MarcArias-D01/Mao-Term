//! File-system completion and `PATH` executable discovery.

use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 300;

/// A directory entry offered as a completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathItem {
    /// Text to insert for the token (directory part kept, separator appended for dirs).
    pub insert: String,
    /// Just the entry name.
    pub name: String,
    pub is_dir: bool,
}

/// Lists entries matching `token` (a partial path, unquoted), relative to `cwd`.
/// Skips UNC/network locations so the UI never blocks on the network.
pub fn complete(token: &str, cwd: Option<&Path>, dirs_only: bool) -> Vec<PathItem> {
    let sep_pos = token.rfind(['/', '\\']);
    let (dir_part, prefix) = match sep_pos {
        Some(i) => (&token[..=i], &token[i + 1..]),
        None => ("", token),
    };
    let base = resolve(dir_part, cwd);
    let Some(base) = base else {
        return Vec::new();
    };
    if is_network(&base) {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let show_hidden = prefix.starts_with('.');
    let mut items: Vec<PathItem> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !starts_with(&name, prefix) || (!show_hidden && name.starts_with('.')) {
                return None;
            }
            // `file_type` doesn't follow symlinks; `metadata` does (a link to a dir is a dir).
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir())
                || (entry.file_type().is_ok_and(|t| t.is_symlink()) && entry.path().is_dir());
            if dirs_only && !is_dir {
                return None;
            }
            let mut insert = format!("{dir_part}{name}");
            if is_dir {
                insert.push(sep);
            }
            Some(PathItem {
                insert,
                name,
                is_dir,
            })
        })
        .take(MAX_ENTRIES * 4)
        .collect();
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items.truncate(MAX_ENTRIES);
    items
}

fn resolve(dir_part: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let expanded = if let Some(rest) = dir_part.strip_prefix('~') {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
    } else {
        PathBuf::from(dir_part)
    };
    if expanded.is_absolute() || dir_part.starts_with(['/', '\\']) {
        Some(expanded)
    } else {
        Some(cwd?.join(expanded))
    }
}

fn is_network(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(r"\\") || s.starts_with("//")
}

/// Prefix match; case-insensitive on Windows.
pub(crate) fn starts_with(name: &str, prefix: &str) -> bool {
    if cfg!(windows) {
        name.len() >= prefix.len()
            && name
                .get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    } else {
        name.starts_with(prefix)
    }
}

/// Executable names found on `PATH` (deduplicated, sorted). Slow (hundreds of
/// directory reads): run it on a background thread.
pub fn scan_path_executables() -> Vec<String> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut names = std::collections::BTreeSet::new();
    #[cfg(windows)]
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.PS1".into())
        .split(';')
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .chain(["ps1".to_owned()])
        .collect();
    for dir in std::env::split_paths(&path) {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            let p = entry.path();
            #[cfg(windows)]
            {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase);
                if ext.is_some_and(|e| exts.contains(&e))
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                {
                    names.insert(stem.to_owned());
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if entry
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                    && let Some(name) = p.file_name().and_then(|s| s.to_str())
                {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mao-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("My Docs")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        std::fs::write(dir.join("src").join("main.rs"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();
        dir
    }

    #[test]
    fn lists_relative_entries_dirs_first() {
        let dir = fixture("list");
        let items = complete("", Some(&dir), false);
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["My Docs", "src", "Cargo.toml"]);
        assert!(items[1].insert.ends_with(['/', '\\']));
    }

    #[test]
    fn filters_by_prefix_and_keeps_dir_part() {
        let dir = fixture("filter");
        let items = complete("src/ma", Some(&dir), false);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert, "src/main.rs");
        assert_eq!(complete(".h", Some(&dir), false)[0].name, ".hidden");
        assert!(complete("", Some(&dir), true).iter().all(|i| i.is_dir));
    }
}
