//! The globe on the GPU.

mod screenshot;
mod zonal;

use eframe::egui;
use isotherm_app::app::{IsothermApp, Layout, Workspace};
use isotherm_app::explore::Layer;
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
    app.folders = Some(isotherm_app::worlds::Folders::new(
        dir.path().to_path_buf(),
        dir.path().to_path_buf(),
    ));
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

/// The app in the Explore workspace with the zonal field.
fn explore_app(view: WorldView, contours: bool) -> IsothermApp {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.workspace = Workspace::Explore;
    app.set_means(Some(zonal::zonal_means()));
    app.explore.contours = contours;
    app.globe.world_view = view;
    app
}

/// The pixel of a place on the globe or on the flat map.
fn place(image: &image::RgbaImage, app: &IsothermApp, lon: f64, lat: f64) -> image::Rgba<u8> {
    let globe = &app.globe;
    let d = lonlat_to_dir(lon, lat);
    let p = match globe.world_view {
        WorldView::Globe => globe.view.project(globe.rect, d),
        WorldView::Flat => globe.flat.project(globe.rect, d),
    };
    let p = p.expect("visible") * PIXELS_PER_POINT;
    *image.get_pixel(p.x as u32, p.y as u32)
}

/// Red minus blue: larger for a warmer color of the temperature.
fn warmth(p: image::Rgba<u8>) -> i32 {
    i32::from(p.0[0]) - i32::from(p.0[2])
}

#[test]
#[ignore = "gpu"]
fn the_equator_is_warmer_in_color_than_the_pole() {
    for view in [WorldView::Globe, WorldView::Flat] {
        // The coastline is off, so that no stroke covers a sample.
        let mut app = explore_app(view, false);
        app.globe.coastline = false;
        let (image, app) = render_app(app);
        let equator = place(&image, &app, 10.0, 2.0);
        let pole = place(&image, &app, 10.0, 75.0);
        assert!(
            warmth(equator) > warmth(pole) + 80,
            "{view:?}: equator {equator:?}, pole {pole:?}"
        );
    }
}

/// The smallest channel: larger for a whiter color.
fn whiteness(p: image::Rgba<u8>) -> i32 {
    i32::from(p.0[0].min(p.0[1]).min(p.0[2]))
}

#[test]
#[ignore = "gpu"]
fn the_ice_layer_is_whiter_at_the_pole_than_at_the_equator() {
    for view in [WorldView::Globe, WorldView::Flat] {
        let mut app = explore_app(view, false);
        app.set_means(Some(zonal::zonal_means_with_ice()));
        app.explore.layer = Layer::IceCover;
        app.globe.coastline = false;
        let (image, app) = render_app(app);
        let equator = place(&image, &app, 10.0, 2.0);
        let pole = place(&image, &app, 10.0, 75.0);
        assert!(
            whiteness(pole) > whiteness(equator) + 80,
            "{view:?}: equator {equator:?}, pole {pole:?}"
        );
    }
}

/// The count of the dark pixels of the contours in the canvas.
fn dark_pixels(image: &image::RgbaImage, app: &IsothermApp) -> usize {
    let r = app.globe.rect * PIXELS_PER_POINT;
    let mut count = 0;
    for y in (r.top() as u32..r.bottom() as u32).step_by(2) {
        for x in (r.left() as u32..r.right() as u32).step_by(2) {
            let p = image.get_pixel(x, y).0;
            if p[0] < 60 && p[1] < 60 && p[2] < 70 && p != [0, 0, 0, 255] {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore = "gpu"]
fn the_contours_draw_lines_on_the_globe_and_the_flat_map() {
    for view in [WorldView::Globe, WorldView::Flat] {
        let (on, app) = render_app(explore_app(view, true));
        let (off, _) = render_app(explore_app(view, false));
        let (lines, none) = (dark_pixels(&on, &app), dark_pixels(&off, &app));
        assert!(
            lines > none + 200,
            "{view:?}: {lines} with lines, {none} without"
        );
    }
}

/// The count of the pixels near the color of the coastline in the canvas.
fn coast_pixels(image: &image::RgbaImage, app: &IsothermApp) -> usize {
    let r = app.globe.rect * PIXELS_PER_POINT;
    let [cr, cg, cb] = isotherm_app::globe::COAST_COLOR.map(i32::from);
    let mut count = 0;
    for y in r.top() as u32..r.bottom() as u32 {
        for x in r.left() as u32..r.right() as u32 {
            let p = image.get_pixel(x, y).0.map(i32::from);
            if (p[0] - cr).abs() < 30 && (p[1] - cg).abs() < 30 && (p[2] - cb).abs() < 40 {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore = "gpu"]
fn the_coastline_draws_in_blue_over_the_temperature() {
    for view in [WorldView::Globe, WorldView::Flat] {
        let (on, app) = render_app(explore_app(view, false));
        let mut off = explore_app(view, false);
        off.globe.coastline = false;
        let (off, _) = render_app(off);
        let (lines, none) = (coast_pixels(&on, &app), coast_pixels(&off, &app));
        assert!(
            lines > none + 500,
            "{view:?}: {lines} with the coastline, {none} without"
        );
    }
}

/// Saves pictures of the Explore workspace on an iPad, an iPhone, and the
/// desktop to `target/isotherm` when `VALE_SCREENSHOTS` is set.
#[test]
#[ignore = "gpu"]
fn manual_screenshots_of_the_explore_workspace() {
    let phone = egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 59.0,
        bottom: 34.0,
    });
    let layouts = [
        ("pad", (1194.0, 834.0), Layout::Pad, None),
        ("phone", (393.0, 852.0), Layout::Pad, Some(phone)),
        ("desktop", (1440.0, 900.0), Layout::Desktop, None),
    ];
    for (name, size, layout, safe_area) in layouts {
        for view in [WorldView::Globe, WorldView::Flat] {
            let mut app = explore_app(view, true);
            app.layout = layout;
            app.safe_area = safe_area;
            app.explore.pin = Some(lonlat_to_dir(10.0, 20.0));
            let mut harness = gpu_harness_with(size, app);
            harness.run_steps(4);
            harness.render().unwrap();
            harness.step();
            let file = format!("explore-{name}-{}", view.name().to_lowercase());
            screenshot::save(&mut harness, &file);
        }
    }
}

/// Saves a picture of the question before a change that deletes the run,
/// on the iPad and the iPhone, when `VALE_SCREENSHOTS` is set.
#[test]
#[ignore = "gpu"]
fn manual_screenshots_of_the_question() {
    for (name, size) in [("pad", (1194.0, 834.0)), ("phone", (390.0, 844.0))] {
        let mut app = IsothermApp::sample(Layout::Pad).unwrap();
        app.workspace = Workspace::Planet;
        let sim = isotherm_app::sim::Simulation::new(&app.doc.world, 8, false);
        app.run.borrow_mut().sim = Some(sim);
        app.ask = Some(isotherm_app::app::Ask::Undo);
        let mut harness = gpu_harness_with(size, app);
        harness.run_steps(20);
        harness.render().unwrap();
        harness.step();
        screenshot::save(&mut harness, &format!("{name}-ask"));
    }
}
