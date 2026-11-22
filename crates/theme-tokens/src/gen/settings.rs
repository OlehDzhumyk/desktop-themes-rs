// SPDX-License-Identifier: GPL-3.0-or-later
//! dconf keys derived from the tokens, per module, in `dconf dump` value syntax. The theme
//! manager merges them *under* the theme's own *.dconf files (a key set by hand wins).

use std::collections::BTreeMap;

use crate::gen::{
    Outputs, Target, WALLPAPER_DARK, WALLPAPER_LIGHT, ptyxis_palette_id, shell_theme_name,
};
use crate::schema::Tokens;
use crate::validate::font_size;

/// Module a derived key belongs to (same ids as the manager's modules).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyModule {
    Look,
    Sounds,
    Apps,
}

pub type Keys = BTreeMap<String, String>;

pub const USER_THEME_EXTENSION: &str = "user-theme@gnome-shell-extensions.gcampax.github.com";
/// Placeholder the manager replaces with Ptyxis' default profile UUID.
pub const PTYXIS_PROFILE: &str = "@DEFAULT_PTYXIS_PROFILE@";

/// GVariant string literal.
pub fn gv_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('\'');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\'' => o.push_str("\\'"),
            '\n' => o.push_str("\\n"),
            _ => o.push(c),
        }
    }
    o.push('\'');
    o
}

/// Percent-encodes a path for a file:// URI (unreserved and `/` stay).
pub fn file_uri(path: &str) -> String {
    let mut o = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn expand_home(p: &str, home: &str) -> String {
    let p = p.trim();
    let p = if let Some(rest) = p.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        p.to_string()
    };
    p.replace("${HOME}", home).replace("$HOME", home)
}

/// Pango weight name for a CSS weight.
pub fn weight_name(w: u16) -> &'static str {
    match w {
        100 => "Thin",
        200 => "Ultra-Light",
        300 => "Light",
        500 => "Medium",
        600 => "Semi-Bold",
        700 => "Bold",
        800 => "Ultra-Bold",
        900 => "Heavy",
        _ => "",
    }
}

/// "Inter 11" + 700 → "Inter Bold 11".
pub fn titlebar_font(ui: &str, weight: u16) -> Option<String> {
    let size = font_size(ui)?;
    let family = ui.trim().rsplit_once(char::is_whitespace)?.0.trim();
    let w = weight_name(weight);
    Some(if w.is_empty() {
        format!("{family} {}", crate::gen::num(size))
    } else {
        format!("{family} {w} {}", crate::gen::num(size))
    })
}

const IFACE: &str = "/org/gnome/desktop/interface";

pub fn keys(t: &Tokens, tg: &Target) -> BTreeMap<KeyModule, Keys> {
    let o = Outputs::for_target(t, tg);
    let mut look = Keys::new();
    if let Some(a) = t.accent() {
        look.insert(format!("{IFACE}/accent-color"), gv_str(a.gnome_name()));
    }
    if let Some(cs) = &t.color_scheme {
        look.insert(format!("{IFACE}/color-scheme"), gv_str(cs));
    }
    let f = &t.fonts;
    if let Some(v) = &f.ui {
        look.insert(format!("{IFACE}/font-name"), gv_str(v));
    }
    if let Some(v) = &f.document {
        look.insert(format!("{IFACE}/document-font-name"), gv_str(v));
    }
    if let Some(v) = &f.mono {
        look.insert(format!("{IFACE}/monospace-font-name"), gv_str(v));
    }
    let titlebar = f
        .titlebar
        .clone()
        .or_else(|| f.ui.as_deref().zip(f.title_weight).and_then(|(u, w)| titlebar_font(u, w)));
    if let Some(v) = titlebar {
        look.insert("/org/gnome/desktop/wm/preferences/titlebar-font".into(), gv_str(&v));
    }
    if o.shell {
        look.insert(
            "/org/gnome/shell/extensions/user-theme/name".into(),
            gv_str(&shell_theme_name(&tg.theme_id)),
        );
    }
    let wall = t.wallpaper.as_ref();
    let pics: Option<(String, String)> = if let Some(img) = wall.and_then(|w| w.image.as_deref()) {
        let p = expand_home(img, &tg.home);
        Some((file_uri(&p), file_uri(&p)))
    } else if o.wallpaper {
        let d = tg.cache_dir.to_string_lossy();
        Some((
            file_uri(&format!("{d}/{WALLPAPER_LIGHT}")),
            file_uri(&format!("{d}/{WALLPAPER_DARK}")),
        ))
    } else {
        None
    };
    if let Some((l, d)) = pics {
        look.insert("/org/gnome/desktop/background/picture-uri".into(), gv_str(&l));
        look.insert("/org/gnome/desktop/background/picture-uri-dark".into(), gv_str(&d));
        look.insert("/org/gnome/desktop/background/picture-options".into(), gv_str("zoom"));
        look.insert("/org/gnome/desktop/screensaver/picture-uri".into(), gv_str(&d));
    }

    let mut sounds = Keys::new();
    if let Some(s) = &t.sounds.theme {
        sounds.insert("/org/gnome/desktop/sound/theme-name".into(), gv_str(s));
    }

    let mut apps = Keys::new();
    if let Some(term) = &t.terminal {
        if let Some(font) = &term.font {
            apps.insert("/org/gnome/Ptyxis/use-system-font".into(), "false".into());
            apps.insert("/org/gnome/Ptyxis/font-name".into(), gv_str(font));
        }
        let id = term
            .builtin
            .clone()
            .or_else(|| o.ptyxis.then(|| ptyxis_palette_id(&tg.theme_id)));
        if let Some(id) = id {
            apps.insert(
                format!("/org/gnome/Ptyxis/Profiles/{PTYXIS_PROFILE}/palette"),
                gv_str(&id),
            );
        }
    }

    let mut m = BTreeMap::new();
    for (k, v) in [(KeyModule::Look, look), (KeyModule::Sounds, sounds), (KeyModule::Apps, apps)] {
        if !v.is_empty() {
            m.insert(k, v);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(gv_str("it's"), "'it\\'s'");
        assert_eq!(file_uri("/home/a b/x.svg"), "file:///home/a%20b/x.svg");
        assert_eq!(titlebar_font("Inter 11", 700).as_deref(), Some("Inter Bold 11"));
        assert_eq!(titlebar_font("Adwaita Sans 10.5", 400).as_deref(), Some("Adwaita Sans 10.5"));
        assert_eq!(titlebar_font("Inter", 700), None);
    }
}
