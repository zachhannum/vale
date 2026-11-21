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

fn class(placement: Placement) -> LabelClass {
    LabelClass {
        name: "c".into(),
        style: TextStyle::new("Noto Sans", 14.0),
        priority: 0,
        margin: 0.0,
        placement,
        duplicate_distance: None,
        margin_hard: true,
        background: false,
        never_remove: false,
        fallbacks: Fallbacks {
            order: Vec::new(),
            ..Default::default()
        },
    }
}

fn points(positions: &[PointPosition]) -> LabelClass {
    class(Placement::Point(PointPlacement {
        positions: positions.to_vec(),
        ..PointPlacement::default()
    }))
}

fn point(id: u64, class: usize, text: &str, p: Point) -> Feature {
    Feature {
        id,
        class,
        text: text.into(),
        priority: 0.0,
        geometry: Geometry::Point {
            position: p,
            symbol_radius: 2.0,
            angle: None,
            outline: None,
        },
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

const PARIS: Point = Point::new(400.0, 300.0);
const TOP_RIGHT: Option<CandidateKind> = Some(CandidateKind::Point(PointPosition::TopRight));

/// Paris alone, with all positions, and the obstacles.
fn paris(obstacles: Vec<Obstacle>) -> Labeling {
    run(
        vec![points(&PointPosition::DEFAULT_ORDER)],
        vec![point(1, 0, "Paris", PARIS)],
        obstacles,
    )
}

fn kind(out: &Labeling, id: u64) -> Option<CandidateKind> {
    out.placed.iter().find(|l| l.feature == id).map(|l| l.kind)
}

/// The bounds of the label of Paris with no obstacles.
fn alone() -> Rect {
    let out = paris(vec![]);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
    out.placed[0].bounds
}

/// A horizontal line through the middle of `b`.
fn across(b: Rect) -> Vec<Point> {
    let y = b.center().y;
    vec![Point::new(b.x0 - 5.0, y), Point::new(b.x1 + 5.0, y)]
}

#[test]
fn a_line_obstacle_has_a_weight_from_free_to_forbidden() {
    let b = alone();
    let line = |weight| Obstacle::Line {
        path: across(b),
        width: 1.0,
        weight,
    };
    assert_eq!(kind(&paris(vec![line(Weight::FREE)]), 1), TOP_RIGHT);
    let costly = paris(vec![line(Weight::Cost(5.0))]);
    assert!(kind(&costly, 1).is_some());
    assert_ne!(kind(&costly, 1), TOP_RIGHT);
    let forbidden = paris(vec![line(Weight::Forbidden)]);
    assert!(kind(&forbidden, 1).is_some());
    assert_ne!(kind(&forbidden, 1), TOP_RIGHT);
}

#[test]
fn a_point_obstacle_is_a_disc() {
    let b = alone();
    let disc = |radius| Obstacle::Point {
        position: b.center(),
        radius,
        weight: Weight::Forbidden,
    };
    assert_ne!(kind(&paris(vec![disc(2.0)]), 1), TOP_RIGHT);
    // A disc that does not reach the text.
    let out = paris(vec![Obstacle::Point {
        position: Point::new(b.x1 + 10.0, b.y0 - 10.0),
        radius: 5.0,
        weight: Weight::Forbidden,
    }]);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
}

#[test]
fn a_label_can_cross_a_fill_but_not_its_outline() {
    let b = alone();
    let base = paris(vec![]).placed[0].cost;
    let forest = |x0: f64| Obstacle::Polygon {
        rings: vec![vec![
            Point::new(x0, 100.0),
            Point::new(700.0, 100.0),
            Point::new(700.0, 500.0),
            Point::new(x0, 500.0),
        ]],
        inside: Weight::Cost(0.5),
        outline: Weight::Forbidden,
    };
    // The label is inside the forest, away from its outline.
    let inside = paris(vec![forest(100.0)]);
    assert_eq!(kind(&inside, 1), TOP_RIGHT);
    assert!((inside.placed[0].cost - base - 0.5).abs() < 1e-9);
    // The outline crosses the label.
    let border = paris(vec![forest(b.center().x)]);
    assert!(kind(&border, 1).is_some());
    assert_ne!(kind(&border, 1), TOP_RIGHT);
}

#[test]
fn a_feature_with_no_text_is_an_obstacle_with_its_weight() {
    let b = alone();
    let road = |weight| {
        class(Placement::Line(LinePlacement {
            weight,
            ..LinePlacement::default()
        }))
    };
    let line = Feature {
        id: 2,
        class: 1,
        text: String::new(),
        priority: 0.0,
        geometry: Geometry::Line {
            path: across(b),
            stroke_width: 1.0,
        },
    };
    let classes = |weight| vec![points(&PointPosition::DEFAULT_ORDER), road(weight)];
    let features = vec![point(1, 0, "Paris", PARIS), line];
    let out = run(classes(Weight::FREE), features.clone(), vec![]);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
    let out = run(classes(Weight::Forbidden), features, vec![]);
    assert_ne!(kind(&out, 1), TOP_RIGHT);
    let u = out.unplaced.iter().find(|u| u.feature == 2).unwrap();
    assert_eq!(u.reason, UnplacedReason::EmptyText);

    // The outline of a polygon with no text.
    let border = |outline| {
        class(Placement::Polygon(PolygonPlacement {
            outline_weight: outline,
            ..PolygonPlacement::default()
        }))
    };
    let x = b.center().x;
    let area = Feature {
        id: 3,
        class: 1,
        text: String::new(),
        priority: 0.0,
        geometry: Geometry::Polygon {
            rings: vec![vec![
                Point::new(x, 100.0),
                Point::new(700.0, 100.0),
                Point::new(700.0, 500.0),
                Point::new(x, 500.0),
            ]],
        },
    };
    let features = vec![point(1, 0, "Paris", PARIS), area];
    let classes = |w| vec![points(&PointPosition::DEFAULT_ORDER), border(w)];
    let out = run(classes(Weight::FREE), features.clone(), vec![]);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
    let out = run(classes(Weight::Forbidden), features, vec![]);
    assert_ne!(kind(&out, 1), TOP_RIGHT);
}

/// Paris at its top right, and Lyon to the right, whose label at its left covers the label
/// of Paris.
fn covered(paris: LabelClass, mut lyon: LabelClass) -> Labeling {
    let b = alone();
    lyon.priority = 1;
    run(
        vec![paris, lyon],
        vec![
            point(1, 0, "Paris", PARIS),
            point(2, 1, "Lyon", Point::new(b.x1 + 6.0, b.center().y)),
        ],
        vec![],
    )
}

#[test]
fn a_background_class_goes_first_and_other_labels_cover_it() {
    let lyon = points(&[PointPosition::Left]);
    let out = covered(points(&[PointPosition::TopRight]), lyon.clone());
    assert!(kind(&out, 2).is_some());
    assert!(kind(&out, 1).is_none());

    let mut background = points(&[PointPosition::TopRight]);
    background.background = true;
    let out = covered(background, lyon);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
    assert!(kind(&out, 2).is_some());
    let (a, b) = (&out.placed[0], &out.placed[1]);
    assert!(
        a.boxes
            .iter()
            .any(|x| b.boxes.iter().any(|y| x.intersects(y)))
    );
}

#[test]
fn a_never_remove_class_places_each_label() {
    let lyon = points(&[PointPosition::Left]);
    let mut paris = points(&[PointPosition::TopRight]);
    paris.never_remove = true;
    let out = covered(paris.clone(), lyon);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
    assert!(kind(&out, 2).is_some());

    // Also over a forbidden obstacle.
    let b = alone();
    let block = vec![Obstacle::rect(b, Weight::Forbidden)];
    let out = run(
        vec![points(&[PointPosition::TopRight])],
        vec![point(1, 0, "Paris", PARIS)],
        block.clone(),
    );
    assert_eq!(out.unplaced[0].reason, UnplacedReason::Obstructed);
    let out = run(vec![paris], vec![point(1, 0, "Paris", PARIS)], block);
    assert_eq!(kind(&out, 1), TOP_RIGHT);
}

/// The point of Lyon whose label at its left ends `gap` right of the label of Paris.
fn margin_gap(gap: f64) -> Point {
    let b = alone();
    let at = Point::new(400.0, b.center().y);
    let out = run(
        vec![points(&[PointPosition::Left])],
        vec![point(2, 0, "Lyon", at)],
        vec![],
    );
    let l = out.placed[0].bounds;
    Point::new(b.x1 + gap + (at.x - l.x0), at.y)
}

#[test]
fn the_margin_can_be_a_hard_rule() {
    // The text of Lyon is clear of the text of Paris, but not of its margin.
    let lyon_at = margin_gap(3.0);
    let margin = |hard: bool, positions: &[PointPosition]| {
        let mut paris = points(&[PointPosition::TopRight]);
        paris.margin = 6.0;
        paris.margin_hard = hard;
        paris.priority = 1;
        run(
            vec![paris, points(positions)],
            vec![point(1, 0, "Paris", PARIS), point(2, 1, "Lyon", lyon_at)],
            vec![],
        )
    };
    let hard = margin(true, &[PointPosition::Left]);
    assert_eq!(kind(&hard, 1), TOP_RIGHT);
    assert!(kind(&hard, 2).is_none());
    let soft = margin(false, &[PointPosition::Left]);
    assert_eq!(kind(&soft, 1), TOP_RIGHT);
    assert_eq!(
        kind(&soft, 2),
        Some(CandidateKind::Point(PointPosition::Left))
    );
}

#[test]
fn a_soft_margin_prefers_a_free_margin() {
    let lyon_at = margin_gap(3.0);
    let mut lyon = points(&[PointPosition::Left, PointPosition::Right]);
    lyon.margin = 6.0;
    lyon.margin_hard = false;
    lyon.priority = 0;
    let mut paris = points(&[PointPosition::TopRight]);
    paris.priority = 1;
    let out = run(
        vec![paris, lyon.clone()],
        vec![point(1, 0, "Paris", PARIS), point(2, 1, "Lyon", lyon_at)],
        vec![],
    );
    assert_eq!(
        kind(&out, 2),
        Some(CandidateKind::Point(PointPosition::Right))
    );
}
