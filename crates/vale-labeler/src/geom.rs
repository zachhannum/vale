//! Polyline helpers.

use kurbo::{Point, Rect};
use std::f64::consts::PI;

const JOIN_EPS: f64 = 1e-9;

/// Clip a polyline to `rect`. Returns the pieces inside, each with at least two points.
pub(crate) fn clip_polyline_to_rect(path: &[Point], rect: Rect) -> Vec<Vec<Point>> {
    let mut pieces: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    for w in path.windows(2) {
        match clip_segment(w[0], w[1], rect) {
            Some((a, b)) => {
                let joins = cur
                    .last()
                    .is_some_and(|l| (l.x - a.x).abs() < JOIN_EPS && (l.y - a.y).abs() < JOIN_EPS);
                if !joins {
                    if cur.len() >= 2 {
                        pieces.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(a);
                }
                cur.push(b);
            }
            None => {
                if cur.len() >= 2 {
                    pieces.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        pieces.push(cur);
    }
    pieces
}

/// Liang and Barsky clipping of one segment.
fn clip_segment(a: Point, b: Point, r: Rect) -> Option<(Point, Point)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mut t0 = 0.0_f64;
    let mut t1 = 1.0_f64;
    for (p, q) in [
        (-dx, a.x - r.x0),
        (dx, r.x1 - a.x),
        (-dy, a.y - r.y0),
        (dy, r.y1 - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return None;
                }
                t1 = t1.min(t);
            }
        }
    }
    let pa = if t0 == 0.0 {
        a
    } else {
        Point::new(a.x + t0 * dx, a.y + t0 * dy)
    };
    let pb = if t1 == 1.0 {
        b
    } else {
        Point::new(a.x + t1 * dx, a.y + t1 * dy)
    };
    Some((pa, pb))
}

/// Points at arc lengths 0, step, 2*step, ... plus the exact last point.
pub(crate) fn resample(path: &[Point], step: f64) -> Vec<Point> {
    if path.len() < 2 || !(step > 0.0) || !step.is_finite() {
        return Vec::new();
    }
    let m = Measured::new(path.to_vec());
    let len = m.length();
    if m.pts.len() < 2 || len <= 0.0 {
        return Vec::new();
    }
    let n = (len / step).floor() as usize;
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..=n {
        let s = i as f64 * step;
        if s >= len {
            break;
        }
        out.push(m.point_at(s));
    }
    out.push(*m.pts.last().unwrap());
    out
}

/// Resample, then smooth with two passes of a moving mean about `window` long.
pub(crate) fn smooth(path: &[Point], step: f64, window: f64) -> Vec<Point> {
    let mut cur = resample(path, step);
    let n = cur.len();
    if n < 3 {
        return cur;
    }
    let k = ((window / step / 2.0).round().max(1.0)) as usize;
    for _ in 0..2 {
        let src = cur.clone();
        for i in 1..n - 1 {
            let j = k.min(i).min(n - 1 - i);
            let (mut sx, mut sy) = (0.0, 0.0);
            for p in &src[i - j..=i + j] {
                sx += p.x;
                sy += p.y;
            }
            let c = (2 * j + 1) as f64;
            cur[i] = Point::new(sx / c, sy / c);
        }
    }
    cur
}

/// A polyline with cumulative arc lengths.
pub(crate) struct Measured {
    pub pts: Vec<Point>,
    pub cum: Vec<f64>,
}

impl Measured {
    /// Build from points; repeated consecutive points are dropped.
    pub fn new(pts: Vec<Point>) -> Self {
        let mut out: Vec<Point> = Vec::with_capacity(pts.len());
        let mut cum: Vec<f64> = Vec::with_capacity(pts.len());
        for p in pts {
            match out.last() {
                Some(l) if l.x == p.x && l.y == p.y => {}
                Some(l) => {
                    let d = cum.last().copied().unwrap_or(0.0) + l.distance(p);
                    out.push(p);
                    cum.push(d);
                }
                None => {
                    out.push(p);
                    cum.push(0.0);
                }
            }
        }
        Measured { pts: out, cum }
    }

    pub fn length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Point at arc length `s`, clamped to the path.
    pub fn point_at(&self, s: f64) -> Point {
        let n = self.pts.len();
        if n == 0 {
            return Point::ZERO;
        }
        if n == 1 || s.is_nan() || s <= 0.0 {
            return self.pts[0];
        }
        if s >= self.length() {
            return self.pts[n - 1];
        }
        // First index with cum > s; the segment is i-1..i.
        let i = self.cum.partition_point(|&c| c <= s).clamp(1, n - 1);
        let seg = self.cum[i] - self.cum[i - 1];
        let t = if seg > 0.0 {
            (s - self.cum[i - 1]) / seg
        } else {
            0.0
        };
        self.pts[i - 1].lerp(self.pts[i], t)
    }
}

/// Wrap an angle into (-PI, PI].
pub(crate) fn wrap_angle(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    let mut r = (a + PI).rem_euclid(2.0 * PI) - PI;
    if r <= -PI {
        r += 2.0 * PI;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn plen(v: &[Point]) -> f64 {
        v.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    #[test]
    fn clip_inside_and_outside() {
        let r = Rect::new(0., 0., 10., 10.);
        let line = vec![p(1., 1.), p(5., 5.), p(9., 2.)];
        assert_eq!(clip_polyline_to_rect(&line, r), vec![line.clone()]);
        assert!(clip_polyline_to_rect(&[p(20., 0.), p(30., 5.)], r).is_empty());
        assert!(clip_polyline_to_rect(&[], r).is_empty());
        assert!(clip_polyline_to_rect(&[p(1., 1.)], r).is_empty());
    }

    #[test]
    fn clip_crossing() {
        let r = Rect::new(0., 0., 10., 10.);
        let out = clip_polyline_to_rect(&[p(-5., 5.), p(15., 5.)], r);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], vec![p(0., 5.), p(10., 5.)]);
    }

    #[test]
    fn clip_zigzag() {
        let r = Rect::new(0., 0., 10., 10.);
        let line = vec![
            p(5., 5.),
            p(5., 15.),
            p(15., 15.),
            p(15., 5.),
            p(5., 5.),
            p(5., -5.),
            p(8., -5.),
            p(8., 5.),
        ];
        let out = clip_polyline_to_rect(&line, r);
        // Inside segments: start->exit, then re-entry ... check count by hand:
        // piece 1: (5,5)-(5,10); piece 2: (10,5)-(5,5)-(5,0); piece 3: (8,0)-(8,5).
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].len(), 3);
    }

    #[test]
    fn resample_line() {
        let r = resample(&[p(0., 0.), p(10., 0.)], 2.5);
        assert_eq!(r.len(), 5);
        assert!((plen(&r) - 10.0).abs() < 1e-9);
        let short = resample(&[p(0., 0.), p(1., 0.)], 5.0);
        assert_eq!(short, vec![p(0., 0.), p(1., 0.)]);
        assert!(resample(&[], 1.0).is_empty());
        assert!(resample(&[p(1., 1.)], 1.0).is_empty());
    }

    #[test]
    fn smooth_line_and_corner() {
        let s = smooth(&[p(0., 0.), p(20., 0.)], 1.0, 4.0);
        assert!(s.iter().all(|q| q.y.abs() < 1e-9));
        assert_eq!(s[0], p(0., 0.));
        assert_eq!(*s.last().unwrap(), p(20., 0.));

        let c = smooth(&[p(0., 0.), p(10., 0.), p(10., 10.)], 1.0, 4.0);
        assert_eq!(c[0], p(0., 0.));
        assert_eq!(*c.last().unwrap(), p(10., 10.));
        let corner = c
            .iter()
            .map(|q| q.distance(p(10., 0.)))
            .fold(f64::INFINITY, f64::min);
        assert!(corner > 0.1, "corner point should move inward");
        // No sharper turn than the original 90 degrees.
        for w in c.windows(3) {
            let a = (w[1] - w[0]).atan2();
            let b = (w[2] - w[1]).atan2();
            assert!(wrap_angle(b - a).abs() <= std::f64::consts::FRAC_PI_2 + 1e-9);
        }
    }

    #[test]
    fn measured() {
        let m = Measured::new(vec![p(0., 0.), p(0., 0.), p(10., 0.), p(10., 10.)]);
        assert_eq!(m.pts.len(), 3);
        assert_eq!(m.length(), 20.0);
        assert_eq!(m.point_at(0.0), p(0., 0.));
        assert_eq!(m.point_at(10.0), p(10., 0.));
        assert_eq!(m.point_at(15.0), p(10., 5.));
        assert_eq!(m.point_at(20.0), p(10., 10.));
        assert_eq!(m.point_at(-3.0), p(0., 0.));
        assert_eq!(m.point_at(99.0), p(10., 10.));
        assert_eq!(Measured::new(vec![]).point_at(1.0), Point::ZERO);
    }

    #[test]
    fn wrap() {
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(-PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-12);
    }
}
