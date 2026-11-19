//! Listing one directory.

use std::path::{Path, PathBuf};

/// Most entries listed per directory (the rest are summarized).
pub const MAX_ENTRIES: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Lists `dir` (not recursive): folders first, then files, natural order.
/// With `respect_ignore`, entries matched by `.gitignore` (and friends) and
/// `.git` itself are skipped; hidden (dot) files are shown either way.
pub fn list_dir(dir: &Path, respect_ignore: bool) -> std::io::Result<Vec<Entry>> {
    // Fail early with a real error (permission denied, missing…).
    std::fs::read_dir(dir)?;

    let walker = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .git_ignore(respect_ignore)
        .git_global(respect_ignore)
        .git_exclude(respect_ignore)
        .ignore(respect_ignore)
        .parents(respect_ignore)
        // Honor .gitignore even outside a git repo (e.g. a fresh folder).
        .require_git(false)
        .follow_links(false)
        .build();

    let mut entries: Vec<Entry> = walker
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .filter(|e| !(respect_ignore && e.file_name() == ".git"))
        .take(MAX_ENTRIES)
        .map(|e| {
            let path = e.path().to_path_buf();
            // A symlink to a directory counts as a directory.
            let is_dir =
                e.file_type().is_some_and(|t| t.is_dir()) || (e.path_is_symlink() && path.is_dir());
            Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path,
                is_dir,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    Ok(entries)
}

/// Case-insensitive comparison where digit runs compare by value ("file2" < "file10").
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    // Case only breaks ties between names that are otherwise equal.
    let mut case_tiebreak = Ordering::Equal;
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return case_tiebreak,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut n = String::new();
                    while let Some(&c) = it.peek().filter(|c| c.is_ascii_digit()) {
                        n.push(c);
                        it.next();
                    }
                    n
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                if case_tiebreak == Ordering::Equal {
                    case_tiebreak = x.cmp(&y).reverse();
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mao-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::write(dir.join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::write(dir.join("README.md"), "").unwrap();
        std::fs::write(dir.join("build.log"), "").unwrap();
        std::fs::write(dir.join("file10.txt"), "").unwrap();
        std::fs::write(dir.join("file2.txt"), "").unwrap();
        dir
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn respects_gitignore_and_sorts() {
        let dir = fixture("ignore");
        let entries = list_dir(&dir, true).unwrap();
        assert_eq!(
            names(&entries),
            ["src", ".gitignore", "file2.txt", "file10.txt", "README.md"]
        );
        assert!(entries[0].is_dir);
    }

    #[test]
    fn can_show_ignored() {
        let dir = fixture("all");
        let entries = list_dir(&dir, false).unwrap();
        assert!(names(&entries).contains(&"target"));
        assert!(names(&entries).contains(&"build.log"));
    }

    #[test]
    fn missing_dir_is_an_error() {
        assert!(list_dir(Path::new("/definitely/not/here"), true).is_err());
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["b", "a10", "A2", "a1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a1", "A2", "a10", "b"]);
    }
}
