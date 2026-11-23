// SPDX-License-Identifier: GPL-3.0-or-later
//! Colour picker engine of deskd (`docs/spec/features/color-picker.md`): formatting through
//! `theme_tokens::picker` and the history of picks.
//!
//! The history lives in `$XDG_DATA_HOME/deskd/colors.json` (0600 file in a 0700 directory), newest
//! first, one entry per `#RRGGBB`. With `history-size` 0 nothing is recorded, and the file is
//! deleted (the same privacy rule as the emoji `remember-recents=false`).

pub mod service;

use std::path::PathBuf;

use desk_proto::color_picker::ColorPick;
use serde::{Deserialize, Serialize};
use theme_tokens::color::Rgba;
use theme_tokens::picker::{self, Format};

use crate::clipboard::{ensure_private_dir, write_private};

/// The schema maximum of `history-size`: a hand-edited file never loads more.
pub const MAX_HISTORY: usize = 50;
const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    /// `#RRGGBB`, upper case.
    hex: String,
    picked_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    picks: Vec<Entry>,
}

/// What the `format` and `history-size` settings give the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub format: Format,
    pub history_size: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            format: Format::Hex,
            history_size: 20,
        }
    }
}

pub struct ColorPicker {
    path: PathBuf,
    cfg: Config,
    /// Newest first, at most `cfg.history_size`.
    picks: Vec<Entry>,
}

fn hex_of(c: Rgba) -> String {
    picker::format_color(c, Format::Hex)
}

/// `#rrggbb`, `RRGGBB` and the like → `#RRGGBB`; `None` for anything else.
fn normalize_hex(s: &str) -> Option<String> {
    let digits = s.trim().strip_prefix('#').unwrap_or(s.trim());
    (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", digits.to_ascii_uppercase()))
}

fn rgba_of(hex: &str) -> Rgba {
    // entries are normalised on the way in; a broken one shows as black instead of panicking
    Rgba::parse(hex).unwrap_or(Rgba::rgb(0, 0, 0))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The format `name` (`""` = `current`); `None` for an unknown name.
pub fn parse_format(name: &str, current: Format) -> Option<Format> {
    if name.trim().is_empty() {
        Some(current)
    } else {
        Format::parse(name)
    }
}

impl ColorPicker {
    /// Load the history from `path`. With `history_size` 0 the file is deleted instead.
    pub fn new(path: PathBuf, cfg: Config) -> ColorPicker {
        let mut cp = ColorPicker {
            path,
            cfg,
            picks: Vec::new(),
        };
        if cp.remembers() {
            cp.load();
        } else {
            cp.forget_file();
        }
        cp
    }

    /// Default path: `$XDG_DATA_HOME/deskd/colors.json` (fallback `~/.local/share/deskd/colors.json`).
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(|| "/tmp".into(), PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        data.join("deskd/colors.json")
    }

    fn remembers(&self) -> bool {
        self.cfg.history_size > 0
    }

    fn limit(&self) -> usize {
        self.cfg.history_size.min(MAX_HISTORY)
    }

    fn load(&mut self) {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        let Ok(store) = serde_json::from_slice::<Store>(&bytes) else {
            eprintln!("deskd: {} is damaged, ignoring it", self.path.display());
            return;
        };
        self.picks.clear();
        for e in store.picks {
            let Some(hex) = normalize_hex(&e.hex) else {
                continue;
            };
            if !self.picks.iter().any(|p| p.hex == hex) {
                self.picks.push(Entry {
                    hex,
                    picked_at: e.picked_at,
                });
            }
        }
        self.picks.truncate(self.limit());
    }

    fn save(&self) {
        let r = (|| {
            if let Some(parent) = self.path.parent() {
                ensure_private_dir(parent)?;
            }
            let store = Store {
                version: STORE_VERSION,
                picks: self.picks.clone(),
            };
            let json = serde_json::to_vec(&store).map_err(std::io::Error::other)?;
            write_private(&self.path, &json)
        })();
        if let Err(e) = r {
            eprintln!("deskd: cannot save the colour history: {e}");
        }
    }

    fn forget_file(&self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("deskd: cannot delete {}: {e}", self.path.display()),
        }
    }

    pub fn config(&self) -> Config {
        self.cfg
    }

    /// New settings. History size 0 forgets everything and deletes the file; a smaller size drops
    /// the oldest picks. True when the history or its text changed (send `Changed`).
    pub fn set_config(&mut self, cfg: Config) -> bool {
        if cfg == self.cfg {
            return false;
        }
        self.cfg = cfg;
        if !self.remembers() {
            self.picks.clear();
            self.forget_file();
        } else if self.picks.len() > self.limit() {
            self.picks.truncate(self.limit());
            self.save();
        }
        true
    }

    /// Format `c` in the format in force and put it first in the history (when it keeps one).
    /// Returns `(formatted, hex, nearest_name)`.
    pub fn record(&mut self, c: Rgba) -> (String, String, String) {
        let hex = hex_of(c);
        let out = (
            picker::format_color(c, self.cfg.format),
            hex.clone(),
            picker::nearest_css_name(c).to_string(),
        );
        if self.remembers() {
            self.picks.retain(|p| p.hex != hex);
            self.picks.insert(
                0,
                Entry {
                    hex,
                    picked_at: now(),
                },
            );
            self.picks.truncate(self.limit());
            self.save();
        }
        out
    }

    /// The history, newest first, in the format in force.
    pub fn list(&self) -> Vec<ColorPick> {
        self.picks
            .iter()
            .map(|p| {
                let c = rgba_of(&p.hex);
                ColorPick {
                    hex: p.hex.clone(),
                    formatted: picker::format_color(c, self.cfg.format),
                    name: picker::nearest_css_name(c).to_string(),
                    picked_at: p.picked_at,
                }
            })
            .collect()
    }

    /// Forget one pick by its `#RRGGBB` (any case, `#` optional). False when it is not there.
    pub fn remove(&mut self, hex: &str) -> bool {
        let Some(hex) = normalize_hex(hex) else {
            return false;
        };
        let before = self.picks.len();
        self.picks.retain(|p| p.hex != hex);
        if self.picks.len() == before {
            return false;
        }
        if self.picks.is_empty() {
            self.forget_file();
        } else {
            self.save();
        }
        true
    }

    /// Forget all picks and delete the file. True when there was something to forget.
    pub fn clear(&mut self) -> bool {
        let had = !self.picks.is_empty();
        self.picks.clear();
        self.forget_file();
        had
    }

    /// WCAG contrast ratio between the two newest picks; 0 with fewer than two.
    pub fn contrast(&self) -> f64 {
        match self.picks.as_slice() {
            [a, b, ..] => picker::contrast_ratio(rgba_of(&a.hex), rgba_of(&b.hex)),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deskd-colors-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("deskd/colors.json")
    }

    fn cleanup(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    fn cfg(format: Format, history_size: usize) -> Config {
        Config {
            format,
            history_size,
        }
    }

    fn hexes(cp: &ColorPicker) -> Vec<String> {
        cp.list().into_iter().map(|p| p.hex).collect()
    }

    #[test]
    fn record_formats_in_the_format_in_force() {
        let p = temp_path("format");
        // AC-COLOR-01
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        let (text, hex, name) = cp.record(Rgba::rgb(0x33, 0x66, 0x99));
        assert_eq!(text, "#336699");
        assert_eq!(hex, "#336699");
        assert!(!name.is_empty());
        // AC-COLOR-02
        assert!(cp.set_config(cfg(Format::Rgb, 20)));
        let (text, hex, _) = cp.record(Rgba::rgb(0x33, 0x66, 0x99));
        assert_eq!(
            (text.as_str(), hex.as_str()),
            ("rgb(51, 102, 153)", "#336699")
        );
        // the history follows the format too
        assert_eq!(cp.list()[0].formatted, "rgb(51, 102, 153)");
        assert!(
            !cp.set_config(cfg(Format::Rgb, 20)),
            "same config: no change"
        );
        cleanup(&p);
    }

    #[test]
    fn history_keeps_the_newest_and_moves_a_repeat_to_the_front() {
        let p = temp_path("trim");
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        // AC-COLOR-03: 25 picks keep the 20 newest
        for i in 0..25u8 {
            cp.record(Rgba::rgb(i, 0, 0));
        }
        let h = hexes(&cp);
        assert_eq!(h.len(), 20);
        assert_eq!(h[0], "#180000");
        assert_eq!(h[19], "#050000");
        // a repeat moves to the front without a duplicate
        cp.record(Rgba::rgb(10, 0, 0));
        let h = hexes(&cp);
        assert_eq!(
            (h.len(), h[0].as_str(), h[1].as_str()),
            (20, "#0A0000", "#180000")
        );
        assert_eq!(h.iter().filter(|x| *x == "#0A0000").count(), 1);

        // 0600 file in a 0700 directory, and it loads back in the same order
        let mode = |q: &std::path::Path| std::fs::metadata(q).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(p.parent().unwrap()), 0o700);
        let again = ColorPicker::new(p.clone(), Config::default());
        assert_eq!(hexes(&again), h);

        // a smaller size drops the oldest, also on disk
        cp.set_config(cfg(Format::Hex, 3));
        assert_eq!(hexes(&cp), h[..3]);
        assert_eq!(
            hexes(&ColorPicker::new(p.clone(), Config::default())),
            h[..3]
        );
        cleanup(&p);
    }

    #[test]
    fn history_size_zero_writes_nothing_and_deletes_the_file() {
        // AC-COLOR-05
        let p = temp_path("zero");
        let mut cp = ColorPicker::new(p.clone(), cfg(Format::Hex, 0));
        let (text, _, _) = cp.record(Rgba::rgb(1, 2, 3));
        assert_eq!(text, "#010203", "the pick is still formatted");
        assert!(cp.list().is_empty());
        assert!(!p.exists());

        // turning it to 0 later forgets the picks and deletes the file
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        cp.record(Rgba::rgb(1, 2, 3));
        assert!(p.exists());
        assert!(cp.set_config(cfg(Format::Hex, 0)));
        assert!(cp.list().is_empty());
        assert!(!p.exists());

        // a file left from before is deleted at start when the size is 0
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        cp.record(Rgba::rgb(1, 2, 3));
        drop(cp);
        let cp = ColorPicker::new(p.clone(), cfg(Format::Hex, 0));
        assert!(cp.list().is_empty());
        assert!(!p.exists());
        cleanup(&p);
    }

    #[test]
    fn remove_and_clear() {
        let p = temp_path("remove");
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        cp.record(Rgba::rgb(0xAA, 0xBB, 0xCC));
        cp.record(Rgba::rgb(0x11, 0x22, 0x33));
        assert!(cp.remove("aabbcc"), "any case, '#' optional");
        assert!(!cp.remove("#AABBCC"), "already gone");
        assert!(!cp.remove("not a colour"));
        assert_eq!(hexes(&cp), ["#112233"]);
        assert_eq!(
            hexes(&ColorPicker::new(p.clone(), Config::default())),
            ["#112233"]
        );
        // the last one removed: no file left
        assert!(cp.remove("#112233"));
        assert!(!p.exists());

        cp.record(Rgba::rgb(1, 1, 1));
        assert!(cp.clear());
        assert!(cp.list().is_empty());
        assert!(!p.exists());
        assert!(!cp.clear(), "nothing left to clear");
        cleanup(&p);
    }

    #[test]
    fn contrast_of_the_two_newest_picks() {
        let p = temp_path("contrast");
        let mut cp = ColorPicker::new(p.clone(), Config::default());
        assert_eq!(cp.contrast(), 0.0);
        cp.record(Rgba::rgb(0, 0, 0));
        assert_eq!(cp.contrast(), 0.0);
        cp.record(Rgba::rgb(255, 255, 255));
        assert!((cp.contrast() - 21.0).abs() < 1e-9, "{}", cp.contrast());
        // only the two newest count
        cp.record(Rgba::rgb(255, 255, 255));
        cp.record(Rgba::rgb(0x77, 0x77, 0x77));
        let r = cp.contrast();
        assert!(r > 1.0 && r < 21.0, "{r}");
        cleanup(&p);
    }

    #[test]
    fn a_damaged_or_hand_edited_file_loads_what_is_valid() {
        let p = temp_path("damaged");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{not json").unwrap();
        assert!(
            ColorPicker::new(p.clone(), Config::default())
                .list()
                .is_empty()
        );

        std::fs::write(
            &p,
            r##"{"version":1,"picks":[{"hex":"#abcdef","picked_at":7},{"hex":"bad","picked_at":1},{"hex":"#ABCDEF","picked_at":3},{"hex":"#000001","picked_at":2}]}"##,
        )
        .unwrap();
        let cp = ColorPicker::new(p.clone(), cfg(Format::Decimal, 20));
        let list = cp.list();
        assert_eq!(list.len(), 2);
        assert_eq!(
            (
                list[0].hex.as_str(),
                list[0].formatted.as_str(),
                list[0].picked_at
            ),
            ("#ABCDEF", "171,205,239", 7)
        );
        cleanup(&p);
    }

    #[test]
    fn format_names() {
        assert_eq!(parse_format("", Format::Hsl), Some(Format::Hsl));
        assert_eq!(parse_format("GTK", Format::Hex), Some(Format::Gtk));
        assert_eq!(parse_format("cmyk", Format::Hex), None);
    }
}
