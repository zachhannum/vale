//! The globe view: an orthographic camera that you rotate and zoom.

use eframe::egui::{Pos2, Rect, Vec2};

use super::math::{Mat3, V3, cross, lonlat_to_dir, normalize};

pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 40.0;

/// A view that the navigation moves.
pub trait Camera {
    /// One step of a drag, a pinch, and a twist together. The place under
    /// `from` moves to `to`.
    fn gesture(&mut self, rect: Rect, from: Pos2, to: Pos2, scale: f64, twist: f64);

    /// Zooms and keeps the place under `at` there.
    fn zoom_at(&mut self, rect: Rect, at: Pos2, scale: f64) {
        self.gesture(rect, at, at, scale, 0.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobeView {
    /// World to view. In view space, x is right, y is up, and z is to the eye.
    pub rot: Mat3,
    pub zoom: f64,
}

impl GlobeView {
    /// A view with north up and the given place at the center.
    pub fn centered(lon: f64, lat: f64) -> GlobeView {
        let z = lonlat_to_dir(lon, lat);
        let x = normalize(cross([0.0, 0.0, 1.0], z));
        let y = cross(z, x);
        GlobeView {
            rot: Mat3([x, y, z]),
            zoom: 1.0,
        }
    }

    /// The globe radius on screen, in points.
    pub fn radius(&self, rect: Rect) -> f64 {
        0.45 * f64::from(rect.width().min(rect.height())) * self.zoom
    }

    /// The screen position of a direction, or `None` on the far side.
    pub fn project(&self, rect: Rect, d: V3) -> Option<Pos2> {
        let v = self.rot.mul_vec(d);
        (v[2] > 0.0).then(|| self.view_to_screen(rect, v))
    }

    pub fn view_to_screen(&self, rect: Rect, v: V3) -> Pos2 {
        let r = self.radius(rect);
        let c = rect.center();
        Pos2::new(c.x + (v[0] * r) as f32, c.y - (v[1] * r) as f32)
    }

    /// The view-space direction under a screen position. A position off the
    /// globe gives the nearest point on the edge, and `false`.
    fn screen_to_view(&self, rect: Rect, pos: Pos2) -> (V3, bool) {
        let r = self.radius(rect);
        let d: Vec2 = pos - rect.center();
        let (x, y) = (f64::from(d.x) / r, -f64::from(d.y) / r);
        let l2 = x * x + y * y;
        if l2 <= 1.0 {
            ([x, y, (1.0 - l2).sqrt()], true)
        } else {
            let l = l2.sqrt();
            ([x / l, y / l, 0.0], false)
        }
    }

    /// The world direction under a screen position, or `None` off the globe.
    pub fn unproject(&self, rect: Rect, pos: Pos2) -> Option<V3> {
        let (v, on) = self.screen_to_view(rect, pos);
        on.then(|| self.rot.inv_mul_vec(v))
    }
}

impl Camera for GlobeView {
    fn gesture(&mut self, rect: Rect, from: Pos2, to: Pos2, scale: f64, twist: f64) {
        let (grab, _) = self.screen_to_view(rect, from);
        let world = self.rot.inv_mul_vec(grab);
        self.zoom = (self.zoom * scale).clamp(ZOOM_MIN, ZOOM_MAX);
        if twist != 0.0 {
            self.rot = Mat3::axis_angle([0.0, 0.0, 1.0], twist).mul(&self.rot);
        }
        let now = self.rot.mul_vec(world);
        let (target, _) = self.screen_to_view(rect, to);
        self.rot = Mat3::between(now, target).mul(&self.rot).orthonormalized();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::math::angle;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn project_and_unproject_agree() {
        let view = GlobeView::centered(30.0, 40.0);
        let d = lonlat_to_dir(50.0, 20.0);
        let p = view.project(rect(), d).unwrap();
        assert!(angle(view.unproject(rect(), p).unwrap(), d) < 1e-5);
    }

    #[test]
    fn gesture_keeps_the_grabbed_point_under_the_finger() {
        let mut view = GlobeView::centered(0.0, 0.0);
        let (from, to) = (Pos2::new(300.0, 250.0), Pos2::new(520.0, 380.0));
        let grabbed = view.unproject(rect(), from).unwrap();
        view.gesture(rect(), from, to, 1.7, 0.4);
        let after = view.project(rect(), grabbed).unwrap();
        assert!((after - to).length() < 0.01, "{after:?}");
    }
}
