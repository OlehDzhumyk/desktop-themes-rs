// SPDX-License-Identifier: GPL-3.0-or-later
//! Which taskbar button shows which running window (no GTK).
//!
//! A window belongs to a pinned application when its Wayland `app_id` names the desktop entry
//! (case-insensitive, without `.desktop`, or the last dotted part of one: `firefox` and
//! `org.mozilla.firefox`). Windows of applications that are not pinned are grouped by `app_id`
//! after the pins, in the order they opened.

use desk_search::DesktopEntry;

use crate::toplevels::{Command, Toplevel};

/// The fallback icon of a window without a desktop entry.
pub const FALLBACK_ICON: &str = "application-x-executable-symbolic";

/// `org.gnome.Nautilus.desktop` → `org.gnome.nautilus`.
fn normalize(id: &str) -> String {
    let lower = id.to_lowercase();
    lower.strip_suffix(".desktop").unwrap_or(&lower).to_owned()
}

/// Strip common Wayland/browser wrapper suffixes (`-wayland`, `-browser`) for loose matching.
fn strip_variant(mut id: &str) -> &str {
    loop {
        if let Some(stem) = id
            .strip_suffix("-wayland")
            .or_else(|| id.strip_suffix("-browser"))
            && !stem.is_empty()
        {
            id = stem;
        } else {
            return id;
        }
    }
}

fn last_part(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// The window with `app_id` belongs to the desktop entry `desktop_id`.
pub fn matches(desktop_id: &str, app_id: &str) -> bool {
    let (entry, app) = (normalize(desktop_id), normalize(app_id));
    if entry.is_empty() || app.is_empty() {
        return false;
    }
    let (entry, app) = (strip_variant(&entry), strip_variant(&app));
    entry == app || last_part(entry) == app || last_part(app) == entry
}

/// The window with `app_id` belongs to `entry` (via its desktop id or `StartupWMClass`).
pub fn matches_entry(entry: &DesktopEntry, app_id: &str) -> bool {
    matches(&entry.id, app_id)
        || entry
            .startup_wm_class
            .as_deref()
            .is_some_and(|wm| matches(wm, app_id))
}

/// The desktop entry of `app_id`: an exact desktop-id match first, then an exact
/// `StartupWMClass` match, then a loose match ([`matches_entry`]).
pub fn entry_for<'a>(apps: &'a [DesktopEntry], app_id: &str) -> Option<&'a DesktopEntry> {
    let app = normalize(app_id);
    if app.is_empty() {
        return None;
    }
    apps.iter()
        .find(|e| normalize(&e.id) == app)
        .or_else(|| {
            apps.iter().find(|e| {
                e.startup_wm_class
                    .as_deref()
                    .is_some_and(|wm| normalize(wm) == app)
            })
        })
        .or_else(|| apps.iter().find(|e| matches_entry(e, app_id)))
}

/// The bar under a taskbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    None,
    /// At least one window is open.
    Running,
    /// One of its windows has the focus.
    Active,
}

/// The indicator of a button with `windows`.
pub fn indicator(windows: &[Toplevel]) -> Indicator {
    if windows.is_empty() {
        Indicator::None
    } else if windows.iter().any(is_focused) {
        Indicator::Active
    } else {
        Indicator::Running
    }
}

fn is_focused(w: &Toplevel) -> bool {
    w.state.activated && !w.state.minimized
}

/// The window a click or the menu acts on: the focused one, else the newest.
pub fn target(windows: &[Toplevel]) -> Option<&Toplevel> {
    windows.iter().find(|w| is_focused(w)).or(windows.last())
}

/// A left click on a button with windows: minimize the focused window, as Windows 11 does,
/// else bring the newest one forward. `None`: no window, launch the application.
pub fn click(windows: &[Toplevel]) -> Option<Command> {
    let w = target(windows)?;
    Some(if is_focused(w) {
        Command::ToggleMinimize(w.id)
    } else {
        Command::Activate(w.id)
    })
}

/// The windows of each pinned entry, and the other windows grouped by application.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Groups {
    /// One list per pin, in pin order.
    pub pinned: Vec<Vec<Toplevel>>,
    /// `(key, windows)`: the lower-case `app_id`, or `#<id>` for a window without one.
    pub unpinned: Vec<(String, Vec<Toplevel>)>,
}

fn group_by<P>(
    pins: &[P],
    windows: &[Toplevel],
    own_app_id: &str,
    pin_matches: impl Fn(&P, &str) -> bool,
) -> Groups {
    let mut g = Groups {
        pinned: vec![Vec::new(); pins.len()],
        unpinned: Vec::new(),
    };
    for w in windows {
        if !own_app_id.is_empty() && w.app_id.eq_ignore_ascii_case(own_app_id) {
            continue;
        }
        if let Some(i) = pins.iter().position(|pin| pin_matches(pin, &w.app_id)) {
            g.pinned[i].push(w.clone());
            continue;
        }
        let key = if w.app_id.is_empty() {
            format!("#{}", w.id)
        } else {
            normalize(&w.app_id)
        };
        match g.unpinned.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(w.clone()),
            None => g.unpinned.push((key, vec![w.clone()])),
        }
    }
    g
}

/// Group `windows` (in the order they opened) for the pins `pin_ids`. Windows of `own_app_id`
/// (the shell itself, in a preview) are left out.
#[cfg(test)]
pub fn group(pin_ids: &[&str], windows: &[Toplevel], own_app_id: &str) -> Groups {
    group_by(pin_ids, windows, own_app_id, |id, app| matches(id, app))
}

/// Group `windows` (in the order they opened) for the pinned [`DesktopEntry`] items (honouring
/// `StartupWMClass` in addition to desktop ids). Windows of `own_app_id` are left out.
pub fn group_entries(pins: &[DesktopEntry], windows: &[Toplevel], own_app_id: &str) -> Groups {
    group_by(pins, windows, own_app_id, matches_entry)
}

/// Apply an update to the list of open windows (kept in opening order).
pub fn apply(windows: &mut Vec<Toplevel>, update: crate::toplevels::Update) {
    use crate::toplevels::Update;
    match update {
        Update::Changed(t) => match windows.iter_mut().find(|w| w.id == t.id) {
            Some(w) => *w = t,
            None => windows.push(t),
        },
        Update::Closed(id) => windows.retain(|w| w.id != id),
        Update::Finished => windows.clear(),
    }
}

/// The tooltip of a button: the application name, then the title of each window.
pub fn tooltip(name: &str, windows: &[Toplevel]) -> String {
    let mut lines = vec![name.to_owned()];
    lines.extend(
        windows
            .iter()
            .map(|w| w.title.trim())
            .filter(|t| !t.is_empty() && *t != name)
            .map(str::to_owned),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toplevels::{State, Update};

    fn win(id: u32, app_id: &str, activated: bool) -> Toplevel {
        Toplevel {
            id,
            title: format!("window {id}"),
            app_id: app_id.into(),
            state: State {
                activated,
                ..State::default()
            },
        }
    }

    fn entry(id: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: id.into(),
            ..DesktopEntry::default()
        }
    }

    #[test]
    fn app_ids_match_desktop_ids() {
        assert!(matches("org.gnome.Nautilus.desktop", "org.gnome.Nautilus"));
        assert!(matches("org.gnome.Nautilus.desktop", "ORG.GNOME.NAUTILUS"));
        assert!(matches("org.mozilla.firefox.desktop", "firefox"));
        assert!(matches("foot.desktop", "foot"));
        assert!(matches("Alacritty.desktop", "org.alacritty.Alacritty"));
        assert!(!matches("org.gnome.Nautilus.desktop", "org.gnome.Ptyxis"));
        assert!(!matches("foot.desktop", ""));
        assert!(!matches("org.gnome.TextEditor.desktop", "editor"));
    }

    #[test]
    fn the_exact_entry_wins_over_a_loose_one() {
        let apps = [
            entry("firefox.desktop"),
            entry("org.mozilla.firefox.desktop"),
        ];
        assert_eq!(
            entry_for(&apps, "org.mozilla.firefox").unwrap().id,
            "org.mozilla.firefox.desktop"
        );
        assert_eq!(entry_for(&apps, "firefox").unwrap().id, "firefox.desktop");
        assert!(entry_for(&apps, "unknown").is_none());
        assert!(entry_for(&apps, "").is_none());
    }

    #[test]
    fn windows_go_to_their_pin_or_to_a_group_after_the_pins() {
        let windows = [
            win(1, "org.gnome.Nautilus", false),
            win(2, "gimp", false),
            win(3, "", false),
            win(4, "GIMP", true),
            win(5, "io.github.OlehDzhumyk.DeskShell", false),
            win(6, "firefox", false),
        ];
        let g = group(
            &["org.gnome.Nautilus.desktop", "org.mozilla.firefox.desktop"],
            &windows,
            "io.github.OlehDzhumyk.DeskShell",
        );
        assert_eq!(g.pinned[0], [windows[0].clone()]);
        assert_eq!(g.pinned[1], [windows[5].clone()]);
        let keys: Vec<&str> = g.unpinned.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["gimp", "#3"]);
        assert_eq!(g.unpinned[0].1, [windows[1].clone(), windows[3].clone()]);
        assert_eq!(group(&["a.desktop"], &[], "").pinned, [vec![]]);
    }

    #[test]
    fn a_click_minimizes_the_focused_window_else_activates_the_newest() {
        assert_eq!(click(&[]), None);
        let a = win(1, "x", false);
        let b = win(2, "x", true);
        assert_eq!(
            click(&[a.clone(), b.clone()]),
            Some(Command::ToggleMinimize(2))
        );
        assert_eq!(indicator(&[a.clone(), b.clone()]), Indicator::Active);
        let c = win(3, "x", false);
        assert_eq!(click(&[a.clone(), c.clone()]), Some(Command::Activate(3)));
        assert_eq!(indicator(std::slice::from_ref(&a)), Indicator::Running);
        assert_eq!(indicator(&[]), Indicator::None);
        // a minimized window that still reports `activated` is brought back, not minimized again
        let mut m = win(4, "x", true);
        m.state.minimized = true;
        assert_eq!(click(&[m.clone()]), Some(Command::Activate(4)));
        assert_eq!(indicator(&[m]), Indicator::Running);
    }

    #[test]
    fn updates_keep_the_opening_order() {
        let mut list = Vec::new();
        apply(&mut list, Update::Changed(win(1, "a", false)));
        apply(&mut list, Update::Changed(win(2, "b", false)));
        apply(&mut list, Update::Changed(win(1, "a", true)));
        assert_eq!(list, [win(1, "a", true), win(2, "b", false)]);
        apply(&mut list, Update::Closed(1));
        assert_eq!(list, [win(2, "b", false)]);
        apply(&mut list, Update::Finished);
        assert!(list.is_empty());
    }

    #[test]
    fn the_tooltip_lists_the_window_titles() {
        let mut w = win(1, "a", false);
        assert_eq!(tooltip("Files", &[]), "Files");
        assert_eq!(tooltip("Files", &[w.clone()]), "Files\nwindow 1");
        w.title = "Files".into();
        assert_eq!(tooltip("Files", &[w]), "Files");
    }

    fn entry_with_wm(id: &str, wm_class: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: id.into(),
            startup_wm_class: Some(wm_class.into()),
            ..DesktopEntry::default()
        }
    }

    #[test]
    fn variant_suffixes_and_startup_wm_class_match() {
        assert!(matches("org.mozilla.firefox.desktop", "firefox-wayland"));
        assert!(matches("firefox-wayland.desktop", "org.mozilla.firefox"));
        assert!(matches("chromium-browser.desktop", "chromium"));
        assert!(matches("org.chromium.Chromium.desktop", "chromium-browser"));
        assert!(matches("chromium.desktop", "chromium-browser-wayland"));
        assert!(!matches("-wayland", "wayland"));

        let spotify = entry_with_wm("com.spotify.Client.desktop", "Spotify");
        assert!(matches_entry(&spotify, "spotify"));
        assert!(matches_entry(&spotify, "Spotify"));
        assert!(!matches_entry(&spotify, "vlc"));
    }

    #[test]
    fn entry_for_prefers_exact_id_then_exact_wm_class_then_loose() {
        let loose = entry("org.mozilla.firefox.desktop");
        let wm = entry_with_wm("custom-browser.desktop", "Firefox");
        let exact = entry("firefox.desktop");

        // exact StartupWMClass beats loose desktop-id match
        let apps = [loose.clone(), wm.clone()];
        assert_eq!(
            entry_for(&apps, "firefox").unwrap().id,
            "custom-browser.desktop"
        );
        // exact desktop id beats exact StartupWMClass
        let apps = [wm, exact];
        assert_eq!(entry_for(&apps, "firefox").unwrap().id, "firefox.desktop");
        // loose matching handles -wayland and -browser suffixes
        let apps = [loose, entry("chromium-browser.desktop")];
        assert_eq!(
            entry_for(&apps, "firefox-wayland").unwrap().id,
            "org.mozilla.firefox.desktop"
        );
        assert_eq!(
            entry_for(&apps, "chromium").unwrap().id,
            "chromium-browser.desktop"
        );
    }

    #[test]
    fn group_entries_matches_startup_wm_class_and_suffixes() {
        let pins = [
            entry_with_wm("com.spotify.Client.desktop", "Spotify"),
            entry("org.mozilla.firefox.desktop"),
            entry("chromium-browser.desktop"),
        ];
        let windows = [
            win(1, "spotify", false),
            win(2, "firefox-wayland", true),
            win(3, "chromium", false),
            win(4, "gimp", false),
            win(5, "io.github.OlehDzhumyk.DeskShell", false),
        ];
        let g = group_entries(&pins, &windows, "io.github.OlehDzhumyk.DeskShell");
        assert_eq!(g.pinned[0], [windows[0].clone()]);
        assert_eq!(g.pinned[1], [windows[1].clone()]);
        assert_eq!(g.pinned[2], [windows[2].clone()]);
        assert_eq!(g.unpinned, vec![("gimp".into(), vec![windows[3].clone()])]);
    }
}
