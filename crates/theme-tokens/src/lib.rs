// SPDX-License-Identifier: GPL-3.0-or-later
//! theme-tokens: the design-token layer of desktop-themes-rs.
//!
//! A theme may contain `tokens.toml` (see docs/design-tokens.md). This crate parses it
//! (with `INHERITS` deep-merge), validates it (ranges, WCAG AA contrast), and turns it into
//! files (libadwaita CSS, GNOME Shell CSS, Ptyxis palette, wallpaper SVG) and dconf keys.
//! It has no GNOME dependencies: the theme manager (themes-core) decides where files are
//! linked and which keys are written.

pub mod accent;
pub mod adwaita;
pub mod cache;
pub mod color;
pub mod generate;
pub mod load;
pub mod palette;
pub mod picker;
pub mod schema;
pub mod validate;

pub use cache::{GENERATOR_VERSION, Materialized, is_current, materialize, planned};
pub use generate::settings::{KeyModule, Keys, USER_THEME_EXTENSION, keys};
pub use generate::{Outputs, Target};
pub use load::{Issue, Layer, Loaded, TokenError, load_chain, load_str};
pub use schema::Tokens;

/// File name of the token file inside a theme directory.
pub const TOKENS_FILE: &str = "tokens.toml";
