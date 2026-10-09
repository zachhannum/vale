use std::path::Path;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kurbo::Point;
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::{Action, AppState, ExportFormat, Workspace, draw};
use vale_sphere::{ProjectionKind, ProjectionSpec};

const SIZE: (f64, f64) = (1280.0, 800.0);

fn state_with(doc: Document) -> AppState {
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.workspace = Workspace::Map;
    state
}

fn harness_with(doc: Document) -> Harness<'static, AppState> {
    Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state_with(doc))
}

fn harness() -> Harness<'static, AppState> {
    harness_with(Document::sample())
}

fn land_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/natural-earth/110m/ne_110m_land.geojson")
}

#[test]
fn first_frames_compose_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let s = h.state();
    assert!(s.composed.is_some());
    assert!(s.texture.is_some());
    assert!(
        s.canvas_size.0 > 650.0 && s.canvas_size.0 < 850.0,
        "{:?}",
        s.canvas_size
    );
    assert!(!s.dirty);
}

#[test]
fn graticule_checkbox_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().composed.as_ref().unwrap().list.items.len();
    h.get_by_label("Graticule").click();
    h.run_steps(3);
    let s = h.state();
    assert!(!s.doc.frame.graticule);
    assert_ne!(s.composed.as_ref().unwrap().list.items.len(), before);
}

#[test]
fn land_checkbox_hides_layer() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label("Land").click();
    h.run_steps(2);
    let doc = &h.state().doc;
    let land = doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Land")
        .unwrap()
        .id;
    assert!(!doc.entry(land).unwrap().visible);
}

#[test]
fn toolbar_buttons_push_actions() {
    let mut h = harness();
    h.run_steps(2);
    h.get_by_label("Open GeoJSON…").click();
    h.run_steps(1);
    assert!(h.state().actions.contains(&Action::OpenDialog));
    h.get_by_label("Export PDF…").click();
    h.run_steps(1);
    assert!(
        h.state()
            .actions
            .contains(&Action::ExportDialog(ExportFormat::Pdf))
    );
}

#[test]
fn file_buttons_can_be_hidden() {
    let mut state = state_with(Document::sample());
    state.file_buttons = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(2);
    assert!(h.query_by_label("Open GeoJSON…").is_none());
    assert!(h.query_by_label("Export PDF…").is_none());
}

#[test]
fn drag_pans_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let before = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, [0.0, 0.0])
            .unwrap()
            .unwrap()
    };
    h.drag_at(egui::pos2(700.0, 400.0));
    h.run_steps(1);
    h.hover_at(egui::pos2(750.0, 420.0));
    h.run_steps(1);
    h.drop_at(egui::pos2(750.0, 420.0));
    h.run_steps(2);
    let s = h.state_mut();
    assert!(s.doc.frame.view.is_some());
    let after = s
        .pipeline
        .lonlat_to_page(&s.doc, size, [0.0, 0.0])
        .unwrap()
        .unwrap();
    assert!(
        (after.x - before.x - 50.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
    assert!(
        (after.y - before.y - 20.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
}

#[test]
fn scroll_zooms_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let fit = {
        let s = h.state_mut();
        s.pipeline.fit_scale(&s.doc, size).unwrap()
    };
    h.hover_at(egui::pos2(700.0, 400.0));
    h.run_steps(1);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    let s = h.state();
    let view = s.doc.frame.view.expect("the scroll sets a view");
    assert!(view.scale > fit * 1.05, "{} vs {fit}", view.scale);
}

fn touch(h: &mut Harness<'static, AppState>, id: u64, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    });
}

/// The first finger is also the pointer, as in the window on iPad.
fn first_finger(h: &mut Harness<'static, AppState>, phase: egui::TouchPhase, pos: egui::Pos2) {
    touch(h, 1, phase, pos);
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

fn page_of_origin(h: &mut Harness<'static, AppState>) -> Point {
    let s = h.state_mut();
    s.pipeline
        .lonlat_to_page(&s.doc, s.canvas_size, [0.0, 0.0])
        .unwrap()
        .unwrap()
}

#[test]
fn one_finger_pans_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let before = page_of_origin(&mut h);
    first_finger(&mut h, egui::TouchPhase::Start, egui::pos2(700.0, 400.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(750.0, 420.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::End, egui::pos2(750.0, 420.0));
    h.run_steps(2);
    let after = page_of_origin(&mut h);
    assert!(
        (after.x - before.x - 50.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
    assert!(
        (after.y - before.y - 20.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
}

#[test]
fn two_fingers_zoom_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let origin = h.state().canvas_origin;
    let fit = {
        let s = h.state_mut();
        s.pipeline.fit_scale(&s.doc, size).unwrap()
    };
    // The fingers start 200 points apart, with the middle at 700, 400.
    let middle = Point::new(700.0 - f64::from(origin.0), 400.0 - f64::from(origin.1));
    let place = {
        let s = h.state_mut();
        s.pipeline
            .page_to_lonlat(&s.doc, size, middle)
            .unwrap()
            .unwrap()
    };
    first_finger(&mut h, egui::TouchPhase::Start, egui::pos2(600.0, 400.0));
    h.run_steps(1);
    touch(&mut h, 2, egui::TouchPhase::Start, egui::pos2(800.0, 400.0));
    h.run_steps(1);

    // The fingers move apart to 300 points. The middle stays.
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(550.0, 400.0));
    touch(&mut h, 2, egui::TouchPhase::Move, egui::pos2(850.0, 400.0));
    h.run_steps(1);
    let view = h.state().doc.frame.view.expect("the pinch sets a view");
    assert!(
        (view.scale / fit - 1.5).abs() < 0.01,
        "{} vs {fit}",
        view.scale
    );
    let at = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, place)
            .unwrap()
            .unwrap()
    };
    assert!((at - middle).hypot() < 1.0, "{at:?} {middle:?}");

    // Both fingers move by 30, 10. The map follows, and the scale stays.
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(580.0, 410.0));
    touch(&mut h, 2, egui::TouchPhase::Move, egui::pos2(880.0, 410.0));
    h.run_steps(1);
    let moved = h.state().doc.frame.view.unwrap();
    assert!((moved.scale / view.scale - 1.0).abs() < 0.01);
    let at = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, place)
            .unwrap()
            .unwrap()
    };
    let want = middle + kurbo::Vec2::new(30.0, 10.0);
    assert!((at - want).hypot() < 1.0, "{at:?} {want:?}");
}

#[test]
fn zoom_helper_zooms() {
    let mut h = harness();
    h.run_steps(3);
    let state = h.state_mut();
    let size = state.canvas_size;
    let fit = state.pipeline.fit_scale(&state.doc, size).unwrap();
    vale_app::ui::canvas::zoom_at(state, Point::new(300.0, 300.0), 2.0, 0.0);
    let view = state.doc.frame.view.unwrap();
    assert!((view.scale - 2.0 * fit).abs() < 1e-9 * fit);
}

#[test]
fn open_files_action() {
    let mut state = state_with(Document::empty());
    state.run_action(Action::OpenFiles(vec![land_path()]));
    assert_eq!(state.doc.frame.entries.len(), 1);
    assert!(state.status.starts_with("Opened"), "{}", state.status);

    let mut state = state_with(Document::empty());
    state.run_action(Action::OpenFiles(vec!["/no/such/file.geojson".into()]));
    assert!(state.doc.frame.entries.is_empty());
    assert!(!state.status.is_empty());
}

#[test]
fn export_action_writes_pdf() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-ui-export.pdf");
    let _ = std::fs::remove_file(&path);
    let mut state = state_with(Document::sample());
    state.canvas_size = (800.0, 600.0);
    state.run_action(Action::Export(path.clone()));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(state.status.starts_with("Exported"));
}

#[test]
fn projection_change_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().composed.as_ref().unwrap().view;
    let s = h.state_mut();
    s.doc.set_projection(ProjectionSpec {
        kind: ProjectionKind::Mercator,
        lon0: 0.0,
        lat0: 0.0,
    });
    s.touch();
    h.run_steps(2);
    assert_ne!(h.state().composed.as_ref().unwrap().view, before);
}

#[test]
fn full_ui_renders_offscreen() {
    let state = state_with(Document::sample());
    let (img, state) = match headless::ui_png(state, SIZE, 1.0) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/test-ui.png").unwrap();
    assert_eq!((img.width(), img.height()), (1280, 800));
    assert!(state.composed.is_some());
    let panel = *img.get_pixel(20, 300);
    let mid = *img.get_pixel(640, 400);
    assert_ne!(panel, mid);
    let colors: std::collections::HashSet<_> = img.pixels().map(|p| p.0).collect();
    assert!(colors.len() > 50, "{}", colors.len());
}
