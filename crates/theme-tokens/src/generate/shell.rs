// SPDX-License-Identifier: GPL-3.0-or-later
//! GNOME Shell stylesheet for the user-theme extension.
//!
//! Design: a small override layer on top of the stock stylesheet, imported from the shell's
//! own resource (`resource:///org/gnome/shell/theme/gnome-shell-{dark,light}.css`). Nothing of
//! upstream is copied or patched, so a shell upgrade keeps every widget styled by its own
//! version and the tokens only recolour/reshape a handful of surfaces. St has no CSS
//! variables or media queries, so values are literal and one scheme is chosen
//! (`shell.scheme`, dark by default: the panel sits on arbitrary wallpapers).

use std::fmt::Write;

use crate::color::Rgba;
use crate::generate::{header, num};
use crate::schema::{Accent, Shadow, Tokens};
use crate::validate::{easing_bezier, effective};

fn rule(out: &mut String, sel: &str, decls: &[String]) {
    if decls.is_empty() {
        return;
    }
    let _ = writeln!(out, "{sel} {{");
    for d in decls {
        let _ = writeln!(out, "  {d};");
    }
    out.push_str("}\n");
}

fn shadow(sh: &Shadow) -> String {
    format!(
        "box-shadow: {}px {}px {}px {}px {}",
        sh.x,
        sh.y,
        sh.blur,
        sh.spread,
        Rgba::rgb(0, 0, 0).with_alpha(sh.opacity).css()
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "renders one CSS document from the tokens, section after section"
)]
pub fn css(t: &Tokens, theme_id: &str) -> String {
    let s = t.shell_scheme();
    let c = t.colors(s);
    let sh = t.shell.clone().unwrap_or_default();
    let mut out = header("/*", "*/", theme_id);
    let _ = writeln!(
        out,
        "@import url(\"resource:///org/gnome/shell/theme/gnome-shell-{}.css\");\n",
        s.id()
    );

    let set = |k: &str| -> Option<Rgba> {
        if k == "accent" {
            return match c.accent {
                Some(Accent::Color(a)) => Some(a),
                _ => None,
            };
        }
        c.colors()
            .into_iter()
            .find(|(n, _)| *n == k)
            .and_then(|(_, v)| v)
    };
    let alpha = |col: Rgba, o: Option<f64>| col.with_alpha(col.alpha() * o.unwrap_or(1.0));
    let fast = t.motion.fast_ms.or(t.motion.normal_ms);
    let ease = t.motion.easing.as_deref().and_then(easing_bezier).map(|b| {
        format!(
            "cubic-bezier({}, {}, {}, {})",
            num(b[0]),
            num(b[1]),
            num(b[2]),
            num(b[3])
        )
    });
    let transition = |ms: Option<u32>| -> Vec<String> {
        let mut v = Vec::new();
        if let Some(ms) = ms {
            v.push(format!("transition-duration: {ms}ms"));
        }
        if let Some(e) = &ease {
            v.push(format!("transition-timing-function: {e}"));
        }
        v
    };
    let sp = &t.spacing;
    let r = &t.radii;

    // top bar
    let mut d = Vec::new();
    let panel_bg = sh.panel_bg.or_else(|| {
        t.surface
            .panel_opacity
            .map(|_| crate::adwaita::shell_panel(s).0)
    });
    if let Some(bg) = panel_bg {
        d.push(format!(
            "background-color: {}",
            alpha(bg, t.surface.panel_opacity).css()
        ));
    }
    rule(&mut out, "#panel", &d);
    let mut d = Vec::new();
    if let Some(fg) = sh.panel_fg {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(b) = r.button {
        d.push(format!("border-radius: {b}px"));
    }
    if let Some(md) = sp.md {
        d.push(format!("-natural-hpadding: {md}px"));
    }
    if let Some(sm) = sp.sm {
        d.push(format!("-minimum-hpadding: {sm}px"));
    }
    d.extend(transition(fast));
    rule(&mut out, "#panel .panel-button", &d);

    // menus and popups
    let popover_bg = set("popover_bg");
    let mut d = Vec::new();
    if let Some(bg) = popover_bg {
        d.push(format!(
            "background-color: {}",
            alpha(bg, t.surface.popover_opacity).css()
        ));
    } else if let Some(o) = t.surface.popover_opacity {
        let bg = effective(c, s, "popover_bg").unwrap_or(Rgba::rgb(0x36, 0x36, 0x3a));
        d.push(format!("background-color: {}", alpha(bg, Some(o)).css()));
    }
    if let Some(fg) = set("popover_fg") {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(p) = r.popover {
        d.push(format!("border-radius: {p}px"));
    }
    if let Some(x) = &t.shadows.popover {
        d.push(shadow(x));
    }
    if let Some(sm) = sp.sm {
        d.push(format!("padding: {sm}px"));
    }
    rule(
        &mut out,
        ".popup-menu-content, .candidate-popup-content",
        &d,
    );
    let mut d = Vec::new();
    if let Some(b) = r.button {
        d.push(format!("border-radius: {b}px"));
    }
    d.extend(transition(fast));
    rule(&mut out, ".popup-menu-item", &d);

    // quick settings
    let mut d = Vec::new();
    if let Some(bg) = popover_bg {
        d.push(format!(
            "background-color: {}",
            alpha(bg, t.surface.popover_opacity).css()
        ));
    }
    if let Some(fg) = set("popover_fg") {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(p) = r.panel {
        d.push(format!("border-radius: {p}px"));
    }
    if let Some(lg) = sp.lg {
        d.push(format!("padding: {lg}px"));
    }
    rule(&mut out, ".quick-settings", &d);
    let mut d = Vec::new();
    if let Some(b) = r.button {
        d.push(format!("border-radius: {b}px"));
    }
    d.extend(transition(fast));
    rule(
        &mut out,
        ".quick-toggle, .quick-toggle-has-menu, .quick-toggle-menu-button",
        &d,
    );

    // surfaces: OSD, switchers, dash
    let mut d = Vec::new();
    if let Some(bg) = popover_bg {
        d.push(format!(
            "background-color: {}",
            alpha(bg, t.surface.popover_opacity).css()
        ));
    }
    if let Some(fg) = set("popover_fg") {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(p) = r.panel {
        d.push(format!("border-radius: {p}px"));
    }
    rule(
        &mut out,
        ".osd-window, .switcher-list, .workspace-switcher",
        &d,
    );
    let mut d = Vec::new();
    if let Some(bg) = popover_bg {
        d.push(format!(
            "background-color: {}",
            alpha(bg, t.surface.panel_opacity).css()
        ));
    }
    if let Some(p) = r.panel {
        d.push(format!("border-radius: {p}px"));
    }
    rule(&mut out, ".dash-background", &d);

    // notifications and calendar messages
    let mut d = Vec::new();
    if let Some(bg) = set("card_bg") {
        d.push(format!("background-color: {}", bg.css()));
    }
    if let Some(fg) = set("card_fg") {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(x) = r.card {
        d.push(format!("border-radius: {x}px"));
    }
    if let Some(x) = &t.shadows.card {
        d.push(shadow(x));
    }
    rule(&mut out, ".notification-banner, .message", &d);

    // dialogs
    let mut d = Vec::new();
    if let Some(bg) = set("dialog_bg") {
        d.push(format!("background-color: {}", bg.css()));
    }
    if let Some(fg) = set("dialog_fg") {
        d.push(format!("color: {}", fg.css()));
    }
    if let Some(w) = r.window {
        d.push(format!("border-radius: {w}px"));
    }
    rule(&mut out, ".modal-dialog", &d);

    // exact accent (a named accent is already applied by the shell through accent-color)
    if let Some(a) = set("accent") {
        let fg = c.accent_fg.unwrap_or(Rgba::rgb(255, 255, 255));
        let hover = a.mix(fg, 0.1);
        let active = a.mix(Rgba::rgb(0, 0, 0), 0.1);
        out.push_str("\n/* accent */\n");
        rule(
            &mut out,
            ".quick-toggle:checked, .quick-toggle-has-menu:checked .quick-toggle, .quick-toggle-menu-button:checked,\n\
             .calendar .calendar-day.calendar-today, .button.default, .modal-dialog .modal-dialog-button.default",
            &[
                format!("background-color: {}", a.css()),
                format!("color: {}", fg.css()),
            ],
        );
        rule(
            &mut out,
            ".quick-toggle:checked:hover, .quick-toggle-has-menu:checked .quick-toggle:hover, .button.default:hover",
            &[format!("background-color: {}", hover.css())],
        );
        rule(
            &mut out,
            ".quick-toggle:checked:active, .button.default:active",
            &[format!("background-color: {}", active.css())],
        );
        rule(
            &mut out,
            ".slider",
            &[format!("-barlevel-active-background-color: {}", a.css())],
        );
        // focus rings, including the lock screen password entry
        rule(
            &mut out,
            "StEntry:focus, .search-entry:focus, .unlock-dialog .login-dialog-prompt-entry:focus",
            &[format!("box-shadow: inset 0 0 0 2px {}", a.css())],
        );
    }
    out
}
