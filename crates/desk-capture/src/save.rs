// SPDX-License-Identifier: GPL-3.0-or-later
//! Where and how a capture is written: the save folder, the file name, and atomic writes.
//!
//! A new file is written to a temporary file in the target folder, flushed, and then linked to
//! its final name with `link(2)`, which fails instead of replacing an existing file. Two captures
//! in the same second therefore get `-2`, `-3`, ... and never overwrite each other, and a reader
//! never sees a half-written PNG.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::Result;

/// The most files with the same timestamp (`-2` .. `-999`) before saving gives up.
const MAX_SUFFIX: u32 = 999;

/// A local wall-clock time for the file name. The caller supplies it (the crate reads no clock
/// and no time zone database).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Stamp {
    /// The UTC time of a Unix timestamp (a fallback when no local time is known).
    pub fn from_unix_utc(secs: i64) -> Stamp {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // days to civil date (H. Hinnant, "chrono-Compatible Low-Level Date Algorithms")
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        let small = |v: i64| u8::try_from(v).unwrap_or(0);
        Stamp {
            year: i32::try_from(year).unwrap_or(i32::MAX),
            month: small(month),
            day: small(day),
            hour: small(rem / 3600),
            minute: small(rem % 3600 / 60),
            second: small(rem % 60),
        }
    }
}

/// `Screenshot YYYY-MM-DD HH-MM-SS.png`, or with `-n` before `.png` for `n >= 2`.
pub fn file_name(s: &Stamp, n: u32) -> String {
    let base = format!(
        "Screenshot {:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        s.year, s.month, s.day, s.hour, s.minute, s.second
    );
    if n <= 1 {
        format!("{base}.png")
    } else {
        format!("{base}-{n}.png")
    }
}

/// The save folder of the `save-folder` setting.
///
/// Empty: `Screenshots` in the pictures folder (`pictures`, the XDG one when known), else
/// `~/Pictures/Screenshots`. `~` and `~/...` are expanded; any other relative path is taken
/// from the home folder.
pub fn resolve_folder(setting: &str, home: &Path, pictures: Option<&Path>) -> PathBuf {
    let s = setting.trim();
    if s.is_empty() {
        return pictures
            .map_or_else(|| home.join("Pictures"), Path::to_path_buf)
            .join("Screenshots");
    }
    if s == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = Path::new(s);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        home.join(p)
    }
}

/// A new temporary file in `dir` (`.desk-capture-<pid>-<n>.tmp`, created exclusively).
fn temp_file(dir: &Path) -> std::io::Result<(PathBuf, File)> {
    static N: AtomicU32 = AtomicU32::new(0);
    loop {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = dir.join(format!(".desk-capture-{}-{n}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Write `bytes` to a flushed temporary file in `dir`.
fn write_temp(dir: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let (tmp, mut f) = temp_file(dir)?;
    let res = f.write_all(bytes).and_then(|()| f.sync_all());
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Give `tmp` the name `dst` if no file has it. `Ok(false)`: the name is taken.
fn claim(tmp: &Path, dst: &Path) -> std::io::Result<bool> {
    match fs::hard_link(tmp, dst) {
        Ok(()) => {
            fs::remove_file(tmp)?;
            Ok(true)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        // file systems without hard links (FAT, some network mounts): check, then rename
        Err(_) => match fs::symlink_metadata(dst) {
            Err(e) if e.kind() == ErrorKind::NotFound => fs::rename(tmp, dst).map(|()| true),
            Err(e) => Err(e),
            Ok(_) => Ok(false),
        },
    }
}

fn sync_dir(dir: &Path) {
    // best effort: the rename is durable after the directory is flushed
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Save `bytes` in `folder` (created if needed) under the first free name of [`file_name`].
pub fn save_new(folder: &Path, stamp: &Stamp, bytes: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let tmp = write_temp(folder, bytes)?;
    for n in 1..=MAX_SUFFIX {
        let dst = folder.join(file_name(stamp, n));
        match claim(&tmp, &dst) {
            Ok(true) => {
                sync_dir(folder);
                return Ok(dst);
            }
            Ok(false) => {}
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e.into());
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "too many screenshots in the same second",
    )
    .into())
}

/// Replace the file at `path` with `bytes` atomically, keeping its permissions.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let perms = fs::metadata(path)?.permissions();
    let tmp = write_temp(dir, bytes)?;
    let res = fs::set_permissions(&tmp, perms).and_then(|()| fs::rename(&tmp, path));
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    sync_dir(dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    const STAMP: Stamp = Stamp {
        year: 2026,
        month: 1,
        day: 2,
        hour: 3,
        minute: 4,
        second: 5,
    };

    #[test]
    fn names_and_utc_stamps() {
        assert_eq!(file_name(&STAMP, 1), "Screenshot 2026-01-02 03-04-05.png");
        assert_eq!(file_name(&STAMP, 3), "Screenshot 2026-01-02 03-04-05-3.png");
        assert_eq!(
            Stamp::from_unix_utc(0),
            Stamp {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        // 2024-02-29 23:59:59 UTC (a leap day)
        assert_eq!(
            Stamp::from_unix_utc(1_709_251_199),
            Stamp {
                year: 2024,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 59
            }
        );
    }

    #[test]
    fn folder_setting_is_expanded() {
        let home = Path::new("/home/user");
        assert_eq!(
            resolve_folder("", home, None),
            Path::new("/home/user/Pictures/Screenshots")
        );
        assert_eq!(
            resolve_folder("  ", home, Some(Path::new("/home/user/Bilder"))),
            Path::new("/home/user/Bilder/Screenshots")
        );
        assert_eq!(resolve_folder("~", home, None), home);
        assert_eq!(
            resolve_folder("~/Shots", home, None),
            Path::new("/home/user/Shots")
        );
        assert_eq!(resolve_folder("/mnt/x", home, None), Path::new("/mnt/x"));
        assert_eq!(
            resolve_folder("Shots", home, None),
            Path::new("/home/user/Shots")
        );
    }

    #[test]
    fn same_second_collisions_get_suffixes() {
        let dir = TempDir::new("collide");
        let folder = dir.0.join("new/Screenshots");
        let names: Vec<String> = (0..3)
            .map(|i| {
                let p = save_new(&folder, &STAMP, &[i]).unwrap();
                p.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "Screenshot 2026-01-02 03-04-05.png",
                "Screenshot 2026-01-02 03-04-05-2.png",
                "Screenshot 2026-01-02 03-04-05-3.png"
            ]
        );
        // each file keeps its own content: nothing was overwritten
        for (i, n) in names.iter().enumerate() {
            assert_eq!(
                fs::read(folder.join(n)).unwrap(),
                [u8::try_from(i).unwrap()]
            );
        }
        // no temporary file is left
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 3);
    }

    #[test]
    fn replace_keeps_the_name_and_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("replace");
        let p = dir.0.join("a.png");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        replace(&p, b"new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        assert!(replace(&dir.0.join("missing.png"), b"x").is_err());
    }
}
