//! The Measure tool on the globe and in the flat view.

use eframe::egui::{self, Pos2, pos2, vec2};
use egui_kittest::Harness;
use vale_app::document::Document;
use vale_app::globe::math::lonlat_to_dir;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Tool, WorldView};
use vale_app::measure::Measure;
use vale_app::session::Session;
use vale_app::ui::globe::measure_lines;
use vale_app::ui::{AppState, Layout, Workspace, draw};
use vale_sphere::LonLat;
use vale_sphere::measure::{path_length, ring_area};

type Pad = Harness<'static, AppState>;

const EARTH: f64 = 6371.0;
/// Four corners around the center of the view.
const SQUARE: [LonLat; 4] = [[0.0, 10.0], [20.0, 10.0], [20.0, 30.0], [0.0, 30.0]];

/// The iPad layout with the Measure tool, no layer selected, and no snap.
fn pad() -> Pad {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.select(None);
    state.globe.tool = Tool::Measure;
    state.globe.snap.on = false;
    state.globe.view = GlobeView::centered(10.0, 20.0);
    let mut h = Harness::builder()
        .with_size(vec2(1194.0, 834.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

/// The first touch is also the pointer. A touch with a force is a pen.
fn touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force,
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

fn tap_at(h: &mut Pad, pos: Pos2, force: Option<f32>) {
    touch(h, egui::TouchPhase::Start, pos, force);
    h.step();
    touch(h, egui::TouchPhase::End, pos, force);
    h.run_steps(3);
}

fn click_at(h: &mut Pad, pos: Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(pos));
    h.event(button(true));
    h.step();
    h.event(button(false));
    h.run_steps(3);
}

fn screen(h: &Pad, p: LonLat) -> Pos2 {
    h.state().globe.project(lonlat_to_dir(p[0], p[1])).unwrap()
}

/// Taps each corner with a pen, the mouse, and a finger in turn.
fn tap_corners(h: &mut Pad, corners: &[LonLat]) {
    for (i, &p) in corners.iter().enumerate() {
        let pos = screen(h, p);
        match i % 3 {
            0 => tap_at(h, pos, Some(0.5)),
            1 => click_at(h, pos),
            _ => tap_at(h, pos, None),
        }
    }
}

fn close_to(a: f64, b: f64, fraction: f64) -> bool {
    (a / b - 1.0).abs() < fraction
}

#[test]
fn taps_add_points_and_the_length_is_the_great_circle_length() {
    let mut h = pad();
    assert_eq!(h.state().measure_readout(), None);
    tap_corners(&mut h, &SQUARE);
    let state = h.state();
    assert_eq!(state.measure.points.len(), 4);
    assert!(!state.measure.closed);
    let length = state.measure.summary(EARTH).length_km;
    let exact = path_length(&SQUARE, false, EARTH);
    assert!(close_to(length, exact, 1e-4), "{length} {exact}");
    assert!(state.measure_readout().unwrap().starts_with("Length "));
}

#[test]
fn a_drag_turns_the_globe_and_adds_no_point() {
    let mut h = pad();
    let before = h.state().globe.view;
    let from = screen(&h, SQUARE[0]);
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for i in 1..=6 {
        let pos = from + vec2(20.0 * i as f32, 0.0);
        touch(&mut h, egui::TouchPhase::Move, pos, None);
        h.step();
    }
    touch(&mut h, egui::TouchPhase::End, from + vec2(120.0, 0.0), None);
    h.run_steps(3);
    assert!(h.state().measure.is_empty());
    assert_ne!(h.state().globe.view, before);
}

#[test]
fn a_tap_on_the_first_point_closes_the_path_and_shows_the_area() {
    for radius in [EARTH, 500.0] {
        let mut h = pad();
        h.state_mut().doc.project.world.radius_km = radius;
        h.step();
        tap_corners(&mut h, &SQUARE);
        let first = screen(&h, SQUARE[0]) + vec2(5.0, 5.0);
        tap_at(&mut h, first, None);
        let state = h.state();
        assert!(state.measure.closed);
        assert_eq!(state.measure.points.len(), 4);
        let summary = state.measure.summary(radius);
        let exact = ring_area(&SQUARE, radius);
        assert!(close_to(summary.area_km2.unwrap(), exact, 1e-3));
        let perimeter = path_length(&SQUARE, true, radius);
        assert!(close_to(summary.length_km, perimeter, 1e-4));
        let text = state.measure_readout().unwrap();
        assert!(
            text.contains(" · Area ") && text.ends_with(" km²"),
            "{text}"
        );
    }
}

#[test]
fn a_closed_path_that_crosses_itself_shows_the_perimeter_and_no_area() {
    let mut h = pad();
    let bow = [SQUARE[0], SQUARE[2], SQUARE[1], SQUARE[3]];
    tap_corners(&mut h, &bow);
    let first = screen(&h, bow[0]);
    tap_at(&mut h, first, None);
    let state = h.state();
    assert!(state.measure.closed);
    let summary = state.measure.summary(EARTH);
    assert_eq!(summary.area_km2, None);
    let units = state.doc.project.world.units;
    let length = vale_app::world::length(summary.length_km, units);
    assert_eq!(
        state.measure_readout().unwrap(),
        format!("Perimeter {length}")
    );
}

#[test]
fn a_point_snaps_to_a_vertex_only_when_snap_is_on() {
    for on in [true, false] {
        let mut h = pad();
        let places = h
            .state()
            .doc
            .project
            .layers()
            .iter()
            .find(|l| l.name == "Places");
        let target = match &places.unwrap().features[0].geometry {
            vale_store::Geometry::Points(points) => points[0],
            _ => panic!("not a point"),
        };
        let state = h.state_mut();
        state.globe.snap.on = on;
        let view = GlobeView::centered(target[0], target[1]);
        state.globe.view = GlobeView { zoom: 20.0, ..view };
        h.run_steps(2);
        let at = screen(&h, target) + vec2(1.0, 1.0);
        tap_at(&mut h, at, Some(0.5));
        let point = h.state().measure.points[0];
        assert_eq!(point.pos == target, on);
        assert_eq!(point.exact, on);
    }
}

/// The state of the undo log of the project file.
fn undo_log(h: &Pad) -> (bool, bool, u64) {
    let session = h.state().session.as_ref().unwrap();
    (session.can_undo(), session.can_redo(), session.undo_bytes())
}

/// A quick tap of `count` fingers side by side about `center`.
fn fingers_tap(h: &mut Pad, count: u64, center: Pos2) {
    let at = |id: u64| center + vec2(40.0 * id as f32, 0.0);
    let touch = |h: &mut Pad, id, phase| {
        h.event(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos: at(id),
            force: None,
        });
    };
    for id in 1..=count {
        touch(h, id, egui::TouchPhase::Start);
    }
    h.step();
    for id in (1..=count).rev() {
        touch(h, id, egui::TouchPhase::End);
    }
    h.run_steps(3);
}

fn key(h: &mut Pad, modifiers: egui::Modifiers, key: egui::Key) {
    h.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.run_steps(3);
}

/// Whether the Undo button and the Redo button are enabled.
fn history_buttons(h: &Pad) -> (bool, bool) {
    let able = |name: &str| {
        use egui_kittest::kittest::{NodeT, Queryable};
        !h.get_by_label(name).accesskit_node().is_disabled()
    };
    (able("Undo"), able("Redo"))
}

fn tap_button(h: &mut Pad, name: &str) {
    let controls = &h.state().pad.controls;
    let rect = controls.iter().find(|c| c.name == name).unwrap().rect;
    tap_at(h, rect.center(), None);
}

#[test]
fn undo_and_redo_act_on_the_path_and_leave_the_project_and_its_undo_log_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    // An edit before the Measure tool puts a step in the undo log.
    state.add_height();
    state.save_waiting().unwrap();
    state.select(None);
    state.globe.tool = Tool::Measure;
    state.globe.snap.on = false;
    state.globe.view = GlobeView::centered(10.0, 20.0);
    // A tap of fingers lasts less than 0.3 seconds, and a step of the
    // harness is short.
    let mut h = Harness::builder()
        .with_size(vec2(1194.0, 834.0))
        .with_pixels_per_point(1.0)
        .with_step_dt(0.01)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    let project = h.state().doc.project.clone();
    let log = undo_log(&h);
    assert!(log.0);
    // With no path, there is nothing to undo.
    assert_eq!(history_buttons(&h), (false, false));
    tap_corners(&mut h, &SQUARE[..3]);
    let first = screen(&h, SQUARE[0]);
    tap_at(&mut h, first, None);
    assert!(h.state().measure.closed);
    assert_eq!(history_buttons(&h), (true, false));

    let path = |h: &Pad| (h.state().measure.points.len(), h.state().measure.closed);
    let empty = pos2(600.0, 700.0);
    // A tap of two fingers reopens the path, and a tap of three closes it.
    fingers_tap(&mut h, 2, empty);
    assert_eq!(path(&h), (3, false));
    assert_eq!(history_buttons(&h), (true, true));
    fingers_tap(&mut h, 3, empty);
    assert_eq!(path(&h), (3, true));
    // Command+Z undoes, and Shift+Command+Z redoes.
    use egui::{Key, Modifiers};
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(path(&h), (3, false));
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(path(&h), (2, false));
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(path(&h), (3, false));
    // The buttons do the same.
    tap_button(&mut h, "Undo");
    tap_button(&mut h, "Undo");
    assert_eq!(path(&h), (1, false));
    tap_button(&mut h, "Redo");
    assert_eq!(path(&h), (2, false));
    // A new point ends the redo.
    let at = screen(&h, SQUARE[3]);
    tap_at(&mut h, at, None);
    assert_eq!(path(&h), (3, false));
    assert_eq!(history_buttons(&h), (true, false));
    // A tap after the close starts a new path, and undo brings back the
    // closed path.
    tap_at(&mut h, first, None);
    assert_eq!(path(&h), (3, true));
    let at = screen(&h, SQUARE[1]);
    tap_at(&mut h, at, None);
    assert_eq!(path(&h), (1, false));
    tap_button(&mut h, "Undo");
    assert_eq!(path(&h), (3, true));
    // Escape clears the path, as one step.
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().measure.is_empty());
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(path(&h), (3, true));
    // Backspace does nothing.
    key(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(path(&h), (3, true));

    let later = std::time::Instant::now() + std::time::Duration::from_secs(5);
    h.state_mut().save_tick(false, later);
    assert_eq!(h.state().doc.project, project);
    assert_eq!(undo_log(&h), log);
    // After the Measure tool, undo acts on the project again.
    h.state_mut().globe.tool = Tool::Navigate;
    h.step();
    assert!(h.state().can_undo());
    h.state_mut().undo();
    assert!(h.state().doc.project.rasters().is_empty());
}

#[test]
fn the_path_is_not_saved_and_a_tool_change_or_a_workspace_switch_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = pad();
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    let project = h.state().doc.project.clone();
    let saves = h.state().session.as_ref().unwrap().saves();
    let log = undo_log(&h);
    tap_corners(&mut h, &SQUARE);
    let first = screen(&h, SQUARE[0]);
    tap_at(&mut h, first, None);
    let later = std::time::Instant::now() + std::time::Duration::from_secs(5);
    h.state_mut().save_tick(false, later);
    assert!(h.state().measure.closed);
    assert_eq!(h.state().doc.project, project);
    assert_eq!(h.state().session.as_ref().unwrap().saves(), saves);
    assert_eq!(undo_log(&h), log);

    h.state_mut().globe.tool = Tool::Navigate;
    h.step();
    assert!(h.state().measure.is_empty());

    h.state_mut().globe.tool = Tool::Measure;
    tap_corners(&mut h, &SQUARE[..2]);
    assert_eq!(h.state().measure.points.len(), 2);
    h.state_mut().workspace = Workspace::Atlas;
    h.step();
    assert!(h.state().measure.is_empty());
}

#[test]
fn a_path_across_180_has_the_same_values_and_no_line_across_the_flat_map() {
    let path = |lon: f64| {
        let mut m = Measure::default();
        for p in [[-10.0, 0.0], [10.0, 0.0], [10.0, 10.0], [-10.0, 10.0]] {
            m.add([lon + p[0], p[1]], false);
        }
        m.close();
        m
    };
    let (at_0, at_180) = (path(0.0), path(180.0));
    let (a, b) = (at_0.summary(EARTH), at_180.summary(EARTH));
    assert!(close_to(a.length_km, b.length_km, 1e-9));
    assert!(close_to(a.area_km2.unwrap(), b.area_km2.unwrap(), 1e-9));

    let mut h = pad();
    h.state_mut().globe.world_view = WorldView::Flat;
    h.state_mut().measure = at_180;
    h.run_steps(2);
    let globe = &h.state().globe;
    let rect = globe.rect;
    let half = globe.flat.map_width(rect) / 2.0;
    let lines = measure_lines(globe, rect, 1.0, &h.state().measure);
    assert_eq!(lines.len(), 4);
    for part in lines.iter().flatten() {
        for w in part.windows(2) {
            assert!((w[0].x - w[1].x).abs() < half, "{w:?}");
        }
    }
    // The two segments across 180° break at the edges of the map.
    let parts: Vec<usize> = lines.iter().map(Vec::len).collect();
    assert_eq!(parts, [2, 1, 2, 1]);
}
