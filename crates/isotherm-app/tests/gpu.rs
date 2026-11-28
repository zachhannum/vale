//! The globe on the GPU.

mod screenshot;

use eframe::egui;
use isotherm_app::app::{IsothermApp, Layout, Workspace};
use isotherm_app::globe::WorldView;
use isotherm_app::headless::{PIXELS_PER_POINT, gpu_harness, gpu_harness_with};
use vale_shell::pad::theme;
use vale_terrain::math::lonlat_to_dir;

fn render(size: (f32, f32)) -> (image::RgbaImage, isotherm_app::app::IsothermApp) {
    let mut harness = gpu_harness(size, None);
    harness.run_steps(4);
    harness.render().expect("renders");
    harness.step();
    let image = harness.render().expect("renders");
    (image, harness.into_state())
}

/// The pixel at a longitude and a latitude.
fn pixel(
    image: &image::RgbaImage,
    app: &isotherm_app::app::IsothermApp,
    lon: f64,
    lat: f64,
) -> image::Rgba<u8> {
    let globe = &app.globe;
    let at = globe
        .view
        .project(globe.rect, lonlat_to_dir(lon, lat))
        .expect("visible")
        * PIXELS_PER_POINT;
    *image.get_pixel(at.x as u32, at.y as u32)
}

#[test]
#[ignore = "gpu"]
fn the_globe_draws_the_sample_world() {
    let (image, app) = render((1194.0, 834.0));
    let center = app.globe.rect.center() * PIXELS_PER_POINT;
    let middle = image.get_pixel(center.x as u32, center.y as u32);
    let back = theme::CANVAS;
    assert_ne!(middle.0[..3], [back.r(), back.g(), back.b()], "{middle:?}");
}

#[test]
#[ignore = "gpu"]
fn the_sea_is_bluer_than_the_land() {
    let (image, app) = render((1194.0, 834.0));
    // The Atlantic Ocean and the Sahara.
    let sea = pixel(&image, &app, -30.0, 20.0);
    let land = pixel(&image, &app, 10.0, 22.0);
    let blue = |p: image::Rgba<u8>| i32::from(p.0[2]) - i32::from(p.0[0]);
    assert!(blue(sea) > blue(land) + 40, "sea {sea:?}, land {land:?}");
}

#[test]
#[ignore = "gpu"]
fn the_cards_are_glass_with_the_gpu() {
    let mut harness = gpu_harness((1194.0, 834.0), None);
    harness.run_steps(2);
    assert!(vale_shell::pad::widgets::is_glass(&harness.ctx));
}

/// Renders `app` at the size of an iPad.
fn render_app(app: IsothermApp) -> (image::RgbaImage, IsothermApp) {
    let mut harness = gpu_harness_with((1194.0, 834.0), app);
    let image = isotherm_app::headless::render(&mut harness).expect("renders");
    (image, harness.into_state())
}

fn blue(p: image::Rgba<u8>) -> i32 {
    i32::from(p.0[2]) - i32::from(p.0[0])
}

/// Writes an 8-bit grey PNG: black in the west half, white in the east half.
fn write_png(path: &std::path::Path) {
    let (w, h) = (64u32, 32u32);
    let pixels: Vec<u8> = (0..w * h)
        .map(|i| if i % w < w / 2 { 0 } else { 255 })
        .collect();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
}

#[test]
#[ignore = "gpu"]
fn the_imported_world_draws_land_and_sea() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("Halves.png");
    write_png(&png);
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.data_dir = Some(dir.path().to_path_buf());
    app.import_path(&png);
    let Some(isotherm_app::app::Pending::Scale { name, samples, .. }) = app.pending.take() else {
        panic!("no scale form: {:?}", app.status);
    };
    let terrain =
        isotherm_app::document::scale_terrain(&samples, isotherm_import::Scale::default()).unwrap();
    app.doc.import(&name, terrain, Some(dir.path())).unwrap();
    app.globe.set_terrain(&app.doc.world.terrain.heights);
    // The globe looks at longitude 0, between the halves.
    app.globe.view = vale_globe::view::GlobeView::centered(0.0, 0.0);
    let (image, app) = render_app(app);
    let west = pixel(&image, &app, -40.0, 0.0);
    let east = pixel(&image, &app, 40.0, 0.0);
    assert!(blue(west) > blue(east) + 40, "west {west:?}, east {east:?}");
}

#[test]
#[ignore = "gpu"]
fn the_coast_moves_with_the_sea_level() {
    let (image, app) = render_app(IsothermApp::sample(Layout::Pad).unwrap());
    // The Sahara is land at 0 m, and sea at 2,000 m.
    let land = pixel(&image, &app, 10.0, 22.0);
    let mut app = app;
    let uploads = app.globe.uploads();
    app.doc.world.terrain.sea_level_m = 2000.0;
    let (image, app) = render_app(app);
    let sea = pixel(&image, &app, 10.0, 22.0);
    assert!(
        blue(sea) > blue(land) + 40,
        "before {land:?}, after {sea:?}"
    );
    assert_eq!(app.globe.uploads(), uploads);
}

#[test]
#[ignore = "gpu"]
fn the_flat_view_draws_the_world() {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.globe.world_view = WorldView::Flat;
    let (image, app) = render_app(app);
    let at = |lon: f64, lat: f64| {
        let globe = &app.globe;
        let p = globe
            .flat
            .project(globe.rect, lonlat_to_dir(lon, lat))
            .expect("on the map")
            * PIXELS_PER_POINT;
        *image.get_pixel(p.x as u32, p.y as u32)
    };
    // The Pacific Ocean and the Sahara, on each side of the map.
    let sea = at(-150.0, 0.0);
    let land = at(10.0, 22.0);
    assert!(blue(sea) > blue(land) + 40, "sea {sea:?}, land {land:?}");
}

/// Saves a picture of each workspace in the desktop, iPad, and iPhone
/// layouts to `target/isotherm` when `VALE_SCREENSHOTS` is set.
#[test]
#[ignore = "gpu"]
fn manual_screenshots_of_each_workspace() {
    let phone = egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 59.0,
        bottom: 34.0,
    });
    let layouts = [
        ("desktop", (1440.0, 900.0), Layout::Desktop, None),
        ("pad", (1194.0, 834.0), Layout::Pad, None),
        ("phone", (390.0, 844.0), Layout::Pad, Some(phone)),
    ];
    for (name, size, layout, safe_area) in layouts {
        for workspace in Workspace::ALL {
            let mut app = IsothermApp::sample(layout).unwrap();
            app.workspace = workspace;
            app.safe_area = safe_area;
            let mut harness = gpu_harness_with(size, app);
            harness.run_steps(4);
            harness.render().unwrap();
            harness.step();
            let file = format!("{name}-{}", workspace.name().to_lowercase());
            screenshot::save(&mut harness, &file);
        }
    }
}
