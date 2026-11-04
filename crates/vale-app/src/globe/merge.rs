//! Merge. The geometries of several features of one layer become the
//! geometry of one feature.
//!
//! Polygons join into one area, and a border between two polygons goes
//! away. Polygons that do not touch become parts of one feature. Lines that
//! share an end join into one line, and the other lines become parts of
//! the feature. Points become one feature with each point.

use vale_store::{Geometry, LonLat};

use super::combine::{Feature, dir};
use super::line::{chains, to_parts_exact};
use super::math::{V3, angle};
use super::polygon::{dirs, overlay, signed_area, to_polygons_many};
use super::reshape::outlines;
use super::snap::{PathVertex, append};
use super::vector;

/// The smallest area of a ring of the result, in steradians. A smaller
/// ring is a sliver where two outlines almost meet.
const MIN_AREA: f64 = 1e-14;
/// The largest angle between two ends of lines that join, in radians.
const SAME: f64 = 1e-9;

/// The geometry of one feature that holds each of the geometries. The
/// geometries must have one kind. No geometry, geometries of more than one
/// kind, or an outline that does not join give `None`.
pub fn merge(geometries: &[&Geometry]) -> Option<Geometry> {
    let first = geometries.first()?;
    if geometries.iter().any(|g| g.kind() != first.kind()) {
        return None;
    }
    match first {
        Geometry::Points(_) => {
            let points = geometries.iter().flat_map(|g| match g {
                Geometry::Points(p) => p.clone(),
                _ => Vec::new(),
            });
            Some(Geometry::Points(points.collect()))
        }
        Geometry::Lines(_) => {
            let lines: Vec<&[Vec<LonLat>]> = geometries
                .iter()
                .filter_map(|g| match g {
                    Geometry::Lines(l) => Some(l.as_slice()),
                    _ => None,
                })
                .collect();
            Some(Geometry::Lines(merge_lines(&lines)))
        }
        Geometry::Polygons(_) => {
            let polygons: Vec<&[Vec<Vec<LonLat>>]> = geometries
                .iter()
                .filter_map(|g| match g {
                    Geometry::Polygons(p) => Some(p.as_slice()),
                    _ => None,
                })
                .collect();
            merge_polygons(&polygons).map(Geometry::Polygons)
        }
    }
}

/// The union of the areas of polygons. The outlines divide the sphere into
/// areas, and an area in one of the polygons is in the result. An edge
/// that two outlines share is one edge, so a shared border goes away.
fn merge_polygons(features: &[&[Vec<Vec<LonLat>>]]) -> Option<Vec<Vec<Vec<LonLat>>>> {
    let mut paths: Vec<Vec<PathVertex>> = Vec::new();
    for polygons in features {
        paths.extend(outlines(polygons)?.into_iter().map(|o| o.ring));
    }
    let areas: Vec<Feature> = features.iter().map(|p| Feature::new(p)).collect();
    let rings: Vec<Vec<PathVertex>> = overlay(&paths, |p| areas.iter().any(|a| a.holds(p)))
        .into_iter()
        .filter(|r| signed_area(&dirs(r)).abs() > MIN_AREA)
        .collect();
    let out = to_polygons_many(&rings);
    (!out.is_empty()).then_some(out)
}

/// The parts of lines, with each two runs that share an end joined into
/// one run. A run turns around to join where it must.
fn merge_lines(features: &[&[Vec<LonLat>]]) -> Vec<Vec<LonLat>> {
    let mut runs: Vec<Vec<PathVertex>> = Vec::new();
    for parts in features {
        for chain in chains(parts) {
            let mut run = Vec::new();
            for i in chain {
                append(&mut run, parts[i].iter().map(|&p| PathVertex::Exact(p)));
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
    }
    while let Some((a, b, flip_a, flip_b)) = touching(&runs) {
        let mut second = runs.remove(b);
        let first = &mut runs[a];
        if flip_a {
            first.reverse();
        }
        if flip_b {
            second.reverse();
        }
        append(first, second);
    }
    runs.iter().flat_map(|r| to_parts_exact(r)).collect()
}

/// Two runs that share an end, with `a < b`, and whether each must turn
/// around so that the end of `a` meets the start of `b`.
fn touching(runs: &[Vec<PathVertex>]) -> Option<(usize, usize, bool, bool)> {
    let ends = |r: &[PathVertex]| -> Option<(V3, V3)> { Some((r.first()?.dir(), r.last()?.dir())) };
    let meet = |p: V3, q: V3| angle(p, q) < SAME;
    for (a, run_a) in runs.iter().enumerate() {
        let Some((a0, a1)) = ends(run_a) else {
            continue;
        };
        for (b, run_b) in runs.iter().enumerate().skip(a + 1) {
            let Some((b0, b1)) = ends(run_b) else {
                continue;
            };
            let found = if meet(a1, b0) {
                Some((false, false))
            } else if meet(a1, b1) {
                Some((false, true))
            } else if meet(a0, b1) {
                Some((true, true))
            } else if meet(a0, b0) {
                Some((true, false))
            } else {
                None
            };
            if let Some((fa, fb)) = found {
                return Some((a, b, fa, fb));
            }
        }
    }
    None
}

/// The size of a geometry: the area of polygons in steradians, the length
/// of lines in radians, or the number of points.
pub fn size(geometry: &Geometry) -> f64 {
    match geometry {
        Geometry::Points(p) => p.len() as f64,
        Geometry::Lines(parts) => parts
            .iter()
            .map(|l| {
                let d: Vec<V3> = l.iter().map(|&p| dir(p)).collect();
                d.windows(2).map(|w| angle(w[0], w[1])).sum::<f64>()
            })
            .sum(),
        Geometry::Polygons(polygons) => {
            let sphere = 4.0 * std::f64::consts::PI;
            polygons
                .iter()
                .map(|poly| {
                    let mut rings = poly
                        .iter()
                        .enumerate()
                        .map(|(i, r)| signed_area(&vector::ring(r, i > 0)).rem_euclid(sphere));
                    let outer = rings.next().unwrap_or(0.0);
                    outer - rings.map(|h| sphere - h).sum::<f64>()
                })
                .sum()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::line::to_parts;
    use crate::globe::polygon::tests::{assert_valid, inside};
    use crate::globe::snap::Run;
    use crate::globe::snap::tests::pen;
    use crate::globe::split::SplitStroke;

    const SCALE: f64 = 300.0;

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

    fn box_way(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<LonLat> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
        Geometry::Polygons(vec![vec![box_way(x0, y0, x1, y1)]])
    }

    /// The polygon that the polygon tool makes of a closed way. A longitude
    /// can be past 180 degrees.
    fn stored(ring: &[LonLat]) -> Geometry {
        crate::globe::polygon::commit(&[Run::Free(pen(ring, 0.0))], SCALE, 0.0).expect("a polygon")
    }

    /// The parts of a geometry that a split stroke along a way gives.
    fn split(g: &Geometry, way: &[LonLat]) -> Vec<Geometry> {
        let stroke = SplitStroke::new(&[Run::Free(pen(way, 0.0))], SCALE, 0.0).expect("a stroke");
        stroke.split(g).expect("a split")
    }

    fn line(way: &[LonLat]) -> Geometry {
        let path: Vec<V3> = way.iter().map(|&p| dir(p)).collect();
        Geometry::Lines(to_parts(&path))
    }

    fn rings(p: &[Vec<Vec<LonLat>>]) -> usize {
        p.iter().map(|poly| poly.len()).sum()
    }

    fn assert_whole(whole: &Geometry, merged: &Geometry, probes: &[LonLat]) {
        let (w, m) = (polygons(whole), polygons(merged));
        assert_valid(&m);
        assert_eq!(rings(&m), rings(&w), "{m:?}");
        for &p in probes {
            assert_eq!(inside(&m, p), inside(&w, p), "{p:?}");
        }
        let (a, b) = (size(whole), size(merged));
        assert!((a - b).abs() < 1e-9 * a, "{a} {b}");
    }

    #[test]
    fn polygons_that_share_a_border_lose_it() {
        let whole = square(0.0, 0.0, 10.0, 10.0);
        let parts = split(&whole, &[[5.0, -3.0], [6.0, 4.0], [4.0, 7.0], [5.0, 13.0]]);
        assert_eq!(parts.len(), 2);
        let merged = merge(&parts.iter().collect::<Vec<_>>()).expect("a merge");
        assert_eq!(polygons(&merged).len(), 1);
        assert_whole(
            &whole,
            &merged,
            &[[5.0, 5.0], [1.0, 9.0], [9.0, 1.0], [11.0, 5.0]],
        );
    }

    #[test]
    fn polygons_that_do_not_touch_become_parts_of_one_feature() {
        let (a, b) = (square(0.0, 0.0, 5.0, 5.0), square(10.0, 0.0, 15.0, 5.0));
        let merged = merge(&[&a, &b]).expect("a merge");
        let p = polygons(&merged);
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        assert!(inside(&p, [2.0, 2.0]) && inside(&p, [12.0, 2.0]));
        assert!(!inside(&p, [7.0, 2.0]));
    }

    #[test]
    fn polygons_that_overlap_become_one_area() {
        let (a, b) = (square(0.0, 0.0, 6.0, 6.0), square(4.0, 4.0, 10.0, 10.0));
        let merged = merge(&[&a, &b]).expect("a merge");
        let p = polygons(&merged);
        assert_valid(&p);
        assert_eq!(rings(&p), 1);
        for q in [[1.0, 1.0], [5.0, 5.0], [9.0, 9.0]] {
            assert!(inside(&p, q), "{q:?}");
        }
        assert!(!inside(&p, [9.0, 1.0]));
    }

    #[test]
    fn a_polygon_around_a_hole_keeps_the_hole() {
        let mut ring = Geometry::Polygons(vec![vec![
            box_way(0.0, 0.0, 10.0, 10.0),
            box_way(4.0, 4.0, 6.0, 6.0).into_iter().rev().collect(),
        ]]);
        let other = square(20.0, 0.0, 25.0, 5.0);
        let merged = merge(&[&ring, &other]).expect("a merge");
        let p = polygons(&merged);
        assert_valid(&p);
        assert!(!inside(&p, [5.0, 5.0]));
        assert!(inside(&p, [2.0, 2.0]));
        // An island in the hole fills it.
        ring = square(4.0, 4.0, 6.0, 6.0);
        let filled = merge(&[&merged, &ring]).expect("a merge");
        assert!(inside(&polygons(&filled), [5.0, 5.0]));
    }

    #[test]
    fn polygons_across_the_180_degree_meridian_join() {
        let whole = stored(&box_way(170.0, -10.0, 190.0, 10.0));
        let parts = split(&whole, &[[178.0, -15.0], [181.0, 0.0], [178.0, 15.0]]);
        assert_eq!(parts.len(), 2);
        let merged = merge(&parts.iter().collect::<Vec<_>>()).expect("a merge");
        let probes = [
            [175.0, 0.0],
            [-175.0, 0.0],
            [179.5, 5.0],
            [-179.5, -5.0],
            [160.0, 0.0],
        ];
        assert_whole(&whole, &merged, &probes);
    }

    #[test]
    fn polygons_around_a_pole_join() {
        let way: Vec<LonLat> = (0..=8).map(|k| [k as f64 * 45.0, 75.0]).collect();
        let whole = stored(&way);
        let parts = split(&whole, &[[30.0, 65.0], [30.0, 90.0], [210.0, 65.0]]);
        assert_eq!(parts.len(), 2);
        let merged = merge(&parts.iter().collect::<Vec<_>>()).expect("a merge");
        let probes = [[0.0, 89.0], [100.0, 80.0], [-100.0, 80.0], [0.0, 70.0]];
        assert_whole(&whole, &merged, &probes);
    }

    #[test]
    fn two_lines_that_share_an_end_become_one_line() {
        let (a, b) = (
            line(&[[0.0, 0.0], [5.0, 1.0]]),
            line(&[[5.0, 1.0], [9.0, 0.0]]),
        );
        let want = lines(&line(&[[0.0, 0.0], [5.0, 1.0], [9.0, 0.0]]));
        assert_eq!(lines(&merge(&[&a, &b]).expect("a merge")), want);
        // The order of the features and the way of each line do not matter.
        let b_back = line(&[[9.0, 0.0], [5.0, 1.0]]);
        let joined = lines(&merge(&[&b_back, &a]).expect("a merge"));
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].len(), 3);
        let ends = [joined[0][0], joined[0][2]];
        assert!(ends.contains(&[0.0, 0.0]) && ends.contains(&[9.0, 0.0]));
    }

    #[test]
    fn lines_that_do_not_touch_become_parts_of_one_feature() {
        let (a, b) = (
            line(&[[0.0, 0.0], [5.0, 1.0]]),
            line(&[[6.0, 1.0], [9.0, 0.0]]),
        );
        let joined = lines(&merge(&[&a, &b]).expect("a merge"));
        assert_eq!(joined.len(), 2);
    }

    #[test]
    fn lines_across_the_180_degree_meridian_join() {
        let whole = line(&[[170.0, 0.0], [178.0, 2.0], [-176.0, 1.0], [-170.0, 0.0]]);
        let parts = split(&whole, &[[-178.0, -5.0], [-178.0, 5.0]]);
        assert_eq!(parts.len(), 2);
        let merged = lines(&merge(&[&parts[1], &parts[0]]).expect("a merge"));
        assert_eq!(merged.len(), 2, "{merged:?}");
        assert_eq!(merged[0].last().unwrap()[0].abs(), 180.0);
        assert_eq!(merged[0].last().unwrap()[1], merged[1][0][1]);
        let length = size(&Geometry::Lines(merged));
        assert!((length - size(&whole)).abs() < 1e-9, "{length}");
    }

    #[test]
    fn points_become_one_feature() {
        let (a, b) = (
            Geometry::Points(vec![[1.0, 2.0]]),
            Geometry::Points(vec![[3.0, 4.0]]),
        );
        let merged = merge(&[&a, &b]).expect("a merge");
        assert_eq!(merged, Geometry::Points(vec![[1.0, 2.0], [3.0, 4.0]]));
    }

    #[test]
    fn geometries_of_two_kinds_do_not_merge() {
        let (a, b) = (
            Geometry::Points(vec![[1.0, 2.0]]),
            line(&[[0.0, 0.0], [1.0, 1.0]]),
        );
        assert!(merge(&[&a, &b]).is_none());
        assert!(merge(&[]).is_none());
    }
}
