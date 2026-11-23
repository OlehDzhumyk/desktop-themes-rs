// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the OCR text extractor (stage 3, docs/spec/features/ocr.md): schema
//! `io.github.OlehDzhumyk.Desk.Ocr`, its shortcut and its page, declared in the registry like the
//! others and kept in their own module so that features added in parallel do not collide.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const OCR_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Ocr";

pub static FEATURE_OCR: Feature = Feature {
    id: "ocr",
    title: n_("Text extractor"),
    origins: &[Origin::Has(n_(
        "In Windows 11 (PowerToys Text Extractor) and macOS (Live Text)",
    ))],
};

pub static OCR_ENABLED: Key = Key {
    schema: OCR_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Text extractor"),
    description: n_("Select a region of the screen and copy the text in it, recognised locally"),
    summary: "Enable the OCR text extractor",
    doc: "",
    feature: "ocr",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static OCR_LANGUAGES: Key = Key {
    schema: OCR_SCHEMA,
    name: "languages",
    kind: Kind::StrList {
        default: &["ukr", "eng"],
    },
    label: n_("Languages"),
    description: n_(
        "Tesseract language codes such as ukr and eng; each needs its tesseract-langpack",
    ),
    summary: "Tesseract language codes used for recognition",
    doc: "Codes as `tesseract --list-langs` prints them. Codes without installed data are left \
          out and named in the result toast; recognition runs with the others.",
    feature: "ocr",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OCR_RECORD_IN_HISTORY: Key = Key {
    schema: OCR_SCHEMA,
    name: "record-in-history",
    kind: Kind::Bool { default: true },
    label: n_("Record in clipboard history"),
    description: n_("Private mode of the clipboard always wins"),
    summary: "Put recognised text into the clipboard history",
    doc: "False: the text is still copied to the clipboard, but the clipboard history does not \
          record it (as in private mode).",
    feature: "ocr",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OCR_KEEP_LINE_BREAKS: Key = Key {
    schema: OCR_SCHEMA,
    name: "keep-line-breaks",
    kind: Kind::Bool { default: true },
    label: n_("Keep line breaks"),
    description: n_("Off: the lines of a paragraph are joined with spaces"),
    summary: "Keep the line breaks inside paragraphs",
    doc: "Paragraph breaks are always kept; words broken with a hyphen at a line end are joined.",
    feature: "ocr",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OCR_MAX_MEGAPIXELS: Key = Key {
    schema: OCR_SCHEMA,
    name: "max-megapixels",
    kind: Kind::UInt {
        default: 20,
        min: 1,
        max: 50,
        step: 1,
    },
    label: n_("Largest region, megapixels"),
    description: n_("Larger regions are refused"),
    summary: "Largest region that is recognised, in megapixels",
    doc: "deskd refuses an image with width × height above this value × 1 000 000.",
    feature: "ocr",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static OCR_SHORTCUT: Key = Key {
    schema: OCR_SCHEMA,
    name: "ocr-shortcut",
    kind: Kind::Accel {
        default: &["<Shift><Super>t"],
    },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that starts the text extractor (in force)",
    doc: "The accelerators the bridge binds. Themes write their default here from desk.dconf \
          (Shift+Super+T for Windows 11 and Fedora, Ctrl+Shift+Super+T for macOS); an override \
          in io.github.OlehDzhumyk.Desk.Shortcuts ocr-toggle replaces it. The key is not called \
          toggle-shortcut because shell keybinding names are global.",
    feature: "ocr",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static OCR: Schema = Schema {
    id: OCR_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/ocr/",
    rust_name: "OcrSettings",
    keys: &[
        &OCR_ENABLED,
        &OCR_LANGUAGES,
        &OCR_RECORD_IN_HISTORY,
        &OCR_KEEP_LINE_BREAKS,
        &OCR_MAX_MEGAPIXELS,
        &OCR_SHORTCUT,
    ],
    children: &[],
};

pub static SC_OCR_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "ocr-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Copy text from the screen"),
    description: "",
    summary: "Your shortcut for the text extractor (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "ocr",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_OCR: Shortcut = Shortcut {
    id: "ocr-toggle",
    label: n_("Copy text from the screen"),
    feature: "ocr",
    key: &OCR_SHORTCUT,
    override_key: &SC_OCR_TOGGLE,
};

pub static PAGE_OCR: Page = Page {
    id: "ocr",
    title: n_("Text extractor"),
    subtitle: n_("Copy text from any part of the screen: Win+Shift+T in Windows 11 (PowerToys)"),
    description: n_(
        "Select a region of the screen and the text in it is recognised on this computer with \
         Tesseract and copied to the clipboard. Nothing is sent to the network. The deskd \
         service runs the recognition; the shell bridge draws the selection in the style of the \
         current theme.",
    ),
    icon: "insert-text-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_OCR),
    groups: &[
        Group {
            title: n_("Recognition"),
            description: n_("Needs the tesseract package and its language data."),
            items: &[
                Item::Key(&OCR_ENABLED),
                Item::Key(&OCR_LANGUAGES),
                Item::Key(&OCR_KEEP_LINE_BREAKS),
                Item::Key(&OCR_MAX_MEGAPIXELS),
            ],
        },
        Group {
            title: n_("Clipboard"),
            description: "",
            items: &[Item::Key(&OCR_RECORD_IN_HISTORY)],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
