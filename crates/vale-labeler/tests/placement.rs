use vale_labeler::*;

#[test]
fn invalid_features_are_reported() {
    let class = LabelClass {
        name: "p".into(),
        style: TextStyle::new("Noto Sans", 12.0),
        priority: 0,
        margin: 0.0,
        placement: Placement::Point(PointPlacement::default()),
    };
    let pt = Geometry::Point {
        position: Point::new(10.0, 10.0),
        symbol_radius: 2.0,
    };
    let feat = |id, class, text: &str, geometry| Feature {
        id,
        class,
        text: text.into(),
        priority: 0.0,
        geometry,
    };
    let input = LabelInput {
        bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
        classes: vec![class],
        features: vec![
            feat(1, 0, "  ", pt.clone()),
            feat(2, 99, "Town", pt.clone()),
            feat(
                3,
                0,
                "River",
                Geometry::Line {
                    path: vec![Point::new(0.0, 0.0), Point::new(50.0, 0.0)],
                    stroke_width: 1.0,
                },
            ),
        ],
        obstacles: vec![],
        options: Options::default(),
    };
    let out = place_labels(&mut Fonts::new(), &input);
    assert!(out.placed.is_empty());
    let reasons: Vec<_> = out.unplaced.iter().map(|u| (u.feature, u.reason)).collect();
    assert_eq!(
        reasons,
        vec![
            (1, UnplacedReason::EmptyText),
            (2, UnplacedReason::InvalidClass),
            (3, UnplacedReason::GeometryMismatch),
        ]
    );
}

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    for name in ["NotoSans-Regular.ttf", "NotoSans-Italic.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.register(std::fs::read(path).unwrap()).unwrap();
    }
    f
}

fn class(priority: i32) -> LabelClass {
    LabelClass {
        name: "p".into(),
        style: TextStyle::new("Noto Sans", 12.0),
        priority,
        margin: 0.0,
        placement: Placement::Point(PointPlacement::default()),
    }
}

fn point(id: u64, class: usize, text: &str, x: f64, y: f64, priority: f64) -> Feature {
    Feature {
        id,
        class,
        text: text.into(),
        priority,
        geometry: Geometry::Point {
            position: Point::new(x, y),
            symbol_radius: 2.0,
        },
    }
}

fn input(features: Vec<Feature>, obstacles: Vec<Obstacle>) -> LabelInput {
    LabelInput {
        bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
        classes: vec![class(0), class(5)],
        features,
        obstacles,
        options: Options::default(),
    }
}

fn kind_of(out: &Labeling, id: u64) -> CandidateKind {
    out.placed.iter().find(|p| p.feature == id).unwrap().kind
}

#[test]
fn single_point_top_right() {
    let inp = input(vec![point(1, 0, "Paris", 200.0, 150.0, 0.0)], vec![]);
    let out = place_labels(&mut fonts(), &inp);
    let p = &out.placed[0];
    assert_eq!(p.kind, CandidateKind::Point(PointPosition::TopRight));
    assert_eq!(p.glyph_runs.len(), 1);
    assert_eq!(p.glyph_runs[0].glyphs.len(), 5);
    assert!(p.bounds.x0 > 200.0 && p.bounds.y1 < 150.0);
    assert!(!p.outlines().elements().is_empty());
}

#[test]
fn point_near_edge_moves_left() {
    let inp = input(vec![point(1, 0, "Paris", 395.0, 150.0, 0.0)], vec![]);
    let out = place_labels(&mut fonts(), &inp);
    let p = &out.placed[0];
    assert!(matches!(
        p.kind,
        CandidateKind::Point(
            PointPosition::TopLeft | PointPosition::BottomLeft | PointPosition::Left
        )
    ));
    assert!(p.bounds.x0 >= 0.0 && p.bounds.x1 <= 400.0);
}

#[test]
fn obstacles_force_or_keep_position() {
    let area = Rect::new(195.0, 120.0, 260.0, 150.0);
    let at = |kind| {
        let inp = input(
            vec![point(1, 0, "Paris", 200.0, 150.0, 0.0)],
            vec![Obstacle { rect: area, kind }],
        );
        kind_of(&place_labels(&mut fonts(), &inp), 1)
    };
    assert_ne!(
        at(ObstacleKind::Hard),
        CandidateKind::Point(PointPosition::TopRight)
    );
    assert_eq!(
        at(ObstacleKind::Soft { cost: 0.5 }),
        CandidateKind::Point(PointPosition::TopRight)
    );
    assert_ne!(
        at(ObstacleKind::Soft { cost: 5.0 }),
        CandidateKind::Point(PointPosition::TopRight)
    );
}

#[test]
fn close_points_do_not_overlap() {
    let inp = input(
        vec![
            point(1, 0, "Alexandria", 200.0, 150.0, 0.0),
            point(2, 0, "Constantinople", 204.0, 150.0, 0.0),
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    assert_eq!(out.placed.len(), 2);
    for a in &out.placed[0].boxes {
        for b in &out.placed[1].boxes {
            assert!(!a.intersects(b));
        }
    }
}

#[test]
fn dense_grid_has_collisions_but_no_overlaps() {
    let mut features = Vec::new();
    for r in 0..5 {
        for c in 0..8 {
            let id = (r * 8 + c) as u64;
            features.push(point(
                id,
                0,
                "Abcdefgh",
                100.0 + 12.0 * c as f64,
                100.0 + 12.0 * r as f64,
                0.0,
            ));
        }
    }
    let inp = input(features, vec![]);
    let out = place_labels(&mut fonts(), &inp);
    assert!(
        out.unplaced
            .iter()
            .any(|u| u.reason == UnplacedReason::Collision)
    );
    for (i, a) in out.placed.iter().enumerate() {
        for b in &out.placed[i + 1..] {
            for x in &a.boxes {
                for y in &b.boxes {
                    assert!(!x.intersects(y));
                }
            }
        }
        for f in &inp.features {
            let Geometry::Point { position, .. } = f.geometry else {
                continue;
            };
            let sym = OrientedBox::from_rect(Rect::new(
                position.x - 2.0,
                position.y - 2.0,
                position.x + 2.0,
                position.y + 2.0,
            ));
            for x in &a.boxes {
                assert!(!x.intersects(&sym));
            }
        }
    }
}

#[test]
fn priorities_decide() {
    // Different classes: class 1 has the higher priority. Other positions are
    // blocked so the two labels must compete for one.
    let inp = input(
        vec![
            point(1, 0, "Low", 200.0, 150.0, 0.0),
            point(2, 1, "High", 200.0, 150.0, 0.0),
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    assert_eq!(
        kind_of(&out, 2),
        CandidateKind::Point(PointPosition::TopRight)
    );
    assert_ne!(kind_of(&out, 1), kind_of(&out, 2));

    let inp = input(
        vec![
            point(1, 0, "Low", 200.0, 150.0, 1.0),
            point(2, 0, "High", 200.0, 150.0, 2.0),
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    assert_eq!(
        kind_of(&out, 2),
        CandidateKind::Point(PointPosition::TopRight)
    );
}

#[test]
fn out_of_bounds_reasons() {
    let inp = input(
        vec![
            point(1, 0, "Far", 500.0, 150.0, 0.0),
            point(2, 0, "Corner Town Name", 1.0, 1.0, 0.0),
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    // The corner point can still use BottomRight, so make it impossible.
    assert_eq!(out.unplaced[0].reason, UnplacedReason::OutOfBounds);
    let mut inp = inp;
    inp.bounds = Rect::new(0.0, 0.0, 20.0, 20.0);
    inp.features = vec![point(2, 0, "Corner Town Name", 1.0, 1.0, 0.0)];
    let out = place_labels(&mut fonts(), &inp);
    assert_eq!(out.unplaced[0].reason, UnplacedReason::OutOfBounds);
}

#[test]
fn font_problems() {
    let mut inp = input(
        vec![
            point(1, 0, "Paris", 100.0, 100.0, 0.0),
            point(2, 0, "北京", 200.0, 100.0, 0.0),
        ],
        vec![],
    );
    inp.classes[0].style.family = "No Such Family".into();
    inp.classes.push(class(0));
    inp.features[1].class = 2;
    let out = place_labels(&mut fonts(), &inp);
    let r: Vec<_> = out.unplaced.iter().map(|u| (u.feature, u.reason)).collect();
    assert!(r.contains(&(2, UnplacedReason::MissingGlyphs)));
    // An unknown family falls back or reports; it must not be placed silently as nothing.
    assert_eq!(out.placed.len() + out.unplaced.len(), 2);
}

#[test]
fn counts_and_determinism() {
    let features: Vec<_> = (0..30)
        .map(|i| {
            point(
                i,
                (i % 2) as usize,
                "Town",
                50.0 + 9.0 * i as f64 % 300.0,
                100.0,
                i as f64,
            )
        })
        .collect();
    let inp = input(features, vec![]);
    let mut f = fonts();
    let a = place_labels(&mut f, &inp);
    let b = place_labels(&mut f, &inp);
    assert_eq!(a.placed.len() + a.unplaced.len(), 30);
    assert_eq!(a, b);
}

// ---------- line labels (T6) ----------

fn line_class(priority: i32, lp: LinePlacement) -> LabelClass {
    let mut style = TextStyle::new("Noto Sans", 14.0);
    style.italic = true;
    LabelClass {
        name: "l".into(),
        style,
        priority,
        margin: 0.0,
        placement: Placement::Line(lp),
    }
}

fn line_feature(id: u64, class: usize, text: &str, path: Vec<Point>) -> Feature {
    Feature {
        id,
        class,
        text: text.into(),
        priority: 0.0,
        geometry: Geometry::Line {
            path,
            stroke_width: 2.0,
        },
    }
}

fn line_input(
    classes: Vec<LabelClass>,
    features: Vec<Feature>,
    obstacles: Vec<Obstacle>,
) -> LabelInput {
    LabelInput {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        classes,
        features,
        obstacles,
        options: Options::default(),
    }
}

fn run_line(lp: LinePlacement, path: Vec<Point>, text: &str) -> Labeling {
    let inp = line_input(
        vec![line_class(0, lp)],
        vec![line_feature(1, 0, text, path)],
        vec![],
    );
    place_labels(&mut fonts(), &inp)
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn glyph_tfs(l: &PlacedLabel) -> Vec<Affine> {
    l.glyph_runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.transform))
        .collect()
}

fn rot(a: &Affine) -> f64 {
    let c = a.as_coeffs();
    c[1].atan2(c[0])
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}

fn box_ys(l: &PlacedLabel) -> (f64, f64) {
    let y0 = l.boxes.iter().map(|b| b.aabb().y0).fold(f64::MAX, f64::min);
    let y1 = l.boxes.iter().map(|b| b.aabb().y1).fold(f64::MIN, f64::max);
    (y0, y1)
}

fn lp_sides(sides: Vec<LineSide>) -> LinePlacement {
    // The offset of 5 keeps the descenders of "pp" off the line.
    LinePlacement {
        sides,
        offset: 5.0,
        ..LinePlacement::default()
    }
}

#[test]
fn horizontal_line_sides() {
    let path = vec![p(100.0, 300.0), p(700.0, 300.0)];
    let out = run_line(lp_sides(vec![LineSide::Above]), path.clone(), "Mississippi");
    assert_eq!(out.placed.len(), 1);
    let l = &out.placed[0];
    assert!(matches!(
        l.kind,
        CandidateKind::Line {
            side: LineSide::Above,
            ..
        }
    ));
    for t in glyph_tfs(l) {
        assert!(rot(&t).abs() < 1e-6);
    }
    assert!(box_ys(l).1 <= 300.0);
    assert!((l.bounds.center().x - 400.0).abs() < 1.0);
    assert!(!l.outlines().elements().is_empty());

    let out = run_line(lp_sides(vec![LineSide::Below]), path.clone(), "Mississippi");
    assert!(box_ys(&out.placed[0]).0 >= 300.0);
    let out = run_line(lp_sides(vec![LineSide::Centered]), path, "Mississippi");
    let (y0, y1) = box_ys(&out.placed[0]);
    assert!(y0 < 300.0 && y1 > 300.0);
}

#[test]
fn reversed_line_is_not_upside_down() {
    let path = vec![p(700.0, 300.0), p(100.0, 300.0)];
    let out = run_line(lp_sides(vec![LineSide::Above]), path, "Mississippi");
    let l = &out.placed[0];
    let xs: Vec<f64> = glyph_tfs(l).iter().map(|t| t.as_coeffs()[4]).collect();
    assert!(xs.windows(2).all(|w| w[1] > w[0]));
    assert!(box_ys(l).1 <= 300.0);
}

#[test]
fn diagonal_and_vertical_lines() {
    let out = run_line(
        LinePlacement::default(),
        vec![p(100.0, 100.0), p(500.0, 500.0)],
        "Mississippi",
    );
    assert_eq!(out.placed.len(), 1);
    for t in glyph_tfs(&out.placed[0]) {
        assert!((rot(&t) - std::f64::consts::FRAC_PI_4).abs() < 1e-3);
    }
    for path in [
        vec![p(400.0, 550.0), p(400.0, 50.0)],
        vec![p(400.0, 50.0), p(400.0, 550.0)],
    ] {
        let out = run_line(LinePlacement::default(), path, "Mississippi");
        assert_eq!(out.placed.len(), 1);
    }
}

#[test]
fn quarter_circle() {
    let path: Vec<Point> = (0..=18)
        .map(|i| {
            let a = (i as f64 * 5.0).to_radians();
            p(100.0 + 300.0 * a.sin(), 500.0 - 300.0 * a.cos())
        })
        .collect();
    let lp = LinePlacement::default();
    let max_turn = lp.max_glyph_turn;
    let out = run_line(lp, path, "Mississippi");
    assert_eq!(out.placed.len(), 1);
    let l = &out.placed[0];
    let r: Vec<f64> = glyph_tfs(l).iter().map(rot).collect();
    let d: Vec<f64> = r.windows(2).map(|w| wrap(w[1] - w[0])).collect();
    assert!(d.iter().all(|&x| x.abs() <= max_turn));
    assert!(d.iter().all(|&x| x >= -1e-9) || d.iter().all(|&x| x <= 1e-9));
    assert!(d.iter().any(|&x| x.abs() > 1e-6));
    assert_eq!(l.boxes.len(), "Mississippi".chars().count());
}

#[test]
fn zigzag_too_curved_and_short_line() {
    // 90 degree corners: alternate right and down/up steps.
    let mut zig = vec![p(100.0, 100.0)];
    for i in 0..30 {
        let last = *zig.last().unwrap();
        zig.push(match i % 3 {
            0 => p(last.x + 20.0, last.y),
            1 => p(last.x, last.y + 20.0),
            _ => p(last.x + 20.0, last.y),
        });
    }
    let lp = LinePlacement {
        smoothing: 0.0,
        ..LinePlacement::default()
    };
    let out = run_line(lp, zig, "Mississippi");
    assert!(out.placed.is_empty());
    assert_eq!(out.unplaced[0].reason, UnplacedReason::TooCurved);

    let out = run_line(
        LinePlacement::default(),
        vec![p(100.0, 100.0), p(130.0, 100.0)],
        "Mississippi River Valley",
    );
    assert_eq!(out.unplaced[0].reason, UnplacedReason::LineTooShort);
}

#[test]
fn line_entering_from_outside() {
    let out = run_line(
        LinePlacement::default(),
        vec![p(-500.0, 300.0), p(400.0, 300.0)],
        "Mississippi",
    );
    assert_eq!(out.placed.len(), 1);
    let b = Rect::new(0.0, 0.0, 800.0, 600.0);
    for bx in &out.placed[0].boxes {
        assert!(bx.is_inside(b));
    }
}

#[test]
fn repeated_labels() {
    let lp = LinePlacement {
        repeat_distance: Some(250.0),
        ..LinePlacement::default()
    };
    let out = run_line(lp, vec![p(20.0, 300.0), p(780.0, 300.0)], "Mississippi");
    assert_eq!(out.placed.len(), 3);
    let reps: Vec<u32> = out.placed.iter().map(|l| l.repeat).collect();
    assert_eq!(reps, vec![0, 1, 2]);
    for (l, cx) in out.placed.iter().zip([147.0, 400.0, 653.0]) {
        assert!((l.bounds.center().x - cx).abs() < 15.0, "{:?}", l.bounds);
    }
    for i in 0..3 {
        for j in i + 1..3 {
            assert!(
                !out.placed[i]
                    .boxes
                    .iter()
                    .any(|a| out.placed[j].boxes.iter().any(|b| a.intersects(b)))
            );
        }
    }
}

fn overlap(a: &PlacedLabel, b: &PlacedLabel) -> bool {
    a.boxes
        .iter()
        .any(|x| b.boxes.iter().any(|y| x.intersects(y)))
}

#[test]
fn line_and_point_compete() {
    let line_path = vec![p(100.0, 300.0), p(700.0, 300.0)];
    for (line_pri, point_pri) in [(5, 0), (0, 5)] {
        let mut pc = class(point_pri);
        pc.style = TextStyle::new("Noto Sans", 14.0);
        let inp = line_input(
            vec![line_class(line_pri, lp_sides(vec![LineSide::Above])), pc],
            vec![
                line_feature(1, 0, "Mississippi", line_path.clone()),
                point(2, 1, "Memphis", 400.0, 292.0, 0.0),
            ],
            vec![],
        );
        let out = place_labels(&mut fonts(), &inp);
        for a in &out.placed {
            for b in &out.placed {
                if a.feature != b.feature {
                    assert!(!overlap(a, b));
                }
            }
        }
        let winner = if line_pri > point_pri { 1 } else { 2 };
        assert!(out.placed.iter().any(|l| l.feature == winner));
    }
}

#[test]
fn symbol_on_line_moves_label() {
    let inp = line_input(
        vec![line_class(0, LinePlacement::default()), class(0)],
        vec![
            line_feature(1, 0, "Mississippi", vec![p(100.0, 300.0), p(700.0, 300.0)]),
            Feature {
                id: 2,
                class: 1,
                text: "X".into(),
                priority: 0.0,
                geometry: Geometry::Point {
                    position: p(400.0, 295.0),
                    symbol_radius: 6.0,
                },
            },
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    let l = out.placed.iter().find(|l| l.feature == 1).unwrap();
    let sym = OrientedBox::from_rect(Rect::new(394.0, 289.0, 406.0, 301.0));
    assert!(l.boxes.iter().all(|b| !b.intersects(&sym)));
    let CandidateKind::Line { side, start } = l.kind else {
        panic!()
    };
    assert!(side != LineSide::Above || (start - 350.0).abs() > 1.0);
}

#[test]
fn point_label_avoids_stroke() {
    let inp = line_input(
        vec![line_class(0, LinePlacement::default()), class(0)],
        vec![
            line_feature(1, 0, "Mississippi", vec![p(100.0, 200.0), p(700.0, 200.0)]),
            point(2, 1, "Town", 400.0, 197.0, 0.0),
        ],
        vec![],
    );
    let out = place_labels(&mut fonts(), &inp);
    let l = out.placed.iter().find(|l| l.feature == 2).unwrap();
    let a = p(100.0, 200.0);
    let b = p(700.0, 200.0);
    assert!(l.boxes.iter().all(|bx| !bx.intersects_segment(a, b)));
}

#[test]
fn line_determinism() {
    let inp = line_input(
        vec![line_class(
            0,
            LinePlacement {
                repeat_distance: Some(250.0),
                ..LinePlacement::default()
            },
        )],
        vec![line_feature(
            1,
            0,
            "Mississippi",
            vec![p(20.0, 300.0), p(780.0, 400.0)],
        )],
        vec![],
    );
    let mut f = fonts();
    assert_eq!(place_labels(&mut f, &inp), place_labels(&mut f, &inp));
}

// ---------- improvement and duplicate control (T7) ----------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn grid40(seed: u64, improve: bool) -> LabelInput {
    let mut features = Vec::new();
    for r in 0..5 {
        for c in 0..8 {
            features.push(point(
                (r * 8 + c) as u64,
                0,
                "Abcdefgh",
                100.0 + 12.0 * c as f64,
                100.0 + 12.0 * r as f64,
                0.0,
            ));
        }
    }
    let mut inp = input(features, vec![]);
    inp.options = Options {
        seed,
        improve,
        duplicate_distance: None,
    };
    inp
}

fn assert_valid(inp: &LabelInput, out: &Labeling) {
    for (i, a) in out.placed.iter().enumerate() {
        for b in &out.placed[i + 1..] {
            assert!(!overlap(a, b));
        }
        for bx in &a.boxes {
            assert!(bx.is_inside(inp.bounds));
        }
        for f in &inp.features {
            let Geometry::Point {
                position,
                symbol_radius: r,
            } = f.geometry
            else {
                continue;
            };
            let sym = OrientedBox::from_rect(Rect::new(
                position.x - r,
                position.y - r,
                position.x + r,
                position.y + r,
            ));
            for bx in &a.boxes {
                assert!(!bx.intersects(&sym));
            }
        }
        for o in &inp.obstacles {
            if o.kind == ObstacleKind::Hard {
                let ob = OrientedBox::from_rect(o.rect);
                for bx in &a.boxes {
                    assert!(!bx.intersects(&ob));
                }
            }
        }
    }
}

#[test]
fn improve_false_is_greedy_and_deterministic() {
    let mut f = fonts();
    let a = place_labels(&mut f, &grid40(1, false));
    let b = place_labels(&mut f, &grid40(99, false));
    assert_eq!(a, b);
}

#[test]
fn improvement_never_worse_and_valid() {
    let mut f = fonts();
    let base = place_labels(&mut f, &grid40(0, false));
    let mut outs = Vec::new();
    for seed in [1, 2, 3, 4, 5] {
        let inp = grid40(seed, true);
        let out = place_labels(&mut f, &inp);
        assert!(out.total_cost <= base.total_cost + 1e-9);
        assert_valid(&inp, &out);
        assert_eq!(out.placed.len() + out.unplaced.len(), 40);
        outs.push(out);
    }
    assert_eq!(outs[0], place_labels(&mut f, &grid40(1, true)));
    assert_ne!(outs[0], outs[1]);
}

fn row_class(positions: Vec<PointPosition>) -> LabelClass {
    let mut c = class(0);
    c.placement = Placement::Point(PointPlacement {
        positions,
        offset: 2.0,
    });
    c
}

#[test]
fn improvement_solves_what_greedy_cannot() {
    let mut inp = input(
        vec![
            point(1, 0, "Abcdefgh", 60.0, 150.0, 0.0),
            point(2, 1, "Abcdefgh", 130.0, 150.0, 0.0),
            point(3, 2, "Abcdefgh", 190.0, 150.0, 0.0),
        ],
        vec![],
    );
    inp.classes = vec![
        row_class(vec![PointPosition::Left]),
        row_class(vec![PointPosition::Right, PointPosition::Left]),
        row_class(vec![PointPosition::Left]),
    ];
    inp.options.improve = false;
    let mut f = fonts();
    let greedy = place_labels(&mut f, &inp);
    assert_eq!(greedy.placed.len(), 2);
    inp.options.improve = true;
    let better = place_labels(&mut f, &inp);
    assert_eq!(better.placed.len(), 3);
    assert_valid(&inp, &better);
}

#[test]
fn high_priority_label_survives_every_seed() {
    for seed in 0..10 {
        let mut inp = input(
            vec![
                point(1, 0, "Low", 200.0, 150.0, 0.0),
                point(2, 1, "High", 200.0, 150.0, 0.0),
            ],
            vec![],
        );
        inp.classes = vec![
            {
                let mut c = row_class(vec![PointPosition::Right]);
                c.priority = 0;
                c
            },
            {
                let mut c = row_class(vec![PointPosition::Right]);
                c.priority = 5;
                c
            },
        ];
        inp.options.seed = seed;
        let out = place_labels(&mut fonts(), &inp);
        assert_eq!(out.placed.len(), 1);
        assert_eq!(out.placed[0].feature, 2);
    }
}

#[test]
fn duplicate_control() {
    let run = |d| {
        let mut inp = line_input(
            vec![line_class(0, LinePlacement::default())],
            vec![
                line_feature(1, 0, "Mississippi", vec![p(100.0, 200.0), p(700.0, 200.0)]),
                line_feature(2, 0, "Mississippi", vec![p(100.0, 230.0), p(700.0, 230.0)]),
            ],
            vec![],
        );
        inp.options.duplicate_distance = d;
        place_labels(&mut fonts(), &inp)
    };
    let out = run(None);
    assert_eq!(out.placed.len(), 2);
    let out = run(Some(1000.0));
    assert_eq!(out.placed.len(), 1);
    assert_eq!(out.unplaced.len(), 1);
    assert_eq!(out.unplaced[0].reason, UnplacedReason::Collision);
}

#[test]
fn speed_guard() {
    let mut rng = Rng(12345);
    let features: Vec<_> = (0..1500)
        .map(|i| {
            point(
                i,
                0,
                "Town",
                3.0 + rng.unit() * 2994.0,
                3.0 + rng.unit() * 1994.0,
                0.0,
            )
        })
        .collect();
    let mut inp = input(features, vec![]);
    inp.bounds = Rect::new(0.0, 0.0, 3000.0, 2000.0);
    let start = std::time::Instant::now();
    let out = place_labels(&mut fonts(), &inp);
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    assert_valid(&inp, &out);
    eprintln!("speed guard took {:?}", start.elapsed());
}
