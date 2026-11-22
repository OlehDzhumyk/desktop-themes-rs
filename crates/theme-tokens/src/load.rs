// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading `tokens.toml` along an `INHERITS` chain: each file is parsed and checked on its own
//! (so errors carry its line numbers), `[colors]` shared keys are copied into both schemes,
//! then the files are deep-merged (a child's key replaces the parent's; tables merge; arrays
//! are replaced) and the result is deserialised and validated.

use std::fmt;

use crate::schema::{SCHEMA_VERSION, Tokens, TokensFile};
use crate::validate::{self, Report};

/// One problem in a token file.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// File the problem comes from, when known.
    pub file: Option<String>,
    /// Dotted token path, e.g. `colors.light.window_fg` (empty for syntax errors).
    pub path: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}: ")?;
        }
        if !self.path.is_empty() {
            write!(f, "{}: ", self.path)?;
        }
        f.write_str(self.message.trim_end())
    }
}

/// Tokens could not be used: syntax errors, unknown keys, out-of-range values or failed
/// contrast checks.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenError {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.errors.len();
        write!(f, "{n} error(s) in tokens")?;
        for e in &self.errors {
            write!(f, "\n  {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TokenError {}

/// Successfully loaded tokens.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub tokens: Tokens,
    /// The merged table (canonical, sorted): the cache key is computed from it.
    pub merged: toml::Table,
    pub warnings: Vec<Issue>,
}

/// A file of the chain: (label for messages, contents).
pub type Layer = (String, String);

fn syntax(file: &str, e: impl fmt::Display) -> Issue {
    Issue {
        file: Some(file.to_string()),
        path: String::new(),
        message: e.to_string(),
    }
}

/// Copies keys of `[colors]` other than `light`/`dark` into both scheme tables (a key already
/// present in a scheme table wins).
fn expand_shared_colors(t: &mut toml::Table) {
    let Some(toml::Value::Table(colors)) = t.get_mut("colors") else {
        return;
    };
    let shared: Vec<(String, toml::Value)> = colors
        .iter()
        .filter(|(k, _)| *k != "light" && *k != "dark")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if shared.is_empty() {
        return;
    }
    for (k, _) in &shared {
        colors.remove(k);
    }
    for scheme in ["light", "dark"] {
        let entry = colors
            .entry(scheme.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let toml::Value::Table(st) = entry {
            for (k, v) in &shared {
                st.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }
}

/// Child over parent: tables merge recursively, anything else is replaced.
pub fn deep_merge(base: &mut toml::Table, over: &toml::Table) {
    for (k, v) in over {
        match (base.get_mut(k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => deep_merge(b, o),
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Parses and checks one file; returns its table with shared colours expanded.
pub fn parse_layer(label: &str, text: &str) -> Result<toml::Table, Issue> {
    // key names and value types, with line numbers
    let file: TokensFile = toml::from_str(text).map_err(|e| syntax(label, e))?;
    if let Some(v) = file.schema
        && v != SCHEMA_VERSION
    {
        return Err(Issue {
            file: Some(label.to_string()),
            path: "schema".into(),
            message: format!("unsupported schema {v} (this version understands {SCHEMA_VERSION})"),
        });
    }
    let mut t: toml::Table = toml::from_str(text).map_err(|e| syntax(label, e))?;
    expand_shared_colors(&mut t);
    Ok(t)
}

/// Loads a chain given child first (as `Theme::layers()` returns it).
pub fn load_chain(layers: &[Layer]) -> Result<Loaded, TokenError> {
    let mut merged = toml::Table::new();
    let mut errors = Vec::new();
    for (label, text) in layers.iter().rev() {
        match parse_layer(label, text) {
            Ok(t) => deep_merge(&mut merged, &t),
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(TokenError {
            errors,
            warnings: vec![],
        });
    }
    let label = layers.first().map(|(l, _)| l.clone());
    let tokens: Tokens =
        toml::Value::Table(merged.clone())
            .try_into()
            .map_err(|e: toml::de::Error| TokenError {
                errors: vec![Issue {
                    file: label.clone(),
                    path: String::new(),
                    message: e.to_string(),
                }],
                warnings: vec![],
            })?;
    let Report { errors, warnings } = validate::validate(&tokens);
    let tag = |mut v: Vec<Issue>| {
        for i in &mut v {
            i.file = label.clone();
        }
        v
    };
    if !errors.is_empty() {
        return Err(TokenError {
            errors: tag(errors),
            warnings: tag(warnings),
        });
    }
    Ok(Loaded {
        tokens,
        merged,
        warnings: tag(warnings),
    })
}

/// Convenience for a single file.
pub fn load_str(label: &str, text: &str) -> Result<Loaded, TokenError> {
    load_chain(&[(label.to_string(), text.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::schema::Accent;

    #[test]
    fn shared_colors_and_inheritance() {
        let parent = r##"
schema = 1
[colors]
accent = "#7a5cd0"
window_bg = "#fafafa"
[colors.dark]
window_bg = "#202020"
[radii]
window = 12
button = 6
[terminal]
builtin = "campbell"
"##;
        let child = r##"
[colors.light]
accent = "teal"
[radii]
button = 4
"##;
        let l = load_chain(&[
            ("child/tokens.toml".into(), child.into()),
            ("parent/tokens.toml".into(), parent.into()),
        ])
        .unwrap();
        let t = &l.tokens;
        assert_eq!(t.colors.light.accent, Some(Accent::Named("teal")));
        assert_eq!(
            t.colors.dark.accent,
            Some(Accent::Color(Rgba::parse("#7a5cd0").unwrap()))
        );
        assert_eq!(
            t.colors.light.window_bg,
            Some(Rgba::parse("#fafafa").unwrap())
        );
        assert_eq!(
            t.colors.dark.window_bg,
            Some(Rgba::parse("#202020").unwrap())
        );
        assert_eq!(t.radii.window, Some(12));
        assert_eq!(t.radii.button, Some(4));
        assert_eq!(
            t.terminal.as_ref().unwrap().builtin.as_deref(),
            Some("campbell")
        );
    }

    #[test]
    fn errors_carry_file_line_and_path() {
        let e = load_str("t/tokens.toml", "[radii]\nwindw = 3\n").unwrap_err();
        let s = e.to_string();
        assert!(s.contains("t/tokens.toml"), "{s}");
        assert!(s.contains("windw"), "{s}");
        assert!(s.contains("line 2"), "{s}");

        let e = load_str("t", "[colors.light]\nwindow_bg = \"red\"\n").unwrap_err();
        assert!(e.to_string().contains("not a colour"), "{e}");

        let e = load_str("t", "schema = 7\n").unwrap_err();
        assert!(e.to_string().contains("unsupported schema"), "{e}");

        let e = load_str("t", "[radii\n").unwrap_err();
        assert_eq!(e.errors.len(), 1);
    }

    #[test]
    fn merged_table_is_canonical() {
        let a = load_str(
            "a",
            "[radii]\nbutton = 3\nwindow = 9\n[fonts]\nui = \"X 11\"\n",
        )
        .unwrap();
        let b = load_str(
            "b",
            "[fonts]\nui = \"X 11\"\n[radii]\nwindow = 9\nbutton = 3\n",
        )
        .unwrap();
        assert_eq!(a.merged.to_string(), b.merged.to_string());
    }
}
