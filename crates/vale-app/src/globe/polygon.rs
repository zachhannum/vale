//! The freehand polygon tool. A pen stroke on the sphere closes into a ring
//! and becomes a polygon feature in longitude and latitude.
//!
//! The short arc between the two ends of the stroke closes the ring. Where
//! the ring crosses itself, the polygon is each area that the ring encloses,
//! which is all of the sphere but the largest area between the parts of the
//! ring. Two such areas that touch at a point are two polygons of the
//! feature.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use std::collections::HashSet;
use std::f64::consts::{PI, TAU};

use vale_store::{Geometry, LonLat};

use super::line::{smooth, to_parts_exact};
use super::math::{V3, angle, cross, dot, length, normalize};
use super::snap::{PathVertex, Run, append, distance_to_arc, slerp};

/// A shorter stroke is a tap, and it makes no polygon.
pub(super) const MIN_LENGTH: f64 = 3.0;
/// A smaller area, in square points, is no polygon.
pub(super) const MIN_AREA: f64 = 1.0;
/// Two points nearer than this, in radians, are at one place.
pub(super) const SAME: f64 = 1e-9;
/// The longest new edge, in radians. A longer arc gets more vertices, so a
/// flat map draws the same shape as the globe.
const MAX_ARC: f64 = PI / 180.0;
/// The places along the edge of the flat map where a ring goes on to the
/// next side: the corners, from the north end of the 180 degree meridian.
const CORNERS: [(f64, LonLat); 4] = [
    (180.0, [180.0, 90.0]),
    (540.0, [-180.0, 90.0]),
    (720.0, [-180.0, -90.0]),
    (0.0, [180.0, -90.0]),
];
/// The length of the edge of the flat map, in degrees.
const PERIMETER: f64 = 1080.0;

/// The polygon of a stroke. The runs come from a `Tracer`. A stroke that is
/// too short or that encloses no area gives `None`.
pub fn commit(runs: &[Run], scale: f64, strength: f64) -> Option<Geometry> {
    let path = path(runs, scale, strength);
    if path_length(&path) < MIN_LENGTH / scale {
        return None;
    }
    let min_area = MIN_AREA / (scale * scale);
    let polygons: Vec<Vec<Vec<LonLat>>> = rings(&densify(&path))
        .into_iter()
        .filter(|r| signed_area(&dirs(r)) > min_area)
        .flat_map(|r| to_polygons(&r))
        .collect();
    (!polygons.is_empty()).then_some(Geometry::Polygons(polygons))
}

/// The path of the runs of a stroke, with each free run smoothed.
pub(super) fn path(runs: &[Run], scale: f64, strength: f64) -> Vec<PathVertex> {
    let mut path = Vec::new();
    for run in runs {
        match run {
            Run::Free(samples) if !samples.is_empty() => {
                let smooth = smooth(samples, &[], scale, strength);
                append(&mut path, smooth.into_iter().map(PathVertex::Free));
            }
            Run::Free(_) => {}
            Run::Traced(traced) => append(&mut path, traced.iter().copied()),
        }
    }
    path
}

pub(super) fn path_length(path: &[PathVertex]) -> f64 {
    path.windows(2).map(|w| angle(w[0].dir(), w[1].dir())).sum()
}

/// The closed path with more vertices on each long arc. The last vertex is
/// not the first again. An arc between two exact vertices is an arc of a
/// target, and it stays as it is.
pub(super) fn densify(path: &[PathVertex]) -> Vec<PathVertex> {
    densify_path(path, true)
}

/// The open path with more vertices on each long arc.
pub(super) fn densify_open(path: &[PathVertex]) -> Vec<PathVertex> {
    densify_path(path, false)
}

fn densify_path(path: &[PathVertex], closed: bool) -> Vec<PathVertex> {
    let mut out = Vec::with_capacity(path.len());
    for (i, &a) in path.iter().enumerate() {
        out.push(a);
        if !closed && i + 1 == path.len() {
            break;
        }
        let b = path[(i + 1) % path.len()];
        if matches!((a, b), (PathVertex::Exact(_), PathVertex::Exact(_))) {
            continue;
        }
        let (da, db) = (a.dir(), b.dir());
        let n = (angle(da, db) / MAX_ARC).ceil() as usize;
        for k in 1..n {
            out.push(PathVertex::Free(slerp(da, db, k as f64 / n as f64)));
        }
    }
    out
}

/// A place on the ring. The vertices of the path at one place, as at a seam
/// or a pole, are one node.
#[derive(Clone)]
struct Node {
    dir: V3,
    group: Vec<PathVertex>,
}

/// The simple rings of the area that a closed path encloses, with the
/// inside on the left.
pub(super) fn rings(path: &[PathVertex]) -> Vec<Vec<PathVertex>> {
    let nodes = unspike(path, true);
    if nodes.len() < 3 {
        return Vec::new();
    }
    let graph = Graph::new(&[(nodes, true)]);
    let faces = graph.faces();
    let Some(outer) = faces.iter().max_by(|a, b| {
        let area = |f: &Vec<usize>| graph.area(f).rem_euclid(2.0 * TAU);
        area(a).total_cmp(&area(b))
    }) else {
        return Vec::new();
    };
    // The way back around the outer face has the rest of the sphere on its
    // left.
    let walk: Vec<(usize, Vec<PathVertex>)> = outer
        .iter()
        .rev()
        .map(|&h| {
            let twin = &graph.half[h ^ 1];
            (twin.to, graph.arrival(twin))
        })
        .collect();
    loops(walk)
}

/// The simple rings of the areas between closed paths that `keep` chooses,
/// with the inside on the left. `keep` gets a point inside an area. Two kept
/// areas that touch at a point have two rings.
pub(super) fn overlay(
    paths: &[Vec<PathVertex>],
    keep: impl Fn(V3) -> bool,
) -> Vec<Vec<PathVertex>> {
    let cycles: Vec<(Vec<Node>, bool)> = paths
        .iter()
        .map(|p| (unspike(p, true), true))
        .filter(|c| c.0.len() >= 3)
        .collect();
    if cycles.is_empty() {
        return Vec::new();
    }
    let graph = Graph::new(&cycles);
    let faces = graph.faces();
    let face_of = graph.face_of(&faces);
    let kept: Vec<bool> = faces.iter().map(|f| keep(graph.inside(f))).collect();
    let boundary = |h: usize| kept[face_of[h]] && !kept[face_of[h ^ 1]];
    graph
        .walk(boundary)
        .into_iter()
        .flat_map(|(_, rings)| rings)
        .collect()
}

/// The pieces of the area between closed paths that `inside` holds, where
/// an open path cuts it from side to side. Each piece is its simple rings,
/// with the inside on the left. Two pieces share the vertices of the cut.
/// A piece with an area below `min_area` joins the piece next to it. A part
/// of the area that the path does not cut goes to the piece that holds it,
/// or else to the largest piece. A path that does not cut gives `None`.
pub(super) fn cut(
    paths: &[Vec<PathVertex>],
    stroke: &[PathVertex],
    inside: impl Fn(V3) -> bool,
    min_area: f64,
) -> Option<Vec<Vec<Vec<PathVertex>>>> {
    let mut all: Vec<(Vec<Node>, bool)> = paths
        .iter()
        .map(|p| (unspike(p, true), true))
        .filter(|c| c.0.len() >= 3)
        .collect();
    let stroke = unspike(stroke, false);
    if all.is_empty() || stroke.len() < 2 {
        return None;
    }
    all.push((stroke, false));
    let graph = Graph::new(&all);
    let faces = graph.faces();
    let face_of = graph.face_of(&faces);
    let kept: Vec<bool> = faces.iter().map(|f| inside(graph.inside(f))).collect();
    let size = |rings: &[Vec<PathVertex>]| -> f64 {
        (rings.iter())
            .map(|r| signed_area(&dirs(r)).rem_euclid(2.0 * TAU))
            .sum()
    };
    let across = |h: usize| graph.half[h].stroke && kept[face_of[h]] && kept[face_of[h ^ 1]];
    // The area of each kept face, from its simple rings.
    let mut area = vec![0.0; faces.len()];
    let each = |h: usize| kept[face_of[h]] && face_of[h] != face_of[h ^ 1];
    for (h, rings) in graph.walk(each) {
        area[face_of[h]] += size(&rings);
    }
    let mut piece: Vec<usize> = (0..faces.len()).collect();
    for h in (0..graph.half.len()).filter(|&h| across(h)) {
        let (a, b) = (face_of[h], face_of[h ^ 1]);
        if area[a] < min_area || area[b] < min_area {
            union(&mut piece, a, b);
        }
    }
    let piece: Vec<usize> = (0..faces.len()).map(|f| find(&mut piece, f)).collect();
    let mut cut = vec![false; faces.len()];
    for h in (0..graph.half.len()).filter(|&h| across(h)) {
        let (a, b) = (piece[face_of[h]], piece[face_of[h ^ 1]]);
        if a != b {
            cut[a] = true;
        }
    }
    if !cut.contains(&true) {
        return None;
    }
    // The faces that the stroke touches. Each one is a disk, so a piece
    // there has no other outline.
    let mut parent: Vec<usize> = (0..graph.points.len()).collect();
    for e in &graph.half {
        union(&mut parent, e.from, e.to);
    }
    let stroke_part = (graph.half.iter())
        .find(|e| e.stroke)
        .map(|e| find(&mut parent, e.from))?;
    let boundary = |h: usize| {
        let (a, b) = (face_of[h], face_of[h ^ 1]);
        kept[a] && (!kept[b] || piece[a] != piece[b])
    };
    let mut pieces: Vec<Vec<Vec<PathVertex>>> = Vec::new();
    let mut index = vec![usize::MAX; faces.len()];
    let mut rest = Vec::new();
    for (h, rings) in graph.walk(boundary) {
        let f = piece[face_of[h]];
        if cut[f] && find(&mut parent, graph.half[h].from) == stroke_part {
            if index[f] == usize::MAX {
                index[f] = pieces.len();
                pieces.push(Vec::new());
            }
            pieces[index[f]].extend(rings);
        } else {
            rest.extend(rings);
        }
    }
    let largest = (0..pieces.len())
        .max_by(|&a, &b| size(&pieces[a]).total_cmp(&size(&pieces[b])))
        .expect("a piece");
    let shells: Vec<Vec<Vec<V3>>> = pieces
        .iter()
        .map(|p| p.iter().map(|r| dirs(r)).collect())
        .collect();
    for ring in rest {
        let p = ring[0].dir();
        let holder = (0..pieces.len())
            .find(|&k| {
                shells[k]
                    .iter()
                    .any(|r| super::vector::contains(std::slice::from_ref(r), p))
            })
            .unwrap_or(largest);
        pieces[holder].push(ring);
    }
    Some(pieces)
}

/// The nodes of a path, with no step out and back on itself.
fn unspike(path: &[PathVertex], closed: bool) -> Vec<Node> {
    let same = |a: &Node, b: &Node| angle(a.dir, b.dir) < SAME;
    let mut out: Vec<Node> = Vec::new();
    for &v in path {
        let node = Node {
            dir: v.dir(),
            group: vec![v],
        };
        let n = out.len();
        if n >= 1 && same(&out[n - 1], &node) {
            out[n - 1].group.push(v);
        } else if n >= 2 && same(&out[n - 2], &node) {
            out.pop();
        } else {
            out.push(node);
        }
    }
    while closed && !out.is_empty() {
        let n = out.len();
        if n >= 2 && same(&out[0], &out[n - 1]) {
            let last = out.pop().expect("a node");
            let first = std::mem::replace(&mut out[0].group, last.group);
            out[0].group.extend(first);
        } else if n >= 3 && same(&out[1], &out[n - 1]) {
            out.pop();
            out.remove(0);
        } else if n >= 3 && same(&out[0], &out[n - 2]) {
            out.truncate(n - 2);
        } else {
            break;
        }
    }
    out
}

/// One direction of an edge of the graph.
struct Half {
    from: usize,
    to: usize,
    /// The node of the path at `to`, if `to` is one, and true if the edge
    /// goes against the path.
    arrive: Option<(usize, bool)>,
    /// The edge is a part of an open path.
    stroke: bool,
}

/// The ring split at each place where it crosses or touches itself. The
/// edges of the graph are parts of the arcs of the ring, and the half edges
/// `2k` and `2k + 1` are the two directions of one edge.
struct Graph {
    nodes: Vec<Node>,
    /// The places of the nodes of the path and then of the crossings.
    points: Vec<V3>,
    half: Vec<Half>,
    /// The half edges out of each place, in counterclockwise order.
    out: Vec<Vec<usize>>,
    /// The place of each half edge in the list of its start.
    rank: Vec<usize>,
}

impl Graph {
    /// The graph of paths of nodes. A closed path is a cycle.
    fn new(paths: &[(Vec<Node>, bool)]) -> Self {
        let nodes: Vec<Node> = paths.iter().flat_map(|p| p.0.iter()).cloned().collect();
        let n = nodes.len();
        // The arcs from each node to the next one in its path, and whether
        // the path is open.
        let mut arcs: Vec<(usize, usize, bool)> = Vec::with_capacity(n);
        let mut start = 0;
        for (c, closed) in paths {
            arcs.extend((1..c.len()).map(|k| (start + k - 1, start + k, !closed)));
            if *closed {
                arcs.push((start + c.len() - 1, start, false));
            }
            start += c.len();
        }
        let m = arcs.len();
        let mut points: Vec<V3> = nodes.iter().map(|v| v.dir).collect();
        let arc = |i: usize| (arcs[i].0, arcs[i].1);
        let mut parent: Vec<usize> = (0..n).collect();
        // The places inside each arc, by their angle from its start.
        let mut splits: Vec<Vec<(f64, usize)>> = vec![Vec::new(); m];
        let boxes: Vec<([f64; 3], [f64; 3])> = (0..m)
            .map(|i| {
                let (a, b) = arc(i);
                bounds(points[a], points[b])
            })
            .collect();
        let mut order: Vec<usize> = (0..m).collect();
        order.sort_by(|&a, &b| boxes[a].0[0].total_cmp(&boxes[b].0[0]));
        for (k, &i) in order.iter().enumerate() {
            for &j in &order[k + 1..] {
                if boxes[j].0[0] > boxes[i].1[0] {
                    break;
                }
                let adjacent = arcs[i].1 == arcs[j].0 || arcs[j].1 == arcs[i].0;
                if adjacent || !overlap(&boxes[i], &boxes[j]) {
                    continue;
                }
                let (ai, bi) = arc(i);
                let (aj, bj) = arc(j);
                let (p, q) = ((points[ai], points[bi]), (points[aj], points[bj]));
                let hits = match crossing(p.0, p.1, q.0, q.1) {
                    Some(x) => vec![x],
                    // On one great circle, each end of one arc on the other
                    // arc is a place where they meet.
                    None => [p.0, p.1, q.0, q.1]
                        .into_iter()
                        .filter(|&x| on_arc(x, p) && on_arc(x, q))
                        .collect(),
                };
                for x in hits {
                    let end = |a: usize, b: usize| {
                        [a, b].into_iter().find(|&e| angle(points[e], x) < SAME)
                    };
                    match (end(ai, bi), end(aj, bj)) {
                        (Some(e), Some(f)) => union(&mut parent, e, f),
                        (Some(e), None) => splits[j].push((angle(q.0, x), e)),
                        (None, Some(f)) => splits[i].push((angle(p.0, x), f)),
                        (None, None) => {
                            let known = splits[i]
                                .iter()
                                .map(|s| s.1)
                                .find(|&s| angle(points[s], x) < SAME);
                            let id = known.unwrap_or_else(|| {
                                points.push(x);
                                parent.push(points.len() - 1);
                                points.len() - 1
                            });
                            splits[i].push((angle(p.0, x), id));
                            splits[j].push((angle(q.0, x), id));
                        }
                    }
                }
            }
        }
        let mut half = Vec::new();
        let mut seen = HashSet::new();
        for (i, split) in splits.iter_mut().enumerate() {
            let (a, b) = arc(i);
            split.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut chain = vec![a];
            chain.extend(split.iter().map(|s| s.1));
            chain.push(b);
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            for w in chain.windows(2) {
                let (u, v) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
                if u == v || !seen.insert((u.min(v), u.max(v))) {
                    continue;
                }
                let stroke = arcs[i].2;
                half.push(Half {
                    from: u,
                    to: v,
                    arrive: (v == rb).then_some((b, false)),
                    stroke,
                });
                half.push(Half {
                    from: v,
                    to: u,
                    arrive: (u == ra).then_some((a, true)),
                    stroke,
                });
            }
        }
        let mut out = vec![Vec::new(); points.len()];
        for (h, e) in half.iter().enumerate() {
            out[e.from].push(h);
        }
        let mut rank = vec![0; half.len()];
        for (v, list) in out.iter_mut().enumerate() {
            let way = |h: &usize| heading(points[v], points[half[*h].to]);
            list.sort_by(|a, b| way(a).total_cmp(&way(b)));
            for (r, &h) in list.iter().enumerate() {
                rank[h] = r;
            }
        }
        Graph {
            nodes,
            points,
            half,
            out,
            rank,
        }
    }

    /// The half edge after `h` around the face on its left: the first one
    /// clockwise from the way back.
    fn next(&self, h: usize) -> usize {
        let twin = h ^ 1;
        let list = &self.out[self.half[twin].from];
        list[(self.rank[twin] + list.len() - 1) % list.len()]
    }

    /// The half edge out of the same place that is next clockwise.
    fn clockwise(&self, h: usize) -> usize {
        let list = &self.out[self.half[h].from];
        list[(self.rank[h] + list.len() - 1) % list.len()]
    }

    /// A point just on the left of the longest edge of a face.
    fn inside(&self, face: &[usize]) -> V3 {
        let ends = |h: usize| (self.points[self.half[h].from], self.points[self.half[h].to]);
        let longest = face
            .iter()
            .map(|&h| ends(h))
            .max_by(|x, y| angle(x.0, x.1).total_cmp(&angle(y.0, y.1)))
            .expect("an edge");
        let (a, b) = longest;
        let m = normalize([0, 1, 2].map(|k| a[k] + b[k]));
        let n = normalize(cross(a, b));
        normalize([0, 1, 2].map(|k| m[k] + 1e-8 * n[k]))
    }

    /// The faces of the graph, each as its half edges in order, with the face
    /// on the left.
    fn faces(&self) -> Vec<Vec<usize>> {
        let mut used = vec![false; self.half.len()];
        let mut faces = Vec::new();
        for start in 0..self.half.len() {
            let mut face = Vec::new();
            let mut h = start;
            while !used[h] {
                used[h] = true;
                face.push(h);
                h = self.next(h);
            }
            if !face.is_empty() {
                faces.push(face);
            }
        }
        faces
    }

    /// The face of each half edge, as an index into `faces`.
    fn face_of(&self, faces: &[Vec<usize>]) -> Vec<usize> {
        let mut out = vec![0; self.half.len()];
        for (k, face) in faces.iter().enumerate() {
            for &h in face {
                out[h] = k;
            }
        }
        out
    }

    /// The simple rings along the half edges that `boundary` chooses, with
    /// the kept side on the left. Each walk gives its first half edge and
    /// its rings.
    fn walk(&self, boundary: impl Fn(usize) -> bool) -> Vec<(usize, Vec<Vec<PathVertex>>)> {
        let mut used = vec![false; self.half.len()];
        let mut out = Vec::new();
        for start in 0..self.half.len() {
            if used[start] || !boundary(start) {
                continue;
            }
            let mut walk = Vec::new();
            let mut h = start;
            while !used[h] {
                used[h] = true;
                let half = &self.half[h];
                walk.push((half.to, self.arrival(half)));
                // The first boundary edge clockwise from the way back. The
                // areas between are kept.
                h = self.next(h);
                while !boundary(h) {
                    h = self.clockwise(h);
                }
            }
            out.push((start, loops(walk)));
        }
        out
    }

    /// The area on the left of a face, up to a multiple of 4π.
    fn area(&self, face: &[usize]) -> f64 {
        let ring: Vec<V3> = face.iter().map(|&h| self.points[self.half[h].to]).collect();
        signed_area(&ring)
    }

    /// The vertices of the path where a half edge ends.
    fn arrival(&self, h: &Half) -> Vec<PathVertex> {
        match h.arrive {
            Some((node, back)) => {
                let mut group = self.nodes[node].group.clone();
                if back {
                    group.reverse();
                }
                group
            }
            None => vec![PathVertex::Free(self.points[h.to])],
        }
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parent, a), find(parent, b));
    parent[a.max(b)] = a.min(b);
}

/// A box around the short arc from `a` to `b`. The arc bends out from its
/// chord by at most the part `1 / cos(θ / 2) - 1`.
pub(super) fn bounds(a: V3, b: V3) -> ([f64; 3], [f64; 3]) {
    let half = angle(a, b) / 2.0;
    let pad = if half < 1.0 {
        1.0 / half.cos() - 1.0
    } else {
        2.0
    } + SAME;
    let lo = [0, 1, 2].map(|k| a[k].min(b[k]) - pad);
    let hi = [0, 1, 2].map(|k| a[k].max(b[k]) + pad);
    (lo, hi)
}

fn overlap(a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|k| a.0[k] <= b.1[k] && b.0[k] <= a.1[k])
}

/// The place where two short arcs cross. Two arcs on one great circle give
/// `None`.
pub(super) fn crossing(a: V3, b: V3, c: V3, d: V3) -> Option<V3> {
    let (n1, n2) = (cross(a, b), cross(c, d));
    let x = cross(n1, n2);
    if length(x) <= 1e-12 * length(n1) * length(n2) {
        return None;
    }
    let x = normalize(x);
    [x, [-x[0], -x[1], -x[2]]]
        .into_iter()
        .find(|&p| on_arc(p, (a, b)) && on_arc(p, (c, d)))
}

fn on_arc(p: V3, (a, b): (V3, V3)) -> bool {
    distance_to_arc(p, a, b) < SAME
}

/// The direction from `v` to `to`, as an angle in the plane that touches
/// the sphere at `v`. The angle goes counterclockwise as seen from outside.
fn heading(v: V3, to: V3) -> f64 {
    let axis = if v[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let e1 = normalize(cross(v, axis));
    let e2 = cross(v, e1);
    let t = cross(cross(v, to), v);
    dot(t, e2).atan2(dot(t, e1))
}

/// The area on the left of a closed ring, from -2π to 2π. A ring with a
/// small area on its right gives a negative area.
pub(super) fn signed_area(ring: &[V3]) -> f64 {
    let Some(&r) = ring.first() else {
        return 0.0;
    };
    let mut sum = 0.0;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        let det = dot(r, cross(a, b));
        sum += 2.0 * det.atan2(1.0 + dot(r, a) + dot(a, b) + dot(b, r));
    }
    (sum + TAU).rem_euclid(2.0 * TAU) - TAU
}

pub(super) fn dirs(ring: &[PathVertex]) -> Vec<V3> {
    ring.iter().map(PathVertex::dir).collect()
}

/// The walk split into simple rings where it comes back to a place.
fn loops(walk: Vec<(usize, Vec<PathVertex>)>) -> Vec<Vec<PathVertex>> {
    let mut stack: Vec<(usize, Vec<PathVertex>)> = Vec::new();
    let mut out = Vec::new();
    let flat = |nodes: &[(usize, Vec<PathVertex>)]| -> Vec<PathVertex> {
        nodes.iter().flat_map(|n| n.1.iter().copied()).collect()
    };
    for (node, group) in walk {
        match stack.iter().position(|s| s.0 == node) {
            Some(k) => {
                let ring = flat(&stack[k..]);
                stack.truncate(k + 1);
                out.push(ring);
            }
            None => stack.push((node, group)),
        }
    }
    out.push(flat(&stack));
    out.retain(|r| r.len() >= 3);
    out
}

/// The polygons in longitude and latitude of a simple ring with its inside
/// on the left. Each ring of the output is closed and goes counterclockwise
/// on a flat map. A ring that crosses the 180 degree meridian gives one
/// polygon on each side for each part. A ring around a pole goes along the
/// pole.
pub(super) fn to_polygons(ring: &[PathVertex]) -> Vec<Vec<Vec<LonLat>>> {
    to_polygons_many(&[ring.to_vec()])
}

/// The polygons in longitude and latitude of the area on the left of simple
/// rings that do not cross each other. Each ring of the output goes
/// counterclockwise on a flat map. A hole is in the polygon that holds it.
pub(super) fn to_polygons_many(rings: &[Vec<PathVertex>]) -> Vec<Vec<Vec<LonLat>>> {
    let mut outers: Vec<Vec<LonLat>> = Vec::new();
    let mut holes: Vec<Vec<LonLat>> = Vec::new();
    let mut parts: Vec<Vec<LonLat>> = Vec::new();
    for ring in rings {
        let Some(&first) = ring.first() else {
            continue;
        };
        let mut closed = ring.clone();
        closed.push(first);
        let mut ring_parts = to_parts_exact(&closed);
        if ring_parts.len() > 1 && ring_parts[ring_parts.len() - 1].last() == ring_parts[0].first()
        {
            let first = ring_parts.remove(0);
            ring_parts
                .last_mut()
                .expect("a part")
                .extend_from_slice(&first[1..]);
        }
        if ring_parts.len() == 1 && ring_parts[0].first() == ring_parts[0].last() {
            let part = ring_parts.remove(0);
            if part.len() >= 4 {
                if flat_area(&part) >= 0.0 {
                    outers.push(part);
                } else {
                    // A stored hole goes counterclockwise too.
                    holes.push(part.into_iter().rev().collect());
                }
            }
        } else {
            parts.extend(ring_parts);
        }
    }
    outers.extend(join_at_edge(&parts));
    let mut out: Vec<Vec<Vec<LonLat>>> = outers.into_iter().map(|r| vec![r]).collect();
    for hole in holes {
        let p = [
            (hole[0][0] + hole[1][0]) / 2.0,
            (hole[0][1] + hole[1][1]) / 2.0,
        ];
        match out.iter_mut().find(|poly| flat_contains(&poly[0], p)) {
            Some(poly) => poly.push(hole),
            // The area outside all outer rings is in the polygon.
            None => out.push(vec![
                vec![
                    [-180.0, -90.0],
                    [180.0, -90.0],
                    [180.0, 90.0],
                    [-180.0, 90.0],
                    [-180.0, -90.0],
                ],
                hole,
            ]),
        }
    }
    out
}

/// The outer rings of parts that each go from the 180 degree meridian to it.
/// The edge of the flat map, with the inside on the left, joins the parts.
fn join_at_edge(parts: &[Vec<LonLat>]) -> Vec<Vec<LonLat>> {
    let at = |p: LonLat| {
        if p[0] > 0.0 {
            p[1] + 90.0
        } else {
            630.0 - p[1]
        }
    };
    let ahead = |from: f64, to: f64| (to - from).rem_euclid(PERIMETER);
    let starts: Vec<f64> = parts.iter().map(|p| at(p[0])).collect();
    let mut used = vec![false; parts.len()];
    let mut out = Vec::new();
    for first in 0..parts.len() {
        if used[first] {
            continue;
        }
        let mut ring: Vec<LonLat> = Vec::new();
        let mut i = first;
        loop {
            used[i] = true;
            ring.extend_from_slice(&parts[i]);
            let end = at(ring[ring.len() - 1]);
            let next = (0..parts.len())
                .min_by(|&a, &b| ahead(end, starts[a]).total_cmp(&ahead(end, starts[b])))
                .expect("a part");
            let gap = ahead(end, starts[next]);
            let mut corners: Vec<(f64, LonLat)> = CORNERS
                .iter()
                .map(|&(s, c)| (ahead(end, s), c))
                .filter(|&(d, _)| d > 0.0 && d < gap)
                .collect();
            corners.sort_by(|a, b| a.0.total_cmp(&b.0));
            ring.extend(corners.into_iter().map(|c| c.1));
            if used[next] {
                break;
            }
            i = next;
        }
        ring.push(ring[0]);
        if ring.len() >= 4 {
            out.push(ring);
        }
    }
    out
}

/// The area of a closed ring on a flat map, positive when it goes
/// counterclockwise.
pub(super) fn flat_area(ring: &[LonLat]) -> f64 {
    ring.windows(2)
        .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
        .sum::<f64>()
        / 2.0
}

/// A point is inside a closed ring on a flat map.
fn flat_contains(ring: &[LonLat], p: LonLat) -> bool {
    let mut inside = false;
    for w in ring.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use vale_store::{GeometryKind, Layer};

    use super::*;
    use crate::globe::math::lonlat_to_dir;
    use crate::globe::snap::SnapSettings;
    use crate::globe::snap::tests as snap_test;
    use crate::globe::vector;

    /// Points per radian: a sphere with a radius of 300 points on screen.
    const SCALE: f64 = 300.0;

    fn dir(p: LonLat) -> V3 {
        lonlat_to_dir(p[0], p[1])
    }

    /// Samples on a circle on the sphere, from the angle `from` to `to` in
    /// degrees, counterclockwise as seen from outside.
    fn circle(center: LonLat, radius: f64, from: f64, to: f64) -> Vec<V3> {
        let c = dir(center);
        let axis = if c[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let e1 = normalize(cross(axis, c));
        let e2 = cross(c, e1);
        let n = ((to - from).abs() * 4.0).ceil() as usize;
        let r = radius.to_radians();
        (0..=n)
            .map(|k| {
                let t = (from + (to - from) * k as f64 / n as f64).to_radians();
                let (s, cs) = (r.sin(), r.cos());
                normalize([0, 1, 2].map(|i| c[i] * cs + s * (e1[i] * t.cos() + e2[i] * t.sin())))
            })
            .collect()
    }

    fn polygons(g: &Geometry) -> &Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not a polygon");
        };
        p
    }

    fn draw(samples: Vec<V3>) -> Vec<Vec<Vec<LonLat>>> {
        let g = commit(&[Run::Free(samples)], SCALE, 0.35).expect("a polygon");
        polygons(&g).clone()
    }

    /// Two segments on a flat map cross, or one touches the inside of the
    /// other.
    fn segments_meet(a: LonLat, b: LonLat, c: LonLat, d: LonLat) -> bool {
        let side = |p: LonLat, q: LonLat, r: LonLat| {
            (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
        };
        let (d1, d2) = (side(c, d, a), side(c, d, b));
        let (d3, d4) = (side(a, b, c), side(a, b, d));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    }

    /// Each ring is closed and counterclockwise on a flat map, and no two
    /// edges cross. Each step of a ring is short in longitude, or it is
    /// along a pole or along the 180 degree meridian.
    pub(crate) fn assert_valid(polygons: &[Vec<Vec<LonLat>>]) {
        let edges: Vec<(LonLat, LonLat)> = polygons
            .iter()
            .flatten()
            .flat_map(|r| r.windows(2).map(|w| (w[0], w[1])))
            .collect();
        for ring in polygons.iter().flatten() {
            assert!(ring.len() >= 4);
            assert_eq!(ring[0], ring[ring.len() - 1]);
            assert!(flat_area(ring) > 0.0, "{ring:?}");
            for w in ring.windows(2) {
                let pole = w[0][1].abs() == 90.0 && w[0][1] == w[1][1];
                assert!((w[1][0] - w[0][0]).abs() <= 45.0 || pole, "{w:?}");
            }
        }
        for (i, e) in edges.iter().enumerate() {
            for f in &edges[i + 1..] {
                assert!(!segments_meet(e.0, e.1, f.0, f.1), "{e:?} crosses {f:?}");
            }
        }
    }

    /// The rings of a polygon as the globe draws them.
    fn globe_rings(polygon: &[Vec<LonLat>]) -> Vec<Vec<V3>> {
        polygon
            .iter()
            .enumerate()
            .map(|(i, r)| vector::ring(r, i > 0))
            .collect()
    }

    pub(crate) fn inside(polygons: &[Vec<Vec<LonLat>>], p: LonLat) -> bool {
        polygons
            .iter()
            .any(|poly| vector::contains(&globe_rings(poly), dir(p)))
    }

    #[test]
    fn a_stroke_becomes_one_polygon() {
        let p = draw(circle([10.0, 20.0], 5.0, 0.0, 360.0));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 1);
        assert_valid(&p);
        assert!(inside(&p, [10.0, 20.0]));
        assert!(!inside(&p, [10.0, 26.0]));
        // A stroke the other way gives the same polygon.
        let p = draw(circle([10.0, 20.0], 5.0, 360.0, 0.0));
        assert_valid(&p);
        assert!(inside(&p, [10.0, 20.0]));
    }

    #[test]
    fn a_stroke_closes_with_the_short_arc_between_its_ends() {
        // Three quarters of a circle.
        let samples = circle([0.0, 40.0], 10.0, 0.0, 270.0);
        let (first, last) = (samples[0], samples[samples.len() - 1]);
        let p = draw(samples);
        assert_eq!(p.len(), 1);
        assert_valid(&p);
        let ring: Vec<V3> = p[0][0].iter().map(|&q| dir(q)).collect();
        let at = |x: V3| {
            ring.iter()
                .position(|&q| angle(q, x) < 1e-9)
                .expect("an end")
        };
        let (i, j) = (at(first), at(last));
        // The ring goes from the last sample to the first one on the arc.
        let n = ring.len() - 1;
        let mut k = j;
        while k != i {
            assert!(distance_to_arc(ring[k], last, first) < 1e-9);
            k = (k + 1) % n;
        }
        assert!((j + 1) % n != i, "the arc has vertices along it");
        // The middle of the arc is inside, near the edge.
        let mid = normalize([0, 1, 2].map(|k| first[k] + last[k]));
        let toward = normalize([0, 1, 2].map(|k| 0.99 * mid[k] + 0.01 * dir([0.0, 40.0])[k]));
        let (lon, lat) = crate::globe::math::dir_to_lonlat(toward);
        assert!(inside(&p, [lon, lat]));
    }

    #[test]
    fn a_stroke_that_crosses_itself_gives_a_valid_polygon() {
        // A figure eight: two lobes that the stroke goes around in opposite
        // directions.
        let samples = (0..=720)
            .map(|k| {
                let t = (90.0 + 0.5 * f64::from(k)).to_radians();
                dir([10.0 * t.sin(), 10.0 * t.sin() * t.cos()])
            })
            .collect();
        let p = draw(samples);
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        assert!(inside(&p, [-5.0, 0.0]) && inside(&p, [5.0, 0.0]));
        assert!(!inside(&p, [0.0, 4.0]));

        // A loop with a small curl in it: the curl is inside, and the
        // polygon has no hole.
        let mut samples = circle([0.0, 0.0], 10.0, 0.0, 90.0);
        samples.extend(circle([1.0, 11.0], 2f64.sqrt(), 225.0, 585.0));
        samples.extend(circle([0.0, 0.0], 10.0, 90.0, 360.0));
        let p = draw(samples);
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 1);
        assert!(inside(&p, [0.0, 0.0]) && inside(&p, [0.0, 11.0]));

        // A stroke that goes around twice.
        let mut samples = circle([0.0, 0.0], 10.0, 0.0, 360.0);
        samples.extend(circle([0.0, 0.0], 8.0, 0.0, 330.0));
        let p = draw(samples);
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert!(inside(&p, [0.0, 0.0]) && inside(&p, [0.0, 9.0]));
    }

    #[test]
    fn a_scribble_gives_a_valid_polygon() {
        // Strokes that turn at random and cross themselves many times, also
        // across 180 degrees and near a pole.
        let (mut seed, mut made, mut split) = (7u64, 0, 0);
        let mut random = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for start in [[0.0f64, 0.0f64], [178.0, 30.0], [40.0, 86.0]] {
            for _ in 0..20 {
                let (mut lon, mut lat, mut heading) = (start[0], start[1], 0.0f64);
                let mut samples = Vec::new();
                for _ in 0..400 {
                    heading += (random() - 0.5) * 1.2;
                    let step = 0.1 / lat.to_radians().cos().max(0.2);
                    lon += step * heading.cos();
                    lat = (lat + 0.1 * heading.sin()).clamp(-89.0, 89.0);
                    samples.push(dir([lon, lat]));
                }
                if let Some(g) = commit(&[Run::Free(samples)], SCALE, random()) {
                    assert_valid(polygons(&g));
                    made += 1;
                    split += usize::from(polygons(&g).len() > 1);
                }
            }
        }
        assert!(made > 50 && split > 10, "{made} polygons, {split} in parts");
    }

    #[test]
    fn a_polygon_across_180_degrees_has_a_part_on_each_side() {
        let p = draw(circle([180.0, 10.0], 5.0, 0.0, 360.0));
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        for poly in &p {
            let ring = &poly[0];
            let east = ring.iter().all(|q| q[0] >= 170.0);
            let west = ring.iter().all(|q| q[0] <= -170.0);
            assert!(east || west, "{ring:?}");
            assert!(ring.iter().any(|q| q[0].abs() == 180.0));
        }
        assert!(inside(&p, [179.0, 10.0]) && inside(&p, [-179.0, 10.0]));
        assert!(!inside(&p, [0.0, 10.0]) && !inside(&p, [-179.0, 16.0]));
    }

    #[test]
    fn a_polygon_around_a_pole_goes_along_the_pole() {
        for (lat, pole) in [(75.0f64, 90.0), (-75.0, -90.0)] {
            for way in [(0.0, 360.0), (360.0, 0.0)] {
                let p = draw(circle([30.0, pole], 90.0 - lat.abs(), way.0, way.1));
                assert_valid(&p);
                assert_eq!(p.len(), 1);
                let ring = &p[0][0];
                assert!(ring.contains(&[180.0, pole]) && ring.contains(&[-180.0, pole]));
                assert!(inside(&p, [0.0, (lat + pole) / 2.0]));
                assert!(inside(&p, [-120.0, (lat + pole) / 2.0]));
                assert!(!inside(&p, [0.0, lat - 5f64.copysign(lat)]));
            }
        }
    }

    #[test]
    fn a_tap_makes_no_polygon() {
        let p = dir([0.0, 0.0]);
        assert_eq!(commit(&[Run::Free(vec![p, p])], SCALE, 0.35), None);
        assert_eq!(commit(&[], SCALE, 0.35), None);
        // Out and back on one line encloses nothing.
        let (a, b) = (dir([0.0, 0.0]), dir([10.0, 0.0]));
        assert_eq!(commit(&[Run::Free(vec![a, b, a])], SCALE, 0.0), None);
    }

    fn polygon_layer(id: u32, rings: Vec<Vec<LonLat>>) -> Layer {
        let geometries = rings
            .into_iter()
            .map(|r| Geometry::Polygons(vec![vec![r]]))
            .collect();
        snap_test::layer(id, GeometryKind::Polygon, geometries)
    }

    /// The polygon of a pen stroke along `way` in a layer next to `target`.
    fn trace(target: Layer, way: &[LonLat]) -> (Vec<Run>, Vec<Vec<Vec<LonLat>>>) {
        let t = Arc::new(snap_test::targets(&[target]));
        let samples = snap_test::pen(way, 0.3);
        let runs = snap_test::trace(&t, &samples, SnapSettings::default(), 9);
        let g = commit(&runs, SCALE, 0.35).expect("a polygon");
        (runs, polygons(&g).clone())
    }

    /// The positions of `want` follow each other in a ring, in either order.
    pub(crate) fn has_run(polygons: &[Vec<Vec<LonLat>>], want: &[LonLat]) -> bool {
        let back: Vec<LonLat> = want.iter().rev().copied().collect();
        polygons.iter().flatten().any(|r| {
            r.windows(want.len())
                .any(|w| w == want || w == back.as_slice())
        })
    }

    #[test]
    fn a_part_of_the_outline_traces_a_border() {
        // A country with a south border of odd vertices.
        let border = [
            [0.1234567, 0.0],
            [3.3333333, 0.4],
            [6.789, -0.3],
            [10.0101, 0.2],
        ];
        let mut ring = border.to_vec();
        ring.extend([[10.0101, 10.0], [0.1234567, 10.0], [0.1234567, 0.0]]);
        let target = polygon_layer(1, vec![ring]);
        // A forest south of it, with its north side on the border.
        let way = [
            [0.5, -6.0],
            [0.6, -0.1],
            [9.6, -0.1],
            [9.6, -6.0],
            [0.5, -6.0],
        ];
        let (runs, p) = trace(target, &way);
        assert!(snap_test::kinds(&runs).contains('T'));
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert!(has_run(&p, &border[1..3]), "{p:?}");
        assert!(inside(&p, [5.0, -3.0]) && !inside(&p, [5.0, 3.0]));
    }

    #[test]
    fn a_trace_across_180_keeps_both_seam_vertices() {
        let east = vec![
            [170.0, 0.0],
            [180.0, 0.0],
            [180.0, 10.0],
            [170.0, 10.0],
            [170.0, 0.0],
        ];
        let west = vec![
            [-180.0, 0.0],
            [-170.0, 0.0],
            [-170.0, 10.0],
            [-180.0, 10.0],
            [-180.0, 0.0],
        ];
        let target = snap_test::layer(
            1,
            GeometryKind::Polygon,
            vec![Geometry::Polygons(vec![vec![east], vec![west]])],
        );
        // An area north of the target that traces its north edge.
        let way = [
            [173.0, 15.0],
            [173.0, 10.0],
            [187.0, 10.0],
            [187.0, 15.0],
            [173.0, 15.0],
        ];
        let (runs, p) = trace(target, &way);
        assert!(snap_test::exact(&runs).contains(&[180.0, 10.0]));
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        let ring = |side: f64| {
            p.iter()
                .map(|poly| &poly[0])
                .find(|r| r.iter().all(|q| q[0] * side >= 0.0))
                .expect("a side")
        };
        assert!(ring(1.0).contains(&[180.0, 10.0]), "{p:?}");
        assert!(ring(-1.0).contains(&[-180.0, 10.0]), "{p:?}");
        assert!(inside(&p, [179.0, 12.0]) && inside(&p, [-179.0, 12.0]));
        assert!(!inside(&p, [179.0, 8.0]));
    }
}
