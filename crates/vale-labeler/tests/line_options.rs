use std::f64::consts::{FRAC_PI_2, PI};
use vale_labeler::*;

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    for name in ["NotoSans-Regular.ttf", "NotoSans-Italic.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.register(std::fs::read(path).unwrap()).unwrap();
    }
    f
}

fn class(lp: LinePlacement) -> LabelClass {
    LabelClass {
        name: "l".into(),
        style: TextStyle::new("Noto Sans", 14.0),
        priority: 0,
        margin: 0.0,
        placement: Placement::Line(lp),
        duplicate_distance: None,
        margin_hard: true,
        background: false,
        never_remove: false,
        fallbacks: Default::default(),
    }
}

fn feature(id: u64, text: &str, path: Vec<Point>) -> Feature {
    Feature {
        id,
        class: 0,
        text: text.into(),
        priority: 0.0,
        geometry: Geometry::Line {
            path,
            stroke_width: 2.0,
        },
    }
}

fn run(lp: LinePlacement, features: Vec<Feature>, obstacles: Vec<Obstacle>) -> Labeling {
    let inp = LabelInput {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        classes: vec![class(lp)],
        features,
        obstacles,
        parallels: Vec::new(),
        options: Options::default(),
    };
    place_labels(&mut fonts(), &inp)
}

fn run_one(lp: LinePlacement, path: Vec<Point>) -> Labeling {
    run(lp, vec![feature(1, "Mississippi", path)], vec![])
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn tfs(l: &PlacedLabel) -> Vec<Affine> {
    l.glyph_runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.transform))
        .collect()
}

fn rot(a: &Affine) -> f64 {
    let c = a.as_coeffs();
    c[1].atan2(c[0])
}

fn origin_x(a: &Affine) -> f64 {
    a.as_coeffs()[4]
}

fn wrap(a: f64) -> f64 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

fn straight() -> Vec<Point> {
    vec![p(100.0, 300.0), p(700.0, 300.0)]
}

fn with(sides: Vec<LineSide>, orientation: LineOrientation) -> LinePlacement {
    LinePlacement {
        sides,
        orientation,
        offset: 5.0,
        ..LinePlacement::default()
    }
}

/// The width of the label on a long straight line, with no options.
fn plain_width() -> f64 {
    run_one(LinePlacement::default(), straight()).placed[0]
        .bounds
        .width()
}

#[test]
fn each_orientation_has_each_side() {
    let (a, b) = (p(100.0, 500.0), p(700.0, 100.0));
    let angle = (b.y - a.y).atan2(b.x - a.x);
    let t = (b - a) / (b - a).hypot();
    // Positive above the line on the page.
    let height = |q: Point| (q - a).dot(Vec2::new(t.y, -t.x));
    for orientation in [
        LineOrientation::Curved,
        LineOrientation::Straight,
        LineOrientation::Horizontal,
        LineOrientation::Perpendicular,
    ] {
        let expected = match orientation {
            LineOrientation::Horizontal => 0.0,
            LineOrientation::Perpendicular => angle + FRAC_PI_2,
            _ => angle,
        };
        for side in [LineSide::Above, LineSide::Below, LineSide::Centered] {
            let out = run_one(with(vec![side], orientation), vec![a, b]);
            assert_eq!(out.placed.len(), 1, "{orientation:?} {side:?}");
            let l = &out.placed[0];
            for tf in tfs(l) {
                assert!(
                    wrap(rot(&tf) - expected).abs() < 0.05,
                    "{orientation:?} {side:?}"
                );
            }
            let hs: Vec<f64> = l.boxes.iter().map(|b| height(b.aabb().center())).collect();
            match side {
                LineSide::Above => assert!(hs.iter().all(|h| *h > 0.0), "{orientation:?}"),
                LineSide::Below => assert!(hs.iter().all(|h| *h < 0.0), "{orientation:?}"),
                LineSide::Centered => assert!(
                    hs.iter().any(|h| *h > 0.0) && hs.iter().any(|h| *h < 0.0),
                    "{orientation:?}"
                ),
            }
        }
    }
}

#[test]
fn a_horizontal_label_clears_a_sloped_line() {
    let (a, b) = (p(100.0, 500.0), p(700.0, 100.0));
    let out = run_one(
        with(vec![LineSide::Above], LineOrientation::Horizontal),
        vec![a, b],
    );
    let l = &out.placed[0];
    // The bottom left corner of the label is above the line.
    let corner = p(l.bounds.x0, l.bounds.y1);
    let t = (b - a) / (b - a).hypot();
    assert!((corner - a).dot(Vec2::new(t.y, -t.x)) > 0.0);
}

fn start_of(out: &Labeling) -> f64 {
    match out.placed[0].kind {
        CandidateKind::Line { start, .. } => start,
        k => panic!("{k:?}"),
    }
}

fn anchored(anchor: LineAnchor, distance: f64) -> LinePlacement {
    LinePlacement {
        anchor,
        anchor_distance: distance,
        sides: vec![LineSide::Above],
        ..LinePlacement::default()
    }
}

#[test]
fn the_class_sets_the_position_along_the_line() {
    let out = run_one(anchored(LineAnchor::FromStart, 50.0), straight());
    assert!((start_of(&out) - 50.0).abs() < 1e-6);
    assert!((out.placed[0].bounds.x0 - 150.0).abs() < 3.0);

    let out = run_one(anchored(LineAnchor::FromEnd, 50.0), straight());
    assert!((out.placed[0].bounds.x1 - 650.0).abs() < 3.0);

    let out = run_one(anchored(LineAnchor::BeforeStart, 20.0), straight());
    assert!((out.placed[0].bounds.x1 - 80.0).abs() < 3.0);

    let out = run_one(anchored(LineAnchor::AfterEnd, 20.0), straight());
    assert!((out.placed[0].bounds.x0 - 720.0).abs() < 3.0);

    let percent = LinePlacement {
        anchor_percent: true,
        ..anchored(LineAnchor::FromStart, 25.0)
    };
    let out = run_one(percent, straight());
    assert!((start_of(&out) - 150.0).abs() < 1e-6);
}

#[test]
fn the_tolerance_lets_the_label_move_from_its_anchor() {
    let block = vec![Obstacle::rect(
        Rect::new(140.0, 250.0, 160.0, 298.0),
        Weight::Forbidden,
    )];
    let lp = anchored(LineAnchor::FromStart, 50.0);
    let out = run(
        lp.clone(),
        vec![feature(1, "Mississippi", straight())],
        block.clone(),
    );
    assert!(out.placed.is_empty());

    let lp = LinePlacement {
        anchor_tolerance: 120.0,
        ..lp
    };
    let out = run(lp, vec![feature(1, "Mississippi", straight())], block);
    let s = start_of(&out);
    assert!(s > 50.0 && s <= 170.0 + 1e-6, "{s}");
}

#[test]
fn the_text_can_go_left_or_right_of_the_line_direction() {
    let reversed = vec![p(700.0, 300.0), p(100.0, 300.0)];
    let lp = with(vec![LineSide::Above], LineOrientation::Curved);
    let out = run_one(lp.clone(), reversed.clone());
    assert!(out.placed[0].bounds.y1 <= 300.0);

    // Left of a line that goes west is south.
    let lp = LinePlacement {
        by_direction: true,
        ..lp
    };
    let out = run_one(lp.clone(), reversed.clone());
    assert!(out.placed[0].bounds.y0 >= 300.0);
    let out = run_one(lp, straight());
    assert!(out.placed[0].bounds.y1 <= 300.0);
}

#[test]
fn a_label_can_go_past_the_ends_of_its_line() {
    let w = plain_width();
    let short = vec![p(300.0, 300.0), p(300.0 + w - 10.0, 300.0)];
    let out = run_one(LinePlacement::default(), short.clone());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::LineTooShort);

    let lp = LinePlacement {
        overrun: 10.0,
        ..LinePlacement::default()
    };
    let out = run_one(lp, short);
    assert_eq!(out.placed.len(), 1);
    let b = out.placed[0].bounds;
    assert!(b.x0 >= 290.0 - 1.0 && b.x1 <= 300.0 + w + 1.0);
}

#[test]
fn connected_lines_with_the_same_text_get_one_label() {
    let w = plain_width();
    // Two pieces, each too short, with the second drawn backward.
    let pieces = || {
        vec![
            feature(
                1,
                "Mississippi",
                vec![p(300.0, 300.0), p(300.0 + w * 0.8, 300.0)],
            ),
            feature(
                2,
                "Mississippi",
                vec![p(300.0 + w * 1.6, 300.0), p(300.0 + w * 0.8, 300.0)],
            ),
            feature(
                3,
                "Missouri",
                vec![p(300.0 + w * 1.6, 300.0), p(700.0, 300.0)],
            ),
        ]
    };
    let out = run(LinePlacement::default(), pieces(), vec![]);
    assert!(out.placed.iter().all(|l| l.feature == 3));

    let lp = LinePlacement {
        merge_connected: true,
        ..LinePlacement::default()
    };
    let out = run(lp, pieces(), vec![]);
    let ids: Vec<u64> = out.placed.iter().map(|l| l.feature).collect();
    assert_eq!(ids, vec![1, 3]);
    assert!(out.unplaced.iter().all(|u| u.feature != 2));
}

#[test]
fn the_letters_spread_along_the_line() {
    let w = plain_width();
    let lp = LinePlacement {
        max_spread: 0.5,
        ..LinePlacement::default()
    };
    let out = run_one(lp, straight());
    let gaps = "Mississippi".len() as f64 - 1.0;
    let spread = out.placed[0].bounds.width();
    assert!((spread - w - gaps * 7.0).abs() < 1.0, "{spread} {w}");

    let lp = LinePlacement {
        spread_fill: true,
        ..LinePlacement::default()
    };
    let out = run_one(lp, straight());
    let b = out.placed[0].bounds;
    assert!(b.x0 < 110.0 && b.x1 > 690.0, "{b:?}");
}

#[test]
fn the_text_can_follow_the_direction_of_the_line() {
    let reversed = vec![p(700.0, 300.0), p(100.0, 300.0)];
    let lp = LinePlacement {
        follow_direction: true,
        ..LinePlacement::default()
    };
    let out = run_one(lp, reversed);
    let t = tfs(&out.placed[0]);
    assert!(t.iter().all(|tf| wrap(rot(tf) - PI).abs() < 1e-6));
    assert!(t.windows(2).all(|w| origin_x(&w[1]) < origin_x(&w[0])));
}

#[test]
fn a_short_line_gets_no_label() {
    let lp = LinePlacement {
        min_length: 700.0,
        ..LinePlacement::default()
    };
    let out = run_one(lp, straight());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::BelowMinimumLength);

    let lp = LinePlacement {
        min_length: 500.0,
        ..LinePlacement::default()
    };
    assert_eq!(run_one(lp, straight()).placed.len(), 1);
}
