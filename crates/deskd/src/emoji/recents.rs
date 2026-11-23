// SPDX-License-Identifier: GPL-3.0-or-later
//! Local persistence of recently picked emoji and symbols (`~/.local/share/deskd/emoji.json`,
//! mode 0600, max 36 items, newest first).

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::clipboard::{ensure_private_dir, write_private};

pub const MAX_RECENTS: usize = 36;
const STORE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    recents: Vec<String>,
}

pub struct Recents {
    path: PathBuf,
    ids: Vec<String>,
    remember: bool,
}

impl Recents {
    pub fn new(path: PathBuf, remember: bool) -> Self {
        Self {
            path,
            ids: Vec::new(),
            remember,
        }
    }

    /// Default path: `$XDG_DATA_HOME/deskd/emoji.json` (fallback `~/.local/share/deskd/emoji.json`).
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(|| "/tmp".into(), PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        data.join("deskd/emoji.json")
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn load(&mut self) {
        if !self.remember {
            self.ids.clear();
            return;
        }
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        let Ok(store) = serde_json::from_slice::<Store>(&bytes) else {
            eprintln!("deskd: {} is damaged, ignoring it", self.path.display());
            return;
        };
        self.ids.clear();
        for id in store.recents {
            if !self.ids.contains(&id) {
                self.ids.push(id);
                if self.ids.len() >= MAX_RECENTS {
                    break;
                }
            }
        }
    }

    pub fn record(&mut self, id: &str) {
        if !self.remember {
            return;
        }
        if let Some(pos) = self.ids.iter().position(|x| x == id) {
            self.ids.remove(pos);
        }
        self.ids.insert(0, id.to_string());
        self.ids.truncate(MAX_RECENTS);
        if let Err(e) = self.save() {
            eprintln!("deskd: cannot save emoji recents: {e}");
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn set_remember(&mut self, remember: bool) {
        self.remember = remember;
        if !remember {
            self.clear();
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if !self.remember {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }
        let store = Store {
            version: STORE_VERSION,
            recents: self.ids.clone(),
        };
        let json = serde_json::to_vec(&store).map_err(std::io::Error::other)?;
        write_private(&self.path, &json)
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
