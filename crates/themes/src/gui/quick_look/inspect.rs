// SPDX-License-Identifier: GPL-3.0-or-later
//! What Quick Look shows for a path, without GTK (docs/spec/features/quick-look.md).
//!
//! The rules that keep the previewer safe live here, so that they are tested without a display:
//!
//! * a file is only **read**, never run: a shell script, an HTML page or an SVG is passive data
//!   (AC-QL-05); the classifier looks at the first bytes and at the name, nothing else;
//! * of a text file, at most `max_text_bytes` are read; the rest never enters memory (AC-QL-04);
//! * only regular files are opened: a FIFO or a device would block or never end;
//! * a missing, unreadable or broken path gives [`Content::Error`], never a panic (AC-QL-06);
//! * a symbolic link shows its target, under the name of the link.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How many bytes of a file the classifier looks at.
const SNIFF_BYTES: usize = 8 * 1024;

/// At most this many entries of a folder are listed.
pub const MAX_DIR_ENTRIES: usize = 5000;

/// Why a path cannot be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorReason {
    /// The path (or the target of a link) does not exist.
    Missing,
    /// The file or folder may not be read.
    PermissionDenied,
    /// Another error of the system, as its text.
    Other(String),
}

impl ErrorReason {
    fn of(e: &io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => ErrorReason::Missing,
            io::ErrorKind::PermissionDenied => ErrorReason::PermissionDenied,
            _ => ErrorReason::Other(e.to_string()),
        }
    }
}

/// One entry of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    /// Size in bytes of a file; `None` for a folder or when unknown.
    pub size: Option<u64>,
}

/// What the previewer shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// An image that the image loaders of the system decode (SVG as an image).
    Image,
    /// Text or code: the start of the file when `truncated`.
    Text {
        content: String,
        truncated: bool,
        total_bytes: u64,
    },
    /// A folder: folders first, then by name; `truncated` after [`MAX_DIR_ENTRIES`].
    Directory {
        entries: Vec<DirEntry>,
        truncated: bool,
    },
    /// No preview for this type: a card with the details.
    MetadataOnly {
        mime: String,
        size: u64,
        modified: Option<SystemTime>,
    },
    /// The path cannot be read.
    Error { reason: ErrorReason },
}

/// A path as the previewer sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspected {
    /// The name shown in the title: the name of the link for a link.
    pub display_name: String,
    /// The path that was given.
    pub path: PathBuf,
    /// The target when `path` is a symbolic link.
    pub link_target: Option<PathBuf>,
    /// Best guess of the type (`text/plain`, `image/png`, `inode/directory`, …).
    pub mime: String,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub content: Content,
}

/// The name shown for a path: its last component, or the whole path for `/` and `..`.
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Look at `path` and decide what to show. Never panics; reads at most `max_text_bytes` of a text
/// file and [`SNIFF_BYTES`] of any other file.
pub fn inspect(path: &Path, max_text_bytes: usize) -> Inspected {
    let mut out = Inspected {
        display_name: display_name(path),
        path: path.to_path_buf(),
        link_target: None,
        mime: String::new(),
        size: None,
        modified: None,
        content: Content::Error {
            reason: ErrorReason::Missing,
        },
    };
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => out.link_target = fs::read_link(path).ok(),
        Ok(_) => {}
        Err(e) => {
            out.content = Content::Error {
                reason: ErrorReason::of(&e),
            };
            return out;
        }
    }
    // follows the link: a broken link is a missing file
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            out.content = Content::Error {
                reason: ErrorReason::of(&e),
            };
            return out;
        }
    };
    out.modified = meta.modified().ok();
    if meta.is_dir() {
        out.mime = "inode/directory".into();
        out.content = match list_dir(path) {
            Ok((entries, truncated)) => Content::Directory { entries, truncated },
            Err(e) => Content::Error {
                reason: ErrorReason::of(&e),
            },
        };
        return out;
    }
    if !meta.is_file() {
        // FIFO, socket, device: opening may block or never end
        out.mime = special_mime(&meta).into();
        out.content = Content::MetadataOnly {
            mime: out.mime.clone(),
            size: 0,
            modified: out.modified,
        };
        return out;
    }
    let size = meta.len();
    out.size = Some(size);
    out.content = match classify_file(path, size, max_text_bytes) {
        Ok((mime, content)) => {
            out.mime = mime;
            content
        }
        Err(e) => Content::Error {
            reason: ErrorReason::of(&e),
        },
    };
    if let Content::MetadataOnly { modified, .. } = &mut out.content {
        *modified = out.modified;
    }
    out
}

#[cfg(unix)]
fn special_mime(meta: &fs::Metadata) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    let t = meta.file_type();
    if t.is_fifo() {
        "inode/fifo"
    } else if t.is_socket() {
        "inode/socket"
    } else if t.is_block_device() {
        "inode/blockdevice"
    } else if t.is_char_device() {
        "inode/chardevice"
    } else {
        "application/octet-stream"
    }
}

#[cfg(not(unix))]
fn special_mime(_meta: &fs::Metadata) -> &'static str {
    "application/octet-stream"
}

fn list_dir(path: &Path) -> io::Result<(Vec<DirEntry>, bool)> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for e in fs::read_dir(path)? {
        let Ok(e) = e else { continue };
        if entries.len() == MAX_DIR_ENTRIES {
            truncated = true;
            break;
        }
        // follows links; a broken link is listed as a file of unknown size
        let meta = fs::metadata(e.path()).ok();
        let is_dir = meta.as_ref().is_some_and(fs::Metadata::is_dir);
        entries.push(DirEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            is_dir,
            size: meta.filter(|m| m.is_file()).map(|m| m.len()),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok((entries, truncated))
}

fn classify_file(path: &Path, size: u64, max_text_bytes: usize) -> io::Result<(String, Content)> {
    let mut file = File::open(path)?;
    let mut head =
        Vec::with_capacity(SNIFF_BYTES.min(usize::try_from(size).unwrap_or(SNIFF_BYTES)));
    (&mut file)
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)?;
    let ext = path
        .extension()
        .map_or_else(String::new, |e| e.to_string_lossy().to_lowercase());
    if let Some(mime) = image_mime(&head, &ext) {
        return Ok((mime.into(), Content::Image));
    }
    if !looks_like_text(&head) {
        let mime = binary_mime(&head).unwrap_or("application/octet-stream");
        return Ok((
            mime.into(),
            Content::MetadataOnly {
                mime: mime.into(),
                size,
                modified: None,
            },
        ));
    }
    // the head is the start of the text; read on up to the cap, never further
    let cap = max_text_bytes as u64;
    let mut bytes = head;
    if (bytes.len() as u64) < cap {
        let rest = cap - bytes.len() as u64;
        file.take(rest).read_to_end(&mut bytes)?;
    }
    bytes.truncate(max_text_bytes);
    let truncated = size > bytes.len() as u64;
    let content = decode_text(&bytes);
    Ok((
        text_mime(&ext).into(),
        Content::Text {
            content,
            truncated,
            total_bytes: size,
        },
    ))
}

/// UTF-8 of `bytes`, without a character cut in two at the end; other bad bytes become U+FFFD.
fn decode_text(bytes: &[u8]) -> String {
    let end = match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        // an incomplete sequence at the very end: the cap cut a character
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => bytes.len(),
    };
    let s = String::from_utf8_lossy(&bytes[..end]);
    s.strip_prefix('\u{feff}').unwrap_or(&s).to_owned()
}

/// Text: no NUL byte and UTF-8 apart from a character cut at the end of the sample.
fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && head.len() - e.valid_up_to() < 4,
    }
}

fn image_mime(head: &[u8], ext: &str) -> Option<&'static str> {
    let magic: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"BM", "image/bmp"),
        (b"\x00\x00\x01\x00", "image/vnd.microsoft.icon"),
        (b"II*\x00", "image/tiff"),
        (b"MM\x00*", "image/tiff"),
    ];
    if let Some((_, m)) = magic.iter().find(|(sig, _)| head.starts_with(sig)) {
        return Some(m);
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" && matches!(&head[8..12], b"avif" | b"avis") {
        return Some("image/avif");
    }
    // SVG is decoded as an image (librsvg runs no scripts); svgz is gzip
    match ext {
        "svg" | "svgz" => Some("image/svg+xml"),
        _ => None,
    }
}

fn binary_mime(head: &[u8]) -> Option<&'static str> {
    let magic: &[(&[u8], &str)] = &[
        (b"%PDF-", "application/pdf"),
        (b"PK\x03\x04", "application/zip"),
        (b"\x1f\x8b", "application/gzip"),
        (b"\xfd7zXZ\x00", "application/x-xz"),
        (b"7z\xbc\xaf\x27\x1c", "application/x-7z-compressed"),
        (b"\x7fELF", "application/x-executable"),
        (b"OggS", "audio/ogg"),
        (b"fLaC", "audio/flac"),
        (b"ID3", "audio/mpeg"),
        (b"\x1aE\xdf\xa3", "video/webm"),
    ];
    magic
        .iter()
        .find(|(sig, _)| head.starts_with(sig))
        .map(|(_, m)| *m)
        .or_else(|| (head.len() >= 8 && &head[4..8] == b"ftyp").then_some("video/mp4"))
}

fn text_mime(ext: &str) -> &'static str {
    match ext {
        "sh" | "bash" | "zsh" => "application/x-shellscript",
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "xml" => "application/xml",
        "rs" => "text/rust",
        "py" => "text/x-python",
        "js" | "mjs" => "text/javascript",
        "ts" => "text/x-typescript",
        "css" => "text/css",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "c" | "h" => "text/x-c",
        _ => "text/plain",
    }
}

/// The files given to the previewer and the one shown: next and previous are O(1).
#[derive(Clone, Debug)]
pub struct Selection {
    paths: Vec<PathBuf>,
    index: usize,
}

impl Selection {
    /// `None` without paths.
    pub fn new(paths: Vec<PathBuf>) -> Option<Self> {
        (!paths.is_empty()).then_some(Self { paths, index: 0 })
    }

    pub fn current(&self) -> &Path {
        &self.paths[self.index]
    }

    /// The counter of the title: (position from 1, count).
    pub fn position(&self) -> (usize, usize) {
        (self.index + 1, self.paths.len())
    }

    /// Move to the next file; false at the last one.
    pub fn next(&mut self) -> bool {
        if self.index + 1 < self.paths.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    /// Move to the previous file; false at the first one.
    pub fn previous(&mut self) -> bool {
        if self.index > 0 {
            self.index -= 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: usize = 1024 * 1024;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ql-inspect-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn selection_counts_and_stops_at_the_ends() {
        // AC-QL-02: three files, Right twice -> the third, "3 of 3"
        let mut s = Selection::new(vec!["a".into(), "b".into(), "c".into()]).unwrap();
        assert_eq!(s.position(), (1, 3));
        assert!(!s.previous());
        assert!(s.next() && s.next());
        assert_eq!(s.position(), (3, 3));
        assert_eq!(s.current(), Path::new("c"));
        assert!(!s.next());
        assert_eq!(s.position(), (3, 3));
        assert!(Selection::new(vec![]).is_none());
    }

    #[test]
    fn large_text_is_truncated_at_the_cap() {
        // AC-QL-04: a 50 MB text file, only the first 1 MiB
        let d = tmp("large");
        let p = d.join("big.log");
        {
            use std::io::Write;
            let mut f = File::create(&p).unwrap();
            let line = [b'x'; 1023];
            for _ in 0..(50 * 1024) {
                f.write_all(&line).unwrap();
                f.write_all(b"\n").unwrap();
            }
        }
        let r = inspect(&p, CAP);
        match r.content {
            Content::Text {
                content,
                truncated,
                total_bytes,
            } => {
                assert!(truncated);
                assert_eq!(total_bytes, 50 * 1024 * 1024);
                assert_eq!(content.len(), CAP);
            }
            other => panic!("{other:?}"),
        }
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn small_text_is_whole_and_a_cut_character_is_dropped() {
        let d = tmp("small");
        let p = d.join("note.txt");
        fs::write(&p, "привіт").unwrap(); // 12 bytes
        let Content::Text {
            content, truncated, ..
        } = inspect(&p, CAP).content
        else {
            panic!()
        };
        assert_eq!((content.as_str(), truncated), ("привіт", false));
        // a cap of 5 bytes cuts the third letter: two letters, no U+FFFD
        let Content::Text {
            content, truncated, ..
        } = inspect(&p, 5).content
        else {
            panic!()
        };
        assert_eq!((content.as_str(), truncated), ("пр", true));
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_script_is_text_and_is_not_run() {
        // AC-QL-05: an executable shell script is shown as text; running it would create a file
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("script");
        let p = d.join("run.sh");
        let marker = d.join("ran");
        fs::write(&p, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let r = inspect(&p, CAP);
        assert_eq!(r.mime, "application/x-shellscript");
        assert!(
            matches!(&r.content, Content::Text { content, .. } if content.starts_with("#!/bin/sh"))
        );
        assert!(!marker.exists());
        // HTML is its source, never rendered
        let h = d.join("page.html");
        fs::write(&h, "<script>alert(1)</script>").unwrap();
        assert!(matches!(inspect(&h, CAP).content, Content::Text { .. }));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_path_is_an_error() {
        // AC-QL-06
        let d = tmp("missing");
        let r = inspect(&d.join("nope.png"), CAP);
        assert_eq!(
            r.content,
            Content::Error {
                reason: ErrorReason::Missing
            }
        );
        assert_eq!(r.display_name, "nope.png");
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_file_is_an_error() {
        // AC-QL-06; root reads everything, so the case only holds for other users
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("unreadable");
        let p = d.join("secret.txt");
        fs::write(&p, "x").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        let r = inspect(&p, CAP);
        if File::open(&p).is_err() {
            assert_eq!(
                r.content,
                Content::Error {
                    reason: ErrorReason::PermissionDenied
                }
            );
        }
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn images_binaries_and_svg_are_told_apart() {
        let d = tmp("types");
        let png = d.join("pic.png");
        fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(inspect(&png, CAP).content, Content::Image);
        // a PNG with a wrong name is still an image
        let renamed = d.join("pic.txt");
        fs::copy(&png, &renamed).unwrap();
        assert_eq!(inspect(&renamed, CAP).mime, "image/png");
        let svg = d.join("logo.svg");
        fs::write(
            &svg,
            "<svg xmlns='http://www.w3.org/2000/svg'><script/></svg>",
        )
        .unwrap();
        assert_eq!(inspect(&svg, CAP).content, Content::Image);
        let pdf = d.join("doc.pdf");
        fs::write(&pdf, b"%PDF-1.7\n\0\x01\x02").unwrap();
        let r = inspect(&pdf, CAP);
        assert!(
            matches!(&r.content, Content::MetadataOnly { mime, size: 12, modified: Some(_) } if mime == "application/pdf")
        );
        let bin = d.join("blob");
        fs::write(&bin, [0u8, 1, 2, 3]).unwrap();
        assert_eq!(inspect(&bin, CAP).mime, "application/octet-stream");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn directory_lists_folders_first_with_sizes() {
        let d = tmp("dir");
        fs::create_dir(d.join("zeta")).unwrap();
        fs::write(d.join("Beta.txt"), "12345").unwrap();
        fs::write(d.join("alpha.txt"), "1").unwrap();
        let r = inspect(&d, CAP);
        assert_eq!(r.mime, "inode/directory");
        let Content::Directory { entries, truncated } = r.content else {
            panic!()
        };
        assert!(!truncated);
        let got: Vec<_> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.is_dir, e.size))
            .collect();
        assert_eq!(
            got,
            [
                ("zeta", true, None),
                ("alpha.txt", false, Some(1)),
                ("Beta.txt", false, Some(5)),
            ]
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_shows_the_target_under_the_link_name() {
        let d = tmp("link");
        let target = d.join("real-name.txt");
        fs::write(&target, "hello").unwrap();
        let link = d.join("shortcut.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let r = inspect(&link, CAP);
        assert_eq!(r.display_name, "shortcut.txt");
        assert_eq!(r.link_target.as_deref(), Some(target.as_path()));
        assert!(matches!(&r.content, Content::Text { content, .. } if content == "hello"));
        // a broken link is a missing file, not a panic
        let broken = d.join("broken");
        std::os::unix::fs::symlink(d.join("gone"), &broken).unwrap();
        assert_eq!(
            inspect(&broken, CAP).content,
            Content::Error {
                reason: ErrorReason::Missing
            }
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_device_is_never_opened() {
        // /dev/zero never ends: reading it would hang or fill the memory
        let r = inspect(Path::new("/dev/zero"), CAP);
        assert_eq!(r.mime, "inode/chardevice");
        assert!(matches!(r.content, Content::MetadataOnly { .. }));
    }
}
