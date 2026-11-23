// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings for focus modes and do-not-disturb schedules (stage 2, docs/spec/features/focus.md):
//! schema `io.github.OlehDzhumyk.Desk.Focus` and its preference page. The modes themselves live
//! in `~/.local/share/deskd/focus.json` (deskd owns them).

use crate::model::*;

pub const FOCUS_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Focus";

pub static FEATURE_FOCUS: Feature = Feature {
    id: "focus",
    title: n_("Focus"),
    origins: &[Origin::Has(n_("Available in Windows 11 and macOS"))],
};

pub static FOCUS_ENABLED: Key = Key {
    schema: FOCUS_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Focus modes"),
    description: n_("Turn off to stop all schedules and end the active mode"),
    summary: "Focus modes and their schedules are on",
    doc: "Off: deskd ends the active mode (restoring what it changed) and arms no schedule.",
    feature: "focus",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static FOCUS_ACTIVE_MODE: Key = Key {
    schema: FOCUS_SCHEMA,
    name: "active-mode",
    kind: Kind::Str { default: "" },
    label: n_("Active mode"),
    description: "",
    summary: "Id of the active focus mode (written by deskd; empty: none)",
    doc: "A mirror of deskd's state for display and recovery: if the state file is lost while this \
          is set, deskd restores the notification banners at start-up. Start and end modes with \
          `themes focus`, the window or the quick settings tile, not by writing this key.",
    feature: "focus",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static FOCUS_SUMMARY_ON_END: Key = Key {
    schema: FOCUS_SCHEMA,
    name: "summary-on-end",
    kind: Kind::Bool { default: true },
    label: n_("Summary when a mode ends"),
    description: n_("Tell how many notifications arrived while the mode was on"),
    summary: "Show the number of missed notifications when a focus mode ends",
    doc: "",
    feature: "focus",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static FOCUS_SESSION_LENGTH: Key = Key {
    schema: FOCUS_SCHEMA,
    name: "session-length",
    kind: Kind::UInt {
        default: 25,
        min: 5,
        max: 120,
        step: 5,
    },
    label: n_("Focus session, minutes"),
    description: n_("Length of a focus session (Windows 11 style)"),
    summary: "Length of a focus session, in minutes",
    doc: "",
    feature: "focus",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static FOCUS_SESSION_BREAK: Key = Key {
    schema: FOCUS_SCHEMA,
    name: "session-break",
    kind: Kind::UInt {
        default: 5,
        min: 0,
        max: 30,
        step: 1,
    },
    label: n_("Break after a session, minutes"),
    description: n_("0: no break"),
    summary: "Break after a focus session, in minutes (0: none)",
    doc: "",
    feature: "focus",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static FOCUS: Schema = Schema {
    id: FOCUS_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/focus/",
    rust_name: "FocusSettings",
    keys: &[
        &FOCUS_ENABLED,
        &FOCUS_ACTIVE_MODE,
        &FOCUS_SUMMARY_ON_END,
        &FOCUS_SESSION_LENGTH,
        &FOCUS_SESSION_BREAK,
    ],
    children: &[],
};

pub static PAGE_FOCUS: Page = Page {
    id: "focus",
    title: n_("Focus"),
    subtitle: n_("Do not disturb, focus modes and schedules"),
    description: n_(
        "Hide notification banners on demand or on a schedule. Notifications are not lost: they \
         stay in the notification list. Critical ones (such as a low battery) always show. Ending \
         a mode restores what it changed.",
    ),
    icon: "weather-clear-night-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_FOCUS),
    groups: &[
        Group {
            title: n_("State"),
            description: "",
            items: &[
                Item::Status {
                    id: "focus",
                    label: n_("Focus"),
                },
                Item::Action {
                    id: "focus-modes",
                    label: n_("Start or edit modes…"),
                    destructive: false,
                },
                Item::Action {
                    id: "focus-stop",
                    label: n_("End focus"),
                    destructive: false,
                },
            ],
        },
        Group {
            title: n_("Options"),
            description: "",
            items: &[Item::Key(&FOCUS_ENABLED), Item::Key(&FOCUS_SUMMARY_ON_END)],
        },
        Group {
            title: n_("Focus sessions"),
            description: n_("Started from the quick settings tile with the Windows 11 style."),
            items: &[
                Item::Key(&FOCUS_SESSION_LENGTH),
                Item::Key(&FOCUS_SESSION_BREAK),
            ],
        },
    ],
};
