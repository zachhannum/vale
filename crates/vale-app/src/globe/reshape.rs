//! Reshape. A stroke from a point of the outline of a line or a polygon
//! takes the place of a part of the outline.
//!
//! On a polygon, the stroke goes from one point of a ring to another point
//! of the same ring. The ring then has two possible shapes, one with each
//! part of the old ring, and the larger area stays. On a line, a stroke that
//! ends on the line takes the place of the part between its ends. A stroke
//! that ends away from the line takes the place of the rest of the line,
//! toward the end of the line that is nearer to the end of the stroke.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use vale_store::{Geometry, LonLat};

use super::line::{chains, to_parts_exact};
use super::math::{V3, angle, lonlat_to_dir};
use super::polygon::{
    MIN_AREA, MIN_LENGTH, SAME, densify, dirs, path, path_length, rings, signed_area, to_polygons,
};
use super::snap::{PathVertex, Run, append, project};
use super::vector;

/// The distance in points from the outline within which an end of a stroke
/// is on it.
pub const REACH: f64 = 12.0;

/// The geometry after a stroke reshapes it. The runs come from a `Tracer`.
/// A stroke that does not start on the outline, or that gives no valid
/// shape, gives `None`.
pub fn reshape(geometry: &Geometry, runs: &[Run], scale: f64, strength: f64) -> Option<Geometry> {
    match geometry {
        Geometry::Lines(parts) => reshape_lines(parts, runs, scale, strength).map(Geometry::Lines),
        Geometry::Polygons(polygons) => {
            reshape_polygons(polygons, runs, scale, strength).map(Geometry::Polygons)
        }
        Geometry::Points(_) => None,
    }
}

/// A place on a path: on the edge from vertex `edge` to the next vertex, at
/// the angle `along` from the vertex.
#[derive(Clone, Copy)]
struct Spot {
    edge: usize,
    along: f64,
    at: PathVertex,
    dist: f64,
}

impl Spot {
    fn before(&self, other: &Spot) -> bool {
        self.edge < other.edge || (self.edge == other.edge && self.along <= other.along)
    }
}

/// The nearest place of a path to `p`. The path of a ring does not repeat
/// its first vertex.
fn locate(path: &[PathVertex], closed: bool, p: V3) -> Option<Spot> {
    let edges = if closed {
        path.len()
    } else {
        path.len().saturating_sub(1)
    };
    let mut best: Option<Spot> = None;
    for i in 0..edges {
        let (a, b) = (path[i], path[(i + 1) % path.len()]);
        let (q, along, dist) = project(p, a.dir(), b.dir());
        if best.is_some_and(|s| s.dist <= dist) {
            continue;
        }
        let at = if angle(q, a.dir()) < SAME {
            a
        } else if angle(q, b.dir()) < SAME {
            b
        } else {
            PathVertex::Free(q)
        };
        best = Some(Spot {
            edge: i,
            along,
            at,
            dist,
        });
    }
    best
}

/// The path from one place to another, forward. On a ring, it goes past
/// the first vertex when `to` is before `from`.
fn slice(path: &[PathVertex], from: &Spot, to: &Spot) -> Vec<PathVertex> {
    let mut out = vec![from.at];
    if from.before(to) {
        append(&mut out, path[from.edge + 1..=to.edge].iter().copied());
    } else {
        append(&mut out, path[from.edge + 1..].iter().copied());
        append(&mut out, path[..=to.edge].iter().copied());
    }
    append(&mut out, [to.at]);
    out
}

/// The first and the last point of a stroke.
fn ends(runs: &[Run]) -> Option<(V3, V3)> {
    let points = |r: &Run| -> Vec<V3> {
        match r {
            Run::Free(s) => s.clone(),
            Run::Traced(t) => t.iter().map(PathVertex::dir).collect(),
        }
    };
    let first = runs.iter().find_map(|r| points(r).first().copied())?;
    let last = runs.iter().rev().find_map(|r| points(r).last().copied())?;
    Some((first, last))
}

/// The path of a stroke from `from`, to `to` when it is given.
fn stroke(
    runs: &[Run],
    from: PathVertex,
    to: Option<PathVertex>,
    scale: f64,
    strength: f64,
) -> Option<Vec<PathVertex>> {
    let mut runs = runs.to_vec();
    runs.retain(|r| match r {
        Run::Free(s) => !s.is_empty(),
        Run::Traced(t) => !t.is_empty(),
    });
    // The smoothing keeps the ends of a run, so the stroke meets the
    // outline without a step.
    match runs.first_mut()? {
        Run::Free(s) => s[0] = from.dir(),
        Run::Traced(t) => t[0] = from,
    }
    if let Some(to) = to {
        match runs.last_mut()? {
            Run::Free(s) => *s.last_mut()? = to.dir(),
            Run::Traced(t) => *t.last_mut()? = to,
        }
    }
    let mut out = path(&runs, scale, strength);
    out[0] = from;
    if let Some(to) = to {
        *out.last_mut()? = to;
    }
    (path_length(&out) >= MIN_LENGTH / scale).then_some(out)
}

fn reversed(path: &[PathVertex]) -> Vec<PathVertex> {
    path.iter().rev().copied().collect()
}

fn reshape_lines(
    parts: &[Vec<LonLat>],
    runs: &[Run],
    scale: f64,
    strength: f64,
) -> Option<Vec<Vec<LonLat>>> {
    let (p0, p1) = ends(runs)?;
    let reach = REACH / scale;
    let chains = chains(parts);
    let paths: Vec<Vec<PathVertex>> = chains
        .iter()
        .map(|chain| {
            let mut path = Vec::new();
            for &i in chain {
                append(&mut path, parts[i].iter().map(|&p| PathVertex::Exact(p)));
            }
            path
        })
        .collect();
    let (k, s0) = paths
        .iter()
        .enumerate()
        .filter_map(|(k, p)| Some((k, locate(p, false, p0)?)))
        .min_by(|a, b| a.1.dist.total_cmp(&b.1.dist))?;
    if s0.dist > reach {
        return None;
    }
    let line = &paths[k];
    let n = line.len();
    let start = Spot {
        edge: 0,
        along: 0.0,
        at: line[0],
        dist: 0.0,
    };
    let end = Spot {
        edge: n - 2,
        along: angle(line[n - 2].dir(), line[n - 1].dir()),
        at: line[n - 1],
        dist: 0.0,
    };
    let s1 = locate(line, false, p1).filter(|s| s.dist <= reach);
    let mut new = Vec::new();
    match s1 {
        Some(s1) => {
            let p = stroke(runs, s0.at, Some(s1.at), scale, strength)?;
            let (a, b, p) = if s0.before(&s1) {
                (s0, s1, p)
            } else {
                (s1, s0, reversed(&p))
            };
            append(&mut new, slice(line, &start, &a));
            append(&mut new, p);
            append(&mut new, slice(line, &b, &end));
        }
        None => {
            let p = stroke(runs, s0.at, None, scale, strength)?;
            if angle(p1, line[n - 1].dir()) <= angle(p1, line[0].dir()) {
                append(&mut new, slice(line, &start, &s0));
                append(&mut new, p);
            } else {
                append(&mut new, reversed(&p));
                append(&mut new, slice(line, &s0, &end));
            }
        }
    }
    let mut out = Vec::new();
    let first = *chains[k].iter().min()?;
    for (i, part) in parts.iter().enumerate() {
        if i == first {
            out.extend(to_parts_exact(&new));
        } else if !chains[k].contains(&i) {
            out.push(part.clone());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A ring of a polygon feature on the sphere, with the polygon on the
/// left. A ring that crosses the 180 degree meridian is one outline.
pub(super) struct Outline {
    ring: Vec<PathVertex>,
    hole: bool,
    /// The polygons and the rings that make the outline.
    sources: Vec<(usize, usize)>,
}

/// The area of a ring on a flat map, positive when it goes
/// counterclockwise.
fn flat_area(ring: &[LonLat]) -> f64 {
    ring.windows(2)
        .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
        .sum::<f64>()
        / 2.0
}

/// The outlines of stored polygons. The edges along the 180 degree
/// meridian and along a pole join the parts of a polygon, and they are not
/// edges of an outline. Rings that do not join give `None`.
pub(super) fn outlines(polygons: &[Vec<Vec<LonLat>>]) -> Option<Vec<Outline>> {
    struct Edge {
        a: LonLat,
        b: LonLat,
        source: (usize, usize),
    }
    let dir = |p: LonLat| lonlat_to_dir(p[0], p[1]);
    let mut edges = Vec::new();
    for (i, polygon) in polygons.iter().enumerate() {
        for (j, ring) in polygon.iter().enumerate() {
            let mut ring = ring.clone();
            if ring.len() > 1 && ring.first() != ring.last() {
                ring.push(ring[0]);
            }
            // An outer ring goes counterclockwise, and a hole clockwise.
            if (flat_area(&ring) > 0.0) == (j > 0) {
                ring.reverse();
            }
            for w in ring.windows(2) {
                let (a, b) = (w[0], w[1]);
                let seam = a[0].abs() == 180.0 && a[0] == b[0];
                if seam || angle(dir(a), dir(b)) < SAME {
                    continue;
                }
                edges.push(Edge {
                    a,
                    b,
                    source: (i, j),
                });
            }
        }
    }
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    for first in 0..edges.len() {
        if used[first] {
            continue;
        }
        let mut ring = Vec::new();
        let mut sources: Vec<(usize, usize)> = Vec::new();
        let mut at = first;
        loop {
            used[at] = true;
            let e = &edges[at];
            append(&mut ring, [PathVertex::Exact(e.a), PathVertex::Exact(e.b)]);
            if !sources.contains(&e.source) {
                sources.push(e.source);
            }
            let end = dir(e.b);
            let next = (0..edges.len()).find(|&n| !used[n] && angle(dir(edges[n].a), end) < SAME);
            match next {
                Some(n) => at = n,
                None if angle(end, dir(edges[first].a)) < SAME => break,
                None => return None,
            }
        }
        if ring.len() > 1 && ring[ring.len() - 1] == ring[0] {
            ring.pop();
        }
        if ring.len() >= 3 {
            out.push(Outline {
                ring,
                hole: edges[first].source.1 > 0,
                sources,
            });
        }
    }
    Some(out)
}

fn reshape_polygons(
    polygons: &[Vec<Vec<LonLat>>],
    runs: &[Run],
    scale: f64,
    strength: f64,
) -> Option<Vec<Vec<Vec<LonLat>>>> {
    let (p0, p1) = ends(runs)?;
    let reach = REACH / scale;
    let outlines = outlines(polygons)?;
    let (k, s0) = outlines
        .iter()
        .enumerate()
        .filter_map(|(k, o)| Some((k, locate(&o.ring, true, p0)?)))
        .min_by(|a, b| a.1.dist.total_cmp(&b.1.dist))?;
    let outline = &outlines[k];
    let s1 = locate(&outline.ring, true, p1)?;
    if s0.dist > reach || s1.dist > reach {
        return None;
    }
    let p = stroke(runs, s0.at, Some(s1.at), scale, strength)?;
    // Both shapes have the polygon on the left.
    let close = |mut c: Vec<PathVertex>| {
        if c.len() > 1 && angle(c[0].dir(), c[c.len() - 1].dir()) < SAME {
            c.pop();
        }
        c
    };
    let mut a = p.clone();
    append(&mut a, slice(&outline.ring, &s1, &s0));
    let mut b = slice(&outline.ring, &s0, &s1);
    append(&mut b, reversed(&p));
    let (a, b) = (close(a), close(b));
    let min_area = MIN_AREA / (scale * scale);
    // The larger shape stays. A hole compares the areas inside it.
    let area = |c: &[PathVertex]| signed_area(&dirs(&densify(c)));
    if outline.hole {
        let (a, b) = (reversed(&a), reversed(&b));
        let ring = if area(&a) >= area(&b) { a } else { b };
        hole(polygons, outline, &ring, min_area)
    } else {
        let ring = if area(&a) >= area(&b) { a } else { b };
        outer(polygons, outline, &ring, min_area)
    }
}

/// The polygons after a reshape of an outer ring.
fn outer(
    polygons: &[Vec<Vec<LonLat>>],
    outline: &Outline,
    ring: &[PathVertex],
    min_area: f64,
) -> Option<Vec<Vec<Vec<LonLat>>>> {
    let mut new: Vec<Vec<Vec<LonLat>>> = rings(&densify(ring))
        .into_iter()
        .filter(|r| signed_area(&dirs(r)) > min_area)
        .flat_map(|r| to_polygons(&r))
        .collect();
    if new.is_empty() {
        return None;
    }
    // A hole goes to the new polygon that holds it.
    let replaced: Vec<usize> = outline.sources.iter().map(|s| s.0).collect();
    for &i in &replaced {
        for hole in polygons[i].iter().skip(1) {
            let p = lonlat_to_dir(hole[0][0], hole[0][1]);
            if let Some(poly) = new
                .iter_mut()
                .find(|poly| vector::contains(&[vector::ring(&poly[0], false)], p))
            {
                poly.push(hole.clone());
            }
        }
    }
    let first = *replaced.iter().min()?;
    let mut out = Vec::new();
    for (i, polygon) in polygons.iter().enumerate() {
        if i == first {
            out.append(&mut new);
        } else if !replaced.contains(&i) {
            out.push(polygon.clone());
        }
    }
    Some(out)
}

/// The polygons after a reshape of a hole. The inside of the hole is on
/// the left of `ring`. A hole that crosses the 180 degree meridian gives
/// `None`.
fn hole(
    polygons: &[Vec<Vec<LonLat>>],
    outline: &Outline,
    ring: &[PathVertex],
    min_area: f64,
) -> Option<Vec<Vec<Vec<LonLat>>>> {
    let [(i, j)] = outline.sources[..] else {
        return None;
    };
    let mut holes = Vec::new();
    for r in rings(&densify(ring)) {
        if signed_area(&dirs(&r)) <= min_area {
            continue;
        }
        let mut closed = r.clone();
        closed.push(r[0]);
        let [part] = &to_parts_exact(&closed)[..] else {
            return None;
        };
        holes.push(part.clone());
    }
    let mut out = polygons.to_vec();
    out[i].splice(j..=j, holes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vale_store::GeometryKind;

    use super::*;
    use crate::globe::math::{V3, lonlat_to_dir};
    use crate::globe::polygon::tests::{assert_valid, has_run, inside};
    use crate::globe::snap::tests::{self as snap_test, kinds, pen, trace_excluding};
    use crate::globe::snap::{SnapSettings, distance_to_arc};

    const SCALE: f64 = 300.0;

    fn polygons(g: &Geometry) -> &Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not a polygon");
        };
        p
    }

    fn lines(g: &Geometry) -> &Vec<Vec<LonLat>> {
        let Geometry::Lines(l) = g else {
            panic!("not a line");
        };
        l
    }

    fn free(way: &[LonLat]) -> Vec<Run> {
        vec![Run::Free(pen(way, 0.0))]
    }

    /// A stored polygon of a ring with its inside on the left.
    /// The polygon that the polygon tool makes of a closed way. A longitude
    /// can be past 180 degrees.
    fn stored(ring: &[LonLat]) -> Vec<Vec<Vec<LonLat>>> {
        let g = crate::globe::polygon::commit(&free(ring), SCALE, 0.0).expect("a polygon");
        polygons(&g).clone()
    }

    fn square() -> Vec<Vec<Vec<LonLat>>> {
        vec![vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ]]]
    }

    fn reshaped(polygons: Vec<Vec<Vec<LonLat>>>, way: &[LonLat]) -> Option<Vec<Vec<Vec<LonLat>>>> {
        let g = reshape(&Geometry::Polygons(polygons), &free(way), SCALE, 0.0)?;
        Some(self::polygons(&g).clone())
    }

    fn corners(p: &[Vec<Vec<LonLat>>]) -> usize {
        let ring = &p[0][0];
        [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]
            .iter()
            .filter(|c| ring.contains(c))
            .count()
    }

    #[test]
    fn a_stroke_between_two_points_of_an_outline_replaces_the_part_between() {
        let way = [[3.0, 0.0], [3.0, -4.0], [7.0, -4.0], [7.0, 0.0]];
        let p = reshaped(square(), &way).expect("a polygon");
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert!(inside(&p, [5.0, -2.0]));
        assert!(inside(&p, [5.0, 5.0]));
        assert!(!inside(&p, [1.0, -2.0]));
        assert_eq!(corners(&p), 4);
        // A stroke the other way gives the same shape.
        let back: Vec<LonLat> = way.iter().rev().copied().collect();
        let q = reshaped(square(), &back).expect("a polygon");
        assert_valid(&q);
        assert!(inside(&q, [5.0, -2.0]));
        assert_eq!(corners(&q), 4);
    }

    #[test]
    fn a_stroke_into_the_polygon_cuts_a_notch() {
        let way = [[3.0, 0.0], [3.0, 4.0], [7.0, 4.0], [7.0, 0.0]];
        let p = reshaped(square(), &way).expect("a polygon");
        assert_valid(&p);
        assert!(!inside(&p, [5.0, 2.0]));
        assert!(inside(&p, [5.0, 6.0]));
        assert!(inside(&p, [1.0, 2.0]));
    }

    #[test]
    fn the_reshape_keeps_the_larger_area() {
        let way = [[0.0, 3.0], [10.0, 3.0]];
        let p = reshaped(square(), &way).expect("a polygon");
        assert_valid(&p);
        assert!(inside(&p, [5.0, 6.0]));
        assert!(!inside(&p, [5.0, 1.0]));
        let way = [[0.0, 7.0], [10.0, 7.0]];
        let p = reshaped(square(), &way).expect("a polygon");
        assert!(inside(&p, [5.0, 4.0]));
        assert!(!inside(&p, [5.0, 9.0]));
    }

    #[test]
    fn a_stroke_that_crosses_itself_gives_a_valid_polygon() {
        let way = [
            [2.0, 0.0],
            [4.0, -4.0],
            [6.0, -1.0],
            [3.0, -1.0],
            [5.0, -5.0],
            [8.0, 0.0],
        ];
        let p = reshaped(square(), &way).expect("a polygon");
        assert_valid(&p);
        assert!(inside(&p, [5.0, 5.0]));
    }

    #[test]
    fn a_stroke_that_does_not_start_on_the_outline_changes_nothing() {
        let way = [[5.0, -5.0], [5.0, 0.0], [7.0, 0.0]];
        assert!(reshaped(square(), &way).is_none());
        let way = [[5.0, 5.0], [5.0, 0.0]];
        assert!(reshaped(square(), &way).is_none());
    }

    #[test]
    fn a_reshape_of_a_hole_keeps_the_hole_in_the_polygon() {
        let mut p = square();
        p[0].push(vec![
            [3.0, 3.0],
            [3.0, 7.0],
            [7.0, 7.0],
            [7.0, 3.0],
            [3.0, 3.0],
        ]);
        let way = [[5.0, 3.0], [5.0, 1.0], [6.0, 3.0]];
        let p = reshaped(p, &way).expect("a polygon");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
        assert!(!inside(&p, [5.0, 5.0]));
        assert!(!inside(&p, [5.2, 2.5]));
        assert!(inside(&p, [1.0, 1.0]));
        assert!(inside(&p, [8.0, 5.0]));
    }

    #[test]
    fn outlines_join_the_parts_of_a_polygon_across_180() {
        let p = stored(&[
            [170.0, -10.0],
            [190.0, -10.0],
            [190.0, 10.0],
            [170.0, 10.0],
            [170.0, -10.0],
        ]);
        assert_eq!(p.len(), 2);
        let o = outlines(&p).expect("outlines");
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].sources.len(), 2);
    }

    #[test]
    fn a_reshape_across_180_is_correct() {
        let p = stored(&[
            [170.0, -10.0],
            [190.0, -10.0],
            [190.0, 10.0],
            [170.0, 10.0],
            [170.0, -10.0],
        ]);
        let way = [
            [175.0, -10.0],
            [175.0, -15.0],
            [185.0, -15.0],
            [185.0, -10.0],
        ];
        let p = reshaped(p, &way).expect("a polygon");
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        for q in [[179.5, -12.0], [-179.5, -12.0], [179.5, 5.0], [-179.5, 5.0]] {
            assert!(inside(&p, q), "{q:?}");
        }
        assert!(!inside(&p, [172.0, -12.0]));
        assert!(!inside(&p, [-172.0, -12.0]));
    }

    #[test]
    fn a_reshape_near_a_pole_keeps_the_pole_inside() {
        // A cap as the polygon tool stores it, along the pole.
        let mut ring: Vec<LonLat> = (0..=36)
            .map(|i| [f64::from(i) * 10.0 - 180.0, 70.0])
            .collect();
        ring.extend([[180.0, 90.0], [-180.0, 90.0], [-180.0, 70.0]]);
        let cap = vec![vec![ring]];
        let p = cap.clone();
        assert_valid(&p);
        assert!(inside(&p, [0.0, 89.0]));
        let way = [[-20.0, 70.0], [-20.0, 80.0], [20.0, 80.0], [20.0, 70.0]];
        let p = reshaped(p, &way).expect("a polygon");
        assert_valid(&p);
        assert!(inside(&p, [0.0, 89.0]));
        assert!(inside(&p, [90.0, 75.0]));
        assert!(!inside(&p, [0.0, 75.0]));
        // A dent across the 180 degree meridian.
        let way = [[160.0, 70.0], [160.0, 85.0], [200.0, 85.0], [200.0, 70.0]];
        let q = reshaped(cap, &way).expect("a polygon");
        assert_valid(&q);
        assert!(inside(&q, [0.0, 89.0]));
        assert!(inside(&q, [0.0, 75.0]));
        for p in [[179.5, 80.0], [-179.5, 80.0]] {
            assert!(!inside(&q, p), "{p:?}");
        }
    }

    #[test]
    fn a_part_of_the_stroke_traces_a_river() {
        let country = square();
        let river = vec![vec![
            [2.0, 1.0],
            [3.1234567, 3.0],
            [5.4321, 2.5],
            [6.789, 3.3],
            [8.0, 1.0],
        ]];
        let a = snap_test::layer(1, GeometryKind::Polygon, vec![Geometry::Polygons(country)]);
        let b = snap_test::layer(2, GeometryKind::Line, vec![Geometry::Lines(river)]);
        let uuid = a.features[0].uuid();
        let t = Arc::new(snap_test::targets(&[a, b]));
        // The pen goes in from the south side, along the river, and out.
        let way = [
            [1.5, 0.0],
            [2.6, 2.1],
            [3.1, 2.9],
            [5.4, 2.6],
            [6.8, 3.2],
            [7.4, 2.2],
            [8.5, 0.0],
        ];
        let runs = trace_excluding(&t, &pen(&way, 0.0), SnapSettings::default(), 1, Some(uuid));
        assert!(kinds(&runs).contains('T'), "{}", kinds(&runs));
        let g = reshape(&Geometry::Polygons(square()), &runs, SCALE, 0.0).expect("a polygon");
        let p = polygons(&g);
        assert_valid(p);
        assert!(has_run(p, &[[3.1234567, 3.0], [5.4321, 2.5], [6.789, 3.3]]));
        assert!(inside(p, [5.0, 6.0]));
        assert!(!inside(p, [5.0, 1.5]));
    }

    #[test]
    fn the_stroke_does_not_trace_the_reshaped_feature() {
        let a = snap_test::layer(1, GeometryKind::Polygon, vec![Geometry::Polygons(square())]);
        let uuid = a.features[0].uuid();
        let t = Arc::new(snap_test::targets(&[a]));
        let way = [[3.0, 0.0], [3.0, -0.1], [7.0, -0.1], [7.0, 0.0]];
        let runs = trace_excluding(&t, &pen(&way, 0.0), SnapSettings::default(), 1, Some(uuid));
        assert!(!kinds(&runs).contains('T'), "{}", kinds(&runs));
    }

    fn line() -> Vec<Vec<LonLat>> {
        vec![(0..=10).map(|i| [f64::from(i) * 2.0, 0.0]).collect()]
    }

    fn reshaped_line(parts: Vec<Vec<LonLat>>, way: &[LonLat]) -> Option<Vec<Vec<LonLat>>> {
        let g = reshape(&Geometry::Lines(parts), &free(way), SCALE, 0.0)?;
        Some(lines(&g).clone())
    }

    fn near(parts: &[Vec<LonLat>], p: LonLat) -> bool {
        let d = lonlat_to_dir(p[0], p[1]);
        let line: Vec<V3> = super::super::line::dirs(parts);
        line.windows(2)
            .any(|w| distance_to_arc(d, w[0], w[1]) < 0.2_f64.to_radians())
    }

    #[test]
    fn a_stroke_between_two_points_of_a_line_replaces_the_part_between() {
        let way = [[5.0, 0.0], [5.0, 3.0], [11.0, 3.0], [11.0, 0.0]];
        let l = reshaped_line(line(), &way).expect("a line");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0][0], [0.0, 0.0]);
        assert_eq!(*l[0].last().unwrap(), [20.0, 0.0]);
        assert!(near(&l, [8.0, 3.0]));
        assert!(!l[0].contains(&[8.0, 0.0]));
        assert!(l[0].contains(&[4.0, 0.0]) && l[0].contains(&[12.0, 0.0]));
        // A stroke the other way gives a line in the same direction.
        let back: Vec<LonLat> = way.iter().rev().copied().collect();
        let m = reshaped_line(line(), &back).expect("a line");
        assert_eq!(m[0][0], [0.0, 0.0]);
        assert_eq!(*m[0].last().unwrap(), [20.0, 0.0]);
        assert!(near(&m, [8.0, 3.0]));
    }

    #[test]
    fn a_stroke_that_ends_away_replaces_the_rest_of_the_line() {
        let other = vec![[0.0, 30.0], [5.0, 30.0]];
        let mut parts = line();
        parts.push(other.clone());
        // Toward the last end.
        let l = reshaped_line(parts.clone(), &[[11.0, 0.0], [16.0, 6.0]]).expect("a line");
        assert!(l.contains(&other));
        let main = l.iter().find(|p| p[0] == [0.0, 0.0]).expect("the line");
        assert!(main.contains(&[10.0, 0.0]));
        assert!(!main.contains(&[14.0, 0.0]));
        assert!(near(&l, [16.0, 6.0]));
        // Toward the first end.
        let l = reshaped_line(parts, &[[9.0, 0.0], [3.0, 6.0]]).expect("a line");
        let main = l
            .iter()
            .find(|p| p.last() == Some(&[20.0, 0.0]))
            .expect("the line");
        assert!(main.contains(&[10.0, 0.0]));
        assert!(!main.contains(&[4.0, 0.0]));
        assert!(near(&l, [3.0, 6.0]));
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn a_reshape_of_a_line_across_180_keeps_one_line() {
        let parts = vec![
            vec![[170.0, 0.0], [175.0, 0.0], [180.0, 0.0]],
            vec![[-180.0, 0.0], [-175.0, 0.0], [-170.0, 0.0]],
        ];
        let way = [[174.0, 0.0], [174.0, 4.0], [186.0, 4.0], [186.0, 0.0]];
        let l = reshaped_line(parts, &way).expect("a line");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0][0], [170.0, 0.0]);
        assert_eq!(*l[1].last().unwrap(), [-170.0, 0.0]);
        assert!(near(&l, [180.0, 4.0]));
        assert!(!near(&l, [180.0, 0.0]));
        let chains = chains(&l);
        assert_eq!(chains.len(), 1);
    }

    #[test]
    fn a_stroke_away_from_a_line_changes_nothing() {
        assert!(reshaped_line(line(), &[[5.0, 5.0], [9.0, 0.0]]).is_none());
    }
}
