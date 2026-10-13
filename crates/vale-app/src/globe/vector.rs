//! The geometry of vector layers on the globe. An edge between two positions
//! is the short arc of a great circle. The edge of the globe cuts each line
//! and each polygon.
//!
//! A drawn point is in disc space: the globe is the unit disc, x is right, and
//! y is up.

use std::f64::consts::TAU;

use super::brush::slerp;
use super::math::{Mat3, V3, add, angle, cross, dot, length, lonlat_to_dir, normalize, scale};

/// A point in disc space.
pub type P2 = [f64; 2];

/// Two directions closer than this are one position.
const SAME: f64 = 1e-12;

/// The largest angle between two drawn points of an arc, in radians. With
/// this step, a drawn arc is at most a quarter of a pixel from the true arc
/// on a globe of this radius.
pub fn step(radius_px: f64) -> f64 {
    (2.0 / radius_px.max(1.0)).sqrt().clamp(0.002, 0.2)
}

fn same(a: V3, b: V3) -> bool {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d) < SAME * SAME
}

/// The directions of a line.
pub fn line(positions: &[[f64; 2]]) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::with_capacity(positions.len());
    for p in positions {
        let d = lonlat_to_dir(p[0], p[1]);
        if out.last().is_none_or(|last| !same(*last, d)) {
            out.push(d);
        }
    }
    out
}

/// The directions of a ring, with the inside of the polygon on the left of
/// the direction of travel. The last position does not repeat the first. An
/// outer ring goes around the smaller of its two sides, and a hole goes
/// around the larger. A ring with no area is empty.
pub fn ring(positions: &[[f64; 2]], hole: bool) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::with_capacity(positions.len());
    for p in positions {
        let d = lonlat_to_dir(p[0], p[1]);
        let n = out.len();
        if n >= 1 && same(out[n - 1], d) {
            continue;
        }
        // A path that goes out and comes back on itself has no area. A ring
        // that touches a pole in longitude and latitude has such a path.
        if n >= 2 && same(out[n - 2], d) {
            out.pop();
            continue;
        }
        out.push(d);
    }
    loop {
        let n = out.len();
        if n >= 2 && same(out[0], out[n - 1]) {
            out.pop();
        } else if n >= 3 && same(out[1], out[n - 1]) {
            out.pop();
            out.remove(0);
        } else if n >= 3 && same(out[0], out[n - 2]) {
            out.truncate(n - 2);
        } else {
            break;
        }
    }
    if out.len() < 3 {
        return Vec::new();
    }
    if (turning(&out) < 0.0) != hole {
        out.reverse();
    }
    out
}

/// The sum of the left turns of a ring, in radians. The area on the left of
/// the ring is 2π minus this sum.
fn turning(ring: &[V3]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|j| {
            let (a, b, c) = (ring[(j + n - 1) % n], ring[j], ring[(j + 1) % n]);
            let (n1, n2) = (cross(a, b), cross(b, c));
            dot(b, cross(n1, n2)).atan2(dot(n1, n2))
        })
        .sum()
}

/// The arc from `p` to `q` crosses the arc from `a` to `b`. Each arc is the
/// short arc of its great circle.
fn arcs_cross(p: V3, q: V3, a: V3, b: V3) -> bool {
    let (n1, n2) = (cross(p, q), cross(a, b));
    if (dot(n1, a) > 0.0) == (dot(n1, b) > 0.0) || (dot(n2, p) > 0.0) == (dot(n2, q) > 0.0) {
        return false;
    }
    let x = cross(n1, n2);
    dot(x, add(a, b)) * dot(x, add(p, q)) > 0.0
}

/// A point is on the left of a ring.
fn left_of(ring: &[V3], p: V3) -> bool {
    let n = ring.len();
    let Some((a, b)) = (0..n)
        .map(|i| (ring[i], ring[(i + 1) % n]))
        .find(|(a, b)| angle(*a, *b) > 1e-9)
    else {
        return false;
    };
    // A point just on the left of the first edge.
    let q = normalize(add(
        normalize(add(a, b)),
        scale(normalize(cross(a, b)), 1e-7),
    ));
    // A path from `p` to `q` of two short arcs.
    let sum = add(p, q);
    let mid = if length(sum) > 1e-3 {
        normalize(sum)
    } else {
        let axis = if p[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        normalize(cross(p, axis))
    };
    let crossings = (0..n)
        .map(|i| (ring[i], ring[(i + 1) % n]))
        .filter(|&(a, b)| arcs_cross(p, mid, a, b) != arcs_cross(mid, q, a, b))
        .count();
    crossings % 2 == 0
}

/// A direction is inside a polygon. The rings come from `ring`.
pub fn contains(rings: &[Vec<V3>], p: V3) -> bool {
    !rings.is_empty() && rings.iter().all(|r| left_of(r, p))
}

/// A run of drawn points on the near side.
struct Piece {
    pts: Vec<P2>,
    /// The angle on the edge of the globe at which the run comes into view.
    enter: Option<f64>,
    /// The angle on the edge of the globe at which the run goes out of view.
    exit: Option<f64>,
}

/// The place where the arc from `a` to `b` meets the edge of the globe. The
/// two directions are in view space, on opposite sides.
fn crossing(a: V3, b: V3) -> V3 {
    let t = a[2] / (a[2] - b[2]);
    let (x, y) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
    let l = x.hypot(y);
    if l > 0.0 {
        [x / l, y / l, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    }
}

/// Adds the points of the arc from `a` to `b`, without `a`.
fn arc(pts: &mut Vec<P2>, a: V3, b: V3, step: f64) {
    let n = (angle(a, b) / step).ceil().max(1.0) as usize;
    for k in 1..n {
        let p = slerp(a, b, k as f64 / n as f64);
        pts.push([p[0], p[1]]);
    }
    pts.push([b[0], b[1]]);
}

/// Cuts a path in view space into its runs on the near side.
fn pieces(view: &[V3], closed: bool, step: f64, out: &mut Vec<Piece>) {
    let n = view.len();
    if n < 2 {
        return;
    }
    let near = |v: V3| v[2] > 0.0;
    // A closed path starts at a hidden point, so that no run goes through the
    // start.
    let start = match closed {
        true => view.iter().position(|v| !near(*v)).unwrap_or(0),
        false => 0,
    };
    let edges = if closed { n } else { n - 1 };
    let mut cur: Option<Piece> = None;
    for k in 0..edges {
        let (a, b) = (view[(start + k) % n], view[(start + k + 1) % n]);
        match (near(a), near(b)) {
            (false, false) => {}
            (false, true) => {
                let x = crossing(a, b);
                let mut pts = vec![[x[0], x[1]]];
                arc(&mut pts, x, b, step);
                cur = Some(Piece {
                    pts,
                    enter: Some(x[1].atan2(x[0])),
                    exit: None,
                });
            }
            (true, to) => {
                let piece = cur.get_or_insert_with(|| Piece {
                    pts: vec![[a[0], a[1]]],
                    enter: None,
                    exit: None,
                });
                if to {
                    arc(&mut piece.pts, a, b, step);
                } else {
                    let x = crossing(a, b);
                    arc(&mut piece.pts, a, x, step);
                    piece.exit = Some(x[1].atan2(x[0]));
                    out.extend(cur.take());
                }
            }
        }
    }
    out.extend(cur);
}

fn to_view(rot: &Mat3, pts: &[V3]) -> Vec<V3> {
    pts.iter().map(|p| rot.mul_vec(*p)).collect()
}

/// The parts of a line on the near side of the globe. `rot` takes a world
/// direction to view space.
pub fn clip_line(rot: &Mat3, pts: &[V3], step: f64) -> Vec<Vec<P2>> {
    let mut out = Vec::new();
    pieces(&to_view(rot, pts), false, step, &mut out);
    out.into_iter()
        .map(|p| p.pts)
        .filter(|p| p.len() >= 2)
        .collect()
}

/// The part of a polygon on the near side of the globe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clipped {
    /// The closed paths around the fill. The last point does not repeat the
    /// first. A path with the fill on its left is counter-clockwise. Part of
    /// a path can go along the edge of the globe.
    pub loops: Vec<Vec<P2>>,
    /// The parts of the rings on the near side, for the stroke.
    pub outlines: Vec<Vec<P2>>,
}

/// Cuts a polygon at the edge of the globe. The rings come from `ring`.
pub fn clip_polygon(rot: &Mat3, rings: &[Vec<V3>], step: f64) -> Clipped {
    let mut out = Clipped::default();
    // The runs that come into view and go out of view at the edge.
    let mut chains: Vec<(f64, f64, Vec<P2>)> = Vec::new();
    for ring in rings {
        let mut runs = Vec::new();
        pieces(&to_view(rot, ring), true, step, &mut runs);
        for run in runs {
            out.outlines.push(run.pts.clone());
            match (run.enter, run.exit) {
                (Some(enter), Some(exit)) => chains.push((enter, exit, run.pts)),
                _ => {
                    let mut pts = run.pts;
                    pts.pop();
                    out.loops.push(pts);
                }
            }
        }
    }
    if chains.is_empty() {
        // No ring crosses the edge, so the edge is all inside or all outside.
        if contains(rings, rot.inv_mul_vec([1.0, 0.0, 0.0])) {
            let mut circle = vec![[1.0, 0.0]];
            limb(&mut circle, 0.0, TAU, step);
            out.loops.push(circle);
        }
        return out;
    }
    // The inside is on the left of a run. So from the place where a run goes
    // out of view, the fill goes counter-clockwise along the edge to the next
    // place where a run comes into view.
    let mut used = vec![false; chains.len()];
    for first in 0..chains.len() {
        if used[first] {
            continue;
        }
        let mut path = Vec::new();
        let mut at = first;
        loop {
            used[at] = true;
            path.extend_from_slice(&chains[at].2);
            let exit = chains[at].1;
            let sweep = |k: usize| (chains[k].0 - exit).rem_euclid(TAU);
            let next = (0..chains.len())
                .min_by(|a, b| sweep(*a).total_cmp(&sweep(*b)))
                .expect("a run");
            limb(&mut path, exit, sweep(next), step);
            if next == first {
                break;
            }
            if used[next] {
                // The rings cross each other or themselves.
                out.loops.clear();
                return out;
            }
            at = next;
        }
        out.loops.push(path);
    }
    out
}

/// Adds the points of the edge of the globe after the angle `from`, through
/// `sweep` radians counter-clockwise, without the two ends.
fn limb(pts: &mut Vec<P2>, from: f64, sweep: f64, step: f64) {
    let n = (sweep / step).ceil() as usize;
    for k in 1..n {
        let (s, c) = (from + sweep * k as f64 / n as f64).sin_cos();
        pts.push([c, s]);
    }
}

/// Twice the area of a closed path. A counter-clockwise path is positive.
fn area2(path: &[P2]) -> f64 {
    let n = path.len();
    (0..n)
        .map(|i| {
            let (a, b) = (path[i], path[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

fn inside(path: &[P2], p: P2) -> bool {
    let n = path.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (path[i], path[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

/// Triangles that cover the fill of `Clipped::loops`: the points, and three
/// indexes for each triangle.
pub fn triangulate(loops: &[Vec<P2>]) -> (Vec<P2>, Vec<u32>) {
    let areas: Vec<f64> = loops.iter().map(|l| area2(l)).collect();
    // A clockwise path is a hole of the smallest path around it.
    let mut holes: Vec<Vec<usize>> = vec![Vec::new(); loops.len()];
    for (h, hole) in loops.iter().enumerate() {
        if areas[h] >= 0.0 || hole.len() < 3 {
            continue;
        }
        let outer = (0..loops.len())
            .filter(|&o| areas[o] > 0.0 && inside(&loops[o], hole[0]))
            .min_by(|a, b| areas[*a].total_cmp(&areas[*b]));
        if let Some(outer) = outer {
            holes[outer].push(h);
        }
    }
    let (mut points, mut indexes) = (Vec::new(), Vec::new());
    let mut earcut = earcut::Earcut::new();
    let mut triangles: Vec<u32> = Vec::new();
    for (o, outer) in loops.iter().enumerate() {
        if areas[o] <= 0.0 || outer.len() < 3 {
            continue;
        }
        let base = points.len();
        points.extend_from_slice(outer);
        let mut starts = Vec::new();
        for &h in &holes[o] {
            starts.push((points.len() - base) as u32);
            points.extend_from_slice(&loops[h]);
        }
        earcut.earcut(points[base..].iter().copied(), &starts, &mut triangles);
        indexes.extend(triangles.iter().map(|i| i + base as u32));
    }
    (points, indexes)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::globe::view::GlobeView;

    const STEP: f64 = 0.05;

    fn rot(lon: f64, lat: f64) -> Mat3 {
        GlobeView::centered(lon, lat).rot
    }

    fn square(lon: f64, lat: f64, half: f64) -> Vec<[f64; 2]> {
        vec![
            [lon - half, lat - half],
            [lon + half, lat - half],
            [lon + half, lat + half],
            [lon - half, lat + half],
            [lon - half, lat - half],
        ]
    }

    /// A square with a square hole, around longitude 0 and latitude 0.
    fn plate() -> Vec<Vec<V3>> {
        vec![
            ring(&square(0.0, 0.0, 30.0), false),
            ring(&square(0.0, 0.0, 12.0), true),
        ]
    }

    /// The directions at `radius` degrees from a center, with the center on
    /// the left.
    fn circle(lon: f64, lat: f64, radius: f64) -> Vec<V3> {
        let to_world = rot(lon, lat);
        let (s, c) = radius.to_radians().sin_cos();
        (0..72)
            .map(|k| {
                let (y, x) = (f64::from(k) * TAU / 72.0).sin_cos();
                to_world.inv_mul_vec([s * x, s * y, c])
            })
            .collect()
    }

    fn near_a_path(paths: &[Vec<P2>], p: P2, limit: f64) -> bool {
        paths.iter().any(|path| {
            (0..path.len()).any(|i| {
                let (a, b) = (path[i], path[(i + 1) % path.len()]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l2 = dx * dx + dy * dy;
                let t = match l2 > 0.0 {
                    true => (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0),
                    false => 0.0,
                };
                (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy) < limit
            })
        })
    }

    /// Makes sure that the fill covers each place of the near side that is in
    /// the polygon, and no other place. Returns the area of the fill.
    fn check_fill(rings: &[Vec<V3>], rot: &Mat3) -> f64 {
        let clipped = clip_polygon(rot, rings, STEP);
        for path in &clipped.loops {
            for p in path {
                assert!(p[0].hypot(p[1]) < 1.0 + 1e-9, "a point off the globe");
            }
        }
        let mut probes = 0;
        for i in 0..61 {
            for j in 0..61 {
                let p = [f64::from(i) / 30.0 - 1.0, f64::from(j) / 30.0 - 1.0];
                let r2 = p[0] * p[0] + p[1] * p[1];
                if r2 > 0.96 || near_a_path(&clipped.loops, p, 0.02) {
                    continue;
                }
                let drawn = clipped.loops.iter().filter(|l| inside(l, p)).count() % 2 == 1;
                let world = rot.inv_mul_vec([p[0], p[1], (1.0 - r2).sqrt()]);
                assert_eq!(drawn, contains(rings, world), "at {p:?}");
                probes += 1;
            }
        }
        assert!(probes > 1500);

        let (points, indexes) = triangulate(&clipped.loops);
        let triangles: f64 = indexes
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| points[t[k] as usize]);
                area2(&[a, b, c]).abs() / 2.0
            })
            .sum();
        let paths: f64 = clipped.loops.iter().map(|l| area2(l) / 2.0).sum();
        // A ring meets the edge of the globe at a tangent, so the chords of
        // the two can cross there by a small part of a step.
        assert!((triangles - paths).abs() < 1e-4, "{triangles} and {paths}");
        paths
    }

    #[test]
    fn a_drawn_arc_is_on_the_great_circle() {
        let (a, b) = (lonlat_to_dir(-50.0, 10.0), lonlat_to_dir(40.0, 60.0));
        let view = rot(0.0, 30.0);
        let radius_px = 400.0;
        let step = step(radius_px);
        let parts = clip_line(&view, &[a, b], step);
        assert_eq!(parts.len(), 1);
        let normal = normalize(cross(view.mul_vec(a), view.mul_vec(b)));
        let on_sphere: Vec<V3> = parts[0]
            .iter()
            .map(|p| [p[0], p[1], (1.0 - p[0] * p[0] - p[1] * p[1]).sqrt()])
            .collect();
        assert!(on_sphere.len() > 10);
        for pair in on_sphere.windows(2) {
            assert!(dot(pair[0], normal).abs() < 1e-9);
            let chord = angle(pair[0], pair[1]);
            assert!(chord <= step + 1e-9);
            // The greatest distance from the chord to the arc, in pixels.
            assert!(radius_px * (1.0 - (chord / 2.0).cos()) <= 0.25);
        }
        assert!(angle(on_sphere[0], view.mul_vec(a)) < 1e-9);
        assert!(angle(*on_sphere.last().unwrap(), view.mul_vec(b)) < 1e-9);
        // The straight line in longitude and latitude is a different curve.
        let straight = view.mul_vec(lonlat_to_dir(-5.0, 35.0));
        assert!(dot(straight, normal).abs() > 0.05);
    }

    #[test]
    fn a_line_stops_on_the_edge_of_the_globe() {
        let view = rot(0.0, 0.0);
        let pts = line(&[[0.0, 0.0], [60.0, 20.0], [150.0, 30.0], [200.0, 0.0]]);
        let parts = clip_line(&view, &pts, STEP);
        assert_eq!(parts.len(), 1);
        let end = *parts[0].last().unwrap();
        assert!((end[0].hypot(end[1]) - 1.0).abs() < 1e-12);
        let normal = normalize(cross(view.mul_vec(pts[1]), view.mul_vec(pts[2])));
        assert!(dot([end[0], end[1], 0.0], normal).abs() < 1e-12);
        for p in &parts[0] {
            assert!(p[0].hypot(p[1]) <= 1.0 + 1e-12);
        }

        // A line that goes behind the globe and comes back has two parts.
        let pts = line(&[[-60.0, 0.0], [-120.0, 0.0], [120.0, 0.0], [60.0, 0.0]]);
        let parts = clip_line(&view, &pts, STEP);
        assert_eq!(parts.len(), 2);
        let (out, back) = (parts[0].last().unwrap(), parts[1][0]);
        assert!((out[0] + 1.0).abs() < 1e-12 && (back[0] - 1.0).abs() < 1e-12);

        assert!(clip_line(&rot(180.0, 0.0), &line(&[[-10.0, 0.0], [10.0, 5.0]]), STEP).is_empty());
    }

    #[test]
    fn a_ring_has_the_inside_on_its_left() {
        let ccw = ring(&square(10.0, 20.0, 5.0), false);
        let mut reversed = square(10.0, 20.0, 5.0);
        reversed.reverse();
        assert_eq!(ccw.len(), 4);
        assert!(turning(&ccw) > 0.0);
        assert!(turning(&ring(&reversed, false)) > 0.0);
        assert!(turning(&ring(&square(10.0, 20.0, 5.0), true)) < 0.0);
        assert!(contains(
            std::slice::from_ref(&ccw),
            lonlat_to_dir(10.0, 20.0)
        ));
        assert!(!contains(
            std::slice::from_ref(&ccw),
            lonlat_to_dir(10.0, 40.0)
        ));
        assert!(!contains(
            std::slice::from_ref(&ccw),
            lonlat_to_dir(-170.0, -20.0)
        ));
        assert!(!contains(&plate(), lonlat_to_dir(0.0, 0.0)));
        assert!(contains(&plate(), lonlat_to_dir(20.0, 0.0)));

        // A ring around a pole, as a flat map stores it: the ring goes down
        // the last meridian to the pole and along the pole.
        let mut cap: Vec<[f64; 2]> = (0..=36)
            .map(|k| [-180.0 + 10.0 * f64::from(k), -70.0])
            .collect();
        cap.extend([[180.0, -90.0], [-180.0, -90.0], [-180.0, -70.0]]);
        let cap = ring(&cap, false);
        assert_eq!(cap.len(), 36);
        assert!(contains(
            std::slice::from_ref(&cap),
            lonlat_to_dir(0.0, -90.0)
        ));
        assert!(!contains(
            std::slice::from_ref(&cap),
            lonlat_to_dir(0.0, 0.0)
        ));

        assert!(ring(&[[0.0, 0.0], [10.0, 0.0], [0.0, 0.0]], false).is_empty());
    }

    #[test]
    fn the_fill_of_a_polygon_ends_at_the_edge_of_the_globe() {
        // All of the polygon is in view.
        let whole = check_fill(&plate(), &rot(0.0, 0.0));
        assert!(whole > 0.5);
        // The outer ring crosses the edge.
        let part = check_fill(&plate(), &rot(75.0, 0.0));
        assert!(part > 0.0 && part < whole);
        // The outer ring and the hole cross the edge.
        let clipped = clip_polygon(&rot(95.0, 5.0), &plate(), STEP);
        assert_eq!((clipped.outlines.len(), clipped.loops.len()), (2, 1));
        check_fill(&plate(), &rot(95.0, 5.0));
        check_fill(&plate(), &rot(-90.0, 60.0));
        // All of the polygon is on the far side.
        let hidden = clip_polygon(&rot(180.0, 0.0), &plate(), STEP);
        assert_eq!(hidden, Clipped::default());
    }

    #[test]
    fn a_polygon_can_hold_the_whole_edge_of_the_globe() {
        // The polygon is all of the sphere but a cap.
        let mut hole = circle(180.0, 0.0, 30.0);
        hole.reverse();
        let rings = vec![hole];
        // The cap is on the far side, so the fill is the disc.
        let clipped = clip_polygon(&rot(0.0, 0.0), &rings, STEP);
        assert!(clipped.outlines.is_empty());
        assert!((check_fill(&rings, &rot(0.0, 0.0)) - PI).abs() < 0.01);
        // The cap is in the middle of the near side.
        let ring_area = PI * (1.0 - 30f64.to_radians().sin().powi(2));
        assert!((check_fill(&rings, &rot(180.0, 0.0)) - ring_area).abs() < 0.01);
        // The cap crosses the edge.
        let bitten = check_fill(&rings, &rot(100.0, 20.0));
        assert!(bitten > ring_area && bitten < PI);
    }

    #[test]
    fn the_fill_is_correct_from_many_directions() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut big = circle(40.0, -30.0, 50.0);
        big.reverse();
        let shapes = [plate(), vec![circle(40.0, -30.0, 50.0)], vec![big]];
        for _ in 0..40 {
            let view = rot(random() * 360.0 - 180.0, random() * 170.0 - 85.0);
            let twist = Mat3::axis_angle([0.0, 0.0, 1.0], random() * TAU);
            for rings in &shapes {
                check_fill(rings, &twist.mul(&view));
            }
        }
    }
}
