//! Tree model: which folders are expanded and what they contain.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::list::Entry;

#[derive(Debug, Clone, Default)]
enum Dir {
    #[default]
    Loading,
    Loaded(Vec<Entry>),
    Failed(String),
}

/// A visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<'a> {
    pub depth: usize,
    pub entry: &'a Entry,
    pub expanded: bool,
    /// Expanded folder whose listing hasn't arrived yet.
    pub loading: bool,
}

/// The tree under `root`. Pure state; listing is requested by the caller.
#[derive(Debug, Default)]
pub struct Tree {
    root: Option<PathBuf>,
    dirs: HashMap<PathBuf, Dir>,
    expanded: HashSet<PathBuf>,
}

impl Tree {
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Changes the root. Returns `true` if it changed (the caller lists it).
    pub fn set_root(&mut self, root: &Path) -> bool {
        if self.root.as_deref() == Some(root) {
            return false;
        }
        self.root = Some(root.to_path_buf());
        self.dirs.entry(root.to_path_buf()).or_default();
        true
    }

    /// Error listing the root, if any.
    pub fn root_error(&self) -> Option<&str> {
        match self.dirs.get(self.root.as_ref()?)? {
            Dir::Failed(e) => Some(e),
            _ => None,
        }
    }

    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    /// Expands or collapses `dir`. Returns `true` if it must be listed now.
    pub fn toggle(&mut self, dir: &Path) -> bool {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            if !self.dirs.contains_key(dir) {
                self.dirs.insert(dir.to_path_buf(), Dir::Loading);
                return true;
            }
        }
        false
    }

    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    /// Stores a listing.
    pub fn on_listed(&mut self, dir: &Path, entries: Vec<Entry>) {
        self.dirs.insert(dir.to_path_buf(), Dir::Loaded(entries));
    }

    pub fn on_failed(&mut self, dir: &Path, error: String) {
        self.dirs.insert(dir.to_path_buf(), Dir::Failed(error));
    }

    /// Forgets cached listings (e.g. when toggling ignored files). Returns the
    /// folders that must be listed again (root and expanded ones).
    pub fn invalidate(&mut self) -> Vec<PathBuf> {
        let out = self.visible_dirs();
        self.dirs.clear();
        for d in &out {
            self.dirs.insert(d.clone(), Dir::Loading);
        }
        out
    }

    /// Folders whose contents are on screen (root + expanded ones reachable from it).
    pub fn visible_dirs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            out.push(root.clone());
            for row in self.rows() {
                if row.expanded {
                    out.push(row.entry.path.clone());
                }
            }
        }
        out
    }

    /// Visible rows in display order.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            self.push_rows(root, 0, &mut out);
        }
        out
    }

    fn push_rows<'a>(&'a self, dir: &Path, depth: usize, out: &mut Vec<Row<'a>>) {
        let Some(Dir::Loaded(entries)) = self.dirs.get(dir) else {
            return;
        };
        for entry in entries {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            let loading =
                expanded && matches!(self.dirs.get(&entry.path), None | Some(Dir::Loading));
            out.push(Row {
                depth,
                entry,
                expanded,
                loading,
            });
            if expanded {
                self.push_rows(&entry.path, depth + 1, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(path: &str, is_dir: bool) -> Entry {
        Entry {
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            path: PathBuf::from(path),
            is_dir,
        }
    }

    #[test]
    fn expands_lazily() {
        let mut t = Tree::default();
        assert!(t.set_root(Path::new("/r")));
        assert!(!t.set_root(Path::new("/r")));
        t.on_listed(
            Path::new("/r"),
            vec![e("/r/src", true), e("/r/a.txt", false)],
        );
        assert_eq!(t.rows().len(), 2);

        // First expand needs a listing; rows show it as loading meanwhile.
        assert!(t.toggle(Path::new("/r/src")));
        assert!(t.rows()[0].loading);
        t.on_listed(Path::new("/r/src"), vec![e("/r/src/main.rs", false)]);
        let rows = t.rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].entry.name, "main.rs");

        // Collapse, then expand again from cache.
        assert!(!t.toggle(Path::new("/r/src")));
        assert_eq!(t.rows().len(), 2);
        assert!(!t.toggle(Path::new("/r/src")));
        assert_eq!(t.rows().len(), 3);
        assert_eq!(
            t.visible_dirs(),
            [PathBuf::from("/r"), PathBuf::from("/r/src")]
        );
    }

    #[test]
    fn invalidate_relists_visible() {
        let mut t = Tree::default();
        t.set_root(Path::new("/r"));
        t.on_listed(Path::new("/r"), vec![e("/r/src", true)]);
        t.toggle(Path::new("/r/src"));
        t.on_listed(Path::new("/r/src"), vec![]);
        let again = t.invalidate();
        assert_eq!(again, [PathBuf::from("/r"), PathBuf::from("/r/src")]);
    }
}
