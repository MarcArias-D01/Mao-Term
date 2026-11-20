//! Automatic shell-integration injection: writes the scripts from
//! `shell-integration/` to a data folder and adjusts the shell's launch so it
//! loads them, without touching the user's dotfiles.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PWSH: &str = include_str!("../../../shell-integration/mao.ps1");
const BASH: &str = include_str!("../../../shell-integration/mao.bash");
const FISH: &str = include_str!("../../../shell-integration/mao.fish");
const ZSH_ENV: &str = include_str!("../../../shell-integration/zsh/.zshenv");
const ZSH_RC: &str = include_str!("../../../shell-integration/zsh/.zshrc");

/// Shells we know how to integrate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    PowerShell,
    Bash,
    Zsh,
    Fish,
    Other,
}

impl ShellKind {
    pub fn detect(program: &str) -> Self {
        // Both separators, whatever the OS (a Windows path in a shared config).
        let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let file = file.to_ascii_lowercase();
        let name = file.strip_suffix(".exe").unwrap_or(&file).to_owned();
        match name.as_str() {
            "pwsh" | "powershell" => Self::PowerShell,
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            _ => Self::Other,
        }
    }
}

/// Adds the arguments/environment that make `program` load our integration
/// script from `dir`. Leaves the launch untouched for unknown shells, when the
/// user's arguments already run a script/command, or if writing the scripts fails.
pub fn inject(
    program: &str,
    args: &mut Vec<String>,
    env: &mut HashMap<String, String>,
    dir: &Path,
) {
    let has = |flags: &[&str]| {
        args.iter()
            .any(|a| flags.iter().any(|f| a.eq_ignore_ascii_case(f)))
    };
    let result = match ShellKind::detect(program) {
        ShellKind::PowerShell => {
            if has(&["-Command", "-c", "-File", "-f", "-EncodedCommand"]) {
                return;
            }
            write(dir, "mao.ps1", PWSH).map(|path| {
                // Dot-source in the global scope after the profile, then stay interactive.
                let path = path.display().to_string().replace('\'', "''");
                args.extend(["-NoExit".into(), "-Command".into(), format!(". '{path}'")]);
            })
        }
        ShellKind::Bash => {
            if has(&["-c", "--rcfile", "--init-file", "--norc", "--posix"]) {
                return;
            }
            write(dir, "mao.bash", BASH).map(|path| {
                args.splice(0..0, ["--rcfile".into(), path.display().to_string()]);
            })
        }
        ShellKind::Zsh => {
            if has(&["-c"]) {
                return;
            }
            let zdir = dir.join("zsh");
            write(&zdir, ".zshenv", ZSH_ENV)
                .and_then(|_| write(&zdir, ".zshrc", ZSH_RC))
                .map(|_| {
                    let user = std::env::var("ZDOTDIR")
                        .ok()
                        .or_else(|| std::env::var("HOME").ok())
                        .unwrap_or_default();
                    env.insert("MAO_USER_ZDOTDIR".into(), user);
                    env.insert("ZDOTDIR".into(), zdir.display().to_string());
                })
        }
        ShellKind::Fish => {
            if has(&["-c", "--command"]) {
                return;
            }
            write(dir, "mao.fish", FISH).map(|path| {
                let path = path.display().to_string().replace('\'', "\\'");
                args.extend(["--init-command".into(), format!("source '{path}'")]);
            })
        }
        ShellKind::Other => return,
    };
    if let Err(e) = result {
        tracing::warn!(
            "shell integration disabled: cannot write scripts to {}: {e}",
            dir.display()
        );
    }
}

/// Writes `content` to `dir/name` unless it is already identical.
fn write(dir: &Path, name: &str, content: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(name);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, content)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mao-integration-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_shells() {
        assert_eq!(
            ShellKind::detect(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            ShellKind::PowerShell
        );
        assert_eq!(ShellKind::detect("powershell.exe"), ShellKind::PowerShell);
        assert_eq!(ShellKind::detect("/usr/bin/zsh"), ShellKind::Zsh);
        assert_eq!(ShellKind::detect("cmd.exe"), ShellKind::Other);
    }

    #[test]
    fn powershell_dot_sources_script() {
        let dir = tmp();
        let mut args = vec!["-NoLogo".to_owned()];
        let mut env = HashMap::new();
        inject("pwsh.exe", &mut args, &mut env, &dir);
        assert_eq!(args[..3], ["-NoLogo", "-NoExit", "-Command"]);
        assert!(args[3].starts_with(". '") && args[3].ends_with("mao.ps1'"));
        assert!(dir.join("mao.ps1").is_file());
    }

    #[test]
    fn respects_user_commands() {
        let dir = tmp();
        let mut args = vec!["-c".to_owned(), "echo hi".to_owned()];
        let mut env = HashMap::new();
        inject("bash", &mut args, &mut env, &dir);
        assert_eq!(args, ["-c", "echo hi"]);
    }

    #[test]
    fn zsh_uses_zdotdir() {
        let dir = tmp();
        let mut args = Vec::new();
        let mut env = HashMap::new();
        inject("zsh", &mut args, &mut env, &dir);
        assert!(env["ZDOTDIR"].ends_with("zsh"));
        assert!(dir.join("zsh").join(".zshrc").is_file());
    }
}
