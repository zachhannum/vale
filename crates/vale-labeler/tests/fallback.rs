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

fn style(size: f64) -> TextStyle {
    TextStyle::new("Noto Sans", size)
}

fn class(placement: Placement, size: f64, order: Vec<Fallback>) -> LabelClass {
    LabelClass {
        name: "c".into(),
        style: style(size),
        priority: 0,
        margin: 0.0,
        placement,
        duplicate_distance: None,
        fallbacks: Fallbacks {
            order,
            min_size: 5.0,
            size_step: 1.0,
            max_lines: 3,
            abbreviations: vec![
                ("Mount".into(), "Mt.".into()),
                ("Saint".into(), "St.".into()),
            ],
            leader_distance: 20.0,
            ..Fallbacks::default()
        },
    }
}

fn horizontal() -> Placement {
    Placement::Polygon(PolygonPlacement {
        mode: PolygonMode::Horizontal,
        ..PolygonPlacement::default()
    })
}

/// A rectangle of `w` by `h` at the middle of the page.
fn rect(text: &str, w: f64, h: f64) -> Feature {
    let (x, y) = (200.0 - w / 2.0, 150.0 - h / 2.0);
    Feature {
        id: 1,
        class: 0,
        text: text.into(),
        priority: 0.0,
        geometry: Geometry::Polygon {
            rings: vec![vec![
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ]],
        },
    }
}

fn run(class: LabelClass, feature: Feature, obstacles: Vec<Obstacle>) -> Labeling {
    let input = LabelInput {
        bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
        classes: vec![class],
        features: vec![feature],
        obstacles,
        parallels: Vec::new(),
        options: Options::default(),
    };
    place_labels(&mut fonts(), &input)
}

fn width(text: &str, size: f64) -> f64 {
    fonts().measure(text, &style(size)).ink_bounds.width()
}

#[test]
fn a_label_that_does_not_fit_is_removed_without_fallbacks() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![]),
        rect(text, 0.8 * width(text, 12.0), 30.0),
        vec![],
    );
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::AreaTooSmall);
}

#[test]
fn a_smaller_font_fits_a_small_polygon() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::SmallerFont]),
        rect(text, 0.8 * width(text, 12.0), 30.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::SmallerFont));
    let size = label.glyph_runs[0].font_size;
    assert!((5.0..12.0).contains(&size), "{size}");
    assert!(out.unplaced.is_empty());
}

#[test]
fn the_largest_smaller_font_that_fits_wins() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::SmallerFont]),
        rect(text, width(text, 10.0) + 6.0, 30.0),
        vec![],
    );
    assert_eq!(out.placed[0].glyph_runs[0].font_size, 10.0);
}

#[test]
fn stacked_lines_fit_a_narrow_polygon() {
    let text = "Gulf of Saint Lawrence";
    let out = run(
        class(horizontal(), 8.0, vec![Fallback::Stack]),
        rect(text, 0.75 * width(text, 8.0), 50.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Stack));
    // The glyphs sit on two baselines.
    let mut ys: Vec<f64> = label
        .glyph_runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.transform.translation().y))
        .collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    assert_eq!(ys.len(), 2, "{ys:?}");
    assert!((ys[1] - ys[0] - 1.2 * 8.0).abs() < 0.01);
}

#[test]
fn an_abbreviation_fits_where_the_full_name_does_not() {
    let text = "Mount Saint Helens";
    let short = width("Mt. St. Helens", 10.0);
    let full = width(text, 10.0);
    let out = run(
        class(horizontal(), 10.0, vec![Fallback::Abbreviate]),
        rect(text, (short + full) / 2.0 + 3.0, 30.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Abbreviate));
    assert_eq!(label.text, text);
    let glyphs: usize = label.glyph_runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(glyphs, "Mt.St.Helens".len() + 2);
}

#[test]
fn a_small_polygon_gets_a_label_outside_with_a_leader_line() {
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::Leader]),
        rect("Islet", 8.0, 8.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Leader));
    let leader = label.leader.expect("a leader line");
    assert!(leader.anchor.distance(Point::new(200.0, 150.0)) < 4.0);
    assert!(!Rect::new(196.0, 146.0, 204.0, 154.0).contains(label.bounds.center()));
}

#[test]
fn a_crowded_point_label_moves_away_with_a_leader_line() {
    let (x, y) = (200.0, 150.0);
    // Hard obstacles around the point, from 3 to 15 units away, with a gap on the right that
    // is too narrow for the text.
    let ring = [
        Rect::new(x - 15.0, y - 15.0, x + 15.0, y - 3.0),
        Rect::new(x - 15.0, y + 3.0, x + 15.0, y + 15.0),
        Rect::new(x - 15.0, y - 3.0, x - 3.0, y + 3.0),
    ];
    let obstacles: Vec<Obstacle> = ring
        .iter()
        .map(|r| Obstacle {
            rect: *r,
            kind: ObstacleKind::Hard,
        })
        .collect();
    let town = Feature {
        id: 1,
        class: 0,
        text: "Town".into(),
        priority: 0.0,
        geometry: Geometry::Point {
            position: Point::new(x, y),
            symbol_radius: 2.0,
        },
    };
    let point = Placement::Point(PointPlacement::default());
    let none = run(
        class(point.clone(), 10.0, vec![]),
        town.clone(),
        obstacles.clone(),
    );
    assert_eq!(none.unplaced[0].reason, UnplacedReason::Obstructed);

    let out = run(class(point, 10.0, vec![Fallback::Leader]), town, obstacles);
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Leader));
    let leader = label.leader.expect("a leader line");
    assert!((leader.anchor.distance(Point::new(x, y)) - 2.0).abs() < 1e-6);
    assert!(leader.end.distance(leader.anchor) > 15.0);
    // The leader line goes through the gap.
    assert!((leader.anchor.y - y).abs() < 3.0 && (leader.end.y - y).abs() < 3.0);
}

#[test]
fn a_leader_line_does_not_cross_a_hard_obstacle() {
    let (x, y) = (200.0, 150.0);
    // Hard obstacles all around the point, so each leader line crosses one.
    let ring = [
        Rect::new(x - 15.0, y - 15.0, x + 15.0, y - 3.0),
        Rect::new(x - 15.0, y + 3.0, x + 15.0, y + 15.0),
        Rect::new(x - 15.0, y - 3.0, x - 3.0, y + 3.0),
        Rect::new(x + 3.0, y - 3.0, x + 15.0, y + 3.0),
    ];
    let obstacles: Vec<Obstacle> = ring
        .iter()
        .map(|r| Obstacle {
            rect: *r,
            kind: ObstacleKind::Hard,
        })
        .collect();
    let town = Feature {
        id: 1,
        class: 0,
        text: "Town".into(),
        priority: 0.0,
        geometry: Geometry::Point {
            position: Point::new(x, y),
            symbol_radius: 2.0,
        },
    };
    let point = Placement::Point(PointPlacement::default());
    let out = run(class(point, 10.0, vec![Fallback::Leader]), town, obstacles);
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::Obstructed);
}

#[test]
fn the_class_sets_the_order_of_the_fallbacks() {
    let text = "Mount Saint Helens";
    let feature = rect(
        text,
        (width("Mt. St. Helens", 10.0) + width(text, 10.0)) / 2.0,
        30.0,
    );
    let p = horizontal();
    let first = |order: Vec<Fallback>| {
        run(class(p.clone(), 10.0, order), feature.clone(), vec![]).placed[0].fallback
    };
    assert_eq!(
        first(vec![Fallback::Abbreviate, Fallback::SmallerFont]),
        Some(Fallback::Abbreviate)
    );
    assert_eq!(
        first(vec![Fallback::SmallerFont, Fallback::Abbreviate]),
        Some(Fallback::SmallerFont)
    );
}

#[test]
fn a_label_that_fits_uses_no_fallback() {
    let out = run(
        class(horizontal(), 10.0, Fallback::ALL.to_vec()),
        rect("Lake", 200.0, 100.0),
        vec![],
    );
    assert_eq!(out.placed[0].fallback, None);
}

#[test]
fn a_label_is_removed_when_no_fallback_fits() {
    let out = run(
        class(
            horizontal(),
            10.0,
            vec![Fallback::Abbreviate, Fallback::Stack],
        ),
        rect("Tasmania", 6.0, 6.0),
        vec![],
    );
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::AreaTooSmall);
}

/// The horizontal scale of the first glyph of `label`.
fn scale_x(label: &PlacedLabel) -> f64 {
    let t = label.glyph_runs[0].glyphs[0].transform.as_coeffs();
    t[0].hypot(t[1])
}

/// The x of the first glyph on each baseline, from the top line down.
fn line_starts(label: &PlacedLabel) -> Vec<f64> {
    let mut lines: Vec<(f64, f64)> = Vec::new();
    for g in label.glyph_runs.iter().flat_map(|r| &r.glyphs) {
        let p = g.transform.translation();
        match lines.iter_mut().find(|(y, _)| (y - p.y).abs() < 0.5) {
            Some((_, x)) => *x = x.min(p.x),
            None => lines.push((p.y, p.x)),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.into_iter().map(|(_, x)| x).collect()
}

#[test]
fn narrow_letters_fit_a_small_polygon() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::Compress]),
        rect(text, 0.92 * width(text, 12.0), 30.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Compress));
    assert_eq!(label.glyph_runs[0].font_size, 12.0);
    let s = scale_x(label);
    assert!((0.8 - 1e-9..1.0).contains(&s), "{s}");
}

#[test]
fn narrow_letters_stop_at_the_smallest_width() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::Compress]),
        rect(text, 0.6 * width(text, 12.0), 30.0),
        vec![],
    );
    assert!(out.placed.is_empty());
}

#[test]
fn narrow_letters_combine_with_a_smaller_font() {
    let text = "Lakeland";
    let feature = rect(text, 0.76 * width(text, 12.0), 30.0);
    let mut c = class(horizontal(), 12.0, vec![Fallback::SmallerFont]);
    c.fallbacks.min_size = 10.0;
    let out = run(c.clone(), feature.clone(), vec![]);
    assert!(out.placed.is_empty(), "a smaller font alone does not fit");
    c.fallbacks.compress_with_size = true;
    let out = run(c, feature, vec![]);
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::SmallerFont));
    assert!(label.glyph_runs[0].font_size < 12.0);
    assert!(scale_x(label) < 1.0);
}

#[test]
fn truncation_fits_a_small_polygon() {
    let text = "Lakeland";
    let out = run(
        class(horizontal(), 12.0, vec![Fallback::Truncate]),
        rect(text, 0.8 * width(text, 12.0), 30.0),
        vec![],
    );
    let label = &out.placed[0];
    assert_eq!(label.fallback, Some(Fallback::Truncate));
    assert_eq!(label.glyph_runs[0].font_size, 12.0);
    let glyphs: usize = label.glyph_runs.iter().map(|r| r.glyphs.len()).sum();
    assert!(glyphs < text.len(), "{glyphs}");
}

#[test]
fn truncation_comes_after_the_other_text_fallbacks() {
    let text = "Lakeland";
    let out = run(
        class(
            horizontal(),
            12.0,
            vec![Fallback::Truncate, Fallback::SmallerFont],
        ),
        rect(text, 0.8 * width(text, 12.0), 30.0),
        vec![],
    );
    assert_eq!(out.placed[0].fallback, Some(Fallback::SmallerFont));
}

#[test]
fn the_class_aligns_stacked_lines() {
    let text = "Gulf of Saint Lawrence";
    let feature = rect(text, 0.75 * width(text, 8.0), 50.0);
    let starts = |align: StackAlign| {
        let mut c = class(horizontal(), 8.0, vec![Fallback::Stack]);
        c.fallbacks.max_lines = 2;
        c.fallbacks.stack_align = align;
        let out = run(c, feature.clone(), vec![]);
        line_starts(&out.placed[0])
    };
    let left = starts(StackAlign::Left);
    assert_eq!(left.len(), 2);
    assert!((left[0] - left[1]).abs() < 1.0, "{left:?}");
    // "Gulf of Saint" is longer than "Lawrence", so a centered or a right second line starts
    // farther right.
    let center = starts(StackAlign::Center);
    let right = starts(StackAlign::Right);
    assert!(center[1] - center[0] > 1.0, "{center:?}");
    assert!(
        right[1] - right[0] > center[1] - center[0] + 1.0,
        "{right:?}"
    );
}

#[test]
fn the_class_sets_the_stack_separators_and_the_line_lengths() {
    let text = "Stratford-upon-Avon";
    let feature = rect(text, 0.7 * width(text, 8.0), 50.0);
    let mut c = class(horizontal(), 8.0, vec![Fallback::Stack]);
    assert!(run(c.clone(), feature.clone(), vec![]).placed.is_empty());
    c.fallbacks.stack_separators = " -".into();
    let out = run(c.clone(), feature.clone(), vec![]);
    assert_eq!(out.placed[0].fallback, Some(Fallback::Stack));
    assert!(line_starts(&out.placed[0]).len() >= 2);
    c.fallbacks.max_line_chars = 4;
    assert!(run(c.clone(), feature.clone(), vec![]).placed.is_empty());
    c.fallbacks.max_line_chars = 64;
    c.fallbacks.min_line_chars = 12;
    assert!(run(c, feature, vec![]).placed.is_empty());
}
