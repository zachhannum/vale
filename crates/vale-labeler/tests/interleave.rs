use vale_labeler::*;

/// Half the width of the polygon.
const HALF: f64 = 50.0;

/// The vertical line, a little east of the middle of the polygon.
const LINE_X: f64 = 305.0;

/// The spread of "KYR". Its gaps are a little wider than the line label.
const SPREAD: f64 = 1.7;

/// The margin of each class.
const MARGIN: f64 = 1.0;

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    let path = format!(
        "{}/../../assets/fonts/NotoSans-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    f.register(std::fs::read(path).unwrap()).unwrap();
    f
}

fn class(name: &str, priority: i32, placement: Placement) -> LabelClass {
    LabelClass {
        name: name.into(),
        style: TextStyle::new("Noto Sans", 14.0),
        priority,
        margin: MARGIN,
        placement,
        duplicate_distance: None,
        margin_hard: true,
        background: false,
        never_remove: false,
        fallbacks: Default::default(),
    }
}

/// A small flat polygon, too flat for its label to pass above or below a vertical label, and
/// too narrow for its label to move by one font size.
fn polygon() -> Geometry {
    Geometry::Polygon {
        rings: vec![vec![
            Point::new(300.0 - HALF, 140.0),
            Point::new(300.0 + HALF, 140.0),
            Point::new(300.0 + HALF, 180.0),
            Point::new(300.0 - HALF, 180.0),
        ]],
    }
}

/// "KYR" on `geometry`, spread wide, and, with `with_line`, a vertical line near the middle
/// whose label is "Ridge Range". The line is free for labels, so only the letters of the two
/// labels can collide.
fn input(kyr: LabelClass, geometry: Geometry, ridge: LabelClass, with_line: bool) -> LabelInput {
    let mut features = vec![Feature {
        id: 1,
        class: 0,
        text: "KYR".into(),
        priority: 0.0,
        geometry,
    }];
    if with_line {
        features.push(Feature {
            id: 2,
            class: 1,
            text: "Ridge Range".into(),
            priority: 0.0,
            geometry: Geometry::Line {
                path: vec![Point::new(LINE_X, 100.0), Point::new(LINE_X, 220.0)],
                stroke_width: 1.0,
            },
        });
    }
    LabelInput {
        bounds: Rect::new(0.0, 0.0, 600.0, 300.0),
        classes: vec![kyr, ridge],
        features,
        obstacles: vec![],
        parallels: Vec::new(),
        options: Options::default(),
    }
}

fn area(priority: i32) -> LabelClass {
    class(
        "area",
        priority,
        Placement::Polygon(PolygonPlacement {
            mode: PolygonMode::Horizontal,
            max_spread: SPREAD,
            ..PolygonPlacement::default()
        }),
    )
}

fn ridge(priority: i32) -> LabelClass {
    class(
        "ridge",
        priority,
        Placement::Line(LinePlacement {
            sides: vec![LineSide::Centered],
            orientation: LineOrientation::Straight,
            weight: Weight::FREE,
            ..LinePlacement::default()
        }),
    )
}

fn label(out: &Labeling, feature: u64) -> &PlacedLabel {
    out.placed
        .iter()
        .find(|l| l.feature == feature)
        .unwrap_or_else(|| panic!("feature {feature} has no label: {:?}", out.unplaced))
}

/// The distance between the origins of the first and the last glyph.
fn span(label: &PlacedLabel) -> f64 {
    let xs: Vec<f64> = label
        .glyph_runs
        .iter()
        .flat_map(|r| &r.glyphs)
        .map(|g| g.transform.translation().x)
        .collect();
    xs[xs.len() - 1] - xs[0]
}

/// Places "KYR" with and without the line, and returns it with the line. "KYR" keeps its
/// spread, and no letter of it touches "Ridge Range".
fn check(kyr: LabelClass, geometry: Geometry, line: LabelClass) -> PlacedLabel {
    let mut fonts = fonts();
    let alone = place_labels(
        &mut fonts,
        &input(kyr.clone(), geometry.clone(), line.clone(), false),
    );
    let out = place_labels(&mut fonts, &input(kyr, geometry, line, true));
    let placed = label(&out, 1);
    let ridge = label(&out, 2);
    assert_eq!(placed.boxes.len(), 3);
    assert!(
        span(placed) > span(label(&alone, 1)) - 1e-6,
        "spread {} of {}",
        span(placed),
        span(label(&alone, 1))
    );
    for a in &placed.boxes {
        assert!(
            !ridge.boxes.iter().any(|b| b.intersects(a)),
            "a letter of KYR at {:?} is on the line label",
            a.center
        );
    }
    placed.clone()
}

/// Each letter of the label stays between `x0` and `x1`.
fn assert_between(label: &PlacedLabel, x0: f64, x1: f64) {
    for b in &label.boxes {
        let ink = b.inflate(-MARGIN).aabb();
        assert!(ink.x0 >= x0 && ink.x1 <= x1, "{ink:?}");
    }
}

#[test]
fn the_middle_letter_sits_on_the_line_without_the_slide() {
    // The test layout needs a slide: the label of the polygon alone has a letter on the line.
    let alone = place_labels(&mut fonts(), &input(area(0), polygon(), ridge(1), false));
    let y = label(&alone, 1).boxes[1].aabb();
    assert!(y.x0 < LINE_X && y.x1 > LINE_X);
}

#[test]
fn a_spread_label_slides_so_that_a_crossing_label_passes_through_a_gap() {
    let placed = check(area(0), polygon(), ridge(1));
    assert_between(&placed, 300.0 - HALF, 300.0 + HALF);
}

#[test]
fn a_placed_spread_label_slides_so_that_a_later_label_can_cross_it() {
    let placed = check(area(1), polygon(), ridge(0));
    assert_between(&placed, 300.0 - HALF, 300.0 + HALF);
}

#[test]
fn a_label_over_a_background_label_crosses_it_at_a_gap() {
    let background = LabelClass {
        background: true,
        ..area(0)
    };
    let placed = check(background, polygon(), ridge(0));
    assert_between(&placed, 300.0 - HALF, 300.0 + HALF);
}

#[test]
fn a_spread_line_label_lets_a_crossing_label_pass_through_a_gap() {
    let river = class(
        "river",
        0,
        Placement::Line(LinePlacement {
            sides: vec![LineSide::Centered],
            orientation: LineOrientation::Straight,
            max_spread: SPREAD,
            weight: Weight::FREE,
            ..LinePlacement::default()
        }),
    );
    let line = Geometry::Line {
        path: vec![
            Point::new(300.0 - HALF, 160.0),
            Point::new(300.0 + HALF, 160.0),
        ],
        stroke_width: 1.0,
    };
    let placed = check(river, line, ridge(1));
    assert_between(&placed, 300.0 - HALF, 300.0 + HALF);
}
