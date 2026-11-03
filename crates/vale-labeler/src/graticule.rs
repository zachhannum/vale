//! The parallels of the graticule in page space, and curves that follow them.

use crate::collide::GridIndex;
use crate::geom::Measured;
use crate::line::{along, parallel};
use kurbo::{Point, Rect};

/// The parallels, with an index of their segments.
pub(crate) struct Parallels {
    lines: Vec<Measured>,
    /// The line and the first vertex of each segment.
    segs: Vec<(usize, usize)>,
    index: GridIndex,
    bounds: Rect,
}

/// A curve that follows the parallels, and the arc length on it of the point it goes through.
pub(crate) struct Through {
    pub curve: Measured,
    pub at: f64,
}

impl Parallels {
    pub fn new(paths: &[Vec<Point>], bounds: Rect) -> Self {
        let lines: Vec<Measured> = paths
            .iter()
            .filter(|p| p.iter().all(|q| q.is_finite()))
            .map(|p| Measured::new(p.clone()))
            .filter(|m| m.pts.len() >= 2)
            .collect();
        let cell = (bounds.width().max(bounds.height()) / 64.0).max(1.0);
        let mut index = GridIndex::new(bounds, cell);
        let mut segs = Vec::new();
        for (li, m) in lines.iter().enumerate() {
            for (k, w) in m.pts.windows(2).enumerate() {
                index.insert(segs.len() as u32, Rect::from_points(w[0], w[1]));
                segs.push((li, k));
            }
        }
        Parallels {
            lines,
            segs,
            index,
            bounds,
        }
    }

    /// The curve through `p` beside the nearest parallel, from west to east. With no
    /// parallels, it is a horizontal line across the bounds.
    pub fn through(&self, p: Point) -> Through {
        let Some((li, k)) = self.nearest(p) else {
            let w = self.bounds.width().max(1.0);
            let curve = Measured::new(vec![
                Point::new(self.bounds.x0.min(p.x) - w, p.y),
                Point::new(self.bounds.x1.max(p.x) + w, p.y),
            ]);
            let at = p.x - curve.pts[0].x;
            return Through { curve, at };
        };
        let m = &self.lines[li];
        let (a, b) = (m.pts[k], m.pts[k + 1]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
        let q = a + ab * t;
        let dir = ab / ab.hypot();
        // Left of the direction, y down.
        let d = (p - q).dot(kurbo::Vec2::new(dir.y, -dir.x));
        let s = m.cum[k] + t * (m.cum[k + 1] - m.cum[k]);
        let curve = parallel(m, d);
        let at = along(m, &curve, s);
        Through { curve, at }
    }

    /// The segment nearest to `p`.
    fn nearest(&self, p: Point) -> Option<(usize, usize)> {
        if self.segs.is_empty() {
            return None;
        }
        let dist = |&(li, k): &(usize, usize)| {
            let m = &self.lines[li];
            let (a, b) = (m.pts[k], m.pts[k + 1]);
            let t = ((p - a).dot(b - a) / (b - a).hypot2()).clamp(0.0, 1.0);
            p.distance_squared(a.lerp(b, t))
        };
        let reach = self.bounds.width() + self.bounds.height();
        let mut r = (reach / 256.0).max(1.0);
        let mut hits = Vec::new();
        let near = |r: f64, hits: &mut Vec<u32>| {
            self.index
                .query(Rect::new(p.x - r, p.y - r, p.x + r, p.y + r), hits);
        };
        loop {
            near(r, &mut hits);
            if !hits.is_empty() || r > 4.0 * reach {
                break;
            }
            r *= 2.0;
        }
        if hits.is_empty() {
            // Far outside the bounds: test every segment.
            hits = (0..self.segs.len() as u32).collect();
        } else {
            // A segment nearer than the best hit can lie outside the box of radius `r`.
            let best = hits
                .iter()
                .map(|&i| dist(&self.segs[i as usize]))
                .fold(f64::INFINITY, f64::min);
            near(best.sqrt().max(r), &mut hits);
        }
        hits.iter()
            .map(|&i| self.segs[i as usize])
            .min_by(|a, b| dist(a).total_cmp(&dist(b)))
    }
}

/// The curve in reading direction over the arc lengths `s0` to `s1`. The curve reverses if its
/// text would be upside down, unless `no_flip` is set. The function maps an arc length of the
/// input curve to the output curve.
pub(crate) fn upright(
    curve: Measured,
    s0: f64,
    s1: f64,
    no_flip: bool,
) -> (Measured, impl Fn(f64) -> f64) {
    let len = curve.length();
    let flip = !no_flip && curve.point_at(s1).x < curve.point_at(s0).x;
    let map = move |s: f64| if flip { len - s } else { s };
    if !flip {
        return (curve, map);
    }
    let mut pts = curve.pts;
    pts.reverse();
    (Measured::new(pts), map)
}
