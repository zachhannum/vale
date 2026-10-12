use std::path::{Path, PathBuf};

use clap::Parser;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;
use vale_app::cli::{Args, export_heightmap};
use vale_app::document::Document;
use vale_app::files::ExportEvent;
use vale_app::session::Session;
use vale_app::ui::panels::EXPORT_GEOTIFF;
use vale_app::ui::{Action, AppState, Workspace, draw};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{EquirectGrid, Heightmap, Limits, Mode, Stamp};

const FACE_SIZE: usize = 64;
const BASE: u16 = 19114;

/// One stroke of one stamp at a place.
fn paint(map: &mut Heightmap, lon: f64, lat: f64, radius: f64) {
    map.begin_stroke();
    map.stamp(&Stamp {
        center: lonlat_to_dir(lon, lat),
        radius,
        hardness: 0.3,
        flow: 1.0,
        mode: Mode::Raise,
        level: 0,
        strength: 9000.0,
        talus: 0.0,
        scale: 0.0,
        seed: 0,
        limits: Limits::default(),
    });
    assert!(map.end_stroke());
}

/// The levels of the equirectangular image of a heightmap, row by row.
fn equirect(map: &Heightmap) -> Vec<u16> {
    let grid = EquirectGrid::for_face_size(map.face_size());
    let mut levels = vec![0; grid.width() * grid.height()];
    for (row, out) in levels.chunks_mut(grid.width()).enumerate() {
        map.equirect_row(&grid, row, out);
    }
    levels
}

/// The size and the levels of a TIFF file.
fn read_tiff(path: &Path) -> ((u32, u32), Vec<u16>) {
    let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let size = decoder.dimensions().unwrap();
    match decoder.read_image().unwrap() {
        DecodingResult::U16(levels) => (size, levels),
        _ => panic!("the samples of {} are not 16-bit", path.display()),
    }
}

/// Makes a project file of a world with this radius. With `painted`, the
/// world has a heightmap with three painted places.
fn project(path: &Path, radius_km: f64, painted: bool) {
    let mut doc = Document::sample();
    doc.set_radius_km(radius_km);
    let mut session = Session::open(path, &mut doc).unwrap();
    if painted {
        let mut map = Heightmap::new(FACE_SIZE, BASE);
        paint(&mut map, 45.0, 35.3, 0.4);
        paint(&mut map, -100.0, 5.0, 0.2);
        paint(&mut map, 0.0, 90.0, 0.3);
        session.flush(&mut doc.project, &mut map).unwrap();
    }
}

fn args(project: &Path, out: &Path) -> Args {
    let (project, out) = (project.to_str().unwrap(), out.to_str().unwrap());
    let list = ["vale-app", "--project", project, "--export-heightmap", out];
    Args::try_parse_from(list).unwrap()
}

/// A state with no window, with a small heightmap that has a painted place.
fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.globe.set_face_size(FACE_SIZE);
    paint(&mut state.globe.map, 20.0, 10.0, 0.4);
    state
}

#[test]
fn the_command_line_exports_the_heightmap_of_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let (file, out) = (dir.path().join("w.gpkg"), dir.path().join("out/w.tif"));
    project(&file, 3000.0, true);

    let done = export_heightmap(&args(&file, &out)).unwrap().unwrap();
    assert_eq!((done.name.as_str(), &done.path), ("w.tif", &out));
    assert_eq!((done.width, done.height), (4 * FACE_SIZE, 2 * FACE_SIZE));

    let mut doc = Document::sample();
    let map = Session::open(&file, &mut doc).unwrap().load_map().unwrap();
    let expected = equirect(&map.unwrap());
    let (size, levels) = read_tiff(&out);
    assert_eq!(size, (4 * FACE_SIZE as u32, 2 * FACE_SIZE as u32));
    assert!(levels == expected, "the levels differ");
    assert!(levels.iter().any(|&level| level > BASE + 5000));
    assert!(levels.contains(&BASE));

    let mut decoder = Decoder::new(std::fs::File::open(&out).unwrap()).unwrap();
    let metadata = decoder.get_tag_ascii_string(Tag::Unknown(42112)).unwrap();
    assert!(
        metadata.contains("role=\"offset\">-6000</Item>"),
        "{metadata}"
    );
    let tiepoint = decoder.get_tag_f64_vec(Tag::ModelTiepointTag).unwrap();
    assert_eq!(tiepoint, [0.0, 0.0, 0.0, -180.0, 90.0, 0.0]);
    let doubles = decoder.get_tag_f64_vec(Tag::GeoDoubleParamsTag).unwrap();
    assert_eq!(doubles, [3_000_000.0, 3_000_000.0, 0.0]);

    // A run with no flag exports nothing.
    let none = Args::try_parse_from(["vale-app"]).unwrap();
    assert_eq!(export_heightmap(&none).unwrap(), None);
}

#[test]
fn the_command_line_fails_for_a_project_file_that_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    let (file, out) = (dir.path().join("none.gpkg"), dir.path().join("w.tif"));
    let error = export_heightmap(&args(&file, &out)).unwrap_err();
    assert!(error.to_string().ends_with("does not exist"), "{error}");
    assert!(!file.exists() && !out.exists());
}

#[test]
fn the_command_line_fails_for_a_world_with_no_raster_layer() {
    let dir = tempfile::tempdir().unwrap();
    let (file, out) = (dir.path().join("w.gpkg"), dir.path().join("w.tif"));
    project(&file, 6371.0, false);
    let error = export_heightmap(&args(&file, &out)).unwrap_err();
    let text = format!("the world of {} has no raster layer", file.display());
    assert_eq!(error.to_string(), text);
    assert!(!out.exists());
}

#[test]
fn the_worker_exports_the_heightmap_and_gives_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("Terra.tif");
    let mut state = state();
    state.headless = false;
    assert!(state.can_export());
    assert_eq!(state.globe.export_progress(), None);

    state.run_action(Action::ExportHeightmap(out.clone()));
    assert!(state.globe.exporting() && !state.can_export());
    // An export does not stop the brush or the save.
    assert!(!state.globe.busy());
    let progress = state.globe.export_progress().unwrap();
    assert!(progress.starts_with("Exporting Terra.tif: "), "{progress}");
    assert!(progress.ends_with('%'), "{progress}");
    // The globe asks for frames until the export ends.
    assert!(state.globe.advance(std::time::Instant::now()) || !state.globe.exporting());

    // A second export fails, and the first continues.
    let world = state.doc.project.world.clone();
    let second = dir.path().join("second.tif");
    assert!(!state.globe.start_export(second.clone(), &world, None));
    state.poll_export();
    let note = state.export_note.take().unwrap();
    assert!(note.warning);
    assert_eq!(
        note.text,
        "cannot export second.tif: the export of Terra.tif is not complete"
    );

    state.globe.wait_export();
    assert!(!state.globe.exporting());
    state.poll_export();
    let note = state.export_note.clone().unwrap();
    assert_eq!(note.text, "Exported Terra.tif (256 x 128)");
    assert!(!note.warning);
    assert_eq!(state.status, note.text);
    assert_eq!(state.share, None);
    assert!(state.can_export() && !second.exists());
    let (size, levels) = read_tiff(&out);
    assert_eq!(size, (256, 128));
    assert!(levels == equirect(&state.globe.map), "the levels differ");

    // The iOS host gets the path of the file.
    state.share_exports = true;
    state.headless = true;
    state.run_action(Action::ExportHeightmap(out.clone()));
    assert_eq!(state.share, Some(out));
}

#[test]
fn an_export_to_a_path_that_cannot_be_written_gives_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, "x").unwrap();
    let mut state = state();
    state.share_exports = true;
    state.run_action(Action::ExportHeightmap(blocker.join("w.tif")));
    let note = state.export_note.clone().unwrap();
    assert!(note.warning);
    assert!(note.text.starts_with("cannot write "), "{}", note.text);
    assert_eq!(state.status, note.text);
    assert_eq!(state.share, None);
    assert!(!state.globe.exporting() && state.can_export());
}

#[test]
fn a_world_with_no_raster_layer_cannot_export() {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.globe.set_face_size(FACE_SIZE);
    assert!(!state.has_raster() && !state.can_export());
    paint(&mut state.globe.map, 20.0, 10.0, 0.4);
    assert!(state.can_export());
}

#[test]
fn a_stroke_after_the_start_of_an_export_does_not_change_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("w.tif");
    let mut state = state();
    let before = equirect(&state.globe.map);
    let world = state.doc.project.world.clone();
    assert!(state.globe.start_export(out.clone(), &world, None));
    paint(&mut state.globe.map, 20.0, 10.0, 0.6);
    paint(&mut state.globe.map, -120.0, -40.0, 0.5);
    state.globe.wait_export();
    assert!(matches!(state.globe.take_export_result(), Some(Ok(_))));
    assert!(equirect(&state.globe.map) != before);
    assert!(read_tiff(&out).1 == before, "the file has the later stroke");
}

#[test]
fn the_export_file_has_the_name_of_the_world() {
    let mut state = AppState::new(Document::sample()).unwrap();
    let mut name = |text: &str| -> String {
        state.doc.project.world.name = text.to_string();
        state.export_name()
    };
    assert_eq!(name("Terra Nova"), "Terra Nova.tif");
    assert_eq!(name(" a/b:c "), "a_b_c.tif");
    assert_eq!(name(""), "heightmap.tif");
    let dir: PathBuf = vale_app::files::export_dir();
    assert!(dir.ends_with("vale-export"));
}

#[test]
fn the_end_of_the_hand_off_to_the_files_app_sets_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("Terra.tif");
    let mut state = state();
    state.share_exports = true;
    let export = |state: &mut AppState| -> PathBuf {
        state.run_action(Action::ExportHeightmap(out.clone()));
        assert_eq!(
            state.export_note.as_ref().unwrap().text,
            "Exported Terra.tif (256 x 128)"
        );
        state.share.take().unwrap()
    };

    let path = export(&mut state);
    state.end_share(ExportEvent::Saved, &path);
    let note = state.export_note.clone().unwrap();
    assert_eq!(note.text, "Saved Terra.tif to Files");
    assert!(!note.warning);
    assert_eq!(state.status, note.text);

    // A cancelled hand-off leaves no file and no text.
    let path = export(&mut state);
    assert!(path.is_file());
    state.end_share(ExportEvent::Cancelled, &path);
    assert!(!path.exists());
    assert_eq!(state.export_note, None);
    assert_eq!(state.status, "");
    assert!(state.can_export());
}

fn desktop(mut state: AppState) -> egui_kittest::Harness<'static, AppState> {
    state.workspace = Workspace::Globe;
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(2);
    h
}

#[test]
fn the_tool_bar_has_a_button_that_exports_the_heightmap() {
    use egui_kittest::kittest::{NodeT, Queryable};
    for file_buttons in [true, false] {
        let mut state = state();
        state.file_buttons = file_buttons;
        let mut h = desktop(state);
        let button = h.get_by_label(EXPORT_GEOTIFF);
        assert!(!button.accesskit_node().is_disabled());
        // The button is after the import button.
        let import = h.get_by_label("Import heightmap…").rect();
        assert!(button.rect().left() > import.right());
        button.click();
        h.run_steps(1);
        let actions = &h.state().actions;
        assert_eq!(actions, &[Action::ExportHeightmapDialog], "{file_buttons}");
    }
}

#[test]
fn the_export_button_is_off_for_a_world_with_no_raster_layer() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    let mut h = desktop(state);
    let button = h.get_by_label(EXPORT_GEOTIFF);
    assert!(button.accesskit_node().is_disabled());
    button.click();
    h.run_steps(1);
    assert!(h.state().actions.is_empty());
    assert!(
        !h.get_by_label("Import heightmap…")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn the_status_bar_shows_the_text_of_an_export() {
    use egui_kittest::kittest::Queryable;
    let dir = tempfile::tempdir().unwrap();
    let mut h = desktop(state());
    let out = dir.path().join("Terra.tif");
    h.state_mut().run_action(Action::ExportHeightmap(out));
    h.run_steps(2);
    assert!(h.query_by_label("Exported Terra.tif (256 x 128)").is_some());
}
