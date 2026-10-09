//! The map view.

use kurbo::{Point, Rect};

/// The part of the projected plane that the map view shows.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct View {
    /// Projected meters.
    pub center: kurbo::Point,
    /// Points per meter.
    pub scale: f64,
}

/// A view and the size of the page it is drawn on, in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewTransform {
    pub view: View,
    pub width: f64,
    pub height: f64,
}

impl ViewTransform {
    /// Projected meters to page points. North is up.
    pub fn to_page(&self, p: Point) -> Point {
        let s = self.view.scale;
        let c = self.view.center;
        Point::new(
            self.width / 2.0 + (p.x - c.x) * s,
            self.height / 2.0 - (p.y - c.y) * s,
        )
    }

    /// Page points to projected meters.
    pub fn from_page(&self, q: Point) -> Point {
        let s = self.view.scale;
        let c = self.view.center;
        Point::new(
            c.x + (q.x - self.width / 2.0) / s,
            c.y - (q.y - self.height / 2.0) / s,
        )
    }

    /// The whole page.
    pub fn page_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
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
        }
    }

    /// The map moves by `(dx, dy)` points on the page.
    pub fn panned(self, dx: f64, dy: f64) -> View {
        View {
            center: Point::new(
                self.center.x - dx / self.scale,
                self.center.y + dy / self.scale,
            ),
            scale: self.scale,
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
        let p = ViewTransform {
            view: self,
            width,
            height,
        }
        .from_page(q);
        let scale = (self.scale * factor).clamp(min_scale, max_scale);
        View {
            center: Point::new(
                p.x - (q.x - width / 2.0) / scale,
                p.y + (q.y - height / 2.0) / scale,
            ),
            scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vt() -> ViewTransform {
        ViewTransform {
            view: View {
                center: Point::new(1000.0, -500.0),
                scale: 0.01,
            },
            width: 800.0,
            height: 600.0,
        }
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
        let t = ViewTransform {
            view: v,
            width: 800.0,
            height: 600.0,
        };
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
}
