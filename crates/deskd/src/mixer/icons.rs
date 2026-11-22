// SPDX-License-Identifier: GPL-3.0-or-later
//! Icon of an application that plays sound but does not name one (`application.icon-name`): the
//! `Icon=` of its desktop entry, found by app id or program name in the XDG data directories
//! (Flatpak exports included when the session lists them, as it does by default).

use std::path::{Path, PathBuf};

/// The data directories, most important first: `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`.
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out: Vec<PathBuf> = Vec::new();
    match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => out.push(p),
        _ => out.extend(home.as_ref().map(|h| h.join(".local/share"))),
    }
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    out.extend(
        dirs.split(':')
            .map(PathBuf::from)
            .filter(|p| p.is_absolute()),
    );
    out
}

/// `Icon=` of the `[Desktop Entry]` group.
fn icon_of(entry: &str) -> Option<String> {
    let mut in_main = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
        } else if in_main && let Some(v) = line.strip_prefix("Icon=") {
            let v = v.trim();
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

/// The icon of the first candidate (`org.mozilla.firefox`, `firefox`, ...) that has a desktop entry.
pub fn resolve(dirs: &[PathBuf], candidates: &[String]) -> Option<String> {
    candidates
        .iter()
        .filter(|c| !c.is_empty() && !c.contains('/'))
        .find_map(|c| {
            dirs.iter().find_map(|d| {
                let p: PathBuf = Path::new(d).join("applications").join(format!("{c}.desktop"));
                std::fs::read_to_string(p).ok().and_then(|e| icon_of(&e))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_icon_of_the_first_known_candidate() {
        let dir = std::env::temp_dir().join(format!("deskd-icons-{}", std::process::id()));
        let apps = dir.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("org.example.Player.desktop"),
            "[Desktop Entry]\nName=Player\nIcon=org.example.Player\n\n[Desktop Action new]\nIcon=other\n",
        )
        .unwrap();
        std::fs::write(apps.join("noicon.desktop"), "[Desktop Entry]\nName=x\n").unwrap();
        let dirs = vec![PathBuf::from("/nonexistent"), dir.clone()];
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            resolve(&dirs, &c(&["noicon", "org.example.Player"])),
            Some("org.example.Player".into())
        );
        assert_eq!(resolve(&dirs, &c(&["../applications/noicon", "mpv"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
