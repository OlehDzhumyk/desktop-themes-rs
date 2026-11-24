// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic 0600 JSON persistence (`layouts.json`) for custom layouts and per-monitor active layout
//! identifiers (`docs/spec/features/window-layouts.md`).

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_LAYOUT_ID, Error, Layout, builtin, builtins};

const STORE_VERSION: u32 = 1;
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreData {
    version: u32,
    #[serde(default)]
    custom_layouts: Vec<Layout>,
    #[serde(default)]
    active_by_monitor: BTreeMap<String, String>,
}

/// Persistent store of custom layouts and per-monitor active layout selections.
#[derive(Clone, Debug)]
pub struct LayoutStore {
    path: PathBuf,
    custom_layouts: Vec<Layout>,
    active_by_monitor: BTreeMap<String, String>,
}

impl LayoutStore {
    /// Default path: `$XDG_DATA_HOME/deskd/layouts.json`
    /// (fallback `~/.local/share/deskd/layouts.json`).
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        data.join("deskd/layouts.json")
    }

    /// Load the store from `path`. A missing or damaged file starts clean without failing.
    pub fn load(path: PathBuf) -> Self {
        let mut store = Self {
            path,
            custom_layouts: Vec::new(),
            active_by_monitor: BTreeMap::new(),
        };
        let Ok(bytes) = fs::read(&store.path) else {
            return store;
        };
        let Ok(data) = serde_json::from_slice::<StoreData>(&bytes) else {
            return store;
        };
        for mut layout in data.custom_layouts {
            layout.builtin = false;
            if builtin(&layout.id).is_some() {
                continue;
            }
            if layout.validate().is_ok() && !store.custom_layouts.iter().any(|l| l.id == layout.id)
            {
                store.custom_layouts.push(layout);
            }
        }
        for (monitor, layout_id) in data.active_by_monitor {
            if !monitor.is_empty() && store.get_layout(&layout_id).is_some() {
                store.active_by_monitor.insert(monitor, layout_id);
            }
        }
        store
    }

    /// Path of the backing `layouts.json` file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All layouts: the six built-ins followed by user custom layouts.
    pub fn all_layouts(&self) -> Vec<Layout> {
        let mut list = builtins();
        list.extend(self.custom_layouts.iter().cloned());
        list
    }

    /// Look up any layout (built-in or custom) by identifier.
    pub fn get_layout(&self, id: &str) -> Option<Layout> {
        builtin(id).or_else(|| {
            self.custom_layouts
                .iter()
                .find(|layout| layout.id == id)
                .cloned()
        })
    }

    /// Active layout identifier for `monitor_key` (defaults to [`DEFAULT_LAYOUT_ID`]).
    pub fn active_layout(&self, monitor_key: &str) -> &str {
        self.active_by_monitor
            .get(monitor_key)
            .map(String::as_str)
            .filter(|id| self.get_layout(id).is_some())
            .unwrap_or(DEFAULT_LAYOUT_ID)
    }

    /// Set the active layout for `monitor_key` and persist atomically.
    /// Returns `Ok(true)` when the selection changed, `Ok(false)` when it was already active.
    pub fn set_active_layout(&mut self, monitor_key: &str, layout_id: &str) -> Result<bool, Error> {
        if monitor_key.is_empty() {
            return Err(Error::InvalidLayout("monitor key must not be empty".into()));
        }
        if self.get_layout(layout_id).is_none() {
            return Err(Error::NotFound(layout_id.to_string()));
        }
        if self.active_by_monitor.get(monitor_key).map(String::as_str) == Some(layout_id) {
            return Ok(false);
        }
        self.active_by_monitor
            .insert(monitor_key.to_string(), layout_id.to_string());
        self.save()?;
        Ok(true)
    }

    /// Add or replace a custom layout and persist atomically. Built-in IDs cannot be overwritten.
    pub fn save_custom_layout(&mut self, mut layout: Layout) -> Result<(), Error> {
        layout.builtin = false;
        layout.validate()?;
        if builtin(&layout.id).is_some() {
            return Err(Error::InvalidLayout(format!(
                "cannot overwrite built-in layout '{}'",
                layout.id
            )));
        }
        if let Some(existing) = self.custom_layouts.iter_mut().find(|l| l.id == layout.id) {
            *existing = layout;
        } else {
            self.custom_layouts.push(layout);
        }
        self.save()
    }

    /// Delete a custom layout by ID. Returns `Ok(false)` if it did not exist; refuses built-ins.
    pub fn delete_custom_layout(&mut self, id: &str) -> Result<bool, Error> {
        if builtin(id).is_some() {
            return Err(Error::InvalidLayout(format!(
                "cannot delete built-in layout '{id}'"
            )));
        }
        let before = self.custom_layouts.len();
        self.custom_layouts.retain(|l| l.id != id);
        if self.custom_layouts.len() == before {
            return Ok(false);
        }
        self.active_by_monitor.retain(|_, v| v != id);
        self.save()?;
        Ok(true)
    }

    /// Write the current state atomically with mode 0600 inside a 0700 directory.
    pub fn save(&self) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }
        let data = StoreData {
            version: STORE_VERSION,
            custom_layouts: self.custom_layouts.clone(),
            active_by_monitor: self.active_by_monitor.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&data)?;
        write_private_atomic(&self.path, &bytes)?;
        Ok(())
    }
}

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("layouts.json");
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.tmp.{}.{seq}", std::process::id()));

    let write_res = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Zone;

    fn temp_store_path(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("desk-layouts-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("deskd/layouts.json")
    }

    fn cleanup(path: &Path) {
        if let Some(root) = path.parent().and_then(Path::parent) {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn persistence_round_trip_and_permissions() {
        let path = temp_store_path("roundtrip");
        let mut store = LayoutStore::load(path.clone());
        assert_eq!(store.all_layouts().len(), 6);
        assert_eq!(store.active_layout("DP-1"), DEFAULT_LAYOUT_ID);

        let custom = Layout {
            id: "ultrawide".into(),
            name: "Ultrawide".into(),
            builtin: false,
            zones: vec![
                Zone::new(0.0, 0.0, 0.2, 1.0),
                Zone::new(0.2, 0.0, 0.6, 1.0),
                Zone::new(0.8, 0.0, 0.2, 1.0),
            ],
        };
        store.save_custom_layout(custom.clone()).unwrap();
        assert!(store.set_active_layout("DP-1", "ultrawide").unwrap());
        assert!(!store.set_active_layout("DP-1", "ultrawide").unwrap());
        assert!(store.set_active_layout("HDMI-1", "focus").unwrap());

        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);

        let reloaded = LayoutStore::load(path.clone());
        assert_eq!(reloaded.all_layouts().len(), 7);
        assert_eq!(reloaded.get_layout("ultrawide"), Some(custom));
        assert_eq!(reloaded.active_layout("DP-1"), "ultrawide");
        assert_eq!(reloaded.active_layout("HDMI-1"), "focus");
        assert_eq!(reloaded.active_layout("eDP-1"), DEFAULT_LAYOUT_ID);

        cleanup(&path);
    }

    #[test]
    fn refuses_unknown_layout_and_builtin_overwrite() {
        let path = temp_store_path("refuse");
        let mut store = LayoutStore::load(path.clone());

        assert!(matches!(
            store.set_active_layout("DP-1", "does-not-exist"),
            Err(Error::NotFound(_))
        ));
        assert!(store.set_active_layout("", "halves").is_err());

        let overwrite = Layout {
            id: "halves".into(),
            name: "Fake Halves".into(),
            builtin: false,
            zones: vec![Zone::new(0.0, 0.0, 1.0, 1.0)],
        };
        assert!(store.save_custom_layout(overwrite).is_err());
        assert!(store.delete_custom_layout("halves").is_err());

        cleanup(&path);
    }

    #[test]
    fn delete_custom_layout_clears_active_reference() {
        let path = temp_store_path("delete");
        let mut store = LayoutStore::load(path.clone());
        store
            .save_custom_layout(Layout {
                id: "custom-two".into(),
                name: "Custom Two".into(),
                builtin: false,
                zones: vec![Zone::new(0.0, 0.0, 0.7, 1.0), Zone::new(0.7, 0.0, 0.3, 1.0)],
            })
            .unwrap();
        store.set_active_layout("DP-1", "custom-two").unwrap();
        assert!(store.delete_custom_layout("custom-two").unwrap());
        assert!(!store.delete_custom_layout("custom-two").unwrap());
        assert_eq!(store.active_layout("DP-1"), DEFAULT_LAYOUT_ID);

        let reloaded = LayoutStore::load(path.clone());
        assert_eq!(reloaded.active_layout("DP-1"), DEFAULT_LAYOUT_ID);
        assert!(reloaded.get_layout("custom-two").is_none());

        cleanup(&path);
    }
}
