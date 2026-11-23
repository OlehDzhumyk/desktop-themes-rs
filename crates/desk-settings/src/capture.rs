// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of screenshots, annotation and screen recording (stage 3,
//! docs/spec/features/screenshots.md): schema `io.github.OlehDzhumyk.Desk.Capture`, its four
//! shortcuts and its page, in their own module like the other stage 3 features.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const CAPTURE_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Capture";

pub static FEATURE_CAPTURE: Feature = Feature {
    id: "capture",
    title: n_("Screenshots"),
    origins: &[Origin::Has(n_("In Windows 11 and macOS"))],
};

pub static CAPTURE_ENABLED: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Screenshots"),
    description: n_("Capture a region or the whole screen with the shortcuts of the theme"),
    summary: "Enable screenshots and screen recording",
    doc: "Off: the bridge binds no capture shortcut and deskd refuses to save captures.",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static CAPTURE_SAVE_FOLDER: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "save-folder",
    kind: Kind::Str { default: "" },
    label: n_("Save folder"),
    description: n_("Empty: Screenshots in the Pictures folder. ~ is your home folder."),
    summary: "Folder for saved screenshots (empty = ~/Pictures/Screenshots)",
    doc: "Empty: Screenshots in the XDG pictures folder (~/Pictures/Screenshots). A leading ~ is \
          the home folder; another relative path is taken from the home folder. The folder is \
          created when needed.",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_COPY_TO_CLIPBOARD: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "copy-to-clipboard",
    kind: Kind::Bool { default: true },
    label: n_("Copy to the clipboard"),
    description: n_("Put each screenshot on the clipboard as well as in the folder"),
    summary: "Copy each capture to the clipboard",
    doc: "",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_SHOW_PREVIEW: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "show-preview",
    kind: Kind::Bool { default: true },
    label: n_("Show a preview"),
    description: n_(
        "A thumbnail (macOS style) or a notification (Windows 11 style) after a capture",
    ),
    summary: "Show the thumbnail or notification after a capture",
    doc: "The macos skin shows a floating thumbnail for 6 s, the windows11 skin a notification \
          with an Edit button; the default skin keeps GNOME's notification.",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_DELAY: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "delay",
    kind: Kind::Choice {
        default: "0",
        choices: &[
            Choice {
                value: "0",
                label: n_("No delay"),
            },
            Choice {
                value: "3",
                label: n_("3 seconds"),
            },
            Choice {
                value: "10",
                label: n_("10 seconds"),
            },
        ],
    },
    label: n_("Delay"),
    description: n_("Wait with a visible countdown before the capture"),
    summary: "Delay before a capture, in seconds",
    doc: "",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_INCLUDE_POINTER: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "include-pointer",
    kind: Kind::Bool { default: false },
    label: n_("Show the mouse pointer"),
    description: n_("Include the pointer in screenshots and recordings"),
    summary: "Include the mouse pointer",
    doc: "",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_RECORD_FORMAT: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "record-format",
    kind: Kind::Choice {
        default: "webm",
        choices: &[
            Choice {
                value: "webm",
                label: n_("WebM (VP8)"),
            },
            Choice {
                value: "mp4",
                label: n_("MP4 (H.264, when the codec is installed)"),
            },
        ],
    },
    label: n_("Video format"),
    description: n_("Screen recordings are saved in ~/Videos/Screencasts"),
    summary: "Container and codec of screen recordings",
    doc: "Encoded by GNOME's screencast service. MP4 needs an H.264 encoder, which Fedora does not \
          ship by default; WebM always works.",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_RECORD_AUDIO: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "record-audio",
    kind: Kind::Choice {
        default: "none",
        choices: &[
            Choice {
                value: "none",
                label: n_("No sound"),
            },
            Choice {
                value: "system",
                label: n_("System sound"),
            },
            Choice {
                value: "mic",
                label: n_("Microphone"),
            },
            Choice {
                value: "both",
                label: n_("System sound and microphone"),
            },
        ],
    },
    label: n_("Sound in recordings"),
    description: n_("Off by default; the microphone is recorded only when you choose it here"),
    summary: "Audio sources of screen recordings",
    doc: "",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_ANNOTATE_AFTER: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "annotate-after-capture",
    kind: Kind::Bool { default: false },
    label: n_("Open the editor after a capture"),
    description: n_("Mark up each screenshot (pen, arrows, redaction, crop) before you keep it"),
    summary: "Open the annotation editor after each capture",
    doc: "",
    feature: "capture",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static CAPTURE_REGION_SHORTCUT: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "capture-region-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that captures a region (in force)",
    doc: "The accelerators the bridge binds. Themes write their default from desk.dconf (Windows 11: \
          Shift+Super+S; macOS: Shift+Super+4; Fedora: none, GNOME's Print stays). An override in \
          io.github.OlehDzhumyk.Desk.Shortcuts capture-region replaces it. The name ends in \
          -shortcut because shell keybinding names are global.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static CAPTURE_SCREEN_SHORTCUT: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "capture-screen-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that captures the whole screen (in force)",
    doc: "Windows 11: Super+Print; macOS: Shift+Super+3; Fedora: none, GNOME's Shift+Print stays. \
          Override: io.github.OlehDzhumyk.Desk.Shortcuts capture-screen.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static CAPTURE_TOOLBAR_SHORTCUT: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "capture-toolbar-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the capture toolbar (in force)",
    doc: "Windows 11: Print; macOS: Shift+Super+5; Fedora: none, GNOME's screenshot UI stays. \
          Override: io.github.OlehDzhumyk.Desk.Shortcuts capture-toolbar.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static CAPTURE_RECORD_SHORTCUT: Key = Key {
    schema: CAPTURE_SCHEMA,
    name: "capture-record-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that starts or stops a screen recording (in force)",
    doc: "Windows 11: Shift+Super+R; macOS: none (record from the Shift+Super+5 toolbar); Fedora: \
          none, GNOME's Ctrl+Shift+Alt+R stays. Override: io.github.OlehDzhumyk.Desk.Shortcuts \
          capture-record.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_CAPTURE_REGION: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "capture-region",
    kind: Kind::Accel { default: &[] },
    label: n_("Capture a region of the screen"),
    description: "",
    summary: "Your shortcut for a region capture (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_CAPTURE_SCREEN: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "capture-screen",
    kind: Kind::Accel { default: &[] },
    label: n_("Capture the whole screen"),
    description: "",
    summary: "Your shortcut for a full-screen capture (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_CAPTURE_TOOLBAR: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "capture-toolbar",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the screenshot toolbar"),
    description: "",
    summary: "Your shortcut for the capture toolbar (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_CAPTURE_RECORD: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "capture-record",
    kind: Kind::Accel { default: &[] },
    label: n_("Start or stop a screen recording"),
    description: "",
    summary: "Your shortcut for screen recording (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "capture",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_CAPTURE_REGION: Shortcut = Shortcut {
    id: "capture-region",
    label: n_("Capture a region of the screen"),
    feature: "capture",
    key: &CAPTURE_REGION_SHORTCUT,
    override_key: &SC_CAPTURE_REGION,
};

pub static SHORTCUT_CAPTURE_SCREEN: Shortcut = Shortcut {
    id: "capture-screen",
    label: n_("Capture the whole screen"),
    feature: "capture",
    key: &CAPTURE_SCREEN_SHORTCUT,
    override_key: &SC_CAPTURE_SCREEN,
};

pub static SHORTCUT_CAPTURE_TOOLBAR: Shortcut = Shortcut {
    id: "capture-toolbar",
    label: n_("Open the screenshot toolbar"),
    feature: "capture",
    key: &CAPTURE_TOOLBAR_SHORTCUT,
    override_key: &SC_CAPTURE_TOOLBAR,
};

pub static SHORTCUT_CAPTURE_RECORD: Shortcut = Shortcut {
    id: "capture-record",
    label: n_("Start or stop a screen recording"),
    feature: "capture",
    key: &CAPTURE_RECORD_SHORTCUT,
    override_key: &SC_CAPTURE_RECORD,
};

pub static CAPTURE: Schema = Schema {
    id: CAPTURE_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/capture/",
    rust_name: "CaptureSettings",
    keys: &[
        &CAPTURE_ENABLED,
        &CAPTURE_SAVE_FOLDER,
        &CAPTURE_COPY_TO_CLIPBOARD,
        &CAPTURE_SHOW_PREVIEW,
        &CAPTURE_DELAY,
        &CAPTURE_INCLUDE_POINTER,
        &CAPTURE_RECORD_FORMAT,
        &CAPTURE_RECORD_AUDIO,
        &CAPTURE_ANNOTATE_AFTER,
        &CAPTURE_REGION_SHORTCUT,
        &CAPTURE_SCREEN_SHORTCUT,
        &CAPTURE_TOOLBAR_SHORTCUT,
        &CAPTURE_RECORD_SHORTCUT,
    ],
    children: &[],
};

pub static PAGE_CAPTURE: Page = Page {
    id: "capture",
    title: n_("Screenshots"),
    subtitle: n_(
        "Capture, mark up and record the screen: Win+Shift+S in Windows 11, Shift+Cmd+4 in macOS",
    ),
    description: n_(
        "Capture a region or the whole screen, mark the picture up with a pen, arrows, shapes and \
         redaction, and record the screen to video. Redaction is real: the covered pixels are \
         replaced in the saved file. Files stay in your folders; nothing is uploaded.",
    ),
    icon: "applets-screenshooter-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_CAPTURE),
    groups: &[
        Group {
            title: n_("Screenshots"),
            description: "",
            items: &[
                Item::Key(&CAPTURE_ENABLED),
                Item::Key(&CAPTURE_SAVE_FOLDER),
                Item::Key(&CAPTURE_COPY_TO_CLIPBOARD),
                Item::Key(&CAPTURE_SHOW_PREVIEW),
                Item::Key(&CAPTURE_DELAY),
                Item::Key(&CAPTURE_INCLUDE_POINTER),
                Item::Key(&CAPTURE_ANNOTATE_AFTER),
            ],
        },
        Group {
            title: n_("Screen recording"),
            description: "",
            items: &[
                Item::Key(&CAPTURE_RECORD_FORMAT),
                Item::Key(&CAPTURE_RECORD_AUDIO),
            ],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcuts are set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
