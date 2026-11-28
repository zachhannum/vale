//! The globe on the GPU.

use isotherm_app::app::Workspace;
use isotherm_app::headless::{PIXELS_PER_POINT, gpu_harness};
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

/// Writes pictures of the hint card and of the open phone menu to
/// `target/isotherm` for review.
#[test]
#[ignore = "gpu"]
fn manual_screenshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/isotherm");
    std::fs::create_dir_all(&dir).unwrap();
    let mut harness = gpu_harness((1194.0, 834.0), None);
    harness.state_mut().workspace = Workspace::Worlds;
    harness.run_steps(4);
    harness.render().unwrap();
    harness.step();
    harness
        .render()
        .unwrap()
        .save(dir.join("worlds.png"))
        .unwrap();

    let mut harness = gpu_harness((393.0, 852.0), None);
    harness.state_mut().menu = true;
    harness.run_steps(4);
    harness.render().unwrap();
    harness.step();
    harness
        .render()
        .unwrap()
        .save(dir.join("menu.png"))
        .unwrap();
}
