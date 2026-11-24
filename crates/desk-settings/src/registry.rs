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

pub static FEATURES: &[&Feature] = &[
    &FEATURE_CLIPBOARD,
    &crate::emoji::FEATURE_EMOJI,
    &crate::color_picker::FEATURE_COLOR_PICKER,
    &crate::ocr::FEATURE_OCR,
    &crate::capture::FEATURE_CAPTURE,
    &crate::mixer::FEATURE_MIXER,
    &crate::osd::FEATURE_OSD,
    &crate::appearance::FEATURE_APPEARANCE,
    &crate::focus::FEATURE_FOCUS,
    &crate::logo_menu::FEATURE_LOGO_MENU,
    &crate::lockscreen::FEATURE_LOCKSCREEN,
    &FEATURE_APP,
];

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
        ("emoji", crate::emoji::EMOJI_SCHEMA),
        ("color-picker", crate::color_picker::COLOR_PICKER_SCHEMA),
        ("ocr", crate::ocr::OCR_SCHEMA),
        ("capture", crate::capture::CAPTURE_SCHEMA),
        ("shortcuts", SHORTCUTS_SCHEMA),
        ("mixer", crate::mixer::MIXER_SCHEMA),
        ("osd", crate::osd::OSD_SCHEMA),
        ("appearance", crate::appearance::APPEARANCE_SCHEMA),
        ("focus", crate::focus::FOCUS_SCHEMA),
        ("logo-menu", crate::logo_menu::LOGO_MENU_SCHEMA),
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

pub static CLIP_IMAGE_BUDGET_MB: Key = Key {
    schema: CLIPBOARD_SCHEMA,
    name: "image-budget-mb",
    kind: Kind::UInt {
        default: 256,
        min: 16,
        max: 4096,
        step: 16,
    },
    label: n_("Image budget, MiB"),
    description: n_("When images take more space, the oldest are dropped"),
    summary: "Total size of the images in the history, in MiB (oldest images are dropped first)",
    doc: "Images wait in $XDG_RUNTIME_DIR (memory), so their total is capped; nothing is written when that directory is nearly full.",
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
        &CLIP_IMAGE_BUDGET_MB,
        &CLIP_PERSIST,
        &CLIP_PERSIST_IMAGES,
        &CLIP_PRIVATE_MODE,
        &CLIP_PASTE_ON_SELECT,
        &CLIP_TOGGLE_SHORTCUT,
        &CLIP_EXCLUDED_APPS,
    ],
    children: &[],
};

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
    keys: &[
        &SC_CLIPBOARD_TOGGLE,
        &crate::emoji::SC_EMOJI_TOGGLE,
        &crate::mixer::SC_MIXER_OPEN,
        &crate::color_picker::SC_COLOR_PICKER_TOGGLE,
        &crate::ocr::SC_OCR_TOGGLE,
        &crate::capture::SC_CAPTURE_REGION,
        &crate::capture::SC_CAPTURE_SCREEN,
        &crate::capture::SC_CAPTURE_TOOLBAR,
        &crate::capture::SC_CAPTURE_RECORD,
    ],
    children: &[],
};

/// Every shortcut of every feature, edited in one place (the «Keyboard shortcuts» page).
pub static SHORTCUTS: &[&Shortcut] = &[
    &SHORTCUT_CLIPBOARD,
    &crate::emoji::SHORTCUT_EMOJI,
    &crate::mixer::SHORTCUT_MIXER,
    &crate::color_picker::SHORTCUT_COLOR_PICKER,
    &crate::ocr::SHORTCUT_OCR,
    &crate::capture::SHORTCUT_CAPTURE_REGION,
    &crate::capture::SHORTCUT_CAPTURE_SCREEN,
    &crate::capture::SHORTCUT_CAPTURE_TOOLBAR,
    &crate::capture::SHORTCUT_CAPTURE_RECORD,
];

/// All schemas, parents first (the order of the generated XML).
pub static SCHEMAS: &[&Schema] = &[
    &DESK,
    &CLIPBOARD,
    &crate::emoji::EMOJI,
    &crate::color_picker::COLOR_PICKER,
    &crate::ocr::OCR,
    &crate::capture::CAPTURE,
    &SHORTCUT_KEYS,
    &crate::mixer::MIXER,
    &crate::osd::OSD,
    &crate::appearance::APPEARANCE,
    &crate::focus::FOCUS,
    &crate::logo_menu::LOGO_MENU,
];

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
                Item::Key(&CLIP_IMAGE_BUDGET_MB),
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
            Item::Shortcut(&crate::emoji::SHORTCUT_EMOJI),
            Item::Shortcut(&crate::mixer::SHORTCUT_MIXER),
            Item::Shortcut(&crate::color_picker::SHORTCUT_COLOR_PICKER),
            Item::Shortcut(&crate::ocr::SHORTCUT_OCR),
            Item::Shortcut(&crate::capture::SHORTCUT_CAPTURE_REGION),
            Item::Shortcut(&crate::capture::SHORTCUT_CAPTURE_SCREEN),
            Item::Shortcut(&crate::capture::SHORTCUT_CAPTURE_TOOLBAR),
            Item::Shortcut(&crate::capture::SHORTCUT_CAPTURE_RECORD),
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
    &crate::emoji::PAGE_EMOJI,
    &crate::color_picker::PAGE_COLOR_PICKER,
    &crate::ocr::PAGE_OCR,
    &crate::capture::PAGE_CAPTURE,
    &crate::mixer::PAGE_MIXER,
    &crate::osd::PAGE_OSD,
    &crate::appearance::PAGE_APPEARANCE,
    &crate::focus::PAGE_FOCUS,
    &crate::logo_menu::PAGE_LOGO_MENU,
    &crate::lockscreen::PAGE_LOCKSCREEN,
    &PAGE_SHORTCUTS,
    &PAGE_GENERAL,
    &PAGE_CREDITS,
];
