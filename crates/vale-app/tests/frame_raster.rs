//! Raster layers in the map frames of the Atlas workspace.

mod images;
mod striped;

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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
fn a_frame_draws_an_image_layer_with_its_colors_over_the_layers_below() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, id, west, east) = atlas_with_image(level);
    assert!(h.state().globe.image_level(id).is_some());
    near(pixel(&mut h, east), [255, 0, 0], "opaque");
    near(pixel(&mut h, west), [grey_of(level); 3], "transparent");
}

#[test]
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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

#[test]
#[ignore = "gpu"]
fn a_raster_override_shows_in_the_frames_of_its_map_only() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, height) = atlas(level);
    vale_app::ui::atlas::add_frame(h.state_mut()).expect("a page takes a frame");
    let frames = vale_app::ui::atlas::open_page(h.state()).frames.clone();
    let middle = |h: &App, f: &vale_store::FrameRecord| {
        screen(h, anchor(&f.shape)) + eframe::egui::vec2(23.0, 17.0)
    };
    let (a, b) = (middle(&h, &frames[0]), middle(&h, &frames[1]));
    let layer = h.state().doc.project.raster_layer(height).unwrap().id;
    let state = h.state_mut();
    let mut look = state.look(None, layer).unwrap();
    look.opacity = 0.0;
    state.set_look(Some(frames[1].map), layer, look);
    let [first, ..] = pixel(&mut h, a);
    let [second, ..] = pixel(&mut h, b);
    assert!(first.abs_diff(grey_of(level)) <= 3, "{first}");
    assert!(second.abs_diff(grey_of(level)) > 20, "{second}");

    // A change of the style of the layer keeps the override.
    h.state_mut().set_raster_blend(height, BlendMode::Screen);
    let map_look = h.state().look(Some(frames[1].map), layer).unwrap();
    assert_eq!((map_look.opacity, map_look.blend), (0.0, BlendMode::Screen));
    let [second, ..] = pixel(&mut h, b);
    assert!(second.abs_diff(grey_of(level)) > 20, "{second}");
    assert_eq!(h.state().doc.project.raster(height).unwrap().opacity, 1.0);
}

#[test]
#[ignore = "gpu"]
fn an_image_override_shows_in_the_frame_of_its_map() {
    let _gpu = one_gpu_test();
    let level = 32768;
    let (mut h, id, _west, east) = atlas_with_image(level);
    let map = open_frame(&h).map;
    let layer = h.state().doc.project.image_layer(id).unwrap().id;
    let state = h.state_mut();
    let mut look = state.look(None, layer).unwrap();
    look.blend = BlendMode::Multiply;
    state.set_look(Some(map), layer, look);
    near(pixel(&mut h, east), [grey_of(level), 0, 0], "multiply");
    let image = h.state().doc.project.image(id).unwrap();
    assert_eq!(image.blend, BlendMode::Normal);
}

#[test]
#[ignore = "gpu"]
fn a_band_color_of_a_map_shows_in_the_frames_of_that_map_only() {
    let _gpu = one_gpu_test();
    let level = 40000;
    let (mut h, height) = atlas(level);
    vale_app::ui::atlas::add_frame(h.state_mut()).expect("a page takes a frame");
    let frames = vale_app::ui::atlas::open_page(h.state()).frames.clone();
    let middle = |h: &App, f: &vale_store::FrameRecord| {
        screen(h, anchor(&f.shape)) + eframe::egui::vec2(23.0, 17.0)
    };
    let (a, b) = (middle(&h, &frames[0]), middle(&h, &frames[1]));
    let layer = h.state().doc.project.raster_layer(height).unwrap().id;
    let state = h.state_mut();
    state.edit_map_raster(frames[1].map, layer, height, |s| {
        s.set_raster_display(height, RasterDisplay::Bands);
        let bands = s.globe.bands_mut(height).unwrap();
        for i in 0..bands.colors().len() {
            bands.set_color(i, [250, 0, 0]);
        }
    });
    near(pixel(&mut h, b), [250, 0, 0], "the map with the colors");
    near(pixel(&mut h, a), [grey_of(level); 3], "the other map");
    let record = h.state().doc.project.raster(height).unwrap();
    assert_eq!(record.display, RasterDisplay::Grey);
}

/// The red values of a row of pixels of the screen.
fn row(h: &mut App, from: Pos2, count: u32) -> Vec<u8> {
    h.remove_cursor();
    h.run_steps(2);
    let image = h.render().unwrap();
    (0..count)
        .map(|i| image.get_pixel(from.x as u32 + i, from.y as u32)[0])
        .collect()
}

#[test]
#[ignore = "gpu"]
fn a_frame_zoomed_on_an_unselected_layer_draws_full_texels() {
    use vale_terrain::math::lonlat_to_dir;
    let _gpu = one_gpu_test();
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.workspace = Workspace::Atlas;
    vale_app::ui::atlas::add_frame(&mut state).expect("a page takes a frame");
    state.globe.set_face_size(8192);
    let height = state.add_height();
    state.set_raster_display(height, RasterDisplay::Grey);
    state.set_raster_smoothing(height, false);
    state.set_raster_hillshade(height, false);
    // The brush paints another layer, which the frame does not show.
    let other = state.add_height();
    let map = vale_app::ui::atlas::open_page(&state).frames[0].map;
    let layer = state.doc.project.raster_layer(other).unwrap().id;
    assert!(state.doc.set_shown(map, layer, false));
    assert_eq!(state.globe.raster(), Some(other));

    // Stripes of two texels around the middle of the frame, which the copy
    // with a quarter of the face makes one grey.
    let center = [10.0, 20.0];
    let (face, a, b) = vale_terrain::face_of(lonlat_to_dir(center[0], center[1]));
    let texel = |s: f64| ((vale_terrain::warp(s) + 1.0) / 2.0 * 8192.0) as usize;
    let (cx, cy) = (texel(a), texel(b));
    let map = state
        .globe
        .raster_map_mut(height)
        .expect("in the main memory");
    for y in cy - 600..cy + 600 {
        for x in cx - 600..cx + 600 {
            let level = if (x / 2) % 2 == 0 { 50000 } else { 15000 };
            map.set(face, x, y, level);
        }
    }
    let id = vale_app::ui::atlas::open_page(&state).frames[0].id;
    let frame = state.doc.project.atlas.frame_mut(id).unwrap();
    frame.center = center;
    // One texel is about 3 millimeters on the page.
    frame.scale = 4.0e5;
    state.touch();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (1194.0, 834.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(4);
    let frame = open_frame(&h);
    let start = screen(&h, anchor(&frame.shape)) + eframe::egui::vec2(-30.0, 17.0);
    // The pixels of the high stripes and of the low stripes. A line of the
    // graticule is neither.
    let stripes = |values: &[u8]| {
        let high = values.iter().filter(|r| **r > 150).count();
        let low = values.iter().filter(|r| **r < 100).count();
        (high, low)
    };

    // With no memory for the detail maps, the frame draws the copy.
    h.state_mut().globe.set_detail_budget(Some(0));
    h.run_steps(4);
    assert_eq!(h.state().globe.frame_detail_level(id.0, height), None);
    let flat = row(&mut h, start, 60);
    assert_eq!(stripes(&flat), (0, 0), "{flat:?}");

    h.state_mut().globe.set_detail_budget(None);
    for _ in 0..100 {
        h.run_steps(1);
        let globe = &h.state().globe;
        if globe.frame_detail_level(id.0, height).is_some() && globe.frame_details_ready(id.0) {
            break;
        }
    }
    let globe = &h.state().globe;
    assert_eq!(globe.frame_detail_level(id.0, height), Some(0));
    assert!(globe.frame_details_ready(id.0));
    let sharp = row(&mut h, start, 60);
    let (high, low) = stripes(&sharp);
    assert!(high >= 10 && low >= 10, "{sharp:?}");
}

/// Exports the open page to a PNG through the export job of the app.
fn export_png(h: &mut App, path: &std::path::Path) -> image::RgbaImage {
    h.state_mut().headless = false;
    h.state_mut().run_action(Action::Export(path.to_path_buf()));
    let start = std::time::Instant::now();
    while h.state().atlas_export.is_some() {
        assert!(start.elapsed().as_secs() < 600, "the export did not end");
        h.run_steps(1);
    }
    let status = &h.state().status;
    assert!(status.starts_with("Exported"), "{status}");
    image::open(path).unwrap().to_rgba8()
}

/// The pixels of the page per millimeter in an export.
const EXPORT_PIXELS_PER_MM: f64 = 300.0 / 25.4;

/// The red values of a row of `count` pixels of an exported page, with
/// its middle at a page point in millimeters.
fn export_row(image: &image::RgbaImage, middle: Point, count: u32) -> Vec<u8> {
    let x = (middle.x * EXPORT_PIXELS_PER_MM) as u32 - count / 2;
    let y = (middle.y * EXPORT_PIXELS_PER_MM) as u32;
    (0..count).map(|i| image.get_pixel(x + i, y)[0]).collect()
}

/// The pixels of a row that show a high stripe, and that show a low
/// stripe.
fn stripes(values: &[u8]) -> (usize, usize) {
    let high = values.iter().filter(|r| **r > 150).count();
    let low = values.iter().filter(|r| **r < 100).count();
    (high, low)
}

/// A file-backed project whose frame zooms in on a layer that is not
/// selected, in the Globe workspace, so that no frame on the screen loads
/// the tiles.
fn striped_app(dir: &std::path::Path, scale: f64) -> (App, RasterId, vale_store::FrameId) {
    let (mut state, height, id) = striped::project(&dir.join("world.gpkg"), scale);
    state.workspace = Workspace::Globe;
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (1194.0, 834.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(4);
    (h, height, id)
}

#[test]
#[ignore = "gpu"]
fn the_export_waits_for_detail_tiles() {
    let _gpu = one_gpu_test();
    let dir = tempfile::tempdir().unwrap();
    let (mut h, height, _) = striped_app(dir.path(), 4.0e5);
    assert!(h.state().globe.raster_map(height).is_none());
    assert_eq!(h.state().globe.detail_bytes(), 0);
    let image = export_png(&mut h, &dir.path().join("page.png"));
    assert!(h.state().export_waits > 0, "the export did not wait");
    assert_eq!(h.state().export_strips, 1);
    // A stripe is about 6 millimeters, or 70 pixels, on the page.
    let middle = anchor(&open_frame(&h).shape) + kurbo::Vec2::new(3.0, 1.0);
    let values = export_row(&image, middle, 600);
    let (high, low) = stripes(&values);
    assert!(high >= 100 && low >= 100, "{values:?}");
}

#[test]
#[ignore = "gpu"]
fn an_export_over_the_detail_budget_draws_in_strips_with_no_seam() {
    let _gpu = one_gpu_test();
    let dir = tempfile::tempdir().unwrap();
    // A texel is about 1.6 pixels on the page. The frame needs more than
    // one texture of 64 tiles in the pool, and a strip needs one.
    let (mut h, _, _) = striped_app(dir.path(), 6.4e6);
    h.state_mut().globe.set_detail_budget(Some(10 << 20));
    let strips = export_png(&mut h, &dir.path().join("strips.png"));
    let count = h.state().export_strips;
    assert!(count >= 2, "{count} strips");
    let frame = open_frame(&h);

    h.state_mut().globe.set_detail_budget(None);
    h.state_mut().export_strips = 0;
    let whole = export_png(&mut h, &dir.path().join("whole.png"));
    assert_eq!(h.state().export_strips, 1);

    // The stripes show in the middle of the frame.
    let middle = anchor(&frame.shape);
    let values = export_row(&strips, middle, 400);
    let (high, low) = stripes(&values);
    assert!(high >= 80 && low >= 80, "{values:?}");

    // The export in strips is the export in one piece.
    let off = strips
        .pixels()
        .zip(whole.pixels())
        .filter(|(a, b)| a.0.iter().zip(b.0).any(|(a, b)| a.abs_diff(b) > 8))
        .count();
    assert!(off <= 100, "{off} pixels differ");

    // The rows on each side of each strip boundary show the stripes.
    let bbox = vale_app::atlas::shape_bbox(&frame.shape);
    let ratio = vale_app::atlas_export::RASTER_PIXEL_RATIO;
    let rows = (bbox.height() * vale_app::atlas::PT_PER_MM * ratio).ceil() as u32;
    let strip = (0..8)
        .map(|k| rows.div_ceil(1 << k))
        .find(|h| u64::from(rows.div_ceil(*h)) == count)
        .expect("the height of a strip");
    let top = bbox.y0 * EXPORT_PIXELS_PER_MM;
    let x0 = (middle.x * EXPORT_PIXELS_PER_MM) as u32 - 200;
    let center = (middle.y - bbox.y0) * EXPORT_PIXELS_PER_MM;
    let striped = |b: &u32| (f64::from(*b) - center).abs() < 800.0;
    let boundaries: Vec<u32> = (strip..rows).step_by(strip as usize).collect();
    assert!(boundaries.iter().any(striped), "{boundaries:?}");
    for boundary in boundaries.into_iter().filter(striped) {
        for y in [boundary - 1, boundary] {
            let y = (top + f64::from(y)).round() as u32;
            let a: Vec<u8> = (0..400).map(|i| strips.get_pixel(x0 + i, y)[0]).collect();
            let (high, low) = stripes(&a);
            assert!(high >= 80 && low >= 80, "row {y}: {a:?}");
        }
    }
}
