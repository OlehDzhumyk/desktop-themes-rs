// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the volume mixer (stage 2, docs/spec/features/mixer.md): schema
//! `io.github.OlehDzhumyk.Desk.Mixer`, its shortcut and its page, declared in the registry like the
//! others and kept in their own module so that features added in parallel do not collide.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const MIXER_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Mixer";

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
        n_(
            "A volume you set is restored when the application plays again; turning it off forgets them",
        ),
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
    description: n_("Applications never listed in the mixer (app id or program name)"),
    summary: "Application keys (app id or program name) never listed in the mixer",
    doc: "",
    feature: "mixer",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
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
                Item::Key(&MIXER_HIDE_APPS),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&MIXER_ENTRY_POINT)],
        },
    ],
};
