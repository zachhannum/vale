//! Hatch lines that fill polygons, and outward offsets of rings.

use kurbo::{Point, Vec2};

/// The most lines that one call of `hatch_segments` makes.
pub const MAX_HATCH_LINES: usize = 100_000;

/// The ratio of mitre length to offset distance above which a join is bevelled.
const MITRE_LIMIT: f64 = 4.0;

/// Clips parallel hatch lines to the inside of the rings, by the even-odd rule.
///
/// The lines run at `angle_deg` counterclockwise from the horizontal on the page, where y points
/// down. One line goes through `origin`, and the others are at multiples of `spacing` from it.
/// Each ring closes implicitly, and a ring inside another ring is a hole. An edge includes its
/// start or its end by the half-open rule, so a vertex on a line counts one time.
/// If `spacing` is not a positive number, the result is empty.
pub fn hatch_segments(
    rings: &[Vec<Point>],
    angle_deg: f64,
    spacing: f64,
    origin: Point,
) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    if !(spacing > 0.0 && spacing.is_finite() && angle_deg.is_finite()) {
        return out;
    }
    let a = angle_deg.to_radians();
    // The line direction, and the normal that steps from one line to the next.
    let u = Vec2::new(a.cos(), -a.sin());
    let n = Vec2::new(-u.y, u.x);
    // Each edge in line coordinates: (s, t) of its start and its end.
    let mut edges: Vec<((f64, f64), (f64, f64))> = Vec::new();
    let (mut smin, mut smax) = (f64::INFINITY, f64::NEG_INFINITY);
    for ring in rings {
        let m = ring.len();
        if m < 2 {
            continue;
        }
        for i in 0..m {
            let p = ring[i] - origin;
            let q = ring[(i + 1) % m] - origin;
            let ps = (p.dot(n), p.dot(u));
            let qs = (q.dot(n), q.dot(u));
            smin = smin.min(ps.0);
            smax = smax.max(ps.0);
            edges.push((ps, qs));
        }
    }
    if edges.is_empty() || !smin.is_finite() || !smax.is_finite() {
        return out;
    }
    let k0 = (smin / spacing).ceil();
    let k1 = (smax / spacing).floor();
    if k1 < k0 || k1 - k0 >= MAX_HATCH_LINES as f64 {
        return out;
    }
    let mut ts = Vec::new();
    let mut k = k0;
    while k <= k1 {
        let s = k * spacing;
        ts.clear();
        for &((ps, pt), (qs, qt)) in &edges {
            if (ps > s) != (qs > s) {
                ts.push(pt + (s - ps) / (qs - ps) * (qt - pt));
            }
        }
        ts.sort_by(f64::total_cmp);
        for &[t0, t1] in ts.as_chunks::<2>().0 {
            if t1 > t0 {
                let base = origin + n * s;
                out.push((base + u * t0, base + u * t1));
            }
        }
        k += 1.0;
    }
    out
}

/// Offsets a simple ring outward by `d`, with mitred joins.
///
/// The winding of the ring does not matter. A join whose mitre is longer than four times `d`
/// becomes a bevel with two points. A ring with fewer than three distinct points comes back
/// unchanged.
pub fn offset_ring(ring: &[Point], d: f64) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::with_capacity(ring.len());
    for &p in ring {
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    while pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let m = pts.len();
    if m < 3 || !d.is_finite() {
        return ring.to_vec();
    }
    let area2: f64 = (0..m)
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % m]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    // The unit normal of each edge, on the outer side.
    let normals: Vec<Vec2> = (0..m)
        .map(|i| {
            let e = (pts[(i + 1) % m] - pts[i]).normalize();
            if area2 > 0.0 {
                Vec2::new(e.y, -e.x)
            } else {
                Vec2::new(-e.y, e.x)
            }
        })
        .collect();
    let mut out = Vec::with_capacity(m);
    for i in 0..m {
        let n1 = normals[(i + m - 1) % m];
        let n2 = normals[i];
        let c = 1.0 + n1.dot(n2);
        if c * MITRE_LIMIT * MITRE_LIMIT > 2.0 {
            out.push(pts[i] + (n1 + n2) * (d / c));
        } else {
            out.push(pts[i] + n1 * d);
            out.push(pts[i] + n2 * d);
        }
    }
    out
}
