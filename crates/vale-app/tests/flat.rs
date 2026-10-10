//! The flat view of the world workspace.

use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::globe::flat::FlatView;
use vale_app::globe::math::{V3, lonlat_to_dir};
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Tool, WorldView};
use vale_app::headless;
use vale_app::ui::pad::{RECENTER, RESET};
use vale_app::ui::{AppState, draw};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_terrain::{Mode, meters_to_level};

type App = Harness<'static, AppState>;

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state
}

/// One GPU test runs at a time, because the software renderer is slow.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A harness with the wgpu renderer, the flat view, and the brush tool. The
/// preview is greyscale, so higher ground is lighter.
fn flat_harness() -> App {
    let mut state = state();
    state.globe.set_face_size(256);
    state.globe.world_view = WorldView::Flat;
    state.globe.tool = Tool::Brush;
    state.globe.brush.size_points = 12.0;
    state.globe.preview.greyscale = true;
    state.globe.preview.graticule = false;
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (640.0, 480.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

/// Runs frames until the stroke is in the CPU heightmap.
fn settle(h: &mut App) {
    for _ in 0..400 {
        h.step();
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the stroke did not end");
}

fn mouse(h: &mut App, pos: Pos2, down: bool) {
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: down,
        modifiers: egui::Modifiers::NONE,
    });
}

/// The first touch is also the pointer. A touch with a force is a pen.
fn pen(h: &mut App, phase: egui::TouchPhase, pos: Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force: Some(0.5),
    });
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

/// A stroke of 80 points to the east, across the middle of the canvas.
fn stroke(h: &mut App, with_pen: bool) {
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    match with_pen {
        true => pen(h, egui::TouchPhase::Start, from),
        false => mouse(h, from, true),
    }
    h.step();
    for i in 1..=8 {
        let pos = from.lerp(to, i as f32 / 8.0);
        match with_pen {
            true => pen(h, egui::TouchPhase::Move, pos),
            false => h.event(egui::Event::PointerMoved(pos)),
        }
        h.step();
    }
    match with_pen {
        true => pen(h, egui::TouchPhase::End, to),
        false => mouse(h, to, false),
    }
    h.step();
    settle(h);
}

/// The frame without the pointer, cut to the canvas.
fn canvas_image(h: &mut App) -> image::RgbaImage {
    h.remove_cursor();
    h.run_steps(2);
    let rect = h.state().globe.rect;
    let (x, y) = (rect.min.x.ceil() as u32, rect.min.y.ceil() as u32);
    let (w, h_) = (rect.width().floor() as u32, rect.height().floor() as u32);
    let frame = h.render().unwrap();
    image::imageops::crop_imm(&frame, x, y, w - 1, h_ - 1).to_image()
}

/// The grey level of the canvas at a screen position.
fn grey(h: &App, image: &image::RgbaImage, pos: Pos2) -> u8 {
    let at = pos - h.state().globe.rect.min;
    image.get_pixel(at.x as u32, at.y as u32).0[0]
}

#[test]
fn one_click_switches_the_view_and_the_tool_stays() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(2);
    h.get_by_label("Brush").click();
    h.run_steps(2);
    h.get_by_label("Lower").click();
    h.run_steps(2);
    for view in [WorldView::Flat, WorldView::Globe] {
        h.get_by_label("Flat").click();
        h.run_steps(2);
        let globe = &h.state().globe;
        assert_eq!(globe.world_view, view);
        assert_eq!((globe.tool, globe.brush.mode), (Tool::Brush, Mode::Lower));
        // The brush panel stays open.
        assert!(h.query_all_by_label("Hardness").next().is_some());
    }
}

#[test]
fn each_mode_paints_in_the_flat_view_and_the_globe_shows_the_stroke() {
    let _gpu = one_gpu_test();
    for with_pen in [false, true] {
        let mut h = flat_harness();
        let middle = h.state().globe.rect.center();
        let under = h.state().globe.unproject(middle).unwrap();
        // The middle of the flat map is also the middle of this globe.
        h.state_mut().globe.view = GlobeView::centered(0.0, 0.0);
        h.state_mut().globe.world_view = WorldView::Globe;
        let globe_before = canvas_image(&mut h);
        assert_eq!(h.state().globe.unproject(middle), Some(under));
        h.state_mut().globe.world_view = WorldView::Flat;
        let flat_before = canvas_image(&mut h);

        let base = h.state().globe.map.base();
        let level = meters_to_level(3000.0);
        h.state_mut().globe.brush.flatten_level = Some(level);
        let paint = |h: &mut App, mode: Mode| {
            h.state_mut().globe.brush.mode = mode;
            stroke(h, with_pen);
            h.state().globe.map.sample(under)
        };
        let raised = paint(&mut h, Mode::Raise);
        assert!(raised > base, "pen {with_pen}: {raised}");

        // The stroke shows in the flat view, and on the globe at the same place.
        let flat_after = canvas_image(&mut h);
        h.state_mut().globe.world_view = WorldView::Globe;
        let globe_after = canvas_image(&mut h);
        h.state_mut().globe.world_view = WorldView::Flat;
        h.run_steps(2);
        let far = middle + Vec2::new(0.0, 100.0);
        for (before, after) in [(&flat_before, &flat_after), (&globe_before, &globe_after)] {
            assert!(grey(&h, after, middle) > grey(&h, before, middle) + 4);
            assert_eq!(grey(&h, after, far), grey(&h, before, far));
        }

        // The ground next to the stroke is lower, so the middle goes down.
        let smooth = paint(&mut h, Mode::Smooth);
        assert!(smooth < raised && smooth > base, "pen {with_pen}: {smooth}");
        let lowered = paint(&mut h, Mode::Lower);
        assert!(lowered < smooth, "pen {with_pen}: {lowered}");
        let flat = paint(&mut h, Mode::Flatten);
        assert!(flat > lowered && flat <= level, "pen {with_pen}: {flat}");
    }
}

#[test]
fn a_stroke_across_the_180_degree_meridian_continues_at_the_other_edge() {
    let _gpu = one_gpu_test();
    let mut h = flat_harness();
    let before = canvas_image(&mut h);
    // The meridian is at the middle of the canvas, and the stroke crosses it.
    h.state_mut().globe.flat = FlatView::new(spec(ProjectionKind::Equirectangular, 180.0, 0.0));
    h.run_steps(2);
    stroke(&mut h, false);
    let globe = &h.state().globe;
    let base = globe.map.base();
    for lon in [165.0, 175.0, 180.0, -175.0, -165.0] {
        let level = globe.map.sample(lonlat_to_dir(lon, 0.0));
        assert!(level > base, "{lon}: {level}");
    }

    // With the meridian at the edges of the map, the stroke is at the two edges.
    h.state_mut().globe.flat = FlatView::default();
    let after = canvas_image(&mut h);
    let rect = h.state().globe.rect;
    let y = rect.center().y;
    for x in [rect.left() + 12.0, rect.right() - 12.0] {
        let pos = Pos2::new(x, y);
        assert!(grey(&h, &after, pos) > grey(&h, &before, pos) + 4, "{x}");
    }
    let middle = rect.center();
    assert_eq!(grey(&h, &after, middle), grey(&h, &before, middle));
}

fn spec(kind: ProjectionKind, lon0: f64, lat0: f64) -> ProjectionSpec {
    ProjectionSpec { kind, lon0, lat0 }
}

/// Stamps one time at a place, and compares the ground that changed with the
/// brush outline. Returns the width and the height of the outline.
fn check_outline(h: &mut App, lon: f64, lat: f64) -> (f32, f32) {
    let what = format!("{:?} at {lon} {lat}", h.state().globe.flat.spec().kind);
    h.state_mut().globe.brush.hardness = 1.0;
    h.run_steps(1);
    let globe = &h.state().globe;
    let rect = globe.rect;
    let center: V3 = lonlat_to_dir(lon, lat);
    let pos = globe.flat.project(rect, center).unwrap();
    let radius = globe.brush.radius(globe.radius());
    mouse(h, pos, true);
    h.step();
    mouse(h, pos, false);
    h.step();
    settle(h);

    let globe = &h.state().globe;
    let (flat, base) = (&globe.flat, globe.map.base());
    let center = flat.unproject(rect, pos).unwrap();
    let lines = flat.outline(rect, center, radius);
    assert_eq!(lines.len(), 1, "{what}");
    for point in &lines[0] {
        let inside = flat.unproject(rect, pos + (*point - pos) * 0.85).unwrap();
        let outside = flat.unproject(rect, pos + (*point - pos) * 1.15).unwrap();
        assert!(globe.map.sample(inside) > base, "{what}: inside {point:?}");
        assert_eq!(globe.map.sample(outside), base, "{what}: outside {point:?}");
    }
    let bounds = egui::Rect::from_points(&lines[0]);
    (bounds.width(), bounds.height())
}

#[test]
fn each_projection_paints_at_the_place_under_the_pen() {
    let _gpu = one_gpu_test();
    for kind in ProjectionKind::ALL {
        let mut h = flat_harness();
        h.state_mut().globe.flat = FlatView::new(spec(kind, 20.0, 30.0));
        let before = canvas_image(&mut h);
        check_outline(&mut h, 40.0, 45.0);
        // The picture shows the stamp at the same place.
        let after = canvas_image(&mut h);
        let globe = &h.state().globe;
        let at = |lon, lat| {
            globe
                .flat
                .project(globe.rect, lonlat_to_dir(lon, lat))
                .unwrap()
        };
        let (stamp, far) = (at(40.0, 45.0), at(0.0, 10.0));
        let (was, is) = (grey(&h, &before, stamp), grey(&h, &after, stamp));
        assert!(is > was, "{kind:?}: {was} and {is}");
        assert_eq!(grey(&h, &after, far), grey(&h, &before, far), "{kind:?}");
    }
}

#[test]
fn the_tool_bar_picks_the_projection_of_the_flat_view() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(2);
    assert!(h.query_by_role(egui::accesskit::Role::ComboBox).is_none());
    h.get_by_label("Flat").click();
    h.run_steps(2);
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(2);
    h.get_by_label("Orthographic").click();
    h.run_steps(2);
    assert_eq!(
        h.state().globe.flat.spec().kind,
        ProjectionKind::Orthographic
    );
    h.state_mut().globe.flat.look_at(50.0, 40.0);
    h.get_by_label(RECENTER).click();
    h.run_steps(2);
    let spec = h.state().globe.flat.spec();
    assert!(
        (spec.lon0 - 50.0).abs() < 1e-6 && (spec.lat0 - 40.0).abs() < 1e-6,
        "{spec:?}"
    );
    h.get_by_label(RESET).click();
    h.run_steps(2);
    let spec = h.state().globe.flat.spec();
    assert_eq!(
        (spec.kind, spec.lon0, spec.lat0),
        (ProjectionKind::Orthographic, 0.0, 0.0)
    );
}

#[test]
fn the_brush_outline_matches_the_ground_that_the_stamp_covers() {
    let _gpu = one_gpu_test();
    let (w, h) = check_outline(&mut flat_harness(), 30.0, 0.0);
    assert!((w - 24.0).abs() < 0.5 && (h - 24.0).abs() < 0.5, "{w} {h}");
    // Near a pole the same stamp covers more longitude, so the outline is wide.
    let (w, h) = check_outline(&mut flat_harness(), 30.0, 70.0);
    assert!(w > 2.5 * h && (h - 24.0).abs() < 0.5, "{w} {h}");
}
