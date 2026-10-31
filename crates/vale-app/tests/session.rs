mod images;

use std::time::Instant;

use vale_app::document::Document;
use vale_app::pipeline::Quality;
use vale_app::session::{SAVE_DELAY, Session, latest_project, new_project_path, worlds};
use vale_app::ui::{Action, AppState, LayerRef, RasterSpec};
use vale_store::{
    GeometryKind, Hillshade, Project, ProjectFile, RasterDisplay, RasterId, Smoothing, Units, World,
};
use vale_style::expr::Filter;
use vale_style::{LayerStyle, PointSymbolizer, Rule, Style, Symbolizer};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{Bands, FACES, Heightmap, Limits, Mode, Stamp, TexelRect, meters_to_level};

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
        motion: [0.0; 3],
        limits: Limits::default(),
    });
    assert!(map.end_stroke());
}

/// Each texel of the heightmap, face by face.
fn texels(map: &Heightmap) -> Vec<u16> {
    let n = map.face_size();
    let all = TexelRect {
        x0: 0,
        y0: 0,
        x1: n,
        y1: n,
    };
    let mut out = Vec::new();
    (0..FACES).for_each(|face| map.read_rect(face, all, &mut out));
    out
}

/// The ID of the first raster layer of a world.
const HEIGHT: RasterId = RasterId(0);

/// Gives the world a height layer for the heightmap. The first raster layer
/// of a world has the ID `HEIGHT`.
fn add_height(doc: &mut Document, map: &Heightmap) -> RasterId {
    let face_size = map.face_size() as u32;
    let (name, unit) = ("Height".to_string(), "m".to_string());
    doc.project
        .add_raster(name, face_size, unit, -6000.0, 6000.0)
}

/// The heightmap of the height layer, for a save.
fn maps(map: &mut Heightmap) -> [(RasterId, &mut Heightmap); 1] {
    [(HEIGHT, map)]
}

/// The heightmap of the height layer of a new start of the app with the
/// file.
fn reopen(path: &std::path::Path) -> (Session, Document, Heightmap) {
    let mut doc = Document::sample();
    let session = Session::open(path, &mut doc).unwrap();
    let (id, map) = session.load_maps().unwrap().remove(0);
    assert_eq!(id, HEIGHT);
    (session, doc, map)
}

#[test]
fn a_painted_world_opens_again_with_the_same_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    assert!(session.load_maps().unwrap().is_empty());

    // The corner of three faces, a place on one face, and a pole.
    let mut map = Heightmap::new(1024, BASE);
    add_height(&mut doc, &map);
    paint(&mut map, 45.0, 35.3, 0.2);
    paint(&mut map, -100.0, 5.0, 0.05);
    paint(&mut map, 0.0, 90.0, 0.1);
    map.bands.add(1234.5).unwrap();
    map.bands.set_color(1, [1, 2, 3]);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    assert!(session.last_save.tiles_written > 3);
    assert_eq!(session.last_save.tiles_written, map.allocated_tiles());
    assert!(!map.has_changes());
    drop(session);

    let (mut session, mut doc, mut loaded) = reopen(&path);
    assert_eq!(loaded.face_size(), 1024);
    assert_eq!(loaded.base(), BASE);
    assert_eq!(loaded.bands, map.bands);
    assert_eq!(loaded.allocated_tiles(), map.allocated_tiles());
    assert!(
        texels(&loaded) == texels(&map),
        "the texels are not the same"
    );
    assert!(texels(&loaded).iter().any(|&level| level > BASE + 5000));
    // The file holds the heightmap that the app loaded.
    assert!(!loaded.has_changes() && !loaded.can_undo());
    assert_eq!(doc.project.rasters().len(), 1);
    let start = Instant::now();
    let tick = session.tick(&mut doc.project, &mut maps(&mut loaded), false, start);
    assert_eq!(tick.unwrap(), None);
}

/// A project file and a world with a height layer. The heightmap of the
/// layer keeps no undo steps in memory, as the app has it.
fn start(path: &std::path::Path) -> (Session, Document, Heightmap) {
    let mut doc = Document::sample();
    let session = Session::open(path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);
    map.set_undo_steps(false);
    add_height(&mut doc, &map);
    (session, doc, map)
}

#[test]
fn undo_works_after_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    assert!(!session.can_undo());
    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let first = texels(&map);
    paint(&mut map, -100.0, 5.0, 0.05);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let second = texels(&map);
    assert!(first != second && !map.can_undo());
    drop(session);

    let (mut session, mut doc, mut map) = reopen(&path);
    map.take_reset();
    map.take_dirty();
    assert!(session.can_undo() && !session.can_redo());
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(texels(&map) == first, "the texels are not the same");
    // The GPU gets the tiles, and the file needs no save.
    assert!(map.take_dirty().iter().any(Option::is_some));
    assert!(!map.has_changes());
    let tick = session.tick(&mut doc.project, &mut maps(&mut map), false, Instant::now());
    assert_eq!(tick.unwrap(), None);
    assert!(session.can_undo() && session.can_redo());
    drop(session);

    // The redo step is in the file too.
    let (mut session, mut doc, mut map) = reopen(&path);
    assert!(texels(&map) == first, "the texels are not the same");
    assert!(session.redo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(texels(&map) == second, "the texels are not the same");
    assert!(!session.can_redo());

    // The first save has the new layer and its first stroke. The undo of
    // that save frees the tiles, and the world has no raster layer again.
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.allocated_tiles(), 0);
    assert!(texels(&map).iter().all(|&level| level == BASE));
    assert!(!session.can_undo());
    assert!(!session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(doc.project.rasters(), []);

    // A new stroke removes the redo steps.
    assert!(session.can_redo());
    let next = add_height(&mut doc, &map);
    assert_ne!(next, HEIGHT);
    paint(&mut map, 0.0, 90.0, 0.1);
    let project = &mut doc.project;
    session.flush(project, &mut [(next, &mut map)]).unwrap();
    assert!(session.can_undo() && !session.can_redo());
}

#[test]
fn brush_strokes_and_vector_edits_use_the_same_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    let entries = doc.frame.entries.len();

    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    // The save gives the height layer its style.
    let layers = doc.project.layers().to_vec();
    let painted = texels(&map);
    let layer = |name: &str| doc.project.layers().iter().find(|l| l.name == name);
    let places = layer("Places").unwrap().id;
    doc.remove_layer(places);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    paint(&mut map, -100.0, 5.0, 0.05);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let last = texels(&map);
    // The height layer and two vector layers stay.
    assert_eq!(doc.project.layers().len(), 3);

    // The edits go back in the reverse order.
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(texels(&map) == painted, "the texels are not the same");
    assert_eq!(doc.project.layers().len(), 3);
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(texels(&map) == painted, "the texels are not the same");
    assert_eq!(doc.project.layers(), layers);
    assert_eq!(doc.frame.entries.len(), entries);
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.allocated_tiles(), 0);
    // The first save had the new height layer and the first stroke.
    assert_eq!(doc.project.layers(), &layers[1..]);
    assert_eq!(doc.project.rasters(), []);
    assert!(!session.can_undo());

    for _ in 0..3 {
        assert!(session.redo(&mut doc, &mut maps(&mut map)).unwrap());
    }
    assert!(texels(&map) == last, "the texels are not the same");
    assert_eq!(doc.project.layers().len(), 3);
    assert_eq!(doc.frame.entries.len(), entries - 1);
    assert!(!session.can_redo());
}

/// The app stops while a stroke is open. The file has no part of it.
#[test]
fn a_stroke_that_did_not_end_is_not_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let first = texels(&map);

    map.begin_stroke();
    map.stamp(&Stamp {
        center: lonlat_to_dir(-100.0, 5.0),
        radius: 0.05,
        hardness: 0.3,
        flow: 1.0,
        mode: Mode::Raise,
        level: 0,
        strength: 9000.0,
        talus: 0.0,
        scale: 0.0,
        seed: 0,
        motion: [0.0; 3],
        limits: Limits::default(),
    });
    assert!(texels(&map) != first);
    drop(session);

    let (mut session, mut doc, mut map) = reopen(&path);
    assert!(texels(&map) == first, "the texels are not the same");
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn one_hold_of_a_control_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    // The file holds the band limits and the ramp of a painted heightmap.
    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let bands = map.bands.clone();
    let stored = ProjectFile::open(&path).unwrap().1;

    let start = Instant::now();
    for i in 0..3 {
        map.bands.set_color(1, [i as u8 + 1, 2, 3]);
        let now = start + 2 * SAVE_DELAY * i;
        let tick = session.tick(&mut doc.project, &mut maps(&mut map), true, now);
        assert_eq!(tick.unwrap(), Some(SAVE_DELAY));
    }
    assert_eq!(ProjectFile::open(&path).unwrap().1, stored);
    // The save comes at the end of the hold, with no delay.
    doc.project.world.name = "Held".to_string();
    let tick = session.tick(&mut doc.project, &mut maps(&mut map), false, start);
    assert_eq!(tick.unwrap(), None);
    assert_eq!(ProjectFile::open(&path).unwrap().1, doc.project);

    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands, bands);
    assert_eq!(doc.project, stored);
    let tick = session.tick(
        &mut doc.project,
        &mut maps(&mut map),
        false,
        start + SAVE_DELAY,
    );
    assert_eq!(tick.unwrap(), None);
    assert!(session.can_redo());
    // The step before the hold is the stroke.
    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn undo_restores_a_band_color() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let before = map.bands.clone();
    assert!(map.bands.set_color(2, [1, 2, 3]));
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let colors = |path: &std::path::Path| {
        let project = ProjectFile::open(path).unwrap().1;
        let style = raster_colors(&project, HEIGHT).colors;
        assert_eq!(project.rasters()[0].colors, style);
        style
    };
    assert_eq!(colors(&path)[2], [1, 2, 3]);

    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands, before);
    assert_eq!(colors(&path), before.colors());
    assert_eq!(raster_colors(&doc.project, HEIGHT).colors, before.colors());
    assert!(session.redo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands.colors()[2], [1, 2, 3]);
}

#[test]
fn a_save_after_one_stroke_writes_only_the_changed_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(2048, BASE);
    add_height(&mut doc, &map);
    paint(&mut map, 45.0, 35.3, 0.4);
    paint(&mut map, -120.0, -40.0, 0.4);
    let start = Instant::now();
    let project = &mut doc.project;
    assert_eq!(
        session
            .tick(project, &mut maps(&mut map), false, start)
            .unwrap(),
        Some(SAVE_DELAY)
    );
    // While a stroke waits for its texels, there is no save.
    let later = start + SAVE_DELAY;
    assert!(
        session
            .tick(project, &mut maps(&mut map), true, later)
            .unwrap()
            .is_some()
    );
    assert!(map.has_changes());
    assert_eq!(
        session
            .tick(project, &mut maps(&mut map), false, later)
            .unwrap(),
        None
    );
    let painted = map.allocated_tiles();
    assert!(painted > 20, "{painted} tiles");
    assert_eq!(session.last_save.tiles_written, painted);

    // One small stroke on the painted ground.
    paint(&mut map, 45.0, 35.3, 0.01);
    let stroke: Vec<usize> = map.changed().collect();
    assert!((1..=4).contains(&stroke.len()), "{stroke:?}");
    assert_eq!(map.allocated_tiles(), painted);
    session.flush(project, &mut maps(&mut map)).unwrap();
    assert_eq!(session.last_save.tiles_written, stroke.len());
    assert_eq!(session.last_save.tiles_deleted, 0);

    // A change of the bands writes the record and no tile.
    map.bands.add(77.0).unwrap();
    session.flush(project, &mut maps(&mut map)).unwrap();
    assert_eq!(session.last_save, Default::default());
    drop(session);
    let (_, _, loaded) = reopen(&path);
    assert_eq!(loaded.bands, map.bands);
    assert!(
        texels(&loaded) == texels(&map),
        "the texels are not the same"
    );
}

#[test]
fn a_heightmap_opens_again_with_its_face_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(2048, BASE);
    add_height(&mut doc, &map);
    paint(&mut map, 10.0, 10.0, 0.1);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    drop(session);

    let (mut session, mut doc, loaded) = reopen(&path);
    assert_eq!(loaded.face_size(), 2048);
    assert_eq!(doc.project.rasters()[0].face_size, 2048);

    // A heightmap of another face size does not go to the file. The world
    // goes to the file, and the file keeps its tiles.
    let mut small = Heightmap::new(1024, meters_to_level(0.0));
    paint(&mut small, -50.0, -50.0, 0.1);
    doc.project.world.name = "Other".to_string();
    session
        .flush(&mut doc.project, &mut maps(&mut small))
        .unwrap();
    assert_eq!(session.last_save, Default::default());
    assert!(small.has_changes());
    drop(session);
    let (_, doc, again) = reopen(&path);
    assert_eq!(doc.project.world.name, "Other");
    assert_eq!(again.face_size(), 2048);
    assert!(
        texels(&again) == texels(&map),
        "the texels are not the same"
    );
}

#[test]
fn a_world_is_the_same_after_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app data/project.gpkg");

    // The first start makes the file from the document.
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let start = Instant::now();
    session.flush(&mut doc.project, &mut []).unwrap();
    let mut tick =
        |doc: &mut Document, now| session.tick(&mut doc.project, &mut [], false, now).unwrap();
    assert_eq!(tick(&mut doc, start), None);

    // A change is on disk after the delay, and not before it.
    doc.project.world.name = "Aerth".to_string();
    (doc.project.world.wind_deg, doc.project.world.rain_shadow) = (45.0, 0.6);
    doc.set_radius_km(2500.0);
    let places = doc.project.layers()[2].id;
    doc.remove_layer(places);
    assert_eq!(tick(&mut doc, start), Some(SAVE_DELAY));
    let half = SAVE_DELAY / 2;
    assert_eq!(tick(&mut doc, start + half), Some(half));
    assert_ne!(ProjectFile::open(&path).unwrap().1, doc.project);
    assert_eq!(tick(&mut doc, start + SAVE_DELAY), None);
    assert_eq!(ProjectFile::open(&path).unwrap().1, doc.project);

    // The app stops with no exit call. The next start has the same world,
    // and the layers keep the styles of the sample.
    drop(session);
    let mut next = Document::sample();
    assert_ne!(next.project, doc.project);
    let mut session = Session::open(&path, &mut next).unwrap();
    assert_eq!(next.project, doc.project);
    assert_eq!(next.frame.entries, doc.frame.entries);
    assert_eq!(next.frame.entries.len(), 2);

    // The exit saves a change that is newer than the delay.
    next.project.world.name = "Last".to_string();
    let now = Instant::now();
    assert!(session.load_maps().unwrap().is_empty());
    let project = &mut next.project;
    assert!(
        session
            .tick(project, &mut [], false, now)
            .unwrap()
            .is_some()
    );
    session.flush(project, &mut []).unwrap();
    assert_eq!(ProjectFile::open(&path).unwrap().1, next.project);
}

#[test]
fn a_new_layer_of_the_file_gets_an_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    drop(Session::open(&path, &mut doc).unwrap());

    let mut empty = Document::empty();
    drop(Session::open(&path, &mut empty).unwrap());
    assert_eq!(empty.project, doc.project);
    let names = |d: &Document| -> Vec<String> {
        let entries = d.frame.entries.iter();
        entries
            .map(|e| d.layer(e.layer).unwrap().name.clone())
            .collect()
    };
    assert_eq!(names(&empty), ["Land", "Rivers", "Places"]);
    assert_eq!(names(&empty), names(&doc));
}

#[test]
fn a_save_adds_no_raster_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);

    // A save of a world with no raster layer adds none.
    doc.project.world.name = "Aerth".to_string();
    let now = Instant::now();
    let tick = session.tick(&mut doc.project, &mut maps(&mut map), false, now);
    assert!(tick.unwrap().is_some());
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    assert_eq!(doc.project.rasters(), []);

    // A painted heightmap with no raster layer does not go to the file.
    paint(&mut map, 10.0, 20.0, 0.1);
    let tick = session.tick(&mut doc.project, &mut maps(&mut map), false, now);
    assert_eq!(tick.unwrap(), None);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    assert_eq!(doc.project.rasters(), []);
    assert!(session.load_maps().unwrap().is_empty());
    drop(session);
    let (file, saved) = ProjectFile::open(&path).unwrap();
    assert_eq!(saved.world.name, "Aerth");
    assert_eq!(saved.rasters(), []);
    drop(file);

    // The heightmap goes to the file after the world has its raster layer.
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    add_height(&mut doc, &map);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    assert_eq!(doc.project.rasters().len(), 1);
    let layer = doc.project.raster_layer(HEIGHT).unwrap();
    assert_eq!(layer.name, "Height");
    drop(session);
    let (_, _, loaded) = reopen(&path);
    assert!(texels(&loaded) == texels(&map));
}

#[test]
fn a_new_raster_layer_and_the_removal_of_one_are_undo_steps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    assert!(!state.can_undo());
    let height = state.add_height();
    let empty = layer_texels(&state, height);
    paint_layer(&mut state, height, 45.0, 35.3);
    let painted = layer_texels(&state, height);
    assert!(state.remove_raster(height));
    assert!(state.doc.project.rasters().is_empty());
    assert!(state.globe.raster_map(height).is_none());
    assert_eq!((state.globe.raster(), state.selected_layer), (None, None));
    let (file, stored) = ProjectFile::open(&path).unwrap();
    assert!(stored.rasters().is_empty());
    drop(file);

    // The undo of the removal puts back the layer with its tiles.
    state.undo();
    assert_eq!(state.doc.project.rasters().len(), 1);
    assert!(layer_texels(&state, height) == painted);
    // No layer is selected, so the brush paints no layer.
    assert_eq!(state.globe.raster(), None);
    assert_eq!(state.shown_raster().map(|raster| raster.id), Some(height));
    state.redo();
    assert!(state.doc.project.rasters().is_empty() && !state.can_redo());
    state.undo();

    // The stroke and the new layer are the two steps before it.
    state.undo();
    assert!(layer_texels(&state, height) == empty);
    assert_eq!(state.doc.project.rasters().len(), 1);
    state.undo();
    assert!(state.doc.project.rasters().is_empty() && !state.can_undo());
    assert!(state.globe.raster_map(height).is_none());
    state.redo();
    assert_eq!(state.doc.project.rasters()[0].id, height);
    assert!(layer_texels(&state, height) == empty);
    state.redo();
    assert!(layer_texels(&state, height) == painted);
}

/// The record of a raster layer in the project.
fn record(project: &Project, id: RasterId) -> &vale_store::Raster {
    project.raster(id).unwrap()
}

/// The name, the raster, and the visibility of a layer record.
fn row(layer: &vale_store::Layer) -> (&str, Option<RasterId>, bool) {
    (layer.name.as_str(), layer.raster(), layer.visible)
}

#[test]
fn the_project_file_stores_each_raster_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();

    let project = &mut doc.project;
    let (name, unit) = ("Height".to_string(), "m".to_string());
    let height = project.add_raster(name, 1024, unit, -6000.0, 6000.0);
    let (name, unit) = ("Rain".to_string(), "mm".to_string());
    let rain = project.add_raster(name, 512, unit, 0.0, 4000.0);
    // A vector layer is between the two raster layers in the draw order.
    let lines = project.add_layer(
        "Roads".to_string(),
        vale_store::GeometryKind::Line,
        None,
        Vec::new(),
    );
    assert!(project.move_layer(lines, 1));
    let rain_layer = project.raster_layer(rain).unwrap().id;
    assert!(project.set_layer_visible(rain_layer, false));
    assert!(project.set_raster_opacity(rain, 0.25));
    let mut ground = Heightmap::new(1024, BASE);
    ground.bands.add(1234.5).unwrap();
    let mut wet = Heightmap::new(512, 0);
    wet.bands = Bands::with_range(0.0, 4000.0, false).unwrap();
    wet.bands.add(250.0).unwrap();
    wet.bands.set_color(1, [1, 2, 3]);
    paint(&mut ground, 45.0, 35.3, 0.2);
    paint(&mut wet, -100.0, 5.0, 0.1);
    paint(&mut wet, 0.0, 90.0, 0.1);
    let mut both = [(height, &mut ground), (rain, &mut wet)];
    session.flush(project, &mut both).unwrap();
    let tiles = ground.allocated_tiles() + wet.allocated_tiles();
    assert_eq!(session.last_save.tiles_written, tiles);
    assert!(!ground.has_changes() && !wet.has_changes());
    drop(session);

    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let project = &doc.project;
    let ids: Vec<RasterId> = project.rasters().iter().map(|r| r.id).collect();
    assert_eq!(ids, [height, rain]);
    // The layer records have the order, the names, and the visibility.
    let rows: Vec<_> = project.layers().iter().map(row).collect();
    assert_eq!(
        rows,
        [
            ("Height", Some(height), true),
            ("Roads", None, true),
            ("Rain", Some(rain), false)
        ]
    );
    let (first, second) = (record(project, height), record(project, rain));
    assert_eq!((first.unit.as_str(), first.opacity), ("m", 1.0));
    assert_eq!((first.value_min, first.value_max), (-6000.0, 6000.0));
    assert_eq!(first.face_size, 1024);
    let values = |stops: &[vale_store::Stop]| stops.iter().map(|s| s.value).collect::<Vec<_>>();
    let map_values = |bands: &Bands| bands.stops().iter().map(|s| s.value).collect::<Vec<_>>();
    assert_eq!(values(&first.stops), map_values(&ground.bands));
    assert!(values(&first.stops).contains(&1234.5));
    assert_eq!((second.unit.as_str(), second.opacity), ("mm", 0.25));
    assert_eq!((second.value_min, second.value_max), (0.0, 4000.0));
    assert_eq!(second.face_size, 512);
    assert_eq!(values(&second.stops), map_values(&wet.bands));
    assert!(values(&second.stops).contains(&250.0));
    assert_eq!(second.colors[1], [1, 2, 3]);

    let loaded = session.load_maps().unwrap();
    assert_eq!(loaded.len(), 2);
    for ((id, loaded), (of, map)) in loaded.iter().zip([(height, &ground), (rain, &wet)]) {
        assert_eq!(*id, of);
        assert_eq!(loaded.face_size(), map.face_size());
        assert_eq!(loaded.base(), map.base());
        assert_eq!(loaded.bands, map.bands);
        assert_eq!(loaded.allocated_tiles(), map.allocated_tiles());
        assert!(texels(loaded) == texels(map), "the texels of {id:?}");
        assert!(!loaded.has_changes());
    }
    assert!(texels(&loaded[0].1) != vec![BASE; texels(&ground).len()]);
    assert!(loaded[1].1.allocated_tiles() > 0);
}

/// Each texel of the heightmap of a raster layer of the app.
fn layer_texels(state: &AppState, id: RasterId) -> Vec<u16> {
    texels(state.globe.raster_map(id).unwrap())
}

/// One stroke on a raster layer of the app, and its save.
fn paint_layer(state: &mut AppState, id: RasterId, lon: f64, lat: f64) {
    state.select(Some(LayerRef::Raster(id)));
    assert_eq!(state.globe.raster(), Some(id));
    paint(&mut state.globe.map, lon, lat, 0.1);
    state.save_waiting().unwrap();
}

#[test]
fn undo_and_redo_work_for_a_stroke_on_each_raster_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let a = state.add_height();
    let b = state
        .add_raster(RasterSpec {
            name: "Rain".to_string(),
            unit: "mm".to_string(),
            min: 0.0,
            max: 4000.0,
            face_size: 512,
        })
        .unwrap();
    let empty = [layer_texels(&state, a), layer_texels(&state, b)];
    paint_layer(&mut state, a, 45.0, 35.3);
    paint_layer(&mut state, b, -100.0, 5.0);
    let painted = [layer_texels(&state, a), layer_texels(&state, b)];
    assert!(painted[0] != empty[0] && painted[1] != empty[1]);
    let check = |state: &AppState, a_painted: bool, b_painted: bool, step: &str| {
        let want_a = if a_painted { &painted[0] } else { &empty[0] };
        let want_b = if b_painted { &painted[1] } else { &empty[1] };
        assert!(layer_texels(state, a) == *want_a, "{step}: the first layer");
        assert!(
            layer_texels(state, b) == *want_b,
            "{step}: the second layer"
        );
    };

    // The second layer is selected. The second undo puts back the first
    // layer, which the globe does not show.
    assert_eq!(state.selected_raster(), Some(b));
    assert!(state.can_undo() && !state.can_redo());
    state.undo();
    check(&state, true, false, "the first undo");
    state.undo();
    check(&state, false, false, "the second undo");
    assert_eq!(state.globe.raster(), Some(b));
    assert_eq!(state.doc.project.rasters().len(), 2);
    state.redo();
    check(&state, true, false, "the first redo");
    state.redo();
    check(&state, true, true, "the second redo");
    assert!(!state.can_redo());

    // The first layer is selected. The first undo puts back the second
    // layer.
    state.select(Some(LayerRef::Raster(a)));
    assert_eq!(state.globe.raster(), Some(a));
    state.undo();
    check(
        &state,
        true,
        false,
        "the undo with the first layer selected",
    );
    state.undo();
    check(
        &state,
        false,
        false,
        "the second undo with the first layer selected",
    );
    state.redo();
    state.redo();
    check(&state, true, true, "the redo with the first layer selected");

    // The file has each stroke after a close and an open.
    state.save_waiting().unwrap();
    drop(state);
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let loaded = session.load_maps().unwrap();
    assert!(texels(&loaded[0].1) == painted[0]);
    assert!(texels(&loaded[1].1) == painted[1]);
    assert!(session.can_undo());
}

#[test]
fn a_new_radius_keeps_each_longitude_and_latitude() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);
    add_height(&mut doc, &map);
    paint(&mut map, 10.0, 20.0, 0.1);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let layers = doc.project.layers().to_vec();
    let before = texels(&map);

    doc.set_radius_km(200.0);
    doc.project.world.units = Units::Imperial;
    assert_eq!(doc.project.layers(), layers);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    drop(session);

    let (_, next, loaded) = reopen(&path);
    assert_eq!(next.project.world.radius_km, 200.0);
    assert_eq!(next.project.world.units, Units::Imperial);
    assert_eq!(next.project.layers(), layers);
    assert!(texels(&loaded) == before);
}

#[test]
fn a_new_project_file_has_the_name_of_its_world_and_a_free_path() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(latest_project(dir.path()), None);
    let first = new_project_path(dir.path(), " Pebble ");
    assert_eq!(first, dir.path().join("Pebble.gpkg"));
    std::fs::write(&first, b"").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"").unwrap();
    assert_eq!(latest_project(dir.path()), Some(first));
    let second = new_project_path(dir.path(), "Pebble");
    assert_eq!(second, dir.path().join("Pebble 2.gpkg"));
    let odd = new_project_path(dir.path(), "a/b: c");
    assert_eq!(odd, dir.path().join("a_b_ c.gpkg"));
    assert_eq!(
        new_project_path(dir.path(), ""),
        dir.path().join("World.gpkg")
    );
}

#[test]
fn the_list_of_the_worlds_has_a_name_for_each_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(worlds(dir.path()).is_empty());
    let world = |name: &str| World {
        name: name.to_string(),
        ..World::default()
    };
    for (file, name) in [("b", "Pebble"), ("a", "Aerth"), ("c", "Pebble")] {
        let path = dir.path().join(format!("{file}.gpkg"));
        drop(ProjectFile::create(&path, &Project::new(world(name))).unwrap());
    }
    std::fs::write(dir.path().join("broken.gpkg"), b"no").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"").unwrap();
    let list = worlds(dir.path());
    let names: Vec<&str> = list.iter().map(|world| world.name.as_str()).collect();
    // Two worlds with one name show the names of their files.
    assert_eq!(names, ["Aerth", "b", "broken", "c"]);
    assert_eq!(list[0].path, dir.path().join("a.gpkg"));
}

/// The face size of a raster layer that has a reduced heightmap.
const LARGE: usize = 4096;

/// A world with a project file and two large raster layers: a height and a
/// rain layer. The brush paints the rain layer.
fn large_world(path: &std::path::Path) -> (AppState, RasterId, RasterId) {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(path, &mut state.doc).unwrap());
    let height = state
        .add_raster(RasterSpec::height("Height", LARGE))
        .unwrap();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: LARGE,
    };
    let rain = state.add_raster(rain).unwrap();
    (state, height, rain)
}

/// True if two heightmaps have the same tiles, texel for texel.
fn same_tiles(a: &Heightmap, b: &Heightmap) -> bool {
    let per_side = a.face_size().div_ceil(a.tile_size());
    let same = |index| match (a.tile(index), b.tile(index)) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        (Some(tile), None) => tile.iter().all(|level| *level == b.base()),
        (None, Some(tile)) => tile.iter().all(|level| *level == a.base()),
    };
    a.face_size() == b.face_size() && (0..FACES * per_side * per_side).all(same)
}

/// The whole heightmap of a raster layer of the app from its project file.
fn stored_map(state: &AppState, id: RasterId) -> Heightmap {
    let session = state.session.as_ref().unwrap();
    session
        .load_map(state.doc.project.raster(id).unwrap())
        .unwrap()
}

/// The whole heightmap of a raster layer of the app: from the main memory,
/// or else from the project file.
fn whole_map(state: &AppState, id: RasterId) -> Heightmap {
    match state.globe.raster_map(id) {
        Some(map) => map.snapshot(),
        None => stored_map(state, id),
    }
}

/// The texels that a reduced heightmap of a whole heightmap has.
fn reduced_texels(whole: &Heightmap, size: usize) -> Vec<u16> {
    let factor = whole.face_size() / size;
    let all = TexelRect {
        x0: 0,
        y0: 0,
        x1: size,
        y1: size,
    };
    let mut out = Vec::new();
    (0..FACES).for_each(|face| whole.read_reduced(face, all, factor, &mut out));
    out
}

#[test]
fn a_layer_that_is_not_the_brush_layer_holds_only_its_reduced_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, height, rain) = large_world(&dir.path().join("project.gpkg"));
    // The brush layer has its whole heightmap. Its strokes go to the file.
    paint_layer(&mut state, height, 45.0, 35.3);
    paint_layer(&mut state, height, -100.0, 5.0);
    let whole = state.globe.map.snapshot();
    let whole_bytes = state.globe.map_bytes(height);
    assert!(whole.allocated_tiles() >= 16, "{}", whole.allocated_tiles());
    assert!(state.globe.raster_map(height).is_some());
    assert!(state.globe.raster_map(rain).is_none());

    // The other layer has only its reduced heightmap, with the mean of each
    // block of texels of the file.
    paint_layer(&mut state, rain, 10.0, -40.0);
    assert_eq!(state.globe.raster(), Some(rain));
    assert!(state.globe.raster_map(height).is_none());
    let reduced = state.globe.display_map(height).unwrap();
    assert_eq!(reduced.face_size(), 2048);
    assert!(texels(reduced) == reduced_texels(&whole, 2048));
    assert!(same_tiles(&stored_map(&state, height), &whole));
    // A tile of the reduced heightmap has the texels of 4 tiles of the
    // whole heightmap.
    let bytes = state.globe.map_bytes(height);
    assert_eq!(bytes, reduced.memory_bytes());
    assert!(
        bytes > 0 && bytes <= whole_bytes / 2,
        "{bytes} of {whole_bytes}"
    );
    assert!(reduced.allocated_tiles() < whole.allocated_tiles());
    // The rain layer is the same after it leaves the brush.
    let rain_whole = state.globe.map.snapshot();
    state.select(Some(LayerRef::Raster(height)));
    assert!(state.globe.raster_map(rain).is_none());
    let reduced = state.globe.display_map(rain).unwrap();
    assert!(texels(reduced) == reduced_texels(&rain_whole, 2048));

    // A layer with a face of 2,048 texels or less stays whole.
    let small = state.add_height();
    assert_eq!(state.globe.map.face_size(), 1024);
    state.select(Some(LayerRef::Raster(rain)));
    assert!(state.globe.raster_map(small).is_some());

    // With no project file, each layer keeps its whole heightmap, and its
    // undo steps.
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    let height = state
        .add_raster(RasterSpec::height("Height", LARGE))
        .unwrap();
    state.globe.map.begin_stroke();
    paint(&mut state.globe.map, 45.0, 35.3, 0.1);
    state.globe.map.end_stroke();
    let rain = RasterSpec::height("Other", LARGE);
    let rain = state.add_raster(rain).unwrap();
    state.pump_layers(true);
    let map = state.globe.raster_map(height).unwrap();
    assert!(map.allocated_tiles() > 0 && map.can_undo());
    state.select(Some(LayerRef::Raster(height)));
    assert_eq!(state.globe.raster(), Some(height));
    assert!(state.globe.raster_map(rain).is_some() && state.globe.can_undo());
}

#[test]
fn the_brush_layer_loads_its_tiles_from_the_working_copy_and_paints_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, a, b) = large_world(&dir.path().join("project.gpkg"));
    // The same strokes on heightmaps that stay in the main memory.
    let mut wanted = [a, b].map(|id| whole_map(&state, id));
    let mut stroke = |state: &mut AppState, at: usize, lon: f64, lat: f64| {
        paint_layer(state, [a, b][at], lon, lat);
        paint(&mut wanted[at], lon, lat, 0.1);
        assert!(same_tiles(&state.globe.map, &wanted[at]), "layer {at}");
    };
    stroke(&mut state, 0, 45.0, 35.3);
    stroke(&mut state, 1, 45.0, 30.0);
    // The first layer comes back from the file, and the next stroke goes
    // on from its saved texels.
    stroke(&mut state, 0, 50.0, 35.3);
    stroke(&mut state, 1, -100.0, 5.0);
    stroke(&mut state, 0, 0.0, 90.0);
    for (id, wanted) in [a, b].into_iter().zip(&wanted) {
        assert!(same_tiles(&whole_map(&state, id), wanted));
        assert!(same_tiles(&stored_map(&state, id), wanted));
    }
    assert!(state.globe.raster_map(b).is_none());

    // In a window, the tiles come in parts, and the brush waits for them.
    state.headless = false;
    state.select(Some(LayerRef::Raster(b)));
    assert_eq!(state.globe.raster(), Some(a));
    assert!(state.globe.busy() && !state.globe.paintable);
    assert!(!state.can_undo());
    let mut calls = 0;
    while state.globe.raster() != Some(b) {
        state.pump_layers(false);
        calls += 1;
        assert!(calls < 100_000, "the layer did not load");
        assert_eq!(state.loading_layer(), state.globe.raster() != Some(b));
    }
    assert!(calls >= 2, "{calls}");
    assert!(state.globe.paintable && !state.loading_layer());
    assert!(same_tiles(&state.globe.map, &wanted[1]));
    // The layer that the brush left goes out of the main memory.
    state.pump_layers(false);
    assert!(state.globe.raster_map(a).is_none());

    // The same world opens with the reduced heightmap of each layer.
    drop(state);
    let mut doc = Document::empty();
    let session = Session::open(&dir.path().join("project.gpkg"), &mut doc).unwrap();
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.open_rasters(session.load_display_maps().unwrap());
    state.session = Some(session);
    for (id, wanted) in [a, b].into_iter().zip(&wanted) {
        assert!(state.globe.raster_map(id).is_none());
        let reduced = state.globe.display_map(id).unwrap();
        assert!(texels(reduced) == reduced_texels(wanted, 2048));
    }
    state.select(Some(LayerRef::Raster(a)));
    assert!(same_tiles(&state.globe.map, &wanted[0]));
}

#[test]
fn an_undo_on_a_layer_that_is_not_the_brush_layer_updates_its_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, a, b) = large_world(&dir.path().join("project.gpkg"));
    paint_layer(&mut state, a, 45.0, 35.3);
    let first = state.globe.map.snapshot();
    paint_layer(&mut state, a, -100.0, 5.0);
    let second = state.globe.map.snapshot();
    // The brush paints the other layer with no edit.
    state.select(Some(LayerRef::Raster(b)));
    assert!(state.globe.raster_map(a).is_none());
    let picture = |state: &AppState| texels(state.globe.display_map(a).unwrap());
    assert!(picture(&state) == reduced_texels(&second, 2048));

    // The undo and the redo change the tiles of the file. The reduced
    // heightmap of the layer follows, with no whole heightmap in memory.
    state.undo();
    assert!(state.globe.raster_map(a).is_none());
    assert!(same_tiles(&stored_map(&state, a), &first));
    assert!(picture(&state) == reduced_texels(&first, 2048));
    assert!(picture(&state) != reduced_texels(&second, 2048));
    state.redo();
    assert!(picture(&state) == reduced_texels(&second, 2048));
    state.undo();
    state.undo();
    let empty = Heightmap::new(LARGE, first.base());
    assert!(picture(&state) == reduced_texels(&empty, 2048));
    assert_eq!(state.globe.raster(), Some(b));
    // The brush layer gets the texels of the file.
    state.redo();
    state.select(Some(LayerRef::Raster(a)));
    assert!(same_tiles(&state.globe.map, &first));
}

#[test]
fn the_export_of_a_layer_that_is_not_the_brush_layer_has_its_full_detail() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, a, b) = large_world(&dir.path().join("project.gpkg"));
    paint_layer(&mut state, a, 45.0, 35.3);
    paint_layer(&mut state, a, 0.0, 90.0);
    // The file of the layer while the brush paints it.
    let brush = dir.path().join("brush.tif");
    state.export_raster(a, brush.clone());
    assert!(!state.export_note.as_ref().unwrap().warning);

    state.select(Some(LayerRef::Raster(b)));
    assert!(state.globe.raster_map(a).is_none());
    let other = dir.path().join("other.tif");
    state.export_raster(a, other.clone());
    assert!(!state.export_note.as_ref().unwrap().warning);
    assert!(state.globe.raster_map(a).is_none());
    let (brush, other) = (std::fs::read(brush).unwrap(), std::fs::read(other).unwrap());
    assert!(brush.len() > 100_000, "{}", brush.len());
    assert!(brush == other, "the two files are not the same");
}

#[test]
fn each_raster_layer_keeps_its_own_smoothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let height = state.add_height();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 128,
    };
    let rain = state.add_raster(rain).unwrap();
    assert!(state.set_raster_smoothing(height, false));
    assert!(state.set_raster_feature_size(rain, 5.0));
    let off = Smoothing {
        on: false,
        feature_size: Smoothing::FEATURE_SIZE_DEFAULT,
    };
    let five = Smoothing {
        on: true,
        feature_size: 5.0,
    };
    let smoothing = |project: &Project, id| project.raster(id).unwrap().smoothing;
    let check = |project: &Project, step: &str| {
        assert_eq!(smoothing(project, height), off, "{step}");
        assert_eq!(smoothing(project, rain), five, "{step}");
    };

    // The undo of a stroke after a save keeps the smoothing.
    state.save_waiting().unwrap();
    paint_layer(&mut state, rain, -100.0, 5.0);
    state.undo();
    check(&state.doc.project, "the undo of a stroke");
    state.undo();
    assert_eq!(smoothing(&state.doc.project, height), Smoothing::default());
    state.redo();
    check(&state.doc.project, "the redo");

    // A new layer saves the changes that wait, and the file keeps them.
    let next = RasterSpec {
        name: "Heat".to_string(),
        unit: "K".to_string(),
        min: 200.0,
        max: 330.0,
        face_size: 128,
    };
    state.add_raster(next).unwrap();
    let (_, stored) = ProjectFile::open(&path).unwrap();
    check(&stored, "the file");
    drop(state);
    let mut doc = Document::empty();
    let _session = Session::open(&path, &mut doc).unwrap();
    check(&doc.project, "the next start");
}

#[test]
fn each_raster_layer_keeps_its_own_hillshade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let height = state.add_height();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 128,
    };
    let rain = state.add_raster(rain).unwrap();
    assert!(state.set_raster_hillshade(height, true));
    assert!(state.set_raster_hillshade_opacity(height, 0.3));
    assert!(state.set_raster_hillshade_opacity(rain, 0.8));
    assert!(state.set_raster_hillshade_highlights(rain, false));
    assert!(state.set_raster_hillshade_azimuth(height, 225.0));
    assert!(state.set_raster_hillshade_altitude(height, 30.0));
    assert!(state.set_raster_hillshade_exaggeration(height, 2.5));
    assert!(state.set_raster_hillshade_color(height, [40, 60, 110]));
    let shaded = Hillshade {
        on: true,
        opacity: 0.3,
        highlights: true,
        azimuth: 225.0,
        altitude: 30.0,
        exaggeration: 2.5,
        color: [40, 60, 110],
    };
    // A layer keeps the opacity and the highlights of its hillshade while
    // the hillshade is off.
    let kept = Hillshade {
        on: false,
        opacity: 0.8,
        highlights: false,
        ..Hillshade::default()
    };
    let hillshade = |project: &Project, id| project.raster(id).unwrap().hillshade;
    let check = |project: &Project, step: &str| {
        assert_eq!(hillshade(project, height), shaded, "{step}");
        assert_eq!(hillshade(project, rain), kept, "{step}");
    };

    // The undo of a stroke after a save keeps the hillshade.
    state.save_waiting().unwrap();
    paint_layer(&mut state, rain, -100.0, 5.0);
    state.undo();
    check(&state.doc.project, "the undo of a stroke");
    state.undo();
    assert_eq!(hillshade(&state.doc.project, height), Hillshade::default());
    state.redo();
    check(&state.doc.project, "the redo");

    // A new layer saves the changes that wait, and the file keeps them.
    let next = RasterSpec {
        name: "Heat".to_string(),
        unit: "K".to_string(),
        min: 200.0,
        max: 330.0,
        face_size: 128,
    };
    state.add_raster(next).unwrap();
    let (_, stored) = ProjectFile::open(&path).unwrap();
    check(&stored, "the file");
    drop(state);
    let mut doc = Document::empty();
    let _session = Session::open(&path, &mut doc).unwrap();
    check(&doc.project, "the next start");
}

#[test]
fn each_height_layer_keeps_its_own_switches_of_the_rivers_and_the_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let off = state.add_height();
    let on = state.add_height();
    assert!(state.set_raster_rivers(off, false));
    assert!(state.set_raster_lines(on, false));
    let rivers = |project: &Project, id| project.raster(id).unwrap().rivers;
    let lines = |project: &Project, id| project.raster(id).unwrap().lines;
    state.save_waiting().unwrap();
    drop(state);

    let mut doc = Document::empty();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert!(!rivers(&doc.project, off));
    assert!(rivers(&doc.project, on));
    assert!(lines(&doc.project, off));
    assert!(!lines(&doc.project, on));
}

#[test]
fn each_raster_layer_keeps_its_own_unit_ramp_band_limits_and_display() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let height = state.add_height();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 128,
    };
    let rain = state.add_raster(rain).unwrap();
    let bands = |state: &AppState, id| state.globe.display_map(id).unwrap().bands.clone();
    let height_bands = bands(&state, height);

    assert!(state.set_raster_display(rain, RasterDisplay::Gradient));
    assert!(state.set_raster_unit(rain, "mm/yr"));
    let map = state.globe.bands_mut(rain).unwrap();
    let index = map.add(1234.0).unwrap();
    map.set_color(1, [1, 2, 3]);
    // The unit of a height layer stays, and no other layer becomes one.
    assert!(!state.set_raster_unit(height, "ft"));
    assert!(!state.set_raster_unit(rain, "m"));
    assert!(state.set_raster_display(height, RasterDisplay::Grey));
    let rain_bands = bands(&state, rain);
    assert_eq!(rain_bands.stops()[index].value, 1234.0);

    // A change of the selection keeps the style of each layer.
    state.select(Some(LayerRef::Raster(height)));
    state.select(Some(LayerRef::Raster(rain)));
    let raster = |state: &AppState, id| state.doc.project.raster(id).unwrap().clone();
    assert_eq!(raster(&state, height).display, RasterDisplay::Grey);
    assert_eq!(raster(&state, height).unit, "m");
    assert_eq!(bands(&state, height), height_bands);
    assert_eq!(raster(&state, rain).display, RasterDisplay::Gradient);
    assert_eq!(raster(&state, rain).unit, "mm/yr");
    assert_eq!(bands(&state, rain), rain_bands);

    // A new layer saves the changes that wait, and the file keeps them.
    let next = RasterSpec {
        name: "Heat".to_string(),
        unit: "K".to_string(),
        min: 200.0,
        max: 330.0,
        face_size: 128,
    };
    state.add_raster(next).unwrap();
    let (_, stored) = ProjectFile::open(&path).unwrap();
    let (stored_height, stored_rain) =
        (stored.raster(height).unwrap(), stored.raster(rain).unwrap());
    assert_eq!(stored_height.display, RasterDisplay::Grey);
    assert_eq!(stored_height.unit, "m");
    assert_eq!(vale_app::session::bands_of(stored_height), height_bands);
    assert_eq!(stored_rain.display, RasterDisplay::Gradient);
    assert_eq!(stored_rain.unit, "mm/yr");
    assert_eq!(vale_app::session::bands_of(stored_rain), rain_bands);
}

/// A project file with one line layer that has no style.
fn unstyled(path: &std::path::Path) -> vale_store::LayerId {
    let mut project = Project::new(World::default());
    let id = project.add_layer("Roads".into(), GeometryKind::Line, None, Vec::new());
    ProjectFile::create(path, &project).unwrap();
    id
}

fn stored_style(path: &std::path::Path, id: vale_store::LayerId) -> Option<String> {
    let (_, project) = ProjectFile::open(path).unwrap();
    project.layer(id).unwrap().style.clone()
}

#[test]
fn opening_a_file_with_no_styles_stores_them_with_no_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.vale");
    let roads = unstyled(&path);
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    assert!(!session.can_undo());
    let style = doc.entry(roads).unwrap().style.clone();
    assert_eq!(style, LayerStyle::default_for(GeometryKind::Line, 0, &[]));
    assert_eq!(doc.changed_styles(), []);
    drop(session);
    assert_eq!(stored_style(&path, roads), Some(style.to_json()));
}

#[test]
fn a_style_survives_a_save_and_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styled.vale");
    let roads = unstyled(&path);
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.style.rules.push(style.style.rules[0].clone());
    style.style.rules[1].filter = Filter::new("kind = 'major'");
    style.style.rules[1].max_scale = Some(5.0e6);
    let (mut file, _) = ProjectFile::open(&path).unwrap();
    file.set_styles(&[(roads, style.to_json())]).unwrap();
    drop(file);

    // A save of another change keeps the style.
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.entry(roads).unwrap().style, style);
    assert!(doc.rename_layer(roads, "Streets"));
    session.flush(&mut doc.project, &mut []).unwrap();
    assert!(session.can_undo());
    drop(session);

    let mut doc = Document::empty();
    Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.layer(roads).unwrap().name, "Streets");
    assert_eq!(doc.entry(roads).unwrap().style, style);
}

#[test]
fn a_style_edit_is_an_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.vale");
    let roads = unstyled(&path);
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let before = doc.entry(roads).unwrap().style.clone();
    let mut after = before.clone();
    after.set_stroke(Some(vale_style::Stroke::solid(
        vale_style::Color::rgb(200, 0, 0),
        3.0,
    )));
    doc.entry_mut(roads).unwrap().style = after.clone();
    doc.store_styles();
    assert_eq!(doc.changed_styles(), []);
    session.flush(&mut doc.project, &mut []).unwrap();
    assert!(session.can_undo());
    assert_eq!(stored_style(&path, roads), Some(after.to_json()));

    assert!(session.undo(&mut doc, &mut []).unwrap());
    assert_eq!(doc.entry(roads).unwrap().style, before);
}

/// The raster symbolizer of the style of the layer of a raster.
fn raster_colors(project: &Project, id: RasterId) -> vale_style::RasterSymbolizer {
    let style = project.raster_layer(id).unwrap().style.as_deref().unwrap();
    let style = LayerStyle::from_json(style).unwrap();
    style.raster_symbolizer().unwrap().clone()
}

/// An older project file with no styles and one undo step: a height layer
/// with no stops, and a rain layer with its own colors. Returns the IDs
/// and the bands of the two layers.
fn old_rasters(path: &std::path::Path) -> ([RasterId; 2], [Bands; 2]) {
    let mut project = Project::new(World::default());
    let (m, mm) = ("m".to_string(), "mm/yr".to_string());
    let height = project.add_raster("Height".into(), 256, m, -6000.0, 6000.0);
    let rain = project.add_raster("Rain".into(), 256, mm, 0.0, 4000.0);
    let mut map = Heightmap::new(256, 0);
    map.bands = Bands::with_range(0.0, 4000.0, false).unwrap();
    map.bands.set_color(1, [9, 8, 7]);
    vale_app::session::sync_record(project.raster_mut(rain).unwrap(), &map);
    project.set_raster_display(rain, RasterDisplay::Gradient);
    project.set_raster_opacity(rain, 0.4);
    assert!(project.raster(height).unwrap().stops.is_empty());
    let bands = [height, rain].map(|id| vale_app::session::bands_of(record(&project, id)));
    let mut file = ProjectFile::create(path, &project).unwrap();
    project.world.name = "Old".into();
    file.save(&project).unwrap();
    assert!(file.can_undo());
    ([height, rain], bands)
}

#[test]
fn an_older_file_keeps_the_look_of_its_raster_layers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.gpkg");
    let (ids, bands) = old_rasters(&path);
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    assert!(!session.can_undo());
    let maps = session.load_maps().unwrap();
    for (i, id) in ids.into_iter().enumerate() {
        let raster = record(&doc.project, id);
        assert!(!raster.stops.is_empty());
        let colors = raster_colors(&doc.project, id);
        assert_eq!(colors, vale_app::raster_style::symbolizer_of(raster));
        assert_eq!(maps[i].1.bands, bands[i]);
    }
    let rain = raster_colors(&doc.project, ids[1]);
    assert_eq!((rain.display, rain.opacity), (RasterDisplay::Gradient, 0.4));
    assert_eq!(rain.colors[1], [9, 8, 7]);
    drop(session);
    assert_eq!(ProjectFile::open(&path).unwrap().1, doc.project);

    // A second open does not save.
    let bytes = std::fs::read(&path).unwrap();
    let session = Session::open(&path, &mut Document::empty()).unwrap();
    drop(session);
    assert!(std::fs::read(&path).unwrap() == bytes, "the file changed");
}

#[test]
fn the_style_of_a_raster_layer_wins_over_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.gpkg");
    let ([_, rain], _) = old_rasters(&path);
    drop(Session::open(&path, &mut Document::empty()).unwrap());
    let (mut file, project) = ProjectFile::open(&path).unwrap();
    let layer = project.raster_layer(rain).unwrap().id;
    let mut colors = raster_colors(&project, rain);
    colors.colors[0] = [1, 1, 1];
    colors.opacity = 0.7;
    let mut style = LayerStyle::from_json(
        project
            .raster_layer(rain)
            .unwrap()
            .style
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    style.set_raster_symbolizer(colors);
    file.set_styles(&[(layer, style.to_json())]).unwrap();
    drop(file);

    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let raster = record(&doc.project, rain);
    assert_eq!((raster.colors[0], raster.opacity), ([1, 1, 1], 0.7));
    let map = &session.load_maps().unwrap()[1].1;
    assert_eq!(map.bands.colors()[0], [1, 1, 1]);
    drop(session);
    let stored = ProjectFile::open(&path).unwrap().1;
    assert_eq!(record(&stored, rain).colors[0], [1, 1, 1]);
}

#[test]
fn a_new_height_layer_has_the_default_stops_in_its_style() {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    let height = state.add_height();
    let colors = raster_colors(&state.doc.project, height);
    let bands = state.globe.raster_map(height).unwrap().bands.clone();
    let raster = record(&state.doc.project, height);
    assert!(!colors.stops.is_empty());
    assert_eq!(colors, vale_app::raster_style::symbolizer_of(raster));
    assert_eq!(colors.colors, bands.colors());
    assert_eq!(
        colors.stops.iter().map(|s| s.value).collect::<Vec<_>>(),
        bands.stops().iter().map(|s| s.value).collect::<Vec<_>>()
    );
}

#[test]
fn undo_of_a_stop_move_goes_through_the_style() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let before = map.bands.clone();
    let value = map.bands.stops()[2].value;
    let moved = map.bands.move_to(2, value + 10.0);
    assert_ne!(moved, value);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
    let stop = |path: &std::path::Path| {
        let project = ProjectFile::open(path).unwrap().1;
        raster_colors(&project, HEIGHT).stops[2].value
    };
    assert_eq!(stop(&path), moved);

    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands, before);
    assert_eq!(stop(&path), value);
    assert_eq!(raster_colors(&doc.project, HEIGHT).stops[2].value, value);
    assert!(session.redo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands.stops()[2].value, moved);
    assert_eq!(stop(&path), moved);
}

#[test]
fn a_raster_layer_from_an_image_and_its_levels_are_one_undo_step() {
    use vale_app::globe::import::{Mapping, Source};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let levels = (0..32).map(|i| if i % 8 < 4 { 0 } else { 65535 }).collect();
    let image = vale_import::Greyscale {
        width: 8,
        height: 4,
        levels,
        bits: 16,
    };
    let spec = RasterSpec {
        name: "Rain".into(),
        unit: "mm".into(),
        min: 0.0,
        max: 4000.0,
        face_size: 16,
    };
    let source = Source::Image {
        name: "rain.png".into(),
        image,
        mapping: Mapping {
            levels: (0, 65535),
            values: (0.0, 4000.0),
            range: (0.0, 4000.0),
        },
    };
    let id = state.add_raster_from(spec, source).unwrap();
    let texels = layer_texels(&state, id);
    assert!(texels.contains(&0) && texels.contains(&65535));

    // The next frame saves the layer and its levels.
    state.save_tick(false, Instant::now());
    assert!(state.can_undo());
    state.undo();
    assert!(state.doc.project.rasters().is_empty() && !state.can_undo());
    state.redo();
    assert_eq!(state.doc.project.rasters()[0].id, id);
    assert!(layer_texels(&state, id) == texels);
}

#[test]
fn an_added_svg_is_saved_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.session = Some(session);

    let svg = dir.path().join("oak.svg");
    std::fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#8040c0"/></svg>"##,
    )
    .unwrap();
    state.run_action(Action::AddSymbol(svg.clone()));
    let note = state.symbol_note.clone().unwrap();
    assert_eq!(note.text, "Added symbol oak");
    assert!(!note.warning);
    assert!(state.doc.symbols().get("oak").is_some());
    // A second file with the name gets a free name.
    state.run_action(Action::AddSymbol(svg.clone()));
    assert_eq!(
        state.symbol_note.clone().unwrap().text,
        "Added symbol oak 2"
    );

    // A file that is not an SVG file, and a file over 1 MB, give a warning.
    let bad = dir.path().join("bad.svg");
    std::fs::write(&bad, "not svg").unwrap();
    state.run_action(Action::AddSymbol(bad));
    assert!(state.symbol_note.clone().unwrap().warning);
    let big = dir.path().join("big.svg");
    std::fs::write(&big, vec![b' '; 1 << 21]).unwrap();
    state.run_action(Action::AddSymbol(big));
    let note = state.symbol_note.clone().unwrap();
    assert!(note.warning && note.text.contains("1 MB"), "{}", note.text);
    // A new project has no symbols, so any name is free.
    let tree = dir.path().join("tree.svg");
    std::fs::copy(&svg, &tree).unwrap();
    state.run_action(Action::AddSymbol(tree));
    assert_eq!(state.symbol_note.clone().unwrap().text, "Added symbol tree");
    drop(state);

    // The reopened project has the symbols, and a style can use them.
    let mut doc = Document::empty();
    drop(Session::open(&path, &mut doc).unwrap());
    let names: Vec<_> = doc
        .project
        .symbols()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["oak", "oak 2", "tree"]);
    let points = vale_import::geojson::read_str(
        "pts",
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#,
    )
    .unwrap();
    let id = doc.add_imported(points, None)[0];
    doc.frame.labels = false;
    doc.entry_mut(id).unwrap().style.style = Style {
        rules: vec![Rule::new(
            "Oak",
            vec![Symbolizer::Symbol(PointSymbolizer::constant("oak", 20.0))],
        )],
    };
    let mut fonts = vale_app::pipeline::fonts().unwrap();
    let c = vale_app::pipeline::Pipeline::new()
        .compose(&doc, (800.0, 600.0), &mut fonts, Quality::Final, None)
        .unwrap();
    assert_eq!(c.symbols.len(), 1);
    let purple = c.list.items.iter().any(|i| {
        matches!(i, vale_render::Item::Fill { color, .. } if (color.r, color.g, color.b) == (0x80, 0x40, 0xc0))
    });
    assert!(purple, "the symbol of the file does not draw");
}

/// Level 0 of an image of the file, row by row, in straight RGBA8.
fn image_pixels(session: &Session, id: vale_store::ImageId, width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0; (width * height * 4) as usize];
    let tile = vale_store::IMAGE_TILE_SIZE;
    for row in 0..height.div_ceil(tile) {
        for column in 0..width.div_ceil(tile) {
            let png = session
                .load_image_tile(id, 0, column, row)
                .unwrap()
                .unwrap();
            let (w, h, rgba) = vale_store::decode_tile(&png).unwrap();
            for y in 0..h {
                let from = (y * w * 4) as usize;
                let to = (((row * tile + y) * width + column * tile) * 4) as usize;
                out[to..to + (w * 4) as usize]
                    .copy_from_slice(&rgba[from..from + (w * 4) as usize]);
            }
        }
    }
    out
}

/// A project file in `dir` with an image layer of 600 by 300 pixels, an
/// opacity of 0.4, and the blend mode Screen. Gives the path, the image,
/// the pixels, and the record of the image.
fn image_project(
    dir: &std::path::Path,
) -> (
    std::path::PathBuf,
    vale_store::ImageId,
    image::RgbaImage,
    vale_store::Image,
) {
    use vale_store::BlendMode;
    let path = dir.join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    state.add_height();
    // An image of 600 by 300 pixels has edge tiles and two levels.
    let picture = image::RgbaImage::from_fn(600, 300, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 40, ((x + y) % 256) as u8])
    });
    let file = images::write_png(&dir.join("plates.png"), &picture);
    state.run_action(vale_app::ui::Action::ImportImageLayer(file));
    let id = state.doc.project.images()[0].id;
    assert_eq!(state.selected_layer, Some(LayerRef::Image(id)));
    assert!(state.set_image_opacity(id, 0.4));
    assert!(state.set_image_blend(id, BlendMode::Screen));
    let layer = state.doc.project.image_layer(id).unwrap().id;
    state.move_layer(layer, 0);
    assert!(state.save_now());
    let record = state.doc.project.image(id).unwrap().clone();
    drop(state);
    (path, id, picture, record)
}

#[test]
fn an_image_layer_is_the_same_after_a_close_and_an_open() {
    use vale_store::BlendMode;
    let dir = tempfile::tempdir().unwrap();
    let (path, id, picture, record) = image_project(dir.path());

    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let image = doc.project.image(id).unwrap();
    assert_eq!(image, &record);
    assert_eq!((image.width, image.height, image.levels), (600, 300, 3));
    assert_eq!((image.opacity, image.blend), (0.4, BlendMode::Screen));
    let layers = doc.project.layers();
    assert_eq!(layers[0].image(), Some(id));
    assert_eq!(layers[0].name, "plates");
    let style = LayerStyle::from_json(layers[0].style.as_deref().unwrap()).unwrap();
    let look = style.image_symbolizer().unwrap();
    assert_eq!((look.opacity, look.blend), (0.4, BlendMode::Screen));
    assert!(image_pixels(&session, id, 600, 300) == picture.into_raw());
}

#[test]
#[ignore = "gpu"]
fn the_globe_loads_the_tiles_of_an_image_layer_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (path, id, _, _) = image_project(dir.path());
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();

    // The globe gets the tiles of the copy of the image from the file.
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.session = Some(session);
    state.sync_globe();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = vale_app::headless::ui_harness(state, (320.0, 240.0), 1.0, setup);
    for _ in 0..20 {
        h.step();
        if !h.state().globe.loading() {
            break;
        }
    }
    let globe = &h.state().globe;
    assert_eq!(globe.image_level(id), Some(0));
    assert!(!globe.loading());
    assert_eq!(globe.image_bytes(), (600 * 300 + 300 * 150 + 150 * 75) * 4);
}

#[test]
fn the_removal_of_an_image_layer_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let file = images::write_png(&dir.path().join("plates.png"), &images::half_red(512, 256));
    state.run_action(vale_app::ui::Action::ImportImageLayer(file));
    let id = state.doc.project.images()[0].id;
    let layer = state.doc.project.image_layer(id).unwrap().id;
    assert!(state.remove_layer(layer));
    assert!(state.doc.project.images().is_empty());
    assert!(!state.globe.holds_image(id));
    state.undo();
    assert_eq!(state.doc.project.images()[0].id, id);
    assert!(state.globe.holds_image(id));
    let session = state.session.as_ref().unwrap();
    assert!(session.load_image_tile(id, 0, 1, 0).unwrap().is_some());
    // The step before it adds the layer.
    state.undo();
    assert!(state.doc.project.images().is_empty() && !state.can_undo());
    state.redo();
    assert_eq!(state.doc.project.images()[0].id, id);
}

#[test]
fn a_raster_style_of_version_2_takes_the_lines_the_smoothing_and_the_hillshade_of_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.gpkg");
    let (ids, _) = old_rasters(&path);
    // The first open gives each raster layer a style.
    drop(Session::open(&path, &mut Document::empty()).unwrap());
    let (mut file, mut project) = ProjectFile::open(&path).unwrap();
    let height = ids[0];
    let layer = project.raster_layer(height).unwrap().id;
    let old = project.raster_mut(height).unwrap();
    old.lines = false;
    old.hillshade.on = true;
    old.hillshade.azimuth = 90.0;
    old.smoothing.on = false;
    // A style of version 2 has no lines, smoothing, or hillshade.
    let text = project.layer(layer).unwrap().style.clone().unwrap();
    let mut root: serde_json::Value = serde_json::from_str(&text).unwrap();
    root["version"] = 2.into();
    let raster = root["rules"][0]["symbolizers"][0].as_object_mut().unwrap();
    for key in ["lines", "smoothing", "hillshade"] {
        raster.remove(key);
    }
    project.set_layer_style(layer, Some(root.to_string()));
    file.save_base(&project).unwrap();
    drop(file);

    let mut doc = Document::empty();
    drop(Session::open(&path, &mut doc).unwrap());
    let colors = raster_colors(&doc.project, height);
    assert!(!colors.lines && colors.hillshade.on && !colors.smoothing.on);
    assert_eq!(colors.hillshade.azimuth, 90.0);
    let text = doc
        .project
        .raster_layer(height)
        .unwrap()
        .style
        .clone()
        .unwrap();
    assert_eq!(
        vale_style::style_version(&text),
        Some(vale_style::STYLE_VERSION)
    );
    assert!(record(&doc.project, height).hillshade.on);
}
