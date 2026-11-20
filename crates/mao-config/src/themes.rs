//! Color themes: built-in ones, plus the user's `<config dir>/themes/*.toml`.
//!
//! A theme file holds the same keys as a color palette:
//!
//! ```toml
//! name = "My Theme"          # optional, defaults to the file name
//! background = "#1a1b26"
//! foreground = "#c0caf5"
//! cursor = "#c0caf5"
//! selection = "#283457"
//! ansi = ["#15161e", …16 colors…]
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Rgb, ThemeConfig, config_dir};

/// Name of the default theme.
pub const DEFAULT_THEME: &str = "Mao Dark";

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: ThemeConfig,
    /// Where it came from (`None` = built in).
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: Option<String>,
    background: Rgb,
    foreground: Rgb,
    cursor: Rgb,
    selection: Rgb,
    ansi: [Rgb; 16],
}

const BUILTIN: &[&str] = &[
    include_str!("../themes/mao-dark.toml"),
    include_str!("../themes/tokyo-night.toml"),
    include_str!("../themes/catppuccin-mocha.toml"),
    include_str!("../themes/dracula.toml"),
    include_str!("../themes/nord.toml"),
    include_str!("../themes/gruvbox-dark.toml"),
    include_str!("../themes/one-dark.toml"),
    include_str!("../themes/rose-pine.toml"),
    include_str!("../themes/solarized-dark.toml"),
    include_str!("../themes/solarized-light.toml"),
    include_str!("../themes/github-light.toml"),
];

/// Parses a theme file; `fallback_name` is used when it has no `name`.
pub fn parse(text: &str, fallback_name: &str) -> Result<Theme, toml::de::Error> {
    let f: ThemeFile = toml::from_str(text)?;
    Ok(Theme {
        name: f.name.unwrap_or_else(|| fallback_name.to_owned()),
        colors: ThemeConfig {
            background: f.background,
            foreground: f.foreground,
            cursor: f.cursor,
            selection: f.selection,
            ansi: f.ansi,
        },
        path: None,
    })
}

pub fn builtin() -> Vec<Theme> {
    BUILTIN
        .iter()
        .filter_map(|text| {
            parse(text, "")
                .map_err(|e| tracing::error!("built-in theme is invalid: {e}"))
                .ok()
        })
        .collect()
}

/// Folder for user themes.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

/// Themes found in `dir` (invalid files are skipped with a warning).
pub fn load_dir(dir: &Path) -> Vec<Theme> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Theme> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            match parse(&text, &stem) {
                Ok(mut t) => {
                    t.path = Some(path);
                    Some(t)
                }
                Err(e) => {
                    tracing::warn!("invalid theme {}: {e}", path.display());
                    None
                }
            }
        })
        .collect();
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

/// Built-in themes followed by the user's (a user theme with a built-in's
/// name replaces it).
pub fn list() -> Vec<Theme> {
    let mut all = builtin();
    for t in themes_dir().map(|d| load_dir(&d)).unwrap_or_default() {
        match all.iter_mut().find(|b| same_name(&b.name, &t.name)) {
            Some(slot) => *slot = t,
            None => all.push(t),
        }
    }
    all
}

/// Looks a theme up by name, ignoring case, spaces, dashes and underscores.
pub fn find(name: &str) -> Option<Theme> {
    list().into_iter().find(|t| same_name(&t.name, name))
}

fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, ' ' | '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    norm(a) == norm(b)
}

/// Writes `theme = "<name>"` into the config file, keeping everything else.
pub fn save_choice(path: &Path, name: &str) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => crate::CONFIG_TEMPLATE.to_owned(),
        Err(e) => return Err(e),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, set_theme_line(&text, name))
}

/// Sets the top-level `theme` key in a TOML document's text.
fn set_theme_line(text: &str, name: &str) -> String {
    let line = format!(
        "theme = \"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    // Only top-level keys count: those before the first table header.
    let first_table = lines
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let is_theme_key = |l: &str| {
        let t = l.trim_start();
        t.strip_prefix("theme")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    if let Some(i) = lines[..first_table].iter().position(|l| is_theme_key(l)) {
        lines[i] = line;
    } else {
        // After the leading comments, before the first key or table.
        let at = lines[..first_table]
            .iter()
            .position(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#')
            })
            .unwrap_or(first_table);
        lines.insert(at, line);
        if at + 1 < lines.len() && !lines[at + 1].trim().is_empty() {
            lines.insert(at + 1, String::new());
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_default_matches() {
        let all = builtin();
        assert_eq!(all.len(), BUILTIN.len());
        assert_eq!(all[0].name, DEFAULT_THEME);
        assert_eq!(all[0].colors, ThemeConfig::default());
    }

    #[test]
    fn find_ignores_case_and_separators() {
        assert_eq!(
            find("tokyo-night").map(|t| t.name).as_deref(),
            Some("Tokyo Night")
        );
        assert_eq!(
            find("CATPPUCCIN_mocha").map(|t| t.name).as_deref(),
            Some("Catppuccin Mocha")
        );
        assert!(find("no such theme").is_none());
    }

    #[test]
    fn user_dir_themes_load() {
        let dir = std::env::temp_dir().join(format!("mao-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mao = include_str!("../themes/mao-dark.toml").replace("name = \"Mao Dark\"\n", "");
        std::fs::write(dir.join("mine.toml"), mao).unwrap();
        std::fs::write(dir.join("broken.toml"), "background = 3").unwrap();
        let themes = load_dir(&dir);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].name, "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_line_is_set_at_top_level() {
        let text = "# comment\n\n[font]\nsize = 12.0\n";
        assert_eq!(
            set_theme_line(text, "Nord"),
            "# comment\n\ntheme = \"Nord\"\n\n[font]\nsize = 12.0\n"
        );
        let text = "theme = \"Old\"\n[font]\ntheme_like = 1\n";
        assert_eq!(
            set_theme_line(text, "Nord"),
            "theme = \"Nord\"\n[font]\ntheme_like = 1\n"
        );
        // A commented example stays; the key goes before the tables.
        let text = "# theme = \"x\"\n[colors]\n";
        assert_eq!(
            set_theme_line(text, "Nord"),
            "# theme = \"x\"\ntheme = \"Nord\"\n\n[colors]\n"
        );
    }
}
