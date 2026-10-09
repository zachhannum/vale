use clap::Parser;
use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::cli::{Args, apply_globe, apply_globe_view};
use vale_app::document::Document;
use vale_app::globe::math::{V3, angle, lonlat_to_dir};
use vale_app::globe::view::GlobeView;
use vale_app::globe::{FACE_SIZE, Tool};
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
    touch_with_force(h, id, phase, pos, None);
}

fn touch_with_force(
    h: &mut Harness<'static, AppState>,
    id: u64,
    phase: egui::TouchPhase,
    pos: Pos2,
    force: Option<f32>,
) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force,
    });
}

/// The first finger is also the pointer, as in the window on iPad.
fn first_finger(h: &mut Harness<'static, AppState>, phase: egui::TouchPhase, pos: Pos2) {
    first_touch(h, phase, pos, None);
}

/// The first touch is also the pointer. A touch with a force is a pen.
fn first_touch(
    h: &mut Harness<'static, AppState>,
    phase: egui::TouchPhase,
    pos: Pos2,
    force: Option<f32>,
) {
    touch_with_force(h, 1, phase, pos, force);
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
    let _gpu = one_gpu_test();
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

/// The face size of the brush tests.
const BRUSH_FACE_SIZE: usize = 256;

/// One test at a time makes a wgpu device. Software adapters fail when two
/// threads make devices at the same time.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A harness with the wgpu renderer. Each step renders, so each step runs the
/// GPU work of the globe.
fn gpu_harness() -> Harness<'static, AppState> {
    let mut state = state();
    state.globe.set_face_size(BRUSH_FACE_SIZE);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (640.0, 480.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

/// Runs frames until the stroke is in the CPU heightmap.
fn settle(h: &mut Harness<'static, AppState>) {
    for _ in 0..400 {
        h.step();
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the stroke did not end");
}

/// A drag of the primary mouse button across the middle of the globe. Returns
/// the middle.
fn mouse_stroke(h: &mut Harness<'static, AppState>) -> Pos2 {
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    button(h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 8.0)));
        h.step();
    }
    button(h, egui::PointerButton::Primary, to, false);
    h.step();
    c
}

/// The frame without the pointer, cut to the canvas of the globe.
fn canvas_image(h: &mut Harness<'static, AppState>) -> image::RgbaImage {
    h.remove_cursor();
    h.run_steps(2);
    let rect = h.state().globe.rect;
    let (x, y) = (rect.min.x.ceil() as u32, rect.min.y.ceil() as u32);
    let (w, h_) = (rect.width().floor() as u32, rect.height().floor() as u32);
    let frame = h.render().unwrap();
    image::imageops::crop_imm(&frame, x, y, w - 1, h_ - 1).to_image()
}

#[test]
fn face_size_sets_the_heightmap_of_the_globe() {
    let args = Args::try_parse_from(["vale-app", "--face-size", "512", "--stroke-test"]).unwrap();
    assert!(args.stroke_test);
    let mut state = state();
    apply_globe(&args, &mut state.globe, FACE_SIZE).unwrap();
    assert_eq!(state.globe.map.face_size(), 512);
    assert_eq!(state.globe.stats.face_size, 512);

    let args = Args::try_parse_from(["vale-app"]).unwrap();
    apply_globe(&args, &mut state.globe, FACE_SIZE).unwrap();
    assert_eq!(state.globe.map.face_size(), FACE_SIZE);

    let args = Args::try_parse_from(["vale-app", "--face-size", "0"]).unwrap();
    assert!(apply_globe(&args, &mut state.globe, FACE_SIZE).is_err());
}

#[test]
fn painting_changes_the_globe() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.get_by_label("Brush").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    let view = h.state().globe.view;
    let before = canvas_image(&mut h);
    mouse_stroke(&mut h);
    settle(&mut h);
    let after = canvas_image(&mut h);
    // The drag paints and does not rotate.
    assert_eq!(h.state().globe.view, view);
    let (w, hh) = before.dimensions();
    let under = (w / 2 + 7, hh / 2 + 5);
    let far = (w / 2 + 7, hh / 2 + 120);
    let red = |img: &image::RgbaImage, (x, y): (u32, u32)| img.get_pixel(x, y).0[0];
    assert!(
        red(&after, under) > red(&before, under) + 4,
        "{} and {}",
        red(&after, under),
        red(&before, under)
    );
    assert_eq!(
        after.get_pixel(far.0, far.1),
        before.get_pixel(far.0, far.1)
    );
}

#[test]
fn the_debug_panel_shows_the_stroke_delay() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    assert!(h.query_by_label_contains("Stroke delay").is_none());
    h.get_by_label("Debug").click();
    h.get_by_label("Brush").click();
    h.run_steps(2);
    assert!(h.query_by_label("Stroke delay: no stroke").is_some());
    assert!(h.state().globe.rect.width() < 640.0);
    mouse_stroke(&mut h);
    settle(&mut h);
    for _ in 0..400 {
        if h.state().globe.stats.last().unwrap().delay_ms.count > 0 {
            break;
        }
        h.step();
    }
    h.run_steps(2);
    let label = h.get_by_label_contains("Stroke delay: last");
    let text = label.accesskit_node().value().unwrap();
    // "Stroke delay: last 1.23 ms, mean ..."
    let number = text
        .strip_prefix("Stroke delay: last ")
        .and_then(|rest| rest.split_once(" ms"))
        .map(|(number, _)| number.parse::<f64>());
    assert!(matches!(number, Some(Ok(ms)) if ms > 0.0), "{text}");
    assert!(h.query_by_label_contains("GPU: ").is_some());
    assert!(h.query_by_label_contains("Frame interval").is_some());
    assert!(h.query_by_label_contains("is not included").is_some());
}

#[test]
fn a_stroke_reaches_the_cpu_heightmap_and_undo_puts_it_back() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.state_mut().globe.tool = Tool::Brush;
    let before = canvas_image(&mut h);
    let base = h.state().globe.map.base();
    let c = mouse_stroke(&mut h);
    let under = place(&h, c);
    // The stroke is on the GPU only, until its texels come back.
    settle(&mut h);
    let globe = &h.state().globe;
    assert!(globe.map.sample(under) > base);
    assert!(globe.map.allocated_tiles() > 0);
    assert!(globe.can_undo());
    let painted = canvas_image(&mut h);
    assert_ne!(painted.as_raw(), before.as_raw());

    assert!(h.state_mut().globe.undo());
    assert_eq!(h.state().globe.map.sample(under), base);
    assert_eq!(h.state().globe.map.allocated_tiles(), 0);
    let after = canvas_image(&mut h);
    assert!(
        after.as_raw() == before.as_raw(),
        "the render is not as before"
    );
}

#[test]
fn navigate_is_unchanged_with_the_brush_tool_off() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 20.0));
    let grabbed = place(&h, from);
    button(&mut h, egui::PointerButton::Primary, from, true);
    h.step();
    h.event(egui::Event::PointerMoved(to));
    h.step();
    button(&mut h, egui::PointerButton::Primary, to, false);
    h.run_steps(3);
    assert!(off(&h, grabbed, to) < 0.05);
    // A pen also rotates.
    let grabbed = place(&h, from);
    first_touch(&mut h, egui::TouchPhase::Start, from, Some(0.5));
    h.step();
    first_touch(&mut h, egui::TouchPhase::Move, to, Some(0.5));
    h.step();
    first_touch(&mut h, egui::TouchPhase::End, to, Some(0.5));
    h.run_steps(3);
    assert!(off(&h, grabbed, to) < 0.05);

    let globe = &h.state().globe;
    assert!(!globe.busy());
    assert!(globe.stats.last().is_none());
    assert_eq!(globe.map.allocated_tiles(), 0);
    assert!(!globe.can_undo());
}

#[test]
fn in_the_brush_tool_a_pen_paints_and_the_other_input_moves_the_globe() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(1);
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 20.0));

    // The secondary button and a finger rotate, and they do not paint.
    let grabbed = place(&h, from);
    button(&mut h, egui::PointerButton::Secondary, from, true);
    h.step();
    h.event(egui::Event::PointerMoved(to));
    h.step();
    button(&mut h, egui::PointerButton::Secondary, to, false);
    h.run_steps(2);
    assert!(off(&h, grabbed, to) < 0.05);
    let grabbed = place(&h, from);
    first_finger(&mut h, egui::TouchPhase::Start, from);
    h.step();
    first_finger(&mut h, egui::TouchPhase::Move, to);
    h.step();
    first_finger(&mut h, egui::TouchPhase::End, to);
    h.run_steps(2);
    assert!(off(&h, grabbed, to) < 0.05);
    assert!(h.state().globe.stats.last().is_none());

    // A pen paints, and it does not rotate.
    let view = h.state().globe.view;
    let base = h.state().globe.map.base();
    let under = place(&h, c);
    first_touch(&mut h, egui::TouchPhase::Start, from, Some(0.5));
    h.step();
    first_touch(&mut h, egui::TouchPhase::Move, c, Some(0.5));
    h.step();
    first_touch(&mut h, egui::TouchPhase::Move, to, Some(0.5));
    h.step();
    first_touch(&mut h, egui::TouchPhase::End, to, Some(0.5));
    settle(&mut h);
    assert_eq!(h.state().globe.view, view);
    assert!(h.state().globe.map.sample(under) > base);
    assert_eq!(h.state().globe.stats.last().unwrap().id, 1);
}

#[test]
fn the_stroke_test_runs_and_gives_the_stroke_delay() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.get_by_label("Debug").click();
    h.run_steps(2);
    let brush = h.state().globe.brush;
    h.state_mut().globe.start_stroke_test(0.25);
    assert!(h.state().globe.busy());
    settle(&mut h);
    h.run_steps(2);
    let globe = &h.state().globe;
    let report = globe.test_report.as_deref().unwrap();
    assert!(report.contains("raise, 160 pt"), "{report}");
    assert!(report.contains("smooth, 160 pt"), "{report}");
    assert_eq!(report.matches("Stroke delay: last ").count(), 2, "{report}");
    assert!(globe.stats.last().unwrap().delay_ms.count >= 1);
    assert_eq!(globe.brush, brush);
    // The stroke is in the CPU heightmap, on both sides of a face edge.
    let base = globe.map.base();
    assert!(globe.map.sample(lonlat_to_dir(30.0, 21.0)) > base);
    assert!(globe.map.sample(lonlat_to_dir(60.0, 21.0)) > base);
    assert!(h.query_by_label("Stroke test").is_some());
    std::fs::create_dir_all("../../target/app").unwrap();
    let frame = h.render().unwrap();
    frame.save("../../target/app/test-brush-debug.png").unwrap();
}
