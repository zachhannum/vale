use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::cli::{Args, apply_globe, apply_import};
use vale_app::document::Document;
use vale_app::globe::import::Imported;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Globe, Tool};
use vale_app::headless;
use vale_app::ui::{Action, AppState, draw};
use vale_import::raster;
use vale_terrain::{Equirect, FACES, Heightmap, TexelRect};

const WIDTH: usize = 512;
const HEIGHT: usize = 256;
const FACE_SIZE: usize = 64;

/// The longest time that an import can take.
const LIMIT: Duration = Duration::from_secs(30);

/// A smooth function of the direction that takes each value from -1 to 1.
fn hills(d: [f64; 3]) -> f64 {
    d[0] + 0.5 * d[1] * d[2]
}

/// The level of `hills` at a direction, over the whole 16-bit range.
fn hill_level(d: [f64; 3]) -> f64 {
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    (hills(d.map(|v| v / length)) + 1.0) * 0.5 * 65535.0
}

/// The levels of an equirectangular image, from a function of the longitude
/// and the latitude of each pixel center in degrees.
fn image_levels(width: usize, height: usize, level: impl Fn(f64, f64) -> u16) -> Vec<u16> {
    let mut levels = Vec::with_capacity(width * height);
    for j in 0..height {
        let lat = 90.0 - (j as f64 + 0.5) * 180.0 / height as f64;
        for i in 0..width {
            levels.push(level(-180.0 + (i as f64 + 0.5) * 360.0 / width as f64, lat));
        }
    }
    levels
}

/// The levels of `hills` in an equirectangular image.
fn hill_levels(width: usize, height: usize) -> Vec<u16> {
    image_levels(width, height, |lon, lat| {
        let (lon, lat) = (lon.to_radians(), lat.to_radians());
        hill_level([lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]).round() as u16
    })
}

/// The path of a test file.
fn test_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-import");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Writes a 16-bit greyscale PNG.
fn save_png(name: &str, width: usize, height: usize, levels: Vec<u16>) -> PathBuf {
    let path = test_path(name);
    let image =
        image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width as u32, height as u32, levels);
    image.unwrap().save(&path).unwrap();
    path
}

/// Writes a 16-bit greyscale TIFF.
fn save_tiff(name: &str, width: usize, height: usize, levels: &[u16]) -> PathBuf {
    use tiff::encoder::{TiffEncoder, colortype::Gray16};
    let path = test_path(name);
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = TiffEncoder::new(std::io::BufWriter::new(file)).unwrap();
    let (width, height) = (width as u32, height as u32);
    encoder
        .write_image::<Gray16>(width, height, levels)
        .unwrap();
    path
}

/// Writes a 16-bit PNG of `width` by `height` pixels with the levels of
/// `hills`.
fn write_png(name: &str, width: usize, height: usize) -> PathBuf {
    save_png(name, width, height, hill_levels(width, height))
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

/// The face size of the GPU tests.
const GPU_FACE_SIZE: usize = 256;

/// One test at a time makes a wgpu device. Software adapters fail when two
/// threads make devices at the same time.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
    let _gpu = one_gpu_test();
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

/// A state with no window, on the globe, with a small heightmap.
fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.globe.set_face_size(FACE_SIZE);
    state
}

/// The largest step between two texels that touch: inside the faces, and
/// across the face edges.
fn largest_steps(map: &Heightmap) -> (u16, u16) {
    let n = map.face_size() as i64;
    let (mut inside, mut across) = (0, 0);
    for face in 0..FACES {
        for a in 0..n {
            for b in 0..n - 1 {
                let along_x = map.get(face, b, a).abs_diff(map.get(face, b + 1, a));
                let along_y = map.get(face, a, b).abs_diff(map.get(face, a, b + 1));
                inside = inside.max(along_x).max(along_y);
            }
            // The texel outside the face is on the face next to it.
            for (out, edge) in [(-1, 0), (n, n - 1)] {
                let over_x = map.get(face, out, a).abs_diff(map.get(face, edge, a));
                let over_y = map.get(face, a, out).abs_diff(map.get(face, a, edge));
                across = across.max(over_x).max(over_y);
            }
        }
    }
    (inside, across)
}

/// The most levels between a texel and the function of the image. One 8-bit
/// grey step is 257 levels.
const ERROR_LIMIT: f64 = 8.0;

#[test]
fn a_16_bit_image_comes_in_with_no_loss_and_no_seam() {
    let levels = hill_levels(WIDTH, HEIGHT);
    let tiff = save_tiff("loss.tiff", WIDTH, HEIGHT, &levels);
    let png = save_png("loss.png", WIDTH, HEIGHT, levels);
    let mut maps = Vec::new();
    for path in [png, tiff] {
        let mut state = state();
        state.run_action(Action::ImportHeightmap(path.clone()));
        assert!(state.status.starts_with("Imported"), "{}", state.status);
        let map = &state.globe.map;
        let mut worst = 0.0f64;
        for face in 0..FACES {
            for y in 0..FACE_SIZE {
                for x in 0..FACE_SIZE {
                    let want = hill_level(map.texel_dir(face, x, y));
                    let got = f64::from(map.get(face, x as i64, y as i64));
                    worst = worst.max((got - want).abs());
                }
            }
        }
        assert!(worst <= ERROR_LIMIT, "{worst}");
        let (inside, across) = largest_steps(map);
        assert!(inside > 0 && across <= inside, "{across} > {inside}");
        maps.push(texels(map));
    }
    assert!(maps[0] == maps[1], "the PNG and the TIFF differ");
}

#[test]
fn an_image_of_one_level_gives_that_level_at_each_texel() {
    const LEVEL: u16 = 12_345;
    let levels = vec![LEVEL; WIDTH * HEIGHT];
    let tiff = save_tiff("level.tiff", WIDTH, HEIGHT, &levels);
    let png = save_png("level.png", WIDTH, HEIGHT, levels);
    for path in [png, tiff] {
        let mut state = state();
        state.run_action(Action::ImportHeightmap(path));
        assert!(texels(&state.globe.map).iter().all(|&l| l == LEVEL));
    }
}

#[test]
fn no_seam_shows_on_the_globe_after_an_import() {
    const ZOOM: f64 = 15.0;
    let _gpu = one_gpu_test();
    let path = write_png("seam.png", WIDTH, HEIGHT);
    let mut h = gpu_harness();
    h.state_mut().run_action(Action::ImportHeightmap(path));
    let globe = &mut h.state_mut().globe;
    globe.preview.graticule = false;
    globe.preview.greyscale = true;
    settle(&mut h);
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
    let mut seen = [u8::MAX, 0];
    for place in places {
        let [x, y, z] = place;
        let lat = z.atan2(x.hypot(y)).to_degrees();
        let lon = y.atan2(x).to_degrees();
        let globe = &mut h.state_mut().globe;
        globe.view = GlobeView::centered(lon, lat);
        globe.view.zoom = ZOOM;
        let img = canvas_image(&mut h);
        // The image is smooth, so a seam is a jump of more than one grey step
        // in one pixel.
        for py in 0..img.height() - 1 {
            for px in 0..img.width() - 1 {
                let v = img.get_pixel(px, py).0[0];
                for other in [img.get_pixel(px + 1, py), img.get_pixel(px, py + 1)] {
                    let step = v.abs_diff(other.0[0]);
                    assert!(step <= 1, "{step} at {px}, {py}, place {place:?}");
                }
            }
        }
        let rect = h.state().globe.rect;
        let middle = rect.center() - rect.min.ceil();
        let grey = img.get_pixel(middle.x as u32, middle.y as u32).0[0];
        let want = hill_level(place) / 65535.0 * 255.0;
        assert!(
            (f64::from(grey) - want).abs() <= 2.0,
            "{grey}, {want}, {place:?}"
        );
        seen = [seen[0].min(grey), seen[1].max(grey)];
    }
    assert!(seen[1] - seen[0] > 60, "{seen:?}");
}

#[test]
fn a_stroke_paints_on_the_imported_map() {
    let _gpu = one_gpu_test();
    let path = write_png("paint.png", WIDTH, HEIGHT);
    let mut h = gpu_harness();
    let empty = texels(&h.state().globe.map);
    h.state_mut().run_action(Action::ImportHeightmap(path));
    settle(&mut h);
    let imported = texels(&h.state().globe.map);
    assert!(imported != empty);
    let c = h.state().globe.rect.center();
    let under = h.state().globe.unproject(c).unwrap();
    let before = h.state().globe.map.sample(under);
    assert!(before < u16::MAX);

    let (from, to) = (c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0));
    button(&mut h, from, true);
    h.step();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 8.0)));
        h.step();
    }
    button(&mut h, to, false);
    settle(&mut h);
    assert!(h.state().globe.map.sample(under) > before);

    assert!(h.state_mut().globe.undo());
    assert!(texels(&h.state().globe.map) == imported);
    settle(&mut h);
    assert!(h.state_mut().globe.undo());
    assert!(texels(&h.state().globe.map) == empty);
    assert!(!h.state().globe.can_undo());
}

fn desktop(state: AppState) -> Harness<'static, AppState> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(2);
    h
}

#[test]
fn a_wrong_aspect_ratio_gives_a_warning() {
    let mut h = desktop(state());
    let path = write_png("ratio.png", 300, 200);
    h.state_mut().run_action(Action::ImportHeightmap(path));
    h.run_steps(2);
    let s = h.state();
    assert!(s.status.contains("twice as wide"), "{}", s.status);
    assert!(s.status.contains("300 x 200"), "{}", s.status);
    assert!(s.import_note.as_ref().unwrap().warning);
    assert!(s.globe.map.allocated_tiles() > 0);
    assert!(h.query_by_label_contains("twice as wide").is_some());

    let path = write_png("ratio-good.png", WIDTH, HEIGHT);
    h.state_mut().run_action(Action::ImportHeightmap(path));
    h.run_steps(2);
    let s = h.state();
    assert_eq!(s.status, "Imported ratio-good.png (512 x 256).");
    assert!(!s.import_note.as_ref().unwrap().warning);
    assert!(h.query_by_label_contains("twice as wide").is_none());
    assert!(
        h.query_by_label_contains("Imported ratio-good.png")
            .is_some()
    );
}

#[test]
fn a_file_that_is_not_an_image_gives_a_warning_and_no_change() {
    let mut h = desktop(state());
    let path = write_png("before-text.png", WIDTH, HEIGHT);
    h.state_mut().run_action(Action::ImportHeightmap(path));
    let before = texels(&h.state().globe.map);
    assert!(h.state_mut().globe.undo());
    assert!(!h.state().globe.can_undo());
    let empty = texels(&h.state().globe.map);
    assert!(empty != before);

    let path = test_path("text.png");
    std::fs::write(&path, "This file has no image.").unwrap();
    h.state_mut().run_action(Action::ImportHeightmap(path));
    h.run_steps(2);
    let s = h.state();
    assert!(
        s.status.contains("is not a PNG or TIFF image"),
        "{}",
        s.status
    );
    assert!(s.import_note.as_ref().unwrap().warning);
    assert!(texels(&s.globe.map) == empty);
    assert!(!s.globe.can_undo());
    assert!(
        h.query_by_label_contains("is not a PNG or TIFF image")
            .is_some()
    );
}

fn args(list: &[&str]) -> Args {
    let mut all = vec!["vale-app"];
    all.extend_from_slice(list);
    Args::try_parse_from(all).unwrap()
}

#[test]
fn the_command_line_imports_a_heightmap() {
    let path = write_png("cli.png", WIDTH, HEIGHT);
    let args = args(&[
        "--import-heightmap",
        path.to_str().unwrap(),
        "--face-size",
        "64",
    ]);
    let mut state = AppState::new(Document::sample()).unwrap();
    apply_globe(&args, &mut state.globe, 1024).unwrap();
    apply_import(&args, &mut state, true).unwrap();
    assert_eq!(state.globe.map.face_size(), 64);
    assert!(state.globe.map.allocated_tiles() > 0);
    assert_eq!(state.status, "Imported cli.png (512 x 256).");
    assert!(!state.globe.busy());

    // The window starts the import at its first frame.
    let mut state = AppState::new(Document::sample()).unwrap();
    apply_import(&args, &mut state, false).unwrap();
    assert_eq!(state.actions, [Action::ImportHeightmap(path)]);
    assert!(!state.globe.importing());
}

#[test]
fn the_command_line_fails_for_a_missing_heightmap() {
    let path = test_path("no-such-file.png");
    let args = args(&["--import-heightmap", path.to_str().unwrap()]);
    let mut state = state();
    let error = apply_import(&args, &mut state, true).unwrap_err();
    assert!(error.to_string().starts_with("cannot read "), "{error}");
    assert!(!state.globe.can_undo());

    let mut state = self::state();
    apply_import(&self::args(&[]), &mut state, true).unwrap();
    assert!(state.actions.is_empty() && state.status.is_empty());
}

#[test]
fn the_screenshot_shows_the_imported_heightmap() {
    const DARK: u16 = 8_000;
    const BRIGHT: u16 = 56_000;
    let _gpu = one_gpu_test();
    let west_east = |lon: f64, _| if lon < 0.0 { DARK } else { BRIGHT };
    let levels = image_levels(WIDTH, HEIGHT, west_east);
    let path = save_png("west-east.png", WIDTH, HEIGHT, levels);
    let args = args(&[
        "--import-heightmap",
        path.to_str().unwrap(),
        "--look-at",
        "0,0",
    ]);
    let mut state = state();
    apply_globe(&args, &mut state.globe, FACE_SIZE).unwrap();
    state.globe.preview.greyscale = true;
    state.globe.preview.graticule = false;
    let import = |state: &mut AppState| apply_import(&args, state, true);
    let (img, state) = headless::ui_png_with(state, (1280.0, 800.0), 1.0, import).unwrap();
    assert!(state.status.starts_with("Imported west-east.png"));
    let center = state.globe.rect.center();
    let grey = |dx: f32| f64::from(img.get_pixel((center.x + dx) as u32, center.y as u32).0[0]);
    let want = |level: u16| f64::from(level) / 65535.0 * 255.0;
    // The shade toward the limb takes a few grey steps.
    let (west, east) = (grey(-100.0), grey(100.0));
    assert!((west - want(DARK)).abs() <= 4.0, "{west}");
    assert!(
        (want(BRIGHT) - east) >= 0.0 && (want(BRIGHT) - east) <= 12.0,
        "{east}"
    );
}
