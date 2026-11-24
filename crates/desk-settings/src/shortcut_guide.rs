// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the Shortcut Guide overlay (stage 4, docs/spec/features/shortcut-guide.md):
//! schema `io.github.OlehDzhumyk.Desk.ShortcutGuide`, its accessible shortcut and its page.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const SHORTCUT_GUIDE_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.ShortcutGuide";

pub static FEATURE_SHORTCUT_GUIDE: Feature = Feature {
    id: "shortcut-guide",
    title: n_("Shortcut guide"),
    origins: &[Origin::Has(n_(
        "From Windows (PowerToys), adapted for macOS style",
    ))],
};

pub static SHORTCUT_GUIDE_ENABLED: Key = Key {
    schema: SHORTCUT_GUIDE_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: false },
    label: n_("Shortcut guide"),
    description: n_("Hold Super to see the active keyboard shortcuts in the current theme"),
    summary: "Show the shortcut guide when Super is held",
    doc: "Off by default on the default skin; enabled by the Windows 11 and macOS themes.",
    feature: "shortcut-guide",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static SHORTCUT_GUIDE_HOLD_DELAY: Key = Key {
    schema: SHORTCUT_GUIDE_SCHEMA,
    name: "hold-delay",
    kind: Kind::UInt {
        default: 900,
        min: 400,
        max: 2000,
        step: 50,
    },
    label: n_("Hold delay (ms)"),
    description: n_("How long Super must be held alone before the guide appears"),
    summary: "Milliseconds to hold Super before showing the shortcut guide",
    doc: "Pressing any other key before this delay elapses cancels the guide so normal Super \
          shortcuts are never interrupted.",
    feature: "shortcut-guide",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SHORTCUT_GUIDE_STICKY: Key = Key {
    schema: SHORTCUT_GUIDE_SCHEMA,
    name: "sticky",
    kind: Kind::Bool { default: false },
    label: n_("Keep open after release"),
    description: n_("Stay open until Esc is pressed, with keyboard search and scrolling"),
    summary: "Keep the shortcut guide open after Super is released",
    doc: "Off: releasing Super closes the guide. On: the guide stays open in modal search mode \
          until Esc is pressed.",
    feature: "shortcut-guide",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SHORTCUT_GUIDE_SHOW_OWNER: Key = Key {
    schema: SHORTCUT_GUIDE_SCHEMA,
    name: "show-owner",
    kind: Kind::Bool { default: true },
    label: n_("Show shortcut source"),
    description: n_("Label each row as a theme default or your own setting"),
    summary: "Show whether each shortcut comes from the theme or your setting",
    doc: "",
    feature: "shortcut-guide",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SHORTCUT_GUIDE_SHORTCUT: Key = Key {
    schema: SHORTCUT_GUIDE_SCHEMA,
    name: "shortcut-guide-shortcut",
    kind: Kind::Accel {
        default: &["<Control><Shift><Super>slash"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the shortcut guide in sticky search mode (in force)",
    doc: "Accessible trigger for users who cannot hold Super; always opens in sticky searchable \
          mode. An override in io.github.OlehDzhumyk.Desk.Shortcuts shortcut-guide-toggle replaces \
          it. The key is not called toggle-shortcut because shell keybinding names are global.",
    feature: "shortcut-guide",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_GUIDE: Schema = Schema {
    id: SHORTCUT_GUIDE_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/shortcut-guide/",
    rust_name: "ShortcutGuideSettings",
    keys: &[
        &SHORTCUT_GUIDE_ENABLED,
        &SHORTCUT_GUIDE_HOLD_DELAY,
        &SHORTCUT_GUIDE_STICKY,
        &SHORTCUT_GUIDE_SHOW_OWNER,
        &SHORTCUT_GUIDE_SHORTCUT,
    ],
    children: &[],
};

pub static SC_SHORTCUT_GUIDE_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "shortcut-guide-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the shortcut guide"),
    description: "",
    summary: "Your shortcut for the shortcut guide (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "shortcut-guide",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_SHORTCUT_GUIDE: Shortcut = Shortcut {
    id: "shortcut-guide-toggle",
    label: n_("Open the shortcut guide"),
    feature: "shortcut-guide",
    key: &SHORTCUT_GUIDE_SHORTCUT,
    override_key: &SC_SHORTCUT_GUIDE_TOGGLE,
};

pub static PAGE_SHORTCUT_GUIDE: Page = Page {
    id: "shortcut-guide",
    title: n_("Shortcut guide"),
    subtitle: n_("Hold Super to view the active keyboard shortcuts of the current theme"),
    description: n_(
        "Hold the Super key to show an overlay with the keyboard shortcuts currently active in \
         your session, read live from system settings. Releasing Super closes it without opening \
         the overview, or turn on sticky mode to search and scroll with the keyboard.",
    ),
    icon: "preferences-desktop-keyboard-shortcuts-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_SHORTCUT_GUIDE),
    groups: &[
        Group {
            title: n_("Overlay"),
            description: "",
            items: &[
                Item::Key(&SHORTCUT_GUIDE_ENABLED),
                Item::Key(&SHORTCUT_GUIDE_HOLD_DELAY),
                Item::Key(&SHORTCUT_GUIDE_STICKY),
                Item::Key(&SHORTCUT_GUIDE_SHOW_OWNER),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_(
                "The accessible shortcut is set on the «Keyboard shortcuts» page; the look \
                 follows the skin of the current theme.",
            ),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
