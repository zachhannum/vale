//! Helpers for the reference tests of the presets.

use std::path::{Path, PathBuf};

use vale_labeler::{CandidateKind, Labeling};
use vale_labeler_demo::render::{RenderOptions, render};
use vale_labeler_demo::scene::Scene;
use vello_cpu::Pixmap;

pub const SEED: u64 = 24301;

pub fn data_dir() -> PathBuf {
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

pub fn png_bytes(scene: &Scene, labeling: &Labeling) -> Vec<u8> {
    render(scene, labeling, &RenderOptions { debug_boxes: false })
        .into_png()
        .unwrap()
}

fn snapshot(labeling: &Labeling) -> String {
    let mut out = String::new();
    for l in &labeling.placed {
        let kind = match l.kind {
            CandidateKind::Point(p) => format!("{p:?}"),
            CandidateKind::Line { side, .. } => format!("Line{side:?}"),
            CandidateKind::Polygon { curved: true } => "Curved".to_string(),
            CandidateKind::Polygon { curved: false } => "Horizontal".to_string(),
        };
        let c = l.bounds.center();
        out.push_str(&format!(
            "{}\t{}\t{}\t{:.1}\t{:.1}\n",
            l.feature, l.text, kind, c.x, c.y
        ));
    }
    out
}

/// Compares the placements with the reference file `name`.
pub fn check_snapshot(labeling: &Labeling, name: &str) {
    let text = snapshot(labeling);
    let path = reference(name);
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

/// Compares the image with the reference file `name`. At most 0.2% of the pixels can
/// differ by more than 16 in a channel.
pub fn check_image(scene: &Scene, labeling: &Labeling, name: &str) {
    let bytes = png_bytes(scene, labeling);
    let path = reference(name);
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
        let out = actual(&name.replace(".png", ".actual.png"));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &bytes).unwrap();
        panic!(
            "{differ} of {total} pixels differ. New image written to {}",
            out.display()
        );
    }
}
