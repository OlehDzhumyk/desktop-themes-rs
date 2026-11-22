// SPDX-License-Identifier: GPL-3.0-or-later
//! "Theme from wallpaper": k-means in OKLab over a subsample of the image, a seed colour picked
//! by population and chroma, and a Material-You-like tonal palette (fixed OKLCH lightness
//! steps on the seed hue) whose text pairs are pushed until they pass WCAG AA.

use std::fmt::Write;

use crate::color::{Oklab, Oklch, Rgba, contrast};
use crate::validate::{AA_LARGE, AA_TEXT};

/// At most this many pixels are clustered (the rest is skipped with a fixed stride).
pub const MAX_SAMPLES: usize = 16_384;
pub const K: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub color: Rgba,
    pub lab: Oklab,
    /// Share of the sampled pixels, 0..1.
    pub share: f64,
}

/// Dominant colours, most populous first. `rgb` is packed RGB (3 bytes per pixel).
pub fn clusters(rgb: &[u8], k: usize) -> Vec<Cluster> {
    let n = rgb.len() / 3;
    if n == 0 || k == 0 {
        return vec![];
    }
    let stride = n.div_ceil(MAX_SAMPLES).max(1);
    let px: Vec<Oklab> = (0..n)
        .step_by(stride)
        .map(|i| Rgba::rgb(rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]).to_oklab())
        .collect();
    // deterministic farthest-point initialisation from the mean
    let mean = {
        let (mut l, mut a, mut b) = (0.0, 0.0, 0.0);
        for p in &px {
            l += p.l;
            a += p.a;
            b += p.b;
        }
        let c = px.len() as f64;
        Oklab {
            l: l / c,
            a: a / c,
            b: b / c,
        }
    };
    let mut cent: Vec<Oklab> = Vec::with_capacity(k);
    let first = *px
        .iter()
        .max_by(|x, y| x.dist2(mean).total_cmp(&y.dist2(mean)))
        .unwrap_or(&mean);
    cent.push(first);
    while cent.len() < k {
        let next = px
            .iter()
            .map(|p| (p, cent.iter().map(|c| p.dist2(*c)).fold(f64::MAX, f64::min)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match next {
            Some((p, d)) if d > 1e-6 => cent.push(*p),
            _ => break,
        }
    }
    let mut assign = vec![0usize; px.len()];
    for _ in 0..16 {
        let mut moved = false;
        for (i, p) in px.iter().enumerate() {
            let best = cent
                .iter()
                .enumerate()
                .min_by(|a, b| p.dist2(*a.1).total_cmp(&p.dist2(*b.1)))
                .map(|(j, _)| j)
                .unwrap_or(0);
            if assign[i] != best {
                assign[i] = best;
                moved = true;
            }
        }
        let mut sum = vec![(0.0, 0.0, 0.0, 0usize); cent.len()];
        for (i, p) in px.iter().enumerate() {
            let s = &mut sum[assign[i]];
            s.0 += p.l;
            s.1 += p.a;
            s.2 += p.b;
            s.3 += 1;
        }
        for (j, s) in sum.iter().enumerate() {
            if s.3 > 0 {
                let c = s.3 as f64;
                cent[j] = Oklab {
                    l: s.0 / c,
                    a: s.1 / c,
                    b: s.2 / c,
                };
            }
        }
        if !moved {
            break;
        }
    }
    let mut count = vec![0usize; cent.len()];
    for a in &assign {
        count[*a] += 1;
    }
    let total = px.len() as f64;
    let mut out: Vec<Cluster> = cent
        .iter()
        .zip(count)
        .filter(|(_, c)| *c > 0)
        .map(|(lab, c)| Cluster {
            color: lab.to_rgb(),
            lab: *lab,
            share: c as f64 / total,
        })
        .collect();
    out.sort_by(|a, b| {
        b.share
            .total_cmp(&a.share)
            .then(a.color.hex().cmp(&b.color.hex()))
    });
    out
}

/// The seed: a populous, colourful cluster that is neither near-black nor near-white.
pub fn seed(cs: &[Cluster]) -> Oklch {
    let score = |c: &Cluster| {
        let lch = c.lab.to_lch();
        let light_ok = if (0.25..=0.9).contains(&lch.l) {
            1.0
        } else {
            0.35
        };
        c.share.sqrt() * (lch.c + 0.01) * light_ok
    };
    let best = cs
        .iter()
        .max_by(|a, b| score(a).total_cmp(&score(b)))
        .map(|c| c.lab.to_lch())
        .unwrap_or(Oklch::new(0.55, 0.12, 260.0));
    if best.c < 0.04 {
        // greyscale image: keep its hue family but give the accent some colour
        Oklch::new(best.l, 0.06, if best.c < 0.005 { 250.0 } else { best.h })
    } else {
        best
    }
}

fn tone(l: f64, c: f64, h: f64) -> Rgba {
    Oklch::new(l, c, h).to_rgb_mapped()
}

/// Darkens (step < 0) or lightens (step > 0) `c` until `fg` on it reaches `min`.
fn push_bg(mut l: f64, c: f64, h: f64, fg: Rgba, min: f64, step: f64) -> Rgba {
    for _ in 0..60 {
        let bg = tone(l, c, h);
        if contrast(fg, bg) >= min {
            return bg;
        }
        l = (l + step).clamp(0.0, 1.0);
    }
    tone(l, c, h)
}

/// Moves a foreground colour's lightness away from `bg` until it reaches `min`.
fn push_fg(mut l: f64, c: f64, h: f64, bg: Rgba, min: f64) -> Rgba {
    let step = if bg.is_dark() { 0.02 } else { -0.02 };
    for _ in 0..60 {
        let fg = tone(l, c, h);
        if contrast(fg, bg) >= min {
            return fg;
        }
        l = (l + step).clamp(0.0, 1.0);
    }
    tone(l, c, h)
}

pub struct Palette {
    pub seed: Rgba,
    pub light: Vec<(&'static str, Rgba)>,
    pub dark: Vec<(&'static str, Rgba)>,
    pub term_light: (Rgba, Rgba, Rgba, Vec<Rgba>),
    pub term_dark: (Rgba, Rgba, Rgba, Vec<Rgba>),
}

const WHITE: Rgba = Rgba::rgb(255, 255, 255);

/// Hues of the six ANSI colours (red, green, yellow, blue, magenta, cyan), nudged 15 % towards
/// the seed so the terminal belongs to the theme.
fn ansi_hues(seed_h: f64) -> [f64; 6] {
    let base = [25.0, 145.0, 90.0, 255.0, 330.0, 200.0];
    base.map(|h: f64| {
        let mut d = seed_h - h;
        if d > 180.0 {
            d -= 360.0;
        }
        if d < -180.0 {
            d += 360.0;
        }
        (h + d * 0.15).rem_euclid(360.0)
    })
}

fn terminal(seed: Oklch, dark: bool, accent: Rgba) -> (Rgba, Rgba, Rgba, Vec<Rgba>) {
    let h = seed.h;
    let nc = (seed.c * 0.15).min(0.02);
    let hues = ansi_hues(h);
    if dark {
        let bg = tone(0.17, nc, h);
        let fg = push_fg(0.92, nc * 0.5, h, bg, AA_TEXT);
        let mut p = vec![tone(0.32, nc, h)];
        p.extend(hues.iter().map(|hh| push_fg(0.72, 0.13, *hh, bg, AA_LARGE)));
        p.push(tone(0.86, nc * 0.5, h));
        p.push(tone(0.50, nc, h));
        p.extend(hues.iter().map(|hh| push_fg(0.82, 0.12, *hh, bg, AA_LARGE)));
        p.push(tone(0.97, nc * 0.3, h));
        (fg, bg, accent, p)
    } else {
        let bg = tone(0.985, nc * 0.5, h);
        let fg = push_fg(0.25, nc, h, bg, AA_TEXT);
        let mut p = vec![tone(0.25, nc, h)];
        p.extend(hues.iter().map(|hh| push_fg(0.50, 0.15, *hh, bg, AA_LARGE)));
        p.push(tone(0.80, nc * 0.5, h));
        p.push(tone(0.45, nc, h));
        p.extend(hues.iter().map(|hh| push_fg(0.42, 0.16, *hh, bg, AA_LARGE)));
        p.push(tone(0.92, nc * 0.3, h));
        (fg, bg, accent, p)
    }
}

pub fn tonal(seed: Oklch) -> Palette {
    let h = seed.h;
    let ac = seed.c.clamp(0.08, 0.18);
    let nc = (seed.c * 0.12).min(0.02);
    let accent_l = push_bg(0.55, ac, h, WHITE, AA_TEXT, -0.02);
    let accent_d = push_bg(0.68, ac, h, WHITE, AA_LARGE, -0.02);
    let fg_l = tone(0.22, nc * 1.5, h);
    let fg_d = tone(0.95, nc * 0.6, h);
    let status = |hue: f64, light: bool| {
        if light {
            push_bg(0.55, 0.17, hue, WHITE, AA_TEXT, -0.02)
        } else {
            push_bg(0.60, 0.15, hue, WHITE, AA_LARGE, -0.02)
        }
    };
    let warn_fg = Rgba::rgba(0, 0, 0, 204);
    let light = vec![
        ("accent", accent_l),
        ("accent_fg", WHITE),
        ("window_bg", tone(0.975, nc, h)),
        ("window_fg", fg_l),
        ("view_bg", tone(0.995, nc * 0.4, h)),
        ("view_fg", fg_l),
        ("headerbar_bg", tone(0.955, nc, h)),
        ("headerbar_fg", fg_l),
        ("sidebar_bg", tone(0.94, nc * 1.2, h)),
        ("sidebar_fg", fg_l),
        ("card_bg", tone(0.995, nc * 0.4, h)),
        ("card_fg", fg_l),
        ("popover_bg", tone(0.995, nc * 0.4, h)),
        ("popover_fg", fg_l),
        ("dialog_bg", tone(0.975, nc, h)),
        ("dialog_fg", fg_l),
        ("destructive_bg", status(25.0, true)),
        ("destructive_fg", WHITE),
        ("success_bg", status(150.0, true)),
        ("success_fg", WHITE),
        (
            "warning_bg",
            push_bg(0.80, 0.15, 85.0, warn_fg, AA_LARGE, 0.02),
        ),
        ("warning_fg", warn_fg),
    ];
    let dark = vec![
        ("accent", accent_d),
        ("accent_fg", WHITE),
        ("window_bg", tone(0.21, nc, h)),
        ("window_fg", fg_d),
        ("view_bg", tone(0.185, nc, h)),
        ("view_fg", fg_d),
        ("headerbar_bg", tone(0.25, nc, h)),
        ("headerbar_fg", fg_d),
        ("sidebar_bg", tone(0.235, nc, h)),
        ("sidebar_fg", fg_d),
        ("card_bg", tone(0.27, nc, h)),
        ("card_fg", fg_d),
        ("popover_bg", tone(0.29, nc, h)),
        ("popover_fg", fg_d),
        ("dialog_bg", tone(0.25, nc, h)),
        ("dialog_fg", fg_d),
        ("destructive_bg", status(25.0, false)),
        ("destructive_fg", WHITE),
        ("success_bg", status(150.0, false)),
        ("success_fg", WHITE),
        (
            "warning_bg",
            push_bg(0.75, 0.14, 85.0, warn_fg, AA_LARGE, 0.02),
        ),
        ("warning_fg", warn_fg),
    ];
    Palette {
        seed: seed.to_rgb_mapped(),
        term_light: terminal(seed, false, accent_l),
        term_dark: terminal(seed, true, accent_d),
        light,
        dark,
    }
}

fn q(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if c.is_control() => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn colors_table(out: &mut String, group: &str, v: &[(&str, Rgba)]) {
    let _ = writeln!(out, "\n[colors.{group}]");
    for (k, c) in v {
        let _ = writeln!(out, "{k} = \"{}\"", c.hex());
    }
}

fn term_table(out: &mut String, group: &str, t: &(Rgba, Rgba, Rgba, Vec<Rgba>)) {
    let _ = writeln!(out, "\n[terminal.{group}]");
    let _ = writeln!(
        out,
        "fg = \"{}\"\nbg = \"{}\"\ncursor = \"{}\"",
        t.0.hex(),
        t.1.hex(),
        t.2.hex()
    );
    out.push_str("palette = [\n");
    for row in t.3.chunks(4) {
        let line: Vec<String> = row.iter().map(|c| format!("\"{}\"", c.hex())).collect();
        let _ = writeln!(out, "  {},", line.join(", "));
    }
    out.push_str("]\n");
}

/// A complete tokens.toml for a palette. `image` becomes `wallpaper.image` when given.
pub fn tokens_toml(p: &Palette, name: &str, image: Option<&str>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Generated by `themes tokens from-image` (seed colour {}). Edit freely: this file is the theme.",
        p.seed.hex()
    );
    let _ = writeln!(out, "schema = 1\nname = {}", q(name));
    colors_table(&mut out, "light", &p.light);
    colors_table(&mut out, "dark", &p.dark);
    out.push_str(
        "\n[radii]\nwindow = 14\nbutton = 8\ncard = 12\npopover = 12\npanel = 18\n\
         \n[fonts]\ntitle_weight = 700\n\
         \n[shell]\nscheme = \"dark\"\n\
         \n[surface]\npanel_opacity = 0.9\n",
    );
    let _ = writeln!(out, "\n[terminal]\nname = {}", q(name));
    term_table(&mut out, "light", &p.term_light);
    term_table(&mut out, "dark", &p.term_dark);
    out.push_str("\n[wallpaper]\n");
    match image {
        Some(i) => {
            let _ = writeln!(out, "image = {}", q(i));
        }
        None => out.push_str("style = \"gradient\"\n"),
    }
    out
}

/// Whole pipeline for packed RGB pixels.
pub fn from_pixels(rgb: &[u8]) -> (Vec<Cluster>, Palette) {
    let cs = clusters(rgb, K);
    let s = seed(&cs);
    (cs.clone(), tonal(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;

    /// Synthetic image: `parts` of (colour, pixel count).
    fn image(parts: &[(Rgba, usize)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (c, n) in parts {
            for _ in 0..*n {
                v.extend([c.r, c.g, c.b]);
            }
        }
        v
    }

    #[test]
    fn clusters_find_dominant_colours() {
        let p = |s| Rgba::parse(s).unwrap();
        let img = image(&[
            (p("#1e3a8a"), 6000),
            (p("#f97316"), 3000),
            (p("#f5f5f4"), 1000),
        ]);
        let cs = clusters(&img, 4);
        assert_eq!(cs[0].color, p("#1e3a8a"));
        assert!((cs[0].share - 0.6).abs() < 0.01);
        assert_eq!(cs[1].color, p("#f97316"));
        assert_eq!(cs.len(), 3, "identical pixels collapse");
    }

    #[test]
    fn seed_prefers_colourful_over_grey() {
        let p = |s| Rgba::parse(s).unwrap();
        // mostly grey with a green accent area
        let img = image(&[(p("#777777"), 8000), (p("#16a34a"), 2000)]);
        let s = seed(&clusters(&img, 8));
        assert!((130.0..160.0).contains(&s.h), "{s:?}");
        // pure grey image still yields a usable accent
        let s = seed(&clusters(&image(&[(p("#808080"), 100)]), 8));
        assert!(s.c >= 0.04);
    }

    #[test]
    fn generated_tokens_validate_and_are_deterministic() {
        for hex in [
            "#1e3a8a", "#f97316", "#16a34a", "#fde047", "#a21caf", "#808080", "#000000", "#ffffff",
        ] {
            let c = Rgba::parse(hex).unwrap();
            let img = image(&[(c, 500), (Rgba::rgb(20, 20, 20), 300)]);
            let (_, pal) = from_pixels(&img);
            let text = tokens_toml(&pal, "Test", None);
            let l = load_str("gen", &text).unwrap_or_else(|e| panic!("{hex}: {e}\n{text}"));
            assert!(l.tokens.terminal_palette().is_some());
            let (_, pal2) = from_pixels(&img);
            assert_eq!(text, tokens_toml(&pal2, "Test", None));
        }
    }

    #[test]
    fn subsampling_caps_work() {
        let img = image(&[(Rgba::rgb(200, 30, 30), MAX_SAMPLES * 3)]);
        let cs = clusters(&img, 8);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].share, 1.0);
    }
}
