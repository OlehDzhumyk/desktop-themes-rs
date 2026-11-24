// SPDX-License-Identifier: GPL-3.0-or-later
//! State of the screenshot editor, without GTK: the tools, colours and widths, the drag that makes
//! an annotation, undo and redo, and the rendering through [`desk_capture::annotate::apply`].
//!
//! Coordinates are pixels of the original image. The list holds every committed annotation; the
//! rendered result uses all of them except crops, and only the last crop (a new crop replaces the
//! old one, and undo brings the old one back). Nothing of this state goes into a saved file: the
//! file has the rendered pixels only (AC-SHOT-05).

use desk_capture::annotate::{BLOCK_RANGE, MAX_ANNOTATIONS, WIDTH_RANGE};
use desk_capture::{Annotation, Image, RedactStyle};
use desk_settings::n_;

/// A tool of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    RedactSolid,
    RedactPixelate,
    Pen,
    Highlighter,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Crop,
}

impl Tool {
    /// All tools in the order of the tool bar.
    pub const ALL: [Tool; 9] = [
        Tool::Pen,
        Tool::Highlighter,
        Tool::Arrow,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::RedactSolid,
        Tool::RedactPixelate,
        Tool::Crop,
    ];

    /// The name (a gettext id).
    pub fn label(self) -> &'static str {
        match self {
            Tool::RedactSolid => n_("Redact (solid)"),
            Tool::RedactPixelate => n_("Redact (pixelate)"),
            Tool::Pen => n_("Pen"),
            Tool::Highlighter => n_("Highlighter"),
            Tool::Arrow => n_("Arrow"),
            Tool::Line => n_("Line"),
            Tool::Rect => n_("Rectangle"),
            Tool::Ellipse => n_("Ellipse"),
            Tool::Crop => n_("Crop"),
        }
    }

    /// The key that selects the tool (without a modifier).
    pub fn key(self) -> char {
        match self {
            Tool::RedactSolid => 'x',
            Tool::RedactPixelate => 'm',
            Tool::Pen => 'p',
            Tool::Highlighter => 'h',
            Tool::Arrow => 'a',
            Tool::Line => 'l',
            Tool::Rect => 'r',
            Tool::Ellipse => 'e',
            Tool::Crop => 'c',
        }
    }

    /// The tool of a key, if any.
    pub fn from_key(c: char) -> Option<Tool> {
        let c = c.to_ascii_lowercase();
        Tool::ALL.into_iter().find(|t| t.key() == c)
    }

    /// Whether the tool uses the colour and the width (redaction and crop do not).
    pub fn uses_color(self) -> bool {
        !matches!(self, Tool::RedactSolid | Tool::RedactPixelate | Tool::Crop)
    }
}

/// A colour of the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedColor {
    /// The name (a gettext id), for the accessible name and the tooltip.
    pub name: &'static str,
    pub rgba: [u8; 4],
}

/// The palette: the accent colour first, then the GNOME palette, black and white.
pub fn palette(accent: [u8; 4]) -> Vec<NamedColor> {
    let c = |name, rgba| NamedColor { name, rgba };
    vec![
        c(n_("Accent colour"), [accent[0], accent[1], accent[2], 255]),
        c(n_("Red"), [0xe0, 0x1b, 0x24, 255]),
        c(n_("Orange"), [0xff, 0x78, 0x00, 255]),
        c(n_("Yellow"), [0xf6, 0xd3, 0x2d, 255]),
        c(n_("Green"), [0x33, 0xd1, 0x7a, 255]),
        c(n_("Blue"), [0x35, 0x84, 0xe4, 255]),
        c(n_("Purple"), [0x91, 0x41, 0xac, 255]),
        c(n_("Black"), [0, 0, 0, 255]),
        c(n_("White"), [255, 255, 255, 255]),
    ]
}

/// Stroke widths in pixels with their names (gettext ids).
pub const WIDTHS: [(u32, &str); 4] = [
    (2, n_("Thin")),
    (4, n_("Medium")),
    (8, n_("Thick")),
    (16, n_("Extra thick")),
];

/// Block size of the pixelate redaction: large enough that text cannot be read back.
pub const PIXELATE_BLOCK: u32 = 12;
/// Fill of the solid redaction (always opaque).
pub const REDACT_FILL: [u8; 4] = [0, 0, 0, 255];
/// A highlighter is this many times wider than the chosen width (and at least 12 px).
const MARKER_FACTOR: u32 = 4;
/// At most this many points in one free-hand stroke (desk-capture allows 1 000 000 in all).
const MAX_STROKE_POINTS: usize = 50_000;

/// The drag in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Drag {
    tool: Tool,
    start: [i32; 2],
    end: [i32; 2],
    points: Vec<[i32; 2]>,
}

/// The editor: the image size, the current tool and style, the annotations and their history.
#[derive(Clone, Debug)]
pub struct EditorState {
    width: u32,
    height: u32,
    pub tool: Tool,
    pub color: [u8; 4],
    pub stroke: u32,
    done: Vec<Annotation>,
    undone: Vec<Annotation>,
    saved: Vec<Annotation>,
    drag: Option<Drag>,
}

impl EditorState {
    /// A new editor for an image of `width` × `height` pixels.
    pub fn new(width: u32, height: u32, color: [u8; 4]) -> EditorState {
        EditorState {
            width,
            height,
            tool: Tool::Pen,
            color,
            stroke: WIDTHS[1].0,
            done: Vec::new(),
            undone: Vec::new(),
            saved: Vec::new(),
            drag: None,
        }
    }

    /// The committed annotations, oldest first.
    #[cfg(test)]
    pub fn annotations(&self) -> &[Annotation] {
        &self.done
    }

    /// Whether the annotations differ from the last save (or from the opened file).
    pub fn dirty(&self) -> bool {
        self.done != self.saved
    }

    /// Record that the current annotations are now saved.
    pub fn mark_saved(&mut self) {
        self.saved = self.done.clone();
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Take back the last annotation. `false`: there was none.
    pub fn undo(&mut self) -> bool {
        self.drag = None;
        match self.done.pop() {
            Some(a) => {
                self.undone.push(a);
                true
            }
            None => false,
        }
    }

    /// Put back the last undone annotation. `false`: there was none.
    pub fn redo(&mut self) -> bool {
        self.drag = None;
        match self.undone.pop() {
            Some(a) => {
                self.done.push(a);
                true
            }
            None => false,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Start a drag with the current tool at `p`.
    pub fn drag_begin(&mut self, p: [i32; 2]) {
        self.drag = Some(Drag {
            tool: self.tool,
            start: p,
            end: p,
            points: vec![p],
        });
    }

    /// Move the drag to `p`. Free-hand strokes keep every new point.
    pub fn drag_update(&mut self, p: [i32; 2]) {
        let Some(d) = &mut self.drag else {
            return;
        };
        d.end = p;
        if matches!(d.tool, Tool::Pen | Tool::Highlighter)
            && d.points.last() != Some(&p)
            && d.points.len() < MAX_STROKE_POINTS
        {
            d.points.push(p);
        }
    }

    /// Drop the drag without an annotation.
    pub fn drag_cancel(&mut self) {
        self.drag = None;
    }

    /// End the drag. Returns `true` if it made an annotation (a click without movement makes
    /// none, except a dot of the pen or highlighter).
    pub fn drag_commit(&mut self) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        if self.done.len() >= MAX_ANNOTATIONS {
            return false;
        }
        let Some(a) = self.annotation_of(&d) else {
            return false;
        };
        self.done.push(a);
        self.undone.clear();
        true
    }

    /// The annotation the drag in progress would make.
    pub fn preview(&self) -> Option<Annotation> {
        self.drag.as_ref().and_then(|d| self.annotation_of(d))
    }

    fn annotation_of(&self, d: &Drag) -> Option<Annotation> {
        let color = self.color;
        let width = self.stroke.clamp(*WIDTH_RANGE.start(), *WIDTH_RANGE.end());
        let rect = || {
            let (x0, x1) = (d.start[0].min(d.end[0]), d.start[0].max(d.end[0]));
            let (y0, y1) = (d.start[1].min(d.end[1]), d.start[1].max(d.end[1]));
            let w = x1.abs_diff(x0);
            let h = y1.abs_diff(y0);
            (w > 0 && h > 0).then_some((x0, y0, w, h))
        };
        let a = match d.tool {
            Tool::RedactSolid | Tool::RedactPixelate => {
                let (x, y, width, height) = rect()?;
                let style = if d.tool == Tool::RedactSolid {
                    RedactStyle::Solid(REDACT_FILL)
                } else {
                    RedactStyle::Pixelate(
                        PIXELATE_BLOCK.clamp(*BLOCK_RANGE.start(), *BLOCK_RANGE.end()),
                    )
                };
                Annotation::Redact {
                    x,
                    y,
                    width,
                    height,
                    style,
                }
            }
            Tool::Crop => {
                let (x, y, w, h) = self.clip(rect()?)?;
                Annotation::Crop {
                    x,
                    y,
                    width: w,
                    height: h,
                }
            }
            Tool::Rect | Tool::Ellipse => {
                let (x, y, w, h) = rect()?;
                if d.tool == Tool::Rect {
                    Annotation::Rect {
                        x,
                        y,
                        width: w,
                        height: h,
                        color,
                        stroke: width,
                        fill: false,
                    }
                } else {
                    Annotation::Ellipse {
                        x,
                        y,
                        width: w,
                        height: h,
                        color,
                        stroke: width,
                        fill: false,
                    }
                }
            }
            Tool::Line | Tool::Arrow => {
                if d.start == d.end {
                    return None;
                }
                let (from, to) = (d.start, d.end);
                if d.tool == Tool::Line {
                    Annotation::Line {
                        from,
                        to,
                        color,
                        width,
                    }
                } else {
                    Annotation::Arrow {
                        from,
                        to,
                        color,
                        width,
                    }
                }
            }
            Tool::Pen => Annotation::Pen {
                points: d.points.clone(),
                color,
                width,
            },
            Tool::Highlighter => Annotation::Highlighter {
                points: d.points.clone(),
                color,
                width: (width * MARKER_FACTOR).clamp(12, *WIDTH_RANGE.end()),
            },
        };
        Some(a)
    }

    /// The part of a rectangle inside the image, if any.
    fn clip(&self, (x, y, w, h): (i32, i32, u32, u32)) -> Option<(i32, i32, u32, u32)> {
        let (iw, ih) = (i64::from(self.width), i64::from(self.height));
        let x0 = i64::from(x).clamp(0, iw);
        let y0 = i64::from(y).clamp(0, ih);
        let x1 = (i64::from(x) + i64::from(w)).clamp(0, iw);
        let y1 = (i64::from(y) + i64::from(h)).clamp(0, ih);
        let w = u32::try_from(x1 - x0).ok().filter(|v| *v > 0)?;
        let h = u32::try_from(y1 - y0).ok().filter(|v| *v > 0)?;
        Some((i32::try_from(x0).ok()?, i32::try_from(y0).ok()?, w, h))
    }

    /// The crop in effect (the last committed one, or the one being dragged): x, y, w, h.
    pub fn crop(&self, with_drag: bool) -> Option<(i32, i32, u32, u32)> {
        let from_drag = with_drag
            .then(|| self.preview())
            .flatten()
            .filter(|a| matches!(a, Annotation::Crop { .. }));
        from_drag
            .iter()
            .chain(self.done.iter().rev())
            .find_map(|a| match a {
                Annotation::Crop {
                    x,
                    y,
                    width,
                    height,
                } => Some((*x, *y, *width, *height)),
                _ => None,
            })
    }

    /// The list that is rendered: every annotation except crops, then the last crop.
    pub fn effective(&self) -> Vec<Annotation> {
        let mut list: Vec<Annotation> = self
            .done
            .iter()
            .filter(|a| !matches!(a, Annotation::Crop { .. }))
            .cloned()
            .collect();
        if let Some((x, y, width, height)) = self.crop(false) {
            list.push(Annotation::Crop {
                x,
                y,
                width,
                height,
            });
        }
        list
    }

    /// The final image: `base` with every annotation, redaction destructive, crop applied.
    pub fn apply_to_image(&self, base: &Image) -> desk_capture::Result<Image> {
        let mut img = base.clone();
        desk_capture::annotate::apply(&mut img, &self.effective())?;
        Ok(img)
    }

    /// `base` with the committed annotations except the crop (the editor's view; the crop is shown
    /// as a dimmed outside, see [`dim_outside`]).
    pub fn render_committed(&self, base: &Image) -> desk_capture::Result<Image> {
        let list: Vec<Annotation> = self
            .done
            .iter()
            .filter(|a| !matches!(a, Annotation::Crop { .. }))
            .cloned()
            .collect();
        let mut img = base.clone();
        desk_capture::annotate::apply(&mut img, &list)?;
        Ok(img)
    }

    /// The editor's view: `committed` (from [`Self::render_committed`]) with the drag in
    /// progress, and the crop in effect shown by dimming the outside.
    pub fn render_view(&self, committed: &Image) -> desk_capture::Result<Image> {
        let mut img = committed.clone();
        if let Some(a) = self
            .preview()
            .filter(|a| !matches!(a, Annotation::Crop { .. }))
        {
            desk_capture::annotate::apply(&mut img, std::slice::from_ref(&a))?;
        }
        if let Some(c) = self.crop(true) {
            dim_outside(&mut img, c);
        }
        Ok(img)
    }
}

/// Write a PNG to `path` atomically (a temporary file renamed over it, see
/// [`desk_capture::save::replace`]). An existing file keeps its permissions; a new file (Save As)
/// gets the default ones and is removed again if the write fails.
pub fn write_png(path: &std::path::Path, png: &[u8]) -> desk_capture::Result<()> {
    let created = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(e.into()),
    };
    let res = desk_capture::save::replace(path, png);
    if res.is_err() && created {
        let _ = std::fs::remove_file(path);
    }
    res
}

/// Darken the pixels outside the rectangle to half (the crop preview).
pub fn dim_outside(img: &mut Image, (x, y, w, h): (i32, i32, u32, u32)) {
    let to_index = |v: i64| usize::try_from(v.max(0)).unwrap_or(usize::MAX);
    let (x0, y0) = (to_index(i64::from(x)), to_index(i64::from(y)));
    let x1 = to_index(i64::from(x) + i64::from(w));
    let y1 = to_index(i64::from(y) + i64::from(h));
    let width = img.width as usize;
    for (i, px) in img.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (px_x, px_y) = (i % width, i / width);
        if px_x < x0 || px_x >= x1 || px_y < y0 || px_y >= y1 {
            for v in &mut px[..3] {
                *v /= 2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];

    /// "Text": every pixel different, so a kept original pixel shows in a diff.
    fn text_image(w: u32, h: u32) -> Image {
        let mut img = Image::filled(w, h, [255, 255, 255, 255]);
        for y in 0..h {
            for x in 0..w {
                let v = |n: u32| u8::try_from(n % 251).unwrap();
                img.set_pixel(x, y, [v(x * 7 + y * 3), v(y * 11 + x), v(x * y + 13), 255]);
            }
        }
        img
    }

    fn drag(s: &mut EditorState, tool: Tool, pts: &[[i32; 2]]) -> bool {
        s.tool = tool;
        s.drag_begin(pts[0]);
        for p in &pts[1..] {
            s.drag_update(*p);
        }
        s.drag_commit()
    }

    #[test]
    fn every_tool_makes_a_valid_annotation() {
        let mut s = EditorState::new(100, 80, RED);
        for tool in Tool::ALL {
            assert!(
                drag(&mut s, tool, &[[30, 20], [10, 50], [60, 40]]),
                "{tool:?}"
            );
        }
        let list = s.annotations();
        assert_eq!(list.len(), Tool::ALL.len());
        desk_capture::annotate::validate(list).unwrap();
        // rectangles are normalised from the start and the last point, whatever the direction
        assert!(matches!(
            list[4],
            Annotation::Rect {
                x: 30,
                y: 20,
                width: 30,
                height: 20,
                color: RED,
                stroke: 4,
                fill: false
            }
        ));
        assert!(matches!(
            list[6],
            Annotation::Redact {
                style: RedactStyle::Solid(REDACT_FILL),
                ..
            }
        ));
        assert!(matches!(
            list[7],
            Annotation::Redact {
                style: RedactStyle::Pixelate(PIXELATE_BLOCK),
                ..
            }
        ));
        match &list[0] {
            Annotation::Pen { points, width, .. } => {
                assert_eq!(points, &vec![[30, 20], [10, 50], [60, 40]]);
                assert_eq!(*width, 4);
            }
            a => panic!("{a:?}"),
        }
        assert!(matches!(list[1], Annotation::Highlighter { width: 16, .. }));
        assert!(matches!(
            list[2],
            Annotation::Arrow {
                from: [30, 20],
                to: [60, 40],
                ..
            }
        ));
        // the JSON form that deskd's ApplyAnnotations takes reads back the same
        let json = serde_json::to_string(list).unwrap();
        assert_eq!(desk_capture::annotate::parse(&json).unwrap(), list);
        // and the whole list renders
        let out = s.apply_to_image(&text_image(100, 80)).unwrap();
        assert_eq!((out.width, out.height), (30, 20));
    }

    #[test]
    fn a_click_without_movement_makes_no_shape_but_the_pen_makes_a_dot() {
        let mut s = EditorState::new(50, 50, RED);
        for tool in Tool::ALL {
            let made = drag(&mut s, tool, &[[5, 5], [5, 5]]);
            assert_eq!(
                made,
                matches!(tool, Tool::Pen | Tool::Highlighter),
                "{tool:?}"
            );
        }
        // a flat rectangle (zero height) is no annotation either
        assert!(!drag(&mut s, Tool::Rect, &[[5, 5], [30, 5]]));
    }

    #[test]
    fn crop_is_clipped_to_the_image_and_the_last_one_wins() {
        let mut s = EditorState::new(40, 30, RED);
        assert!(!drag(&mut s, Tool::Crop, &[[50, 50], [70, 70]]));
        assert!(drag(&mut s, Tool::Crop, &[[-10, -10], [20, 10]]));
        assert_eq!(s.crop(false), Some((0, 0, 20, 10)));
        assert!(drag(&mut s, Tool::Crop, &[[10, 5], [35, 25]]));
        assert_eq!(s.crop(false), Some((10, 5, 25, 20)));
        let out = s.apply_to_image(&text_image(40, 30)).unwrap();
        assert_eq!((out.width, out.height), (25, 20));
        // undo gives the first crop back
        assert!(s.undo());
        let out = s.apply_to_image(&text_image(40, 30)).unwrap();
        assert_eq!((out.width, out.height), (20, 10));
        // the crop being dragged shows in the view, before the commit
        s.tool = Tool::Crop;
        s.drag_begin([0, 0]);
        s.drag_update([5, 5]);
        assert_eq!(s.crop(true), Some((0, 0, 5, 5)));
        assert_eq!(s.crop(false), Some((0, 0, 20, 10)));
    }

    #[test]
    fn undo_redo_and_dirty() {
        let mut s = EditorState::new(50, 50, RED);
        assert!(!s.dirty() && !s.can_undo() && !s.can_redo());
        assert!(!s.undo() && !s.redo());
        drag(&mut s, Tool::Line, &[[0, 0], [10, 10]]);
        drag(&mut s, Tool::Rect, &[[0, 0], [10, 10]]);
        assert!(s.dirty() && s.can_undo());
        assert!(s.undo());
        assert_eq!(s.annotations().len(), 1);
        assert!(s.can_redo());
        assert!(s.redo());
        assert_eq!(s.annotations().len(), 2);
        assert!(!s.can_redo());

        s.mark_saved();
        assert!(!s.dirty());
        // undo after a save is a change; redo back to the saved list is not
        s.undo();
        assert!(s.dirty());
        s.redo();
        assert!(!s.dirty());
        // a new annotation drops the redo history
        s.undo();
        drag(&mut s, Tool::Ellipse, &[[1, 1], [9, 9]]);
        assert!(!s.can_redo());
        assert!(s.dirty());
        // undo to empty after a save of a non-empty list is still a change
        while s.undo() {}
        assert!(s.dirty());
        // a cancelled drag changes nothing
        s.tool = Tool::Pen;
        s.drag_begin([1, 1]);
        s.drag_update([3, 3]);
        assert!(s.preview().is_some());
        s.drag_cancel();
        assert!(s.preview().is_none() && !s.can_undo());
    }

    #[test]
    fn redaction_destroys_the_original_pixels_in_the_saved_file() {
        // AC-SHOT-05: the saved file's pixels in the redacted rectangle no longer hold the
        // original, and the file is one plain RGBA image without text or time chunks.
        let orig = text_image(64, 48);
        let mut s = EditorState::new(64, 48, RED);
        assert!(drag(&mut s, Tool::RedactSolid, &[[4, 4], [24, 14]]));
        assert!(drag(&mut s, Tool::RedactPixelate, &[[30, 20], [60, 44]]));
        let out = s.apply_to_image(&orig).unwrap();
        let png = desk_capture::codec::encode(&out).unwrap();

        // only the critical chunks of one image: no layer, no history, no metadata
        let mut types = Vec::new();
        let mut at = 8;
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            types.push(String::from_utf8_lossy(&png[at + 4..at + 8]).into_owned());
            at += 12 + len;
        }
        assert!(
            types
                .iter()
                .all(|t| ["IHDR", "IDAT", "IEND"].contains(&t.as_str())),
            "{types:?}"
        );

        let saved = desk_capture::codec::decode(&png).unwrap();
        assert_eq!((saved.width, saved.height), (64, 48));
        let (mut solid, mut pixelated, mut pix_changed) = (0, 0, 0);
        for y in 0..48 {
            for x in 0..64 {
                let (p, o) = (saved.pixel(x, y), orig.pixel(x, y));
                if (4..24).contains(&x) && (4..14).contains(&y) {
                    assert_eq!(p, REDACT_FILL, "({x},{y})");
                    solid += 1;
                } else if (30..60).contains(&x) && (20..44).contains(&y) {
                    // one colour per block: the original detail is gone
                    let (bx, by) = (30 + (x - 30) / 12 * 12, 20 + (y - 20) / 12 * 12);
                    assert_eq!(p, saved.pixel(bx, by), "({x},{y})");
                    pixelated += 1;
                    if p != o {
                        pix_changed += 1;
                    }
                } else {
                    assert_eq!(p, o, "({x},{y}) outside the redactions");
                }
            }
        }
        assert_eq!(solid, 200);
        assert_eq!(pixelated, 720);
        assert!(pix_changed >= 700, "only {pix_changed} of 720 changed");
    }

    #[test]
    fn the_view_shows_the_drag_and_dims_outside_the_crop() {
        let base = Image::filled(20, 10, [200, 200, 200, 255]);
        let mut s = EditorState::new(20, 10, RED);
        drag(&mut s, Tool::Crop, &[[5, 0], [15, 10]]);
        let committed = s.render_committed(&base).unwrap();
        // the committed view keeps the full size: the crop is not cut while editing
        assert_eq!((committed.width, committed.height), (20, 10));
        s.tool = Tool::RedactSolid;
        s.drag_begin([6, 2]);
        s.drag_update([8, 4]);
        let view = s.render_view(&committed).unwrap();
        assert_eq!(view.pixel(6, 2), REDACT_FILL);
        assert_eq!(view.pixel(10, 8), [200, 200, 200, 255]);
        assert_eq!(view.pixel(0, 0), [100, 100, 100, 255]);
        // nothing of the drag is committed
        assert_eq!(s.annotations().len(), 1);
    }

    #[test]
    fn write_png_replaces_or_creates_and_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("capture-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.png");
        std::fs::write(&old, b"old").unwrap();
        write_png(&old, b"new").unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), b"new");
        let new = dir.join("new.png");
        write_png(&new, b"png").unwrap();
        assert_eq!(std::fs::read(&new).unwrap(), b"png");
        assert!(write_png(&dir.join("missing/x.png"), b"png").is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tool_keys_are_unique() {
        for t in Tool::ALL {
            assert_eq!(Tool::from_key(t.key()), Some(t));
            assert_eq!(Tool::from_key(t.key().to_ascii_uppercase()), Some(t));
        }
        assert_eq!(Tool::from_key('z'), None);
        assert_eq!(palette([1, 2, 3, 4])[0].rgba, [1, 2, 3, 255]);
    }
}
