//! Simplified copies of a vector layer at a few levels of detail.
//!
//! The boundaries of the layer split into arcs between the points where
//! three or more features meet. Each level simplifies each arc one time,
//! and each feature that has the arc takes the same points. So two polygons
//! that share an edge also share it at each level. A level simplifies the
//! points of the next finer level, so each level keeps a subset of the
//! points of the finer level.
//!
//! The input points keep their values in the output. Two points are the
//! same point when their longitude and latitude are equal after -180°
//! becomes 180° and the longitude at a pole becomes 0°. A ring can repeat
//! its first point at the end or not, and the output ring does the same.
//! An output ring can start at a different point of the ring.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use crate::frame::LonLat;
use crate::measure::{V3, arcs_cross, cross, dot, norm, vec as unit};

/// Level sizes on an Earth-size world.
pub const LEVEL_KM: [f64; 4] = [1.0, 4.0, 16.0, 64.0];
pub const LEVELS: usize = 4;

const EARTH_KM: f64 = 6371.0;
/// The longest step in longitude that a level makes between two points
/// that were not neighbors in the input.
const MAX_STEP_DEG: f64 = 45.0;
const NONE: u32 = u32::MAX;

/// Tolerance of each level in radians: LEVEL_KM / 6371.
pub fn tolerance(level: usize) -> f64 {
    LEVEL_KM[level] / EARTH_KM
}

/// The geometry of one feature.
pub enum Shape<'a> {
    Points,
    Lines(&'a [Vec<LonLat>]),
    /// Polygons, each with its outer ring first and then its holes.
    Polygons(&'a [Vec<Vec<LonLat>>]),
}

/// The geometry of one feature at a level.
#[derive(Clone, Debug, PartialEq)]
pub enum Simple {
    /// The full geometry of the feature.
    Same,
    /// The feature is smaller than the tolerance.
    Gone,
    Lines(Vec<Vec<LonLat>>),
    Polygons(Vec<Vec<Vec<LonLat>>>),
}

#[derive(Clone, Debug)]
pub struct Level {
    pub index: usize,
    pub tolerance: f64,
    /// One entry for each input feature, in the input order.
    pub features: Vec<Simple>,
}

/// Builds the levels, finest first, and calls `done` with each level as soon as it is ready.
pub fn build(shapes: &[Shape], mut done: impl FnMut(Level)) {
    let graph = Graph::new(shapes);
    let mut finer: Vec<Vec<u32>> = graph
        .arcs
        .iter()
        .map(|a| (0..a.verts.len() as u32).collect())
        .collect();
    for index in 0..LEVELS {
        let tol = tolerance(index);
        let (kept, keep) = graph.simplify(&finer, tol);
        done(Level {
            index,
            tolerance: tol,
            features: graph.features(shapes, &kept, &keep),
        });
        finer = kept;
    }
}

/// The level to draw for a pixel of `pixel` radians: the coarsest level with a tolerance below one pixel among the ready ones, else the next finer ready level; None means the full geometry.
pub fn choose(pixel: f64, ready: [bool; LEVELS]) -> Option<usize> {
    let want = (0..LEVELS).rev().find(|&l| tolerance(l) < pixel)?;
    (0..=want).rev().find(|&l| ready[l])
}

/// A fast hash for integer keys.
#[derive(Default)]
struct Mix(u64);

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(26) ^ i).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }

    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }

    fn finish(&self) -> u64 {
        let x = (self.0 ^ (self.0 >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^ (x >> 33)
    }
}

type Map<K, V> = HashMap<K, V, BuildHasherDefault<Mix>>;

fn key(p: LonLat) -> (u64, u64) {
    let (mut lon, lat) = (p[0], p[1]);
    if lon == -180.0 {
        lon = 180.0;
    }
    if lat.abs() == 90.0 {
        lon = 0.0;
    }
    // Adding 0.0 makes -0.0 into 0.0.
    ((lon + 0.0).to_bits(), (lat + 0.0).to_bits())
}

fn edge_key(a: u32, b: u32) -> u64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    ((lo as u64) << 32) | hi as u64
}

/// One coordinate of a grid cell, with room for the cells below 0.
fn cell(i: i64) -> u64 {
    (i + 1) as u64 & 0x1f_ffff
}

fn lon_step(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(360.0 - d)
}

/// The sine of the distance from `p` to the arc from `a` to `b`, or 1 when
/// the distance is more than 90°.
fn off(p: V3, a: V3, b: V3, n: V3, nn: f64) -> f64 {
    if nn > 1e-15 && dot(cross(a, p), n) >= 0.0 && dot(cross(p, b), n) >= 0.0 {
        return dot(p, n).abs() / nn;
    }
    near(p, a).min(near(p, b))
}

fn near(p: V3, q: V3) -> f64 {
    if dot(p, q) < 0.0 {
        1.0
    } else {
        norm(cross(p, q))
    }
}

fn angle(a: V3, b: V3) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

/// A path between two junctions, or a closed loop with no junction. Its
/// points are vertex ids.
struct Arc {
    verts: Vec<u32>,
    /// Whether the arc keeps its farthest point at each level: a loop, or
    /// one of two arcs with the same two ends.
    force: bool,
}

/// The use of an arc by a ring or a line. Point `i` of the arc is at
/// position `pos0 + i` of the path, or `pos0 - i` when reversed, modulo
/// the length of the path.
struct Occ {
    arc: u32,
    rev: bool,
    pos0: u32,
}

/// A ring or a line of the input.
struct Path {
    /// The polygon or line index in the feature.
    part: u32,
    /// The ring index in the polygon.
    ring: u32,
    is_ring: bool,
    /// Whether the input ring repeats its first point at the end.
    closed: bool,
    /// The input index of each position, with repeated points removed.
    orig: Vec<u32>,
    occ: Vec<Occ>,
    /// The smallest dot product of the first point with each point.
    min_dot: f64,
    /// Whether the path has enough points to be part of the arcs.
    graph: bool,
}

struct Feature {
    paths: std::ops::Range<usize>,
}

struct Graph {
    v: Vec<V3>,
    lon: Vec<f64>,
    arcs: Vec<Arc>,
    paths: Vec<Path>,
    features: Vec<Feature>,
}

/// A segment of a simplified arc, between positions `lo` and `hi` of the
/// finer points of the arc.
#[derive(Clone, Copy)]
struct Seg {
    arc: u32,
    lo: u32,
    hi: u32,
    a: u32,
    b: u32,
    alive: bool,
}

fn input_path<'a>(shape: &'a Shape, part: u32, ring: u32) -> &'a [LonLat] {
    match shape {
        Shape::Points => &[],
        Shape::Lines(lines) => &lines[part as usize],
        Shape::Polygons(polys) => &polys[part as usize][ring as usize],
    }
}

impl Graph {
    fn new(shapes: &[Shape]) -> Self {
        let mut g = Graph {
            v: Vec::new(),
            lon: Vec::new(),
            arcs: Vec::new(),
            paths: Vec::new(),
            features: Vec::new(),
        };
        let mut ids: Map<(u64, u64), u32> = Map::default();
        let mut path_ids: Vec<Vec<u32>> = Vec::new();
        for shape in shapes {
            let start = g.paths.len();
            let mut add = |part: u32, ring: u32, is_ring: bool, src: &[LonLat]| {
                let mut orig: Vec<u32> = Vec::with_capacity(src.len());
                for (i, p) in src.iter().enumerate() {
                    if orig.last().is_none_or(|&j| src[j as usize] != *p) {
                        orig.push(i as u32);
                    }
                }
                let closed = is_ring && src.len() > 1 && src[0] == src[src.len() - 1];
                if closed && orig.len() > 1 {
                    orig.pop();
                }
                let pts: Vec<u32> = orig
                    .iter()
                    .map(|&i| {
                        let p = src[i as usize];
                        let n = g.v.len() as u32;
                        *ids.entry(key(p)).or_insert_with(|| {
                            let k = key(p);
                            let q = [f64::from_bits(k.0), f64::from_bits(k.1)];
                            g.v.push(unit(q));
                            g.lon.push(q[0]);
                            n
                        })
                    })
                    .collect();
                let first = pts.first().map(|&i| g.v[i as usize]);
                let min_dot = pts
                    .iter()
                    .map(|&i| dot(first.unwrap(), g.v[i as usize]))
                    .fold(1.0, f64::min);
                let graph = orig.len() >= if is_ring { 3 } else { 2 };
                g.paths.push(Path {
                    part,
                    ring,
                    is_ring,
                    closed,
                    orig,
                    occ: Vec::new(),
                    min_dot,
                    graph,
                });
                path_ids.push(if graph { pts } else { Vec::new() });
            };
            match shape {
                Shape::Points => {}
                Shape::Lines(lines) => {
                    for (i, line) in lines.iter().enumerate() {
                        add(i as u32, 0, false, line);
                    }
                }
                Shape::Polygons(polys) => {
                    for (i, poly) in polys.iter().enumerate() {
                        for (j, ring) in poly.iter().enumerate() {
                            add(i as u32, j as u32, true, ring);
                        }
                    }
                }
            }
            g.features.push(Feature {
                paths: start..g.paths.len(),
            });
        }

        let n = g.v.len();
        let mut junction = vec![false; n];
        let mut edges: Map<u64, u32> = Map::default();
        edges.reserve(path_ids.iter().map(Vec::len).sum());
        for (path, pts) in g.paths.iter().zip(&path_ids) {
            if pts.is_empty() {
                continue;
            }
            let m = pts.len();
            let count = if path.is_ring { m } else { m - 1 };
            for i in 0..count {
                let (a, b) = (pts[i], pts[(i + 1) % m]);
                if a == b {
                    junction[a as usize] = true;
                }
                edges.entry(edge_key(a, b)).or_insert(NONE);
            }
            if !path.is_ring {
                junction[pts[0] as usize] = true;
                junction[pts[m - 1] as usize] = true;
            }
        }
        let mut degree = vec![0u8; n];
        for &k in edges.keys() {
            let (lo, hi) = ((k >> 32) as usize, (k & 0xffff_ffff) as usize);
            if lo != hi {
                degree[lo] = degree[lo].saturating_add(1);
                degree[hi] = degree[hi].saturating_add(1);
            }
        }
        for (j, d) in junction.iter_mut().zip(&degree) {
            *j |= *d != 2;
        }

        for (p, pts) in path_ids.iter().enumerate() {
            if !pts.is_empty() {
                let is_ring = g.paths[p].is_ring;
                g.paths[p].occ = g.walk(pts, is_ring, &junction, &mut edges);
            }
        }

        let mut ends: Map<u64, u32> = Map::default();
        for arc in &g.arcs {
            if arc.verts.len() > 2 {
                *ends
                    .entry(edge_key(arc.verts[0], *arc.verts.last().unwrap()))
                    .or_default() += 1;
            }
        }
        for arc in &mut g.arcs {
            let (a, b) = (arc.verts[0], *arc.verts.last().unwrap());
            arc.force = arc.verts.len() > 2 && (a == b || ends[&edge_key(a, b)] >= 2);
        }
        g
    }

    /// Splits a path into arcs, and adds the arcs that are new.
    fn walk(
        &mut self,
        pts: &[u32],
        is_ring: bool,
        junction: &[bool],
        edges: &mut Map<u64, u32>,
    ) -> Vec<Occ> {
        let n = pts.len();
        let starts: Vec<usize> = (0..n).filter(|&i| junction[pts[i] as usize]).collect();
        let mut occ = Vec::new();
        if is_ring && starts.is_empty() {
            let s = (0..n).min_by_key(|&i| pts[i]).unwrap();
            let seq: Vec<u32> = (0..=n).map(|t| pts[(s + t) % n]).collect();
            occ.push(self.occ(seq, s, edges));
            return occ;
        }
        let k = starts.len();
        let pairs = if is_ring { k } else { k - 1 };
        for w in 0..pairs {
            let a = starts[w];
            let b = if w + 1 < k {
                starts[w + 1]
            } else {
                starts[0] + n
            };
            let seq: Vec<u32> = (a..=b).map(|t| pts[t % n]).collect();
            occ.push(self.occ(seq, a, edges));
        }
        occ
    }

    fn occ(&mut self, seq: Vec<u32>, pos: usize, edges: &mut Map<u64, u32>) -> Occ {
        let fwd = seq[0] <= seq[1];
        let found = edges[&edge_key(seq[0], seq[1])];
        if found != NONE {
            let arc = found >> 1;
            let rev = (found & 1 == 1) != fwd;
            debug_assert_eq!(self.arcs[arc as usize].verts.len(), seq.len());
            let pos0 = if rev { pos + seq.len() - 1 } else { pos };
            return Occ {
                arc,
                rev,
                pos0: pos0 as u32,
            };
        }
        let arc = self.arcs.len() as u32;
        for w in seq.windows(2) {
            edges.insert(edge_key(w[0], w[1]), (arc << 1) | (w[0] <= w[1]) as u32);
        }
        self.arcs.push(Arc {
            verts: seq,
            force: false,
        });
        Occ {
            arc,
            rev: false,
            pos0: pos as u32,
        }
    }

    fn point(&self, arc: usize, finer: &[u32], pos: usize) -> u32 {
        self.arcs[arc].verts[finer[pos] as usize]
    }

    /// The finer point between `lo` and `hi` that is farthest from the
    /// segment from `lo` to `hi`, and the sine of its distance.
    fn farthest(&self, arc: usize, finer: &[u32], lo: usize, hi: usize) -> (usize, f64) {
        let verts = &self.arcs[arc].verts;
        let a = self.v[verts[finer[lo] as usize] as usize];
        let b = self.v[verts[finer[hi] as usize] as usize];
        let n = cross(a, b);
        let nn = norm(n);
        let mut best = (lo + 1, -1.0);
        for (i, &f) in finer.iter().enumerate().take(hi).skip(lo + 1) {
            let d = off(self.v[verts[f as usize] as usize], a, b, n, nn);
            if d > best.1 {
                best = (i, d);
            }
        }
        best
    }

    /// Douglas-Peucker on the spans of an arc.
    fn refine(
        &self,
        arc: usize,
        finer: &[u32],
        mask: &mut [bool],
        spans: &mut Vec<(usize, usize)>,
        sin_tol: f64,
    ) {
        while let Some((lo, hi)) = spans.pop() {
            if hi <= lo + 1 {
                continue;
            }
            let (best, d) = self.farthest(arc, finer, lo, hi);
            let (a, b) = (self.point(arc, finer, lo), self.point(arc, finer, hi));
            if d > sin_tol || lon_step(self.lon[a as usize], self.lon[b as usize]) > MAX_STEP_DEG {
                mask[best] = true;
                spans.push((lo, best));
                spans.push((best, hi));
            }
        }
    }

    /// Puts back the finer point that is farthest from the span, and the
    /// points that the span then needs to stay within the tolerance.
    fn put_back(
        &self,
        arc: usize,
        finer: &[u32],
        mask: &mut [bool],
        lo: usize,
        hi: usize,
        sin_tol: f64,
    ) -> bool {
        if hi <= lo + 1 {
            return false;
        }
        let (best, _) = self.farthest(arc, finer, lo, hi);
        mask[best] = true;
        self.refine(arc, finer, mask, &mut vec![(lo, best), (best, hi)], sin_tol);
        true
    }

    /// Puts back the finer point that is farthest from its span, in the
    /// span where that distance is largest.
    fn put_back_best(&self, arc: usize, finer: &[u32], mask: &mut [bool], sin_tol: f64) -> bool {
        let marks: Vec<usize> = (0..mask.len()).filter(|&i| mask[i]).collect();
        let best = marks
            .windows(2)
            .filter(|w| w[1] > w[0] + 1)
            .map(|w| (w[0], w[1], self.farthest(arc, finer, w[0], w[1]).1))
            .max_by(|x, y| x.2.total_cmp(&y.2));
        match best {
            Some((lo, hi, _)) => self.put_back(arc, finer, mask, lo, hi, sin_tol),
            None => false,
        }
    }

    /// Whether each path is in the level.
    fn keep(&self, cos_tol: f64) -> Vec<bool> {
        let mut keep = vec![false; self.paths.len()];
        let mut outer = false;
        for (k, p) in self.paths.iter().enumerate() {
            let big = p.min_dot <= cos_tol;
            if p.is_ring && p.ring > 0 {
                keep[k] = outer && big;
            } else {
                keep[k] = big;
                outer = big;
            }
        }
        keep
    }

    /// The finer points that each arc keeps at the tolerance `tol`, and
    /// whether each path is in the level.
    fn simplify(&self, finer: &[Vec<u32>], tol: f64) -> (Vec<Vec<u32>>, Vec<bool>) {
        let sin_tol = tol.sin();
        let mut masks: Vec<Vec<bool>> = Vec::with_capacity(self.arcs.len());
        let mut spans = Vec::new();
        for (a, arc) in self.arcs.iter().enumerate() {
            let f = &finer[a];
            let last = f.len() - 1;
            let mut mask = vec![false; f.len()];
            mask[0] = true;
            mask[last] = true;
            if arc.force && last > 1 {
                let (best, _) = self.farthest(a, f, 0, last);
                mask[best] = true;
                spans.push((0, best));
                spans.push((best, last));
            } else {
                spans.push((0, last));
            }
            self.refine(a, f, &mut mask, &mut spans, sin_tol);
            masks.push(mask);
        }

        let keep = self.keep(tol.cos());
        let mut active = vec![false; self.arcs.len()];
        for (p, path) in self.paths.iter().enumerate() {
            if !keep[p] || !path.graph {
                continue;
            }
            for o in &path.occ {
                active[o.arc as usize] = true;
            }
            if !path.is_ring {
                continue;
            }
            // A ring keeps at least 3 points.
            loop {
                let count: usize = path
                    .occ
                    .iter()
                    .map(|o| masks[o.arc as usize].iter().filter(|&&m| m).count() - 1)
                    .sum();
                if count >= 3 {
                    break;
                }
                let mut more = false;
                for o in &path.occ {
                    let a = o.arc as usize;
                    more |= self.put_back_best(a, &finer[a], &mut masks[a], sin_tol);
                }
                if !more {
                    break;
                }
            }
        }

        self.untangle(finer, &mut masks, &active, sin_tol);

        let kept = masks
            .iter()
            .zip(finer)
            .map(|(m, f)| (0..m.len()).filter(|&i| m[i]).map(|i| f[i]).collect())
            .collect();
        (kept, keep)
    }

    fn seg_cells(&self, s: &Seg, size: f64, out: &mut Vec<u64>) {
        out.clear();
        let (a, b) = (self.v[s.a as usize], self.v[s.b as usize]);
        let steps = (angle(a, b) / size).ceil().max(1.0) as usize;
        let mut prev = a;
        for t in 1..=steps {
            let w = t as f64 / steps as f64;
            let q = [
                a[0] + (b[0] - a[0]) * w,
                a[1] + (b[1] - a[1]) * w,
                a[2] + (b[2] - a[2]) * w,
            ];
            let l = norm(q);
            let p = if l > 0.0 {
                [q[0] / l, q[1] / l, q[2] / l]
            } else {
                b
            };
            // The arc bulges out from its chord by at most this much.
            let pad = 1.0 - (angle(prev, p) / 2.0).cos() + 1e-12;
            let mut lo = [0i64; 3];
            let mut hi = [0i64; 3];
            for d in 0..3 {
                lo[d] = ((prev[d].min(p[d]) - pad + 1.0) / size).floor() as i64;
                hi[d] = ((prev[d].max(p[d]) + pad + 1.0) / size).floor() as i64;
            }
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        out.push((cell(x) << 42) | (cell(y) << 21) | cell(z));
                    }
                }
            }
            prev = p;
        }
        out.sort_unstable();
        out.dedup();
    }

    fn cross_segs(&self, s: &Seg, t: &Seg) -> bool {
        if s.a == s.b || t.a == t.b || s.a == t.a || s.a == t.b || s.b == t.a || s.b == t.b {
            return false;
        }
        let v = &self.v;
        arcs_cross(
            v[s.a as usize],
            v[s.b as usize],
            v[t.a as usize],
            v[t.b as usize],
        )
    }

    fn segs_of(&self, arc: usize, finer: &[u32], mask: &[bool], lo: usize, hi: usize) -> Vec<Seg> {
        let marks: Vec<usize> = (lo..=hi).filter(|&i| mask[i]).collect();
        marks
            .windows(2)
            .map(|w| Seg {
                arc: arc as u32,
                lo: w[0] as u32,
                hi: w[1] as u32,
                a: self.point(arc, finer, w[0]),
                b: self.point(arc, finer, w[1]),
                alive: true,
            })
            .collect()
    }

    /// Puts back points where two simplified segments cross, until no
    /// segments cross. Two segments that left out no finer point stay.
    fn untangle(&self, finer: &[Vec<u32>], masks: &mut [Vec<bool>], active: &[bool], sin_tol: f64) {
        let mut segs: Vec<Seg> = Vec::new();
        for a in (0..self.arcs.len()).filter(|&a| active[a]) {
            segs.extend(self.segs_of(a, &finer[a], &masks[a], 0, masks[a].len() - 1));
        }
        if segs.is_empty() {
            return;
        }
        let total: f64 = segs
            .iter()
            .map(|s| angle(self.v[s.a as usize], self.v[s.b as usize]))
            .sum();
        let size = (2.0 * total / segs.len() as f64).clamp(4e-6, 0.5);

        let mut cells = Vec::new();
        let mut grid: Vec<(u64, u32)> = Vec::new();
        for (i, s) in segs.iter().enumerate() {
            self.seg_cells(s, size, &mut cells);
            grid.extend(cells.iter().map(|&c| (c, i as u32)));
        }
        grid.sort_unstable();
        let mut queue: Vec<(u32, u32)> = Vec::new();
        for group in grid.chunk_by(|x, y| x.0 == y.0) {
            for (i, x) in group.iter().enumerate() {
                for y in &group[i + 1..] {
                    if self.cross_segs(&segs[x.1 as usize], &segs[y.1 as usize]) {
                        queue.push((x.1, y.1));
                    }
                }
            }
        }

        let mut extra: Map<u64, Vec<u32>> = Map::default();
        let mut new: Vec<u32> = Vec::new();
        while let Some((s1, s2)) = queue.pop() {
            if !segs[s1 as usize].alive || !segs[s2 as usize].alive {
                continue;
            }
            new.clear();
            for s in [s1, s2] {
                let seg = segs[s as usize];
                let (a, lo, hi) = (seg.arc as usize, seg.lo as usize, seg.hi as usize);
                if self.put_back(a, &finer[a], &mut masks[a], lo, hi, sin_tol) {
                    segs[s as usize].alive = false;
                    for part in self.segs_of(a, &finer[a], &masks[a], lo, hi) {
                        new.push(segs.len() as u32);
                        segs.push(part);
                    }
                }
            }
            for &n in &new {
                self.seg_cells(&segs[n as usize], size, &mut cells);
                for &c in &cells {
                    extra.entry(c).or_default().push(n);
                }
            }
            for &n in &new {
                let s = segs[n as usize];
                self.seg_cells(&s, size, &mut cells);
                for &c in &cells {
                    let start = grid.partition_point(|e| e.0 < c);
                    let fixed = grid[start..].iter().take_while(|e| e.0 == c).map(|e| e.1);
                    let added = extra.get(&c).into_iter().flatten().copied();
                    for t in fixed.chain(added) {
                        let other = &segs[t as usize];
                        if t != n && other.alive && self.cross_segs(&s, other) {
                            queue.push((n, t));
                        }
                    }
                }
            }
        }
    }

    fn emit(&self, path: &Path, kept: &[Vec<u32>], src: &[LonLat]) -> Vec<LonLat> {
        if !path.graph {
            return src.to_vec();
        }
        let n = path.orig.len() as i64;
        let at = |pos: i64| src[path.orig[pos.rem_euclid(n) as usize] as usize];
        let mut out = Vec::new();
        for o in &path.occ {
            let ks = &kept[o.arc as usize];
            let p0 = o.pos0 as i64;
            if o.rev {
                out.extend(ks[1..].iter().rev().map(|&i| at(p0 - i as i64)));
            } else {
                out.extend(ks[..ks.len() - 1].iter().map(|&i| at(p0 + i as i64)));
            }
        }
        if !path.is_ring {
            out.push(at(n - 1));
        } else if path.closed {
            out.push(out[0]);
        }
        out
    }

    fn features(&self, shapes: &[Shape], kept: &[Vec<u32>], keep: &[bool]) -> Vec<Simple> {
        let mut out = Vec::with_capacity(shapes.len());
        for (shape, f) in shapes.iter().zip(&self.features) {
            if matches!(shape, Shape::Points) {
                out.push(Simple::Same);
                continue;
            }
            let mut same = true;
            let mut lines = Vec::new();
            let mut polys: Vec<Vec<Vec<LonLat>>> = Vec::new();
            for p in f.paths.clone() {
                let path = &self.paths[p];
                let src = input_path(shape, path.part, path.ring);
                if !keep[p] {
                    same = false;
                    continue;
                }
                let pts = self.emit(path, kept, src);
                same &= pts.len() == src.len();
                if !path.is_ring {
                    lines.push(pts);
                } else if path.ring == 0 {
                    polys.push(vec![pts]);
                } else {
                    polys.last_mut().unwrap().push(pts);
                }
            }
            out.push(if lines.is_empty() && polys.is_empty() {
                Simple::Gone
            } else if same {
                Simple::Same
            } else if polys.is_empty() {
                Simple::Lines(lines)
            } else {
                Simple::Polygons(polys)
            });
        }
        out
    }
}
