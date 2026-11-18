use std::f64::consts::{FRAC_PI_6, PI};
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

fn class(pp: PointPlacement) -> LabelClass {
    LabelClass {
        name: "p".into(),
        style: TextStyle::new("Noto Sans", 14.0),
        priority: 0,
        margin: 0.0,
        placement: Placement::Point(pp),
        duplicate_distance: None,
        fallbacks: Fallbacks {
            order: Vec::new(),
            ..Default::default()
        },
    }
}

fn point(id: u64, class: usize, text: &str, x: f64, y: f64) -> Feature {
    Feature {
        id,
        class,
        text: text.into(),
        priority: 0.0,
        geometry: Geometry::Point {
            position: Point::new(x, y),
            symbol_radius: 2.0,
            angle: None,
            outline: None,
        },
    }
}

fn turned(angle: f64) -> Feature {
    let mut f = point(1, 0, "Paris", 400.0, 300.0);
    if let Geometry::Point { angle: a, .. } = &mut f.geometry {
        *a = Some(angle);
    }
    f
}

fn hard(rect: Rect) -> Obstacle {
    Obstacle {
        rect,
        kind: ObstacleKind::Hard,
    }
}

fn run(classes: Vec<LabelClass>, features: Vec<Feature>, obstacles: Vec<Obstacle>) -> Labeling {
    let inp = LabelInput {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        classes,
        features,
        obstacles,
        parallels: Vec::new(),
        options: Options::default(),
    };
    place_labels(&mut fonts(), &inp)
}

fn run_one(pp: PointPlacement, obstacles: Vec<Obstacle>) -> Labeling {
    run(
        vec![class(pp)],
        vec![point(1, 0, "Paris", 400.0, 300.0)],
        obstacles,
    )
}

fn first(l: &PlacedLabel) -> Affine {
    l.glyph_runs[0].glyphs[0].transform
}

/// The boxes, costs, and glyph transforms of the placed labels.
fn placement(out: &Labeling) -> Vec<(Vec<OrientedBox>, f64, Vec<Affine>)> {
    out.placed
        .iter()
        .map(|l| {
            let glyphs = l.glyph_runs.iter().flat_map(|r| &r.glyphs);
            (
                l.boxes.clone(),
                l.cost,
                glyphs.map(|g| g.transform).collect(),
            )
        })
        .collect()
}

fn rot(a: &Affine) -> f64 {
    let c = a.as_coeffs();
    c[1].atan2(c[0])
}

fn wrap(a: f64) -> f64 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

fn only(positions: &[PointPosition]) -> PointPlacement {
    PointPlacement {
        positions: positions.to_vec(),
        ..Default::default()
    }
}

#[test]
fn a_blocked_position_is_never_used() {
    let pp = only(&[PointPosition::Left, PointPosition::Bottom]);
    let out = run_one(pp.clone(), vec![]);
    assert_eq!(
        out.placed[0].kind,
        CandidateKind::Point(PointPosition::Left)
    );

    let left = hard(Rect::new(300.0, 290.0, 398.0, 302.0));
    let out = run_one(pp.clone(), vec![left]);
    assert_eq!(
        out.placed[0].kind,
        CandidateKind::Point(PointPosition::Bottom)
    );

    // The free positions to the right and above are blocked.
    let below = hard(Rect::new(300.0, 302.0, 500.0, 400.0));
    let out = run_one(pp, vec![left, below]);
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::Obstructed);
}

#[test]
fn a_crowded_label_moves_out_to_the_largest_offset() {
    let pp = |max_offset| PointPlacement {
        positions: vec![PointPosition::Right],
        offset: 2.0,
        max_offset,
        ..Default::default()
    };
    // With room, the label keeps the preferred offset.
    let free = run_one(pp(None), vec![]);
    assert_eq!(
        placement(&run_one(pp(Some(20.0)), vec![])),
        placement(&free)
    );
    assert_eq!(placement(&run_one(pp(Some(2.0)), vec![])), placement(&free));

    let strip = hard(Rect::new(404.0, 250.0, 410.0, 350.0));
    let out = run_one(pp(None), vec![strip]);
    assert!(out.placed.is_empty());
    let out = run_one(pp(Some(20.0)), vec![strip]);
    let l = &out.placed[0];
    assert_eq!(l.kind, CandidateKind::Point(PointPosition::Right));
    let x = first(l).as_coeffs()[4];
    assert!(l.bounds.x0 >= 410.0, "{:?}", l.bounds);
    assert!(x <= 400.0 + 2.0 + 20.0 + 1e-9, "{x}");
    assert!(l.cost > free.placed[0].cost);
}

#[test]
fn a_centered_label_shifts_a_little_from_the_center() {
    let pp = |center_shift| PointPlacement {
        positions: vec![PointPosition::Center],
        center_shift,
        ..Default::default()
    };
    let free = run_one(pp(0.0), vec![]);
    let l = &free.placed[0];
    assert_eq!(l.kind, CandidateKind::Point(PointPosition::Center));
    assert!(l.bounds.contains(Point::new(400.0, 300.0)));
    assert!((l.bounds.center().x - 400.0).abs() < 2.0, "{:?}", l.bounds);
    let x0 = first(l).translation();

    let b = l.bounds;
    let right = hard(Rect::new(b.x1 - 2.0, b.y0 - 20.0, b.x1 + 20.0, b.y1 + 20.0));
    let out = run_one(pp(0.0), vec![right]);
    assert!(out.placed.is_empty());
    let out = run_one(pp(6.0), vec![right]);
    let shift = first(&out.placed[0]).translation() - x0;
    assert!(shift.x < -2.0, "{shift:?}");
    assert!(shift.hypot() <= 6.0 + 1e-9, "{shift:?}");
}

#[test]
fn a_point_label_turns_to_its_angle_and_stays_upright() {
    let right = only(&[PointPosition::Right]);
    let center = Point::new(400.0, 300.0);
    // Counterclockwise on the page is a negative turn in page space, where y points down.
    for (angle, no_flip, turn) in [
        (FRAC_PI_6, false, -FRAC_PI_6),
        (5.0 * FRAC_PI_6, false, FRAC_PI_6),
        (5.0 * FRAC_PI_6, true, -5.0 * FRAC_PI_6),
    ] {
        let pp = PointPlacement {
            no_flip,
            ..right.clone()
        };
        let out = run(vec![class(pp)], vec![turned(angle)], vec![]);
        let l = &out.placed[0];
        for g in l.glyph_runs.iter().flat_map(|r| &r.glyphs) {
            assert!(
                wrap(rot(&g.transform) - turn).abs() < 1e-9,
                "{angle} {no_flip}"
            );
        }
        assert!(wrap(l.boxes[0].angle - turn).abs() < 1e-9);
        // "Right" is the reading direction of the turned text.
        let reading = Vec2::new(turn.cos(), turn.sin());
        let across = Vec2::new(-turn.sin(), turn.cos());
        let d = l.boxes[0].center - center;
        assert!(d.dot(reading) > 5.0, "{angle} {no_flip}");
        assert!(d.dot(across).abs() < 3.0, "{angle} {no_flip}");
    }
}

#[test]
fn the_offset_is_measured_from_the_outline() {
    let ring = vec![
        Point::new(380.0, 297.0),
        Point::new(420.0, 297.0),
        Point::new(420.0, 303.0),
        Point::new(380.0, 303.0),
    ];
    let with_outline = |f: &mut Feature| {
        if let Geometry::Point { outline, .. } = &mut f.geometry {
            *outline = Some(ring.clone());
        }
    };
    for (pos, axis, from_radius, from_outline) in [
        (PointPosition::Right, 4, 404.0, 422.0),
        (PointPosition::Top, 5, 296.0, 295.0),
    ] {
        let pp = only(&[pos]);
        let plain = run_one(pp.clone(), vec![]);
        let at = first(&plain.placed[0]).as_coeffs()[axis];
        assert!((at - from_radius).abs() < 1e-6, "{pos:?} {at}");

        let mut f = point(1, 0, "Paris", 400.0, 300.0);
        with_outline(&mut f);
        let out = run(vec![class(pp)], vec![f], vec![]);
        let at = first(&out.placed[0]).as_coeffs()[axis];
        assert!((at - from_outline).abs() < 1e-3, "{pos:?} {at}");
    }
}

#[test]
fn an_ambiguous_label_moves_or_goes_away() {
    let pp = |positions: &[PointPosition], ambiguous| PointPlacement {
        offset: 10.0,
        ambiguous,
        ..only(positions)
    };
    let both = [PointPosition::Right, PointPosition::Left];
    let kind = |out: &Labeling| out.placed.iter().find(|l| l.feature == 1).map(|l| l.kind);
    let right = Some(CandidateKind::Point(PointPosition::Right));
    let left = Some(CandidateKind::Point(PointPosition::Left));

    let alone = run_one(pp(&both, AmbiguousLabels::Keep), vec![]);
    assert_eq!(kind(&alone), right);
    // A point without text, 8 units right of the label, where the gap to its own point is 10.
    let other_x = alone.placed[0].bounds.x1 + 8.0;
    let case = |a: PointPlacement, other_class: usize| {
        let classes = vec![class(a), class(PointPlacement::default())];
        let features = vec![
            point(1, 0, "Paris", 400.0, 300.0),
            point(2, other_class, "", other_x, 300.0),
        ];
        run(classes, features, vec![])
    };

    assert_eq!(kind(&case(pp(&both, AmbiguousLabels::Keep), 1)), right);
    assert_eq!(kind(&case(pp(&both, AmbiguousLabels::AllClasses), 1)), left);
    assert_eq!(kind(&case(pp(&both, AmbiguousLabels::OwnClass), 1)), right);
    assert_eq!(kind(&case(pp(&both, AmbiguousLabels::OwnClass), 0)), left);

    let one = [PointPosition::Right];
    let out = case(pp(&one, AmbiguousLabels::AllClasses), 1);
    let u = out.unplaced.iter().find(|u| u.feature == 1).unwrap();
    assert_eq!(u.reason, UnplacedReason::Ambiguous);

    let mut leader = class(pp(&one, AmbiguousLabels::AllClasses));
    leader.fallbacks.order = vec![Fallback::Leader];
    let out = run(
        vec![leader, class(PointPlacement::default())],
        vec![
            point(1, 0, "Paris", 400.0, 300.0),
            point(2, 1, "", other_x, 300.0),
        ],
        vec![],
    );
    let l = out.placed.iter().find(|l| l.feature == 1).unwrap();
    assert_eq!(l.fallback, Some(Fallback::Leader));
}
