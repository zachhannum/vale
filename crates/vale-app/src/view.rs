//! The map view.

use kurbo::{Point, Rect};

/// The part of the projected plane that the map view shows.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct View {
    /// Projected meters.
    pub center: kurbo::Point,
    /// Points per meter.
    pub scale: f64,
    /// Radians clockwise. The map turns clockwise on the page.
    pub rotation: f64,
}

/// A view and the size of the page it is drawn on, in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewTransform {
    pub view: View,
    pub width: f64,
    pub height: f64,
    /// The page point of the center of the view.
    pub origin: Point,
}

impl ViewTransform {
    /// A transform with the center of the view at the middle of the page.
    pub fn new(view: View, width: f64, height: f64) -> ViewTransform {
        ViewTransform {
            view,
            width,
            height,
            origin: Point::new(width / 2.0, height / 2.0),
        }
    }

    /// Projected meters to page points. North is up when the rotation is 0.
    pub fn to_page(&self, p: Point) -> Point {
        let s = self.view.scale;
        let c = self.view.center;
        let (sin, cos) = self.view.rotation.sin_cos();
        let (dx, dy) = ((p.x - c.x) * s, (p.y - c.y) * s);
        Point::new(
            self.origin.x + dx * cos + dy * sin,
            self.origin.y - (-dx * sin + dy * cos),
        )
    }

    /// Page points to projected meters.
    pub fn from_page(&self, q: Point) -> Point {
        let s = self.view.scale;
        let c = self.view.center;
        let (sin, cos) = self.view.rotation.sin_cos();
        let (u, v) = (q.x - self.origin.x, self.origin.y - q.y);
        Point::new(c.x + (u * cos - v * sin) / s, c.y + (u * sin + v * cos) / s)
    }

    /// The whole page.
    pub fn page_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// The page box of a box of projected meters.
    pub fn page_bbox(&self, b: Rect) -> Rect {
        let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)];
        let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in corners {
            let q = self.to_page(Point::new(x, y));
            r = Rect::new(r.x0.min(q.x), r.y0.min(q.y), r.x1.max(q.x), r.y1.max(q.y));
        }
        r
    }
}

impl View {
    /// The view that shows `bounds` in a page, with a margin on every side.
    pub fn fit(bounds: Rect, width: f64, height: f64, margin: f64) -> View {
        let aw = (width - 2.0 * margin).max(1.0);
        let ah = (height - 2.0 * margin).max(1.0);
        let bw = bounds.width().max(1e-9);
        let bh = bounds.height().max(1e-9);
        View {
            center: bounds.center(),
            scale: (aw / bw).min(ah / bh),
            rotation: 0.0,
        }
    }

    /// The map moves by `(dx, dy)` points on the page.
    pub fn panned(self, dx: f64, dy: f64) -> View {
        let moved = ViewTransform::new(self, 0.0, 0.0).from_page(Point::new(dx, dy));
        View {
            center: self.center - (moved - self.center),
            ..self
        }
    }

    /// Zooms so that the projected point under `q` stays under `q`.
    pub fn zoomed_at(
        self,
        q: Point,
        factor: f64,
        width: f64,
        height: f64,
        min_scale: f64,
        max_scale: f64,
    ) -> View {
        let p = ViewTransform::new(self, width, height).from_page(q);
        let scale = (self.scale * factor).clamp(min_scale, max_scale);
        let at_p = View {
            center: p,
            scale,
            ..self
        };
        let off = ViewTransform::new(at_p, width, height).from_page(q) - p;
        View {
            center: p - off,
            ..at_p
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vt() -> ViewTransform {
        ViewTransform::new(
            View {
                center: Point::new(1000.0, -500.0),
                scale: 0.01,
                rotation: 0.0,
            },
            800.0,
            600.0,
        )
    }

    #[test]
    fn round_trip() {
        let t = vt();
        let p = Point::new(-2345.0, 6789.0);
        let r = t.from_page(t.to_page(p));
        assert!((r - p).hypot() < 1e-9);
        assert_eq!(t.to_page(t.view.center), Point::new(400.0, 300.0));
    }

    #[test]
    fn north_is_up() {
        let t = vt();
        let a = t.to_page(Point::new(0.0, 0.0));
        let b = t.to_page(Point::new(0.0, 100.0));
        assert!(b.y < a.y);
        assert_eq!(t.page_rect(), Rect::new(0.0, 0.0, 800.0, 600.0));
    }

    #[test]
    fn fit_bounds() {
        let b = Rect::new(-1000.0, -300.0, 3000.0, 700.0);
        let v = View::fit(b, 800.0, 600.0, 24.0);
        let t = ViewTransform::new(v, 800.0, 600.0);
        let c = t.to_page(b.center());
        assert!((c.x - 400.0).abs() < 1e-9 && (c.y - 300.0).abs() < 1e-9);
        for p in [
            Point::new(b.x0, b.y0),
            Point::new(b.x1, b.y0),
            Point::new(b.x0, b.y1),
            Point::new(b.x1, b.y1),
        ] {
            let q = t.to_page(p);
            assert!(q.x >= 0.0 && q.x <= 800.0 && q.y >= 0.0 && q.y <= 600.0);
        }
        let tiny = View::fit(b, 10.0, 10.0, 24.0);
        assert!(tiny.scale > 0.0);
    }

    #[test]
    fn pan_moves_by_the_amount() {
        let t = vt();
        let p = Point::new(123.0, 456.0);
        let a = t.to_page(p);
        let t2 = ViewTransform {
            view: t.view.panned(10.0, 0.0),
            ..t
        };
        let b = t2.to_page(p);
        assert!((b.x - a.x - 10.0).abs() < 1e-9 && (b.y - a.y).abs() < 1e-9);
        let t3 = ViewTransform {
            view: t.view.panned(0.0, -7.0),
            ..t
        };
        assert!((t3.to_page(p).y - a.y + 7.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_keeps_the_cursor_point() {
        let t = vt();
        let q = Point::new(123.0, 456.0);
        let before = t.from_page(q);
        let v = t.view.zoomed_at(q, 2.5, 800.0, 600.0, 0.001, 10.0);
        let t2 = ViewTransform { view: v, ..t };
        let after = t2.from_page(q);
        assert!((after - before).hypot() < 1e-9);
        assert!((v.scale - 0.025).abs() < 1e-12);
        let hi = t.view.zoomed_at(q, 1e6, 800.0, 600.0, 0.001, 10.0);
        assert_eq!(hi.scale, 10.0);
        let lo = t.view.zoomed_at(q, 1e-9, 800.0, 600.0, 0.001, 10.0);
        assert_eq!(lo.scale, 0.001);
    }

    #[test]
    fn a_rotation_turns_the_map_clockwise_and_round_trips() {
        let mut t = vt();
        t.view.rotation = std::f64::consts::FRAC_PI_2;
        t.origin = Point::new(10.0, 20.0);
        let c = t.view.center;
        // North points right on the page.
        let north = t.to_page(Point::new(c.x, c.y + 100.0));
        assert!((north - Point::new(11.0, 20.0)).hypot() < 1e-9, "{north:?}");
        let p = Point::new(-2345.0, 6789.0);
        assert!((t.from_page(t.to_page(p)) - p).hypot() < 1e-9);
        let b = Rect::new(c.x, c.y, c.x + 200.0, c.y + 100.0);
        let bb = t.page_bbox(b);
        assert!((bb.width() - 1.0).abs() < 1e-9 && (bb.height() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn pan_and_zoom_follow_a_rotation() {
        let mut t = vt();
        t.view.rotation = 0.7;
        let p = Point::new(123.0, 456.0);
        let a = t.to_page(p);
        let moved = ViewTransform {
            view: t.view.panned(10.0, -4.0),
            ..t
        };
        let b = moved.to_page(p);
        assert!((b - a - kurbo::Vec2::new(10.0, -4.0)).hypot() < 1e-9);
        let q = Point::new(50.0, 80.0);
        let before = t.from_page(q);
        let zoomed = ViewTransform {
            view: t.view.zoomed_at(q, 3.0, 800.0, 600.0, 0.001, 10.0),
            ..t
        };
        assert!((zoomed.from_page(q) - before).hypot() < 1e-6);
    }
}
