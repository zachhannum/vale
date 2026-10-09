//! Densify, unwrap, and planar clipping.

use crate::frame::{LonLat, wrap180};

/// A small margin inside the longitude cut.
pub const EPS: f64 = 1e-7;

/// A clip rectangle in frame degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClipRect {
    pub lon_min: f64,
    pub lon_max: f64,
    pub lat_min: f64,
    pub lat_max: f64,
}

/// Adds points so that no edge is longer than `step` degrees. Linear in the two coordinates.
pub fn densify(pts: &[LonLat], step: f64, closed: bool) -> Vec<LonLat> {
    let mut out = Vec::new();
    let n = pts.len();
    if n == 0 {
        return out;
    }
    let edges = if closed { n } else { n - 1 };
    for i in 0..edges {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let k = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()) / step)
            .ceil()
            .max(1.0) as usize;
        for j in 0..k {
            let t = j as f64 / k as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

/// Removes the jumps of 360 degrees from the longitudes of frame points.
pub fn unwrap(pts: &[LonLat]) -> Vec<LonLat> {
    let mut out: Vec<LonLat> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        if i == 0 {
            out.push(*p);
        } else {
            let prev = out[i - 1];
            out.push([prev[0] + wrap180(p[0] - pts[i - 1][0]), p[1]]);
        }
    }
    out
}

/// The shifts (multiples of 360) that bring a part of `pts` into -180..180.
pub fn shifts(pts: &[LonLat]) -> Vec<f64> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lo = lo.min(p[0]);
        hi = hi.max(p[0]);
    }
    let k0 = ((lo + 180.0) / 360.0).floor() as i64;
    let k1 = ((hi + 180.0) / 360.0).floor() as i64;
    (k0..=k1).map(|k| -360.0 * k as f64).collect()
}

/// Sutherland and Hodgman clip of a closed ring against the rectangle.
pub fn clip_ring_rect(ring: &[LonLat], r: &ClipRect) -> Vec<LonLat> {
    let mut cur = ring.to_vec();
    for (axis, bound, keep_greater) in [
        (0, r.lon_min, true),
        (0, r.lon_max, false),
        (1, r.lat_min, true),
        (1, r.lat_max, false),
    ] {
        let inside = |p: &LonLat| {
            if keep_greater {
                p[axis] >= bound
            } else {
                p[axis] <= bound
            }
        };
        let mut out = Vec::with_capacity(cur.len() + 4);
        let n = cur.len();
        for i in 0..n {
            let (a, b) = (cur[i], cur[(i + 1) % n]);
            let (ia, ib) = (inside(&a), inside(&b));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let t = (bound - a[axis]) / (b[axis] - a[axis]);
                let mut x = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                x[axis] = bound;
                out.push(x);
            }
        }
        cur = out;
        if cur.len() < 3 {
            return Vec::new();
        }
    }
    cur
}

/// Liang and Barsky clip of an open line against the rectangle. Returns the pieces inside.
pub fn clip_line_rect(line: &[LonLat], r: &ClipRect) -> Vec<Vec<LonLat>> {
    fn flush(cur: &mut Vec<LonLat>, out: &mut Vec<Vec<LonLat>>) {
        if cur.len() >= 2 {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    }
    let mut out: Vec<Vec<LonLat>> = Vec::new();
    let mut cur: Vec<LonLat> = Vec::new();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut ok = true;
        for (p, q) in [
            (-d[0], a[0] - r.lon_min),
            (d[0], r.lon_max - a[0]),
            (-d[1], a[1] - r.lat_min),
            (d[1], r.lat_max - a[1]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    ok = false;
                    break;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    if t > t1 {
                        ok = false;
                        break;
                    }
                    if t > t0 {
                        t0 = t;
                    }
                } else {
                    if t < t0 {
                        ok = false;
                        break;
                    }
                    if t < t1 {
                        t1 = t;
                    }
                }
            }
        }
        if !ok {
            flush(&mut cur, &mut out);
            continue;
        }
        let pa = [a[0] + d[0] * t0, a[1] + d[1] * t0];
        let pb = [a[0] + d[0] * t1, a[1] + d[1] * t1];
        if t0 > 0.0 || cur.is_empty() {
            flush(&mut cur, &mut out);
            cur.push(pa);
        }
        cur.push(pb);
        if t1 < 1.0 {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: ClipRect = ClipRect {
        lon_min: 0.0,
        lon_max: 10.0,
        lat_min: 0.0,
        lat_max: 10.0,
    };

    fn area(r: &[LonLat]) -> f64 {
        let n = r.len();
        (0..n)
            .map(|i| r[i][0] * r[(i + 1) % n][1] - r[(i + 1) % n][0] * r[i][1])
            .sum::<f64>()
            .abs()
            / 2.0
    }

    fn sq(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<LonLat> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn densify_counts() {
        assert_eq!(densify(&[[0.0, 0.0], [10.0, 0.0]], 1.0, false).len(), 11);
        let tri = [[0.0, 0.0], [0.5, 0.0], [0.0, 0.5]];
        assert_eq!(densify(&tri, 1.0, true).len(), 3);
    }

    #[test]
    fn unwrap_jumps() {
        let u = unwrap(&[[170.0, 0.0], [179.0, 0.0], [-179.0, 0.0], [-170.0, 0.0]]);
        let lons: Vec<f64> = u.iter().map(|p| p[0]).collect();
        assert_eq!(lons, vec![170.0, 179.0, 181.0, 190.0]);
    }

    #[test]
    fn shifts_of_straddle() {
        let s = shifts(&[[170.0, 0.0], [190.0, 0.0]]);
        assert_eq!(s, vec![0.0, -360.0]);
    }

    #[test]
    fn ring_clip() {
        assert_eq!(clip_ring_rect(&sq(2.0, 2.0, 4.0, 4.0), &R).len(), 4);
        let half = clip_ring_rect(&sq(5.0, 2.0, 15.0, 4.0), &R);
        assert!((area(&half) - 10.0).abs() < 1e-9);
        assert!(clip_ring_rect(&sq(20.0, 20.0, 30.0, 30.0), &R).is_empty());
    }

    #[test]
    fn line_clip() {
        let inside = vec![[1.0, 1.0], [5.0, 5.0]];
        assert_eq!(clip_line_rect(&inside, &R), vec![inside.clone()]);
        let through = clip_line_rect(&[[-5.0, 5.0], [15.0, 5.0]], &R);
        assert_eq!(through.len(), 1);
        assert_eq!(through[0], vec![[0.0, 5.0], [10.0, 5.0]]);
        let two = clip_line_rect(&[[5.0, 5.0], [15.0, 5.0], [15.0, 8.0], [5.0, 8.0]], &R);
        assert_eq!(two.len(), 2);
        assert!(clip_line_rect(&[[20.0, 20.0], [30.0, 30.0]], &R).is_empty());
    }
}
