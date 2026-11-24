// SPDX-License-Identifier: GPL-3.0-or-later
//! Window layouts engine of deskd (`docs/spec/features/window-layouts.md`): the settings that
//! matter here (`enabled`, `gap-outer`, `gap-inner`) and the checks around the pure-Rust core
//! `desk-layouts`. The bridge moves the windows; deskd only answers geometry and keeps
//! `layouts.json`.

pub mod service;

use std::fmt;
use std::path::{Path, PathBuf};

use desk_layouts::{LayoutStore, Rect, resolve_zones};
use serde::Serialize;

/// The longest monitor key accepted (connector plus vendor, product and serial fit easily).
pub const MAX_MONITOR_KEY: usize = 256;

/// What the `enabled`, `gap-outer` and `gap-inner` settings give the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub gap_outer: u32,
    pub gap_inner: u32,
}

impl Default for Config {
    /// The schema defaults.
    fn default() -> Config {
        Config {
            enabled: true,
            gap_outer: 8,
            gap_inner: 6,
        }
    }
}

/// Why a layouts call failed.
#[derive(Debug)]
pub enum LayoutsError {
    /// The `enabled` setting is off.
    Disabled,
    /// A bad argument: empty or too long monitor key, empty work area.
    BadArgs(String),
    /// The core refused or failed (unknown layout, I/O, JSON).
    Core(desk_layouts::Error),
}

impl fmt::Display for LayoutsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutsError::Disabled => f.write_str("window layouts are turned off"),
            LayoutsError::BadArgs(why) => f.write_str(why),
            LayoutsError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl From<desk_layouts::Error> for LayoutsError {
    fn from(e: desk_layouts::Error) -> Self {
        LayoutsError::Core(e)
    }
}

/// One resolved zone in the `ResolveZones` answer.
#[derive(Serialize)]
struct ZoneOut {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// `layouts.json` next to the other deskd state (`~/.local/share/deskd/layouts.json`).
pub fn default_store_path() -> PathBuf {
    let (clip, _) = crate::clipboard::Engine::default_dirs();
    clip.parent()
        .map_or_else(|| clip.clone(), Path::to_path_buf)
        .join("layouts.json")
}

/// The engine: the settings and the store. No timer, no thread; each call is a lookup or one
/// small file write.
pub struct Layouts {
    config: Config,
    store: LayoutStore,
}

impl Layouts {
    pub fn new(config: Config, store: LayoutStore) -> Layouts {
        Layouts { config, store }
    }

    /// Take new settings. Returns true when the answers of the engine change (the bridge reads
    /// the zones again).
    pub fn set_config(&mut self, config: Config) -> bool {
        let changed = self.config != config;
        self.config = config;
        changed
    }

    fn check_enabled(&self) -> Result<(), LayoutsError> {
        if self.config.enabled {
            Ok(())
        } else {
            Err(LayoutsError::Disabled)
        }
    }

    fn check_monitor(key: &str) -> Result<(), LayoutsError> {
        if key.is_empty() || key.len() > MAX_MONITOR_KEY || key.chars().any(char::is_control) {
            return Err(LayoutsError::BadArgs(format!(
                "the monitor key must have 1 to {MAX_MONITOR_KEY} bytes and no control characters"
            )));
        }
        Ok(())
    }

    /// All layouts (built-in, then custom) as a JSON list.
    pub fn list_json(&self) -> Result<String, LayoutsError> {
        self.check_enabled()?;
        Ok(serde_json::to_string(&self.store.all_layouts()).map_err(desk_layouts::Error::from)?)
    }

    /// The active layout id of a monitor.
    pub fn active(&self, monitor_key: &str) -> Result<String, LayoutsError> {
        self.check_enabled()?;
        Self::check_monitor(monitor_key)?;
        Ok(self.store.active_layout(monitor_key).to_owned())
    }

    /// Choose and save the active layout of a monitor. Returns true when the choice is new.
    pub fn set_active(&mut self, monitor_key: &str, layout_id: &str) -> Result<bool, LayoutsError> {
        self.check_enabled()?;
        Self::check_monitor(monitor_key)?;
        Ok(self.store.set_active_layout(monitor_key, layout_id)?)
    }

    /// The zones of a layout in a work area, gaps applied, as a JSON list.
    pub fn resolve_json(&self, layout_id: &str, work_area: Rect) -> Result<String, LayoutsError> {
        self.check_enabled()?;
        if work_area.is_empty() {
            return Err(LayoutsError::BadArgs(
                "the work area must have a positive width and height".into(),
            ));
        }
        let layout = self
            .store
            .get_layout(layout_id)
            .ok_or_else(|| desk_layouts::Error::NotFound(layout_id.to_owned()))?;
        let zones: Vec<ZoneOut> = resolve_zones(
            &layout,
            work_area,
            self.config.gap_outer,
            self.config.gap_inner,
        )
        .into_iter()
        .map(|r| ZoneOut {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        })
        .collect();
        Ok(serde_json::to_string(&zones).map_err(desk_layouts::Error::from)?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    pub(crate) fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deskd-layouts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub(crate) fn engine_in(dir: &Path, config: Config) -> Layouts {
        Layouts::new(config, LayoutStore::load(dir.join("layouts.json")))
    }

    #[test]
    fn resolve_uses_the_gap_settings() {
        let dir = tmp("gaps");
        let mut eng = engine_in(&dir, Config::default());
        let fhd = Rect::new(0, 0, 1920, 1080);
        let json = eng.resolve_json("halves", fhd).unwrap();
        let zones: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(zones[0]["x"], 8);
        assert_eq!(zones[0]["y"], 8);
        assert_eq!(
            zones[1]["x"].as_i64().unwrap()
                - zones[0]["x"].as_i64().unwrap()
                - zones[0]["w"].as_i64().unwrap(),
            6
        );

        // the Fedora theme: no gaps
        assert!(eng.set_config(Config {
            enabled: true,
            gap_outer: 0,
            gap_inner: 0,
        }));
        let json = eng.resolve_json("halves", fhd).unwrap();
        assert_eq!(
            json,
            r#"[{"x":0,"y":0,"w":960,"h":1080},{"x":960,"y":0,"w":960,"h":1080}]"#
        );
        assert!(!eng.set_config(eng.config.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refusals() {
        let dir = tmp("refuse");
        let mut eng = engine_in(&dir, Config::default());
        let fhd = Rect::new(0, 0, 1920, 1080);
        assert!(matches!(
            eng.resolve_json("nope", fhd),
            Err(LayoutsError::Core(desk_layouts::Error::NotFound(_)))
        ));
        assert!(matches!(
            eng.resolve_json("halves", Rect::new(0, 0, 0, 1080)),
            Err(LayoutsError::BadArgs(_))
        ));
        for key in ["", "a\nb", &"x".repeat(MAX_MONITOR_KEY + 1)] {
            assert!(matches!(eng.active(key), Err(LayoutsError::BadArgs(_))));
            assert!(matches!(
                eng.set_active(key, "halves"),
                Err(LayoutsError::BadArgs(_))
            ));
        }
        assert!(!dir.join("layouts.json").exists(), "nothing written");

        eng.set_config(Config {
            enabled: false,
            ..Config::default()
        });
        assert!(matches!(eng.list_json(), Err(LayoutsError::Disabled)));
        assert!(matches!(eng.active("DP-1"), Err(LayoutsError::Disabled)));
        assert!(matches!(
            eng.set_active("DP-1", "thirds"),
            Err(LayoutsError::Disabled)
        ));
        assert!(matches!(
            eng.resolve_json("halves", fhd),
            Err(LayoutsError::Disabled)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_layout_is_saved_per_monitor() {
        let dir = tmp("active");
        let mut eng = engine_in(&dir, Config::default());
        assert_eq!(eng.active("DP-1").unwrap(), desk_layouts::DEFAULT_LAYOUT_ID);
        assert!(eng.set_active("DP-1", "focus").unwrap());
        assert!(!eng.set_active("DP-1", "focus").unwrap());

        let again = engine_in(&dir, Config::default());
        assert_eq!(again.active("DP-1").unwrap(), "focus");
        assert_eq!(again.active("HDMI-1").unwrap(), "halves");
        let list: serde_json::Value = serde_json::from_str(&again.list_json().unwrap()).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
