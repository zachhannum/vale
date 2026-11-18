//! Point label candidates.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::graticule::{Parallels, upright};
use crate::line::{along, parallel};
use crate::model::{CandidateKind, LabelClass, Leader, PointPlacement, PointPosition};
use crate::polygon::{follow, glyph_boxes};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect, Vec2};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

/// The cost of the largest offset, added to the rank of the position.
const OFFSET_COST: f64 = 8.0;

/// The most offsets between the preferred and the largest offset.
const MAX_OFFSET_STEPS: usize = 8;

/// The geometry of one point feature.
pub(crate) struct PointSymbol<'a> {
    pub position: Point,
    pub radius: f64,
    /// Radians, counterclockwise on the page from +x.
    pub angle: Option<f64>,
    pub outline: Option<&'a [Point]>,
}

/// One candidate per distinct listed position and offset, positions in listed order inside
/// each offset. If the class follows the graticule, "right" is the reading direction along the
/// parallel through the point, and "top" is the side of the parallel above the text. If the
/// feature has an angle, "right" is the reading direction of the turned text.
pub(crate) fn candidates(
    symbol: &PointSymbol,
    class: &LabelClass,
    pp: &PointPlacement,
    shaped: &ShapedText,
    parallels: &Parallels,
) -> Vec<Candidate> {
    let position = symbol.position;
    let w = shaped.advance;
    let h = shaped.cap_height;
    let graticule = pp.graticule && symbol.angle.is_none();
    // The positions are laid out in a frame that turns with the text.
    let frame = symbol.angle.map(|a| {
        Affine::translate(position.to_vec2()) * Affine::rotate(-upright_angle(a, pp.no_flip))
    });
    let (px, py) = if graticule || frame.is_some() {
        (0.0, 0.0)
    } else {
        (position.x, position.y)
    };
    let through = graticule.then(|| {
        let t = parallels.through(position);
        let (curve, map) = upright(t.curve, t.at - w / 2.0, t.at + w / 2.0, pp.no_flip);
        (curve, map(t.at))
    });
    // The outline in the frame, relative to the point.
    let outline: Option<Vec<Point>> = symbol.outline.filter(|_| !graticule).map(|ring| {
        let to_frame = frame.map_or(Affine::translate(-position.to_vec2()), |f| f.inverse());
        ring.iter().map(|&p| to_frame * p).collect()
    });

    let candidate = |pos: PointPosition, left: f64, top: f64, cost: f64| -> Option<Candidate> {
        let origin = Point::new(left, top + h);
        if let Some((curve, at)) = &through {
            // `origin` is the start of the baseline relative to the point, along and below
            // the curve.
            let base = parallel(curve, -origin.y);
            let start = along(curve, &base, at + origin.x);
            if start < 0.0 || start + w > base.length() {
                return None;
            }
            let transforms = follow(&base, start, shaped, 0.0).transforms;
            let (boxes, bounds) = glyph_boxes(shaped, &transforms, class.margin);
            return Some(Candidate {
                leader: None,
                kind: CandidateKind::Point(pos),
                cost,
                transforms,
                bounds,
                boxes,
            });
        }
        let to_page = frame.unwrap_or(Affine::IDENTITY);
        let transforms: Vec<Affine> = shaped
            .glyphs()
            .map(|(_, g)| {
                let t = Affine::translate((origin.x + g.x, origin.y + g.y));
                if frame.is_some() { to_page * t } else { t }
            })
            .collect();
        let ink = Rect::new(
            shaped.ink_bounds.x0 + origin.x,
            shaped.ink_bounds.y0 + origin.y,
            shaped.ink_bounds.x1 + origin.x,
            shaped.ink_bounds.y1 + origin.y,
        );
        let b = match frame {
            Some(f) => OrientedBox::from_local_rect(ink, f),
            None => OrientedBox::from_rect(ink),
        }
        .inflate(class.margin);
        Some(Candidate {
            leader: None,
            kind: CandidateKind::Point(pos),
            cost,
            transforms,
            bounds: b.aabb(),
            boxes: vec![b],
        })
    };

    let mut ranked: Vec<PointPosition> = Vec::new();
    for &pos in &pp.positions {
        if !ranked.contains(&pos) {
            ranked.push(pos);
        }
    }
    let offsets = offsets(pp, h);
    let steps = offsets.len() - 1;
    let mut out = Vec::new();
    for (k, &offset) in offsets.iter().enumerate() {
        let level = if steps == 0 {
            0.0
        } else {
            OFFSET_COST * k as f64 / steps as f64
        };
        for (rank, &pos) in ranked.iter().enumerate() {
            let cost = rank as f64 + level;
            if pos == PointPosition::Center {
                if k == 0 {
                    for (shift, extra) in center_shifts(pp.center_shift) {
                        let left = px - w / 2.0 + shift.x;
                        let top = py - h / 2.0 + shift.y;
                        out.extend(candidate(pos, left, top, cost + extra));
                    }
                }
                continue;
            }
            let d = match &outline {
                Some(ring) => outline_distance(pos, ring, offset, w, h),
                None => symbol.radius + offset,
            };
            let (left, top) = corner(pos, px, py, d, w, h);
            out.extend(candidate(pos, left, top, cost));
        }
    }
    out
}

/// The offsets to try, from the preferred to the largest.
fn offsets(pp: &PointPlacement, cap_height: f64) -> Vec<f64> {
    match pp.max_offset {
        Some(max) if max > pp.offset => {
            let extra = max - pp.offset;
            let step = (0.5 * cap_height).max(0.5);
            let n = ((extra / step).ceil() as usize).clamp(1, MAX_OFFSET_STEPS);
            (0..=n)
                .map(|k| pp.offset + extra * k as f64 / n as f64)
                .collect()
        }
        _ => vec![pp.offset],
    }
}

/// The shifts of a centered label and their costs: none, then half of `shift` and all of it in
/// eight directions.
fn center_shifts(shift: f64) -> Vec<(Vec2, f64)> {
    let mut out = vec![(Vec2::ZERO, 0.0)];
    if shift > 0.0 {
        let q = FRAC_1_SQRT_2;
        let dirs = [
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(q, -q),
            Vec2::new(-q, -q),
            Vec2::new(q, q),
            Vec2::new(-q, q),
        ];
        for f in [0.5, 1.0] {
            out.extend(dirs.iter().map(|d| (*d * (shift * f), f)));
        }
    }
    out
}

/// The top left corner of a `w` by `h` text box at position `pos`, with its nearest corner or
/// edge middle at distance `d` from the point (`px`, `py`).
fn corner(pos: PointPosition, px: f64, py: f64, d: f64, w: f64, h: f64) -> (f64, f64) {
    let q = d * FRAC_1_SQRT_2;
    match pos {
        PointPosition::TopRight => (px + q, py - q - h),
        PointPosition::TopLeft => (px - q - w, py - q - h),
        PointPosition::BottomRight => (px + q, py + q),
        PointPosition::BottomLeft => (px - q - w, py + q),
        PointPosition::Right => (px + d, py - h / 2.0),
        PointPosition::Left => (px - d - w, py - h / 2.0),
        PointPosition::Top => (px - w / 2.0, py - d - h),
        PointPosition::Bottom => (px - w / 2.0, py + d),
        PointPosition::Center => (px - w / 2.0, py - h / 2.0),
    }
}

/// The smallest distance from the point at which the text box at `pos` is at least `offset`
/// away from `ring`. The ring is relative to the point.
fn outline_distance(pos: PointPosition, ring: &[Point], offset: f64, w: f64, h: f64) -> f64 {
    let fits = |t: f64| {
        let (left, top) = corner(pos, 0.0, 0.0, t, w, h);
        separation(Rect::new(left, top, left + w, top + h), ring).is_some_and(|s| s >= offset)
    };
    if fits(0.0) {
        return 0.0;
    }
    let reach = ring.iter().map(|p| p.to_vec2().hypot()).fold(0.0, f64::max);
    let (mut lo, mut hi) = (0.0, reach + offset + w + h);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// The angle in (-π/2, π/2], so that the text reads from left to right, unless `no_flip`.
pub(crate) fn upright_angle(a: f64, no_flip: bool) -> f64 {
    let wrap = |a: f64| {
        let a = a.rem_euclid(TAU);
        if a > PI { a - TAU } else { a }
    };
    let a = wrap(a);
    if !no_flip && a.abs() > FRAC_PI_2 {
        wrap(a - PI)
    } else {
        a
    }
}

/// The distance between the box and the closed `ring`, or 0 if they overlap.
pub(crate) fn outline_gap(b: &OrientedBox, ring: &[Point]) -> f64 {
    let to_local = Affine::rotate(-b.angle) * Affine::translate(-b.center.to_vec2());
    let local: Vec<Point> = ring.iter().map(|&p| to_local * p).collect();
    separation(Rect::new(-b.half.x, -b.half.y, b.half.x, b.half.y), &local).unwrap_or(0.0)
}

/// The distance between `rect` and the closed `ring`, or `None` if they overlap.
fn separation(rect: Rect, ring: &[Point]) -> Option<f64> {
    let corners = [
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x1, rect.y1),
        Point::new(rect.x0, rect.y1),
    ];
    let mut best = f64::INFINITY;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        if segment_hits_rect(a, b, rect) {
            return None;
        }
        best = best.min(rect_distance(a, rect));
        for c in corners {
            best = best.min(segment_distance(c, a, b));
        }
    }
    if contains(ring, rect.center()) {
        None
    } else {
        Some(best)
    }
}

/// Even-odd test.
fn contains(ring: &[Point], p: Point) -> bool {
    let mut inside = false;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

fn segment_hits_rect(a: Point, b: Point, r: Rect) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, dd, lo, hi) in [(a.x, d.x, r.x0, r.x1), (a.y, d.y, r.y0, r.y1)] {
        if dd.abs() < 1e-15 {
            if p < lo || p > hi {
                return false;
            }
        } else {
            let (mut ta, mut tb) = ((lo - p) / dd, (hi - p) / dd);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn rect_distance(p: Point, r: Rect) -> f64 {
    let dx = (r.x0 - p.x).max(p.x - r.x1).max(0.0);
    let dy = (r.y0 - p.y).max(p.y - r.y1).max(0.0);
    dx.hypot(dy)
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let d = b - a;
    let len2 = d.hypot2();
    let t = if len2 > 0.0 {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + d * t)).hypot()
}

/// A leader line from the edge of the symbol to the nearest point of the ink `rect`.
pub(crate) fn leader(position: Point, symbol_radius: f64, rect: Rect) -> Leader {
    let end = Point::new(
        position.x.clamp(rect.x0, rect.x1),
        position.y.clamp(rect.y0, rect.y1),
    );
    let d = end - position;
    let anchor = if d.hypot() > symbol_radius && symbol_radius > 0.0 {
        position + d * (symbol_radius / d.hypot())
    } else {
        position
    };
    Leader { anchor, end }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_angles_read_from_left_to_right() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(close(upright_angle(0.5, false), 0.5));
        assert!(close(upright_angle(FRAC_PI_2, false), FRAC_PI_2));
        assert!(close(upright_angle(2.5, false), 2.5 - PI));
        assert!(close(upright_angle(-2.5, false), PI - 2.5));
        assert!(close(upright_angle(2.5, true), 2.5));
        assert!(close(upright_angle(TAU + 0.5, false), 0.5));
    }

    #[test]
    fn separation_measures_from_the_ring() {
        let square = [
            Point::new(-2.0, -2.0),
            Point::new(2.0, -2.0),
            Point::new(2.0, 2.0),
            Point::new(-2.0, 2.0),
        ];
        let s = separation(Rect::new(5.0, -1.0, 9.0, 1.0), &square).unwrap();
        assert!((s - 3.0).abs() < 1e-12);
        assert!(separation(Rect::new(1.0, -1.0, 9.0, 1.0), &square).is_none());
        assert!(separation(Rect::new(-1.0, -1.0, 1.0, 1.0), &square).is_none());
    }
}
