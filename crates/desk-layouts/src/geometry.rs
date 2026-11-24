// SPDX-License-Identifier: GPL-3.0-or-later
//! Zone geometry resolution: portrait adaptation, outer/inner gaps, even-pixel rounding and
//! minimum-size fitting (`docs/spec/features/window-layouts.md`, AC-WIN-01 and AC-WIN-07).

use serde::{Deserialize, Serialize};

use crate::Layout;

/// Fractional tolerance for deciding whether a zone edge lies on the monitor outer boundary.
const EDGE_EPS: f64 = 1e-4;

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(self) -> i32 {
        self.x.saturating_add(self.w)
    }

    pub const fn bottom(self) -> i32 {
        self.y.saturating_add(self.h)
    }

    pub const fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "screen coordinates in logical pixels are well within i32 range"
)]
fn round_even(value: f64) -> i32 {
    ((value / 2.0).round() as i32).saturating_mul(2)
}

/// The smallest even number `>= value`.
const fn ceil_even(value: i32) -> i32 {
    value.saturating_add(value & 1)
}

/// The largest even number `<= value`.
const fn floor_even(value: i32) -> i32 {
    value & !1
}

/// The even-pixel edges of one axis: the outer box `[lo, hi]` (outer gap applied) and the inner
/// gap. One fractional seam always resolves to the same pair of edges, so two neighbouring zones
/// are separated by exactly `inner` pixels whatever the rounding.
struct Axis {
    lo: i32,
    hi: i32,
    inner: i32,
}

impl Axis {
    /// `None` when the outer gaps leave no room on this axis.
    fn new(start: i32, len: i32, outer: i32, inner: u32) -> Option<Self> {
        let outer = outer.min(len / 2);
        let lo = ceil_even(start.saturating_add(outer));
        let hi = floor_even(start.saturating_add(len).saturating_sub(outer));
        let inner = i32::try_from(inner).unwrap_or(i32::MAX);
        (hi > lo).then_some(Self {
            lo,
            hi,
            inner: ceil_even(inner).min(hi - lo),
        })
    }

    /// The end edge of the zone before the seam at fraction `f` (`f` in `0.0..=1.0`).
    fn seam_end(&self, f: f64) -> i32 {
        if f >= 1.0 - EDGE_EPS {
            return self.hi;
        }
        let seam = f64::from(self.lo) + f * f64::from(self.hi - self.lo);
        round_even(seam - f64::from(self.inner) / 2.0).clamp(self.lo, self.hi - self.inner)
    }

    /// The start edge of the zone after the seam at fraction `f`.
    fn seam_start(&self, f: f64) -> i32 {
        if f <= EDGE_EPS {
            return self.lo;
        }
        self.seam_end(f) + self.inner
    }

    /// Start and size of the span `[f0, f1]`.
    fn span(&self, f0: f64, f1: f64) -> (i32, i32) {
        let start = self.seam_start(f0);
        let end = self.seam_end(f1).max(start);
        (start, end - start)
    }
}

/// Resolve a layout's fractional zones into logical-pixel [`Rect`]s inside `work_area`.
///
/// - When `work_area.h > work_area.w` (portrait monitor), columns and rows are transposed.
/// - `gap_outer` is kept free along the edges of `work_area`; `gap_inner` (rounded up to even)
///   separates neighbouring zones exactly.
/// - Every returned rectangle has even coordinates and sizes and stays inside `work_area`.
pub fn resolve_zones(
    layout: &Layout,
    work_area: Rect,
    gap_outer: u32,
    gap_inner: u32,
) -> Vec<Rect> {
    if work_area.is_empty() {
        return Vec::new();
    }
    let outer = i32::try_from(gap_outer).unwrap_or(i32::MAX);
    let (Some(cols), Some(rows)) = (
        Axis::new(work_area.x, work_area.w, outer, gap_inner),
        Axis::new(work_area.y, work_area.h, outer, gap_inner),
    ) else {
        return Vec::new();
    };
    let portrait = work_area.h > work_area.w;

    layout
        .zones
        .iter()
        .map(|raw_zone| {
            let zone = if portrait {
                raw_zone.transpose()
            } else {
                *raw_zone
            };
            let fx0 = zone.x.clamp(0.0, 1.0);
            let fx1 = (zone.x + zone.w).clamp(fx0, 1.0);
            let fy0 = zone.y.clamp(0.0, 1.0);
            let fy1 = (zone.y + zone.h).clamp(fy0, 1.0);
            let (x, w) = cols.span(fx0, fx1);
            let (y, h) = rows.span(fy0, fy1);
            Rect::new(x, y, w, h)
        })
        .collect()
}

/// Adjust `zone` to respect a window's minimum size (`min_w`, `min_h`) while staying completely
/// inside `work_area` (`AC-WIN-07`).
pub fn fit_min_size(zone: Rect, min_w: i32, min_h: i32, work_area: Rect) -> Rect {
    if work_area.is_empty() {
        return Rect::new(work_area.x, work_area.y, 0, 0);
    }

    let w = zone.w.max(min_w).min(work_area.w).max(0);
    let h = zone.h.max(min_h).min(work_area.h).max(0);

    let max_x = work_area.right().saturating_sub(w).max(work_area.x);
    let max_y = work_area.bottom().saturating_sub(h).max(work_area.y);

    let x = zone.x.clamp(work_area.x, max_x);
    let y = zone.y.clamp(work_area.y, max_y);

    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builtin, builtins};

    const FHD: Rect = Rect::new(0, 0, 1920, 1080);

    fn assert_inside_and_even(rects: &[Rect], work_area: Rect) {
        for r in rects {
            assert_eq!(r.x % 2, 0, "x must be even: {r:?}");
            assert_eq!(r.y % 2, 0, "y must be even: {r:?}");
            assert_eq!(r.w % 2, 0, "w must be even: {r:?}");
            assert_eq!(r.h % 2, 0, "h must be even: {r:?}");
            assert!(r.x >= work_area.x, "{r:?} left of {work_area:?}");
            assert!(r.y >= work_area.y, "{r:?} above {work_area:?}");
            assert!(
                r.right() <= work_area.right(),
                "{r:?} right of {work_area:?}"
            );
            assert!(
                r.bottom() <= work_area.bottom(),
                "{r:?} below {work_area:?}"
            );
            assert!(!r.is_empty(), "zone became empty: {r:?}");
        }
    }

    /// Two zones side by side (or one above the other) are exactly `gap` pixels apart.
    fn assert_exact_inner_gaps(rects: &[Rect], gap: i32, id: &str) {
        for a in rects {
            for b in rects {
                let rows_overlap = a.y < b.bottom() && b.y < a.bottom();
                let cols_overlap = a.x < b.right() && b.x < a.right();
                if b.x >= a.right() && rows_overlap {
                    assert!(b.x - a.right() >= gap, "{id}: {a:?} {b:?}");
                    if b.x - a.right() < 2 * gap {
                        assert_eq!(b.x - a.right(), gap, "{id}: {a:?} {b:?}");
                    }
                }
                if b.y >= a.bottom() && cols_overlap {
                    assert!(b.y - a.bottom() >= gap, "{id}: {a:?} {b:?}");
                    if b.y - a.bottom() < 2 * gap {
                        assert_eq!(b.y - a.bottom(), gap, "{id}: {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn halves_with_windows11_gaps_and_zero_gaps() {
        let halves = builtin("halves").unwrap();
        // AC-WIN-01: Windows 11 defaults (outer=8, inner=6)
        let rects = resolve_zones(&halves, FHD, 8, 6);
        assert_eq!(rects.len(), 2);
        assert_inside_and_even(&rects, FHD);
        assert_eq!(rects[0].x, 8);
        assert_eq!(rects[0].y, 8);
        assert_eq!(rects[0].bottom(), 1072);
        assert_eq!(rects[1].right(), 1912);
        assert_eq!(rects[1].x - rects[0].right(), 6, "inner gap");

        // Fedora defaults (outer=0, inner=0)
        let zero = resolve_zones(&halves, FHD, 0, 0);
        assert_eq!(
            zero,
            [Rect::new(0, 0, 960, 1080), Rect::new(960, 0, 960, 1080)]
        );
    }

    #[test]
    fn all_builtins_resolve_cleanly_on_landscape_and_odd_origin() {
        // GNOME top bar height 27px gives an odd y origin and odd height
        let with_topbar = Rect::new(0, 27, 1920, 1053);
        for layout in builtins() {
            let rects = resolve_zones(&layout, FHD, 8, 6);
            assert_eq!(rects.len(), layout.zones.len());
            assert_inside_and_even(&rects, FHD);

            let topbar_rects = resolve_zones(&layout, with_topbar, 8, 6);
            assert_eq!(topbar_rects.len(), layout.zones.len());
            assert_inside_and_even(&topbar_rects, with_topbar);
            assert_eq!(
                topbar_rects[0].y, 36,
                "{}: outer gap below the top bar",
                layout.id
            );
            assert_exact_inner_gaps(&rects, 6, &layout.id);
            assert_exact_inner_gaps(&topbar_rects, 6, &layout.id);
        }
    }

    #[test]
    fn portrait_monitor_transposes_columns_to_rows() {
        let portrait = Rect::new(0, 0, 1080, 1920);
        let halves = builtin("halves").unwrap();
        let rects = resolve_zones(&halves, portrait, 8, 6);
        assert_inside_and_even(&rects, portrait);
        // Top row and bottom row span the full width (minus outer gaps)
        assert_eq!(rects[0].x, 8);
        assert_eq!(rects[0].right(), 1072);
        assert_eq!(rects[1].x, 8);
        assert_eq!(rects[1].right(), 1072);
        assert_eq!(rects[1].y - rects[0].bottom(), 6, "vertical inner gap");

        let stack = builtin("stack").unwrap();
        let s_rects = resolve_zones(&stack, portrait, 0, 0);
        assert_inside_and_even(&s_rects, portrait);
        // Main 60% on top, two 40%-tall side-by-side at the bottom
        assert_eq!(s_rects[0], Rect::new(0, 0, 1080, 1152));
        assert_eq!(s_rects[1], Rect::new(0, 1152, 540, 768));
        assert_eq!(s_rects[2], Rect::new(540, 1152, 540, 768));
    }

    #[test]
    fn empty_work_area_returns_empty() {
        let halves = builtin("halves").unwrap();
        assert!(resolve_zones(&halves, Rect::new(0, 0, 0, 1080), 8, 6).is_empty());
        assert!(resolve_zones(&halves, Rect::new(0, 0, 1920, -10), 8, 6).is_empty());
    }

    #[test]
    fn fit_min_size_respects_minimum_and_work_area_bounds() {
        let right_half = Rect::new(964, 8, 948, 1064);

        // Smaller min size leaves the zone untouched.
        assert_eq!(fit_min_size(right_half, 400, 300, FHD), right_half);

        // Wider min size shifts left so the window stays inside the work area.
        let widened = fit_min_size(right_half, 1100, 500, FHD);
        assert_eq!(widened, Rect::new(820, 8, 1100, 1064));
        assert!(widened.x >= FHD.x && widened.right() <= FHD.right());

        // Min size larger than the monitor is clamped to the work area.
        let clamped = fit_min_size(right_half, 2500, 1500, FHD);
        assert_eq!(clamped, FHD);
    }
}
