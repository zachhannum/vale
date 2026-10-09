//! Collision boxes and spatial index.

use kurbo::{Affine, Point, Rect, Vec2};

/// A rectangle with a center, half extents, and a rotation.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OrientedBox {
    /// Center of the box in page space.
    pub center: Point,
    /// Half width and half height, before rotation.
    pub half: Vec2,
    /// Rotation in radians; positive turns from +x toward +y.
    pub angle: f64,
}

const EPS: f64 = 1e-9;

impl OrientedBox {
    /// An unrotated box covering `rect`.
    pub fn from_rect(rect: Rect) -> Self {
        OrientedBox {
            center: rect.center(),
            half: Vec2::new(rect.width() * 0.5, rect.height() * 0.5),
            angle: 0.0,
        }
    }

    /// A box covering `rect` after `transform`, which must be rotation plus translation.
    pub fn from_local_rect(rect: Rect, transform: Affine) -> Self {
        let c = transform.as_coeffs();
        OrientedBox {
            center: transform * rect.center(),
            half: Vec2::new(rect.width() * 0.5, rect.height() * 0.5),
            angle: c[1].atan2(c[0]),
        }
    }

    fn axes(&self) -> (Vec2, Vec2) {
        let (s, c) = self.angle.sin_cos();
        (Vec2::new(c, s), Vec2::new(-s, c))
    }

    /// The four corners in order around the box.
    pub fn corners(&self) -> [Point; 4] {
        let (u, v) = self.axes();
        let a = u * self.half.x;
        let b = v * self.half.y;
        [
            self.center - a - b,
            self.center + a - b,
            self.center + a + b,
            self.center - a + b,
        ]
    }

    /// The smallest axis-aligned rectangle that holds the box.
    pub fn aabb(&self) -> Rect {
        let (u, v) = self.axes();
        let ex = u.x.abs() * self.half.x + v.x.abs() * self.half.y;
        let ey = u.y.abs() * self.half.x + v.y.abs() * self.half.y;
        Rect::new(
            self.center.x - ex,
            self.center.y - ey,
            self.center.x + ex,
            self.center.y + ey,
        )
    }

    /// Grow both half extents by `margin`.
    pub fn inflate(&self, margin: f64) -> Self {
        OrientedBox {
            center: self.center,
            half: Vec2::new(self.half.x + margin, self.half.y + margin),
            angle: self.angle,
        }
    }

    /// Separating axis test. Boxes that only touch do not intersect.
    pub fn intersects(&self, other: &OrientedBox) -> bool {
        let ca = self.corners();
        let cb = other.corners();
        let (a0, a1) = self.axes();
        let (b0, b1) = other.axes();
        for axis in [a0, a1, b0, b1] {
            let (min_a, max_a) = project(&ca, axis);
            let (min_b, max_b) = project(&cb, axis);
            if max_a.min(max_b) - min_a.max(min_b) <= EPS {
                return false;
            }
        }
        true
    }

    /// True when the segment `a`-`b` touches the box.
    pub fn intersects_segment(&self, a: Point, b: Point) -> bool {
        let (s, c) = (-self.angle).sin_cos();
        let to_local = |p: Point| {
            let d = p - self.center;
            Point::new(d.x * c - d.y * s, d.x * s + d.y * c)
        };
        let p = to_local(a);
        let q = to_local(b);
        let d = q - p;
        let mut t0 = 0.0_f64;
        let mut t1 = 1.0_f64;
        for (p0, dd, h) in [(p.x, d.x, self.half.x), (p.y, d.y, self.half.y)] {
            if dd.abs() < 1e-15 {
                if p0.abs() > h {
                    return false;
                }
            } else {
                let mut ta = (-h - p0) / dd;
                let mut tb = (h - p0) / dd;
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

    /// True when all four corners are inside `bounds`; edges count as inside.
    pub fn is_inside(&self, bounds: Rect) -> bool {
        self.corners()
            .iter()
            .all(|p| p.x >= bounds.x0 && p.x <= bounds.x1 && p.y >= bounds.y0 && p.y <= bounds.y1)
    }
}

fn project(corners: &[Point; 4], axis: Vec2) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in corners {
        let d = p.x * axis.x + p.y * axis.y;
        lo = lo.min(d);
        hi = hi.max(d);
    }
    (lo, hi)
}

/// Uniform grid over axis-aligned bounds. Deterministic: results come back sorted and without duplicates.
pub(crate) struct GridIndex {
    origin: Point,
    cell: f64,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<(u32, Rect)>>,
}

impl GridIndex {
    pub fn new(bounds: Rect, cell: f64) -> Self {
        let cell = if cell.is_finite() && cell > 0.0 {
            cell
        } else {
            1.0
        };
        let cols = ((bounds.width() / cell).ceil().max(1.0)) as usize;
        let rows = ((bounds.height() / cell).ceil().max(1.0)) as usize;
        GridIndex {
            origin: Point::new(bounds.x0, bounds.y0),
            cell,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
        }
    }

    fn range(&self, r: Rect) -> (usize, usize, usize, usize) {
        let f = |v: f64, o: f64, n: usize| -> usize {
            let i = ((v - o) / self.cell).floor();
            if i.is_nan() || i < 0.0 {
                0
            } else {
                (i as usize).min(n - 1)
            }
        };
        (
            f(r.x0.min(r.x1), self.origin.x, self.cols),
            f(r.x0.max(r.x1), self.origin.x, self.cols),
            f(r.y0.min(r.y1), self.origin.y, self.rows),
            f(r.y0.max(r.y1), self.origin.y, self.rows),
        )
    }

    pub fn insert(&mut self, id: u32, aabb: Rect) {
        let (c0, c1, r0, r1) = self.range(aabb);
        for r in r0..=r1 {
            for c in c0..=c1 {
                self.cells[r * self.cols + c].push((id, aabb));
            }
        }
    }

    /// Remove one entry; pass the same `aabb` as at insert.
    pub fn remove(&mut self, id: u32, aabb: Rect) {
        let (c0, c1, r0, r1) = self.range(aabb);
        for r in r0..=r1 {
            for c in c0..=c1 {
                let cell = &mut self.cells[r * self.cols + c];
                if let Some(i) = cell.iter().position(|e| e.0 == id && e.1 == aabb) {
                    cell.swap_remove(i);
                }
            }
        }
    }

    /// Ids whose rectangle overlaps `aabb`. Clears `out`; result is sorted and unique.
    pub fn query(&self, aabb: Rect, out: &mut Vec<u32>) {
        out.clear();
        let (c0, c1, r0, r1) = self.range(aabb);
        let q = Rect::new(
            aabb.x0.min(aabb.x1),
            aabb.y0.min(aabb.y1),
            aabb.x0.max(aabb.x1),
            aabb.y0.max(aabb.y1),
        );
        for r in r0..=r1 {
            for c in c0..=c1 {
                for &(id, b) in &self.cells[r * self.cols + c] {
                    if b.x0 <= q.x1 && b.x1 >= q.x0 && b.y0 <= q.y1 && b.y1 >= q.y0 {
                        out.push(id);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> OrientedBox {
        OrientedBox::from_rect(Rect::new(x0, y0, x1, y1))
    }

    #[test]
    fn sat_axis_aligned() {
        assert!(bx(0., 0., 10., 10.).intersects(&bx(5., 5., 15., 15.)));
        assert!(!bx(0., 0., 10., 10.).intersects(&bx(20., 0., 30., 10.)));
        assert!(!bx(0., 0., 10., 10.).intersects(&bx(10., 0., 20., 10.)));
    }

    #[test]
    fn sat_rotated() {
        let diamond = OrientedBox {
            center: Point::new(0.0, 0.0),
            half: Vec2::new(5.0, 5.0),
            angle: FRAC_PI_4,
        };
        // AABB of the diamond reaches x = 7.07; this box sits in its corner region.
        let other = bx(5.0, 5.0, 8.0, 8.0);
        assert!(diamond.aabb().intersect(other.aabb()).area() > 0.0);
        assert!(!diamond.intersects(&other));
        let moved = bx(3.0, 3.0, 6.0, 6.0);
        assert!(diamond.intersects(&moved));
    }

    #[test]
    fn segments() {
        let b = bx(0., 0., 10., 10.);
        assert!(b.intersects_segment(Point::new(-5., 5.), Point::new(15., 5.)));
        assert!(b.intersects_segment(Point::new(2., 2.), Point::new(8., 8.)));
        assert!(!b.intersects_segment(Point::new(-5., -5.), Point::new(-1., 20.)));
        let diamond = OrientedBox {
            center: Point::new(0.0, 0.0),
            half: Vec2::new(5.0, 5.0),
            angle: FRAC_PI_4,
        };
        assert!(!diamond.intersects_segment(Point::new(5., 5.), Point::new(7., 5.)));
        assert!(diamond.aabb().contains(Point::new(6., 5.)) || diamond.aabb().x1 > 6.0);
        assert!(diamond.intersects_segment(Point::new(-10., 0.), Point::new(10., 0.)));
    }

    #[test]
    fn local_rect_transform() {
        let t = Affine::translate((10.0, 20.0)) * Affine::rotate(FRAC_PI_2);
        let b = OrientedBox::from_local_rect(Rect::new(0.0, 0.0, 4.0, 2.0), t);
        // Local (x, y) maps to (10 - y, 20 + x).
        let expect = [(10.0, 20.0), (10.0, 24.0), (8.0, 24.0), (8.0, 20.0)];
        let got = b.corners();
        for e in expect {
            assert!(
                got.iter()
                    .any(|p| (p.x - e.0).abs() < 1e-9 && (p.y - e.1).abs() < 1e-9),
                "missing corner {e:?} in {got:?}"
            );
        }
    }

    #[test]
    fn inflate_grows_aabb() {
        let a = bx(0., 0., 10., 4.).inflate(1.0).aabb();
        assert_eq!(a, Rect::new(-1., -1., 11., 5.));
    }

    #[test]
    fn inside() {
        let bounds = Rect::new(0., 0., 10., 10.);
        assert!(bx(0., 0., 10., 10.).is_inside(bounds));
        assert!(!bx(-1., 0., 5., 5.).is_inside(bounds));
    }

    #[test]
    fn grid_basic() {
        let mut g = GridIndex::new(Rect::new(0., 0., 100., 100.), 10.);
        g.insert(3, Rect::new(0., 0., 5., 5.));
        g.insert(1, Rect::new(2., 2., 8., 8.));
        g.insert(2, Rect::new(50., 50., 60., 60.));
        let mut out = vec![99];
        g.query(Rect::new(0., 0., 10., 10.), &mut out);
        assert_eq!(out, vec![1, 3]);
        g.remove(1, Rect::new(2., 2., 8., 8.));
        g.query(Rect::new(0., 0., 10., 10.), &mut out);
        assert_eq!(out, vec![3]);
        g.query(Rect::new(0., 0., 100., 100.), &mut out);
        assert_eq!(out, vec![2, 3]);
    }

    #[test]
    fn grid_spanning_and_outside() {
        let mut g = GridIndex::new(Rect::new(0., 0., 100., 100.), 10.);
        g.insert(7, Rect::new(0., 0., 100., 100.));
        let mut out = Vec::new();
        g.query(Rect::new(0., 0., 100., 100.), &mut out);
        assert_eq!(out, vec![7]);
        let far = Rect::new(500., 500., 510., 510.);
        g.insert(8, far);
        g.query(far, &mut out);
        assert_eq!(out, vec![8]);
        g.remove(8, far);
        g.query(far, &mut out);
        assert!(out.is_empty());
        let neg = Rect::new(-50., -50., -40., -40.);
        g.insert(9, neg);
        g.query(neg, &mut out);
        assert!(out.contains(&9));
        g.remove(9, neg);
        g.query(neg, &mut out);
        assert!(!out.contains(&9));
    }
}
