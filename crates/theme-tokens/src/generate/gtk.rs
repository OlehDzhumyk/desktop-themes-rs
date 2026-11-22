// SPDX-License-Identifier: GPL-3.0-or-later
//! libadwaita 1.10 stylesheet (`~/.config/gtk-4.0/gtk.css`).
//!
//! libadwaita 1.6+ exposes its named colours as CSS custom properties on `:root`
//! (`--window-bg-color`, `--accent-bg-color`, ...) and switches them with
//! `@media (prefers-color-scheme: dark)`; `@define-color` names still exist for older custom
//! CSS. The user stylesheet is loaded by GTK at USER priority (above the theme), so it only
//! has to redefine what the tokens set. Light values go into
//! `@media (prefers-color-scheme: light)`: a plain `:root` would also win in dark mode.

use std::fmt::Write;

use crate::color::Rgba;
use crate::generate::{header, num};
use crate::schema::{Accent, ColorSet, Scheme, Shadow, Tokens};
use crate::validate::{easing_bezier, effective};

/// token key → (CSS variable, legacy @define-color name)
fn names(key: &str) -> (String, String) {
    let base = if key == "accent" { "accent_bg" } else { key };
    (
        format!("--{}-color", base.replace('_', "-")),
        format!("{base}_color"),
    )
}

type Decls = Vec<(String, String)>;

/// (CSS variables, legacy @define-color) of one scheme.
fn scheme_block(c: &ColorSet) -> (Decls, Decls) {
    let mut vars = Vec::new();
    let mut defs = Vec::new();
    if let Some(Accent::Color(a)) = c.accent {
        let (v, d) = names("accent");
        vars.push((v, a.css()));
        defs.push((d, a.css()));
    }
    for (k, v) in c.colors() {
        if let Some(v) = v {
            let (var, def) = names(k);
            vars.push((var, v.css()));
            defs.push((def, v.css()));
        }
    }
    if let Some(o) = c.border_opacity {
        vars.push(("--border-opacity".into(), format!("{}%", num(o * 100.0))));
    }
    (vars, defs)
}

fn write_scheme(out: &mut String, t: &Tokens, s: Scheme) {
    let c = t.colors(s);
    let (vars, defs) = scheme_block(c);
    let shadows = shadow_rules(t, s);
    if vars.is_empty() && shadows.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n@media (prefers-color-scheme: {}) {{", s.id());
    if !vars.is_empty() {
        out.push_str("  :root {\n");
        for (k, v) in &vars {
            let _ = writeln!(out, "    {k}: {v};");
        }
        out.push_str("  }\n");
        for (k, v) in &defs {
            let _ = writeln!(out, "  @define-color {k} {v};");
        }
    }
    for r in shadows {
        let _ = writeln!(out, "  {r}");
    }
    out.push_str("}\n");
}

fn shadow_css(sh: &Shadow, shade: Rgba) -> String {
    let base = shade.opaque();
    format!(
        "0 0 0 1px {}, {}px {}px {}px {}px {}",
        base.with_alpha((sh.opacity * 0.5).min(0.12)).css(),
        sh.x,
        sh.y,
        sh.blur,
        sh.spread,
        base.with_alpha(sh.opacity).css()
    )
}

fn shadow_rules(t: &Tokens, s: Scheme) -> Vec<String> {
    let shade = effective(t.colors(s), s, "shade").unwrap_or(Rgba::rgba(0, 0, 6, 18));
    let mut v = Vec::new();
    if let Some(sh) = &t.shadows.card {
        v.push(format!(
            ".card, list.boxed-list, list.boxed-list-separate > row {{ box-shadow: {}; }}",
            shadow_css(sh, shade)
        ));
    }
    if let Some(sh) = &t.shadows.popover {
        v.push(format!(
            "popover > contents {{ box-shadow: {}; }}",
            shadow_css(sh, shade)
        ));
    }
    v
}

const BUTTONS: &str = "button:not(.circular):not(.pill):not(.sidebar-button):not(.color),\nentry,\nspinbutton,\nsplitbutton,\ntoggle-group";

fn button_radius(out: &mut String, r: u32) {
    let _ = writeln!(
        out,
        "\n/* radii.button */\n{BUTTONS} {{ border-radius: {r}px; }}"
    );
    // buttons that must stay round or square inside composite widgets
    out.push_str("windowcontrols > button { border-radius: 9999px; }\n");
    out.push_str("spinbutton > button { border-radius: 0; }\n");
    // linked groups: inner corners stay square (same selectors as libadwaita)
    for el in [
        "button",
        "menubutton > button",
        "dropdown > button",
        "combobox > box > button",
        "entry",
    ] {
        let (head, tail) = el.split_once(" > ").map_or((el, ""), |(h, t)| (h, t));
        let sel = |pseudo: &str| {
            if tail.is_empty() {
                format!("{head}{pseudo}")
            } else {
                format!("{head}{pseudo} > {tail}")
            }
        };
        let _ = writeln!(
            out,
            ".linked:not(.vertical) > {} {{ border-top-left-radius: 0; border-bottom-left-radius: 0; }}",
            sel(":not(:first-child)")
        );
        let _ = writeln!(
            out,
            ".linked:not(.vertical) > {} {{ border-top-right-radius: 0; border-bottom-right-radius: 0; }}",
            sel(":not(:last-child)")
        );
        let _ = writeln!(
            out,
            ".linked.vertical > {} {{ border-top-left-radius: 0; border-top-right-radius: 0; }}",
            sel(":not(:first-child)")
        );
        let _ = writeln!(
            out,
            ".linked.vertical > {} {{ border-bottom-left-radius: 0; border-bottom-right-radius: 0; }}",
            sel(":not(:last-child)")
        );
    }
    out.push_str(
        "splitbutton > button { border-top-right-radius: 0; border-bottom-right-radius: 0; }\n",
    );
    out.push_str(
        "splitbutton > menubutton > button { border-top-left-radius: 0; border-bottom-left-radius: 0; }\n",
    );
    let _ = writeln!(
        out,
        "popover.menu modelbutton, popover.menu list > row, popover.menu listview > row,\n\
         .navigation-sidebar > row, .navigation-sidebar > child, tooltip, tabbar tab {{ border-radius: {r}px; }}"
    );
}

pub fn css(t: &Tokens, theme_id: &str) -> String {
    let mut out = header("/*", "*/", theme_id);
    out.push_str("/* libadwaita 1.10: CSS variables, light/dark via prefers-color-scheme */\n");

    let mut root = Vec::new();
    if let Some(r) = t.radii.window {
        root.push(format!("--window-radius: {r}px;"));
    }
    if !root.is_empty() {
        out.push_str("\n:root {\n");
        for l in &root {
            let _ = writeln!(out, "  {l}");
        }
        out.push_str("}\n");
    }
    write_scheme(&mut out, t, Scheme::Light);
    write_scheme(&mut out, t, Scheme::Dark);

    if let Some(r) = t.radii.button {
        button_radius(&mut out, r);
    }
    if let Some(r) = t.radii.card {
        let _ = writeln!(
            out,
            "\n/* radii.card */\nlist.boxed-list, list.content, list.boxed-list-separate > row, .card, frame,\n\
             menubutton.card > button {{ border-radius: {r}px; }}\n\
             list.boxed-list > row:first-child, list.content > row:first-child {{ border-top-left-radius: {r}px; border-top-right-radius: {r}px; }}\n\
             list.boxed-list > row:last-child, list.content > row:last-child {{ border-bottom-left-radius: {r}px; border-bottom-right-radius: {r}px; }}"
        );
    }
    if let Some(r) = t.radii.popover {
        let _ = writeln!(
            out,
            "\n/* radii.popover */\npopover > contents, popover.menu > contents {{ border-radius: {r}px; }}"
        );
    }
    if let Some(w) = t.fonts.title_weight {
        let _ = writeln!(
            out,
            "\n/* fonts.title_weight */\nwindowtitle > .title, headerbar .title {{ font-weight: {w}; }}"
        );
    }
    let m = &t.motion;
    let ease = m.easing.as_deref().and_then(easing_bezier).map(|b| {
        format!(
            "cubic-bezier({}, {}, {}, {})",
            num(b[0]),
            num(b[1]),
            num(b[2]),
            num(b[3])
        )
    });
    if m.fast_ms.is_some() || m.normal_ms.is_some() || ease.is_some() {
        out.push_str("\n/* motion */\n");
        let rule = |sel: &str, ms: Option<u32>| {
            let mut decl = String::new();
            if let Some(ms) = ms {
                let _ = write!(decl, " transition-duration: {ms}ms;");
            }
            if let Some(e) = &ease {
                let _ = write!(decl, " transition-timing-function: {e};");
            }
            if decl.is_empty() {
                String::new()
            } else {
                format!("{sel} {{{decl} }}\n")
            }
        };
        out.push_str(&rule(
            "button, modelbutton, row, tab",
            m.fast_ms.or(m.normal_ms),
        ));
        out.push_str(&rule(
            "switch, switch > slider, checkbutton > check, checkbutton > radio, entry, stackswitcher > button",
            m.normal_ms.or(m.fast_ms),
        ));
    }
    out
}
