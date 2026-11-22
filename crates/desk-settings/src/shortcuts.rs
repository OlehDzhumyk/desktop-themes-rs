// SPDX-License-Identifier: GPL-3.0-or-later
//! Shortcut rules shared by the window, the CLI and the apply planner: precedence of the user
//! override over the theme default, and detection of one accelerator owned twice.
//!
//! Decision (docs/spec/features/behavior.md, "Settings"): per-feature shortcuts are registry
//! entries ([`crate::SHORTCUTS`]); a theme only supplies defaults, an override of the user wins
//! and is kept across theme switches; a conflict is shown and refused.

use std::collections::BTreeMap;

/// GTK convention for "no accelerator"; as the only item of an override it disables the shortcut.
pub const DISABLED: &str = "disabled";

/// Where the accelerators in force come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The user's own choice.
    Override,
    /// The active theme's default.
    Theme,
    /// The registry default (no theme sets it).
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub accels: Vec<String>,
    pub source: Source,
}

/// Override (if set) wins over the theme default, which wins over the registry default.
/// An empty override list means "not set"; `["disabled"]` means "no shortcut".
pub fn resolve(user: &[String], theme: Option<&[String]>, fallback: &[&str]) -> Resolved {
    if !user.is_empty() {
        let accels = if user.iter().all(|a| a == DISABLED) {
            Vec::new()
        } else {
            user.to_vec()
        };
        return Resolved {
            accels,
            source: Source::Override,
        };
    }
    match theme {
        Some(t) => Resolved {
            accels: t.to_vec(),
            source: Source::Theme,
        },
        None => Resolved {
            accels: fallback.iter().map(|s| s.to_string()).collect(),
            source: Source::Default,
        },
    }
}

/// Canonical form of a GTK accelerator for comparison: modifiers sorted and unified
/// (`<Ctrl>`, `<Primary>` = `<Control>`; `<Mod1>` = `<Alt>`; `<Mod4>`, `<Meta>` = `<Super>`),
/// key name lower-cased. `None` for empty, `disabled` or malformed input.
pub fn normalize(accel: &str) -> Option<String> {
    let mut rest = accel.trim();
    let mut mods: Vec<&str> = Vec::new();
    while let Some(r) = rest.strip_prefix('<') {
        let (m, after) = r.split_once('>')?;
        let m = match m.to_ascii_lowercase().as_str() {
            "shift" => "shift",
            "control" | "ctrl" | "primary" => "control",
            "alt" | "mod1" => "alt",
            "super" | "mod4" | "meta" => "super",
            "hyper" | "mod3" => "hyper",
            _ => return None,
        };
        if !mods.contains(&m) {
            mods.push(m);
        }
        rest = after;
    }
    if rest.is_empty() || rest.eq_ignore_ascii_case(DISABLED) || rest.contains(['<', '>']) {
        return None;
    }
    mods.sort_unstable();
    let mut out = String::new();
    for m in mods {
        out.push('<');
        out.push_str(m);
        out.push('>');
    }
    out.push_str(&rest.to_ascii_lowercase());
    Some(out)
}

/// One accelerator and whoever owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Stable owner id: a registry shortcut id or a dconf key / custom keybinding path.
    pub owner: String,
    pub accel: String,
    /// Owned by a feature of ours or a look-module extension (launcher), not by a system
    /// keybinding of the behavior module. On a clash the feature keeps the key.
    pub feature: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Canonical accelerator.
    pub accel: String,
    /// Distinct owners, sorted.
    pub owners: Vec<String>,
    /// The subset of `owners` that is feature-owned.
    pub feature_owners: Vec<String>,
}

/// Accelerators owned by two or more distinct owners (one owner listing a key twice is fine).
pub fn find_conflicts(bindings: &[Binding]) -> Vec<Conflict> {
    let mut by: BTreeMap<String, BTreeMap<&str, bool>> = BTreeMap::new();
    for b in bindings {
        if let Some(n) = normalize(&b.accel) {
            let e = by.entry(n).or_default().entry(&b.owner).or_insert(false);
            *e |= b.feature;
        }
    }
    by.into_iter()
        .filter(|(_, o)| o.len() > 1)
        .map(|(accel, o)| Conflict {
            accel,
            owners: o.keys().map(|s| s.to_string()).collect(),
            feature_owners: o
                .iter()
                .filter(|(_, f)| **f)
                .map(|(k, _)| k.to_string())
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    fn b(owner: &str, accel: &str, feature: bool) -> Binding {
        Binding {
            owner: owner.into(),
            accel: accel.into(),
            feature,
        }
    }

    #[test]
    fn override_beats_theme_beats_default() {
        let theme = v(&["<Super>v"]);
        let r = resolve(&v(&["<Ctrl><Alt>c"]), Some(&theme), &["<Shift><Super>v"]);
        assert_eq!(r.accels, v(&["<Ctrl><Alt>c"]));
        assert_eq!(r.source, Source::Override);
        let r = resolve(&[], Some(&theme), &["<Shift><Super>v"]);
        assert_eq!((r.accels, r.source), (theme.clone(), Source::Theme));
        let r = resolve(&[], None, &["<Shift><Super>v"]);
        assert_eq!(
            (r.accels, r.source),
            (v(&["<Shift><Super>v"]), Source::Default)
        );
    }

    /// A theme may deliberately set no shortcut (`@as []`): that is a theme default, not "unset".
    #[test]
    fn empty_theme_default_is_not_the_registry_default() {
        let r = resolve(&[], Some(&[]), &["<Super>v"]);
        assert_eq!((r.accels, r.source), (vec![], Source::Theme));
    }

    #[test]
    fn disabled_override_means_no_shortcut_and_still_wins() {
        let theme = v(&["<Super>v"]);
        let r = resolve(&v(&[DISABLED]), Some(&theme), &[]);
        assert_eq!((r.accels, r.source), (vec![], Source::Override));
    }

    #[test]
    fn normalize_unifies_spellings() {
        assert_eq!(normalize("<Shift><Super>v"), normalize("<Super><Shift>V"));
        assert_eq!(normalize("<Ctrl>a"), normalize("<Primary>a"));
        assert_eq!(normalize("<Mod4>space"), Some("<super>space".into()));
        for bad in ["", "disabled", "<Super>", "<Bogus>x", "<Super"] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }

    #[test]
    fn detects_two_owners_of_one_accelerator() {
        let c = find_conflicts(&[
            b("launcher", "<Super>space", true),
            b("input-source", "<Super>Space", false),
            b("input-source", "XF86Keyboard", false),
            b("clipboard", "<Super>v", true),
        ]);
        assert_eq!(
            c,
            vec![Conflict {
                accel: "<super>space".into(),
                owners: v(&["input-source", "launcher"]),
                feature_owners: v(&["launcher"]),
            }]
        );
    }

    #[test]
    fn one_owner_twice_is_no_conflict() {
        assert!(find_conflicts(&[b("a", "<Super>x", false), b("a", "<Super>X", false)]).is_empty());
    }
}
