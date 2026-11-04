mod common;

use common::{SEED, check_image, check_snapshot, data_dir, png_bytes};
use kurbo::Point;
use vale_labeler::{CandidateKind, Fonts, Labeling, PlacedLabel, glyph_outline, place_labels};
use vale_labeler_demo::render::WATER;
use vale_labeler_demo::scene::{Preset, Scene, build, fonts};
use vello_cpu::Pixmap;

fn setup() -> (Scene, Fonts) {
    let scene = build(Preset::World, &data_dir(), SEED, true).unwrap();
    (scene, fonts().unwrap())
}

fn run() -> (Scene, Labeling) {
    let (scene, mut fonts) = setup();
    let labeling = place_labels(&mut fonts, &scene.input);
    (scene, labeling)
}

#[test]
fn counts() {
    let (_, labeling) = run();
    let points = labeling.placed.iter().filter(|l| l.class != 1).count();
    let lines = labeling.placed.iter().filter(|l| l.class == 1).count();
    assert!(points >= 100, "point labels: {points}");
    assert!(lines >= 6, "line labels: {lines}");
}

/// The edges of the outline of each glyph of `label` in page space.
fn glyph_edges(label: &PlacedLabel) -> Vec<Vec<(Point, Point)>> {
    let mut out = Vec::new();
    for run in &label.glyph_runs {
        for g in &run.glyphs {
            let Some(path) = glyph_outline(&run.font, &run.normalized_coords, run.font_size, g.id)
            else {
                continue;
            };
            let mut polys: Vec<Vec<Point>> = Vec::new();
            kurbo::flatten(g.transform * path, 0.02, |el| match el {
                kurbo::PathEl::MoveTo(p) => polys.push(vec![p]),
                kurbo::PathEl::LineTo(p) => polys.last_mut().unwrap().push(p),
                _ => {}
            });
            let edges = polys
                .iter()
                .flat_map(|p| (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()])))
                .collect();
            out.push(edges);
        }
    }
    out
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let side = |p: Point, q: Point, r: Point| (q - p).cross(r - p);
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

#[test]
fn no_label_has_glyphs_that_overlap() {
    let (_, labeling) = run();
    let mut overlapping = Vec::new();
    for label in &labeling.placed {
        let glyphs = glyph_edges(label);
        // Each letter has a drawn glyph.
        let letters = label.text.chars().filter(|c| !c.is_whitespace()).count();
        assert_eq!(glyphs.len(), letters, "{}", label.text);
        let crossing = (0..glyphs.len()).any(|i| {
            (i + 1..glyphs.len()).any(|j| {
                glyphs[i]
                    .iter()
                    .any(|&(a, b)| glyphs[j].iter().any(|&(c, d)| segments_cross(a, b, c, d)))
            })
        });
        if crossing {
            overlapping.push(label.text.clone());
        }
    }
    assert!(overlapping.is_empty(), "{overlapping:?}");
}

#[test]
fn validity() {
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
    assert!(labeling.placed.len() + labeling.unplaced.len() >= scene.input.features.len());
}

#[test]
fn well_known_names() {
    let (_, labeling) = run();
    let has = |n: &str| labeling.placed.iter().any(|l| l.text == n);
    for n in ["Tokyo", "Cairo", "Brasília"] {
        assert!(has(n), "{n} not placed");
    }
    assert!(
        ["Amazonas", "Nile", "Mississippi", "Volga", "Yangtze"]
            .iter()
            .any(|n| has(n)),
        "no well-known river placed"
    );
}

#[test]
fn country_and_ocean_names() {
    let (_, labeling) = run();
    let kind = |n: &str| {
        labeling
            .placed
            .iter()
            .find(|l| l.text == n)
            .unwrap_or_else(|| panic!("{n} not placed"))
            .kind
    };
    for n in ["RUSSIA", "CANADA", "INDIAN OCEAN", "SOUTH ATLANTIC OCEAN"] {
        kind(n);
    }
    assert_eq!(kind("BRAZIL"), CandidateKind::Polygon { curved: false });
    assert_eq!(kind("ARGENTINA"), CandidateKind::Polygon { curved: true });
}

#[test]
fn deterministic() {
    let (scene, mut fonts) = setup();
    let a = place_labels(&mut fonts, &scene.input);
    let b = place_labels(&mut fonts, &scene.input);
    assert_eq!(a, b);
    assert_eq!(png_bytes(&scene, &a), png_bytes(&scene, &b));
}

#[test]
fn png_basics() {
    let (scene, labeling) = run();
    let bytes = png_bytes(&scene, &labeling);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let pm = Pixmap::from_png(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!((pm.width(), pm.height()), (1800, 900));
    let p = pm.sample(5, 5);
    assert_eq!([p.r, p.g, p.b], WATER);
    let dark = pm
        .data()
        .iter()
        .filter(|p| p.r < 80 && p.g < 80 && p.b < 80)
        .count();
    assert!(dark >= 2000, "dark pixels: {dark}");
}

#[test]
fn placement_snapshot() {
    let (_, labeling) = run();
    check_snapshot(&labeling, "world-110m.placements.txt");
}

#[test]
fn reference_image() {
    let (scene, labeling) = run();
    check_image(&scene, &labeling, "world-110m.png");
}
