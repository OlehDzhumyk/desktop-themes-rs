// SPDX-License-Identifier: GPL-3.0-or-later
//! Ranges and WCAG 2.x contrast checks.
//!
//! Contrast policy: a text/background pair is checked when the theme sets at least one side
//! (the other side falls back to the libadwaita default). Body text needs AA, 4.5:1; text on
//! filled controls (accent, destructive, success, warning, error buttons, bold/large by HIG)
//! needs 3:1 (AA large text / UI components). Stock values alone are never reported: the
//! policy judges the theme, not upstream.

use crate::adwaita;
use crate::color::{Rgba, contrast_on};
use crate::load::Issue;
use crate::schema::{ColorSet, Scheme, Shadow, TermColors, Tokens};

pub const AA_TEXT: f64 = 4.5;
pub const AA_LARGE: f64 = 3.0;

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl Report {
    fn err(&mut self, path: impl Into<String>, msg: impl Into<String>) {
        self.errors.push(Issue {
            file: None,
            path: path.into(),
            message: msg.into(),
        });
    }
    fn warn(&mut self, path: impl Into<String>, msg: impl Into<String>) {
        self.warnings.push(Issue {
            file: None,
            path: path.into(),
            message: msg.into(),
        });
    }
}

/// (foreground key, background key, minimum ratio)
pub const TEXT_PAIRS: &[(&str, &str, f64)] = &[
    ("window_fg", "window_bg", AA_TEXT),
    ("view_fg", "view_bg", AA_TEXT),
    ("headerbar_fg", "headerbar_bg", AA_TEXT),
    ("sidebar_fg", "sidebar_bg", AA_TEXT),
    ("card_fg", "card_bg", AA_TEXT),
    ("popover_fg", "popover_bg", AA_TEXT),
    ("dialog_fg", "dialog_bg", AA_TEXT),
    ("accent_fg", "accent", AA_LARGE),
    ("destructive_fg", "destructive_bg", AA_LARGE),
    ("success_fg", "success_bg", AA_LARGE),
    ("warning_fg", "warning_bg", AA_LARGE),
    ("error_fg", "error_bg", AA_LARGE),
];

fn get(c: &ColorSet, key: &str) -> Option<Rgba> {
    if key == "accent" {
        return c.accent.map(|a| a.rgba());
    }
    c.colors()
        .into_iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v)
}

/// Effective value: set by the theme, else the libadwaita default.
pub fn effective(c: &ColorSet, s: Scheme, key: &str) -> Option<Rgba> {
    get(c, key).or_else(|| adwaita::default_color(s, key))
}

fn window_base(c: &ColorSet, s: Scheme) -> Rgba {
    let under = if s == Scheme::Light {
        Rgba::rgb(255, 255, 255)
    } else {
        Rgba::rgb(0, 0, 0)
    };
    effective(c, s, "window_bg").unwrap_or(under).over(under)
}

fn check_colors(r: &mut Report, c: &ColorSet, s: Scheme) {
    let base = window_base(c, s);
    for (fg, bg, min) in TEXT_PAIRS {
        if get(c, fg).is_none() && get(c, bg).is_none() {
            continue;
        }
        let (Some(f), Some(b)) = (effective(c, s, fg), effective(c, s, bg)) else {
            continue;
        };
        let under = if *bg == "window_bg" { b.opaque() } else { base };
        let ratio = contrast_on(f, b, under);
        if ratio + 1e-9 < *min {
            let side = |k: &str| {
                if get(c, k).is_some() {
                    format!("{k} {}", effective(c, s, k).unwrap())
                } else {
                    format!("{k} (default {})", effective(c, s, k).unwrap())
                }
            };
            r.err(
                format!("colors.{}.{fg}", s.id()),
                format!(
                    "contrast {:.2}:1 of {} on {} is below WCAG AA {min}:1",
                    ratio,
                    side(fg),
                    side(bg)
                ),
            );
        }
    }
    // standalone accent (links, accent-coloured text): libadwaita derives it from the accent
    // by clamping OKLab lightness (<= 0.5 light, >= 0.85 dark); check the derived colour.
    if let Some(a) = c.accent {
        let lab = a.rgba().to_oklab();
        let l = if s == Scheme::Light {
            lab.l.min(0.5)
        } else {
            lab.l.max(0.85)
        };
        let standalone = crate::color::Oklab { l, ..lab }.to_rgb();
        let view = effective(c, s, "view_bg").unwrap_or(base).over(base);
        let ratio = contrast_on(standalone, view, base);
        if ratio < AA_TEXT {
            r.warn(
                format!("colors.{}.accent", s.id()),
                format!(
                    "accent-coloured text ({standalone}, derived by libadwaita) has {ratio:.2}:1 on view_bg"
                ),
            );
        }
    }
    if let Some(o) = c.border_opacity
        && !(0.0..=1.0).contains(&o)
    {
        r.err(
            format!("colors.{}.border_opacity", s.id()),
            "must be within 0..1",
        );
    }
}

fn check_range(r: &mut Report, path: &str, v: Option<u32>, max: u32) {
    if let Some(v) = v
        && v > max
    {
        r.err(path, format!("{v} is out of range 0..={max}"));
    }
}

fn check_unit(r: &mut Report, path: &str, v: Option<f64>) {
    if let Some(v) = v
        && !(0.0..=1.0).contains(&v)
    {
        r.err(path, format!("{v} is out of range 0..1"));
    }
}

/// "Family [Style...] Size" with a size in points.
pub fn font_size(desc: &str) -> Option<f64> {
    let last = desc.split_whitespace().last()?;
    let n: f64 = last.parse().ok()?;
    (desc.split_whitespace().count() >= 2).then_some(n)
}

fn check_font(r: &mut Report, path: &str, v: &Option<String>) {
    let Some(v) = v else { return };
    match font_size(v) {
        Some(n) if (4.0..=72.0).contains(&n) => {}
        Some(n) => r.err(path, format!("font size {n} is out of range 4..72")),
        None => r.err(
            path,
            format!("«{v}» is not a Pango font description \"Family [Style] Size\""),
        ),
    }
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// Named easing → cubic-bezier control points.
pub fn easing_bezier(name: &str) -> Option<[f64; 4]> {
    let v = match name {
        "linear" => [0.0, 0.0, 1.0, 1.0],
        "ease" => [0.25, 0.1, 0.25, 1.0],
        "ease-in" => [0.42, 0.0, 1.0, 1.0],
        "ease-out" => [0.0, 0.0, 0.58, 1.0],
        "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
        "ease-out-quad" => [0.25, 0.46, 0.45, 0.94],
        "ease-out-cubic" => [0.215, 0.61, 0.355, 1.0],
        "ease-out-quint" => [0.23, 1.0, 0.32, 1.0],
        "ease-in-out-cubic" => [0.645, 0.045, 0.355, 1.0],
        _ => {
            let inner = name.strip_prefix("cubic-bezier(")?.strip_suffix(')')?;
            let v: Vec<f64> = inner
                .split(',')
                .map(|p| p.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .ok()?;
            if v.len() != 4 || !(0.0..=1.0).contains(&v[0]) || !(0.0..=1.0).contains(&v[2]) {
                return None;
            }
            [v[0], v[1], v[2], v[3]]
        }
    };
    Some(v)
}

fn check_shadow(r: &mut Report, path: &str, s: &Option<Shadow>) {
    let Some(s) = s else { return };
    if s.x.abs() > 32 || s.y.abs() > 32 {
        r.err(path, "x/y offsets must be within -32..32");
    }
    if s.blur > 64 {
        r.err(path, "blur must be within 0..64");
    }
    if s.spread.abs() > 32 {
        r.err(path, "spread must be within -32..32");
    }
    if !(0.0..=1.0).contains(&s.opacity) {
        r.err(path, "opacity must be within 0..1");
    }
}

fn check_term(r: &mut Report, path: &str, t: &TermColors) {
    let Some(p) = &t.palette else {
        r.err(format!("{path}.palette"), "16 colours are required");
        return;
    };
    if p.len() != 16 {
        r.err(
            format!("{path}.palette"),
            format!("{} colours given, 16 required", p.len()),
        );
        return;
    }
    let (Some(fg), Some(bg)) = (t.fg, t.bg) else {
        r.err(path, "fg and bg are required");
        return;
    };
    let ratio = contrast_on(fg, bg, bg.opaque());
    if ratio < AA_TEXT {
        r.err(
            format!("{path}.fg"),
            format!("contrast {ratio:.2}:1 of fg {fg} on bg {bg} is below WCAG AA 4.5:1"),
        );
    }
    if let Some(c) = t.cursor
        && contrast_on(c, bg, bg.opaque()) < AA_LARGE
    {
        r.warn(
            format!("{path}.cursor"),
            "cursor is hard to see on bg (< 3:1)",
        );
    }
    // normal + bright colours except black/white, which are allowed to blend with bg
    let weak: Vec<String> = p
        .iter()
        .enumerate()
        .filter(|(i, _)| !matches!(i, 0 | 7 | 8 | 15))
        .filter(|(_, c)| contrast_on(**c, bg, bg.opaque()) < AA_LARGE)
        .map(|(i, _)| i.to_string())
        .collect();
    if !weak.is_empty() {
        r.warn(
            format!("{path}.palette"),
            format!("colours {} have less than 3:1 on bg", weak.join(", ")),
        );
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one validation pass over all token groups"
)]
pub fn validate(t: &Tokens) -> Report {
    let mut r = Report::default();
    check_colors(&mut r, &t.colors.light, Scheme::Light);
    check_colors(&mut r, &t.colors.dark, Scheme::Dark);
    if let Some(cs) = &t.color_scheme
        && !matches!(cs.as_str(), "default" | "prefer-dark" | "prefer-light")
    {
        r.err(
            "color_scheme",
            "must be \"default\", \"prefer-dark\" or \"prefer-light\"",
        );
    }
    if let Some(n) = &t.name
        && (n.trim().is_empty() || n.len() > 80)
    {
        r.err("name", "must be 1..80 characters");
    }
    let rd = &t.radii;
    check_range(&mut r, "radii.window", rd.window, 48);
    check_range(&mut r, "radii.button", rd.button, 64);
    check_range(&mut r, "radii.card", rd.card, 64);
    check_range(&mut r, "radii.popover", rd.popover, 64);
    check_range(&mut r, "radii.panel", rd.panel, 64);

    let f = &t.fonts;
    check_font(&mut r, "fonts.ui", &f.ui);
    check_font(&mut r, "fonts.document", &f.document);
    check_font(&mut r, "fonts.mono", &f.mono);
    check_font(&mut r, "fonts.titlebar", &f.titlebar);
    if let Some(w) = f.title_weight
        && (!(100..=900).contains(&w) || w % 100 != 0)
    {
        r.err(
            "fonts.title_weight",
            format!("{w} is not one of 100, 200, ... 900"),
        );
    }

    let sp = &t.spacing;
    let steps = [
        ("xs", sp.xs),
        ("sm", sp.sm),
        ("md", sp.md),
        ("lg", sp.lg),
        ("xl", sp.xl),
    ];
    let mut prev: Option<(&str, u32)> = None;
    for (n, v) in steps {
        check_range(&mut r, &format!("spacing.{n}"), v, 96);
        if let Some(v) = v {
            if let Some((pn, pv)) = prev
                && v < pv
            {
                r.err(
                    format!("spacing.{n}"),
                    format!("{v} is smaller than spacing.{pn} = {pv}"),
                );
            }
            prev = Some((n, v));
        }
    }

    check_shadow(&mut r, "shadows.card", &t.shadows.card);
    check_shadow(&mut r, "shadows.popover", &t.shadows.popover);
    check_unit(&mut r, "surface.panel_opacity", t.surface.panel_opacity);
    check_unit(&mut r, "surface.popover_opacity", t.surface.popover_opacity);

    let m = &t.motion;
    let durs = [
        ("fast_ms", m.fast_ms),
        ("normal_ms", m.normal_ms),
        ("slow_ms", m.slow_ms),
    ];
    let mut prev: Option<(&str, u32)> = None;
    for (n, v) in durs {
        check_range(&mut r, &format!("motion.{n}"), v, 2000);
        if let Some(v) = v {
            if let Some((pn, pv)) = prev
                && v < pv
            {
                r.err(
                    format!("motion.{n}"),
                    format!("{v} is shorter than motion.{pn} = {pv}"),
                );
            }
            prev = Some((n, v));
        }
    }
    if let Some(e) = &m.easing
        && easing_bezier(e).is_none()
    {
        r.err(
            "motion.easing",
            format!("«{e}» is not a known easing or cubic-bezier(a, b, c, d)"),
        );
    }

    if let Some(s) = &t.sounds.theme
        && !valid_name(s)
    {
        r.err(
            "sounds.theme",
            format!("«{s}» is not a valid sound theme name"),
        );
    }

    if let Some(term) = &t.terminal {
        if let Some(b) = &term.builtin
            && !valid_name(b)
        {
            r.err(
                "terminal.builtin",
                format!("«{b}» is not a valid palette id"),
            );
        }
        check_font(&mut r, "terminal.font", &term.font);
        if term.builtin.is_some() && (term.light.is_some() || term.dark.is_some()) {
            r.err(
                "terminal",
                "set either builtin or [terminal.light]/[terminal.dark], not both",
            );
        }
        if let Some(d) = &term.dark {
            check_term(&mut r, "terminal.dark", d);
        }
        if let Some(l) = &term.light {
            check_term(&mut r, "terminal.light", l);
        }
        if let Some(n) = &term.name
            && (n.trim().is_empty() || n.contains(['\n', '[', ']']))
        {
            r.err("terminal.name", "must be a single line without brackets");
        }
    }

    if let Some(sh) = &t.shell {
        if let Some(s) = &sh.scheme
            && !matches!(s.as_str(), "dark" | "light")
        {
            r.err("shell.scheme", "must be \"dark\" or \"light\"");
        }
        let scheme = t.shell_scheme();
        if sh.panel_bg.is_some() || sh.panel_fg.is_some() {
            let (dbg, dfg) = adwaita::shell_panel(scheme);
            let bg = sh.panel_bg.unwrap_or(dbg);
            let fg = sh.panel_fg.unwrap_or(dfg);
            let ratio = contrast_on(fg, bg, Rgba::rgb(0, 0, 0));
            if ratio < AA_TEXT {
                r.err(
                    "shell.panel_fg",
                    format!("contrast {ratio:.2}:1 of panel_fg {fg} on panel_bg {bg} is below WCAG AA 4.5:1"),
                );
            }
        }
    }

    if let Some(w) = &t.wallpaper {
        if let Some(s) = &w.style
            && s != "gradient"
            && s != "none"
        {
            r.err("wallpaper.style", "must be \"gradient\" or \"none\"");
        }
        if let Some(i) = &w.image
            && i.trim().is_empty()
        {
            r.err("wallpaper.image", "is empty");
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use crate::load::load_str;

    #[test]
    fn stock_values_alone_pass() {
        // accent only: stock white-on-blue (3.8:1) is fine for buttons (3:1)
        assert!(load_str("t", "[colors]\naccent = \"blue\"\n").is_ok());
        assert!(load_str("t", "").is_ok());
    }

    #[test]
    fn contrast_failures_name_both_sides() {
        let e = load_str("t", "[colors.light]\nwindow_bg = \"#202020\"\n").unwrap_err();
        let s = e.to_string();
        assert!(s.contains("colors.light.window_fg"), "{s}");
        assert!(s.contains("default"), "{s}");
        assert!(s.contains("4.5"), "{s}");

        let e = load_str(
            "t",
            "[colors.dark]\naccent = \"#ffd60a\"\naccent_fg = \"#ffffff\"\n",
        )
        .unwrap_err();
        assert!(e.to_string().contains("accent_fg"), "{e}");
        // dark text on yellow passes
        assert!(
            load_str(
                "t",
                "[colors.dark]\naccent = \"#ffd60a\"\naccent_fg = \"#000000cc\"\n"
            )
            .is_ok()
        );
    }

    #[test]
    fn ranges() {
        for (src, needle) in [
            ("[radii]\nwindow = 100\n", "radii.window"),
            ("[fonts]\nui = \"Inter\"\n", "fonts.ui"),
            ("[fonts]\nui = \"Inter 200\"\n", "fonts.ui"),
            ("[fonts]\ntitle_weight = 650\n", "title_weight"),
            ("[spacing]\nsm = 8\nmd = 4\n", "spacing.md"),
            (
                "[motion]\nfast_ms = 300\nnormal_ms = 100\n",
                "motion.normal_ms",
            ),
            ("[motion]\neasing = \"bouncy\"\n", "motion.easing"),
            ("[surface]\npanel_opacity = 1.5\n", "panel_opacity"),
            (
                "[shadows.card]\nblur = 100\nopacity = 0.2\n",
                "shadows.card",
            ),
            ("color_scheme = \"dark\"\n", "color_scheme"),
            ("[sounds]\ntheme = \"a b\"\n", "sounds.theme"),
            (
                "[terminal.dark]\nfg = \"#ffffff\"\nbg = \"#000000\"\npalette = [\"#000000\"]\n",
                "16 required",
            ),
            (
                "[terminal.dark]\nfg = \"#333333\"\nbg = \"#000000\"\npalette = []\n",
                "terminal.dark",
            ),
            ("[shell]\nscheme = \"blue\"\n", "shell.scheme"),
            ("[shell]\npanel_bg = \"#ffffff\"\n", "shell.panel_fg"),
        ] {
            let e = load_str("t", src).expect_err(src);
            assert!(e.to_string().contains(needle), "{src}: {e}");
        }
        assert!(load_str("t", "[motion]\neasing = \"cubic-bezier(0.2, 0, 0, 1)\"\n").is_ok());
        assert!(load_str("t", "[fonts]\nui = \"Adwaita Sans Semi-Bold 11\"\n").is_ok());
    }
}
