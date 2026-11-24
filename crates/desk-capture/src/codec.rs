// SPDX-License-Identifier: GPL-3.0-or-later
//! PNG checks, metadata scrubbing, and RGBA decode and encode.
//!
//! [`check`] and [`scrub`] work on the chunk structure only (no inflate), so a capture is checked
//! and cleaned without holding a decoded copy. [`decode`] checks the size cap before it allocates.

use std::io::Cursor;

use crate::{Error, Result};

/// Captures with more pixels than this many million are refused (spec: edge cases).
pub const MAX_MEGAPIXELS: u64 = 100;
const MAX_PIXELS: u64 = MAX_MEGAPIXELS * 1_000_000;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Ancillary chunks that can carry free text or a time: an author, a host name, a path, the
/// software, the capture time. They are never needed to show the image.
const STRIPPED: [&[u8; 4]; 5] = [b"tEXt", b"zTXt", b"iTXt", b"tIME", b"eXIf"];

/// What the IHDR chunk says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
}

/// An 8-bit RGBA image, rows top to bottom, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Image {
    /// An image of one colour. Panics above the size cap (test and tool helper).
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        check_size(width, height).expect("image size");
        let n = width as usize * height as usize;
        Image {
            width,
            height,
            data: rgba.repeat(n),
        }
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// The pixel at (`x`, `y`). Panics outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.offset(x, y);
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Set the pixel at (`x`, `y`). Panics outside the image.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let i = self.offset(x, y);
        self.data[i..i + 4].copy_from_slice(&rgba);
    }
}

fn check_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::Corrupt("zero width or height".into()));
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::TooLarge { width, height });
    }
    Ok(())
}

/// One chunk: its type and its bytes in the file (length, type, data and CRC).
struct Chunk<'a> {
    ty: [u8; 4],
    raw: &'a [u8],
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Walk the chunks up to IEND, checking lengths, types and CRCs. Bytes after IEND are ignored.
fn walk(bytes: &[u8]) -> Result<(Info, Vec<Chunk<'_>>)> {
    if bytes.len() < SIGNATURE.len() || bytes[..8] != SIGNATURE {
        return Err(Error::NotPng);
    }
    let mut at = SIGNATURE.len();
    let mut chunks = Vec::new();
    let mut info = None;
    let mut idat = false;
    loop {
        let head = bytes
            .get(at..at + 8)
            .ok_or_else(|| Error::Corrupt("no IEND chunk".into()))?;
        let len = be32(head) as usize;
        let ty: [u8; 4] = [head[4], head[5], head[6], head[7]];
        if len > 0x7fff_ffff || !ty.iter().all(u8::is_ascii_alphabetic) {
            return Err(Error::Corrupt("bad chunk header".into()));
        }
        let end = at
            .checked_add(12 + len)
            .filter(|&e| e <= bytes.len())
            .ok_or_else(|| Error::Corrupt("truncated chunk".into()))?;
        let raw = &bytes[at..end];
        if crc32fast::hash(&raw[4..8 + len]) != be32(&raw[8 + len..]) {
            return Err(Error::Corrupt(format!(
                "bad CRC in {}",
                String::from_utf8_lossy(&ty)
            )));
        }
        match (&ty, info.is_some()) {
            (b"IHDR", false) if len == 13 => {
                let d = &raw[8..];
                let i = Info {
                    width: be32(d),
                    height: be32(&d[4..]),
                    bit_depth: d[8],
                    color_type: d[9],
                };
                check_size(i.width, i.height)?;
                info = Some(i);
            }
            (_, false) => return Err(Error::NotPng),
            (b"IHDR", true) => return Err(Error::Corrupt("second IHDR".into())),
            (b"IDAT", true) => idat = true,
            _ => {}
        }
        chunks.push(Chunk { ty, raw });
        at = end;
        if &ty == b"IEND" {
            break;
        }
    }
    if !idat {
        return Err(Error::Corrupt("no image data".into()));
    }
    Ok((info.expect("IHDR is first"), chunks))
}

/// Check the structure and the size cap of a PNG without decoding the pixels.
pub fn check(bytes: &[u8]) -> Result<Info> {
    walk(bytes).map(|(info, _)| info)
}

/// The PNG without its text, time and EXIF chunks (and without bytes after IEND).
pub fn scrub(bytes: &[u8]) -> Result<Vec<u8>> {
    let (_, chunks) = walk(bytes)?;
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&SIGNATURE);
    for c in chunks.iter().filter(|c| !STRIPPED.contains(&&c.ty)) {
        out.extend_from_slice(c.raw);
    }
    Ok(out)
}

fn decode_err(e: png::DecodingError) -> Error {
    match e {
        png::DecodingError::IoError(e) => Error::Io(e),
        e => Error::Corrupt(e.to_string()),
    }
}

/// Decode to 8-bit RGBA (palette, grey, 16-bit and transparency are converted).
pub fn decode(bytes: &[u8]) -> Result<Image> {
    let info = check(bytes)?;
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_ignore_text_chunk(true);
    dec.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = dec.read_info().map_err(decode_err)?;
    let size = reader.output_buffer_size().ok_or(Error::TooLarge {
        width: info.width,
        height: info.height,
    })?;
    let mut buf = vec![0; size];
    let out = reader.next_frame(&mut buf).map_err(decode_err)?;
    buf.truncate(out.buffer_size());
    let data = match out.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(Error::Corrupt("palette not expanded".into())),
    };
    let img = Image {
        width: out.width,
        height: out.height,
        data,
    };
    if img.data.len() != img.width as usize * img.height as usize * 4 {
        return Err(Error::Corrupt("unexpected pixel data size".into()));
    }
    Ok(img)
}

/// Encode as an 8-bit RGBA PNG. The encoder writes no text, time or EXIF chunk.
pub fn encode(img: &Image) -> Result<Vec<u8>> {
    check_size(img.width, img.height)?;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let enc_err = |e: png::EncodingError| match e {
        png::EncodingError::IoError(e) => Error::Io(e),
        e => Error::Corrupt(e.to_string()),
    };
    let mut w = enc.write_header().map_err(enc_err)?;
    w.write_image_data(&img.data).map_err(enc_err)?;
    w.finish().map_err(enc_err)?;
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        c.extend_from_slice(ty);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32fast::hash(&c[4..]).to_be_bytes());
        c
    }

    /// `png` with one more chunk right after IHDR.
    pub fn with_chunk(png: &[u8], ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let ihdr_end = 8 + 12 + 13;
        let mut out = png[..ihdr_end].to_vec();
        out.extend(chunk(ty, data));
        out.extend_from_slice(&png[ihdr_end..]);
        out
    }

    pub fn chunk_types(png: &[u8]) -> Vec<[u8; 4]> {
        walk(png).unwrap().1.iter().map(|c| c.ty).collect()
    }

    /// A PNG whose IHDR claims `w`×`h`, with a tiny (not matching) IDAT: enough for [`check`].
    fn header_only(w: u32, h: u32) -> Vec<u8> {
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        let mut out = SIGNATURE.to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(
            b"IDAT",
            &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
        ));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn more_than_100_megapixels_is_refused_before_decoding() {
        let big = header_only(10_001, 10_000);
        assert!(matches!(
            check(&big),
            Err(Error::TooLarge {
                width: 10_001,
                height: 10_000
            })
        ));
        // decode and scrub refuse on the header alone: nothing near 400 MB is allocated
        assert!(matches!(decode(&big), Err(Error::TooLarge { .. })));
        assert!(matches!(scrub(&big), Err(Error::TooLarge { .. })));
        assert!(matches!(
            check(&header_only(u32::MAX, u32::MAX)),
            Err(Error::TooLarge { .. })
        ));
        // exactly 100 MP passes the cap
        assert_eq!(check(&header_only(10_000, 10_000)).unwrap().width, 10_000);
    }

    #[test]
    fn invalid_png_is_refused() {
        let good = encode(&Image::filled(2, 2, [1, 2, 3, 255])).unwrap();
        assert!(check(&good).is_ok());
        assert!(matches!(check(b""), Err(Error::NotPng)));
        assert!(matches!(
            check(b"\x89PNG\r\n\x1a\nxx"),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(check(b"GIF89a....."), Err(Error::NotPng)));
        // first chunk is not IHDR
        let mut no_ihdr = SIGNATURE.to_vec();
        no_ihdr.extend(chunk(b"IEND", &[]));
        assert!(matches!(check(&no_ihdr), Err(Error::NotPng)));
        // a flipped bit in the image data breaks the CRC
        let mut bad_crc = good.clone();
        let n = bad_crc.len();
        bad_crc[n - 20] ^= 1;
        assert!(matches!(check(&bad_crc), Err(Error::Corrupt(_))));
        // truncated: no IEND
        assert!(matches!(
            check(&good[..good.len() - 12]),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            check(&good[..good.len() - 5]),
            Err(Error::Corrupt(_))
        ));
        // zero size
        assert!(matches!(check(&header_only(0, 5)), Err(Error::Corrupt(_))));
        // no IDAT
        let mut no_idat = good[..33].to_vec();
        no_idat.extend(chunk(b"IEND", &[]));
        assert!(matches!(check(&no_idat), Err(Error::Corrupt(_))));
        assert!(decode(&bad_crc).is_err());
    }

    #[test]
    fn scrub_removes_text_and_time_chunks_only() {
        let img = Image::filled(3, 2, [9, 8, 7, 255]);
        let mut png = encode(&img).unwrap();
        for (ty, data) in [
            (b"tEXt", &b"Author\0user"[..]),
            (b"zTXt", b"Comment\0\0x"),
            (b"iTXt", b"Source\0\0\0\0\0/home/user/secret"),
            (b"tIME", &[7, 234, 11, 23, 9, 5, 7]),
            (b"eXIf", b"MM\0*"),
            (b"pHYs", &[0, 0, 11, 19, 0, 0, 11, 19, 1]),
        ] {
            png = with_chunk(&png, ty, data);
        }
        png.extend_from_slice(b"trailing /home/user");
        let clean = scrub(&png).unwrap();
        let types = chunk_types(&clean);
        for t in STRIPPED {
            assert!(!types.contains(t), "{}", String::from_utf8_lossy(t));
        }
        assert!(types.contains(b"pHYs"), "image chunks are kept");
        assert!(!clean.windows(4).any(|w| w == b"user"));
        assert_eq!(decode(&clean).unwrap(), img);
    }

    #[test]
    fn decode_converts_to_rgba() {
        // an RGB (no alpha) PNG becomes opaque RGBA
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 2, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        w.finish().unwrap();
        let img = decode(&out).unwrap();
        assert_eq!(img.data, [1, 2, 3, 255, 4, 5, 6, 255]);

        let grey = {
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[77]).unwrap();
            w.finish().unwrap();
            out
        };
        assert_eq!(decode(&grey).unwrap().data, [77, 77, 77, 255]);
    }
}
