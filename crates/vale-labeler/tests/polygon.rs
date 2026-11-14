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
            fallbacks: Default::default(),
        }],
        features: vec![Feature {
            id: 1,
            class: 0,
            text: text.into(),
            priority: 0.0,
            geometry: Geometry::Polygon { rings },
        }],
        obstacles: vec![],
        parallels: Vec::new(),
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

#[test]
fn curved_mode_falls_back_to_horizontal() {
    // The band bends more than the curve limit allows anywhere.
    let pp = PolygonPlacement {
        mode: PolygonMode::Curved,
        max_glyph_turn: 0.001,
        ..PolygonPlacement::default()
    };
    let out = run(pp, vec![band()], "Sea", Rect::new(0.0, 0.0, 600.0, 400.0));
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    assert_eq!(out.placed[0].kind, CandidateKind::Polygon { curved: false });
}

/// A square ring from `(x0, y0)` with side `side`.
fn square(x0: f64, y0: f64, side: f64) -> Vec<Point> {
    vec![
        Point::new(x0, y0),
        Point::new(x0 + side, y0),
        Point::new(x0 + side, y0 + side),
        Point::new(x0, y0 + side),
    ]
}

/// Labels of polygon features, one for each item of `shapes`, all with the class `pp`.
fn run_all(pp: PolygonPlacement, shapes: Vec<Vec<Vec<Point>>>, text: &str) -> Labeling {
    let input = LabelInput {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        classes: vec![LabelClass {
            name: "area".into(),
            style: TextStyle::new("Noto Sans", 14.0),
            priority: 0,
            margin: 0.0,
            placement: Placement::Polygon(pp),
            duplicate_distance: None,
            fallbacks: Default::default(),
        }],
        features: shapes
            .into_iter()
            .enumerate()
            .map(|(i, rings)| Feature {
                id: i as u64 + 1,
                class: 0,
                text: text.into(),
                priority: 0.0,
                geometry: Geometry::Polygon { rings },
            })
            .collect(),
        obstacles: vec![],
        parallels: Vec::new(),
        options: Options::default(),
    };
    place_labels(&mut fonts(), &input)
}

fn horizontal() -> PolygonPlacement {
    PolygonPlacement {
        mode: PolygonMode::Horizontal,
        max_spread: 0.0,
        ..PolygonPlacement::default()
    }
}

/// The ranks of the zones inside, with `best` at rank 1 and the others at rank 2.
fn prefer(best: Zone) -> PolygonZones {
    let mut zones = PolygonZones::default();
    for z in Zone::INSIDE {
        zones.inside[z as usize] = if z == best { 1 } else { 2 };
    }
    zones
}

#[test]
fn the_best_zone_inside_gets_the_label() {
    let rings = vec![square(100.0, 100.0, 300.0)];
    for (zone, check) in [
        (Zone::TopLeft, (true, true)),
        (Zone::BottomRight, (false, false)),
    ] {
        let pp = PolygonPlacement {
            zones: prefer(zone),
            ..horizontal()
        };
        let out = run_all(pp, vec![rings.clone()], "Land");
        let c = out.placed[0].bounds.center();
        assert_eq!((c.x < 200.0, c.y < 200.0), check, "{zone:?}: {c:?}");
        assert!(out.placed[0].leader.is_none());
    }
}

#[test]
fn a_blocked_zone_gets_no_label() {
    let rings = vec![square(100.0, 100.0, 300.0)];
    let mut zones = PolygonZones::default();
    zones.inside[Zone::Center as usize] = 0;
    let pp = PolygonPlacement {
        zones,
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Land");
    let c = out.placed[0].bounds.center();
    assert!(c.distance(Point::new(250.0, 250.0)) > 40.0, "{c:?}");

    // With every zone inside blocked, and one outside, the label goes outside.
    let mut zones = PolygonZones {
        inside: [0; 9],
        outside: [0; 8],
    };
    zones.outside[Zone::OUTSIDE
        .iter()
        .position(|z| *z == Zone::Right)
        .unwrap()] = 1;
    let pp = PolygonPlacement {
        zones,
        ..horizontal()
    };
    let out = run_all(pp, vec![vec![square(100.0, 100.0, 300.0)]], "Land");
    let label = &out.placed[0];
    assert!(label.bounds.x0 > 400.0, "{:?}", label.bounds);
    assert!(label.leader.is_some());
}

#[test]
fn a_small_polygon_gets_a_label_outside_in_the_best_zone() {
    let rings = vec![square(300.0, 300.0, 10.0)];
    let out = run_all(horizontal(), vec![rings.clone()], "Island");
    assert_eq!(out.unplaced[0].reason, UnplacedReason::AreaTooSmall);

    let mut zones = PolygonZones {
        outside: [2; 8],
        ..PolygonZones::default()
    };
    zones.outside[Zone::OUTSIDE
        .iter()
        .position(|z| *z == Zone::TopRight)
        .unwrap()] = 1;
    let pp = PolygonPlacement {
        zones,
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Island");
    let label = &out.placed[0];
    assert!(
        label.bounds.x0 > 305.0 && label.bounds.y1 < 305.0,
        "{:?}",
        label.bounds
    );
}

#[test]
fn the_leader_starts_at_the_anchor_of_the_class() {
    // A C shape: its centroid is in the gap, outside the shape.
    let ring = vec![
        Point::new(100.0, 100.0),
        Point::new(300.0, 100.0),
        Point::new(300.0, 140.0),
        Point::new(140.0, 140.0),
        Point::new(140.0, 260.0),
        Point::new(300.0, 260.0),
        Point::new(300.0, 300.0),
        Point::new(100.0, 300.0),
    ];
    let mut zones = PolygonZones {
        inside: [0; 9],
        outside: [0; 8],
    };
    zones.outside[Zone::OUTSIDE
        .iter()
        .position(|z| *z == Zone::Right)
        .unwrap()] = 1;
    let leader = |anchor| {
        let pp = PolygonPlacement {
            zones: zones.clone(),
            anchor,
            ..horizontal()
        };
        let out = run_all(pp, vec![vec![ring.clone()]], "Bay");
        out.placed[0].leader.expect("a leader")
    };
    let rings = [ring.clone()];
    let center = leader(LeaderAnchor::Center);
    assert!(!contains(&rings, center.anchor), "{center:?}");
    assert!(
        center.anchor.distance(Point::new(180.0, 200.0)) < 20.0,
        "{center:?}"
    );

    let inside = leader(LeaderAnchor::Inside);
    assert!(contains(&rings, inside.anchor), "{inside:?}");

    let outline = leader(LeaderAnchor::Outline);
    let on_edge = ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| {
        let ab = *b - *a;
        let t = ((outline.anchor - *a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
        (*a + ab * t).distance(outline.anchor) < 1e-6
    });
    assert!(on_edge, "{outline:?}");
    // The leader ends on the edge of the label.
    let label = run_all(
        PolygonPlacement {
            zones: zones.clone(),
            anchor: LeaderAnchor::Outline,
            ..horizontal()
        },
        vec![vec![ring.clone()]],
        "Bay",
    );
    let b = label.placed[0].bounds;
    assert!(outline.end.x >= b.x0 - 1.0 && outline.end.x <= b.x1 + 1.0);
}

/// A lake with an island in the middle, 200 wide and 50 high.
fn lake() -> Vec<Vec<Point>> {
    vec![
        square(100.0, 100.0, 300.0),
        vec![
            Point::new(150.0, 225.0),
            Point::new(350.0, 225.0),
            Point::new(350.0, 275.0),
            Point::new(150.0, 275.0),
        ],
    ]
}

#[test]
fn a_label_avoids_holes_unless_the_class_allows_them() {
    let rings = lake();
    let island = Rect::new(150.0, 225.0, 350.0, 275.0);
    let out = run_all(horizontal(), vec![rings.clone()], "Lake");
    assert!(
        out.placed[0].bounds.intersect(island).area() <= 0.0,
        "{:?}",
        out.placed[0].bounds
    );

    let pp = PolygonPlacement {
        over_holes: true,
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Lake");
    let c = out.placed[0].bounds.center();
    assert!(island.contains(c), "{c:?}");
}

#[test]
fn a_label_can_avoid_other_polygons() {
    let lake = vec![lake()[1].clone()];
    let land = vec![square(100.0, 100.0, 300.0)];
    let island = Rect::new(150.0, 225.0, 350.0, 275.0);
    let label_of = |pp: PolygonPlacement| {
        let mut out = run_all(pp, vec![land.clone(), lake.clone()], "Land");
        out.placed.retain(|l| l.feature == 1);
        out.placed[0].bounds
    };
    assert!(label_of(horizontal()).intersect(island).area() > 0.0);
    let pp = PolygonPlacement {
        avoid_other_polygons: true,
        ..horizontal()
    };
    let b = label_of(pp);
    assert!(b.intersect(island).area() <= 0.0, "{b:?}");
}

#[test]
fn the_class_labels_the_largest_part_or_each_part() {
    let rings = vec![
        square(100.0, 100.0, 200.0),
        square(400.0, 100.0, 120.0),
        square(600.0, 100.0, 5.0),
    ];
    let out = run_all(horizontal(), vec![rings.clone()], "Isles");
    assert_eq!(out.placed.len(), 1);
    assert!(out.placed[0].bounds.center().x < 300.0);

    let pp = PolygonPlacement {
        largest_part_only: false,
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Isles");
    // The smallest part has no room, and it is not reported.
    assert_eq!(out.placed.len(), 2, "{:?}", out.unplaced);
    assert!(out.unplaced.is_empty());
    assert!(out.placed.iter().any(|l| l.bounds.center().x > 400.0));
}

#[test]
fn a_large_polygon_repeats_its_label() {
    let rings = vec![square(20.0, 20.0, 560.0)];
    let out = run_all(horizontal(), vec![rings.clone()], "Sea");
    assert_eq!(out.placed.len(), 1);

    let pp = PolygonPlacement {
        repeat_distance: Some(200.0),
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Sea");
    assert!(out.placed.len() >= 4, "{}", out.placed.len());
    for a in &out.placed {
        for b in &out.placed {
            if a.repeat != b.repeat {
                assert!(a.bounds.center().distance(b.bounds.center()) > 100.0);
            }
        }
    }
}

#[test]
fn a_label_can_go_past_the_outline_by_the_overrun() {
    // A strip a little lower than the text.
    let rings = vec![vec![
        Point::new(100.0, 100.0),
        Point::new(400.0, 100.0),
        Point::new(400.0, 109.0),
        Point::new(100.0, 109.0),
    ]];
    let out = run_all(horizontal(), vec![rings.clone()], "Strip");
    assert!(out.placed.is_empty());

    let pp = PolygonPlacement {
        overrun: 6.0,
        ..horizontal()
    };
    let out = run_all(pp, vec![rings], "Strip");
    let b = out.placed[0].bounds;
    assert!(b.y0 >= 94.0 && b.y1 <= 115.0, "{b:?}");
}

#[test]
fn a_polygon_below_the_minimum_area_gets_no_label() {
    let rings = vec![square(100.0, 100.0, 100.0)];
    let pp = PolygonPlacement {
        min_area: 12_000.0,
        ..horizontal()
    };
    let out = run_all(pp.clone(), vec![rings.clone()], "Land");
    assert_eq!(out.unplaced[0].reason, UnplacedReason::BelowMinimumArea);
    let pp = PolygonPlacement {
        min_area: 8_000.0,
        ..pp
    };
    assert_eq!(run_all(pp, vec![rings], "Land").placed.len(), 1);
}
