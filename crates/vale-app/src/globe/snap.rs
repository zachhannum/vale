//! Snapping and tracing. A snap moves a position to the nearest vertex or
//! edge of a vector feature.
//!
//! Distances in points are on the screen. `scale` is the number of points in
//! one radian of the sphere, so a snap has the same reach at each zoom.

use std::collections::{HashMap, HashSet};
use std::f64::consts::FRAC_PI_2;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::RangeInclusive;
use std::sync::Arc;

use vale_store::{Geometry, GeometryKind, Layer, LayerId, LonLat, Uuid};
use vale_terrain::{face_of, warp};

use super::line::chains;
use super::math::{V3, angle, cross, dot, length, lonlat_to_dir, normalize, scale};

/// The snap distance of a new project, in points.
pub const DEFAULT_DISTANCE: f64 = 12.0;
/// The range of the snap distance, in points.
pub const DISTANCE_RANGE: RangeInclusive<f64> = 4.0..=32.0;
/// The number of cells along each side of a cube face in the index.
const CELLS: usize = 64;
/// The angle of a cell side at the center of a face.
const CELL: f64 = FRAC_PI_2 / CELLS as f64;
/// The spacing of the samples of an edge and of a query in the index.
const STEP: f64 = CELL / 4.0;
/// The largest angle across a cell, with a margin.
const CELL_SPAN: f64 = 2.0 * CELL;
/// A query wider than this reads each edge.
const WIDE: f64 = 0.5;
/// An edge shorter than this has no length.
const ZERO: f64 = 1e-12;

/// The snap settings of a project.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapSettings {
    /// The control that turns all snapping on and off.
    pub on: bool,
    /// Snap to the vertices of features.
    pub vertices: bool,
    /// Snap to the edges of features, and trace them.
    pub edges: bool,
    /// The reach of a snap, in points.
    pub distance: f64,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings {
            on: true,
            vertices: true,
            edges: true,
            distance: DEFAULT_DISTANCE,
        }
    }
}

/// What a position snaps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Vertex,
    Edge,
}

/// A snapped position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// The position, on the unit sphere.
    pub dir: V3,
    pub kind: Kind,
    /// The stored position of a vertex.
    pub exact: Option<LonLat>,
    pub layer: LayerId,
    pub feature: Uuid,
}

/// The vertices and edges of the features of one vector layer, with an
/// index of the edges on the cells of the cube faces.
pub struct LayerTargets {
    layer: LayerId,
    line: bool,
    paths: Vec<TargetPath>,
    /// The target edges, as a path and an edge of that path.
    edges: Vec<(u32, u32)>,
    /// The target edges in each cell.
    cells: HashMap<u32, Vec<u32>>,
    /// The paths of one vertex in each cell. A point is such a path.
    points: HashMap<u32, Vec<u32>>,
}

/// A point, a line, or a polygon ring. A line is a chain of the stored
/// parts.
pub(super) struct TargetPath {
    pub(super) uuid: Uuid,
    /// The stored positions. A seam at 180 degrees and a pole keep both of
    /// their vertices.
    pub(super) ll: Vec<LonLat>,
    pub(super) dirs: Vec<V3>,
    /// The arc length at each vertex. A ring has one more, its full length.
    pub(super) cum: Vec<f64>,
}

impl TargetPath {
    fn new(uuid: Uuid, closed: bool, ll: Vec<LonLat>) -> Self {
        let dirs: Vec<V3> = ll.iter().map(|p| lonlat_to_dir(p[0], p[1])).collect();
        let mut cum = vec![0.0];
        let n = dirs.len();
        let m = if n < 2 {
            0
        } else if closed {
            n
        } else {
            n - 1
        };
        for k in 0..m {
            cum.push(cum[k] + angle(dirs[k], dirs[(k + 1) % n]));
        }
        TargetPath {
            uuid,
            ll,
            dirs,
            cum,
        }
    }

    pub(super) fn edges(&self) -> usize {
        self.cum.len() - 1
    }

    pub(super) fn ends(&self, k: usize) -> (V3, V3) {
        (self.dirs[k], self.dirs[(k + 1) % self.dirs.len()])
    }

    pub(super) fn len(&self, k: usize) -> f64 {
        self.cum[k + 1] - self.cum[k]
    }

    /// An edge along the cut of the stored map: on the 180 degree meridian,
    /// or along a pole. It is not a target, and a trace stops at it.
    pub(super) fn cut(&self, k: usize) -> bool {
        let (a, b) = (self.ll[k], self.ll[(k + 1) % self.ll.len()]);
        let seam = a[0].abs() == 180.0 && b[0].abs() == 180.0;
        let pole = a[1].abs() == 90.0 && a[1] == b[1];
        self.len(k) > ZERO && (seam || pole)
    }

    fn target(&self, k: usize) -> bool {
        self.len(k) > ZERO && !self.cut(k)
    }
}

impl LayerTargets {
    /// The targets of the features of a vector layer.
    pub fn build(id: LayerId, layer: &Layer) -> Self {
        let mut paths = Vec::new();
        for f in &layer.features {
            let uuid = f.uuid();
            match &f.geometry {
                Geometry::Points(points) => {
                    for &p in points {
                        paths.push(TargetPath::new(uuid, false, vec![p]));
                    }
                }
                Geometry::Lines(parts) => {
                    for chain in chains(parts) {
                        let ll = chain.iter().flat_map(|&i| parts[i].clone()).collect();
                        paths.push(TargetPath::new(uuid, false, ll));
                    }
                }
                Geometry::Polygons(polygons) => {
                    for ring in polygons.iter().flatten() {
                        let mut ll = ring.clone();
                        if ll.len() > 1 && ll.first() == ll.last() {
                            ll.pop();
                        }
                        if !ll.is_empty() {
                            paths.push(TargetPath::new(uuid, true, ll));
                        }
                    }
                }
            }
        }
        let mut edges = Vec::new();
        let mut cells: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut points: HashMap<u32, Vec<u32>> = HashMap::new();
        for (i, path) in paths.iter().enumerate() {
            if path.dirs.len() == 1 {
                points.entry(cell(path.dirs[0])).or_default().push(i as u32);
            }
            for k in (0..path.edges()).filter(|&k| path.target(k)) {
                let id = edges.len() as u32;
                edges.push((i as u32, k as u32));
                let (a, b) = path.ends(k);
                let n = (path.len(k) / STEP).ceil().max(1.0) as usize;
                let mut seen = HashSet::new();
                for j in 0..=n {
                    let c = cell(slerp(a, b, j as f64 / n as f64));
                    if seen.insert(c) {
                        cells.entry(c).or_default().push(id);
                    }
                }
            }
        }
        LayerTargets {
            layer: id,
            line: layer.geometry() == Some(GeometryKind::Line),
            paths,
            edges,
            cells,
            points,
        }
    }

    /// A hash of the features of a layer. The targets of a layer with the
    /// same hash are the same.
    pub fn fingerprint(layer: &Layer) -> u64 {
        let mut h = DefaultHasher::new();
        layer.id.hash(&mut h);
        layer.kind.hash(&mut h);
        for f in &layer.features {
            f.uuid().hash(&mut h);
            let mut add = |parts: &[LonLat]| {
                parts.len().hash(&mut h);
                for p in parts {
                    p[0].to_bits().hash(&mut h);
                    p[1].to_bits().hash(&mut h);
                }
            };
            match &f.geometry {
                Geometry::Points(points) => add(points),
                Geometry::Lines(parts) => parts.iter().for_each(|p| add(p)),
                Geometry::Polygons(polygons) => polygons.iter().flatten().for_each(|r| add(r)),
            }
        }
        h.finish()
    }

    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// The target edges and the points that can be within `reach` of `p`.
    fn near(&self, p: V3, reach: f64) -> (Vec<u32>, Vec<u32>) {
        let Some(cells) = cells_near(p, reach) else {
            let points = (0..self.paths.len() as u32)
                .filter(|&i| self.paths[i as usize].dirs.len() == 1)
                .collect();
            return ((0..self.edges.len() as u32).collect(), points);
        };
        let mut edges = HashSet::new();
        let mut points = Vec::new();
        for c in cells {
            edges.extend(self.cells.get(&c).into_iter().flatten().copied());
            points.extend(self.points.get(&c).into_iter().flatten().copied());
        }
        (edges.into_iter().collect(), points)
    }
}

/// A vertex or a point on an edge of a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hit {
    /// The index of the layer in the targets.
    pub(super) layer: usize,
    pub(super) path: usize,
    /// The vertex, or the edge.
    pub(super) k: usize,
    /// The arc length along the path.
    pub(super) s: f64,
    pub(super) dir: V3,
    pub(super) dist: f64,
}

/// The targets of the visible vector layers.
pub struct Targets {
    layers: Vec<Arc<LayerTargets>>,
}

impl Targets {
    pub fn new(layers: Vec<Arc<LayerTargets>>) -> Self {
        Targets { layers }
    }

    #[cfg(test)]
    pub(super) fn layer(&self, i: usize) -> &LayerTargets {
        &self.layers[i]
    }

    /// The position that `p` snaps to, within `reach` radians. A vertex
    /// wins over an edge. The feature `exclude` is not a target.
    pub fn snap(&self, p: V3, reach: f64, s: &SnapSettings, exclude: Option<Uuid>) -> Option<Snap> {
        if !s.on {
            return None;
        }
        let (hit, kind) =
            if let Some(h) = s.vertices.then(|| self.vertex(p, reach, exclude)).flatten() {
                (h, Kind::Vertex)
            } else {
                (
                    s.edges.then(|| self.edge(p, reach, exclude)).flatten()?,
                    Kind::Edge,
                )
            };
        let layer = &self.layers[hit.layer];
        let path = &layer.paths[hit.path];
        Some(Snap {
            dir: hit.dir,
            kind,
            exact: (kind == Kind::Vertex).then(|| path.ll[hit.k]),
            layer: layer.layer,
            feature: path.uuid,
        })
    }

    /// The line end in the layer `layer` that is nearest to `p`, within
    /// `reach` radians.
    pub fn line_end(&self, layer: LayerId, p: V3, reach: f64) -> Option<(Uuid, V3)> {
        let mut best: Option<(f64, Uuid, V3)> = None;
        for l in self.layers.iter().filter(|l| l.layer == layer && l.line) {
            for path in l.paths.iter().filter(|p| p.dirs.len() > 1) {
                for end in [path.dirs[0], path.dirs[path.dirs.len() - 1]] {
                    let d = angle(p, end);
                    if d <= reach && best.is_none_or(|b| d < b.0) {
                        best = Some((d, path.uuid, end));
                    }
                }
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// The nearest vertex within `reach`: an end of a target edge, or a
    /// point.
    pub(super) fn vertex(&self, p: V3, reach: f64, exclude: Option<Uuid>) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (li, layer) in self.layers.iter().enumerate() {
            let (edges, points) = layer.near(p, reach);
            let ends = edges.iter().flat_map(|&e| {
                let (i, k) = layer.edges[e as usize];
                let n = layer.paths[i as usize].dirs.len() as u32;
                [(i, k), (i, (k + 1) % n)]
            });
            for (i, k) in ends.chain(points.iter().map(|&i| (i, 0))) {
                let path = &layer.paths[i as usize];
                let d = angle(p, path.dirs[k as usize]);
                if d > reach || Some(path.uuid) == exclude || best.is_some_and(|b| b.dist <= d) {
                    continue;
                }
                best = Some(Hit {
                    layer: li,
                    path: i as usize,
                    k: k as usize,
                    s: path.cum[k as usize],
                    dir: path.dirs[k as usize],
                    dist: d,
                });
            }
        }
        best
    }

    /// The nearest point on a target edge within `reach`.
    pub(super) fn edge(&self, p: V3, reach: f64, exclude: Option<Uuid>) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (li, layer) in self.layers.iter().enumerate() {
            for e in layer.near(p, reach).0 {
                let (i, k) = layer.edges[e as usize];
                let path = &layer.paths[i as usize];
                if Some(path.uuid) == exclude {
                    continue;
                }
                let (a, b) = path.ends(k as usize);
                let (q, along, d) = project(p, a, b);
                if d > reach || best.is_some_and(|b| b.dist <= d) {
                    continue;
                }
                best = Some(Hit {
                    layer: li,
                    path: i as usize,
                    k: k as usize,
                    s: path.cum[k as usize] + along,
                    dir: q,
                    dist: d,
                });
            }
        }
        best
    }
}

/// The cell of a direction: the face, then the row and the column of
/// equal-angle cells.
fn cell(d: V3) -> u32 {
    let (face, a, b) = face_of(d);
    let index = |x: f64| {
        ((warp(x) + 1.0) * 0.5 * CELLS as f64)
            .floor()
            .clamp(0.0, CELLS as f64 - 1.0) as usize
    };
    (face * CELLS * CELLS + index(a) * CELLS + index(b)) as u32
}

/// The cells that can hold a point within `reach` of `p`. A wide query
/// gives `None`.
///
/// The points of a grid around `p` hit the cells. The grid is fine enough
/// to hit each cell within the disc, and the disc has a margin of one cell,
/// so it holds each cell that holds a sample within reach.
fn cells_near(p: V3, reach: f64) -> Option<HashSet<u32>> {
    let r = reach + STEP + CELL_SPAN;
    if r > WIDE {
        return None;
    }
    let axis = if p[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(p, axis));
    let v = cross(p, u);
    let n = (r.tan() / STEP).ceil() as i32;
    let mut out = HashSet::new();
    for i in -n..=n {
        for j in -n..=n {
            if i * i + j * j > (n + 1) * (n + 1) {
                continue;
            }
            let (x, y) = (f64::from(i) * STEP, f64::from(j) * STEP);
            let g = [
                p[0] + u[0] * x + v[0] * y,
                p[1] + u[1] * x + v[1] * y,
                p[2] + u[2] * x + v[2] * y,
            ];
            out.insert(cell(normalize(g)));
        }
    }
    Some(out)
}

/// The point at the part `t` of the arc length of the short arc from `a`
/// to `b`.
pub(super) fn slerp(a: V3, b: V3, t: f64) -> V3 {
    let theta = angle(a, b);
    if theta < ZERO {
        return a;
    }
    let (wa, wb) = (((1.0 - t) * theta).sin(), (t * theta).sin());
    let s = theta.sin();
    normalize([
        (a[0] * wa + b[0] * wb) / s,
        (a[1] * wa + b[1] * wb) / s,
        (a[2] * wa + b[2] * wb) / s,
    ])
}

/// The nearest point of the short arc from `a` to `b` to `p`, its angle
/// from `a`, and its angle from `p`.
pub(super) fn project(p: V3, a: V3, b: V3) -> (V3, f64, f64) {
    let n = cross(a, b);
    let len = length(n);
    if len < 1e-15 {
        return (a, 0.0, angle(p, a));
    }
    let n = scale(n, 1.0 / len);
    // The point is beside the arc when it is on the inner side of both ends.
    if dot(cross(a, p), n) >= 0.0 && dot(cross(p, b), n) >= 0.0 {
        let h = dot(p, n);
        let q = normalize([p[0] - n[0] * h, p[1] - n[1] * h, p[2] - n[2] * h]);
        (q, angle(a, q), h.abs().asin())
    } else if angle(p, a) <= angle(p, b) {
        (a, 0.0, angle(p, a))
    } else {
        (b, angle(a, b), angle(p, b))
    }
}

/// The angle from `p` to the nearest point of the short arc from `a` to `b`.
pub(super) fn distance_to_arc(p: V3, a: V3, b: V3) -> f64 {
    project(p, a, b).2
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use vale_store::{Feature, LayerKind};

    use super::*;

    /// Points per radian: a sphere with a radius of 300 points on screen.
    const SCALE: f64 = 300.0;

    pub(crate) fn layer(id: u32, kind: GeometryKind, geometries: Vec<Geometry>) -> Layer {
        Layer {
            id: LayerId(id),
            name: String::new(),
            kind: LayerKind::Vector(kind),
            visible: true,
            source: None,
            fields: Vec::new(),
            features: geometries
                .into_iter()
                .map(|g| Feature::new(g, BTreeMap::new()))
                .collect(),
        }
    }

    pub(crate) fn targets(layers: &[Layer]) -> Targets {
        Targets::new(
            layers
                .iter()
                .map(|l| Arc::new(LayerTargets::build(l.id, l)))
                .collect(),
        )
    }

    fn dir(lon: f64, lat: f64) -> V3 {
        lonlat_to_dir(lon, lat)
    }

    /// The point `pts` points north of a position, at a scale.
    fn north(lon: f64, lat: f64, pts: f64, scale: f64) -> V3 {
        dir(lon, lat + (pts / scale).to_degrees())
    }

    #[test]
    fn a_position_snaps_to_a_vertex_before_an_edge() {
        let points = layer(
            1,
            GeometryKind::Point,
            vec![Geometry::Points(vec![[40.0, 0.0]])],
        );
        let lines = layer(
            2,
            GeometryKind::Line,
            vec![Geometry::Lines(vec![vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [20.0, 0.0],
            ]])],
        );
        let polygons = layer(
            3,
            GeometryKind::Polygon,
            vec![Geometry::Polygons(vec![vec![vec![
                [-20.0, -10.0],
                [-10.0, -10.0],
                [-10.0, 10.0],
                [-20.0, 10.0],
                [-20.0, -10.0],
            ]]])],
        );
        let t = targets(&[points, lines.clone(), polygons]);
        let s = SnapSettings::default();
        for scale in [SCALE, 4.0 * SCALE] {
            let reach = s.distance / scale;
            // A point.
            let snap = t
                .snap(north(40.0, 0.0, 8.0, scale), reach, &s, None)
                .unwrap();
            assert_eq!((snap.kind, snap.exact), (Kind::Vertex, Some([40.0, 0.0])));
            // A line vertex wins over the nearer edge.
            let p = dir(
                10.0 + (4.0 / scale).to_degrees(),
                (5.0 / scale).to_degrees(),
            );
            let snap = t.snap(p, reach, &s, None).unwrap();
            assert_eq!(snap.exact, Some([10.0, 0.0]));
            assert_eq!(snap.feature, lines.features[0].uuid());
            // An edge, away from the vertices.
            let snap = t
                .snap(north(5.0, 0.0, 8.0, scale), reach, &s, None)
                .unwrap();
            assert_eq!(snap.kind, Kind::Edge);
            assert!(angle(snap.dir, dir(5.0, 0.0)) < 1e-12);
            // A polygon vertex and a polygon edge.
            let snap = t
                .snap(north(-10.0, 10.0, 8.0, scale), reach, &s, None)
                .unwrap();
            assert_eq!(snap.exact, Some([-10.0, 10.0]));
            let p = dir(-10.0 + (8.0 / scale).to_degrees(), 0.0);
            let snap = t.snap(p, reach, &s, None).unwrap();
            assert_eq!(snap.kind, Kind::Edge);
            // Nothing outside the reach.
            assert_eq!(t.snap(north(5.0, 0.0, 13.0, scale), reach, &s, None), None);
            assert_eq!(t.snap(north(40.0, 0.0, 13.0, scale), reach, &s, None), None);
        }
    }

    /// A small random number generator for the tests.
    pub(crate) struct Lcg(pub u64);

    impl Lcg {
        pub fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn the_index_finds_what_a_scan_finds() {
        let mut rng = Lcg(7);
        // Places near face edges and cube corners.
        let corner = 1f64 / 2f64.sqrt();
        let centers = [
            [45.0, 0.0],
            [45.0, corner.atan().to_degrees()],
            [-135.0, -corner.atan().to_degrees()],
            [0.0, 45.0],
            [180.0, 0.0],
            [90.0, 89.0],
        ];
        let mut jitter = |c: LonLat, r: f64| {
            [
                c[0] + (rng.next() - 0.5) * r,
                (c[1] + (rng.next() - 0.5) * r).clamp(-90.0, 90.0),
            ]
        };
        let mut lines = Vec::new();
        for c in centers {
            for _ in 0..20 {
                let part: Vec<LonLat> = (0..4).map(|_| jitter(c, 6.0)).collect();
                lines.push(Geometry::Lines(vec![part]));
            }
        }
        let queries: Vec<V3> = centers
            .iter()
            .flat_map(|&c| (0..50).map(move |i| (c, i)))
            .map(|(c, _)| jitter(c, 6.0))
            .map(|p| dir(p[0], p[1]))
            .collect();
        let l = layer(1, GeometryKind::Line, lines);
        let t = targets(std::slice::from_ref(&l));
        let lt = t.layer(0);
        for reach in [0.002, 0.02, 0.1] {
            for &p in &queries {
                let scan = lt
                    .edges
                    .iter()
                    .map(|&(i, k)| {
                        let (a, b) = lt.paths[i as usize].ends(k as usize);
                        distance_to_arc(p, a, b)
                    })
                    .filter(|&d| d <= reach)
                    .fold(f64::INFINITY, f64::min);
                let found = t.edge(p, reach, None).map_or(f64::INFINITY, |h| h.dist);
                assert_eq!(found, scan, "reach {reach}");
            }
        }
    }
}
