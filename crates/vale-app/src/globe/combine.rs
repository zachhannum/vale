//! Add and Subtract. A pen stroke closes into a ring, as in the polygon
//! tool, and its area joins a polygon or is cut out of it.
//!
//! The outlines of the polygon and the rings of the stroke divide the
//! sphere into areas. Add keeps each area in the polygon or in the stroke,
//! and Subtract keeps each area in the polygon and not in the stroke. The
//! stored vertices of the polygon stay where they are.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use std::cell::Cell;

use vale_store::{Geometry, LonLat};

use super::math::{V3, lonlat_to_dir};
use super::polygon::{
    MIN_AREA, MIN_LENGTH, bounds, densify, dirs, overlay, path, path_length, rings, signed_area,
    to_polygons_many,
};
use super::reshape::{REACH, ends, outlines};
use super::snap::{PathVertex, Run, distance_to_arc};
use super::vector;

/// How the polygon tool uses a stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolygonMode {
    /// The stroke makes a new feature.
    #[default]
    New,
    /// The area of the stroke joins a polygon.
    Add,
    /// The area of the stroke is cut out of polygons.
    Subtract,
}

/// The closed area of a stroke, or the area of a stored polygon that cuts
/// other polygons.
pub struct StrokeArea {
    rings: Vec<Vec<PathVertex>>,
    area: Feature,
    /// The closed path of the stroke, in the order of the pen. For a stored
    /// polygon, the vertices of its outlines.
    path: Vec<V3>,
    /// A box around the path.
    bounds: Bounds,
    min_area: f64,
}

/// A box in 3D around a part of the sphere.
pub(super) type Bounds = ([f64; 3], [f64; 3]);

impl StrokeArea {
    /// The area of a stroke. The runs come from a `Tracer`. A stroke that is
    /// too short or that encloses no area gives `None`.
    pub fn new(runs: &[Run], scale: f64, strength: f64) -> Option<Self> {
        ends(runs)?;
        let path = path(runs, scale, strength);
        if path_length(&path) < MIN_LENGTH / scale {
            return None;
        }
        let min_area = MIN_AREA / (scale * scale);
        let closed = densify(&path);
        let rings: Vec<Vec<PathVertex>> = rings(&closed)
            .into_iter()
            .filter(|r| signed_area(&dirs(r)) > min_area)
            .collect();
        if rings.is_empty() {
            return None;
        }
        // Each ring of a stroke is an area of its own.
        let area = Feature(rings.iter().map(|r| vec![dirs(r)]).collect());
        let path = super::polygon::dirs(&closed);
        let bounds = boxed(path.iter().copied(), REACH / scale);
        Some(StrokeArea {
            rings,
            area,
            path,
            bounds,
            min_area,
        })
    }

    /// The area of a stored polygon, for Cut out. The outlines of the
    /// polygon keep their stored vertices, so a cut follows them exactly.
    /// A geometry that is not a polygon, or whose rings do not join, gives
    /// `None`.
    pub fn of_polygons(geometry: &Geometry, scale: f64) -> Option<Self> {
        let Geometry::Polygons(polygons) = geometry else {
            return None;
        };
        let rings: Vec<Vec<PathVertex>> = outlines(polygons)?.into_iter().map(|o| o.ring).collect();
        let path: Vec<V3> = rings.iter().flat_map(|r| dirs(r)).collect();
        if path.is_empty() {
            return None;
        }
        let bounds = boxed(path.iter().copied(), REACH / scale);
        Some(StrokeArea {
            rings,
            area: Feature::new(polygons),
            path,
            bounds,
            min_area: MIN_AREA / (scale * scale),
        })
    }

    fn holds(&self, p: V3) -> bool {
        self.area.holds(p)
    }

    /// The geometry can overlap the area of the stroke. The test is quick,
    /// and it can be true for a geometry that does not overlap.
    pub fn near(&self, geometry: &Geometry) -> bool {
        let Geometry::Polygons(polygons) = geometry else {
            return false;
        };
        let mut edges = polygons.iter().flatten().flat_map(|r| r.windows(2));
        let crosses = edges.any(|w| {
            let (lo, hi) = bounds(dir(w[0]), dir(w[1]));
            (0..3).all(|k| lo[k] <= self.bounds.1[k] && self.bounds.0[k] <= hi[k])
        });
        if crosses {
            return true;
        }
        // With no outline near the stroke, one area holds the other, or
        // they do not meet.
        let inside = self
            .path
            .first()
            .is_some_and(|&p| Feature::new(polygons).holds(p));
        let around = polygons
            .iter()
            .flatten()
            .flatten()
            .next()
            .is_some_and(|&p| self.holds(dir(p)));
        inside || around
    }

    /// The place along the stroke where it first reaches a polygon of the
    /// geometry: the index of the first vertex of the path in the polygon.
    /// A start within `REACH` points of the outline reaches it at 0. A
    /// stroke that does not reach the geometry gives `None`.
    pub fn reaches(&self, geometry: &Geometry, scale: f64) -> Option<usize> {
        let Geometry::Polygons(polygons) = geometry else {
            return None;
        };
        let start = *self.path.first()?;
        let near_start = polygons.iter().flatten().any(|ring| {
            ring.windows(2).any(|w| {
                let (a, b) = (dir(w[0]), dir(w[1]));
                distance_to_arc(start, a, b) * scale <= REACH
            })
        });
        if near_start {
            return Some(0);
        }
        let feature = Feature::new(polygons);
        self.path.iter().position(|&p| feature.holds(p))
    }

    /// The geometry with the area of the stroke joined to it. A stroke that
    /// adds no area gives `None`.
    pub fn add(&self, geometry: &Geometry) -> Option<Geometry> {
        let polygons = self.combine(geometry, |a, b| a || b)??;
        Some(Geometry::Polygons(polygons))
    }

    /// The geometry with the area of the stroke cut out of it. A stroke
    /// that does not overlap the geometry gives `None`, and a stroke that
    /// covers it gives `Some(None)`.
    pub fn subtract(&self, geometry: &Geometry) -> Option<Option<Geometry>> {
        let polygons = self.combine(geometry, |a, b| a && !b)?;
        Some(polygons.map(Geometry::Polygons))
    }

    /// The polygons of the areas that `keep` chooses, from whether an area
    /// is in the geometry and whether it is in the stroke. A result with no
    /// change gives `None`, and an empty result gives `Some(None)`.
    #[allow(clippy::type_complexity)]
    fn combine(
        &self,
        geometry: &Geometry,
        keep: impl Fn(bool, bool) -> bool,
    ) -> Option<Option<Vec<Vec<Vec<LonLat>>>>> {
        let Geometry::Polygons(polygons) = geometry else {
            return None;
        };
        let feature = Feature::new(polygons);
        let mut paths: Vec<Vec<PathVertex>> =
            outlines(polygons)?.into_iter().map(|o| o.ring).collect();
        paths.extend(self.rings.iter().cloned());
        let changed = Cell::new(false);
        let result = overlay(&paths, |p| {
            let a = feature.holds(p);
            let k = keep(a, self.holds(p));
            if k != a {
                changed.set(true);
            }
            k
        });
        if !changed.get() {
            return None;
        }
        let rings: Vec<Vec<PathVertex>> = result
            .into_iter()
            .filter(|r| signed_area(&dirs(r)).abs() > self.min_area)
            .collect();
        let out = to_polygons_many(&rings);
        Some((!out.is_empty()).then_some(out))
    }
}

/// The stored polygons of a feature, as the globe draws them.
pub(super) struct Feature(Vec<Vec<Vec<V3>>>);

impl Feature {
    pub(super) fn new(polygons: &[Vec<Vec<LonLat>>]) -> Self {
        Feature(
            polygons
                .iter()
                .map(|poly| {
                    poly.iter()
                        .enumerate()
                        .map(|(i, r)| vector::ring(r, i > 0))
                        .collect()
                })
                .collect(),
        )
    }

    pub(super) fn holds(&self, p: V3) -> bool {
        self.0.iter().any(|poly| vector::contains(poly, p))
    }
}

/// A box around directions, with a margin in radians.
pub(super) fn boxed(dirs: impl Iterator<Item = V3>, margin: f64) -> Bounds {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for d in dirs {
        for k in 0..3 {
            lo[k] = lo[k].min(d[k] - margin);
            hi[k] = hi[k].max(d[k] + margin);
        }
    }
    (lo, hi)
}

pub(super) fn dir(p: LonLat) -> V3 {
    lonlat_to_dir(p[0], p[1])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vale_store::GeometryKind;

    use super::*;
    use crate::globe::polygon::tests::{assert_valid, has_run, inside};
    use crate::globe::snap::SnapSettings;
    use crate::globe::snap::tests::{self as snap_test, kinds, pen, trace};

    const SCALE: f64 = 300.0;

    fn polygons(g: &Geometry) -> Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not a polygon");
        };
        p.clone()
    }

    fn area(way: &[LonLat]) -> StrokeArea {
        StrokeArea::new(&[Run::Free(pen(way, 0.0))], SCALE, 0.0).expect("an area")
    }

    /// The polygon that the polygon tool makes of a closed way. A longitude
    /// can be past 180 degrees.
    fn stored(ring: &[LonLat]) -> Geometry {
        crate::globe::polygon::commit(&[Run::Free(pen(ring, 0.0))], SCALE, 0.0).expect("a polygon")
    }

    fn square() -> Geometry {
        Geometry::Polygons(vec![vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ]]])
    }

    fn box_way(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<LonLat> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
    }

    #[test]
    fn add_joins_the_area_of_the_stroke() {
        let s = area(&box_way(5.0, 2.0, 15.0, 8.0));
        let p = polygons(&s.add(&square()).expect("a change"));
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 1);
        assert!(inside(&p, [12.0, 5.0]));
        assert!(inside(&p, [2.0, 5.0]));
        assert!(!inside(&p, [12.0, 9.0]));
        // The stored corners stay.
        for c in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
            assert!(p[0][0].contains(&c), "{c:?}");
        }
    }

    #[test]
    fn add_fills_a_gap_only_inside_the_ring() {
        // A stroke from the polygon out east, south, and back in west of
        // its start. The ground between the stroke and the polygon below
        // the ring stays out.
        let way = [
            [8.0, 5.0],
            [14.0, 5.0],
            [14.0, -6.0],
            [6.0, -6.0],
            [6.0, -2.0],
            [9.0, -2.0],
            [9.0, 4.0],
        ];
        let s = area(&way);
        let p = polygons(&s.add(&square()).expect("a change"));
        assert_valid(&p);
        assert!(inside(&p, [12.0, 0.0]));
        assert!(inside(&p, [11.0, -4.0]));
        assert!(!inside(&p, [7.5, -1.0]));
    }

    #[test]
    fn a_stroke_inside_the_polygon_adds_nothing() {
        let s = area(&box_way(2.0, 2.0, 8.0, 8.0));
        assert!(s.add(&square()).is_none());
    }

    #[test]
    fn the_stroke_reaches_the_polygon_from_inside_or_from_outside() {
        // From inside.
        let s = area(&box_way(5.0, 2.0, 15.0, 8.0));
        assert_eq!(s.reaches(&square(), SCALE), Some(0));
        // From within reach of the outline, outside.
        let s = area(&box_way(10.5, 2.0, 15.0, 8.0));
        assert_eq!(s.reaches(&square(), SCALE), Some(0));
        // From away, into the polygon and out again.
        let s = area(&[[16.0, 2.0], [6.0, 2.0], [6.0, 8.0], [16.0, 8.0]]);
        assert!(s.reaches(&square(), SCALE).is_some_and(|at| at > 0));
        assert!(s.near(&square()));
        let p = polygons(&s.add(&square()).expect("a change"));
        assert_valid(&p);
        assert!(inside(&p, [13.0, 5.0]));
        // Inside the polygon, away from its outline, and around it.
        assert!(area(&box_way(4.0, 4.0, 6.0, 6.0)).near(&square()));
        assert!(area(&box_way(-5.0, -5.0, 15.0, 15.0)).near(&square()));
        // Away from the polygon.
        let s = area(&box_way(20.0, 2.0, 25.0, 8.0));
        assert_eq!(s.reaches(&square(), SCALE), None);
        assert!(!s.near(&square()));
    }

    #[test]
    fn a_stroke_reaches_the_nearer_polygon_first() {
        let east = Geometry::Polygons(vec![vec![box_way(20.0, 0.0, 30.0, 10.0)]]);
        // From the gap between the two, west into the square, and then east
        // into the other polygon.
        let s = area(&[
            [15.0, 4.0],
            [5.0, 4.0],
            [5.0, 6.0],
            [25.0, 6.0],
            [25.0, 4.0],
        ]);
        let (a, b) = (s.reaches(&square(), SCALE), s.reaches(&east, SCALE));
        assert!(a.unwrap() < b.unwrap(), "{a:?} {b:?}");
    }

    #[test]
    fn a_cut_through_leaves_two_parts() {
        let s = area(&box_way(4.0, -2.0, 6.0, 12.0));
        let p = polygons(&s.subtract(&square()).expect("a change").expect("an area"));
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        assert!(inside(&p, [2.0, 5.0]));
        assert!(inside(&p, [8.0, 5.0]));
        assert!(!inside(&p, [5.0, 5.0]));
    }

    #[test]
    fn a_cut_inside_leaves_a_hole() {
        let s = area(&box_way(3.0, 3.0, 7.0, 7.0));
        let p = polygons(&s.subtract(&square()).expect("a change").expect("an area"));
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
        assert!(inside(&p, [1.0, 5.0]));
        assert!(!inside(&p, [5.0, 5.0]));
        // Add fills the hole again.
        let s = area(&box_way(2.0, 2.0, 8.0, 8.0));
        let q = polygons(&s.add(&Geometry::Polygons(p)).expect("a change"));
        assert_valid(&q);
        assert_eq!(q[0].len(), 1);
        assert!(inside(&q, [5.0, 5.0]));
    }

    #[test]
    fn a_cut_away_from_the_polygon_changes_nothing() {
        let s = area(&box_way(20.0, 2.0, 25.0, 8.0));
        assert!(s.subtract(&square()).is_none());
    }

    #[test]
    fn a_cut_over_the_whole_polygon_leaves_nothing() {
        let s = area(&box_way(-2.0, -2.0, 12.0, 12.0));
        assert!(matches!(s.subtract(&square()), Some(None)));
    }

    #[test]
    fn add_and_subtract_work_across_the_180_degree_meridian() {
        let forest = stored(&box_way(170.0, -10.0, 190.0, 10.0));
        let s = area(&box_way(185.0, -5.0, 200.0, 5.0));
        let p = polygons(&s.add(&forest).expect("a change"));
        assert_valid(&p);
        assert!(inside(&p, [-162.0, 0.0]));
        assert!(inside(&p, [175.0, 0.0]));
        assert!(!inside(&p, [-162.0, 8.0]));
        // A cut across the meridian.
        let s = area(&box_way(175.0, -3.0, 185.0, 3.0));
        let p = polygons(&s.subtract(&forest).expect("a change").expect("an area"));
        assert_valid(&p);
        assert!(!inside(&p, [179.0, 0.0]));
        assert!(!inside(&p, [-179.0, 0.0]));
        assert!(inside(&p, [179.0, 6.0]));
        assert!(inside(&p, [-179.0, -6.0]));
    }

    #[test]
    fn add_and_subtract_work_around_a_pole() {
        let cap = stored(&[[0.0, 75.0], [120.0, 75.0], [240.0, 75.0], [360.0, 75.0]]);
        let p = polygons(&cap);
        assert!(inside(&p, [0.0, 89.0]));
        // A cut around the pole leaves a ring of land.
        let s = area(&[[0.0, 85.0], [120.0, 85.0], [240.0, 85.0], [360.0, 85.0]]);
        assert!(s.near(&cap));
        let p = polygons(&s.subtract(&cap).expect("a change").expect("an area"));
        assert_valid(&p);
        assert!(!inside(&p, [30.0, 89.0]));
        assert!(inside(&p, [30.0, 80.0]));
        assert!(inside(&p, [-150.0, 80.0]));
        assert!(!inside(&p, [30.0, 70.0]));
        // Add on the side of the cap.
        let s = area(&box_way(-20.0, 70.0, 20.0, 78.0));
        let q = polygons(&s.add(&cap).expect("a change"));
        assert_valid(&q);
        assert!(inside(&q, [0.0, 72.0]));
        assert!(inside(&q, [0.0, 89.0]));
        assert!(!inside(&q, [90.0, 72.0]));
    }

    fn cutter(g: &Geometry) -> StrokeArea {
        StrokeArea::of_polygons(g, SCALE).expect("a cutter")
    }

    #[test]
    fn a_polygon_cuts_a_hole_with_its_own_vertices() {
        let lake = Geometry::Polygons(vec![vec![box_way(3.0, 3.0, 7.0, 7.0)]]);
        let c = cutter(&lake);
        assert!(c.near(&square()));
        let p = polygons(&c.subtract(&square()).expect("a change").expect("an area"));
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
        assert!(!inside(&p, [5.0, 5.0]));
        assert!(inside(&p, [1.0, 5.0]));
        for corner in [[3.0, 3.0], [7.0, 3.0], [7.0, 7.0], [3.0, 7.0]] {
            assert!(p[0][1].contains(&corner), "{corner:?}");
        }
    }

    #[test]
    fn a_polygon_with_a_hole_leaves_an_island() {
        let ring = Geometry::Polygons(vec![vec![
            box_way(2.0, 2.0, 8.0, 8.0),
            box_way(4.0, 4.0, 6.0, 6.0),
        ]]);
        let p = polygons(
            &cutter(&ring)
                .subtract(&square())
                .expect("a change")
                .expect("an area"),
        );
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        assert!(inside(&p, [5.0, 5.0]));
        assert!(inside(&p, [1.0, 5.0]));
        assert!(!inside(&p, [3.0, 5.0]));
    }

    #[test]
    fn a_polygon_over_the_whole_polygon_leaves_nothing() {
        let sea = Geometry::Polygons(vec![vec![box_way(-2.0, -2.0, 12.0, 12.0)]]);
        assert!(matches!(cutter(&sea).subtract(&square()), Some(None)));
        let away = Geometry::Polygons(vec![vec![box_way(20.0, 2.0, 25.0, 8.0)]]);
        assert!(!cutter(&away).near(&square()));
        assert!(cutter(&away).subtract(&square()).is_none());
        assert!(StrokeArea::of_polygons(&Geometry::Lines(vec![]), SCALE).is_none());
    }

    #[test]
    fn a_polygon_cuts_across_the_180_degree_meridian() {
        let forest = stored(&box_way(170.0, -10.0, 190.0, 10.0));
        let lake = stored(&box_way(175.0, -3.0, 185.0, 3.0));
        let c = cutter(&lake);
        assert!(c.near(&forest));
        let p = polygons(&c.subtract(&forest).expect("a change").expect("an area"));
        assert_valid(&p);
        assert!(!inside(&p, [179.0, 0.0]));
        assert!(!inside(&p, [-179.0, 0.0]));
        assert!(inside(&p, [179.0, 6.0]));
        assert!(inside(&p, [-179.0, -6.0]));
    }

    #[test]
    fn a_polygon_cuts_around_a_pole() {
        let cap = stored(&[[0.0, 75.0], [120.0, 75.0], [240.0, 75.0], [360.0, 75.0]]);
        let ice = stored(&[[0.0, 85.0], [120.0, 85.0], [240.0, 85.0], [360.0, 85.0]]);
        let c = cutter(&ice);
        assert!(c.near(&cap));
        let p = polygons(&c.subtract(&cap).expect("a change").expect("an area"));
        assert_valid(&p);
        assert!(!inside(&p, [30.0, 89.0]));
        assert!(inside(&p, [30.0, 80.0]));
        assert!(inside(&p, [-150.0, 80.0]));
        // The cap cuts the ring of land away, and the ice cap stays.
        assert!(matches!(cutter(&cap).subtract(&ice), Some(None)));
    }

    #[test]
    fn a_part_of_the_stroke_traces_a_border() {
        let border = vec![vec![
            [12.0, -5.0],
            [12.1234567, 2.0],
            [11.7654321, 5.0],
            [12.3333333, 8.0],
            [12.0, 15.0],
        ]];
        let a = snap_test::layer(1, GeometryKind::Line, vec![Geometry::Lines(border)]);
        let t = Arc::new(snap_test::targets(&[a]));
        // From the forest east to the border, along it, and back.
        let way = [
            [8.0, 1.0],
            [11.9, 1.0],
            [12.12, 2.0],
            [11.77, 5.0],
            [12.33, 8.0],
            [11.9, 9.0],
            [8.0, 9.0],
        ];
        let runs = trace(&t, &pen(&way, 0.0), SnapSettings::default(), 9);
        assert!(kinds(&runs).contains('T'), "{}", kinds(&runs));
        let s = StrokeArea::new(&runs, SCALE, 0.0).expect("an area");
        let p = polygons(&s.add(&square()).expect("a change"));
        assert_valid(&p);
        assert!(has_run(
            &p,
            &[[12.1234567, 2.0], [11.7654321, 5.0], [12.3333333, 8.0]]
        ));
        assert!(inside(&p, [11.0, 5.0]));
    }
}
