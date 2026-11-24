// SPDX-License-Identifier: GPL-3.0-or-later
//! Screenshot engine of deskd (`docs/spec/features/screenshots.md`): the settings that matter here
//! (`enabled`, `save-folder`) and the checks around the pure-Rust core `desk-capture`.
//!
//! deskd holds no image between calls: a capture is checked, scrubbed and written in one call, and
//! an annotated file is decoded, drawn on and written back in one call.

pub mod service;

use std::fmt;
use std::path::{Path, PathBuf};

use desk_capture::Stamp;

/// The largest PNG file `ApplyAnnotations` reads. 100 megapixels of noise compress to about
/// 400 MB; anything bigger cannot be a capture under the size cap.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// What the `enabled` and `save-folder` settings give the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub save_folder: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            enabled: true,
            save_folder: String::new(),
        }
    }
}

/// The folders of the user that the save folder setting refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    pub home: PathBuf,
    /// The XDG pictures folder, when it is configured.
    pub pictures: Option<PathBuf>,
}

impl Dirs {
    pub fn of_user() -> Dirs {
        Dirs {
            home: gio::glib::home_dir(),
            pictures: gio::glib::user_special_dir(gio::glib::UserDirectory::Pictures),
        }
    }
}

/// Why a capture call failed.
#[derive(Debug)]
pub enum CaptureError {
    /// The `enabled` setting is off.
    Disabled,
    /// The path given to `ApplyAnnotations` is not an absolute path of a regular `.png` file.
    BadPath(String),
    /// The core refused or failed (bad PNG, too large, bad annotations, I/O).
    Core(desk_capture::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Disabled => f.write_str("screenshots are turned off"),
            CaptureError::BadPath(why) => write!(f, "bad path: {why}"),
            CaptureError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl From<desk_capture::Error> for CaptureError {
    fn from(e: desk_capture::Error) -> Self {
        CaptureError::Core(e)
    }
}

impl From<std::io::Error> for CaptureError {
    fn from(e: std::io::Error) -> Self {
        CaptureError::Core(desk_capture::Error::Io(e))
    }
}

/// The local time for the file name; UTC when GLib knows no local time.
pub fn local_stamp() -> Stamp {
    match gio::glib::DateTime::now_local() {
        Ok(t) => Stamp {
            year: t.year(),
            month: u8::try_from(t.month()).unwrap_or(1),
            day: u8::try_from(t.day_of_month()).unwrap_or(1),
            hour: u8::try_from(t.hour()).unwrap_or(0),
            minute: u8::try_from(t.minute()).unwrap_or(0),
            second: u8::try_from(t.second()).unwrap_or(0),
        },
        Err(_) => Stamp::from_unix_utc(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0)),
        ),
    }
}

pub struct Capture {
    cfg: Config,
    dirs: Dirs,
}

impl Capture {
    pub fn new(cfg: Config, dirs: Dirs) -> Capture {
        Capture { cfg, dirs }
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    /// The folder new captures go to.
    pub fn folder(&self) -> PathBuf {
        desk_capture::save::resolve_folder(
            &self.cfg.save_folder,
            &self.dirs.home,
            self.dirs.pictures.as_deref(),
        )
    }

    fn check_enabled(&self) -> Result<(), CaptureError> {
        if self.cfg.enabled {
            Ok(())
        } else {
            Err(CaptureError::Disabled)
        }
    }

    /// Check, scrub and save a PNG capture; returns the new file.
    pub fn save(&self, png: &[u8], stamp: &Stamp) -> Result<PathBuf, CaptureError> {
        self.check_enabled()?;
        Ok(desk_capture::save_screenshot(&self.folder(), png, stamp)?)
    }

    /// Draw the annotations (JSON) into the PNG file at `path` and replace it.
    pub fn annotate(&self, path: &str, json: &str) -> Result<PathBuf, CaptureError> {
        self.check_enabled()?;
        let path = checked_png_path(path)?;
        let list = desk_capture::annotate::parse(json)?;
        desk_capture::annotate_file(&path, &list)?;
        Ok(path)
    }
}

/// An absolute path of an existing regular `.png` file (not a symbolic link) of sane size.
fn checked_png_path(path: &str) -> Result<PathBuf, CaptureError> {
    let p = Path::new(path);
    let bad = |why: &str| Err(CaptureError::BadPath(why.into()));
    if !p.is_absolute() {
        return bad("not absolute");
    }
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return bad("not a .png file");
    }
    let meta = std::fs::symlink_metadata(p)?;
    if !meta.file_type().is_file() {
        return bad("not a regular file");
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(CaptureError::Core(desk_capture::Error::Corrupt(
            "file too large for a capture".into(),
        )));
    }
    Ok(p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use desk_capture::{Image, codec};

    pub(super) fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deskd-capture-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub(super) const STAMP: Stamp = Stamp {
        year: 2026,
        month: 11,
        day: 23,
        hour: 12,
        minute: 0,
        second: 1,
    };

    fn engine(home: &Path, enabled: bool) -> Capture {
        Capture::new(
            Config {
                enabled,
                save_folder: "~/Shots".into(),
            },
            Dirs {
                home: home.to_path_buf(),
                pictures: None,
            },
        )
    }

    #[test]
    fn disabled_engine_refuses_and_writes_nothing() {
        let home = tmp("disabled");
        let png = codec::encode(&Image::filled(2, 2, [1, 2, 3, 255])).unwrap();
        let e = engine(&home, false);
        assert!(matches!(e.save(&png, &STAMP), Err(CaptureError::Disabled)));
        assert!(matches!(
            e.annotate("/x.png", "[]"),
            Err(CaptureError::Disabled)
        ));
        assert!(!home.join("Shots").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn folder_follows_the_setting() {
        let home = Path::new("/home/user");
        let mut e = engine(home, true);
        assert_eq!(e.folder(), Path::new("/home/user/Shots"));
        e.set_config(Config::default());
        assert_eq!(e.folder(), Path::new("/home/user/Pictures/Screenshots"));
    }

    #[test]
    fn annotate_checks_the_path() {
        let home = tmp("paths");
        let e = engine(&home, true);
        let png = codec::encode(&Image::filled(2, 2, [1, 2, 3, 255])).unwrap();
        let file = e.save(&png, &STAMP).unwrap();
        let link = home.join("link.png");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let txt = home.join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        for p in [
            "relative.png".to_string(),
            txt.display().to_string(),
            link.display().to_string(),
            home.display().to_string(),
        ] {
            assert!(
                matches!(e.annotate(&p, "[]"), Err(CaptureError::BadPath(_))),
                "{p}"
            );
        }
        assert!(matches!(
            e.annotate(&home.join("missing.png").display().to_string(), "[]"),
            Err(CaptureError::Core(desk_capture::Error::Io(_)))
        ));
        // the target of the refused link is unchanged
        assert_eq!(std::fs::read(&file).unwrap(), png);
        let _ = std::fs::remove_dir_all(&home);
    }
}
