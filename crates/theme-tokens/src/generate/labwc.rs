// SPDX-License-Identifier: GPL-3.0-or-later
//! labwc window decorations: an Openbox-style `themerc` (keys from labwc-theme(5)), installed as
//! `~/.local/share/themes/<name>/openbox-3/themerc`.
//!
//! labwc themes have one scheme, so one colour set is used: the shell scheme (`shell.scheme`,
//! dark by default), like the GNOME Shell layer. Unset colours fall back to libadwaita's values,
//! so a theme with only a GNOME accent name still gets a complete file. Colours are written
//! opaque (translucent tokens are composited on the window background), except the shadow and
//! the snap overlay, which labwc draws with alpha. `titlebar.height` is not written: labwc 0.20
//! deprecates it and derives the height from the font and the titlebar padding. The corner
//! radius lives in labwc's `rc.xml` (`<cornerRadius>`), not in the theme.

use std::fmt::Write;

use crate::color::Rgba;
use crate::generate::header;
use crate::schema::{Accent, Scheme, Tokens};
use crate::validate::effective;

/// Opaque `col` on `under`: the decorations stay solid.
fn solid(col: Rgba, under: Rgba) -> Rgba {
    col.over(under.opaque()).opaque()
}

fn line(out: &mut String, key: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{key}: {value}");
}

/// The colours of the themerc, resolved from one scheme of the tokens.
struct Palette {
    title_bg: Rgba,
    title_bg_inactive: Rgba,
    title_fg: Rgba,
    title_fg_inactive: Rgba,
    border: Rgba,
    border_inactive: Rgba,
    close_bg: Rgba,
    close_fg: Rgba,
    menu_bg: Rgba,
    menu_fg: Rgba,
    menu_selected_bg: Rgba,
    line: Rgba,
    accent: Rgba,
}

fn palette(t: &Tokens, s: Scheme) -> Palette {
    let c = t.colors(s);
    let under = if s == Scheme::Light {
        Rgba::rgb(255, 255, 255)
    } else {
        Rgba::rgb(0, 0, 0)
    };
    let eff = |k: &str| effective(c, s, k).unwrap_or(under);
    let window_bg = solid(eff("window_bg"), under);
    let title_bg = solid(eff("headerbar_bg"), window_bg);
    let title_bg_inactive = solid(c.headerbar_backdrop.unwrap_or(window_bg), window_bg);
    let fg = eff("headerbar_fg");
    let title_fg = solid(fg, title_bg);
    let title_fg_inactive = solid(fg.with_alpha(fg.alpha() * 0.6), title_bg_inactive);
    // libadwaita draws borders with currentColor at --border-opacity (0.15 by default)
    let opacity = c.border_opacity.unwrap_or(0.15);
    let border = c.headerbar_border.map_or_else(
        || solid(fg.opaque().with_alpha(opacity), title_bg),
        |b| solid(b, title_bg),
    );
    let border_inactive = solid(fg.opaque().with_alpha(opacity * 0.7), title_bg_inactive);
    let menu_bg = solid(eff("popover_bg"), window_bg);
    let menu_fg = solid(eff("popover_fg"), menu_bg);
    let accent = match c.accent {
        Some(Accent::Color(a)) => a,
        Some(a @ Accent::Named(_)) => a.rgba(),
        None => eff("accent"),
    };
    Palette {
        title_bg,
        title_bg_inactive,
        title_fg,
        title_fg_inactive,
        border,
        border_inactive,
        close_bg: solid(eff("destructive_bg"), title_bg),
        close_fg: solid(eff("destructive_fg"), title_bg),
        menu_bg,
        menu_fg,
        menu_selected_bg: solid(menu_fg.with_alpha(0.1), menu_bg),
        line: solid(menu_fg.with_alpha(opacity), menu_bg),
        accent: solid(accent, window_bg),
    }
}

pub fn themerc(t: &Tokens, theme_id: &str) -> String {
    let s = t.shell_scheme();
    let p = palette(t, s);
    let sp = &t.spacing;
    let mut out = header("#", "", theme_id);
    let _ = writeln!(
        out,
        "# labwc theme (labwc-theme(5)), {} scheme. The corner radius is set in rc.xml.",
        s.id()
    );

    out.push_str("\n# --- geometry ---\n");
    line(&mut out, "border.width", 1);
    line(&mut out, "padding.width", sp.sm.unwrap_or(8));
    line(
        &mut out,
        "window.titlebar.padding.width",
        sp.xs.unwrap_or(0),
    );
    line(
        &mut out,
        "window.titlebar.padding.height",
        sp.xs.unwrap_or(0),
    );
    line(&mut out, "window.button.spacing", sp.xs.unwrap_or(0));
    if let Some(r) = t.radii.button {
        line(&mut out, "window.button.hover.bg.shape", "rectangle");
        line(&mut out, "window.button.hover.bg.corner-radius", r);
    }

    out.push_str("\n# --- active window ---\n");
    line(&mut out, "window.active.border.color", p.border.hex());
    line(&mut out, "window.active.title.bg.color", p.title_bg.hex());
    line(&mut out, "window.active.label.text.color", p.title_fg.hex());
    line(
        &mut out,
        "window.active.button.unpressed.image.color",
        p.title_fg.hex(),
    );
    line(
        &mut out,
        "window.active.button.hover.image.color",
        p.title_fg.hex(),
    );

    out.push_str("\n# --- inactive window ---\n");
    line(
        &mut out,
        "window.inactive.border.color",
        p.border_inactive.hex(),
    );
    line(
        &mut out,
        "window.inactive.title.bg.color",
        p.title_bg_inactive.hex(),
    );
    line(
        &mut out,
        "window.inactive.label.text.color",
        p.title_fg_inactive.hex(),
    );
    line(
        &mut out,
        "window.inactive.button.unpressed.image.color",
        p.title_fg_inactive.hex(),
    );

    out.push_str("\n# --- close button: red on hover ---\n");
    line(
        &mut out,
        "window.active.button.close.unpressed.image.color",
        p.title_fg.hex(),
    );
    line(
        &mut out,
        "window.inactive.button.close.unpressed.image.color",
        p.title_fg_inactive.hex(),
    );
    line(
        &mut out,
        "window.active.button.close.hover.image.color",
        p.close_fg.hex(),
    );
    line(
        &mut out,
        "window.active.button.close.hover.bg.color",
        p.close_bg.hex(),
    );

    if let Some(sh) = &t.shadows.popover {
        let shade = effective(t.colors(s), s, "shade")
            .unwrap_or(Rgba::rgb(0, 0, 0))
            .opaque();
        out.push_str("\n# --- shadows (shadows.popover) ---\n");
        line(&mut out, "window.active.shadow.size", sh.blur);
        line(&mut out, "window.inactive.shadow.size", sh.blur / 2);
        line(
            &mut out,
            "window.active.shadow.color",
            shade.with_alpha(sh.opacity).hex(),
        );
        line(
            &mut out,
            "window.inactive.shadow.color",
            shade.with_alpha(sh.opacity * 0.6).hex(),
        );
    }

    out.push_str("\n# --- menus (window menu, root menu) ---\n");
    line(&mut out, "menu.border.width", 1);
    line(&mut out, "menu.border.color", p.line.hex());
    line(&mut out, "menu.items.bg.color", p.menu_bg.hex());
    line(&mut out, "menu.items.text.color", p.menu_fg.hex());
    line(
        &mut out,
        "menu.items.active.bg.color",
        p.menu_selected_bg.hex(),
    );
    line(&mut out, "menu.items.active.text.color", p.menu_fg.hex());
    line(&mut out, "menu.items.padding.x", sp.md.unwrap_or(12));
    line(&mut out, "menu.items.padding.y", sp.sm.unwrap_or(6));
    line(&mut out, "menu.separator.color", p.line.hex());
    line(&mut out, "menu.title.bg.color", p.menu_bg.hex());
    line(&mut out, "menu.title.text.color", p.menu_fg.hex());

    out.push_str("\n# --- on-screen display (window switcher) ---\n");
    line(&mut out, "osd.bg.color", p.menu_bg.hex());
    line(&mut out, "osd.border.color", p.line.hex());
    line(&mut out, "osd.border.width", 1);
    line(&mut out, "osd.label.text.color", p.menu_fg.hex());
    line(
        &mut out,
        "osd.window-switcher.item.active.bg.color",
        p.menu_selected_bg.hex(),
    );
    line(
        &mut out,
        "osd.window-switcher.item.active.border.color",
        p.accent.hex(),
    );
    line(&mut out, "osd.window-switcher.item.active.border.width", 2);

    out.push_str("\n# --- snap overlay ---\n");
    let tint = p.accent.with_alpha(0.2).hex();
    line(&mut out, "snapping.overlay.region.bg.color", &tint);
    line(&mut out, "snapping.overlay.edge.bg.color", &tint);
    line(
        &mut out,
        "snapping.overlay.region.border.color",
        p.accent.hex(),
    );
    line(
        &mut out,
        "snapping.overlay.edge.border.color",
        p.accent.hex(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_str;

    fn tokens(s: &str) -> Tokens {
        load_str("t", s).unwrap().tokens
    }

    fn value<'a>(rc: &'a str, key: &str) -> Option<&'a str> {
        rc.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
    }

    #[test]
    fn empty_tokens_give_a_complete_dark_file() {
        let rc = themerc(&Tokens::default(), "t");
        // libadwaita dark defaults
        assert_eq!(value(&rc, "window.active.title.bg.color"), Some("#2e2e32"));
        assert_eq!(
            value(&rc, "window.active.label.text.color"),
            Some("#ffffff")
        );
        assert_eq!(
            value(&rc, "window.active.button.close.hover.bg.color"),
            Some("#c01c28")
        );
        assert_eq!(
            value(&rc, "osd.window-switcher.item.active.border.color"),
            Some("#3584e4")
        );
        assert!(!rc.contains("titlebar.height"), "{rc}");
        assert!(!rc.contains("shadow"), "{rc}");
        // every line is a comment, blank, or `key: value`
        for l in rc.lines() {
            assert!(
                l.is_empty() || l.starts_with('#') || l.contains(": "),
                "{l}"
            );
        }
    }

    #[test]
    fn tokens_and_shell_scheme_drive_the_colours() {
        let t = tokens(
            "[colors.light]\nheaderbar_bg = \"#eeeeee\"\n[colors.dark]\nheaderbar_bg = \"#101010\"\n\
             accent = \"#aa3355\"\n[spacing]\nsm = 4\nmd = 10\n[radii]\nbutton = 6\n",
        );
        let rc = themerc(&t, "t");
        assert_eq!(value(&rc, "window.active.title.bg.color"), Some("#101010"));
        assert_eq!(
            value(&rc, "snapping.overlay.edge.border.color"),
            Some("#aa3355")
        );
        assert_eq!(
            value(&rc, "snapping.overlay.edge.bg.color"),
            Some("#aa335533")
        );
        assert_eq!(value(&rc, "padding.width"), Some("4"));
        assert_eq!(value(&rc, "menu.items.padding.x"), Some("10"));
        assert_eq!(
            value(&rc, "window.button.hover.bg.corner-radius"),
            Some("6")
        );

        let mut light = t.clone();
        light.shell = Some(crate::schema::Shell {
            scheme: Some("light".into()),
            ..Default::default()
        });
        let rc = themerc(&light, "t");
        assert_eq!(value(&rc, "window.active.title.bg.color"), Some("#eeeeee"));
    }

    #[test]
    fn translucent_colours_are_made_opaque() {
        let t = tokens("[colors.dark]\npopover_bg = \"#ffffff20\"\nwindow_bg = \"#000000\"\n");
        let rc = themerc(&t, "t");
        assert_eq!(value(&rc, "menu.items.bg.color"), Some("#202020"));
    }
}
