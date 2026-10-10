//! The PROJ wrapper with clipping.

use std::fmt;

use kurbo::{Point, Rect};

use crate::clip::{ClipRect, EPS, clip_line_rect, clip_ring_rect, densify, shifts, unwrap};
use crate::frame::{Frame, LonLat, wrap180};
use crate::spec::{ProjectionKind, ProjectionSpec};

/// An error from building a projection.
#[derive(Debug)]
pub enum SphereError {
    /// The radius is not finite or not above zero.
    BadRadius(f64),
    /// PROJ refused the projection string.
    Proj(String),
}

impl fmt::Display for SphereError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SphereError::BadRadius(r) => write!(f, "bad sphere radius: {r}"),
            SphereError::Proj(s) => write!(f, "PROJ error: {s}"),
        }
    }
}

impl std::error::Error for SphereError {}

/// A grid of places and their projected points. The vertices go row by row.
/// `None`: PROJ does not project the place.
pub struct Mesh {
    pub cols: usize,
    pub rows: usize,
    pub vertices: Vec<Option<(Point, LonLat)>>,
}

/// A projection on a sphere, with a frame and a clip rectangle. Not `Send`.
pub struct Projection {
    spec: ProjectionSpec,
    radius_m: f64,
    frame: Frame,
    clip: ClipRect,
    pj: proj::Proj,
}

impl Projection {
    /// Builds a projection. The spec is normalized. The radius must be finite and above zero.
    pub fn new(spec: ProjectionSpec, radius_m: f64) -> Result<Self, SphereError> {
        if !radius_m.is_finite() || radius_m <= 0.0 {
            return Err(SphereError::BadRadius(radius_m));
        }
        let spec = spec.normalized();
        let pj = proj::Proj::new(&spec.proj_string(radius_m))
            .map_err(|e| SphereError::Proj(e.to_string()))?;
        let (frame, lat_min, lat_max) = match spec.kind {
            ProjectionKind::Equirectangular | ProjectionKind::EqualEarth => {
                (Frame::Shift { lon0: spec.lon0 }, -90.0, 90.0)
            }
            ProjectionKind::Mercator => (Frame::Shift { lon0: spec.lon0 }, -85.0, 85.0),
            k => {
                let lat_min = match k {
                    ProjectionKind::LambertAzimuthal => -85.0,
                    ProjectionKind::Orthographic => 0.01,
                    _ => -20.0,
                };
                (
                    Frame::Polar {
                        lon0: spec.lon0,
                        lat0: spec.lat0,
                    },
                    lat_min,
                    90.0,
                )
            }
        };
        let clip = ClipRect {
            lon_min: -180.0 + EPS,
            lon_max: 180.0 - EPS,
            lat_min,
            lat_max,
        };
        Ok(Projection {
            spec,
            radius_m,
            frame,
            clip,
            pj,
        })
    }

    /// The normalized spec.
    pub fn spec(&self) -> ProjectionSpec {
        self.spec
    }

    /// The sphere radius in meters.
    pub fn radius_m(&self) -> f64 {
        self.radius_m
    }

    /// The frame of this projection.
    pub fn frame(&self) -> Frame {
        self.frame
    }

    /// The clip rectangle, in frame degrees.
    pub fn clip(&self) -> ClipRect {
        self.clip
    }

    /// True when the frame point is inside the clip rectangle.
    pub fn is_visible(&self, p: LonLat) -> bool {
        let f = self.frame.to_frame(p);
        self.in_clip(f, 0.0)
    }

    fn in_clip(&self, f: LonLat, tol: f64) -> bool {
        let c = &self.clip;
        f[0] >= c.lon_min - tol
            && f[0] <= c.lon_max + tol
            && f[1] >= c.lat_min - tol
            && f[1] <= c.lat_max + tol
    }

    fn forward_raw(&self, p: LonLat) -> Option<Point> {
        let (x, y) = self
            .pj
            .project((p[0].to_radians(), p[1].to_radians()), false)
            .ok()?;
        (x.is_finite() && y.is_finite()).then(|| Point::new(x, y))
    }

    /// Projects a point. `None` when it is not visible or PROJ fails.
    pub fn forward(&self, p: LonLat) -> Option<Point> {
        if !self.is_visible(p) {
            return None;
        }
        self.forward_raw(p)
    }

    /// Unprojects a point. `None` when it is outside the map.
    pub fn inverse(&self, p: Point) -> Option<LonLat> {
        let (lon, lat) = self.pj.project((p.x, p.y), true).ok()?;
        if !lon.is_finite() || !lat.is_finite() {
            return None;
        }
        let ll = [wrap180(lon.to_degrees()), lat.to_degrees()];
        let f = self.frame.to_frame(ll);
        if !self.in_clip(f, 1e-6) {
            return None;
        }
        let back = self.forward_raw(ll)?;
        if (back - p).hypot() > 1e-6 * self.radius_m {
            return None;
        }
        Some(ll)
    }

    fn emit(&self, frame_points: &[LonLat]) -> Vec<Point> {
        frame_points
            .iter()
            .filter_map(|p| self.forward_raw(self.frame.from_frame(*p)))
            .collect()
    }

    /// Open lines in projected meters.
    pub fn project_line(&self, line: &[LonLat]) -> Vec<Vec<Point>> {
        if line.len() < 2 {
            return Vec::new();
        }
        let dense = densify(line, 1.0, false);
        let f: Vec<LonLat> = dense.iter().map(|p| self.frame.to_frame(*p)).collect();
        let u = unwrap(&f);
        let mut out = Vec::new();
        for s in shifts(&u) {
            let shifted: Vec<LonLat> = u.iter().map(|p| [p[0] + s, p[1]]).collect();
            for piece in clip_line_rect(&shifted, &self.clip) {
                let pts = self.emit(&densify(&piece, 1.0, false));
                if pts.len() >= 2 {
                    out.push(pts);
                }
            }
        }
        out
    }

    /// Closed rings in projected meters, ready for an even-odd fill.
    pub fn project_ring(&self, ring: &[LonLat]) -> Vec<Vec<Point>> {
        let mut ring = ring.to_vec();
        if ring.len() >= 2 && ring[0] == ring[ring.len() - 1] {
            ring.pop();
        }
        if ring.len() < 3 {
            return Vec::new();
        }
        // The closing edge takes the short way round. A ring that goes around a pole
        // closes with a jump of about 360 degrees, which is the same point.
        let first = ring[0];
        let last = ring[ring.len() - 1];
        let dl = first[0] - last[0];
        let target = if dl > 180.0 {
            [first[0] - 360.0, first[1]]
        } else if dl < -180.0 {
            [first[0] + 360.0, first[1]]
        } else {
            first
        };
        ring.push(target);
        let mut dense = densify(&ring, 1.0, false);
        dense.pop();
        let f: Vec<LonLat> = dense.iter().map(|p| self.frame.to_frame(*p)).collect();
        let mut u = unwrap(&f);
        let n = u.len();
        let close = wrap180(f[0][0] - f[n - 1][0]);
        let turns = ((u[n - 1][0] + close - u[0][0]) / 360.0).round();
        if turns != 0.0 {
            // The ring goes around a pole of the frame. Close it along that pole.
            let total = turns * 360.0;
            let mut a_north = 0.0; // area between the ring and the north pole, unit sphere
            for i in 0..n {
                let a = u[i];
                let b = if i + 1 < n {
                    u[i + 1]
                } else {
                    [u[0][0] + total, u[0][1]]
                };
                a_north += (b[0] - a[0]).to_radians()
                    * (1.0 - (a[1].to_radians().sin() + b[1].to_radians().sin()) / 2.0);
            }
            // The inside is the smaller of the two parts of the sphere.
            let pole = if a_north.abs() <= 2.0 * std::f64::consts::PI {
                90.0
            } else {
                -90.0
            };
            let first = u[0];
            u.push([first[0] + total, first[1]]);
            u.push([first[0] + total, pole]);
            u.push([first[0], pole]);
        }
        let mut out = Vec::new();
        for s in shifts(&u) {
            let shifted: Vec<LonLat> = u.iter().map(|p| [p[0] + s, p[1]]).collect();
            let c = clip_ring_rect(&shifted, &self.clip);
            if c.len() < 3 {
                continue;
            }
            let pts = self.emit(&densify(&c, 1.0, true));
            if pts.len() >= 3 {
                out.push(pts);
            }
        }
        out
    }

    /// All rings of a polygon, for one even-odd fill.
    pub fn project_polygon(&self, rings: &[Vec<LonLat>]) -> Vec<Vec<Point>> {
        rings.iter().flat_map(|r| self.project_ring(r)).collect()
    }

    /// The strokes of a polygon. Edges that lie on the border of the source are dropped.
    pub fn project_outline_of(&self, rings: &[Vec<LonLat>]) -> Vec<Vec<Point>> {
        let mut out = Vec::new();
        for ring in rings {
            let mut ring = ring.clone();
            if ring.len() >= 2 && ring[0] == ring[ring.len() - 1] {
                ring.pop();
            }
            let n = ring.len();
            if n < 2 {
                continue;
            }
            let keep: Vec<bool> = (0..n)
                .map(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % n]);
                    let lon_border = a[0].abs() >= 180.0 && b[0].abs() >= 180.0 && a[0] == b[0];
                    let lat_border = a[1].abs() >= 90.0 && b[1].abs() >= 90.0;
                    !(lon_border || lat_border)
                })
                .collect();
            let mut runs: Vec<Vec<LonLat>> = Vec::new();
            match keep.iter().position(|k| !k) {
                None => {
                    let mut run = ring.clone();
                    run.push(ring[0]);
                    runs.push(run);
                }
                Some(d) => {
                    let mut cur: Vec<LonLat> = Vec::new();
                    for j in 1..=n {
                        let i = (d + j) % n;
                        if keep[i] {
                            if cur.is_empty() {
                                cur.push(ring[i]);
                            }
                            cur.push(ring[(i + 1) % n]);
                        } else if !cur.is_empty() {
                            runs.push(std::mem::take(&mut cur));
                        }
                    }
                    if !cur.is_empty() {
                        runs.push(cur);
                    }
                }
            }
            for run in runs {
                out.extend(self.project_line(&run));
            }
        }
        out
    }

    /// The closed edge of the projection.
    pub fn outline(&self) -> Vec<Point> {
        let c = &self.clip;
        match self.frame {
            Frame::Shift { .. } => {
                let corners = [
                    [c.lon_min, c.lat_min],
                    [c.lon_max, c.lat_min],
                    [c.lon_max, c.lat_max],
                    [c.lon_min, c.lat_max],
                ];
                self.emit(&densify(&corners, 1.0, true))
            }
            Frame::Polar { .. } => {
                let edge = [[c.lon_min, c.lat_min], [c.lon_max, c.lat_min]];
                self.emit(&densify(&edge, 1.0, false))
            }
        }
    }

    /// A grid of places that covers the whole projection, for a GPU that
    /// warps a raster. The cells are `step` degrees wide and high, or less.
    pub fn mesh(&self, step: f64) -> Mesh {
        let c = &self.clip;
        let count = |min: f64, max: f64| ((max - min) / step).ceil().max(1.0) as usize + 1;
        let (cols, rows) = (count(c.lon_min, c.lon_max), count(c.lat_min, c.lat_max));
        let at = |i: usize, n: usize, min: f64, max: f64| {
            // The last vertex is at the limit with no rounding error.
            if i + 1 == n {
                max
            } else {
                min + (max - min) * i as f64 / (n - 1) as f64
            }
        };
        let mut vertices = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            let lat = at(row, rows, c.lat_min, c.lat_max);
            for col in 0..cols {
                let place = self
                    .frame
                    .from_frame([at(col, cols, c.lon_min, c.lon_max), lat]);
                vertices.push(self.forward_raw(place).map(|point| (point, place)));
            }
        }
        Mesh {
            cols,
            rows,
            vertices,
        }
    }

    /// The bounding box of the outline.
    pub fn bounds(&self) -> Rect {
        let pts = self.outline();
        let mut r = Rect::new(
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in pts {
            r.x0 = r.x0.min(p.x);
            r.y0 = r.y0.min(p.y);
            r.x1 = r.x1.max(p.x);
            r.y1 = r.y1.max(p.y);
        }
        if r.x0 > r.x1 {
            return Rect::ZERO;
        }
        r
    }

    /// Projected meters per ground meter, east-west, at `p`.
    pub fn local_scale(&self, p: LonLat) -> Option<f64> {
        let cos = p[1].to_radians().cos();
        if cos < 1e-6 {
            return None;
        }
        let a = self.forward(p)?;
        let b = self.forward([p[0] + 0.01, p[1]])?;
        Some((b - a).hypot() / (self.radius_m * cos * 0.01f64.to_radians()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mesh_covers_the_projection() {
        for kind in ProjectionKind::ALL {
            let p = proj(kind, 30.0, 40.0);
            let mesh = p.mesh(5.0);
            assert_eq!(mesh.vertices.len(), mesh.cols * mesh.rows);
            let bounds = p.bounds().inflate(1.0, 1.0);
            let mut covered = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for vertex in &mesh.vertices {
                let (point, place) = vertex.unwrap_or_else(|| panic!("{kind:?}"));
                assert!(bounds.contains(point), "{kind:?}: {point:?}");
                covered = covered.union_pt(point);
                // A place inside the edge projects to the same point.
                if let Some(same) = p.forward(place) {
                    assert!((same - point).hypot() < 1e-6 * p.radius_m(), "{kind:?}");
                }
            }
            let all = p.bounds();
            assert!((covered.width() - all.width()).abs() < 0.02 * all.width());
            assert!((covered.height() - all.height()).abs() < 0.02 * all.height());
        }
    }

    const EE: f64 = 2.70663;

    fn spec(kind: ProjectionKind, lon0: f64, lat0: f64) -> ProjectionSpec {
        ProjectionSpec { kind, lon0, lat0 }
    }

    fn proj(kind: ProjectionKind, lon0: f64, lat0: f64) -> Projection {
        Projection::new(spec(kind, lon0, lat0), 1.0).unwrap()
    }

    fn area(rings: &[Vec<Point>]) -> f64 {
        rings
            .iter()
            .map(|r| {
                let n = r.len();
                (0..n)
                    .map(|i| r[i].x * r[(i + 1) % n].y - r[(i + 1) % n].x * r[i].y)
                    .sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    fn square(l0: f64, b0: f64, l1: f64, b1: f64) -> Vec<LonLat> {
        vec![[l0, b0], [l1, b0], [l1, b1], [l0, b1]]
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b
    }

    #[test]
    fn equal_earth_basics() {
        let p = proj(ProjectionKind::EqualEarth, 0.0, 0.0);
        let e = p.forward([179.9999, 0.0]).unwrap();
        assert!((e.x - EE).abs() < 1e-4);
        let o = p.forward([0.0, 0.0]).unwrap();
        assert!(o.x.abs() < 1e-12 && o.y.abs() < 1e-12);
        let p2 = Projection::new(spec(ProjectionKind::EqualEarth, 0.0, 0.0), 2.0).unwrap();
        let e2 = p2.forward([179.9999, 0.0]).unwrap();
        assert!((e2.x - 2.0 * e.x).abs() < 1e-9);
    }

    #[test]
    fn orthographic_basics() {
        let p = proj(ProjectionKind::Orthographic, 20.0, 30.0);
        let c = p.forward([20.0, 30.0]).unwrap();
        assert!(c.x.abs() < 1e-9 && c.y.abs() < 1e-9);
        assert!(p.forward([-160.0, -30.0]).is_none());
        assert!(p.forward([20.0, -58.0]).is_some());
    }

    #[test]
    fn mercator_basics() {
        let p = proj(ProjectionKind::Mercator, 0.0, 0.0);
        assert!(p.forward([0.0, 86.0]).is_none());
        assert!((p.local_scale([0.0, 0.0]).unwrap() - 1.0).abs() < 1e-3);
        assert!((p.local_scale([0.0, 60.0]).unwrap() - 2.0).abs() < 1e-2);
    }

    #[test]
    fn every_preset_projects_its_center() {
        let kinds = [
            ProjectionKind::EqualEarth,
            ProjectionKind::Mercator,
            ProjectionKind::LambertAzimuthal,
            ProjectionKind::Orthographic,
            ProjectionKind::Stereographic,
        ];
        for kind in kinds {
            for lon0 in [-170.0, -73.5, 0.0, 15.0, 16.0, 90.0, 179.0] {
                for lat0 in [-60.0, -20.0, 0.0, 10.0, 30.0, 52.0, 70.0] {
                    let p = proj(kind, lon0, lat0);
                    let s = p.spec();
                    let c = [s.lon0, s.lat0];
                    let q = p
                        .forward(c)
                        .unwrap_or_else(|| panic!("{kind:?} {lon0},{lat0} center not visible"));
                    assert!(q.x.is_finite() && q.y.is_finite());
                    assert!(q.to_vec2().hypot() < 1e-3 * p.radius_m(), "{kind:?} {q:?}");
                }
            }
        }
    }

    #[test]
    fn antipode_stays_hidden() {
        for kind in [
            ProjectionKind::LambertAzimuthal,
            ProjectionKind::Orthographic,
            ProjectionKind::Stereographic,
        ] {
            let p = proj(kind, 15.0, 52.0);
            assert!(p.forward([-165.0, -52.0]).is_none(), "{kind:?}");
        }
    }

    #[test]
    fn bad_radius() {
        for r in [0.0, f64::NAN, -1.0] {
            assert!(matches!(
                Projection::new(ProjectionSpec::default(), r),
                Err(SphereError::BadRadius(_))
            ));
        }
    }

    #[test]
    fn inverse_round_trip() {
        for kind in ProjectionKind::ALL {
            let p = proj(kind, 20.0, 30.0);
            let mut found = 0;
            let mut i = 0;
            while found < 100 && i < 100000 {
                // a deterministic spread of points
                let lon = wrap180(i as f64 * 37.3);
                let lat = ((i as f64 * 11.7) % 150.0) - 75.0;
                i += 1;
                if let Some(pt) = p.forward([lon, lat]) {
                    let back = p
                        .inverse(pt)
                        .unwrap_or_else(|| panic!("{kind:?} {lon} {lat}"));
                    assert!(wrap180(back[0] - lon).abs() < 1e-6, "{kind:?}");
                    assert!((back[1] - lat).abs() < 1e-6, "{kind:?}");
                    found += 1;
                }
            }
            assert_eq!(found, 100, "{kind:?}");
        }
    }

    #[test]
    fn inverse_outside() {
        for kind in ProjectionKind::ALL {
            let p = proj(kind, 20.0, 30.0);
            let b = p.bounds();
            let cy = (b.y0 + b.y1) / 2.0;
            let cx = (b.x0 + b.x1) / 2.0;
            assert!(
                p.inverse(Point::new(b.x1 + 0.2 * b.width(), cy)).is_none(),
                "{kind:?}"
            );
            assert!(
                p.inverse(Point::new(b.x0 - 0.2 * b.width(), cy)).is_none(),
                "{kind:?}"
            );
            assert!(
                p.inverse(Point::new(cx, b.y1 + 0.2 * b.height())).is_none(),
                "{kind:?}"
            );
            assert!(
                p.inverse(Point::new(cx, b.y0 - 0.2 * b.height())).is_none(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn small_square_area() {
        let p = proj(ProjectionKind::EqualEarth, 0.0, 0.0);
        let truth = 10f64.to_radians() * 10f64.to_radians().sin();
        let rings = p.project_ring(&square(0.0, 0.0, 10.0, 10.0));
        assert_eq!(rings.len(), 1);
        assert!(rel(area(&rings), truth) < 0.01);
    }

    #[test]
    fn square_over_the_cut() {
        let truth = 10f64.to_radians() * 20f64.to_radians() / 1.0 * 0.0
            + 20f64.to_radians() * 10f64.to_radians().sin();
        let sq = square(170.0, 0.0, 190.0, 10.0);
        let a = proj(ProjectionKind::EqualEarth, 0.0, 0.0).project_ring(&sq);
        assert_eq!(a.len(), 2);
        assert!(rel(area(&a), truth) < 0.01);
        let b = proj(ProjectionKind::EqualEarth, 180.0, 0.0).project_ring(&sq);
        assert_eq!(b.len(), 1);
        assert!(rel(area(&b), truth) < 0.01);
    }

    fn polar_cap() -> Vec<LonLat> {
        (0..36).map(|i| [-180.0 + i as f64 * 10.0, 80.0]).collect()
    }

    #[test]
    fn pole_ring() {
        let truth = 2.0 * std::f64::consts::PI * (1.0 - 80f64.to_radians().sin());
        let p = proj(ProjectionKind::EqualEarth, 0.0, 0.0);
        let rings = p.project_ring(&polar_cap());
        assert_eq!(rings.len(), 1);
        assert!(
            rel(area(&rings), truth) < 0.02,
            "{} {}",
            area(&rings),
            truth
        );
        let xs: Vec<f64> = rings[0].iter().map(|q| q.x).collect();
        assert!(xs.iter().cloned().fold(f64::INFINITY, f64::min) < -0.5);
        assert!(xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) > 0.5);

        let o = proj(ProjectionKind::Orthographic, 0.0, 90.0).project_ring(&polar_cap());
        assert_eq!(o.len(), 1);
        // contains the origin (checked next to it): crossing count
        let r = &o[0];
        let mut inside = false;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            // a point next to the origin, because the pole closure touches the origin
            let (ty, tx) = (0.01, 0.01);
            if (a.y > ty) != (b.y > ty) && a.x + (ty - a.y) / (b.y - a.y) * (b.x - a.x) > tx {
                inside = !inside;
            }
        }
        assert!(inside);
        let s = proj(ProjectionKind::Orthographic, 0.0, -90.0).project_ring(&polar_cap());
        assert!(s.is_empty());
    }

    #[test]
    fn ortho_edge_square() {
        let p = proj(ProjectionKind::Orthographic, 0.0, 0.0);
        let rings = p.project_ring(&square(60.0, -10.0, 120.0, 10.0));
        assert_eq!(rings.len(), 1);
        let d: Vec<f64> = rings[0].iter().map(|q| q.to_vec2().hypot()).collect();
        assert!(d.iter().all(|x| *x <= 1.0 + 1e-6));
        assert!(d.iter().any(|x| *x > 0.9999));
    }

    #[test]
    fn line_projection() {
        let p = proj(ProjectionKind::EqualEarth, 90.0, 0.0);
        let pieces = p.project_line(&[[-180.0, 0.0], [180.0, 0.0]]);
        let len: f64 = pieces
            .iter()
            .map(|l| l.windows(2).map(|w| (w[1] - w[0]).hypot()).sum::<f64>())
            .sum();
        assert!((len - 2.0 * EE).abs() / (2.0 * EE) < 1e-3, "{len}");
        let o = proj(ProjectionKind::Orthographic, 0.0, 0.0);
        let m = o.project_line(&[[30.0, -90.0], [30.0, 90.0]]);
        assert_eq!(m.len(), 1);
        assert!(m[0].iter().all(|q| q.to_vec2().hypot() <= 1.0 + 1e-6));
    }

    #[test]
    fn outlines() {
        let b = proj(ProjectionKind::EqualEarth, 0.0, 0.0).bounds();
        assert!((b.x0 + EE).abs() < 1e-3 && (b.x1 - EE).abs() < 1e-3);
        assert!((b.y0 + 1.31736).abs() < 1e-3 && (b.y1 - 1.31736).abs() < 1e-3);
        let o = proj(ProjectionKind::Orthographic, 20.0, 30.0);
        let want = 0.01f64.to_radians().cos();
        for q in o.outline() {
            assert!((q.to_vec2().hypot() - want).abs() < 1e-6);
        }
    }
}
