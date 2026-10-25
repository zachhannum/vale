use std::path::Path;

use kurbo::{Point, Shape};
use vale_app::document::Document;
use vale_app::headless::{export, map_png, probe};
use vale_app::pipeline::{Pipeline, Quality, fonts};
use vale_render::{FillRule, Item};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_style::expr::{Filter, Property};
use vale_style::{Color, Rule, Stroke, StrokeSymbolizer, Style, Symbolizer};

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
    doc.set_visible(land, false);
    assert_eq!(color_at(&doc, [25.0, 25.0]), Some(WATER));
}

#[test]
fn orthographic_hides_the_far_side() {
    let mut doc = plain();
    doc.set_projection(ProjectionSpec::new(
        ProjectionKind::Orthographic,
        20.0,
        30.0,
    ));
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

/// A short description of each item, with the color and the width.
fn describe(item: &Item) -> String {
    match item {
        Item::Fill { color, rule, .. } => {
            format!(
                "fill {rule:?} {} {} {} {}",
                color.r, color.g, color.b, color.a
            )
        }
        Item::Stroke {
            color,
            width,
            round,
            ..
        } => format!(
            "stroke {round} {} {} {} {} {width}",
            color.r, color.g, color.b, color.a
        ),
        Item::Glyphs(_) => "glyphs".to_string(),
    }
}

#[test]
fn default_styles_draw_the_sample_as_one_batch_per_paint() {
    let doc = plain();
    let c = compose(&doc, &mut Pipeline::new());
    let mut items: Vec<String> = c.list.items.iter().map(describe).collect();
    // Land: one fill for each land polygon on the page.
    let land = "fill EvenOdd 244 241 232 255";
    let lands = items.iter().filter(|i| *i == land).count();
    assert!(lands > 10, "{items:?}");
    items.dedup();
    // The scale bar follows.
    assert_eq!(
        items[..8],
        [
            "fill NonZero 236 236 232 255",
            "fill NonZero 214 232 242 255",
            land,
            "stroke false 150 160 165 255 0.6",
            "stroke true 90 150 200 255 1.2",
            "fill NonZero 60 60 60 255",
            "stroke true 255 255 255 255 1",
            "stroke false 90 100 105 255 0.8",
        ]
    );
}

/// A document with one line layer. Each line has a `kind` and a `flow`.
fn lines(features: &[(&str, f64)]) -> (Document, vale_store::LayerId) {
    let features: Vec<String> = features
        .iter()
        .enumerate()
        .map(|(i, (kind, flow))| {
            let y = -40.0 + 10.0 * i as f64;
            format!(
                r#"{{"type":"Feature","properties":{{"kind":"{kind}","flow":{flow}}},"geometry":{{"type":"LineString","coordinates":[[-40,{y}],[40,{y}]]}}}}"#
            )
        })
        .collect();
    let text = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    let imported = vale_import::geojson::read_str("lines", &text).unwrap();
    let id = doc.add_imported(imported, None)[0];
    (doc, id)
}

/// The width and the color of each round stroke: the strokes of the line layer.
fn line_strokes(doc: &Document) -> Vec<(f64, [u8; 3])> {
    compose(doc, &mut Pipeline::new())
        .list
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Stroke {
                color,
                width,
                round: true,
                ..
            } => Some((*width, [color.r, color.g, color.b])),
            _ => None,
        })
        .collect()
}

fn stroke_rule(name: &str, filter: &str, color: Color, width: f64) -> Rule {
    let mut rule = Rule::new(
        name,
        vec![Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke {
            color,
            width,
        }))],
    );
    rule.filter = Filter::new(filter);
    rule
}

#[test]
fn rules_with_filters_draw_their_features_in_their_own_colors() {
    let (mut doc, id) = lines(&[("major", 1.0), ("minor", 1.0), ("major", 1.0)]);
    let (red, blue) = (Color::rgb(200, 0, 0), Color::rgb(0, 0, 200));
    doc.entry_mut(id).unwrap().style.style = Style {
        rules: vec![
            stroke_rule("Minor", "kind = 'minor'", blue, 1.0),
            stroke_rule("Major", "kind = 'major'", red, 3.0),
        ],
    };
    // Rule by rule: the minor line, then the two major lines in one batch.
    assert_eq!(line_strokes(&doc), [(1.0, [0, 0, 200]), (3.0, [200, 0, 0])]);

    // A rule draws nothing at a scale denominator from its maximum.
    let style = &mut doc.entry_mut(id).unwrap().style.style;
    style.rules[1].max_scale = Some(1000.0);
    assert_eq!(line_strokes(&doc), [(1.0, [0, 0, 200])]);
}

#[test]
fn a_width_expression_follows_the_attribute() {
    let (mut doc, id) = lines(&[("a", 2.0), ("a", 6.0), ("a", 2.0), ("a", 4.0)]);
    let blue = Color::rgb(0, 0, 200);
    doc.entry_mut(id).unwrap().style.style = Style {
        rules: vec![Rule::new(
            "Flow",
            vec![Symbolizer::Stroke(StrokeSymbolizer {
                color: Property::constant(blue),
                width: Property::expression("flow * 0.5", 1.0),
            })],
        )],
    };
    // One batch for each width, in the order of the features.
    let widths: Vec<f64> = line_strokes(&doc).iter().map(|(w, _)| *w).collect();
    assert_eq!(widths, [1.0, 3.0, 2.0]);
}

/// The presets of each kind with the center away from 0, and two oblique aspects.
fn each_projection() -> Vec<ProjectionSpec> {
    let mut specs: Vec<ProjectionSpec> = ProjectionKind::ALL
        .into_iter()
        .map(|kind| ProjectionSpec::new(kind, 20.0, 30.0).normalized())
        .collect();
    for kind in [ProjectionKind::Mercator, ProjectionKind::EqualEarth] {
        let oblique = ProjectionSpec {
            oblique: Some(35.0),
            ..ProjectionSpec::new(kind, 20.0, 30.0)
        };
        specs.push(oblique.normalized());
    }
    specs
}

#[test]
fn each_projection_draws_the_map_inside_the_page() {
    let page = kurbo::Rect::new(0.0, 0.0, SIZE.0, SIZE.1).inflate(2.0, 2.0);
    for spec in each_projection() {
        let mut doc = plain();
        doc.set_projection(spec);
        let c = compose(&doc, &mut Pipeline::new());
        let mut fills = 0;
        for item in &c.list.items {
            let path = match item {
                Item::Fill { path, .. } => {
                    fills += 1;
                    path
                }
                Item::Stroke { path, .. } => path,
                Item::Glyphs(_) => continue,
            };
            let b = path.bounding_box();
            let finite = [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite());
            assert!(finite && page.contains_rect(b), "{spec:?}: {b:?}");
        }
        // The page, the sea, and land.
        assert!(fills > 2, "{spec:?}: {fills} fills");
        assert_eq!(color_at(&doc, [25.0, 25.0]), Some(LAND), "{spec:?}");
        map_png(&doc, SIZE, 1.0).unwrap();
    }
}
