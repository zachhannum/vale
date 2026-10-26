//! Raster layers in the map frames of the Atlas workspace.

use eframe::egui::{Pos2, pos2};
use egui_kittest::Harness;
use kurbo::Point;
use vale_app::atlas::anchor;
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::{AppState, Layout, Workspace};
use vale_store::{BlendMode, RasterDisplay, RasterId};

type App = Harness<'static, AppState>;

/// One GPU test runs at a time, because the software renderer is slow.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// An Atlas workspace with a height layer of one level, shown in grey, and
/// a page with one frame.
fn atlas(level: u16) -> (App, RasterId) {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.workspace = Workspace::Atlas;
    vale_app::ui::atlas::add_frame(&mut state).expect("a page takes a frame");
    state.globe.set_face_size(128);
    let height = state.add_height();
    state.set_raster_display(height, RasterDisplay::Grey);
    state.globe.raster_map_mut(height).unwrap().fill(level);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (1194.0, 834.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(4);
    (h, height)
}

fn open_frame(h: &App) -> vale_store::FrameRecord {
    vale_app::ui::atlas::open_page(h.state()).frames[0].clone()
}

/// The pixel of the screen at a page point in millimeters.
fn screen(h: &App, p: Point) -> Pos2 {
    h.state().atlas.place.unwrap().to_screen(p)
}

fn pixel(h: &mut App, at: Pos2) -> [u8; 3] {
    h.remove_cursor();
    h.run_steps(2);
    let image = h.render().unwrap();
    let p = image.get_pixel(at.x as u32, at.y as u32);
    [p[0], p[1], p[2]]
}

fn grey_of(level: u16) -> u8 {
    (f32::from(level) / 65535.0 * 255.0).round() as u8
}

#[test]
fn a_frame_draws_the_raster_layers_that_its_map_shows() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, height) = atlas(level);
    assert!(h.state().globe.draws(height), "the globe draws the layer");
    let frame = open_frame(&h);
    let middle = screen(&h, anchor(&frame.shape)) + eframe::egui::vec2(23.0, 17.0);
    let [r, g, b] = pixel(&mut h, middle);
    assert!(
        r.abs_diff(grey_of(level)) <= 3 && r == g && g == b,
        "{r} {g} {b}"
    );

    // The paper outside the frame stays white.
    let page = h.state().atlas.place.unwrap().rect;
    let corner = pos2(page.min.x + 2.0, page.min.y + 2.0);
    assert_eq!(pixel(&mut h, corner), [255, 255, 255]);

    // A map that hides the layer does not draw it.
    let layer = h.state().doc.project.raster_layer(height).unwrap().id;
    assert!(h.state_mut().doc.set_shown(frame.map, layer, false));
    h.state_mut().touch();
    let [r, ..] = pixel(&mut h, middle);
    assert!(r.abs_diff(grey_of(level)) > 20, "{r}");
}

#[test]
fn the_blend_mode_works_in_a_frame() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, _below) = atlas(level);
    // A second layer above the first. The blend mode works between the
    // raster layers of the frame.
    let (upper_level, upper) = (30000, {
        let state = h.state_mut();
        let id = state.add_height();
        state.set_raster_display(id, RasterDisplay::Grey);
        state.globe.raster_map_mut(id).unwrap().fill(30000);
        id
    });
    let frame = open_frame(&h);
    let middle = screen(&h, anchor(&frame.shape)) + eframe::egui::vec2(23.0, 17.0);
    h.state_mut().touch();
    h.run_steps(3);
    let [r, ..] = pixel(&mut h, middle);
    assert!(r.abs_diff(grey_of(upper_level)) <= 3, "normal: {r}");
    h.state_mut().set_raster_blend(upper, BlendMode::Multiply);
    h.state_mut().touch();
    h.run_steps(3);
    let [r, g, b] = pixel(&mut h, middle);
    let want = (f32::from(grey_of(level)) * f32::from(grey_of(upper_level)) / 255.0).round() as u8;
    assert!(
        r.abs_diff(want) <= 3 && r == g && g == b,
        "{r} {g} {b} and {want}"
    );
}
