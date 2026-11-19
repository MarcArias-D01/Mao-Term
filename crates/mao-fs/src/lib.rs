//! File tree for the sidebar: lazy directory listing on a background thread,
//! `.gitignore`-aware, with watchers only on expanded folders.
//! See `docs/specs/file-tree.md`.

mod list;
mod service;
mod tree;

pub use list::{Entry, list_dir};
pub use service::{FsEvent, FsService};
pub use tree::{Row, Tree};

/// Opens `path` with the OS default application.
pub fn open_with_default_app(path: &std::path::Path) -> std::io::Result<()> {
    open::that_detached(path)
}

/// Shows `path` in the OS file manager (selected on Windows).
pub fn reveal_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(drop)
    }
    #[cfg(not(windows))]
    {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        open::that_detached(dir)
    }
}
