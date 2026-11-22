// SPDX-License-Identifier: GPL-3.0-or-later
//! THE settings of desktop-themes-rs. Edit here, then run `cargo xtask gen`: it rewrites
//! `data/io.github.OlehDzhumyk.Desk.gschema.xml`, `crates/desk-settings/src/accessors.rs` and
//! `docs/settings.md` (a test fails while they are out of date). The window builds its preference
//! pages from [`PAGES`] at run time.
//!
//! Keys, types, defaults and paths must stay compatible with values already stored in dconf:
//! never rename a key or change its type; add new keys instead.

use crate::model::*;

pub const DESK_SCHEMA: &str = "io.github.OlehDzhumyk.Desk";
pub const CLIPBOARD_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Clipboard";
pub const SHORTCUTS_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Shortcuts";
pub const MIXER_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Mixer";
/// gettext domain of the Rust programs, the schema, the bridge and the desktop files.
pub const GETTEXT_DOMAIN: &str = "desktop-themes-rs";

// ------------------------------------------------------------------ features

pub static FEATURE_CLIPBOARD: Feature = Feature {
    id: "clipboard",
    title: n_("Clipboard"),
    origins: &[
        Origin::Has(n_("Available in Windows 11 and macOS")),
        Origin::BasedOn {
            project: "Clipboard Indicator",
            licence: "MIT",
        },
    ],
};

pub static FEATURE_APP: Feature = Feature {
    id: "app",
    title: n_("Application"),
    origins: &[],
};

pub static FEATURES: &[&Feature] = &[&FEATURE_CLIPBOARD, &FEATURE_MIXER, &FEATURE_APP];

// ------------------------------------------------------------------ io.github.OlehDzhumyk.Desk

pub static SKIN: Key = Key {
    schema: DESK_SCHEMA,
    name: "skin",
    kind: Kind::Choice {
        default: "default",
        choices: &[
            Choice {
                value: "default",
                label: n_("simple menu (and a toggle in quick settings)"),
            },
            Choice {
                value: "macos",
                label: n_("macOS-style panel (centred, with search)"),
            },
            Choice {
                value: "windows11",
                label: n_("Windows 11-style Win+V panel (next to the cursor)"),
            },
        ],
    },
    label: n_("Panel style"),
    description: "",
    summary: "Skin of the shell-side UI",
    doc: "Which UI variant and stylesheet the bridge uses for its panels. Themes set it in their \
          look module (desk.dconf).",
    feature: "clipboard",
    widget: Widget::Display,
    sensitive_if: None,
};

pub static LANGUAGE: Key = Key {
    schema: DESK_SCHEMA,
    name: "language",
    kind: Kind::Choice {
        default: "system",
        choices: &[
            Choice {
                value: "system",
                label: n_("System default"),
            },
            // autonyms: shown untranslated
            Choice {
                value: "uk",
                label: "Українська", // i18n-ok: autonym
            },
            Choice {
                value: "en",
                label: "English",
            },
        ],
    },
    label: n_("Language"),
    description: n_("Language of the window, the command line and the shell panels"),
    summary: "Interface language",
    doc: "'system' follows the session locale (LANGUAGE, LC_MESSAGES); 'uk' or 'en' overrides it \
          for desktop-themes-rs only (the window, the themes command, deskd and desk-bridge).",
    feature: "app",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static DESK: Schema = Schema {
    id: DESK_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/",
    rust_name: "DeskSettings",
    keys: &[&SKIN, &LANGUAGE],
    children: &[
        ("clipboard", CLIPBOARD_SCHEMA),
        ("shortcuts", SHORTCUTS_SCHEMA),
        ("mixer", MIXER_SCHEMA),
    ],
};

// ------------------------------------------------------------------ io.github.OlehDzhumyk.Desk.Clipboard

const fn clip_bool(
    name: &'static str,
    default: bool,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: CLIPBOARD_SCHEMA,
        name,
        kind: Kind::Bool { default },
        label,
        description,
        summary,
        doc: "",
        feature: "clipboard",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

pub static CLIP_ENABLED: Key = clip_bool(
    "enabled",
    true,
    n_("Record history"),
    n_("Turn off to disable the feature completely"),
    "Record clipboard history",
);

pub static CLIP_HISTORY_SIZE: Key = Key {
    schema: CLIPBOARD_SCHEMA,
    name: "history-size",
    kind: Kind::UInt {
        default: 50,
        min: 5,
        max: 500,
        step: 5,
    },
    label: n_("History size"),
    description: n_("How many unpinned items to keep"),
    summary: "Maximum number of unpinned items",
    doc: "",
    feature: "clipboard",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static CLIP_IMAGES: Key = clip_bool(
    "images",
    true,
    n_("Images"),
    n_("Also record copied images"),
    "Record images",
);

pub static CLIP_IMAGE_MAX_KB: Key = Key {
    schema: CLIPBOARD_SCHEMA,
    name: "image-max-kb",
    kind: Kind::UInt {
        default: 8192,
        min: 64,
        max: 65536,
        step: 1024,
    },
    label: n_("Largest image, KiB"),
    description: n_("Bigger images are not recorded"),
    summary: "Largest image recorded, in KiB",
    doc: "",
    feature: "clipboard",
    widget: Widget::Edit,
    sensitive_if: Some("images"),
};

pub static CLIP_PERSIST: Key = Key {
    doc: "Stored in ~/.local/share/deskd/clipboard (mode 0600).",
    ..clip_bool(
        "persist",
        true,
        n_("Keep between sessions"),
        n_("Text is stored in ~/.local/share/deskd/clipboard (only you can read it)"),
        "Keep text history across sessions",
    )
};

pub static CLIP_PERSIST_IMAGES: Key = Key {
    doc: "Off by default: images live only in $XDG_RUNTIME_DIR while the session runs.",
    sensitive_if: Some("persist"),
    ..clip_bool(
        "persist-images",
        false,
        n_("Keep images between sessions"),
        n_("Otherwise images live only until you log out"),
        "Keep images across sessions",
    )
};

pub static CLIP_PRIVATE_MODE: Key = clip_bool(
    "private-mode",
    false,
    n_("Private mode"),
    n_("Temporarily record nothing"),
    "Private mode: record nothing",
);

pub static CLIP_PASTE_ON_SELECT: Key = clip_bool(
    "paste-on-select",
    true,
    n_("Paste the chosen item"),
    n_("After you choose an item it is pasted into the window you were working in"),
    "Paste the selected item into the focused window",
);

pub static CLIP_TOGGLE_SHORTCUT: Key = Key {
    schema: CLIPBOARD_SCHEMA,
    name: "toggle-shortcut",
    kind: Kind::Accel {
        default: &["<Shift><Super>v"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the clipboard panel (in force)",
    doc: "The accelerators the bridge binds. Themes write their default here from desk.dconf \
          (Super+V for Windows 11, Shift+Super+V for macOS); an override in \
          io.github.OlehDzhumyk.Desk.Shortcuts clipboard-toggle replaces it and is re-applied \
          after every theme apply. Change it in the window (Keyboard shortcuts) or with \
          `themes shortcut`.",
    feature: "clipboard",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static CLIP_EXCLUDED_APPS: Key = Key {
    schema: CLIPBOARD_SCHEMA,
    name: "excluded-apps",
    kind: Kind::StrList {
        default: &[
            "org.keepassxc.KeePassXC",
            "KeePassXC",
            "keepassxc",
            "Bitwarden",
            "bitwarden",
            "com.bitwarden.desktop",
            "1Password",
            "1password",
            "org.gnome.World.Secrets",
            "secrets",
            "org.gnome.seahorse.Application",
            "Seahorse",
            "proton-pass",
            "Proton Pass",
            "me.proton.Pass",
        ],
    },
    label: n_("Excluded applications"),
    description: "",
    summary: "Applications whose copies are never recorded (WM class or app id)",
    doc: "",
    feature: "clipboard",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static CLIPBOARD: Schema = Schema {
    id: CLIPBOARD_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/clipboard/",
    rust_name: "ClipboardSettings",
    keys: &[
        &CLIP_ENABLED,
        &CLIP_HISTORY_SIZE,
        &CLIP_IMAGES,
        &CLIP_IMAGE_MAX_KB,
        &CLIP_PERSIST,
        &CLIP_PERSIST_IMAGES,
        &CLIP_PRIVATE_MODE,
        &CLIP_PASTE_ON_SELECT,
        &CLIP_TOGGLE_SHORTCUT,
        &CLIP_EXCLUDED_APPS,
    ],
    children: &[],
};

// ------------------------------------------------------------------ io.github.OlehDzhumyk.Desk.Mixer
// Volume mixer (stage 2, docs/spec/features/mixer.md). Kept in one block, apart from other features.

pub static FEATURE_MIXER: Feature = Feature {
    id: "mixer",
    title: n_("Volume mixer"),
    origins: &[
        Origin::Has(n_("From Windows, adapted for macOS")),
        Origin::BasedOn {
            project: "wiremix",
            licence: "MIT OR Apache-2.0",
        },
    ],
};

const fn mixer_bool(
    name: &'static str,
    default: bool,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: MIXER_SCHEMA,
        name,
        kind: Kind::Bool { default },
        label,
        description,
        summary,
        doc: "",
        feature: "mixer",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

pub static MIXER_ENABLED: Key = mixer_bool(
    "enabled",
    true,
    n_("Volume mixer"),
    n_("Volume of each application and the choice of output and input devices"),
    "Enable the volume mixer",
);

pub static MIXER_ENTRY_POINT: Key = Key {
    schema: MIXER_SCHEMA,
    name: "entry-point",
    kind: Kind::Choice {
        default: "menu-and-icon",
        choices: &[
            Choice {
                value: "menu",
                label: n_("In the sound menu of quick settings"),
            },
            Choice {
                value: "menu-and-icon",
                label: n_("Also by right-clicking the volume icon in the top bar"),
            },
        ],
    },
    label: n_("Where to open it"),
    description: n_("The keyboard shortcut works in both cases"),
    summary: "Where the mixer opens",
    doc: "'menu': inside the volume menu of quick settings only; 'menu-and-icon': a secondary \
          click on the system area of the top bar opens it directly as well.",
    feature: "mixer",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static MIXER_SHOW_SILENT: Key = Key {
    sensitive_if: Some("enabled"),
    ..mixer_bool(
        "show-silent-apps",
        false,
        n_("Show silent applications"),
        n_("Also list applications that keep a sound stream open without playing"),
        "List applications with a paused audio stream",
    )
};

pub static MIXER_REMEMBER: Key = Key {
    doc: "Stored in ~/.local/share/deskd/mixer/apps.json (mode 0600): application ids with a \
          volume and a mute flag. Turning it off forgets them and deletes the file.",
    sensitive_if: Some("enabled"),
    ..mixer_bool(
        "remember-app-volume",
        false,
        n_("Remember application volume"),
        n_("A volume you set is restored when the application plays again; turning it off forgets them"),
        "Restore the volume set for an application when it plays again",
    )
};

pub static MIXER_VOLUME_STEP: Key = Key {
    schema: MIXER_SCHEMA,
    name: "volume-step",
    kind: Kind::UInt {
        default: 5,
        min: 1,
        max: 20,
        step: 1,
    },
    label: n_("Volume step, %"),
    description: n_("How much an arrow key changes a slider (Shift: 1 %)"),
    summary: "Slider step for the arrow keys, in percent",
    doc: "",
    feature: "mixer",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static MIXER_HIDE_APPS: Key = Key {
    schema: MIXER_SCHEMA,
    name: "hide-apps",
    kind: Kind::StrList { default: &[] },
    label: n_("Hidden applications"),
    description: "",
    summary: "Application keys (app id or program name) never listed in the mixer",
    doc: "",
    feature: "mixer",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static MIXER_SHORTCUT: Key = Key {
    schema: MIXER_SCHEMA,
    name: "mixer-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the volume mixer (in force)",
    doc: "The accelerators the bridge binds. Themes write their default here from desk.dconf \
          (Ctrl+Super+V for Windows 11; none for macOS and Fedora, which have no such key); an \
          override in io.github.OlehDzhumyk.Desk.Shortcuts mixer-open replaces it. The key is \
          not called toggle-shortcut because shell keybinding names are global.",
    feature: "mixer",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static MIXER: Schema = Schema {
    id: MIXER_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/mixer/",
    rust_name: "MixerSettings",
    keys: &[
        &MIXER_ENABLED,
        &MIXER_ENTRY_POINT,
        &MIXER_SHOW_SILENT,
        &MIXER_REMEMBER,
        &MIXER_VOLUME_STEP,
        &MIXER_HIDE_APPS,
        &MIXER_SHORTCUT,
    ],
    children: &[],
};

pub static SC_MIXER_OPEN: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "mixer-open",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the volume mixer"),
    description: "",
    summary: "Your shortcut for the volume mixer (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "mixer",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_MIXER: Shortcut = Shortcut {
    id: "mixer-open",
    label: n_("Open the volume mixer"),
    feature: "mixer",
    key: &MIXER_SHORTCUT,
    override_key: &SC_MIXER_OPEN,
};

pub static PAGE_MIXER: Page = Page {
    id: "mixer",
    title: n_("Volume mixer"),
    subtitle: n_("Volume of each application, output and input devices"),
    description: n_(
        "The volume of every application that plays sound and the output and input devices, in \
         the sound menu of quick settings. The deskd service follows the audio server; the shell \
         bridge draws the mixer in the style of the current theme.",
    ),
    icon: "audio-volume-high-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_MIXER),
    groups: &[
        Group {
            title: n_("Mixer"),
            description: "",
            items: &[
                Item::Key(&MIXER_ENABLED),
                Item::Key(&MIXER_SHOW_SILENT),
                Item::Key(&MIXER_REMEMBER),
                Item::Key(&MIXER_VOLUME_STEP),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&MIXER_ENTRY_POINT)],
        },
    ],
};

// ------------------------------------------------------------------ end of the mixer block

// ------------------------------------------------------------------ io.github.OlehDzhumyk.Desk.Shortcuts

pub static SC_CLIPBOARD_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "clipboard-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the clipboard panel"),
    description: "",
    summary: "Your shortcut for the clipboard panel (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "clipboard",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_CLIPBOARD: Shortcut = Shortcut {
    id: "clipboard-toggle",
    label: n_("Open the clipboard panel"),
    feature: "clipboard",
    key: &CLIP_TOGGLE_SHORTCUT,
    override_key: &SC_CLIPBOARD_TOGGLE,
};

pub static SHORTCUT_KEYS: Schema = Schema {
    id: SHORTCUTS_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/shortcuts/",
    rust_name: "ShortcutSettings",
    keys: &[&SC_CLIPBOARD_TOGGLE, &SC_MIXER_OPEN],
    children: &[],
};

/// Every shortcut of every feature, edited in one place (the «Keyboard shortcuts» page).
pub static SHORTCUTS: &[&Shortcut] = &[&SHORTCUT_CLIPBOARD, &SHORTCUT_MIXER];

/// All schemas, parents first (the order of the generated XML).
pub static SCHEMAS: &[&Schema] = &[&DESK, &CLIPBOARD, &SHORTCUT_KEYS, &MIXER];

// ------------------------------------------------------------------ pages of the window

/// Text at the top of the «Features» page.
pub const ROOT_DESCRIPTION: &str = n_(
    "Features that Windows 11 and macOS have, for GNOME. The deskd service runs them, and their \
     look follows the current theme.",
);

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Features => n_("Features"),
            Section::App => n_("Application"),
            Section::About => n_("About the project"),
        }
    }
}

pub static PAGE_CLIPBOARD: Page = Page {
    id: "clipboard",
    title: n_("Clipboard"),
    subtitle: n_("History of copied items: Win+V in Windows 11, Shift+Super+V in macOS"),
    description: n_(
        "History of copied items: texts and images, pinning, search. Kept by the deskd service; \
         the shell bridge draws the panel in the style of the current theme.",
    ),
    icon: "edit-paste-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_CLIPBOARD),
    groups: &[
        Group {
            title: n_("History"),
            description: "",
            items: &[
                Item::Key(&CLIP_ENABLED),
                Item::Key(&CLIP_HISTORY_SIZE),
                Item::Key(&CLIP_IMAGES),
                Item::Key(&CLIP_IMAGE_MAX_KB),
                Item::Key(&CLIP_PASTE_ON_SELECT),
            ],
        },
        Group {
            title: n_("Privacy"),
            description: n_(
                "Passwords from password managers (the x-kde-passwordManagerHint mark, KeePassXC, \
                 Bitwarden, 1Password, Secrets and others) are never recorded.",
            ),
            items: &[
                Item::Key(&CLIP_PRIVATE_MODE),
                Item::Key(&CLIP_PERSIST),
                Item::Key(&CLIP_PERSIST_IMAGES),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[
                Item::Key(&SKIN),
                Item::Status {
                    id: "deskd",
                    label: n_("deskd service"),
                },
            ],
        },
        Group {
            title: "",
            description: "",
            items: &[Item::Action {
                id: "clear-history",
                label: n_("Clear history"),
                destructive: true,
            }],
        },
    ],
};

pub static PAGE_GENERAL: Page = Page {
    id: "general",
    title: n_("General"),
    subtitle: n_("Language of the application"),
    description: n_("Settings of the Themes application itself."),
    icon: "preferences-desktop-locale-symbolic",
    section: Section::App,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_APP),
    groups: &[Group {
        title: n_("Language"),
        description: n_("The change takes effect after the window is restarted."),
        items: &[Item::Key(&LANGUAGE)],
    }],
};

pub static PAGE_SHORTCUTS: Page = Page {
    id: "shortcuts",
    title: n_("Keyboard shortcuts"),
    subtitle: n_("All shortcuts of the features in one place"),
    description: n_(
        "The theme provides the default shortcut; your own choice wins over it and stays when you \
         switch themes. A shortcut that another action already uses is refused.",
    ),
    icon: "input-keyboard-symbolic",
    section: Section::App,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_APP),
    groups: &[Group {
        title: n_("Features"),
        description: "",
        items: &[
            Item::Shortcut(&SHORTCUT_CLIPBOARD),
            Item::Shortcut(&SHORTCUT_MIXER),
        ],
    }],
};

pub static PAGE_CREDITS: Page = Page {
    id: "credits",
    title: n_("Credits"),
    subtitle: n_("Projects the themes and features are based on, and their licences"),
    description: n_(
        "desktop-themes-rs stands on the shoulders of other projects. The list comes from \
         CREDITS.md; the licences are those the authors stated (check the LICENSE file in their \
         repositories).",
    ),
    icon: "emblem-favorite-symbolic",
    section: Section::About,
    kind: PageKind::Credits,
    feature: None,
    groups: &[],
};

/// The pages, in the order they are listed.
pub static PAGES: &[&Page] = &[
    &PAGE_CLIPBOARD,
    &PAGE_MIXER,
    &PAGE_SHORTCUTS,
    &PAGE_GENERAL,
    &PAGE_CREDITS,
];
