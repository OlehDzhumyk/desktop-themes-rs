// SPDX-License-Identifier: GPL-3.0-or-later
//! desk-settings — the one place where the settings of desktop-themes-rs are declared
//! ([`registry`]), the generators of everything derived from them ([`generate`]: the GSettings schema,
//! typed accessors, docs/settings.md), the generated accessors ([`accessors`], feature `gio`) and
//! the gettext helpers shared by all Rust crates ([`i18n`], [`tr!`], [`trn!`]).

pub mod generate;
pub mod i18n;
pub mod lockscreen;
pub mod model;
pub mod registry;
pub mod shortcuts;
pub mod stable_hash;
pub mod validate;

#[cfg(feature = "gio")]
#[rustfmt::skip]
pub mod accessors;

pub use lockscreen::*;
pub use model::*;
pub use registry::*;

/// Every schema of the registry: ours ([`SCHEMAS`]) and those of the repository's extensions
/// ([`EXTENSION_SCHEMAS`]).
pub fn all_schemas() -> impl Iterator<Item = &'static Schema> {
    SCHEMAS
        .iter()
        .copied()
        .chain(EXTENSION_SCHEMAS.iter().map(|e| e.schema))
}

/// The extension that owns a schema (`None` for ours): its `schemas/` directory holds the schema.
pub fn extension_of(schema: &str) -> Option<&'static ExtensionSchema> {
    EXTENSION_SCHEMAS
        .iter()
        .copied()
        .find(|e| e.schema.id == schema)
}

/// Is the schema installed? (`gio::Settings::new` aborts the process without it.)
#[cfg(feature = "gio")]
pub fn schema_installed(id: &str) -> bool {
    gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(id, true))
        .is_some()
}

/// Find a key by schema id and name.
pub fn key(schema: &str, name: &str) -> Option<&'static Key> {
    all_schemas()
        .find(|s| s.id == schema)?
        .keys
        .iter()
        .copied()
        .find(|k| k.name == name)
}

/// The dconf path of a key (`/io/github/OlehDzhumyk/Desk/clipboard/toggle-shortcut`).
pub fn dconf_path(k: &Key) -> String {
    let base = all_schemas()
        .find(|s| s.id == k.schema)
        .map_or("/", |s| s.path);
    format!("{base}{}", k.name)
}

pub fn shortcut(id: &str) -> Option<&'static Shortcut> {
    SHORTCUTS.iter().copied().find(|s| s.id == id)
}

pub fn page(id: &str) -> Option<&'static Page> {
    PAGES.iter().copied().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    /// The checked-in artefacts are what the registry generates (run `cargo xtask gen`).
    #[test]
    fn generated_files_up_to_date() {
        for (path, want) in generate::all() {
            let have = std::fs::read_to_string(repo().join(&path)).unwrap_or_default();
            assert!(have == want, "{path} is out of date: run `cargo xtask gen`");
        }
    }

    #[test]
    fn registry_is_consistent() {
        let mut seen = std::collections::HashSet::new();
        for s in all_schemas() {
            assert!(s.path.starts_with('/') && s.path.ends_with('/'), "{}", s.id);
            for k in s.keys {
                assert_eq!(k.schema, s.id, "{} lists a key of another schema", s.id);
                assert!(seen.insert((s.id, k.name)), "duplicate {}.{}", s.id, k.name);
                assert!(
                    FEATURES.iter().any(|f| f.id == k.feature),
                    "{}: feature",
                    k.name
                );
                assert!(!k.label.is_empty() && !k.schema_summary().is_empty());
                if let Some(dep) = k.sensitive_if {
                    let d = key(s.id, dep).expect("sensitive_if key");
                    assert!(matches!(d.kind, Kind::Bool { .. }));
                }
                match &k.kind {
                    Kind::UInt {
                        default,
                        min,
                        max,
                        step,
                    } => {
                        assert!(min <= default && default <= max && *step > 0, "{}", k.name)
                    }
                    Kind::Int {
                        default,
                        min,
                        max,
                        step,
                    } => {
                        assert!(min <= default && default <= max && *step > 0, "{}", k.name)
                    }
                    Kind::Double {
                        default,
                        min,
                        max,
                        step,
                    } => {
                        assert!(
                            min <= default && default <= max && *step > 0.0,
                            "{}",
                            k.name
                        )
                    }
                    Kind::Color { default } => {
                        assert!(
                            default.is_empty() || validate::is_css_color(default),
                            "{}",
                            k.name
                        )
                    }
                    Kind::Choice { default, choices } => {
                        assert!(choices.iter().any(|c| c.value == *default), "{}", k.name)
                    }
                    _ => {}
                }
            }
            for (_, child) in s.children {
                assert!(SCHEMAS.iter().any(|x| x.id == *child));
            }
        }
        // shortcuts: both keys exist, are accelerator lists, and the override starts unset
        for sc in SHORTCUTS {
            for k in [sc.key, sc.override_key] {
                assert!(key(k.schema, k.name).is_some(), "{}", sc.id);
                assert!(matches!(k.kind, Kind::Accel { .. }), "{}", sc.id);
            }
            assert!(matches!(&sc.override_key.kind, Kind::Accel { default } if default.is_empty()));
            assert!(FEATURES.iter().any(|f| f.id == sc.feature), "{}", sc.id);
            assert!(
                PAGES
                    .iter()
                    .any(|p| p.shortcuts().any(|x| std::ptr::eq(x, *sc))),
                "{} is on no page",
                sc.id
            );
        }
        // every key on a page is in its schema; editable keys are on some page
        for p in PAGES {
            for k in p.keys() {
                assert!(key(k.schema, k.name).is_some(), "{} not in schema", k.name);
            }
        }
        for s in all_schemas() {
            for k in s.keys {
                if k.widget != Widget::Hidden {
                    assert!(
                        PAGES.iter().any(|p| p.keys().any(|x| std::ptr::eq(x, *k))),
                        "{}",
                        k.name
                    );
                }
            }
        }
    }

    /// Keys, types, defaults and paths of the schema shipped before the registry (stage 1):
    /// values users already have in dconf must keep working.
    #[test]
    fn stage1_keys_unchanged() {
        let xml = generate::gschema_xml();
        for (schema, path) in [
            ("io.github.OlehDzhumyk.Desk", "/io/github/OlehDzhumyk/Desk/"),
            (
                "io.github.OlehDzhumyk.Desk.Clipboard",
                "/io/github/OlehDzhumyk/Desk/clipboard/",
            ),
        ] {
            assert!(xml.contains(&format!("<schema id=\"{schema}\" path=\"{path}\">")));
        }
        for (schema, name, ty, default) in [
            (DESK_SCHEMA, "skin", "s", "'default'"),
            (CLIPBOARD_SCHEMA, "enabled", "b", "true"),
            (CLIPBOARD_SCHEMA, "history-size", "u", "50"),
            (CLIPBOARD_SCHEMA, "images", "b", "true"),
            (CLIPBOARD_SCHEMA, "image-max-kb", "u", "8192"),
            (CLIPBOARD_SCHEMA, "persist", "b", "true"),
            (CLIPBOARD_SCHEMA, "persist-images", "b", "false"),
            (CLIPBOARD_SCHEMA, "private-mode", "b", "false"),
            (CLIPBOARD_SCHEMA, "paste-on-select", "b", "true"),
            (
                CLIPBOARD_SCHEMA,
                "toggle-shortcut",
                "as",
                "['<Shift><Super>v']",
            ),
        ] {
            let k = key(schema, name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(k.kind.gvariant_type(), ty, "{name}");
            assert_eq!(k.kind.default_text(), default, "{name}");
        }
        assert!(xml.contains("<range min=\"5\" max=\"500\"/>"));
        assert!(xml.contains("<range min=\"64\" max=\"65536\"/>"));
    }
}
