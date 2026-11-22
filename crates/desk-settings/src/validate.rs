// SPDX-License-Identifier: GPL-3.0-or-later
//! Checks of values against the registry: what GSettings cannot enforce (CSS colours) and the same
//! for values that come from outside the schema (a theme's `.dconf` file, a hand-edited dconf key),
//! in the text format of GVariant that `dconf dump` and theme files use.
//!
//! Why: colours and similar strings end up in St style strings of the lock screen. A hostile or
//! sloppy theme must not be able to add declarations (`red; margin: 0`).

use crate::model::{Key, Kind};

/// The same grammar as a JavaScript regular expression: `extensions/lockscreen-layout@.../extension.js`
/// carries this exact literal (a test compares them, and another checks that both agree).
pub const CSS_COLOR_JS_RE: &str = r"^(?:#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})|rgba?\(\s*\d+(?:\.\d+)?%?\s*,\s*\d+(?:\.\d+)?%?\s*,\s*\d+(?:\.\d+)?%?\s*(?:,\s*\d+(?:\.\d+)?%?\s*)?\))$";

fn is_number(s: &str) -> bool {
    let s = s.strip_suffix('%').unwrap_or(s);
    let (int, frac) = s.split_once('.').map_or((s, None), |(i, f)| (i, Some(f)));
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)` or `rgba(r, g, b, a)` (numbers, optional
/// `%`). Named colours, `var()`, `;`, `url()` and everything else are refused. Empty is not a colour
/// (a [`Kind::Color`] key allows it separately).
pub fn is_css_color(s: &str) -> bool {
    if let Some(hex) = s.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let Some(args) = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    else {
        return false;
    };
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    matches!(parts.len(), 3 | 4) && parts.iter().all(|p| is_number(p))
}

/// A GVariant text string (`'x'` or `"x"`, backslash escapes) → its content.
pub fn unquote(text: &str) -> Option<String> {
    let t = text.trim();
    let q = t.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = t.strip_prefix(q)?.strip_suffix(q)?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            c if c == q => return None, // an unescaped quote inside
            c => out.push(c),
        }
    }
    Some(out)
}

/// GVariant text of a number may carry its type (`uint32 5`, `int32 5`, `double 0.5`, `@i 5`).
fn number_text(text: &str) -> &str {
    let t = text.trim();
    for p in ["uint32 ", "int32 ", "double ", "@i ", "@u ", "@d "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim();
        }
    }
    t
}

/// Is `text` (GVariant text, as in a dconf dump) an allowed value of `k`? `Err` says what is wrong.
/// Lists (`as`) are not checked here.
pub fn validate_value(k: &Key, text: &str) -> Result<(), String> {
    let bad_range = |lo: String, hi: String| format!("{} must be {lo}…{hi}", k.name);
    match &k.kind {
        Kind::Bool { .. } => match text.trim() {
            "true" | "false" => Ok(()),
            _ => Err(format!("{} must be true or false", k.name)),
        },
        Kind::UInt { min, max, .. } => match number_text(text).parse::<u32>() {
            Ok(v) if (*min..=*max).contains(&v) => Ok(()),
            _ => Err(bad_range(min.to_string(), max.to_string())),
        },
        Kind::Int { min, max, .. } => match number_text(text).parse::<i32>() {
            Ok(v) if (*min..=*max).contains(&v) => Ok(()),
            _ => Err(bad_range(min.to_string(), max.to_string())),
        },
        Kind::Double { min, max, .. } => match number_text(text).parse::<f64>() {
            Ok(v) if v.is_finite() && (*min..=*max).contains(&v) => Ok(()),
            _ => Err(bad_range(min.to_string(), max.to_string())),
        },
        Kind::Choice { choices, .. } => match unquote(text) {
            Some(v) if choices.iter().any(|c| c.value == v) => Ok(()),
            Some(v) => Err(format!(
                "{} cannot be '{v}' (one of: {})",
                k.name,
                choices
                    .iter()
                    .map(|c| c.value)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            None => Err(format!("{} must be a string", k.name)),
        },
        Kind::Color { .. } => match unquote(text) {
            Some(v) if v.is_empty() || is_css_color(&v) => Ok(()),
            Some(v) => Err(format!(
                "{} cannot be '{v}' (a colour such as #rrggbb or rgba(r, g, b, a), or empty)",
                k.name
            )),
            None => Err(format!("{} must be a string", k.name)),
        },
        Kind::Str { .. } => unquote(text)
            .map(|_| ())
            .ok_or_else(|| format!("{} must be a string", k.name)),
        Kind::StrList { .. } | Kind::Accel { .. } => Ok(()),
    }
}

/// The key a dconf path (`/org/gnome/shell/extensions/lockscreen-layout/tint`) belongs to.
pub fn key_by_path(path: &str) -> Option<&'static Key> {
    crate::all_schemas().find_map(|s| {
        let name = path.strip_prefix(s.path)?;
        s.keys.iter().copied().find(|k| k.name == name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockscreen::*;

    #[test]
    fn css_colours() {
        for ok in [
            "#fff",
            "#ffff",
            "#a1b2c3",
            "#A1B2C3D4",
            "rgb(1,2,3)",
            "rgba(90, 40, 160, 0.35)",
            "rgba(0,0,0,0.22)",
            "rgba(255,255,255,1.0)",
            "rgb( 10% , 20% , 30% )",
        ] {
            assert!(is_css_color(ok), "{ok}");
        }
        for bad in [
            "",
            "red",
            "#ff",
            "#fffff",
            "#ggg",
            "red; margin: 0",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "rgba(1,2,3,0.5); x",
            "rgba(1,2,3,0.5))",
            "rgb(1,2,-3)",
            "rgb(1,2,3.)",
            "rgb(var(--x),2,3)",
            "url(x)",
            "#fff;",
            "RGBA(1,2,3,1)",
        ] {
            assert!(!is_css_color(bad), "{bad}");
        }
    }

    /// The JavaScript literal in the extension is the one exported here, and it agrees with the
    /// Rust grammar on every vector (so the extension refuses what the registry refuses).
    #[test]
    fn js_regex_matches_rust_grammar() {
        let re = regex::Regex::new(CSS_COLOR_JS_RE).unwrap();
        for v in [
            "#fff",
            "#ffff",
            "#a1b2c3",
            "#A1B2C3D4",
            "rgb(1,2,3)",
            "rgba(90, 40, 160, 0.35)",
            "rgb( 10% , 20% , 30% )",
            "",
            "red",
            "#ff",
            "#fffff",
            "#ggg",
            "red; margin: 0",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "rgba(1,2,3,0.5); x",
            "rgb(1,2,-3)",
            "rgb(1,2,3.)",
            "url(x)",
            "#fff;",
            "RGBA(1,2,3,1)",
            "rgba(1,2,3,.5)",
            "rgb(1,2,3,4)",
        ] {
            assert_eq!(re.is_match(v), is_css_color(v), "{v:?}");
        }
        let js = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/lockscreen-layout@desktop-themes-rs/extension.js"
        ))
        .unwrap();
        assert!(
            js.contains(&format!("const COLOR_RE = /{CSS_COLOR_JS_RE}/;")),
            "extension.js must define COLOR_RE with CSS_COLOR_JS_RE (crates/desk-settings/src/validate.rs)"
        );
    }

    #[test]
    fn values_by_kind() {
        let ok = |k: &Key, t: &str| assert_eq!(validate_value(k, t), Ok(()), "{} {t}", k.name);
        let bad = |k: &Key, t: &str| assert!(validate_value(k, t).is_err(), "{} {t}", k.name);
        ok(&LS_CLOCK_POSITION, "'center-top'");
        bad(&LS_CLOCK_POSITION, "'middle'");
        bad(&LS_CLOCK_POSITION, "center-top");
        ok(&LS_MARGIN_Y, "110");
        ok(&LS_MARGIN_Y, "int32 110");
        bad(&LS_MARGIN_Y, "2001");
        bad(&LS_MARGIN_Y, "-1");
        bad(&LS_MARGIN_Y, "'110'");
        ok(&LS_PROMPT_OFFSET, "-500");
        bad(&LS_PROMPT_OFFSET, "-501");
        ok(&LS_BRIGHTNESS, "0.78");
        ok(&LS_BRIGHTNESS, "1.0");
        ok(&LS_BRIGHTNESS, "1");
        bad(&LS_BRIGHTNESS, "1.5");
        bad(&LS_BRIGHTNESS, "nan");
        ok(&LS_BLUR, "false");
        bad(&LS_BLUR, "'false'");
        ok(&LS_CLOCK_FONT_WEIGHT, "'200'");
        ok(&LS_CLOCK_FONT_WEIGHT, "''");
        ok(&LS_CLOCK_FONT_WEIGHT, "'bold'");
        bad(&LS_CLOCK_FONT_WEIGHT, "'200; margin: 0'");
        ok(&LS_TINT, "''");
        ok(&LS_TINT, "'rgba(0,0,0,0.22)'");
        ok(&LS_CLOCK_COLOR, "\"#ffffff\"");
        bad(&LS_CLOCK_COLOR, "'red; margin:0'");
        bad(&LS_CLOCK_COLOR, "'red'");
        ok(&LS_BACKGROUND_URI, "'screensaver'");
    }

    #[test]
    fn keys_found_by_dconf_path() {
        let k = key_by_path("/org/gnome/shell/extensions/lockscreen-layout/tint").unwrap();
        assert_eq!(k.name, "tint");
        assert!(key_by_path("/org/gnome/shell/extensions/lockscreen-layout/nope").is_none());
        assert!(key_by_path("/io/github/OlehDzhumyk/Desk/skin").is_some());
        assert!(key_by_path("/org/gnome/desktop/interface/gtk-theme").is_none());
    }

    #[test]
    fn unquote_handles_escapes() {
        assert_eq!(unquote("'a'").as_deref(), Some("a"));
        assert_eq!(unquote("\"a'b\"").as_deref(), Some("a'b"));
        assert_eq!(unquote("'it\\'s'").as_deref(), Some("it's"));
        assert_eq!(unquote("'a'b'"), None);
        assert_eq!(unquote("a"), None);
        assert_eq!(unquote("'a"), None);
    }
}
