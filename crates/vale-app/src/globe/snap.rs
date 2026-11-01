//! Snapping and tracing. A snap moves a position to the nearest vertex or
//! edge of a vector feature, or to the line of a color stop. A trace makes a
//! stroke follow an edge or such a line. The traced part of an edge has the
//! stored vertices of the target.
//!
//! Distances in points are on the screen. `scale` is the number of points in
//! one radian of the sphere, so a snap has the same reach at each zoom.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::f64::consts::FRAC_PI_2;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::RangeInclusive;
use std::sync::Arc;

use vale_store::{Geometry, GeometryKind, Layer, LayerId, LonLat, RasterId, Uuid};
use vale_terrain::{face_of, warp};

use super::line::{JOIN, chains};
use super::math::{V3, angle, cross, dot, length, lonlat_to_dir, normalize, scale};
use super::stops::{self, StopHit, StopLock, StopTargets};

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
/// The trace leaves its target at this many times the snap distance.
pub const RELEASE: f64 = 2.0;
/// A trace shorter than this, in points, is free. A stroke that crosses a
/// target does not trace it.
pub const MIN_TRACE: f64 = 8.0;
/// A sample within this part of the snap distance is on the target. The
/// rest of the snap distance is for the blends into and out of a trace.
const ON: f64 = 0.25;
/// A trace keeps its direction after this many points.
const TURN: f64 = 2.0;
/// An edge shorter than this has no length.
const ZERO: f64 = 1e-12;
/// Two points nearer than this are at the same place.
const SAME: f64 = 1e-9;

/// The snap settings of a project.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapSettings {
    /// The control that turns all snapping on and off.
    pub on: bool,
    /// Snap to the vertices of features.
    pub vertices: bool,
    /// Snap to the edges of features, and trace them.
    pub edges: bool,
    /// The reach of a snap, in points.
    pub distance: f64,
    /// The raster layers whose color stops are not targets.
    pub stops_off: BTreeSet<RasterId>,
}

impl SnapSettings {
    /// True if the color stops of the raster layer `id` are targets.
    pub fn stops(&self, id: RasterId) -> bool {
        !self.stops_off.contains(&id)
    }

    /// Makes the color stops of the raster layer `id` targets or not.
    pub fn set_stops(&mut self, id: RasterId, on: bool) {
        match on {
            true => self.stops_off.remove(&id),
            false => self.stops_off.insert(id),
        };
    }
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings {
            on: true,
            vertices: true,
            edges: true,
            distance: DEFAULT_DISTANCE,
            stops_off: BTreeSet::new(),
        }
    }
}

/// What a position snaps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Vertex,
    Edge,
    /// The line of a color stop.
    Stop,
}

/// The target of a snap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Feature {
        layer: LayerId,
        feature: Uuid,
    },
    /// The index of a stop between the two ends of the range.
    Stop {
        raster: RasterId,
        stop: usize,
    },
}

/// A snapped position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// The position, on the unit sphere.
    pub dir: V3,
    pub kind: Kind,
    /// The stored position of a vertex.
    pub exact: Option<LonLat>,
    pub source: Source,
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

    pub(super) fn path(&self, i: usize) -> &TargetPath {
        &self.paths[i]
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

/// The nearest point on an edge or on the line of a color stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum LineHit {
    Edge(Hit),
    Stop(StopHit),
}

impl LineHit {
    fn dir(&self) -> V3 {
        match self {
            LineHit::Edge(h) => h.dir,
            LineHit::Stop(h) => h.dir,
        }
    }
}

/// The targets of the visible vector layers and of the color stops of the
/// raster layers.
pub struct Targets {
    layers: Vec<Arc<LayerTargets>>,
    stops: Vec<Arc<StopTargets>>,
}

impl Targets {
    pub fn new(layers: Vec<Arc<LayerTargets>>, stops: Vec<Arc<StopTargets>>) -> Self {
        Targets { layers, stops }
    }

    pub fn layers(&self) -> &[Arc<LayerTargets>] {
        &self.layers
    }

    pub fn stops(&self) -> &[Arc<StopTargets>] {
        &self.stops
    }

    pub(super) fn layer(&self, i: usize) -> &LayerTargets {
        &self.layers[i]
    }

    /// The position that `p` snaps to at `scale` points per radian. A
    /// vertex wins over an edge and a color stop, and of an edge and a
    /// color stop, the nearer one wins. The feature `exclude` is not a
    /// target.
    pub fn snap(&self, p: V3, scale: f64, s: &SnapSettings, exclude: Option<Uuid>) -> Option<Snap> {
        if !s.on {
            return None;
        }
        let reach = s.distance / scale;
        let vertex = s.vertices.then(|| self.vertex(p, reach, exclude)).flatten();
        let (hit, kind) = match vertex {
            Some(h) => (h, Kind::Vertex),
            None => match self.line(p, scale, s, exclude)? {
                LineHit::Edge(h) => (h, Kind::Edge),
                LineHit::Stop(h) => {
                    let layer = &self.stops[h.layer];
                    return Some(Snap {
                        dir: h.dir,
                        kind: Kind::Stop,
                        exact: None,
                        source: Source::Stop {
                            raster: layer.raster(),
                            stop: h.level,
                        },
                    });
                }
            },
        };
        let layer = &self.layers[hit.layer];
        let path = &layer.paths[hit.path];
        Some(Snap {
            dir: hit.dir,
            kind,
            exact: (kind == Kind::Vertex).then(|| path.ll[hit.k]),
            source: Source::Feature {
                layer: layer.layer,
                feature: path.uuid,
            },
        })
    }

    /// The nearer of the nearest point on an edge and on the line of a
    /// color stop, within the snap distance.
    pub(super) fn line(
        &self,
        p: V3,
        scale: f64,
        s: &SnapSettings,
        exclude: Option<Uuid>,
    ) -> Option<LineHit> {
        let reach = s.distance / scale;
        let edge = s.edges.then(|| self.edge(p, reach, exclude)).flatten();
        let within = edge.map_or(reach, |e| e.dist);
        match stops::snap(&self.stops, |r| s.stops(r), p, scale, within) {
            Some(stop) if edge.is_none_or(|e| stop.dist < e.dist) => Some(LineHit::Stop(stop)),
            _ => edge.map(LineHit::Edge),
        }
    }

    /// The point where a stroke from `p` starts, at `scale` points per
    /// radian: the end of a line in the layer `layer` that the stroke
    /// extends, or the position that `p` snaps to. A pen that hovers shows
    /// it before the stroke.
    pub fn start(&self, layer: LayerId, p: V3, scale: f64, s: &SnapSettings) -> Option<V3> {
        match self.line_end(layer, p, JOIN / scale) {
            Some((_, end)) => Some(end),
            None => self.snap(p, scale, s, None).map(|snap| snap.dir),
        }
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

impl TargetPath {
    fn closed(&self) -> bool {
        self.dirs.len() > 1 && self.cum.len() > self.dirs.len()
    }

    fn total(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }

    /// The edge or the vertex of an index that counts on past the end of a
    /// ring, and the arc length of the laps before it.
    fn wrap(&self, k: i64) -> (usize, f64) {
        if !self.closed() {
            return (k as usize, 0.0);
        }
        let m = self.edges() as i64;
        (
            k.rem_euclid(m) as usize,
            k.div_euclid(m) as f64 * self.total(),
        )
    }

    /// The edge at the arc length `s`. On a ring, `s` counts on past the end.
    fn edge_at(&self, s: f64) -> i64 {
        let m = self.edges();
        let (lap, local) = if self.closed() {
            let lap = (s / self.total()).floor();
            (lap as i64, s - lap * self.total())
        } else {
            (0, s)
        };
        let k = self.cum[..m]
            .partition_point(|&c| c <= local)
            .saturating_sub(1);
        lap * m as i64 + k as i64
    }

    fn vertex_s(&self, v: i64) -> f64 {
        let (i, lap) = self.wrap(v);
        lap + self.cum[i]
    }

    fn point_at(&self, s: f64) -> V3 {
        let (k, lap) = self.wrap(self.edge_at(s));
        let (a, b) = self.ends(k);
        let len = self.len(k);
        if len < ZERO {
            return a;
        }
        slerp(a, b, ((s - lap - self.cum[k]) / len).clamp(0.0, 1.0))
    }

    /// The part of the path around the edge `k0` that a trace can follow,
    /// as the arc lengths of its ends. Each end is true when it is a cut or
    /// the end of a line. On a ring with no cut, the part is a little
    /// shorter than one lap each way from `from`.
    fn span(&self, k0: i64, from: f64) -> (f64, bool, f64, bool) {
        let m = self.edges() as i64;
        let cut = |k: i64| self.cut(self.wrap(k).0);
        if self.closed() {
            let (mut lo, mut hi) = (k0, k0);
            while hi - k0 < m && !cut(hi + 1) {
                hi += 1;
            }
            while k0 - lo < m && !cut(lo - 1) {
                lo -= 1;
            }
            if hi - k0 >= m {
                let lap = self.total() - SAME;
                return (from - lap, false, from + lap, false);
            }
            return (self.vertex_s(lo), true, self.vertex_s(hi + 1), true);
        }
        let (mut lo, mut hi) = (k0, k0);
        while hi + 1 < m && !cut(hi + 1) {
            hi += 1;
        }
        while lo > 0 && !cut(lo - 1) {
            lo -= 1;
        }
        (self.vertex_s(lo), true, self.vertex_s(hi + 1), true)
    }

    /// The point of the path between the arc lengths `a` and `b` that is
    /// nearest to `p`: its arc length and its distance.
    fn nearest(&self, p: V3, a: f64, b: f64) -> (f64, f64) {
        let mut best = (a, angle(p, self.point_at(a)));
        for k in self.edge_at(a)..=self.edge_at(b) {
            let (i, lap) = self.wrap(k);
            if !self.target(i) {
                continue;
            }
            let (x, y) = self.ends(i);
            let s = (lap + self.cum[i] + project(p, x, y).1).clamp(a, b);
            let d = angle(p, self.point_at(s));
            if d < best.1 {
                best = (s, d);
            }
        }
        best
    }

    /// The vertices of a trace from the arc length `from` to `to`. The
    /// stored vertices between them come verbatim. An end at a vertex is
    /// that vertex.
    fn trace(&self, from: f64, to: f64) -> Vec<PathVertex> {
        let (lo, hi) = (from.min(to), from.max(to));
        let n = self.dirs.len() as i64;
        let mut vs: Vec<i64> = (self.edge_at(lo) - 1..=self.edge_at(hi) + 2)
            .filter(|&v| self.closed() || (0..n).contains(&v))
            .filter(|&v| (lo - SAME..=hi + SAME).contains(&self.vertex_s(v)))
            .collect();
        if to < from {
            vs.reverse();
        }
        let near = |v: i64, s: f64| (self.vertex_s(v) - s).abs() < SAME;
        let exact = |v: i64| PathVertex::Exact(self.ll[self.wrap(v).0 % n as usize]);
        let start = vs.iter().take_while(|&&v| near(v, from)).last();
        let mut out =
            vec![start.map_or_else(|| PathVertex::Free(self.point_at(from)), |&v| exact(v))];
        if hi - lo < SAME {
            return out;
        }
        let end = vs.iter().rev().take_while(|&&v| near(v, to)).last();
        out.extend(
            vs.iter()
                .filter(|&&v| !near(v, from) && !near(v, to))
                .map(|&v| exact(v)),
        );
        out.push(end.map_or_else(|| PathVertex::Free(self.point_at(to)), |&v| exact(v)));
        out
    }
}

/// A vertex of the path of a stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathVertex {
    /// A new position, on the unit sphere.
    Free(V3),
    /// A stored vertex of a target. It keeps its stored position.
    Exact(LonLat),
}

impl PathVertex {
    pub fn dir(&self) -> V3 {
        match *self {
            PathVertex::Free(d) => d,
            PathVertex::Exact(p) => lonlat_to_dir(p[0], p[1]),
        }
    }
}

/// A part of a stroke. Each run starts where the run before it ends.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    /// Pen samples, for the tool to smooth.
    Free(Vec<V3>),
    /// A part that follows a target, or a vertex that an end snaps to. The
    /// tool keeps it as it is.
    Traced(Vec<PathVertex>),
}

/// What the globe shows while a stroke snaps or traces.
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    /// The traced part.
    pub span: Vec<V3>,
    /// The position that the pen snaps to.
    pub point: Option<V3>,
}

/// Adds vertices to a path. Where two vertices are at one place, one stays,
/// the exact one when there is one. Two different exact vertices at one
/// place both stay, as at a seam or a pole.
pub(super) fn append(path: &mut Vec<PathVertex>, more: impl IntoIterator<Item = PathVertex>) {
    for v in more {
        let Some(last) = path.last_mut() else {
            path.push(v);
            continue;
        };
        if angle(last.dir(), v.dir()) >= ZERO {
            path.push(v);
            continue;
        }
        match (*last, v) {
            (PathVertex::Exact(a), PathVertex::Exact(b)) if a != b => path.push(v),
            (PathVertex::Free(_), PathVertex::Exact(_)) => *last = v,
            _ => {}
        }
    }
}

/// A part of a trace on one path, as arc lengths.
#[derive(Clone, Copy, Debug)]
struct Piece {
    path: usize,
    from: f64,
    to: f64,
}

/// A stroke that follows an edge.
struct PathLock {
    layer: usize,
    /// The parts of the trace on the paths before this one.
    done: Vec<Piece>,
    path: usize,
    from: f64,
    s: f64,
    /// 1 or -1 when the trace has a direction, else 0.
    dir: f64,
    lo: f64,
    lo_end: bool,
    hi: f64,
    hi_end: bool,
    /// The samples since the lock.
    raw: Vec<V3>,
}

impl PathLock {
    fn new(hit: &Hit, path: &TargetPath, raw: Vec<V3>) -> PathLock {
        let (lo, lo_end, hi, hi_end) = path.span(hit.k as i64, hit.s);
        PathLock {
            layer: hit.layer,
            done: Vec::new(),
            path: hit.path,
            from: hit.s,
            s: hit.s,
            dir: 0.0,
            lo,
            lo_end,
            hi,
            hi_end,
            raw,
        }
    }

    fn pieces(&self) -> impl Iterator<Item = Piece> + '_ {
        let current = Piece {
            path: self.path,
            from: self.from,
            to: self.s,
        };
        self.done.iter().copied().chain([current])
    }

    fn length(&self) -> f64 {
        self.pieces().map(|p| (p.to - p.from).abs()).sum()
    }

    fn vertices(&self, layer: &LayerTargets) -> Vec<PathVertex> {
        let mut out = Vec::new();
        for p in self.pieces() {
            append(&mut out, layer.path(p.path).trace(p.from, p.to));
        }
        out
    }

    /// Moves the trace on to another path at the end of this one, when the
    /// pen goes on past the end. The other path has a vertex at the end.
    /// Gives the distance of the pen from the other path.
    fn go_on(
        &mut self,
        layer: &LayerTargets,
        p: V3,
        side: f64,
        exclude: Option<Uuid>,
    ) -> Option<f64> {
        let path = layer.path(self.path);
        let end_s = if side > 0.0 { self.hi } else { self.lo };
        let end = path.point_at(end_s);
        let came = path.wrap(path.edge_at(end_s - side * ZERO)).0;
        let mut best: Option<(f64, usize, usize, f64, f64)> = None;
        for e in layer.near(end, SAME).0 {
            let (i, k) = layer.edges[e as usize];
            let (i, k) = (i as usize, k as usize);
            let other = layer.path(i);
            if (i, k) == (self.path, came) || Some(other.uuid) == exclude {
                continue;
            }
            let (a, b) = other.ends(k);
            let (_, along, dist) = project(p, a, b);
            for (v, dir, away) in [(a, 1.0, along), (b, -1.0, other.len(k) - along)] {
                if angle(v, end) > SAME || away <= ZERO || best.is_some_and(|x| x.0 <= dist) {
                    continue;
                }
                best = Some((dist, i, k, dir, along));
            }
        }
        let (dist, i, k, dir, along) = best?;
        self.done.push(Piece {
            path: self.path,
            from: self.from,
            to: end_s,
        });
        let other = layer.path(i);
        self.path = i;
        self.from = other.cum[if dir > 0.0 { k } else { k + 1 }];
        self.s = other.cum[k] + along;
        self.dir = dir;
        (self.lo, self.lo_end, self.hi, self.hi_end) = other.span(k as i64, self.from);
        Some(dist)
    }
}

/// A stroke that follows a target.
enum Lock {
    Path(PathLock),
    Stop(StopLock),
}

impl Lock {
    fn length(&self) -> f64 {
        match self {
            Lock::Path(l) => l.length(),
            Lock::Stop(l) => l.length(),
        }
    }

    fn vertices(&self, targets: &Targets) -> Vec<PathVertex> {
        match self {
            Lock::Path(l) => l.vertices(targets.layer(l.layer)),
            Lock::Stop(l) => l.vertices(),
        }
    }

    /// The point of the trace now.
    fn point(&self, targets: &Targets) -> V3 {
        match self {
            Lock::Path(l) => targets.layer(l.layer).path(l.path).point_at(l.s),
            Lock::Stop(l) => l.point(),
        }
    }

    /// The samples since the lock.
    fn raw(&mut self) -> &mut Vec<V3> {
        match self {
            Lock::Path(l) => &mut l.raw,
            Lock::Stop(l) => &mut l.raw,
        }
    }
}

/// Makes the runs of a stroke from its pen samples. A stroke that comes
/// near an edge follows it, and it leaves the edge when the pen moves away.
/// The first and the last sample snap to a position.
pub struct Tracer {
    targets: Arc<Targets>,
    settings: SnapSettings,
    scale: f64,
    layer: LayerId,
    /// The line that the stroke extends, or the feature that the stroke
    /// reshapes. It is not a target.
    exclude: Option<Uuid>,
    last: Option<V3>,
    runs: Vec<Run>,
    free: Vec<V3>,
    lock: Option<Lock>,
    point: Option<V3>,
    /// The end of the line that the stroke extends.
    joined: Option<V3>,
}

impl Tracer {
    /// A tracer for a stroke at `scale` points per radian. A stroke that
    /// starts at the end of a line in the layer `layer` extends that line.
    pub fn new(targets: Arc<Targets>, settings: SnapSettings, scale: f64, layer: LayerId) -> Self {
        Tracer {
            targets,
            settings,
            scale,
            layer,
            exclude: None,
            last: None,
            runs: Vec::new(),
            free: Vec::new(),
            lock: None,
            point: None,
            joined: None,
        }
    }

    /// A tracer that does not snap to or trace the feature `uuid`, and does
    /// not extend a line.
    pub fn excluding(mut self, uuid: Uuid) -> Self {
        self.exclude = Some(uuid);
        self
    }

    fn reach(&self) -> f64 {
        self.settings.distance / self.scale
    }

    /// Adds a pen sample, on the unit sphere.
    pub fn push(&mut self, p: V3) {
        let step = self.last.map_or(0.0, |l| angle(l, p));
        let first = self.last.is_none();
        self.last = Some(p);
        if first {
            self.free.push(p);
            let join = self.exclude.is_none();
            if let Some((uuid, end)) = join
                .then(|| self.targets.line_end(self.layer, p, JOIN / self.scale))
                .flatten()
            {
                // The join wins over a snap.
                self.exclude = Some(uuid);
                self.joined = Some(end);
                return;
            }
            if !self.settings.on {
                return;
            }
            self.snap_end(true);
        } else if !self.settings.on {
            self.free.push(p);
            return;
        } else if self.lock.is_some() {
            self.follow(p, step);
            return;
        } else {
            self.free.push(p);
        }
        self.try_lock(p);
    }

    /// The target that the stroke follows or snaps to now.
    pub fn highlight(&self) -> Option<Highlight> {
        if let Some(lock) = &self.lock {
            return Some(Highlight {
                span: lock
                    .vertices(&self.targets)
                    .iter()
                    .map(PathVertex::dir)
                    .collect(),
                point: Some(lock.point(&self.targets)),
            });
        }
        // The end of the line that the stroke extends shows while the pen
        // is near it.
        let joined = self.joined.filter(|&j| {
            let last = self.last.expect("a sample after the join");
            angle(last, j) <= JOIN / self.scale
        });
        self.point.or(joined).map(|p| Highlight {
            span: Vec::new(),
            point: Some(p),
        })
    }

    /// The runs of the stroke.
    pub fn finish(mut self) -> Vec<Run> {
        if !self.release(false) && self.settings.on {
            self.snap_end(false);
        }
        self.flush();
        self.runs
    }

    /// Snaps the last free point. A vertex adds a run of the exact vertex.
    fn snap_end(&mut self, start: bool) {
        let Some(&p) = self.free.last() else {
            return;
        };
        let Some(snap) = self.targets.snap(p, self.scale, &self.settings, None) else {
            return;
        };
        *self.free.last_mut().expect("a free point") = snap.dir;
        if let Some(ll) = snap.exact {
            let vertex = Run::Traced(vec![PathVertex::Exact(ll)]);
            if !start {
                self.flush();
            }
            self.runs.push(vertex);
        }
    }

    fn try_lock(&mut self, p: V3) {
        let (s, targets) = (&self.settings, &self.targets);
        let vertex = s
            .vertices
            .then(|| targets.vertex(p, self.reach(), self.exclude))
            .flatten();
        let line = targets.line(p, self.scale, s, self.exclude);
        self.point = vertex.map(|v| v.dir).or(line.map(|l| l.dir()));
        let Some(line) = line else {
            return;
        };
        let raw = || vec![*self.free.last().expect("a free point")];
        let lock = match line {
            LineHit::Edge(hit) => {
                let path = targets.layer(hit.layer).path(hit.path);
                Lock::Path(PathLock::new(&hit, path, raw()))
            }
            LineHit::Stop(hit) => {
                let layer = &targets.stops[hit.layer];
                match StopLock::new(layer, &hit, self.scale, raw()) {
                    Some(lock) => Lock::Stop(lock),
                    None => return,
                }
            }
        };
        self.free.pop();
        self.lock = Some(lock);
        self.point = None;
    }

    /// Moves the trace on along its target, or leaves the target.
    fn follow(&mut self, p: V3, step: f64) {
        let reach = self.reach();
        let turn = TURN / self.scale;
        let targets = Arc::clone(&self.targets);
        let lock = match self.lock.as_mut().expect("a lock") {
            Lock::Path(lock) => lock,
            Lock::Stop(lock) => {
                lock.raw.push(p);
                let layer = &targets.stops[lock.layer];
                if lock.follow(layer, p, step, reach, turn) > RELEASE * reach {
                    self.release(true);
                }
                return;
            }
        };
        lock.raw.push(p);
        let layer = targets.layer(lock.layer);
        let path = layer.path(lock.path);
        // The pen goes from the target, or along it, but it does not jump
        // to another part of it.
        let w = step + 2.0 * reach;
        let (s, mut dist) = path.nearest(p, (lock.s - w).max(lock.lo), (lock.s + w).min(lock.hi));
        if lock.dir == 0.0 {
            lock.s = s;
            if (s - lock.from).abs() > turn {
                lock.dir = (s - lock.from).signum();
            }
        } else if (s - lock.s) * lock.dir > 0.0 {
            lock.s = s;
        }
        let at_hi = lock.hi_end && lock.dir >= 0.0 && lock.s >= lock.hi - ZERO;
        let at_lo = lock.lo_end && lock.dir <= 0.0 && lock.s <= lock.lo + ZERO;
        if at_hi || at_lo {
            let side = if at_hi { 1.0 } else { -1.0 };
            if let Some(d) = lock.go_on(layer, p, side, self.exclude) {
                dist = d;
            }
        }
        if dist > RELEASE * reach {
            self.release(true);
        }
    }

    /// Ends the trace, and gives true when it makes a traced run. A short
    /// trace gives its samples back to the free run. `away` is true when
    /// the pen moved away from the target.
    ///
    /// The traced run goes from the first to the last sample that is on the
    /// target. Before it and after it, the stroke blends from the pen to
    /// the target over the samples that are within the snap distance. So
    /// the stroke meets the target at the angle of the pen. A stroke that
    /// starts or ends on the target has no blend there.
    fn release(&mut self, away: bool) -> bool {
        let Some(mut lock) = self.lock.take() else {
            return false;
        };
        let raw = std::mem::take(lock.raw());
        if lock.length() < MIN_TRACE / self.scale {
            self.free.extend(raw);
            return false;
        }
        let trace = lock.vertices(&self.targets);
        let dirs: Vec<V3> = trace.iter().map(PathVertex::dir).collect();
        let cum = cumulative(&dirs);
        let total = cum[cum.len() - 1];
        let reach = self.reach();
        let near = along(&dirs, &cum, &raw, 4.0 * reach);
        let on = |&(_, d): &(f64, f64)| d <= ON * reach;
        let nearest = (0..near.len())
            .min_by(|&i, &j| near[i].1.total_cmp(&near[j].1))
            .expect("a sample");
        let first = near.iter().position(on).unwrap_or(nearest);
        let last = near.iter().rposition(on).unwrap_or(nearest);
        let blends = !self.free.is_empty();
        let t_in = if blends { near[first].0 } else { 0.0 };
        let t_out = if away { near[last].0.max(t_in) } else { total };
        // The blends go to the samples that are a snap distance away.
        let far_in = (0..first).rev().find(|&i| near[i].1 >= reach).unwrap_or(0);
        let far_out = (last..near.len())
            .find(|&i| near[i].1 >= reach)
            .unwrap_or(near.len() - 1);
        let weight = |x: f64| {
            let x = x.clamp(0.0, 1.0);
            x * x * (3.0 - 2.0 * x)
        };
        if blends {
            let from = near[far_in].0;
            for (i, (&p, &(t, _))) in raw.iter().zip(&near).enumerate().take(first) {
                let w = if i < far_in || t_in <= from {
                    0.0
                } else {
                    weight((t - from) / (t_in - from))
                };
                self.free.push(slerp(p, point_at(&dirs, &cum, t), w));
            }
        }
        let entry = point_at(&dirs, &cum, t_in);
        if self.free.last().is_none_or(|&l| angle(l, entry) >= ZERO) {
            self.free.push(entry);
        }
        self.flush();
        let run = cut(&trace, &cum, t_in, t_out);
        self.free = vec![run[run.len() - 1].dir()];
        self.runs.push(Run::Traced(run));
        if away {
            let to = near[far_out].0;
            let samples = raw.iter().zip(&near).enumerate().skip(last + 1);
            for (i, (&p, &(t, _))) in samples {
                let w = if i > far_out || to <= t_out {
                    0.0
                } else {
                    weight((to - t) / (to - t_out))
                };
                self.free.push(slerp(p, point_at(&dirs, &cum, t), w));
            }
        }
        true
    }

    fn flush(&mut self) {
        let free = std::mem::take(&mut self.free);
        if free.len() > 1 {
            self.runs.push(Run::Free(free));
        }
    }
}

/// The arc length of a path at each of its vertices.
fn cumulative(path: &[V3]) -> Vec<f64> {
    let mut out = vec![0.0];
    for w in path.windows(2) {
        out.push(out[out.len() - 1] + angle(w[0], w[1]));
    }
    out
}

/// The point of a path at the arc length `t`.
fn point_at(path: &[V3], cum: &[f64], t: f64) -> V3 {
    if path.len() < 2 {
        return path[0];
    }
    let j = cum.partition_point(|&c| c <= t).clamp(1, path.len() - 1);
    let len = cum[j] - cum[j - 1];
    if len < ZERO {
        return path[j];
    }
    slerp(
        path[j - 1],
        path[j],
        ((t - cum[j - 1]) / len).clamp(0.0, 1.0),
    )
}

/// The arc length along a path of the nearest point to each sample, and
/// the distance of the sample from the path. The arc length only goes
/// forward, and it looks ahead at most `window` for each sample.
fn along(path: &[V3], cum: &[f64], samples: &[V3], window: f64) -> Vec<(f64, f64)> {
    let (mut k, mut t) = (0, 0.0);
    let mut out = Vec::with_capacity(samples.len());
    for &p in samples {
        let mut best: Option<(f64, f64, usize)> = None;
        let mut j = k;
        while j + 1 < path.len() && cum[j] <= t + window {
            let (_, a, d) = project(p, path[j], path[j + 1]);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, (cum[j] + a).max(t), j));
            }
            j += 1;
        }
        let d = match best {
            Some((d, s, j)) => {
                (t, k) = (s, j);
                d
            }
            None => angle(p, path[0]),
        };
        out.push((t, d));
    }
    out
}

/// The part of a traced path from the arc length `a` to `b`. A cut end is
/// a free vertex, and the vertices between stay as they are.
fn cut(path: &[PathVertex], cum: &[f64], a: f64, b: f64) -> Vec<PathVertex> {
    let dirs: Vec<V3> = path.iter().map(PathVertex::dir).collect();
    let total = cum[cum.len() - 1];
    let (start, end) = (a <= 0.0, b >= total);
    let mut out = Vec::new();
    if start {
        out.push(path[0]);
    } else {
        out.push(PathVertex::Free(point_at(&dirs, cum, a)));
    }
    let inner = (1..path.len() - 1).filter(|&i| (start || cum[i] > a) && (end || cum[i] < b));
    append(&mut out, inner.map(|i| path[i]));
    if end {
        append(&mut out, [path[path.len() - 1]]);
    } else {
        append(&mut out, [PathVertex::Free(point_at(&dirs, cum, b))]);
    }
    out
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
pub(super) mod tests {
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
            style: None,
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
            Vec::new(),
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
            // A point.
            let snap = t
                .snap(north(40.0, 0.0, 8.0, scale), scale, &s, None)
                .unwrap();
            assert_eq!((snap.kind, snap.exact), (Kind::Vertex, Some([40.0, 0.0])));
            // A line vertex wins over the nearer edge.
            let p = dir(
                10.0 + (4.0 / scale).to_degrees(),
                (5.0 / scale).to_degrees(),
            );
            let snap = t.snap(p, scale, &s, None).unwrap();
            assert_eq!(snap.exact, Some([10.0, 0.0]));
            let feature = lines.features[0].uuid();
            assert_eq!(
                snap.source,
                Source::Feature {
                    layer: lines.id,
                    feature
                }
            );
            // An edge, away from the vertices.
            let snap = t
                .snap(north(5.0, 0.0, 8.0, scale), scale, &s, None)
                .unwrap();
            assert_eq!(snap.kind, Kind::Edge);
            assert!(angle(snap.dir, dir(5.0, 0.0)) < 1e-12);
            // A polygon vertex and a polygon edge.
            let snap = t
                .snap(north(-10.0, 10.0, 8.0, scale), scale, &s, None)
                .unwrap();
            assert_eq!(snap.exact, Some([-10.0, 10.0]));
            let p = dir(-10.0 + (8.0 / scale).to_degrees(), 0.0);
            let snap = t.snap(p, scale, &s, None).unwrap();
            assert_eq!(snap.kind, Kind::Edge);
            // Nothing outside the reach.
            assert_eq!(t.snap(north(5.0, 0.0, 13.0, scale), scale, &s, None), None);
            assert_eq!(t.snap(north(40.0, 0.0, 13.0, scale), scale, &s, None), None);
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

    /// Pen samples along a path in longitude and latitude, about 0.5
    /// points apart, with a wiggle of `noise` degrees in latitude.
    pub(crate) fn pen(way: &[LonLat], noise: f64) -> Vec<V3> {
        let mut out = vec![dir(way[0][0], way[0][1])];
        for w in way.windows(2) {
            let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            let n = ((dx * dx + dy * dy).sqrt() / 0.1).ceil() as usize;
            for j in 1..=n {
                let t = j as f64 / n as f64;
                let wiggle = noise * (out.len() as f64 * 1.3).sin();
                out.push(dir(w[0][0] + dx * t, w[0][1] + dy * t + wiggle));
            }
        }
        out
    }

    pub(crate) fn trace(t: &Arc<Targets>, samples: &[V3], s: SnapSettings, layer: u32) -> Vec<Run> {
        trace_excluding(t, samples, s, layer, None)
    }

    pub(crate) fn trace_excluding(
        t: &Arc<Targets>,
        samples: &[V3],
        s: SnapSettings,
        layer: u32,
        exclude: Option<Uuid>,
    ) -> Vec<Run> {
        let mut tracer = Tracer::new(Arc::clone(t), s, SCALE, LayerId(layer));
        if let Some(uuid) = exclude {
            tracer = tracer.excluding(uuid);
        }
        for &p in samples {
            tracer.push(p);
        }
        tracer.finish()
    }

    /// F for a free run, T for a traced run, V for a snapped vertex.
    pub(crate) fn kinds(runs: &[Run]) -> String {
        runs.iter()
            .map(|r| match r {
                Run::Free(_) => 'F',
                Run::Traced(v) if v.len() == 1 => 'V',
                Run::Traced(_) => 'T',
            })
            .collect()
    }

    /// The exact vertices of the traced runs.
    pub(crate) fn exact(runs: &[Run]) -> Vec<LonLat> {
        let mut out = Vec::new();
        for r in runs {
            if let Run::Traced(v) = r {
                for p in v {
                    if let PathVertex::Exact(ll) = p {
                        out.push(*ll);
                    }
                }
            }
        }
        out
    }

    fn equator() -> Layer {
        let part = (0..=10).map(|i| [3.0 * f64::from(i), 0.0]).collect();
        layer(1, GeometryKind::Line, vec![Geometry::Lines(vec![part])])
    }

    #[test]
    fn a_stroke_does_not_trace_or_extend_the_excluded_feature() {
        let line = equator();
        let uuid = line.features[0].uuid();
        let t = Arc::new(targets(&[line]));
        let samples = pen(&[[0.0, 0.0], [20.0, 0.0], [24.0, 8.0]], 0.5);
        let runs = trace_excluding(&t, &samples, SnapSettings::default(), 1, Some(uuid));
        // The start can snap to a vertex of the feature.
        assert_eq!(kinds(&runs), "VF");
    }

    #[test]
    fn a_stroke_follows_a_line_and_leaves_it() {
        let t = Arc::new(targets(&[equator()]));
        // The wiggle is about 3 points each way.
        let samples = pen(&[[0.0, 6.0], [4.0, 0.0], [20.0, 0.0], [24.0, 8.0]], 0.5);
        let runs = trace(&t, &samples, SnapSettings::default(), 9);
        assert_eq!(kinds(&runs), "FTF");
        // The blends cover about 2.3 degrees at each end.
        let lons: Vec<f64> = exact(&runs).iter().map(|p| p[0]).collect();
        assert_eq!(lons, [6.0, 9.0, 12.0, 15.0, 18.0]);
        // The runs meet.
        let Run::Traced(trace) = &runs[1] else {
            panic!("not a trace");
        };
        let (Run::Free(a), Run::Free(b)) = (&runs[0], &runs[2]) else {
            panic!("not free");
        };
        assert_eq!(a[a.len() - 1], trace[0].dir());
        assert_eq!(b[0], trace[trace.len() - 1].dir());
        // The last free run goes to the pen.
        assert_eq!(b[b.len() - 1], samples[samples.len() - 1]);
    }

    #[test]
    fn a_stroke_follows_a_polygon_ring_past_its_start() {
        let ring = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let l = layer(
            1,
            GeometryKind::Polygon,
            vec![Geometry::Polygons(vec![vec![ring]])],
        );
        let t = Arc::new(targets(&[l]));
        let way = [
            [5.0, -5.0],
            [5.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, -5.0],
        ];
        let runs = trace(&t, &pen(&way, 0.3), SnapSettings::default(), 9);
        assert_eq!(kinds(&runs), "FTF");
        assert_eq!(
            exact(&runs),
            [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn a_short_crossing_does_not_trace() {
        let t = Arc::new(targets(&[equator()]));
        let samples = pen(&[[4.0, -5.0], [5.0, 5.0]], 0.0);
        let runs = trace(&t, &samples, SnapSettings::default(), 9);
        assert_eq!(runs, [Run::Free(samples)]);
    }

    #[test]
    fn a_trace_across_a_face_edge_is_continuous() {
        // The middle vertex is a corner of the cube.
        let corner = (1f64 / 2f64.sqrt()).atan().to_degrees();
        let part = vec![
            [35.0, 30.0],
            [40.0, 33.0],
            [45.0, corner],
            [50.0, 37.0],
            [55.0, 39.0],
        ];
        let l = layer(
            1,
            GeometryKind::Line,
            vec![Geometry::Lines(vec![part.clone()])],
        );
        let t = Arc::new(targets(&[l]));
        let mut way = vec![[30.0, 35.0]];
        way.extend_from_slice(&part[..4]);
        way.extend([[52.0, 37.8], [52.0, 45.0]]);
        let runs = trace(&t, &pen(&way, 0.3), SnapSettings::default(), 9);
        assert_eq!(kinds(&runs), "FTF");
        // The blend into the target takes the first vertex.
        assert_eq!(exact(&runs), part[1..4]);
    }

    #[test]
    fn each_switch_turns_off_its_target_kind() {
        let t = Arc::new(targets(&[equator()]));
        let near_vertex = north(9.0, 0.0, 4.0, SCALE);
        let near_edge = north(10.5, 0.0, 10.0, SCALE);
        let on = SnapSettings::default();
        let snap = |p: V3, s: &SnapSettings| t.snap(p, SCALE, s, None).map(|s| s.kind);
        assert_eq!(snap(near_vertex, &on), Some(Kind::Vertex));
        let no_vertices = SnapSettings {
            vertices: false,
            ..on.clone()
        };
        assert_eq!(snap(near_vertex, &no_vertices), Some(Kind::Edge));
        let no_edges = SnapSettings {
            edges: false,
            ..on.clone()
        };
        assert_eq!(snap(near_vertex, &no_edges), Some(Kind::Vertex));
        assert_eq!(snap(near_edge, &no_edges), None);
        let off = SnapSettings { on: false, ..on };
        assert_eq!(snap(near_vertex, &off), None);

        let samples = pen(&[[0.0, 6.0], [4.0, 0.0], [20.0, 0.0], [24.0, 8.0]], 0.5);
        assert_eq!(kinds(&trace(&t, &samples, no_edges, 9)), "F");
        assert_eq!(trace(&t, &samples, off, 9), [Run::Free(samples)]);
    }
}
