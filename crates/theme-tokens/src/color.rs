// SPDX-License-Identifier: GPL-3.0-or-later
//! sRGB colours with alpha: parsing, formatting, WCAG 2.x contrast, OKLab/OKLCH.

use std::fmt;

/// A channel value: rounded, and held in 0..=255 (so the cast cannot lose anything).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 right before the cast"
)]
fn to_u8(x: f64) -> u8 {
    x.round().clamp(0.0, 255.0) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }
    #[allow(clippy::self_named_constructors)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Result<Rgba, String> {
        let t = s.trim();
        let Some(h) = t.strip_prefix('#') else {
            return Err(format!(
                "«{s}» is not a colour (expected #rrggbb or #rrggbbaa)"
            ));
        };
        let b = h.as_bytes();
        let d: Option<Vec<u8>> = b.iter().map(|c| hexval(*c)).collect();
        let Some(d) = d else {
            return Err(format!("«{s}» contains a non-hex digit"));
        };
        let v = match d.len() {
            3 => Rgba::rgb(d[0] * 17, d[1] * 17, d[2] * 17),
            4 => Rgba::rgba(d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17),
            6 => Rgba::rgb(d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]),
            8 => Rgba::rgba(
                d[0] * 16 + d[1],
                d[2] * 16 + d[3],
                d[4] * 16 + d[5],
                d[6] * 16 + d[7],
            ),
            _ => {
                return Err(format!(
                    "«{s}» has {} hex digits (expected 3, 4, 6 or 8)",
                    d.len()
                ));
            }
        };
        Ok(v)
    }

    /// `#rrggbb` (opaque) or `#rrggbbaa`.
    pub fn hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// CSS form understood by both GTK and St: `#rrggbb` or `rgba(r, g, b, a)`.
    pub fn css(&self) -> String {
        if self.a == 255 {
            self.hex()
        } else {
            format!(
                "rgba({}, {}, {}, {})",
                self.r,
                self.g,
                self.b,
                fmt_alpha(self.alpha())
            )
        }
    }

    pub fn alpha(&self) -> f64 {
        self.a as f64 / 255.0
    }

    pub fn opaque(&self) -> Rgba {
        Rgba { a: 255, ..*self }
    }

    pub fn with_alpha(&self, a: f64) -> Rgba {
        Rgba {
            a: to_u8(a.clamp(0.0, 1.0) * 255.0),
            ..*self
        }
    }

    /// Source-over compositing of `self` on top of `bg` (result has bg's coverage combined).
    pub fn over(&self, bg: Rgba) -> Rgba {
        let fa = self.alpha();
        let ba = bg.alpha();
        let oa = fa + ba * (1.0 - fa);
        if oa <= 0.0 {
            return Rgba::rgba(0, 0, 0, 0);
        }
        let ch = |f: u8, b: u8| -> u8 { to_u8((f as f64 * fa + b as f64 * ba * (1.0 - fa)) / oa) };
        Rgba {
            r: ch(self.r, bg.r),
            g: ch(self.g, bg.g),
            b: ch(self.b, bg.b),
            a: to_u8(oa * 255.0),
        }
    }

    /// Linear interpolation in sRGB (t = 0 → self, 1 → other).
    pub fn mix(&self, other: Rgba, t: f64) -> Rgba {
        let l = |a: u8, b: u8| to_u8(a as f64 + (b as f64 - a as f64) * t);
        Rgba {
            r: l(self.r, other.r),
            g: l(self.g, other.g),
            b: l(self.b, other.b),
            a: l(self.a, other.a),
        }
    }

    /// WCAG 2.x relative luminance of the opaque colour.
    pub fn luminance(&self) -> f64 {
        let f = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(self.r) + 0.7152 * f(self.g) + 0.0722 * f(self.b)
    }

    pub fn is_dark(&self) -> bool {
        self.luminance() < 0.18
    }

    pub fn to_oklab(&self) -> Oklab {
        let f = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (f(self.r), f(self.g), f(self.b));
        let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
        let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
        Oklab {
            l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        }
    }
}

/// Contrast ratio (1..21) of `fg` drawn on `bg`; a translucent `fg` is composited on `bg`
/// first, a translucent `bg` is composited on `base` (the surface underneath).
pub fn contrast_on(fg: Rgba, bg: Rgba, base: Rgba) -> f64 {
    let bg = bg.over(base.opaque());
    let fg = fg.over(bg);
    let (a, b) = (fg.luminance(), bg.luminance());
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// Contrast of two opaque (or composited-on-each-other) colours.
pub fn contrast(fg: Rgba, bg: Rgba) -> f64 {
    contrast_on(fg, bg, bg.opaque())
}

fn fmt_alpha(a: f64) -> String {
    let s = format!("{a:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    /// degrees, 0..360
    pub h: f64,
}

impl Oklab {
    /// Linear sRGB components (may be out of 0..1 when out of gamut).
    fn to_linear(self) -> (f64, f64, f64) {
        let l = self.l + 0.3963377774 * self.a + 0.2158037573 * self.b;
        let m = self.l - 0.1055613458 * self.a - 0.0638541728 * self.b;
        let s = self.l - 0.0894841775 * self.a - 1.2914855480 * self.b;
        let (l, m, s) = (l * l * l, m * m * m, s * s * s);
        (
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        )
    }

    pub fn in_gamut(self) -> bool {
        let (r, g, b) = self.to_linear();
        let ok = |c: f64| (-1e-4..=1.0 + 1e-4).contains(&c);
        ok(r) && ok(g) && ok(b)
    }

    /// Clipped conversion to sRGB.
    pub fn to_rgb(self) -> Rgba {
        let (r, g, b) = self.to_linear();
        let f = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            let v = if c <= 0.0031308 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            to_u8(v * 255.0)
        };
        Rgba::rgb(f(r), f(g), f(b))
    }

    pub fn to_lch(self) -> Oklch {
        let c = (self.a * self.a + self.b * self.b).sqrt();
        let mut h = self.b.atan2(self.a).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        Oklch { l: self.l, c, h }
    }

    pub fn dist2(self, o: Oklab) -> f64 {
        let (dl, da, db) = (self.l - o.l, self.a - o.a, self.b - o.b);
        dl * dl + da * da + db * db
    }
}

impl Oklch {
    pub fn new(l: f64, c: f64, h: f64) -> Oklch {
        Oklch {
            l,
            c,
            h: h.rem_euclid(360.0),
        }
    }

    pub fn to_lab(self) -> Oklab {
        let hr = self.h.to_radians();
        Oklab {
            l: self.l,
            a: self.c * hr.cos(),
            b: self.c * hr.sin(),
        }
    }

    /// sRGB with chroma reduced (binary search) until the colour fits the gamut; hue and
    /// lightness are kept.
    pub fn to_rgb_mapped(self) -> Rgba {
        let l = self.l.clamp(0.0, 1.0);
        let probe = Oklch { l, ..self };
        if probe.to_lab().in_gamut() {
            return probe.to_lab().to_rgb();
        }
        let (mut lo, mut hi) = (0.0, self.c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if (Oklch {
                l,
                c: mid,
                h: self.h,
            })
            .to_lab()
            .in_gamut()
            {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Oklch {
            l,
            c: lo,
            h: self.h,
        }
        .to_lab()
        .to_rgb()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        assert_eq!(Rgba::parse("#fff").unwrap(), WHITE);
        assert_eq!(Rgba::parse("#3584e4").unwrap(), Rgba::rgb(0x35, 0x84, 0xe4));
        assert_eq!(Rgba::parse("#000000cc").unwrap().a, 0xcc);
        assert_eq!(Rgba::parse("#000c").unwrap().a, 0xcc);
        assert!(Rgba::parse("3584e4").is_err());
        assert!(Rgba::parse("#12345").is_err());
        assert!(Rgba::parse("#gg0000").is_err());
        assert_eq!(Rgba::rgb(1, 2, 3).hex(), "#010203");
        assert_eq!(Rgba::rgba(0, 0, 6, 204).css(), "rgba(0, 0, 6, 0.8)");
    }

    #[test]
    fn wcag_contrast() {
        assert!((contrast(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(WHITE, WHITE) - 1.0).abs() < 1e-9);
        // GNOME blue with white text: ~3.8:1 (known value)
        let c = contrast(WHITE, Rgba::parse("#3584e4").unwrap());
        assert!((3.7..3.9).contains(&c), "{c}");
        // translucent text is composited
        let fg = Rgba::rgba(0, 0, 6, 204);
        let c = contrast(fg, Rgba::parse("#fafafb").unwrap());
        assert!(c > 10.0 && c < 21.0, "{c}");
    }

    #[test]
    fn oklab_roundtrip() {
        for hex in [
            "#3584e4", "#e01b24", "#2ec27e", "#ffffff", "#000000", "#9141ac",
        ] {
            let c = Rgba::parse(hex).unwrap();
            let back = c.to_oklab().to_rgb();
            assert_eq!(c, back, "{hex}");
            let lch = c.to_oklab().to_lch();
            assert_eq!(Oklch::new(lch.l, lch.c, lch.h).to_rgb_mapped(), c, "{hex}");
        }
        // out of gamut gets mapped, not clipped to a different hue family
        let v = Oklch::new(0.6, 0.5, 30.0).to_rgb_mapped();
        assert!(v.r > v.g && v.r > v.b);
    }
}
