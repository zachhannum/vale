mod common;
mod masked;

use std::path::Path;

use kurbo::{Point, Shape};
use vale_app::atlas::{
    FrameTarget, PT_PER_MM, auto_graticule_interval, coord_text, frame_transform,
    graticule_interval, new_frame,
};
use vale_app::document::Document;
use vale_app::edge_labels::{EdgeLabel, Side, edge_labels, outside_reach, quads_overlap};
use vale_app::furniture::{self, Purpose};
use vale_app::headless::{export, map_png, probe};
use vale_app::pipeline::{PAPER, Pipeline, Quality, fonts};
use vale_app::scale_bar;
use vale_render::{FillRule, Item, render_pixmap, render_rgba};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{
    ColorKeyItem, CoordFormat, EdgeOrientation, FrameId, FrameShape, Furniture, FurnitureId,
    FurnitureText, KeyLayout, LegendEntry, LegendItem, NorthArrowItem, PageId, PageUnits,
    RasterDisplay, ScaleBarItem, ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, SecondUnit,
    TextCase, TextItem, TickFrequency,
};
use vale_style::expr::{Filter, Property};
use vale_style::{
    Color, Hatch, MarkerShape, Mask, PointSymbolizer, Rule, ScatterSymbolizer, Stroke, StrokeKind,
    StrokeSymbolizer, Style, Symbolizer,
};

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
    pipeline.compose(doc, SIZE, &mut f, Quality::Final).unwrap()
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
    doc.set_projection(
        doc.first_map(),
        ProjectionSpec::new(ProjectionKind::Orthographic, 20.0, 30.0),
    );
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
            Item::Image { .. } => "image".to_string(),
            Item::Rasterize { .. } => "rasterize".to_string(),
            Item::Layer { .. } => "layer".to_string(),
            Item::PushClip { .. } => "push clip".to_string(),
            Item::PopClip => "pop clip".to_string(),
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
        Item::Image { .. } => "image".to_string(),
        Item::Rasterize { .. } => "rasterize".to_string(),
        Item::Layer { alpha, .. } => format!("layer {alpha}"),
        Item::PushClip { rule, .. } => format!("push clip {rule:?}"),
        Item::PopClip => "pop clip".to_string(),
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
        vec![Symbolizer::Stroke(StrokeSymbolizer::constant(
            Stroke::solid(color, width),
        ))],
    );
    rule.filter = Filter::new(filter);
    rule
}

#[test]
fn rules_with_filters_draw_their_features_in_their_own_colors() {
    let (mut doc, id) = lines(&[("major", 1.0), ("minor", 1.0), ("major", 1.0)]);
    let (red, blue) = (Color::rgb(200, 0, 0), Color::rgb(0, 0, 200));
    doc.entry_mut(id).unwrap().style.style = Style {
        mask: None,
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
        mask: None,
        rules: vec![Rule::new(
            "Flow",
            vec![Symbolizer::Stroke(StrokeSymbolizer {
                color: Property::constant(blue),
                width: Property::expression("flow * 0.5", 1.0),
                kind: StrokeKind::Solid,
            })],
        )],
    };
    // One batch for each width, in the order of the features.
    let widths: Vec<f64> = line_strokes(&doc).iter().map(|(w, _)| *w).collect();
    assert_eq!(widths, [1.0, 3.0, 2.0]);
}

/// A page and a frame of map 0 on it.
fn frame_on(doc: &mut Document, pipeline: &mut Pipeline, page: PageId) -> FrameId {
    let atlas = &doc.project.atlas;
    let projection = pipeline.projection(doc, atlas.maps[0].id).unwrap();
    let frame = new_frame(atlas.page(page).unwrap(), &atlas.maps[0], projection);
    doc.project.atlas.add_frame(page, frame).unwrap()
}

fn compose_frame(doc: &Document, pipeline: &mut Pipeline, id: FrameId) -> Vec<Item> {
    let atlas = &doc.project.atlas;
    let page = atlas.page(atlas.page_of_frame(id).unwrap()).unwrap();
    let frame = atlas.frame(id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    let composed = pipeline
        .compose_frame(doc, page, frame, target, &mut f, Quality::Final)
        .unwrap();
    composed.list.items
}

fn compose_frame_full(
    doc: &Document,
    pipeline: &mut Pipeline,
    id: FrameId,
) -> vale_app::pipeline::Composed {
    let atlas = &doc.project.atlas;
    let page = atlas.page(atlas.page_of_frame(id).unwrap()).unwrap();
    let frame = atlas.frame(id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    pipeline
        .compose_frame(doc, page, frame, target, &mut f, Quality::Final)
        .unwrap()
}

#[test]
fn each_projection_draws_a_graticule_in_a_frame() {
    const RED: [u8; 4] = [200, 10, 20, 255];
    let red = Color::rgb(RED[0], RED[1], RED[2]);
    let has_stroke = |c: &vale_app::pipeline::Composed| {
        c.list.items.iter().any(|i| {
            matches!(i, Item::Stroke { color, width, path, .. }
                if *color == red && *width == 1.5 && !path.elements().is_empty())
        })
    };
    for kind in ProjectionKind::ALL {
        let mut doc = plain();
        let mut pipeline = Pipeline::new();
        let map = doc.first_map();
        doc.set_projection(map, ProjectionSpec::new(kind, 0.0, 0.0));
        let page = doc.project.atlas.pages[0].id;
        let id = frame_on(&mut doc, &mut pipeline, page);
        let frame = doc.project.atlas.frame_mut(id).unwrap();
        frame.graticule.color = RED;
        frame.graticule.width = 1.5;

        let frame = doc.project.atlas.frame(id).unwrap().clone();
        let auto = auto_graticule_interval(pipeline.projection(&doc, map).unwrap(), &frame);
        let world = compose_frame_full(&doc, &mut pipeline, id);
        assert_eq!(world.graticule_step, Some(auto), "{kind:?}");
        assert!(has_stroke(&world), "{kind:?}");

        doc.project.atlas.frame_mut(id).unwrap().scale /= 100.0;
        let closer = compose_frame_full(&doc, &mut pipeline, id);
        assert!(closer.graticule_step.unwrap() < auto, "{kind:?}");
        assert!(has_stroke(&closer), "{kind:?}");

        doc.project.atlas.frame_mut(id).unwrap().scale = 5_000.0;
        let street = compose_frame_full(&doc, &mut pipeline, id);
        assert!(street.graticule_step.unwrap() < 1.0, "{kind:?}");
        assert!(has_stroke(&street), "{kind:?}");
    }
}

#[test]
fn a_frame_draws_only_inside_its_shape() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = false;
    // The outer ring has a notch at the bottom, and the polygon has a hole.
    frame.shape = FrameShape::Polygon(vec![
        vec![
            [10.0, 10.0],
            [287.0, 10.0],
            [287.0, 200.0],
            [148.5, 120.0],
            [10.0, 200.0],
        ],
        vec![[40.0, 40.0], [80.0, 40.0], [80.0, 80.0], [40.0, 80.0]],
    ]);
    let page = doc.project.atlas.page(page).unwrap();
    let mut f = fonts().unwrap();
    let list = pipeline
        .compose_page(&doc, page, &mut f, Quality::Final)
        .unwrap();
    assert!(list.items.iter().any(|i| matches!(
        i,
        Item::PushClip {
            rule: FillRule::EvenOdd,
            ..
        }
    )));
    let pixmap = render_pixmap(&list, 1.0).unwrap();
    let w = usize::from(pixmap.width());
    let data = pixmap.data_as_u8_slice();
    let at = |x_mm: f64, y_mm: f64| {
        let (x, y) = ((x_mm * PT_PER_MM) as usize, (y_mm * PT_PER_MM) as usize);
        let i = (y * w + x) * 4;
        [data[i], data[i + 1], data[i + 2]]
    };
    let paper = [PAPER.r, PAPER.g, PAPER.b];
    assert_eq!(at(60.0, 60.0), paper, "the hole");
    assert_eq!(at(148.5, 180.0), paper, "the notch");
    assert_eq!(at(5.0, 5.0), paper, "the margin");
    assert_ne!(at(150.0, 95.0), paper, "the map");
}

#[test]
fn the_paper_shows_in_a_frame_outside_the_outline_of_the_projection() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame(id).unwrap().clone();
    let items = compose_frame(&doc, &mut pipeline, id);
    let background = doc.frame.style.background;
    assert!(
        !items
            .iter()
            .any(|i| matches!(i, Item::Fill { color, .. } if *color == background))
    );
    let page = doc.project.atlas.page(page).unwrap();
    let mut f = fonts().unwrap();
    let list = pipeline
        .compose_page(&doc, page, &mut f, Quality::Final)
        .unwrap();
    let pixmap = render_pixmap(&list, 1.0).unwrap();
    let w = usize::from(pixmap.width());
    let data = pixmap.data_as_u8_slice();
    let at = |x_mm: f64, y_mm: f64| {
        let (x, y) = ((x_mm * PT_PER_MM) as usize, (y_mm * PT_PER_MM) as usize);
        let i = (y * w + x) * 4;
        [data[i], data[i + 1], data[i + 2]]
    };
    let b = vale_app::atlas::shape_bbox(&frame.shape);
    let paper = [PAPER.r, PAPER.g, PAPER.b];
    assert_eq!(at(b.x0 + 2.0, b.y0 + 2.0), paper, "the corner of the frame");
    assert_ne!(at(b.center().x, b.center().y), paper, "the map");
}

#[test]
fn a_frame_draws_the_layers_that_its_map_shows() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let land = doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Land")
        .unwrap()
        .id;
    let land_fills = |doc: &Document, pipeline: &mut Pipeline| {
        let items = compose_frame(doc, pipeline, id);
        let land = Color::rgb(244, 241, 232);
        let fills = items
            .iter()
            .filter(|i| matches!(i, Item::Fill { color, .. } if *color == land));
        fills.count()
    };
    assert!(land_fills(&doc, &mut pipeline) > 0);
    let map = doc.first_map();
    assert!(doc.set_shown(map, land, false));
    assert!(!doc.shown(map, land));
    assert_eq!(land_fills(&doc, &mut pipeline), 0);
    // The eye of the World workspace does not hide a layer in a frame.
    assert!(doc.set_shown(map, land, true));
    doc.set_visible(land, false);
    assert!(land_fills(&doc, &mut pipeline) > 0);
}

#[test]
fn an_area_filter_picks_the_large_polygon_in_a_frame() {
    // A square of 20°, about 1,900,000 square miles, and a square of 1°
    // at 30°, 0°, about 4,800 square miles.
    let text = r#"{"type": "FeatureCollection", "features": [
      {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
        "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}},
      {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
        "coordinates": [[[30, 0], [31, 0], [31, 1], [30, 1], [30, 0]]]}}
    ]}"#;
    let mut doc = Document::empty();
    let imported = vale_import::geojson::read_str("areas", text).unwrap();
    let layer = doc.add_imported(imported, None)[0];
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    doc.project.atlas.frame_mut(id).unwrap().graticule.on = false;
    let red = Color::rgb(200, 0, 0);
    let mut red_box = |doc: &mut Document, filter: &str| {
        let rule = &mut doc.entry_mut(layer).unwrap().style.style.rules[0];
        rule.filter = Filter::new(filter);
        rule.set_fill(Some(red));
        let items = compose_frame(doc, &mut pipeline, id);
        let boxes = items.iter().filter_map(|i| match i {
            Item::Fill { path, color, .. } if *color == red => Some(path.bounding_box()),
            _ => None,
        });
        boxes.reduce(|a, b| a.union(b)).expect("a red fill")
    };
    let large = red_box(&mut doc, "area('mi2') > 10000");
    let small = red_box(&mut doc, "area('mi2') <= 10000");
    assert!(large.x1 < small.x0, "{large:?} {small:?}");
    assert!(large.width() > 5.0 * small.width(), "{large:?} {small:?}");
}

#[test]
fn frames_on_two_pages_share_one_projection() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let first = doc.project.atlas.pages[0].id;
    let second = doc
        .project
        .atlas
        .add_page(Some(first), 420.0, 297.0, PageUnits::Mm);
    let a = frame_on(&mut doc, &mut pipeline, first);
    let b = frame_on(&mut doc, &mut pipeline, second);
    assert_eq!(doc.project.atlas.frame(b).unwrap().map, doc.first_map());
    let mut f = fonts().unwrap();
    for page in [first, second] {
        let page = doc.project.atlas.page(page).unwrap();
        let list = pipeline
            .compose_page(&doc, page, &mut f, Quality::Final)
            .unwrap();
        assert!(list.items.iter().any(|i| matches!(i, Item::Fill { .. })));
    }
    assert!(!compose_frame(&doc, &mut pipeline, a).is_empty());
    assert_eq!(pipeline.cache_count(), 1);
}

#[test]
fn each_frame_draws_with_the_projection_of_its_map() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let a = frame_on(&mut doc, &mut pipeline, page);
    let b = frame_on(&mut doc, &mut pipeline, page);
    let copy = doc.project.atlas.add_map();
    doc.project.atlas.frame_mut(b).unwrap().map = copy;
    let draw = |doc: &Document, pipeline: &mut Pipeline, id| {
        format!("{:?}", compose_frame(doc, pipeline, id))
    };
    let before = [draw(&doc, &mut pipeline, a), draw(&doc, &mut pipeline, b)];
    doc.set_projection(
        copy,
        ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0),
    );
    assert_eq!(draw(&doc, &mut pipeline, a), before[0]);
    assert_ne!(draw(&doc, &mut pipeline, b), before[1]);
    assert_eq!(pipeline.cache_count(), 2);
}

#[test]
fn a_new_projection_changes_each_frame() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let first = doc.project.atlas.pages[0].id;
    let second = doc
        .project
        .atlas
        .add_page(None, 297.0, 210.0, PageUnits::Mm);
    let frames = [
        frame_on(&mut doc, &mut pipeline, first),
        frame_on(&mut doc, &mut pipeline, second),
    ];
    let before: Vec<String> = frames
        .iter()
        .map(|id| format!("{:?}", compose_frame(&doc, &mut pipeline, *id)))
        .collect();
    doc.set_projection(
        doc.first_map(),
        ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0),
    );
    assert_eq!(doc.project.atlas.maps[0].projection, "+proj=merc +lon_0=0");
    for (id, before) in frames.iter().zip(&before) {
        let after = format!("{:?}", compose_frame(&doc, &mut pipeline, *id));
        assert_ne!(&after, before);
    }
    assert_eq!(pipeline.cache_count(), 2);
}

fn neatline_items(list: &vale_render::DisplayList, color: [u8; 4], width: f64) -> Vec<&Item> {
    let color = vale_render::Color::rgba(color[0], color[1], color[2], color[3]);
    (list.items.iter())
        .filter(
            |i| matches!(i, Item::Stroke { color: c, width: w, .. } if *c == color && *w == width),
        )
        .collect()
}

#[test]
fn the_neatline_follows_each_ring_of_the_shape_and_stays_vector() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = false;
    frame.shape = FrameShape::Polygon(vec![
        vec![
            [10.0, 10.0],
            [287.0, 10.0],
            [287.0, 200.0],
            [148.5, 120.0],
            [10.0, 200.0],
        ],
        vec![[40.0, 40.0], [80.0, 40.0], [80.0, 80.0], [40.0, 80.0]],
    ]);
    frame.neatline.color = [200, 10, 20, 255];
    frame.neatline.width = 3.0;
    let render = |doc: &Document, pipeline: &mut Pipeline| {
        let page = doc.project.atlas.page(page).unwrap();
        let mut f = fonts().unwrap();
        pipeline
            .compose_page(doc, page, &mut f, Quality::Final)
            .unwrap()
    };
    let list = render(&doc, &mut pipeline);
    let neatlines = neatline_items(&list, [200, 10, 20, 255], 3.0);
    assert_eq!(neatlines.len(), 1);
    let Item::Stroke { path, .. } = neatlines[0] else {
        unreachable!()
    };
    let rings = path
        .elements()
        .iter()
        .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
        .count();
    assert_eq!(rings, 2, "the outer ring and the hole");
    assert!(list.items.iter().all(|i| !matches!(i, Item::Image { .. })));
    assert!(
        vale_render::render_pdf(&list)
            .unwrap()
            .starts_with(b"%PDF-")
    );

    // The neatline is on the edge of the notch.
    let pixmap = render_pixmap(&list, 1.0).unwrap();
    let w = usize::from(pixmap.width());
    let data = pixmap.data_as_u8_slice();
    let (x, y) = ((148.5 * PT_PER_MM) as usize, (120.0 * PT_PER_MM) as usize);
    let i = (y * w + x) * 4;
    assert_eq!(&data[i..i + 3], &[200, 10, 20]);

    doc.project.atlas.frame_mut(id).unwrap().neatline.on = false;
    let list = render(&doc, &mut pipeline);
    assert!(neatline_items(&list, [200, 10, 20, 255], 3.0).is_empty());
}

/// The page as a list, with one rectangle frame from 40, 40 to 200, 160 mm.
/// The frame has no graticule and no neatline.
fn bordered() -> (Document, Pipeline, FrameId) {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.shape = FrameShape::Rectangle([40.0, 40.0, 200.0, 160.0]);
    frame.graticule.on = false;
    frame.neatline.on = false;
    (doc, pipeline, id)
}

fn page_list(doc: &Document, pipeline: &mut Pipeline) -> vale_render::DisplayList {
    let page = &doc.project.atlas.pages[0];
    let mut f = fonts().unwrap();
    pipeline
        .compose_page(doc, page, &mut f, Quality::Final)
        .unwrap()
}

fn fills_of(list: &vale_render::DisplayList, color: [u8; 4]) -> Vec<kurbo::Rect> {
    let color = vale_render::Color::rgba(color[0], color[1], color[2], color[3]);
    (list.items.iter())
        .filter_map(|i| match i {
            Item::Fill { path, color: c, .. } if *c == color => Some(path.bounding_box()),
            _ => None,
        })
        .collect()
}

fn pixel(list: &vale_render::DisplayList, at_mm: (f64, f64)) -> [u8; 4] {
    let pixmap = render_pixmap(list, 1.0).unwrap();
    let w = usize::from(pixmap.width());
    let (x, y) = (
        (at_mm.0 * PT_PER_MM) as usize,
        (at_mm.1 * PT_PER_MM) as usize,
    );
    let i = (y * w + x) * 4;
    let d = pixmap.data_as_u8_slice();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

fn is_pdf(list: &vale_render::DisplayList) -> bool {
    vale_render::render_pdf(list).unwrap().starts_with(b"%PDF-")
}

#[test]
fn the_alpha_of_the_frame_colors_reaches_the_display_list() {
    let (mut doc, mut pipeline, id) = bordered();
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = true;
    frame.graticule.interval = Some(30.0);
    frame.graticule.color = [200, 10, 20, 100];
    frame.neatline.on = true;
    frame.neatline.color = [10, 20, 200, 90];
    frame.border.on = true;
    frame.border.color = [1, 2, 3, 80];
    frame.border.stroke = true;
    frame.border.stroke_color = [4, 5, 6, 70];
    let list = page_list(&doc, &mut pipeline);
    let stroked = |[r, g, b, a]: [u8; 4]| {
        let c = vale_render::Color::rgba(r, g, b, a);
        (list.items.iter()).any(|i| matches!(i, Item::Stroke { color, .. } if *color == c))
    };
    assert!(stroked([200, 10, 20, 100]), "the graticule");
    assert!(stroked([10, 20, 200, 90]), "the neatline");
    assert!(stroked([4, 5, 6, 70]), "the stroke of the border");
    assert_eq!(fills_of(&list, [1, 2, 3, 80]).len(), 1, "the border");
}

#[test]
fn a_page_draws_its_paper_color() {
    let (mut doc, mut pipeline, _) = bordered();
    doc.project.atlas.pages[0].paper = [0xf3, 0xef, 0xdf];
    let list = page_list(&doc, &mut pipeline);
    assert_eq!(fills_of(&list, [0xf3, 0xef, 0xdf, 0xff]).len(), 1);
    assert!(matches!(list.items[0], Item::Fill { .. }));
    assert_eq!(pixel(&list, (5.0, 5.0)), [0xf3, 0xef, 0xdf, 0xff]);
    assert!(is_pdf(&list));
}

#[test]
fn the_border_is_at_its_distance_outside_the_box_and_can_be_off() {
    let (mut doc, mut pipeline, id) = bordered();
    let red = [200, 10, 20, 255];
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.on = true;
    frame.border.color = red;
    frame.border.width_mm = 2.0;
    frame.border.distances_mm = [8.0; 4];
    let list = page_list(&doc, &mut pipeline);
    let band = fills_of(&list, red);
    assert_eq!(band.len(), 1);
    let pt = |mm: f64| mm * PT_PER_MM;
    let outer = kurbo::Rect::new(pt(30.0), pt(30.0), pt(210.0), pt(170.0));
    let b = band[0];
    for (a, e) in [
        (b.x0, outer.x0),
        (b.y0, outer.y0),
        (b.x1, outer.x1),
        (b.y1, outer.y1),
    ] {
        assert!((a - e).abs() < 1e-6, "{b:?}");
    }
    // The band, and paper between the band and the shape.
    assert_eq!(pixel(&list, (31.0, 100.0)), red);
    assert_eq!(pixel(&list, (35.0, 100.0)), [255, 255, 255, 255]);
    assert_eq!(pixel(&list, (100.0, 169.0)), red);
    assert!(list.items.iter().all(|i| !matches!(i, Item::Image { .. })));
    assert!(is_pdf(&list));

    doc.project.atlas.frame_mut(id).unwrap().border.on = false;
    assert!(fills_of(&page_list(&doc, &mut pipeline), red).is_empty());
}

#[test]
fn a_graduated_border_changes_color_at_each_graticule_line() {
    let (mut doc, mut pipeline, id) = bordered();
    let (a, b) = ([200, 10, 20, 255], [10, 20, 200, 255]);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.on = true;
    frame.border.style = vale_store::BorderStyle::Graduated;
    frame.border.color = a;
    frame.border.color2 = b;
    frame.graticule.interval = Some(30.0);
    // The map fills the band.
    frame.scale /= 3.0;
    let pieces = |doc: &Document, pipeline: &mut Pipeline| {
        let list = page_list(doc, pipeline);
        assert!(is_pdf(&list));
        let mut top: Vec<(f64, bool)> = (fills_of(&list, a).iter().map(|r| (r, true)))
            .chain(fills_of(&list, b).iter().map(|r| (r, false)))
            .filter(|(r, _)| r.y0 < 31.0 * PT_PER_MM)
            .map(|(r, first)| (r.x0, first))
            .collect();
        top.sort_by(|p, q| p.0.total_cmp(&q.0));
        top
    };
    let coarse = pieces(&doc, &mut pipeline);
    assert!(coarse.len() >= 2, "{coarse:?}");
    for w in coarse.windows(2) {
        assert_ne!(w[0].1, w[1].1, "the colors alternate: {coarse:?}");
    }
    // Each change is where a meridian meets the middle of the top band.
    let (projection, frame) = (
        pipeline.projection(&doc, doc.first_map()).unwrap(),
        doc.project.atlas.frame(id).unwrap(),
    );
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = frame_transform(projection, frame, target);
    for (x, _) in &coarse[1..] {
        let q = vt.from_page(Point::new(*x, 31.0 * PT_PER_MM));
        let [lon, lat] = projection.inverse(q).unwrap();
        let near = |v: f64| (v / 30.0 - (v / 30.0).round()).abs() < 1e-3;
        assert!(near(lon) || near(lat), "{lon} {lat}");
    }
    doc.project.atlas.frame_mut(id).unwrap().graticule.interval = Some(10.0);
    assert!(pieces(&doc, &mut pipeline).len() > coarse.len());
}

/// The text and the first glyph position of each glyph run.
fn runs(items: &[Item]) -> Vec<(String, Point)> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(run) => Some((run.text.clone(), run.glyphs[0].1.translation().to_point())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_reference_grid_labels_each_graticule_interval_in_the_band() {
    let (mut doc, mut pipeline, id) = bordered();
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.on = true;
    frame.border.grid = true;
    frame.graticule.interval = Some(30.0);
    // The map fills the band.
    frame.scale /= 3.0;
    let list = page_list(&doc, &mut pipeline);
    assert!(list.items.iter().all(|i| !matches!(i, Item::Image { .. })));
    assert!(is_pdf(&list));
    let runs = runs(&list.items);
    let pt = |mm: f64| mm * PT_PER_MM;
    let outer = kurbo::Rect::new(pt(30.0), pt(30.0), pt(210.0), pt(170.0));
    let inner = kurbo::Rect::new(pt(32.0), pt(32.0), pt(208.0), pt(168.0));
    for (text, at) in &runs {
        assert!(
            outer.contains(*at) && !inner.contains(*at),
            "{text} at {at:?}"
        );
    }

    let projection = pipeline.projection(&doc, doc.first_map()).unwrap();
    let frame = doc.project.atlas.frame(id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = frame_transform(projection, frame, target);
    // The letters on the top go from the west, one for each interval of
    // the meridians, and the bottom has the same letters.
    let side = |top: bool| {
        let mut letters: Vec<(f64, String)> = (runs.iter())
            .filter(|(t, at)| t.chars().all(|c| c.is_ascii_uppercase()) && (at.y < inner.y0) == top)
            .map(|(t, at)| (at.x, t.clone()))
            .collect();
        letters.sort_by(|a, b| a.0.total_cmp(&b.0));
        letters
    };
    let top = side(true);
    assert!(top.len() >= 3, "{top:?}");
    let names: Vec<&str> = top.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(names[..3], ["A", "B", "C"]);
    let bottom: Vec<String> = side(false).into_iter().map(|(_, t)| t).collect();
    assert_eq!(bottom, names);
    let middle_y = (outer.y0 + inner.y0) / 2.0;
    let cells: Vec<i64> = (top.iter())
        .map(|(x, _)| {
            // Just right of the start of the letter.
            let at = vt.from_page(Point::new(*x + 1.0, middle_y));
            let [lon, _] = projection.inverse(at).unwrap();
            (lon / 30.0).floor() as i64
        })
        .collect();
    for w in cells.windows(2) {
        assert_eq!(w[1], w[0] + 1, "one letter for each interval: {cells:?}");
    }
    // The numbers on the left go from the north, one for each interval of
    // the parallels.
    let mut left: Vec<(f64, String)> = (runs.iter())
        .filter(|(t, at)| t.chars().all(|c| c.is_ascii_digit()) && at.x < inner.x0)
        .map(|(t, at)| (at.y, t.clone()))
        .collect();
    left.sort_by(|a, b| a.0.total_cmp(&b.0));
    let numbers: Vec<&str> = left.iter().map(|(_, t)| t.as_str()).collect();
    assert!(numbers.len() >= 3, "{numbers:?}");
    assert_eq!(numbers[..3], ["1", "2", "3"]);

    // A finer graticule gives more labels.
    doc.project.atlas.frame_mut(id).unwrap().graticule.interval = Some(10.0);
    let finer = texts(&page_list(&doc, &mut pipeline).items).len();
    assert!(finer > runs.len(), "{finer} {}", runs.len());

    doc.project.atlas.frame_mut(id).unwrap().border.grid = false;
    assert!(texts(&page_list(&doc, &mut pipeline).items).is_empty());
}

#[test]
fn a_part_of_the_shape_crosses_the_band_and_covers_it() {
    let (mut doc, mut pipeline, id) = bordered();
    let red = [200, 10, 20, 255];
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    // A body from y 60 down, with a part that goes up to y 40 from x 80 to
    // 120.
    frame.shape = FrameShape::Polygon(vec![vec![
        [40.0, 60.0],
        [80.0, 60.0],
        [80.0, 40.0],
        [120.0, 40.0],
        [120.0, 60.0],
        [200.0, 60.0],
        [200.0, 160.0],
        [40.0, 160.0],
    ]]);
    frame.scale /= 3.0;
    frame.graticule.interval = Some(10.0);
    frame.border.on = true;
    frame.border.color = red;
    frame.border.stroke = true;
    frame.border.grid = true;
    // The band runs from y 52 to 54, under the part on the top.
    frame.border.distances_mm = [-14.0, 8.0, 8.0, 8.0];
    let list = page_list(&doc, &mut pipeline);
    assert_eq!(pixel(&list, (60.0, 53.0)), red);
    assert_eq!(pixel(&list, (160.0, 53.0)), red);
    assert_ne!(
        pixel(&list, (100.0, 53.0)),
        red,
        "the shape covers the band"
    );
    assert_eq!(
        pixel(&list, (28.0, 100.0)),
        [255, 255, 255, 255],
        "outside the band"
    );
    assert_eq!(pixel(&list, (31.0, 100.0)), red);

    let pt = |mm: f64| mm * PT_PER_MM;
    let runs = runs(&list.items);
    let top: Vec<&(String, Point)> = runs.iter().filter(|(_, at)| at.y < pt(55.0)).collect();
    assert!(!top.is_empty());
    for (text, at) in &top {
        let covered = at.x > pt(78.0) && at.x < pt(120.0);
        assert!(!covered, "{text} at {at:?} is under the shape");
    }
    assert!(is_pdf(&list));
}

#[test]
fn the_band_keeps_a_gap_around_the_shape_where_it_crosses() {
    let (mut doc, mut pipeline, id) = bordered();
    let red = [200, 10, 20, 255];
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.shape = FrameShape::Polygon(vec![vec![
        [40.0, 60.0],
        [80.0, 60.0],
        [80.0, 40.0],
        [120.0, 40.0],
        [120.0, 60.0],
        [200.0, 60.0],
        [200.0, 160.0],
        [40.0, 160.0],
    ]]);
    frame.neatline.on = false;
    frame.border.on = true;
    frame.border.color = red;
    frame.border.distances_mm = [-14.0, 8.0, 8.0, 8.0];
    frame.border.gap_mm = 2.0;
    let list = page_list(&doc, &mut pipeline);
    assert_eq!(pixel(&list, (77.0, 53.0)), red);
    assert_eq!(pixel(&list, (79.0, 53.0)), [255, 255, 255, 255], "the gap");
    assert_eq!(pixel(&list, (121.0, 53.0)), [255, 255, 255, 255], "the gap");
    assert_eq!(pixel(&list, (123.0, 53.0)), red);

    doc.project.atlas.frame_mut(id).unwrap().border.gap_mm = 0.0;
    let list = page_list(&doc, &mut pipeline);
    assert_eq!(pixel(&list, (79.0, 53.0)), red);
}

#[test]
fn the_grid_labels_stay_beside_the_map_and_count_from_the_whole_shape() {
    let (mut doc, mut pipeline, id) = bordered();
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.scale /= 3.0;
    frame.graticule.interval = Some(10.0);
    frame.border.on = true;
    frame.border.grid = true;
    // A wide band reaches far past the corners of the box.
    frame.border.distances_mm = [20.0; 4];
    let list = page_list(&doc, &mut pipeline);
    let pt = |mm: f64| mm * PT_PER_MM;
    let placed = runs(&list.items);
    let (top, left): (Vec<_>, Vec<_>) = (
        placed.iter().filter(|(_, at)| at.y < pt(30.0)).collect(),
        placed.iter().filter(|(_, at)| at.x < pt(30.0)).collect(),
    );
    assert!(top.len() > 3 && left.len() > 3);
    for (text, at) in &top {
        assert!(at.x > pt(39.0) && at.x < pt(201.0), "{text} at {at:?}");
    }
    for (text, at) in &left {
        assert!(at.y > pt(39.0) && at.y < pt(161.0), "{text} at {at:?}");
    }

    // The count starts at the westmost and the northmost cell of the shape.
    let projection = pipeline.projection(&doc, doc.first_map()).unwrap();
    let frame = doc.project.atlas.frame(id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = frame_transform(projection, frame, target);
    let cell = |mm: (f64, f64)| {
        let [lon, lat] = projection
            .inverse(vt.from_page(Point::new(pt(mm.0), pt(mm.1))))
            .unwrap();
        ((lon / 10.0).floor() as i64, (lat / 10.0).floor() as i64)
    };
    let corners = [(40.0, 40.0), (200.0, 40.0), (40.0, 160.0), (200.0, 160.0)].map(cell);
    let west = corners.iter().map(|c| c.0).min().unwrap();
    let north = corners.iter().map(|c| c.1).max().unwrap();
    let west_label = (top.iter()).min_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap();
    let first_column = cell((west_label.1.x / PT_PER_MM + 1.0, west_label.1.y / PT_PER_MM)).0;
    let want = vale_app::frame_border::column_name((first_column - west) as u32);
    assert_eq!(west_label.0, want);
    let north_label = (left.iter())
        .min_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .unwrap();
    let first_row = cell((
        north_label.1.x / PT_PER_MM,
        north_label.1.y / PT_PER_MM - 0.5,
    ))
    .1;
    assert_eq!(north_label.0, (north - first_row + 1).to_string());

    // Numbered columns give lettered rows.
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.grid_columns = vale_store::GridColumns::Numbers;
    let runs = runs(&page_list(&doc, &mut pipeline).items);
    for (text, at) in &runs {
        if at.y < pt(30.0) {
            assert!(text.chars().all(|c| c.is_ascii_digit()), "{text}");
        }
        if at.x < pt(30.0) {
            assert!(text.chars().all(|c| c.is_ascii_uppercase()), "{text}");
        }
    }
}

#[test]
fn the_border_stroke_draws_the_edges_and_the_divisions_of_the_band() {
    let (mut doc, mut pipeline, id) = bordered();
    let black = [1, 2, 3, 255];
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.on = true;
    frame.border.stroke = true;
    frame.border.stroke_color = black;
    frame.border.stroke_width = 1.5;
    let list = page_list(&doc, &mut pipeline);
    let strokes = neatline_items(&list, black, 1.5);
    assert_eq!(strokes.len(), 1);
    let Item::Stroke { path, .. } = strokes[0] else {
        unreachable!()
    };
    let rings = (path.elements().iter())
        .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
        .count();
    assert_eq!(rings, 2, "the outer and the inner edge");
    let b = path.bounding_box();
    let pt = |mm: f64| mm * PT_PER_MM;
    assert!((b.x0 - pt(30.0)).abs() < 1e-6 && (b.x1 - pt(210.0)).abs() < 1e-6);
    assert!(is_pdf(&list));

    // A graduated band has a line across it at each division.
    let (a, c) = ([200, 10, 20, 255], [10, 20, 200, 255]);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.border.style = vale_store::BorderStyle::Graduated;
    (frame.border.color, frame.border.color2) = (a, c);
    frame.graticule.interval = Some(30.0);
    frame.scale /= 3.0;
    let list = page_list(&doc, &mut pipeline);
    let pieces = fills_of(&list, a).len() + fills_of(&list, c).len();
    let Item::Stroke { path, .. } = neatline_items(&list, black, 1.5)[0] else {
        unreachable!()
    };
    let parts = (path.elements().iter())
        .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
        .count();
    assert!(pieces > 4, "{pieces}");
    assert_eq!(
        parts,
        2 + (pieces - 4),
        "two edges and one line per division"
    );

    doc.project.atlas.frame_mut(id).unwrap().border.stroke = false;
    assert!(neatline_items(&page_list(&doc, &mut pipeline), black, 1.5).is_empty());
}

/// A document with one line layer. Each line has a `kind` and its coordinates.
fn lines_at(features: &[(&str, &[[f64; 2]])]) -> (Document, vale_store::LayerId) {
    let features: Vec<String> = features
        .iter()
        .map(|(kind, coords)| {
            let coords: Vec<String> = coords.iter().map(|[x, y]| format!("[{x},{y}]")).collect();
            format!(
                r#"{{"type":"Feature","properties":{{"kind":"{kind}"}},"geometry":{{"type":"LineString","coordinates":[{}]}}}}"#,
                coords.join(",")
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

fn kind_rule(kind: &str, color: Color, width: f64, stroke_kind: StrokeKind) -> Rule {
    let mut rule = stroke_rule(kind, &format!("kind = '{kind}'"), color, width);
    if let Symbolizer::Stroke(s) = &mut rule.symbolizers[0] {
        s.kind = stroke_kind;
    }
    rule
}

/// The polylines of a path. A path of lines only.
fn subpaths(path: &kurbo::BezPath) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => out.push(vec![p]),
            kurbo::PathEl::LineTo(p) => out.last_mut().unwrap().push(p),
            _ => panic!("not a polyline: {el:?}"),
        }
    }
    out
}

fn length(pts: &[Point]) -> f64 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

#[test]
fn a_dashed_curved_line_has_even_dashes() {
    // A parallel, which an oblique orthographic map shows as an arc.
    let parallel: Vec<[f64; 2]> = (0..=24)
        .map(|i| [-60.0 + 5.0 * f64::from(i), 45.0])
        .collect();
    let (mut doc, id) = lines_at(&[("a", &parallel)]);
    doc.set_projection(
        doc.first_map(),
        ProjectionSpec::new(ProjectionKind::Orthographic, 0.0, 60.0),
    );
    let (dash, gap) = (6.0, 3.0);
    doc.entry_mut(id).unwrap().style.style = Style {
        mask: None,
        rules: vec![kind_rule(
            "a",
            Color::rgb(0, 0, 200),
            1.5,
            StrokeKind::Dashed { dash, gap },
        )],
    };
    let composed = compose(&doc, &mut Pipeline::new());
    let dashed: Vec<&Item> = composed
        .list
        .items
        .iter()
        .filter(|i| matches!(i, Item::Stroke { round: false, color, .. } if color.b == 200))
        .collect();
    assert_eq!(dashed.len(), 1);
    let Item::Stroke { path, .. } = dashed[0] else {
        unreachable!()
    };
    let dashes = subpaths(path);
    assert!(dashes.len() > 10, "{} dashes", dashes.len());
    // The line is curved on the page: the dashes do not all point the same way.
    let dir = |d: &Vec<Point>| (d[d.len() - 1] - d[0]).atan2();
    let turn = (dir(&dashes[0]) - dir(&dashes[dashes.len() - 2])).abs();
    assert!(turn > 0.2, "the line turns {turn} rad");
    // Each dash but the last is one full dash long, along the curve.
    for d in &dashes[..dashes.len() - 1] {
        assert!((length(d) - dash).abs() < 1e-6, "a dash of {}", length(d));
    }
    // Each gap is one full gap long.
    for w in dashes.windows(2) {
        let g = w[0][w[0].len() - 1].distance(w[1][0]);
        assert!(g <= gap + 1e-6 && g > gap * 0.95, "a gap of {g}");
    }
}

/// The pixels of a row, between `x0` and `x1`, that are mostly `red`.
fn red_run(px: &vale_render::Rgba, y: f64, x0: f64, x1: f64) -> usize {
    let y = y.floor() as usize;
    (x0.floor() as usize..x1.ceil() as usize)
        .filter(|&x| {
            let i = (y * px.width + x) * 4;
            let [r, g, b] = [px.data[i], px.data[i + 1], px.data[i + 2]].map(i32::from);
            r - g.max(b) >= 100
        })
        .count()
}

#[test]
fn a_tapered_tributary_joins_a_wider_river_with_no_step() {
    let (mut doc, id) = lines_at(&[
        ("main", &[[-40.0, 0.0], [40.0, 0.0]]),
        ("trib", &[[0.0, 30.0], [0.0, 0.0]]),
    ]);
    doc.set_projection(
        doc.first_map(),
        ProjectionSpec::new(ProjectionKind::Equirectangular, 0.0, 0.0),
    );
    let (main_w, trib_w, start) = (4.0, 2.0, 0.25);
    doc.entry_mut(id).unwrap().style.style = Style {
        mask: None,
        rules: vec![
            kind_rule("main", Color::rgb(0, 0, 200), main_w, StrokeKind::Solid),
            kind_rule(
                "trib",
                Color::rgb(200, 0, 0),
                trib_w,
                StrokeKind::Taper { start },
            ),
        ],
    };
    let mut pipeline = Pipeline::new();
    let join = pipeline
        .lonlat_to_page(&doc, SIZE, [0.0, 0.0])
        .unwrap()
        .unwrap();
    let source = pipeline
        .lonlat_to_page(&doc, SIZE, [0.0, 30.0])
        .unwrap()
        .unwrap();
    // The tributary runs straight up the page to the main river.
    assert!((join.x - source.x).abs() < 1e-9 && source.y < join.y);
    let len = join.y - source.y;
    let scale = 4.0;
    let px = render_rgba(&compose(&doc, &mut pipeline).list, scale).unwrap();
    let x = join.x * scale;
    // The width in pixels at `t` points upstream of the join.
    let width_at = |t: f64| red_run(&px, (join.y - t) * scale, x - 20.0, x + 20.0) as f64;
    let expected = |t: f64| trib_w * (1.0 - (1.0 - start) * t / len) * scale;
    // Inside the main band and just upstream of it, the tributary has its full width.
    for t in [0.5, 1.5, 2.5, 3.0, 4.0] {
        let w = width_at(t);
        assert!((w - trib_w * scale).abs() <= 1.0, "{w} px at {t} pt");
        assert!((w - expected(t)).abs() <= 1.0, "{w} px at {t} pt");
    }
    // Upstream, the width shrinks to the start width.
    let far = len - 2.0;
    assert!((width_at(far) - expected(far)).abs() <= 1.0);
    // No tributary pixel shows on the far side of the main river.
    for t in [-2.5, -3.0, -4.0, -6.0] {
        assert_eq!(width_at(t), 0.0, "red at {t} pt");
    }
}

#[test]
fn each_stroke_kind_draws_in_a_frame_and_in_pdf() {
    let line = |y: f64| [[-60.0, y], [-20.0, y + 8.0], [20.0, y], [60.0, y + 8.0]];
    let (a, b, c, d) = (line(-45.0), line(-15.0), line(15.0), line(45.0));
    let (mut doc, id) = lines_at(&[("solid", &a), ("dashed", &b), ("brush", &c), ("taper", &d)]);
    let kinds = [
        ("solid", [200, 0, 0], StrokeKind::Solid),
        ("dashed", [0, 150, 0], StrokeKind::dashed()),
        ("brush", [200, 0, 200], StrokeKind::Brush),
        ("taper", [0, 0, 200], StrokeKind::taper()),
    ];
    let style = |kinds: &[(&str, [u8; 3], StrokeKind)]| Style {
        mask: None,
        rules: kinds
            .iter()
            .map(|(k, [r, g, b], s)| kind_rule(k, Color::rgb(*r, *g, *b), 4.0, *s))
            .collect(),
    };
    doc.entry_mut(id).unwrap().style.style = style(&kinds);
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let frame = frame_on(&mut doc, &mut pipeline, page);
    let list = compose_frame_full(&doc, &mut pipeline, frame).list;
    let px = render_rgba(&list, 2.0).unwrap();
    for (name, color, _) in kinds {
        let hits = px
            .data
            .chunks(4)
            .filter(|p| {
                let near = |a: u8, b: u8| a.abs_diff(b) <= 40;
                near(p[0], color[0]) && near(p[1], color[1]) && near(p[2], color[2])
            })
            .count();
        assert!(hits > 50, "{name}: {hits} pixels");
    }
    // Only the brush is a raster image in the PDF.
    let images = |list: &vale_render::DisplayList| {
        let pdf = vale_render::render_pdf(list).unwrap();
        String::from_utf8_lossy(&pdf).contains("/Subtype/Image")
    };
    let rasters = list
        .items
        .iter()
        .filter(|i| matches!(i, Item::Rasterize { .. }))
        .count();
    assert_eq!(rasters, 1);
    assert!(images(&list));
    let no_brush: Vec<_> = kinds.iter().copied().filter(|k| k.0 != "brush").collect();
    doc.entry_mut(id).unwrap().style.style = style(&no_brush);
    let list = compose_frame_full(&doc, &mut pipeline, frame).list;
    assert!(!images(&list), "a vector stroke became an image");
}

#[test]
fn a_translucent_taper_or_brush_applies_its_alpha_once() {
    let cross_a = [[-40.0, -20.0], [40.0, 20.0]];
    let cross_b = [[-40.0, 20.0], [40.0, -20.0]];
    let (mut doc, id) = lines_at(&[
        ("taper", &cross_a),
        ("taper", &cross_b),
        ("brush", &cross_a),
        ("brush", &cross_b),
    ]);
    let color = Color::rgba(0, 0, 200, 128);
    doc.entry_mut(id).unwrap().style.style = Style {
        rules: vec![
            kind_rule("taper", color, 4.0, StrokeKind::taper()),
            kind_rule("brush", color, 4.0, StrokeKind::Brush),
        ],
        mask: None,
    };
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let frame = frame_on(&mut doc, &mut pipeline, page);
    let list = compose_frame_full(&doc, &mut pipeline, frame).list;
    // The two crossing tapers are one fill, so the crossing is not darker.
    let tapers = list
        .items
        .iter()
        .filter(|i| matches!(i, Item::Fill { color: c, .. } if *c == color))
        .count();
    assert_eq!(tapers, 1);
    // The dabs of the brush are in one layer with the alpha of the color.
    let layers: Vec<_> = list
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rasterize { items, .. } => Some(items),
            _ => None,
        })
        .collect();
    assert_eq!(layers.len(), 1);
    let [Item::Layer { alpha, items }] = layers[0].as_slice() else {
        panic!("{:?}", layers[0].iter().map(describe).collect::<Vec<_>>());
    };
    assert!((alpha - 128.0 / 255.0).abs() < 1e-6);
    assert!(items.len() > 10);
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Fill { color, .. } if color.a > 128))
    );
}

/// A document with one layer of the GeoJSON features `features`.
fn one_layer(name: &str, features: &[String]) -> (Document, vale_store::LayerId) {
    let text = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    let imported = vale_import::geojson::read_str(name, &text).unwrap();
    let id = doc.add_imported(imported, None)[0];
    (doc, id)
}

fn square(lon0: f64, lat0: f64, lon1: f64, lat1: f64) -> String {
    format!(
        r#"{{"type":"Feature","properties":{{}},"geometry":{{"type":"Polygon","coordinates":[[[{lon0},{lat0}],[{lon1},{lat0}],[{lon1},{lat1}],[{lon0},{lat1}],[{lon0},{lat0}]]]}}}}"#
    )
}

fn style_of(symbolizer: Symbolizer) -> Style {
    Style {
        mask: None,
        rules: vec![Rule::new("Symbols", vec![symbolizer])],
    }
}

/// A view centered on `center` that shows `degrees` of longitude in `width` points.
fn view_on(
    doc: &Document,
    pipeline: &mut Pipeline,
    center: [f64; 2],
    degrees: f64,
    width: f64,
) -> vale_app::view::View {
    let [lon, lat] = center;
    let mut at = |lon: f64| pipeline.project_lonlat(doc, [lon, lat]).unwrap().unwrap();
    let (a, b, c) = (at(lon - degrees / 2.0), at(lon + degrees / 2.0), at(lon));
    vale_app::view::View {
        center: c,
        scale: width / (b.x - a.x),
        rotation: 0.0,
    }
}

/// The scatter symbols of a composition, by place on the sphere.
fn scatter_places(c: &vale_app::pipeline::Composed) -> Vec<([u64; 2], Point)> {
    let mut out: Vec<_> = c
        .symbols
        .iter()
        .filter_map(|s| {
            s.lonlat
                .map(|[x, y]| ([x.to_bits(), y.to_bits()], s.center))
        })
        .collect();
    out.sort_by_key(|(k, _)| *k);
    out
}

fn item_bbox(item: &Item) -> Option<kurbo::Rect> {
    use kurbo::Shape;
    match item {
        Item::Fill { path, .. } | Item::Stroke { path, .. } => Some(path.bounding_box()),
        _ => None,
    }
}

#[test]
fn a_point_symbol_takes_its_size_and_rotation_from_attributes() {
    let point = |lon: f64, size: f64, rot: f64| {
        format!(
            r#"{{"type":"Feature","properties":{{"size":{size},"rot":{rot}}},"geometry":{{"type":"Point","coordinates":[{lon},0]}}}}"#
        )
    };
    let (mut doc, id) = one_layer(
        "trees",
        &[
            point(-20.0, 10.0, 0.0),
            point(0.0, 10.0, 90.0),
            point(20.0, 20.0, 0.0),
        ],
    );
    common::add_symbols(&mut doc, &["pine"]);
    doc.entry_mut(id).unwrap().style.style = style_of(Symbolizer::Symbol(PointSymbolizer {
        symbol: "pine".to_string(),
        size: Property::expression("size * 2", 5.0),
        rotation: Property::expression("rot", 0.0),
        tint: None,
    }));
    let mut p = Pipeline::new();
    let c = compose(&doc, &mut p);
    assert_eq!(c.symbols.len(), 3);
    for (i, lon) in [-20.0, 0.0, 20.0].into_iter().enumerate() {
        let s = &c.symbols[i];
        assert_eq!(s.feature, i);
        let at = p.lonlat_to_page(&doc, SIZE, [lon, 0.0]).unwrap().unwrap();
        assert!((s.center - at).hypot() < 1e-6, "{:?} {at:?}", s.center);
        // The items of the symbol fill its box.
        let union = c
            .list
            .items
            .iter()
            .filter_map(item_bbox)
            .filter(|b| s.bounds.inflate(0.01, 0.01).contains_rect(*b))
            .reduce(|a, b| a.union(b))
            .unwrap();
        assert!((union.width() - s.bounds.width()).abs() < 0.01);
        assert!((union.height() - s.bounds.height()).abs() < 0.01);
    }
    let ((w0, h0), (w1, h1), (w2, h2)) = (dims(&c, 0), dims(&c, 1), dims(&c, 2));
    // A pine is taller than wide. Turned by 90 degrees, it is wider than tall.
    assert!((h0 - 20.0).abs() < 0.01 && w0 < 18.0, "{w0} x {h0}");
    assert!((w1 - 20.0).abs() < 0.01 && h1 < 18.0, "{w1} x {h1}");
    assert!((h2 - 40.0).abs() < 0.01 && w2 < 36.0, "{w2} x {h2}");
    assert!((w1 - h0).abs() < 0.01 && (h1 - w0).abs() < 0.01);
}

fn dims(c: &vale_app::pipeline::Composed, i: usize) -> (f64, f64) {
    (c.symbols[i].bounds.width(), c.symbols[i].bounds.height())
}

/// A document with one forest polygon that a scatter of trees fills.
fn forest(
    lon: (f64, f64),
    lat: (f64, f64),
    size: f64,
    spacing: f64,
) -> (Document, vale_store::LayerId) {
    let (mut doc, id) = one_layer("forest", &[square(lon.0, lat.0, lon.1, lat.1)]);
    common::add_symbols(&mut doc, &["tree"]);
    let mut scatter = ScatterSymbolizer::new("tree", size, spacing);
    scatter.rotation_jitter = 10.0;
    scatter.tint = Some(Property::constant(Color::rgb(30, 120, 40)));
    doc.entry_mut(id).unwrap().style.style = style_of(Symbolizer::Scatter(scatter));
    (doc, id)
}

#[test]
fn a_scatter_fill_draws_symbols_that_do_not_overlap() {
    let (size, spacing) = (8.0, 16.0);
    let (mut doc, id) = forest((-5.0, 5.0), (-5.0, 5.0), size, spacing);
    // The page is the plane of the scatter.
    let spec = ProjectionSpec::new(ProjectionKind::LambertAzimuthal, 0.0, 0.0);
    doc.set_projection(doc.first_map(), spec);
    let mut p = Pipeline::new();
    doc.frame.view = Some(view_on(&doc, &mut p, [0.0, 0.0], 12.0, 1080.0));
    let c = compose(&doc, &mut p);
    let centers: Vec<Point> = scatter_places(&c).into_iter().map(|(_, q)| q).collect();
    let n = centers.len();
    let mut nearest = f64::INFINITY;
    for (i, a) in centers.iter().enumerate() {
        for b in &centers[i + 1..] {
            nearest = nearest.min((*a - *b).hypot());
        }
    }
    // The real minimum distance is from the spacing to twice the spacing.
    let view = doc.frame.view.unwrap();
    let plane = vale_sphere::ScatterPlane::new([0.0, 0.0], 6_371_000.0);
    let distance = vale_sphere::min_distance(&plane, spacing / view.scale) * view.scale;
    assert!((spacing..=2.0 * spacing).contains(&distance), "{distance}");
    assert!(nearest >= size, "{nearest}");
    assert!(nearest >= 0.999 * distance, "{nearest} {distance}");
    // The polygon is 900 points wide.
    let area = 900.0 * 900.0;
    let d2 = distance * distance;
    let (low, high) = (area / (12.0 * d2), 1.2 * area / d2);
    assert!(
        n as f64 >= low && n as f64 <= high,
        "{n} not in {low}..{high}"
    );
    // Each symbol is inside the polygon.
    let bounds = {
        let a = p.lonlat_to_page(&doc, SIZE, [-5.0, -5.0]).unwrap().unwrap();
        let b = p.lonlat_to_page(&doc, SIZE, [5.0, 5.0]).unwrap().unwrap();
        kurbo::Rect::from_points(a, b)
    };
    for s in &c.symbols {
        assert!(bounds.contains_rect(s.bounds), "{:?}", s.bounds);
    }
    // The instances merge into a few items.
    let mut empty = doc.clone();
    empty.entry_mut(id).unwrap().style.style = Style {
        mask: None,
        rules: vec![],
    };
    let items = c.list.items.len() - compose(&empty, &mut p).list.items.len();
    assert!(items * 20 < n, "{items} items for {n} symbols");
}

/// The keys of the places that are inside `rect` of the page of `c`.
fn keys_in(places: &[([u64; 2], Point)], rect: kurbo::Rect) -> Vec<[u64; 2]> {
    places
        .iter()
        .filter(|(_, q)| rect.contains(*q))
        .map(|(k, _)| *k)
        .collect()
}

#[test]
fn a_scatter_stays_on_the_ground() {
    // A big forest: the view shows a small part of it.
    let (mut doc, _) = forest((-40.0, 40.0), (-30.0, 30.0), 10.0, 16.0);
    let mut p = Pipeline::new();
    let view = view_on(&doc, &mut p, [0.0, 0.0], 2.0, 1280.0);
    doc.frame.view = Some(view);
    let first = scatter_places(&compose(&doc, &mut p));
    assert!(first.len() > 200, "{}", first.len());
    let page = kurbo::Rect::new(0.0, 0.0, SIZE.0, SIZE.1);

    // A pan with a new pipeline: the symbols in both views are the same.
    let shift = kurbo::Vec2::new(400.0, 250.0);
    let mut panned = view;
    panned.center.x += shift.x / view.scale;
    panned.center.y -= shift.y / view.scale;
    doc.frame.view = Some(panned);
    let second = scatter_places(&compose(&doc, &mut Pipeline::new()));
    let both = page.intersect(page + shift).inset(-12.0);
    let before = keys_in(&first, both);
    let after = keys_in(
        &second
            .iter()
            .map(|(k, q)| (*k, *q + shift))
            .collect::<Vec<_>>(),
        both,
    );
    assert!(before.len() > 100, "{}", before.len());
    assert_eq!(before, after);

    // A zoom in keeps each symbol and adds more.
    let mut zoomed = view;
    zoomed.scale *= 2.5;
    doc.frame.view = Some(zoomed);
    let third = scatter_places(&compose(&doc, &mut p));
    let middle = page.center();
    let inner = page.inset(-12.0);
    let kept: Vec<_> = first
        .iter()
        .filter(|(_, q)| inner.contains(middle + (*q - middle) * 2.5))
        .map(|(k, _)| *k)
        .collect();
    let now: Vec<_> = third.iter().map(|(k, _)| *k).collect();
    assert!(!kept.is_empty());
    for k in &kept {
        assert!(now.binary_search(k).is_ok(), "a symbol moved in the zoom");
    }
    assert!(third.len() > kept.len());

    // A save and a reload give the same symbols.
    doc.frame.view = Some(view);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forest.gpkg");
    drop(vale_app::session::Session::open(&path, &mut doc).unwrap());
    let mut reloaded = Document::empty();
    drop(vale_app::session::Session::open(&path, &mut reloaded).unwrap());
    reloaded.frame.labels = false;
    reloaded.frame.graticule = false;
    reloaded.frame.view = Some(view);
    let again = scatter_places(&compose(&reloaded, &mut Pipeline::new()));
    assert_eq!(first, again);
}

#[test]
fn a_scatter_is_the_same_in_each_target_of_a_frame() {
    let (mut doc, _) = forest((-40.0, 40.0), (-30.0, 30.0), 2.5, 3.0);
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let atlas = &doc.project.atlas;
    let page = atlas.page(page).unwrap();
    let frame = atlas.frame(id).unwrap();
    let mut f = fonts().unwrap();
    let mut compose_in = |pts_per_mm: f64| {
        let target = FrameTarget {
            origin: Point::new(30.0, 20.0),
            pts_per_mm,
        };
        pipeline
            .compose_frame(&doc, page, frame, target, &mut f, Quality::Final)
            .unwrap()
    };
    let pdf = compose_in(PT_PER_MM);
    let raster = compose_in(3.0 * PT_PER_MM);
    let (a, b) = (scatter_places(&pdf), scatter_places(&raster));
    assert!(a.len() > 50, "{}", a.len());
    let keys = |v: &[([u64; 2], Point)]| v.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    assert_eq!(keys(&a), keys(&b));
    // The sizes follow the target.
    let side = |c: &vale_app::pipeline::Composed| {
        let s = &c.symbols[0];
        s.bounds.width().max(s.bounds.height())
    };
    assert!((side(&raster) / side(&pdf) - 3.0).abs() < 1e-6);
    // The symbols stay vector in the PDF.
    let bytes = vale_render::render_pdf(&pdf.list).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("/Subtype/Image"));
    let px = render_rgba(&raster.list, 1.0).unwrap();
    assert!(
        px.data
            .chunks(4)
            .any(|p| p[1] > p[0] + 40 && p[1] > p[2] + 40)
    );
}

/// Each edge label of a frame: its line meets the edge there, its text names
/// that line, and it overlaps no other label.
fn assert_edge_labels(doc: &Document, pipeline: &mut Pipeline, id: FrameId, what: &str) -> usize {
    let frame = doc.project.atlas.frame(id).unwrap().clone();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    let projection = pipeline.projection(doc, frame.map).unwrap();
    let interval = graticule_interval(projection, &frame).unwrap();
    let labels = edge_labels(projection, &frame, target, &mut f);
    let vt = frame_transform(projection, &frame, target);
    let rings: Vec<Vec<Point>> = (frame.shape.rings().iter())
        .map(|r| {
            r.iter()
                .map(|&[x, y]| target.to_target(Point::new(x, y)))
                .collect()
        })
        .collect();
    for label in &labels {
        let on_edge = rings.iter().any(|ring| {
            (0..ring.len()).any(|i| {
                let line = kurbo::Line::new(ring[i], ring[(i + 1) % ring.len()]);
                kurbo::ParamCurveNearest::nearest(&line, label.at, 1e-9).distance_sq < 1e-6
            })
        });
        assert!(on_edge, "{what}: {} is not on the edge", label.text);
        let [lon, lat] = projection.inverse(vt.from_page(label.at)).unwrap();
        let near = |v: f64| {
            let line = (v / interval).round() * interval;
            ((v - line).abs() < 0.05 * interval).then_some(line)
        };
        let format = frame.graticule.format;
        let named = near(lon)
            .is_some_and(|l| coord_text(l, interval, format, 'E', 'W') == label.text)
            || near(lat).is_some_and(|l| coord_text(l, interval, format, 'N', 'S') == label.text);
        assert!(named, "{what}: {} at {lon}, {lat}", label.text);
    }
    for label in &labels {
        let sides = label.quad.map(|q| even_odd(q, &rings));
        assert!(
            sides.iter().all(|inside| *inside != label.outside),
            "{what}: {} is not on its side of the edge",
            label.text
        );
    }
    for (i, a) in labels.iter().enumerate() {
        for b in &labels[i + 1..] {
            assert!(
                !quads_overlap(&a.quad, &b.quad),
                "{what}: {} and {} overlap",
                a.text,
                b.text
            );
        }
    }

    let page = doc
        .project
        .atlas
        .page(doc.project.atlas.page_of_frame(id).unwrap())
        .unwrap();
    let composed = pipeline
        .compose_frame(doc, page, &frame, target, &mut f, Quality::Final)
        .unwrap();
    let texts: Vec<&str> = (composed.list.items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(g) => Some(g.text.as_str()),
            _ => None,
        })
        .collect();
    for label in &labels {
        assert!(
            texts.contains(&label.text.as_str()),
            "{what}: {}",
            label.text
        );
    }
    labels.len()
}

#[test]
fn each_projection_has_graticule_labels_at_the_frame_edge() {
    for kind in ProjectionKind::ALL {
        let mut doc = plain();
        let mut pipeline = Pipeline::new();
        let map = doc.first_map();
        let spec = match kind {
            ProjectionKind::LambertConformal | ProjectionKind::AlbersEqualArea => {
                let mut s = ProjectionSpec::new(kind, 10.0, 40.0);
                s.lat1 = 30.0;
                s.lat2 = 50.0;
                s
            }
            _ => ProjectionSpec::new(kind, 0.0, 0.0),
        };
        doc.set_projection(map, spec);
        let page = doc.project.atlas.pages[0].id;
        let id = frame_on(&mut doc, &mut pipeline, page);
        let frame = doc.project.atlas.frame_mut(id).unwrap();
        frame.graticule.on = true;
        // The edge of a frame of the whole world is off the map.
        frame.scale /= 4.0;
        if kind == ProjectionKind::PolarStereographic {
            frame.center = [0.0, 70.0];
        } else if spec.lat1 != 0.0 {
            frame.center = [10.0, 40.0];
        }

        let region = assert_edge_labels(&doc, &mut pipeline, id, &format!("{kind:?}"));
        assert!(region > 0, "{kind:?} has no labels");

        let frame = doc.project.atlas.frame_mut(id).unwrap();
        frame.scale /= 20.0;
        frame.rotation = 30.0;
        frame.graticule.format = CoordFormat::Decimal;
        let closer = assert_edge_labels(&doc, &mut pipeline, id, &format!("{kind:?} closer"));
        assert!(closer > 0, "{kind:?} closer has no labels");
    }
}

#[test]
fn the_labels_at_a_corner_do_not_overlap() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let map = doc.first_map();
    doc.set_projection(
        map,
        ProjectionSpec::new(ProjectionKind::Equirectangular, 0.0, 0.0),
    );
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = true;
    frame.graticule.interval = Some(10.0);
    frame.rotation = 0.0;
    frame.scale /= 4.0;
    // The center moves so that a meridian and a parallel meet at the top
    // left corner of the frame.
    let frame = doc.project.atlas.frame(id).unwrap().clone();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let projection = pipeline.projection(&doc, map).unwrap();
    let vt = frame_transform(projection, &frame, target);
    let b = vale_app::atlas::shape_bbox(&frame.shape);
    let corner = target.to_target(Point::new(b.x0, b.y0));
    let [lon, lat] = projection.inverse(vt.from_page(corner)).unwrap();
    let to = [(lon / 10.0).round() * 10.0, (lat / 10.0).round() * 10.0];
    let center = &mut doc.project.atlas.frame_mut(id).unwrap().center;
    center[0] += to[0] - lon;
    center[1] += to[1] - lat;

    let n = assert_edge_labels(&doc, &mut pipeline, id, "corner");
    assert!(n > 0);
}

#[test]
fn a_polygon_frame_has_labels_on_each_edge() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = true;
    frame.scale /= 4.0;
    let b = vale_app::atlas::shape_bbox(&frame.shape);
    let (cx, cy) = (b.center().x, b.center().y);
    // A frame with a notch at the top right.
    frame.shape = FrameShape::Polygon(vec![vec![
        [b.x0, b.y0],
        [cx, b.y0],
        [cx, cy],
        [b.x1, cy],
        [b.x1, b.y1],
        [b.x0, b.y1],
    ]]);
    let n = assert_edge_labels(&doc, &mut pipeline, id, "polygon");
    assert!(n > 0);
}

/// Whether `p` is inside the rings, by the even-odd rule.
fn even_odd(p: Point, rings: &[Vec<Point>]) -> bool {
    let mut odd = false;
    for ring in rings {
        for (i, &a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
    }
    odd
}

/// A frame of map 0 at a regional scale, with the graticule on.
fn regional_frame(doc: &mut Document, pipeline: &mut Pipeline) -> FrameId {
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(doc, pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = true;
    frame.scale /= 4.0;
    id
}

fn labels_of(doc: &Document, pipeline: &mut Pipeline, id: FrameId) -> Vec<EdgeLabel> {
    let frame = doc.project.atlas.frame(id).unwrap().clone();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let projection = pipeline.projection(doc, frame.map).unwrap();
    edge_labels(projection, &frame, target, &mut fonts().unwrap())
}

#[test]
fn each_set_of_edge_labels_has_its_own_switch() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let id = regional_frame(&mut doc, &mut pipeline);
    let ends = |labels: &[EdgeLabel], letters: &[char]| {
        labels
            .iter()
            .filter(|l| letters.iter().any(|c| l.text.ends_with(*c)))
            .count()
    };
    let both = labels_of(&doc, &mut pipeline, id);
    assert!(ends(&both, &['E', 'W']) > 0 && ends(&both, &['N', 'S']) > 0);

    doc.project
        .atlas
        .frame_mut(id)
        .unwrap()
        .graticule
        .meridians
        .on = false;
    let parallels = assert_edge_labels(&doc, &mut pipeline, id, "parallels");
    assert!(parallels > 0);
    let labels = labels_of(&doc, &mut pipeline, id);
    assert_eq!(ends(&labels, &['E', 'W']), 0);

    doc.project
        .atlas
        .frame_mut(id)
        .unwrap()
        .graticule
        .parallels
        .on = false;
    let items = compose_frame(&doc, &mut pipeline, id);
    assert!(!items.iter().any(|i| matches!(i, Item::Glyphs(_))));
}

#[test]
fn outside_labels_sit_outside_the_frame() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let id = regional_frame(&mut doc, &mut pipeline);
    let g = &mut doc.project.atlas.frame_mut(id).unwrap().graticule;
    g.meridians.outside = true;
    g.parallels.outside = true;
    g.parallels.offset = 6.0;
    let n = assert_edge_labels(&doc, &mut pipeline, id, "outside");
    assert!(n > 0);
    // The outside labels come after the clip of the frame and its neatline.
    let items = compose_frame(&doc, &mut pipeline, id);
    let pop = items
        .iter()
        .rposition(|i| matches!(i, Item::PopClip))
        .unwrap();
    let first_glyph = items
        .iter()
        .position(|i| matches!(i, Item::Glyphs(_)))
        .unwrap();
    assert!(first_glyph > pop);
    let frame = doc.project.atlas.frame(id).unwrap().clone();
    let reach = outside_reach(&frame, &mut fonts().unwrap());
    let shape = vale_app::atlas::shape_bbox(&frame.shape);
    let shape = kurbo::Rect::new(
        shape.x0 * PT_PER_MM,
        shape.y0 * PT_PER_MM,
        shape.x1 * PT_PER_MM,
        shape.y1 * PT_PER_MM,
    )
    .inflate(reach, reach);
    for label in labels_of(&doc, &mut pipeline, id) {
        assert!(
            label.quad.iter().all(|q| shape.contains(*q)),
            "{}",
            label.text
        );
    }
}

#[test]
fn horizontal_labels_are_level() {
    for outside in [false, true] {
        let mut doc = plain();
        let mut pipeline = Pipeline::new();
        let map = doc.first_map();
        let mut spec = ProjectionSpec::new(ProjectionKind::LambertConformal, 10.0, 40.0);
        spec.lat1 = 30.0;
        spec.lat2 = 50.0;
        doc.set_projection(map, spec);
        let id = regional_frame(&mut doc, &mut pipeline);
        let frame = doc.project.atlas.frame_mut(id).unwrap();
        frame.center = [10.0, 45.0];
        for set in [
            &mut frame.graticule.meridians,
            &mut frame.graticule.parallels,
        ] {
            set.orientation = EdgeOrientation::Horizontal;
            set.outside = outside;
        }
        let n = assert_edge_labels(&doc, &mut pipeline, id, &format!("outside {outside}"));
        assert!(n > 0);
        for label in labels_of(&doc, &mut pipeline, id) {
            let [a, b, ..] = label.quad;
            assert!((a.y - b.y).abs() < 1e-9, "{}", label.text);
        }
    }
}

#[test]
fn an_edge_switch_removes_the_labels_of_its_side() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let id = regional_frame(&mut doc, &mut pipeline);
    let sides = |labels: &[EdgeLabel]| -> Vec<Side> { labels.iter().map(|l| l.side).collect() };
    let all = sides(&labels_of(&doc, &mut pipeline, id));
    for side in [Side::Top, Side::Bottom, Side::Left, Side::Right] {
        assert!(all.contains(&side), "{side:?}");
    }
    let g = &mut doc.project.atlas.frame_mut(id).unwrap().graticule;
    g.meridians.edges.top = false;
    g.parallels.edges.top = false;
    g.parallels.edges.left = false;
    let some = sides(&labels_of(&doc, &mut pipeline, id));
    assert!(!some.contains(&Side::Top));
    assert!(some.contains(&Side::Bottom));
    let labels = labels_of(&doc, &mut pipeline, id);
    assert!(
        !labels
            .iter()
            .any(|l| l.side == Side::Left && (l.text.ends_with('N') || l.text.ends_with('S')))
    );
}

#[test]
fn edge_labels_have_their_style_and_halo() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let id = regional_frame(&mut doc, &mut pipeline);
    let set = &mut doc.project.atlas.frame_mut(id).unwrap().graticule.meridians;
    set.color = [200, 10, 20, 128];
    set.size = 11.0;
    set.halo = true;
    set.halo_color = [1, 2, 3, 128];
    set.halo_width = 1.5;
    let items = compose_frame(&doc, &mut pipeline, id);
    let red = vale_render::Color::rgba(200, 10, 20, 128);
    let halo = vale_render::Color::rgba(1, 2, 3, 128);
    let glyph = items
        .iter()
        .position(|i| matches!(i, Item::Glyphs(g) if g.color == red && g.font_size == 11.0))
        .expect("a meridian label in its color and size");
    let stroke = items
        .iter()
        .position(
            |i| matches!(i, Item::Stroke { color, width, .. } if *color == halo && *width == 1.5),
        )
        .expect("a halo");
    assert!(stroke < glyph);
}

#[test]
fn a_frame_with_the_labels_off_has_no_edge_labels() {
    let mut doc = plain();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let id = frame_on(&mut doc, &mut pipeline, page);
    let frame = doc.project.atlas.frame_mut(id).unwrap();
    frame.graticule.on = true;
    frame.scale /= 4.0;
    assert!(assert_edge_labels(&doc, &mut pipeline, id, "on") > 0);
    let g = &mut doc.project.atlas.frame_mut(id).unwrap().graticule;
    g.meridians.on = false;
    g.parallels.on = false;
    let items = compose_frame(&doc, &mut pipeline, id);
    assert!(!items.iter().any(|i| matches!(i, Item::Glyphs(_))));
}

/// Saves a conic frame and a polar frame with edge labels to
/// `target/edge-labels-*.png` for a person to look at.
#[test]
#[ignore = "manual"]
fn manual_edge_label_pictures() {
    for (name, spec, center) in [
        (
            "conic",
            {
                let mut s = ProjectionSpec::new(ProjectionKind::LambertConformal, 10.0, 40.0);
                s.lat1 = 30.0;
                s.lat2 = 50.0;
                s
            },
            [10.0, 45.0],
        ),
        (
            "polar",
            ProjectionSpec::new(ProjectionKind::PolarStereographic, 0.0, 90.0),
            [0.0, 75.0],
        ),
    ] {
        let mut doc = Document::sample();
        let mut pipeline = Pipeline::new();
        let map = doc.first_map();
        doc.set_projection(map, spec);
        let page = doc.project.atlas.pages[0].id;
        let id = frame_on(&mut doc, &mut pipeline, page);
        let frame = doc.project.atlas.frame_mut(id).unwrap();
        frame.graticule.on = true;
        frame.scale /= 3.0;
        frame.center = center;
        if name == "polar" {
            // Shrink the frame so that the outside labels have room.
            if let FrameShape::Rectangle(r) = &mut frame.shape {
                *r = [r[0] + 15.0, r[1] + 15.0, r[2] - 15.0, r[3] - 15.0];
            }
            let g = &mut frame.graticule;
            g.parallels.outside = true;
            g.parallels.orientation = vale_store::EdgeOrientation::Horizontal;
            g.meridians.outside = true;
            g.meridians.color = [140, 30, 30, 255];
            g.meridians.size = 8.0;
            g.parallels.halo = true;
            g.parallels.edges.top = false;
        }
        let page = doc.project.atlas.page(page).unwrap().clone();
        let mut f = fonts().unwrap();
        let list = pipeline
            .compose_page(&doc, &page, &mut f, Quality::Final)
            .unwrap();
        let px = render_rgba(&list, 2.0).unwrap();
        let path = format!(
            "{}/../../target/edge-labels-{name}.png",
            env!("CARGO_MANIFEST_DIR")
        );
        image::RgbaImage::from_raw(px.width as u32, px.height as u32, px.data)
            .unwrap()
            .save(path)
            .unwrap();
    }
}

/// A document with one layer for each GeoJSON geometry, named by its name.
fn furniture_doc(layers: &[(&str, &str)]) -> (Document, Vec<vale_store::LayerId>) {
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    let mut ids = Vec::new();
    for (name, g) in layers {
        let text = format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{g}}}]}}"#
        );
        let imported = vale_import::geojson::read_str(name, &text).unwrap();
        ids.extend(doc.add_imported(imported, None));
    }
    (doc, ids)
}

const POLYGON: &str = r#"{"type":"Polygon","coordinates":[[[0,0],[40,0],[40,40],[0,40],[0,0]]]}"#;
const LINE: &str = r#"{"type":"LineString","coordinates":[[-40,-40],[-10,-10]]}"#;
const POINT: &str = r#"{"type":"Point","coordinates":[20,-20]}"#;

/// A frame of map 0 on page 0.
fn first_frame(doc: &mut Document, pipeline: &mut Pipeline) -> FrameId {
    let page = doc.project.atlas.pages[0].id;
    frame_on(doc, pipeline, page)
}

/// Adds an item of furniture to page 0.
fn add_item(doc: &mut Document, rect: [f64; 4], item: Furniture) -> FurnitureId {
    let page = doc.project.atlas.pages[0].id;
    doc.project.atlas.add_furniture(page, rect, item).unwrap()
}

/// The items of one item of furniture in page points.
fn furniture_items(
    doc: &Document,
    pipeline: &mut Pipeline,
    id: FurnitureId,
    purpose: Purpose,
) -> Vec<Item> {
    let rec = doc.project.atlas.furniture(id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    furniture::items(pipeline, doc, rec, target, &mut f, purpose)
}

fn texts(items: &[Item]) -> Vec<String> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(run) => Some(run.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_legend_lists_the_visible_rules_with_their_symbols() {
    let (mut doc, ids) = furniture_doc(&[
        ("Land", POLYGON),
        ("Rivers", LINE),
        ("Roads", LINE),
        ("Towns", POINT),
    ]);
    let [land, rivers, roads, towns] = ids[..] else {
        panic!("{ids:?}")
    };
    let (green, tan, blue) = (
        Color::rgb(20, 160, 40),
        Color::rgb(210, 180, 120),
        Color::rgb(10, 20, 220),
    );
    let fill = |name: &str, color: Color| {
        Rule::new(
            name,
            vec![Symbolizer::Fill {
                color: Property::constant(color),
            }],
        )
    };
    let mut swamp = fill("Swamp", Color::rgb(90, 90, 40));
    swamp.max_scale = Some(1000.0);
    doc.entry_mut(land).unwrap().style.style = Style {
        mask: None,
        rules: vec![fill("Forest", green), fill("Desert", tan), swamp],
    };
    doc.entry_mut(rivers).unwrap().style.style = Style {
        mask: None,
        rules: vec![Rule::new(
            "Default",
            vec![Symbolizer::Stroke(StrokeSymbolizer::constant(
                Stroke::solid(blue, 2.0),
            ))],
        )],
    };
    let mut pipeline = Pipeline::new();
    let frame = first_frame(&mut doc, &mut pipeline);
    let map = doc.first_map();
    assert!(doc.set_shown(map, towns, false));
    let mut legend = LegendItem {
        frame: Some(frame),
        title: "Legend".to_string(),
        title_style: FurnitureText {
            case: TextCase::Upper,
            ..FurnitureText::sized(14.0)
        },
        ..LegendItem::default()
    };
    let rename = LegendEntry {
        name: Some("Big rivers".to_string()),
        hidden: false,
    };
    legend
        .entries
        .insert((rivers, "Default".to_string()), rename);
    let hide = LegendEntry {
        name: None,
        hidden: true,
    };
    legend.entries.insert((roads, "Default".to_string()), hide);
    let id = add_item(
        &mut doc,
        [20.0, 20.0, 120.0, 120.0],
        Furniture::Legend(legend.clone()),
    );

    // The rows are from the top layer down, with the hidden rows, and with
    // no row for a layer that the map does not show or a rule out of scale.
    let record = doc.project.atlas.frame(frame).unwrap().clone();
    let projection = pipeline.projection(&doc, map).unwrap();
    let rows = furniture::legend_rows(&doc, projection, &record, &legend);
    let rows: Vec<_> = (rows.iter())
        .map(|r| (r.label.as_str(), r.rule.as_str(), r.hidden))
        .collect();
    assert_eq!(
        rows,
        [
            ("Roads", "Default", true),
            ("Big rivers", "Default", false),
            ("Forest", "Forest", false),
            ("Desert", "Desert", false),
        ]
    );

    // The title has the style of the title, and the entries the style of
    // the entries.
    let items = furniture_items(&doc, &mut pipeline, id, Purpose::Export);
    assert_eq!(texts(&items), ["LEGEND", "Big rivers", "Forest", "Desert"]);
    let sizes: Vec<f32> = (items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(run) => Some(run.font_size),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, [14.0, 10.0, 10.0, 10.0]);
    let has = |want: Color, fill: bool| {
        items.iter().any(|i| match i {
            Item::Fill { color, .. } => fill && *color == want,
            Item::Stroke { color, .. } => !fill && *color == want,
            _ => false,
        })
    };
    assert!(has(green, true) && has(tan, true) && has(blue, false));
}

#[test]
fn the_scale_bar_is_true_for_any_radius() {
    for radius in [1737.4, 6371.0, 100_000.0] {
        let (mut doc, _) = furniture_doc(&[]);
        doc.project.world.radius_km = radius;
        let map = doc.first_map();
        doc.set_projection(
            map,
            ProjectionSpec::new(ProjectionKind::Equirectangular, 0.0, 0.0),
        );
        let mut pipeline = Pipeline::new();
        let frame = first_frame(&mut doc, &mut pipeline);
        let record = doc.project.atlas.frame_mut(frame).unwrap();
        record.center = [0.0, 0.0];
        record.scale = 5e6;
        let bar = ScaleBarItem {
            frame: Some(frame),
            second_unit: Some(ScaleUnit::Miles),
            ..ScaleBarItem::default()
        };
        let rect = [20.0, 150.0, 90.0, 170.0];
        let id = add_item(&mut doc, rect, Furniture::ScaleBar(bar.clone()));

        // On the equator of an equirectangular map, 1 mm at 1:5,000,000 is
        // 5 km on the ground, at each radius.
        let record = doc.project.atlas.frame(frame).unwrap().clone();
        let projection = pipeline.projection(&doc, map).unwrap();
        let km_per_mm = furniture::scale_at_center(projection, &record, radius).unwrap();
        assert!((km_per_mm - 5.0).abs() < 1e-3, "{radius}: {km_per_mm}");

        // The kilometers and the miles are both true, and the item draws the
        // same bar.
        let mut f = fonts().unwrap();
        let [x0, y0, x1, y1] = rect.map(|v| v * PT_PER_MM);
        let points = kurbo::Rect::new(x0, y0, x1, y1);
        let drawing = scale_bar::draw(&bar, points, 1.0, km_per_mm / PT_PER_MM, &mut f);
        let [km, miles] = &drawing.rows[..] else {
            panic!("{:?}", drawing.rows)
        };
        assert_eq!(
            (km.unit, miles.unit),
            (ScaleUnit::Kilometers, ScaleUnit::Miles)
        );
        for row in [km, miles] {
            let ground = (row.end - row.zero) / PT_PER_MM * km_per_mm;
            let want = f64::from(row.divisions) * row.value * row.unit.km();
            assert!((ground - want).abs() < 1e-6 * want, "{radius} {row:?}");
            let last = &row.numbers.last().unwrap().0;
            let value: f64 = last.replace(',', "").parse().unwrap();
            assert_eq!(value, f64::from(row.divisions) * row.value);
        }
        assert!(
            drawing.bounds.x1 <= rect[2] * PT_PER_MM,
            "{:?}",
            drawing.bounds
        );
        let items = furniture_items(&doc, &mut pipeline, id, Purpose::Export);
        assert_eq!(texts(&items), texts(&drawing.items));
        let outline = |items: &[Item]| {
            (items.iter())
                .find_map(|i| match i {
                    Item::Stroke { path, .. } => Some(path.bounding_box()),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(outline(&items), outline(&drawing.items));

        // A larger scale denominator has more ground in each millimeter.
        doc.project.atlas.frame_mut(frame).unwrap().scale = 1e7;
        let record = doc.project.atlas.frame(frame).unwrap().clone();
        let projection = pipeline.projection(&doc, map).unwrap();
        let doubled = furniture::scale_at_center(projection, &record, radius).unwrap();
        assert!((doubled - 10.0).abs() < 1e-3, "{doubled}");
    }
}

/// The scale bar of the atlas mockup: a line with miles above and
/// kilometers below, at 1:3,500,000.
fn mockup_bar() -> ScaleBarItem {
    let upper = FurnitureText {
        case: TextCase::Upper,
        ..FurnitureText::sized(7.0)
    };
    ScaleBarItem {
        kind: ScaleBarKind::Line,
        unit: ScaleUnit::Miles,
        second_unit: Some(ScaleUnit::Kilometers),
        fit: ScaleFit::AdjustDivisions,
        division_value: Some(30.0),
        second_division_value: Some(60.0),
        subdivisions: 2,
        mark_frequency: TickFrequency::DivisionsAndFirstSubdivisions,
        number_frequency: TickFrequency::DivisionsAndFirstSubdivisions,
        division_mark_height: 1.5,
        subdivision_mark_height: 1.5,
        unit_style: upper,
        ..ScaleBarItem::default()
    }
}

#[test]
fn the_scale_bar_of_the_mockup_has_its_numbers() {
    let mut f = fonts().unwrap();
    // 1:3,500,000 is 3.5 km to a millimeter.
    let km_per_point = 3.5 / PT_PER_MM;
    let rect = kurbo::Rect::new(0.0, 0.0, 100.0 * PT_PER_MM, 15.0 * PT_PER_MM);
    let drawing = scale_bar::draw(&mockup_bar(), rect, 1.0, km_per_point, &mut f);
    let numbers = |row: &scale_bar::BarRow| {
        let n = row.numbers.iter().map(|n| n.0.as_str());
        n.collect::<Vec<_>>().join(" ")
    };
    let [miles, km] = &drawing.rows[..] else {
        panic!("{:?}", drawing.rows)
    };
    assert_eq!(numbers(miles), "0 15 30 60 90 120 150");
    assert_eq!(numbers(km), "0 30 60 120 180 240");
    let all = texts(&drawing.items);
    assert!(all.contains(&"MILES".to_string()) && all.contains(&"KILOMETERS".to_string()));
    // Each unit label is after the numbers of its unit.
    let x = |text: &str| {
        (drawing.items.iter())
            .find_map(|i| match i {
                Item::Glyphs(run) if run.text == text => Some(run.glyphs[0].1.translation()),
                _ => None,
            })
            .unwrap()
    };
    assert!(x("MILES").x > miles.numbers.last().unwrap().1);
    assert!(x("KILOMETERS").x > km.numbers.last().unwrap().1);
    assert!(x("MILES").y < x("KILOMETERS").y);
}

/// Saves each kind of scale bar and the bar of the mockup to
/// `target/app/scalebars.png` for a person to look at.
#[test]
#[ignore = "manual"]
fn manual_scale_bar_pictures() {
    if std::env::var_os("VALE_SCREENSHOTS").is_none() {
        return;
    }
    let mut f = fonts().unwrap();
    let (w, h) = (110.0 * PT_PER_MM, 16.0 * PT_PER_MM);
    let mut bars: Vec<(ScaleBarItem, f64)> = Vec::new();
    for kind in ScaleBarKind::ALL {
        let bar = ScaleBarItem {
            kind,
            subdivisions: 4,
            ..ScaleBarItem::default()
        };
        bars.push((bar, 3.5));
    }
    bars.push((mockup_bar(), 3.5));
    let double = ScaleBarItem {
        second_unit: Some(ScaleUnit::Miles),
        before_zero: true,
        subdivisions: 2,
        number_frequency: TickFrequency::DivisionsAndFirstSubdivisions,
        unit_label_style: vale_store::UnitLabelStyle::Abbreviation,
        ..ScaleBarItem::default()
    };
    bars.push((double, 3.5));
    let centered = ScaleBarItem {
        kind: ScaleBarKind::Hollow,
        bar_height: 3.0,
        number_position: ScalePosition::Center,
        mark_frequency: TickFrequency::None,
        unit_position: vale_store::UnitPosition::BelowCenter,
        number_style: FurnitureText {
            halo: true,
            ..FurnitureText::sized(6.0)
        },
        ..ScaleBarItem::default()
    };
    bars.push((centered, 3.5));
    let hatched = |kind, background| vale_store::FillPattern {
        kind,
        background,
        ..vale_store::FillPattern::default()
    };
    let hatch = ScaleBarItem {
        subdivisions: 4,
        fill1: hatched(vale_store::FillKind::Hatch, None),
        ..ScaleBarItem::default()
    };
    bars.push((hatch, 3.5));
    let cross = ScaleBarItem {
        kind: ScaleBarKind::DoubleAlternating,
        bar_height: 3.0,
        fill1: hatched(
            vale_store::FillKind::CrossHatch,
            Some([0xf2, 0xe6, 0xc8, 0xff]),
        ),
        ..ScaleBarItem::default()
    };
    bars.push((cross, 3.5));
    let doubling = ScaleBarItem {
        spacing: vale_store::DivisionSpacing::Doubling,
        divisions: 5,
        subdivisions: 2,
        before_zero: true,
        ..ScaleBarItem::default()
    };
    bars.push((doubling, 3.5));
    let mut list = vale_render::DisplayList::new(w, h * bars.len() as f64);
    list.items.push(Item::Fill {
        path: kurbo::Rect::new(0.0, 0.0, w, h * bars.len() as f64).to_path(0.1),
        color: Color::rgb(255, 255, 255),
        rule: FillRule::NonZero,
    });
    for (k, (bar, km_per_mm)) in bars.iter().enumerate() {
        let rect = kurbo::Rect::new(0.0, k as f64 * h, w, (k + 1) as f64 * h);
        let drawing = scale_bar::draw(bar, rect, 1.0, km_per_mm / PT_PER_MM, &mut f);
        list.items.extend(drawing.items);
    }
    let px = render_rgba(&list, 4.0).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/scalebars.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_raw(px.width as u32, px.height as u32, px.data)
        .unwrap()
        .save(path)
        .unwrap();
}

#[test]
fn the_north_arrow_follows_the_rotation() {
    let (mut doc, _) = furniture_doc(&[]);
    let mut pipeline = Pipeline::new();
    let map = doc.first_map();
    let frame = first_frame(&mut doc, &mut pipeline);
    let north = |doc: &Document, pipeline: &mut Pipeline| {
        let record = doc.project.atlas.frame(frame).unwrap().clone();
        let projection = pipeline.projection(doc, map).unwrap();
        furniture::north(projection, &record).unwrap()
    };
    // A small frame near the center of the map.
    let record = doc.project.atlas.frame_mut(frame).unwrap();
    record.scale = 1e6;
    record.center = [10.0, 20.0];
    let (straight, variation) = north(&doc, &mut pipeline);
    assert!(variation < 1.0, "{variation}");
    doc.project.atlas.frame_mut(frame).unwrap().rotation = 30.0;
    let (turned, _) = north(&doc, &mut pipeline);
    assert!(
        (turned - straight - 30.0).abs() < 1e-6,
        "{straight} {turned}"
    );

    // A frame of the whole world has a different north at its edges, so
    // the arrow is not in the export, and the screen shows a dashed box.
    let arrow = NorthArrowItem {
        frame: Some(frame),
        hide_if_varies: true,
        ..NorthArrowItem::default()
    };
    let id = add_item(
        &mut doc,
        [20.0, 20.0, 50.0, 50.0],
        Furniture::NorthArrow(arrow),
    );
    let shown = furniture_items(&doc, &mut pipeline, id, Purpose::Export);
    assert_eq!(texts(&shown), ["N"]);
    let mut page = doc.project.atlas.pages[0].clone();
    page.frames.clear();
    let projection = pipeline.projection(&doc, map).unwrap();
    let whole = new_frame(&page, &doc.project.atlas.maps[0], projection);
    let record = doc.project.atlas.frame_mut(frame).unwrap();
    record.scale = whole.scale;
    record.center = [0.0, 0.0];
    record.rotation = 0.0;
    let (_, variation) = north(&doc, &mut pipeline);
    assert!(variation > furniture::MAX_VARIATION, "{variation}");
    let record = doc.project.atlas.frame_mut(frame).unwrap();
    record.center = [60.0, 30.0];
    let (_, variation) = north(&doc, &mut pipeline);
    assert!(variation > furniture::MAX_VARIATION, "{variation}");
    assert!(furniture_items(&doc, &mut pipeline, id, Purpose::Export).is_empty());
    let screen = furniture_items(&doc, &mut pipeline, id, Purpose::Screen);
    assert!(matches!(screen[..], [Item::Stroke { .. }]));
}

#[test]
fn furniture_stays_vector_and_text_in_pdf() {
    let (mut doc, _) = furniture_doc(&[("Land", POLYGON), ("Rivers", LINE), ("Towns", POINT)]);
    let mut pipeline = Pipeline::new();
    let frame = first_frame(&mut doc, &mut pipeline);
    let text = TextItem {
        text: "A world".to_string(),
        ..TextItem::default()
    };
    let legend = LegendItem {
        frame: Some(frame),
        title: "Legend".to_string(),
        ..LegendItem::default()
    };
    let bar = ScaleBarItem {
        frame: Some(frame),
        ..ScaleBarItem::default()
    };
    // The frame shows the whole world, where north varies.
    let arrow = NorthArrowItem {
        frame: Some(frame),
        hide_if_varies: false,
        ..NorthArrowItem::default()
    };
    let raster = (doc.project).add_raster("Height".to_string(), 64, "m".to_string(), -50.0, 50.0);
    let key = ColorKeyItem {
        frame: Some(frame),
        layer: Some(doc.project.raster_layer(raster).unwrap().id),
        ..ColorKeyItem::default()
    };
    let ids = [
        add_item(&mut doc, [20.0, 20.0, 120.0, 40.0], Furniture::Text(text)),
        add_item(
            &mut doc,
            [20.0, 50.0, 120.0, 120.0],
            Furniture::Legend(legend),
        ),
        add_item(
            &mut doc,
            [20.0, 150.0, 90.0, 170.0],
            Furniture::ScaleBar(bar),
        ),
        add_item(
            &mut doc,
            [200.0, 20.0, 230.0, 50.0],
            Furniture::NorthArrow(arrow),
        ),
        add_item(
            &mut doc,
            [130.0, 150.0, 230.0, 170.0],
            Furniture::ColorKey(key),
        ),
    ];
    let mut all = Vec::new();
    for id in ids {
        let items = furniture_items(&doc, &mut pipeline, id, Purpose::Export);
        assert!(!texts(&items).is_empty(), "{id:?}");
        all.extend(items);
    }
    assert!(
        (all.iter())
            .all(|i| matches!(i, Item::Fill { .. } | Item::Stroke { .. } | Item::Glyphs(_))),
        "{:?}",
        all.iter().map(describe).collect::<Vec<_>>()
    );

    // The page draws the furniture over the frames.
    let page = doc.project.atlas.pages[0].clone();
    let mut f = fonts().unwrap();
    let list = pipeline
        .compose_page(&doc, &page, &mut f, Quality::Final)
        .unwrap();
    let tail = &list.items[list.items.len() - all.len()..];
    assert_eq!(
        tail.iter().map(describe).collect::<Vec<_>>(),
        all.iter().map(describe).collect::<Vec<_>>()
    );

    // The export of a raster layer needs a GPU, so the PDF leaves it out.
    let map = doc.project.atlas.frame(frame).unwrap().map;
    let height = doc.project.raster_layer(raster).unwrap().id;
    assert!(doc.set_shown(map, height, false));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.pdf");
    vale_app::atlas_export::export_pdf(&doc, vec![page], &path).unwrap();
    let pdf = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    assert!(pdf.contains("/BaseFont"));
    assert!(!pdf.contains("/Subtype/Image"));
}

#[test]
fn an_item_of_furniture_renders_to_an_image() {
    let (mut doc, _) = furniture_doc(&[]);
    let text = TextItem {
        text: "A world".to_string(),
        ..TextItem::default()
    };
    let id = add_item(&mut doc, [20.0, 20.0, 120.0, 40.0], Furniture::Text(text));
    let rec = doc.project.atlas.furniture(id).unwrap().clone();
    let mut f = fonts().unwrap();
    let mut pipeline = Pipeline::new();
    let (image, rect) =
        furniture::render(&mut pipeline, &doc, &rec, &mut f, 2.0, Purpose::Screen).unwrap();
    let m = furniture::RENDER_MARGIN_MM;
    assert_eq!(
        rect,
        kurbo::Rect::new(20.0 - m, 20.0 - m, 120.0 + m, 40.0 + m)
    );
    let width = (rect.width() * PT_PER_MM * 2.0).round() as usize;
    assert!(image.width.abs_diff(width) <= 1, "{} {width}", image.width);
    assert!(image.data.chunks(4).any(|px| px[3] > 0));

    // An arrow with no frame draws only on the screen.
    let arrow = add_item(
        &mut doc,
        [20.0, 50.0, 50.0, 80.0],
        Furniture::NorthArrow(NorthArrowItem::default()),
    );
    assert!(furniture_items(&doc, &mut pipeline, arrow, Purpose::Export).is_empty());
    assert!(!furniture_items(&doc, &mut pipeline, arrow, Purpose::Screen).is_empty());
}

/// A point layer at (0, 0) with one marker of `shape`, a fill, and `hatch`.
fn marker_doc(shape: MarkerShape, hatch: Option<Hatch>) -> Document {
    let (mut doc, id) = one_layer(
        "towns",
        &[
            r#"{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}}"#
                .to_string(),
        ],
    );
    doc.entry_mut(id).unwrap().style.style = style_of(Symbolizer::Marker {
        radius: Property::constant(20.0),
        shape,
        fill: Some(Property::constant(Color::rgb(200, 20, 20))),
        hatch,
        stroke: Some(StrokeSymbolizer::constant(Stroke::solid(
            Color::rgb(20, 20, 200),
            1.0,
        ))),
    });
    doc
}

/// Whether `p` is inside the polygon `ring` or within `tol` of its edges.
fn inside_or_on(ring: &[Point], p: Point, tol: f64) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let line = kurbo::Line::new(a, b);
        if kurbo::ParamCurveNearest::nearest(&line, p, 1e-9)
            .distance_sq
            .sqrt()
            <= tol
        {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[test]
fn a_cross_hatched_star_draws_lines_inside_its_outline_and_no_solid_fill() {
    let hatch = Hatch {
        cross: true,
        angle: 30.0,
        spacing: 3.0,
        line_width: 0.5,
    };
    let doc = marker_doc(MarkerShape::Star, Some(hatch));
    let mut p = Pipeline::new();
    let c = compose(&doc, &mut p);
    let red = vale_render::Color::rgb(200, 20, 20);
    assert!(
        !c.list
            .items
            .iter()
            .any(|i| matches!(i, Item::Fill { color, .. } if *color == red)),
        "a hatched marker has no solid fill"
    );
    let hatch_path = c
        .list
        .items
        .iter()
        .find_map(|i| match i {
            Item::Stroke {
                path, color, width, ..
            } if *color == red => Some((path.clone(), *width)),
            _ => None,
        })
        .expect("hatch lines");
    assert_eq!(hatch_path.1, 0.5);
    let center = p.lonlat_to_page(&doc, SIZE, [0.0, 0.0]).unwrap().unwrap();
    let star = vale_app::pipeline::marker_outline(MarkerShape::Star, center, 20.0);
    let segments = subpaths(&hatch_path.0);
    assert!(segments.len() > 6, "{}", segments.len());
    let mut angles = Vec::new();
    for s in &segments {
        assert_eq!(s.len(), 2);
        for q in s {
            assert!(inside_or_on(&star, *q, 1e-6), "{q:?} is outside the star");
        }
        let d = s[1] - s[0];
        let a = (-d.y).atan2(d.x).to_degrees().rem_euclid(180.0);
        if !angles.iter().any(|b: &f64| (a - b).abs() < 1e-6) {
            angles.push(a);
        }
    }
    angles.sort_by(f64::total_cmp);
    assert_eq!(angles.len(), 2, "{angles:?}");
    assert!((angles[0] - 30.0).abs() < 1e-6 && (angles[1] - 120.0).abs() < 1e-6);
}

#[test]
fn a_solid_square_marker_fills_a_square() {
    let doc = marker_doc(MarkerShape::Square, None);
    let mut p = Pipeline::new();
    let c = compose(&doc, &mut p);
    let red = vale_render::Color::rgb(200, 20, 20);
    let path = c
        .list
        .items
        .iter()
        .find_map(|i| match i {
            Item::Fill { path, color, .. } if *color == red => Some(path.clone()),
            _ => None,
        })
        .expect("a solid fill");
    let center = p.lonlat_to_page(&doc, SIZE, [0.0, 0.0]).unwrap().unwrap();
    let corners = path
        .elements()
        .iter()
        .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_) | kurbo::PathEl::LineTo(_)))
        .count();
    assert_eq!(corners, 4, "a polygon, not curves");
    let b = path.bounding_box();
    assert!((b.width() - 40.0).abs() < 1e-6 && (b.height() - 40.0).abs() < 1e-6);
    assert!((b.center() - center).hypot() < 1e-6);
}

const MASKED_FILL: Color = Color::rgb(200, 10, 10);
const MASKED_STROKE: Color = Color::rgb(10, 200, 10);

/// A document with a layer for each GeoJSON geometry, with no labels and
/// no graticule. Each layer has a red fill and a green stroke.
fn shapes(geometries: &[&str]) -> (Document, Vec<vale_store::LayerId>) {
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    let ids = geometries
        .iter()
        .map(|g| {
            let imported = vale_import::geojson::read_str("shape", g).unwrap();
            let id = doc.add_imported(imported, None)[0];
            let style = &mut doc.entry_mut(id).unwrap().style;
            style.set_fill(Some(MASKED_FILL));
            style.set_stroke(Some(Stroke::solid(MASKED_STROKE, 2.0)));
            id
        })
        .collect();
    (doc, ids)
}

/// The square from `(x0, y0)` to `(x1, y1)` as a GeoJSON polygon.
fn square_json(x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    format!(
        r#"{{"type":"Polygon","coordinates":[[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}],[{x0},{y0}]]]}}"#
    )
}

/// The square from `(x0, y0)` to `(x1, y1)` as a GeoJSON polygon with a
/// vertex at each degree, so that its great-circle edges stay near the
/// parallels.
fn dense_square_json(x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    let steps = |a: f64, b: f64| {
        let n = (b - a).abs().ceil() as usize;
        (0..n).map(move |k| a + (b - a) * k as f64 / n as f64)
    };
    let mut ring: Vec<[f64; 2]> = Vec::new();
    ring.extend(steps(x0, x1).map(|x| [x, y0]));
    ring.extend(steps(y0, y1).map(|y| [x1, y]));
    ring.extend(steps(x1, x0).map(|x| [x, y1]));
    ring.extend(steps(y1, y0).map(|y| [x0, y]));
    ring.push(ring[0]);
    let points: Vec<String> = ring
        .iter()
        .map(|p| format!("[{},{}]", p[0], p[1]))
        .collect();
    format!(
        r#"{{"type":"Polygon","coordinates":[[{}]]}}"#,
        points.join(",")
    )
}

/// The points of each subpath of a path.
fn poly_points(path: &kurbo::BezPath) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => out.push(vec![p]),
            kurbo::PathEl::LineTo(p) => out.last_mut().unwrap().push(p),
            _ => {}
        }
    }
    out
}

/// The subpaths of the fills and the strokes in the colors of `shapes`.
fn masked_paths(items: &[Item]) -> (Vec<Vec<Point>>, Vec<Vec<Point>>) {
    let (mut fills, mut strokes) = (Vec::new(), Vec::new());
    for item in items {
        match item {
            Item::Fill { path, color, .. } if *color == MASKED_FILL => {
                fills.extend(poly_points(path))
            }
            Item::Stroke { path, color, .. } if *color == MASKED_STROKE => {
                strokes.extend(poly_points(path))
            }
            _ => {}
        }
    }
    (fills, strokes)
}

/// The longitude and latitude of each point of the paths of a compose.
fn to_lonlat(
    doc: &Document,
    pipeline: &mut Pipeline,
    paths: &[Vec<Point>],
) -> Vec<Vec<vale_sphere::LonLat>> {
    (paths.iter())
        .map(|p| {
            (p.iter())
                .map(|q| pipeline.page_to_lonlat(doc, SIZE, *q).unwrap().unwrap())
                .collect()
        })
        .collect()
}

/// The longitudes of the paths, from the least to the most.
fn lon_range(paths: &[Vec<vale_sphere::LonLat>]) -> (f64, f64) {
    let lons = paths.iter().flatten().map(|p| p[0]);
    lons.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
        (a.min(x), b.max(x))
    })
}

/// No step of a stroke goes along the meridian `lon`.
fn assert_no_step_along(strokes: &[Vec<vale_sphere::LonLat>], lon: f64) {
    for s in strokes {
        for w in s.windows(2) {
            let on = |p: vale_sphere::LonLat| (p[0] - lon).abs() < 0.05;
            assert!(
                !(on(w[0]) && on(w[1]) && (w[0][1] - w[1][1]).abs() > 0.05),
                "a stroke goes along the edge of the mask: {w:?}"
            );
        }
    }
}

#[test]
fn a_polygon_mask_cuts_the_fill_and_the_stroke_of_a_polygon() {
    let (mut doc, ids) = shapes(&[
        &square_json(0.0, 0.0, 40.0, 40.0),
        &square_json(-10.0, -10.0, 20.0, 50.0),
    ]);
    let (layer, mask) = (ids[0], ids[1]);
    doc.set_visible(mask, false);
    let mut pipeline = Pipeline::new();
    let whole = masked_paths(&compose(&doc, &mut pipeline).list.items);
    let (_, east) = lon_range(&to_lonlat(&doc, &mut pipeline, &whole.0));
    assert!((east - 40.0).abs() < 0.1, "{east}");

    doc.entry_mut(layer).unwrap().style.style.mask = Some(Mask::Polygons { layer: mask });
    let (fills, strokes) = masked_paths(&compose(&doc, &mut pipeline).list.items);
    let (fills, strokes) = (
        to_lonlat(&doc, &mut pipeline, &fills),
        to_lonlat(&doc, &mut pipeline, &strokes),
    );
    let (west, east) = lon_range(&fills);
    assert!(west > -0.1 && (east - 20.0).abs() < 0.1, "{west} {east}");
    let (west, east) = lon_range(&strokes);
    assert!(west > -0.1 && (east - 20.0).abs() < 0.1, "{west} {east}");
    assert_no_step_along(&strokes, 20.0);
}

#[test]
fn a_mask_cuts_the_stroke_of_a_line() {
    let line = r#"{"type":"LineString","coordinates":[[-40,10],[40,10]]}"#;
    let (mut doc, ids) = shapes(&[line, &square_json(-10.0, -10.0, 20.0, 50.0)]);
    doc.set_visible(ids[1], false);
    doc.entry_mut(ids[0]).unwrap().style.style.mask = Some(Mask::Polygons { layer: ids[1] });
    let mut pipeline = Pipeline::new();
    let (_, strokes) = masked_paths(&compose(&doc, &mut pipeline).list.items);
    let (west, east) = lon_range(&to_lonlat(&doc, &mut pipeline, &strokes));
    assert!(
        (west + 10.0).abs() < 0.1 && (east - 20.0).abs() < 0.1,
        "{west} {east}"
    );
}

/// The box around the points of paths.
fn path_box(paths: &[Vec<Point>]) -> kurbo::Rect {
    let mut points = paths.iter().flatten();
    let first = *points.next().expect("a point");
    points.fold(kurbo::Rect::from_points(first, first), |r, p| {
        r.union_pt(*p)
    })
}

#[test]
fn a_mask_cuts_a_polygon_in_a_map_frame() {
    let (mut doc, ids) = shapes(&[
        &square_json(0.0, 0.0, 40.0, 40.0),
        &square_json(-10.0, -10.0, 20.0, 50.0),
    ]);
    doc.set_visible(ids[1], false);
    let map = doc.project.atlas.maps[0].id;
    doc.set_shown(map, ids[1], false);
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let frame = frame_on(&mut doc, &mut pipeline, page);
    let whole = masked_paths(&compose_frame(&doc, &mut pipeline, frame));
    doc.entry_mut(ids[0]).unwrap().style.style.mask = Some(Mask::Polygons { layer: ids[1] });
    let (fills, strokes) = masked_paths(&compose_frame(&doc, &mut pipeline, frame));
    let (before, after) = (path_box(&whole.0), path_box(&fills));
    let ratio = after.width() / before.width();
    assert!((0.4..0.6).contains(&ratio), "{ratio}");
    assert!((after.x0 - before.x0).abs() < 0.5);
    let stroke = path_box(&strokes);
    assert!((stroke.x1 - after.x1).abs() < 0.5);
    // No step of the stroke goes along the cut.
    for s in &strokes {
        for w in s.windows(2) {
            let on = |p: Point| (p.x - stroke.x1).abs() < 0.2;
            assert!(
                !(on(w[0]) && on(w[1]) && (w[0].y - w[1].y).abs() > 0.2),
                "{w:?}"
            );
        }
    }
}

#[test]
fn a_pdf_has_the_masked_fill_and_stroke_as_paths_that_follow_the_coast() {
    // A hidden polygon within 60 degrees of 0 degrees east, 0 degrees
    // north, and a polygon over its edge with it as its mask.
    let ring: Vec<String> = (0..=360)
        .map(|i| {
            let (b, r) = (-(i as f64).to_radians(), 60f64.to_radians());
            let lat = (r.sin() * b.cos()).asin().to_degrees();
            let lon = (b.sin() * r.sin()).atan2(r.cos()).to_degrees();
            format!("[{lon},{lat}]")
        })
        .collect();
    let circle = format!(
        r#"{{"type":"Polygon","coordinates":[[{}]]}}"#,
        ring.join(",")
    );
    let (mut doc, ids) = shapes(&[&dense_square_json(-80.0, -20.0, 80.0, 20.0), &circle]);
    let (id, mask) = (ids[0], ids[1]);
    doc.entry_mut(id).unwrap().style.style.mask = Some(Mask::Polygons { layer: mask });
    let center = vale_app::globe::math::lonlat_to_dir(0.0, 0.0);

    let map = doc.first_map();
    doc.set_visible(mask, false);
    doc.set_shown(map, mask, false);
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    frame_on(&mut doc, &mut pipeline, page);
    let page = doc.project.atlas.pages[0].clone();
    let mut f = fonts().unwrap();
    let list = pipeline
        .compose_page(&doc, &page, &mut f, Quality::Final)
        .unwrap();
    assert!(!(list.items.iter()).any(|i| matches!(i, Item::Rasterize { .. } | Item::Image { .. })));
    let (fills, strokes) = masked_paths(&list.items);
    assert!(!fills.is_empty() && !strokes.is_empty());

    // Each point is inside the mask, and the cut is on its edge.
    let record = page.frames[0].clone();
    let projection = pipeline.projection(&doc, map).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = frame_transform(projection, &record, target);
    let from_center = |q: &Point| {
        let [lon, lat] = projection.inverse(vt.from_page(*q)).unwrap();
        let d = vale_app::globe::math::lonlat_to_dir(lon, lat);
        vale_app::globe::math::angle(d, center).to_degrees()
    };
    // The fill follows the edge, and the stroke stops at the edge four times.
    for (paths, on_edge) in [(&fills, 10), (&strokes, 4)] {
        let far: Vec<f64> = paths.iter().flatten().map(from_center).collect();
        assert!(far.iter().all(|&a| a < 60.1), "{far:?}");
        let n = far.iter().filter(|&&a| (a - 60.0).abs() < 0.1).count();
        assert!(n >= on_edge, "{n}");
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("masked.pdf");
    vale_app::atlas_export::export_pdf(&doc, vec![page], &path).unwrap();
    let pdf = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    assert!(!pdf.contains("/Subtype/Image"));
}

/// A document with two polygon layers side by side, each with a fill and a
/// stroke. The first layer has a raster mask.
fn masked_doc() -> (Document, vale_store::LayerId) {
    let polygon = |x0: f64, x1: f64| {
        format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{{"type":"Polygon","coordinates":[[[{x0},-30],[{x1},-30],[{x1},30],[{x0},30],[{x0},-30]]]}}}}]}}"#
        )
    };
    let mut doc = Document::empty();
    doc.frame.labels = false;
    doc.frame.graticule = false;
    let a = vale_import::geojson::read_str("a", &polygon(-60.0, -5.0)).unwrap();
    let b = vale_import::geojson::read_str("b", &polygon(5.0, 60.0)).unwrap();
    let a = doc.add_imported(a, None)[0];
    let b = doc.add_imported(b, None)[0];
    let style = |fill: Color, stroke: Color| Style {
        mask: None,
        rules: vec![Rule::new(
            "Default",
            vec![
                Symbolizer::Fill {
                    color: Property::constant(fill),
                },
                Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke::solid(stroke, 2.0))),
            ],
        )],
    };
    doc.entry_mut(a).unwrap().style.style = Style {
        mask: Some(Mask::Raster {
            layer: b,
            min: Some(0.0),
            max: None,
            offset: 0.0,
        }),
        ..style(MASKED_FILL, MASKED_STROKE)
    };
    doc.entry_mut(b).unwrap().style.style = style(OTHER_FILL, Color::rgb(0, 0, 100));
    (doc, a)
}

const OTHER_FILL: Color = Color::rgb(0, 0, 200);

fn item_color(item: &Item) -> Option<Color> {
    match item {
        Item::Fill { color, .. } | Item::Stroke { color, .. } => Some(*color),
        _ => None,
    }
}

#[test]
fn a_page_mask_clips_the_fills_and_the_strokes_of_its_layer() {
    use vale_app::mask::PageMask;
    let (mut doc, a) = masked_doc();
    let mut pipeline = Pipeline::new();
    let page = doc.project.atlas.pages[0].id;
    let frame = frame_on(&mut doc, &mut pipeline, page);
    let masked = |c: Option<Color>| c == Some(MASKED_FILL) || c == Some(MASKED_STROKE);

    // With no page mask yet, the masked layer draws no fill and no stroke.
    let unmasked = compose_frame_full(&doc, &mut pipeline, frame).list;
    assert!(!unmasked.items.iter().any(|i| masked(item_color(i))));
    assert!(
        unmasked
            .items
            .iter()
            .any(|i| item_color(i) == Some(OTHER_FILL))
    );

    let square = vec![
        Point::new(0.0, 0.0),
        Point::new(150.0, 0.0),
        Point::new(150.0, 150.0),
        Point::new(0.0, 150.0),
    ];
    pipeline.set_page_mask(frame, a, Some(PageMask::new(vec![square])));
    let list = compose_frame_full(&doc, &mut pipeline, frame).list;
    let clips: Vec<usize> = (list.items.iter().enumerate())
        .filter(|(_, i)| {
            matches!(
                i,
                Item::PushClip {
                    rule: FillRule::NonZero,
                    ..
                }
            )
        })
        .map(|(k, _)| k)
        .collect();
    assert_eq!(clips.len(), 1);
    let start = clips[0];
    let end = start
        + (list.items[start..].iter())
            .position(|i| matches!(i, Item::PopClip))
            .unwrap();
    let inside = &list.items[start + 1..end];
    assert!(inside.iter().any(|i| matches!(i, Item::Fill { .. })));
    assert!(inside.iter().any(|i| matches!(i, Item::Stroke { .. })));
    assert!(inside.iter().all(|i| masked(item_color(i))));
    let outside: Vec<&Item> = (list.items[..start].iter())
        .chain(&list.items[end..])
        .collect();
    assert!(outside.iter().all(|i| !masked(item_color(i))));
    assert!(outside.iter().any(|i| item_color(i) == Some(OTHER_FILL)));

    // The PDF has one more nonzero clip path than with no mask.
    let pdf = vale_render::render_pdf(&list).unwrap();
    let before = vale_render::render_pdf(&unmasked).unwrap();
    assert_eq!(masked::pdf_clips(&pdf).0, masked::pdf_clips(&before).0 + 1);
    assert_eq!(masked::pdf_clips(&pdf).1, masked::pdf_clips(&before).1);
}

/// A document with one raster layer of `unit` with bands at `stops`, the
/// first and last of which are the ends of its range, and with `names`.
/// Returns the document, the pipeline, a frame of its map, and the layer.
fn key_doc(
    unit: &str,
    stops: &[f64],
    names: &[&str],
) -> (Document, Pipeline, FrameId, vale_store::LayerId) {
    let (mut doc, _) = furniture_doc(&[]);
    let (min, max) = (stops[0], stops[stops.len() - 1]);
    let raster = (doc.project).add_raster("Key".to_string(), 64, unit.to_string(), min, max);
    let record = doc.project.raster_mut(raster).unwrap();
    record.stops = (stops.iter())
        .map(|&value| vale_store::Stop {
            value,
            line: vale_store::StopLine::default(),
        })
        .collect();
    record.colors = (0..stops.len() - 1)
        .map(|i| [(40 * i % 256) as u8, 120, (200 - 20 * i % 200) as u8])
        .collect();
    record.names = names.iter().map(|n| n.to_string()).collect();
    let layer = doc.project.raster_layer(raster).unwrap().id;
    let mut pipeline = Pipeline::new();
    let frame = first_frame(&mut doc, &mut pipeline);
    (doc, pipeline, frame, layer)
}

/// The stops of a height layer from 6,000 m below sea level to 6,000 m
/// above it.
const HEIGHT_STOPS: [f64; 7] = [-6000.0, -4000.0, -200.0, 0.0, 200.0, 1000.0, 6000.0];
/// The stops of a precipitation layer in millimeters.
const RAIN_STOPS: [f64; 6] = [0.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0];

/// Adds a color key to page 0 and returns its items.
fn key_items(doc: &mut Document, pipeline: &mut Pipeline, key: ColorKeyItem) -> Vec<Item> {
    let id = add_item(doc, [20.0, 20.0, 160.0, 70.0], Furniture::ColorKey(key));
    furniture_items(doc, pipeline, id, Purpose::Export)
}

/// The x of the start of each run of glyphs, by its text.
fn run_x(items: &[Item], text: &str) -> Vec<f64> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(run) if run.text == text => Some(run.glyphs[0].1.translation().x),
            _ => None,
        })
        .collect()
}

/// The boxes of the fills of `items`.
fn fill_boxes(items: &[Item]) -> Vec<kurbo::Rect> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Fill { path, .. } => Some(path.bounding_box()),
            _ => None,
        })
        .collect()
}

fn strip(frame: FrameId, layer: vale_store::LayerId) -> ColorKeyItem {
    ColorKeyItem {
        frame: Some(frame),
        layer: Some(layer),
        ..ColorKeyItem::default()
    }
}

#[test]
fn the_strip_labels_inner_lines_and_closed_ends() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let key = ColorKeyItem {
        split_at_sea_level: false,
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    // Both ends are open, so each value is at a line between two boxes.
    assert_eq!(
        texts(&items),
        ["-4,000", "-200", "0", "200", "1,000", "meters"]
    );
    let boxes = fill_boxes(&items);
    assert_eq!(boxes.len(), 6);
    for b in &boxes {
        assert!((b.width() - boxes[0].width()).abs() < 1e-6, "{boxes:?}");
        assert!((b.y0 - boxes[0].y0).abs() < 1e-6, "{boxes:?}");
    }
    for w in boxes.windows(2) {
        assert!((w[0].x1 - w[1].x0).abs() < 1e-6, "{boxes:?}");
    }
    // A value is centered on its line.
    let zero = run_x(&items, "0")[0];
    let mut f = fonts().unwrap();
    let style = vale_labeler::TextStyle::new(vale_app::fonts::DEFAULT_FAMILY, 8.0);
    let half = f.measure("0", &style).advance / 2.0;
    assert!(
        (zero + half - boxes[3].x0).abs() < 0.5,
        "{zero} {:?}",
        boxes[3]
    );

    // With a closed end, the end has a value too.
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let items = key_items(&mut doc, &mut pipeline, strip(frame, layer));
    assert_eq!(
        texts(&items),
        ["4,000", "200", "0", "meters", "0", "200", "1,000", "meters"]
    );
}

#[test]
fn the_split_strip_puts_the_unit_at_the_open_end() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let items = key_items(&mut doc, &mut pipeline, strip(frame, layer));
    let boxes = fill_boxes(&items);
    // The sea row is above the land row, and both start at the left.
    assert_eq!(boxes.len(), 6);
    assert!(boxes[0].y1 < boxes[3].y0, "{boxes:?}");
    assert!((boxes[0].x0 - boxes[3].x0).abs() < 1e-6, "{boxes:?}");
    // The deepest band is at the left of the sea row, so its open end is
    // at the left, and the unit is left of the value next to it.
    let units = run_x(&items, "meters");
    assert!(units[0] < run_x(&items, "4,000")[0], "{units:?}");
    // The land row is open at the right.
    assert!(units[1] > run_x(&items, "1,000")[0], "{units:?}");
}

#[test]
fn the_second_unit_rounds_to_the_primary_decimals() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let feet = SecondUnit {
        name: "feet".to_string(),
        factor: 3.28084,
        offset: 0.0,
    };
    let key = ColorKeyItem {
        second_unit: Some(feet),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(
        texts(&items),
        [
            "4,000", "200", "0", "meters", "13,123", "656", "0", "feet", "0", "200", "1,000",
            "meters", "0", "656", "3,281", "feet",
        ]
    );
    // A value with decimals keeps them in the second unit.
    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &[0.0, 12.5, 250.0, 4000.0], &[]);
    let inches = SecondUnit {
        name: "in".to_string(),
        factor: 0.0393701,
        offset: 0.0,
    };
    let key = ColorKeyItem {
        second_unit: Some(inches),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(
        texts(&items),
        ["12.5", "250", "millimeters", "0.5", "10", "in"]
    );
}

#[test]
fn the_list_key_shows_names_or_ranges() {
    let (mut doc, mut pipeline, frame, layer) =
        key_doc("mm", &RAIN_STOPS, &["", "", "", "", "Very wet"]);
    let mut key = ColorKeyItem {
        layout: KeyLayout::List,
        title: "Annual precipitation in mm".to_string(),
        ..strip(frame, layer)
    };
    let rename = LegendEntry {
        name: Some("Dry".to_string()),
        hidden: false,
    };
    key.bands.insert("250".to_string(), rename);
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(
        texts(&items),
        [
            "Annual precipitation in mm",
            "Very wet",
            "1,000–2,000",
            "500–1,000",
            "Dry",
            "Under 250",
        ]
    );
    // The boxes are in a column with space between them.
    let boxes = fill_boxes(&items);
    assert_eq!(boxes.len(), 5);
    for w in boxes.windows(2) {
        assert!(w[1].y0 > w[0].y1 + 1.0, "{boxes:?}");
        assert!((w[1].x0 - w[0].x0).abs() < 1e-6, "{boxes:?}");
    }
}

#[test]
fn names_in_boxes_draws_the_band_name_inside() {
    let (mut doc, mut pipeline, frame, layer) =
        key_doc("", &[0.0, 10.0, 20.0, 30.0], &["1-A", "1", "2-a"]);
    let mut key = ColorKeyItem {
        layout: KeyLayout::List,
        names_in_boxes: true,
        ..strip(frame, layer)
    };
    let rename = LegendEntry {
        name: Some("Intermediate".to_string()),
        hidden: false,
    };
    key.bands.insert("1".to_string(), rename);
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(
        texts(&items),
        ["2-a", "Over 20", "1", "Intermediate", "1-A", "Under 10"]
    );
    let boxes = fill_boxes(&items);
    let inside = run_x(&items, "1")[0];
    assert!(
        boxes[1].x0 < inside && inside < boxes[1].x1,
        "{inside} {boxes:?}"
    );
    assert!(run_x(&items, "Intermediate")[0] > boxes[1].x1);
}

/// The color and the font size of the first run of glyphs with `text`.
fn run_look(items: &[Item], text: &str) -> (vale_render::Color, f32) {
    (items.iter())
        .find_map(|i| match i {
            Item::Glyphs(run) if run.text == text => Some((run.color, run.font_size)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no run {text}"))
}

/// The color and the width of each stroke of `items`.
fn key_strokes(items: &[Item]) -> Vec<(vale_render::Color, f64)> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Stroke { color, width, .. } => Some((*color, *width)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_second_unit_and_box_names_use_their_own_styles() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let feet = SecondUnit {
        name: "feet".to_string(),
        factor: 3.28084,
        offset: 0.0,
    };
    let blue = FurnitureText {
        color: [0, 0, 200, 255],
        ..FurnitureText::sized(12.0)
    };
    let key = ColorKeyItem {
        second_unit: Some(feet),
        second_unit_style: blue.clone(),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    let (value_color, value_size) = run_look(&items, "200");
    let (feet_color, feet_size) = run_look(&items, "656");
    assert_eq!(feet_color, vale_render::Color::rgb(0, 0, 200));
    assert_ne!(value_color, feet_color);
    assert!(
        (feet_size / value_size - 1.5).abs() < 1e-3,
        "{feet_size} {value_size}"
    );

    let (mut doc, mut pipeline, frame, layer) =
        key_doc("", &[0.0, 10.0, 20.0, 30.0], &["1-A", "1", "2-a"]);
    let key = ColorKeyItem {
        layout: KeyLayout::List,
        names_in_boxes: true,
        box_text_style: blue,
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    let (name_color, name_size) = run_look(&items, "1-A");
    let (label_color, label_size) = run_look(&items, "Under 10");
    assert_eq!(name_color, vale_render::Color::rgb(0, 0, 200));
    assert_ne!(label_color, name_color);
    assert!((name_size / label_size - 1.5).abs() < 1e-3);
    // The box is wide enough for the larger name.
    let boxes = fill_boxes(&items);
    let inside = run_x(&items, "1-A")[0];
    assert!(boxes[2].x0 < inside, "{inside} {boxes:?}");
}

#[test]
fn the_border_takes_its_color_and_width() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let thin = key_items(&mut doc, &mut pipeline, strip(frame, layer));
    let thin_width = key_strokes(&thin)[0].1;
    let key = ColorKeyItem {
        border_color: [255, 255, 255, 255],
        border_width: 1.2,
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    let strokes = key_strokes(&items);
    assert_eq!(strokes.len(), fill_boxes(&items).len());
    for (color, width) in strokes {
        assert_eq!(color, vale_render::Color::rgb(255, 255, 255));
        assert!(
            (width / thin_width - 4.0).abs() < 1e-6,
            "{width} {thin_width}"
        );
    }
}

#[test]
fn a_border_width_of_zero_draws_no_lines() {
    for layout in KeyLayout::ALL {
        let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
        let key = ColorKeyItem {
            layout,
            border_width: 0.0,
            ..strip(frame, layer)
        };
        let items = key_items(&mut doc, &mut pipeline, key);
        assert!(key_strokes(&items).is_empty(), "{layout:?}");
        assert!(!fill_boxes(&items).is_empty(), "{layout:?}");
    }
}

#[test]
fn hidden_bands_are_skipped() {
    let hide = || {
        let mut bands = std::collections::BTreeMap::new();
        let hidden = LegendEntry {
            name: None,
            hidden: true,
        };
        bands.insert("1,000".to_string(), hidden);
        bands
    };
    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
    let key = ColorKeyItem {
        bands: hide(),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(texts(&items), ["250", "500", "2,000", "millimeters"]);
    assert_eq!(fill_boxes(&items).len(), 4);

    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
    let key = ColorKeyItem {
        layout: KeyLayout::List,
        bands: hide(),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    assert_eq!(
        texts(&items),
        ["Over 2,000", "500–1,000", "250–500", "Under 250"]
    );
}

#[test]
fn a_key_whose_frame_hides_its_layer_draws_nothing() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
    let map = doc.project.atlas.frame(frame).unwrap().map;
    assert!(doc.set_shown(map, layer, false));
    let id = add_item(
        &mut doc,
        [20.0, 20.0, 160.0, 70.0],
        Furniture::ColorKey(strip(frame, layer)),
    );
    assert!(furniture_items(&doc, &mut pipeline, id, Purpose::Export).is_empty());
    let screen = furniture_items(&doc, &mut pipeline, id, Purpose::Screen);
    assert!(matches!(screen[..], [Item::Stroke { .. }]));
    // A key with no layer draws nothing either.
    let key = ColorKeyItem {
        layer: None,
        ..strip(frame, layer)
    };
    assert!(key_items(&mut doc, &mut pipeline, key).is_empty());
}

#[test]
fn the_gradient_and_grey_keys_label_their_stops() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
    let raster = doc.project.layer(layer).unwrap().raster().unwrap();
    doc.project
        .set_raster_display(raster, RasterDisplay::Gradient);
    let items = key_items(&mut doc, &mut pipeline, strip(frame, layer));
    assert_eq!(
        texts(&items),
        ["250", "500", "1,000", "2,000", "millimeters"]
    );
    // Each box is a run of colors.
    let colors: std::collections::HashSet<_> = (items.iter())
        .filter_map(|i| match i {
            Item::Fill { color, .. } => Some(format!("{color:?}")),
            _ => None,
        })
        .collect();
    assert!(colors.len() > 20, "{}", colors.len());

    doc.project.set_raster_display(raster, RasterDisplay::Grey);
    let items = key_items(&mut doc, &mut pipeline, strip(frame, layer));
    assert_eq!(texts(&items), ["0", "4,000", "millimeters"]);
}

/// Saves a height strip, a precipitation list, and a list with codes to
/// `target/app/colorkeys.png` for a person to look at.
#[test]
#[ignore = "manual"]
fn manual_color_key_pictures() {
    if std::env::var_os("VALE_SCREENSHOTS").is_none() {
        return;
    }
    let hex = |h: u32| [(h >> 16) as u8, (h >> 8) as u8, h as u8];
    let height = [
        -6000.0, -4000.0, -3000.0, -2000.0, -1000.0, -200.0, -100.0, 0.0, 100.0, 200.0, 300.0,
        500.0, 700.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 6000.0,
    ];
    let height_colors = [
        0xd8e8e4, 0xcce4e0, 0xbfe0dc, 0xb0d8d6, 0x70b8c8, 0x66b0c4, 0x5aa8c0, 0x9fd0a8, 0xcfe2a8,
        0xe6e8b8, 0xf0e8a4, 0xecc8a8, 0xe4b4a0, 0xd8a098, 0xcc8088, 0xc06070, 0xb44858, 0xa83848,
    ];
    let rain = [0.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0];
    let rain_colors = [0xf2ecd0, 0xd8ccaa, 0xb8d0a0, 0x80b890, 0x4a7a5a];
    let codes = [0.0, 10.0, 20.0, 30.0, 40.0];
    let code_colors = [0xe07860, 0x9cc888, 0x68a868, 0x50905a];
    let make = |unit: &str, stops: &[f64], colors: &[u32], names: &[&str]| {
        let (mut doc, pipeline, frame, layer) = key_doc(unit, stops, names);
        let raster = doc.project.layer(layer).unwrap().raster().unwrap();
        doc.project.raster_mut(raster).unwrap().colors = colors.iter().map(|c| hex(*c)).collect();
        (doc, pipeline, frame, layer)
    };
    let feet = SecondUnit {
        name: "feet".to_string(),
        factor: 3.28084,
        offset: 0.0,
    };
    let mut items = Vec::new();
    let mut draw = |(mut doc, mut pipeline, frame, layer): (Document, Pipeline, FrameId, _),
                    rect: [f64; 4],
                    key: fn(FrameId, vale_store::LayerId) -> ColorKeyItem,
                    feet: Option<SecondUnit>| {
        let key = ColorKeyItem {
            second_unit: feet,
            ..key(frame, layer)
        };
        let id = add_item(&mut doc, rect, Furniture::ColorKey(key));
        items.extend(furniture_items(&doc, &mut pipeline, id, Purpose::Export));
    };
    draw(
        make("m", &height, &height_colors, &[]),
        [5.0, 5.0, 205.0, 40.0],
        strip,
        Some(feet.clone()),
    );
    draw(
        make("mm", &rain, &rain_colors, &[]),
        [5.0, 45.0, 95.0, 110.0],
        |frame, layer| ColorKeyItem {
            layout: KeyLayout::List,
            title: "Annual precipitation in mm".to_string(),
            ..strip(frame, layer)
        },
        None,
    );
    draw(
        make("", &codes, &code_colors, &["1-A", "1", "1-a", "2-a"]),
        [105.0, 45.0, 205.0, 110.0],
        |frame, layer| {
            let mut key = ColorKeyItem {
                layout: KeyLayout::List,
                names_in_boxes: true,
                title: "Equatorial climates".to_string(),
                ..strip(frame, layer)
            };
            for (band, text) in [
                ("1-A", "Dry (subequatorial)"),
                ("1", "Intermediate"),
                ("1-a", "Very wet"),
                ("2-a", "Wet"),
            ] {
                let entry = LegendEntry {
                    name: Some(text.to_string()),
                    hidden: false,
                };
                key.bands.insert(band.to_string(), entry);
            }
            key
        },
        None,
    );
    draw(
        make("m", &height, &height_colors, &[]),
        [5.0, 115.0, 205.0, 150.0],
        |frame, layer| ColorKeyItem {
            box_width: Some(7.0),
            box_height: Some(7.0),
            ..strip(frame, layer)
        },
        None,
    );
    draw(
        make("m", &height, &height_colors, &[]),
        [5.0, 155.0, 205.0, 195.0],
        |frame, layer| ColorKeyItem {
            border_color: [255, 255, 255, 255],
            border_width: 1.0,
            second_unit_style: FurnitureText {
                color: [40, 80, 160, 255],
                italic: true,
                ..FurnitureText::sized(7.0)
            },
            ..strip(frame, layer)
        },
        Some(feet.clone()),
    );
    let (w, h) = (210.0 * PT_PER_MM, 200.0 * PT_PER_MM);
    let mut list = vale_render::DisplayList::new(w, h);
    list.items.push(Item::Fill {
        path: kurbo::Rect::new(0.0, 0.0, w, h).to_path(0.1),
        color: Color::rgb(250, 246, 232),
        rule: FillRule::NonZero,
    });
    list.items.extend(items);
    let px = render_rgba(&list, 4.0).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/colorkeys.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_raw(px.width as u32, px.height as u32, px.data)
        .unwrap()
        .save(path)
        .unwrap();
}

#[test]
fn square_boxes_make_a_strip_of_squares() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let key = ColorKeyItem {
        box_width: Some(6.0),
        box_height: Some(6.0),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    let boxes = fill_boxes(&items);
    assert_eq!(boxes.len(), 6);
    for b in &boxes {
        assert!((b.width() - b.height()).abs() < 1e-6, "{b:?}");
        assert!((b.width() - boxes[0].width()).abs() < 1e-6, "{boxes:?}");
    }
    // The rows start at the same x.
    assert!((boxes[0].x0 - boxes[3].x0).abs() < 1e-6, "{boxes:?}");
}

#[test]
fn square_boxes_that_shrink_to_fit_stay_square() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let auto = fill_boxes(&key_items(&mut doc, &mut pipeline, strip(frame, layer)));
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let key = ColorKeyItem {
        box_width: Some(500.0),
        box_height: Some(500.0),
        ..strip(frame, layer)
    };
    let boxes = fill_boxes(&key_items(&mut doc, &mut pipeline, key));
    assert_eq!(boxes.len(), 6, "{boxes:?}");
    for (b, a) in boxes.iter().zip(&auto) {
        assert!(b.width() <= a.width() + 1e-6, "{boxes:?}");
        assert!((b.width() - boxes[0].width()).abs() < 1e-6, "{boxes:?}");
        assert!((b.width() - b.height()).abs() < 1e-6, "{b:?}");
    }
}

#[test]
fn a_fixed_box_width_shrinks_to_fit_the_key() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let auto = fill_boxes(&key_items(&mut doc, &mut pipeline, strip(frame, layer)));
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let key = ColorKeyItem {
        box_width: Some(500.0),
        ..strip(frame, layer)
    };
    let wide = fill_boxes(&key_items(&mut doc, &mut pipeline, key));
    assert_eq!(wide, auto);
    let (mut doc, mut pipeline, frame, layer) = key_doc("m", &HEIGHT_STOPS, &[]);
    let key = ColorKeyItem {
        box_width: Some(4.0),
        ..strip(frame, layer)
    };
    let narrow = fill_boxes(&key_items(&mut doc, &mut pipeline, key));
    assert!(narrow[0].width() < auto[0].width(), "{narrow:?}");
    // The rows start at the same x, which leaves room for the unit at the
    // open start of the sea row.
    assert!(narrow[0].x0 >= auto[0].x0 - 1e-6, "{narrow:?}");
    assert!((narrow[0].x0 - narrow[3].x0).abs() < 1e-6, "{narrow:?}");
}

#[test]
fn the_list_follows_the_box_height() {
    let (mut doc, mut pipeline, frame, layer) = key_doc("mm", &RAIN_STOPS, &[]);
    let key = ColorKeyItem {
        layout: KeyLayout::List,
        box_width: Some(9.0),
        box_height: Some(9.0),
        ..strip(frame, layer)
    };
    let items = key_items(&mut doc, &mut pipeline, key);
    // Five rows of 9 mm do not fit in the 50 mm of the key.
    let boxes = fill_boxes(&items);
    assert_eq!(boxes.len(), 4, "{boxes:?}");
    for b in &boxes {
        assert!((b.width() - b.height()).abs() < 1e-6, "{b:?}");
    }
    for w in boxes.windows(2) {
        assert!(w[1].y0 > w[0].y1 + 1.0, "{boxes:?}");
    }
    // Each text is at the middle of its box.
    for (b, text) in boxes.iter().zip(["Over 2,000", "1,000–2,000", "500–1,000"]) {
        let y = (items.iter())
            .find_map(|i| match i {
                Item::Glyphs(run) if run.text == text => Some(run.glyphs[0].1.translation().y),
                _ => None,
            })
            .unwrap();
        assert!(y > b.y0 && y < b.y1, "{text} {y} {b:?}");
    }
}
