//! Stroke geometry for dashes, tapers, and brushes.
//!
//! All functions take the parts of one feature in page space. A feature can have several
//! parts, and the arc length runs on from the end of one part to the start of the next.

use kurbo::{BezPath, Circle, Point, Rect, Shape, Vec2};

/// The most dabs that one feature gets from `brush_dabs`.
pub const MAX_DABS: usize = 20_000;

/// Cuts the parts into dashes of length `dash` with gaps of length `gap`, along the arc length.
///
/// A dash includes the vertices that it crosses. The phase runs on across vertices and parts.
/// If `dash` or `gap` is not a positive number, the parts come back unchanged.
pub fn dash(parts: &[Vec<Point>], dash: f64, gap: f64) -> Vec<Vec<Point>> {
    let all = Rect::new(
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::INFINITY,
    );
    dash_within(parts, dash, gap, all)
}

/// The dashes of `dash` that are inside `area`. A dash that crosses the edge of `area` stops at
/// the edge. The phase is the phase of the whole parts, so a dash keeps its place when `area`
/// moves. The work is in proportion to the vertices and to the dashes inside `area`.
pub fn dash_within(parts: &[Vec<Point>], dash: f64, gap: f64, area: Rect) -> Vec<Vec<Point>> {
    if !(dash > 0.0 && gap > 0.0 && dash.is_finite() && gap.is_finite()) {
        return parts.to_vec();
    }
    let mut d = Dasher {
        dash,
        gap,
        on: true,
        left: dash,
        cur: Vec::new(),
        out: Vec::new(),
    };
    for part in parts {
        if part.len() < 2 {
            continue;
        }
        d.cur = if d.on { vec![part[0]] } else { Vec::new() };
        for w in part.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            let Some((t0, t1)) = clip(a, b, area) else {
                d.skip(len, b);
                continue;
            };
            let (p0, p1) = (a.lerp(b, t0), a.lerp(b, t1));
            if t0 > 0.0 {
                d.skip(len * t0, p0);
            }
            d.walk(p0, p1);
            if t1 < 1.0 {
                d.skip(len * (1.0 - t1), b);
            }
        }
        d.flush();
    }
    d.out
}

/// The state of `dash_within` along the parts.
struct Dasher {
    dash: f64,
    gap: f64,
    /// The pen is in a dash.
    on: bool,
    /// The length to the end of the dash or the gap.
    left: f64,
    cur: Vec<Point>,
    out: Vec<Vec<Point>>,
}

impl Dasher {
    /// Cuts the segment `a`-`b` into dashes.
    fn walk(&mut self, a: Point, b: Point) {
        let len = a.distance(b);
        let mut t = 0.0;
        while len > 0.0 && len - t >= self.left {
            t += self.left;
            let p = a.lerp(b, t / len);
            if self.on {
                push_new(&mut self.cur, p);
                self.flush();
            } else {
                self.cur = vec![p];
            }
            self.on = !self.on;
            self.left = if self.on { self.dash } else { self.gap };
        }
        self.left -= len - t;
        if self.on {
            push_new(&mut self.cur, b);
        }
    }

    /// Moves the phase on by `length` with no dashes, to the point `to`.
    fn skip(&mut self, length: f64, to: Point) {
        self.flush();
        let cycle = self.dash + self.gap;
        let at = if self.on {
            self.dash - self.left
        } else {
            cycle - self.left
        };
        let at = (at + length).rem_euclid(cycle);
        self.on = at < self.dash;
        self.left = if self.on { self.dash - at } else { cycle - at };
        self.cur = if self.on { vec![to] } else { Vec::new() };
    }

    /// Ends the dash at the last point.
    fn flush(&mut self) {
        let cur = std::mem::take(&mut self.cur);
        if self.on && cur.len() >= 2 {
            self.out.push(cur);
        }
    }
}

/// The part of the segment `a`-`b` inside `area`, as the parameters of its ends, by the
/// Liang-Barsky method.
fn clip(a: Point, b: Point, area: Rect) -> Option<(f64, f64)> {
    let inside =
        |p: Point| (area.x0..=area.x1).contains(&p.x) && (area.y0..=area.y1).contains(&p.y);
    if inside(a) && inside(b) {
        return Some((0.0, 1.0));
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-d.x, a.x - area.x0),
        (d.x, area.x1 - a.x),
        (-d.y, a.y - area.y0),
        (d.y, area.y1 - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 < t1).then_some((t0, t1))
}

fn push_new(v: &mut Vec<Point>, p: Point) {
    if v.last() != Some(&p) {
        v.push(p);
    }
}

/// One piece of a tapered stroke. The union of all pieces is the stroke.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TaperPiece {
    /// A center and a radius.
    Circle(Point, f64),
    /// The hull between two circles, in order around the outline.
    Quad([Point; 4]),
}

/// A stroke whose width changes linearly with the arc length over all parts, from `start_w` at
/// the first vertex to `end_w` at the last.
pub fn taper(parts: &[Vec<Point>], start_w: f64, end_w: f64) -> Vec<TaperPiece> {
    let total: f64 = parts.iter().map(|p| polyline_length(p)).sum();
    let mut s = 0.0;
    let measured: Vec<Vec<(Point, f64)>> = parts
        .iter()
        .map(|part| {
            part.iter()
                .enumerate()
                .map(|(i, &p)| {
                    if i > 0 {
                        s += part[i - 1].distance(p);
                    }
                    (p, if total > 0.0 { s / total } else { 0.0 })
                })
                .collect()
        })
        .collect();
    taper_measured(&measured, start_w, end_w)
}

/// Like `taper`, but each point carries its fraction from 0 to 1 along the whole line.
pub fn taper_measured(parts: &[Vec<(Point, f64)>], start_w: f64, end_w: f64) -> Vec<TaperPiece> {
    let radius = |f: f64| ((start_w + (end_w - start_w) * f) * 0.5).max(0.0);
    let mut out = Vec::new();
    for part in parts {
        for (i, &(p, f)) in part.iter().enumerate() {
            let r = radius(f);
            if i > 0 {
                let (q, g) = part[i - 1];
                if q == p {
                    continue;
                }
                if let Some(quad) = hull(q, radius(g), p, r) {
                    out.push(TaperPiece::Quad(quad));
                }
            }
            out.push(TaperPiece::Circle(p, r));
        }
    }
    out
}

/// The outer-tangent hull of two circles, or `None` if one circle contains the other.
fn hull(c1: Point, r1: f64, c2: Point, r2: f64) -> Option<[Point; 4]> {
    let d = c1.distance(c2);
    if d <= (r1 - r2).abs() {
        return None;
    }
    let u = (c2 - c1) / d;
    let n = Vec2::new(-u.y, u.x);
    let a = (r1 - r2) / d;
    let b = (1.0 - a * a).max(0.0).sqrt();
    let (vp, vm) = (a * u + b * n, a * u - b * n);
    Some([c1 + r1 * vp, c2 + r2 * vp, c2 + r2 * vm, c1 + r1 * vm])
}

/// The outline of a tapered stroke. Fill it with the nonzero rule.
pub fn taper_path(pieces: &[TaperPiece]) -> BezPath {
    let circle_sign = Circle::new((0.0, 0.0), 1.0).to_path(0.1).area().signum();
    let mut out = BezPath::new();
    for piece in pieces {
        match *piece {
            TaperPiece::Circle(c, r) => {
                if r > 0.0 {
                    let tolerance = (r * 0.01).clamp(1e-4, 0.1);
                    out.extend(Circle::new(c, r).path_elements(tolerance));
                }
            }
            TaperPiece::Quad(mut q) => {
                let area: f64 = (0..4)
                    .map(|i| {
                        let (a, b) = (q[i], q[(i + 1) % 4]);
                        a.x * b.y - b.x * a.y
                    })
                    .sum();
                if area.signum() != circle_sign {
                    q.reverse();
                }
                out.move_to(q[0]);
                for &p in &q[1..] {
                    out.line_to(p);
                }
                out.close_path();
            }
        }
    }
    out
}

/// One dab of a brush stroke. `alpha` is relative, and the caller multiplies it with the
/// alpha of the color.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Dab {
    pub center: Point,
    pub radius: f64,
    pub alpha: f32,
}

/// The dabs of a brush stroke of `width`. The same input always gives the same dabs.
pub fn brush_dabs(parts: &[Vec<Point>], width: f64, seed: u64) -> Vec<Dab> {
    if !(width > 0.0 && width.is_finite()) {
        return Vec::new();
    }
    let total: f64 = parts.iter().map(|p| polyline_length(p)).sum();
    let stations_max = (MAX_DABS / 3) as f64;
    let spacing = (0.3 * width).max(0.25).max(total / (stations_max - 1.0));
    let mut out = Vec::new();
    let mut station = 0u64;
    let mut next = 0.0;
    let mut pos = 0.0;
    for part in parts {
        for w in part.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            if len <= 0.0 {
                continue;
            }
            let dir = (b - a) / len;
            let normal = Vec2::new(-dir.y, dir.x);
            while next <= pos + len && out.len() + 3 <= MAX_DABS {
                let p = a + dir * (next - pos);
                for k in 0..3u64 {
                    let mut h = splitmix64(seed ^ splitmix64(station.wrapping_mul(3) + k));
                    let mut unit = || {
                        h = splitmix64(h);
                        (h >> 11) as f64 / (1u64 << 53) as f64
                    };
                    let offset = (unit() - 0.5) * 0.7 * width;
                    let radius = width * (0.18 + 0.12 * unit());
                    let alpha = (0.35 + 0.4 * unit()) as f32;
                    out.push(Dab {
                        center: p + normal * offset,
                        radius,
                        alpha,
                    });
                }
                station += 1;
                next += spacing;
            }
            pos += len;
        }
    }
    out
}

fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn polyline_length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}
