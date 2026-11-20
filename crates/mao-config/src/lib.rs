//! Configuration for Mao-Term.
//!
//! Every field has a default, so an empty or missing config file is valid.
//! See `docs/specs/config-theming.md`.

mod color;
pub mod themes;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use color::Rgb;

/// File name of the user configuration inside [`config_dir`].
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Errors that can happen while loading the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Root configuration.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Theme name (built in, or a file in `<config dir>/themes/`); a table of
    /// colors is also accepted (older configs).
    pub theme: ThemeChoice,
    /// Colors that override the theme's.
    pub colors: ColorOverrides,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub shell: ShellConfig,
    pub input: InputConfig,
    pub agents: AgentsConfig,
    /// The resolved colors (theme + overrides), filled in when loading.
    #[serde(skip)]
    pub palette: ThemeConfig,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ThemeChoice {
    Name(String),
    Colors(ColorOverrides),
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Name(themes::DEFAULT_THEME.to_owned())
    }
}

/// Individual colors to override; unset ones come from the theme.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorOverrides {
    pub background: Option<Rgb>,
    pub foreground: Option<Rgb>,
    pub cursor: Option<Rgb>,
    pub selection: Option<Rgb>,
    pub ansi: Option<[Rgb; 16]>,
}

impl ColorOverrides {
    pub fn apply(&self, t: &mut ThemeConfig) {
        if let Some(c) = self.background {
            t.background = c;
        }
        if let Some(c) = self.foreground {
            t.foreground = c;
        }
        if let Some(c) = self.cursor {
            t.cursor = c;
        }
        if let Some(c) = self.selection {
            t.selection = c;
        }
        if let Some(a) = self.ansi {
            t.ansi = a;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    pub family: String,
    /// Font size in points.
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono".to_owned(),
            size: 13.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// The 16 ANSI colors: black, red, green, yellow, blue, magenta, cyan, white,
    /// then their bright variants in the same order.
    pub ansi: [Rgb; 16],
}

impl Default for ThemeConfig {
    fn default() -> Self {
        let c = Rgb::new;
        Self {
            background: c(0x14, 0x16, 0x1b),
            foreground: c(0xd8, 0xde, 0xe9),
            cursor: c(0x8a, 0xb4, 0xf8),
            selection: c(0x2e, 0x3a, 0x55),
            ansi: [
                c(0x1d, 0x20, 0x27),
                c(0xf2, 0x77, 0x7a),
                c(0x99, 0xcc, 0x99),
                c(0xff, 0xcc, 0x66),
                c(0x66, 0x99, 0xcc),
                c(0xcc, 0x99, 0xcc),
                c(0x66, 0xcc, 0xcc),
                c(0xd8, 0xde, 0xe9),
                c(0x5c, 0x63, 0x70),
                c(0xff, 0x8f, 0x8f),
                c(0xb5, 0xe8, 0xa0),
                c(0xff, 0xe0, 0x8a),
                c(0x8a, 0xb4, 0xf8),
                c(0xe0, 0xb0, 0xff),
                c(0x8b, 0xe9, 0xfd),
                c(0xff, 0xff, 0xff),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Lines of scrollback history.
    pub scrollback: usize,
    /// Padding around the grid, in logical pixels.
    pub padding: f32,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            padding: 8.0,
        }
    }
}

/// Shell to launch. When `program` is unset the OS default is used
/// (`pwsh` → `powershell` on Windows, `$SHELL` on Linux).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    /// Load Mao-Term's shell integration (prompt marks, cwd) automatically.
    pub integration: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            integration: true,
        }
    }
}

/// The native input editor used at the shell prompt (ADR 0002).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    /// Edit commands in Mao-Term's editor (needs shell integration).
    pub editor: bool,
    /// Show history suggestions as ghost text while typing.
    pub suggestions: bool,
    /// Where the editor is: a box at the bottom of the pane (Warp-style, the
    /// output stays above) or inline at the shell's prompt.
    pub position: InputPosition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputPosition {
    #[default]
    Bottom,
    Inline,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            editor: true,
            suggestions: true,
            position: InputPosition::Bottom,
        }
    }
}

/// The agents panel (docs/specs/agents-panel.md).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentsConfig {
    /// Extra agents to recognize (a detector with a built-in name replaces it).
    #[serde(rename = "detector")]
    pub detectors: Vec<AgentDetector>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentDetector {
    pub name: String,
    /// Executable names, without `.exe`.
    pub exe: Vec<String>,
    /// Substrings of a command-line argument (agents running under node/python).
    pub cmdline: Vec<String>,
    /// Substrings of the executable path/arguments that rule a process out.
    pub exclude: Vec<String>,
    pub sessions_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    /// Initial inner width in logical pixels.
    pub width: u32,
    /// Initial inner height in logical pixels.
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 700,
        }
    }
}

impl Config {
    /// Parses a configuration from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut config: Self = toml::from_str(text)?;
        config.resolve_theme();
        Ok(config)
    }

    /// Name of the chosen theme (`None` for inline colors).
    pub fn theme_name(&self) -> Option<&str> {
        match &self.theme {
            ThemeChoice::Name(n) => Some(n),
            ThemeChoice::Colors(_) => None,
        }
    }

    /// Computes [`Self::palette`] from the theme and the overrides.
    pub fn resolve_theme(&mut self) {
        let mut palette = match &self.theme {
            ThemeChoice::Name(name) => themes::find(name).map_or_else(
                || {
                    tracing::warn!(name, "unknown theme, using {}", themes::DEFAULT_THEME);
                    ThemeConfig::default()
                },
                |t| t.colors,
            ),
            ThemeChoice::Colors(c) => {
                let mut t = ThemeConfig::default();
                c.apply(&mut t);
                t
            }
        };
        self.colors.apply(&mut palette);
        self.palette = palette;
    }

    /// Switches to `theme` (keeping the `[colors]` overrides).
    pub fn set_theme(&mut self, theme: &themes::Theme) {
        self.theme = ThemeChoice::Name(theme.name.clone());
        let mut palette = theme.colors.clone();
        self.colors.apply(&mut palette);
        self.palette = palette;
    }

    /// Loads the configuration from `path`. A missing file yields the defaults.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "no config file, using defaults");
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        Self::from_toml(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    }

    /// Loads the user configuration from the default location, falling back to
    /// the defaults (and logging the error) if it cannot be loaded.
    pub fn load_or_default() -> Self {
        let Some(path) = config_file_path() else {
            tracing::warn!("could not determine the config directory, using defaults");
            return Self::default();
        };
        match Self::load_from(&path) {
            Ok(config) => config,
            Err(e) => {
                tracing::error!("{e}; using defaults");
                Self::default()
            }
        }
    }
}

/// Directory holding the user configuration:
/// `%APPDATA%\mao-term\config` on Windows, `$XDG_CONFIG_HOME/mao-term` on Linux.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "mao-term").map(|d| d.config_dir().to_owned())
}

/// Directory for data Mao-Term manages (history, shell-integration scripts):
/// `%LOCALAPPDATA%\mao-term\data` on Windows, `$XDG_DATA_HOME/mao-term` on Linux.
pub fn data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "mao-term").map(|d| d.data_local_dir().to_owned())
}

/// Starting point written by [`ensure_config_file`]: every option, commented out.
pub const CONFIG_TEMPLATE: &str = r##"# Mao-Term configuration. Every value below is the default; uncomment to change.
# Saved changes apply the next time Mao-Term starts.

# Color theme: Mao Dark, Tokyo Night, Catppuccin Mocha, Dracula, Nord, Gruvbox Dark,
# One Dark, Rosé Pine, Solarized Dark, Solarized Light or GitHub Light.
# Your own: a .toml file in the themes/ folder next to this one (same keys as [colors]).
# Try them live: command palette (Ctrl+Shift+P) → Select Theme.
# theme = "Mao Dark"

# [font]
# family = "JetBrains Mono"   # falls back to Cascadia Mono, Consolas, DejaVu Sans Mono…
# size = 13.0                 # points

# [window]
# width = 1100
# height = 700

# [terminal]
# scrollback = 10000
# padding = 8.0

# [shell]
# program = "pwsh.exe"        # default: pwsh → powershell (Windows), $SHELL (Linux)
# args = ["-NoLogo"]
# integration = true          # prompt marks + cwd, needed by the input editor

# [input]
# editor = true               # Mao-Term's own line editor at the prompt
# suggestions = true          # history ghost text
# position = "bottom"         # "bottom" (input box under the output, like Warp) or "inline"

# Extra AI agents for the agents panel (built-in: Claude Code, opencode, Codex,
# Gemini CLI, Aider, Cursor Agent, Goose, Amp, Copilot CLI). Example:
## [[agents.detector]]
## name = "My Agent"
## exe = ["my-agent"]                 # executable name, without .exe
## cmdline = ["my-agent/cli.js"]      # or a substring of its command line
## sessions_dir = "~/.my-agent/sessions"

# Override single colors of the theme. Example:
## [colors]
## background = "#101010"
## cursor = "#ff9900"
## ansi = [16 colors: black, red, green, yellow, blue, magenta, cyan, white, then bright]
"##;

/// Returns the config file path, creating it from [`CONFIG_TEMPLATE`] if missing.
pub fn ensure_config_file() -> std::io::Result<PathBuf> {
    let path = config_file_path()
        .ok_or_else(|| std::io::Error::other("no config directory on this system"))?;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, CONFIG_TEMPLATE)?;
    }
    Ok(path)
}

/// Full path of the user config file.
pub fn config_file_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CONFIG_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_default() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let config = Config::from_toml(
            r##"
            [font]
            size = 16.0

            [theme]
            background = "#000000"
            "##,
        )
        .unwrap();
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.font.family, FontConfig::default().family);
        assert_eq!(config.palette.background, Rgb::new(0, 0, 0));
        assert_eq!(config.palette.foreground, ThemeConfig::default().foreground);
    }

    #[test]
    fn themes_by_name_with_overrides() {
        let config = Config::from_toml(
            "theme = \"nord\"
[colors]
cursor = \"#ff0000\"",
        )
        .unwrap();
        let nord = themes::find("Nord").unwrap().colors;
        assert_eq!(config.palette.background, nord.background);
        assert_eq!(config.palette.cursor, Rgb::new(255, 0, 0));
        // Unknown names fall back to the default theme.
        let config = Config::from_toml("theme = \"nope\"").unwrap();
        assert_eq!(config.palette, ThemeConfig::default());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Config::from_toml("[font]\nsise = 12.0").is_err());
    }

    #[test]
    fn template_is_the_defaults() {
        // Uncommenting everything must give exactly the defaults.
        let uncommented: String = CONFIG_TEMPLATE
            .lines()
            .map(|l| l.strip_prefix("# ").unwrap_or(""))
            .filter(|l| !l.starts_with(char::is_uppercase) && !l.starts_with("Saved"))
            .map(|l| l.split(" #").next().unwrap_or(l))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let mut expected = Config::default();
        expected.shell.program = Some("pwsh.exe".into());
        expected.shell.args = vec!["-NoLogo".into()];
        assert_eq!(Config::from_toml(&uncommented).unwrap(), expected);
    }

    #[test]
    fn missing_file_is_default() {
        let path = std::env::temp_dir().join("mao-term-definitely-missing/config.toml");
        assert_eq!(Config::load_from(&path).unwrap(), Config::default());
    }
}
