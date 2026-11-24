// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings of snap layouts and always-on-top (stage 4,
//! `docs/spec/features/window-layouts.md`): schema `io.github.OlehDzhumyk.Desk.Layouts`, its two
//! shortcuts (`layout-picker-shortcut`, `always-on-top-shortcut`) and its page.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const LAYOUTS_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Layouts";

pub static FEATURE_LAYOUTS: Feature = Feature {
    id: "layouts",
    title: n_("Window layouts"),
    origins: &[Origin::Has(n_("In Windows 11 and macOS"))],
};

pub static LAYOUTS_ENABLED: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Window layouts"),
    description: n_("Arrange windows in snap zones and pin windows above others"),
    summary: "Enable window snap layouts and always-on-top",
    doc: "Off: deskd refuses layout calls and the bridge binds no layout shortcut.",
    feature: "layouts",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static LAYOUTS_GAP_OUTER: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "gap-outer",
    kind: Kind::UInt {
        default: 8,
        min: 0,
        max: 64,
        step: 2,
    },
    label: n_("Outer gap, px"),
    description: n_("Space between snapped windows and the edges of the screen"),
    summary: "Outer gap around snapped windows, in logical pixels",
    doc: "Even pixels from 0 to 64. Windows 11 uses 8 px; Fedora uses 0 px.",
    feature: "layouts",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static LAYOUTS_GAP_INNER: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "gap-inner",
    kind: Kind::UInt {
        default: 6,
        min: 0,
        max: 64,
        step: 2,
    },
    label: n_("Inner gap, px"),
    description: n_("Space between adjacent snapped windows"),
    summary: "Inner gap between adjacent snapped windows, in logical pixels",
    doc: "Even pixels from 0 to 64. Windows 11 uses 6 px; Fedora uses 0 px.",
    feature: "layouts",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static LAYOUTS_SNAP_ASSIST: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "snap-assist",
    kind: Kind::Bool { default: true },
    label: n_("Snap assist"),
    description: n_("Suggest other open windows to fill the remaining zones after snapping"),
    summary: "Offer other windows to fill remaining snap zones",
    doc: "On by default in Windows 11; off by default in macOS.",
    feature: "layouts",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static LAYOUTS_HOVER_PICKER: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "hover-picker",
    kind: Kind::Bool { default: true },
    label: n_("Show layout picker on hover"),
    description: n_("Open the layout picker when hovering over a window's maximise button"),
    summary: "Show the layout picker when hovering the maximise button",
    doc: "On by default in Windows 11; off by default in macOS and Fedora.",
    feature: "layouts",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static LAYOUTS_PICKER_SHORTCUT: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "layout-picker-shortcut",
    kind: Kind::Accel {
        default: &["<Super>z"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the window layout picker (in force)",
    doc: "The accelerators the bridge binds. Themes write their default from desk.dconf \
          (Windows 11 and Fedora: Super+Z; macOS: Ctrl+Super+Up). An override in \
          io.github.OlehDzhumyk.Desk.Shortcuts layout-picker replaces it. The name ends in \
          -shortcut because shell keybinding names are global.",
    feature: "layouts",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static LAYOUTS_ALWAYS_ON_TOP_SHORTCUT: Key = Key {
    schema: LAYOUTS_SCHEMA,
    name: "always-on-top-shortcut",
    kind: Kind::Accel {
        default: &["<Control><Super>t"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that toggles always-on-top for the focused window (in force)",
    doc: "The accelerators the bridge binds (default: Ctrl+Super+T, as PowerToys Always on Top). \
          An override in io.github.OlehDzhumyk.Desk.Shortcuts always-on-top replaces it.",
    feature: "layouts",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_LAYOUT_PICKER: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "layout-picker",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the window layout picker"),
    description: "",
    summary: "Your shortcut for the window layout picker (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "layouts",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_ALWAYS_ON_TOP: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "always-on-top",
    kind: Kind::Accel { default: &[] },
    label: n_("Pin or unpin the focused window above others"),
    description: "",
    summary: "Your shortcut for always-on-top (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "layouts",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_LAYOUT_PICKER: Shortcut = Shortcut {
    id: "layout-picker",
    label: n_("Open the window layout picker"),
    feature: "layouts",
    key: &LAYOUTS_PICKER_SHORTCUT,
    override_key: &SC_LAYOUT_PICKER,
};

pub static SHORTCUT_ALWAYS_ON_TOP: Shortcut = Shortcut {
    id: "always-on-top",
    label: n_("Pin or unpin the focused window above others"),
    feature: "layouts",
    key: &LAYOUTS_ALWAYS_ON_TOP_SHORTCUT,
    override_key: &SC_ALWAYS_ON_TOP,
};

pub static LAYOUTS: Schema = Schema {
    id: LAYOUTS_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/layouts/",
    rust_name: "LayoutsSettings",
    keys: &[
        &LAYOUTS_ENABLED,
        &LAYOUTS_PICKER_SHORTCUT,
        &LAYOUTS_ALWAYS_ON_TOP_SHORTCUT,
        &LAYOUTS_GAP_OUTER,
        &LAYOUTS_GAP_INNER,
        &LAYOUTS_SNAP_ASSIST,
        &LAYOUTS_HOVER_PICKER,
    ],
    children: &[],
};

pub static PAGE_LAYOUTS: Page = Page {
    id: "layouts",
    title: n_("Window layouts"),
    subtitle: n_("Snap windows into zones and pin a window above others"),
    description: n_(
        "Arrange windows in ready-made zones (halves, thirds, two-thirds, quarters, focus and \
         stack) that adapt automatically to portrait monitors, with configurable outer and inner \
         gaps. Layouts and per-monitor selections stay in your local data folder.",
    ),
    icon: "view-grid-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_LAYOUTS),
    groups: &[
        Group {
            title: n_("Layouts and gaps"),
            description: "",
            items: &[
                Item::Key(&LAYOUTS_ENABLED),
                Item::Key(&LAYOUTS_GAP_OUTER),
                Item::Key(&LAYOUTS_GAP_INNER),
            ],
        },
        Group {
            title: n_("Behaviour"),
            description: "",
            items: &[
                Item::Key(&LAYOUTS_SNAP_ASSIST),
                Item::Key(&LAYOUTS_HOVER_PICKER),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcuts are set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
