use std::path::Path;

use kurbo::Point;
use vale_app::atlas::{FrameTarget, PT_PER_MM, auto_graticule_interval, new_frame};
use vale_app::document::Document;
use vale_app::headless::{export, map_png, probe};
use vale_app::pipeline::{PAPER, Pipeline, Quality, fonts};
use vale_render::{FillRule, Item, render_pixmap, render_rgba};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{FrameId, FrameShape, PageId, PageUnits};
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
