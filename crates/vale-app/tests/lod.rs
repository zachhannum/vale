//! The views draw the levels of detail of the vector layers.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, Pos2, Rect};
use kurbo::Point;
use vale_app::atlas::{FrameTarget, PT_PER_MM, new_frame};
use vale_app::document::Document;
use vale_app::globe::math::{Mat3, lonlat_to_dir};
use vale_app::globe::select::FeatureRef;
use vale_app::globe::snap::{LayerTargets, SnapSettings, Targets};
use vale_app::globe::view::GlobeView;
use vale_app::lod;
use vale_app::pipeline::{FrameDetail, Pipeline, Quality, fonts};
use vale_app::ui::AppState;
use vale_app::ui::globe::vector_layer;
use vale_render::Item;
use vale_store::{FrameId, Geometry, LayerId};
use vale_style::expr::Filter;

const RECT: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 400.0));

/// A GeoJSON layer of `cols` × `rows` zones from 170° W to 170° E and from
/// 80° S to 80° N. Each edge of a zone has `steps` points, and neighbor
/// zones share their edges point for point. Each point is up to 0.003° off
/// the straight edge.
fn mosaic(cols: usize, rows: usize, steps: usize) -> String {
    let (nx, ny) = ((cols * steps) as f64, (rows * steps) as f64);
    let point = |gx: usize, gy: usize| {
        let wiggle = 0.003 * ((gx as f64) * 1.7 + (gy as f64) * 2.3).sin();
        let lon = -170.0 + 340.0 * gx as f64 / nx + wiggle;
        let lat = -80.0 + 160.0 * gy as f64 / ny + wiggle;
        format!("[{lon:.6}, {lat:.6}]")
    };
    let mut features = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let (x, y) = (c * steps, r * steps);
            let mut ring = Vec::new();
            ring.extend((0..steps).map(|t| point(x + t, y)));
            ring.extend((0..steps).map(|t| point(x + steps, y + t)));
            ring.extend((0..steps).map(|t| point(x + steps - t, y + steps)));
            ring.extend((0..=steps).map(|t| point(x, y + steps - t)));
            features.push(format!(
                r#"{{"type": "Feature", "properties": {{"zone": {}}}, "geometry": {{"type": "Polygon", "coordinates": [[{}]]}}}}"#,
                r * cols + c,
                ring.join(", ")
            ));
        }
    }
    format!(
        r#"{{"type": "FeatureCollection", "features": [{}]}}"#,
        features.join(", ")
    )
}

fn import(doc: &mut Document, name: &str, text: &str) -> LayerId {
    let imported = vale_import::geojson::read_str(name, text).unwrap();
    doc.add_imported(imported, None)[0]
}

/// A pipeline that builds the levels of a small layer at once.
fn pipeline() -> Pipeline {
    let mut pipeline = Pipeline::new();
    small(&mut pipeline);
    pipeline
}

fn small(pipeline: &mut Pipeline) {
    let lod = pipeline.lod_mut();
    lod.min_points = 1000;
    lod.quiet = Duration::ZERO;
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

/// The number of drawn positions.
fn count(shapes: &[egui::Shape]) -> usize {
    (shapes.iter())
        .map(|shape| match shape {
            egui::Shape::Mesh(mesh) => mesh.vertices.len(),
            egui::Shape::Path(path) => path.points.len(),
            _ => 0,
        })
        .sum()
}

fn points(doc: &Document, id: LayerId) -> usize {
    let layer = doc.layer(id).unwrap();
    layer
        .features
        .iter()
        .map(|f| lod::points(&f.geometry))
        .sum()
}

/// The globe draws the whole world, with a pixel of about 35 km.
fn world() -> GlobeView {
    GlobeView::centered(0.0, 0.0)
}

#[test]
fn a_zoomed_out_globe_draws_a_coarse_level() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    let full = draw(&doc, &mut pipeline, &world(), id);
    assert_eq!(pipeline.lod().builds(id), 1);
    pipeline.poll_lod(true);
    assert_eq!(pipeline.lod().ready(id), [0, 1, 2, 3]);
    let coarse = draw(&doc, &mut pipeline, &world(), id);
    assert!(
        count(&coarse) * 2 < count(&full),
        "{} {}",
        count(&coarse),
        count(&full)
    );

    // Near the ground, the globe draws the full geometry again.
    let mut near = world();
    near.zoom = 2000.0;
    let mut alone = Pipeline::new();
    assert_eq!(
        count(&draw(&doc, &mut pipeline, &near, id)),
        count(&draw(&doc, &mut alone, &near, id))
    );
}

#[test]
fn a_small_layer_draws_its_full_geometry() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = Pipeline::new();
    assert!(points(&doc, id) < lod::MIN_POINTS);
    draw(&doc, &mut pipeline, &world(), id);
    pipeline.poll_lod(true);
    assert_eq!(pipeline.lod().builds(id), 0);
}

#[test]
fn the_view_draws_the_full_geometry_while_the_levels_build() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    pipeline.lod_mut().held = true;
    let full = draw(&doc, &mut pipeline, &world(), id);
    assert!(pipeline.poll_lod(true));
    let held = draw(&doc, &mut pipeline, &world(), id);
    assert_eq!(count(&held), count(&full));
    let work = pipeline.globe_shapes().work(id);

    // The level arrives, and the globe makes its shapes again by itself.
    pipeline.lod_mut().held = false;
    assert!(!pipeline.poll_lod(true));
    let coarse = draw(&doc, &mut pipeline, &world(), id);
    assert_eq!(
        pipeline.globe_shapes().work(id).directions,
        work.directions + 1
    );
    assert!(count(&coarse) < count(&full));
}

#[test]
fn an_edit_builds_the_levels_of_only_that_layer() {
    let mut doc = Document::empty();
    let a = import(&mut doc, "a", &mosaic(12, 6, 40));
    let b = import(&mut doc, "b", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    draw(&doc, &mut pipeline, &world(), a);
    draw(&doc, &mut pipeline, &world(), b);
    pipeline.poll_lod(true);

    let uuid = doc.layer(a).unwrap().features[0].uuid();
    let square = vec![vec![vec![
        [0.0, 0.0],
        [5.0, 0.0],
        [5.0, 5.0],
        [0.0, 5.0],
        [0.0, 0.0],
    ]]];
    assert!(
        doc.project
            .set_geometry(a, uuid, Geometry::Polygons(square))
    );
    pipeline.forget(a);
    draw(&doc, &mut pipeline, &world(), a);
    draw(&doc, &mut pipeline, &world(), b);
    pipeline.poll_lod(true);
    assert_eq!(pipeline.lod().builds(a), 2);
    assert_eq!(pipeline.lod().builds(b), 1);
}

#[test]
fn an_edit_starts_no_build_until_the_layer_is_quiet() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    draw(&doc, &mut pipeline, &world(), id);
    pipeline.poll_lod(true);
    pipeline.lod_mut().quiet = Duration::from_secs(3600);
    pipeline.forget(id);
    draw(&doc, &mut pipeline, &world(), id);
    // The layer waits, and the app draws again later.
    assert!(pipeline.poll_lod(false));
    assert_eq!(pipeline.lod().builds(id), 1);
}

/// The app with two mosaic layers and a project file.
fn app(dir: &std::path::Path) -> (AppState, LayerId, LayerId) {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    small(&mut state.pipeline);
    let a = import(&mut state.doc, "a", &mosaic(12, 6, 40));
    let b = import(&mut state.doc, "b", &mosaic(12, 6, 40));
    let session = vale_app::session::Session::open(&dir.join("w.gpkg"), &mut state.doc).unwrap();
    state.session = Some(session);
    state.save_waiting().unwrap();
    (state, a, b)
}

fn draw_both(state: &mut AppState, a: LayerId, b: LayerId) {
    draw(&state.doc, &mut state.pipeline, &world(), a);
    draw(&state.doc, &mut state.pipeline, &world(), b);
    state.pipeline.poll_lod(true);
}

#[test]
fn an_undo_builds_the_levels_of_only_the_changed_layer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, a, b) = app(dir.path());
    draw_both(&mut state, a, b);
    let uuid = state.doc.layer(a).unwrap().features[0].uuid();
    state.set_feature_selection(vec![FeatureRef { layer: a, uuid }]);
    state.delete_selection();
    state.save_waiting().unwrap();
    draw_both(&mut state, a, b);
    assert_eq!(
        (
            state.pipeline.lod().builds(a),
            state.pipeline.lod().builds(b)
        ),
        (2, 1)
    );

    state.undo();
    draw_both(&mut state, a, b);
    assert_eq!(
        (
            state.pipeline.lod().builds(a),
            state.pipeline.lod().builds(b)
        ),
        (3, 1)
    );
    assert_eq!(state.doc.layer(a).unwrap().features.len(), 72);
    // The level of layer a has the feature again.
    let level = state
        .pipeline
        .lod_mut()
        .level(state.doc.layer(a).unwrap(), 0.01);
    let full = points(&state.doc, a);
    assert!(level.unwrap().points(state.doc.layer(a).unwrap()) < full);
}

/// A vertex of feature 0 that the full geometry has and the coarsest
/// level leaves out.
fn dropped_vertex(doc: &Document, pipeline: &mut Pipeline, id: LayerId) -> [f64; 2] {
    let layer = doc.layer(id).unwrap();
    draw(doc, pipeline, &world(), id);
    pipeline.poll_lod(true);
    let level = pipeline.lod_mut().level(layer, 1.0).unwrap();
    assert_eq!(level.index, 3);
    let full = &layer.features[0].geometry;
    let (Geometry::Polygons(full_rings), Some(Geometry::Polygons(rings))) =
        (full, lod::geometry(Some(&level), 0, full))
    else {
        panic!("polygons");
    };
    let kept: Vec<_> = rings.iter().flatten().flatten().collect();
    *full_rings[0][0]
        .iter()
        .find(|p| !kept.contains(p))
        .expect("a dropped vertex")
}

#[test]
fn snapping_finds_a_vertex_of_the_full_geometry() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    let vertex = dropped_vertex(&doc, &mut pipeline, id);
    let targets = Targets::new(
        vec![Arc::new(LayerTargets::build(id, doc.layer(id).unwrap()))],
        Vec::new(),
    );
    // A scale of 1,000 points per radian at the vertex.
    let at = lonlat_to_dir(vertex[0] + 0.0001, vertex[1]);
    let snap = targets.snap(at, 1000.0, &SnapSettings::default(), None);
    assert_eq!(snap.and_then(|s| s.exact), Some(vertex));
}

#[test]
fn the_table_and_pick_use_the_full_geometry() {
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    let vertex = dropped_vertex(&doc, &mut pipeline, id);
    let layer = doc.layer(id).unwrap();
    let rows = vale_app::table::build(
        layer,
        doc.project.world.radius_km,
        None,
        &Filter::new(""),
        None,
    );
    assert_eq!(rows.len(), 72);
    assert_eq!(rows.len(), layer.features.len());

    // Pick finds the zone at the dropped vertex, just inside the zone.
    let size = (800.0, 400.0);
    let inside = [vertex[0] + 0.5, vertex[1] + 0.5];
    let at = pipeline
        .lonlat_to_page(&doc, size, inside)
        .unwrap()
        .unwrap();
    let picked = pipeline.pick(&doc, size, at).unwrap();
    assert!(picked.is_some());
}

/// A page and a frame of map 0 that shows the whole world.
fn frame_on(doc: &mut Document, pipeline: &mut Pipeline) -> FrameId {
    let atlas = &doc.project.atlas;
    let page = atlas.pages[0].id;
    let projection = pipeline.projection(doc, atlas.maps[0].id).unwrap();
    let frame = new_frame(atlas.page(page).unwrap(), &atlas.maps[0], projection);
    doc.project.atlas.add_frame(page, frame).unwrap()
}

/// The number of path elements of the fills and the strokes of a list.
fn elements(items: &[Item]) -> usize {
    (items.iter())
        .map(|item| match item {
            Item::Fill { path, .. } | Item::Stroke { path, .. } => path.elements().len(),
            _ => 0,
        })
        .sum()
}

#[test]
fn a_frame_on_the_screen_draws_a_level_and_an_export_draws_the_full_geometry() {
    let mut doc = Document::empty();
    doc.frame.labels = false;
    let id = import(&mut doc, "zones", &mosaic(12, 6, 40));
    let mut pipeline = pipeline();
    let frame_id = frame_on(&mut doc, &mut pipeline);
    let atlas = &doc.project.atlas;
    let page = atlas.page(atlas.page_of_frame(frame_id).unwrap()).unwrap();
    let frame = atlas.frame(frame_id).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let part = kurbo::Rect::new(
        0.0,
        0.0,
        page.width_mm * PT_PER_MM,
        page.height_mm * PT_PER_MM,
    );
    let mut f = fonts().unwrap();
    let mut compose = |pipeline: &mut Pipeline, detail| {
        let composed = pipeline
            .compose_frame_part(&doc, frame, target, part, &mut f, Quality::Final, detail)
            .unwrap();
        elements(&composed.list.items)
    };
    let full = compose(&mut pipeline, FrameDetail::Full);
    // The first compose on the screen starts the build.
    assert_eq!(compose(&mut pipeline, FrameDetail::Screen(1.0)), full);
    assert_eq!(pipeline.lod().builds(id), 1);
    pipeline.poll_lod(true);
    let screen = compose(&mut pipeline, FrameDetail::Screen(1.0));
    assert!(screen < full, "{screen} {full}");
    assert_eq!(compose(&mut pipeline, FrameDetail::Full), full);
    let whole = pipeline
        .compose_frame(&doc, page, frame, target, &mut f, Quality::Final)
        .unwrap();
    assert_eq!(elements(&whole.list.items), full);
}

/// Prints the frame times of the globe with a layer of about 700,000
/// points, with and without the levels of detail.
#[test]
#[ignore = "manual"]
fn manual_lod_frame_time() {
    use std::time::Instant;
    let text = mosaic(72, 36, 67);
    let mut doc = Document::empty();
    let id = import(&mut doc, "zones", &text);
    println!("points: {}", points(&doc, id));
    let start = Instant::now();
    let mut levels = Pipeline::new();
    draw(&doc, &mut levels, &world(), id);
    levels.poll_lod(true);
    println!("build: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    let mut full = Pipeline::new();
    full.lod_mut().min_points = usize::MAX;
    let turn = Mat3::axis_angle([0.0, 1.0, 0.0], 0.01);
    let frames = 20;
    for zoom in [1.0, 4.0, 16.0, 64.0, 256.0] {
        let mut times = Vec::new();
        for pipeline in [&mut levels, &mut full] {
            let mut view = GlobeView::centered(10.0, 45.0);
            view.zoom = zoom;
            draw(&doc, pipeline, &view, id);
            let start = Instant::now();
            for _ in 0..frames {
                view.rot = view.rot.mul(&turn);
                draw(&doc, pipeline, &view, id);
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames));
        }
        println!(
            "zoom {zoom}: {:.1} ms with levels, {:.1} ms with full geometry",
            times[0], times[1]
        );
    }
}

/// The snap targets of the visible layer `id`.
fn layer_targets(state: &AppState, id: LayerId) -> std::sync::Arc<LayerTargets> {
    let targets = state.globe.vector_targets().unwrap();
    let layer = targets.layers().iter().find(|t| t.layer() == id);
    layer.unwrap().clone()
}

#[test]
fn a_show_keeps_the_snap_targets_of_the_layer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, a, b) = app(dir.path());
    state.refresh_targets();
    let before = layer_targets(&state, a);
    state.set_layer_visible(a, false);
    state.refresh_targets();
    state.set_layer_visible(a, true);
    state.refresh_targets();
    assert!(std::sync::Arc::ptr_eq(&before, &layer_targets(&state, a)));

    // An edit builds the targets of only that layer again.
    let (kept, edited) = (layer_targets(&state, a), layer_targets(&state, b));
    let uuid = state.doc.layer(b).unwrap().features[0].uuid();
    state.set_feature_selection(vec![FeatureRef { layer: b, uuid }]);
    state.delete_selection();
    state.refresh_targets();
    assert!(std::sync::Arc::ptr_eq(&kept, &layer_targets(&state, a)));
    assert!(!std::sync::Arc::ptr_eq(&edited, &layer_targets(&state, b)));
}
