use vale_render::Point;
use vale_render::stroke::{Dab, TaperPiece, brush_dabs, dash, taper, taper_measured, taper_path};

/// The distance from `p` to the segment `a`-`b`, and the distance from `a` to the foot.
fn project(p: Point, a: Point, b: Point) -> (f64, f64) {
    let d = b - a;
    let len2 = d.hypot2();
    let t = if len2 > 0.0 {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let foot = a + d * t;
    (p.distance(foot), t * len2.sqrt())
}

/// The arc length position of a point that lies on the polyline.
fn arc_pos(line: &[Point], p: Point) -> f64 {
    let mut s = 0.0;
    for w in line.windows(2) {
        let (dist, along) = project(p, w[0], w[1]);
        if dist < 1e-9 {
            return s + along;
        }
        s += w[0].distance(w[1]);
    }
    panic!("{p:?} is not on the line");
}

fn length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

#[test]
fn dashes_are_even_along_an_irregular_curve() {
    // An arc with long and very short segments.
    let mut line = Vec::new();
    let mut angle: f64 = 0.0;
    for i in 0..60 {
        line.push(Point::new(50.0 * angle.cos(), 50.0 * angle.sin()));
        angle += if i % 3 == 0 { 0.11 } else { 0.0007 };
    }
    let split = 31;
    let parts = vec![line[..=split].to_vec(), line[split..].to_vec()];
    let total = length(&line);
    let pieces = dash(&parts, 3.0, 2.0);
    assert!(pieces.len() > 10);

    // Each piece as (start, end) on the whole line. A dash cut by the part boundary is
    // joined again, so the phase must carry across the parts.
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut joined = 0;
    for piece in &pieces {
        let (s, e) = (
            arc_pos(&line, piece[0]),
            arc_pos(&line, *piece.last().unwrap()),
        );
        assert!(
            (length(piece) - (e - s)).abs() < 1e-9,
            "a dash skips a vertex"
        );
        match spans.last_mut() {
            Some(last) if (last.1 - s).abs() < 1e-9 => {
                last.1 = e;
                joined += 1;
            }
            _ => spans.push((s, e)),
        }
    }
    assert!(joined <= 1);
    assert!(spans[0].0.abs() < 1e-9);
    for (k, &(s, e)) in spans.iter().enumerate() {
        if k + 1 < spans.len() {
            assert!((e - s - 3.0).abs() < 1e-9, "dash {k} is {}", e - s);
            let gap = spans[k + 1].0 - e;
            assert!((gap - 2.0).abs() < 1e-9, "gap {k} is {gap}");
        } else {
            assert!(e - s <= 3.0 + 1e-9);
            assert!(total - e < 2.0 + 1e-9);
        }
    }
}

#[test]
fn dash_with_bad_lengths_keeps_the_line() {
    let parts = vec![vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]];
    assert_eq!(dash(&parts, 0.0, 2.0), parts);
    assert_eq!(dash(&parts, 3.0, -1.0), parts);
}

fn assert_tangent(c: Point, r: f64, on: Point, other: Point) {
    assert!((on.distance(c) - r).abs() < 1e-9, "not on the circle");
    let edge = other - on;
    let radial = on - c;
    assert!(edge.dot(radial).abs() < 1e-9 * edge.hypot().max(1.0) * r.max(1.0));
}

#[test]
fn taper_widths_at_the_ends_and_tangent_join() {
    let parts = vec![
        vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        vec![Point::new(20.0, 10.0), Point::new(30.0, 10.0)],
    ];
    let pieces = taper(&parts, 6.0, 2.0);
    let circles: Vec<(Point, f64)> = pieces
        .iter()
        .filter_map(|p| match *p {
            TaperPiece::Circle(c, r) => Some((c, r)),
            _ => None,
        })
        .collect();
    assert_eq!(circles.len(), 5);
    assert!((circles[0].1 - 3.0).abs() < 1e-12);
    assert!((circles[4].1 - 1.0).abs() < 1e-12);
    // The width runs on across the parts: 10 of 30 at the second vertex.
    assert!((circles[1].1 - (3.0 - 2.0 / 3.0)).abs() < 1e-12);
    assert!((circles[3].1 - (3.0 - 4.0 / 3.0)).abs() < 1e-12);

    let mut quads = 0;
    for (i, piece) in pieces.iter().enumerate() {
        let TaperPiece::Quad(q) = *piece else {
            continue;
        };
        quads += 1;
        let (TaperPiece::Circle(c1, r1), TaperPiece::Circle(c2, r2)) =
            (pieces[i - 1], pieces[i + 1])
        else {
            panic!("a quad is not between two circles");
        };
        assert_tangent(c1, r1, q[0], q[1]);
        assert_tangent(c2, r2, q[1], q[0]);
        assert_tangent(c2, r2, q[2], q[3]);
        assert_tangent(c1, r1, q[3], q[2]);
    }
    assert_eq!(quads, 3);
    assert!(!taper_path(&pieces).is_empty());

    // A small circle inside a large one has no hull.
    let inside = taper_measured(
        &[vec![
            (Point::new(0.0, 0.0), 0.0),
            (Point::new(1.0, 0.0), 1.0),
        ]],
        10.0,
        1.0,
    );
    assert_eq!(inside.len(), 2);
}

fn distance_to_line(parts: &[Vec<Point>], p: Point) -> f64 {
    parts
        .iter()
        .flat_map(|part| part.windows(2).map(move |w| project(p, w[0], w[1]).0))
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn brush_is_deterministic_and_inside_the_width() {
    let parts = vec![
        vec![
            Point::new(0.0, 0.0),
            Point::new(40.0, 5.0),
            Point::new(60.0, 30.0),
        ],
        vec![Point::new(70.0, 30.0), Point::new(90.0, 10.0)],
    ];
    let width = 4.0;
    let a = brush_dabs(&parts, width, 7);
    assert_eq!(a, brush_dabs(&parts, width, 7));
    assert_ne!(a, brush_dabs(&parts, width, 8));
    assert_eq!(a.len() % 3, 0);
    assert!(a.len() >= 3 * 50);
    for Dab {
        center,
        radius,
        alpha,
    } in &a
    {
        assert!(distance_to_line(&parts, *center) <= 0.35 * width + 1e-9);
        assert!((0.18 * width..=0.30 * width).contains(radius));
        assert!((0.35..=0.75).contains(alpha));
    }

    // A long line gets no more than the cap.
    let long = vec![vec![Point::new(0.0, 0.0), Point::new(1.0e6, 0.0)]];
    assert!(brush_dabs(&long, 1.0, 1).len() <= vale_render::stroke::MAX_DABS);
}
