use std::path::{Path, PathBuf};

use vale_labeler::{CandidateKind, Fonts, Labeling, place_labels};
use vale_labeler_demo::render::{RenderOptions, WATER, render};
use vale_labeler_demo::scene::{Preset, Scene, build, fonts};
use vello_cpu::Pixmap;

const SEED: u64 = 24301;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/natural-earth")
}

fn reference(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/reference")
        .join(name)
}

fn actual(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/demo/{name}"))
}

fn update() -> bool {
    std::env::var("VALE_UPDATE_REFERENCE").is_ok_and(|v| v == "1")
}

fn setup() -> (Scene, Fonts) {
    let scene = build(Preset::World, &data_dir(), SEED, true).unwrap();
    (scene, fonts().unwrap())
}

fn run() -> (Scene, Labeling) {
    let (scene, mut fonts) = setup();
    let labeling = place_labels(&mut fonts, &scene.input);
    (scene, labeling)
}

fn png_bytes(scene: &Scene, labeling: &Labeling) -> Vec<u8> {
    render(scene, labeling, &RenderOptions { debug_boxes: false })
        .into_png()
        .unwrap()
}

#[test]
fn counts() {
    let (_, labeling) = run();
    let points = labeling.placed.iter().filter(|l| l.class != 1).count();
    let lines = labeling.placed.iter().filter(|l| l.class == 1).count();
    assert!(points >= 100, "point labels: {points}");
    assert!(lines >= 6, "line labels: {lines}");
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

fn snapshot(labeling: &Labeling) -> String {
    let mut out = String::new();
    for l in &labeling.placed {
        let kind = match l.kind {
            CandidateKind::Point(p) => format!("{p:?}"),
            CandidateKind::Line { side, .. } => format!("Line{side:?}"),
        };
        let c = l.bounds.center();
        out.push_str(&format!(
            "{}\t{}\t{}\t{:.1}\t{:.1}\n",
            l.feature, l.text, kind, c.x, c.y
        ));
    }
    out
}

#[test]
fn placement_snapshot() {
    let (_, labeling) = run();
    let text = snapshot(&labeling);
    let path = reference("world-110m.placements.txt");
    if update() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("missing reference snapshot");
    assert_eq!(
        text, expected,
        "placements changed. Review them, then run with VALE_UPDATE_REFERENCE=1"
    );
}

#[test]
fn reference_image() {
    let (scene, labeling) = run();
    let bytes = png_bytes(&scene, &labeling);
    let path = reference("world-110m.png");
    if update() {
        std::fs::write(&path, &bytes).unwrap();
        return;
    }
    let expected_bytes = std::fs::read(&path).expect("missing reference image");
    let expected = Pixmap::from_png(std::io::Cursor::new(&expected_bytes)).unwrap();
    let got = Pixmap::from_png(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(
        (got.width(), got.height()),
        (expected.width(), expected.height())
    );
    let differ = got
        .data()
        .iter()
        .zip(expected.data())
        .filter(|(a, b)| {
            a.r.abs_diff(b.r) > 16
                || a.g.abs_diff(b.g) > 16
                || a.b.abs_diff(b.b) > 16
                || a.a.abs_diff(b.a) > 16
        })
        .count();
    let total = got.data().len();
    if differ * 1000 > total * 2 {
        let out = actual("world-110m.actual.png");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &bytes).unwrap();
        panic!(
            "{differ} of {total} pixels differ. New image written to {}",
            out.display()
        );
    }
}
