// SPDX-License-Identifier: GPL-3.0-or-later
//! desk-capture: the screenshot core of desktop-themes-rs (docs/spec/features/screenshots.md).
//!
//! Pure Rust and desktop-agnostic (ADR 0011): no GTK, GLib, D-Bus or compositor code, so the same
//! crate serves deskd on GNOME, a future KDE or wlroots adapter, and a plain CLI.
//!
//! * [`codec`]: check a PNG without decoding it, refuse captures larger than [`MAX_MEGAPIXELS`],
//!   remove the text and time chunks (no user name, host name or path in a saved file), and
//!   decode and encode RGBA.
//! * [`save`]: the file name `Screenshot YYYY-MM-DD HH-MM-SS.png` (with `-2`, `-3` in the same
//!   second), the save folder (`~` and the default `~/Pictures/Screenshots`), and atomic writes.
//! * [`annotate`]: the annotations of the editor as data (`serde`), rendered into the pixels.
//!   Redaction is destructive: the covered pixels are replaced, nothing of them is kept.

pub mod annotate;
pub mod codec;
pub mod save;

pub use annotate::{Annotation, RedactStyle, apply};
pub use codec::{Image, Info, MAX_MEGAPIXELS};
pub use save::Stamp;

/// What can go wrong with a capture.
#[derive(Debug)]
pub enum Error {
    /// The data is not a PNG (wrong signature, or no IHDR first).
    NotPng,
    /// The PNG is damaged (bad chunk, bad CRC, missing data).
    Corrupt(String),
    /// The image has more than [`MAX_MEGAPIXELS`] million pixels.
    TooLarge { width: u32, height: u32 },
    /// An annotation is invalid (bad JSON, zero size, too many points, ...).
    Annotation(String),
    /// A file could not be read or written.
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotPng => f.write_str("not a PNG image"),
            Error::Corrupt(why) => write!(f, "damaged PNG image: {why}"),
            Error::TooLarge { width, height } => write!(
                f,
                "image too large: {width}×{height} is more than {MAX_MEGAPIXELS} megapixels"
            ),
            Error::Annotation(why) => write!(f, "invalid annotation: {why}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Scrub a capture and save it in `folder` under a new name; returns the path.
///
/// The PNG is checked (size cap included) and its text and time chunks are removed first, so a
/// refused or damaged capture writes nothing.
pub fn save_screenshot(
    folder: &std::path::Path,
    png_bytes: &[u8],
    stamp: &Stamp,
) -> Result<std::path::PathBuf> {
    let clean = codec::scrub(png_bytes)?;
    save::save_new(folder, stamp, &clean)
}

/// Apply annotations to the PNG file at `path` and replace it atomically; returns the path.
///
/// The file is decoded, drawn on and encoded again: the result has one layer, no metadata, and no
/// trace of redacted pixels.
pub fn annotate_file(path: &std::path::Path, annotations: &[Annotation]) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let mut img = codec::decode(&bytes)?;
    drop(bytes);
    annotate::apply(&mut img, annotations)?;
    let out = codec::encode(&img)?;
    save::replace(path, &out)
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A new empty directory under the system temp directory; removed when dropped.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> TempDir {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p =
                std::env::temp_dir().join(format!("desk-capture-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_util::TempDir;

    fn stamp() -> Stamp {
        Stamp {
            year: 2026,
            month: 11,
            day: 23,
            hour: 9,
            minute: 5,
            second: 7,
        }
    }

    #[test]
    fn save_then_redact_round_trip() {
        let dir = TempDir::new("roundtrip");
        let img = Image::filled(8, 4, [200, 10, 10, 255]);
        let with_text =
            codec::tests::with_chunk(&codec::encode(&img).unwrap(), b"tEXt", b"Author\0me");
        let path = save_screenshot(&dir.0, &with_text, &stamp()).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "Screenshot 2026-11-23 09-05-07.png"
        );
        assert!(
            codec::tests::chunk_types(&std::fs::read(&path).unwrap())
                .iter()
                .all(|t| t != b"tEXt")
        );

        let redact = Annotation::Redact {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            style: RedactStyle::Solid([0, 0, 0, 255]),
        };
        annotate_file(&path, &[redact]).unwrap();
        let out = codec::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(out.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(out.pixel(7, 3), [200, 10, 10, 255]);
        // nothing else is left in the folder (no temporary file)
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn refused_capture_writes_nothing() {
        let dir = TempDir::new("refused");
        assert!(matches!(
            save_screenshot(&dir.0, b"GIF89a", &stamp()),
            Err(Error::NotPng)
        ));
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
    }
}
