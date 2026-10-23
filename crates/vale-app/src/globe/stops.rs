//! The snap target of a color stop: the line where the field that the
//! globe shows for a raster layer crosses the level of the stop. The target
//! reads the field only near the pen.
//!
//! A point is above the stop when its field is at the level or higher, as
//! on the GPU. Distances in points are on the screen, at `scale` points per
//! radian.

use std::collections::VecDeque;
use std::sync::Arc;

use vale_store::{RasterId, Smoothing};
use vale_terrain::{Heightmap, LocalPyramid, smoothing_level};

use super::line::simplify;
use super::math::{V3, add, angle, cross, dot, length, normalize, scale};
use super::snap::{PathVertex, project};

/// The step of a march along the line, in points.
const STEP: f64 = 0.5;
/// The most steps of a march for one pen sample. A march that is behind the
/// pen catches up at the next samples.
pub const MAX_MARCH: usize = 300;
/// The number of cells across the disc of a position snap.
const GRID: f64 = 24.0;
/// The halvings of a bracket that find a crossing.
const HALVINGS: usize = 12;
/// A traced vertex nearer than this to the simpler line goes, in points.
const TOLERANCE: f64 = 0.1;
/// The halvings of a step that finds no line.
const RETRIES: usize = 4;

/// The color stops of one raster layer, and the field near the pen.
pub struct StopTargets {
    raster: RasterId,
    field: LocalPyramid,
    smoothing: Smoothing,
    /// The levels of the stops, from 0 to 1, in order.
    levels: Vec<f64>,
}

impl StopTargets {
    /// The targets of the stops at `levels` of a raster layer. `map` is a
    /// snapshot of the heightmap, and the GPU has a copy of it with a face
    /// of `face_size` texels. A level at an end of the range has no line.
    /// `None`: the copy is not a reduction of `map`.
    pub fn new(
        raster: RasterId,
        map: Heightmap,
        face_size: u32,
        smoothing: Smoothing,
        levels: impl IntoIterator<Item = f64>,
    ) -> Option<Self> {
        let mut levels: Vec<f64> = levels.into_iter().filter(|&l| l > 0.0 && l < 1.0).collect();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        Some(StopTargets {
            raster,
            field: LocalPyramid::new(map, face_size)?,
            smoothing,
            levels,
        })
    }

    pub fn raster(&self) -> RasterId {
        self.raster
    }

    pub fn levels(&self) -> &[f64] {
        &self.levels
    }

    pub fn field(&self) -> &LocalPyramid {
        &self.field
    }

    /// The level of the smooth field at `scale`, as the globe shows it.
    pub fn lod(&self, scale: f64) -> f64 {
        if !self.smoothing.on {
            return 0.0;
        }
        let size = f64::from(self.smoothing.feature_size);
        smoothing_level(size, scale, self.field.face_size())
    }

    fn up(&self, d: V3, lod: f64, level: f64) -> bool {
        self.field.field(d, lod) >= level
    }

    /// The point between `a` and `b` where the side of the stop changes.
    /// `a` and `b` are on two sides.
    fn crossing(&self, a: V3, b: V3, lod: f64, level: f64) -> V3 {
        let side = self.up(a, lod, level);
        let (mut a, mut b) = (a, b);
        for _ in 0..HALVINGS {
            let mid = normalize(add(a, b));
            if self.up(mid, lod, level) == side {
                a = mid;
            } else {
                b = mid;
            }
        }
        normalize(add(a, b))
    }

    /// The direction of the line through `q`, with the higher side at the
    /// left, from the slope of the field over `h` radians. `None`: the
    /// field is flat.
    fn tangent(&self, q: V3, lod: f64, h: f64) -> Option<V3> {
        let (u, v) = basis(q);
        let at = |x: f64, y: f64| self.field.field(offset(q, u, v, x, y), lod);
        let gu = at(h, 0.0) - at(-h, 0.0);
        let gv = at(0.0, h) - at(0.0, -h);
        let g = add(scale(u, gu), scale(v, gv));
        (length(g) > 0.0).then(|| normalize(cross(g, q)))
    }
}

/// A point on the line of a color stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopHit {
    /// The index of the layer in the targets.
    pub layer: usize,
    /// The index of the level in the layer.
    pub level: usize,
    pub dir: V3,
    pub dist: f64,
}

/// Two directions at a right angle in the tangent plane at `p`.
fn basis(p: V3) -> (V3, V3) {
    let axis = if p[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(p, axis));
    (u, cross(p, u))
}

fn offset(p: V3, u: V3, v: V3, x: f64, y: f64) -> V3 {
    normalize(add(p, add(scale(u, x), scale(v, y))))
}

/// The nearest point to `p` on the line of a stop, within `reach` radians.
/// `on` gives the raster layers that are targets.
pub fn snap(
    stops: &[Arc<StopTargets>],
    on: impl Fn(RasterId) -> bool,
    p: V3,
    scale_: f64,
    reach: f64,
) -> Option<StopHit> {
    let mut best: Option<StopHit> = None;
    // The cells are about one point, and a grid has at most 49 by 49 nodes.
    let h = (1.0 / scale_).max(2.0 * reach / GRID);
    let n = (reach / h).ceil() as i32;
    let (u, v) = basis(p);
    let side = (2 * n + 1) as usize;
    let node = |i: i32, j: i32| offset(p, u, v, f64::from(i) * h, f64::from(j) * h);
    let index = |i: i32, j: i32| (j + n) as usize * side + (i + n) as usize;
    let inside = |i: i32, j: i32| i * i + j * j <= (n + 1) * (n + 1);
    for (li, layer) in stops.iter().enumerate() {
        if !on(layer.raster) || layer.levels.is_empty() {
            continue;
        }
        let lod = layer.lod(scale_);
        let mut values = vec![f64::NAN; side * side];
        for j in -n..=n {
            for i in (-n..=n).filter(|&i| inside(i, j)) {
                values[index(i, j)] = layer.field.field(node(i, j), lod);
            }
        }
        for j in -n..=n {
            for i in (-n..=n).filter(|&i| inside(i, j)) {
                for (di, dj) in [(1, 0), (0, 1)] {
                    let (i2, j2) = (i + di, j + dj);
                    if i2 > n || j2 > n || !inside(i2, j2) {
                        continue;
                    }
                    let near = f64::from(i * i + j * j)
                        .sqrt()
                        .min(f64::from(i2 * i2 + j2 * j2).sqrt());
                    let limit = best.map_or(reach, |b| b.dist);
                    if (near - 1.0) * h > limit {
                        continue;
                    }
                    let (va, vb) = (values[index(i, j)], values[index(i2, j2)]);
                    for (k, &level) in layer.levels.iter().enumerate() {
                        if (va >= level) == (vb >= level) {
                            continue;
                        }
                        let q = layer.crossing(node(i, j), node(i2, j2), lod, level);
                        let dist = angle(p, q);
                        if dist <= reach && best.is_none_or(|b| dist < b.dist) {
                            best = Some(StopHit {
                                layer: li,
                                level: k,
                                dir: q,
                                dist,
                            });
                        }
                    }
                }
            }
        }
    }
    let mut hit = best?;
    // The crossing of the grid is near the nearest point of the line, and
    // two steps along the line toward `p` find it.
    let layer = &stops[hit.layer];
    let (lod, level) = (layer.lod(scale_), layer.levels[hit.level]);
    for _ in 0..2 {
        let Some(t) = layer.tangent(hit.dir, lod, h / 4.0) else {
            break;
        };
        let foot = normalize(add(hit.dir, scale(t, dot(p, t))));
        let across = normalize(cross(t, foot));
        let a = normalize(add(foot, scale(across, -h)));
        let b = normalize(add(foot, scale(across, h)));
        if layer.up(a, lod, level) == layer.up(b, lod, level) {
            break;
        }
        let q = layer.crossing(a, b, lod, level);
        let dist = angle(p, q);
        if dist >= hit.dist {
            break;
        }
        (hit.dir, hit.dist) = (q, dist);
    }
    Some(hit)
}

/// A stroke that follows the line of a color stop. The lock marches along
/// the line from the snapped point both ways until the trace has a
/// direction, and then only forward.
pub struct StopLock {
    pub layer: usize,
    level: f64,
    lod: f64,
    scale: f64,
    /// The march, with the signed arc length of each point. The snapped
    /// point is at 0, and the arc length grows to the back.
    points: VecDeque<V3>,
    arcs: VecDeque<f64>,
    /// The direction of the line at each end of the march, away from the
    /// march. `None`: the line ends there, as at a flat part.
    front: Option<V3>,
    back: Option<V3>,
    from: f64,
    s: f64,
    /// 1 or -1 when the trace has a direction, else 0.
    dir: f64,
    /// The samples since the lock.
    pub raw: Vec<V3>,
}

impl StopLock {
    /// A lock at `hit`, at `scale` points per radian. `None`: the field is
    /// flat at the hit.
    pub fn new(layer: &StopTargets, hit: &StopHit, scale_: f64, raw: Vec<V3>) -> Option<Self> {
        let lod = layer.lod(scale_);
        let t = layer.tangent(hit.dir, lod, STEP / 2.0 / scale_)?;
        Some(StopLock {
            layer: hit.layer,
            level: layer.levels[hit.level],
            lod,
            scale: scale_,
            points: VecDeque::from([hit.dir]),
            arcs: VecDeque::from([0.0]),
            front: Some(scale(t, -1.0)),
            back: Some(t),
            from: 0.0,
            s: 0.0,
            dir: 0.0,
            raw,
        })
    }

    pub fn length(&self) -> f64 {
        (self.s - self.from).abs()
    }

    /// One step of the march from the point `last` in the direction `d`.
    /// Gives the next point and the direction there.
    fn step(&self, layer: &StopTargets, last: V3, d: V3) -> Option<(V3, V3)> {
        let mut h = STEP / self.scale;
        for _ in 0..RETRIES {
            let guess = normalize(add(last, scale(d, h)));
            let across = normalize(cross(guess, d));
            let a = normalize(add(guess, scale(across, -h)));
            let b = normalize(add(guess, scale(across, h)));
            if layer.up(a, self.lod, self.level) != layer.up(b, self.lod, self.level) {
                let q = layer.crossing(a, b, self.lod, self.level);
                let chord = add(q, scale(last, -1.0));
                if dot(chord, d) > 0.25 * h {
                    let t = add(chord, scale(q, -dot(chord, q)));
                    return Some((q, normalize(t)));
                }
            }
            h *= 0.5;
        }
        None
    }

    /// Marches on at the back or at the front.
    fn march(&mut self, layer: &StopTargets, back: bool) {
        let (last, d) = match back {
            true => (self.points[self.points.len() - 1], self.back),
            false => (self.points[0], self.front),
        };
        let Some(d) = d else {
            return;
        };
        let next = self.step(layer, last, d);
        let (end, arc) = match back {
            true => (&mut self.back, self.arcs[self.arcs.len() - 1]),
            false => (&mut self.front, self.arcs[0]),
        };
        *end = next.map(|n| n.1);
        let Some((q, _)) = next else {
            return;
        };
        let len = angle(last, q);
        if back {
            self.points.push_back(q);
            self.arcs.push_back(arc + len);
        } else {
            self.points.push_front(q);
            self.arcs.push_front(arc - len);
        }
    }

    fn lo(&self) -> f64 {
        self.arcs[0]
    }

    fn hi(&self) -> f64 {
        self.arcs[self.arcs.len() - 1]
    }

    /// The segment at the arc length `s`.
    fn segment(&self, s: f64) -> usize {
        let last = self.points.len().saturating_sub(2);
        self.arcs
            .partition_point(|&a| a <= s)
            .saturating_sub(1)
            .min(last)
    }

    fn point_at(&self, s: f64) -> V3 {
        if self.points.len() < 2 {
            return self.points[0];
        }
        let k = self.segment(s);
        let len = self.arcs[k + 1] - self.arcs[k];
        let t = ((s - self.arcs[k]) / len).clamp(0.0, 1.0);
        super::snap::slerp(self.points[k], self.points[k + 1], t)
    }

    /// The point of the march between the arc lengths `a` and `b` that is
    /// nearest to `p`: its arc length and its distance.
    fn nearest(&self, p: V3, a: f64, b: f64) -> (f64, f64) {
        let mut best = (a, angle(p, self.point_at(a)));
        if self.points.len() < 2 {
            return best;
        }
        for k in self.segment(a)..=self.segment(b) {
            let along = project(p, self.points[k], self.points[k + 1]).1;
            let s = (self.arcs[k] + along).clamp(a, b);
            let d = angle(p, self.point_at(s));
            if d < best.1 {
                best = (s, d);
            }
        }
        best
    }

    /// Moves the trace on along the line toward the pen at `p`, after a
    /// step of `step` radians. Gives the distance of the pen from the line.
    pub fn follow(&mut self, layer: &StopTargets, p: V3, step: f64, reach: f64, turn: f64) -> f64 {
        let w = step + 2.0 * reach;
        let mut budget = MAX_MARCH;
        while budget > 0 {
            let back = self.dir >= 0.0 && self.back.is_some() && self.hi() < self.s + w;
            let front = self.dir <= 0.0 && self.front.is_some() && self.lo() > self.s - w;
            if !back && !front {
                break;
            }
            if back {
                self.march(layer, true);
                budget -= 1;
            }
            if front && budget > 0 {
                self.march(layer, false);
                budget -= 1;
            }
        }
        let a = (self.s - w).max(self.lo());
        let b = (self.s + w).min(self.hi());
        let (s, dist) = self.nearest(p, a, b);
        if self.dir == 0.0 {
            self.s = s;
            if (s - self.from).abs() > turn {
                self.dir = (s - self.from).signum();
            }
        } else if (s - self.s) * self.dir > 0.0 {
            self.s = s;
        }
        dist
    }

    /// The point of the trace now.
    pub fn point(&self) -> V3 {
        self.point_at(self.s)
    }

    /// The traced vertices, from the lock to the point now, with the
    /// vertices near the simpler line removed.
    pub fn vertices(&self) -> Vec<PathVertex> {
        let (lo, hi) = (self.from.min(self.s), self.from.max(self.s));
        let mut path = vec![self.point_at(lo)];
        path.extend(
            self.points
                .iter()
                .zip(&self.arcs)
                .filter(|&(_, &a)| a > lo && a < hi)
                .map(|(&q, _)| q),
        );
        if hi > lo {
            path.push(self.point_at(hi));
        }
        if self.s < self.from {
            path.reverse();
        }
        simplify(&path, TOLERANCE / self.scale)
            .into_iter()
            .map(PathVertex::Free)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::time::Instant;

    use vale_store::{Geometry, GeometryKind, LayerId, LonLat};
    use vale_terrain::{FACES, TexelRect};

    use super::*;
    use crate::globe::math::lonlat_to_dir;
    use crate::globe::polygon;
    use crate::globe::snap::tests::{kinds, layer, pen};
    use crate::globe::snap::{Kind, LayerTargets, Run, SnapSettings, Source, Targets, Tracer};

    /// Points per radian: a sphere with a radius of 300 points on screen.
    const SCALE: f64 = 300.0;
    const RASTER: RasterId = RasterId(3);

    /// A heightmap whose level is 0.5 + 0.3 times the cosine of the angle
    /// from `c`. The line of each stop is a circle around `c`.
    fn cap(c: V3) -> Heightmap {
        let n = 512;
        let mut map = Heightmap::new(n, 0);
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        for face in 0..FACES {
            let mut data = Vec::with_capacity(n * n);
            for y in 0..n {
                for x in 0..n {
                    let d = map.texel_dir(face, x, y);
                    data.push(((0.5 + 0.3 * dot(d, c)) * 65535.0).round() as u16);
                }
            }
            map.store_rect(face, full, &data);
        }
        map
    }

    /// The level of the circle of `radius` degrees around the center.
    fn level(radius: f64) -> f64 {
        0.5 + 0.3 * radius.to_radians().cos()
    }

    fn stop(c: V3, radius: f64) -> Arc<StopTargets> {
        let smoothing = Smoothing {
            on: true,
            feature_size: 2.0,
        };
        Arc::new(StopTargets::new(RASTER, cap(c), 512, smoothing, [level(radius)]).unwrap())
    }

    fn targets(stop: &Arc<StopTargets>, vectors: &[vale_store::Layer]) -> Arc<Targets> {
        let layers = vectors
            .iter()
            .map(|l| Arc::new(LayerTargets::build(l.id, l)))
            .collect();
        Arc::new(Targets::new(layers, vec![stop.clone()]))
    }

    /// True if `d` is within 0.001 points of the line, at `scale`: the
    /// field differs from the level by less than its slope over that
    /// distance.
    fn on_line(t: &StopTargets, d: V3, c: V3, scale: f64) -> bool {
        let lod = t.lod(scale);
        let slope = 0.3 * angle(d, c).sin();
        (t.field().field(d, lod) - t.levels()[0]).abs() < 2.0 * slope * 1e-3 / scale
    }

    fn north(lon: f64, lat: f64, pts: f64, scale: f64) -> V3 {
        lonlat_to_dir(lon, lat + (pts / scale).to_degrees())
    }

    fn traced(runs: &[Run]) -> Vec<V3> {
        let mut out = Vec::new();
        for r in runs {
            if let Run::Traced(v) = r {
                for p in v {
                    let PathVertex::Free(d) = *p else {
                        panic!("a trace of a stop has no stored vertex");
                    };
                    out.push(d);
                }
            }
        }
        out
    }

    fn run(t: &Arc<Targets>, samples: &[V3], s: SnapSettings) -> Vec<Run> {
        let mut tracer = Tracer::new(Arc::clone(t), s, SCALE, LayerId(9));
        for &p in samples {
            tracer.push(p);
        }
        tracer.finish()
    }

    fn length(path: &[V3]) -> f64 {
        path.windows(2).map(|w| angle(w[0], w[1])).sum()
    }

    /// Pen samples around a circle of `radius` degrees about `c`, from the
    /// angle `from` to `to` in turns, about half a point apart. The pen
    /// comes in from 4 degrees outside, and it goes 8 degrees out at the end, and it wiggles
    /// by `wiggle` degrees.
    fn around(c: V3, radius: f64, from: f64, to: f64, wiggle: f64) -> Vec<V3> {
        let axis = if c[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let u = normalize(cross(c, axis));
        let v = cross(c, u);
        let at = |turn: f64, r: f64| {
            let (a, r) = (turn * TAU, r.to_radians());
            let side = add(scale(u, a.cos()), scale(v, a.sin()));
            normalize(add(scale(c, r.cos()), scale(side, r.sin())))
        };
        let step = 0.5 / SCALE / (radius.to_radians().sin() * TAU);
        let mut out = Vec::new();
        for i in 0..20 {
            out.push(at(from, radius + 4.0 - 0.2 * f64::from(i)));
        }
        let n = ((to - from) / step).ceil() as usize;
        for i in 0..=n {
            let t = from + (to - from) * i as f64 / n as f64;
            out.push(at(t, radius + wiggle * (i as f64 * 0.05).sin()));
        }
        for i in 1..=20 {
            out.push(at(to, radius + 0.4 * f64::from(i)));
        }
        out
    }

    #[test]
    fn a_position_snaps_to_the_line_of_a_stop() {
        let c = lonlat_to_dir(0.0, 90.0);
        let t = stop(c, 90.0);
        let s = SnapSettings::default();
        // A point north of the line, and a line north of it.
        let point = layer(
            1,
            GeometryKind::Point,
            vec![Geometry::Points(vec![[5.0, (11.0 / SCALE).to_degrees()]])],
        );
        let lat = (10.5 / SCALE).to_degrees();
        let part: Vec<LonLat> = vec![[20.0, lat], [30.0, lat]];
        let line = layer(2, GeometryKind::Line, vec![Geometry::Lines(vec![part])]);
        let all = targets(&t, &[point, line]);
        for scale in [SCALE, 4.0 * SCALE] {
            let only = targets(&t, &[]);
            let p = north(10.0, 0.0, 8.0, scale);
            let snap = only.snap(p, scale, &s, None).unwrap();
            assert_eq!(snap.kind, Kind::Stop);
            assert_eq!(
                snap.source,
                Source::Stop {
                    raster: RASTER,
                    stop: 0
                }
            );
            assert!(on_line(&t, snap.dir, c, scale));
            // The snap is the nearest point of the line: 8 points south.
            assert!((angle(p, snap.dir) * scale - 8.0).abs() < 0.05);
            assert_eq!(
                only.snap(north(10.0, 0.0, 13.0, scale), scale, &s, None),
                None
            );
            let mut off = s.clone();
            off.set_stops(RASTER, false);
            assert!(!off.stops(RASTER));
            assert_eq!(only.snap(p, scale, &off, None), None);
        }
        // A vertex wins over a stop, and the nearer of an edge and a stop
        // wins.
        let snap = |p| all.snap(p, SCALE, &s, None).unwrap().kind;
        assert_eq!(snap(north(5.0, 0.0, 3.0, SCALE)), Kind::Vertex);
        assert_eq!(snap(north(25.0, 0.0, 3.0, SCALE)), Kind::Stop);
        assert_eq!(snap(north(25.0, 0.0, 7.0, SCALE)), Kind::Edge);
    }

    #[test]
    fn a_stroke_traces_the_line_of_a_stop_and_leaves_it() {
        let c = lonlat_to_dir(0.0, 90.0);
        let t = stop(c, 90.0);
        let all = targets(&t, &[]);
        // The wiggle is about 3 points each way.
        let samples = pen(&[[0.0, 6.0], [4.0, 0.0], [20.0, 0.0], [24.0, 8.0]], 0.5);
        let runs = run(&all, &samples, SnapSettings::default());
        assert_eq!(kinds(&runs), "FTF");
        let line = traced(&runs);
        for &d in &line {
            assert!(on_line(&t, d, c, SCALE));
        }
        let lon = |d: V3| d[1].atan2(d[0]).to_degrees();
        assert!((3.0..8.0).contains(&lon(line[0])));
        let end = lon(line[line.len() - 1]);
        assert!((16.0..21.0).contains(&end), "{end}");
        // The runs meet, and the last run goes to the pen.
        let (Run::Free(a), Run::Free(b)) = (&runs[0], &runs[2]) else {
            panic!("not free");
        };
        assert_eq!(a[a.len() - 1], line[0]);
        assert_eq!(b[0], line[line.len() - 1]);
        assert_eq!(b[b.len() - 1], samples[samples.len() - 1]);

        // A short crossing does not trace.
        let samples = pen(&[[4.0, -5.0], [5.0, 5.0]], 0.0);
        let runs = run(&all, &samples, SnapSettings::default());
        assert_eq!(runs, [Run::Free(samples)]);
    }

    #[test]
    fn a_polygon_traces_the_line_of_a_stop() {
        let c = lonlat_to_dir(0.0, 90.0);
        let t = stop(c, 90.0);
        let all = targets(&t, &[]);
        // A shape south of the line, with its north side on the line.
        let way = [
            [0.5, -6.0],
            [0.6, -0.1],
            [9.6, -0.1],
            [9.6, -6.0],
            [0.5, -6.0],
        ];
        let runs = run(&all, &pen(&way, 0.3), SnapSettings::default());
        assert!(kinds(&runs).contains('T'));
        let Some(Geometry::Polygons(p)) = polygon::commit(&runs, SCALE, 0.35) else {
            panic!("no polygon");
        };
        assert_eq!(p.len(), 1);
        let on: Vec<LonLat> = p[0][0]
            .iter()
            .filter(|ll| on_line(&t, lonlat_to_dir(ll[0], ll[1]), c, SCALE))
            .copied()
            .collect();
        assert!(on.len() > 2, "{on:?}");
        assert!(on.iter().any(|ll| ll[0] < 3.0) && on.iter().any(|ll| ll[0] > 7.0));
    }

    #[test]
    fn a_stroke_traces_a_closed_line_past_its_start() {
        let c = lonlat_to_dir(30.0, 40.0);
        let t = stop(c, 5.0);
        let all = targets(&t, &[]);
        let samples = around(c, 5.0, 0.0, 1.2, 0.3);
        let runs = run(&all, &samples, SnapSettings::default());
        assert_eq!(kinds(&runs), "FTF");
        let line = traced(&runs);
        let lap = TAU * 5f64.to_radians().sin();
        assert!(length(&line) > lap, "{} {lap}", length(&line));
    }

    #[test]
    fn a_trace_has_no_gap_or_step_at_a_face_edge_or_a_cube_corner() {
        let corner = normalize([1.0, 1.0, 1.0]);
        // A circle around a cube corner crosses three face edges. The
        // other circle passes 0.2 degrees from the corner.
        let side = normalize(cross(corner, [0.0, 0.0, 1.0]));
        let near = normalize(add(corner, scale(side, 6.2f64.to_radians().tan())));
        for c in [corner, near] {
            let t = stop(c, 6.0);
            let all = targets(&t, &[]);
            let samples = around(c, 6.0, 0.0, 1.1, 0.2);
            let runs = run(&all, &samples, SnapSettings::default());
            assert_eq!(kinds(&runs), "FTF");
            let line = traced(&runs);
            let lap = TAU * 6f64.to_radians().sin();
            assert!(length(&line) > lap);
            // Each vertex is on the line, and the middle of each segment
            // is within 0.15 points of it. The line is near the circle: the
            // smoothing moves it a little, and more at a cube corner.
            let lod = t.lod(SCALE);
            let near = |d: V3, pts: f64| {
                let slope = 0.3 * angle(d, c).sin();
                (t.field().field(d, lod) - t.levels()[0]).abs() < 1.5 * slope * pts / SCALE
            };
            for w in line.windows(2) {
                assert!(on_line(&t, w[0], c, SCALE));
                assert!(near(normalize(add(w[0], w[1])), 0.15));
                assert!((angle(w[0], c).to_degrees() - 6.0).abs() < 0.1);
            }
        }
    }

    #[test]
    fn a_fast_trace_keeps_up_with_the_pen() {
        let c = lonlat_to_dir(0.0, 90.0);
        let t = stop(c, 90.0);
        let all = targets(&t, &[]);
        // 30 points for each sample: 3,600 points in a second at 120
        // samples a second.
        let step = (30.0 / SCALE).to_degrees();
        let samples: Vec<V3> = (0..40)
            .map(|i| lonlat_to_dir(f64::from(i) * step, 0.0))
            .collect();
        let mut tracer = Tracer::new(all, SnapSettings::default(), SCALE, LayerId(9));
        let mut worst = 0f64;
        for (i, &p) in samples.iter().enumerate() {
            let filled = t.field().filled();
            let start = Instant::now();
            tracer.push(p);
            worst = worst.max(start.elapsed().as_secs_f64());
            if i > 0 {
                assert!(t.field().filled() - filled <= 16);
                let at = tracer.highlight().unwrap().point.unwrap();
                assert!(angle(at, p) * SCALE < 1.0, "{i}");
            }
        }
        eprintln!("the slowest sample took {:.2} ms", worst * 1e3);
        let runs = tracer.finish();
        assert!(kinds(&runs).contains('T'));
    }
}
