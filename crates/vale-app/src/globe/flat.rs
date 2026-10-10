//! The flat view: the whole world in the equirectangular projection. You pan
//! and zoom it. The map repeats to the east and to the west.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use eframe::egui::{Pos2, Rect};

use super::math::{V3, add, cross, normalize, scale};
use super::view::Camera;

/// At this zoom, the whole world fits the canvas.
pub const ZOOM_MIN: f64 = 1.0;
pub const ZOOM_MAX: f64 = 64.0;

/// The number of points of a brush outline.
const OUTLINE_POINTS: usize = 128;

/// The longitude in the range from -π to π.
fn wrap(lon: f64) -> f64 {
    (lon + PI).rem_euclid(TAU) - PI
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatView {
    /// The longitude and the latitude at the middle of the canvas, in radians.
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
}

impl Default for FlatView {
    fn default() -> FlatView {
        FlatView {
            lon: 0.0,
            lat: 0.0,
            zoom: 1.0,
        }
    }
}

impl FlatView {
    /// A view with the given place at the middle, in degrees.
    pub fn centered(lon: f64, lat: f64) -> FlatView {
        FlatView {
            lon: wrap(lon.to_radians()),
            lat: lat.to_radians().clamp(-FRAC_PI_2, FRAC_PI_2),
            zoom: 1.0,
        }
    }

    /// The size of one radian on screen, in points.
    pub fn scale(&self, rect: Rect) -> f64 {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        (w / TAU).min(h / PI) * self.zoom
    }

    /// The width of one copy of the world on screen, in points.
    pub fn world_width(&self, rect: Rect) -> f32 {
        (self.scale(rect) * TAU) as f32
    }

    /// The longitude and the latitude under a screen position, in radians.
    /// The longitude has no limit, and the latitude can be past a pole.
    fn lonlat_at(&self, rect: Rect, pos: Pos2) -> (f64, f64) {
        let (s, d) = (self.scale(rect), pos - rect.center());
        (self.lon + f64::from(d.x) / s, self.lat - f64::from(d.y) / s)
    }

    fn screen(&self, rect: Rect, lon: f64, lat: f64) -> Pos2 {
        let (s, c) = (self.scale(rect), rect.center());
        Pos2::new(
            c.x + ((lon - self.lon) * s) as f32,
            c.y - ((lat - self.lat) * s) as f32,
        )
    }

    /// The screen position of a direction, in the copy of the world that is
    /// nearest to the middle of the canvas.
    pub fn project(&self, rect: Rect, d: V3) -> Pos2 {
        let lon = self.lon + wrap(d[1].atan2(d[0]) - self.lon);
        self.screen(rect, lon, d[2].clamp(-1.0, 1.0).asin())
    }

    /// The world direction under a screen position, or `None` past a pole.
    pub fn unproject(&self, rect: Rect, pos: Pos2) -> Option<V3> {
        let (lon, lat) = self.lonlat_at(rect, pos);
        (lat.abs() <= FRAC_PI_2).then(|| [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()])
    }

    /// Keeps the map on the canvas. A map that is less high than the canvas
    /// stays in the middle.
    pub fn clamp(&mut self, rect: Rect) {
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.lon = wrap(self.lon);
        let half = 0.5 * f64::from(rect.height()) / self.scale(rect);
        let limit = (FRAC_PI_2 - half).max(0.0);
        self.lat = self.lat.clamp(-limit, limit);
    }

    /// The outline of a brush: the circle on the sphere with this center and
    /// this radius in radians, as lines on screen. A circle that crosses the
    /// 180 degree meridian shows at the two edges of the map, so the outline
    /// has one line for each copy of the world on the canvas.
    pub fn outline(&self, rect: Rect, center: V3, radius: f64) -> Vec<Vec<Pos2>> {
        let pole = if center[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let u = normalize(cross(pole, center));
        let v = cross(center, u);
        let (sin, cos) = radius.sin_cos();
        // The longitude follows the ring with no jump. A ring that holds a
        // pole ends one turn from its start.
        let mut lon = self.lon;
        let ring: Vec<Pos2> = (0..=OUTLINE_POINTS)
            .map(|i| {
                let (st, ct) = (i as f64 / OUTLINE_POINTS as f64 * TAU).sin_cos();
                let around = add(scale(u, ct * sin), scale(v, st * sin));
                let d = add(scale(center, cos), around);
                lon += wrap(d[1].atan2(d[0]) - lon);
                self.screen(rect, lon, d[2].clamp(-1.0, 1.0).asin())
            })
            .collect();
        let width = self.world_width(rect);
        let (min, max) = ring.iter().fold((f32::MAX, f32::MIN), |(min, max), p| {
            (min.min(p.x), max.max(p.x))
        });
        let first = ((rect.left() - max) / width).ceil() as i32;
        let last = ((rect.right() - min) / width).floor() as i32;
        (first..=last)
            .map(|copy| {
                let shift = copy as f32 * width;
                ring.iter().map(|p| Pos2::new(p.x + shift, p.y)).collect()
            })
            .collect()
    }
}

impl Camera for FlatView {
    /// One step of a drag and a pinch together. The place under `from` moves
    /// to `to`. The map does not turn.
    fn gesture(&mut self, rect: Rect, from: Pos2, to: Pos2, scale: f64, _twist: f64) {
        let (lon, lat) = self.lonlat_at(rect, from);
        self.zoom = (self.zoom * scale).clamp(ZOOM_MIN, ZOOM_MAX);
        let (s, d) = (self.scale(rect), to - rect.center());
        self.lon = lon - f64::from(d.x) / s;
        self.lat = lat + f64::from(d.y) / s;
        self.clamp(rect);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::Vec2;

    use super::*;
    use crate::globe::math::{angle, lonlat_to_dir};

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn the_whole_world_fits_the_canvas() {
        let view = FlatView::default();
        assert_eq!(view.world_width(rect()), 800.0);
        let west = view.project(rect(), lonlat_to_dir(-179.99, 0.0));
        let north = view.project(rect(), lonlat_to_dir(0.0, 90.0));
        assert!((west.x - 10.0).abs() < 0.1, "{west:?}");
        assert!((north.y - (320.0 - 200.0)).abs() < 0.1, "{north:?}");
        assert!(view.unproject(rect(), Pos2::new(400.0, 100.0)).is_none());
    }

    #[test]
    fn project_and_unproject_agree() {
        let mut view = FlatView::centered(170.0, 10.0);
        view.zoom = 3.0;
        let d = lonlat_to_dir(-150.0, 20.0);
        let p = view.project(rect(), d);
        assert!(rect().contains(p), "{p:?}");
        assert!(angle(view.unproject(rect(), p).unwrap(), d) < 1e-5);
    }

    #[test]
    fn a_gesture_keeps_the_grabbed_place_under_the_finger() {
        let mut view = FlatView::default();
        let (from, to) = (Pos2::new(300.0, 250.0), Pos2::new(520.0, 300.0));
        let grabbed = view.unproject(rect(), from).unwrap();
        view.gesture(rect(), from, to, 2.5, 0.4);
        assert_eq!(view.zoom, 2.5);
        let after = view.project(rect(), grabbed);
        assert!((after - to).length() < 0.01, "{after:?}");
    }

    #[test]
    fn the_map_stays_on_the_canvas() {
        let mut view = FlatView::default();
        // The map is less high than the canvas, so it does not move up.
        view.gesture(
            rect(),
            Pos2::new(400.0, 300.0),
            Pos2::new(400.0, 100.0),
            1.0,
            0.0,
        );
        assert_eq!(view.lat, 0.0);
        view.zoom_at(rect(), Pos2::new(400.0, 300.0), 0.1);
        assert_eq!(view.zoom, ZOOM_MIN);
        view.zoom = 4.0;
        view.gesture(
            rect(),
            Pos2::new(400.0, 300.0),
            Pos2::new(400.0, 9000.0),
            1.0,
            0.0,
        );
        let top = view.project(rect(), lonlat_to_dir(view.lon.to_degrees(), 90.0));
        assert!((top.y - rect().top()).abs() < 0.01, "{top:?}");
    }

    /// The largest distance on the sphere from the brush radius to a point of
    /// the outline, in radians.
    fn outline_error(view: &FlatView, center: V3, radius: f64) -> f64 {
        let lines = view.outline(rect(), center, radius);
        assert!(!lines.is_empty());
        let points = lines.iter().flatten();
        let errors = points.map(|p| {
            let d = view.unproject(rect(), *p).unwrap();
            (angle(d, center) - radius).abs()
        });
        errors.fold(0.0, f64::max)
    }

    #[test]
    fn the_outline_is_the_edge_of_the_stamp() {
        let view = FlatView::default();
        for (lon, lat) in [(20.0, 0.0), (20.0, 75.0), (-100.0, -84.0), (179.0, 30.0)] {
            let error = outline_error(&view, lonlat_to_dir(lon, lat), 0.1);
            assert!(error < 1e-4, "{lon} {lat}: {error}");
        }
    }

    /// The width and the height of the first line of an outline, in points.
    fn outline_size(view: &FlatView, lat: f64, radius: f64) -> (f32, f32) {
        let lines = view.outline(rect(), lonlat_to_dir(20.0, lat), radius);
        assert_eq!(lines.len(), 1);
        let bounds = Rect::from_points(&lines[0]);
        (bounds.width(), bounds.height())
    }

    #[test]
    fn the_outline_is_round_at_the_equator_and_wide_near_a_pole() {
        let view = FlatView::default();
        let radius = 0.1;
        let points = (2.0 * radius * view.scale(rect())) as f32;
        let (w, h) = outline_size(&view, 0.0, radius);
        assert!((w - points).abs() < 0.1 && (h - points).abs() < 0.1);
        // At 75 degrees, the circle covers asin(sin r / cos lat) of longitude
        // to each side.
        let (w, h) = outline_size(&view, 75.0, radius);
        let half = (radius.sin() / 75_f64.to_radians().cos()).asin();
        let wide = (2.0 * half * view.scale(rect())) as f32;
        assert!((w - wide).abs() < 0.2, "{w} and {wide}");
        assert!((h - points).abs() < 0.2, "{h}");
        assert!(w > 3.5 * h);
    }

    #[test]
    fn an_outline_across_the_meridian_shows_at_the_two_edges() {
        let view = FlatView::default();
        let lines = view.outline(rect(), lonlat_to_dir(179.0, 0.0), 0.1);
        assert_eq!(lines.len(), 2);
        let (left, right) = (rect().left(), rect().right());
        assert!(lines[0].iter().any(|p| p.x > left && p.x < left + 30.0));
        assert!(lines[1].iter().any(|p| p.x < right && p.x > right - 30.0));
    }

    #[test]
    fn an_outline_around_a_pole_goes_across_the_whole_map() {
        let view = FlatView::default();
        let lines = view.outline(rect(), lonlat_to_dir(40.0, 88.0), 0.1);
        let width = view.world_width(rect());
        for line in &lines {
            let (first, last) = (line[0], line[line.len() - 1]);
            assert!(((last.x - first.x).abs() - width).abs() < 0.01);
            assert!((last.y - first.y).abs() < 0.01);
        }
        // The lines of the copies join, and they cover the canvas.
        let bounds = Rect::from_points(&lines.concat());
        assert!(bounds.left() <= rect().left() && bounds.right() >= rect().right());
    }
}
