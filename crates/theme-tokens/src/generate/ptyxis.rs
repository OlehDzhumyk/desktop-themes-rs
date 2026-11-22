// SPDX-License-Identifier: GPL-3.0-or-later
//! Ptyxis palette (GKeyFile), the format Ptyxis loads from
//! `$XDG_DATA_HOME/org.gnome.Ptyxis/palettes/<id>.palette` (ptyxis-palette.c: `[Palette]`
//! `Name`; `[Light]`/`[Dark]` with `Foreground`, `Background`, `Color0`..`Color15` required and
//! `Cursor` optional; when both scheme groups exist both must be complete). The palette id
//! stored in the profile's `palette` key is the file name without `.palette`.

use std::fmt::Write;

use crate::generate::Target;
use crate::schema::{TermColors, Tokens};

fn face(out: &mut String, group: &str, c: &TermColors) {
    let _ = writeln!(out, "\n[{group}]");
    if let Some(fg) = c.fg {
        let _ = writeln!(out, "Foreground={}", fg.opaque().hex());
    }
    if let Some(bg) = c.bg {
        let _ = writeln!(out, "Background={}", bg.opaque().hex());
    }
    if let Some(cur) = c.cursor {
        let _ = writeln!(out, "Cursor={}", cur.opaque().hex());
    }
    for (i, col) in c.palette.iter().flatten().enumerate() {
        let _ = writeln!(out, "Color{i}={}", col.opaque().hex());
    }
}

/// Keyfile values must be one line; brackets would be read as a group header by humans.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn palette(t: &Tokens, tg: &Target) -> String {
    let term = t.terminal_palette().cloned().unwrap_or_default();
    let name = term
        .name
        .clone()
        .or_else(|| t.name.clone())
        .unwrap_or_else(|| tg.theme_name.clone());
    let mut out = crate::generate::header("#", "", &tg.theme_id);
    let _ = writeln!(out, "[Palette]\nName={}", clean(&name));
    // a scheme the theme leaves out reuses the other one (as Ptyxis' single-scheme palettes do)
    let dark = term
        .dark
        .clone()
        .or_else(|| term.light.clone())
        .unwrap_or_default();
    let light = term.light.clone().unwrap_or_else(|| dark.clone());
    face(&mut out, "Light", &light);
    face(&mut out, "Dark", &dark);
    out
}
