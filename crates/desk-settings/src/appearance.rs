// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings for automatic light/dark appearance (stage 2, docs/spec/features/auto-dark.md) and
//! the accent from the wallpaper (docs/spec/features/theme-from-wallpaper.md): schema `io.github.OlehDzhumyk.Desk.Appearance` and its preference page.

use crate::model::*;

pub const APPEARANCE_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Appearance";

pub static FEATURE_APPEARANCE: Feature = Feature {
    id: "appearance",
    title: n_("Appearance"),
    origins: &[Origin::Has(n_("From macOS, adapted for Windows 11 style"))],
};

pub static APPEARANCE_MODE: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "mode",
    kind: Kind::Choice {
        default: "manual",
        choices: &[
            Choice {
                value: "manual",
                label: n_("Manual"),
            },
            Choice {
                value: "light",
                label: n_("Light"),
            },
            Choice {
                value: "dark",
                label: n_("Dark"),
            },
            Choice {
                value: "auto-sun",
                label: n_("Automatic (sunset to sunrise)"),
            },
            Choice {
                value: "auto-time",
                label: n_("Automatic (scheduled times)"),
            },
        ],
    },
    label: n_("Colour scheme mode"),
    description: n_("Switch between light and dark automatically by sunset or fixed times"),
    summary: "Colour scheme mode",
    doc: "How deskd manages org.gnome.desktop.interface color-scheme: 'manual' leaves it alone, \
          'light' and 'dark' keep it fixed, 'auto-sun' follows local sunrise and sunset, and \
          'auto-time' switches at dark-from and light-from.",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_DARK_FROM: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "dark-from",
    kind: Kind::Str { default: "20:00" },
    label: n_("Dark from"),
    description: n_("Time to switch to dark mode (HH:MM, 24-hour)"),
    summary: "Time to switch to dark mode (HH:MM, 24-hour)",
    doc: "",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_LIGHT_FROM: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "light-from",
    kind: Kind::Str { default: "07:00" },
    label: n_("Light from"),
    description: n_("Time to switch to light mode (HH:MM, 24-hour)"),
    summary: "Time to switch to light mode (HH:MM, 24-hour)",
    doc: "",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_LOCATION: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "location",
    kind: Kind::Str { default: "" },
    label: n_("Location"),
    description: n_("City name or coordinates (lat,lon); empty uses the system timezone"),
    summary: "City name or coordinates (lat,lon) for sunrise and sunset",
    doc: "Used locally to compute sunrise and sunset (no network requests). Accepts a city name \
          from the built-in table or decimal coordinates 'lat,lon'; empty falls back to the \
          system timezone.",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_TWILIGHT_OFFSET: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "twilight-offset",
    kind: Kind::Int {
        default: 0,
        min: -60,
        max: 60,
        step: 5,
    },
    label: n_("Twilight offset (minutes)"),
    description: n_("Shift sunrise and sunset earlier (negative) or later (positive)"),
    summary: "Shift sunrise and sunset earlier (negative) or later (positive), in minutes",
    doc: "",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_OVERRIDE_UNTIL_NEXT: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "override-until-next",
    kind: Kind::Bool { default: true },
    label: n_("Keep manual changes until next transition"),
    description: n_(
        "If you switch light or dark manually, wait until the next sunrise or sunset before \
         switching back",
    ),
    summary: "Keep manual light/dark changes until the next scheduled transition",
    doc: "",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_ACCENT_FROM_WALLPAPER: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "accent-from-wallpaper",
    kind: Kind::Bool { default: false },
    label: n_("Accent from wallpaper"),
    description: n_("The desktop accent colour follows the current wallpaper"),
    summary: "The desktop accent colour follows the current wallpaper",
    doc: "When on, deskd watches org.gnome.desktop.background picture-uri and picture-uri-dark \
          and sets org.gnome.desktop.interface accent-color to the GNOME named accent nearest the \
          wallpaper's seed colour. The image is decoded by a `themes tokens seed` child process. \
          Turning it off restores the accent that was in force before.",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static APPEARANCE_ACCENT_SOURCE: Key = Key {
    schema: APPEARANCE_SCHEMA,
    name: "accent-source",
    kind: Kind::Choice {
        default: "active",
        choices: &[
            Choice {
                value: "active",
                label: n_("Active wallpaper"),
            },
            Choice {
                value: "light",
                label: n_("Light wallpaper"),
            },
            Choice {
                value: "dark",
                label: n_("Dark wallpaper"),
            },
        ],
    },
    label: n_("Wallpaper source"),
    description: n_("Which wallpaper variant gives the accent colour"),
    summary: "Which wallpaper variant gives the accent colour",
    doc: "'active' uses picture-uri-dark when color-scheme is 'prefer-dark' and picture-uri \
          otherwise; 'light' always uses picture-uri; 'dark' always uses picture-uri-dark.",
    feature: "appearance",
    widget: Widget::Edit,
    sensitive_if: Some("accent-from-wallpaper"),
};

pub static APPEARANCE: Schema = Schema {
    id: APPEARANCE_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/appearance/",
    rust_name: "AppearanceSettings",
    keys: &[
        &APPEARANCE_MODE,
        &APPEARANCE_DARK_FROM,
        &APPEARANCE_LIGHT_FROM,
        &APPEARANCE_LOCATION,
        &APPEARANCE_TWILIGHT_OFFSET,
        &APPEARANCE_OVERRIDE_UNTIL_NEXT,
        &APPEARANCE_ACCENT_FROM_WALLPAPER,
        &APPEARANCE_ACCENT_SOURCE,
    ],
    children: &[],
};

pub static PAGE_APPEARANCE: Page = Page {
    id: "appearance",
    title: n_("Appearance"),
    subtitle: n_("Automatic light and dark mode, accent from the wallpaper"),
    description: n_(
        "Switch between light and dark appearance automatically at local sunrise and sunset or \
         at fixed times, and take the accent colour from the wallpaper. Everything is computed \
         locally with no network requests.",
    ),
    icon: "weather-clear-night-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_APPEARANCE),
    groups: &[
        Group {
            title: n_("Mode"),
            description: "",
            items: &[
                Item::Key(&APPEARANCE_MODE),
                Item::Key(&APPEARANCE_OVERRIDE_UNTIL_NEXT),
            ],
        },
        Group {
            title: n_("Solar schedule"),
            description: "",
            items: &[
                Item::Key(&APPEARANCE_LOCATION),
                Item::Key(&APPEARANCE_TWILIGHT_OFFSET),
            ],
        },
        Group {
            title: n_("Fixed schedule"),
            description: "",
            items: &[
                Item::Key(&APPEARANCE_DARK_FROM),
                Item::Key(&APPEARANCE_LIGHT_FROM),
            ],
        },
        Group {
            title: n_("Wallpaper"),
            description: "",
            items: &[
                Item::Key(&APPEARANCE_ACCENT_FROM_WALLPAPER),
                Item::Key(&APPEARANCE_ACCENT_SOURCE),
                Item::Status {
                    id: "wallpaper-accent",
                    label: n_("Accent now"),
                },
                Item::Action {
                    id: "create-theme-from-wallpaper",
                    label: n_("Create theme from wallpaper…"),
                    destructive: false,
                },
            ],
        },
    ],
};
