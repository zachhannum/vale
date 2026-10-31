mod common;

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
    CoordFormat, EdgeOrientation, FrameId, FrameShape, Furniture, FurnitureId, FurnitureText,
    LegendEntry, LegendItem, NorthArrowItem, PageId, PageUnits, ScaleBarItem, ScaleBarKind,
    ScaleFit, ScalePosition, ScaleUnit, TextCase, TextItem, TickFrequency,
};
use vale_style::expr::{Filter, Property};
use vale_style::{
    Color, PointSymbolizer, Rule, ScatterSymbolizer, Stroke, StrokeKind, StrokeSymbolizer, Style,
    Symbolizer,
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
        .compose_frame(doc, page, frame, target, &mut f, Quality::Final, None)
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
        .compose_frame(doc, page, frame, target, &mut f, Quality::Final, None)
        .unwrap()
}

#[test]
fn each_projection_draws_a_graticule_in_a_frame() {
    const RED: [u8; 3] = [200, 10, 20];
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

fn border_items(list: &vale_render::DisplayList, color: [u8; 3], width: f64) -> Vec<&Item> {
    let color = vale_render::Color::rgb(color[0], color[1], color[2]);
    (list.items.iter())
        .filter(
            |i| matches!(i, Item::Stroke { color: c, width: w, .. } if *c == color && *w == width),
        )
        .collect()
}

#[test]
fn the_border_follows_each_ring_of_the_shape_and_stays_vector() {
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
    frame.border.color = [200, 10, 20];
    frame.border.width = 3.0;
    let render = |doc: &Document, pipeline: &mut Pipeline| {
        let page = doc.project.atlas.page(page).unwrap();
        let mut f = fonts().unwrap();
        pipeline
            .compose_page(doc, page, &mut f, Quality::Final)
            .unwrap()
    };
    let list = render(&doc, &mut pipeline);
    let borders = border_items(&list, [200, 10, 20], 3.0);
    assert_eq!(borders.len(), 1);
    let Item::Stroke { path, .. } = borders[0] else {
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

    // The border is on the edge of the notch.
    let pixmap = render_pixmap(&list, 1.0).unwrap();
    let w = usize::from(pixmap.width());
    let data = pixmap.data_as_u8_slice();
    let (x, y) = ((148.5 * PT_PER_MM) as usize, (120.0 * PT_PER_MM) as usize);
    let i = (y * w + x) * 4;
    assert_eq!(&data[i..i + 3], &[200, 10, 20]);

    doc.project.atlas.frame_mut(id).unwrap().border.on = false;
    let list = render(&doc, &mut pipeline);
    assert!(border_items(&list, [200, 10, 20], 3.0).is_empty());
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
    empty.entry_mut(id).unwrap().style.style = Style { rules: vec![] };
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
            .compose_frame(&doc, page, frame, target, &mut f, Quality::Final, None)
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
        .compose_frame(doc, page, &frame, target, &mut f, Quality::Final, None)
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
    // The outside labels come after the clip of the frame and its border.
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
    set.color = [200, 10, 20];
    set.opacity = 0.5;
    set.size = 11.0;
    set.halo = true;
    set.halo_color = [1, 2, 3];
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
            g.meridians.color = [140, 30, 30];
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
        rules: vec![fill("Forest", green), fill("Desert", tan), swamp],
    };
    doc.entry_mut(rivers).unwrap().style.style = Style {
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
        fill1: hatched(vale_store::FillKind::CrossHatch, Some([0xf2, 0xe6, 0xc8])),
        ..ScaleBarItem::default()
    };
    bars.push((cross, 3.5));
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
