// SPDX-License-Identifier: GPL-3.0-or-later
//! The look of the shell: Windows 11 Fluent first (mica-like acrylic surfaces, rounded pills,
//! accent underline, 2 px focus ring), with light, dark and high-contrast palettes.
//!
//! Every shell window carries the classes of [`Look::classes`]; the palette lives in CSS custom
//! properties (GTK ≥ 4.16), so one stylesheet serves every combination. The `macos` and `default`
//! skins reuse the Windows 11 layout in this first version and only change the palette and the
//! corner radii (ADR 0012).

use desk_settings::accessors::Skin;
use gtk::gdk;
use gtk::prelude::*;

/// What the classes of a shell window depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub skin: Skin,
    pub dark: bool,
    pub high_contrast: bool,
}

impl Look {
    /// The CSS classes of a shell window.
    pub fn classes(self) -> Vec<&'static str> {
        let skin = match self.skin {
            Skin::Windows11 => "skin-windows11",
            Skin::Macos => "skin-macos",
            Skin::Default => "skin-default",
        };
        let mut out = vec!["desk-shell", skin, if self.dark { "dark" } else { "light" }];
        if self.high_contrast {
            out.push("hc");
        }
        out
    }

    /// Put the classes on `widget`, replacing those of an earlier look.
    pub fn apply(self, widget: &impl IsA<gtk::Widget>) {
        for c in ALL_CLASSES {
            widget.remove_css_class(c);
        }
        for c in self.classes() {
            widget.add_css_class(c);
        }
    }
}

const ALL_CLASSES: [&str; 7] = [
    "desk-shell",
    "skin-windows11",
    "skin-macos",
    "skin-default",
    "dark",
    "light",
    "hc",
];

/// The stylesheet.
pub const CSS: &str = r#"
.desk-shell {
  --ds-bg: rgba(243, 243, 243, 0.92);
  --ds-flyout: rgba(249, 249, 249, 0.96);
  --ds-fg: #1b1b1b;
  --ds-dim: rgba(0, 0, 0, 0.62);
  --ds-hover: rgba(0, 0, 0, 0.055);
  --ds-active: rgba(0, 0, 0, 0.09);
  --ds-stroke: rgba(0, 0, 0, 0.08);
  --ds-field: rgba(255, 255, 255, 0.85);
  --ds-accent: #005fb8;
  --ds-on-accent: #ffffff;
  --ds-radius: 4px;
  --ds-flyout-radius: 8px;
  font-family: "Segoe UI Variable", "Segoe UI", "Inter", "Cantarell", sans-serif;
  color: var(--ds-fg);
}
.desk-shell.dark {
  --ds-bg: rgba(32, 32, 32, 0.90);
  --ds-flyout: rgba(44, 44, 44, 0.96);
  --ds-fg: #ffffff;
  --ds-dim: rgba(255, 255, 255, 0.72);
  --ds-hover: rgba(255, 255, 255, 0.06);
  --ds-active: rgba(255, 255, 255, 0.10);
  --ds-stroke: rgba(255, 255, 255, 0.09);
  --ds-field: rgba(255, 255, 255, 0.06);
  --ds-accent: #60cdff;
  --ds-on-accent: #000000;
}
.desk-shell.skin-windows11 { --ds-radius: 4px; --ds-flyout-radius: 8px; }
.desk-shell.skin-macos { --ds-accent: #007aff; --ds-radius: 6px; --ds-flyout-radius: 12px; }
.desk-shell.skin-macos.dark { --ds-accent: #0a84ff; --ds-on-accent: #ffffff; }
.desk-shell.skin-default { --ds-accent: #3584e4; --ds-on-accent: #ffffff; --ds-radius: 6px; --ds-flyout-radius: 12px; }
.desk-shell.hc {
  --ds-bg: #000000;
  --ds-flyout: #000000;
  --ds-fg: #ffffff;
  --ds-dim: #ffffff;
  --ds-hover: #1aebff;
  --ds-active: #1aebff;
  --ds-stroke: #ffffff;
  --ds-field: #000000;
  --ds-accent: #ffff00;
  --ds-on-accent: #000000;
}
.desk-shell.hc.light {
  --ds-bg: #ffffff;
  --ds-flyout: #ffffff;
  --ds-fg: #000000;
  --ds-dim: #000000;
  --ds-hover: #37006e;
  --ds-active: #37006e;
  --ds-stroke: #000000;
  --ds-field: #ffffff;
  --ds-accent: #37006e;
  --ds-on-accent: #ffffff;
}

window.desk-shell { background: none; }

.desk-shell-taskbar {
  min-height: 48px;
  background-color: var(--ds-bg);
  border-top: 1px solid var(--ds-stroke);
  padding: 0 8px;
}
.desk-shell-taskbar button {
  min-height: 40px;
  min-width: 40px;
  margin: 4px 2px;
  padding: 0 6px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-taskbar button:hover { background-color: var(--ds-hover); }
.desk-shell-taskbar button:active,
.desk-shell-taskbar button:checked { background-color: var(--ds-active); }
.desk-shell-taskbar .pin { border-bottom: 3px solid transparent; }
.desk-shell-taskbar .pin:checked { border-bottom-color: var(--ds-accent); }
.desk-shell-taskbar .pin image { -gtk-icon-size: 24px; }

.desk-shell-start-tile { min-width: 8px; min-height: 8px; border-radius: 1px; background-color: #0078d4; }
.desk-shell.dark .desk-shell-start-tile { background-color: #4cc2ff; }
.desk-shell.hc .desk-shell-start-tile { background-color: var(--ds-accent); }

.desk-shell-search-pill {
  min-width: 180px;
  border-radius: 999px;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
}
.desk-shell-search-pill label { color: var(--ds-dim); }

.desk-shell-clock { padding: 0 8px; }
.desk-shell-clock label { font-size: 12px; }

.desk-shell-start-menu {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.28);
}
.desk-shell-start-menu .start-body { padding: 24px 32px 12px 32px; }
.desk-shell-start-menu entry,
.desk-shell-start-menu searchbar entry {
  min-height: 36px;
  border-radius: 999px;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
  border-bottom: 2px solid var(--ds-accent);
  color: var(--ds-fg);
}
.desk-shell-start-menu .section-title { font-weight: 600; margin: 16px 0 8px 4px; }
.desk-shell-start-menu .tile {
  min-width: 96px;
  min-height: 80px;
  padding: 8px 4px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-start-menu .tile image { -gtk-icon-size: 32px; }
.desk-shell-start-menu .tile label { font-size: 12px; }
.desk-shell-start-menu .tile:hover { background-color: var(--ds-hover); }
.desk-shell-start-menu .tile:active { background-color: var(--ds-active); }
.desk-shell-start-menu list { background: none; }
.desk-shell-start-menu list row { border-radius: var(--ds-radius); padding: 6px 8px; margin: 1px 0; }
.desk-shell-start-menu list row:hover { background-color: var(--ds-hover); }
.desk-shell-start-menu list row:selected {
  background-color: var(--ds-active);
  color: var(--ds-fg);
  box-shadow: inset 3px 0 0 var(--ds-accent);
}
.desk-shell-start-menu .result-subtitle { color: var(--ds-dim); font-size: 12px; }
.desk-shell-start-menu .empty { color: var(--ds-dim); margin: 24px; }
.desk-shell-start-menu .start-footer {
  min-height: 56px;
  padding: 0 32px;
  border-top: 1px solid var(--ds-stroke);
  background-color: var(--ds-hover);
  border-radius: 0 0 var(--ds-flyout-radius) var(--ds-flyout-radius);
}
.desk-shell-start-menu .start-footer button {
  min-height: 36px;
  min-width: 36px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-start-menu .start-footer button:hover { background-color: var(--ds-hover); }

.desk-shell-tray-pill { padding: 0 10px; }

.desk-shell-flyout {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.28);
  padding: 16px;
  color: var(--ds-fg);
}
.desk-shell-flyout button {
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
  min-height: 32px;
  min-width: 32px;
}
.desk-shell-flyout button:hover { background-color: var(--ds-hover); }
.desk-shell-flyout button:active { background-color: var(--ds-active); }
.desk-shell-flyout .section-title { font-weight: 600; margin: 4px 0; }
.desk-shell-flyout .dim,
.desk-shell-flyout .qs-percent { color: var(--ds-dim); font-size: 12px; }
.desk-shell-flyout .empty { color: var(--ds-dim); margin: 24px; }
.desk-shell-flyout .qs-tile {
  min-height: 72px;
  padding: 8px 4px;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
}
.desk-shell-flyout .qs-tile image { -gtk-icon-size: 20px; }
.desk-shell-flyout .qs-tile label { font-size: 12px; }
.desk-shell-flyout .qs-tile:checked {
  background-color: var(--ds-accent);
  color: var(--ds-on-accent);
  border-color: var(--ds-accent);
}
.desk-shell-flyout .qs-tile:disabled { opacity: 0.5; }
.desk-shell-flyout .qs-audio {
  border-top: 1px solid var(--ds-stroke);
  padding-top: 12px;
}
.desk-shell-flyout scale trough { min-height: 4px; border-radius: 2px; background-color: var(--ds-stroke); }
.desk-shell-flyout scale highlight { background-color: var(--ds-accent); border-radius: 2px; }
.desk-shell-flyout scale slider {
  min-width: 18px;
  min-height: 18px;
  border-radius: 999px;
  background-color: var(--ds-accent);
  border: 4px solid var(--ds-flyout);
}
.desk-shell-flyout list { background: none; }
.desk-shell-flyout list row {
  border-radius: var(--ds-radius);
  padding: 8px;
  margin: 2px 0;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
}
.desk-shell-flyout list row:hover { background-color: var(--ds-hover); }
.desk-shell-flyout list row:selected {
  background-color: var(--ds-active);
  color: var(--ds-fg);
  box-shadow: inset 3px 0 0 var(--ds-accent);
}
.desk-shell-flyout .clip-pin-badge { color: var(--ds-accent); }
.desk-shell-flyout .clip-banner {
  padding: 8px;
  border-radius: var(--ds-radius);
  background-color: var(--ds-active);
  border-left: 3px solid var(--ds-accent);
}

.desk-shell button:focus-visible,
.desk-shell row:focus-visible {
  outline: 2px solid var(--ds-fg);
  outline-offset: -2px;
}
.desk-shell.hc button:hover,
.desk-shell.hc row:selected,
.desk-shell.hc .qs-tile:checked { color: var(--ds-on-accent); }
"#;

/// Load [`CSS`] for the display (once, at start-up).
pub fn install(display: &gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// The current look: `skin` and the system's colour scheme and contrast preference.
pub fn current(skin: Skin) -> Look {
    let settings = gtk::Settings::default();
    let dark = settings
        .as_ref()
        .is_some_and(|s| s.gtk_interface_color_scheme() == gtk::InterfaceColorScheme::Dark);
    let high_contrast = settings
        .as_ref()
        .is_some_and(|s| s.gtk_interface_contrast() == gtk::InterfaceContrast::More);
    Look {
        skin,
        dark,
        high_contrast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_every_combination() {
        for skin in Skin::ALL {
            for dark in [false, true] {
                for high_contrast in [false, true] {
                    let c = Look {
                        skin,
                        dark,
                        high_contrast,
                    }
                    .classes();
                    assert_eq!(c[0], "desk-shell");
                    assert!(c.iter().all(|x| ALL_CLASSES.contains(x)), "{c:?}");
                    assert_eq!(c.contains(&"hc"), high_contrast);
                    // every class the code sets has a rule in the stylesheet
                    for class in &c {
                        assert!(CSS.contains(&format!(".{class}")), "{class}");
                    }
                }
            }
        }
    }

    #[test]
    fn stylesheet_is_balanced_and_names_the_surfaces() {
        assert_eq!(CSS.matches('{').count(), CSS.matches('}').count());
        for surface in [
            ".desk-shell-taskbar",
            ".desk-shell-start-menu",
            ".desk-shell-flyout",
            ".qs-tile:checked",
            ".clip-banner",
            ":focus-visible",
            ".desk-shell.hc",
            ".desk-shell.dark",
        ] {
            assert!(CSS.contains(surface), "{surface}");
        }
    }
}
