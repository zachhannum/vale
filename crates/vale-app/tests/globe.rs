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
use vale_app::pen::{PenEvent, PenPhase, PenQueue};
use vale_app::ui::elevation::{BAR_LABEL, limit_y};
use vale_app::ui::{AppState, Workspace, draw};
use vale_terrain::{ChannelMap, CoarseHeights, Mode, channel_map, meters_to_level};

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
    state.globe.preview.greyscale = true;
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
    assert!(report.contains("Stroke 3: raise, 4 pt"), "{report}");
    assert!(report.contains("Stroke 4: smooth, 4 pt"), "{report}");
    assert_eq!(report.matches("Stroke delay: last ").count(), 4, "{report}");
    assert_eq!(report.matches("Passes in a frame").count(), 4, "{report}");
    assert_eq!(globe.stats.backlog, 0);
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

#[test]
fn a_fast_stroke_of_a_small_brush_does_not_fall_behind() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.state_mut().globe.brush.size_points = 4.0;
    h.run_steps(1);
    let base = h.state().globe.map.base();
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(110.0, 0.0), c + Vec2::new(110.0, 0.0));
    let under = place(&h, c);
    button(&mut h, egui::PointerButton::Primary, from, true);
    h.step();
    h.event(egui::Event::PointerMoved(to));
    h.step();
    // One frame took all stamps of the move.
    assert_eq!(h.state().globe.stats.backlog, 0);
    button(&mut h, egui::PointerButton::Primary, to, false);
    settle(&mut h);
    h.run_steps(2);

    let globe = &h.state().globe;
    let stroke = globe.stats.last().unwrap();
    // The move gives half of the stamps, and the release gives the other half.
    assert!(stroke.stamps.worst >= 150.0, "{}", stroke.stamps_line());
    assert!(stroke.stamps.count <= 3, "{}", stroke.stamps_line());
    // 32 stamps go in one pass on each face that they touch.
    let passes = stroke.passes.worst;
    assert!(
        passes >= stroke.stamps.worst / 32.0,
        "{}",
        stroke.passes_line()
    );
    assert!(
        passes <= stroke.stamps.worst / 8.0,
        "{}",
        stroke.passes_line()
    );
    assert_eq!(globe.stats.backlog, 0);
    assert!(globe.map.sample(under) > base);
    assert!(globe.map.sample(place(&h, c + Vec2::new(100.0, 0.0))) > base);
    assert_eq!(globe.map.sample(place(&h, c + Vec2::new(0.0, 40.0))), base);
}

/// A pen stroke across the middle of the globe, with half of the full force.
fn pen_stroke(h: &mut Harness<'static, AppState>) {
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    first_touch(h, egui::TouchPhase::Start, from, Some(0.5));
    h.step();
    for i in 1..=8 {
        let pos = from.lerp(to, i as f32 / 8.0);
        first_touch(h, egui::TouchPhase::Move, pos, Some(0.5));
        h.step();
    }
    first_touch(h, egui::TouchPhase::End, to, Some(0.5));
    h.step();
}

#[test]
fn the_four_modes_work_with_a_mouse_and_with_a_pen() {
    let _gpu = one_gpu_test();
    for pen in [false, true] {
        let mut h = gpu_harness();
        h.state_mut().globe.tool = Tool::Brush;
        h.run_steps(1);
        let base = h.state().globe.map.base();
        let under = place(&h, h.state().globe.rect.center());
        let level = meters_to_level(3000.0);
        h.state_mut().globe.brush.flatten_level = Some(level);
        let mut stroke = |mode: Mode| {
            h.state_mut().globe.brush.mode = mode;
            match pen {
                true => pen_stroke(&mut h),
                false => drop(mouse_stroke(&mut h)),
            }
            settle(&mut h);
            h.state().globe.map.sample(under)
        };
        let raised = stroke(Mode::Raise);
        assert!(raised > base, "pen {pen}: {raised}");
        // The ground next to the stroke is lower, so the middle goes down.
        let smooth = stroke(Mode::Smooth);
        assert!(smooth < raised && smooth > base, "pen {pen}: {smooth}");
        let lowered = stroke(Mode::Lower);
        assert!(lowered < smooth, "pen {pen}: {lowered}");
        let flat = stroke(Mode::Flatten);
        assert!(flat > lowered && flat <= level, "pen {pen}: {flat}");
        assert_eq!(h.state().globe.stats.last().unwrap().id, 4);
    }
}

#[test]
fn a_press_on_the_globe_picks_the_flatten_level() {
    use vale_terrain::Mode;
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(1);
    let base = h.state().globe.map.base();
    let c = mouse_stroke(&mut h);
    settle(&mut h);
    let under = place(&h, c);
    let raised = h.state().globe.map.sample(under);
    assert!(raised > base);

    // A mouse press picks the level, and it does not paint.
    h.state_mut().globe.brush.mode = Mode::Flatten;
    h.state_mut().globe.pick_level = true;
    h.run_steps(1);
    button(&mut h, egui::PointerButton::Primary, c, true);
    h.step();
    button(&mut h, egui::PointerButton::Primary, c, false);
    h.run_steps(2);
    let globe = &h.state().globe;
    assert_eq!(globe.brush.flatten_level, Some(raised));
    assert!(!globe.pick_level && !globe.busy());
    assert_eq!(globe.stats.last().unwrap().id, 1);
    assert_eq!(globe.map.sample(under), raised);

    // A pen picks the level of the ground beside the stroke.
    let beside = c + Vec2::new(0.0, 90.0);
    h.state_mut().globe.pick_level = true;
    first_touch(&mut h, egui::TouchPhase::Start, beside, Some(0.3));
    h.step();
    first_touch(&mut h, egui::TouchPhase::End, beside, Some(0.3));
    h.run_steps(2);
    let globe = &h.state().globe;
    assert_eq!(globe.brush.flatten_level, Some(base));
    assert!(!globe.pick_level);
    assert_eq!(globe.stats.last().unwrap().id, 1);

    // The next stroke moves the ground to that level.
    mouse_stroke(&mut h);
    settle(&mut h);
    let flat = h.state().globe.map.sample(under);
    assert!(flat < raised && flat >= base, "{flat}");
    assert_eq!(h.state().globe.stats.last().unwrap().id, 2);
    std::fs::create_dir_all("../../target/app").unwrap();
    let frame = h.render().unwrap();
    frame.save("../../target/app/test-brush-panel.png").unwrap();
}

#[test]
fn the_brush_panel_locks_the_ground_size_of_the_brush() {
    let mut h = harness();
    assert!(h.query_by_label("Lock size").is_none());
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    for label in ["Raise", "Lower", "Smooth", "Flatten", "Radius", "Hardness"] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label}");
    }
    assert!(h.query_by_label("Pick from globe").is_none());
    h.get_by_label("Flatten").click();
    h.run_steps(2);
    assert!(h.query_by_label("Pick from globe").is_some());

    // Without the lock, the brush covers half of the ground at twice the zoom.
    let world_km = h.state().doc.project.world.radius_km;
    let km = |h: &Harness<'static, AppState>| {
        let globe = &h.state().globe;
        globe.brush.radius_km(globe.radius(), world_km)
    };
    let before = km(&h);
    assert!((before - 36.0 / h.state().globe.radius() * world_km).abs() < 1e-6);
    h.state_mut().globe.view.zoom = 2.0;
    h.run_steps(1);
    assert!((km(&h) - before / 2.0).abs() < 1e-6);

    h.get_by_label("Lock size").click();
    h.run_steps(2);
    assert!(h.state().globe.brush.lock.is_some());
    let locked = km(&h);
    assert!((locked - before / 2.0).abs() < 1e-6);
    for zoom in [0.5, 1.0, 8.0, 40.0] {
        h.state_mut().globe.view.zoom = zoom;
        h.run_steps(1);
        assert!((km(&h) - locked).abs() < 1e-6, "zoom {zoom}");
    }
}

/// A GPU harness with no graticule, and the color at the middle of the globe.
fn preview_harness() -> Harness<'static, AppState> {
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.preview.graticule = false;
    globe.preview.panel = true;
    globe.view = GlobeView::centered(0.0, 0.0);
    h.step();
    h
}

/// Sets the whole world to one elevation and returns the color at the middle
/// of the globe.
fn color_at(h: &mut Harness<'static, AppState>, meters: f64) -> [u8; 3] {
    h.state_mut().globe.map.fill(meters_to_level(meters));
    middle(h)
}

fn middle(h: &mut Harness<'static, AppState>) -> [u8; 3] {
    let img = canvas_image(h);
    let p = img.get_pixel(img.width() / 2, img.height() / 2).0;
    [p[0], p[1], p[2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}

#[test]
fn the_ramp_has_separate_colors_above_and_below_sea_level() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    let ramp = &mut h.state_mut().globe.map.bands.ramp;
    ramp.sea = vec![[0, 0, 100], [0, 0, 250]];
    ramp.land = vec![[100, 0, 0], [250, 0, 0]];
    for meters in [-5000.0, -1500.0, -100.0] {
        let [r, g, b] = color_at(&mut h, meters);
        assert!(r <= 2 && g <= 2 && b >= 98, "{meters}: {:?}", [r, g, b]);
    }
    for meters in [100.0, 1500.0, 5000.0] {
        let [r, g, b] = color_at(&mut h, meters);
        assert!(r >= 98 && g <= 2 && b <= 2, "{meters}: {:?}", [r, g, b]);
    }
    // A height at sea level is land.
    let [r, _, b] = color_at(&mut h, 0.0);
    assert!(r >= 98 && b <= 2);
}

#[test]
fn the_preview_shows_the_color_of_the_band() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    let bands = h.state().globe.map.bands.bands();
    for meters in [-3000.0, -150.0, 300.0, 1500.0, 4500.0] {
        let band = bands.iter().find(|b| b.min <= meters && meters < b.max);
        let color = color_at(&mut h, meters);
        assert!(close(color, band.unwrap().color), "{meters}: {color:?}");
    }
}

#[test]
fn one_switch_shows_the_plain_greyscale() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    let [r, g, b] = color_at(&mut h, 1500.0);
    assert!(r != g || g != b);
    h.get_by_label("Greyscale").click();
    h.step();
    let grey = middle(&mut h);
    let level = (f32::from(meters_to_level(1500.0)) / 65535.0 * 255.0).round() as u8;
    assert!(close(grey, [level; 3]), "{grey:?}");
    h.get_by_label("Greyscale").click();
    h.step();
    assert_eq!(middle(&mut h), [r, g, b]);
}

#[test]
fn without_levels_the_ramp_is_smooth() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    // Two heights in the band from 1000 m to 2000 m.
    let (low, high) = (color_at(&mut h, 1200.0), color_at(&mut h, 1800.0));
    assert_eq!(low, high);
    h.get_by_label("Levels").click();
    h.step();
    assert!(!h.state().globe.preview.levels);
    let (low, high) = (color_at(&mut h, 1200.0), color_at(&mut h, 1800.0));
    assert_ne!(low, high);
    // The ramp still has a cut at sea level.
    let bands = h.state().globe.map.bands.bands();
    let sea = h.state().globe.map.bands.sea_index();
    assert!(close(color_at(&mut h, 0.0), bands[sea + 1].bottom));
    assert!(close(color_at(&mut h, -1.0), bands[sea].top));
}

#[test]
fn a_change_of_a_band_limit_shows_in_the_same_frame() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    let before = color_at(&mut h, 300.0);
    // The frame that changes the limit also draws the globe.
    let frame = |h: &mut Harness<'static, AppState>| {
        h.step();
        let rect = h.state().globe.rect;
        let img = h.render().unwrap();
        let c = rect.center();
        let p = img.get_pixel(c.x as u32, c.y as u32).0;
        [p[0], p[1], p[2]]
    };
    let bands = &mut h.state_mut().globe.map.bands;
    let index = bands.limits().iter().position(|m| *m == 200.0).unwrap();
    bands.move_to(index, 400.0);
    let moved = frame(&mut h);
    assert_ne!(moved, before);
    assert!(h.state_mut().globe.map.bands.remove(index));
    let removed = frame(&mut h);
    assert_ne!(removed, moved);
    assert!(h.state_mut().globe.map.bands.add(250.0).is_some());
    let added = frame(&mut h);
    assert_ne!(added, removed);
}

#[test]
fn the_bar_adds_moves_and_removes_a_band_limit() {
    let mut h = harness();
    h.get_by_label("Elevation").click();
    h.run_steps(2);
    let count = h.state().globe.map.bands.limits().len();
    let rect = h.get_by_label(BAR_LABEL).rect();
    let at = |meters: f64| Pos2::new(rect.left() + 20.0, limit_y(rect, meters));

    button(&mut h, egui::PointerButton::Primary, at(5000.0), true);
    h.step();
    button(&mut h, egui::PointerButton::Primary, at(5000.0), false);
    h.step();
    let limits = h.state().globe.map.bands.limits().to_vec();
    assert_eq!(limits.len(), count + 1);
    let added = *limits.last().unwrap();
    assert!((added - 5000.0).abs() < 80.0, "{added}");
    assert_eq!(h.state().globe.preview.selected, Some(count));

    button(&mut h, egui::PointerButton::Primary, at(added), true);
    h.step();
    for meters in [5300.0, 5600.0, 5800.0] {
        h.event(egui::Event::PointerMoved(at(meters)));
        h.step();
    }
    button(&mut h, egui::PointerButton::Primary, at(5800.0), false);
    h.step();
    let moved = *h.state().globe.map.bands.limits().last().unwrap();
    assert!((moved - 5800.0).abs() < 80.0, "{moved}");
    assert_eq!(h.state().globe.map.bands.limits().len(), count + 1);

    // Sea level stays.
    let sea = h.state().globe.map.bands.sea_index();
    button(&mut h, egui::PointerButton::Primary, at(0.0), true);
    h.step();
    h.event(egui::Event::PointerMoved(at(-3000.0)));
    h.step();
    button(&mut h, egui::PointerButton::Primary, at(-3000.0), false);
    h.step();
    assert_eq!(h.state().globe.map.bands.limits()[sea], 0.0);
    assert_eq!(h.state().globe.preview.selected, None);

    button(&mut h, egui::PointerButton::Primary, at(moved), true);
    h.step();
    button(&mut h, egui::PointerButton::Primary, at(moved), false);
    h.step();
    assert_eq!(h.state().globe.preview.selected, Some(count));
    h.get_by_label("Remove").click();
    h.step();
    assert_eq!(h.state().globe.map.bands.limits().len(), count);
}

/// A level for a texel with no relation to the texels next to it.
fn noise(face: usize, x: usize, y: usize) -> u16 {
    let mut v = (face as u32 * 73_856_093) ^ (x as u32 * 19_349_663) ^ (y as u32 * 83_492_791);
    v ^= v >> 13;
    v = v.wrapping_mul(0x5bd1_e995);
    (v ^ (v >> 15)) as u16
}

#[test]
fn no_seam_shows_at_a_face_edge_or_at_a_cube_corner() {
    const N: usize = 16;
    const ZOOM: f64 = 15.0;
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.set_face_size(N);
    globe.preview.graticule = false;
    globe.preview.greyscale = true;
    for face in 0..6 {
        for y in 0..N {
            for x in 0..N {
                globe.map.set(face, x, y, noise(face, x, y));
            }
        }
    }
    // The 8 corners and the middles of the 12 edges.
    let mut places = Vec::new();
    for x in [-1.0, 0.0, 1.0f64] {
        for y in [-1.0, 0.0, 1.0f64] {
            for z in [-1.0, 0.0, 1.0f64] {
                if x.abs() + y.abs() + z.abs() >= 2.0 {
                    places.push([x, y, z]);
                }
            }
        }
    }
    assert_eq!(places.len(), 20);
    // The filter changes the height by one texel step over one texel at most.
    // A seam is a jump of a large part of a texel step in one pixel.
    for [x, y, z] in places {
        let lat = z.atan2(x.hypot(y)).to_degrees();
        let lon = y.atan2(x).to_degrees();
        let globe = &mut h.state_mut().globe;
        globe.view = GlobeView::centered(lon, lat);
        globe.view.zoom = ZOOM;
        let img = canvas_image(&mut h);
        let texel = h.state().globe.view.radius(h.state().globe.rect) * std::f64::consts::FRAC_PI_2
            / N as f64;
        let limit = (3.0 * 255.0 / texel).ceil() as u8 + 1;
        assert!(limit < 12, "{limit}");
        let mut seen = [u8::MAX, 0];
        for py in 0..img.height() - 1 {
            for px in 0..img.width() - 1 {
                let v = img.get_pixel(px, py).0[0];
                seen = [seen[0].min(v), seen[1].max(v)];
                for other in [img.get_pixel(px + 1, py), img.get_pixel(px, py + 1)] {
                    let step = v.abs_diff(other.0[0]);
                    assert!(step <= limit, "{step} at {px}, {py}, place {:?}", [x, y, z]);
                }
            }
        }
        assert!(seen[1] - seen[0] > 60, "{seen:?}");
    }
}

fn pen_event(phase: PenPhase, pos: Pos2, time: f64) -> PenEvent {
    let hovers = matches!(phase, PenPhase::Hover | PenPhase::HoverEnd);
    PenEvent {
        phase,
        pos: [pos.x, pos.y],
        force: if hovers { 0.0 } else { 0.5 },
        altitude: Some(1.0),
        azimuth: Some(2.0),
        height: hovers.then_some(0.4),
        time,
    }
}

/// A harness in the brush tool, with a pen queue as on iPad.
fn queue_harness() -> (Harness<'static, AppState>, PenQueue) {
    let mut h = gpu_harness();
    let queue = PenQueue::default();
    h.state_mut().globe.pen.queue = Some(queue.clone());
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(1);
    (h, queue)
}

#[test]
fn the_debug_panel_shows_240_samples_per_second_for_a_pen_from_the_queue() {
    let _gpu = one_gpu_test();
    let (mut h, queue) = queue_harness();
    h.state_mut().globe.debug = true;
    h.run_steps(2);
    assert!(h.query_by_label("Pen and timing").is_some());
    assert!(h.query_by_label("Pen source: UIKit").is_some());
    assert!(h.query_by_label("Last stroke: none").is_some());
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    let view = h.state().globe.view;
    let base = h.state().globe.map.base();
    let under = place(&h, c);

    // Each frame has two samples in the queue and one touch in egui, as on
    // iPad at 120 frames per second.
    let at = |i: u32| from.lerp(to, i as f32 / 40.0);
    let time = |i: u32| 500.0 + f64::from(i) / 240.0;
    queue.push([pen_event(PenPhase::Down, from, time(0))]);
    first_touch(&mut h, egui::TouchPhase::Start, from, Some(0.5));
    h.step();
    for i in (1..40).step_by(2) {
        queue.push([
            pen_event(PenPhase::Move, at(i), time(i)),
            pen_event(PenPhase::Move, at(i + 1), time(i + 1)),
        ]);
        first_touch(&mut h, egui::TouchPhase::Move, at(i + 1), Some(0.5));
        h.step();
    }
    queue.push([pen_event(PenPhase::Up, to, time(41))]);
    first_touch(&mut h, egui::TouchPhase::End, to, Some(0.5));
    settle(&mut h);
    h.run_steps(2);

    let globe = &h.state().globe;
    assert!((globe.pen.stats.rate().unwrap() - 240.0).abs() < 1e-6);
    assert_eq!(globe.view, view);
    assert!(globe.map.sample(under) > base);
    // The touches in egui did not start a second stroke.
    assert_eq!(globe.stats.last().unwrap().id, 1);
    assert!(
        h.query_by_label("Last stroke: 42 samples, 240 per second")
            .is_some()
    );
    assert!(h.query_by_label("Pen force: 0.50").is_some());
    assert!(h.query_by_label_contains("Tilt: 57°").is_some());
}

#[test]
fn the_brush_circle_follows_a_pen_that_hovers() {
    let _gpu = one_gpu_test();
    let (mut h, queue) = queue_harness();
    assert_eq!(h.state().globe.input.pos, None);
    let c = h.state().globe.rect.center();
    for (i, pos) in [c, c + Vec2::new(30.0, -20.0)].into_iter().enumerate() {
        queue.push([pen_event(PenPhase::Hover, pos, i as f64)]);
        h.step();
        assert_eq!(h.state().globe.input.pos, Some(pos));
    }
    assert!(h.state().cursor_lonlat.is_some());
    assert_eq!(
        h.state().globe.pen.stats.hover_line(),
        "Hover: seen, height 0.40"
    );
    // The view did not move, and no stroke started.
    assert!(h.state().globe.stats.last().is_none());

    // A pen above the tool bar has no circle.
    queue.push([pen_event(PenPhase::Hover, Pos2::new(300.0, 10.0), 2.0)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, None);
    queue.push([pen_event(PenPhase::Hover, c, 3.0)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, Some(c));
    queue.push([pen_event(PenPhase::HoverEnd, c, 4.0)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, None);

    // A hover event at the start of a stroke arrives after the pen is down.
    // After the stroke, the circle does not go back to the start.
    let to = c + Vec2::new(40.0, 0.0);
    queue.push([
        pen_event(PenPhase::Down, c, 5.0),
        pen_event(PenPhase::Hover, c, 5.0),
    ]);
    h.step();
    queue.push([pen_event(PenPhase::Move, to, 5.1)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, Some(to));
    queue.push([pen_event(PenPhase::Up, to, 5.2)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, None);
    settle(&mut h);
    let near = to + Vec2::new(5.0, 5.0);
    queue.push([pen_event(PenPhase::Hover, near, 5.3)]);
    h.step();
    assert_eq!(h.state().globe.input.pos, Some(near));
}

#[test]
fn with_the_queue_a_finger_rotates_the_pen_paints_and_a_palm_does_nothing() {
    let _gpu = one_gpu_test();
    let (mut h, queue) = queue_harness();
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 20.0));

    let grabbed = place(&h, from);
    first_finger(&mut h, egui::TouchPhase::Start, from);
    h.step();
    first_finger(&mut h, egui::TouchPhase::Move, to);
    h.step();
    first_finger(&mut h, egui::TouchPhase::End, to);
    h.run_steps(2);
    assert!(off(&h, grabbed, to) < 0.05);
    assert!(h.state().globe.stats.last().is_none());

    let view = h.state().globe.view;
    let base = h.state().globe.map.base();
    let under = place(&h, c);
    queue.push([pen_event(PenPhase::Down, from, 1.0)]);
    first_touch(&mut h, egui::TouchPhase::Start, from, Some(0.5));
    h.step();
    // A palm comes down and moves.
    let palm = c + Vec2::new(60.0, 90.0);
    touch(&mut h, 7, egui::TouchPhase::Start, palm);
    queue.push([pen_event(PenPhase::Move, c, 1.01)]);
    first_touch(&mut h, egui::TouchPhase::Move, c, Some(0.5));
    h.step();
    touch(
        &mut h,
        7,
        egui::TouchPhase::Move,
        palm + Vec2::new(30.0, -50.0),
    );
    queue.push([pen_event(PenPhase::Up, to, 1.02)]);
    first_touch(&mut h, egui::TouchPhase::End, to, Some(0.5));
    h.step();
    touch(&mut h, 7, egui::TouchPhase::End, palm);
    settle(&mut h);

    let globe = &h.state().globe;
    assert_eq!(globe.view, view);
    assert_eq!(globe.nav.palms, 1);
    assert!(globe.map.sample(under) > base);
    assert_eq!(globe.stats.last().unwrap().id, 1);
}

/// A smooth value from -1 to 1 for each place, with no pattern.
fn value_noise(p: V3) -> f64 {
    let cell = p.map(f64::floor);
    let corner = |dx: f64, dy: f64, dz: f64| {
        let [x, y, z] = [cell[0] + dx, cell[1] + dy, cell[2] + dz].map(|v| v as i64 as u32);
        let mut v =
            x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791);
        v ^= v >> 13;
        v = v.wrapping_mul(0x5bd1_e995);
        f64::from((v ^ (v >> 15)) & 0xffff) / 32767.5 - 1.0
    };
    let [tx, ty, tz] = [0, 1, 2].map(|i| {
        let t = p[i] - cell[i];
        t * t * (3.0 - 2.0 * t)
    });
    let mix = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let plane = |dz: f64| {
        let low = mix(corner(0.0, 0.0, dz), corner(1.0, 0.0, dz), tx);
        let high = mix(corner(0.0, 1.0, dz), corner(1.0, 1.0, dz), tx);
        mix(low, high, ty)
    };
    mix(plane(0.0), plane(1.0), tz)
}

/// Hills of 4 sizes, from -1 to 1. The largest hills are `1 / scale` radians
/// wide.
fn hills(d: V3, scale: f64) -> f64 {
    let octave = |i: i32| value_noise(d.map(|v| v * scale * f64::from(1 << i))) / f64::from(1 << i);
    (0..4).map(octave).sum::<f64>() / 1.875
}

/// The elevation of an island with hills around `center`, in meters. The
/// coast is about 0.9 radians from the center.
fn island(d: V3, center: V3) -> f64 {
    let cone = 2600.0 * (1.0 - angle(d, center) / 0.9);
    (cone + 900.0 * hills(d, 5.0)).max(-2500.0)
}

/// Puts the island in the middle of the view of `centered(0, 0)`.
fn set_island(map: &mut vale_terrain::Heightmap) {
    let center = lonlat_to_dir(0.0, 0.0);
    let n = map.face_size();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let meters = island(map.texel_dir(face, x, y), center);
                map.set(face, x, y, meters_to_level(meters));
            }
        }
    }
}

/// The number of pixels of the canvas that show a river on land.
fn river_pixels(h: &Harness<'static, AppState>, img: &image::RgbaImage) -> usize {
    let globe = &h.state().globe;
    let land = meters_to_level(150.0);
    let min = globe.rect.min.ceil();
    let pixels = img.enumerate_pixels().filter(|(x, y, p)| {
        let [r, g, b, _] = p.0.map(i32::from);
        let pos = min + Vec2::new(*x as f32 + 0.5, *y as f32 + 0.5);
        let on_land = globe
            .unproject(pos)
            .is_some_and(|dir| globe.map.sample(dir) >= land);
        on_land && b > r + 60 && b > g + 30
    });
    pixels.count()
}

fn save(img: &image::RgbaImage, name: &str) {
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save(format!("../../target/app/{name}.png")).unwrap();
}

#[test]
fn rivers_show_on_the_globe_and_a_switch_hides_them() {
    let _gpu = one_gpu_test();
    let mut h = preview_harness();
    set_island(&mut h.state_mut().globe.map);
    let on = canvas_image(&mut h);
    save(&on, "rivers-on");
    assert!(h.state().globe.channels().unwrap().has_rivers());
    let count = river_pixels(&h, &on);
    assert!(count > 300, "{count}");

    h.get_by_label("Rivers").click();
    h.run_steps(2);
    assert!(!h.state().globe.preview.rivers);
    let off = canvas_image(&mut h);
    save(&off, "rivers-off");
    assert_eq!(river_pixels(&h, &off), 0);
}

#[test]
fn the_flow_map_follows_each_stroke_and_each_undo() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.map.fill(meters_to_level(-100.0));
    globe.tool = Tool::Brush;
    globe.brush.strength_m = 4000.0;
    globe.brush.size_points = 60.0;
    h.run_steps(2);
    assert!(!h.state().globe.channels().unwrap().has_rivers());

    // The stroke makes a ridge above the sea. Its sides have a slope.
    let c = mouse_stroke(&mut h);
    settle(&mut h);
    let globe = &h.state().globe;
    assert!(globe.map.sample(place(&h, c)) > meters_to_level(0.0));
    assert!(globe.channels().unwrap().has_rivers());

    assert!(h.state_mut().globe.undo());
    h.run_steps(2);
    assert!(!h.state().globe.channels().unwrap().has_rivers());
}

/// Runs frames until the rivers of the last change are on the GPU.
fn settle_rivers(h: &mut Harness<'static, AppState>) {
    for _ in 0..4000 {
        h.step();
        if !h.state().globe.rivers_pending() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the rivers did not arrive");
}

#[test]
fn painting_does_not_wait_for_the_flow_map() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.set_rivers_sync(false);
    globe.view = GlobeView::centered(0.0, 0.0);
    set_island(&mut globe.map);
    globe.tool = Tool::Brush;
    h.step();
    settle_rivers(&mut h);
    let rivers =
        |h: &Harness<'static, AppState>| h.state().globe.channels().unwrap().bytes().to_vec();
    let before = rivers(&h);
    assert!(h.state().globe.channels().unwrap().has_rivers());

    let c = mouse_stroke(&mut h);
    let under = place(&h, c);
    let level = h.state().globe.map.sample(under);
    settle(&mut h);
    // The stroke is complete, and its rivers are not.
    let globe = &h.state().globe;
    assert!(globe.map.sample(under) > level);
    assert!(globe.can_undo());
    assert!(globe.rivers_pending());
    assert_eq!(rivers(&h), before);

    settle_rivers(&mut h);
    assert_ne!(rivers(&h), before);
}

#[test]
fn carve_cuts_a_valley_along_a_river_and_stays_above_the_sea() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.preview.graticule = false;
    set_island(&mut globe.map);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Carve;
    globe.brush.size_points = 80.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);
    save(&canvas_image(&mut h), "carve-before");

    let map = &h.state().globe.map;
    let n = map.face_size();
    let texels = || (0..6 * n * n).map(|i| (i / (n * n), i % n, i / n % n));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<u16> {
        texels()
            .map(|(face, x, y)| map.get(face, x as i64, y as i64))
            .collect()
    };
    let before = levels(map);
    // The rivers of the app are the rivers of the whole heightmap.
    let channels = channel_map(&CoarseHeights::new(map));
    assert!(channels.bytes() == h.state().globe.channels().unwrap().bytes());

    mouse_stroke(&mut h);
    settle(&mut h);
    save(&canvas_image(&mut h), "carve-after");
    let after = levels(&h.state().globe.map);
    let sea = meters_to_level(0.0);
    let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / n as f64 - 0.5;
    let (mut on_river, mut cut) = (0, 0);
    for (i, (face, x, y)) in texels().enumerate() {
        let (old, new) = (before[i], after[i]);
        let (distance, flow) = channels.sample(face, at(x), at(y));
        assert!(new <= old, "{new} over {old}");
        assert!(new >= old.min(sea), "{new} under the sea from {old}");
        // The valley of the largest river is 6 channel texels wide on each side.
        if flow == 0 || distance > 6.1 {
            assert_eq!(new, old, "far from a river, at {distance}");
        }
        if distance < 0.5 && flow > 0 && new < old {
            on_river += 1;
        }
        if new + 20 < old {
            cut += 1;
        }
    }
    assert!(on_river > 50, "{on_river}");
    assert!(cut > 200, "{cut}");
}

#[test]
fn the_brush_panel_has_the_carve_mode() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    h.get_by_label("Carve").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);
}

/// The elevation of a world with continents, in meters.
fn continents(d: V3) -> f64 {
    let wave = |a: f64, b: f64, c: f64, phase: f64| (a * d[0] + b * d[1] + c * d[2] + phase).sin();
    let broad = wave(2.1, 1.3, -0.7, 0.4) + 0.8 * wave(-1.2, 2.6, 1.9, 2.0);
    (1300.0 * broad + 1500.0 * hills(d, 6.0) - 300.0).max(-2500.0)
}

/// Writes images of both layouts with rivers, for a person to look at. The
/// last image is near a cube edge.
#[test]
#[ignore]
fn screenshots_with_rivers() {
    use vale_app::ui::Layout;
    let _gpu = one_gpu_test();
    for (layout, size, zoom, name) in [
        (Layout::Desktop, (1440.0, 900.0), 1.0, "rivers-desktop"),
        (Layout::Pad, (1194.0, 834.0), 1.0, "rivers-pad"),
        (Layout::Desktop, (1440.0, 900.0), 6.0, "rivers-zoom"),
    ] {
        let mut state = state();
        state.set_layout(layout);
        state.globe.tool = Tool::Brush;
        state.globe.brush.mode = Mode::Carve;
        if zoom > 1.0 {
            state.globe.view = GlobeView::centered(45.0, 20.0);
            state.globe.view.zoom = zoom;
        }
        let map = &mut state.globe.map;
        let n = map.face_size();
        for face in 0..6 {
            for y in 0..n {
                for x in 0..n {
                    let meters = continents(map.texel_dir(face, x, y));
                    map.set(face, x, y, meters_to_level(meters));
                }
            }
        }
        let (img, state) = headless::ui_png(state, size, 1.0).unwrap();
        assert!(state.globe.channels().is_some_and(ChannelMap::has_rivers));
        save(&img, name);
    }
}
