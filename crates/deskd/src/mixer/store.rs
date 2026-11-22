// SPDX-License-Identifier: GPL-3.0-or-later
//! The per-application volume memory (GSettings `remember-app-volume`):
//! `$XDG_DATA_HOME/deskd/mixer/apps.json`, mode 0600 in a 0700 directory. It holds application keys
//! (app ids or program names) with a volume and a mute flag, nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::Remembered;
use crate::clipboard::{ensure_private_dir, write_private};

pub fn default_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/tmp".into());
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
        .join("deskd/mixer/apps.json")
}

/// The memory on disk; empty when missing or unreadable.
pub fn load(path: &Path) -> BTreeMap<String, Remembered> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write the memory; an empty memory removes the file (clearing is real deletion).
pub fn save(path: &Path, memory: &BTreeMap<String, Remembered>) -> std::io::Result<()> {
    if memory.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        ensure_private_dir(dir)?;
    }
    let data = serde_json::to_vec_pretty(memory).map_err(std::io::Error::other)?;
    write_private(path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_private_and_cleared() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("deskd-mixer-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("mixer/apps.json");
        assert!(load(&path).is_empty());
        let mut m = BTreeMap::new();
        m.insert(
            "mpv".to_string(),
            Remembered {
                volume: 0.4,
                mute: true,
            },
        );
        save(&path, &m).unwrap();
        assert_eq!(load(&path), m);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        save(&path, &BTreeMap::new()).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
