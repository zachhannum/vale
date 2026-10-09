use clap::Parser;
use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::cli::{Args, apply_globe_view};
use vale_app::document::Document;
use vale_app::globe::math::{V3, angle, lonlat_to_dir};
use vale_app::globe::view::GlobeView;
use vale_app::headless;
use vale_app::ui::{AppState, Workspace, draw};

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state
}

fn harness() -> Harness<'static, AppState> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(2);
    h
}

/// The place on the globe under a screen position.
fn place(h: &Harness<'static, AppState>, pos: Pos2) -> V3 {
    let globe = &h.state().globe;
    globe.view.unproject(globe.rect, pos).unwrap()
}

/// The distance on screen from a place on the globe to a screen position.
fn off(h: &Harness<'static, AppState>, place: V3, pos: Pos2) -> f32 {
    let globe = &h.state().globe;
    (globe.view.project(globe.rect, place).unwrap() - pos).length()
}

fn button(h: &mut Harness<'static, AppState>, button: egui::PointerButton, pos: Pos2, down: bool) {
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton {
        pos,
        button,
        pressed: down,
        modifiers: egui::Modifiers::NONE,
    });
}

fn wheel(h: &mut Harness<'static, AppState>, unit: egui::MouseWheelUnit, delta: Vec2) {
    h.event(egui::Event::MouseWheel {
        unit,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
}

fn touch(h: &mut Harness<'static, AppState>, id: u64, phase: egui::TouchPhase, pos: Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    });
}

/// The first finger is also the pointer, as in the window on iPad.
fn first_finger(h: &mut Harness<'static, AppState>, phase: egui::TouchPhase, pos: Pos2) {
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

const A: Pos2 = Pos2::new(600.0, 350.0);
const B: Pos2 = Pos2::new(680.0, 420.0);

#[test]
fn the_app_opens_on_the_globe() {
    let args = Args::try_parse_from(["vale-app"]).unwrap();
    assert_eq!(args.workspace, Workspace::Globe);
    let h = harness();
    let s = h.state();
    assert_eq!(s.workspace, Workspace::Globe);
    assert!(s.composed.is_none());
    // The globe has the full width, with no side panels.
    assert_eq!(s.globe.rect.width(), 1280.0);
    assert!(h.query_by_label("Projection").is_none());
}

#[test]
fn the_flat_map_is_reachable() {
    let mut h = harness();
    h.get_by_label("Map").click();
    h.run_steps(3);
    let s = h.state();
    assert_eq!(s.workspace, Workspace::Map);
    assert!(s.composed.is_some());
    assert!(h.query_by_label("Projection").is_some());
    h.get_by_label("Globe").click();
    h.run_steps(2);
    assert_eq!(h.state().workspace, Workspace::Globe);

    let args = Args::try_parse_from(["vale-app", "--workspace", "map"]).unwrap();
    assert_eq!(args.workspace, Workspace::Map);
}

#[test]
fn look_at_and_zoom_set_the_globe_view() {
    let args = Args::try_parse_from(["vale-app", "--look-at", "40,-30", "--zoom", "3"]).unwrap();
    let mut view = GlobeView::centered(0.0, 0.0);
    apply_globe_view(&args, &mut view).unwrap();
    assert_eq!(view.zoom, 3.0);
    let center = view.rot.inv_mul_vec([0.0, 0.0, 1.0]);
    assert!(angle(center, lonlat_to_dir(40.0, -30.0)) < 1e-9);
}

#[test]
fn a_mouse_drag_rotates_with_each_button() {
    for which in [
        egui::PointerButton::Primary,
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
    ] {
        let mut h = harness();
        let grabbed = place(&h, A);
        button(&mut h, which, A, true);
        h.run_steps(1);
        h.event(egui::Event::PointerMoved(B));
        h.run_steps(1);
        button(&mut h, which, B, false);
        h.run_steps(1);
        assert!(off(&h, grabbed, B) < 0.05, "{which:?}");
        assert_eq!(h.state().globe.view.zoom, 1.0);
        assert!(!h.state().globe.nav.active());
    }
}

#[test]
fn a_shift_drag_twists_about_the_middle() {
    let mut h = harness();
    let center = h.state().globe.rect.center();
    let middle = place(&h, center);
    let from = center + Vec2::new(100.0, 0.0);
    let to = center + Vec2::new(0.0, 100.0);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    button(&mut h, egui::PointerButton::Primary, from, true);
    h.run_steps(1);
    h.event(egui::Event::PointerMoved(to));
    h.run_steps(1);
    button(&mut h, egui::PointerButton::Primary, to, false);
    h.run_steps(1);
    assert!(off(&h, middle, center) < 0.05);
    // A quarter turn clockwise on screen: north was up and now points right.
    let up = h.state().globe.view.rot.inv_mul_vec([1.0, 0.0, 0.0]);
    let before = GlobeView::centered(15.0, 25.0)
        .rot
        .inv_mul_vec([0.0, 1.0, 0.0]);
    assert!(angle(up, before) < 1e-6);
}

#[test]
fn the_wheel_zooms_about_the_pointer() {
    let mut h = harness();
    let under = place(&h, A);
    h.hover_at(A);
    h.run_steps(1);
    wheel(&mut h, egui::MouseWheelUnit::Line, Vec2::new(0.0, 3.0));
    h.run_steps(30);
    assert!(h.state().globe.view.zoom > 1.1);
    assert!(off(&h, under, A) < 0.05);
}

#[test]
fn a_trackpad_scroll_rotates() {
    let mut h = harness();
    let under = place(&h, A);
    h.hover_at(A);
    h.run_steps(1);
    wheel(&mut h, egui::MouseWheelUnit::Point, B - A);
    h.run_steps(30);
    assert_eq!(h.state().globe.view.zoom, 1.0);
    assert!(off(&h, under, B) < 1.0, "{}", off(&h, under, B));
}

#[test]
fn a_trackpad_pinch_zooms_about_the_pointer() {
    let mut h = harness();
    let under = place(&h, A);
    h.hover_at(A);
    h.run_steps(1);
    h.event(egui::Event::Zoom(1.5));
    h.run_steps(2);
    assert!((h.state().globe.view.zoom - 1.5).abs() < 1e-6);
    assert!(off(&h, under, A) < 0.05);
}

#[test]
fn a_trackpad_rotate_gesture_twists_about_the_pointer() {
    let mut h = harness();
    let center = h.state().globe.rect.center();
    let under = place(&h, center);
    h.hover_at(center);
    h.run_steps(1);
    // egui counts a clockwise turn as positive.
    h.event(egui::Event::Rotate(std::f32::consts::FRAC_PI_2));
    h.run_steps(2);
    assert!(off(&h, under, center) < 0.05);
    let right = h.state().globe.view.rot.inv_mul_vec([1.0, 0.0, 0.0]);
    let up_before = GlobeView::centered(15.0, 25.0)
        .rot
        .inv_mul_vec([0.0, 1.0, 0.0]);
    assert!(angle(right, up_before) < 1e-6);
}

#[test]
fn one_finger_rotates() {
    let mut h = harness();
    let grabbed = place(&h, A);
    first_finger(&mut h, egui::TouchPhase::Start, A);
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::Move, B);
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::End, B);
    h.run_steps(2);
    assert!(off(&h, grabbed, B) < 0.05);
    assert!(!h.state().globe.nav.active());
}

#[test]
fn two_fingers_zoom_and_twist() {
    let mut h = harness();
    let center = h.state().globe.rect.center();
    let middle = place(&h, center);
    first_finger(
        &mut h,
        egui::TouchPhase::Start,
        center - Vec2::new(50.0, 0.0),
    );
    h.run_steps(1);
    touch(
        &mut h,
        2,
        egui::TouchPhase::Start,
        center + Vec2::new(50.0, 0.0),
    );
    h.run_steps(1);
    // Twice as far apart, and a quarter turn clockwise on screen.
    first_finger(
        &mut h,
        egui::TouchPhase::Move,
        center - Vec2::new(0.0, 100.0),
    );
    touch(
        &mut h,
        2,
        egui::TouchPhase::Move,
        center + Vec2::new(0.0, 100.0),
    );
    h.run_steps(1);
    assert!((h.state().globe.view.zoom - 2.0).abs() < 1e-6);
    assert!(off(&h, middle, center) < 0.05);
    let right = h.state().globe.view.rot.inv_mul_vec([1.0, 0.0, 0.0]);
    let up_before = GlobeView::centered(15.0, 25.0)
        .rot
        .inv_mul_vec([0.0, 1.0, 0.0]);
    assert!(angle(right, up_before) < 1e-6);
}

#[test]
fn a_touch_on_the_tool_bar_does_not_move_the_globe() {
    let mut h = harness();
    let before = h.state().globe.view;
    first_finger(&mut h, egui::TouchPhase::Start, Pos2::new(600.0, 10.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::Move, Pos2::new(650.0, 300.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::End, Pos2::new(650.0, 300.0));
    h.run_steps(1);
    assert_eq!(h.state().globe.view, before);
}

#[test]
fn the_screenshot_draws_the_heightmap_on_the_globe() {
    let mut state = state();
    // A high patch in the middle of the face that looks at the eye.
    state.globe.view = GlobeView::centered(0.0, 0.0);
    let n = state.globe.map.face_size();
    for y in n / 2 - 40..n / 2 + 40 {
        for x in n / 2 - 40..n / 2 + 40 {
            state.globe.map.set(0, x, y, u16::MAX);
        }
    }
    let (img, state) = headless::ui_png(state, (1280.0, 800.0), 1.0).unwrap();
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/test-globe.png").unwrap();
    let rect = state.globe.rect;
    let at = |dx: f32, dy: f32| {
        let p = rect.center() + Vec2::new(dx, dy);
        img.get_pixel(p.x as u32, p.y as u32).0
    };
    let (patch, disk, outside) = (at(4.0, 4.0), at(150.0, 104.0), at(-600.0, 0.0));
    assert!(patch[0] > 240, "{patch:?}");
    assert!(disk[0] > 40 && disk[0] < 140, "{disk:?}");
    assert_eq!(disk[0], disk[1]);
    assert_ne!(outside, disk);
    assert!(state.composed.is_none());
}
