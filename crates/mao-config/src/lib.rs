//! Configuration for Mao-Term.
//!
//! Every field has a default, so an empty or missing config file is valid.
//! See `docs/specs/config-theming.md`.

mod color;

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
    pub font: FontConfig,
    pub theme: ThemeConfig,
    pub window: WindowConfig,
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
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background: Rgb::new(0x14, 0x16, 0x1b),
            foreground: Rgb::new(0xd8, 0xde, 0xe9),
        }
    }
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
        toml::from_str(text)
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
        assert_eq!(config.theme.background, Rgb::new(0, 0, 0));
        assert_eq!(config.theme.foreground, ThemeConfig::default().foreground);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Config::from_toml("[font]\nsise = 12.0").is_err());
    }

    #[test]
    fn missing_file_is_default() {
        let path = std::env::temp_dir().join("mao-term-definitely-missing/config.toml");
        assert_eq!(Config::load_from(&path).unwrap(), Config::default());
    }
}
