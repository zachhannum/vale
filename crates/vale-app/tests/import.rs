use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use vale_app::document::Document;
use vale_app::globe::import::Imported;
use vale_app::globe::{Globe, Tool};
use vale_app::headless;
use vale_app::ui::AppState;
use vale_import::raster;
use vale_terrain::{Equirect, FACES, Heightmap, TexelRect};

const WIDTH: usize = 512;
const HEIGHT: usize = 256;
const FACE_SIZE: usize = 64;

/// The longest time that an import can take.
const LIMIT: Duration = Duration::from_secs(30);

/// Writes a 16-bit PNG of `width` by `height` pixels. The levels are a smooth
/// function of the direction that takes each value of the 16-bit range.
fn write_png(name: &str, width: usize, height: usize) -> PathBuf {
    let mut levels = Vec::with_capacity(width * height);
    for j in 0..height {
        let lat = (90.0 - (j as f64 + 0.5) * 180.0 / height as f64).to_radians();
        for i in 0..width {
            let lon = (-180.0 + (i as f64 + 0.5) * 360.0 / width as f64).to_radians();
            let d = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
            let hills = d[0] + 0.5 * d[1] * d[2];
            levels.push(((hills + 1.0) * 0.5 * 65535.0).round() as u16);
        }
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-import");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let image =
        image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width as u32, height as u32, levels);
    image.unwrap().save(&path).unwrap();
    path
}

/// All texels of a heightmap, face by face.
fn texels(map: &Heightmap) -> Vec<u16> {
    let n = map.face_size();
    let mut out = Vec::new();
    for face in 0..FACES {
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        map.read_rect(face, full, &mut out);
    }
    out
}

/// The texels of a heightmap that got the file with no worker.
fn plain_import(path: &Path, face_size: usize) -> Vec<u16> {
    let image = raster::read_file(path).unwrap();
    let src = Equirect::new(image.width, image.height, image.levels).unwrap();
    let mut map = Heightmap::new(face_size, 0);
    map.import_equirect(&src.reduced_for(face_size));
    texels(&map)
}

/// Runs frames of a globe with no GPU until the globe is idle.
fn run(globe: &mut Globe) {
    let start = Instant::now();
    while globe.busy() {
        assert!(start.elapsed() < LIMIT, "the import did not end");
        globe.advance(Instant::now());
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_worker_imports_the_image_in_frames() {
    let path = write_png("frames.png", WIDTH, HEIGHT);
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path.clone());
    assert!(globe.importing() && globe.busy());
    let progress = globe.import_progress().unwrap();
    assert_eq!(progress, "Importing frames.png: reading the file");
    // The frames continue while the import runs.
    assert!(globe.advance(Instant::now()));
    assert!(!globe.can_undo());
    run(&mut globe);

    assert!(!globe.importing());
    assert_eq!(globe.import_progress(), None);
    let levels = texels(&globe.map);
    assert!(
        levels == plain_import(&path, FACE_SIZE),
        "the texels differ"
    );
    let (low, high) = (levels.iter().min().unwrap(), levels.iter().max().unwrap());
    assert!(*low < 2000 && *high > 63000, "{low} to {high}");
    let imported = Imported {
        name: "frames.png".to_string(),
        width: WIDTH,
        height: HEIGHT,
        two_to_one: true,
    };
    assert_eq!(imported.message(), "Imported frames.png (512 x 256).");
    assert_eq!(globe.take_import_result(), Some(Ok(imported)));
    assert_eq!(globe.take_import_result(), None);

    // The import is one undo entry.
    assert!(globe.undo());
    assert_eq!(globe.map.allocated_tiles(), 0);
    assert!(!globe.can_undo());
}

#[test]
fn wait_import_gives_the_same_map_in_one_call() {
    let path = write_png("wait.png", WIDTH, HEIGHT);
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path.clone());
    globe.wait_import();
    assert!(!globe.importing());
    assert!(texels(&globe.map) == plain_import(&path, FACE_SIZE));
    assert!(matches!(globe.take_import_result(), Some(Ok(i)) if i.two_to_one));
    assert!(globe.can_undo());
}

#[test]
fn an_image_that_is_not_two_to_one_covers_the_globe() {
    let path = write_png("square.png", 256, 256);
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path.clone());
    globe.wait_import();
    assert!(texels(&globe.map) == plain_import(&path, FACE_SIZE));
    let imported = globe.take_import_result().unwrap().unwrap();
    assert!(!imported.two_to_one);
    assert_eq!(
        imported.message(),
        "Imported square.png. The image is 256 x 256. A full-globe image is twice as wide as \
         it is tall, so the app stretched the image over the globe."
    );
}

#[test]
fn a_new_face_size_drops_the_import() {
    let path = write_png("drop.png", WIDTH, HEIGHT);
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path);
    // The import is in the middle of the faces.
    let start = Instant::now();
    while globe.import_progress().unwrap() != "Importing drop.png: face 3 of 6" {
        assert!(start.elapsed() < LIMIT, "the import did not start");
        globe.advance(Instant::now());
    }
    assert!(globe.map.allocated_tiles() > 0);
    globe.set_face_size(32);
    assert!(!globe.busy() && !globe.importing());
    std::thread::sleep(Duration::from_millis(50));
    assert!(!globe.advance(Instant::now()));
    assert_eq!(globe.map.face_size(), 32);
    assert_eq!(globe.map.allocated_tiles(), 0);
    assert!(!globe.can_undo());
    assert_eq!(globe.take_import_result(), None);
}

#[test]
fn a_missing_file_gives_an_error_and_no_undo_entry() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/app/no-such-file.png");
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path);
    run(&mut globe);
    let text = globe.take_import_result().unwrap().unwrap_err();
    assert!(text.starts_with("cannot read "), "{text}");
    assert!(text.contains("no-such-file.png"), "{text}");
    assert!(!globe.can_undo());
    assert_eq!(globe.map.allocated_tiles(), 0);
}

#[test]
fn a_second_import_fails_while_the_first_runs() {
    let path = write_png("first.png", WIDTH, HEIGHT);
    let mut globe = Globe::with_face_size(FACE_SIZE);
    globe.start_import(path.clone());
    globe.start_import(PathBuf::from("second.png"));
    let text = globe.take_import_result().unwrap().unwrap_err();
    assert_eq!(
        text,
        "cannot import second.png: the import of first.png is not complete"
    );
    assert!(globe.importing());
    run(&mut globe);
    assert!(matches!(globe.take_import_result(), Some(Ok(i)) if i.name == "first.png"));
    assert!(texels(&globe.map) == plain_import(&path, FACE_SIZE));
}

/// The face size of the GPU test.
const GPU_FACE_SIZE: usize = 256;

/// A harness with the wgpu renderer and the brush tool. Each step renders,
/// so each step runs the GPU work of the globe.
fn gpu_harness() -> Harness<'static, AppState> {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.globe.set_face_size(GPU_FACE_SIZE);
    state.globe.tool = Tool::Brush;
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (640.0, 480.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

/// Runs frames until the globe is idle.
fn settle(h: &mut Harness<'static, AppState>) {
    let start = Instant::now();
    loop {
        h.step();
        if !h.state().globe.busy() {
            return;
        }
        assert!(start.elapsed() < LIMIT, "the globe did not become idle");
    }
}

fn button(h: &mut Harness<'static, AppState>, pos: Pos2, down: bool) {
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: down,
        modifiers: egui::Modifiers::NONE,
    });
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
fn an_import_during_a_stroke_waits_for_the_stroke() {
    let path = write_png("stroke.png", WIDTH, HEIGHT);
    let imported = plain_import(&path, GPU_FACE_SIZE);
    let mut h = gpu_harness();
    let empty = texels(&h.state().globe.map);
    let c = h.state().globe.rect.center();
    let under = h.state().globe.unproject(c).unwrap();
    let base = h.state().globe.map.base();
    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    button(&mut h, from, true);
    h.step();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 8.0)));
        h.step();
        if i == 6 {
            // The button is down, and the stroke is on the GPU only.
            let globe = &mut h.state_mut().globe;
            assert!(globe.busy() && !globe.map.can_undo());
            globe.start_import(path.clone());
            // The call does not wait for the faces while the stroke is open.
            globe.wait_import();
            let progress = globe.import_progress().unwrap();
            assert_eq!(progress, "Importing stroke.png: face 1 of 6");
            assert!(!globe.map.can_undo());
        }
    }
    button(&mut h, to, false);
    settle(&mut h);

    let globe = &mut h.state_mut().globe;
    assert!(matches!(globe.take_import_result(), Some(Ok(_))));
    assert!(texels(&globe.map) == imported, "the texels differ");
    let after_import = canvas_image(&mut h);

    // The first undo takes the import away, and the stroke shows again.
    assert!(h.state_mut().globe.undo());
    let painted = texels(&h.state().globe.map);
    assert!(painted != empty && painted != imported);
    assert!(h.state().globe.map.sample(under) > base);
    settle(&mut h);
    assert!(h.state_mut().globe.undo());
    assert!(texels(&h.state().globe.map) == empty);
    assert!(!h.state().globe.can_undo());
    drop(h);

    let mut plain = gpu_harness();
    plain.state_mut().globe.start_import(path);
    plain.state_mut().globe.wait_import();
    settle(&mut plain);
    assert!(texels(&plain.state().globe.map) == imported);
    let image = canvas_image(&mut plain);
    assert!(
        image.as_raw() == after_import.as_raw(),
        "the render differs"
    );
}
