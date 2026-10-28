use std::collections::HashSet;
use std::time::Instant;

use vale_sphere::{
    LonLat, Point, Rect, ScatterParams, ScatterPlane, ScatterPoint, min_distance, scatter,
    seed_from,
};

const R: f64 = 6_371_000.0;

/// The point at `dist` meters from `c` in the direction `az` degrees.
fn destination(c: LonLat, az: f64, dist: f64) -> LonLat {
    let (lat1, lon1) = (c[1].to_radians(), c[0].to_radians());
    let (d, az) = (dist / R, az.to_radians());
    let lat2 = (lat1.sin() * d.cos() + lat1.cos() * d.sin() * az.cos()).asin();
    let lon2 = lon1 + (az.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());
    [vale_sphere::wrap180(lon2.to_degrees()), lat2.to_degrees()]
}

/// A wobbly ring around `c` with a mean radius of `r` meters.
fn blob(c: LonLat, r: f64, n: usize) -> Vec<LonLat> {
    (0..n)
        .map(|k| {
            let a = 360.0 * k as f64 / n as f64;
            let t = a.to_radians();
            destination(
                c,
                a,
                r * (1.0 + 0.15 * (3.0 * t).sin() + 0.05 * (7.0 * t).cos()),
            )
        })
        .collect()
}

fn params(spacing: f64) -> ScatterParams {
    ScatterParams {
        seed: 7,
        spacing,
        inset: 0.0,
        size_jitter: 0.3,
        rotation_jitter: 20.0,
        max_points: 1_000_000,
    }
}

fn plane_points(plane: &ScatterPlane, pts: &[ScatterPoint]) -> Vec<Point> {
    pts.iter()
        .map(|p| plane.forward(p.lonlat).unwrap())
        .collect()
}

fn min_pair(pts: &[Point]) -> f64 {
    let mut best = f64::INFINITY;
    let mut sorted = pts.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
    for (k, a) in sorted.iter().enumerate() {
        for b in &sorted[k + 1..] {
            if b.x - a.x >= best {
                break;
            }
            best = best.min((*b - *a).hypot());
        }
    }
    best
}

fn area(ring: &[Point]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|k| {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
    (p - (a + ab * t)).hypot()
}

fn forest() -> Vec<Vec<LonLat>> {
    vec![blob([10.0, 45.0], 150_000.0, 120)]
}

fn keys(pts: &[ScatterPoint]) -> HashSet<[u64; 2]> {
    pts.iter()
        .map(|p| [p.lonlat[0].to_bits(), p.lonlat[1].to_bits()])
        .collect()
}

#[test]
fn points_keep_the_minimum_distance() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let d = min_distance(&plane, 2000.0);
    assert!((2000.0..4000.0).contains(&d), "{d}");
    let pts = scatter(&plane, &rings, &params(2000.0), None);
    assert!(pts.len() > 1000, "{}", pts.len());
    let m = min_pair(&plane_points(&plane, &pts));
    assert!(m >= d - 1e-6, "{m} < {d}");
    for p in &pts {
        assert!((0.7..=1.0).contains(&p.scale));
        assert!((-20.0..=20.0).contains(&p.rotation));
    }
}

#[test]
fn smaller_spacing_adds_points_and_moves_none() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let coarse = scatter(&plane, &rings, &params(4000.0), None);
    let fine = scatter(&plane, &rings, &params(1000.0), None);
    assert!(fine.len() > 2 * coarse.len());
    let fine_keys = keys(&fine);
    assert!(keys(&coarse).is_subset(&fine_keys));
    // The scale and the rotation of a point also stay.
    for p in &coarse {
        assert!(fine.contains(p));
    }
}

#[test]
fn a_window_is_the_full_result_clipped() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let full = scatter(&plane, &rings, &params(3000.0), None);
    let full_plane = plane_points(&plane, &full);
    let mut windows = vec![
        Rect::new(-50_000.0, -40_000.0, 30_000.0, 20_000.0),
        Rect::new(-500_000.0, -10.0, 500_000.0, 10.0),
        Rect::new(100_000.0, 100_000.0, 400_000.0, 400_000.0),
    ];
    // A pan: the same window shifted in small steps.
    for k in 0..6 {
        let dx = 7_321.5 * k as f64;
        windows.push(Rect::new(
            -90_000.0 + dx,
            -70_000.0,
            -10_000.0 + dx,
            15_000.0,
        ));
    }
    for w in windows {
        let part = scatter(&plane, &rings, &params(3000.0), Some(w));
        let clipped: Vec<ScatterPoint> = full
            .iter()
            .zip(&full_plane)
            .filter(|(_, q)| w.contains(**q))
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(part, clipped, "window {w:?}");
    }
}

#[test]
fn the_count_follows_the_area() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let spacing = 2000.0;
    let ring: Vec<Point> = rings[0]
        .iter()
        .map(|&p| plane.forward(p).unwrap())
        .collect();
    let a = area(&ring);
    let n = scatter(&plane, &rings, &params(spacing), None).len() as f64;
    let low = a / (std::f64::consts::PI * (2.0 * spacing).powi(2));
    let high = a / (spacing * spacing * 3f64.sqrt() / 2.0);
    assert!(low < n && n < high, "{low} < {n} < {high}");
}

#[test]
fn the_scatter_is_deterministic_and_follows_the_seed() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let a = scatter(&plane, &rings, &params(3000.0), None);
    let b = scatter(&plane, &rings, &params(3000.0), None);
    assert_eq!(a, b);
    let other = ScatterParams {
        seed: seed_from(b"6b1f0a52-3c1e-4d2a-9e0f-0c6f1b2a3d4e"),
        ..params(3000.0)
    };
    assert_ne!(
        other.seed,
        seed_from(b"6b1f0a52-3c1e-4d2a-9e0f-0c6f1b2a3d4f")
    );
    let c = scatter(&plane, &rings, &other, None);
    assert!(keys(&a).intersection(&keys(&c)).count() < a.len() / 10);
}

#[test]
fn the_plane_round_trips() {
    let plane = ScatterPlane::for_rings(&forest(), R);
    assert_eq!(plane.center(), [10.0, 45.0]);
    for p in [[10.0, 45.0], [12.5, 44.0], [-170.0, -30.0], [100.0, 89.0]] {
        let q = plane.inverse(plane.forward(p).unwrap());
        assert!(
            (q[0] - p[0]).abs() < 1e-7 && (q[1] - p[1]).abs() < 1e-7,
            "{p:?} {q:?}"
        );
    }
    assert!(plane.forward([-170.0, -45.0]).is_none());
}

#[test]
fn points_keep_the_inset() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let inset = 5000.0;
    let pts = scatter(
        &plane,
        &rings,
        &ScatterParams {
            inset,
            ..params(2000.0)
        },
        None,
    );
    let all = scatter(&plane, &rings, &params(2000.0), None);
    assert!(!pts.is_empty() && pts.len() < all.len());
    let ring: Vec<Point> = rings[0]
        .iter()
        .map(|&p| plane.forward(p).unwrap())
        .collect();
    let n = ring.len();
    for p in plane_points(&plane, &pts) {
        let d = (0..n)
            .map(|k| seg_dist(p, ring[k], ring[(k + 1) % n]))
            .fold(f64::INFINITY, f64::min);
        assert!(d >= inset - 1e-3, "{d}");
    }
}

#[test]
fn a_hole_has_no_points() {
    let c = [10.0, 45.0];
    let rings = vec![blob(c, 150_000.0, 120), blob(c, 60_000.0, 60)];
    let plane = ScatterPlane::for_rings(&rings, R);
    let pts = scatter(&plane, &rings, &params(2000.0), None);
    let hole: Vec<Point> = rings[1]
        .iter()
        .map(|&p| plane.forward(p).unwrap())
        .collect();
    let pts = plane_points(&plane, &pts);
    assert!(pts.len() > 1000);
    assert!(pts.iter().all(|&p| !inside(&hole, p)));
    let solid = scatter(&plane, &rings[..1], &params(2000.0), None);
    assert!(
        plane_points(&plane, &solid)
            .iter()
            .any(|&p| inside(&hole, p))
    );
}

fn inside(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    let mut c = false;
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
    }
    c
}

#[test]
fn works_across_the_antimeridian() {
    let rings = vec![vec![
        [178.0, -2.0],
        [-178.0, -2.0],
        [-178.0, 2.0],
        [178.0, 2.0],
    ]];
    let plane = ScatterPlane::for_rings(&rings, R);
    assert_eq!(plane.center()[1], 0.0);
    assert_eq!(plane.center()[0].abs(), 180.0);
    let pts = scatter(&plane, &rings, &params(5000.0), None);
    assert!(pts.len() > 500, "{}", pts.len());
    assert!(pts.iter().any(|p| p.lonlat[0] > 0.0) && pts.iter().any(|p| p.lonlat[0] < 0.0));
    for p in &pts {
        assert!(
            p.lonlat[0].abs() > 177.9 && p.lonlat[1].abs() < 2.1,
            "{:?}",
            p.lonlat
        );
    }
    let m = min_pair(&plane_points(&plane, &pts));
    assert!(m >= min_distance(&plane, 5000.0) - 1e-6);
}

#[test]
fn works_around_a_pole() {
    let ring: Vec<LonLat> = (0..72).map(|k| [-180.0 + 5.0 * k as f64, 80.0]).collect();
    let rings = vec![ring];
    let plane = ScatterPlane::for_rings(&rings, R);
    assert_eq!(plane.center()[1], 90.0);
    let pts = scatter(&plane, &rings, &params(10_000.0), None);
    assert!(pts.len() > 1000, "{}", pts.len());
    assert!(pts.iter().all(|p| p.lonlat[1] > 79.9));
    assert!(pts.iter().any(|p| p.lonlat[1] > 89.0));
    let m = min_pair(&plane_points(&plane, &pts));
    assert!(m >= min_distance(&plane, 10_000.0) - 1e-6);
}

#[test]
fn the_cost_guard_lowers_the_density() {
    let rings = forest();
    let plane = ScatterPlane::for_rings(&rings, R);
    let p = ScatterParams {
        max_points: 500,
        ..params(500.0)
    };
    let pts = scatter(&plane, &rings, &p, None);
    assert!(!pts.is_empty() && pts.len() <= 500);
    // The points of the coarser level are the same points.
    let all = scatter(&plane, &rings, &params(500.0), None);
    assert!(keys(&pts).is_subset(&keys(&all)));
}

#[test]
fn a_large_forest_is_fast() {
    let rings = vec![blob([10.0, 45.0], 500_000.0, 400)];
    let plane = ScatterPlane::for_rings(&rings, R);
    let p = ScatterParams {
        max_points: 100_000,
        ..params(2000.0)
    };
    let start = Instant::now();
    let pts = scatter(&plane, &rings, &p, None);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("1000 km forest at 2 km: {} points in {ms:.1} ms", pts.len());
    let limit = if cfg!(debug_assertions) { 2000.0 } else { 200.0 };
    assert!(ms < limit, "{ms} ms");
}
