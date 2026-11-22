// SPDX-License-Identifier: GPL-3.0-or-later
//! The typed schema of `tokens.toml`. Every field is optional: what a theme leaves out keeps
//! the upstream (libadwaita / GNOME Shell) value, and generators emit only what is set.

use serde::{Deserialize, Deserializer};

use crate::accent;
use crate::color::Rgba;

/// Supported `schema = N`.
pub const SCHEMA_VERSION: u32 = 1;

fn de_color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Rgba>, D::Error> {
    let s = String::deserialize(d)?;
    Rgba::parse(&s).map(Some).map_err(serde::de::Error::custom)
}

/// The accent: a GNOME accent name (`blue`, `teal`, ... `slate`) or an exact colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Accent {
    Named(&'static str),
    Color(Rgba),
}

impl Accent {
    pub fn parse(s: &str) -> Result<Accent, String> {
        let t = s.trim();
        if t.starts_with('#') {
            return Rgba::parse(t).map(Accent::Color);
        }
        accent::ACCENTS
            .iter()
            .find(|(n, _)| *n == t)
            .map(|(n, _)| Accent::Named(n))
            .ok_or_else(|| {
                format!(
                    "«{s}» is neither a colour nor a GNOME accent name ({})",
                    accent::ACCENTS.map(|(n, _)| n).join(", ")
                )
            })
    }
    pub fn rgba(&self) -> Rgba {
        match self {
            Accent::Named(n) => accent::named(n).unwrap_or(accent::ACCENTS[0].1),
            Accent::Color(c) => *c,
        }
    }
    /// Value for `org.gnome.desktop.interface accent-color`.
    pub fn gnome_name(&self) -> &'static str {
        match self {
            Accent::Named(n) => n,
            Accent::Color(c) => accent::nearest(*c),
        }
    }
}

fn de_accent<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Accent>, D::Error> {
    let s = String::deserialize(d)?;
    Accent::parse(&s)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

fn de_colors<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Rgba>>, D::Error> {
    let v = Vec::<String>::deserialize(d)?;
    v.iter()
        .map(|s| Rgba::parse(s))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// Defines the per-scheme colour set once and the `[colors]` table (which accepts the same keys
/// as defaults for both schemes, plus `light` and `dark`).
macro_rules! color_set {
    ($( $(#[$m:meta])* $name:ident ),* $(,)?) => {
        /// Colours of one scheme (`[colors.light]` / `[colors.dark]`).
        #[derive(Clone, Debug, Default, Deserialize, PartialEq)]
        #[serde(deny_unknown_fields)]
        pub struct ColorSet {
            /// Accent: GNOME accent name or colour (`--accent-bg-color`).
            #[serde(default, deserialize_with = "de_accent")]
            pub accent: Option<Accent>,
            /// Opacity of `currentColor` used for borders (`--border-opacity`), 0..1.
            #[serde(default)]
            pub border_opacity: Option<f64>,
            $( $(#[$m])* #[serde(default, deserialize_with = "de_color")] pub $name: Option<Rgba>, )*
        }

        /// `[colors]`: keys here apply to both schemes unless `[colors.light]`/`[colors.dark]`
        /// set them (merged before deserialising; this type only checks the key names).
        #[derive(Clone, Debug, Default, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct ColorsTable {
            #[serde(default)]
            pub light: Option<ColorSet>,
            #[serde(default)]
            pub dark: Option<ColorSet>,
            #[serde(default, deserialize_with = "de_accent")]
            pub accent: Option<Accent>,
            #[serde(default)]
            pub border_opacity: Option<f64>,
            $( #[serde(default, deserialize_with = "de_color")] pub $name: Option<Rgba>, )*
        }

        impl ColorSet {
            /// (token key, value) of every plain colour field.
            pub fn colors(&self) -> Vec<(&'static str, Option<Rgba>)> {
                vec![ $( (stringify!($name), self.$name), )* ]
            }
        }

        /// Names of the plain colour keys (for docs and tests).
        pub const COLOR_KEYS: &[&str] = &[ $( stringify!($name), )* ];
    };
}

color_set! {
    accent_fg,
    window_bg, window_fg,
    view_bg, view_fg,
    headerbar_bg, headerbar_fg, headerbar_border, headerbar_backdrop,
    sidebar_bg, sidebar_fg, sidebar_backdrop,
    card_bg, card_fg,
    popover_bg, popover_fg,
    dialog_bg, dialog_fg,
    destructive_bg, destructive_fg,
    success_bg, success_fg,
    warning_bg, warning_fg,
    error_bg, error_fg,
    shade,
}

/// `[radii]`, pixels.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Radii {
    pub window: Option<u32>,
    pub button: Option<u32>,
    pub card: Option<u32>,
    pub popover: Option<u32>,
    /// Shell surfaces: quick settings, notifications, OSD, dash, switchers.
    pub panel: Option<u32>,
}

/// `[fonts]`: Pango font descriptions ("Family [Style] Size").
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fonts {
    pub ui: Option<String>,
    pub document: Option<String>,
    pub mono: Option<String>,
    /// Server-side titlebars (mutter). Derived from `ui` + `title_weight` when absent.
    pub titlebar: Option<String>,
    /// CSS weight of window titles, 100..900.
    pub title_weight: Option<u16>,
}

/// `[spacing]`, pixels, xs <= sm <= md <= lg <= xl.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub xs: Option<u32>,
    pub sm: Option<u32>,
    pub md: Option<u32>,
    pub lg: Option<u32>,
    pub xl: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub blur: u32,
    #[serde(default)]
    pub spread: i32,
    /// Opacity of the scheme's shade colour, 0..1.
    pub opacity: f64,
}

/// `[shadows]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shadows {
    pub card: Option<Shadow>,
    pub popover: Option<Shadow>,
}

/// `[surface]`: transparency and blur intent.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// Top bar background opacity, 0..1.
    pub panel_opacity: Option<f64>,
    /// Shell popups (menus, quick settings) background opacity, 0..1.
    pub popover_opacity: Option<f64>,
    /// Intent only: the theme expects a blurred panel/overview (realised by Blur my Shell when
    /// the theme enables it; the generators never enable extensions on their own).
    pub blur: Option<bool>,
}

/// `[motion]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub fast_ms: Option<u32>,
    pub normal_ms: Option<u32>,
    pub slow_ms: Option<u32>,
    /// `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `ease-out-quad`,
    /// `ease-out-cubic`, `ease-out-quint`, `ease-in-out-cubic` or `cubic-bezier(a, b, c, d)`.
    pub easing: Option<String>,
}

/// `[sounds]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sounds {
    /// freedesktop sound theme name (`org.gnome.desktop.sound theme-name`).
    pub theme: Option<String>,
}

/// 16 ANSI colours + foreground, background and cursor of one scheme.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TermColors {
    #[serde(default, deserialize_with = "de_color")]
    pub fg: Option<Rgba>,
    #[serde(default, deserialize_with = "de_color")]
    pub bg: Option<Rgba>,
    #[serde(default, deserialize_with = "de_color")]
    pub cursor: Option<Rgba>,
    #[serde(default, deserialize_with = "de_colors")]
    pub palette: Option<Vec<Rgba>>,
}

/// `[terminal]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    /// Name shown in Ptyxis preferences (default: the theme's name).
    pub name: Option<String>,
    /// Use a palette Ptyxis already ships (e.g. "campbell") instead of `[terminal.dark]`.
    pub builtin: Option<String>,
    /// Terminal font (Pango description); sets Ptyxis `use-system-font=false`.
    pub font: Option<String>,
    pub light: Option<TermColors>,
    pub dark: Option<TermColors>,
}

/// `[shell]`: the presence of the table turns the GNOME Shell layer on.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// `false` keeps the table but generates nothing.
    pub generate: Option<bool>,
    /// Which stock stylesheet to layer on and which colour set to use: "dark" (default) | "light".
    pub scheme: Option<String>,
    #[serde(default, deserialize_with = "de_color")]
    pub panel_bg: Option<Rgba>,
    #[serde(default, deserialize_with = "de_color")]
    pub panel_fg: Option<Rgba>,
}

/// `[gtk]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Gtk {
    /// `false`: never generate ~/.config/gtk-4.0 CSS from these tokens.
    pub generate: Option<bool>,
}

/// `[wallpaper]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wallpaper {
    /// "gradient": an SVG generated from the colours (light and dark variants).
    pub style: Option<String>,
    /// An existing image (`~`/`$HOME` allowed); wins over `style`.
    pub image: Option<String>,
}

/// A whole `tokens.toml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokens {
    pub schema: Option<u32>,
    /// Display name used by generated artefacts (palette name); default: theme NAME.
    pub name: Option<String>,
    /// `org.gnome.desktop.interface color-scheme`: "default" | "prefer-dark" | "prefer-light".
    pub color_scheme: Option<String>,
    #[serde(default)]
    pub colors: ColorsResolved,
    #[serde(default)]
    pub radii: Radii,
    #[serde(default)]
    pub fonts: Fonts,
    #[serde(default)]
    pub spacing: Spacing,
    #[serde(default)]
    pub shadows: Shadows,
    #[serde(default)]
    pub surface: Surface,
    #[serde(default)]
    pub motion: Motion,
    #[serde(default)]
    pub sounds: Sounds,
    pub terminal: Option<Terminal>,
    pub shell: Option<Shell>,
    #[serde(default)]
    pub gtk: Gtk,
    pub wallpaper: Option<Wallpaper>,
}

/// `[colors]` after the shared keys were copied into both schemes (see `load`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorsResolved {
    #[serde(default)]
    pub light: ColorSet,
    #[serde(default)]
    pub dark: ColorSet,
}

/// Raw view of a single file, used only to check key names with line numbers.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub(crate) struct TokensFile {
    pub schema: Option<u32>,
    pub name: Option<String>,
    pub color_scheme: Option<String>,
    pub colors: Option<ColorsTable>,
    pub radii: Option<Radii>,
    pub fonts: Option<Fonts>,
    pub spacing: Option<Spacing>,
    pub shadows: Option<Shadows>,
    pub surface: Option<Surface>,
    pub motion: Option<Motion>,
    pub sounds: Option<Sounds>,
    pub terminal: Option<Terminal>,
    pub shell: Option<Shell>,
    pub gtk: Option<Gtk>,
    pub wallpaper: Option<Wallpaper>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub fn id(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }
}

impl Tokens {
    pub fn colors(&self, s: Scheme) -> &ColorSet {
        match s {
            Scheme::Light => &self.colors.light,
            Scheme::Dark => &self.colors.dark,
        }
    }

    /// Accent of the light scheme, else of the dark one.
    pub fn accent(&self) -> Option<Accent> {
        self.colors.light.accent.or(self.colors.dark.accent)
    }

    pub fn shell_enabled(&self) -> bool {
        self.shell
            .as_ref()
            .is_some_and(|s| s.generate != Some(false))
    }

    pub fn shell_scheme(&self) -> Scheme {
        match self.shell.as_ref().and_then(|s| s.scheme.as_deref()) {
            Some("light") => Scheme::Light,
            _ => Scheme::Dark,
        }
    }

    /// Whether the libadwaita stylesheet would contain anything.
    pub fn has_gtk_content(&self) -> bool {
        if self.gtk.generate == Some(false) {
            return false;
        }
        let set = |c: &ColorSet| *c != ColorSet::default() && !accent_only_named(c);
        set(&self.colors.light)
            || set(&self.colors.dark)
            || self.radii.window.is_some()
            || self.radii.button.is_some()
            || self.radii.card.is_some()
            || self.radii.popover.is_some()
            || self.fonts.title_weight.is_some()
            || self.shadows != Shadows::default()
            || self.motion.normal_ms.is_some()
            || self.motion.fast_ms.is_some()
            || self.motion.easing.is_some()
    }

    /// Terminal palette is generated (not a builtin reference).
    pub fn terminal_palette(&self) -> Option<&Terminal> {
        self.terminal
            .as_ref()
            .filter(|t| t.builtin.is_none() && (t.dark.is_some() || t.light.is_some()))
    }

    pub fn wallpaper_svg(&self) -> bool {
        self.wallpaper
            .as_ref()
            .is_some_and(|w| w.image.is_none() && w.style.as_deref() == Some("gradient"))
    }
}

/// A colour set that only names a GNOME accent needs no CSS: the system accent does it.
fn accent_only_named(c: &ColorSet) -> bool {
    matches!(c.accent, Some(Accent::Named(_)))
        && ColorSet {
            accent: None,
            ..c.clone()
        } == ColorSet::default()
}
