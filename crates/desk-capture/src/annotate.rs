// SPDX-License-Identifier: GPL-3.0-or-later
//! The annotations of the editor and their renderer (8-bit RGBA, no anti-aliasing).
//!
//! Coordinates are pixels of the original capture, (0, 0) at the top left. The annotations are
//! drawn in list order; crops are applied last (their intersection), so a crop never moves the
//! coordinates of the other annotations. Shapes may extend past the image and are clipped.
//!
//! [`Annotation::Redact`] is destructive: [`RedactStyle::Solid`] writes an opaque colour and
//! [`RedactStyle::Pixelate`] replaces each block with its average. The saved file is the result
//! only; no layer, undo step or original pixel is kept (AC-SHOT-05).
//!
//! JSON form (`serde_json`), one object per annotation with a `kind`:
//!
//! ```json
//! [{"kind": "redact", "x": 10, "y": 10, "width": 80, "height": 20, "style": {"pixelate": 8}},
//!  {"kind": "arrow", "from": [0, 0], "to": [50, 40], "color": [230, 0, 0, 255], "width": 4},
//!  {"kind": "crop", "x": 0, "y": 0, "width": 200, "height": 100}]
//! ```

use serde::{Deserialize, Serialize};

use crate::{Error, Image, Result};

/// At most this many annotations in one request.
pub const MAX_ANNOTATIONS: usize = 4096;
/// At most this many points in all strokes of one request.
pub const MAX_POINTS: usize = 1_000_000;
/// At most this many bytes of JSON.
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Stroke widths, in pixels.
pub const WIDTH_RANGE: std::ops::RangeInclusive<u32> = 1..=256;
/// Pixelate block sizes, in pixels. A block under 4 px keeps small text readable.
pub const BLOCK_RANGE: std::ops::RangeInclusive<u32> = 4..=256;

/// How a redaction hides the pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactStyle {
    /// Fill with this colour; the alpha is ignored (the fill is always opaque).
    Solid([u8; 4]),
    /// Replace each block of this many pixels square by its average colour.
    Pixelate(u32),
}

fn default_stroke() -> u32 {
    3
}

fn default_marker() -> u32 {
    16
}

/// One annotation of the editor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Annotation {
    /// Destroy the pixels of a rectangle.
    Redact {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        style: RedactStyle,
    },
    /// Keep only this rectangle.
    Crop {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// A rectangle: an outline `stroke` pixels wide inside its bounds, or filled.
    Rect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: [u8; 4],
        #[serde(default = "default_stroke")]
        stroke: u32,
        #[serde(default)]
        fill: bool,
    },
    /// An ellipse in the bounds; outline or filled like [`Annotation::Rect`].
    Ellipse {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: [u8; 4],
        #[serde(default = "default_stroke")]
        stroke: u32,
        #[serde(default)]
        fill: bool,
    },
    /// A straight line with round ends.
    Line {
        from: [i32; 2],
        to: [i32; 2],
        color: [u8; 4],
        #[serde(default = "default_stroke")]
        width: u32,
    },
    /// A line with a filled arrow head at `to`.
    Arrow {
        from: [i32; 2],
        to: [i32; 2],
        color: [u8; 4],
        #[serde(default = "default_stroke")]
        width: u32,
    },
    /// A marker stroke: multiplies the colour into the image (text under it stays readable).
    Highlighter {
        points: Vec<[i32; 2]>,
        color: [u8; 4],
        #[serde(default = "default_marker")]
        width: u32,
    },
    /// A free-hand stroke (a polyline with round joins).
    Pen {
        points: Vec<[i32; 2]>,
        color: [u8; 4],
        #[serde(default = "default_stroke")]
        width: u32,
    },
}

/// Parse the JSON list of annotations and check it ([`validate`]).
pub fn parse(json: &str) -> Result<Vec<Annotation>> {
    if json.len() > MAX_JSON_BYTES {
        return Err(Error::Annotation("JSON too large".into()));
    }
    let list: Vec<Annotation> =
        serde_json::from_str(json).map_err(|e| Error::Annotation(e.to_string()))?;
    validate(&list)?;
    Ok(list)
}

fn bad(why: &str) -> Error {
    Error::Annotation(why.into())
}

/// Check sizes, widths and limits. [`apply`] calls it first, so a bad list changes nothing.
pub fn validate(list: &[Annotation]) -> Result<()> {
    if list.len() > MAX_ANNOTATIONS {
        return Err(bad("too many annotations"));
    }
    let mut points = 0usize;
    for a in list {
        match a {
            Annotation::Redact {
                width,
                height,
                style,
                ..
            } => {
                if *width == 0 || *height == 0 {
                    return Err(bad("empty redaction"));
                }
                if let RedactStyle::Pixelate(b) = style
                    && !BLOCK_RANGE.contains(b)
                {
                    return Err(bad("pixelate block out of range"));
                }
            }
            Annotation::Crop { width, height, .. } if *width == 0 || *height == 0 => {
                return Err(bad("empty crop"));
            }
            Annotation::Crop { .. } => {}
            Annotation::Rect {
                width,
                height,
                stroke,
                ..
            }
            | Annotation::Ellipse {
                width,
                height,
                stroke,
                ..
            } => {
                if *width == 0 || *height == 0 {
                    return Err(bad("empty shape"));
                }
                if !WIDTH_RANGE.contains(stroke) {
                    return Err(bad("stroke width out of range"));
                }
            }
            Annotation::Line { width, .. } | Annotation::Arrow { width, .. } => {
                if !WIDTH_RANGE.contains(width) {
                    return Err(bad("line width out of range"));
                }
            }
            Annotation::Highlighter {
                points: p, width, ..
            }
            | Annotation::Pen {
                points: p, width, ..
            } => {
                if p.is_empty() {
                    return Err(bad("stroke without points"));
                }
                if !WIDTH_RANGE.contains(width) {
                    return Err(bad("stroke width out of range"));
                }
                points += p.len();
            }
        }
    }
    if points > MAX_POINTS {
        return Err(bad("too many points"));
    }
    Ok(())
}

/// Draw the annotations into `img` (see the module docs for the order).
pub fn apply(img: &mut Image, list: &[Annotation]) -> Result<()> {
    validate(list)?;
    let mut crop: Option<Box2> = None;
    for a in list {
        match a {
            Annotation::Redact {
                x,
                y,
                width,
                height,
                style,
            } => {
                if let Some(r) = Box2::rect(*x, *y, *width, *height).clip(img) {
                    match style {
                        RedactStyle::Solid([r0, g, b, _]) => {
                            fill_opaque(img, r, [*r0, *g, *b, 255])
                        }
                        RedactStyle::Pixelate(block) => pixelate(img, r, i64::from(*block)),
                    }
                }
            }
            Annotation::Crop {
                x,
                y,
                width,
                height,
            } => {
                let r = Box2::rect(*x, *y, *width, *height);
                crop = Some(crop.map_or(r, |c| c.intersect(r)));
            }
            Annotation::Rect {
                x,
                y,
                width,
                height,
                color,
                stroke,
                fill,
            } => draw_rect(
                img,
                Box2::rect(*x, *y, *width, *height),
                *color,
                *stroke,
                *fill,
            ),
            Annotation::Ellipse {
                x,
                y,
                width,
                height,
                color,
                stroke,
                fill,
            } => draw_ellipse(
                img,
                Box2::rect(*x, *y, *width, *height),
                *color,
                *stroke,
                *fill,
            ),
            Annotation::Line {
                from,
                to,
                color,
                width,
            } => stroke_path(img, &[*from, *to], *color, *width, Blend::Over),
            Annotation::Arrow {
                from,
                to,
                color,
                width,
            } => draw_arrow(img, *from, *to, *color, *width),
            Annotation::Highlighter {
                points,
                color,
                width,
            } => stroke_path(img, points, *color, *width, Blend::Multiply),
            Annotation::Pen {
                points,
                color,
                width,
            } => stroke_path(img, points, *color, *width, Blend::Over),
        }
    }
    if let Some(c) = crop {
        let r = c.clip(img).ok_or_else(|| bad("crop outside the image"))?;
        *img = cropped(img, r);
    }
    Ok(())
}

/// A box of pixels, `x0..x1` × `y0..y1` (ends excluded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Box2 {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

impl Box2 {
    fn rect(x: i32, y: i32, w: u32, h: u32) -> Box2 {
        let (x, y) = (i64::from(x), i64::from(y));
        Box2 {
            x0: x,
            y0: y,
            x1: x + i64::from(w),
            y1: y + i64::from(h),
        }
    }

    fn intersect(self, o: Box2) -> Box2 {
        Box2 {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }

    /// The part inside the image, if any.
    fn clip(self, img: &Image) -> Option<Box2> {
        let b = self.intersect(Box2 {
            x0: 0,
            y0: 0,
            x1: i64::from(img.width),
            y1: i64::from(img.height),
        });
        (b.x0 < b.x1 && b.y0 < b.y1).then_some(b)
    }

    fn width(self) -> i64 {
        self.x1 - self.x0
    }

    fn height(self) -> i64 {
        self.y1 - self.y0
    }
}

/// A non-negative coordinate as an index (callers clip first).
fn ux(v: i64) -> usize {
    usize::try_from(v).unwrap_or(0)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped to the i32 range before the cast"
)]
fn floor(v: f64) -> i64 {
    v.floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped to the i32 range before the cast"
)]
fn ceil(v: f64) -> i64 {
    v.ceil().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
}

/// A coordinate as a float (exact: coordinates stay far below 2^52).
fn fx(v: i64) -> f64 {
    v as f64
}

fn index(img: &Image, x: i64, y: i64) -> usize {
    (ux(y) * img.width as usize + ux(x)) * 4
}

fn fill_opaque(img: &mut Image, r: Box2, rgba: [u8; 4]) {
    for y in r.y0..r.y1 {
        let (a, b) = (index(img, r.x0, y), index(img, r.x1, y));
        for px in img.data[a..b].as_chunks_mut::<4>().0 {
            px.copy_from_slice(&rgba);
        }
    }
}

fn pixelate(img: &mut Image, r: Box2, block: i64) {
    let mut by = r.y0;
    while by < r.y1 {
        let mut bx = r.x0;
        while bx < r.x1 {
            let b = Box2 {
                x0: bx,
                y0: by,
                x1: (bx + block).min(r.x1),
                y1: (by + block).min(r.y1),
            };
            let mut sum = [0u64; 4];
            for y in b.y0..b.y1 {
                let (a, e) = (index(img, b.x0, y), index(img, b.x1, y));
                for px in img.data[a..e].as_chunks::<4>().0 {
                    for (s, v) in sum.iter_mut().zip(px) {
                        *s += u64::from(*v);
                    }
                }
            }
            let n = u64::try_from(b.width() * b.height()).unwrap_or(1).max(1);
            let avg = sum.map(|s| u8::try_from((s + n / 2) / n).unwrap_or(u8::MAX));
            fill_opaque(img, b, avg);
            bx += block;
        }
        by += block;
    }
}

#[derive(Clone, Copy)]
enum Blend {
    /// Source over destination with the colour's alpha.
    Over,
    /// Multiply the colour into the destination, mixed by the colour's alpha.
    Multiply,
}

fn mix(dst: u8, src: u8, a: u32) -> u8 {
    let v = (u32::from(src) * a + u32::from(dst) * (255 - a) + 127) / 255;
    u8::try_from(v).unwrap_or(u8::MAX)
}

fn blend(px: &mut [u8], c: [u8; 4], mode: Blend) {
    let a = u32::from(c[3]);
    for i in 0..3 {
        let src = match mode {
            Blend::Over => c[i],
            Blend::Multiply => {
                u8::try_from((u32::from(px[i]) * u32::from(c[i]) + 127) / 255).unwrap_or(u8::MAX)
            }
        };
        px[i] = mix(px[i], src, a);
    }
    let da = u32::from(px[3]);
    px[3] = u8::try_from(a + (da * (255 - a) + 127) / 255).unwrap_or(u8::MAX);
}

/// The pixels a shape covers, within its bounds clipped to the image (one bit per pixel, so a
/// translucent shape is blended once even where its parts overlap).
struct Mask {
    b: Box2,
    bits: Vec<u64>,
}

impl Mask {
    fn new(img: &Image, b: Box2) -> Option<Mask> {
        let b = b.clip(img)?;
        let n = ux(b.width() * b.height());
        Some(Mask {
            b,
            bits: vec![0; n.div_ceil(64)],
        })
    }

    /// Mark the pixels of `area` whose centre passes `inside`.
    fn mark(&mut self, area: Box2, inside: impl Fn(f64, f64) -> bool) {
        let a = area.intersect(self.b);
        let w = self.b.width();
        for y in a.y0..a.y1 {
            for x in a.x0..a.x1 {
                if inside(fx(x) + 0.5, fx(y) + 0.5) {
                    let i = ux((y - self.b.y0) * w + (x - self.b.x0));
                    self.bits[i / 64] |= 1 << (i % 64);
                }
            }
        }
    }

    fn paint(&self, img: &mut Image, c: [u8; 4], mode: Blend) {
        let w = self.b.width();
        for y in self.b.y0..self.b.y1 {
            for x in self.b.x0..self.b.x1 {
                let i = ux((y - self.b.y0) * w + (x - self.b.x0));
                if self.bits[i / 64] & (1 << (i % 64)) != 0 {
                    let o = index(img, x, y);
                    blend(&mut img.data[o..o + 4], c, mode);
                }
            }
        }
    }
}

fn draw_rect(img: &mut Image, r: Box2, c: [u8; 4], stroke: u32, fill: bool) {
    let Some(mut m) = Mask::new(img, r) else {
        return;
    };
    let s = i64::from(stroke);
    let inner = Box2 {
        x0: r.x0 + s,
        y0: r.y0 + s,
        x1: r.x1 - s,
        y1: r.y1 - s,
    };
    m.mark(r, |x, y| {
        fill || !(x > fx(inner.x0) && x < fx(inner.x1) && y > fx(inner.y0) && y < fx(inner.y1))
    });
    m.paint(img, c, Blend::Over);
}

fn draw_ellipse(img: &mut Image, r: Box2, c: [u8; 4], stroke: u32, fill: bool) {
    let Some(mut m) = Mask::new(img, r) else {
        return;
    };
    let (cx, cy) = (fx(r.x0 + r.x1) / 2.0, fx(r.y0 + r.y1) / 2.0);
    let (rx, ry) = (fx(r.width()) / 2.0, fx(r.height()) / 2.0);
    let s = f64::from(stroke);
    let (ix, iy) = (rx - s, ry - s);
    let within = |x: f64, y: f64, a: f64, b: f64| {
        a > 0.0 && b > 0.0 && ((x - cx) / a).powi(2) + ((y - cy) / b).powi(2) <= 1.0
    };
    m.mark(r, |x, y| {
        within(x, y, rx, ry) && (fill || !within(x, y, ix, iy))
    });
    m.paint(img, c, Blend::Over);
}

fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

/// The bounds of points (pixel centres) widened by `pad` pixels.
fn bounds(points: &[(f64, f64)], pad: f64) -> Box2 {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    Box2 {
        x0: floor(x0 - pad),
        y0: floor(y0 - pad),
        x1: ceil(x1 + pad) + 1,
        y1: ceil(y1 + pad) + 1,
    }
}

fn centre(p: [i32; 2]) -> (f64, f64) {
    (f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.5)
}

/// Mark a polyline of width `w` (round ends and joins) into `m`.
fn mark_path(m: &mut Mask, pts: &[(f64, f64)], w: f64) {
    let half = w / 2.0;
    let segs: Vec<((f64, f64), (f64, f64))> = if pts.len() == 1 {
        vec![(pts[0], pts[0])]
    } else {
        pts.windows(2).map(|s| (s[0], s[1])).collect()
    };
    for (a, b) in segs {
        m.mark(bounds(&[a, b], half), |x, y| {
            distance_to_segment((x, y), a, b) <= half
        });
    }
}

fn stroke_path(img: &mut Image, points: &[[i32; 2]], c: [u8; 4], width: u32, mode: Blend) {
    let pts: Vec<(f64, f64)> = points.iter().copied().map(centre).collect();
    let w = f64::from(width);
    let Some(mut m) = Mask::new(img, bounds(&pts, w / 2.0)) else {
        return;
    };
    mark_path(&mut m, &pts, w);
    m.paint(img, c, mode);
}

fn draw_arrow(img: &mut Image, from: [i32; 2], to: [i32; 2], c: [u8; 4], width: u32) {
    let (a, tip) = (centre(from), centre(to));
    let w = f64::from(width);
    let head = (w * 4.0).max(12.0);
    let (dx, dy) = (tip.0 - a.0, tip.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        stroke_path(img, &[from], c, width, Blend::Over);
        return;
    }
    let (ux_, uy) = (dx / len, dy / len);
    let head = head.min(len);
    let base = (tip.0 - ux_ * head, tip.1 - uy * head);
    let half = head * 0.5;
    let l = (base.0 - uy * half, base.1 + ux_ * half);
    let r = (base.0 + uy * half, base.1 - ux_ * half);
    let Some(mut m) = Mask::new(img, bounds(&[a, tip, l, r], w / 2.0)) else {
        return;
    };
    mark_path(&mut m, &[a, base], w);
    let cross = |p: (f64, f64), q: (f64, f64), s: (f64, f64)| {
        (q.0 - p.0) * (s.1 - p.1) - (q.1 - p.1) * (s.0 - p.0)
    };
    m.mark(bounds(&[tip, l, r], 0.0), |x, y| {
        let p = (x, y);
        let (d1, d2, d3) = (cross(tip, l, p), cross(l, r, p), cross(r, tip, p));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    });
    m.paint(img, c, Blend::Over);
}

fn cropped(img: &Image, r: Box2) -> Image {
    let (w, h) = (r.width(), r.height());
    let mut data = Vec::with_capacity(ux(w * h * 4));
    for y in r.y0..r.y1 {
        data.extend_from_slice(&img.data[index(img, r.x0, y)..index(img, r.x1, y)]);
    }
    Image {
        width: u32::try_from(w).unwrap_or(0),
        height: u32::try_from(h).unwrap_or(0),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    /// A "text" image: a different colour in every pixel, so any kept original pixel is visible.
    fn gradient(w: u32, h: u32) -> Image {
        let mut img = Image::filled(w, h, WHITE);
        for y in 0..h {
            for x in 0..w {
                let v = |n: u32| u8::try_from(n % 256).unwrap();
                img.set_pixel(x, y, [v(x * 7 + y), v(y * 13 + x * 3), v(x * y + 5), 255]);
            }
        }
        img
    }

    fn redact(x: i32, y: i32, w: u32, h: u32, style: RedactStyle) -> Annotation {
        Annotation::Redact {
            x,
            y,
            width: w,
            height: h,
            style,
        }
    }

    #[test]
    fn solid_redaction_destroys_the_pixels() {
        let orig = gradient(40, 30);
        let mut img = orig.clone();
        // a translucent colour is still written opaque
        apply(
            &mut img,
            &[redact(5, 5, 10, 8, RedactStyle::Solid([0, 0, 0, 10]))],
        )
        .unwrap();
        for y in 0..30 {
            for x in 0..40 {
                let inside = (5..15).contains(&x) && (5..13).contains(&y);
                if inside {
                    assert_eq!(img.pixel(x, y), [0, 0, 0, 255], "({x},{y})");
                } else {
                    assert_eq!(img.pixel(x, y), orig.pixel(x, y), "({x},{y})");
                }
            }
        }
    }

    #[test]
    fn pixelate_redaction_destroys_the_pixels() {
        let orig = gradient(40, 30);
        let mut img = orig.clone();
        // starts outside the image: clipped to 0..12 × 0..8
        apply(
            &mut img,
            &[redact(-4, -4, 16, 12, RedactStyle::Pixelate(4))],
        )
        .unwrap();
        let mut changed = 0;
        for y in 0..8u32 {
            for x in 0..12u32 {
                // every 4×4 block (from the clipped origin) holds one colour
                assert_eq!(
                    img.pixel(x, y),
                    img.pixel(x - x % 4, y - y % 4),
                    "({x},{y})"
                );
                if img.pixel(x, y) != orig.pixel(x, y) {
                    changed += 1;
                }
            }
        }
        assert!(changed >= 90, "only {changed} of 96 pixels changed");
        assert_eq!(img.pixel(12, 0), orig.pixel(12, 0));
        assert_eq!(img.pixel(0, 8), orig.pixel(0, 8));
        // the encoded result has no copy of the original
        let png = crate::codec::encode(&img).unwrap();
        assert_eq!(crate::codec::decode(&png).unwrap(), img);
    }

    #[test]
    fn crop_keeps_only_the_intersection_and_is_applied_last() {
        let orig = gradient(20, 10);
        let mut img = orig.clone();
        let crop = |x, y, w, h| Annotation::Crop {
            x,
            y,
            width: w,
            height: h,
        };
        apply(
            &mut img,
            &[
                crop(2, 1, 10, 8),
                // in original coordinates although it comes after the first crop
                redact(4, 3, 1, 1, RedactStyle::Solid(RED)),
                crop(3, 0, 50, 6),
            ],
        )
        .unwrap();
        assert_eq!((img.width, img.height), (9, 5));
        assert_eq!(img.pixel(0, 0), orig.pixel(3, 1));
        assert_eq!(img.pixel(1, 2), RED);
        assert_eq!(img.pixel(8, 4), orig.pixel(11, 5));

        let mut img = orig.clone();
        assert!(apply(&mut img, &[crop(100, 100, 5, 5)]).is_err());
    }

    #[test]
    fn shapes_are_drawn() {
        let mut img = Image::filled(40, 40, WHITE);
        let list = [
            Annotation::Rect {
                x: 2,
                y: 2,
                width: 10,
                height: 10,
                color: RED,
                stroke: 2,
                fill: false,
            },
            Annotation::Line {
                from: [0, 20],
                to: [39, 20],
                color: [0, 0, 255, 255],
                width: 1,
            },
            Annotation::Pen {
                points: vec![[20, 30], [25, 30], [25, 35]],
                color: [0, 128, 0, 255],
                width: 3,
            },
            Annotation::Arrow {
                from: [33, 2],
                to: [33, 38],
                color: [0, 0, 0, 255],
                width: 2,
            },
            Annotation::Ellipse {
                x: 0,
                y: 24,
                width: 12,
                height: 12,
                color: RED,
                stroke: 1,
                fill: true,
            },
        ];
        apply(&mut img, &list).unwrap();
        // rectangle outline: border pixels red, inside untouched
        assert_eq!(img.pixel(2, 2), RED);
        assert_eq!(img.pixel(3, 7), RED);
        assert_eq!(img.pixel(11, 11), RED);
        assert_eq!(img.pixel(7, 7), WHITE);
        // the line runs along row 20 only (the arrow, drawn later, crosses it at x = 33)
        assert!((0..28).all(|x| img.pixel(x, 20) == [0, 0, 255, 255]));
        assert_eq!(img.pixel(5, 22), WHITE);
        // pen: both segments and the joint
        assert_eq!(img.pixel(22, 30), [0, 128, 0, 255]);
        assert_eq!(img.pixel(25, 30), [0, 128, 0, 255]);
        assert_eq!(img.pixel(25, 34), [0, 128, 0, 255]);
        assert_eq!(img.pixel(22, 34), WHITE);
        // arrow: a 2 px shaft, and a head wider than the shaft near the tip
        assert_eq!(img.pixel(33, 10), [0, 0, 0, 255]);
        assert_eq!(img.pixel(30, 10), WHITE);
        assert_eq!(img.pixel(30, 30), [0, 0, 0, 255]);
        // filled ellipse: centre in, corner out
        assert_eq!(img.pixel(6, 30), RED);
        assert_eq!(img.pixel(0, 24), WHITE);
    }

    #[test]
    fn highlighter_multiplies_once_where_it_overlaps() {
        let mut img = Image::filled(30, 10, [200, 200, 200, 255]);
        img.set_pixel(5, 5, [0, 0, 0, 255]);
        apply(
            &mut img,
            &[Annotation::Highlighter {
                // goes right and back over itself
                points: vec![[2, 5], [20, 5], [4, 5]],
                color: [255, 255, 0, 255],
                width: 4,
            }],
        )
        .unwrap();
        // yellow multiply: red and green kept, blue gone; dark "text" stays dark
        assert_eq!(img.pixel(10, 5), [200, 200, 0, 255]);
        assert_eq!(img.pixel(5, 5), [0, 0, 0, 255]);
        assert_eq!(img.pixel(10, 0), [200, 200, 200, 255]);
    }

    #[test]
    fn translucent_pen_blends() {
        let mut img = Image::filled(5, 5, WHITE);
        apply(
            &mut img,
            &[Annotation::Pen {
                points: vec![[2, 2]],
                color: [0, 0, 0, 128],
                width: 1,
            }],
        )
        .unwrap();
        assert_eq!(img.pixel(2, 2), [127, 127, 127, 255]);
    }

    #[test]
    fn json_round_trip_and_validation() {
        let json = r#"[
            {"kind": "redact", "x": 1, "y": 2, "width": 3, "height": 4, "style": {"solid": [0, 0, 0, 255]}},
            {"kind": "redact", "x": 1, "y": 2, "width": 3, "height": 4, "style": {"pixelate": 8}},
            {"kind": "crop", "x": 0, "y": 0, "width": 9, "height": 9},
            {"kind": "pen", "points": [[0, 0], [3, 3]], "color": [1, 2, 3, 255]}
        ]"#;
        let list = parse(json).unwrap();
        assert_eq!(list.len(), 4);
        assert!(matches!(list[3], Annotation::Pen { width: 3, .. }));
        let again: Vec<Annotation> =
            serde_json::from_str(&serde_json::to_string(&list).unwrap()).unwrap();
        assert_eq!(again, list);

        for bad in [
            "not json",
            r#"[{"kind": "sparkles"}]"#,
            r#"[{"kind": "redact", "x": 0, "y": 0, "width": 0, "height": 4, "style": {"solid": [0,0,0,255]}}]"#,
            r#"[{"kind": "redact", "x": 0, "y": 0, "width": 4, "height": 4, "style": {"pixelate": 1}}]"#,
            r#"[{"kind": "pen", "points": [], "color": [0,0,0,255]}]"#,
            r#"[{"kind": "line", "from": [0,0], "to": [1,1], "color": [0,0,0,255], "width": 0}]"#,
        ] {
            assert!(matches!(parse(bad), Err(Error::Annotation(_))), "{bad}");
        }
        // a bad list changes nothing
        let mut img = Image::filled(4, 4, WHITE);
        let list = [
            redact(0, 0, 4, 4, RedactStyle::Solid(RED)),
            redact(0, 0, 0, 0, RedactStyle::Solid(RED)),
        ];
        assert!(apply(&mut img, &list).is_err());
        assert_eq!(img, Image::filled(4, 4, WHITE));
    }
}
