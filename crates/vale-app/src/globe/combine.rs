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
    MIN_AREA, MIN_LENGTH, densify, dirs, overlay, path, path_length, rings, signed_area,
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

/// The closed area of a stroke.
pub struct StrokeArea {
    rings: Vec<Vec<PathVertex>>,
    dirs: Vec<Vec<V3>>,
    start: V3,
    min_area: f64,
}

impl StrokeArea {
    /// The area of a stroke. The runs come from a `Tracer`. A stroke that is
    /// too short or that encloses no area gives `None`.
    pub fn new(runs: &[Run], scale: f64, strength: f64) -> Option<Self> {
        let (start, _) = ends(runs)?;
        let path = path(runs, scale, strength);
        if path_length(&path) < MIN_LENGTH / scale {
            return None;
        }
        let min_area = MIN_AREA / (scale * scale);
        let rings: Vec<Vec<PathVertex>> = rings(&densify(&path))
            .into_iter()
            .filter(|r| signed_area(&dirs(r)) > min_area)
            .collect();
        if rings.is_empty() {
            return None;
        }
        let dirs = rings.iter().map(|r| dirs(r)).collect();
        Some(StrokeArea {
            rings,
            dirs,
            start,
            min_area,
        })
    }

    fn holds(&self, p: V3) -> bool {
        self.dirs
            .iter()
            .any(|r| vector::contains(std::slice::from_ref(r), p))
    }

    /// The stroke starts inside a polygon of the geometry, or within
    /// `REACH` points of its outline.
    pub fn starts_in(&self, geometry: &Geometry, scale: f64) -> bool {
        let Geometry::Polygons(polygons) = geometry else {
            return false;
        };
        if Feature::new(polygons).holds(self.start) {
            return true;
        }
        polygons.iter().flatten().any(|ring| {
            ring.windows(2).any(|w| {
                let (a, b) = (dir(w[0]), dir(w[1]));
                distance_to_arc(self.start, a, b) * scale <= REACH
            })
        })
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
struct Feature(Vec<Vec<Vec<V3>>>);

impl Feature {
    fn new(polygons: &[Vec<Vec<LonLat>>]) -> Self {
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

    fn holds(&self, p: V3) -> bool {
        self.0.iter().any(|poly| vector::contains(poly, p))
    }
}

fn dir(p: LonLat) -> V3 {
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
    fn the_start_finds_the_polygon() {
        let s = area(&box_way(5.0, 2.0, 15.0, 8.0));
        assert!(s.starts_in(&square(), SCALE));
        let s = area(&box_way(20.0, 2.0, 25.0, 8.0));
        assert!(!s.starts_in(&square(), SCALE));
        // Within reach of the outline, from outside.
        let s = area(&box_way(10.5, 2.0, 15.0, 8.0));
        assert!(s.starts_in(&square(), SCALE));
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
