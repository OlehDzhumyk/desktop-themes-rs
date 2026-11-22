// SPDX-License-Identifier: GPL-3.0-or-later
//! A quiet gradient wallpaper (SVG, 3840x2160) derived from the colours: the window
//! background shading into a tint of the accent, with two soft accent glows.

use std::fmt::Write;

use crate::color::{Oklch, Rgba};
use crate::schema::{Scheme, Tokens};
use crate::validate::effective;

fn svg(t: &Tokens, s: Scheme) -> String {
    let c = t.colors(s);
    let base = effective(c, s, "window_bg").unwrap_or(Rgba::rgb(0xfa, 0xfa, 0xfb)).opaque();
    let accent = effective(c, s, "accent").unwrap_or(Rgba::rgb(0x35, 0x84, 0xe4));
    let a = accent.to_oklab().to_lch();
    let b = base.to_oklab().to_lch();
    // far corner: background lightness pushed towards the accent hue
    let (far_l, glow_l) = match s {
        Scheme::Light => ((b.l - 0.08).max(0.0), (a.l + 0.15).min(0.92)),
        Scheme::Dark => ((b.l + 0.06).min(1.0), (a.l - 0.1).max(0.25)),
    };
    let far = Oklch::new(far_l, (a.c * 0.35).min(0.08), a.h).to_rgb_mapped();
    let glow1 = Oklch::new(glow_l, a.c * 0.8, a.h).to_rgb_mapped();
    let glow2 = Oklch::new(glow_l, a.c * 0.6, a.h + 40.0).to_rgb_mapped();
    let (o1, o2) = match s {
        Scheme::Light => (0.45, 0.30),
        Scheme::Dark => (0.40, 0.25),
    };
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"3840\" height=\"2160\" viewBox=\"0 0 3840 2160\">\n\
         <defs>\n\
         <linearGradient id=\"bg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"{base}\"/><stop offset=\"1\" stop-color=\"{far}\"/></linearGradient>\n\
         <radialGradient id=\"g1\" cx=\"0.78\" cy=\"0.22\" r=\"0.55\">\
         <stop offset=\"0\" stop-color=\"{glow1}\" stop-opacity=\"{o1}\"/><stop offset=\"1\" stop-color=\"{glow1}\" stop-opacity=\"0\"/></radialGradient>\n\
         <radialGradient id=\"g2\" cx=\"0.18\" cy=\"0.85\" r=\"0.6\">\
         <stop offset=\"0\" stop-color=\"{glow2}\" stop-opacity=\"{o2}\"/><stop offset=\"1\" stop-color=\"{glow2}\" stop-opacity=\"0\"/></radialGradient>\n\
         </defs>\n\
         <rect width=\"3840\" height=\"2160\" fill=\"url(#bg)\"/>\n\
         <rect width=\"3840\" height=\"2160\" fill=\"url(#g1)\"/>\n\
         <rect width=\"3840\" height=\"2160\" fill=\"url(#g2)\"/>\n\
         </svg>\n"
    );
    out
}

/// (light, dark)
pub fn svgs(t: &Tokens) -> (String, String) {
    (svg(t, Scheme::Light), svg(t, Scheme::Dark))
}
