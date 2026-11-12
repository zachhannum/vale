//! Split. A pen stroke cuts lines and polygons into parts, and each part is
//! a new feature.
//!
//! A polygon becomes one part for each area between its outline and the
//! stroke. A line becomes one part between each two places where the stroke
//! crosses it. A part of a feature that the stroke does not cut goes to the
//! part that holds it, or else to the largest part.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use vale_store::{Geometry, LonLat};

use super::combine::{Bounds, Feature, boxed, dir};
use super::line::{chains, to_parts_exact};
use super::math::{V3, angle, cross, dot, normalize};
use super::polygon::{
    MIN_AREA, MIN_LENGTH, SAME, bounds, crossing, cut, densify_open, dirs, path, path_length,
    signed_area, to_polygons_many,
};
use super::reshape::{REACH, ends, outlines};
use super::snap::{PathVertex, Run, append};

/// The path of a split stroke.
pub struct SplitStroke {
    /// The open path, with each end longer by `REACH`.
    path: Vec<PathVertex>,
    /// A box around the path.
    bounds: Bounds,
    min_area: f64,
    min_length: f64,
}

impl SplitStroke {
    /// The path of a stroke. The runs come from a `Tracer`. A stroke that is
    /// too short gives `None`.
    pub fn new(runs: &[Run], scale: f64, strength: f64) -> Option<Self> {
        ends(runs)?;
        let mut path = path(runs, scale, strength);
        if path_length(&path) < MIN_LENGTH / scale {
            return None;
        }
        // An end within reach of an outline crosses it.
        let reach = REACH / scale;
        let first = extend(path.iter().map(PathVertex::dir), reach);
        let last = extend(path.iter().rev().map(PathVertex::dir), reach);
        path.insert(0, PathVertex::Free(first));
        path.push(PathVertex::Free(last));
        let path = densify_open(&path);
        let bounds = boxed(path.iter().map(PathVertex::dir), 0.0);
        Some(SplitStroke {
            path,
            bounds,
            min_area: MIN_AREA / (scale * scale),
            min_length: MIN_LENGTH / scale,
        })
    }

    /// The stroke can cross the geometry. The test is quick, and it can be
    /// true for a geometry that the stroke does not cross.
    pub fn near(&self, geometry: &Geometry) -> bool {
        let rings: Vec<&Vec<LonLat>> = match geometry {
            Geometry::Polygons(p) => p.iter().flatten().collect(),
            Geometry::Lines(l) => l.iter().collect(),
            Geometry::Points(_) => return false,
        };
        let mut edges = rings.into_iter().flat_map(|r| r.windows(2));
        edges.any(|w| {
            let (lo, hi) = bounds(dir(w[0]), dir(w[1]));
            (0..3).all(|k| lo[k] <= self.bounds.1[k] && self.bounds.0[k] <= hi[k])
        })
    }

    /// The parts of the geometry, two or more. A stroke that does not cut
    /// the geometry gives `None`.
    pub fn split(&self, geometry: &Geometry) -> Option<Vec<Geometry>> {
        match geometry {
            Geometry::Polygons(p) => self.split_polygons(p),
            Geometry::Lines(l) => self.split_lines(l),
            Geometry::Points(_) => None,
        }
    }

    fn split_polygons(&self, polygons: &[Vec<Vec<LonLat>>]) -> Option<Vec<Geometry>> {
        let feature = Feature::new(polygons);
        let paths: Vec<Vec<PathVertex>> = outlines(polygons)?.into_iter().map(|o| o.ring).collect();
        let pieces = cut(&paths, &self.path, |p| feature.holds(p), self.min_area)?;
        let out: Vec<Geometry> = pieces
            .into_iter()
            .map(|rings| {
                let rings: Vec<Vec<PathVertex>> = (rings.into_iter())
                    .filter(|r| signed_area(&dirs(r)).abs() > self.min_area)
                    .collect();
                to_polygons_many(&rings)
            })
            .filter(|p| !p.is_empty())
            .map(Geometry::Polygons)
            .collect();
        (out.len() >= 2).then_some(out)
    }

    fn split_lines(&self, parts: &[Vec<LonLat>]) -> Option<Vec<Geometry>> {
        let mut pieces: Vec<Vec<Vec<PathVertex>>> = Vec::new();
        let mut rest = Vec::new();
        for chain in chains(parts) {
            let mut line = Vec::new();
            for i in chain {
                append(&mut line, parts[i].iter().map(|&p| PathVertex::Exact(p)));
            }
            let cuts = self.cuts(&line);
            if cuts.is_empty() {
                rest.push(line);
                continue;
            }
            let mut piece = Vec::new();
            let mut at = 0;
            for (edge, x) in cuts {
                append(&mut piece, line[at..=edge].iter().copied());
                append(&mut piece, [PathVertex::Free(x)]);
                pieces.push(vec![std::mem::replace(
                    &mut piece,
                    vec![PathVertex::Free(x)],
                )]);
                at = edge + 1;
            }
            append(&mut piece, line[at..].iter().copied());
            pieces.push(vec![piece]);
        }
        if pieces.len() < 2 {
            return None;
        }
        let length = |p: &[Vec<PathVertex>]| p.iter().map(|l| path_length(l)).sum::<f64>();
        let longest = (0..pieces.len())
            .max_by(|&a, &b| length(&pieces[a]).total_cmp(&length(&pieces[b])))
            .expect("a piece");
        pieces[longest].extend(rest);
        let out = pieces
            .iter()
            .map(|p| Geometry::Lines(p.iter().flat_map(|l| to_parts_exact(l)).collect()))
            .collect();
        Some(out)
    }

    /// The places where the stroke crosses a line, in the order of the
    /// line, as the edge of the line and the place. A place where the
    /// stroke touches the line and turns back is no crossing. A part of the
    /// line shorter than `MIN_LENGTH` points gets no cut at its ends.
    fn cuts(&self, line: &[PathVertex]) -> Vec<(usize, V3)> {
        let stroke = dirs(&self.path);
        let mut hits: Vec<(usize, f64, V3)> = Vec::new();
        for (i, w) in line.windows(2).enumerate() {
            let (a, b) = (w[0].dir(), w[1].dir());
            if angle(a, b) < SAME {
                continue;
            }
            let (lo, hi) = bounds(a, b);
            if !(0..3).all(|k| lo[k] <= self.bounds.1[k] && self.bounds.0[k] <= hi[k]) {
                continue;
            }
            let nl = cross(a, b);
            for s in stroke.windows(2) {
                let (u, v) = (s[0], s[1]);
                let ns = cross(u, v);
                let side = |p: V3, n: V3| dot(n, p) >= 0.0;
                if side(a, ns) == side(b, ns) || side(u, nl) == side(v, nl) {
                    continue;
                }
                // The two arcs can meet on the far side of the sphere.
                let x = crossing(a, b, u, v).or_else(|| {
                    [a, b, u, v].into_iter().find(|&p| {
                        dot(normalize(nl), p).abs() < SAME && dot(normalize(ns), p).abs() < SAME
                    })
                });
                if let Some(x) = x {
                    hits.push((i, angle(a, x), x));
                }
            }
        }
        hits.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
        // Hits at one place come from a vertex. An even number of them is a
        // touch.
        let mut places: Vec<(usize, V3, usize)> = Vec::new();
        for (i, _, x) in hits {
            match places.last_mut() {
                Some(last) if angle(last.1, x) < 1e-9 => last.2 += 1,
                _ => places.push((i, x, 1)),
            }
        }
        let along = |edge: usize, x: V3| -> f64 {
            path_length(&line[..=edge]) + angle(line[edge].dir(), x)
        };
        let total = path_length(line);
        let mut out: Vec<(usize, V3)> = Vec::new();
        let mut last = 0.0;
        for (i, x, n) in places {
            let at = along(i, x);
            if n % 2 == 1 && at - last >= self.min_length && total - at >= self.min_length {
                out.push((i, x));
                last = at;
            }
        }
        out
    }
}

/// The point `reach` past the first point of a path, on the way from the
/// next point that is not at the same place.
fn extend(mut path: impl Iterator<Item = V3>, reach: f64) -> V3 {
    let first = path.next().expect("a point");
    let Some(next) = path.find(|&p| angle(p, first) > SAME) else {
        return first;
    };
    let t = reach / angle(first, next);
    normalize([0, 1, 2].map(|k| first[k] + t * (first[k] - next[k])))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vale_store::GeometryKind;

    use super::*;
    use crate::globe::polygon::tests::{assert_valid, inside};
    use crate::globe::snap::SnapSettings;
    use crate::globe::snap::tests::{self as snap_test, kinds, pen, trace};

    const SCALE: f64 = 300.0;

    fn stroke(way: &[LonLat]) -> SplitStroke {
        SplitStroke::new(&[Run::Free(pen(way, 0.0))], SCALE, 0.0).expect("a stroke")
    }

    fn polygons(g: &Geometry) -> Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not a polygon");
        };
        p.clone()
    }

    fn lines(g: &Geometry) -> Vec<Vec<LonLat>> {
        let Geometry::Lines(l) = g else {
            panic!("not a line");
        };
        l.clone()
    }

    /// The polygon that the polygon tool makes of a closed way. A longitude
    /// can be past 180 degrees.
    fn stored(ring: &[LonLat]) -> Geometry {
        crate::globe::polygon::commit(&[Run::Free(pen(ring, 0.0))], SCALE, 0.0).expect("a polygon")
    }

    /// The line that the line tool makes of a way.
    fn stored_line(way: &[LonLat]) -> Geometry {
        let path: Vec<V3> = way.iter().map(|&p| dir(p)).collect();
        Geometry::Lines(crate::globe::line::to_parts(&path))
    }

    fn box_way(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<LonLat> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
    }

    fn square() -> Geometry {
        Geometry::Polygons(vec![vec![box_way(0.0, 0.0, 10.0, 10.0)]])
    }

    /// The area of polygons on the sphere.
    fn area(p: &[Vec<Vec<LonLat>>]) -> f64 {
        let sphere = 4.0 * std::f64::consts::PI;
        p.iter()
            .map(|poly| {
                let mut rings = poly.iter().enumerate().map(|(i, r)| {
                    signed_area(&crate::globe::vector::ring(r, i > 0)).rem_euclid(sphere)
                });
                let outer = rings.next().unwrap_or(0.0);
                outer - rings.map(|h| sphere - h).sum::<f64>()
            })
            .sum()
    }

    /// The vertices of the outlines of polygons, on the sphere.
    fn vertices(p: &[Vec<Vec<LonLat>>]) -> Vec<V3> {
        p.iter().flatten().flatten().map(|&q| dir(q)).collect()
    }

    fn shared(a: &[V3], b: &[V3]) -> usize {
        a.iter()
            .filter(|&&p| b.iter().any(|&q| angle(p, q) < 1e-12))
            .count()
    }

    /// Two parts of a polygon: no point is in both, each point of the
    /// polygon is in one, and they share the vertices of the cut.
    fn assert_parts(whole: &Geometry, parts: &[Geometry], probes: &[LonLat]) {
        let w = polygons(whole);
        let p: Vec<_> = parts.iter().map(polygons).collect();
        for q in &p {
            assert_valid(q);
        }
        for &x in probes {
            let n = p.iter().filter(|q| inside(q, x)).count();
            assert_eq!(n, usize::from(inside(&w, x)), "{x:?}");
        }
        let sum: f64 = p.iter().map(|q| area(q)).sum();
        assert!(
            (sum - area(&w)).abs() < 1e-9 * area(&w).max(1e-6),
            "{sum} {}",
            area(&w)
        );
        assert!(shared(&vertices(&p[0]), &vertices(&p[1])) >= 2);
    }

    fn grid(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<LonLat> {
        let mut out = Vec::new();
        for i in 0..=20 {
            for j in 0..=20 {
                let x = x0 + (x1 - x0) * (i as f64 + 0.37) / 21.0;
                let y = y0 + (y1 - y0) * (j as f64 + 0.41) / 21.0;
                out.push([x, y]);
            }
        }
        out
    }

    #[test]
    fn a_stroke_across_a_polygon_gives_two_polygons_that_share_the_cut() {
        let s = stroke(&[[5.0, -3.0], [6.0, 4.0], [4.0, 7.0], [5.0, 13.0]]);
        assert!(s.near(&square()));
        let parts = s.split(&square()).expect("a split");
        assert_eq!(parts.len(), 2);
        assert_parts(&square(), &parts, &grid(-1.0, -1.0, 11.0, 11.0));
        let (a, b) = (polygons(&parts[0]), polygons(&parts[1]));
        assert!(inside(&a, [1.0, 5.0]) != inside(&b, [1.0, 5.0]));
        assert!(inside(&a, [1.0, 5.0]) != inside(&a, [9.0, 5.0]));
    }

    #[test]
    fn a_wavy_stroke_gives_a_part_for_each_area() {
        // In at the south, out at the east, and in again at the north.
        let s = stroke(&[
            [3.0, -2.0],
            [3.0, 5.0],
            [12.0, 5.0],
            [12.0, 7.0],
            [7.0, 7.0],
            [7.0, 12.0],
        ]);
        let parts = s.split(&square()).expect("a split");
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn a_stroke_that_does_not_cross_changes_nothing() {
        // It ends inside.
        assert!(
            stroke(&[[5.0, -3.0], [5.0, 5.0]])
                .split(&square())
                .is_none()
        );
        // It starts and ends inside.
        assert!(stroke(&[[3.0, 3.0], [7.0, 7.0]]).split(&square()).is_none());
        // It goes in and back out on the same side, and cuts off a corner
        // only when it goes out again.
        assert!(
            stroke(&[[5.0, -3.0], [5.0, 5.0], [5.5, -3.0]])
                .split(&square())
                .is_some()
        );
        // It touches the outline and turns back.
        assert!(
            stroke(&[[5.0, -3.0], [5.0, 0.0], [6.0, -3.0]])
                .split(&square())
                .is_none()
        );
        // It is away from the polygon.
        let s = stroke(&[[20.0, -3.0], [20.0, 13.0]]);
        assert!(!s.near(&square()));
        assert!(s.split(&square()).is_none());
    }

    #[test]
    fn a_stroke_that_ends_just_inside_or_outside_still_splits() {
        let reach = REACH / SCALE * 0.5;
        let d = reach.to_degrees();
        let s = stroke(&[[5.0, d], [5.0, 10.0 - d]]);
        assert_eq!(s.split(&square()).map(|p| p.len()), Some(2));
        let s = stroke(&[[5.0, -d], [5.0, 10.0 + d]]);
        assert_eq!(s.split(&square()).map(|p| p.len()), Some(2));
    }

    #[test]
    fn islands_and_holes_go_to_the_part_that_holds_them() {
        let island = box_way(30.0, 0.0, 35.0, 5.0);
        let hole: Vec<LonLat> = box_way(1.0, 1.0, 3.0, 3.0).into_iter().rev().collect();
        let g = Geometry::Polygons(vec![
            vec![box_way(0.0, 0.0, 10.0, 10.0), hole],
            vec![island],
        ]);
        // The west part is the smaller one.
        let s = stroke(&[[4.0, -3.0], [4.0, 13.0]]);
        let parts = s.split(&g).expect("a split");
        assert_eq!(parts.len(), 2);
        let (a, b) = (polygons(&parts[0]), polygons(&parts[1]));
        let (west, east) = if inside(&a, [0.5, 5.0]) {
            (a, b)
        } else {
            (b, a)
        };
        assert!(!inside(&west, [2.0, 2.0]));
        assert!(inside(&west, [0.5, 2.0]));
        assert!(inside(&east, [32.0, 2.0]));
        assert!(!inside(&west, [32.0, 2.0]));
        assert_valid(&west);
        assert_valid(&east);
    }

    #[test]
    fn a_stroke_from_the_outline_into_a_hole_changes_nothing() {
        let hole: Vec<LonLat> = box_way(4.0, 4.0, 6.0, 6.0).into_iter().rev().collect();
        let g = Geometry::Polygons(vec![vec![box_way(0.0, 0.0, 10.0, 10.0), hole]]);
        assert!(stroke(&[[5.0, -3.0], [5.0, 5.0]]).split(&g).is_none());
        // Through the hole and out cuts the ring in two.
        let parts = stroke(&[[5.0, -3.0], [5.0, 13.0]])
            .split(&g)
            .expect("a split");
        assert_eq!(parts.len(), 2);
        assert_parts(&g, &parts, &grid(-1.0, -1.0, 11.0, 11.0));
    }

    #[test]
    fn a_polygon_split_across_the_180_degree_meridian_is_correct() {
        let g = stored(&box_way(170.0, -10.0, 190.0, 10.0));
        // Along the meridian, a little to the east.
        let s = stroke(&[[181.0, -15.0], [181.0, 15.0]]);
        let parts = s.split(&g).expect("a split");
        assert_eq!(parts.len(), 2);
        let mut probes = grid(165.0, -12.0, 180.0, 12.0);
        probes.extend(grid(-180.0, -12.0, -165.0, 12.0));
        assert_parts(&g, &parts, &probes);
        // Across the meridian.
        let s = stroke(&[[165.0, 1.0], [195.0, -1.0]]);
        let parts = s.split(&g).expect("a split");
        assert_eq!(parts.len(), 2);
        assert_parts(&g, &parts, &probes);
        let (a, b) = (polygons(&parts[0]), polygons(&parts[1]));
        let north = if inside(&a, [179.0, 5.0]) { &a } else { &b };
        assert!(inside(north, [-179.0, 5.0]));
        assert!(!inside(north, [-179.0, -5.0]));
    }

    #[test]
    fn a_polygon_split_through_a_pole_is_correct() {
        let cap = stored(&[[0.0, 75.0], [120.0, 75.0], [240.0, 75.0], [360.0, 75.0]]);
        let s = stroke(&[[0.0, 70.0], [0.0, 89.0], [180.0, 89.0], [180.0, 70.0]]);
        let parts = s.split(&cap).expect("a split");
        assert_eq!(parts.len(), 2);
        let probes: Vec<LonLat> = (0..36)
            .flat_map(|i| [80.0, 86.0].map(|lat| [i as f64 * 10.0 - 175.0, lat]))
            .collect();
        assert_parts(&cap, &parts, &probes);
        let (a, b) = (polygons(&parts[0]), polygons(&parts[1]));
        assert!(inside(&a, [90.0, 80.0]) != inside(&a, [-90.0, 80.0]));
        assert!(inside(&b, [90.0, 80.0]) != inside(&b, [-90.0, 80.0]));
    }

    #[test]
    fn a_traced_stroke_splits_along_a_river() {
        let river = vec![vec![
            [5.0, -5.0],
            [5.1234567, 2.0],
            [4.7654321, 5.0],
            [5.3333333, 8.0],
            [5.0, 15.0],
        ]];
        let a = snap_test::layer(1, GeometryKind::Line, vec![Geometry::Lines(river)]);
        let t = Arc::new(snap_test::targets(&[a]));
        let way = [
            [5.02, -2.0],
            [5.12, 2.0],
            [4.77, 5.0],
            [5.33, 8.0],
            [5.02, 12.0],
        ];
        let runs = trace(&t, &pen(&way, 0.0), SnapSettings::every(&t), 9);
        assert!(kinds(&runs).contains('T'), "{}", kinds(&runs));
        let s = SplitStroke::new(&runs, SCALE, 0.0).expect("a stroke");
        let parts = s.split(&square()).expect("a split");
        assert_eq!(parts.len(), 2);
        for p in &parts {
            let q = polygons(p);
            assert!(q[0][0].contains(&[4.7654321, 5.0]), "{q:?}");
        }
    }

    #[test]
    fn a_stroke_across_a_line_gives_two_lines_at_the_crossing() {
        let road = stored_line(&[[0.0, 0.0], [10.0, 0.0]]);
        let s = stroke(&[[4.0, -3.0], [4.0, 3.0]]);
        assert!(s.near(&road));
        let parts = s.split(&road).expect("a split");
        assert_eq!(parts.len(), 2);
        let (a, b) = (lines(&parts[0]), lines(&parts[1]));
        let end = *a[a.len() - 1].last().unwrap();
        assert!(angle(dir(end), dir([4.0, 0.0])) < 1e-9, "{end:?}");
        assert_eq!(b[0][0], end);
        assert_eq!(a[0][0], [0.0, 0.0]);
        assert_eq!(*b[b.len() - 1].last().unwrap(), [10.0, 0.0]);
    }

    #[test]
    fn k_crossings_give_k_plus_one_lines() {
        let road = stored_line(&[[0.0, 0.0], [10.0, 0.0]]);
        let s = stroke(&[
            [2.0, -3.0],
            [2.0, 3.0],
            [5.0, 3.0],
            [5.0, -3.0],
            [8.0, -3.0],
            [8.0, 3.0],
        ]);
        assert_eq!(s.split(&road).map(|p| p.len()), Some(4));
    }

    #[test]
    fn a_stroke_that_does_not_cross_a_line_changes_nothing() {
        let road = stored_line(&[[0.0, 0.0], [10.0, 0.0]]);
        // It ends before the line, out of reach.
        assert!(stroke(&[[4.0, -6.0], [4.0, -3.0]]).split(&road).is_none());
        // It touches the line and turns back.
        assert!(
            stroke(&[[4.0, -3.0], [4.0, 0.0], [5.0, -3.0]])
                .split(&road)
                .is_none()
        );
        // It crosses past the end of the line.
        assert!(stroke(&[[12.0, -3.0], [12.0, 3.0]]).split(&road).is_none());
    }

    #[test]
    fn a_line_split_across_the_180_degree_meridian_is_correct() {
        let road = stored_line(&[[170.0, 0.0], [190.0, 2.0]]);
        assert!(lines(&road).len() >= 2);
        // East of the meridian: the west part crosses it.
        let s = stroke(&[[185.0, -3.0], [185.0, 5.0]]);
        let parts = s.split(&road).expect("a split");
        assert_eq!(parts.len(), 2);
        let west = lines(&parts[0]);
        assert_eq!(west.len(), 2, "{west:?}");
        assert_eq!(chains(&west).len(), 1);
        let east = lines(&parts[1]);
        assert!(
            angle(dir(east[0][0]), dir([-175.0, 1.5])) < 1e-3,
            "{east:?}"
        );
        // At the meridian.
        let s = stroke(&[[180.0, -3.0], [180.0, 5.0]]);
        assert_eq!(s.split(&road).map(|p| p.len()), Some(2));
    }

    #[test]
    fn a_line_split_near_a_pole_is_correct() {
        let road = stored_line(&[[0.0, 80.0], [0.0, 90.0], [180.0, 80.0]]);
        let s = stroke(&[[-90.0, 85.0], [90.0, 85.0]]);
        let parts = s.split(&road).expect("a split");
        assert_eq!(parts.len(), 2);
        let (a, b) = (lines(&parts[0]), lines(&parts[1]));
        let near = |p: LonLat, q: LonLat| angle(dir(p), dir(q)) < 1e-9;
        assert!(near(a[0][0], [0.0, 80.0]));
        assert!(near(*b[b.len() - 1].last().unwrap(), [180.0, 80.0]));
        let end = dir(*a[a.len() - 1].last().unwrap());
        assert!(angle(end, [0.0, 0.0, 1.0]) < 1e-3 || dot(end, [0.0, 0.0, 1.0]) > 0.99);
    }

    #[test]
    fn untouched_parts_of_a_line_go_to_the_longest_part() {
        let mut parts = lines(&stored_line(&[[0.0, 0.0], [10.0, 0.0]]));
        parts.extend(lines(&stored_line(&[[0.0, 20.0], [5.0, 20.0]])));
        let road = Geometry::Lines(parts);
        let s = stroke(&[[3.0, -3.0], [3.0, 3.0]]);
        let out = s.split(&road).expect("a split");
        assert_eq!(out.len(), 2);
        let (a, b) = (lines(&out[0]), lines(&out[1]));
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 2);
    }
}
