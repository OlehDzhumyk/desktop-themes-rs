// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the emoji and symbol picker (stage 2, docs/spec/features/emoji.md): schema
//! `io.github.OlehDzhumyk.Desk.Emoji`, its shortcut and its page, declared in the registry like
//! the others and kept in their own module so that features added in parallel do not collide.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const EMOJI_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Emoji";

pub static FEATURE_EMOJI: Feature = Feature {
    id: "emoji",
    title: n_("Emoji and symbols"),
    origins: &[Origin::Has(n_("In Windows 11 and macOS"))],
};

const fn emoji_bool(
    name: &'static str,
    default: bool,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: EMOJI_SCHEMA,
        name,
        kind: Kind::Bool { default },
        label,
        description,
        summary,
        doc: "",
        feature: "emoji",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

pub static EMOJI_ENABLED: Key = emoji_bool(
    "enabled",
    true,
    n_("Emoji and symbol picker"),
    n_("Open a keyboard-first panel to search and insert emoji, symbols and text faces"),
    "Enable the emoji and symbol picker",
);

pub static EMOJI_SHORTCUT: Key = Key {
    schema: EMOJI_SCHEMA,
    name: "emoji-shortcut",
    kind: Kind::Accel {
        default: &["<Super>period"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the emoji and symbol picker (in force)",
    doc: "The accelerators the bridge binds. Themes write their default here from desk.dconf \
          (Super+. and Super+; for Windows 11, Ctrl+Super+Space for macOS, Super+. for Fedora); \
          an override in io.github.OlehDzhumyk.Desk.Shortcuts emoji-toggle replaces it. The key \
          is not called toggle-shortcut because shell keybinding names are global.",
    feature: "emoji",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static EMOJI_SKIN_TONE: Key = Key {
    schema: EMOJI_SCHEMA,
    name: "skin-tone",
    kind: Kind::Choice {
        default: "none",
        choices: &[
            Choice {
                value: "none",
                label: n_("Default (yellow)"),
            },
            Choice {
                value: "light",
                label: n_("Light tone"),
            },
            Choice {
                value: "medium-light",
                label: n_("Medium-light tone"),
            },
            Choice {
                value: "medium",
                label: n_("Medium tone"),
            },
            Choice {
                value: "medium-dark",
                label: n_("Medium-dark tone"),
            },
            Choice {
                value: "dark",
                label: n_("Dark tone"),
            },
        ],
    },
    label: n_("Default skin tone"),
    description: n_("Applied to person and hand emoji when inserted"),
    summary: "Default Fitzpatrick skin-tone modifier for person emoji",
    doc: "",
    feature: "emoji",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static EMOJI_REMEMBER_RECENTS: Key = Key {
    doc: "Stored in ~/.local/share/deskd/emoji.json (mode 0600, last 36 picked). Turning it off \
          clears the list and deletes the file.",
    sensitive_if: Some("enabled"),
    ..emoji_bool(
        "remember-recents",
        true,
        n_("Remember recent picks"),
        n_(
            "Keep the last 36 picked emoji and symbols for quick access; turning it off clears them",
        ),
        "Keep recently picked emoji and symbols across sessions",
    )
};

pub static EMOJI_CLOSE_AFTER_INSERT: Key = Key {
    schema: EMOJI_SCHEMA,
    name: "close-after-insert",
    kind: Kind::Choice {
        default: "skin",
        choices: &[
            Choice {
                value: "skin",
                label: n_("Follow the active skin"),
            },
            Choice {
                value: "always",
                label: n_("Always close"),
            },
            Choice {
                value: "never",
                label: n_("Keep open"),
            },
        ],
    },
    label: n_("After inserting an item"),
    description: n_("Windows 11 stays open by default; macOS and Fedora close after one pick"),
    summary: "Whether the emoji picker closes after inserting an item",
    doc: "'skin': windows11 stays open while macos and default close; 'always': close in every \
          skin; 'never': keep open until dismissed.",
    feature: "emoji",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static EMOJI: Schema = Schema {
    id: EMOJI_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/emoji/",
    rust_name: "EmojiSettings",
    keys: &[
        &EMOJI_ENABLED,
        &EMOJI_SHORTCUT,
        &EMOJI_SKIN_TONE,
        &EMOJI_REMEMBER_RECENTS,
        &EMOJI_CLOSE_AFTER_INSERT,
    ],
    children: &[],
};

pub static SC_EMOJI_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "emoji-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the emoji and symbol picker"),
    description: "",
    summary: "Your shortcut for the emoji and symbol picker (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "emoji",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_EMOJI: Shortcut = Shortcut {
    id: "emoji-toggle",
    label: n_("Open the emoji and symbol picker"),
    feature: "emoji",
    key: &EMOJI_SHORTCUT,
    override_key: &SC_EMOJI_TOGGLE,
};

pub static PAGE_EMOJI: Page = Page {
    id: "emoji",
    title: n_("Emoji and symbols"),
    subtitle: n_("Emoji, symbols and text faces: Win+. in Windows 11, Ctrl+Super+Space in macOS"),
    description: n_(
        "A keyboard-first picker for emoji, Unicode symbols and text faces, searchable in \
         Ukrainian and English with recents and skin tones. The deskd service provides the \
         offline dataset; the shell bridge draws the picker in the style of the current theme.",
    ),
    icon: "face-smile-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_EMOJI),
    groups: &[
        Group {
            title: n_("Picker"),
            description: "",
            items: &[
                Item::Key(&EMOJI_ENABLED),
                Item::Key(&EMOJI_SKIN_TONE),
                Item::Key(&EMOJI_REMEMBER_RECENTS),
                Item::Key(&EMOJI_CLOSE_AFTER_INSERT),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
