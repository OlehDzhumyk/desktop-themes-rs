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
    /// `s` with `<choices>` → combo row
    Choice {
        default: &'static str,
        choices: &'static [Choice],
    },
    /// `s` → entry row
    Str { default: &'static str },
    /// `as` → not edited in the window (shown read-only when asked)
    StrList { default: &'static [&'static str] },
    /// `as` of GTK accelerators → shortcut row
    Accel { default: &'static [&'static str] },
}

impl Kind {
    pub fn gvariant_type(&self) -> &'static str {
        match self {
            Kind::Bool { .. } => "b",
            Kind::UInt { .. } => "u",
            Kind::Choice { .. } | Kind::Str { .. } => "s",
            Kind::StrList { .. } | Kind::Accel { .. } => "as",
        }
    }

    /// The default value in GVariant text format (as in a `.gschema.xml` `<default>`).
    pub fn default_text(&self) -> String {
        fn s(v: &str) -> String {
            format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        fn list(v: &[&str]) -> String {
            format!("[{}]", v.iter().map(|x| s(x)).collect::<Vec<_>>().join(", "))
        }
        match self {
            Kind::Bool { default } => default.to_string(),
            Kind::UInt { default, .. } => default.to_string(),
            Kind::Choice { default, .. } | Kind::Str { default } => s(default),
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
        if self.summary.is_empty() { self.label } else { self.summary }
    }

    pub fn schema_description(&self) -> &'static str {
        if self.doc.is_empty() { self.description } else { self.doc }
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

/// An entry of a preference group.
#[derive(Debug)]
pub enum Item {
    Key(&'static Key),
    /// A button row; the window binds the handler by `id`.
    Action {
        id: &'static str,
        label: &'static str,
        destructive: bool,
    },
    /// An information row whose subtitle the window fills by `id` (e.g. the deskd status).
    Status { id: &'static str, label: &'static str },
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
    pub fn keys(&self) -> impl Iterator<Item = &'static Key> + '_ {
        self.groups.iter().flat_map(|g| g.items.iter()).filter_map(|i| match i {
            Item::Key(k) => Some(*k),
            _ => None,
        })
    }
}
