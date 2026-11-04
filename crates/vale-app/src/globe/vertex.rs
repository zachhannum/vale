//! Vertex. The tool shows the vertices of one line or polygon as handles. A
//! drag moves a vertex, a tap on an edge adds one, and the selected
//! vertices can be deleted or smoothed.
//!
//! A vertex is known by its stored position. A trace copies the stored
//! positions of its target, so the features that share a border have their
//! vertices at the same positions, and a move of shared vertices moves each
//! of them.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use eframe::egui::{Pos2, Rect};
use vale_store::{Geometry, Layer, LonLat, Uuid};

use super::Globe;
use super::brush::slerp;
use super::line::{chains, smooth, to_parts_exact};
use super::math::{Mat3, V3, angle, dir_to_lonlat, lonlat_to_dir};
use super::polygon::MIN_AREA;
use super::reshape::{hole, outer, outlines};
use super::select::Pointer;
use super::snap::{PathVertex, append, project};
use crate::measure::TAP_SLOP;

/// The largest angle between two sampled points of an edge on screen, in
/// radians.
const SAMPLE: f64 = 0.0175;

/// The state of the Vertex tool that the project does not save.
pub struct VertexTool {
    pub press: VertexPress,
    /// A move of a vertex moves each vertex at the same position in the
    /// other features of the layer.
    pub shared: bool,
    /// The screen positions of the handles of the edited feature.
    pub handles: Vec<Pos2>,
}

impl Default for VertexTool {
    fn default() -> Self {
        VertexTool {
            press: VertexPress::default(),
            shared: true,
            handles: Vec::new(),
        }
    }
}

/// True if two stored positions are one place: equal, two sides of the 180
/// degree meridian, or one pole.
pub fn same(a: LonLat, b: LonLat) -> bool {
    a == b
        || (a[0].abs() == 180.0 && b[0].abs() == 180.0 && a[1] == b[1])
        || (a[1].abs() == 90.0 && a[1] == b[1])
}

/// The stored position of a point on the unit sphere.
pub fn lonlat(d: V3) -> LonLat {
    let (lon, lat) = dir_to_lonlat(d);
    [lon, lat]
}

fn dir(p: LonLat) -> V3 {
    lonlat_to_dir(p[0], p[1])
}

/// A line or a ring of a polygon on the sphere. A ring has the polygon on
/// its left, and its last vertex is not a copy of the first.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub verts: Vec<LonLat>,
    pub closed: bool,
}

impl Path {
    fn edges(&self) -> usize {
        let n = self.verts.len();
        if self.closed && n > 2 {
            n
        } else {
            n.saturating_sub(1)
        }
    }

    fn edge(&self, i: usize) -> (LonLat, LonLat) {
        (self.verts[i], self.verts[(i + 1) % self.verts.len()])
    }

    fn min(&self) -> usize {
        if self.closed { 3 } else { 2 }
    }

    fn has(&self, at: LonLat) -> bool {
        self.verts.iter().any(|&v| same(v, at))
    }
}

fn exact(verts: &[LonLat]) -> Vec<PathVertex> {
    verts.iter().map(|&p| PathVertex::Exact(p)).collect()
}

/// The paths of a geometry: the lines with their parts at the 180 degree
/// meridian joined, or the outlines of the polygons.
pub fn paths(geometry: &Geometry) -> Vec<Path> {
    match geometry {
        Geometry::Points(_) => Vec::new(),
        Geometry::Lines(parts) => chains(parts)
            .iter()
            .map(|chain| {
                let mut path = Vec::new();
                for &i in chain {
                    append(&mut path, exact(&parts[i]));
                }
                Path {
                    verts: stored(&path),
                    closed: false,
                }
            })
            .collect(),
        Geometry::Polygons(polygons) => outlines(polygons)
            .unwrap_or_default()
            .iter()
            .map(|o| Path {
                verts: stored(&o.ring),
                closed: true,
            })
            .collect(),
    }
}

/// The vertices with the vertices at one place joined. A ring does not end
/// with a copy of its first vertex.
fn clean(verts: impl IntoIterator<Item = LonLat>, closed: bool) -> Vec<LonLat> {
    let mut path = Vec::new();
    append(&mut path, verts.into_iter().map(PathVertex::Exact));
    let mut out = stored(&path);
    if closed && out.len() > 1 && out[0] == out[out.len() - 1] {
        out.pop();
    }
    out
}

/// The geometry after `op` changes its paths. `op` gives the new vertices
/// of a path that it changes. With `once`, `op` changes one path at most. A
/// path with too few vertices or a polygon with no area gives `None`.
fn edit(
    geometry: &Geometry,
    scale: f64,
    once: bool,
    mut op: impl FnMut(&Path) -> Option<Vec<LonLat>>,
) -> Option<Geometry> {
    match geometry {
        Geometry::Points(_) => None,
        Geometry::Lines(parts) => {
            let chains = chains(parts);
            let paths = paths(geometry);
            let mut new: Vec<Option<Vec<LonLat>>> = Vec::new();
            for path in &paths {
                let done = once && new.iter().any(Option::is_some);
                let changed = if done { None } else { op(path) };
                if changed.as_ref().is_some_and(|c| c.len() < 2) {
                    return None;
                }
                new.push(changed);
            }
            if new.iter().all(Option::is_none) {
                return None;
            }
            let mut out = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                let k = chains.iter().position(|c| c.contains(&i))?;
                match &new[k] {
                    None => out.push(part.clone()),
                    Some(verts) if chains[k].iter().min() == Some(&i) => {
                        out.extend(to_parts_exact(&exact(verts)));
                    }
                    Some(_) => {}
                }
            }
            Some(Geometry::Lines(out))
        }
        Geometry::Polygons(polygons) => {
            let min_area = MIN_AREA / (scale * scale);
            let mut polygons = polygons.clone();
            let mut changed = false;
            // Each step can change the outlines, so they are found again.
            // The operations do not change a path that they changed before.
            for _ in 0..64 {
                let outlines = outlines(&polygons)?;
                let Some((o, verts)) = outlines.iter().find_map(|o| {
                    let path = Path {
                        verts: stored(&o.ring),
                        closed: true,
                    };
                    op(&path).map(|v| (o, v))
                }) else {
                    break;
                };
                if verts.len() < 3 {
                    return None;
                }
                let ring = exact(&verts);
                polygons = if o.hole {
                    let ring: Vec<PathVertex> = ring.into_iter().rev().collect();
                    hole(&polygons, o, &ring, min_area)?
                } else {
                    outer(&polygons, o, &ring, min_area)?
                };
                changed = true;
                if once {
                    break;
                }
            }
            changed.then_some(Geometry::Polygons(polygons))
        }
    }
}

fn stored(ring: &[PathVertex]) -> Vec<LonLat> {
    ring.iter()
        .map(|v| match *v {
            PathVertex::Exact(p) => p,
            PathVertex::Free(d) => lonlat(d),
        })
        .collect()
}

/// True if the geometry has a vertex at `at`.
pub fn has_vertex(geometry: &Geometry, at: LonLat) -> bool {
    paths(geometry).iter().any(|p| p.has(at))
}

/// The features of the layer, other than `except`, with a vertex at `at`.
pub fn sharing(layer: &Layer, at: LonLat, except: Uuid) -> Vec<Uuid> {
    let has = |g: &Geometry| match g {
        Geometry::Points(_) => false,
        Geometry::Lines(parts) => parts.iter().flatten().any(|&v| same(v, at)),
        Geometry::Polygons(p) => p.iter().flatten().flatten().any(|&v| same(v, at)),
    };
    (layer.features.iter())
        .filter(|f| f.uuid() != except && has(&f.geometry))
        .map(|f| f.uuid())
        .collect()
}

/// The geometry with each vertex at `from` moved to `to`.
pub fn move_vertex(geometry: &Geometry, from: LonLat, to: LonLat, scale: f64) -> Option<Geometry> {
    move_vertices(geometry, &[(from, to)], scale)
}

/// The geometry with each vertex at the first position of a move moved to
/// the second position.
pub fn move_vertices(
    geometry: &Geometry,
    moves: &[(LonLat, LonLat)],
    scale: f64,
) -> Option<Geometry> {
    let to = |v: LonLat| moves.iter().find(|m| same(v, m.0)).map(|m| m.1);
    edit(geometry, scale, false, |path| {
        path.verts.iter().any(|&v| to(v).is_some()).then(|| {
            let moved = path.verts.iter().map(|&v| to(v).unwrap_or(v));
            clean(moved, path.closed)
        })
    })
}

/// The moves of a drag of the vertex at `from` to `to`, with the vertices
/// `group`. The vertex goes to `to`, and each vertex of the group turns on
/// the sphere by the same rotation.
pub fn group_moves(
    from: LonLat,
    to: (V3, Option<LonLat>),
    group: &[LonLat],
) -> Vec<(LonLat, LonLat)> {
    let turn = Mat3::between(dir(from), to.0);
    let mut out = vec![(from, to.1.unwrap_or_else(|| lonlat(to.0)))];
    for &v in group.iter().filter(|&&v| !same(v, from)) {
        out.push((v, lonlat(turn.mul_vec(dir(v)))));
    }
    out
}

/// The geometry with a vertex at `at` on each edge from `a` to `b`, in
/// either direction.
pub fn insert_vertex(
    geometry: &Geometry,
    a: LonLat,
    b: LonLat,
    at: LonLat,
    scale: f64,
) -> Option<Geometry> {
    edit(geometry, scale, false, |path| {
        let mut out = Vec::with_capacity(path.verts.len() + 1);
        let mut changed = false;
        for (i, &v) in path.verts.iter().enumerate() {
            out.push(v);
            if i < path.edges() {
                let (p, q) = path.edge(i);
                if (same(p, a) && same(q, b)) || (same(p, b) && same(q, a)) {
                    out.push(at);
                    changed = true;
                }
            }
        }
        changed.then(|| clean(out, path.closed))
    })
}

/// The geometry with the vertices at the positions `selected` deleted. A
/// path keeps its vertices if it would have too few.
pub fn delete_vertices(geometry: &Geometry, selected: &[LonLat], scale: f64) -> Option<Geometry> {
    edit(geometry, scale, false, |path| {
        let kept = path
            .verts
            .iter()
            .copied()
            .filter(|&v| !selected.iter().any(|&s| same(v, s)));
        let kept = clean(kept, path.closed);
        (kept.len() < path.verts.len() && kept.len() >= path.min()).then_some(kept)
    })
}

/// The geometry with the part between the selected vertices smoothed, with
/// `strength` as the smoothing of the line tool. On a ring, the part is the
/// ring without the longest run of vertices that are not selected. One path
/// changes, and the vertices outside the part keep their positions.
pub fn smooth_vertices(
    geometry: &Geometry,
    selected: &[LonLat],
    scale: f64,
    strength: f64,
) -> Option<Geometry> {
    edit(geometry, scale, true, |path| {
        smooth_path(path, selected, scale, strength)
    })
}

fn smooth_path(path: &Path, selected: &[LonLat], scale: f64, strength: f64) -> Option<Vec<LonLat>> {
    let n = path.verts.len();
    let idx: Vec<usize> = (0..n)
        .filter(|&i| selected.iter().any(|&s| same(path.verts[i], s)))
        .collect();
    if idx.len() < 2 {
        return None;
    }
    // The vertices of the part, in order, and the vertices before and after.
    let (before, span, after): (Vec<LonLat>, Vec<LonLat>, Vec<LonLat>) = if !path.closed {
        let (a, b) = (idx[0], idx[idx.len() - 1]);
        (
            path.verts[..a].to_vec(),
            path.verts[a..=b].to_vec(),
            path.verts[b + 1..].to_vec(),
        )
    } else if idx.len() == n {
        let mut span = path.verts.clone();
        span.push(path.verts[0]);
        (Vec::new(), span, Vec::new())
    } else {
        // The longest gap is from idx[g] to the next selected vertex.
        let gap = |j: usize| (idx[(j + 1) % idx.len()] + n - idx[j]) % n;
        let g = (0..idx.len()).max_by_key(|&j| gap(j))?;
        let start = idx[(g + 1) % idx.len()];
        let len = (idx[g] + n - start) % n + 1;
        let ring: Vec<LonLat> = (0..n).map(|i| path.verts[(start + i) % n]).collect();
        (Vec::new(), ring[..len].to_vec(), ring[len..].to_vec())
    };
    if span.len() < 3 {
        return None;
    }
    let dirs: Vec<V3> = span.iter().map(|&p| dir(p)).collect();
    let smoothed = smooth(&dirs, &[], scale, strength);
    let mut out = before;
    out.push(span[0]);
    if smoothed.len() > 2 {
        out.extend(smoothed[1..smoothed.len() - 1].iter().map(|&d| lonlat(d)));
    }
    if !(path.closed && idx.len() == n) {
        out.push(span[span.len() - 1]);
    }
    out.extend(after);
    let out = clean(out, path.closed);
    (out != path.verts).then_some(out)
}

/// The shown vertices of a geometry, one for each place, with their screen
/// positions.
pub fn handles(globe: &Globe, geometry: &Geometry) -> Vec<(LonLat, Pos2)> {
    let mut out: Vec<(LonLat, Pos2)> = Vec::new();
    for path in paths(geometry) {
        for v in path.verts {
            if out.iter().any(|h| same(h.0, v)) {
                continue;
            }
            if let Some(pos) = globe.project(dir(v)) {
                out.push((v, pos));
            }
        }
    }
    out
}

/// The handle nearest to `at`, within `reach` points.
pub fn hit_handle(handles: &[(LonLat, Pos2)], at: Pos2, reach: f32) -> Option<LonLat> {
    (handles.iter())
        .map(|h| (h.0, h.1.distance(at)))
        .filter(|h| h.1 <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|h| h.0)
}

/// The handles in a screen box.
pub fn in_box(handles: &[(LonLat, Pos2)], rect: Rect) -> Vec<LonLat> {
    (handles.iter())
        .filter(|h| rect.contains(h.1))
        .map(|h| h.0)
        .collect()
}

/// The edge of a geometry nearest to `at` on screen, within `reach` points:
/// its two ends and the point of the edge under `at`.
pub fn hit_edge(
    globe: &Globe,
    geometry: &Geometry,
    at: Pos2,
    reach: f32,
) -> Option<(LonLat, LonLat, LonLat)> {
    let p = globe.unproject(at)?;
    let jump = globe.rect.width() / 2.0;
    let mut best: Option<(f32, LonLat, LonLat)> = None;
    for path in paths(geometry) {
        for i in 0..path.edges() {
            let (a, b) = path.edge(i);
            let (da, db) = (dir(a), dir(b));
            let steps = (angle(da, db) / SAMPLE).ceil().max(1.0) as usize;
            let mut last: Option<Pos2> = None;
            for s in 0..=steps {
                let pos = globe.project(slerp(da, db, s as f64 / steps as f64));
                if let (Some(p0), Some(p1)) = (last, pos)
                    && p0.distance(p1) < jump
                {
                    let d = segment_distance(at, p0, p1);
                    if d <= reach && best.is_none_or(|b| d < b.0) {
                        best = Some((d, a, b));
                    }
                }
                last = pos;
            }
        }
    }
    let (_, a, b) = best?;
    let (q, _, _) = project(p, dir(a), dir(b));
    Some((a, b, lonlat(q)))
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// What a press of the Vertex tool did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VertexEvent {
    Tap {
        pos: Pos2,
        pointer: Pointer,
    },
    /// The press moved past the slop. `pos` is where it went down.
    DragStart {
        pos: Pos2,
        pointer: Pointer,
    },
    Drag {
        pos: Pos2,
    },
    DragEnd {
        pos: Pos2,
    },
    /// The drag ends with no effect.
    Cancel,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    id: u64,
    start: Pos2,
    pointer: Pointer,
    on_handle: bool,
    dragging: bool,
    broken: bool,
}

/// One press of the Vertex tool. A tap of any pointer selects. A pen or the
/// mouse drags from anywhere, and a finger drags from a handle only. Other
/// drags of a finger are left to the view. A second finger ends the press.
/// A finger while a pen is down is a palm.
#[derive(Debug, Default)]
pub struct VertexPress {
    press: Option<Press>,
    pub events: Vec<VertexEvent>,
}

impl VertexPress {
    /// A pen, a finger, or the mouse goes down. `on_handle` is true if a
    /// handle is within reach.
    pub fn down(&mut self, id: u64, pos: Pos2, pointer: Pointer, on_handle: bool) {
        match &mut self.press {
            Some(p) if p.pointer == Pointer::Pen && pointer == Pointer::Finger => {}
            Some(p) => {
                if p.dragging && !p.broken {
                    self.events.push(VertexEvent::Cancel);
                }
                p.broken = true;
            }
            None => {
                self.press = Some(Press {
                    id,
                    start: pos,
                    pointer,
                    on_handle,
                    dragging: false,
                    broken: false,
                });
            }
        }
    }

    pub fn moved(&mut self, id: u64, pos: Pos2) {
        let Some(p) = self.press.as_mut().filter(|p| p.id == id && !p.broken) else {
            return;
        };
        if !p.dragging && p.start.distance(pos) > TAP_SLOP {
            if p.pointer == Pointer::Finger && !p.on_handle {
                p.broken = true;
                return;
            }
            p.dragging = true;
            let (pos, pointer) = (p.start, p.pointer);
            self.events.push(VertexEvent::DragStart { pos, pointer });
        }
        if p.dragging {
            self.events.push(VertexEvent::Drag { pos });
        }
    }

    pub fn up(&mut self, id: u64, pos: Pos2) {
        self.moved(id, pos);
        let Some(p) = self.press.take_if(|p| p.id == id) else {
            return;
        };
        match (p.broken, p.dragging) {
            (true, _) => {}
            (false, true) => self.events.push(VertexEvent::DragEnd { pos }),
            (false, false) => self.events.push(VertexEvent::Tap {
                pos,
                pointer: p.pointer,
            }),
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(p) = self.press.take_if(|p| p.id == id)
            && p.dragging
            && !p.broken
        {
            self.events.push(VertexEvent::Cancel);
        }
    }

    /// True while the mouse makes a press.
    pub fn mouse(&self) -> bool {
        self.press
            .as_ref()
            .is_some_and(|p| p.pointer == Pointer::Mouse)
    }

    /// The finger that drags a handle. The view does not get it.
    pub fn claimed_finger(&self) -> Option<u64> {
        self.press
            .as_ref()
            .filter(|p| p.pointer == Pointer::Finger && p.on_handle && !p.broken)
            .map(|p| p.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::polygon::tests::{assert_valid, inside};

    const SCALE: f64 = 300.0;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
        Geometry::Polygons(vec![vec![vec![
            [x0, y0],
            [x1, y0],
            [x1, y1],
            [x0, y1],
            [x0, y0],
        ]]])
    }

    fn rings(g: &Geometry) -> &Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not polygons")
        };
        p
    }

    fn parts(g: &Geometry) -> &Vec<Vec<LonLat>> {
        let Geometry::Lines(p) = g else {
            panic!("not lines")
        };
        p
    }

    #[test]
    fn a_move_changes_one_corner_of_a_square() {
        let g = square(0.0, 0.0, 10.0, 10.0);
        let moved = move_vertex(&g, [10.0, 10.0], [14.0, 12.0], SCALE).unwrap();
        let ring = &rings(&moved)[0][0];
        assert!(ring.contains(&[14.0, 12.0]));
        assert!(!ring.contains(&[10.0, 10.0]));
        for p in [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]] {
            assert!(ring.contains(&p), "{p:?} kept its bits");
        }
        assert_valid(rings(&moved));
        assert!(inside(rings(&moved), [12.0, 10.5]));
    }

    #[test]
    fn a_move_of_a_shared_vertex_gives_both_squares_one_position() {
        let a = square(0.0, 0.0, 10.0, 10.0);
        let b = square(10.0, 0.0, 20.0, 10.0);
        let to = [12.5, 4.25];
        let a2 = move_vertex(&a, [10.0, 0.0], to, SCALE).unwrap();
        let b2 = move_vertex(&b, [10.0, 0.0], to, SCALE).unwrap();
        assert!(rings(&a2)[0][0].contains(&to));
        assert!(rings(&b2)[0][0].contains(&to));
        // The border goes from (12.5, 4.25) to (10, 10), with no gap and no
        // overlap.
        assert!(inside(rings(&a2), [11.0, 7.0]) && !inside(rings(&b2), [11.0, 7.0]));
        assert!(inside(rings(&b2), [11.6, 7.2]) && !inside(rings(&a2), [11.6, 7.2]));
    }

    #[test]
    fn an_insert_adds_a_vertex_on_the_edge_in_either_direction() {
        let g = square(0.0, 0.0, 10.0, 10.0);
        for (a, b) in [([0.0, 0.0], [10.0, 0.0]), ([10.0, 0.0], [0.0, 0.0])] {
            let new = insert_vertex(&g, a, b, [5.0, 0.0], SCALE).unwrap();
            let ring = &rings(&new)[0][0];
            assert_eq!(ring.len(), 6);
            assert!(ring.contains(&[5.0, 0.0]));
        }
        let line = Geometry::Lines(vec![vec![[0.0, 0.0], [10.0, 0.0]]]);
        let new = insert_vertex(&line, [0.0, 0.0], [10.0, 0.0], [5.0, 1.0], SCALE).unwrap();
        assert_eq!(parts(&new)[0], vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]]);
    }

    #[test]
    fn a_delete_keeps_a_path_that_would_have_too_few_vertices() {
        let line = Geometry::Lines(vec![vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]]]);
        let new = delete_vertices(&line, &[[5.0, 1.0]], SCALE).unwrap();
        assert_eq!(parts(&new)[0], vec![[0.0, 0.0], [10.0, 0.0]]);
        assert!(delete_vertices(&new, &[[0.0, 0.0]], SCALE).is_none());
        let g = square(0.0, 0.0, 10.0, 10.0);
        let tri = delete_vertices(&g, &[[10.0, 10.0]], SCALE).unwrap();
        assert_eq!(rings(&tri)[0][0].len(), 4);
        assert!(delete_vertices(&tri, &[[0.0, 0.0]], SCALE).is_none());
    }

    #[test]
    fn a_smooth_changes_only_the_selected_part() {
        let mut verts = Vec::new();
        for i in 0..=10 {
            verts.push([i as f64, if i % 2 == 0 { 0.0 } else { 1.0 }]);
        }
        let line = Geometry::Lines(vec![verts.clone()]);
        let new = smooth_vertices(&line, &[[3.0, 1.0], [7.0, 1.0]], SCALE, 1.0).unwrap();
        let out = &parts(&new)[0];
        assert_eq!(out[..4], verts[..4]);
        assert_eq!(out[out.len() - 4..], verts[7..]);
        assert_ne!(out[4..out.len() - 4], verts[4..7]);
    }

    #[test]
    fn a_smooth_of_a_ring_leaves_the_longest_unselected_run() {
        let ring: Vec<LonLat> = (0..12)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 12.0;
                let r = if i % 2 == 0 { 10.0 } else { 7.0 };
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let mut closed = ring.clone();
        closed.push(ring[0]);
        let g = Geometry::Polygons(vec![vec![closed]]);
        let new = smooth_vertices(&g, &[ring[1], ring[5]], SCALE, 1.0).unwrap();
        let out = &rings(&new)[0][0];
        for i in [0, 1, 5, 6, 7, 8, 9, 10, 11] {
            assert!(out.contains(&ring[i]), "vertex {i} kept its bits");
        }
        for i in [2, 3, 4] {
            assert!(!out.contains(&ring[i]), "vertex {i} moved");
        }
        assert_valid(rings(&new));
    }

    #[test]
    fn a_move_across_the_180_degree_meridian_gives_valid_polygons() {
        let g = square(170.0, 0.0, 178.0, 10.0);
        let moved = move_vertex(&g, [178.0, 10.0], [-175.0, 12.0], SCALE).unwrap();
        let polygons = rings(&moved);
        assert_valid(polygons);
        assert!(inside(polygons, [179.0, 9.0]));
        assert!(inside(polygons, [-178.0, 9.5]));
    }

    #[test]
    fn a_group_move_turns_each_vertex_by_the_rotation_of_the_dragged_one() {
        let (from, to) = ([10.0, 10.0], [14.0, 12.0]);
        let moves = group_moves(from, (dir(to), None), &[[10.0, 0.0], from]);
        assert_eq!(moves.len(), 2);
        assert!(near(moves[0].1, to));
        // The angle between the two vertices stays the same.
        let before = angle(dir(from), dir([10.0, 0.0]));
        let after = angle(dir(moves[0].1), dir(moves[1].1));
        assert!((before - after).abs() < 1e-12);
        let g = square(0.0, 0.0, 10.0, 10.0);
        let moved = move_vertices(&g, &moves, SCALE).unwrap();
        let ring = &rings(&moved)[0][0];
        assert!(!ring.contains(&[10.0, 10.0]) && !ring.contains(&[10.0, 0.0]));
        assert!(ring.contains(&[0.0, 0.0]) && ring.contains(&[0.0, 10.0]));
        assert_valid(rings(&moved));
        // A snapped position is kept as it is.
        let moves = group_moves(from, (dir(to), Some(to)), &[[10.0, 0.0]]);
        assert_eq!(moves[0].1, to);
    }

    fn near(a: LonLat, b: LonLat) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn the_sides_of_the_meridian_and_a_pole_are_one_place() {
        assert!(same([180.0, 5.0], [-180.0, 5.0]));
        assert!(same([10.0, 90.0], [-50.0, 90.0]));
        assert!(!same([10.0, 90.0], [10.0, -90.0]));
        assert!(!same([10.0, 5.0], [10.0, 5.000001]));
    }

    #[test]
    fn a_finger_drags_from_a_handle_and_not_from_elsewhere() {
        let mut press = VertexPress::default();
        press.down(1, Pos2::new(0.0, 0.0), Pointer::Finger, true);
        assert_eq!(press.claimed_finger(), Some(1));
        press.moved(1, Pos2::new(30.0, 0.0));
        press.up(1, Pos2::new(40.0, 0.0));
        let events = std::mem::take(&mut press.events);
        assert!(matches!(events[0], VertexEvent::DragStart { .. }));
        assert!(matches!(events.last(), Some(VertexEvent::DragEnd { .. })));

        press.down(2, Pos2::new(0.0, 0.0), Pointer::Finger, false);
        assert_eq!(press.claimed_finger(), None);
        press.moved(2, Pos2::new(30.0, 0.0));
        press.up(2, Pos2::new(40.0, 0.0));
        assert!(std::mem::take(&mut press.events).is_empty());

        press.down(3, Pos2::new(0.0, 0.0), Pointer::Pen, false);
        press.moved(3, Pos2::new(30.0, 0.0));
        press.up(3, Pos2::new(40.0, 0.0));
        assert!(matches!(press.events[0], VertexEvent::DragStart { .. }));
    }
}
