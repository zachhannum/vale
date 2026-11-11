//! The globe keeps the shapes of the vector layers between frames.

use eframe::egui::{self, Pos2, Rect};
use vale_app::document::Document;
use vale_app::globe::math::Mat3;
use vale_app::globe::select::FeatureRef;
use vale_app::globe::view::GlobeView;
use vale_app::pipeline::Pipeline;
use vale_app::ui::globe::vector_layer;
use vale_app::ui::{Action, AppState};
use vale_store::{Geometry, LayerId, World};
use vale_style::Color;

const RECT: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 400.0));

/// A polygon layer with a square at 0°, 0° and a square on the far side of
/// the globe at 180°, 0°.
const SHAPES: &str = r#"{"type": "FeatureCollection", "features": [
  {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
    "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}},
  {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
    "coordinates": [[[170, -5], [-170, -5], [-170, 5], [170, 5], [170, -5]]]}}
]}"#;

fn import(doc: &mut Document, name: &str, text: &str) -> LayerId {
    let imported = vale_import::geojson::read_str(name, text).unwrap();
    doc.add_imported(imported, None)[0]
}

/// The shapes that the globe draws for layer `id` in `view`.
fn draw(
    doc: &Document,
    pipeline: &mut Pipeline,
    view: &GlobeView,
    id: LayerId,
) -> Vec<egui::Shape> {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.ctx().layer_painter(egui::LayerId::background());
        vector_layer(&painter, None, RECT, doc, view, id, pipeline);
    });
    output.textures_delta.clear();
    output.shapes.into_iter().map(|s| s.shape).collect()
}

/// The positions of the drawn meshes and paths.
fn positions(shapes: &[egui::Shape]) -> Vec<Pos2> {
    let mut out = Vec::new();
    for shape in shapes {
        match shape {
            egui::Shape::Mesh(mesh) => out.extend(mesh.vertices.iter().map(|v| v.pos)),
            egui::Shape::Path(path) => out.extend(path.points.iter().copied()),
            egui::Shape::Circle(c) => out.push(c.center),
            _ => {}
        }
    }
    out
}

fn mesh_colors(shapes: &[egui::Shape]) -> Vec<egui::Color32> {
    (shapes.iter())
        .filter_map(|s| match s {
            egui::Shape::Mesh(m) => m.vertices.first().map(|v| v.color),
            _ => None,
        })
        .collect()
}

fn same(a: &[egui::Shape], b: &[egui::Shape]) -> bool {
    a.len() == b.len() && positions(a) == positions(b)
}

#[test]
fn a_frame_with_no_change_does_no_geometry_work() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "shapes", SHAPES);
    let mut pipeline = Pipeline::new();
    let view = GlobeView::centered(0.0, 0.0);
    let first = draw(&doc, &mut pipeline, &view, id);
    assert!(!first.is_empty());
    let work = pipeline.globe_shapes().work(id);
    assert_eq!((work.directions, work.views, work.draws), (1, 1, 1));
    let second = draw(&doc, &mut pipeline, &view, id);
    assert_eq!(pipeline.globe_shapes().work(id), work);
    assert!(same(&first, &second));
}

#[test]
fn a_turn_of_the_globe_keeps_the_directions() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "shapes", SHAPES);
    let mut pipeline = Pipeline::new();
    let mut view = GlobeView::centered(0.0, 0.0);
    let before = draw(&doc, &mut pipeline, &view, id);
    view.rot = view.rot.mul(&Mat3::axis_angle([0.0, 1.0, 0.0], 0.1));
    let after = draw(&doc, &mut pipeline, &view, id);
    let work = pipeline.globe_shapes().work(id);
    assert_eq!((work.directions, work.views, work.draws), (1, 2, 2));
    assert_ne!(positions(&before), positions(&after));
}

#[test]
fn a_feature_on_the_far_side_or_off_the_screen_is_not_cut() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "shapes", SHAPES);
    let mut pipeline = Pipeline::new();
    let view = GlobeView::centered(0.0, 0.0);
    draw(&doc, &mut pipeline, &view, id);
    // The square at 180° is on the far side.
    assert_eq!(pipeline.globe_shapes().work(id).clipped, 1);

    // At zoom 20, the screen shows about 3° around 30°, 30°, and no
    // feature is on it.
    let mut pipeline = Pipeline::new();
    let mut view = GlobeView::centered(30.0, 30.0);
    view.zoom = 20.0;
    let shapes = draw(&doc, &mut pipeline, &view, id);
    assert_eq!(pipeline.globe_shapes().work(id).clipped, 0);
    assert!(positions(&shapes).is_empty());
}

#[test]
fn a_change_to_one_layer_computes_only_that_layer_again() {
    let mut doc = Document::empty();
    let a = import(&mut doc, "a", SHAPES);
    let b = import(&mut doc, "b", SHAPES);
    let mut pipeline = Pipeline::new();
    let view = GlobeView::centered(0.0, 0.0);
    let a_before = draw(&doc, &mut pipeline, &view, a);
    let b_before = draw(&doc, &mut pipeline, &view, b);
    let b_work = pipeline.globe_shapes().work(b);

    // The edit moves the square of layer a to the east, as the app does it.
    let uuid = doc.layer(a).unwrap().features[0].uuid();
    let square = vec![vec![vec![
        [20.0, -10.0],
        [40.0, -10.0],
        [40.0, 10.0],
        [20.0, 10.0],
        [20.0, -10.0],
    ]]];
    assert!(
        doc.project
            .set_geometry(a, uuid, Geometry::Polygons(square))
    );
    pipeline.forget(a);

    let a_after = draw(&doc, &mut pipeline, &view, a);
    let b_after = draw(&doc, &mut pipeline, &view, b);
    assert_eq!(pipeline.globe_shapes().work(a).directions, 2);
    assert_eq!(pipeline.globe_shapes().work(b), b_work);
    assert!(same(&b_before, &b_after));
    let right = |shapes: &[egui::Shape]| positions(shapes).iter().map(|p| p.x).fold(0.0, f32::max);
    assert!(right(&a_after) > right(&a_before) + 20.0);
}

#[test]
fn a_style_change_draws_again_with_the_kept_parts() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "shapes", SHAPES);
    let mut pipeline = Pipeline::new();
    let view = GlobeView::centered(0.0, 0.0);
    draw(&doc, &mut pipeline, &view, id);
    let before = pipeline.globe_shapes().work(id);
    let red = Color::rgb(255, 0, 0);
    let entry = doc.entry_mut(id).unwrap();
    entry.style.style.rules[0].set_fill(Some(red));
    let shapes = draw(&doc, &mut pipeline, &view, id);
    let after = pipeline.globe_shapes().work(id);
    assert_eq!(after.draws, before.draws + 1);
    assert_eq!(
        (after.directions, after.clipped),
        (before.directions, before.clipped)
    );
    assert!(mesh_colors(&shapes).contains(&egui::Color32::from_rgb(255, 0, 0)));
}

/// The app with one polygon layer and a project file.
fn app(dir: &std::path::Path) -> (AppState, LayerId) {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    let id = import(&mut state.doc, "shapes", SHAPES);
    let session = vale_app::session::Session::open(&dir.join("w.gpkg"), &mut state.doc).unwrap();
    state.session = Some(session);
    state.save_waiting().unwrap();
    (state, id)
}

fn app_draw(state: &mut AppState, id: LayerId) -> Vec<egui::Shape> {
    let view = GlobeView::centered(0.0, 0.0);
    draw(&state.doc, &mut state.pipeline, &view, id)
}

#[test]
fn an_undo_and_a_redo_show_the_correct_layers_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, id) = app(dir.path());
    let full = app_draw(&mut state, id);
    let uuid = state.doc.layer(id).unwrap().features[0].uuid();
    state.set_feature_selection(vec![FeatureRef { layer: id, uuid }]);
    state.delete_selection();
    state.save_waiting().unwrap();
    let deleted = app_draw(&mut state, id);
    assert!(positions(&deleted).len() < positions(&full).len());
    state.undo();
    assert!(same(&app_draw(&mut state, id), &full));
    state.redo();
    assert!(same(&app_draw(&mut state, id), &deleted));
}

#[test]
fn a_new_world_shows_its_own_layers_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, id) = app(dir.path());
    assert!(!app_draw(&mut state, id).is_empty());
    state.run_action(Action::NewWorld(World::default()));
    assert!(app_draw(&mut state, id).is_empty());
    // A layer of the new world can have the ID of a layer of the old one.
    let line = r#"{"type": "Feature", "properties": {}, "geometry":
        {"type": "LineString", "coordinates": [[0, 0], [3, 3]]}}"#;
    let new = import(&mut state.doc, "line", line);
    let shapes = app_draw(&mut state, new);
    let drawn = positions(&shapes);
    assert!(!drawn.is_empty());
    let center = RECT.center();
    let spread = drawn.iter().map(|p| p.distance(center)).fold(0.0, f32::max);
    // The line of 4° reaches about 13 points from the center, and the old
    // square more than 30.
    assert!(spread < 30.0, "{spread}");
}

/// A square of 20° at 0°, 0°, and a square of 1° at 30°, 0°: about
/// 1,900,000 and 4,800 square miles.
const LARGE_AND_SMALL: &str = r#"{"type": "FeatureCollection", "features": [
  {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
    "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}},
  {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
    "coordinates": [[[30, 0], [31, 0], [31, 1], [30, 1], [30, 0]]]}}
]}"#;

fn set_filter(doc: &mut Document, id: LayerId, filter: &str) {
    let rule = &mut doc.entry_mut(id).unwrap().style.style.rules[0];
    rule.filter = vale_style::expr::Filter::new(filter);
}

/// The smallest and the largest x of the drawn shapes.
fn x_range(shapes: &[egui::Shape]) -> (f32, f32) {
    let xs = positions(shapes).into_iter().map(|p| p.x);
    xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)))
}

/// The shapes of the large square with filter `large`, and of the small
/// square with filter `small`, as `draw` gives them. The large square is
/// west of the small one and wider.
fn assert_large_and_small(draw: impl Fn(&Document, &mut Pipeline, LayerId) -> Vec<egui::Shape>) {
    let mut doc = Document::empty();
    let id = import(&mut doc, "areas", LARGE_AND_SMALL);
    set_filter(&mut doc, id, "area('mi2') > 10000");
    let large = x_range(&draw(&doc, &mut Pipeline::new(), id));
    set_filter(&mut doc, id, "area('mi2') <= 10000");
    let small = x_range(&draw(&doc, &mut Pipeline::new(), id));
    assert!(
        large.0 < large.1 && small.0 < small.1,
        "{large:?} {small:?}"
    );
    assert!(large.1 < small.0, "{large:?} {small:?}");
    assert!(large.1 - large.0 > 5.0 * (small.1 - small.0));
}

#[test]
fn an_area_filter_picks_the_large_polygon_on_the_globe() {
    let view = GlobeView::centered(15.0, 0.0);
    assert_large_and_small(|doc, pipeline, id| draw(doc, pipeline, &view, id));
}

#[test]
fn an_area_filter_picks_the_large_polygon_in_the_flat_view() {
    let mut flat = vale_app::globe::flat::FlatView::default();
    flat.look_at(15.0, 0.0);
    flat.zoom = 2.0;
    assert_large_and_small(|doc, pipeline, id| {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            vale_app::ui::globe::flat_vector_layer(&painter, None, RECT, doc, &flat, id, pipeline);
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|s| s.shape).collect()
    });
}

#[test]
fn a_vertex_edit_and_an_undo_change_what_an_area_filter_picks() {
    use vale_app::globe::Tool;
    use vale_app::ui::LayerRef;
    // A square of 1° with a spike to 40°, 10°: about 26,000 square miles,
    // and 4,800 without the spike.
    let spike = r#"{"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
        "coordinates": [[[30, 0], [31, 0], [31, 1], [40, 10], [30, 1], [30, 0]]]}}"#;
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    let id = import(&mut state.doc, "spike", spike);
    set_filter(&mut state.doc, id, "area('mi2') > 10000");
    let path = dir.path().join("w.gpkg");
    state.session = Some(vale_app::session::Session::open(&path, &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    let view = GlobeView::centered(30.0, 0.0);
    let drawn = |state: &mut AppState| !draw(&state.doc, &mut state.pipeline, &view, id).is_empty();
    assert!(drawn(&mut state));

    state.select(Some(LayerRef::Vector(id)));
    state.globe.tool = Tool::Vertex;
    let uuid = state.doc.layer(id).unwrap().features[0].uuid();
    state.set_feature_selection(vec![FeatureRef { layer: id, uuid }]);
    state.vertex_selection = vec![[40.0, 10.0]];
    // The edit cleans the ring at the scale of the globe on the screen.
    state.globe.rect = RECT;
    state.delete_vertices();
    state.save_waiting().unwrap();
    let Geometry::Polygons(p) = &state.doc.layer(id).unwrap().features[0].geometry else {
        panic!("not a polygon");
    };
    assert_eq!(p[0][0].len(), 5);
    assert!(!drawn(&mut state));

    state.undo();
    assert!(drawn(&mut state));
}

/// Prints the frame times of the globe with a large polygon layer: the
/// first frame, a frame with no change, and the frames of a turn and of a
/// zoom. `VALE_LAND` names a GeoJSON file, for example the Natural Earth
/// 10m land layer. Without it, the layer is 2,000 circles of 200 points.
#[test]
#[ignore = "manual"]
fn manual_frame_times_of_a_large_layer() {
    use std::time::Instant;
    let text = match std::env::var("VALE_LAND") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => circles(2000, 200),
    };
    let mut doc = Document::empty();
    let id = import(&mut doc, "land", &text);
    let mut pipeline = Pipeline::new();
    let time = |pipeline: &mut Pipeline, view: &GlobeView| {
        let start = Instant::now();
        draw(&doc, pipeline, view, id);
        start.elapsed().as_secs_f64() * 1000.0
    };
    let mut view = GlobeView::centered(10.0, 45.0);
    println!("first frame: {:.1} ms", time(&mut pipeline, &view));
    println!("frame with no change: {:.1} ms", time(&mut pipeline, &view));
    let turn = Mat3::axis_angle([0.0, 1.0, 0.0], 0.01);
    let frames = 30;
    let mut total = 0.0;
    for _ in 0..frames {
        view.rot = view.rot.mul(&turn);
        total += time(&mut pipeline, &view);
    }
    println!("frame of a turn: {:.1} ms", total / f64::from(frames));
    for zoom in [4.0, 16.0, 64.0] {
        view.zoom = zoom;
        let mut total = 0.0;
        for _ in 0..frames {
            view.rot = view.rot.mul(&turn);
            total += time(&mut pipeline, &view);
        }
        println!(
            "frame of a turn at zoom {zoom}: {:.1} ms",
            total / f64::from(frames)
        );
    }
}

/// A GeoJSON polygon layer of `count` circles of `points` points each.
fn circles(count: usize, points: usize) -> String {
    let mut features = Vec::new();
    for i in 0..count {
        // A spiral of circle centers over the sphere.
        let t = (i as f64 + 0.5) / count as f64;
        let lat = (1.0 - 2.0 * t).asin().to_degrees();
        let lon = (i as f64 * 137.508) % 360.0 - 180.0;
        let ring: Vec<String> = (0..=points)
            .map(|k| {
                let a = k as f64 / points as f64 * std::f64::consts::TAU;
                let x = lon + 1.5 * a.cos() / lat.to_radians().cos().max(0.2);
                format!("[{:.5}, {:.5}]", x, lat + 1.5 * a.sin())
            })
            .collect();
        features.push(format!(
            r#"{{"type": "Feature", "properties": {{}}, "geometry": {{"type": "Polygon", "coordinates": [[{}]]}}}}"#,
            ring.join(", ")
        ));
    }
    format!(
        r#"{{"type": "FeatureCollection", "features": [{}]}}"#,
        features.join(", ")
    )
}
