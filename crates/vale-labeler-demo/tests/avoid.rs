mod common;

use common::{SEED, check_image, check_snapshot, data_dir};
use kurbo::Point;
use vale_labeler::{Geometry, Labeling, place_labels};
use vale_labeler_demo::scene::{Preset, Scene, build, fonts};

fn run() -> (Scene, Labeling) {
    let scene = build(Preset::Avoid, &data_dir(), SEED, true).unwrap();
    let labeling = place_labels(&mut fonts().unwrap(), &scene.input);
    (scene, labeling)
}

/// The segments of the rivers and the borders, each with the ID of its feature.
fn lines(scene: &Scene) -> Vec<(Point, Point, u64)> {
    let mut out = Vec::new();
    for f in &scene.input.features {
        match &f.geometry {
            Geometry::Line { path, .. } => {
                out.extend(path.windows(2).map(|w| (w[0], w[1], f.id)));
            }
            Geometry::Polygon { rings } if f.class == 3 => {
                for r in rings {
                    let n = r.len();
                    out.extend((0..n).map(|k| (r[k], r[(k + 1) % n], f.id)));
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn labels_do_not_cross_rivers_or_borders() {
    let (scene, labeling) = run();
    let lines = lines(&scene);
    // Rivers that join into one label have more than one feature.
    let text_of = |id: u64| {
        let f = scene.input.features.iter().find(|f| f.id == id).unwrap();
        (f.class, f.text.clone())
    };
    let mut crossings = Vec::new();
    for l in &labeling.placed {
        for &(a, b, id) in &lines {
            if id == l.feature || text_of(id) == (l.class, l.text.clone()) {
                continue;
            }
            if l.boxes.iter().any(|x| x.intersects_segment(a, b)) {
                crossings.push((l.text.clone(), id));
                break;
            }
        }
    }
    assert!(crossings.is_empty(), "{crossings:?}");
    assert!(
        labeling.placed.len() >= 60,
        "placed: {}",
        labeling.placed.len()
    );
}

#[test]
fn placement_snapshot() {
    let (_, labeling) = run();
    check_snapshot(&labeling, "avoid-110m.placements.txt");
}

#[test]
fn reference_image() {
    let (scene, labeling) = run();
    check_image(&scene, &labeling, "avoid-110m.png");
}
