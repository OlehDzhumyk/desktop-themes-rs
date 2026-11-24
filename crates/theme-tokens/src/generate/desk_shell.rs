// SPDX-License-Identifier: GPL-3.0-or-later
//! desk-shell stylesheet: the `--ds-*` custom properties of `crates/desk-shell/src/style.rs`
//! (GTK ≥ 4.16) for the light and dark palettes, the same values as `@define-color` names
//! (`desk_shell_<scheme>_<name>`) for older custom CSS, and the token-driven surface rules
//! (taskbar, Start menu, flyouts, hover and active states).
//!
//! The file is a layer above desk-shell's built-in stylesheet: an unset token keeps the
//! built-in Windows 11 value, and a GNOME accent name (like in the libadwaita stylesheet) keeps
//! the accent of desk-shell's skin. Each palette is scoped to its scheme class (`.light`,
//! `.dark`; desk-shell always sets one) and excludes `.hc`: the high-contrast palettes of
//! desk-shell always win, even when this file is loaded at a higher priority.

use std::fmt::Write;

use crate::color::Rgba;
use crate::generate::header;
use crate::schema::{Accent, Scheme, Tokens};

/// desk-shell's built-in (Windows 11) value of a palette entry, for a token the theme leaves out.
struct Defaults {
    bg: Rgba,
    bg_opacity: f64,
    flyout: Rgba,
    flyout_opacity: f64,
    fg: Rgba,
    dim: f64,
    hover: f64,
    active: f64,
    stroke: f64,
}

fn defaults(s: Scheme) -> Defaults {
    match s {
        Scheme::Light => Defaults {
            bg: Rgba::rgb(243, 243, 243),
            bg_opacity: 0.92,
            flyout: Rgba::rgb(249, 249, 249),
            flyout_opacity: 0.96,
            fg: Rgba::rgb(0x1b, 0x1b, 0x1b),
            dim: 0.62,
            hover: 0.055,
            active: 0.09,
            stroke: 0.08,
        },
        Scheme::Dark => Defaults {
            bg: Rgba::rgb(32, 32, 32),
            bg_opacity: 0.90,
            flyout: Rgba::rgb(44, 44, 44),
            flyout_opacity: 0.96,
            fg: Rgba::rgb(255, 255, 255),
            dim: 0.72,
            hover: 0.06,
            active: 0.10,
            stroke: 0.09,
        },
    }
}

/// (custom property without `--ds-`, value) of one scheme, in style.rs order.
fn palette(t: &Tokens, s: Scheme) -> Vec<(&'static str, Rgba)> {
    let c = t.colors(s);
    let d = defaults(s);
    // the opacity a token colour already carries is kept, the surface opacity multiplies it
    let surface = |col: Rgba, o: f64| col.with_alpha(col.alpha() * o);
    let bg = surface(
        c.window_bg.unwrap_or(d.bg),
        t.surface.panel_opacity.unwrap_or(d.bg_opacity),
    );
    let flyout = surface(
        c.popover_bg.unwrap_or(d.flyout),
        t.surface.popover_opacity.unwrap_or(d.flyout_opacity),
    );
    let fg = c.window_fg.unwrap_or(d.fg);
    let tint = |o: f64| fg.opaque().with_alpha(o);
    let field = match s {
        Scheme::Light => c
            .view_bg
            .unwrap_or(Rgba::rgb(255, 255, 255))
            .with_alpha(0.85),
        Scheme::Dark => tint(0.06),
    };
    let mut v = vec![
        ("bg", bg),
        ("flyout", flyout),
        ("fg", fg),
        ("dim", tint(d.dim)),
        ("hover", tint(d.hover)),
        ("active", tint(d.active)),
        ("stroke", tint(c.border_opacity.unwrap_or(d.stroke))),
        ("field", field),
    ];
    // a GNOME accent name emits nothing (as in the libadwaita stylesheet): desk-shell keeps
    // the accent of its skin
    if let Some(Accent::Color(a)) = c.accent {
        v.push(("accent", a));
        // libadwaita's accent foreground is white unless the theme says otherwise
        v.push(("on-accent", c.accent_fg.unwrap_or(Rgba::rgb(255, 255, 255))));
    } else if let Some(fg) = c.accent_fg {
        v.push(("on-accent", fg));
    }
    v
}

fn write_scheme(out: &mut String, t: &Tokens, s: Scheme) {
    let p = palette(t, s);
    let _ = writeln!(out, "\n/* {} palette */", s.id());
    for (k, v) in &p {
        let _ = writeln!(
            out,
            "@define-color desk_shell_{}_{} {};",
            s.id(),
            k.replace('-', "_"),
            v.css()
        );
    }
    let sel = match s {
        Scheme::Light => ".desk-shell.light:not(.hc)",
        Scheme::Dark => ".desk-shell.dark:not(.hc)",
    };
    let _ = writeln!(out, "{sel} {{");
    for (k, v) in &p {
        let _ = writeln!(out, "  --ds-{k}: {};", v.css());
    }
    out.push_str("}\n");
}

const SURFACES: &str = "
/* surfaces */
.desk-shell-taskbar {
  background-color: var(--ds-bg);
  border-top: 1px solid var(--ds-stroke);
}
.desk-shell-start-menu,
.desk-shell-flyout {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  color: var(--ds-fg);
}
.desk-shell-taskbar button,
.desk-shell-start-menu .tile,
.desk-shell-start-menu list row,
.desk-shell-flyout button,
.desk-shell-flyout list row { border-radius: var(--ds-radius); }

/* hover and active states */
.desk-shell-taskbar button:hover,
.desk-shell-start-menu .tile:hover,
.desk-shell-start-menu list row:hover,
.desk-shell-flyout button:hover,
.desk-shell-flyout list row:hover { background-color: var(--ds-hover); }
.desk-shell-taskbar button:active,
.desk-shell-taskbar button:checked,
.desk-shell-start-menu .tile:active,
.desk-shell-flyout button:active { background-color: var(--ds-active); }
.desk-shell-taskbar .pin:checked { border-bottom-color: var(--ds-accent); }
.desk-shell-flyout .qs-tile:checked {
  background-color: var(--ds-accent);
  color: var(--ds-on-accent);
  border-color: var(--ds-accent);
}
";

pub fn css(t: &Tokens, theme_id: &str) -> String {
    let mut out = header("/*", "*/", theme_id);
    out.push_str("/* desk-shell: --ds-* palette (GTK 4.16+ custom properties) and surfaces */\n");
    write_scheme(&mut out, t, Scheme::Light);
    write_scheme(&mut out, t, Scheme::Dark);

    let mut root = Vec::new();
    if let Some(r) = t.radii.button {
        root.push(format!("--ds-radius: {r}px;"));
    }
    if let Some(r) = t.radii.popover {
        root.push(format!("--ds-flyout-radius: {r}px;"));
    }
    if !root.is_empty() {
        out.push_str("\n/* radii.button, radii.popover */\n.desk-shell {");
        for l in &root {
            let _ = write!(out, " {l}");
        }
        out.push_str(" }\n");
    }
    out.push_str(SURFACES);
    if let Some(sh) = &t.shadows.popover {
        let _ = writeln!(
            out,
            "\n/* shadows.popover */\n.desk-shell-start-menu,\n.desk-shell-flyout {{ box-shadow: {}px {}px {}px {}px {}; }}",
            sh.x,
            sh.y,
            sh.blur,
            sh.spread,
            Rgba::rgb(0, 0, 0).with_alpha(sh.opacity).css()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_str;

    fn tokens(s: &str) -> Tokens {
        load_str("t", s).unwrap().tokens
    }

    #[test]
    fn empty_tokens_keep_the_builtin_palette() {
        let css = css(&Tokens::default(), "t");
        assert!(
            css.contains("  --ds-bg: rgba(243, 243, 243, 0.922);\n"),
            "{css}"
        );
        assert!(
            css.contains("  --ds-bg: rgba(32, 32, 32, 0.902);\n"),
            "{css}"
        );
        assert!(
            css.contains("@define-color desk_shell_dark_fg #ffffff;"),
            "{css}"
        );
        // no accent colour: desk-shell's per-skin accents stay
        assert!(!css.contains("--ds-accent:"), "{css}");
        let named = super::css(&tokens("[colors]\naccent = \"blue\"\n"), "t");
        assert!(!named.contains("--ds-accent:"), "{named}");
        assert!(!css.contains("--ds-radius:"), "{css}");
        assert!(!css.contains("box-shadow"), "{css}");
    }

    #[test]
    fn tokens_override_palette_radii_and_shadow() {
        let t = tokens(
            "[colors]\nborder_opacity = 0.2\n[colors.dark]\naccent = \"#8f74e6\"\n\
             window_bg = \"#1d1a27\"\n[radii]\nbutton = 8\npopover = 12\n\
             [surface]\npanel_opacity = 0.5\n[shadows.popover]\ny = 4\nblur = 16\nopacity = 0.25\n",
        );
        let css = css(&t, "t");
        assert!(
            css.contains("  --ds-bg: rgba(29, 26, 39, 0.502);\n"),
            "{css}"
        );
        assert!(css.contains("  --ds-accent: #8f74e6;\n"), "{css}");
        assert!(css.contains("  --ds-on-accent: #ffffff;\n"), "{css}");
        assert!(css.contains("@define-color desk_shell_dark_on_accent #ffffff;"));
        assert!(
            css.contains("  --ds-stroke: rgba(255, 255, 255, 0.2);\n"),
            "{css}"
        );
        assert!(css.contains(".desk-shell { --ds-radius: 8px; --ds-flyout-radius: 12px; }"));
        assert!(
            css.contains("box-shadow: 0px 4px 16px 0px rgba(0, 0, 0, 0.251);"),
            "{css}"
        );
        // the light scheme has no accent of its own
        let light = css.split("/* dark palette */").next().unwrap();
        assert!(!light.contains("--ds-accent:"), "{light}");
    }

    #[test]
    fn palette_rules_leave_high_contrast_alone() {
        let css = css(&Tokens::default(), "t");
        assert_eq!(css.matches('{').count(), css.matches('}').count());
        let palettes: Vec<&str> = css.split('}').filter(|b| b.contains("--ds-fg:")).collect();
        assert_eq!(palettes.len(), 2, "{css}");
        for b in palettes {
            assert!(b.contains(":not(.hc) {"), "{b}");
        }
        assert!(css.contains(".desk-shell.light:not(.hc) {"));
        assert!(css.contains(".desk-shell.dark:not(.hc) {"));
    }
}
