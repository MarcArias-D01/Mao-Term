//! OS-specific helpers. See `docs/skills/cross-platform/SKILL.md`.

use alacritty_terminal::tty::Shell;

/// Default shell when none is configured.
///
/// Windows: `pwsh` (PowerShell 7) if found on `PATH`, else `powershell`.
/// Unix: `None`, letting the PTY layer use `$SHELL` / the passwd entry.
#[cfg(windows)]
pub fn default_shell() -> Option<Shell> {
    let program = if find_in_path("pwsh.exe") {
        "pwsh.exe"
    } else {
        "powershell.exe"
    };
    Some(Shell::new(program.to_owned(), vec!["-NoLogo".to_owned()]))
}

#[cfg(not(windows))]
pub fn default_shell() -> Option<Shell> {
    None
}

#[cfg(windows)]
fn find_in_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(exe).is_file()))
}
