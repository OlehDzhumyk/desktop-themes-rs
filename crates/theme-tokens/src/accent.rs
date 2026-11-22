// SPDX-License-Identifier: GPL-3.0-or-later
//! GNOME's named accent colours (`org.gnome.desktop.interface accent-color`, libadwaita 1.6+)
//! and mapping of an arbitrary colour to the nearest one.

use crate::color::Rgba;

/// (name, libadwaita `--accent-<name>` value). Order = GSettings enum order.
pub const ACCENTS: [(&str, Rgba); 9] = [
    ("blue", Rgba::rgb(0x35, 0x84, 0xe4)),
    ("teal", Rgba::rgb(0x21, 0x90, 0xa4)),
    ("green", Rgba::rgb(0x3a, 0x94, 0x4a)),
    ("yellow", Rgba::rgb(0xc8, 0x88, 0x00)),
    ("orange", Rgba::rgb(0xed, 0x5b, 0x00)),
    ("red", Rgba::rgb(0xe6, 0x2d, 0x42)),
    ("pink", Rgba::rgb(0xd5, 0x61, 0x99)),
    ("purple", Rgba::rgb(0x91, 0x41, 0xac)),
    ("slate", Rgba::rgb(0x6f, 0x83, 0x96)),
];

pub fn named(name: &str) -> Option<Rgba> {
    ACCENTS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Below this OKLCH chroma a colour reads as grey: it maps to `slate`.
const GREY_CHROMA: f64 = 0.045;

/// Nearest named accent: greys go to `slate`, everything else to the accent with the closest
/// OKLCH hue (lightness differences do not matter: GNOME derives its own tints).
pub fn nearest(c: Rgba) -> &'static str {
    let lch = c.to_oklab().to_lch();
    if lch.c < GREY_CHROMA {
        return "slate";
    }
    let mut best = ("blue", f64::MAX);
    for (name, v) in ACCENTS.iter().filter(|(n, _)| *n != "slate") {
        let h = v.to_oklab().to_lch().h;
        let d = (lch.h - h).abs();
        let d = d.min(360.0 - d);
        if d < best.1 {
            best = (name, d);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_accents_map_to_themselves() {
        for (n, c) in ACCENTS {
            assert_eq!(nearest(c), n);
        }
    }

    #[test]
    fn nearest_examples() {
        let p = |s| Rgba::parse(s).unwrap();
        assert_eq!(nearest(p("#0a84ff")), "blue");
        assert_eq!(nearest(p("#808080")), "slate");
        assert_eq!(nearest(p("#7a5cd0")), "purple");
        assert_eq!(nearest(p("#ff2d55")), "red");
        assert_eq!(nearest(p("#30d158")), "green");
        assert_eq!(nearest(p("#ffd60a")), "yellow");
        assert_eq!(named("teal"), Some(p("#2190a4")));
        assert_eq!(named("magenta"), None);
    }
}
