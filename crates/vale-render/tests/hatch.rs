use vale_render::Point;
use vale_render::hatch::{hatch_segments, offset_ring};

fn pts(v: &[(f64, f64)]) -> Vec<Point> {
    v.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

/// Even-odd point in polygon test.
fn inside(rings: &[Vec<Point>], p: Point) -> bool {
    let mut c = false;
    for r in rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
    }
    c
}

fn mid(s: &(Point, Point)) -> Point {
    s.0.midpoint(s.1)
}

#[test]
fn square_at_zero_degrees_gives_horizontal_segments() {
    let rings = vec![pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)])];
    let segs = hatch_segments(&rings, 0.0, 2.0, Point::new(0.0, 1.0));
    assert_eq!(segs.len(), 5);
    for (a, b) in &segs {
        assert!((a.y - b.y).abs() < 1e-9);
        assert!((a.x.min(b.x)).abs() < 1e-9 && (a.x.max(b.x) - 10.0).abs() < 1e-9);
        assert!(a.y > 0.0 && a.y < 10.0);
    }
}

#[test]
fn vertex_on_a_line_counts_once() {
    // A diamond whose side vertices lie on the line y = 0.
    let rings = vec![pts(&[(0.0, -5.0), (5.0, 0.0), (0.0, 5.0), (-5.0, 0.0)])];
    let segs = hatch_segments(&rings, 0.0, 5.0, Point::ORIGIN);
    let at_zero: Vec<_> = segs.iter().filter(|s| s.0.y.abs() < 1e-9).collect();
    assert_eq!(at_zero.len(), 1);
    assert!((at_zero[0].0.distance(at_zero[0].1) - 10.0).abs() < 1e-9);
}

#[test]
fn concave_star_segments_are_inside() {
    let mut star = Vec::new();
    for i in 0..10 {
        let a = i as f64 * std::f64::consts::PI / 5.0;
        let r = if i % 2 == 0 { 10.0 } else { 4.0 };
        star.push(Point::new(r * a.cos(), r * a.sin()));
    }
    let rings = vec![star];
    for angle in [0.0, 30.0, 45.0, 72.0, 90.0, 135.0] {
        let segs = hatch_segments(&rings, angle, 0.7, Point::new(0.3, 0.1));
        assert!(segs.len() > 10);
        for s in &segs {
            assert!(inside(&rings, mid(s)), "angle {angle}: {s:?}");
        }
    }
}

#[test]
fn hole_stays_empty() {
    let rings = vec![
        pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        pts(&[(3.0, 3.0), (3.0, 7.0), (7.0, 7.0), (7.0, 3.0)]),
    ];
    let segs = hatch_segments(&rings, 0.0, 1.0, Point::new(0.0, 0.5));
    for s in &segs {
        let m = mid(s);
        assert!(inside(&rings, m));
        if m.y > 3.0 && m.y < 7.0 {
            assert!(s.0.x.max(s.1.x) <= 3.0 + 1e-9 || s.0.x.min(s.1.x) >= 7.0 - 1e-9);
        }
    }
    let through_hole = segs.iter().filter(|s| s.0.y == 5.5).count();
    assert_eq!(through_hole, 2);
}

#[test]
fn ninety_degrees_gives_vertical_lines() {
    let rings = vec![pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)])];
    let segs = hatch_segments(&rings, 90.0, 2.5, Point::new(1.0, 0.0));
    assert_eq!(segs.len(), 4);
    for (a, b) in &segs {
        assert!((a.x - b.x).abs() < 1e-9);
        assert!((a.distance(*b) - 10.0).abs() < 1e-9);
    }
}

#[test]
fn zero_spacing_is_empty() {
    let rings = vec![pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)])];
    assert!(hatch_segments(&rings, 0.0, 0.0, Point::ORIGIN).is_empty());
    assert!(hatch_segments(&rings, 0.0, -1.0, Point::ORIGIN).is_empty());
}

#[test]
fn offset_unit_square_by_one_for_both_windings() {
    let ccw = pts(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    let cw: Vec<Point> = ccw.iter().rev().copied().collect();
    for ring in [ccw, cw] {
        let out = offset_ring(&ring, 1.0);
        assert_eq!(out.len(), 4);
        let (xs, ys): (Vec<f64>, Vec<f64>) = out.iter().map(|p| (p.x, p.y)).unzip();
        let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!((min(&xs) + 1.0).abs() < 1e-9 && (max(&xs) - 2.0).abs() < 1e-9);
        assert!((min(&ys) + 1.0).abs() < 1e-9 && (max(&ys) - 2.0).abs() < 1e-9);
        for p in &out {
            assert!((p.x + 1.0).abs() < 1e-9 || (p.x - 2.0).abs() < 1e-9);
            assert!((p.y + 1.0).abs() < 1e-9 || (p.y - 2.0).abs() < 1e-9);
        }
    }
}
