//! The flat view of the world workspace.

mod images;
mod lines;
mod masked;

use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use lines::{CRATER_RADIUS, canvas_image, set_crater, show_as, two_bands};
use vale_app::document::Document;
use vale_app::globe::flat::FlatView;
use vale_app::globe::math::{V3, angle, lonlat_to_dir};
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Tool, WorldView};
use vale_app::headless;
use vale_app::ui::pad::{RECENTER, RESET};
use vale_app::ui::{Action, AppState, draw};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::RasterDisplay;
use vale_terrain::{Mode, meters_to_level};

type App = Harness<'static, AppState>;

/// The sample world with its vector layers hidden, so that each place of the
/// map shows the heightmap.
fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    let ids: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in ids {
        state.doc.set_visible(id, false);
    }
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
    state.add_height();
    state.globe.world_view = WorldView::Flat;
    state.globe.tool = Tool::Brush;
    state.globe.brush.size_points = 12.0;
    show_as(&mut state, RasterDisplay::Grey);
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

/// The grey level of the canvas at a screen position.
fn grey(h: &App, image: &image::RgbaImage, pos: Pos2) -> u8 {
    let at = pos - h.state().globe.rect.min;
    image.get_pixel(at.x as u32, at.y as u32).0[0]
}

#[test]
fn one_click_switches_the_view_and_the_tool_stays() {
    let mut state = state();
    state.add_height();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
    ProjectionSpec::new(kind, lon0, lat0)
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
fn the_brush_outline_matches_the_ground_that_the_stamp_covers() {
    let _gpu = one_gpu_test();
    let (w, h) = check_outline(&mut flat_harness(), 30.0, 0.0);
    assert!((w - 24.0).abs() < 0.5 && (h - 24.0).abs() < 0.5, "{w} {h}");
    // Near a pole the same stamp covers more longitude, so the outline is wide.
    let (w, h) = check_outline(&mut flat_harness(), 30.0, 70.0);
    assert!(w > 2.5 * h && (h - 24.0).abs() < 0.5, "{w} {h}");
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

/// The number of pixels of the canvas that show a river on land.
fn river_pixels(h: &App, img: &image::RgbaImage) -> usize {
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

#[test]
#[ignore = "gpu"]
fn rivers_show_in_the_flat_view_and_a_switch_hides_them() {
    let _gpu = one_gpu_test();
    let mut h = flat_harness();
    show_as(h.state_mut(), RasterDisplay::Bands);
    let globe = &mut h.state_mut().globe;
    globe.preview.panel = true;
    // Without the brush panel, the canvas has more room.
    globe.tool = Tool::Navigate;
    globe.flat.zoom = 2.0;
    let center = lonlat_to_dir(0.0, 0.0);
    let n = globe.map.face_size();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let meters = island(globe.map.texel_dir(face, x, y), center);
                globe.map.set(face, x, y, meters_to_level(meters));
            }
        }
    }
    let on = canvas_image(&mut h);
    std::fs::create_dir_all("../../target/app").unwrap();
    on.save("../../target/app/rivers-flat.png").unwrap();
    let count = river_pixels(&h, &on);
    assert!(count > 300, "{count}");

    h.get_by_label("Rivers").click();
    h.run_steps(2);
    assert!(!h.state().panel_raster().unwrap().rivers);
    let off = canvas_image(&mut h);
    assert_eq!(river_pixels(&h, &off), 0);
}

#[test]
#[ignore = "gpu"]
fn lakes_show_in_the_flat_view_and_a_switch_hides_them() {
    let _gpu = one_gpu_test();
    let mut h = flat_harness();
    show_as(h.state_mut(), RasterDisplay::Bands);
    let id = h.state().globe.raster().unwrap();
    let globe = &mut h.state_mut().globe;
    two_bands(globe.bands_mut(id).unwrap(), [40, 40, 40], [150, 140, 120]);
    globe.preview.panel = true;
    globe.tool = Tool::Navigate;
    globe.flat.zoom = 4.0;
    let crater = lonlat_to_dir(0.0, 0.0);
    set_crater(&mut globe.map, crater, 800.0);
    let img = canvas_image(&mut h);
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/lakes-flat.png").unwrap();
    let globe = &h.state().globe;
    let channels = globe.channels().unwrap();
    assert!(channels.has_lakes());
    // The pixels of the crater well below the lake level are water.
    let margin = f64::from(meters_to_level(150.0)) - f64::from(meters_to_level(0.0));
    let min = globe.rect.min.ceil();
    let (mut wet, mut blue) = (0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        let pos = min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        let Some(dir) = globe.unproject(pos) else {
            continue;
        };
        let level = f64::from(globe.map.sample(dir));
        if angle(dir, crater) < CRATER_RADIUS && level < channels.lake_level(dir) - margin {
            let [r, g, b, _] = p.0.map(i32::from);
            wet += 1;
            blue += usize::from(b > r + 60 && b > g + 30);
        }
    }
    assert!(wet > 300 && blue == wet, "{blue} of {wet}");

    h.get_by_label("Rivers").click();
    h.run_steps(2);
    assert!(!h.state().panel_raster().unwrap().rivers);
    let off = canvas_image(&mut h);
    assert_eq!(river_pixels(&h, &off), 0);
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

#[test]
#[ignore = "gpu"]
fn zooming_in_gives_finer_rivers_in_the_flat_view() {
    let _gpu = one_gpu_test();
    let mut state = state();
    state.globe.set_face_size(1024);
    state.add_height();
    let globe = &mut state.globe;
    globe.world_view = WorldView::Flat;
    globe.preview.graticule = false;
    globe.flat.zoom = 16.0;
    set_small_island(&mut globe.map, 0.35);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (640.0, 480.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);

    let globe = &h.state().globe;
    let window = globe.window().expect("a near view has a window");
    assert_eq!(window.window().cell, 1);
    let (fine, coarse) = river_parts(globe);
    assert!(coarse > 0.0 && fine > 1.5 * coarse, "{fine} and {coarse}");
    let img = canvas_image(&mut h);
    img.save("../../target/app/rivers-flat-near.png").unwrap();
    let count = river_pixels(&h, &img);
    assert!(count > 300, "{count}");

    // A far view has the rivers of the channel map only.
    h.state_mut().globe.flat.zoom = 2.0;
    h.run_steps(2);
    assert!(h.state().globe.window().is_none());
}

#[test]
#[ignore = "gpu"]
fn the_flat_view_blends_the_raster_layers_by_their_opacity() {
    let _gpu = one_gpu_test();
    let mut h = flat_harness();
    let height = h.state().globe.raster().unwrap();
    let rain = vale_app::ui::RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 128,
    };
    let rain = h.state_mut().add_raster(rain).unwrap();
    show_as(h.state_mut(), RasterDisplay::Grey);
    let (low, high) = (16000, 50000);
    let state = h.state_mut();
    state.globe.raster_map_mut(height).unwrap().fill(low);
    state.globe.raster_map_mut(rain).unwrap().fill(high);
    h.run_steps(3);
    let grey_of = |level: u16| (f32::from(level) / 65535.0 * 255.0).round() as u8;
    let middle = |h: &mut App| {
        let image = canvas_image(h);
        let center = h.state().globe.rect.center();
        grey(h, &image, center)
    };
    // The top layer hides the layer below it.
    assert!(middle(&mut h).abs_diff(grey_of(high)) <= 2);
    h.state_mut().set_raster_opacity(rain, 0.5);
    let mean = ((u16::from(grey_of(low)) + u16::from(grey_of(high))) / 2) as u8;
    assert!(middle(&mut h).abs_diff(mean) <= 2, "{}", middle(&mut h));
    h.state_mut().set_raster_opacity(rain, 0.0);
    assert!(middle(&mut h).abs_diff(grey_of(low)) <= 2);
    // A hidden layer leaves the picture, and the ground is below the
    // layers.
    h.state_mut().set_raster_opacity(rain, 0.5);
    h.state_mut().set_raster_visible(height, false);
    let ground = vale_app::ui::pad::theme::GROUND;
    let mean = ((u16::from(ground.r()) + u16::from(grey_of(high))) / 2) as u8;
    assert!(middle(&mut h).abs_diff(mean) <= 2, "{}", middle(&mut h));
    h.state_mut().set_raster_visible(rain, false);
    assert_eq!(middle(&mut h), ground.r());
}

#[test]
#[ignore = "gpu"]
fn the_flat_view_mixes_a_layer_with_the_layers_below_by_its_blend_mode() {
    use vale_store::BlendMode;
    let _gpu = one_gpu_test();
    let mut h = flat_harness();
    let height = h.state().globe.raster().unwrap();
    let rain = vale_app::ui::RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 128,
    };
    let rain = h.state_mut().add_raster(rain).unwrap();
    show_as(h.state_mut(), RasterDisplay::Grey);
    let (low, high) = (16000u16, 50000u16);
    let state = h.state_mut();
    state.globe.raster_map_mut(height).unwrap().fill(low);
    state.globe.raster_map_mut(rain).unwrap().fill(high);
    h.run_steps(3);
    let unit = |level: u16| f32::from(level) / 65535.0;
    let middle = |h: &mut App| {
        let image = canvas_image(h);
        let center = h.state().globe.rect.center();
        f32::from(grey(h, &image, center)) / 255.0
    };
    let (b, s) = (unit(low), unit(high));
    let near = |got: f32, want: f32, what: &str| {
        assert!(
            (got - want).abs() <= 3.0 / 255.0,
            "{what}: {got} and {want}"
        );
    };
    // A layer with the mode Normal hides the layer below it.
    near(middle(&mut h), s, "normal");
    // A blend with an opacity of 100 % shows the mode.
    let state = h.state_mut();
    state.set_raster_blend(rain, BlendMode::Multiply);
    near(middle(&mut h), b * s, "multiply");
    h.state_mut().set_raster_blend(rain, BlendMode::Screen);
    near(middle(&mut h), b + s - b * s, "screen");
    h.state_mut().set_raster_blend(rain, BlendMode::Difference);
    near(middle(&mut h), (b - s).abs(), "difference");
    h.state_mut().set_raster_blend(rain, BlendMode::HardLight);
    near(
        middle(&mut h),
        b + (2.0 * s - 1.0) - b * (2.0 * s - 1.0),
        "hard light",
    );
    h.state_mut().set_raster_blend(rain, BlendMode::Overlay);
    let overlay = if b <= 0.5 {
        2.0 * b * s
    } else {
        s + (2.0 * b - 1.0) - s * (2.0 * b - 1.0)
    };
    near(middle(&mut h), overlay, "overlay");
    // The opacity mixes the result with the layer below.
    h.state_mut().set_raster_blend(rain, BlendMode::Multiply);
    h.state_mut().set_raster_opacity(rain, 0.5);
    near(middle(&mut h), 0.5 * b + 0.5 * b * s, "multiply at half");
    // Over the ground alone, a blend shows the ground as the backdrop.
    h.state_mut().set_raster_opacity(rain, 1.0);
    h.state_mut().set_raster_visible(height, false);
    let ground = f32::from(vale_app::ui::pad::theme::GROUND.r()) / 255.0;
    near(middle(&mut h), ground * s, "multiply over the ground");
}

/// Makes sure that the coast line of `ground` has its weight across the
/// line and its middle on the band edge in the flat view, at each zoom: on
/// the equator across a face edge, and on the circle of latitude through a
/// cube corner.
fn assert_flat_coasts(ground: fn(f64) -> f64, name: &str) {
    let corner = (1.0f64 / 3.0).sqrt().asin().to_degrees();
    for (place, lon, lat) in [("face edge", 45.0, 0.0), ("cube corner", 45.0, corner)] {
        let z = lat.to_radians().sin();
        let mut h = flat_harness();
        lines::measured_scene(&mut h, 64, move |d| ground(d[2] - z));
        for zoom in [1.0, 4.0, 15.0, 40.0] {
            let globe = &mut h.state_mut().globe;
            globe.flat = FlatView::new(spec(ProjectionKind::Equirectangular, 0.0, 0.0));
            globe.flat.zoom = zoom;
            globe.flat.look_at(lon, lat);
            let what = format!("{name}, {place}, zoom {zoom}");
            let found = lines::measure_across(&mut h, &what);
            lines::assert_across(&found, 1.0, 20, &what);
        }
    }
}

#[test]
#[ignore = "gpu"]
fn a_stop_line_on_an_even_slope_has_its_weight_in_the_flat_view() {
    let _gpu = one_gpu_test();
    assert_flat_coasts(lines::even_slope, "even slope");
}

#[test]
#[ignore = "gpu"]
fn a_stop_line_next_to_a_flat_sea_has_its_weight_in_the_flat_view() {
    let _gpu = one_gpu_test();
    assert_flat_coasts(lines::flat_sea, "flat sea");
}

#[test]
#[ignore = "gpu"]
fn a_stop_line_next_to_flat_land_has_its_weight_in_the_flat_view() {
    let _gpu = one_gpu_test();
    assert_flat_coasts(lines::flat_land, "flat land");
}

/// A flat view with a grey height layer of `level`, and right above it an
/// image layer whose west half is transparent and whose east half is red.
/// Returns the image and the screen points west and east of the center.
fn flat_with_image(level: u16) -> (App, vale_store::ImageId, Pos2, Pos2) {
    let mut h = flat_harness();
    let height = h.state().globe.raster().unwrap();
    h.state_mut()
        .globe
        .raster_map_mut(height)
        .unwrap()
        .fill(level);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plates.png");
    images::write_png(&path, &images::half_red(512, 256));
    h.state_mut().run_action(Action::ImportImageLayer(path));
    let id = h.state().doc.project.images()[0].id;
    // The image goes right above the height layer, below the vector layers,
    // so a blend mixes it with the height layer.
    let layer = h.state().doc.project.image_layer(id).unwrap().id;
    h.state_mut().move_layer(layer, 1);
    h.run_steps(3);
    let c = h.state().globe.rect.center();
    (h, id, c - Vec2::new(60.0, 0.0), c + Vec2::new(60.0, 0.0))
}

fn rgb(h: &App, image: &image::RgbaImage, pos: Pos2) -> [u8; 3] {
    let at = pos - h.state().globe.rect.min;
    let p = image.get_pixel(at.x as u32, at.y as u32).0;
    [p[0], p[1], p[2]]
}

fn near(got: [u8; 3], want: [u8; 3], what: &str) {
    let close = got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 3);
    assert!(close, "{what}: {got:?} and {want:?}");
}

#[test]
#[ignore = "gpu"]
fn the_flat_view_draws_an_image_layer_with_its_colors_over_the_layers_below() {
    let _gpu = one_gpu_test();
    let level = 16000;
    let (mut h, _, west, east) = flat_with_image(level);
    let g = (f32::from(level) / 65535.0 * 255.0).round() as u8;
    let image = canvas_image(&mut h);
    near(rgb(&h, &image, east), [255, 0, 0], "opaque");
    near(rgb(&h, &image, west), [g; 3], "transparent");
}

#[test]
#[ignore = "gpu"]
fn the_opacity_and_the_blend_mode_apply_to_an_image_layer_in_the_flat_view() {
    use vale_store::BlendMode;
    let _gpu = one_gpu_test();
    let level = 32768;
    let (mut h, id, west, east) = flat_with_image(level);
    let g = (f32::from(level) / 65535.0 * 255.0).round() as u8;
    let half = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
    h.state_mut().set_image_opacity(id, 0.5);
    let image = canvas_image(&mut h);
    near(rgb(&h, &image, east), [half(255, g), g / 2, g / 2], "half");
    near(rgb(&h, &image, west), [g; 3], "half, transparent");
    h.state_mut().set_image_opacity(id, 1.0);
    h.state_mut().set_image_blend(id, BlendMode::Multiply);
    let image = canvas_image(&mut h);
    near(rgb(&h, &image, east), [g, 0, 0], "multiply");
    near(rgb(&h, &image, west), [g; 3], "multiply, transparent");
    h.state_mut().set_image_blend(id, BlendMode::Screen);
    let image = canvas_image(&mut h);
    near(rgb(&h, &image, east), [255, g, g], "screen");
}

#[test]
fn the_flat_view_draws_a_masked_polygon_only_inside_the_mask() {
    let (doc, id) = masked::masked_square();
    let mut flat = FlatView::default();
    flat.look_at(20.0, 20.0);
    flat.zoom = 4.0;
    let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 800.0));
    let mut pipeline = vale_app::pipeline::Pipeline::new();
    let (fills, strokes) = masked::painted(|painter| {
        vale_app::ui::globe::flat_vector_layer(painter, None, rect, &doc, &flat, id, &mut pipeline);
    });
    assert!(!fills.is_empty() && !strokes.is_empty());
    masked::assert_masked(&fills, &strokes, |p| flat.unproject(rect, p));
}

/// The flat view of the crater of `set_crater` at zoom 4, in two bands with
/// rivers, with the hidden polygon layer of `masked::raster_masked` over it.
fn masked_flat() -> (App, vale_store::RasterId, masked::RasterMasked) {
    let mut h = flat_harness();
    show_as(h.state_mut(), RasterDisplay::Bands);
    let id = h.state().globe.raster().unwrap();
    let globe = &mut h.state_mut().globe;
    two_bands(globe.bands_mut(id).unwrap(), [40, 40, 40], [150, 140, 120]);
    globe.preview.panel = true;
    globe.tool = Tool::Navigate;
    globe.flat.zoom = 4.0;
    set_crater(&mut globe.map, lonlat_to_dir(0.0, 0.0), 800.0);
    let found = masked::raster_masked(h.state_mut(), id);
    for _ in 0..4000 {
        h.step();
        if !h.state().globe.rivers_pending() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (h, id, found)
}

/// The canvas image with the polygon layer hidden, and then with it shown.
fn mask_images(h: &mut App, layer: vale_store::LayerId) -> (image::RgbaImage, image::RgbaImage) {
    masked::show(h.state_mut(), layer, false);
    let plain = canvas_image(h);
    masked::show(h.state_mut(), layer, true);
    let masked = canvas_image(h);
    (plain, masked)
}

#[test]
#[ignore = "gpu"]
fn a_raster_mask_cuts_a_fill_to_the_land_and_the_lakes_of_the_flat_view() {
    let _gpu = one_gpu_test();
    let (mut h, id, found) = masked_flat();
    // A line at a stop that no height crosses makes the flat view draw the
    // band edges from the cut heights.
    let bands = h.state_mut().globe.bands_mut(id).unwrap();
    let (sea, land) = ([40, 40, 40], [150, 140, 120]);
    lines::set_stops(bands, true, &[0.0, 5000.0], &[sea, land, land]);
    let top = bands
        .stops()
        .iter()
        .position(|s| s.value == 5000.0)
        .unwrap();
    let line = vale_terrain::StopLine {
        on: true,
        weight: 3.0,
        color: [0; 3],
        ..Default::default()
    };
    assert!(bands.set_line(top, line));
    let (plain, masked) = mask_images(&mut h, found.layer);
    masked::save_images("mask-flat", &plain, &masked);
    let globe = &h.state().globe;
    let agreement = masked::agreement(&plain, &masked, masked::inside_polygon(globe));
    assert!(
        agreement.compared > 50_000 && agreement.land > 5_000 && agreement.wrong == 0,
        "{agreement:?}"
    );
    let (wet, red) = masked::lake_red(globe, &plain, &masked);
    assert!(wet > 50 && red == 0, "{red} of {wet}");
}

#[test]
#[ignore = "gpu"]
fn a_hidden_raster_layer_masks_a_fill_in_the_flat_view_at_its_smoothing() {
    let _gpu = one_gpu_test();
    let (mut h, id, found) = masked_flat();
    h.state_mut().set_raster_rivers(id, false);
    masked::ripple(&mut h.state_mut().globe.map);
    let sharp = canvas_image(&mut h);
    let state = h.state_mut();
    assert!(state.set_raster_smoothing(id, true));
    assert!(state.set_raster_feature_size(id, 24.0));
    h.step();
    assert!(h.state().globe.smoothing_level(id) >= 1.0);
    masked::show(h.state_mut(), found.layer, false);
    let plain = canvas_image(&mut h);
    masked::show(h.state_mut(), found.raster, false);
    masked::show(h.state_mut(), found.layer, true);
    let masked = canvas_image(&mut h);
    masked::save_images("mask-flat-hidden", &plain, &masked);
    let inside = masked::inside_polygon(&h.state().globe);
    let agreement = masked::agreement(&plain, &masked, &inside);
    assert!(
        agreement.compared > 50_000 && agreement.land > 10_000 && agreement.wrong == 0,
        "{agreement:?}"
    );
    // The smoothing moves the coast.
    let moved = masked::agreement(&sharp, &masked, &inside);
    assert!(moved.wrong > 200, "{moved:?}");
}
