// SPDX-License-Identifier: GPL-3.0-or-later
//! The vocabulary of the settings registry: schemas, keys, features, pages and groups.
//!
//! Everything is `'static` data, so the registry is plain Rust statics that the generators
//! (GSettings XML, typed accessors, docs) and the window read alike. User-visible texts are
//! English gettext ids marked with [`n_`]; they are translated where they are shown.

/// Marks a string for translation without translating it (for statics). `xgettext -kn_`.
pub const fn n_(s: &'static str) -> &'static str {
    s
}

/// One value of a choice key and its label (an English gettext id or an untranslated autonym).
#[derive(Debug)]
pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

/// Type, default and limits of a key. The GSettings type follows from it.
#[derive(Debug)]
pub enum Kind {
    /// `b` → switch row
    Bool { default: bool },
    /// `u` with a range → spin row
    UInt {
        default: u32,
        min: u32,
        max: u32,
        step: u32,
    },
    /// `i` with a range → spin row (signed, e.g. a pixel offset)
    Int {
        default: i32,
        min: i32,
        max: i32,
        step: i32,
    },
    /// `d` with a range → spin row
    Double {
        default: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    /// `s`: a CSS colour (`#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`) or empty; GSettings cannot
    /// check it, so [`crate::validate`] does (the window, theme apply, the extension that uses it)
    Color { default: &'static str },
    /// `s` with `<choices>` → combo row
    Choice {
        default: &'static str,
        choices: &'static [Choice],
    },
    /// `s` → entry row
    Str { default: &'static str },
    /// `as` → list editor row
    StrList { default: &'static [&'static str] },
    /// `as` of GTK accelerators → shortcut row
    Accel { default: &'static [&'static str] },
}

impl Kind {
    pub fn gvariant_type(&self) -> &'static str {
        match self {
            Kind::Bool { .. } => "b",
            Kind::UInt { .. } => "u",
            Kind::Int { .. } => "i",
            Kind::Double { .. } => "d",
            Kind::Choice { .. } | Kind::Str { .. } | Kind::Color { .. } => "s",
            Kind::StrList { .. } | Kind::Accel { .. } => "as",
        }
    }

    /// The default value in GVariant text format (as in a `.gschema.xml` `<default>`).
    pub fn default_text(&self) -> String {
        fn s(v: &str) -> String {
            format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        fn list(v: &[&str]) -> String {
            format!(
                "[{}]",
                v.iter().map(|x| s(x)).collect::<Vec<_>>().join(", ")
            )
        }
        match self {
            Kind::Bool { default } => default.to_string(),
            Kind::UInt { default, .. } => default.to_string(),
            Kind::Int { default, .. } => default.to_string(),
            Kind::Double { default, .. } => format!("{default:?}"),
            Kind::Choice { default, .. } | Kind::Str { default } | Kind::Color { default } => {
                s(default)
            }
            Kind::StrList { default } | Kind::Accel { default } => list(default),
        }
    }
}

/// How the window shows a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widget {
    /// The row type follows from [`Kind`] and is editable.
    Edit,
    /// A read-only row with the current value (set by themes, not by the user).
    Display,
    /// Not shown in the window (still in the schema, documented in docs/settings.md).
    Hidden,
}

/// One GSettings key.
#[derive(Debug)]
pub struct Key {
    /// Schema id the key belongs to.
    pub schema: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    /// Row title (gettext id).
    pub label: &'static str,
    /// Row subtitle (gettext id, may be empty).
    pub description: &'static str,
    /// `<summary>` in the schema when it should differ from `label`.
    pub summary: &'static str,
    /// `<description>` in the schema when it should differ from `description`.
    pub doc: &'static str,
    /// Feature id ([`Feature::id`]) the key belongs to.
    pub feature: &'static str,
    pub widget: Widget,
    /// Name of a boolean key of the same schema that must be on for this row to be editable.
    pub sensitive_if: Option<&'static str>,
}

impl Key {
    pub fn schema_summary(&self) -> &'static str {
        if self.summary.is_empty() {
            self.label
        } else {
            self.summary
        }
    }

    pub fn schema_description(&self) -> &'static str {
        if self.doc.is_empty() {
            self.description
        } else {
            self.doc
        }
    }
}

/// A GSettings schema: id, path, keys and child schemas.
#[derive(Debug)]
pub struct Schema {
    pub id: &'static str,
    pub path: &'static str,
    /// Rust type name of the generated accessor (`ClipboardSettings`).
    pub rust_name: &'static str,
    pub keys: &'static [&'static Key],
    /// `(child name, schema id)`
    pub children: &'static [(&'static str, &'static str)],
}

/// A schema that belongs to a GNOME Shell extension of this repository: the generated
/// `.gschema.xml` goes into the extension's own `schemas/` directory (it is compiled there, next to
/// the extension, not into the user's schema directory). Same keys, pages and checks as [`Schema`].
#[derive(Debug)]
pub struct ExtensionSchema {
    /// Extension uuid (directory name below `extensions/`).
    pub uuid: &'static str,
    pub schema: &'static Schema,
}

impl ExtensionSchema {
    /// Path of the generated schema file, relative to the repository root.
    pub fn file(&self) -> String {
        format!(
            "extensions/{}/schemas/{}.gschema.xml",
            self.uuid, self.schema.id
        )
    }
}

/// Where a feature comes from: shown as badges on its page and in docs/settings.md.
#[derive(Debug)]
pub enum Origin {
    /// «Є в Windows 11 і macOS» (gettext id)
    Has(&'static str),
    /// «Засновано на {project} ({licence})»
    BasedOn {
        project: &'static str,
        licence: &'static str,
    },
}

/// A desktop feature or a part of the application that owns settings.
#[derive(Debug)]
pub struct Feature {
    pub id: &'static str,
    pub title: &'static str,
    pub origins: &'static [Origin],
}

/// A keyboard shortcut of a feature. Two keys carry it: `key` holds the accelerators in force
/// (what the bridge binds; themes write the theme default into it) and `override_key` the user's
/// own choice, which wins over the theme and survives theme switches (see [`crate::shortcuts`]).
#[derive(Debug)]
pub struct Shortcut {
    pub id: &'static str,
    /// Row title (gettext id).
    pub label: &'static str,
    /// Feature id ([`Feature::id`]).
    pub feature: &'static str,
    /// The key in force ([`Kind::Accel`]).
    pub key: &'static Key,
    /// The user override: empty = follow the theme, `["disabled"]` = no shortcut.
    pub override_key: &'static Key,
}

/// An entry of a preference group.
#[derive(Debug)]
pub enum Item {
    Key(&'static Key),
    /// An editable shortcut row (change, reset to the theme default, conflict check).
    Shortcut(&'static Shortcut),
    /// A button row; the window binds the handler by `id`.
    Action {
        id: &'static str,
        label: &'static str,
        destructive: bool,
    },
    /// An information row whose subtitle the window fills by `id` (e.g. the deskd status).
    Status {
        id: &'static str,
        label: &'static str,
    },
}

#[derive(Debug)]
pub struct Group {
    pub title: &'static str,
    pub description: &'static str,
    pub items: &'static [Item],
}

/// Which list of the «Features» page a page is linked from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Features,
    App,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// Groups of rows from the registry.
    Settings,
    /// The «Credits» page (rows from CREDITS.md).
    Credits,
}

/// A preference page of the window (`themes gui --page <id>`).
#[derive(Debug)]
pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
    /// Subtitle of the row that opens the page.
    pub subtitle: &'static str,
    /// Text at the top of the page.
    pub description: &'static str,
    pub icon: &'static str,
    pub section: Section,
    pub kind: PageKind,
    pub feature: Option<&'static Feature>,
    pub groups: &'static [Group],
}

impl Page {
    pub fn shortcuts(&self) -> impl Iterator<Item = &'static Shortcut> + '_ {
        self.groups
            .iter()
            .flat_map(|g| g.items.iter())
            .filter_map(|i| match i {
                Item::Shortcut(s) => Some(*s),
                _ => None,
            })
    }

    pub fn keys(&self) -> impl Iterator<Item = &'static Key> + '_ {
        self.groups
            .iter()
            .flat_map(|g| g.items.iter())
            .filter_map(|i| match i {
                Item::Key(k) => Some(*k),
                _ => None,
            })
    }
}
