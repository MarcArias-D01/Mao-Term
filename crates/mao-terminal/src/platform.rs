//! OS-specific helpers. See `docs/skills/cross-platform/SKILL.md`.

/// Default shell (program and arguments) when none is configured.
///
/// Windows: `pwsh` (PowerShell 7) if found on `PATH`, else `powershell`.
/// Unix: `$SHELL`, else `/bin/bash`.
#[cfg(windows)]
pub fn default_shell() -> (String, Vec<String>) {
    let program = if find_in_path("pwsh.exe") {
        "pwsh.exe"
    } else {
        "powershell.exe"
    };
    (program.to_owned(), vec!["-NoLogo".to_owned()])
}

#[cfg(not(windows))]
pub fn default_shell() -> (String, Vec<String>) {
    let program = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/bash".to_owned());
    (program, Vec::new())
}

#[cfg(windows)]
fn find_in_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(exe).is_file()))
}
