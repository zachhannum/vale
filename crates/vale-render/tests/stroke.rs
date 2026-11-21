use kurbo::BezPath;
use vale_render::stroke::DOT;
use vale_render::stroke::{
    Dab, TaperPiece, brush_dabs, dash, sampled, taper, taper_measured, taper_path, varying,
};
use vale_render::{Color, DisplayList, Item, Point, render_rgba};

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
    let pieces = dash(&parts, &[3.0, 2.0]);
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
    for bad in [
        &[3.0][..],
        &[3.0, 2.0, 1.0],
        &[-1.0, 2.0],
        &[3.0, 0.0],
        &[f64::NAN, 2.0],
        &[1.0; 10],
    ] {
        assert_eq!(dash(&parts, bad), parts, "{bad:?}");
    }
}

#[test]
fn a_dash_dot_pattern_cuts_dashes_and_dots() {
    let parts = vec![vec![Point::new(0.0, 5.0), Point::new(30.0, 5.0)]];
    let pieces = dash(&parts, &[4.0, 2.0, 0.0, 2.0]);
    let spans: Vec<(f64, f64)> = pieces
        .iter()
        .map(|p| {
            assert!(p.iter().all(|q| q.y == 5.0), "a piece leaves the line");
            (p[0].x, p[p.len() - 1].x)
        })
        .collect();
    let want = [
        (0.0, 4.0),
        (6.0, 6.0),
        (8.0, 12.0),
        (14.0, 14.0),
        (16.0, 20.0),
        (22.0, 22.0),
        (24.0, 28.0),
        (30.0, 30.0),
    ];
    assert_eq!(spans.len(), want.len(), "{spans:?}");
    for ((s, e), (ws, we)) in spans.iter().zip(want) {
        if ws == we {
            assert!((s - (ws - DOT / 2.0)).abs() < 1e-9 && (e - (we + DOT / 2.0)).abs() < 1e-9);
        } else {
            assert!((s - ws).abs() < 1e-9 && (e - we).abs() < 1e-9, "{s} {e}");
        }
    }
}

#[test]
fn a_dot_follows_the_direction_of_the_line() {
    let parts = vec![vec![Point::new(0.0, 0.0), Point::new(0.0, 10.0)]];
    let pieces = dash(&parts, &[0.0, 3.0]);
    assert_eq!(pieces.len(), 4);
    for p in &pieces {
        assert_eq!(p.len(), 2);
        assert!((p[1] - p[0] - kurbo::Vec2::new(0.0, DOT)).hypot() < 1e-12);
    }
}

#[test]
fn a_round_dot_draws_a_disk() {
    let mut list = DisplayList::new(40.0, 20.0);
    let mut path = BezPath::new();
    for piece in dash(
        &[vec![Point::new(5.0, 10.0), Point::new(35.0, 10.0)]],
        &[0.0, 10.0],
    ) {
        path.move_to(piece[0]);
        path.line_to(piece[1]);
    }
    list.push(Item::Stroke {
        path,
        color: Color::rgb(0, 0, 255),
        width: 6.0,
        round: true,
    });
    let img = render_rgba(&list, 1.0).unwrap();
    let alpha = |x: usize, y: usize| img.data[(y * img.width + x) * 4 + 3];
    for x in [5, 15, 25] {
        assert!(alpha(x, 10) > 200, "no dot at {x}");
        assert!(alpha(x, 8) > 100, "the dot at {x} is not round");
    }
    assert_eq!(alpha(10, 10), 0, "a gap is drawn");
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

#[test]
fn varying_with_linear_widths_is_the_taper() {
    let part = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    ];
    let measured: Vec<(Point, f64)> = vec![(part[0], 0.0), (part[1], 0.5), (part[2], 1.0)];
    let widths: Vec<(Point, f64)> = vec![(part[0], 2.0), (part[1], 4.0), (part[2], 6.0)];
    assert_eq!(taper_measured(&[measured], 2.0, 6.0), varying(&[widths]));
}

#[test]
fn sampled_cuts_long_segments_and_takes_the_nearest_width() {
    let part = vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)];
    let samples: Vec<(Point, f64)> = (0..=10)
        .map(|i| {
            let x = 10.0 * i as f64;
            (Point::new(x, 0.0), if x < 50.0 { 1.0 } else { 5.0 })
        })
        .collect();
    let out = sampled(&[part], &samples);
    assert_eq!(out.len(), 1);
    let points = &out[0];
    assert!(points.len() > 10, "{}", points.len());
    for &(p, w) in points {
        let expected = if p.x < 45.0 {
            1.0
        } else if p.x > 50.0 {
            5.0
        } else {
            continue;
        };
        assert_eq!(w, expected, "{p:?}");
    }
    assert!(points.windows(2).all(|w| w[1].0.x > w[0].0.x));
    // With no samples, the width is 0.
    let none = sampled(&[vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]], &[]);
    assert!(none[0].iter().all(|&(_, w)| w == 0.0));
}
