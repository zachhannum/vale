mod common;

use std::f64::consts::PI;

use common::{SEED, check_image, check_snapshot, data_dir};
use kurbo::{Affine, Point};
use vale_labeler::{Labeling, PlacedLabel, place_labels};
use vale_labeler_demo::scene::{Preset, Scene, build, fonts};

fn run() -> (Scene, Labeling) {
    let scene = build(Preset::Conic, &data_dir(), SEED, true).unwrap();
    let labeling = place_labels(&mut fonts().unwrap(), &scene.input);
    (scene, labeling)
}

fn origin(t: Affine) -> Point {
    let c = t.as_coeffs();
    Point::new(c[4], c[5])
}

fn angle(t: Affine) -> f64 {
    let c = t.as_coeffs();
    c[1].atan2(c[0])
}

/// The difference of two angles, in (-PI, PI].
fn turn(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(2.0 * PI);
    if d > PI { d - 2.0 * PI } else { d }
}

/// The center of the circle through three points.
fn circumcenter(a: Point, b: Point, c: Point) -> Point {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let (a2, b2, c2) = (
        a.to_vec2().hypot2(),
        b.to_vec2().hypot2(),
        c.to_vec2().hypot2(),
    );
    Point::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    )
}

/// The image of the pole: the parallels of a conic projection are circles around it.
fn apex(scene: &Scene) -> Point {
    let line = scene
        .input
        .parallels
        .iter()
        .max_by_key(|l| l.len())
        .unwrap();
    let apex = circumcenter(line[0], line[line.len() / 2], *line.last().unwrap());
    assert!(apex.y < 0.0, "the pole is above the page: {apex:?}");
    apex
}

fn label<'a>(labeling: &'a Labeling, text: &str) -> &'a PlacedLabel {
    labeling
        .placed
        .iter()
        .find(|l| l.text == text)
        .unwrap_or_else(|| panic!("{text} not placed"))
}

#[test]
fn country_names_follow_the_parallels() {
    let (scene, labeling) = run();
    let apex = apex(&scene);
    for name in [
        "RUSSIA",
        "UKRAINE",
        "FRANCE",
        "SPAIN",
        "POLAND",
        "GERMANY",
        "ROMANIA",
        "TURKEY",
        "FINLAND",
        "KAZAKHSTAN",
    ] {
        let glyphs: Vec<Affine> = label(&labeling, name)
            .glyph_runs
            .iter()
            .flat_map(|r| r.glyphs.iter().map(|g| g.transform))
            .collect();
        // The baseline is on one parallel.
        let radii: Vec<f64> = glyphs.iter().map(|t| origin(*t).distance(apex)).collect();
        let (lo, hi) = radii
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), r| (a.min(*r), b.max(*r)));
        assert!(hi - lo < 1.0, "{name}: baseline radii {radii:?}");
        // Each glyph points east along the parallel.
        for t in &glyphs {
            let p = origin(*t) - apex.to_vec2();
            let east = (-p.x).atan2(p.y);
            assert!(
                turn(angle(*t), east).abs() < 0.03,
                "{name}: glyph angle {} east {east}",
                angle(*t)
            );
        }
        let first = origin(glyphs[0]);
        let last = origin(*glyphs.last().unwrap());
        assert!(last.x > first.x, "{name} reads to the right");
    }
}

#[test]
fn labels_are_on_the_page_and_do_not_overlap() {
    let (scene, labeling) = run();
    let boxes: Vec<_> = labeling
        .placed
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.boxes.iter().map(move |b| (i, b)))
        .collect();
    for (a, (i, ba)) in boxes.iter().enumerate() {
        assert!(ba.is_inside(scene.input.bounds), "box outside the page");
        for (j, bb) in &boxes[a + 1..] {
            if i != j {
                assert!(
                    !ba.intersects(bb),
                    "{} overlaps {}",
                    labeling.placed[*i].text,
                    labeling.placed[*j].text
                );
            }
        }
    }
}

#[test]
fn placement_snapshot() {
    let (_, labeling) = run();
    check_snapshot(&labeling, "conic-110m.placements.txt");
}

#[test]
fn reference_image() {
    let (scene, labeling) = run();
    check_image(&scene, &labeling, "conic-110m.png");
}
