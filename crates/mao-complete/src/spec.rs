//! Command specs: subcommands, options and argument kinds, loaded from TOML.

use serde::Deserialize;

/// What a positional argument expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgKind {
    /// Anything; file paths are offered.
    #[default]
    Any,
    /// Files and directories.
    Path,
    /// Directories only.
    Dir,
    /// Git branches.
    Branch,
    /// Nothing worth completing.
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    /// e.g. `["-f", "--force"]`.
    pub names: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "sub")]
    pub subcommands: Vec<Spec>,
    #[serde(default, rename = "option")]
    pub options: Vec<OptionSpec>,
    #[serde(default)]
    pub args: ArgKind,
}

impl Spec {
    pub fn matches(&self, word: &str) -> bool {
        self.name == word || self.aliases.iter().any(|a| a == word)
    }

    pub fn subcommand(&self, word: &str) -> Option<&Spec> {
        self.subcommands.iter().find(|s| s.matches(word))
    }
}

#[derive(Debug, Deserialize)]
struct SpecFile {
    #[serde(rename = "spec")]
    specs: Vec<Spec>,
}

/// Built-in specs shipped with Mao-Term.
pub fn builtin() -> Vec<Spec> {
    parse(include_str!("../specs/builtin.toml")).unwrap_or_else(|e| {
        tracing::error!("invalid built-in completion specs: {e}");
        Vec::new()
    })
}

pub fn parse(text: &str) -> Result<Vec<Spec>, toml::de::Error> {
    toml::from_str::<SpecFile>(text).map(|f| f.specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_specs_parse() {
        let specs = builtin();
        let git = specs.iter().find(|s| s.name == "git").expect("git spec");
        assert_eq!(
            git.subcommand("checkout").map(|s| s.args),
            Some(ArgKind::Branch)
        );
        assert!(specs.iter().any(|s| s.name == "cargo"));
        assert!(specs.iter().any(|s| s.matches("Set-Location")));
    }
}
