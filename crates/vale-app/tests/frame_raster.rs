//! Raster layers in the map frames of the Atlas workspace.

mod images;

use eframe::egui::{Pos2, pos2};
use egui_kittest::Harness;
use kurbo::Point;
use vale_app::atlas::anchor;
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::{Action, AppState, Layout, Workspace};
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
fn a_raster_layer_switch_changes_only_the_frames_of_its_map() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, height) = atlas(level);
    vale_app::ui::atlas::add_frame(h.state_mut()).expect("a page takes a frame");
    let frames = vale_app::ui::atlas::open_page(h.state()).frames.clone();
    assert_ne!(frames[0].map, frames[1].map);
    let middle = |h: &App, f: &vale_store::FrameRecord| {
        screen(h, anchor(&f.shape)) + eframe::egui::vec2(23.0, 17.0)
    };
    let (a, b) = (middle(&h, &frames[0]), middle(&h, &frames[1]));
    let layer = h.state().doc.project.raster_layer(height).unwrap().id;
    h.state_mut().set_map_shown(frames[0].map, layer, false);
    let [first, ..] = pixel(&mut h, a);
    let [second, ..] = pixel(&mut h, b);
    assert!(first.abs_diff(grey_of(level)) > 20, "{first}");
    assert!(second.abs_diff(grey_of(level)) <= 3, "{second}");
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

/// The atlas of `atlas` with an image layer above the height layer whose
/// west half is transparent and whose east half is red, and the screen
/// points west and east of the middle of the frame.
fn atlas_with_image(level: u16) -> (App, vale_store::ImageId, Pos2, Pos2) {
    let (mut h, _) = atlas(level);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plates.png");
    images::write_png(&path, &images::half_red(512, 256));
    h.state_mut().run_action(Action::ImportImageLayer(path));
    let id = h.state().doc.project.images()[0].id;
    h.state_mut().workspace = Workspace::Atlas;
    h.state_mut().touch();
    h.run_steps(3);
    let frame = open_frame(&h);
    let middle = screen(&h, anchor(&frame.shape));
    let side = eframe::egui::vec2(40.0, 7.0);
    (h, id, middle - side, middle + side)
}

fn near(got: [u8; 3], want: [u8; 3], what: &str) {
    let close = got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 3);
    assert!(close, "{what}: {got:?} and {want:?}");
}

#[test]
fn a_frame_draws_an_image_layer_with_its_colors_over_the_layers_below() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, id, west, east) = atlas_with_image(level);
    assert!(h.state().globe.image_level(id).is_some());
    near(pixel(&mut h, east), [255, 0, 0], "opaque");
    near(pixel(&mut h, west), [grey_of(level); 3], "transparent");
}

#[test]
fn the_opacity_and_the_blend_mode_of_an_image_layer_work_in_a_frame() {
    let _gpu = one_gpu_test();
    let level = 32768;
    let (mut h, id, west, east) = atlas_with_image(level);
    let g = grey_of(level);
    let half = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
    h.state_mut().set_image_opacity(id, 0.5);
    near(pixel(&mut h, east), [half(255, g), g / 2, g / 2], "half");
    h.state_mut().set_image_opacity(id, 1.0);
    h.state_mut().set_image_blend(id, BlendMode::Multiply);
    near(pixel(&mut h, east), [g, 0, 0], "multiply");
    near(pixel(&mut h, west), [g; 3], "multiply, transparent");
    h.state_mut().set_image_blend(id, BlendMode::Screen);
    near(pixel(&mut h, east), [255, g, g], "screen");
}

#[test]
fn a_pdf_export_embeds_the_raster_layers_of_a_frame() {
    let _gpu = one_gpu_test();
    let below_level = 40000;
    let (mut h, _) = atlas(below_level);
    let (upper_level, upper) = (30000, {
        let state = h.state_mut();
        let id = state.add_height();
        state.set_raster_display(id, RasterDisplay::Grey);
        state.globe.raster_map_mut(id).unwrap().fill(30000);
        id
    });
    h.state_mut().set_raster_blend(upper, BlendMode::Multiply);
    h.state_mut().touch();
    h.run_steps(3);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atlas.pdf");
    h.state_mut().headless = false;
    h.state_mut().run_action(Action::ExportAtlas(path.clone()));
    for _ in 0..60 {
        h.run_steps(1);
        if h.state().atlas_export.is_none() {
            break;
        }
    }
    assert!(h.state().atlas_export.is_none(), "the export did not end");
    assert!(
        h.state().status.starts_with("Exported"),
        "{}",
        h.state().status
    );
    let bytes = std::fs::read(path).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(
        raw.contains("/Subtype/Image"),
        "the PDF has no raster image"
    );

    let png = dir.path().join("page.png");
    h.state_mut().run_action(Action::Export(png.clone()));
    for _ in 0..60 {
        h.run_steps(1);
        if h.state().atlas_export.is_none() {
            break;
        }
    }
    assert!(
        h.state().status.starts_with("Exported"),
        "{}",
        h.state().status
    );
    let image = image::open(png).unwrap().to_rgba8();
    let at = anchor(&open_frame(&h).shape) + kurbo::Vec2::new(23.0, 17.0);
    let pixels_per_mm = 300.0 / 25.4;
    let pixel = image.get_pixel((at.x * pixels_per_mm) as u32, (at.y * pixels_per_mm) as u32);
    let want =
        (f32::from(grey_of(below_level)) * f32::from(grey_of(upper_level)) / 255.0).round() as u8;
    assert!(
        pixel[0].abs_diff(want) <= 3 && pixel[0] == pixel[1] && pixel[1] == pixel[2],
        "{pixel:?} and {want}"
    );
}
