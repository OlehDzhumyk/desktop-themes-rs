// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the colour picker (stage 3, docs/spec/features/color-picker.md): schema
//! `io.github.OlehDzhumyk.Desk.ColorPicker`, its shortcut and its page, declared in the registry
//! like the others and kept in their own module so that features added in parallel do not collide.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const COLOR_PICKER_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.ColorPicker";

pub static FEATURE_COLOR_PICKER: Feature = Feature {
    id: "color-picker",
    title: n_("Colour picker"),
    origins: &[Origin::Has(n_(
        "In Windows 11 (PowerToys) and macOS (Digital Color Meter)",
    ))],
};

pub static COLOR_PICKER_ENABLED: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Colour picker"),
    description: n_("Pick the colour of any pixel on the screen with a magnifying loupe"),
    summary: "Enable the colour picker",
    doc: "",
    feature: "color-picker",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static COLOR_PICKER_FORMAT: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "format",
    kind: Kind::Choice {
        default: "hex",
        choices: &[
            Choice {
                value: "hex",
                label: n_("HEX (#RRGGBB)"),
            },
            Choice {
                value: "rgb",
                label: n_("RGB (rgb(r, g, b))"),
            },
            Choice {
                value: "hsl",
                label: n_("HSL (hsl(h, s%, l%))"),
            },
            Choice {
                value: "hsv",
                label: n_("HSV (hsv(h, s%, v%))"),
            },
            Choice {
                value: "name",
                label: n_("CSS name when exact, otherwise HEX"),
            },
            Choice {
                value: "gtk",
                label: n_("GTK @define-color line"),
            },
            Choice {
                value: "decimal",
                label: n_("Decimal (r,g,b)"),
            },
        ],
    },
    label: n_("Format"),
    description: n_("How a picked colour is copied and shown; F cycles it while picking"),
    summary: "Text format of picked colours",
    doc: "deskd formats picks and the history in this format (theme_tokens::picker).",
    feature: "color-picker",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static COLOR_PICKER_ZOOM: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "zoom",
    kind: Kind::UInt {
        default: 10,
        min: 4,
        max: 16,
        step: 1,
    },
    label: n_("Loupe magnification"),
    description: n_("The scroll wheel and the + and - keys change it while picking"),
    summary: "Magnification of the loupe (4x to 16x)",
    doc: "",
    feature: "color-picker",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static COLOR_PICKER_HISTORY_SIZE: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "history-size",
    kind: Kind::UInt {
        default: 20,
        min: 0,
        max: 50,
        step: 1,
    },
    label: n_("History size"),
    description: n_("How many picked colours to remember; 0 remembers nothing"),
    summary: "Number of picked colours kept in the history (0 = none)",
    doc: "Stored in ~/.local/share/deskd/colors.json (mode 0600), newest first. 0 keeps no \
          history: the list is cleared and the file is deleted.",
    feature: "color-picker",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static COLOR_PICKER_COPY_ON_PICK: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "copy-on-pick",
    kind: Kind::Bool { default: true },
    label: n_("Copy on pick"),
    description: n_("Put the picked colour on the clipboard in the chosen format"),
    summary: "Copy the picked colour to the clipboard",
    doc: "",
    feature: "color-picker",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static COLOR_PICKER_SHORTCUT: Key = Key {
    schema: COLOR_PICKER_SCHEMA,
    name: "color-picker-shortcut",
    kind: Kind::Accel {
        default: &["<Shift><Super>c"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that starts the colour picker (in force)",
    doc: "The accelerators the bridge binds. Themes write their default here from desk.dconf \
          (Shift+Super+C for Windows 11 and Fedora, Ctrl+Shift+Super+C for macOS); an override \
          in io.github.OlehDzhumyk.Desk.Shortcuts color-picker-toggle replaces it. The key is \
          not called toggle-shortcut because shell keybinding names are global.",
    feature: "color-picker",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static COLOR_PICKER: Schema = Schema {
    id: COLOR_PICKER_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/color-picker/",
    rust_name: "ColorPickerSettings",
    keys: &[
        &COLOR_PICKER_ENABLED,
        &COLOR_PICKER_FORMAT,
        &COLOR_PICKER_ZOOM,
        &COLOR_PICKER_HISTORY_SIZE,
        &COLOR_PICKER_COPY_ON_PICK,
        &COLOR_PICKER_SHORTCUT,
    ],
    children: &[],
};

pub static SC_COLOR_PICKER_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "color-picker-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Pick a colour from the screen"),
    description: "",
    summary: "Your shortcut for the colour picker (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "color-picker",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_COLOR_PICKER: Shortcut = Shortcut {
    id: "color-picker-toggle",
    label: n_("Pick a colour from the screen"),
    feature: "color-picker",
    key: &COLOR_PICKER_SHORTCUT,
    override_key: &SC_COLOR_PICKER_TOGGLE,
};

pub static PAGE_COLOR_PICKER: Page = Page {
    id: "color-picker",
    title: n_("Colour picker"),
    subtitle: n_("An eyedropper with a loupe: Win+Shift+C in Windows 11 (PowerToys)"),
    description: n_(
        "Pick the colour of any pixel on the screen, copy it as HEX, RGB, HSL and other formats, \
         and keep a short history of picks. The deskd service formats and stores the colours; \
         the shell bridge draws the loupe in the style of the current theme.",
    ),
    icon: "color-select-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_COLOR_PICKER),
    groups: &[
        Group {
            title: n_("Picker"),
            description: "",
            items: &[
                Item::Key(&COLOR_PICKER_ENABLED),
                Item::Key(&COLOR_PICKER_FORMAT),
                Item::Key(&COLOR_PICKER_ZOOM),
                Item::Key(&COLOR_PICKER_COPY_ON_PICK),
            ],
        },
        Group {
            title: n_("History"),
            description: n_("Picked colours are kept only on this computer."),
            items: &[Item::Key(&COLOR_PICKER_HISTORY_SIZE)],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
