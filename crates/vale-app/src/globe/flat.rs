//! The flat view: the whole world in one projection. You pan and zoom it.

use std::f64::consts::TAU;
use std::sync::Arc;

use eframe::egui::{Pos2, Rect};
use vale_sphere::{Point, Projection, ProjectionKind, ProjectionSpec, wrap180};

use super::math::{V3, add, cross, dir_to_lonlat, lonlat_to_dir, normalize, scale};
use super::view::Camera;

/// The limits of the zoom. At a zoom of 1, the whole map fits the canvas.
pub const ZOOM_MIN: f64 = 0.25;
pub const ZOOM_MAX: f64 = 64.0;

/// The number of points of a brush outline.
const OUTLINE_POINTS: usize = 128;

/// The size of a cell of the mesh, in degrees.
const MESH_STEP: f64 = 1.0;

/// The projection of a new flat view.
pub const DEFAULT_SPEC: ProjectionSpec = ProjectionSpec {
    kind: ProjectionKind::Equirectangular,
    lon0: 0.0,
    lat0: 0.0,
};

/// One corner of the mesh: a point of the map on a sphere of radius 1, and
/// the world direction of the place there.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub point: [f32; 2],
    pub dir: [f32; 3],
}

/// The triangles that cover the map of one projection. PROJ gives the
/// corners, and the GPU interpolates between them.
pub struct FlatMesh {
    pub spec: ProjectionSpec,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl FlatMesh {
    fn new(projection: &Projection) -> FlatMesh {
        let mesh = projection.mesh(MESH_STEP);
        let vertices = mesh.vertices.iter().map(|vertex| {
            let (point, place) = (*vertex).unwrap_or_default();
            Vertex {
                point: [point.x as f32, point.y as f32],
                dir: lonlat_to_dir(place[0], place[1]).map(|v| v as f32),
            }
        });
        let mut indices = Vec::with_capacity((mesh.cols - 1) * (mesh.rows - 1) * 6);
        for row in 0..mesh.rows - 1 {
            for col in 0..mesh.cols - 1 {
                let a = row * mesh.cols + col;
                let corners = [a, a + 1, a + mesh.cols, a + mesh.cols + 1];
                if corners.iter().all(|i| mesh.vertices[*i].is_some()) {
                    let [a, b, c, d] = corners.map(|i| i as u32);
                    indices.extend([a, b, c, b, d, c]);
                }
            }
        }
        FlatMesh {
            spec: projection.spec(),
            vertices: vertices.collect(),
            indices,
        }
    }
}

pub struct FlatView {
    /// The projection on a sphere of radius 1.
    projection: Projection,
    /// The box around the map, in the units of the projection.
    bounds: vale_sphere::Rect,
    /// The point of the map at the middle of the canvas.
    pub center: Point,
    pub zoom: f64,
    mesh: Arc<FlatMesh>,
}

impl Default for FlatView {
    fn default() -> FlatView {
        FlatView::new(DEFAULT_SPEC)
    }
}

impl FlatView {
    pub fn new(spec: ProjectionSpec) -> FlatView {
        let projection = Projection::new(spec, 1.0).expect("each preset is a projection of PROJ");
        let bounds = projection.bounds();
        let mesh = Arc::new(FlatMesh::new(&projection));
        FlatView {
            projection,
            bounds,
            center: bounds.center(),
            zoom: 1.0,
            mesh,
        }
    }

    pub fn spec(&self) -> ProjectionSpec {
        self.projection.spec()
    }

    /// Changes the projection, and shows the whole map again.
    pub fn set_spec(&mut self, spec: ProjectionSpec) {
        if spec.normalized() != self.spec() {
            *self = FlatView::new(spec);
        }
    }

    /// The projection with its center at the place at the middle of the
    /// canvas. `None`: the place is off the map, or it is the center now.
    fn spec_here(&self) -> Option<ProjectionSpec> {
        let [lon0, lat0] = self.projection.inverse(self.center)?;
        let (now, kind) = (self.spec(), self.spec().kind);
        let here = ProjectionSpec { kind, lon0, lat0 }.normalized();
        let moved = (here.lon0 - now.lon0).abs() > 1e-6 || (here.lat0 - now.lat0).abs() > 1e-6;
        moved.then_some(here)
    }

    /// True if `center_here` changes the projection.
    pub fn can_center_here(&self) -> bool {
        self.spec_here().is_some()
    }

    /// Makes the place at the middle of the canvas the center of the
    /// projection.
    pub fn center_here(&mut self) {
        if let Some(spec) = self.spec_here() {
            self.set_spec(spec);
        }
    }

    /// True if `reset` changes the projection or the view.
    pub fn can_reset(&self) -> bool {
        let spec = self.spec();
        let moved = self.zoom != 1.0 || self.center != self.bounds.center();
        spec.lon0 != 0.0 || spec.lat0 != 0.0 || moved
    }

    /// Puts the center of the projection back at 0 degrees east and 0 degrees
    /// north, and shows the whole map.
    pub fn reset(&mut self) {
        let kind = self.spec().kind;
        *self = FlatView::new(ProjectionSpec {
            kind,
            lon0: 0.0,
            lat0: 0.0,
        });
    }

    /// Puts a place at the middle of the canvas. The place is in degrees.
    pub fn look_at(&mut self, lon: f64, lat: f64) {
        if let Some(point) = self.projection.forward([lon, lat]) {
            self.center = point;
        }
    }

    pub fn mesh(&self) -> Arc<FlatMesh> {
        self.mesh.clone()
    }

    /// The size of one unit of the projection on screen, in points. A unit
    /// is one radian on the ground where the projection has its true scale.
    pub fn scale(&self, rect: Rect) -> f64 {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        (w / self.bounds.width()).min(h / self.bounds.height()) * self.zoom
    }

    fn point_at(&self, rect: Rect, pos: Pos2) -> Point {
        let (s, d) = (self.scale(rect), pos - rect.center());
        Point::new(
            self.center.x + f64::from(d.x) / s,
            self.center.y - f64::from(d.y) / s,
        )
    }

    fn screen(&self, rect: Rect, point: Point) -> Pos2 {
        let (s, c) = (self.scale(rect), rect.center());
        Pos2::new(
            c.x + ((point.x - self.center.x) * s) as f32,
            c.y - ((point.y - self.center.y) * s) as f32,
        )
    }

    /// The screen position of a direction, or `None` for a place that the
    /// projection does not show.
    pub fn project(&self, rect: Rect, d: V3) -> Option<Pos2> {
        let (lon, lat) = dir_to_lonlat(d);
        let point = self.projection.forward([lon, lat])?;
        Some(self.screen(rect, point))
    }

    /// The world direction under a screen position, or `None` off the map.
    pub fn unproject(&self, rect: Rect, pos: Pos2) -> Option<V3> {
        let [lon, lat] = self.projection.inverse(self.point_at(rect, pos))?;
        Some(lonlat_to_dir(lon, lat))
    }

    /// Keeps the middle of the canvas inside the box around the map. Each
    /// place of the map can thus come to the middle at each zoom.
    pub fn clamp(&mut self) {
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let b = &self.bounds;
        self.center = Point::new(
            self.center.x.clamp(b.x0, b.x1),
            self.center.y.clamp(b.y0, b.y1),
        );
    }

    /// The outline of a brush: the circle on the sphere with this center and
    /// this radius in radians, as lines on screen. The projection cuts the
    /// circle at its edge, so a circle across the edge shows on each side.
    pub fn outline(&self, rect: Rect, center: V3, radius: f64) -> Vec<Vec<Pos2>> {
        let pole = if center[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let u = normalize(cross(pole, center));
        let v = cross(center, u);
        let (sin, cos) = radius.sin_cos();
        // The longitude follows the ring with no jump of 360 degrees, so
        // each step of the ring goes the short way.
        let mut east = 0.0;
        let ring: Vec<[f64; 2]> = (0..=OUTLINE_POINTS)
            .map(|i| {
                let (st, ct) = (i as f64 / OUTLINE_POINTS as f64 * TAU).sin_cos();
                let around = add(scale(u, ct * sin), scale(v, st * sin));
                let (lon, lat) = dir_to_lonlat(add(scale(center, cos), around));
                east += wrap180(lon - east);
                [east, lat]
            })
            .collect();
        let lines = self.projection.project_line(&ring);
        let on_screen = |line: Vec<Point>| line.into_iter().map(|p| self.screen(rect, p)).collect();
        lines.into_iter().map(on_screen).collect()
    }
}

impl Camera for FlatView {
    /// One step of a drag and a pinch together. The place under `from` moves
    /// to `to`. The map does not turn.
    fn gesture(&mut self, rect: Rect, from: Pos2, to: Pos2, scale: f64, _twist: f64) {
        let grabbed = self.point_at(rect, from);
        self.zoom = (self.zoom * scale).clamp(ZOOM_MIN, ZOOM_MAX);
        let (s, d) = (self.scale(rect), to - rect.center());
        self.center = Point::new(
            grabbed.x - f64::from(d.x) / s,
            grabbed.y + f64::from(d.y) / s,
        );
        self.clamp();
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::Vec2;

    use super::*;
    use crate::globe::math::angle;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    fn view(kind: ProjectionKind, lon0: f64, lat0: f64) -> FlatView {
        FlatView::new(ProjectionSpec { kind, lon0, lat0 })
    }

    #[test]
    fn the_whole_world_fits_the_canvas() {
        let view = FlatView::default();
        assert_eq!(view.spec().kind, ProjectionKind::Equirectangular);
        let west = view.project(rect(), lonlat_to_dir(-179.9, 0.0)).unwrap();
        let north = view.project(rect(), lonlat_to_dir(0.0, 90.0)).unwrap();
        assert!((west.x - 10.0).abs() < 0.5, "{west:?}");
        assert!((north.y - (320.0 - 200.0)).abs() < 0.1, "{north:?}");
        assert!(view.unproject(rect(), Pos2::new(400.0, 100.0)).is_none());
    }

    #[test]
    fn each_projection_fits_the_canvas_and_inverts() {
        for kind in ProjectionKind::ALL {
            let view = view(kind, 30.0, 40.0);
            let d = lonlat_to_dir(50.0, 20.0);
            let p = view.project(rect(), d).unwrap();
            assert!(rect().contains(p), "{kind:?}: {p:?}");
            assert!(angle(view.unproject(rect(), p).unwrap(), d) < 1e-5);
            // The map touches the canvas on two sides, and no part is off it.
            let mesh = view.mesh();
            let points = mesh.vertices.iter().map(|v| {
                let point = Point::new(f64::from(v.point[0]), f64::from(v.point[1]));
                view.screen(rect(), point)
            });
            let map = Rect::from_points(&points.collect::<Vec<_>>());
            assert!(rect().expand(0.5).contains_rect(map), "{kind:?}: {map:?}");
            let full = (map.width() - 800.0).abs() < 1.0 || (map.height() - 600.0).abs() < 1.0;
            assert!(full, "{kind:?}: {map:?}");
        }
    }

    #[test]
    fn each_vertex_of_the_mesh_has_the_direction_of_its_place() {
        for kind in ProjectionKind::ALL {
            let view = view(kind, -70.0, 55.0);
            let mesh = view.mesh();
            assert!(!mesh.indices.is_empty());
            // The inverse of PROJ is not exact at the edge, so the test
            // reads the vertices inside it.
            let mut read = 0;
            for v in mesh.vertices.iter().step_by(37) {
                let point = Point::new(f64::from(v.point[0]), f64::from(v.point[1]));
                let Some([lon, lat]) = view.projection.inverse(point) else {
                    continue;
                };
                let d = v.dir.map(f64::from);
                assert!(angle(lonlat_to_dir(lon, lat), d) < 1e-4, "{kind:?}");
                read += 1;
            }
            assert!(read > 100, "{kind:?}: {read}");
        }
    }

    #[test]
    fn a_gesture_keeps_the_grabbed_place_under_the_finger() {
        for kind in ProjectionKind::ALL {
            let mut view = view(kind, 0.0, 0.0);
            let (from, to) = (Pos2::new(380.0, 300.0), Pos2::new(430.0, 330.0));
            let grabbed = view.unproject(rect(), from).unwrap();
            view.gesture(rect(), from, to, 2.5, 0.4);
            assert_eq!(view.zoom, 2.5);
            let after = view.project(rect(), grabbed).unwrap();
            assert!((after - to).length() < 0.01, "{kind:?}: {after:?}");
        }
    }

    #[test]
    fn each_place_of_the_map_comes_to_the_middle_at_each_zoom() {
        let middle = rect().center();
        for zoom in [ZOOM_MIN, 1.0, 4.0] {
            let mut view = FlatView::new(DEFAULT_SPEC);
            view.zoom = zoom;
            let place = lonlat_to_dir(150.0, -70.0);
            let from = view.project(rect(), place).unwrap();
            view.gesture(rect(), from, middle, 1.0, 0.0);
            let after = view.project(rect(), place).unwrap();
            assert!((after - middle).length() < 0.01, "{zoom}: {after:?}");
            assert!(view.can_center_here());
        }
        // The middle of the canvas does not leave the map.
        let mut view = FlatView::default();
        view.gesture(rect(), middle, Pos2::new(9000.0, 9000.0), 1.0, 0.0);
        let corner = view.project(rect(), lonlat_to_dir(-179.99, 89.99)).unwrap();
        assert!((corner - middle).length() < 1.0, "{corner:?}");
        view.zoom_at(rect(), middle, 0.01);
        assert_eq!(view.zoom, ZOOM_MIN);
    }

    #[test]
    fn a_new_projection_shows_the_whole_map_and_center_here_moves_its_center() {
        let mut view = FlatView::new(DEFAULT_SPEC);
        view.zoom = 5.0;
        view.look_at(40.0, 30.0);
        view.center_here();
        let spec = view.spec();
        assert!(
            (spec.lon0 - 40.0).abs() < 1e-6 && spec.lat0 == 0.0,
            "{spec:?}"
        );
        assert_eq!(view.zoom, 1.0);

        view.set_spec(ProjectionSpec {
            kind: ProjectionKind::Orthographic,
            ..spec
        });
        view.look_at(60.0, 50.0);
        view.center_here();
        let spec = view.spec();
        assert!((spec.lon0 - 60.0).abs() < 1e-6 && (spec.lat0 - 50.0).abs() < 1e-6);
        let middle = view.project(rect(), lonlat_to_dir(60.0, 50.0)).unwrap();
        assert!((middle - rect().center()).length() < 0.01);

        // A reset keeps the projection and puts its center and the view back.
        view.zoom = 3.0;
        assert!(view.can_reset());
        view.reset();
        let spec = view.spec();
        assert_eq!(spec.kind, ProjectionKind::Orthographic);
        assert_eq!((spec.lon0, spec.lat0, view.zoom), (0.0, 0.0, 1.0));
        assert!(!view.can_reset());
    }

    #[test]
    fn the_outline_is_the_edge_of_the_stamp_in_each_projection() {
        let radius = 0.1;
        for kind in ProjectionKind::ALL {
            let view = view(kind, 10.0, 30.0);
            for (lon, lat) in [(20.0, 0.0), (20.0, 75.0), (-60.0, 40.0), (175.0, 30.0)] {
                let center = lonlat_to_dir(lon, lat);
                let lines = view.outline(rect(), center, radius);
                // The orthographic projection shows one half of the world.
                let shown = view.project(rect(), center).is_some();
                assert_eq!(!lines.is_empty(), shown, "{kind:?} {lon} {lat}");
                for p in lines.iter().flatten() {
                    // A point on the edge of the map can have no inverse.
                    if let Some(d) = view.unproject(rect(), *p) {
                        let error = (angle(d, center) - radius).abs();
                        assert!(error < 1e-3, "{kind:?} {lon} {lat}: {error}");
                    }
                }
            }
        }
    }

    /// The width and the height of an outline, in points.
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
        let (left, right) = (rect().left(), rect().right());
        let points = || lines.iter().flatten();
        assert!(points().any(|p| p.x >= left && p.x < left + 30.0));
        assert!(points().any(|p| p.x <= right && p.x > right - 30.0));
        assert!(points().all(|p| p.x < left + 30.0 || p.x > right - 30.0));
    }

    #[test]
    fn an_outline_around_a_pole_goes_across_the_whole_map() {
        let view = FlatView::default();
        let lines = view.outline(rect(), lonlat_to_dir(40.0, 88.0), 0.1);
        let bounds = Rect::from_points(&lines.concat());
        assert!(bounds.left() < rect().left() + 1.0 && bounds.right() > rect().right() - 1.0);
    }
}
