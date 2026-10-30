use std::f64::consts::PI;

use vale_sphere::measure::*;

const EARTH: f64 = 6371.0;
const SMALL: f64 = 500.0;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

#[test]
fn a_quarter_of_the_equator_is_a_quarter_circle() {
    for r in [EARTH, SMALL] {
        let l = path_length(&[[0.0, 0.0], [45.0, 0.0], [90.0, 0.0]], false, r);
        assert!(close(l, PI * r / 2.0, 1e-4), "{l}");
    }
}

#[test]
fn a_closed_path_counts_the_last_edge() {
    let tri = [[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]];
    let l = path_length(&tri, true, EARTH);
    assert!(close(l, 3.0 * PI * EARTH / 2.0, 1e-9));
}

#[test]
fn short_and_antipodal_lengths_are_stable() {
    let l = path_length(&[[10.0, 10.0], [10.0, 10.000001]], false, EARTH);
    assert!(close(l, EARTH * 1e-6_f64.to_radians(), 1e-4), "{l}");
    let l = path_length(&[[0.0, 0.0], [180.0, 0.0]], false, EARTH);
    assert!(close(l, PI * EARTH, 1e-9));
}

#[test]
fn the_octant_has_an_eighth_of_the_sphere_in_both_windings() {
    for r in [EARTH, SMALL] {
        let ccw = [[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]];
        let cw = [[0.0, 0.0], [0.0, 90.0], [90.0, 0.0]];
        let exact = PI * r * r / 2.0;
        assert!(close(signed_ring_area(&ccw, r), exact, 1e-9));
        assert!(close(signed_ring_area(&cw, r), -exact, 1e-9));
        assert!(close(ring_area(&ccw, r), exact, 1e-9));
        assert!(close(ring_area(&cw, r), exact, 1e-9));
    }
}

#[test]
fn a_cap_has_its_exact_area() {
    // A ring along the parallel at 60°N, with arcs short enough to follow it.
    for r in [EARTH, SMALL] {
        let ring: Vec<_> = (0..3600).map(|i| [i as f64 * 0.1 - 180.0, 60.0]).collect();
        let exact = 2.0 * PI * r * r * (1.0 - 60f64.to_radians().sin());
        assert!(
            close(ring_area(&ring, r), exact, 1e-3),
            "{}",
            ring_area(&ring, r)
        );
    }
}

#[test]
fn a_ring_across_the_antimeridian_measures_the_same_as_at_zero() {
    let at = |lon0: f64| {
        vec![
            [lon0 - 10.0, -5.0],
            [lon0 + 10.0, -5.0],
            [lon0 + 10.0, 5.0],
            [lon0 - 10.0, 5.0],
        ]
    };
    let wrap = |r: Vec<[f64; 2]>| {
        r.into_iter()
            .map(|p| [vale_sphere::wrap180(p[0]), p[1]])
            .collect::<Vec<_>>()
    };
    let (zero, seam) = (at(0.0), wrap(at(180.0)));
    assert!(close(
        ring_area(&seam, EARTH),
        ring_area(&zero, EARTH),
        1e-9
    ));
    assert!(close(
        path_length(&seam, true, EARTH),
        path_length(&zero, true, EARTH),
        1e-9
    ));
}

#[test]
fn a_polygon_subtracts_its_holes() {
    let outer = vec![[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]];
    let hole = vec![[10.0, 10.0], [20.0, 10.0], [10.0, 20.0]];
    let a = polygon_area(&[outer.clone(), hole.clone()], EARTH);
    assert!(close(
        a,
        ring_area(&outer, EARTH) - ring_area(&hole, EARTH),
        1e-9
    ));
}

#[test]
fn densified_points_are_on_the_arc_and_close_together() {
    let pts = [[-170.0, 20.0], [170.0, 50.0], [10.0, -30.0]];
    let d = densify_great_circle(&pts, 1.0, true);
    assert_eq!(d.first(), Some(&pts[0]));
    assert_eq!(d.last(), Some(&pts[0]));
    for w in d.windows(2) {
        assert!(central_angle(w[0], w[1]).to_degrees() <= 1.0 + 1e-9);
    }
    let sum: f64 = d.windows(2).map(|w| central_angle(w[0], w[1])).sum();
    assert!(close(sum * EARTH, path_length(&pts, true, EARTH), 1e-9));
    assert!(d.iter().all(|p| p[0] >= -180.0 && p[0] <= 180.0));
}

#[test]
fn a_bow_tie_crosses_itself() {
    let bow = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
    let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    assert!(ring_crosses_itself(&bow));
    assert!(!ring_crosses_itself(&square));
}
