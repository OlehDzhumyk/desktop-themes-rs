// SPDX-License-Identifier: GPL-3.0-or-later
//! Feature shortcuts at the theme-manager level: the user override survives every apply, a
//! clashing system keybinding gives way to the feature that needs the key, and the window and the
//! CLI change or reset a shortcut here (rules: `desk_settings::shortcuts`).
//!
//! Layout: the key in force (`Shortcut::key`, what the bridge binds) is written by themes as
//! their default; the override (`Shortcut::override_key`, schema `...Desk.Shortcuts`) is never a
//! managed theme key. [`apply_overrides`] puts the override into the plan so that it wins, and
//! [`reset`] puts the theme default back.

use std::collections::BTreeMap;

use desk_settings::shortcuts::{
    self as rules, Binding, Conflict, DISABLED, Resolved, Source, find_conflicts, normalize,
};
use desk_settings::{Key, SHORTCUTS, Shortcut, dconf_path, tr};

use crate::Result;
use crate::dconf::{self, Settings};
use crate::modules::Module;
use crate::paths::Ctx;
use crate::state::State;
use crate::theme::all_themes;

/// Keybinding schemas of GNOME whose keys are accelerator lists owned by the system.
const SYSTEM_PREFIXES: &[&str] = &[
    "/org/gnome/desktop/wm/keybindings/",
    "/org/gnome/shell/keybindings/",
    "/org/gnome/settings-daemon/plugins/media-keys/",
];
/// Accelerators of extensions that act as a feature (the launcher of the look module).
const EXTENSION_SHORTCUTS: &[&str] = &["/org/gnome/shell/extensions/spotlight/toggle-shortcut"];
const CUSTOM_LIST: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

fn accels(s: &Settings, k: &Key) -> Option<Vec<String>> {
    s.get(&dconf_path(k)).map(|v| dconf::parse_strv(v))
}

fn default_of(k: &Key) -> Vec<&'static str> {
    match &k.kind {
        desk_settings::Kind::Accel { default } => default.to_vec(),
        _ => Vec::new(),
    }
}

/// The accelerators the theme sources in `targets` give a shortcut (later module wins, as for
/// every key); `None` when no source sets it.
pub fn theme_default(targets: &BTreeMap<Module, Settings>, sc: &Shortcut) -> Option<Vec<String>> {
    Module::ALL
        .iter()
        .rev()
        .find_map(|m| targets.get(m).and_then(|t| accels(t, sc.key)))
}

/// Put the user's overrides (read from `current`) into the plan's targets so that they win over
/// whatever the themes set. A key that no source sets goes into the look module.
pub fn apply_overrides(targets: &mut BTreeMap<Module, Settings>, current: &Settings) {
    for sc in SHORTCUTS {
        let user = accels(current, sc.override_key).unwrap_or_default();
        if user.is_empty() {
            continue;
        }
        let value = dconf::fmt_strv(&rules::resolve(&user, None, &[]).accels);
        let path = dconf_path(sc.key);
        let mut found = false;
        for t in targets.values_mut() {
            if let Some(v) = t.get_mut(&path) {
                *v = value.clone();
                found = true;
            }
        }
        if !found {
            targets.entry(Module::Look).or_default().insert(path, value);
        }
    }
}

/// Every accelerator in `s` with its owner: GNOME keybindings and custom keybindings (system),
/// the launcher extension and our registry shortcuts (features).
pub fn collect_bindings(s: &Settings) -> Vec<Binding> {
    let mut out = Vec::new();
    for sc in SHORTCUTS {
        for a in accels(s, sc.key).unwrap_or_default() {
            out.push(Binding {
                owner: sc.id.into(),
                accel: a,
                feature: true,
            });
        }
    }
    for (path, val) in s {
        let feature = EXTENSION_SHORTCUTS.contains(&path.as_str());
        let system = SYSTEM_PREFIXES.iter().any(|p| path.starts_with(p))
            && !path.starts_with(&format!("{CUSTOM_LIST}/"))
            && path != CUSTOM_LIST;
        if feature || system {
            for a in dconf::parse_strv(val) {
                out.push(Binding {
                    owner: path.clone(),
                    accel: a,
                    feature,
                });
            }
        } else if path.starts_with(&format!("{CUSTOM_LIST}/")) && path.ends_with("/binding") {
            out.push(Binding {
                owner: path.clone(),
                accel: val.trim().trim_matches('\'').to_string(),
                feature: false,
            });
        }
    }
    out
}

/// Readable owner for messages: a registry label or the last two path segments.
pub fn owner_name(owner: &str) -> String {
    if let Some(sc) = desk_settings::shortcut(owner) {
        return desk_settings::i18n::gettext(sc.label);
    }
    let mut parts: Vec<&str> = owner.rsplit('/').take(2).collect();
    parts.reverse();
    parts.join("/")
}

pub fn describe(c: &Conflict) -> String {
    tr!(
        "{accel} is used by {owners}",
        accel = c.accel,
        owners = c
            .owners
            .iter()
            .map(|o| owner_name(o))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Result of [`resolve_conflicts`].
#[derive(Debug, Default, PartialEq)]
pub struct Resolution {
    /// Every conflict found (also the ones left alone).
    pub conflicts: Vec<Conflict>,
    /// System keys changed to give way: dconf path → new value.
    pub stripped: Settings,
}

/// Look wins for look-owned keys (docs/spec/features/behavior.md, rule 4): when a feature and a
/// system keybinding own one accelerator, the accelerator is removed from the system key in
/// `settings`. Conflicts without exactly one kind of owner on each side are only reported.
pub fn resolve_conflicts(settings: &mut Settings) -> Resolution {
    let conflicts = find_conflicts(&collect_bindings(settings));
    let mut stripped = Settings::new();
    for c in &conflicts {
        if c.feature_owners.is_empty() {
            continue;
        }
        for owner in c.owners.iter().filter(|o| !c.feature_owners.contains(o)) {
            let Some(v) = settings.get(owner) else {
                continue; // a custom keybinding (a string), left to the user
            };
            if !v.contains(['[', '@']) {
                continue;
            }
            let list: Vec<String> = dconf::parse_strv(v)
                .into_iter()
                .filter(|a| normalize(a).as_deref() != Some(c.accel.as_str()))
                .collect();
            let new = dconf::fmt_strv(&list);
            settings.insert(owner.clone(), new.clone());
            stripped.insert(owner.clone(), new);
        }
    }
    Resolution {
        conflicts,
        stripped,
    }
}

/// The paths of every override key.
pub fn override_paths() -> Vec<String> {
    SHORTCUTS
        .iter()
        .map(|s| dconf_path(s.override_key))
        .collect()
}

/// The theme sources in force now, as the plan would see them.
fn current_targets(ctx: &Ctx) -> BTreeMap<Module, Settings> {
    let st = State::load(ctx);
    let themes = all_themes(ctx);
    Module::ALL
        .iter()
        .filter_map(|m| {
            let id = st.modules.get(m)?;
            let t = themes.iter().find(|t| &t.id == id)?;
            Some((*m, t.settings(*m)))
        })
        .collect()
}

/// A shortcut as the window and `themes shortcut list` show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: &'static str,
    pub label: &'static str,
    pub in_force: Vec<String>,
    pub resolved: Resolved,
    /// What a reset would give: the active theme's value, else the registry default.
    pub theme_default: Vec<String>,
    pub user_override: Vec<String>,
}

pub fn status(ctx: &Ctx, current: &Settings) -> Vec<Row> {
    let targets = current_targets(ctx);
    SHORTCUTS
        .iter()
        .map(|sc| {
            let user = accels(current, sc.override_key).unwrap_or_default();
            let theme = theme_default(&targets, sc);
            let resolved = rules::resolve(&user, theme.as_deref(), &default_of(sc.key));
            let theme_default =
                theme.unwrap_or_else(|| default_of(sc.key).into_iter().map(String::from).collect());
            Row {
                id: sc.id,
                label: sc.label,
                in_force: accels(current, sc.key).unwrap_or_else(|| theme_default.clone()),
                resolved,
                theme_default,
                user_override: user,
            }
        })
        .collect()
}

fn find(id: &str) -> Result<&'static Shortcut> {
    desk_settings::shortcut(id).ok_or_else(|| {
        crate::Error(tr!(
            "unknown shortcut “{id}” (known: {known})",
            id = id,
            known = SHORTCUTS
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// Conflicts that involve the shortcut `id` if it took `new` (what is in `current` otherwise).
pub fn conflicts_for(current: &Settings, id: &str, new: &[String]) -> Vec<Conflict> {
    let Some(sc) = desk_settings::shortcut(id) else {
        return Vec::new();
    };
    let mut s = current.clone();
    s.insert(dconf_path(sc.key), dconf::fmt_strv(new));
    find_conflicts(&collect_bindings(&s))
        .into_iter()
        .filter(|c| c.owners.iter().any(|o| o == id))
        .collect()
}

/// Make `accels` the user's choice for shortcut `id` (`[]` or `["disabled"]` = no shortcut).
/// A conflict refuses the change: the conflicts are returned and nothing is written.
pub fn set_override(ctx: &Ctx, id: &str, new: &[String]) -> Result<Vec<Conflict>> {
    let _ = ctx;
    let sc = find(id)?;
    for a in new {
        if a != DISABLED && normalize(a).is_none() {
            let msg = tr!("“{accel}” is not a valid shortcut", accel = a);
            crate::bail!("{msg}");
        }
    }
    let list: Vec<String> = new.iter().filter(|a| *a != DISABLED).cloned().collect();
    let conflicts = conflicts_for(&dconf::dump()?, id, &list);
    if !conflicts.is_empty() {
        return Ok(conflicts);
    }
    let stored = if list.is_empty() {
        vec![DISABLED.to_string()]
    } else {
        list.clone()
    };
    dconf::write(&dconf_path(sc.override_key), &dconf::fmt_strv(&stored))?;
    dconf::write(&dconf_path(sc.key), &dconf::fmt_strv(&list))?;
    Ok(Vec::new())
}

/// Forget the override of `id` and put the active theme's default back. Returns the conflicts the
/// default has (informational: it is applied anyway, apply resolves them the same way).
pub fn reset(ctx: &Ctx, id: &str) -> Result<Vec<Conflict>> {
    let sc = find(id)?;
    dconf::reset(&[dconf_path(sc.override_key)]);
    let cur = dconf::dump()?;
    let row = status(ctx, &cur)
        .into_iter()
        .find(|r| r.id == sc.id)
        .map(|r| r.theme_default)
        .unwrap_or_default();
    dconf::write(&dconf_path(sc.key), &dconf::fmt_strv(&row))?;
    Ok(conflicts_for(&cur, id, &row))
}

/// `themes reset`: forget every override.
pub fn clear_all() {
    dconf::reset(&override_paths());
}

/// Source label for the CLI.
pub fn source_name(s: Source) -> String {
    match s {
        Source::Override => tr!("your choice"),
        Source::Theme => tr!("theme default"),
        Source::Default => tr!("standard default"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desk_settings::{CLIP_TOGGLE_SHORTCUT, SC_CLIPBOARD_TOGGLE, SHORTCUT_CLIPBOARD};

    fn s(pairs: &[(&str, &str)]) -> Settings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn key_path() -> String {
        dconf_path(&CLIP_TOGGLE_SHORTCUT)
    }

    const INPUT: &str = "/org/gnome/desktop/wm/keybindings/switch-input-source";
    const SPOT: &str = "/org/gnome/shell/extensions/spotlight/toggle-shortcut";
    const TRAY: &str = "/org/gnome/shell/keybindings/toggle-message-tray";

    #[test]
    fn override_wins_over_every_theme_source_and_is_kept_on_switch() {
        let ov = dconf_path(&SC_CLIPBOARD_TOGGLE);
        let cur = s(&[(&ov, "['<Control><Alt>c']")]);
        // two themes set different defaults; the override beats both
        for default in ["['<Super>v']", "['<Shift><Super>v']"] {
            let mut t: BTreeMap<Module, Settings> =
                [(Module::Look, s(&[(&key_path(), default)]))].into();
            apply_overrides(&mut t, &cur);
            assert_eq!(t[&Module::Look][&key_path()], "['<Control><Alt>c']");
        }
        // a theme that does not set the key at all still gets the override
        let mut t: BTreeMap<Module, Settings> = [(Module::Behavior, Settings::new())].into();
        apply_overrides(&mut t, &cur);
        assert_eq!(t[&Module::Look][&key_path()], "['<Control><Alt>c']");
        // no override: the theme default stays
        let mut t: BTreeMap<Module, Settings> =
            [(Module::Look, s(&[(&key_path(), "['<Super>v']")]))].into();
        apply_overrides(&mut t, &Settings::new());
        assert_eq!(t[&Module::Look][&key_path()], "['<Super>v']");
    }

    #[test]
    fn disabled_override_writes_an_empty_list() {
        let ov = dconf_path(&SC_CLIPBOARD_TOGGLE);
        let mut t: BTreeMap<Module, Settings> =
            [(Module::Look, s(&[(&key_path(), "['<Super>v']")]))].into();
        apply_overrides(&mut t, &s(&[(&ov, "['disabled']")]));
        assert_eq!(t[&Module::Look][&key_path()], "@as []");
    }

    #[test]
    fn theme_default_is_the_latest_module_that_sets_it() {
        let t: BTreeMap<Module, Settings> = [
            (Module::Look, s(&[(&key_path(), "['<Super>v']")])),
            (Module::Behavior, s(&[(&key_path(), "['<Super>b']")])),
        ]
        .into();
        assert_eq!(
            theme_default(&t, &SHORTCUT_CLIPBOARD),
            Some(vec!["<Super>b".to_string()])
        );
        assert_eq!(theme_default(&BTreeMap::new(), &SHORTCUT_CLIPBOARD), None);
    }

    /// macos look (launcher on Super+Space) mixed with windows11 behavior (input switch on
    /// Super+Space): the look keeps the key, the input switch keeps its other binding.
    #[test]
    fn mixing_look_and_behavior_resolves_super_space_for_the_launcher() {
        let mut fin = s(&[
            (SPOT, "['<Super>space']"),
            (INPUT, "['<Super>space', 'XF86Keyboard']"),
        ]);
        let r = resolve_conflicts(&mut fin);
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.stripped, s(&[(INPUT, "['XF86Keyboard']")]));
        assert_eq!(fin[INPUT], "['XF86Keyboard']");
        assert_eq!(fin[SPOT], "['<Super>space']");
        // idempotent: nothing left to resolve
        assert_eq!(resolve_conflicts(&mut fin), Resolution::default());
    }

    /// windows11 look (clipboard on Super+V) with macos behavior (message tray on Super+V).
    #[test]
    fn clipboard_keeps_super_v_over_the_message_tray() {
        let mut fin = s(&[
            (&key_path(), "['<Super>v']"),
            (TRAY, "['<Super>v', '<Super>m']"),
        ]);
        let r = resolve_conflicts(&mut fin);
        assert_eq!(r.stripped, s(&[(TRAY, "['<Super>m']")]));
    }

    #[test]
    fn a_system_key_left_empty_becomes_an_explicit_empty_list() {
        let mut fin = s(&[(&key_path(), "['<Super>v']"), (TRAY, "['<Super>v']")]);
        resolve_conflicts(&mut fin);
        assert_eq!(fin[TRAY], "@as []");
    }

    #[test]
    fn two_features_are_reported_not_resolved() {
        let mut fin = s(&[
            (&key_path(), "['<Super>space']"),
            (SPOT, "['<Super>space']"),
        ]);
        let before = fin.clone();
        let r = resolve_conflicts(&mut fin);
        assert_eq!(r.conflicts.len(), 1);
        assert!(r.stripped.is_empty());
        assert_eq!(fin, before);
    }

    #[test]
    fn user_choice_that_clashes_is_found_for_the_picker() {
        let cur = s(&[(&key_path(), "['<Super>v']"), (INPUT, "['<Super>space']")]);
        let c = conflicts_for(&cur, "clipboard-toggle", &["<Super>space".to_string()]);
        assert_eq!(c.len(), 1);
        assert!(conflicts_for(&cur, "clipboard-toggle", &["<Super>F9".to_string()]).is_empty());
        // a clash between two others is not this shortcut's business
        let cur = s(&[(INPUT, "['<Super>x']"), (TRAY, "['<Super>x']")]);
        assert!(conflicts_for(&cur, "clipboard-toggle", &["<Super>F9".to_string()]).is_empty());
    }

    /// AC-BEHAV-09 for every look x behavior pair of the bundled themes: after resolution no
    /// accelerator is left with a feature owner and a system owner.
    #[test]
    fn no_mix_of_bundled_themes_leaves_a_feature_clash() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let ctx = Ctx::new(std::path::PathBuf::from("/nonexistent-home"), repo);
        let themes = all_themes(&ctx);
        assert!(themes.len() >= 3);
        let mut resolved_any = false;
        for look in &themes {
            for beh in &themes {
                let mut fin = Settings::new();
                fin.extend(look.settings(Module::Look));
                fin.extend(beh.settings(Module::Behavior));
                let r = resolve_conflicts(&mut fin);
                resolved_any |= !r.stripped.is_empty();
                let left = find_conflicts(&collect_bindings(&fin));
                assert!(
                    left.is_empty(),
                    "look {} + behavior {}: {left:?}",
                    look.id,
                    beh.id
                );
            }
        }
        assert!(
            resolved_any,
            "the known Super+Space mix should have been resolved"
        );
    }
}
