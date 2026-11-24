// SPDX-License-Identifier: GPL-3.0-or-later
//! Screen pixels on wlroots compositors for the one-shot commands: `slurp` selects a region (or
//! a point), `grim` captures it to its standard output (PNG, or a binary PPM for one pixel), and
//! `wl-copy` puts a result on the clipboard. The parsers ([`ppm_pixel`], [`png_size`]) and the
//! file names of the fallback save are pure and tested; the rest runs the tools.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The most bytes read from `grim` (a 100-megapixel PNG fits; deskd refuses larger images).
pub const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;

/// Why a tool gave no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    /// The program is not installed (not in `PATH`).
    Missing(&'static str),
    /// It ran and failed, or its output was unusable.
    Failed(&'static str),
    /// The capture is larger than [`MAX_CAPTURE_BYTES`].
    TooLarge,
}

fn start_error(tool: &'static str, e: &std::io::Error) -> ToolError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ToolError::Missing(tool)
    } else {
        eprintln!("desk-shell: {tool}: {e}");
        ToolError::Failed(tool)
    }
}

/// Let the user select a region (`point`: one pixel) with `slurp`; `Ok(None)` when the
/// selection was cancelled (`Esc`).
pub fn select(point: bool) -> Result<Option<String>, ToolError> {
    let mut cmd = Command::new("slurp");
    if point {
        cmd.arg("-p");
    }
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| start_error("slurp", &e))?;
    if !out.status.success() {
        return Ok(None);
    }
    let region = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Ok(Some(region).filter(|r| !r.is_empty()))
}

/// Capture `region` with `grim` (`ppm`: a binary PPM, else PNG), reading at most
/// [`MAX_CAPTURE_BYTES`].
pub fn capture(region: &str, ppm: bool) -> Result<Vec<u8>, ToolError> {
    let mut cmd = Command::new("grim");
    cmd.args(["-g", region]);
    if ppm {
        cmd.args(["-t", "ppm"]);
    }
    let mut child = cmd
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| start_error("grim", &e))?;
    let mut data = Vec::new();
    let read = child
        .stdout
        .take()
        .map(|out| out.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut data));
    let too_large = data.len() as u64 > MAX_CAPTURE_BYTES;
    if too_large {
        // the child this process spawned (never one found by name)
        let _ = child.kill();
    }
    let status = child.wait();
    if too_large {
        return Err(ToolError::TooLarge);
    }
    match (read, status) {
        (Some(Ok(_)), Ok(s)) if s.success() && !data.is_empty() => Ok(data),
        _ => Err(ToolError::Failed("grim")),
    }
}

/// Put `data` on the clipboard with `wl-copy` (`mime`: its type, else text).
pub fn copy(data: &[u8], mime: Option<&str>) -> Result<(), ToolError> {
    let mut cmd = Command::new("wl-copy");
    if let Some(m) = mime {
        cmd.args(["--type", m]);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| start_error("wl-copy", &e))?;
    let written = child.stdin.take().map(|mut i| i.write_all(data));
    // wl-copy forks a server into the background; this waits for the first process only
    match (written, child.wait()) {
        (Some(Ok(())), Ok(s)) if s.success() => Ok(()),
        _ => Err(ToolError::Failed("wl-copy")),
    }
}

/// Why a PPM cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PpmError {
    /// Not a binary PPM (`P6`).
    NotP6,
    /// A header field is missing, not a number, or out of range.
    BadHeader,
    /// The first pixel is not all there.
    Truncated,
}

/// The next header token of a PPM from `pos` (whitespace and `#` comments skipped).
fn ppm_token<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    loop {
        match data.get(*pos)? {
            b if b.is_ascii_whitespace() => *pos += 1,
            b'#' => {
                while data.get(*pos).is_some_and(|b| *b != b'\n') {
                    *pos += 1;
                }
            }
            _ => break,
        }
    }
    let start = *pos;
    while data.get(*pos).is_some_and(|b| !b.is_ascii_whitespace()) {
        *pos += 1;
    }
    Some(&data[start..*pos])
}

fn ppm_number(data: &[u8], pos: &mut usize) -> Result<u32, PpmError> {
    let token = ppm_token(data, pos).ok_or(PpmError::BadHeader)?;
    std::str::from_utf8(token)
        .ok()
        .and_then(|t| t.parse().ok())
        .ok_or(PpmError::BadHeader)
}

/// The first pixel of a binary PPM (`P6`) as 8-bit sRGB: `grim -t ppm` of `slurp -p` gives one
/// pixel (more on a scaled output; the first one counts). Samples of a `maxval` other than 255
/// are scaled (two bytes, big-endian, above 255).
pub fn ppm_pixel(data: &[u8]) -> Result<(u8, u8, u8), PpmError> {
    if !data.starts_with(b"P6") {
        return Err(PpmError::NotP6);
    }
    let mut pos = 2;
    if !data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        return Err(PpmError::NotP6);
    }
    let width = ppm_number(data, &mut pos)?;
    let height = ppm_number(data, &mut pos)?;
    let maxval = ppm_number(data, &mut pos)?;
    if width == 0 || height == 0 || !(1..=65535).contains(&maxval) {
        return Err(PpmError::BadHeader);
    }
    // exactly one whitespace byte ends the header
    if !data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        return Err(PpmError::Truncated);
    }
    pos += 1;
    let size = if maxval > 255 { 2 } else { 1 };
    let raw = data.get(pos..pos + 3 * size).ok_or(PpmError::Truncated)?;
    let sample = |i: usize| -> u8 {
        let v = if size == 2 {
            u32::from(raw[2 * i]) << 8 | u32::from(raw[2 * i + 1])
        } else {
            u32::from(raw[i])
        };
        let scaled = (v.min(maxval) * 255 + maxval / 2) / maxval;
        u8::try_from(scaled).unwrap_or(u8::MAX)
    };
    Ok((sample(0), sample(1), sample(2)))
}

/// `#RRGGBB` of a colour.
pub fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Width and height of a PNG (its `IHDR` chunk); `None` when the bytes are not a PNG.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !png.starts_with(SIGNATURE) || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(png.get(at..at + 4)?.try_into().ok()?))
    };
    let (w, h) = (be(16)?, be(20)?);
    (w > 0 && h > 0).then_some((w, h))
}

/// The folder of the fallback save: `XDG_PICTURES_DIR` (absolute), else the pictures folder of
/// `user-dirs.dirs`, else `~/Pictures`; with `Screenshots` in it.
pub fn screenshot_dir(env: Option<&str>, special: Option<&Path>, home: &Path) -> PathBuf {
    env.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| special.map(Path::to_path_buf))
        .unwrap_or_else(|| home.join("Pictures"))
        .join("Screenshots")
}

/// The file name of the `n`-th capture (1, 2, ...) in the second `stamp`
/// (`YYYY-MM-DD HH-MM-SS`), as deskd names them.
pub fn screenshot_name(stamp: &str, n: u32) -> String {
    if n <= 1 {
        format!("Screenshot {stamp}.png")
    } else {
        format!("Screenshot {stamp}-{n}.png")
    }
}

/// Write `png` as a new file in `dir` (never replacing one; mode 0600 as deskd's files);
/// `stamp` names it.
pub fn save_new(dir: &Path, stamp: &str, png: &[u8]) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::create_dir_all(dir)?;
    for n in 1..=1000 {
        let path = dir.join(screenshot_name(stamp, n));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(png)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pixel_ppms_are_read() {
        assert_eq!(
            ppm_pixel(b"P6\n1 1\n255\n\x12\xab\xff"),
            Ok((0x12, 0xab, 0xff))
        );
        // grim writes "P6\n<w> <h>\n255\n"; any whitespace and comments are allowed
        assert_eq!(
            ppm_pixel(b"P6 # grim\n 1\t1 # size\n255 \x00\x80\x01"),
            Ok((0, 0x80, 1))
        );
        // a scaled output gives more pixels: the first one counts
        assert_eq!(
            ppm_pixel(b"P6\n2 2\n255\n\x01\x02\x03\x04\x05\x06"),
            Ok((1, 2, 3))
        );
        // a header byte that looks like whitespace in the samples is still a sample
        assert_eq!(ppm_pixel(b"P6\n1 1\n255\n\n\n\n"), Ok((10, 10, 10)));
    }

    #[test]
    fn other_maxvals_are_scaled_to_8_bit() {
        assert_eq!(ppm_pixel(b"P6\n1 1\n15\n\x0f\x00\x08"), Ok((255, 0, 136)));
        assert_eq!(ppm_pixel(b"P6\n1 1\n1\n\x01\x00\x01"), Ok((255, 0, 255)));
        // 16-bit samples, big-endian
        assert_eq!(
            ppm_pixel(b"P6\n1 1\n65535\n\xff\xff\x00\x00\x80\x00"),
            Ok((255, 0, 128))
        );
        // a sample above maxval is clamped
        assert_eq!(ppm_pixel(b"P6\n1 1\n100\n\xff\x32\x00"), Ok((255, 128, 0)));
    }

    #[test]
    fn broken_ppms_are_refused() {
        assert_eq!(ppm_pixel(b""), Err(PpmError::NotP6));
        assert_eq!(ppm_pixel(b"P3\n1 1\n255\n1 2 3"), Err(PpmError::NotP6));
        assert_eq!(ppm_pixel(b"P61 1 255 abc"), Err(PpmError::NotP6));
        assert_eq!(ppm_pixel(b"\x89PNG\r\n"), Err(PpmError::NotP6));
        assert_eq!(ppm_pixel(b"P6\n1\n"), Err(PpmError::BadHeader));
        assert_eq!(ppm_pixel(b"P6\nx 1\n255\n123"), Err(PpmError::BadHeader));
        assert_eq!(ppm_pixel(b"P6\n0 1\n255\n123"), Err(PpmError::BadHeader));
        assert_eq!(ppm_pixel(b"P6\n1 1\n0\n123"), Err(PpmError::BadHeader));
        assert_eq!(
            ppm_pixel(b"P6\n1 1\n70000\n123456"),
            Err(PpmError::BadHeader)
        );
        assert_eq!(ppm_pixel(b"P6\n1 1\n-1\n123"), Err(PpmError::BadHeader));
        assert_eq!(ppm_pixel(b"P6\n1 1\n255"), Err(PpmError::Truncated));
        assert_eq!(
            ppm_pixel(b"P6\n1 1\n255\n\x01\x02"),
            Err(PpmError::Truncated)
        );
        assert_eq!(
            ppm_pixel(b"P6\n1 1\n65535\n\x01\x02\x03\x04\x05"),
            Err(PpmError::Truncated)
        );
    }

    #[test]
    fn colours_print_as_hex() {
        assert_eq!(hex(255, 0, 128), "#FF0080");
        assert_eq!(hex(0, 0, 0), "#000000");
    }

    #[test]
    fn png_sizes_come_from_the_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&640_u32.to_be_bytes());
        png.extend_from_slice(&480_u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(&png[..22]), None);
        assert_eq!(png_size(b"P6\n1 1\n255\n123"), None);
        let mut zero = png.clone();
        zero[16..20].copy_from_slice(&0_u32.to_be_bytes());
        assert_eq!(png_size(&zero), None);
        let mut other = png;
        other[12..16].copy_from_slice(b"IDAT");
        assert_eq!(png_size(&other), None);
    }

    #[test]
    fn fallback_saves_go_to_the_pictures_folder() {
        let home = Path::new("/home/user");
        assert_eq!(
            screenshot_dir(
                Some("/data/Pics"),
                Some(Path::new("/home/user/Bilder")),
                home
            ),
            PathBuf::from("/data/Pics/Screenshots")
        );
        // a relative XDG_PICTURES_DIR is ignored (as by the XDG rules)
        assert_eq!(
            screenshot_dir(Some("Pics"), Some(Path::new("/home/user/Bilder")), home),
            PathBuf::from("/home/user/Bilder/Screenshots")
        );
        assert_eq!(
            screenshot_dir(None, None, home),
            PathBuf::from("/home/user/Pictures/Screenshots")
        );
        assert_eq!(
            screenshot_name("2026-11-24 10-00-00", 1),
            "Screenshot 2026-11-24 10-00-00.png"
        );
        assert_eq!(
            screenshot_name("2026-11-24 10-00-00", 3),
            "Screenshot 2026-11-24 10-00-00-3.png"
        );
    }

    #[test]
    fn fallback_saves_never_replace_a_file() {
        let dir = std::env::temp_dir().join(format!("desk-shell-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = save_new(&dir, "2026-11-24 10-00-00", b"one").unwrap();
        let b = save_new(&dir, "2026-11-24 10-00-00", b"two").unwrap();
        assert_ne!(a, b);
        assert!(b.ends_with("Screenshot 2026-11-24 10-00-00-2.png"));
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&a).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
