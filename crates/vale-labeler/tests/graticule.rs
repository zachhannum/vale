//! Labels that follow the parallels of the graticule.

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

const PAGE: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

/// The apex of a conic projection, above the page.
const APEX: Point = Point::new(400.0, -600.0);

fn class(placement: Placement) -> LabelClass {
    LabelClass {
        name: "c".into(),
        style: TextStyle::new("Noto Sans", 14.0),
        priority: 0,
        margin: 0.0,
        placement,
        duplicate_distance: None,
    }
}

fn run(
    placement: Placement,
    geometry: Geometry,
    text: &str,
    parallels: Vec<Vec<Point>>,
) -> Labeling {
    let input = LabelInput {
        bounds: PAGE,
        classes: vec![class(placement)],
        features: vec![Feature {
            id: 1,
            class: 0,
            text: text.into(),
            priority: 0.0,
            geometry,
        }],
        obstacles: vec![],
        parallels,
        options: Options::default(),
    };
    place_labels(&mut fonts(), &input)
}

/// A point on the circle of radius `r` around the apex, at the angle `a` in degrees.
fn polar(r: f64, a: f64) -> Point {
    let a = a.to_radians();
    Point::new(APEX.x + r * a.cos(), APEX.y + r * a.sin())
}

/// The parallels of a conic map: arcs around the apex, one each 10 units, from west to east.
fn conic() -> Vec<Vec<Point>> {
    (60..=140)
        .map(|k| {
            let r = 10.0 * k as f64;
            (0..=120)
                .map(|i| polar(r, 140.0 - i as f64 * 100.0 / 120.0))
                .collect()
        })
        .collect()
}

/// A ring between two parallels of `conic` and two meridians.
fn sector(r0: f64, r1: f64, a0: f64, a1: f64) -> Vec<Point> {
    let n = 60;
    let at = |k: usize| a0 + (a1 - a0) * k as f64 / n as f64;
    let mut ring: Vec<Point> = (0..=n).map(|k| polar(r0, at(k))).collect();
    ring.extend((0..=n).rev().map(|k| polar(r1, at(k))));
    ring
}

fn origin(t: Affine) -> Point {
    let c = t.as_coeffs();
    Point::new(c[4], c[5])
}

fn angle(t: Affine) -> f64 {
    let c = t.as_coeffs();
    c[1].atan2(c[0])
}

fn glyphs(l: &PlacedLabel) -> Vec<Affine> {
    l.glyph_runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.transform))
        .collect()
}

/// The difference of two angles, in (-PI, PI].
fn turn(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(2.0 * PI);
    if d > PI { d - 2.0 * PI } else { d }
}

/// Asserts that the baseline of `l` is an arc around the apex, and that each glyph turns
/// with it, to the right on the page.
fn assert_on_arc(l: &PlacedLabel) {
    let g = glyphs(l);
    let radii: Vec<f64> = g.iter().map(|t| origin(*t).distance(APEX)).collect();
    let (lo, hi) = radii
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), r| (a.min(*r), b.max(*r)));
    assert!(hi - lo < 1.0, "baseline radii {radii:?}");
    for t in &g {
        let p = origin(*t) - APEX.to_vec2();
        // The direction of the parallel to the east: clockwise around the apex, y down.
        let east = (-p.x).atan2(p.y);
        assert!(
            turn(angle(*t), east).abs() < 0.05,
            "glyph angle {} east {east}",
            angle(*t)
        );
    }
    let first = origin(g[0]);
    let last = origin(*g.last().unwrap());
    assert!(last.x > first.x, "the text reads to the right");
}

fn parallel_mode() -> PolygonPlacement {
    PolygonPlacement {
        mode: PolygonMode::Parallel,
        ..Default::default()
    }
}

#[test]
fn a_polygon_label_follows_a_curved_parallel() {
    let ring = sector(850.0, 950.0, 75.0, 105.0);
    let out = run(
        Placement::Polygon(parallel_mode()),
        Geometry::Polygon { rings: vec![ring] },
        "PARALLELIA",
        conic(),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    assert_eq!(out.placed[0].kind, CandidateKind::Polygon { curved: true });
    assert_on_arc(&out.placed[0]);
}

/// A thin wide bar on top of a tall block. The visual center is in the block, but the bar has
/// the most room along a parallel.
fn bar_and_block() -> Vec<Point> {
    vec![
        Point::new(100.0, 100.0),
        Point::new(700.0, 100.0),
        Point::new(700.0, 150.0),
        Point::new(500.0, 150.0),
        Point::new(500.0, 450.0),
        Point::new(300.0, 450.0),
        Point::new(300.0, 150.0),
        Point::new(100.0, 150.0),
    ]
}

/// Straight parallels, one each 10 units.
fn straight() -> Vec<Vec<Point>> {
    (0..=60)
        .map(|k| {
            let y = 10.0 * k as f64;
            vec![Point::new(-10.0, y), Point::new(810.0, y)]
        })
        .collect()
}

#[test]
fn a_polygon_label_uses_the_parallel_with_the_most_room() {
    for parallels in [straight(), Vec::new()] {
        let out = run(
            Placement::Polygon(parallel_mode()),
            Geometry::Polygon {
                rings: vec![bar_and_block()],
            },
            "Long Bar",
            parallels,
        );
        assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
        let l = &out.placed[0];
        let c = l.bounds.center();
        assert!(c.y > 100.0 && c.y < 150.0, "the label is in the bar: {c:?}");
        for t in glyphs(l) {
            assert!(angle(t).abs() < 1e-6, "the text is straight");
        }
    }
}

#[test]
fn the_letters_spread_along_the_parallel_up_to_the_limit_of_the_class() {
    let width = |max_spread: f64| {
        let out = run(
            Placement::Polygon(PolygonPlacement {
                max_spread,
                ..parallel_mode()
            }),
            Geometry::Polygon {
                rings: vec![bar_and_block()],
            },
            "Bar",
            straight(),
        );
        assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
        out.placed[0].bounds.width()
    };
    let tight = width(0.0);
    let limited = width(2.0);
    let open = width(100.0);
    assert!(tight < 40.0, "no spread: {tight}");
    // Two gaps of at most 2 font sizes each.
    assert!(
        limited > tight + 50.0 && limited <= tight + 2.0 * 2.0 * 14.0 + 1.0,
        "{limited}"
    );
    // The text fills most of the 600 units of the bar.
    assert!(open > 400.0 && open < 600.0, "{open}");
}

#[test]
fn a_point_label_follows_the_parallel_through_the_point() {
    let p = polar(900.0, 90.0);
    let out = run(
        Placement::Point(PointPlacement {
            positions: vec![PointPosition::Right],
            graticule: true,
            ..Default::default()
        }),
        Geometry::Point {
            position: p,
            symbol_radius: 3.0,
        },
        "Pointville on the arc",
        conic(),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    let l = &out.placed[0];
    assert_on_arc(l);
    let first = origin(glyphs(l)[0]);
    assert!(first.x > p.x, "the text starts to the right of the point");
    // The text centers on the parallel through the point: the baseline is below it by about
    // half the cap height.
    let r = first.distance(APEX);
    assert!(r > 900.0 && r < 910.0, "baseline radius {r}");
}

#[test]
fn a_horizontal_polygon_label_follows_the_parallels() {
    let ring = sector(800.0, 1000.0, 80.0, 100.0);
    let out = run(
        Placement::Polygon(PolygonPlacement {
            mode: PolygonMode::Horizontal,
            graticule: true,
            ..Default::default()
        }),
        Geometry::Polygon { rings: vec![ring] },
        "Arcland",
        conic(),
    );
    assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
    assert_eq!(out.placed[0].kind, CandidateKind::Polygon { curved: false });
    assert_on_arc(&out.placed[0]);
}

/// Straight parallels that run from west to east to the left on the page, as below the pole
/// of a polar map.
fn reversed() -> Vec<Vec<Point>> {
    straight()
        .into_iter()
        .map(|mut p| {
            p.reverse();
            p
        })
        .collect()
}

#[test]
fn the_text_stays_upright_unless_the_class_allows_flipped_text() {
    for no_flip in [false, true] {
        let point = run(
            Placement::Point(PointPlacement {
                graticule: true,
                no_flip,
                ..Default::default()
            }),
            Geometry::Point {
                position: Point::new(400.0, 305.0),
                symbol_radius: 2.0,
            },
            "Upright",
            reversed(),
        );
        let polygon = run(
            Placement::Polygon(PolygonPlacement {
                no_flip,
                ..parallel_mode()
            }),
            Geometry::Polygon {
                rings: vec![bar_and_block()],
            },
            "Upright",
            reversed(),
        );
        for out in [point, polygon] {
            assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
            for t in glyphs(&out.placed[0]) {
                let want = if no_flip { PI } else { 0.0 };
                assert!(
                    turn(angle(t), want).abs() < 1e-6,
                    "no_flip {no_flip}: {}",
                    angle(t)
                );
            }
        }
    }
}

#[test]
fn a_parallel_far_longer_than_the_page_gives_the_same_label_quickly() {
    // At a large scale, a parallel is millions of units long on the page.
    let long: Vec<Vec<Point>> = (0..=60)
        .map(|k| {
            let y = 10.0 * k as f64;
            vec![Point::new(-1.0e8, y), Point::new(1.0e8, y)]
        })
        .collect();
    let label = |parallels| {
        let out = run(
            Placement::Polygon(parallel_mode()),
            Geometry::Polygon {
                rings: vec![bar_and_block()],
            },
            "Long Bar",
            parallels,
        );
        assert_eq!(out.placed.len(), 1, "{:?}", out.unplaced);
        out.placed[0].bounds
    };
    let start = std::time::Instant::now();
    let far = label(long);
    assert!(start.elapsed().as_secs_f64() < 5.0, "{:?}", start.elapsed());
    // The samples along each parallel start at its start, so the labels can differ by less
    // than a step of half a cell.
    let near = label(straight());
    assert!(
        (far.center() - near.center()).hypot() < 2.0,
        "{far:?} {near:?}"
    );
}
