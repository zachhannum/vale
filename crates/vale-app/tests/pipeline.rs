use std::path::Path;

use kurbo::Point;
use vale_app::document::Document;
use vale_app::headless::{export, map_png, probe};
use vale_app::pipeline::{Pipeline, Quality, fonts};
use vale_render::{FillRule, Item};
use vale_sphere::{ProjectionKind, ProjectionSpec};

const SIZE: (f64, f64) = (1280.0, 800.0);
const LAND: [u8; 3] = [244, 241, 232];
const WATER: [u8; 3] = [214, 232, 242];
const BACKGROUND: [u8; 3] = [236, 236, 232];

fn plain() -> Document {
    let mut d = Document::sample();
    d.frame.labels = false;
    d.frame.graticule = false;
    d
}

fn color_at(doc: &Document, at: [f64; 2]) -> Option<[u8; 3]> {
    probe(doc, SIZE, 1.0, at).unwrap().map(|(_, c)| c)
}

fn compose(doc: &Document, pipeline: &mut Pipeline) -> vale_app::pipeline::Composed {
    let mut f = fonts().unwrap();
    pipeline
        .compose(doc, SIZE, &mut f, Quality::Final, None)
        .unwrap()
}

fn strokes(c: &vale_app::pipeline::Composed) -> usize {
    c.list
        .items
        .iter()
        .filter(|i| matches!(i, Item::Stroke { .. }))
        .count()
}

#[test]
fn compose_basics() {
    let doc = plain();
    let mut p = Pipeline::new();
    let c = compose(&doc, &mut p);
    assert_eq!((c.list.width, c.list.height), SIZE);
    assert!(
        matches!(c.list.items[0], Item::Fill { rule: FillRule::NonZero, color, .. }
        if color == doc.frame.style.background)
    );
    assert_eq!(c.view, p.resolve_view(&doc, SIZE).unwrap());
}

#[test]
fn colors() {
    let mut doc = plain();
    assert_eq!(color_at(&doc, [25.0, 25.0]), Some(LAND));
    assert_eq!(color_at(&doc, [-27.0, -22.0]), Some(WATER));
    let img = map_png(&doc, SIZE, 1.0).unwrap();
    let decoded = image::load_from_memory(&img.png).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(2, 2).0[..3], BACKGROUND);
    let land = doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Land")
        .unwrap()
        .id;
    doc.entry_mut(land).unwrap().visible = false;
    assert_eq!(color_at(&doc, [25.0, 25.0]), Some(WATER));
}

#[test]
fn orthographic_hides_the_far_side() {
    let mut doc = plain();
    doc.set_projection(ProjectionSpec {
        kind: ProjectionKind::Orthographic,
        lon0: 20.0,
        lat0: 30.0,
    });
    assert_eq!(color_at(&doc, [-150.0, -30.0]), None);
    assert_eq!(color_at(&doc, [25.0, 25.0]), Some(LAND));
}

#[test]
fn radius_change_keeps_the_picture() {
    let mut doc = plain();
    let mut p = Pipeline::new();
    let tokyo = [139.7, 35.7];
    let view = p.resolve_view(&doc, SIZE).unwrap();
    doc.frame.view = Some(view);
    let before = p.lonlat_to_page(&doc, SIZE, tokyo).unwrap().unwrap();
    let k0 = compose(&doc, &mut p).km_per_point;
    doc.set_radius_km(3000.0);
    let after = p.lonlat_to_page(&doc, SIZE, tokyo).unwrap().unwrap();
    assert!((after - before).hypot() < 1e-6);
    let k1 = compose(&doc, &mut p).km_per_point;
    assert!(((k1 / k0) / (3000.0 / 6371.0) - 1.0).abs() < 1e-9);
}

#[test]
fn graticule_step_and_items() {
    let mut doc = plain();
    let mut p = Pipeline::new();
    let off = compose(&doc, &mut p);
    assert_eq!(off.graticule_step, None);
    doc.frame.graticule = true;
    let on = compose(&doc, &mut p);
    assert_eq!(on.graticule_step, Some(30.0));
    assert_eq!(strokes(&on), strokes(&off) + 1);
    let mut v = on.view;
    v.scale *= 8.0;
    doc.frame.view = Some(v);
    let zoomed = compose(&doc, &mut p);
    assert!(zoomed.graticule_step.unwrap() < 30.0);
}

#[test]
fn lonlat_round_trip() {
    let doc = plain();
    let mut p = Pipeline::new();
    let tokyo = [139.7, 35.7];
    let q = p.lonlat_to_page(&doc, SIZE, tokyo).unwrap().unwrap();
    let back = p.page_to_lonlat(&doc, SIZE, q).unwrap().unwrap();
    assert!((back[0] - tokyo[0]).abs() < 1e-6 && (back[1] - tokyo[1]).abs() < 1e-6);
    assert!(
        p.page_to_lonlat(&doc, SIZE, Point::new(1.0, 1.0))
            .unwrap()
            .is_none()
    );
}

#[test]
fn pixel_ratio_and_determinism() {
    let doc = plain();
    let a = map_png(&doc, SIZE, 2.0).unwrap();
    assert_eq!((a.width, a.height), (2560, 1600));
    let b = map_png(&doc, SIZE, 2.0).unwrap();
    assert_eq!(a.png, b.png);
}

#[test]
fn export_formats() {
    let doc = plain();
    let pdf = Path::new("../../target/app/test-export.pdf");
    export(&doc, SIZE, 1.0, pdf).unwrap();
    assert!(std::fs::read(pdf).unwrap().starts_with(b"%PDF-"));
    let err = export(&doc, SIZE, 1.0, Path::new("x.svg")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("png") && msg.contains("pdf"), "{msg}");
}

#[test]
fn one_layer_of_each_kind() {
    let fc = |g: &str| {
        format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{g}}}]}}"#
        )
    };
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    for (name, g) in [
        (
            "poly",
            r#"{"type":"Polygon","coordinates":[[[0,0],[40,0],[40,40],[0,40],[0,0]]]}"#,
        ),
        (
            "line",
            r#"{"type":"LineString","coordinates":[[-40,-40],[-10,-10]]}"#,
        ),
        ("pt", r#"{"type":"Point","coordinates":[20,-20]}"#),
    ] {
        let imported = vale_import::geojson::read_str(name, &fc(g)).unwrap();
        doc.add_imported(imported, None);
    }
    let mut p = Pipeline::new();
    let c = compose(&doc, &mut p);
    let kinds: Vec<_> = c
        .list
        .items
        .iter()
        .map(|i| match i {
            Item::Fill { rule, .. } => format!("fill {rule:?}"),
            Item::Stroke { round, .. } => format!("stroke {round}"),
            Item::Glyphs(_) => "glyphs".to_string(),
        })
        .collect();
    assert!(kinds.contains(&"fill EvenOdd".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"stroke true".to_string()), "{kinds:?}");
    // Page fill, water, polygon fill, point fill.
    assert!(
        kinds.iter().filter(|k| k.starts_with("fill")).count() >= 4,
        "{kinds:?}"
    );
}
