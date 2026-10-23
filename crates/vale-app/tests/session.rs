use std::time::Instant;

use vale_app::document::Document;
use vale_app::session::{SAVE_DELAY, Session, latest_project, new_project_path, worlds};
use vale_app::ui::{AppState, LayerRef, RasterSpec};
use vale_store::{Project, ProjectFile, RasterDisplay, RasterId, Smoothing, Units, World};
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
    let layers = doc.project.layers().to_vec();
    let entries = doc.frame.entries.len();

    paint(&mut map, 45.0, 35.3, 0.2);
    session
        .flush(&mut doc.project, &mut maps(&mut map))
        .unwrap();
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
        project.rasters()[0].colors.clone()
    };
    assert_eq!(colors(&path)[2], [1, 2, 3]);

    assert!(session.undo(&mut doc, &mut maps(&mut map)).unwrap());
    assert_eq!(map.bands, before);
    assert_eq!(colors(&path), before.colors());
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
