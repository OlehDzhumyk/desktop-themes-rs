// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure-Rust window layouts core of desktop-themes-rs (`docs/spec/features/window-layouts.md`,
//! ADR 0011): built-in snap layouts, fractional zone validation, portrait monitor adaptation,
//! even-pixel gap geometry, minimum-size fitting and atomic 0600 JSON persistence.
//!
//! Zero GTK, GLib, GNOME or D-Bus dependencies: usable from `deskd`, a CLI, or a future KDE /
//! wlroots compositor adapter alike.

pub mod geometry;
pub mod store;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use geometry::{Rect, fit_min_size, resolve_zones};
pub use store::LayoutStore;

/// Default active layout when a monitor has no saved selection.
pub const DEFAULT_LAYOUT_ID: &str = "halves";

/// Tolerance for fractional zone bounds (`x + w <= 1.0`, `y + h <= 1.0`).
const FRACTION_EPS: f64 = 1e-6;

/// Errors returned by layout validation, lookup and persistence.
#[derive(Debug)]
pub enum Error {
    InvalidLayout(String),
    NotFound(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidLayout(msg) => write!(f, "invalid layout: {msg}"),
            Error::NotFound(id) => write!(f, "unknown layout: {id}"),
            Error::Io(err) => write!(f, "layout store I/O error: {err}"),
            Error::Json(err) => write!(f, "layout JSON error: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Json(err) => Some(err),
            Error::InvalidLayout(_) | Error::NotFound(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

/// One zone in a layout, expressed as fractions of the monitor work area (`0.0..=1.0`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Zone {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// Transpose columns and rows for portrait monitors (`height > width`).
    pub const fn transpose(self) -> Self {
        Self {
            x: self.y,
            y: self.x,
            w: self.h,
            h: self.w,
        }
    }

    /// Check that every coordinate is finite, inside `0.0..=1.0`, and has positive size.
    pub fn validate(&self) -> Result<(), Error> {
        let coords = [self.x, self.y, self.w, self.h];
        if !coords.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidLayout(
                "zone coordinates must be finite".into(),
            ));
        }
        if self.x < 0.0 || self.y < 0.0 || self.x > 1.0 || self.y > 1.0 {
            return Err(Error::InvalidLayout(
                "zone origin must lie in 0.0..=1.0".into(),
            ));
        }
        if self.w <= 0.0 || self.h <= 0.0 || self.w > 1.0 || self.h > 1.0 {
            return Err(Error::InvalidLayout(
                "zone size must lie in (0.0, 1.0]".into(),
            ));
        }
        if self.x + self.w > 1.0 + FRACTION_EPS || self.y + self.h > 1.0 + FRACTION_EPS {
            return Err(Error::InvalidLayout(
                "zone extends outside 0.0..=1.0".into(),
            ));
        }
        Ok(())
    }
}

/// A named window layout made of one or more fractional [`Zone`]s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
    pub zones: Vec<Zone>,
}

impl Layout {
    /// Validate the layout identifier, name and every zone.
    pub fn validate(&self) -> Result<(), Error> {
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(Error::InvalidLayout(
                "layout id must be non-empty ASCII [a-z0-9_-]".into(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(Error::InvalidLayout("layout name must not be empty".into()));
        }
        if self.zones.is_empty() {
            return Err(Error::InvalidLayout(
                "layout must contain at least one zone".into(),
            ));
        }
        for zone in &self.zones {
            zone.validate()?;
        }
        Ok(())
    }
}

/// The six built-in layouts defined by `docs/spec/features/window-layouts.md`.
pub fn builtins() -> Vec<Layout> {
    let third = 1.0 / 3.0;
    let two_thirds = 2.0 / 3.0;
    vec![
        Layout {
            id: "halves".into(),
            name: "Halves".into(),
            builtin: true,
            zones: vec![Zone::new(0.0, 0.0, 0.5, 1.0), Zone::new(0.5, 0.0, 0.5, 1.0)],
        },
        Layout {
            id: "thirds".into(),
            name: "Thirds".into(),
            builtin: true,
            zones: vec![
                Zone::new(0.0, 0.0, third, 1.0),
                Zone::new(third, 0.0, third, 1.0),
                Zone::new(two_thirds, 0.0, third, 1.0),
            ],
        },
        Layout {
            id: "two-thirds".into(),
            name: "Two-thirds".into(),
            builtin: true,
            zones: vec![
                Zone::new(0.0, 0.0, two_thirds, 1.0),
                Zone::new(two_thirds, 0.0, third, 1.0),
            ],
        },
        Layout {
            id: "quarters".into(),
            name: "Quarters".into(),
            builtin: true,
            zones: vec![
                Zone::new(0.0, 0.0, 0.5, 0.5),
                Zone::new(0.5, 0.0, 0.5, 0.5),
                Zone::new(0.0, 0.5, 0.5, 0.5),
                Zone::new(0.5, 0.5, 0.5, 0.5),
            ],
        },
        Layout {
            id: "focus".into(),
            name: "Focus".into(),
            builtin: true,
            zones: vec![
                Zone::new(0.0, 0.0, 0.25, 1.0),
                Zone::new(0.25, 0.0, 0.5, 1.0),
                Zone::new(0.75, 0.0, 0.25, 1.0),
            ],
        },
        Layout {
            id: "stack".into(),
            name: "Stack".into(),
            builtin: true,
            zones: vec![
                Zone::new(0.0, 0.0, 0.6, 1.0),
                Zone::new(0.6, 0.0, 0.4, 0.5),
                Zone::new(0.6, 0.5, 0.4, 0.5),
            ],
        },
    ]
}

/// Look up a built-in layout by its identifier.
pub fn builtin(id: &str) -> Option<Layout> {
    builtins().into_iter().find(|layout| layout.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_six_builtins_exist_and_validate() {
        let list = builtins();
        let ids: Vec<&str> = list.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "halves",
                "thirds",
                "two-thirds",
                "quarters",
                "focus",
                "stack"
            ]
        );
        for layout in &list {
            assert!(layout.builtin, "{}", layout.id);
            layout
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", layout.id));
            // Total fractional area of each built-in layout covers the full work area (1.0).
            let area: f64 = layout.zones.iter().map(|z| z.w * z.h).sum();
            assert!((area - 1.0).abs() < 1e-6, "{}: area {area}", layout.id);
        }
        assert!(builtin(DEFAULT_LAYOUT_ID).is_some());
        assert!(builtin("missing").is_none());
    }

    #[test]
    fn zone_and_layout_validation_rejects_bad_inputs() {
        assert!(Zone::new(-0.1, 0.0, 0.5, 1.0).validate().is_err());
        assert!(Zone::new(0.0, 0.0, 0.0, 1.0).validate().is_err());
        assert!(Zone::new(0.6, 0.0, 0.5, 1.0).validate().is_err());
        assert!(Zone::new(f64::NAN, 0.0, 0.5, 1.0).validate().is_err());

        let bad_id = Layout {
            id: "Bad ID".into(),
            name: "Test".into(),
            builtin: false,
            zones: vec![Zone::new(0.0, 0.0, 1.0, 1.0)],
        };
        assert!(bad_id.validate().is_err());

        let empty_zones = Layout {
            id: "empty".into(),
            name: "Empty".into(),
            builtin: false,
            zones: vec![],
        };
        assert!(empty_zones.validate().is_err());
    }
}
