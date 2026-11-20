//! Changing single values of `config.toml` while keeping the user's comments
//! and layout (used by the settings screen and the theme picker).

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

/// A value to write.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Float(f64),
    Int(i64),
    Bool(bool),
}

/// Sets `section.key = value` (or a top-level `key` when `section` is empty)
/// in the file at `path`, creating it from the template if missing.
pub fn set(path: &Path, section: &str, key: &str, v: Value) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => crate::CONFIG_TEMPLATE.to_owned(),
        Err(e) => return Err(e),
    };
    let new = set_in(&text, section, key, v).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, new)
}

/// The same on a document's text.
pub fn set_in(
    text: &str,
    section: &str,
    key: &str,
    v: Value,
) -> Result<String, toml_edit::TomlError> {
    let mut doc: DocumentMut = text.parse()?;
    let item = match v {
        Value::Str(s) => value(s),
        Value::Float(f) => value((f * 1000.0).round() / 1000.0),
        Value::Int(i) => value(i),
        Value::Bool(b) => value(b),
    };
    if section.is_empty() {
        doc[key] = item;
    } else {
        if !doc.contains_table(section) {
            let mut t = Table::new();
            t.set_implicit(false);
            doc[section] = Item::Table(t);
        }
        doc[section][key] = item;
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_comments_and_adds_tables() {
        let text = "# my config\n[font]\n# the size\nsize = 13.0 # pt\n";
        let out = set_in(text, "font", "size", Value::Float(15.5)).unwrap();
        assert!(out.contains("# my config"));
        assert!(out.contains("# the size"));
        assert!(out.contains("size = 15.5"));
        let out = set_in(&out, "window", "opacity", Value::Float(0.7)).unwrap();
        assert!(out.contains("[window]\nopacity = 0.7"));
        let out = set_in(&out, "", "theme", Value::Str("Nord".into())).unwrap();
        // Top-level keys go before the tables.
        assert!(out.find("theme = \"Nord\"").unwrap() < out.find("[font]").unwrap());
        let config = crate::Config::from_toml(&out).unwrap();
        assert_eq!(config.font.size, 15.5);
        assert!((config.window.opacity - 0.7).abs() < 1e-6);
    }

    #[test]
    fn works_on_the_template() {
        let out = set_in(
            crate::CONFIG_TEMPLATE,
            "input",
            "editor",
            Value::Bool(false),
        )
        .unwrap();
        assert!(!crate::Config::from_toml(&out).unwrap().input.editor);
    }
}
