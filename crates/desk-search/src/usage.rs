// SPDX-License-Identifier: GPL-3.0-or-later
//! The usage store (`search-usage.json`, 0600, written atomically): how often each result was
//! launched, with exponential decay, to break ties in the ranking. With `remember_usage` off it
//! reads nothing, writes nothing and deletes the file (AC-SRCH-07).

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::Error;

const STORE_VERSION: u32 = 1;
/// A launch counts half as much after this many seconds (14 days).
pub const HALF_LIFE_SECS: u64 = 14 * 24 * 60 * 60;
/// Items whose decayed count falls below this are forgotten when the file is written.
pub const FORGET_BELOW: f64 = 0.05;
/// At most this many items are kept (the most used).
pub const MAX_ITEMS: usize = 500;
/// The longest item id accepted, in bytes.
pub const MAX_ID_BYTES: usize = 512;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The decayed count of one item at the time of its last launch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub count: f64,
    /// Unix seconds of the last launch.
    pub last: u64,
}

impl Usage {
    /// The count decayed to `now`.
    pub fn at(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.last) as f64;
        self.count * 0.5f64.powf(age / HALF_LIFE_SECS as f64)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct StoreData {
    version: u32,
    #[serde(default)]
    items: BTreeMap<String, Usage>,
}

/// The usage counts of launched items.
#[derive(Clone, Debug)]
pub struct UsageStore {
    path: PathBuf,
    remember: bool,
    items: BTreeMap<String, Usage>,
}

impl UsageStore {
    /// Default path: `$XDG_DATA_HOME/deskd/search-usage.json`
    /// (fallback `~/.local/share/deskd/search-usage.json`).
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        data.join("deskd/search-usage.json")
    }

    /// A store that keeps nothing (no file is touched).
    pub fn disabled(path: PathBuf) -> UsageStore {
        UsageStore {
            path,
            remember: false,
            items: BTreeMap::new(),
        }
    }

    /// Load from `path`. A missing or damaged file starts empty. With `remember` off the file is
    /// not read and is deleted if it exists.
    pub fn load(path: PathBuf, remember: bool) -> UsageStore {
        let mut store = UsageStore::disabled(path);
        if !remember {
            let _ = fs::remove_file(&store.path);
            return store;
        }
        store.remember = true;
        let Ok(bytes) = fs::read(&store.path) else {
            return store;
        };
        if let Ok(data) = serde_json::from_slice::<StoreData>(&bytes) {
            store.items = data
                .items
                .into_iter()
                .filter(|(id, u)| valid_id(id) && u.count.is_finite() && u.count > 0.0)
                .collect();
        }
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remembers(&self) -> bool {
        self.remember
    }

    /// The decayed launch count of `id` at `now` (0 when unknown or not remembering).
    pub fn weight(&self, id: &str, now: u64) -> f64 {
        self.items.get(id).map_or(0.0, |u| u.at(now))
    }

    /// The remembered ids with their weight at `now`, most used first.
    pub fn top(&self, now: u64) -> Vec<(&str, f64)> {
        let mut v: Vec<(&str, f64)> = self
            .items
            .iter()
            .map(|(id, u)| (id.as_str(), u.at(now)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v
    }

    /// Count one launch of `id` and save. Returns `Ok(false)` and writes nothing when the store
    /// does not remember (AC-SRCH-07).
    pub fn record(&mut self, id: &str, now: u64) -> Result<bool, Error> {
        if !valid_id(id) {
            return Err(Error::InvalidId(id.chars().take(64).collect()));
        }
        if !self.remember {
            return Ok(false);
        }
        let count = self.weight(id, now) + 1.0;
        self.items.insert(id.to_owned(), Usage { count, last: now });
        self.prune(now);
        self.save()?;
        Ok(true)
    }

    /// Forget everything and delete the file.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.items.clear();
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io(e)),
            _ => Ok(()),
        }
    }

    /// Turn remembering on or off. Off forgets everything and deletes the file; on reads the
    /// file again.
    pub fn set_remember(&mut self, remember: bool) {
        if remember == self.remember {
            return;
        }
        *self = UsageStore::load(self.path.clone(), remember);
    }

    fn prune(&mut self, now: u64) {
        self.items.retain(|_, u| u.at(now) >= FORGET_BELOW);
        if self.items.len() > MAX_ITEMS {
            let keep: Vec<String> = self
                .top(now)
                .into_iter()
                .take(MAX_ITEMS)
                .map(|(id, _)| id.to_owned())
                .collect();
            self.items.retain(|id, _| keep.contains(id));
        }
    }

    fn save(&self) -> Result<(), Error> {
        if !self.remember {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }
        let data = StoreData {
            version: STORE_VERSION,
            items: self.items.clone(),
        };
        write_private_atomic(&self.path, &serde_json::to_vec(&data)?)?;
        Ok(())
    }
}

/// An id is 1 to [`MAX_ID_BYTES`] bytes without control characters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_BYTES && !id.chars().any(char::is_control)
}

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("search-usage.json");
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{name}.tmp.{}.{seq}", std::process::id()));
    let res = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "desk-search-{tag}-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn records_decays_and_persists_0600() {
        let dir = tmp("usage");
        let path = dir.join("deskd/search-usage.json");
        let mut s = UsageStore::load(path.clone(), true);
        assert!(s.record("app:org.gnome.TextEditor.desktop", 1_000).unwrap());
        assert!(s.record("app:org.gnome.TextEditor.desktop", 1_000).unwrap());
        assert!(s.record("action:lock", 1_000).unwrap());
        assert!((s.weight("app:org.gnome.TextEditor.desktop", 1_000) - 2.0).abs() < 1e-9);
        // one half-life later the count is halved
        let later = 1_000 + HALF_LIFE_SECS;
        assert!((s.weight("app:org.gnome.TextEditor.desktop", later) - 1.0).abs() < 1e-9);
        assert_eq!(s.weight("unknown", later), 0.0);
        assert_eq!(s.top(1_000)[0].0, "app:org.gnome.TextEditor.desktop");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let again = UsageStore::load(path.clone(), true);
        assert!((again.weight("action:lock", 1_000) - 1.0).abs() < 1e-9);
        // no temporary files are left behind
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(names.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_items_are_forgotten() {
        let dir = tmp("forget");
        let mut s = UsageStore::load(dir.join("u.json"), true);
        s.record("old", 0).unwrap();
        // 1 * 0.5^(t/half) < 0.05 after about 4.33 half-lives
        s.record("new", 5 * HALF_LIFE_SECS).unwrap();
        assert_eq!(s.weight("old", 5 * HALF_LIFE_SECS), 0.0);
        assert!(s.weight("new", 5 * HALF_LIFE_SECS) > 0.9);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ac_srch_07_no_record_when_not_remembering() {
        let dir = tmp("off");
        let path = dir.join("search-usage.json");
        let mut s = UsageStore::load(path.clone(), false);
        assert!(!s.record("app:x.desktop", 10 * DAY).unwrap());
        assert!(!path.exists());
        assert!(!dir.exists(), "not even the folder is made");

        // turning it off later deletes what was there
        let mut s2 = UsageStore::load(path.clone(), true);
        s2.record("app:x.desktop", DAY).unwrap();
        assert!(path.exists());
        s2.set_remember(false);
        assert!(!path.exists());
        assert_eq!(s2.weight("app:x.desktop", DAY), 0.0);
        assert!(!s2.record("app:x.desktop", DAY).unwrap());
        assert!(!path.exists());

        // a file left from before is deleted on load with remembering off
        let mut s3 = UsageStore::load(path.clone(), true);
        s3.record("a", DAY).unwrap();
        let s4 = UsageStore::load(path.clone(), false);
        assert!(!path.exists());
        assert!(!s4.remembers());
        s.set_remember(true);
        assert!(s.remembers());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_and_bad_input() {
        let dir = tmp("clear");
        let path = dir.join("u.json");
        let mut s = UsageStore::load(path.clone(), true);
        s.record("a", 1).unwrap();
        s.clear().unwrap();
        assert!(!path.exists());
        assert_eq!(s.weight("a", 1), 0.0);
        s.clear().unwrap(); // clearing twice is fine

        for bad in ["", "a\nb", &"x".repeat(MAX_ID_BYTES + 1)] {
            assert!(matches!(s.record(bad, 1), Err(Error::InvalidId(_))));
        }
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, b"{ damaged").unwrap();
        assert_eq!(UsageStore::load(path.clone(), true).top(1).len(), 0);
        fs::write(
            &path,
            br#"{"version":1,"items":{"ok":{"count":2.0,"last":1},"bad\u0001":{"count":1.0,"last":1},"neg":{"count":-1.0,"last":1}}}"#,
        )
        .unwrap();
        let loaded = UsageStore::load(path.clone(), true);
        assert_eq!(loaded.top(1), [("ok", 2.0)]);
        let _ = fs::remove_dir_all(&dir);
    }
}
