use std::f64::consts::PI;
use vale_labeler::*;

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    let path = format!(
        "{}/../../assets/fonts/NotoSans-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    f.register(std::fs::read(path).unwrap()).unwrap();
    f
}

fn run(pp: PolygonPlacement, rings: Vec<Vec<Point>>, text: &str, bounds: Rect) -> Labeling {
    let input = LabelInput {
        bounds,
        classes: vec![LabelClass {
            name: "area".into(),
            style: TextStyle::new("Noto Sans", 14.0),
            priority: 0,
            margin: 0.0,
            placement: Placement::Polygon(pp),
            duplicate_distance: None,
        }],
        features: vec![Feature {
            id: 1,
            class: 0,
            text: text.into(),
            priority: 0.0,
            geometry: Geometry::Polygon { rings },
        }],
        obstacles: vec![],
        options: Options::default(),
    };
    place_labels(&mut fonts(), &input)
}

/// A band that bends through 140 degrees, 60 units wide.
fn band() -> Vec<Point> {
    let c = Point::new(300.0, 420.0);
    let at = |r: f64, a: f64| Point::new(c.x + r * a.cos(), c.y + r * a.sin());
    let n = 40;
    let angle = |k: usize| (200.0 + 140.0 * k as f64 / n as f64) * PI / 180.0;
    let mut ring: Vec<Point> = (0..=n).map(|k| at(330.0, angle(k))).collect();
    ring.extend((0..=n).rev().map(|k| at(270.0, angle(k))));
    ring
}

/// Even-odd point in polygon.
fn contains(rings: &[Vec<Point>], p: Point) -> bool {
    let mut inside = false;
    for ring in rings {
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            if (a.y <= p.y) != (b.y <= p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

fn glyph_angles(label: &PlacedLabel) -> Vec<f64> {
    label
        .glyph_runs
        .iter()
        .flat_map(|r| &r.glyphs)
        .map(|g| {
            let c = g.transform.as_coeffs();
            c[1].atan2(c[0])
        })
        .collect()
}

#[test]
fn long_curved_polygon_gets_curved_spread_label() {
    let rings = vec![band()];
    let text = "Curved Sea";
    let out = run(
        PolygonPlacement::default(),
        rings.clone(),
        text,
        Rect::new(0.0, 0.0, 600.0, 400.0),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    let label = &out.placed[0];
    assert_eq!(label.kind, CandidateKind::Polygon { curved: true });

    let angles = glyph_angles(label);
    let spread = angles.iter().cloned().fold(f64::MIN, f64::max)
        - angles.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread > 0.5, "the text turns by {spread} radians");

    let advance = fonts()
        .measure(text, &TextStyle::new("Noto Sans", 14.0))
        .advance;
    let glyphs: Vec<_> = label.glyph_runs.iter().flat_map(|r| &r.glyphs).collect();
    let first = glyphs[0].transform.translation().to_point();
    let last = glyphs[glyphs.len() - 1].transform.translation().to_point();
    assert!(
        first.distance(last) > 1.5 * advance,
        "the letters span {} for an advance of {advance}",
        first.distance(last)
    );
    assert!(first.x < last.x, "the label reads left to right");
    for b in &label.boxes {
        for p in b.corners() {
            assert!(contains(&rings, p), "{p:?} is outside the band");
        }
    }
}

#[test]
fn compact_polygon_gets_horizontal_label_at_visual_center() {
    // A right triangle: the visual center is the incenter, near (170, 210), far from the
    // center of the bounding box at (220, 160).
    let rings = vec![vec![
        Point::new(100.0, 40.0),
        Point::new(100.0, 280.0),
        Point::new(340.0, 280.0),
    ]];
    let out = run(
        PolygonPlacement::default(),
        rings.clone(),
        "Land",
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    let label = &out.placed[0];
    assert_eq!(label.kind, CandidateKind::Polygon { curved: false });
    assert!(glyph_angles(label).iter().all(|a| a.abs() < 1e-9));
    let c = label.bounds.center();
    assert!(
        c.distance(Point::new(170.3, 209.7)) < 15.0,
        "the label center is {c:?}"
    );
    for b in &label.boxes {
        for p in b.corners() {
            assert!(contains(&rings, p));
        }
    }
}

#[test]
fn label_stays_in_the_visible_part_of_a_cut_polygon() {
    // Most of the shape is left of the frame.
    let rings = vec![vec![
        Point::new(-600.0, 60.0),
        Point::new(160.0, 60.0),
        Point::new(160.0, 240.0),
        Point::new(-600.0, 240.0),
    ]];
    let out = run(
        PolygonPlacement::default(),
        rings,
        "Cut",
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    let b = out.placed[0].bounds;
    assert!(b.x0 >= 0.0 && b.x1 <= 160.0 && b.y0 >= 60.0 && b.y1 <= 240.0);
    assert!((b.center().x - 80.0).abs() < 20.0, "{b:?}");
}

#[test]
fn label_goes_in_the_largest_part() {
    let square = |x: f64, y: f64, s: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + s, y),
            Point::new(x + s, y + s),
            Point::new(x, y + s),
        ]
    };
    let out = run(
        PolygonPlacement::default(),
        vec![square(20.0, 20.0, 60.0), square(200.0, 100.0, 150.0)],
        "Isles",
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    let c = out.placed[0].bounds.center();
    assert!(c.x > 200.0 && c.y > 100.0, "{c:?}");
}

#[test]
fn small_polygon_is_reported() {
    let rings = vec![vec![
        Point::new(10.0, 10.0),
        Point::new(30.0, 10.0),
        Point::new(30.0, 18.0),
    ]];
    let out = run(
        PolygonPlacement::default(),
        rings,
        "Tiny island",
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::AreaTooSmall);

    let outside = vec![vec![
        Point::new(500.0, 10.0),
        Point::new(600.0, 10.0),
        Point::new(600.0, 80.0),
    ]];
    let out = run(
        PolygonPlacement::default(),
        outside,
        "Away",
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    assert_eq!(out.unplaced[0].reason, UnplacedReason::OutOfBounds);
}

#[test]
fn horizontal_only_never_curves() {
    let pp = PolygonPlacement {
        mode: PolygonMode::Horizontal,
        ..PolygonPlacement::default()
    };
    let out = run(pp, vec![band()], "Sea", Rect::new(0.0, 0.0, 600.0, 400.0));
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    assert_eq!(out.placed[0].kind, CandidateKind::Polygon { curved: false });
}

#[test]
fn polygon_labels_are_deterministic() {
    let bounds = Rect::new(0.0, 0.0, 600.0, 400.0);
    let a = run(
        PolygonPlacement::default(),
        vec![band()],
        "Curved Sea",
        bounds,
    );
    let b = run(
        PolygonPlacement::default(),
        vec![band()],
        "Curved Sea",
        bounds,
    );
    assert_eq!(a.placed[0].boxes, b.placed[0].boxes);
    assert_eq!(a.total_cost, b.total_cost);
}

fn triangle() -> Vec<Vec<Point>> {
    vec![vec![
        Point::new(100.0, 40.0),
        Point::new(100.0, 280.0),
        Point::new(340.0, 280.0),
    ]]
}

/// The distance from the first glyph to the last glyph.
fn glyph_span(label: &PlacedLabel) -> f64 {
    let glyphs: Vec<_> = label.glyph_runs.iter().flat_map(|r| &r.glyphs).collect();
    let first = glyphs[0].transform.translation().to_point();
    let last = glyphs[glyphs.len() - 1].transform.translation().to_point();
    first.distance(last)
}

#[test]
fn curved_mode_curves_a_compact_shape() {
    let pp = PolygonPlacement {
        mode: PolygonMode::Curved,
        ..PolygonPlacement::default()
    };
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let out = run(pp, triangle(), "Land", bounds);
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    assert_eq!(out.placed[0].kind, CandidateKind::Polygon { curved: true });
    for b in &out.placed[0].boxes {
        for p in b.corners() {
            assert!(contains(&triangle(), p));
        }
    }
    let auto = run(PolygonPlacement::default(), triangle(), "Land", bounds);
    assert_eq!(
        auto.placed[0].kind,
        CandidateKind::Polygon { curved: false }
    );
}

#[test]
fn horizontal_labels_spread_their_letters() {
    let rings = vec![vec![
        Point::new(60.0, 100.0),
        Point::new(340.0, 100.0),
        Point::new(340.0, 220.0),
        Point::new(60.0, 220.0),
    ]];
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let advance = fonts()
        .measure("Land", &TextStyle::new("Noto Sans", 14.0))
        .advance;
    let pp = PolygonPlacement {
        mode: PolygonMode::Horizontal,
        ..PolygonPlacement::default()
    };
    let spread = run(pp.clone(), rings.clone(), "Land", bounds);
    let label = &spread.placed[0];
    assert!(glyph_angles(label).iter().all(|a| a.abs() < 1e-9));
    assert!(glyph_span(label) > 1.5 * advance, "{}", glyph_span(label));
    assert!((label.bounds.center().x - 200.0).abs() < 10.0);

    let tight = PolygonPlacement {
        max_spread: 0.0,
        ..pp
    };
    let out = run(tight, rings, "Land", bounds);
    assert!(glyph_span(&out.placed[0]) < advance);
}
