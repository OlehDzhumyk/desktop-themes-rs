// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the extension `lockscreen-layout@desktop-themes-rs` (the lock screen of GNOME
//! Shell 51), declared in the registry like ours. `cargo xtask gen` writes its schema to
//! `extensions/lockscreen-layout@desktop-themes-rs/schemas/`; themes set the keys in
//! `lockscreen.dconf` (look module), the window has a «Lock screen» page, and `themes apply`
//! refuses values outside what is declared here ([`crate::validate`]).
//!
//! Keys, types, defaults and paths are those of the schema shipped before the registry (stage 0):
//! values already stored in dconf keep working (a test pins them). What changed is only what is
//! *accepted*: the clock position, font weight and the colours used to be free strings.

use crate::model::*;

pub const LOCKSCREEN_SCHEMA: &str = "org.gnome.shell.extensions.lockscreen-layout";
pub const LOCKSCREEN_UUID: &str = "lockscreen-layout@desktop-themes-rs";

pub static FEATURE_LOCKSCREEN: Feature = Feature {
    id: "lockscreen",
    title: n_("Lock screen"),
    origins: &[Origin::Has(n_("Available in Windows 11 and macOS"))],
};

const fn int(
    name: &'static str,
    default: i32,
    (min, max, step): (i32, i32, i32),
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: LOCKSCREEN_SCHEMA,
        name,
        kind: Kind::Int {
            default,
            min,
            max,
            step,
        },
        label,
        description,
        summary,
        doc: "",
        feature: "lockscreen",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

const fn colour(
    name: &'static str,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: LOCKSCREEN_SCHEMA,
        name,
        kind: Kind::Color { default: "" },
        label,
        description,
        summary,
        doc: "",
        feature: "lockscreen",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

const fn boolean(
    name: &'static str,
    default: bool,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: LOCKSCREEN_SCHEMA,
        name,
        kind: Kind::Bool { default },
        label,
        description,
        summary,
        doc: "",
        feature: "lockscreen",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

pub static LS_CLOCK_POSITION: Key = Key {
    schema: LOCKSCREEN_SCHEMA,
    name: "clock-position",
    kind: Kind::Choice {
        default: "default",
        choices: &[
            Choice {
                value: "default",
                label: n_("Standard (centred)"),
            },
            Choice {
                value: "center-top",
                label: n_("Top centre (macOS)"),
            },
            Choice {
                value: "bottom-left",
                label: n_("Bottom left (Windows 11)"),
            },
        ],
    },
    label: n_("Clock position"),
    description: n_("Where the time and date are on the lock screen"),
    summary: "Clock position: default (stock, centred), center-top, bottom-left",
    doc: "",
    feature: "lockscreen",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static LS_MARGIN_X: Key = int(
    "margin-x",
    64,
    (0, 2000, 8),
    n_("Distance from the left edge"),
    n_("Bottom left layout, in pixels"),
    "bottom-left: distance from the left edge, px",
);

pub static LS_MARGIN_Y: Key = int(
    "margin-y",
    96,
    (0, 2000, 8),
    n_("Distance from the top or bottom"),
    n_("Top centre: from the top; bottom left: from the bottom, in pixels"),
    "center-top: distance from the top; bottom-left: from the bottom, px",
);

pub static LS_CLOCK_FONT_SIZE: Key = int(
    "clock-font-size",
    0,
    (0, 400, 2),
    n_("Time size"),
    n_("In pixels; 0 keeps the standard size"),
    "Time font size in px (0 = stock)",
);

pub static LS_DATE_FONT_SIZE: Key = int(
    "date-font-size",
    0,
    (0, 200, 2),
    n_("Date size"),
    n_("In pixels; 0 keeps the standard size"),
    "Date font size in px (0 = stock)",
);

pub static LS_CLOCK_FONT_FAMILY: Key = Key {
    schema: LOCKSCREEN_SCHEMA,
    name: "clock-font-family",
    kind: Kind::Str { default: "" },
    label: n_("Font"),
    description: "",
    summary: "Font family for time and date (empty = stock)",
    doc: "A family name; quotes and backslashes are stripped by the extension.",
    feature: "lockscreen",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static LS_CLOCK_FONT_WEIGHT: Key = Key {
    schema: LOCKSCREEN_SCHEMA,
    name: "clock-font-weight",
    kind: Kind::Choice {
        default: "",
        choices: &[
            Choice {
                value: "",
                label: n_("Standard"),
            },
            Choice {
                value: "100",
                label: n_("Thin (100)"),
            },
            Choice {
                value: "200",
                label: n_("Extra light (200)"),
            },
            Choice {
                value: "300",
                label: n_("Light (300)"),
            },
            Choice {
                value: "400",
                label: n_("Regular (400)"),
            },
            Choice {
                value: "500",
                label: n_("Medium (500)"),
            },
            Choice {
                value: "600",
                label: n_("Semi-bold (600)"),
            },
            Choice {
                value: "700",
                label: n_("Bold (700)"),
            },
            Choice {
                value: "800",
                label: n_("Extra bold (800)"),
            },
            Choice {
                value: "900",
                label: n_("Black (900)"),
            },
            Choice {
                value: "normal",
                label: n_("Normal"),
            },
            Choice {
                value: "bold",
                label: n_("Bold"),
            },
        ],
    },
    label: n_("Time weight"),
    description: "",
    summary: "CSS font-weight for the time (empty = stock)",
    doc: "",
    feature: "lockscreen",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static LS_CLOCK_GLOW: Key = colour(
    "clock-glow",
    n_("Glow around the time"),
    n_("A colour such as #rrggbb or rgba(r, g, b, a); empty = none"),
    "CSS colour of a soft glow (text-shadow) around the time (empty = none)",
);

pub static LS_CLOCK_COLOR: Key = colour(
    "clock-color",
    n_("Time colour"),
    n_("A colour such as #rrggbb or rgba(r, g, b, a); empty = standard"),
    "CSS colour of the time (empty = stock)",
);

pub static LS_DATE_COLOR: Key = colour(
    "date-color",
    n_("Date colour"),
    n_("A colour such as #rrggbb or rgba(r, g, b, a); empty = standard"),
    "CSS colour of the date (empty = stock)",
);

pub static LS_HIDE_HINT: Key = boolean(
    "hide-hint",
    false,
    n_("Hide the hint"),
    n_("Hide “Click or press a key”"),
    "Hide the \"Click or press a key\" hint",
);

pub static LS_PROMPT_OFFSET: Key = int(
    "prompt-offset",
    0,
    (-500, 500, 10),
    n_("Move the password field down"),
    n_("In pixels; negative moves it up"),
    "Shift the password prompt down (px, may be negative)",
);

pub static LS_BLUR: Key = boolean(
    "blur",
    true,
    n_("Blur the background"),
    n_("Off shows the wallpaper sharp"),
    "Blur the background (false = sharp wallpaper)",
);

pub static LS_BLUR_RADIUS: Key = Key {
    sensitive_if: Some("blur"),
    ..int(
        "blur-radius",
        90,
        (0, 200, 5),
        n_("Blur radius"),
        "",
        "Blur radius",
    )
};

pub static LS_BRIGHTNESS: Key = Key {
    schema: LOCKSCREEN_SCHEMA,
    name: "brightness",
    kind: Kind::Double {
        default: 0.65,
        min: 0.0,
        max: 1.0,
        step: 0.05,
    },
    label: n_("Background brightness"),
    description: n_("When the background is blurred"),
    summary: "Background brightness when blurred",
    doc: "",
    feature: "lockscreen",
    widget: Widget::Edit,
    sensitive_if: Some("blur"),
};

pub static LS_TINT: Key = colour(
    "tint",
    n_("Background tint"),
    n_("A colour such as #rrggbb or rgba(r, g, b, a) laid over the background; empty = none"),
    "CSS colour laid over the background, e.g. rgba(90,40,160,0.35) (empty = none)",
);

pub static LS_BACKGROUND_URI: Key = Key {
    schema: LOCKSCREEN_SCHEMA,
    name: "background-uri",
    kind: Kind::Str { default: "" },
    label: n_("Background image"),
    description: n_(
        "A file:// address; empty = the desktop wallpaper; “screensaver” = the screen saver picture of the theme",
    ),
    summary: "file:// image for the lock screen (empty = desktop wallpaper, as stock GNOME 51; the word screensaver = org.gnome.desktop.screensaver picture-uri)",
    doc: "",
    feature: "lockscreen",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static LOCKSCREEN: Schema = Schema {
    id: LOCKSCREEN_SCHEMA,
    path: "/org/gnome/shell/extensions/lockscreen-layout/",
    rust_name: "LockscreenSettings",
    keys: &[
        &LS_CLOCK_POSITION,
        &LS_MARGIN_X,
        &LS_MARGIN_Y,
        &LS_CLOCK_FONT_SIZE,
        &LS_DATE_FONT_SIZE,
        &LS_CLOCK_FONT_FAMILY,
        &LS_CLOCK_FONT_WEIGHT,
        &LS_CLOCK_GLOW,
        &LS_CLOCK_COLOR,
        &LS_DATE_COLOR,
        &LS_HIDE_HINT,
        &LS_PROMPT_OFFSET,
        &LS_BLUR,
        &LS_BLUR_RADIUS,
        &LS_BRIGHTNESS,
        &LS_TINT,
        &LS_BACKGROUND_URI,
    ],
    children: &[],
};

pub static LOCKSCREEN_EXTENSION: ExtensionSchema = ExtensionSchema {
    uuid: LOCKSCREEN_UUID,
    schema: &LOCKSCREEN,
};

/// Schemas that live in an extension's own directory instead of ours.
pub static EXTENSION_SCHEMAS: &[&ExtensionSchema] = &[&LOCKSCREEN_EXTENSION];

pub static PAGE_LOCKSCREEN: Page = Page {
    id: "lockscreen",
    title: n_("Lock screen"),
    subtitle: n_("Clock position, fonts, blur and background of the lock screen"),
    description: n_(
        "The look of the lock screen. Themes set these values; here you can change them. They need \
         the lockscreen-layout extension, which the macOS and Windows 11 themes turn on.",
    ),
    icon: "system-lock-screen-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_LOCKSCREEN),
    groups: &[
        Group {
            title: n_("Layout"),
            description: "",
            items: &[
                Item::Key(&LS_CLOCK_POSITION),
                Item::Key(&LS_MARGIN_X),
                Item::Key(&LS_MARGIN_Y),
                Item::Key(&LS_PROMPT_OFFSET),
                Item::Key(&LS_HIDE_HINT),
            ],
        },
        Group {
            title: n_("Time and date"),
            description: "",
            items: &[
                Item::Key(&LS_CLOCK_FONT_FAMILY),
                Item::Key(&LS_CLOCK_FONT_WEIGHT),
                Item::Key(&LS_CLOCK_FONT_SIZE),
                Item::Key(&LS_DATE_FONT_SIZE),
                Item::Key(&LS_CLOCK_COLOR),
                Item::Key(&LS_DATE_COLOR),
                Item::Key(&LS_CLOCK_GLOW),
            ],
        },
        Group {
            title: n_("Background"),
            description: "",
            items: &[
                Item::Key(&LS_BLUR),
                Item::Key(&LS_BLUR_RADIUS),
                Item::Key(&LS_BRIGHTNESS),
                Item::Key(&LS_TINT),
                Item::Key(&LS_BACKGROUND_URI),
            ],
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys, types, defaults and ranges of the schema shipped before the registry: values users
    /// (and the bundled themes) already have in dconf must keep working.
    #[test]
    fn stage0_keys_unchanged() {
        let xml = crate::generate::extension_gschema_xml(&LOCKSCREEN_EXTENSION);
        assert!(xml.contains(
            "<schema id=\"org.gnome.shell.extensions.lockscreen-layout\" \
             path=\"/org/gnome/shell/extensions/lockscreen-layout/\">"
        ));
        for (name, ty, default, range) in [
            ("clock-position", "s", "'default'", None),
            ("margin-x", "i", "64", Some((0, 2000))),
            ("margin-y", "i", "96", Some((0, 2000))),
            ("clock-font-size", "i", "0", Some((0, 400))),
            ("date-font-size", "i", "0", Some((0, 200))),
            ("clock-font-family", "s", "''", None),
            ("clock-font-weight", "s", "''", None),
            ("clock-glow", "s", "''", None),
            ("clock-color", "s", "''", None),
            ("date-color", "s", "''", None),
            ("hide-hint", "b", "false", None),
            ("prompt-offset", "i", "0", Some((-500, 500))),
            ("blur", "b", "true", None),
            ("blur-radius", "i", "90", Some((0, 200))),
            ("brightness", "d", "0.65", None),
            ("tint", "s", "''", None),
            ("background-uri", "s", "''", None),
        ] {
            let k = crate::key(LOCKSCREEN_SCHEMA, name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(k.kind.gvariant_type(), ty, "{name}");
            assert_eq!(k.kind.default_text(), default, "{name}");
            if let Some((lo, hi)) = range {
                assert!(
                    xml.contains(&format!("<range min=\"{lo}\" max=\"{hi}\"/>")),
                    "{name}"
                );
            }
        }
        assert!(xml.contains("<range min=\"0.0\" max=\"1.0\"/>"));
        assert_eq!(LOCKSCREEN.keys.len(), 17);
    }

    /// The values of the bundled themes (what stored configurations look like) are all accepted.
    #[test]
    fn bundled_theme_values_are_valid() {
        let themes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let mut seen = 0;
        for d in std::fs::read_dir(themes).unwrap().flatten() {
            let Ok(text) = std::fs::read_to_string(d.path().join("lockscreen.dconf")) else {
                continue;
            };
            let mut in_schema = false;
            for line in text.lines().map(str::trim) {
                if let Some(sec) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    in_schema = format!("/{sec}/") == LOCKSCREEN.path;
                } else if in_schema && let Some((k, v)) = line.split_once('=') {
                    let key = crate::key(LOCKSCREEN_SCHEMA, k.trim())
                        .unwrap_or_else(|| panic!("{}: unknown key {k}", d.path().display()));
                    assert_eq!(crate::validate::validate_value(key, v), Ok(()));
                    seen += 1;
                }
            }
        }
        assert!(seen >= 20, "found {seen} lock screen values in the themes");
    }
}
