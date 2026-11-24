// SPDX-License-Identifier: GPL-3.0-or-later
//! The pinned applications of the taskbar and the Start flyout, chosen from the installed XDG
//! desktop entries (no GTK). A slot lists known desktop ids in order of preference; the first
//! installed one wins, and a slot with none installed is skipped.

use desk_search::DesktopEntry;

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
}
