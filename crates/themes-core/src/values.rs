// SPDX-License-Identifier: GPL-3.0-or-later
//! Values of registry keys that come from outside (a theme's `.dconf` files, dconf itself) are
//! checked against the registry before they are written or reported: a colour or weight that
//! would end up in a style string of the lock screen must be one the registry accepts.

use desk_settings::tr;
use desk_settings::validate::{key_by_path, validate_value};

use crate::dconf::Settings;

/// Registry keys of `settings` whose value the registry refuses: `(dconf path, what is wrong)`.
pub fn invalid_values(settings: &Settings) -> Vec<(String, String)> {
    settings
        .iter()
        .filter_map(|(path, value)| {
            let k = key_by_path(path)?;
            validate_value(k, value).err().map(|e| (path.clone(), e))
        })
        .collect()
}

/// Take the refused values out of a theme's settings (the key then keeps its current or standard
/// value) and say so, one warning each.
pub fn drop_invalid(settings: &mut Settings) -> Vec<String> {
    invalid_values(settings)
        .into_iter()
        .map(|(path, why)| {
            settings.remove(&path);
            tr!(
                "a theme sets {key} to a value that is not allowed ({why}); it is ignored",
                key = path,
                why = why
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pairs: &[(&str, &str)]) -> Settings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const TINT: &str = "/org/gnome/shell/extensions/lockscreen-layout/tint";
    const WEIGHT: &str = "/org/gnome/shell/extensions/lockscreen-layout/clock-font-weight";
    const MARGIN: &str = "/org/gnome/shell/extensions/lockscreen-layout/margin-y";

    #[test]
    fn hostile_and_out_of_range_values_are_dropped_others_stay() {
        let mut t = s(&[
            (TINT, "'red; margin: 0'"),
            (WEIGHT, "'200'"),
            (MARGIN, "99999"),
            ("/org/gnome/desktop/interface/gtk-theme", "'anything goes'"),
        ]);
        let w = drop_invalid(&mut t);
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(
            t,
            s(&[
                (WEIGHT, "'200'"),
                ("/org/gnome/desktop/interface/gtk-theme", "'anything goes'")
            ])
        );
        assert!(drop_invalid(&mut t).is_empty());
    }

    /// Every bundled theme passes: nothing of ours is dropped from a shipped configuration.
    #[test]
    fn bundled_themes_have_no_invalid_registry_values() {
        use crate::modules::Module;
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let ctx = crate::paths::Ctx::new(std::path::PathBuf::from("/nonexistent-home"), repo);
        let themes = crate::theme::all_themes(&ctx);
        assert!(themes.len() >= 3);
        for t in &themes {
            for m in Module::ALL {
                let bad = invalid_values(&t.settings(m));
                assert!(bad.is_empty(), "{} {m}: {bad:?}", t.id);
            }
        }
    }
}
