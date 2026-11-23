// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the skin-aware on-screen displays (stage 3, docs/spec/features/osd.md): schema
//! `io.github.OlehDzhumyk.Desk.Osd` and its page, declared in the registry like the others and kept
//! in their own module so that features added in parallel do not collide.

use crate::model::*;

pub const OSD_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Osd";

pub static FEATURE_OSD: Feature = Feature {
    id: "osd",
    title: n_("On-screen displays"),
    origins: &[Origin::Has(n_("In Windows 11 and macOS"))],
};

pub static OSD_ENABLED: Key = Key {
    schema: OSD_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Themed on-screen displays"),
    description: n_(
        "Show volume, brightness and keyboard backlight changes in the style of the current theme",
    ),
    summary: "Draw the volume and brightness OSD in the style of the skin",
    doc: "Off, or with the default skin, the stock GNOME Shell OSD is used and the bridge does \
          not touch Main.osdWindowManager.",
    feature: "osd",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static OSD_SHOW_PERCENTAGE: Key = Key {
    schema: OSD_SCHEMA,
    name: "show-percentage",
    kind: Kind::Bool { default: true },
    label: n_("Show the percentage"),
    description: n_("The number next to the level bar; high contrast always shows it"),
    summary: "Show the level as a number",
    doc: "",
    feature: "osd",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OSD_TIMEOUT_MS: Key = Key {
    schema: OSD_SCHEMA,
    name: "timeout-ms",
    kind: Kind::UInt {
        default: 1500,
        min: 500,
        max: 4000,
        step: 100,
    },
    label: n_("Hide after (ms)"),
    description: n_("Time after the last change before the display hides"),
    summary: "Milliseconds after the last change before the OSD hides",
    doc: "A repeated key press restarts this one timer; it does not open a second OSD.",
    feature: "osd",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OSD_POSITION: Key = Key {
    schema: OSD_SCHEMA,
    name: "position",
    kind: Kind::Choice {
        default: "skin-default",
        choices: &[
            Choice {
                value: "skin-default",
                label: n_("As in the skin (Windows 11: bottom centre, macOS: centre)"),
            },
            Choice {
                value: "bottom-center",
                label: n_("Bottom centre"),
            },
            Choice {
                value: "center",
                label: n_("Centre"),
            },
            Choice {
                value: "top-right",
                label: n_("Top right"),
            },
        ],
    },
    label: n_("Position"),
    description: n_("Where on the monitor with the pointer the display appears"),
    summary: "Position of the OSD on the monitor",
    doc: "",
    feature: "osd",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OSD: Schema = Schema {
    id: OSD_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/osd/",
    rust_name: "OsdSettings",
    keys: &[
        &OSD_ENABLED,
        &OSD_SHOW_PERCENTAGE,
        &OSD_TIMEOUT_MS,
        &OSD_POSITION,
    ],
    children: &[],
};

pub static PAGE_OSD: Page = Page {
    id: "osd",
    title: n_("On-screen displays"),
    subtitle: n_("Volume and brightness pop-ups in the style of Windows 11 or macOS"),
    description: n_(
        "When a hardware key changes the volume, the brightness or the keyboard backlight, a short \
         display confirms the new level. The shell bridge draws it in the style of the current \
         theme; with the simple skin GNOME's own display stays in use.",
    ),
    icon: "display-brightness-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_OSD),
    groups: &[
        Group {
            title: n_("Display"),
            description: "",
            items: &[
                Item::Key(&OSD_ENABLED),
                Item::Key(&OSD_SHOW_PERCENTAGE),
                Item::Key(&OSD_TIMEOUT_MS),
                Item::Key(&OSD_POSITION),
            ],
        },
        Group {
            title: n_("Style"),
            description: n_("The look follows the skin of the current theme."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
