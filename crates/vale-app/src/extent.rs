//! The extent of a map: the part of the world that the frames of the map
//! show. The globe draws its outline and darkens the ground outside it.

use std::sync::Arc;
use std::sync::mpsc;

use kurbo::{ParamCurveNearest, Point};
use vale_sphere::{Projection, ProjectionSpec};
use vale_store::{FrameRecord, MapId, Project};

use crate::atlas::{FrameTarget, contains, frame_transform};
use crate::globe::math::{V3, lonlat_to_dir};
use crate::view::ViewTransform;
pub use vale_globe::extent::{GpuExtent, TABLE_HEIGHT, TABLE_WIDTH, Table};

/// The number of points on the rings of all frames of a map.
const FRAME_SAMPLES: f64 = 1024.0;
/// The steps of a bisection at the end of a run.
const BISECT: usize = 30;

/// The outline of an extent, as open runs of points on the sphere.
pub type Outline = Arc<Vec<Vec<V3>>>;

/// One frame of a map: its view and its rings, in page millimeters.
struct Place {
    view: ViewTransform,
    rings: Vec<Vec<Point>>,
    shape: vale_store::FrameShape,
}

/// The frames of a map in its projection.
pub struct Frames<'a> {
    projection: &'a Projection,
    places: Vec<Place>,
}

impl<'a> Frames<'a> {
    /// The frames of `map` on all pages. `projection` is the projection of
    /// the map at the radius of the world.
    pub fn new(project: &Project, map: MapId, projection: &'a Projection) -> Frames<'a> {
        let frames = project.atlas.pages.iter().flat_map(|page| &page.frames);
        let places = frames
            .filter(|frame| frame.map == map)
            .map(|frame| place(projection, frame))
            .collect();
        Frames { projection, places }
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// True if a frame other than `skip` shows a projected point.
    fn shows(&self, p: Point, skip: Option<usize>) -> bool {
        let others = self
            .places
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != skip);
        others
            .into_iter()
            .any(|(_, place)| contains(&place.shape, place.view.to_page(p)))
    }

    /// The outline of the extent, as open runs of points on the sphere.
    pub fn outline(&self) -> Vec<Vec<V3>> {
        let mut out = Vec::new();
        let perimeter: f64 = self
            .places
            .iter()
            .map(|place| perimeter(&place.rings))
            .sum();
        let step = perimeter / FRAME_SAMPLES;
        let projection = self.projection;
        for (i, place) in self.places.iter().enumerate() {
            let probe = |q: Point| {
                let p = place.view.from_page(q);
                let ll = projection.inverse(p)?;
                (!self.shows(p, Some(i))).then(|| lonlat_to_dir(ll[0], ll[1]))
            };
            for ring in &place.rings {
                out.extend(runs(&densify(ring, step), true, probe));
            }
        }
        let edge = projection.outline();
        let r = projection.radius_m();
        let probe = |p: Point| {
            if !self.shows(p, None) {
                return None;
            }
            let (inner, across) = self.across(p, &edge, r)?;
            (!self.covered(across)).then_some(inner)
        };
        let step = 0.25_f64.to_radians() * r;
        out.extend(runs(&densify(&edge, step), true, probe));
        out
    }

    /// For a point on the edge of the map: the direction of the point just
    /// inside the map, and the direction just across the edge on the
    /// sphere. `None` if no side of the point is inside the map.
    fn across(&self, p: Point, edge: &[Point], r: f64) -> Option<(V3, V3)> {
        let normal = edge_normal(edge, p)?;
        let dir = |q: Point| {
            let ll = self.projection.inverse(q)?;
            Some(lonlat_to_dir(ll[0], ll[1]))
        };
        let (near, far) = (1e-7 * r, 1e-4 * r);
        let (sign, inner) = [1.0, -1.0]
            .into_iter()
            .find_map(|s| Some((s, dir(p + normal * (s * near))?)))?;
        let inside = dir(p + normal * (sign * far))?;
        let across = std::array::from_fn(|k| 2.0 * inner[k] - inside[k]);
        Some((inner, normalize(across)))
    }

    /// True if a frame shows a direction.
    fn covered(&self, d: V3) -> bool {
        let (lon, lat) = crate::globe::math::dir_to_lonlat(d);
        self.projection
            .forward([lon, lat])
            .is_some_and(|p| self.shows(p, None))
    }

    /// The data of the frames for the shader of the extent: one texel of
    /// counts, then for each frame its header and the edges of its rings.
    /// The header has the affine map from unit projected space to the page,
    /// the range of its edges, and the box of its rings. An edge is two
    /// page points.
    pub fn gpu_data(&self) -> Vec<[f32; 4]> {
        let r = self.projection.radius_m();
        let headers = 1 + 3 * self.places.len();
        let mut out = vec![[self.places.len() as f32, 0.0, 0.0, 0.0]];
        let mut edges = Vec::new();
        for place in &self.places {
            let o = place.view.to_page(Point::ZERO);
            let ex = place.view.to_page(Point::new(r, 0.0)) - o;
            let ey = place.view.to_page(Point::new(0.0, r)) - o;
            let start = headers + edges.len();
            let mut bbox = kurbo::Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for ring in &place.rings {
                for (k, a) in ring.iter().enumerate() {
                    let b = ring[(k + 1) % ring.len()];
                    edges.push([a.x as f32, a.y as f32, b.x as f32, b.y as f32]);
                    bbox = bbox.union_pt(*a);
                }
            }
            let count = headers + edges.len() - start;
            out.push([o.x as f32, o.y as f32, ex.x as f32, ex.y as f32]);
            out.push([ey.x as f32, ey.y as f32, start as f32, count as f32]);
            let b = [bbox.x0, bbox.y0, bbox.x1, bbox.y1];
            out.push(b.map(|v| v as f32));
        }
        out.extend(edges);
        out
    }
}

fn place(projection: &Projection, frame: &FrameRecord) -> Place {
    let view = frame_transform(projection, frame, FrameTarget::MM);
    let rings = frame.shape.rings();
    let rings = rings
        .iter()
        .map(|ring| ring.iter().map(|&[x, y]| Point::new(x, y)).collect())
        .collect();
    Place {
        view,
        rings,
        shape: frame.shape.clone(),
    }
}

fn perimeter(rings: &[Vec<Point>]) -> f64 {
    let edges = rings.iter().flat_map(|ring| {
        (0..ring.len()).map(move |k| (ring[(k + 1) % ring.len()] - ring[k]).hypot())
    });
    edges.sum()
}

/// A closed ring with no edge longer than `step`.
fn densify(ring: &[Point], step: f64) -> Vec<Point> {
    let mut out = Vec::new();
    for (k, &a) in ring.iter().enumerate() {
        let b = ring[(k + 1) % ring.len()];
        let n = ((b - a).hypot() / step).ceil().max(1.0) as usize;
        out.extend((0..n).map(|i| a.lerp(b, i as f64 / n as f64)));
    }
    out
}

/// The unit normal of the edge of the map near `p`, from its nearest edge.
fn edge_normal(edge: &[Point], p: Point) -> Option<kurbo::Vec2> {
    let n = edge.len();
    let distance = |k: usize| {
        kurbo::Line::new(edge[k], edge[(k + 1) % n])
            .nearest(p, 0.0)
            .distance_sq
    };
    let k = (0..n).min_by(|&a, &b| distance(a).total_cmp(&distance(b)))?;
    let along = edge[(k + 1) % n] - edge[k];
    let length = along.hypot();
    (length > 0.0).then(|| kurbo::Vec2::new(-along.y, along.x) / length)
}

fn normalize(v: V3) -> V3 {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / length)
}

/// The runs of a ring of points where `probe` gives a direction, with the
/// ends of each run found by bisection. A closed ring that is all kept is
/// one run that ends at its start.
fn runs(points: &[Point], closed: bool, probe: impl Fn(Point) -> Option<V3>) -> Vec<Vec<V3>> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let kept: Vec<Option<V3>> = points.iter().map(|&p| probe(p)).collect();
    // The last point of `a` to `b` that the probe keeps, where it keeps `a`
    // and not `b`.
    let end = |a: Point, b: Point| {
        let (mut lo, mut hi) = (0.0, 1.0);
        let mut best = None;
        for _ in 0..BISECT {
            let t = (lo + hi) / 2.0;
            match probe(a.lerp(b, t)) {
                Some(d) => (lo, best) = (t, Some(d)),
                None => hi = t,
            }
        }
        best
    };
    if kept.iter().all(Option::is_some) {
        let mut run: Vec<V3> = kept.into_iter().flatten().collect();
        if closed {
            run.push(run[0]);
        }
        return vec![run];
    }
    // Start at a point that the probe does not keep, so no run wraps.
    let start = kept
        .iter()
        .position(Option::is_none)
        .expect("one point is not kept");
    let count = if closed { n } else { n - start };
    let mut out = Vec::new();
    let mut run: Vec<V3> = Vec::new();
    for s in 1..=count {
        let (k, prev) = ((start + s) % n, (start + s - 1) % n);
        if s == count && !closed {
            break;
        }
        match (kept[prev], kept[k]) {
            (None, Some(d)) => {
                run.extend(end(points[k], points[prev]));
                run.push(d);
            }
            (Some(_), Some(d)) => run.push(d),
            (Some(_), None) => {
                run.extend(end(points[prev], points[k]));
                out.push(std::mem::take(&mut run));
            }
            (None, None) => {}
        }
    }
    if !run.is_empty() {
        out.push(run);
    }
    out.retain(|run| run.len() > 1);
    out
}

/// The extent that the globe shows, with its outline and the table of its
/// projection. A worker makes the table of a new projection.
#[derive(Default)]
pub struct ExtentCache {
    /// The map, the frame data of the outline, and the outline.
    outline: Option<(MapId, Vec<[f32; 4]>, Outline)>,
    table: Option<Arc<Table>>,
    pending: Option<(ProjectionSpec, mpsc::Receiver<Option<Table>>)>,
}

impl ExtentCache {
    /// Updates the extent of `map`, and gives its outline and its data for
    /// the GPU. The data is `None` while the worker makes the table.
    /// `wait`: make the table on this thread.
    pub fn update(
        &mut self,
        project: &Project,
        map: MapId,
        projection: &Projection,
        wait: bool,
    ) -> (Outline, Option<GpuExtent>) {
        let frames = Frames::new(project, map, projection);
        let data = frames.gpu_data();
        let outline = match &self.outline {
            Some((m, d, outline)) if *m == map && *d == data => outline.clone(),
            _ => {
                let outline = Arc::new(frames.outline());
                self.outline = Some((map, data.clone(), outline.clone()));
                outline
            }
        };
        let gpu = (!frames.is_empty())
            .then(|| self.table(projection.spec(), wait))
            .flatten()
            .map(|table| GpuExtent {
                table,
                frames: Arc::new(data),
            });
        (outline, gpu)
    }

    fn table(&mut self, spec: ProjectionSpec, wait: bool) -> Option<Arc<Table>> {
        if let Some(table) = &self.table
            && table.spec == spec
        {
            return Some(table.clone());
        }
        if wait {
            self.pending = None;
            self.table = Table::new(spec).map(Arc::new);
            return self.table.clone();
        }
        match &self.pending {
            Some((of, receiver)) if *of == spec => {
                if let Ok(table) = receiver.try_recv() {
                    self.pending = None;
                    self.table = table.map(Arc::new);
                    return self.table.clone();
                }
            }
            _ => {
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || sender.send(Table::new(spec)));
                self.pending = Some((spec, receiver));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::math::{angle, dir_to_lonlat};
    use vale_store::FrameShape;

    const R: f64 = 6_371_000.0;

    fn project_with(proj: &str, frame: impl Fn(&mut FrameRecord)) -> (Project, MapId) {
        let mut project = Project::new(vale_store::World::default());
        let map = project.atlas.maps[0].id;
        project.atlas.maps[0].projection = proj.to_string();
        let spec = ProjectionSpec::from_proj(proj).unwrap();
        let projection = Projection::new(spec, R).unwrap();
        let page = &project.atlas.pages[0];
        let mut record = crate::atlas::new_frame(page, &project.atlas.maps[0], &projection);
        frame(&mut record);
        project.atlas.pages[0].frames = vec![record];
        (project, map)
    }

    fn projection(project: &Project, map: MapId) -> Projection {
        let text = &project.atlas.map(map).unwrap().projection;
        Projection::new(ProjectionSpec::from_proj(text).unwrap(), R).unwrap()
    }

    /// The smallest angle from a direction to the outline, in degrees.
    fn distance(outline: &[Vec<V3>], d: V3) -> f64 {
        let points = outline.iter().flatten();
        points
            .map(|&p| angle(p, d))
            .fold(f64::MAX, f64::min)
            .to_degrees()
    }

    fn regional(frame: &mut FrameRecord, center: [f64; 2], scale: f64) {
        frame.shape = FrameShape::Rectangle([20.0, 20.0, 180.0, 120.0]);
        frame.center = center;
        frame.scale = scale;
    }

    #[test]
    fn corners_of_a_frame_are_on_the_outline() {
        let (project, map) = project_with("+proj=eqearth +lon_0=0", |f| {
            regional(f, [10.0, 45.0], 20_000_000.0)
        });
        let projection = projection(&project, map);
        let frames = Frames::new(&project, map, &projection);
        let outline = frames.outline();
        assert_eq!(outline.len(), 1);
        let frame = &project.atlas.pages[0].frames[0];
        let view = frame_transform(&projection, frame, FrameTarget::MM);
        for corner in [[20.0, 20.0], [180.0, 20.0], [180.0, 120.0], [20.0, 120.0]] {
            let ll = projection
                .inverse(view.from_page(Point::new(corner[0], corner[1])))
                .unwrap();
            assert!(distance(&outline, lonlat_to_dir(ll[0], ll[1])) < 0.05);
        }
        let run = &outline[0];
        assert!(
            angle(run[0], *run.last().unwrap()) < 1e-9,
            "the outline closes"
        );
    }

    #[test]
    fn outline_crosses_the_antimeridian() {
        let (project, map) = project_with("+proj=eqearth +lon_0=180", |f| {
            regional(f, [180.0, -15.0], 20_000_000.0)
        });
        let projection = projection(&project, map);
        let outline = Frames::new(&project, map, &projection).outline();
        assert_eq!(outline.len(), 1);
        let lons: Vec<f64> = outline[0].iter().map(|&d| dir_to_lonlat(d).0).collect();
        assert!(lons.iter().any(|&lon| lon > 170.0));
        assert!(lons.iter().any(|&lon| lon < -170.0));
    }

    #[test]
    fn outline_goes_around_a_pole() {
        let (project, map) = project_with("+proj=laea +lat_0=90 +lon_0=0", |f| {
            regional(f, [0.0, 90.0], 30_000_000.0)
        });
        let projection = projection(&project, map);
        let outline = Frames::new(&project, map, &projection).outline();
        assert_eq!(outline.len(), 1);
        let lats = outline[0].iter().map(|&d| dir_to_lonlat(d).1);
        assert!(lats.clone().all(|lat| lat > 50.0 && lat < 89.0));
        let lons: Vec<f64> = outline[0].iter().map(|&d| dir_to_lonlat(d).0).collect();
        for quarter in [-135.0, -45.0, 45.0, 135.0] {
            assert!(lons.iter().any(|lon| (lon - quarter).abs() < 10.0));
        }
    }

    #[test]
    fn world_map_drops_the_seam() {
        let (project, map) = project_with("+proj=eqc +lon_0=0", |f| {
            // One millimeter is 1.2 degrees, so the frame shows all
            // longitudes, and the latitudes from -60 to 60.
            f.shape = FrameShape::Rectangle([0.0, 0.0, 400.0, 100.0]);
            f.center = [0.0, 0.0];
            f.scale = 1000.0 * 1.2_f64.to_radians() * R;
        });
        let projection = projection(&project, map);
        let outline = Frames::new(&project, map, &projection).outline();
        assert!(!outline.is_empty());
        let points = || outline.iter().flatten().map(|&d| dir_to_lonlat(d));
        for (lon, lat) in points() {
            assert!((lat.abs() - 60.0).abs() < 0.01, "{lon} {lat}");
        }
        assert!(points().any(|(lon, _)| lon > 179.0));
        assert!(points().any(|(lon, _)| lon < -179.0));
    }

    #[test]
    fn frame_changes_change_the_outline() {
        let base = |f: &mut FrameRecord| regional(f, [10.0, 45.0], 20_000_000.0);
        let outline = |proj: &str, change: &dyn Fn(&mut FrameRecord)| {
            let (project, map) = project_with(proj, |f| {
                base(f);
                change(f);
            });
            let projection = projection(&project, map);
            Frames::new(&project, map, &projection).outline()
        };
        let proj = "+proj=eqearth +lon_0=0";
        let before = outline(proj, &|_| {});
        let changes: [&dyn Fn(&mut FrameRecord); 4] = [
            &|f| f.center = [20.0, 45.0],
            &|f| f.scale = 10_000_000.0,
            &|f| f.rotation = 30.0,
            &|f| f.shape = FrameShape::Rectangle([20.0, 20.0, 120.0, 120.0]),
        ];
        for change in changes {
            assert_ne!(outline(proj, change), before);
        }
        assert_ne!(outline("+proj=ortho +lat_0=45 +lon_0=10", &|_| {}), before);
    }

    #[test]
    fn gpu_data_maps_the_page() {
        let (project, map) = project_with("+proj=eqearth +lon_0=0", |f| {
            regional(f, [10.0, 45.0], 20_000_000.0)
        });
        let projection = projection(&project, map);
        let data = Frames::new(&project, map, &projection).gpu_data();
        assert_eq!(data[0][0], 1.0);
        let [ox, oy, exx, exy] = data[1];
        let [eyx, eyy, start, count] = data[2];
        assert_eq!((start, count), (4.0, 4.0));
        let p = projection.forward([10.0, 45.0]).unwrap();
        let (x, y) = ((p.x / R) as f32, (p.y / R) as f32);
        let page = [ox + exx * x + eyx * y, oy + exy * x + eyy * y];
        assert!((page[0] - 100.0).abs() < 0.01 && (page[1] - 70.0).abs() < 0.01);
    }
}
