//! Git branch names, read straight from `.git` (no process spawn).

use std::path::{Path, PathBuf};

/// Finds the `.git` directory for `cwd` (walking up), following `gitdir:` files.
fn git_dir(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let path = dir.join(target);
            // Worktrees keep refs in the common dir.
            let common = std::fs::read_to_string(path.join("commondir"))
                .ok()
                .map(|c| path.join(c.trim()));
            return Some(common.unwrap_or(path));
        }
    }
    None
}

/// Local branches first, then remote ones as `remote/branch`.
pub fn branches(cwd: &Path) -> Vec<String> {
    let Some(git) = git_dir(cwd) else {
        return Vec::new();
    };
    let mut local = Vec::new();
    let mut remote = Vec::new();
    collect(&git.join("refs").join("heads"), "", &mut local);
    collect(&git.join("refs").join("remotes"), "", &mut remote);
    if let Ok(packed) = std::fs::read_to_string(git.join("packed-refs")) {
        for line in packed.lines() {
            let Some((_, name)) = line.split_once(' ') else {
                continue;
            };
            if let Some(b) = name.strip_prefix("refs/heads/") {
                local.push(b.to_owned());
            } else if let Some(b) = name.strip_prefix("refs/remotes/") {
                remote.push(b.to_owned());
            }
        }
    }
    remote.retain(|r| !r.ends_with("/HEAD"));
    local.sort();
    local.dedup();
    remote.sort();
    remote.dedup();
    // Skip `origin/x` when `x` exists locally.
    let local_set: std::collections::HashSet<String> = local.iter().cloned().collect();
    local.extend(remote.into_iter().filter(|r| {
        !r.split_once('/')
            .is_some_and(|(_, b)| local_set.contains(b))
    }));
    local
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(&entry.path(), &full, out);
        } else {
            out.push(full);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_loose_and_packed_refs() {
        let root = std::env::temp_dir().join(format!("mao-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("refs/heads/feature")).unwrap();
        std::fs::create_dir_all(git.join("refs/remotes/origin")).unwrap();
        std::fs::write(git.join("refs/heads/main"), "x").unwrap();
        std::fs::write(git.join("refs/heads/feature/login"), "x").unwrap();
        std::fs::write(git.join("refs/remotes/origin/HEAD"), "x").unwrap();
        std::fs::write(git.join("refs/remotes/origin/dev"), "x").unwrap();
        std::fs::write(git.join("packed-refs"), "# pack\nabc refs/heads/old\n").unwrap();
        let sub = root.join("src");
        std::fs::create_dir_all(&sub).unwrap();

        let b = branches(&sub);
        assert_eq!(b, ["feature/login", "main", "old", "origin/dev"]);
    }
}
