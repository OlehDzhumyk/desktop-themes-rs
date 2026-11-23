// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure-Rust colour formatting, W3C CSS Color 4 named colour lookup, and picker format cycling
//! for the Stage 3 Colour Picker (`docs/spec/features/color-picker.md`).

use crate::color::{self, Rgba};

/// Output format for picked colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Hex,
    Rgb,
    Hsl,
    Hsv,
    Name,
    Gtk,
    Decimal,
}

impl Format {
    /// Parse a format name from settings (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hex" => Some(Self::Hex),
            "rgb" => Some(Self::Rgb),
            "hsl" => Some(Self::Hsl),
            "hsv" => Some(Self::Hsv),
            "name" => Some(Self::Name),
            "gtk" => Some(Self::Gtk),
            "decimal" => Some(Self::Decimal),
            _ => None,
        }
    }

    /// Canonical schema string for this format.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Rgb => "rgb",
            Self::Hsl => "hsl",
            Self::Hsv => "hsv",
            Self::Name => "name",
            Self::Gtk => "gtk",
            Self::Decimal => "decimal",
        }
    }

    /// Next format in the cycle (for the `F` key while picking).
    pub const fn next(self) -> Self {
        match self {
            Self::Hex => Self::Rgb,
            Self::Rgb => Self::Hsl,
            Self::Hsl => Self::Hsv,
            Self::Hsv => Self::Name,
            Self::Name => Self::Gtk,
            Self::Gtk => Self::Decimal,
            Self::Decimal => Self::Hex,
        }
    }
}

/// Format an opaque sRGB colour in the requested picker format.
pub fn format_color(c: Rgba, fmt: Format) -> String {
    match fmt {
        Format::Hex => format_hex(c),
        Format::Rgb => format!("rgb({}, {}, {})", c.r, c.g, c.b),
        Format::Hsl => {
            let (h, s, l) = rgb_to_hsl(c);
            format!("hsl({h}, {s}%, {l}%)")
        }
        Format::Hsv => {
            let (h, s, v) = rgb_to_hsv(c);
            format!("hsv({h}, {s}%, {v}%)")
        }
        Format::Name => exact_css_name(c).map_or_else(|| format_hex(c), str::to_string),
        Format::Gtk => format!("@define-color picked {};", format_hex(c)),
        Format::Decimal => format!("{},{},{}", c.r, c.g, c.b),
    }
}

/// Exact W3C CSS Color 4 named colour (lowercase) for `c`, if any.
pub fn exact_css_name(c: Rgba) -> Option<&'static str> {
    for &(name, r, g, b) in CSS_COLORS {
        if r == c.r && g == c.g && b == c.b {
            return Some(name);
        }
    }
    None
}

/// Nearest W3C CSS Color 4 named colour to `c` in OKLab perceptual space.
///
/// Used for screen-reader accessible swatch names.
pub fn nearest_css_name(c: Rgba) -> &'static str {
    if let Some(name) = exact_css_name(c) {
        return name;
    }
    let target = c.to_oklab();
    let mut best_name = CSS_COLORS[0].0;
    let mut best_dist = f64::INFINITY;
    for &(name, r, g, b) in CSS_COLORS {
        let dist = target.dist2(Rgba::rgb(r, g, b).to_oklab());
        if dist < best_dist {
            best_dist = dist;
            best_name = name;
        }
    }
    best_name
}

/// WCAG 2.x contrast ratio (`1.0..=21.0`) between two picked opaque colours.
pub fn contrast_ratio(a: Rgba, b: Rgba) -> f64 {
    color::contrast(a.opaque(), b.opaque())
}

fn format_hex(c: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=max right before the cast"
)]
fn round_clamped_u16(x: f64, max: u16) -> u16 {
    x.round().clamp(0.0, f64::from(max)) as u16
}

fn hue_degrees(c: Rgba, delta: f64) -> u16 {
    if delta == 0.0 {
        return 0;
    }
    let r = f64::from(c.r) / 255.0;
    let g = f64::from(c.g) / 255.0;
    let b = f64::from(c.b) / 255.0;
    let raw = if c.r >= c.g && c.r >= c.b {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if c.g >= c.r && c.g >= c.b {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    round_clamped_u16(raw, 360) % 360
}

fn rgb_to_hsl(c: Rgba) -> (u16, u16, u16) {
    let r = f64::from(c.r) / 255.0;
    let g = f64::from(c.g) / 255.0;
    let b = f64::from(c.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;
    let s = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs())
    };
    let h = hue_degrees(c, delta);
    let s = round_clamped_u16(s * 100.0, 100);
    let l = round_clamped_u16(l * 100.0, 100);
    (h, s, l)
}

fn rgb_to_hsv(c: Rgba) -> (u16, u16, u16) {
    let r = f64::from(c.r) / 255.0;
    let g = f64::from(c.g) / 255.0;
    let b = f64::from(c.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let s = if max == 0.0 { 0.0 } else { delta / max };
    let h = hue_degrees(c, delta);
    let s = round_clamped_u16(s * 100.0, 100);
    let v = round_clamped_u16(max * 100.0, 100);
    (h, s, v)
}

/// Standard W3C CSS Color 4 named colours (all 139 unique sRGB values; canonical spellings
/// `gray`/`cyan`/`magenta` are kept instead of duplicate aliases `grey`/`aqua`/`fuchsia`).
const CSS_COLORS: &[(&str, u8, u8, u8)] = &[
    ("aliceblue", 240, 248, 255),
    ("antiquewhite", 250, 235, 215),
    ("aquamarine", 127, 255, 212),
    ("azure", 240, 255, 255),
    ("beige", 245, 245, 220),
    ("bisque", 255, 228, 196),
    ("black", 0, 0, 0),
    ("blanchedalmond", 255, 235, 205),
    ("blue", 0, 0, 255),
    ("blueviolet", 138, 43, 226),
    ("brown", 165, 42, 42),
    ("burlywood", 222, 184, 135),
    ("cadetblue", 95, 158, 160),
    ("chartreuse", 127, 255, 0),
    ("chocolate", 210, 105, 30),
    ("coral", 255, 127, 80),
    ("cornflowerblue", 100, 149, 237),
    ("cornsilk", 255, 248, 220),
    ("crimson", 220, 20, 60),
    ("cyan", 0, 255, 255),
    ("darkblue", 0, 0, 139),
    ("darkcyan", 0, 139, 139),
    ("darkgoldenrod", 184, 134, 11),
    ("darkgray", 169, 169, 169),
    ("darkgreen", 0, 100, 0),
    ("darkkhaki", 189, 183, 107),
    ("darkmagenta", 139, 0, 139),
    ("darkolivegreen", 85, 107, 47),
    ("darkorange", 255, 140, 0),
    ("darkorchid", 153, 50, 204),
    ("darkred", 139, 0, 0),
    ("darksalmon", 233, 150, 122),
    ("darkseagreen", 143, 188, 143),
    ("darkslateblue", 72, 61, 139),
    ("darkslategray", 47, 79, 79),
    ("darkturquoise", 0, 206, 209),
    ("darkviolet", 148, 0, 211),
    ("deeppink", 255, 20, 147),
    ("deepskyblue", 0, 191, 255),
    ("dimgray", 105, 105, 105),
    ("dodgerblue", 30, 144, 255),
    ("firebrick", 178, 34, 34),
    ("floralwhite", 255, 250, 240),
    ("forestgreen", 34, 139, 34),
    ("gainsboro", 220, 220, 220),
    ("ghostwhite", 248, 248, 255),
    ("gold", 255, 215, 0),
    ("goldenrod", 218, 165, 32),
    ("gray", 128, 128, 128),
    ("green", 0, 128, 0),
    ("greenyellow", 173, 255, 47),
    ("honeydew", 240, 255, 240),
    ("hotpink", 255, 105, 180),
    ("indianred", 205, 92, 92),
    ("indigo", 75, 0, 130),
    ("ivory", 255, 255, 240),
    ("khaki", 240, 230, 140),
    ("lavender", 230, 230, 250),
    ("lavenderblush", 255, 240, 245),
    ("lawngreen", 124, 252, 0),
    ("lemonchiffon", 255, 250, 205),
    ("lightblue", 173, 216, 230),
    ("lightcoral", 240, 128, 128),
    ("lightcyan", 224, 255, 255),
    ("lightgoldenrodyellow", 250, 250, 210),
    ("lightgray", 211, 211, 211),
    ("lightgreen", 144, 238, 144),
    ("lightpink", 255, 182, 193),
    ("lightsalmon", 255, 160, 122),
    ("lightseagreen", 32, 178, 170),
    ("lightskyblue", 135, 206, 250),
    ("lightslategray", 119, 136, 153),
    ("lightsteelblue", 176, 196, 222),
    ("lightyellow", 255, 255, 224),
    ("lime", 0, 255, 0),
    ("limegreen", 50, 205, 50),
    ("linen", 250, 240, 230),
    ("magenta", 255, 0, 255),
    ("maroon", 128, 0, 0),
    ("mediumaquamarine", 102, 205, 170),
    ("mediumblue", 0, 0, 205),
    ("mediumorchid", 186, 85, 211),
    ("mediumpurple", 147, 112, 219),
    ("mediumseagreen", 60, 179, 113),
    ("mediumslateblue", 123, 104, 238),
    ("mediumspringgreen", 0, 250, 154),
    ("mediumturquoise", 72, 209, 204),
    ("mediumvioletred", 199, 21, 133),
    ("midnightblue", 25, 25, 112),
    ("mintcream", 245, 255, 250),
    ("mistyrose", 255, 228, 225),
    ("moccasin", 255, 228, 181),
    ("navajowhite", 255, 222, 173),
    ("navy", 0, 0, 128),
    ("oldlace", 253, 245, 230),
    ("olive", 128, 128, 0),
    ("olivedrab", 107, 142, 35),
    ("orange", 255, 165, 0),
    ("orangered", 255, 69, 0),
    ("orchid", 218, 112, 214),
    ("palegoldenrod", 238, 232, 170),
    ("palegreen", 152, 251, 152),
    ("paleturquoise", 175, 238, 238),
    ("palevioletred", 219, 112, 147),
    ("papayawhip", 255, 239, 213),
    ("peachpuff", 255, 218, 185),
    ("peru", 205, 133, 63),
    ("pink", 255, 192, 203),
    ("plum", 221, 160, 221),
    ("powderblue", 176, 224, 230),
    ("purple", 128, 0, 128),
    ("rebeccapurple", 102, 51, 153),
    ("red", 255, 0, 0),
    ("rosybrown", 188, 143, 143),
    ("royalblue", 65, 105, 225),
    ("saddlebrown", 139, 69, 19),
    ("salmon", 250, 128, 114),
    ("sandybrown", 244, 164, 96),
    ("seagreen", 46, 139, 87),
    ("seashell", 255, 245, 238),
    ("sienna", 160, 82, 45),
    ("silver", 192, 192, 192),
    ("skyblue", 135, 206, 235),
    ("slateblue", 106, 90, 205),
    ("slategray", 112, 128, 144),
    ("snow", 255, 250, 250),
    ("springgreen", 0, 255, 127),
    ("steelblue", 70, 130, 180),
    ("tan", 210, 180, 140),
    ("teal", 0, 128, 128),
    ("thistle", 216, 191, 216),
    ("tomato", 255, 99, 71),
    ("turquoise", 64, 224, 208),
    ("violet", 238, 130, 238),
    ("wheat", 245, 222, 179),
    ("white", 255, 255, 255),
    ("whitesmoke", 245, 245, 245),
    ("yellow", 255, 255, 0),
    ("yellowgreen", 154, 205, 50),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{BLACK, WHITE};

    #[test]
    fn format_parse_as_str_and_cycle() {
        let all = [
            Format::Hex,
            Format::Rgb,
            Format::Hsl,
            Format::Hsv,
            Format::Name,
            Format::Gtk,
            Format::Decimal,
        ];
        assert_eq!(Format::default(), Format::Hex);

        let mut cur = Format::Hex;
        for &expected in &all {
            assert_eq!(cur, expected);
            assert_eq!(Format::parse(cur.as_str()), Some(cur));
            assert_eq!(Format::parse(&cur.as_str().to_uppercase()), Some(cur));
            cur = cur.next();
        }
        assert_eq!(cur, Format::Hex);
        assert_eq!(Format::parse("  RgB "), Some(Format::Rgb));
        assert_eq!(Format::parse("cmyk"), None);
    }

    #[test]
    fn format_all_seven_on_ac_color_sample() {
        // AC-COLOR-01 and AC-COLOR-02 sample: #336699 = rgb(51, 102, 153)
        let c = Rgba::rgb(0x33, 0x66, 0x99);
        assert_eq!(format_color(c, Format::Hex), "#336699");
        assert_eq!(format_color(c, Format::Rgb), "rgb(51, 102, 153)");
        assert_eq!(format_color(c, Format::Hsl), "hsl(210, 50%, 40%)");
        assert_eq!(format_color(c, Format::Hsv), "hsv(210, 67%, 60%)");
        assert_eq!(format_color(c, Format::Name), "#336699");
        assert_eq!(
            format_color(c, Format::Gtk),
            "@define-color picked #336699;"
        );
        assert_eq!(format_color(c, Format::Decimal), "51,102,153");

        // Uppercase hex check (#A1B2C3 from spec)
        let spec = Rgba::rgb(0xA1, 0xB2, 0xC3);
        assert_eq!(format_color(spec, Format::Hex), "#A1B2C3");
        assert_eq!(
            format_color(spec, Format::Gtk),
            "@define-color picked #A1B2C3;"
        );
    }

    #[test]
    fn hsl_and_hsv_conversions() {
        let cases = [
            (BLACK, "hsl(0, 0%, 0%)", "hsv(0, 0%, 0%)"),
            (WHITE, "hsl(0, 0%, 100%)", "hsv(0, 0%, 100%)"),
            (
                Rgba::rgb(128, 128, 128),
                "hsl(0, 0%, 50%)",
                "hsv(0, 0%, 50%)",
            ),
            (
                Rgba::rgb(255, 0, 0),
                "hsl(0, 100%, 50%)",
                "hsv(0, 100%, 100%)",
            ),
            (
                Rgba::rgb(0, 255, 0),
                "hsl(120, 100%, 50%)",
                "hsv(120, 100%, 100%)",
            ),
            (
                Rgba::rgb(0, 0, 255),
                "hsl(240, 100%, 50%)",
                "hsv(240, 100%, 100%)",
            ),
            (
                Rgba::rgb(0x33, 0x66, 0x99),
                "hsl(210, 50%, 40%)",
                "hsv(210, 67%, 60%)",
            ),
            // Hue near 360 wraps cleanly to 0
            (
                Rgba::rgb(255, 0, 1),
                "hsl(0, 100%, 50%)",
                "hsv(0, 100%, 100%)",
            ),
        ];
        for (c, hsl, hsv) in cases {
            assert_eq!(format_color(c, Format::Hsl), hsl, "{c}");
            assert_eq!(format_color(c, Format::Hsv), hsv, "{c}");
        }
    }

    #[test]
    fn exact_and_nearest_css_names() {
        assert_eq!(exact_css_name(BLACK), Some("black"));
        assert_eq!(exact_css_name(WHITE), Some("white"));
        assert_eq!(
            exact_css_name(Rgba::rgb(0x66, 0x33, 0x99)),
            Some("rebeccapurple")
        );
        assert_eq!(
            format_color(Rgba::rgb(0x66, 0x33, 0x99), Format::Name),
            "rebeccapurple"
        );
        assert_eq!(exact_css_name(Rgba::rgb(0x33, 0x66, 0x99)), None);
        assert_eq!(
            format_color(Rgba::rgb(0x33, 0x66, 0x99), Format::Name),
            "#336699"
        );

        // Nearest matches exact entries verbatim and picks close neighbours otherwise
        assert_eq!(
            nearest_css_name(Rgba::rgb(0x66, 0x33, 0x99)),
            "rebeccapurple"
        );
        assert_eq!(nearest_css_name(Rgba::rgb(254, 2, 1)), "red");
        assert_eq!(nearest_css_name(Rgba::rgb(1, 2, 1)), "black");
        assert_eq!(nearest_css_name(Rgba::rgb(253, 254, 255)), "white");
    }

    #[test]
    fn wcag_contrast_ratio() {
        assert!((contrast_ratio(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(WHITE, BLACK) - 21.0).abs() < 1e-9);
        let sample = Rgba::rgb(0x33, 0x66, 0x99);
        assert!((contrast_ratio(sample, sample) - 1.0).abs() < 1e-9);
        assert!((contrast_ratio(sample, WHITE) - contrast_ratio(WHITE, sample)).abs() < 1e-9);
    }
}
