// SPDX-License-Identifier: GPL-3.0-or-later
//! The pinned applications of the taskbar and the Start flyout, chosen from the installed XDG
//! desktop entries (no GTK). A slot lists known desktop ids in order of preference; the first
//! installed one wins, and a slot with none installed is skipped.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use desk_search::DesktopEntry;

/// Subdirectory under `$XDG_CONFIG_HOME` holding user taskbar configuration.
const CONFIG_DIR: &str = "desk-shell";
/// Filename storing user-pinned desktop entry ids.
const PINNED_FILE: &str = "pinned.json";
/// Bound on `pinned.json` size (64 KiB).
const MAX_PINNED_BYTES: u64 = 64 * 1024;

/// One pinned place.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// Stable name, for tests and logs.
    pub name: &'static str,
    /// Desktop ids, most preferred first.
    pub candidates: &'static [&'static str],
}

/// The desktop id of the settings hub (`themes gui`).
pub const THEMES_ID: &str = "io.github.OlehDzhumyk.Themes.desktop";
/// The slot that the default web browser of the session fills first.
pub const BROWSER: &str = "browser";

/// The slots, in taskbar order. The taskbar shows the first [`TASKBAR_SLOTS`].
pub static SLOTS: &[Slot] = &[
    Slot {
        name: "files",
        candidates: &[
            "org.gnome.Nautilus.desktop",
            "org.kde.dolphin.desktop",
            "thunar.desktop",
            "nemo.desktop",
            "pcmanfm.desktop",
            "pcmanfm-qt.desktop",
        ],
    },
    Slot {
        name: "terminal",
        candidates: &[
            "org.gnome.Ptyxis.desktop",
            "org.gnome.Console.desktop",
            "org.gnome.Terminal.desktop",
            "foot.desktop",
            "Alacritty.desktop",
            "kitty.desktop",
            "org.wezfurlong.wezterm.desktop",
            "org.kde.konsole.desktop",
        ],
    },
    Slot {
        name: BROWSER,
        candidates: &[
            "org.mozilla.firefox.desktop",
            "firefox.desktop",
            "org.chromium.Chromium.desktop",
            "chromium-browser.desktop",
            "google-chrome.desktop",
            "com.brave.Browser.desktop",
            "org.gnome.Epiphany.desktop",
        ],
    },
    Slot {
        name: "settings",
        candidates: &[THEMES_ID],
    },
    Slot {
        name: "editor",
        candidates: &[
            "org.gnome.TextEditor.desktop",
            "org.kde.kate.desktop",
            "org.gnome.gedit.desktop",
            "mousepad.desktop",
        ],
    },
    Slot {
        name: "calculator",
        candidates: &["org.gnome.Calculator.desktop", "org.kde.kcalc.desktop"],
    },
    Slot {
        name: "software",
        candidates: &["org.gnome.Software.desktop", "org.kde.discover.desktop"],
    },
    Slot {
        name: "photos",
        candidates: &["org.gnome.Loupe.desktop", "org.gnome.eog.desktop"],
    },
];

/// How many slots the taskbar shows.
pub const TASKBAR_SLOTS: usize = 5;
/// The Start flyout grid: 4 columns × 3 rows.
pub const GRID_COLUMNS: usize = 4;
pub const GRID_TILES: usize = 12;

/// The entry of each slot that has one installed, in slot order, without duplicates.
/// `default_browser` (the session's handler of `https`) is tried first for [`BROWSER`].
pub fn resolve<'a>(
    slots: &[Slot],
    entries: &'a [DesktopEntry],
    default_browser: Option<&str>,
) -> Vec<&'a DesktopEntry> {
    let find = |id: &str| entries.iter().find(|e| e.id == id);
    let mut out: Vec<&DesktopEntry> = Vec::new();
    for slot in slots {
        let preferred = default_browser.filter(|_| slot.name == BROWSER);
        let found = preferred
            .into_iter()
            .chain(slot.candidates.iter().copied())
            .find_map(find);
        if let Some(e) = found
            && !out.iter().any(|o| o.id == e.id)
        {
            out.push(e);
        }
    }
    out
}

/// The taskbar pins.
pub fn taskbar<'a>(
    entries: &'a [DesktopEntry],
    default_browser: Option<&str>,
) -> Vec<&'a DesktopEntry> {
    resolve(&SLOTS[..TASKBAR_SLOTS], entries, default_browser)
}

/// The Start grid: every resolved slot, then other applications by name, up to [`GRID_TILES`].
pub fn start_grid<'a>(
    entries: &'a [DesktopEntry],
    default_browser: Option<&str>,
) -> Vec<&'a DesktopEntry> {
    let mut out = resolve(SLOTS, entries, default_browser);
    let mut rest: Vec<&DesktopEntry> = entries
        .iter()
        .filter(|e| !out.iter().any(|o| o.id == e.id))
        .collect();
    rest.sort_by_cached_key(|e| e.display_name().to_lowercase());
    out.extend(rest);
    out.truncate(GRID_TILES);
    out
}

/// Resolve `$XDG_CONFIG_HOME/desk-shell/pinned.json`, falling back to
/// `$HOME/.config/desk-shell/pinned.json`.
pub fn pinned_file_path_from(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join(CONFIG_DIR).join(PINNED_FILE));
    }
    home.filter(|s| !s.is_empty()).map(|h| {
        PathBuf::from(h)
            .join(".config")
            .join(CONFIG_DIR)
            .join(PINNED_FILE)
    })
}

/// Resolve the user pins file path from the process environment.
pub fn pinned_file_path() -> Option<PathBuf> {
    pinned_file_path_from(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// Read user-pinned desktop ids from `path`. Returns `None` when the file is missing, unreadable,
/// oversized, or not a JSON array of strings.
pub fn load_user_pins(path: &Path) -> Option<Vec<String>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PINNED_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Vec<String>>(&text).ok()
}

/// Atomically write user-pinned desktop ids to `path` as a JSON array of strings.
pub fn save_user_pins(path: &Path, ids: &[String]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "pinned.json path has no parent directory",
        )
    })?;
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
    }
    let mut json = serde_json::to_string_pretty(ids)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');

    let pid = std::process::id();
    let (tmp_path, mut file) = loop {
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{PINNED_FILE}.{pid}.{seq}.tmp"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };

    if let Err(e) = file.write_all(json.as_bytes()) {
        drop(file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = file.sync_all() {
        drop(file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// Resolve user-saved desktop ids against installed `entries`, preserving order, deduplicating,
/// and skipping missing entries. An empty list is allowed (all pins removed).
pub fn resolve_user_pins<'a>(ids: &[String], entries: &'a [DesktopEntry]) -> Vec<&'a DesktopEntry> {
    let mut out: Vec<&'a DesktopEntry> = Vec::new();
    for id in ids {
        if let Some(e) = entries.iter().find(|e| e.id == *id)
            && !out.iter().any(|o| o.id == e.id)
        {
            out.push(e);
        }
    }
    out
}

/// Toggle `desktop_id` in `ids`: remove all occurrences and return `false` if present, otherwise
/// append `desktop_id` and return `true`.
pub fn toggle_pin(ids: &mut Vec<String>, desktop_id: &str) -> bool {
    let before = ids.len();
    ids.retain(|id| id != desktop_id);
    if ids.len() < before {
        false
    } else {
        ids.push(desktop_id.to_owned());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: name.into(),
            ..DesktopEntry::default()
        }
    }

    fn ids(v: &[&DesktopEntry]) -> Vec<String> {
        v.iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn missing_slots_are_skipped_and_preference_order_wins() {
        let installed = [
            entry("foot.desktop", "Foot"),
            entry("org.gnome.Ptyxis.desktop", "Ptyxis"),
            entry("firefox.desktop", "Firefox"),
            entry(THEMES_ID, "Themes"),
        ];
        // no file manager installed: the slot is skipped; Ptyxis beats foot
        assert_eq!(
            ids(&taskbar(&installed, None)),
            ["org.gnome.Ptyxis.desktop", "firefox.desktop", THEMES_ID]
        );
        assert!(taskbar(&[], None).is_empty());
    }

    #[test]
    fn the_default_browser_fills_the_browser_slot() {
        let installed = [
            entry("firefox.desktop", "Firefox"),
            entry("com.example.Browser.desktop", "Example"),
        ];
        assert_eq!(
            ids(&taskbar(&installed, Some("com.example.Browser.desktop"))),
            ["com.example.Browser.desktop"]
        );
        // a default browser that is not installed falls back to the list
        assert_eq!(
            ids(&taskbar(&installed, Some("gone.desktop"))),
            ["firefox.desktop"]
        );
        // a default handler that is a pinned app of another slot is not shown twice
        let both = [entry("foot.desktop", "Foot")];
        assert_eq!(ids(&taskbar(&both, Some("foot.desktop"))), ["foot.desktop"]);
    }

    #[test]
    fn the_start_grid_is_filled_by_name_and_capped() {
        let mut installed: Vec<DesktopEntry> = (0..20)
            .map(|i| entry(&format!("app{i:02}.desktop"), &format!("App {i:02}")))
            .collect();
        installed.push(entry("org.gnome.Nautilus.desktop", "Files"));
        installed.push(entry("zeta.desktop", "alpha"));
        let grid = start_grid(&installed, None);
        assert_eq!(grid.len(), GRID_TILES);
        assert_eq!(grid[0].id, "org.gnome.Nautilus.desktop");
        // case-insensitive name order: "alpha" sorts before "App 00"
        assert_eq!(grid[1].id, "zeta.desktop");
        assert_eq!(grid[2].id, "app00.desktop");
        assert_eq!(GRID_TILES, GRID_COLUMNS * 3);
    }

    #[test]
    fn slot_names_are_unique() {
        let mut names: Vec<&str> = SLOTS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SLOTS.len());
        assert!(SLOTS.iter().any(|s| s.name == BROWSER));
    }

    #[test]
    fn pinned_file_path_respects_xdg_config_home_and_home() {
        let home_pins = PathBuf::from("/home/user/.config/desk-shell/pinned.json");
        assert_eq!(
            pinned_file_path_from(Some("/tmp/xdg"), Some("/home/user")),
            Some(PathBuf::from("/tmp/xdg/desk-shell/pinned.json"))
        );
        assert_eq!(
            pinned_file_path_from(Some(""), Some("/home/user")),
            Some(home_pins.clone())
        );
        assert_eq!(
            pinned_file_path_from(None, Some("/home/user")),
            Some(home_pins)
        );
        assert_eq!(pinned_file_path_from(None, None), None);
        assert_eq!(pinned_file_path_from(Some(""), Some("")), None);
    }

    #[test]
    fn user_pins_load_save_and_fall_back_on_missing_or_corrupt() {
        let dir = std::env::temp_dir().join(format!("desk-shell-pins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("desk-shell").join("pinned.json");

        assert_eq!(load_user_pins(&path), None);

        let saved = vec!["a.desktop".to_owned(), "b.desktop".to_owned()];
        save_user_pins(&path, &saved).unwrap();
        assert_eq!(load_user_pins(&path), Some(saved));

        // Explicit empty list is preserved (user unpinned everything)
        save_user_pins(&path, &[]).unwrap();
        assert_eq!(load_user_pins(&path), Some(Vec::new()));

        // Invalid JSON or wrong JSON shape falls back to None
        std::fs::write(&path, "{not valid json").unwrap();
        assert_eq!(load_user_pins(&path), None);
        std::fs::write(&path, r#"{"pins": []}"#).unwrap();
        assert_eq!(load_user_pins(&path), None);
        std::fs::write(&path, "[1, 2, 3]").unwrap();
        assert_eq!(load_user_pins(&path), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_user_pins_deduplicates_skips_missing_and_allows_empty() {
        let installed = [
            entry("a.desktop", "A"),
            entry("b.desktop", "B"),
            entry("c.desktop", "C"),
        ];
        let want: Vec<String> = ["c.desktop", "missing.desktop", "a.desktop", "c.desktop"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            ids(&resolve_user_pins(&want, &installed)),
            ["c.desktop", "a.desktop"]
        );
        assert!(resolve_user_pins(&[], &installed).is_empty());
    }

    #[test]
    fn toggle_pin_adds_and_removes_all_occurrences() {
        let mut pins = vec!["a.desktop".to_owned()];
        assert!(toggle_pin(&mut pins, "b.desktop"));
        assert_eq!(pins, ["a.desktop", "b.desktop"]);
        assert!(!toggle_pin(&mut pins, "a.desktop"));
        assert_eq!(pins, ["b.desktop"]);

        let mut dup = vec![
            "x.desktop".to_owned(),
            "y.desktop".to_owned(),
            "x.desktop".to_owned(),
        ];
        assert!(!toggle_pin(&mut dup, "x.desktop"));
        assert_eq!(dup, ["y.desktop"]);
    }
}
