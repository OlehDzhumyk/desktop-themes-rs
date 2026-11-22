// SPDX-License-Identifier: GPL-3.0-or-later
//! Stock libadwaita 1.10 values (from its compiled stylesheet), used to validate partially
//! specified token sets: a theme that sets only `window_bg` is checked against Adwaita's
//! `window_fg`.

use crate::color::Rgba;
use crate::schema::Scheme;

const FG_LIGHT: Rgba = Rgba::rgba(0, 0, 6, 204);
const WHITE: Rgba = Rgba::rgb(255, 255, 255);

/// Default of a colour token (`None` for tokens without a fixed default, e.g. backdrop
/// colours that follow other colours).
pub fn default_color(s: Scheme, key: &str) -> Option<Rgba> {
    let light = s == Scheme::Light;
    let v = match key {
        "window_bg" => {
            if light {
                Rgba::rgb(0xfa, 0xfa, 0xfb)
            } else {
                Rgba::rgb(0x22, 0x22, 0x26)
            }
        }
        "view_bg" => {
            if light {
                WHITE
            } else {
                Rgba::rgb(0x1d, 0x1d, 0x20)
            }
        }
        "headerbar_bg" => {
            if light {
                WHITE
            } else {
                Rgba::rgb(0x2e, 0x2e, 0x32)
            }
        }
        "sidebar_bg" => {
            if light {
                Rgba::rgb(0xeb, 0xeb, 0xed)
            } else {
                Rgba::rgb(0x2e, 0x2e, 0x32)
            }
        }
        "card_bg" => {
            if light {
                WHITE
            } else {
                Rgba::rgba(255, 255, 255, 20)
            }
        }
        "popover_bg" | "dialog_bg" => {
            if light {
                if key == "dialog_bg" {
                    Rgba::rgb(0xfa, 0xfa, 0xfb)
                } else {
                    WHITE
                }
            } else {
                Rgba::rgb(0x36, 0x36, 0x3a)
            }
        }
        "window_fg" | "view_fg" | "headerbar_fg" | "sidebar_fg" | "card_fg" | "popover_fg"
        | "dialog_fg" => {
            if light {
                FG_LIGHT
            } else {
                WHITE
            }
        }
        "accent" => Rgba::rgb(0x35, 0x84, 0xe4),
        "accent_fg" | "destructive_fg" | "success_fg" | "error_fg" => WHITE,
        "destructive_bg" | "error_bg" => {
            if light {
                Rgba::rgb(0xe0, 0x1b, 0x24)
            } else {
                Rgba::rgb(0xc0, 0x1c, 0x28)
            }
        }
        "success_bg" => {
            if light {
                Rgba::rgb(0x2e, 0xc2, 0x7e)
            } else {
                Rgba::rgb(0x26, 0xa2, 0x69)
            }
        }
        "warning_bg" => {
            if light {
                Rgba::rgb(0xe5, 0xa5, 0x0a)
            } else {
                Rgba::rgb(0xcd, 0x93, 0x09)
            }
        }
        "warning_fg" => Rgba::rgba(0, 0, 0, 204),
        "shade" => {
            if light {
                Rgba::rgba(0, 0, 6, 18)
            } else {
                Rgba::rgba(0, 0, 6, 64)
            }
        }
        _ => return None,
    };
    Some(v)
}

/// GNOME Shell 51 stock top bar colours (dark and light stylesheets).
pub fn shell_panel(s: Scheme) -> (Rgba, Rgba) {
    match s {
        Scheme::Dark => (Rgba::rgb(0, 0, 0), Rgba::rgb(0xff, 0xff, 0xff)),
        Scheme::Light => (Rgba::rgb(0xfa, 0xfa, 0xfa), Rgba::rgb(0x22, 0x22, 0x26)),
    }
}
