use clap::Parser;
use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::cli::{Args, apply_globe, apply_globe_view};
use vale_app::document::Document;
use vale_app::globe::brush::slerp;
use vale_app::globe::math::{V3, angle, lonlat_to_dir};
use vale_app::globe::view::GlobeView;
use vale_app::globe::{FACE_SIZE, Tool};
use vale_app::headless;
use vale_app::pen::{PenEvent, PenPhase, PenQueue};
use vale_app::ui::elevation::{BAR_LABEL, limit_y};
use vale_app::ui::{AppState, Workspace, draw};
use vale_import::geojson;
use vale_style::{Color, Stroke};
use vale_terrain::{
    ChannelMap, CoarseHeights, FlowMap, Limits, Mode, WindowHeights, channel_map, level_to_meters,
    meters_to_level, window_channels,
};

/// The sample world with its vector layers hidden, so that each place of the
/// globe shows the heightmap.
fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    let ids: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in ids {
        state.doc.set_visible(id, false);
    }
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

#[test]
fn the_globe_draws_vector_layers_with_great_circle_edges_up_to_its_edge() {
    let _gpu = one_gpu_test();
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.globe.view = GlobeView::centered(0.0, 0.0);
    // A polygon that goes around the edge of the globe at longitude 90, a
    // line with one long edge, and a point on each side of the globe.
    let ids: Vec<_> = [
        r#"{"type":"Polygon","coordinates":[[[60,-30],[120,-30],[120,30],[60,30],[60,-30]]]}"#,
        r#"{"type":"LineString","coordinates":[[-50,40],[50,40]]}"#,
        r#"{"type":"MultiPoint","coordinates":[[-40,-20],[170,-20]]}"#,
    ]
    .into_iter()
    .map(|text| {
        state
            .doc
            .add_imported(geojson::read_str("shapes", text).unwrap(), None)[0]
    })
    .collect();
    let (red, green, blue) = (
        Color::rgb(255, 0, 0),
        Color::rgb(0, 255, 0),
        Color::rgb(0, 0, 255),
    );
    for (id, fill, stroke) in [
        (ids[0], Some(red), None),
        (ids[1], None, Some(green)),
        (ids[2], Some(blue), None),
    ] {
        let style = &mut state.doc.entry_mut(id).unwrap().style;
        style.fill = fill;
        style.stroke = stroke.map(|color| Stroke { color, width: 4.0 });
        style.point_radius = 5.0;
    }
    let hidden = state.doc.add_imported(
        geojson::read_str(
            "hidden",
            r#"{"type":"Polygon","coordinates":[[[-20,-70],[20,-70],[20,-50],[-20,-50],[-20,-70]]]}"#,
        )
        .unwrap(),
        None,
    )[0];
    state.doc.entry_mut(hidden).unwrap().style.fill = Some(red);
    state.doc.set_visible(hidden, false);

    let (img, state) = headless::ui_png(state, (1280.0, 800.0), 1.0).unwrap();
    img.save("../../target/app/test-globe-vectors.png").unwrap();
    let (rect, view) = (state.globe.rect, state.globe.view);
    let pixel = |p: Pos2| {
        let [r, g, b, _] = img.get_pixel(p.x as u32, p.y as u32).0;
        [r, g, b]
    };
    let at = |lon: f64, lat: f64| pixel(view.project(rect, lonlat_to_dir(lon, lat)).unwrap());
    let radius = view.radius(rect) as f32;
    let edge = |dx: f32, dy: f32| pixel(rect.center() + Vec2::new(dx, dy));

    // The fill goes to the edge of the globe and stops there.
    assert_eq!(at(75.0, 0.0), [255, 0, 0]);
    assert_eq!(edge(radius - 3.0, 0.0), [255, 0, 0]);
    let outside = edge(radius + 3.0, 0.0);
    assert!(outside[0] < 60, "{outside:?}");
    assert_ne!(at(50.0, 0.0), [255, 0, 0]);
    // The side of the polygon at latitude 30 is a great circle, so it is
    // north of that latitude in the middle.
    assert_eq!(at(75.0, 30.5), [255, 0, 0]);

    // The line is a great circle and not a line of latitude.
    let middle = slerp(lonlat_to_dir(-50.0, 40.0), lonlat_to_dir(50.0, 40.0), 0.5);
    assert_eq!(pixel(view.project(rect, middle).unwrap()), [0, 255, 0]);
    assert_ne!(at(0.0, 40.0), [0, 255, 0]);

    assert_eq!(at(-40.0, -20.0), [0, 0, 255]);
    // The hidden layer is not there.
    assert_ne!(at(0.0, -60.0), [255, 0, 0]);
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

/// The largest step of the grey level from a pixel to the pixel next to it,
/// and the range of the grey levels.
fn largest_step(img: &image::RgbaImage) -> (u8, u8) {
    let (mut step, mut seen) = (0, [u8::MAX, 0]);
    for py in 0..img.height() - 1 {
        for px in 0..img.width() - 1 {
            let v = img.get_pixel(px, py).0[0];
            seen = [seen[0].min(v), seen[1].max(v)];
            for other in [img.get_pixel(px + 1, py), img.get_pixel(px, py + 1)] {
                step = step.max(v.abs_diff(other.0[0]));
            }
        }
    }
    (step, seen[1] - seen[0])
}

/// The middle of face 0 is a corner of four tiles. The face +z has no tile.
#[test]
fn no_seam_shows_at_a_tile_edge_at_each_zoom() {
    const N: usize = 2 * vale_terrain::TILE_SIZE;
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.set_face_size(N);
    globe.preview.graticule = false;
    globe.preview.greyscale = true;
    // A slope from the south pole to the north pole. The face +z has the
    // level of the north pole, which is the base level.
    let slope = |z: f64| (32768.0 + 24000.0 * z.min(0.55)) as u16;
    globe.map.fill(slope(1.0));
    for face in [0, 1, 2, 3, 5] {
        for y in 0..N {
            for x in 0..N {
                let level = slope(globe.map.texel_dir(face, x, y)[2]);
                globe.map.set(face, x, y, level);
            }
        }
    }
    assert_eq!(globe.map.allocated_tiles(), 5 * 4);
    // The whole globe, then the tile corner, then the edge of the face with
    // no tile. The level is the same on the two sides of that edge.
    for (lon, lat, zoom, range) in [
        (0.0, 0.0, 1.0, 100),
        (20.0, 30.0, 1.0, 70),
        (0.0, 0.0, 8.0, 20),
        (0.0, 0.0, 60.0, 2),
        (0.0, 45.0, 8.0, 0),
    ] {
        let globe = &mut h.state_mut().globe;
        globe.view = GlobeView::centered(lon, lat);
        globe.view.zoom = zoom;
        let img = canvas_image(&mut h);
        // The part of the canvas that is well inside the edge of the globe.
        let radius = h.state().globe.view.radius(h.state().globe.rect);
        let half = ((0.6 * radius) as u32).min(img.width().min(img.height()) / 2 - 1);
        let (x, y) = (img.width() / 2 - half, img.height() / 2 - half);
        let img = image::imageops::crop_imm(&img, x, y, 2 * half, 2 * half).to_image();
        let (step, seen) = largest_step(&img);
        // The slope gives a step of 1 grey level at most.
        assert!(step <= 2, "a step of {step} at {lon}, {lat}, zoom {zoom}");
        assert!(
            seen >= range,
            "a range of {seen} at {lon}, {lat}, zoom {zoom}"
        );
    }
    // Noise in the four tiles at the corner. A texel is many pixels wide, and
    // the filter changes the height by one texel step over one texel at most.
    let globe = &mut h.state_mut().globe;
    for y in N / 2 - 8..N / 2 + 8 {
        for x in N / 2 - 8..N / 2 + 8 {
            globe.map.set(0, x, y, noise(0, x % 16, y % 16));
        }
    }
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.view.zoom = 480.0;
    let img = canvas_image(&mut h);
    let texel =
        h.state().globe.view.radius(h.state().globe.rect) * std::f64::consts::FRAC_PI_2 / N as f64;
    let limit = (3.0 * 255.0 / texel).ceil() as u8 + 1;
    assert!(limit < 12, "{limit}");
    let (step, seen) = largest_step(&img);
    assert!(step <= limit, "a step of {step} in the noise");
    assert!(seen > 60, "{seen}");
}

/// Sets each texel of some faces to noise.
fn set_noise(h: &mut Harness<'static, AppState>, faces: &[usize], seed: usize) {
    let map = &mut h.state_mut().globe.map;
    let n = map.face_size();
    for &face in faces {
        for y in 0..n {
            for x in 0..n {
                // The noise takes small numbers only.
                let block = (x / 16 * 16 + y / 16) as u16;
                let level = noise(face + seed, x % 16, y % 16) ^ block.wrapping_mul(251);
                map.set(face, x, y, level);
            }
        }
    }
}

#[test]
fn a_large_change_reaches_the_gpu_in_parts() {
    let _gpu = one_gpu_test();
    let all = [0, 1, 2, 3, 4, 5];
    // The first and the last face change again while strips wait.
    let again = [0, 5];
    let mut parts = gpu_harness();
    parts.state_mut().globe.uploads.budget = 16 * BRUSH_FACE_SIZE;
    set_noise(&mut parts, &all, 0);
    parts.step();
    assert!(parts.state().globe.busy());
    assert!(!parts.state().globe.can_undo());
    set_noise(&mut parts, &again, 6);
    let mut steps = 0;
    while parts.state().globe.busy() {
        parts.step();
        steps += 1;
        assert!(steps < 400, "the texels did not reach the GPU");
    }
    // Each frame sends two budgets, and the six faces are 96 budgets.
    assert!(steps >= 40, "{steps}");

    let mut whole = gpu_harness();
    set_noise(&mut whole, &all, 0);
    set_noise(&mut whole, &again, 6);
    whole.step();
    assert!(!whole.state().globe.busy());

    for (lon, lat) in [(15.0, 25.0), (-165.0, -25.0), (100.0, 80.0)] {
        for h in [&mut parts, &mut whole] {
            h.state_mut().globe.view = GlobeView::centered(lon, lat);
        }
        let (a, b) = (canvas_image(&mut parts), canvas_image(&mut whole));
        assert!(
            a.as_raw() == b.as_raw(),
            "the render differs at {lon}, {lat}"
        );
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

    assert!(h.state_mut().globe.redo());
    h.run_steps(2);
    assert!(h.state().globe.channels().unwrap().has_rivers());
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

#[test]
fn the_brush_panel_has_the_ridge_mode() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    h.get_by_label("Ridge").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Ridge);
}

/// The mean of the numbers, which are not empty.
fn mean(values: &[f64]) -> f64 {
    assert!(values.len() > 20, "{}", values.len());
    values.iter().sum::<f64>() / values.len() as f64
}

#[test]
fn ridge_raises_the_land_between_the_rivers_and_leaves_the_sea() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.preview.graticule = false;
    set_island(&mut globe.map);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Ridge;
    globe.brush.size_points = 80.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);
    save(&canvas_image(&mut h), "ridge-before");

    let map = &h.state().globe.map;
    let n = map.face_size();
    let texels = || (0..6 * n * n).map(|i| (i / (n * n), i % n, i / n % n));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<u16> {
        texels()
            .map(|(face, x, y)| map.get(face, x as i64, y as i64))
            .collect()
    };
    let before = levels(map);
    let channels = channel_map(&CoarseHeights::new(map));
    assert!(channels.bytes() == h.state().globe.channels().unwrap().bytes());

    mouse_stroke(&mut h);
    settle(&mut h);
    save(&canvas_image(&mut h), "ridge-after");
    let after = levels(&h.state().globe.map);
    let sea = meters_to_level(0.0);
    let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / n as f64 - 0.5;
    // The rise of each texel that the stroke raised, near a river and far
    // from each river.
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for (i, (face, x, y)) in texels().enumerate() {
        let (old, new) = (before[i], after[i]);
        assert!(new >= old, "{new} under {old}");
        if old <= sea {
            assert_eq!(new, old, "the sea floor");
        }
        if new == old {
            continue;
        }
        let (distance, _) = channels.sample(face, at(x), at(y));
        if distance < 0.5 {
            near.push(f64::from(new - old));
        } else if distance > 3.0 {
            far.push(f64::from(new - old));
        }
    }
    let (near, far) = (mean(&near), mean(&far));
    assert!(far > 2.0 * near, "{near} {far}");
}

#[test]
fn the_brush_panel_has_the_deposit_mode() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    h.get_by_label("Deposit").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Deposit);
}

#[test]
fn deposit_raises_low_ground_to_the_fill_level_and_the_sea_floor_to_sea_level_at_most() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.preview.graticule = false;
    set_island(&mut globe.map);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Deposit;
    globe.brush.size_points = 80.0;
    h.run_steps(2);
    save(&canvas_image(&mut h), "deposit-before");

    let map = &h.state().globe.map;
    let n = map.face_size();
    let texels = || (0..6 * n * n).map(|i| (i / (n * n), i % n, i / n % n));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<u16> {
        texels()
            .map(|(face, x, y)| map.get(face, x as i64, y as i64))
            .collect()
    };
    let before = levels(map);
    let channels = channel_map(&CoarseHeights::new(map));
    assert!(channels.fills() == h.state().globe.channels().unwrap().fills());

    mouse_stroke(&mut h);
    settle(&mut h);
    save(&canvas_image(&mut h), "deposit-after");
    let after = levels(&h.state().globe.map);
    let sea = meters_to_level(0.0);
    let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / n as f64 - 0.5;
    let mut raised = 0;
    for (i, (face, x, y)) in texels().enumerate() {
        let (old, new) = (before[i], after[i]);
        assert!(new >= old, "{new} under {old}");
        if old < sea {
            assert!(new <= sea, "the sea floor is at {new}");
        }
        if new == old {
            continue;
        }
        raised += 1;
        // The GPU stamp can differ from the CPU stamp by 1 level.
        let fill = channels.fill_level(face, at(x), at(y));
        assert!(f64::from(new) <= fill + 1.0, "{new} over {fill}");
    }
    assert!(raised > 100, "{raised}");
}

#[test]
fn ridge_in_a_near_view_reads_the_window() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.2, 14.0);
    let globe = &mut h.state_mut().globe;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Ridge;
    globe.brush.size_points = 80.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);

    // The texels of face 0 around the stroke.
    let texels = || (N / 4..3 * N / 4).flat_map(|y| (N / 4..3 * N / 4).map(move |x| (x, y)));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<u16> {
        texels()
            .map(|(x, y)| map.get(0, x as i64, y as i64))
            .collect()
    };
    let map = &h.state().globe.map;
    let before = levels(map);
    let w = h.state().globe.window().unwrap().window();
    assert_eq!(w.cell, 1);
    let flow = FlowMap::new(&CoarseHeights::new(map));
    let window = window_channels(&WindowHeights::new(map, w), &flow);
    assert!(window.bytes() == h.state().globe.window().unwrap().bytes());
    let channels = flow.channels();
    let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / N as f64 - 0.5;

    mouse_stroke(&mut h);
    settle(&mut h);
    let after = levels(&h.state().globe.map);
    // The rise of each texel that the stroke raised: near a river of the
    // window, far from each river of the window, and far from each river of
    // the window but near a river of the channel map.
    let (mut near, mut far, mut far_in_window) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (x, y)) in texels().enumerate() {
        let (old, new) = (before[i], after[i]);
        if new == old {
            continue;
        }
        assert!(new > old, "{new} under {old}");
        let (u, v) = w.channel_of_texel(x as i64, y as i64);
        let (distance, _) = window.lookup(u, v).expect("the window covers the stroke");
        let rise = f64::from(new - old);
        if distance < 0.5 {
            near.push(rise);
        } else if distance > 3.0 {
            far.push(rise);
            if channels.sample(0, at(x), at(y)).0 < 0.5 {
                far_in_window.push(rise);
            }
        }
    }
    let (near, far, far_in_window) = (mean(&near), mean(&far), mean(&far_in_window));
    assert!(far > 2.0 * near, "{near} {far}");
    assert!(far_in_window > 2.0 * near, "{near} {far_in_window}");
}

#[test]
fn the_brush_panel_has_the_erode_mode() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    h.get_by_label("Erode").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Erode);
}

#[test]
fn the_brush_panel_has_the_weather_mode_and_its_angle_slider() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Raise);
    assert!(h.query_all_by_label("Angle").next().is_none());
    h.get_by_label("Weather").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Weather);
    assert!(h.query_all_by_label("Angle").next().is_some());
    assert_eq!(h.state().globe.brush.talus_deg, 0.5);
}

#[test]
fn the_brush_panel_has_the_warp_mode_and_its_two_sliders() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    assert!(h.query_all_by_label("Scale").next().is_none());
    h.get_by_label("Warp").click();
    h.run_steps(2);
    let brush = &h.state().globe.brush;
    assert_eq!(brush.mode, Mode::Warp);
    assert_eq!((brush.warp_km, brush.scale_km), (50.0, 200.0));
    assert!(h.query_all_by_label("Strength").next().is_some());
    assert!(h.query_all_by_label("Scale").next().is_some());
    assert!(h.query_all_by_label("Seed").next().is_some());
}

#[test]
fn the_brush_panel_has_the_floor_and_the_ceiling() {
    use egui::accesskit::Role;
    for mode in ["Raise", "Erode", "Warp"] {
        let mut h = harness();
        h.state_mut().globe.tool = Tool::Brush;
        h.run_steps(2);
        h.get_by_label(mode).click();
        h.run_steps(2);
        // The elevations that take no input.
        let off = |h: &Harness<'static, AppState>| {
            let values = h.query_all_by_role(Role::SpinButton);
            values.filter(|v| v.accesskit_node().is_disabled()).count()
        };
        // Both limits are off at the start.
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && !brush.ceiling_on, "{mode}");
        assert_eq!((brush.floor_m, brush.ceiling_m), (0.0, 1000.0), "{mode}");
        assert_eq!(brush.limits(), Limits::default(), "{mode}");
        assert_eq!(off(&h), 2, "{mode}");

        h.get_by_label("Floor").click();
        h.run_steps(2);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && !brush.ceiling_on, "{mode}");
        assert_eq!(off(&h), 1, "{mode}");
        h.get_by_label("Ceiling").click();
        h.run_steps(2);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && brush.ceiling_on, "{mode}");
        assert_eq!(off(&h), 0, "{mode}");
        let (floor, ceiling) = (meters_to_level(0.0), meters_to_level(1000.0));
        assert_eq!(brush.limits(), Limits { floor, ceiling }, "{mode}");

        // A drag on the elevation of the ceiling changes it.
        let pos = {
            let values = h.query_all_by_role(Role::SpinButton);
            let mut value = values.filter(|v| v.accesskit_node().numeric_value() == Some(1000.0));
            value.next_back().unwrap().rect().center()
        };
        button(&mut h, egui::PointerButton::Primary, pos, true);
        h.step();
        for i in 1..=5 {
            let moved = pos + Vec2::new(8.0 * i as f32, 0.0);
            h.event(egui::Event::PointerMoved(moved));
            h.step();
        }
        let to = pos + Vec2::new(40.0, 0.0);
        button(&mut h, egui::PointerButton::Primary, to, false);
        h.run_steps(2);
        let brush = h.state().globe.brush;
        assert!(brush.ceiling_m > 1000.0, "{mode}: {}", brush.ceiling_m);
        assert_eq!(brush.floor_m, 0.0, "{mode}");
    }
}

/// The level of each texel of the heightmap.
fn map_levels(map: &vale_terrain::Heightmap) -> Vec<u16> {
    let n = map.face_size();
    let texels = (0..6 * n * n).map(|i| (i / (n * n), i % n, i / n % n));
    texels
        .map(|(face, x, y)| map.get(face, x as i64, y as i64))
        .collect()
}

#[test]
fn weather_lowers_a_steep_hill_and_one_undo_puts_it_back() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    // A texel of this world is 614 m wide, so the limit of 10 degrees is
    // 108 m for each texel.
    h.state_mut().doc.set_radius_km(100.0);
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Weather;
    globe.brush.talus_deg = 10.0;
    globe.brush.size_points = 40.0;
    // A cone of 3,000 m with a side of 375 m for each texel.
    let top = lonlat_to_dir(0.0, 0.0);
    let n = globe.map.face_size();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let part = 1.0 - angle(globe.map.texel_dir(face, x, y), top) / 0.05;
                if part > 0.0 {
                    globe.map.set(face, x, y, meters_to_level(3000.0 * part));
                }
            }
        }
    }
    h.run_steps(2);
    let before = map_levels(&h.state().globe.map);
    let peak = h.state().globe.map.sample(top);
    let sum = |levels: &[u16]| levels.iter().map(|&l| i64::from(l)).sum::<i64>();

    mouse_stroke(&mut h);
    settle(&mut h);
    let globe = &h.state().globe;
    assert_eq!(globe.world_km, 100.0);
    let after = map_levels(&globe.map);
    let lowered = globe.map.sample(top);
    assert!(lowered + 200 < peak, "{lowered} from {peak}");
    let (down, up) = before
        .iter()
        .zip(&after)
        .fold((0, 0), |(down, up), (b, a)| {
            (down + usize::from(a < b), up + usize::from(a > b))
        });
    assert!(down > 50 && up > 50, "{down} down, {up} up");
    // The ground moves, and its amount stays.
    assert_eq!(sum(&after), sum(&before));
    assert_eq!(globe.stats.last().unwrap().id, 1);
    std::fs::create_dir_all("../../target/app").unwrap();
    let frame = h.render().unwrap();
    frame
        .save("../../target/app/test-weather-panel.png")
        .unwrap();

    assert!(h.state_mut().globe.undo());
    assert!(map_levels(&h.state().globe.map) == before);
    assert!(!h.state().globe.can_undo());
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

#[test]
fn the_keyboard_shortcuts_undo_and_redo_a_stroke_of_each_mode() {
    use egui::{Key, Modifiers};
    use vale_terrain::{Mode, meters_to_level};
    let _gpu = one_gpu_test();
    let redo_keys = [
        (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
        (Modifiers::COMMAND, Key::Y),
    ];
    let modes = [Mode::Raise, Mode::Lower, Mode::Smooth, Mode::Flatten];
    for (i, mode) in modes.into_iter().enumerate() {
        let mut h = gpu_harness();
        h.state_mut().globe.tool = Tool::Brush;
        h.state_mut().globe.brush.flatten_level = Some(meters_to_level(3000.0));
        h.run_steps(1);
        let under = place(&h, h.state().globe.rect.center());
        // A hill gives the smooth mode a slope.
        mouse_stroke(&mut h);
        settle(&mut h);
        let before = (h.state().globe.map.sample(under), canvas_image(&mut h));
        h.state_mut().globe.brush.mode = mode;
        mouse_stroke(&mut h);
        settle(&mut h);
        let after = (h.state().globe.map.sample(under), canvas_image(&mut h));
        assert_ne!(after.0, before.0, "{mode:?}");
        assert!(!h.state().globe.can_redo(), "{mode:?}");

        let check = |h: &mut Harness<'static, AppState>, want: &(u16, image::RgbaImage)| {
            assert_eq!(h.state().globe.map.sample(under), want.0, "{mode:?}");
            let image = canvas_image(h);
            assert!(image.as_raw() == want.1.as_raw(), "{mode:?}: the render");
        };
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.step();
        check(&mut h, &before);
        assert!(h.state().globe.can_undo() && h.state().globe.can_redo());
        let (modifiers, key) = redo_keys[i % 2];
        h.key_press_modifiers(modifiers, key);
        h.step();
        check(&mut h, &after);
        assert!(!h.state().globe.can_redo(), "{mode:?}");

        // Two steps go back to the empty world.
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.step();
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.step();
        assert!(!h.state().globe.can_undo(), "{mode:?}");
        assert_eq!(h.state().globe.map.allocated_tiles(), 0, "{mode:?}");
    }
}

#[test]
fn a_shortcut_does_nothing_during_a_stroke() {
    let _gpu = one_gpu_test();
    let mut h = gpu_harness();
    h.state_mut().globe.tool = Tool::Brush;
    let c = mouse_stroke(&mut h);
    settle(&mut h);
    let under = place(&h, c);
    let painted = h.state().globe.map.sample(under);
    button(&mut h, egui::PointerButton::Primary, c, true);
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.step();
    assert!(h.state().globe.busy());
    button(&mut h, egui::PointerButton::Primary, c, false);
    settle(&mut h);
    assert!(h.state().globe.map.sample(under) >= painted);
    assert!(!h.state().globe.can_redo());
}

/// Puts an island with small hills at longitude 0 and latitude 0, in the
/// middle of face 0. The coast is about `radius` radians from the middle.
fn set_small_island(map: &mut vale_terrain::Heightmap, radius: f64) {
    let center = lonlat_to_dir(0.0, 0.0);
    let n = map.face_size();
    // The sea floor is at the base level from 1.4 times the radius.
    let half = ((1.4 * radius / std::f64::consts::FRAC_PI_2 * n as f64) as usize).min(n / 2);
    for y in n / 2 - half..n / 2 + half {
        for x in n / 2 - half..n / 2 + half {
            let d = map.texel_dir(0, x, y);
            let slope = 1.0 - angle(d, center) / radius;
            let cone = 2600.0 * slope * if slope < 0.0 { 4.0 } else { 1.0 };
            let meters = cone + 900.0 * hills(d, 5.0 / radius);
            if meters > -2500.0 {
                map.set(0, x, y, meters_to_level(meters));
            }
        }
    }
}

/// The part of the ground in the middle of the window that is nearer to a
/// river than one channel texel of the channel map. The first number is for
/// the rivers of the window, and the second number is for the rivers of the
/// channel map. Each part goes up with the length of the rivers on that
/// ground.
fn river_parts(globe: &vale_app::globe::Globe) -> (f64, f64) {
    let (window, channels) = (globe.window().unwrap(), globe.channels().unwrap());
    let w = window.window();
    // The size of a channel texel of the map in channel texels of the window.
    let scale = (2 * w.face_size) as f64 / (channels.size() * w.cell) as f64;
    let (mut fine, mut coarse, mut all) = (0, 0, 0);
    for v in (64..window.size() - 64).step_by(2) {
        for u in (64..window.size() - 64).step_by(2) {
            all += 1;
            let [distance, flow] = window.texel(u, v);
            fine += usize::from(flow > 0 && f64::from(distance) <= 32.0 * scale);
            let d = w.dir((u as f64 + 0.5) / 2.0, (v as f64 + 0.5) / 2.0);
            let (distance, flow) = channels.at(d);
            coarse += usize::from(flow > 0 && distance <= 1.0);
        }
    }
    (fine as f64 / all as f64, coarse as f64 / all as f64)
}

/// A harness with the small island in the middle of the view. With a face
/// size of 1024 at least, a near view has a window of small rivers.
fn island_harness(face_size: usize, radius: f64, zoom: f64) -> Harness<'static, AppState> {
    let mut state = state();
    let globe = &mut state.globe;
    globe.set_face_size(face_size);
    globe.preview.graticule = false;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.view.zoom = zoom;
    set_small_island(&mut globe.map, radius);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (640.0, 480.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

#[test]
fn zooming_in_gives_finer_rivers() {
    let _gpu = one_gpu_test();
    let mut h = island_harness(1024, 0.35, 8.0);
    let globe = &h.state().globe;
    let window = globe.window().expect("a near view has a window");
    assert_eq!(window.window().cell, 1);
    assert!(window.has_rivers());
    // The window has more length of rivers on the same ground.
    let (fine, coarse) = river_parts(globe);
    assert!(coarse > 0.0 && fine > 1.5 * coarse, "{fine} and {coarse}");
    let img = canvas_image(&mut h);
    save(&img, "rivers-near");
    let count = river_pixels(&h, &img);
    assert!(count > 300, "{count}");
}

#[test]
fn a_far_view_draws_the_global_rivers() {
    let _gpu = one_gpu_test();
    let mut h = island_harness(1024, 0.35, 1.0);
    assert!(h.state().globe.window().is_none());
    assert!(h.state().globe.channels().unwrap().has_rivers());
    let img = canvas_image(&mut h);
    let count = river_pixels(&h, &img);
    assert!(count > 50, "{count}");

    // The window goes away when the view goes back.
    h.state_mut().globe.view.zoom = 8.0;
    h.run_steps(2);
    assert!(h.state().globe.window().is_some());
    h.state_mut().globe.view.zoom = 1.0;
    h.run_steps(2);
    assert!(h.state().globe.window().is_none());
    let img = canvas_image(&mut h);
    assert_eq!(river_pixels(&h, &img), count);
}

#[test]
fn carve_in_a_near_view_cuts_a_narrow_valley() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.2, 14.0);
    let globe = &mut h.state_mut().globe;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Carve;
    globe.brush.size_points = 80.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);

    // The texels of face 0 around the stroke.
    let texels = || (N / 4..3 * N / 4).flat_map(|y| (N / 4..3 * N / 4).map(move |x| (x, y)));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<u16> {
        texels()
            .map(|(x, y)| map.get(0, x as i64, y as i64))
            .collect()
    };
    let map = &h.state().globe.map;
    let before = levels(map);
    let w = h.state().globe.window().unwrap().window();
    assert_eq!(w.cell, 1);
    // The window of the app has the rivers of the heightmap.
    let flow = FlowMap::new(&CoarseHeights::new(map));
    let window = window_channels(&WindowHeights::new(map, w), &flow);
    assert!(window.bytes() == h.state().globe.window().unwrap().bytes());
    // A channel texel of the map is this number of channel texels of the
    // window. A valley has the same width in channel texels in both.
    let channels = h.state().globe.channels().unwrap().size();
    let ratio = (2 * N) as f64 / (channels * w.cell) as f64;
    assert!(ratio >= 4.0, "{ratio}");

    mouse_stroke(&mut h);
    settle(&mut h);
    let after = levels(&h.state().globe.map);
    let (mut cut, mut widest) = (0, 0.0f64);
    for (i, (x, y)) in texels().enumerate() {
        let (old, new) = (before[i], after[i]);
        if new == old {
            continue;
        }
        assert!(new < old, "{new} over {old}");
        let (u, v) = w.channel_of_texel(x as i64, y as i64);
        let (distance, _) = window.lookup(u, v).expect("the window covers the stroke");
        // The texel is in the valley of a river.
        assert!(
            window.valley(u, v).unwrap() > 0.0,
            "{distance} from a river"
        );
        widest = widest.max(distance);
        cut += 1;
    }
    assert!(cut > 100, "{cut}");
    // The widest valley of the channel map is 6 of its channel texels wide
    // on each side.
    assert!(widest * 4.0 <= 6.0 * ratio, "{widest}");
}

/// A harness with the island in the middle of a far view, and the erode
/// brush.
fn erode_harness() -> Harness<'static, AppState> {
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.preview.graticule = false;
    set_island(&mut globe.map);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Erode;
    globe.brush.size_points = 80.0;
    h.run_steps(2);
    h
}

/// The level of each texel of the heightmap.
fn all_levels(h: &Harness<'static, AppState>) -> Vec<u16> {
    let map = &h.state().globe.map;
    let n = map.face_size();
    let texels = (0..6 * n * n).map(|i| (i / (n * n), i % n, i / n % n));
    texels
        .map(|(face, x, y)| map.get(face, x as i64, y as i64))
        .collect()
}

/// The sum of the levels that the ground went down.
fn lowered(before: &[u16], after: &[u16]) -> u64 {
    let drops = before.iter().zip(after).map(|(&old, &new)| {
        assert!(new <= old, "{new} over {old}");
        u64::from(old - new)
    });
    drops.sum()
}

/// Holds the primary mouse button at one place for a number of frames. The
/// mouse does not move.
fn hold(h: &mut Harness<'static, AppState>, pos: Pos2, frames: usize) {
    button(h, egui::PointerButton::Primary, pos, true);
    h.run_steps(frames);
    button(h, egui::PointerButton::Primary, pos, false);
    h.step();
    settle(h);
}

#[test]
fn a_held_erode_brush_repeats_the_step() {
    let _gpu = one_gpu_test();
    let mut h = erode_harness();
    let before = all_levels(&h);
    let pos = h.state().globe.rect.center() + Vec2::new(60.0, 20.0);

    hold(&mut h, pos, 5);
    let short = lowered(&before, &all_levels(&h));
    assert!(h.state_mut().globe.undo());
    h.run_steps(2);
    assert!(all_levels(&h) == before);

    hold(&mut h, pos, 20);
    let long = lowered(&before, &all_levels(&h));
    assert!(short > 0, "{short}");
    assert!(long > 2 * short, "{long} after {short}");
}

#[test]
fn erode_in_a_far_view_lowers_the_land_and_the_sea_floor() {
    let _gpu = one_gpu_test();
    let mut h = erode_harness();
    save(&canvas_image(&mut h), "erode-before");
    let before = all_levels(&h);
    // A far view has no window, so the cells are the cells of the global map.
    assert!(h.state().globe.window().is_none());

    // The stroke goes from the land to the sea.
    let c = h.state().globe.rect.center();
    let reach = 0.88 * h.state().globe.radius() as f32;
    let (from, to) = (c, c + Vec2::new(reach, 0.0));
    assert!(h.state().globe.map.sample(place(&h, to)) < meters_to_level(0.0));
    button(&mut h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=40 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 40.0)));
        h.step();
    }
    button(&mut h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(&mut h);
    save(&canvas_image(&mut h), "erode-after");

    let after = all_levels(&h);
    let sea = meters_to_level(0.0);
    let map = &h.state().globe.map;
    let n = map.face_size();
    let radius = h.state().globe.brush.radius(h.state().globe.radius());
    let (a, b) = (place(&h, from), place(&h, to));
    let (mut cut, mut deep, mut drowned, mut sea_floor) = (0, 0, 0, 0);
    for (i, (&old, &new)) in before.iter().zip(&after).enumerate() {
        assert!(new <= old, "{new} over {old}");
        drowned += usize::from(old >= sea && new < sea);
        sea_floor += usize::from(old < sea && new < old);
        if new == old {
            continue;
        }
        // The texel is under the brush at one place of the stroke, or one
        // cell and a half from it.
        let d = map.texel_dir(i / (n * n), i % n, i / n % n);
        let near = (0..=40).any(|k| {
            let at = vale_app::globe::brush::slerp(a, b, f64::from(k) / 40.0);
            angle(d, at) < radius + 0.02
        });
        assert!(near, "far from the stroke");
        cut += 1;
        deep += usize::from(new + 100 < old);
    }
    assert!(cut > 5000, "{cut}");
    assert!(deep > 2000, "{deep}");
    // The coast moves, and the sea floor goes down.
    assert!(drowned > 100 && sea_floor > 1000, "{drowned} {sea_floor}");
    // Some land is left.
    let land = |levels: &[u16]| levels.iter().filter(|&&level| level > sea + 200).count();
    assert!(land(&after) * 10 > land(&before) * 9);
}

/// Paints an erode stroke from the land of the island to the sea.
fn erode_to_the_sea(h: &mut Harness<'static, AppState>) {
    let c = h.state().globe.rect.center();
    let reach = 0.88 * h.state().globe.radius() as f32;
    let (from, to) = (c, c + Vec2::new(reach, 0.0));
    button(h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=40 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 40.0)));
        h.step();
    }
    button(h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(h);
}

#[test]
fn an_erode_stroke_with_a_floor_keeps_the_land_above_the_sea() {
    let _gpu = one_gpu_test();
    let mut h = erode_harness();
    let before = all_levels(&h);
    let sea = meters_to_level(0.0);

    // Without the floor, the stroke moves land below the sea and lowers
    // the sea floor.
    erode_to_the_sea(&mut h);
    let free = all_levels(&h);
    let pairs = || before.iter().zip(&free);
    let drowned = pairs().filter(|&(&old, &new)| old > sea && new < sea);
    let sea_floor = pairs().filter(|&(&old, &new)| old <= sea && new < old);
    let (drowned, sea_floor) = (drowned.count(), sea_floor.count());
    assert!(drowned > 100 && sea_floor > 1000, "{drowned} {sea_floor}");
    undo_stroke(&mut h, &before);

    let brush = &mut h.state_mut().globe.brush;
    (brush.floor_on, brush.floor_m) = (true, 0.0);
    assert_eq!(brush.limits().floor, sea);
    erode_to_the_sea(&mut h);
    let after = all_levels(&h);
    let (mut cut, mut at_floor) = (0, 0);
    for (&old, &new) in before.iter().zip(&after) {
        assert!(new <= old, "{new} over {old}");
        // Land stays at the floor or above it, and the sea floor stays.
        assert!(new >= old.min(sea), "{new} from {old}");
        cut += usize::from(new < old);
        at_floor += usize::from(old > sea && new == sea);
    }
    assert!(cut > 5000, "{cut}");
    assert!(at_floor > 20, "{at_floor}");
}

#[test]
fn erode_in_a_near_view_works_on_the_cells_of_the_window() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.2, 14.0);
    let globe = &mut h.state_mut().globe;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Erode;
    globe.brush.size_points = 80.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);
    let w = h.state().globe.window().unwrap().window();
    assert_eq!(w.cell, 1);
    // A cell of the global map is this number of texels wide.
    let cell = N / CoarseHeights::size_for(N);
    assert_eq!(cell, 4);

    // The texels of face 0 around the stroke.
    let texels = || (N / 4..3 * N / 4).flat_map(|y| (N / 4..3 * N / 4).map(move |x| (x, y)));
    let levels = |map: &vale_terrain::Heightmap| -> Vec<i64> {
        texels()
            .map(|(x, y)| i64::from(map.get(0, x as i64, y as i64)))
            .collect()
    };
    let before = levels(&h.state().globe.map);
    let c = h.state().globe.rect.center();
    hold(&mut h, c, 20);
    let after = levels(&h.state().globe.map);

    let side = N / 2;
    let drop = |x: usize, y: usize| before[y * side + x] - after[y * side + x];
    let (mut cut, mut bends) = (0, 0);
    for (i, (x, y)) in texels().enumerate() {
        assert!(after[i] <= before[i]);
        cut += usize::from(after[i] < before[i]);
        // These 3 texels are between the centers of 2 cells of the global
        // map. The drops of the global cells give them drops on a straight
        // line.
        let (x, y) = (x - N / 4, y - N / 4);
        if x % cell == 3 && y % cell == 3 && x + 1 < side {
            let bend = drop(x - 1, y) - 2 * drop(x, y) + drop(x + 1, y);
            bends += usize::from(bend.abs() > 8);
        }
    }
    assert!(cut > 1000, "{cut}");
    assert!(bends > 50, "{bends}");
}

#[test]
fn painting_does_not_wait_for_an_erode_step() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.35, 1.0);
    let globe = &mut h.state_mut().globe;
    globe.set_rivers_sync(false);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Erode;
    h.step();
    settle_rivers(&mut h);
    let c = h.state().globe.rect.center() + Vec2::new(20.0, 0.0);
    let under = place(&h, c);
    let level = h.state().globe.map.sample(under);

    button(&mut h, egui::PointerButton::Primary, c, true);
    h.step();
    // The frame is complete, and the step of its pen sample is not.
    assert!(h.state().globe.erode_pending());
    button(&mut h, egui::PointerButton::Primary, c, false);
    // The stroke ends after its last step.
    let mut frames = 0;
    while h.state().globe.busy() {
        h.step();
        frames += 1;
        assert!(frames < 4000, "the stroke did not end");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(frames > 2, "{frames}");
    let globe = &h.state().globe;
    assert!(!globe.erode_pending());
    assert!(globe.map.sample(under) < level);
    assert!(globe.can_undo());
}

#[test]
fn one_undo_puts_back_the_ground_from_before_an_erode_stroke() {
    let _gpu = one_gpu_test();
    let mut h = erode_harness();
    let before = (all_levels(&h), canvas_image(&mut h));
    let c = h.state().globe.rect.center();
    hold(&mut h, c + Vec2::new(40.0, 0.0), 30);
    let eroded = (all_levels(&h), canvas_image(&mut h));
    // Each of the 30 frames made one step.
    assert!(lowered(&before.0, &eroded.0) > 100_000);
    assert_eq!(h.state().globe.stats.last().unwrap().mode, Mode::Erode);
    assert!(eroded.1.as_raw() != before.1.as_raw());

    assert!(h.state_mut().globe.undo());
    assert!(all_levels(&h) == before.0);
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == before.1.as_raw(), "the render");

    assert!(h.state_mut().globe.redo());
    assert!(all_levels(&h) == eroded.0);
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == eroded.1.as_raw(), "the render");
}

/// Holds the erode brush of this size at the middle of the view for 3
/// frames. Returns the cell of the window of the brush of the stroke, in
/// texels, or `None` for a stroke on the cells of the view.
fn erode_cell(h: &mut Harness<'static, AppState>, size_points: f32) -> Option<usize> {
    h.state_mut().globe.brush.size_points = size_points;
    h.step();
    let globe = &h.state().globe;
    let c = globe.rect.center();
    let (view, n) = (globe.window().map(|w| w.window()), globe.map.face_size());
    let radius = globe.brush.radius(globe.radius());
    let rule = vale_app::globe::erode::erode_window(view, place(h, c), radius, n);
    let before = h.state().globe.map.sample(place(h, c));

    button(h, egui::PointerButton::Primary, c, true);
    h.run_steps(3);
    let window = h.state().globe.erode_window();
    button(h, egui::PointerButton::Primary, c, false);
    h.step();
    settle(h);
    assert!(h.state().globe.erode_window().is_none());
    assert!(h.state().globe.map.sample(place(h, c)) < before);
    assert!(h.state_mut().globe.undo());
    h.run_steps(2);

    let cell = window.map(|window| window.cell);
    assert_eq!(cell, rule.map(|window| window.cell));
    if let Some(window) = window {
        // The pen is at the middle of the window.
        let (u, v) = window.place(place(h, c)).unwrap();
        let half = window.cells as f64 / 2.0;
        assert!((u - half).abs() < 1.0 && (v - half).abs() < 1.0, "{u} {v}");
    }
    cell
}

#[test]
fn a_small_erode_brush_works_on_the_cells_of_the_window_of_the_brush() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    // A cell of the global map is 4 texels wide.
    assert_eq!(N / CoarseHeights::size_for(N), 4);
    for (zoom, view_cell) in [(1.0, 4), (7.0, 2)] {
        let mut h = island_harness(N, 0.35, zoom);
        let globe = &mut h.state_mut().globe;
        globe.tool = Tool::Brush;
        globe.brush.mode = Mode::Erode;
        globe.brush.strength_m = 3000.0;
        h.run_steps(2);
        let globe = &h.state().globe;
        let view = globe.window().map(|w| w.window());
        assert_eq!(view.map_or(4, |view| view.cell), view_cell);
        // The radius of a brush of one point, in cells of the view.
        let texel = std::f64::consts::FRAC_PI_2 / N as f64;
        let cells = |size_points: f32| {
            let globe = &h.state().globe;
            let mut brush = globe.brush;
            brush.size_points = size_points;
            brush.radius(globe.radius()) / (texel * view_cell as f64)
        };
        let (small, large) = ((12.0 / cells(1.0)) as f32, (20.0 / cells(1.0)) as f32);
        assert!(cells(small) < 16.0 && cells(large) > 16.0);

        let cell = erode_cell(&mut h, small).expect("the window of the brush");
        assert!(cell < view_cell, "{cell}");
        assert_eq!(erode_cell(&mut h, large), None);
    }
}

#[test]
fn one_undo_puts_back_the_ground_from_before_a_stroke_on_the_window_of_the_brush() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.35, 1.0);
    let globe = &mut h.state_mut().globe;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Erode;
    globe.brush.size_points = 6.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);
    let rivers =
        |h: &Harness<'static, AppState>| h.state().globe.channels().unwrap().bytes().to_vec();
    let before = (all_levels(&h), rivers(&h));

    // The stroke goes over the land of the island.
    let c = h.state().globe.rect.center();
    let (from, to) = (c - Vec2::new(30.0, 10.0), c + Vec2::new(30.0, 10.0));
    button(&mut h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=30 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 30.0)));
        h.step();
        let window = h
            .state()
            .globe
            .erode_window()
            .expect("the window of the brush");
        assert!(window.cell < 4);
    }
    button(&mut h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(&mut h);
    let eroded = (all_levels(&h), rivers(&h));
    assert!(lowered(&before.0, &eroded.0) > 10_000);
    // The rivers of the view follow the new ground.
    assert!(eroded.1 != before.1);

    assert!(h.state_mut().globe.undo());
    h.run_steps(2);
    assert!(all_levels(&h) == before.0);
    assert!(rivers(&h) == before.1);
    assert!(!h.state().globe.can_undo());

    assert!(h.state_mut().globe.redo());
    h.run_steps(2);
    assert!(all_levels(&h) == eroded.0);
    assert!(rivers(&h) == eroded.1);
}

#[test]
fn the_river_width_slider_changes_the_lines() {
    let _gpu = one_gpu_test();
    let mut h = island_harness(1024, 0.35, 8.0);
    h.state_mut().globe.preview.panel = true;
    h.run_steps(2);
    assert!(h.query_all_by_label("River width").next().is_some());
    let state_of = |h: &Harness<'static, AppState>| {
        let globe = &h.state().globe;
        let n = globe.map.face_size() as i64;
        let levels: Vec<u16> = (0..n * n).map(|i| globe.map.get(0, i % n, i / n)).collect();
        let channels = globe.channels().unwrap().bytes().to_vec();
        (levels, channels, globe.window().unwrap().bytes().to_vec())
    };
    let before = state_of(&h);
    let img = canvas_image(&mut h);
    let thin = river_pixels(&h, &img);
    h.state_mut().globe.preview.river_width = 3.0;
    let img = canvas_image(&mut h);
    let wide = river_pixels(&h, &img);
    assert!(thin > 100 && wide > thin * 3 / 2, "{thin} and {wide}");
    assert!(state_of(&h) == before);
}

/// A drag of the primary mouse button from one place to another.
fn mouse_drag(h: &mut Harness<'static, AppState>, from: Pos2, to: Pos2) {
    button(h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 8.0)));
        h.step();
    }
    button(h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(h);
}

/// Writes images of a near view with the window of small rivers, for a
/// person to look at.
#[test]
#[ignore]
fn screenshots_with_a_river_window() {
    let _gpu = one_gpu_test();
    let mut state = state();
    let globe = &mut state.globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.view.zoom = 7.0;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Carve;
    globe.brush.strength_m = 3000.0;
    globe.debug = true;
    set_small_island(&mut globe.map, 0.6);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (1440.0, 900.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    assert!(h.state().globe.window().is_some());
    let shot = |h: &mut Harness<'static, AppState>, name: &str| {
        h.remove_cursor();
        h.run_steps(2);
        save(&h.render().unwrap(), name);
    };
    shot(&mut h, "rivers-window");
    h.state_mut().globe.preview.river_width = 3.0;
    shot(&mut h, "rivers-window-wide");
    h.state_mut().globe.preview.river_width = 1.0;

    let c = h.state().globe.rect.center();
    for (from, to) in [
        (Vec2::new(-300.0, -200.0), Vec2::new(250.0, -120.0)),
        (Vec2::new(-250.0, 0.0), Vec2::new(300.0, 60.0)),
        (Vec2::new(-200.0, 220.0), Vec2::new(200.0, 150.0)),
    ] {
        mouse_drag(&mut h, c + from, c + to);
    }
    shot(&mut h, "rivers-window-carved");

    // The view moves, and the frame comes before the next window. The frame
    // shows the window of the view before, and the channel map around it.
    h.state_mut().globe.set_rivers_sync(false);
    h.state_mut().globe.view = GlobeView::centered(22.0, 0.0);
    h.state_mut().globe.view.zoom = 7.0;
    h.step();
    assert!(h.state().globe.rivers_pending());
    save(&h.render().unwrap(), "rivers-window-replaced");
}

#[test]
fn the_brush_panel_has_the_uplift_mode_and_its_two_sliders() {
    let mut h = harness();
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("Uplift").count(), 1);
    assert!(h.query_all_by_label("Erosion").next().is_none());
    assert!(h.query_all_by_label("Strength").next().is_some());
    h.get_by_label("Uplift").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Uplift);
    // The mode and the slider of the rate of the uplift.
    assert!(h.query_all_by_label("Uplift").count() > 1);
    assert!(h.query_all_by_label("Strength").next().is_none());
    assert!(h.query_all_by_label("Erosion").next().is_some());
    assert_eq!(h.state().globe.brush.erosion, 0.5);
}

/// A harness with the empty world in a far view, and the uplift brush.
fn uplift_harness() -> Harness<'static, AppState> {
    let mut h = gpu_harness();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.preview.graticule = false;
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Uplift;
    globe.brush.size_points = 40.0;
    globe.brush.strength_m = 3000.0;
    h.run_steps(2);
    h
}

/// The two ends of the stroke of the uplift tests.
fn uplift_ends(h: &Harness<'static, AppState>) -> (Pos2, Pos2) {
    let c = h.state().globe.rect.center();
    (c - Vec2::new(90.0, 20.0), c + Vec2::new(90.0, 20.0))
}

/// Presses the primary mouse button and moves it along the first part of the
/// stroke of the uplift tests. The button stays down.
fn uplift_part(h: &mut Harness<'static, AppState>, moves: usize) {
    // The width of the brush panel follows the mode, and the canvas follows
    // the panel.
    h.run_steps(2);
    let (from, to) = uplift_ends(h);
    button(h, egui::PointerButton::Primary, from, true);
    h.step();
    for i in 1..=moves {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 24.0)));
        h.step();
    }
}

/// The frame with the pointer, cut to the canvas of the globe. The pointer
/// stays where it is, so a stroke continues.
fn canvas_with_pointer(h: &mut Harness<'static, AppState>) -> image::RgbaImage {
    let rect = h.state().globe.rect;
    let (x, y) = (rect.min.x.ceil() as u32, rect.min.y.ceil() as u32);
    let (w, h_) = (rect.width().floor() as u32, rect.height().floor() as u32);
    let frame = h.render().unwrap();
    image::imageops::crop_imm(&frame, x, y, w - 1, h_ - 1).to_image()
}

/// Paints the stroke of the uplift tests. Returns the levels after it.
fn uplift_stroke(h: &mut Harness<'static, AppState>) -> Vec<u16> {
    uplift_part(h, 24);
    let (_, to) = uplift_ends(h);
    button(h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(h);
    all_levels(h)
}

/// Takes the last stroke away.
fn undo_stroke(h: &mut Harness<'static, AppState>, before: &[u16]) {
    assert!(h.state_mut().globe.undo());
    h.run_steps(2);
    assert!(all_levels(h) == before);
    assert!(!h.state().globe.can_undo());
}

#[test]
fn uplift_raises_the_land_and_erodes_it_during_the_stroke() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = all_levels(&h);

    // Without erosion, the stroke raises the land as the raise mode does.
    h.state_mut().globe.brush.erosion = 0.0;
    uplift_part(&mut h, 12);
    let raise_part = canvas_with_pointer(&mut h);
    let raised = uplift_stroke_end(&mut h);
    undo_stroke(&mut h, &before);

    // The angle of 20 degrees is steeper than each slope of the range, so
    // the weather stamps change nothing.
    h.state_mut().globe.brush.erosion = 0.5;
    h.state_mut().globe.brush.talus_deg = 20.0;
    uplift_part(&mut h, 12);
    // The pen is down, and the ground on the screen has valleys.
    assert!(h.state().globe.busy());
    let eroded_part = canvas_with_pointer(&mut h);
    assert_eq!(eroded_part.dimensions(), raise_part.dimensions());
    assert!(eroded_part.as_raw() != raise_part.as_raw());
    let eroded = uplift_stroke_end(&mut h);
    save(&canvas_image(&mut h), "uplift-after");
    assert_eq!(h.state().globe.stats.last().unwrap().mode, Mode::Uplift);
    undo_stroke(&mut h, &before);

    let (mut up, mut cut, mut deep) = (0, 0, 0);
    for ((&old, &flat), &new) in before.iter().zip(&raised).zip(&eroded) {
        // The uplift at this erosion is 1.5 times the rise of the raise
        // mode. The erosion takes ground away from the raised land only.
        // Each stamp rounds its rise to a whole level.
        let most = old + (flat - old) / 2 * 3 + 40;
        assert!(old <= new && new <= most, "{old} {new} {most}");
        up += usize::from(new > old + 1000);
        cut += usize::from(new + 80 < most);
        deep += usize::from(new + 1000 < most);
    }
    assert!(up > 2000, "{up}");
    assert!(cut > 2000 && deep > 100, "{cut} {deep}");

    // With a small angle, the weather stamps move ground down the slopes.
    h.state_mut().globe.brush.talus_deg = 0.05;
    let weathered = uplift_stroke(&mut h);
    let pairs = eroded.iter().zip(&weathered);
    let (down, up) = pairs.fold((0, 0), |(down, up), (a, b)| {
        (down + usize::from(b < a), up + usize::from(b > a))
    });
    assert!(down > 500 && up > 500, "{down} down, {up} up");
}

#[test]
fn an_uplift_stroke_with_a_ceiling_gives_a_plateau() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = all_levels(&h);
    let start = *before.iter().max().unwrap();

    // Without the ceiling, the range is much higher than the ceiling.
    let free = uplift_stroke(&mut h);
    let top = *free.iter().max().unwrap();
    assert!(top > start + 2000, "{top} from {start}");
    undo_stroke(&mut h, &before);

    let brush = &mut h.state_mut().globe.brush;
    brush.ceiling_on = true;
    brush.ceiling_m = level_to_meters(start + (top - start) / 3).round();
    let ceiling = brush.limits().ceiling;
    assert!(start + 500 < ceiling && ceiling + 500 < top, "{ceiling}");
    assert!(free.iter().filter(|&&level| level > ceiling).count() > 1000);

    let after = uplift_stroke(&mut h);
    let (mut up, mut plateau) = (0, 0);
    for (&old, &new) in before.iter().zip(&after) {
        assert!(old < ceiling);
        assert!(new <= ceiling, "{new} over the ceiling {ceiling}");
        up += usize::from(new > old);
        plateau += usize::from(new == ceiling);
    }
    assert!(up > 2000, "{up}");
    assert!(plateau > 500, "{plateau}");
}

#[test]
fn an_uplift_stroke_with_full_erosion_gives_about_the_height_of_a_raise_stroke() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = all_levels(&h);
    let floor = *before.iter().max().unwrap();
    let rise = |levels: &[u16]| f64::from(*levels.iter().max().unwrap() - floor);

    h.state_mut().globe.brush.erosion = 0.0;
    let raise = rise(&uplift_stroke(&mut h));
    undo_stroke(&mut h, &before);

    h.state_mut().globe.brush.erosion = 1.0;
    let once = rise(&uplift_stroke(&mut h));
    assert!(once > 0.6 * raise && once < 1.4 * raise, "{once} {raise}");
    // More strokes on the range do not add their heights.
    uplift_stroke(&mut h);
    let third = rise(&uplift_stroke(&mut h));
    eprintln!("raise {raise}, once {once}, third {third}");
    assert!(
        third > 0.6 * raise && third < 1.4 * raise,
        "{third} {raise}"
    );
}

/// Writes images of ranges of three sizes on a world of the size of the
/// Earth, for a person to look at. The views are 3,500 km, 1,000 km, and
/// 350 km high.
#[test]
#[ignore]
fn screenshots_of_uplift_ranges() {
    let _gpu = one_gpu_test();
    for (name, zoom, radius_km) in [
        ("continent", 4.0, 150.0),
        ("region", 14.0, 40.0),
        ("near", 40.0, 12.0),
    ] {
        for erosion in [0, 50, 100] {
            let mut state = state();
            state.globe.set_face_size(vale_app::globe::WINDOW_FACE_SIZE);
            let setup = egui_kittest::wgpu::default_wgpu_setup();
            let mut h = headless::ui_harness(state, (1100.0, 800.0), 1.0, setup);
            h.set_render_every_step(true);
            let globe = &mut h.state_mut().globe;
            globe.view = GlobeView::centered(0.0, 0.0);
            globe.view.zoom = zoom;
            globe.preview.graticule = false;
            globe.tool = Tool::Brush;
            globe.brush.mode = Mode::Uplift;
            globe.brush.erosion = f64::from(erosion) / 100.0;
            globe.brush.strength_m = 6000.0;
            h.run_steps(4);
            let globe = &mut h.state_mut().globe;
            let (radius, world) = (globe.radius(), globe.world_km);
            globe.brush.set_radius_km(radius_km, radius, world);
            h.run_steps(2);
            let rect = h.state().globe.rect;
            let (c, half) = (rect.center(), 0.36 * rect.width());
            let at =
                |t: f32| c + Vec2::new(half * t, 0.2 * half * (2.5 * t).sin() + 0.1 * half * t);
            let start = std::time::Instant::now();
            button(&mut h, egui::PointerButton::Primary, at(-1.0), true);
            h.step();
            for i in 1..=120 {
                h.event(egui::Event::PointerMoved(at(-1.0 + i as f32 / 60.0)));
                h.step();
            }
            button(&mut h, egui::PointerButton::Primary, at(1.0), false);
            h.step();
            settle(&mut h);
            let globe = &h.state().globe;
            let km = globe.brush.radius_km(globe.radius(), globe.world_km);
            let high = f64::from(rect.height()) / globe.radius() * globe.world_km;
            eprintln!(
                "{name} {erosion}: radius {km:.0} km, view {high:.0} km, {:?}",
                start.elapsed()
            );
            save(&canvas_image(&mut h), &format!("range-{name}-{erosion}"));
        }
    }
}

/// Lifts the button at the end of the stroke that `uplift_part` began, after
/// the rest of its moves. Returns the levels after the stroke.
fn uplift_stroke_end(h: &mut Harness<'static, AppState>) -> Vec<u16> {
    let (from, to) = uplift_ends(h);
    for i in 13..=24 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 24.0)));
        h.step();
    }
    button(h, egui::PointerButton::Primary, to, false);
    h.step();
    settle(h);
    all_levels(h)
}

/// Paints a stroke along the equator with the pen of the globe. The places
/// of the stroke do not follow the layout of the window. Returns the levels
/// after it.
fn pen_stroke_on_the_equator(h: &mut Harness<'static, AppState>) -> Vec<u16> {
    h.state_mut().globe.pen_down();
    for i in 0..=24 {
        let sample = vale_app::globe::brush::Sample {
            dir: Some(lonlat_to_dir(f64::from(i) - 12.0, 0.0)),
            flow: 0.5,
            radius: 0.06,
        };
        let globe = &mut h.state_mut().globe;
        globe.pen_sample(sample, std::time::Instant::now());
        h.step();
    }
    h.state_mut().globe.pen_up();
    h.step();
    settle(h);
    all_levels(h)
}

#[test]
fn with_the_erosion_at_zero_uplift_gives_the_result_of_raise() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = all_levels(&h);
    h.state_mut().globe.brush.mode = Mode::Raise;
    let raised = pen_stroke_on_the_equator(&mut h);
    assert!(raised != before);
    undo_stroke(&mut h, &before);

    let brush = &mut h.state_mut().globe.brush;
    brush.mode = Mode::Uplift;
    brush.erosion = 0.0;
    brush.talus_deg = 0.02;
    assert!(pen_stroke_on_the_equator(&mut h) == raised);
    undo_stroke(&mut h, &before);

    h.state_mut().globe.brush.erosion = 0.5;
    assert!(pen_stroke_on_the_equator(&mut h) != raised);
}

#[test]
fn two_equal_uplift_strokes_on_equal_ground_give_equal_results() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = all_levels(&h);
    let first = uplift_stroke(&mut h);
    assert!(first != before);
    undo_stroke(&mut h, &before);
    assert!(uplift_stroke(&mut h) == first);

    // A second harness starts from the same ground.
    drop(h);
    let mut h = uplift_harness();
    assert!(all_levels(&h) == before);
    assert!(uplift_stroke(&mut h) == first);
}

#[test]
fn painting_does_not_wait_for_an_uplift_erosion_step() {
    const N: usize = 2048;
    let _gpu = one_gpu_test();
    let mut h = island_harness(N, 0.35, 1.0);
    let globe = &mut h.state_mut().globe;
    globe.set_rivers_sync(false);
    globe.tool = Tool::Brush;
    globe.brush.mode = Mode::Uplift;
    h.step();
    settle_rivers(&mut h);
    let c = h.state().globe.rect.center() + Vec2::new(20.0, 0.0);
    let under = place(&h, c);
    let level = h.state().globe.map.sample(under);

    button(&mut h, egui::PointerButton::Primary, c, true);
    h.step();
    // The frame is complete, and the step of its stamp is not.
    assert!(h.state().globe.erode_pending());
    button(&mut h, egui::PointerButton::Primary, c, false);
    // The stroke ends after its last step.
    let mut frames = 0;
    while h.state().globe.busy() {
        h.step();
        frames += 1;
        assert!(frames < 4000, "the stroke did not end");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(frames > 2, "{frames}");
    let globe = &h.state().globe;
    assert!(!globe.erode_pending());
    assert!(globe.map.sample(under) > level);
    assert!(globe.can_undo());
}

#[test]
fn one_undo_puts_back_the_ground_from_before_an_uplift_stroke() {
    let _gpu = one_gpu_test();
    let mut h = uplift_harness();
    let before = (all_levels(&h), canvas_image(&mut h));
    let lifted = (uplift_stroke(&mut h), canvas_image(&mut h));
    assert!(lifted.0 != before.0);
    assert!(lifted.1.as_raw() != before.1.as_raw());
    // The stamps, the erode steps, and the weather stamps are one stroke.
    assert_eq!(h.state().globe.stats.last().unwrap().id, 1);

    assert!(h.state_mut().globe.undo());
    assert!(all_levels(&h) == before.0);
    assert!(!h.state().globe.can_undo());
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == before.1.as_raw(), "the render");

    assert!(h.state_mut().globe.redo());
    assert!(all_levels(&h) == lifted.0);
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == lifted.1.as_raw(), "the render");
}

/// A harness with a ridge from a raise stroke, in the warp mode.
fn warp_harness(seed: u32) -> Harness<'static, AppState> {
    let mut h = uplift_harness();
    h.state_mut().globe.brush.mode = Mode::Raise;
    uplift_stroke(&mut h);
    let state = h.state_mut();
    state.doc.project.world.seed = seed;
    state.globe.brush.mode = Mode::Warp;
    state.globe.brush.warp_km = 300.0;
    state.globe.brush.scale_km = 300.0;
    h.run_steps(2);
    h
}

#[test]
fn two_equal_warp_strokes_with_the_same_seed_give_equal_results() {
    let _gpu = one_gpu_test();
    let mut h = warp_harness(7);
    assert_eq!(h.state().globe.world_seed, 7);
    let before = all_levels(&h);
    let first = uplift_stroke(&mut h);
    assert!(first != before);
    // The stroke moves the ground, and it adds no height.
    let (lo, hi) = (before.iter().min(), before.iter().max());
    assert!(first.iter().min() >= lo && first.iter().max() <= hi);
    assert!(h.state_mut().globe.undo());
    h.run_steps(2);
    assert!(all_levels(&h) == before);
    assert!(uplift_stroke(&mut h) == first);

    // A second harness starts from the same ground.
    drop(h);
    let mut h = warp_harness(7);
    assert!(all_levels(&h) == before);
    assert!(uplift_stroke(&mut h) == first);

    // Another seed gives another result.
    drop(h);
    let mut h = warp_harness(8);
    assert!(all_levels(&h) == before);
    let other = uplift_stroke(&mut h);
    assert!(other != before && other != first);
}

#[test]
fn one_undo_puts_back_the_ground_from_before_a_warp_stroke() {
    let _gpu = one_gpu_test();
    let mut h = warp_harness(7);
    let before = (all_levels(&h), canvas_image(&mut h));
    let warped = (uplift_stroke(&mut h), canvas_image(&mut h));
    assert!(warped.0 != before.0);
    assert!(warped.1.as_raw() != before.1.as_raw());

    assert!(h.state_mut().globe.undo());
    assert!(all_levels(&h) == before.0);
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == before.1.as_raw(), "the render");

    assert!(h.state_mut().globe.redo());
    assert!(all_levels(&h) == warped.0);
    let image = canvas_image(&mut h);
    assert!(image.as_raw() == warped.1.as_raw(), "the render");
}

/// Writes an image of a ridge after warp strokes, for a person to look at.
#[test]
#[ignore]
fn screenshots_of_warp() {
    let _gpu = one_gpu_test();
    let mut h = warp_harness(7);
    save(&canvas_image(&mut h), "warp-before");
    for _ in 0..3 {
        uplift_stroke(&mut h);
    }
    save(&canvas_image(&mut h), "warp-after");
}

/// Writes images of a range from uplift strokes, for a person to look at.
#[test]
#[ignore]
fn screenshots_of_uplift() {
    let _gpu = one_gpu_test();
    for (name, erosion) in [("none", 0.0), ("half", 0.5), ("full", 1.0)] {
        let mut h = uplift_harness();
        let globe = &mut h.state_mut().globe;
        globe.brush.erosion = erosion;
        globe.brush.strength_m = 6000.0;
        globe.view.zoom = 2.0;
        for _ in 0..3 {
            uplift_stroke(&mut h);
        }
        save(&canvas_image(&mut h), &format!("uplift-{name}"));
    }
}
