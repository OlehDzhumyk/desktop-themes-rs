// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings of Quick Look (stage 4, `docs/spec/features/quick-look.md`): schema
//! `io.github.OlehDzhumyk.Desk.QuickLook`, its shortcut (`quick-look-shortcut`, override
//! `quick-look-toggle`) and its page.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const QUICK_LOOK_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.QuickLook";

pub static FEATURE_QUICK_LOOK: Feature = Feature {
    id: "quick-look",
    title: n_("Quick Look"),
    origins: &[Origin::Has(n_("From macOS, adapted for Windows 11 style"))],
};

pub static QUICK_LOOK_ENABLED: Key = Key {
    schema: QUICK_LOOK_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Quick Look"),
    description: n_("Preview a file at once, without opening an application"),
    summary: "Enable Quick Look",
    doc: "Off: the bridge binds no Quick Look shortcut and `themes quick-look` refuses to open.",
    feature: "quick-look",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static QUICK_LOOK_SHORTCUT: Key = Key {
    schema: QUICK_LOOK_SCHEMA,
    name: "quick-look-shortcut",
    kind: Kind::Accel {
        default: &["<Control>space"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens Quick Look for the selected files (in force)",
    doc: "The accelerators the bridge binds. Themes write their default from desk.dconf (Windows \
          11: Ctrl+Space, as PowerToys Peek; macOS: Super+Y, as Cmd+Y; Fedora: none, the Space \
          key of Files stays). Plain Space is never bound: shell keybindings are global. An \
          override in io.github.OlehDzhumyk.Desk.Shortcuts quick-look-toggle replaces it. The \
          name ends in -shortcut because shell keybinding names are global.",
    feature: "quick-look",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static QUICK_LOOK_MAX_TEXT_KB: Key = Key {
    schema: QUICK_LOOK_SCHEMA,
    name: "max-text-kb",
    kind: Kind::UInt {
        default: 1024,
        min: 64,
        max: 8192,
        step: 64,
    },
    label: n_("Largest text shown, KiB"),
    description: n_("Of a longer text or code file, only the start is read and shown"),
    summary: "How much of a text file is read for the preview, in KiB",
    doc: "The previewer reads at most this many KiB of a text or code file and shows a notice \
          that only the start of the file is shown. The rest is never read into memory.",
    feature: "quick-look",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static QUICK_LOOK_AUTOPLAY_MEDIA: Key = Key {
    schema: QUICK_LOOK_SCHEMA,
    name: "autoplay-media",
    kind: Kind::Bool { default: false },
    label: n_("Play video and audio at once"),
    description: n_("Start playback when the preview opens"),
    summary: "Start video and audio playback when the preview opens",
    doc: "Reserved for the media player of the previewer (not implemented yet).",
    feature: "quick-look",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static QUICK_LOOK_FOLLOW_SELECTION: Key = Key {
    schema: QUICK_LOOK_SCHEMA,
    name: "follow-selection",
    kind: Kind::Bool { default: true },
    label: n_("Follow the selection"),
    description: n_("Arrow keys move through the selected files"),
    summary: "Arrow keys move to the previous or next selected file",
    doc: "Off: the arrow keys do nothing and the preview stays on the first file.",
    feature: "quick-look",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SC_QUICK_LOOK_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "quick-look-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Preview the selected files (Quick Look)"),
    description: "",
    summary: "Your shortcut for Quick Look (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "quick-look",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_QUICK_LOOK: Shortcut = Shortcut {
    id: "quick-look-toggle",
    label: n_("Preview the selected files (Quick Look)"),
    feature: "quick-look",
    key: &QUICK_LOOK_SHORTCUT,
    override_key: &SC_QUICK_LOOK_TOGGLE,
};

pub static QUICK_LOOK: Schema = Schema {
    id: QUICK_LOOK_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/quick-look/",
    rust_name: "QuickLookSettings",
    keys: &[
        &QUICK_LOOK_ENABLED,
        &QUICK_LOOK_SHORTCUT,
        &QUICK_LOOK_MAX_TEXT_KB,
        &QUICK_LOOK_AUTOPLAY_MEDIA,
        &QUICK_LOOK_FOLLOW_SELECTION,
    ],
    children: &[],
};

pub static PAGE_QUICK_LOOK: Page = Page {
    id: "quick-look",
    title: n_("Quick Look"),
    subtitle: n_("Preview files without opening them: Cmd+Y in macOS, Ctrl+Space in Windows 11"),
    description: n_(
        "See the contents of a file at once in a floating window: images, text and code, folders, \
         and a card with the details of other files. Files are never run: scripts and web pages \
         are shown as text. Arrow keys move through the files, Space or Esc closes.",
    ),
    icon: "view-reveal-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_QUICK_LOOK),
    groups: &[
        Group {
            title: n_("Preview"),
            description: "",
            items: &[
                Item::Key(&QUICK_LOOK_ENABLED),
                Item::Key(&QUICK_LOOK_MAX_TEXT_KB),
                Item::Key(&QUICK_LOOK_FOLLOW_SELECTION),
                Item::Key(&QUICK_LOOK_AUTOPLAY_MEDIA),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
