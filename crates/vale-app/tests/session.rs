use std::time::Instant;

use vale_app::document::Document;
use vale_app::session::{SAVE_DELAY, Session, latest_project, new_project_path, worlds};
use vale_store::{Project, ProjectFile, Units, World};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{FACES, Heightmap, Mode, Stamp, TexelRect, meters_to_level};

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

/// The heightmap of a new start of the app with the file.
fn reopen(path: &std::path::Path) -> (Session, Document, Heightmap) {
    let mut doc = Document::sample();
    let session = Session::open(path, &mut doc).unwrap();
    let map = session.load_map().unwrap().unwrap();
    (session, doc, map)
}

#[test]
fn a_painted_world_opens_again_with_the_same_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    assert!(session.load_map().unwrap().is_none());

    // The corner of three faces, a place on one face, and a pole.
    let mut map = Heightmap::new(1024, BASE);
    paint(&mut map, 45.0, 35.3, 0.2);
    paint(&mut map, -100.0, 5.0, 0.05);
    paint(&mut map, 0.0, 90.0, 0.1);
    map.bands.add(1234.5).unwrap();
    map.bands.ramp.land[0] = [1, 2, 3];
    session.flush(&mut doc.project, &mut map).unwrap();
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
    let tick = session.tick(&mut doc.project, &mut loaded, false, start);
    assert_eq!(tick.unwrap(), None);
}

/// A project file with a heightmap that keeps no undo steps in memory, as
/// the app has it.
fn start(path: &std::path::Path) -> (Session, Document, Heightmap) {
    let mut doc = Document::sample();
    let session = Session::open(path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);
    map.set_undo_steps(false);
    (session, doc, map)
}

#[test]
fn undo_works_after_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    assert!(!session.can_undo());
    paint(&mut map, 45.0, 35.3, 0.2);
    session.flush(&mut doc.project, &mut map).unwrap();
    let first = texels(&map);
    paint(&mut map, -100.0, 5.0, 0.05);
    session.flush(&mut doc.project, &mut map).unwrap();
    let second = texels(&map);
    assert!(first != second && !map.can_undo());
    drop(session);

    let (mut session, mut doc, mut map) = reopen(&path);
    map.take_reset();
    map.take_dirty();
    assert!(session.can_undo() && !session.can_redo());
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(texels(&map) == first, "the texels are not the same");
    // The GPU gets the tiles, and the file needs no save.
    assert!(map.take_dirty().iter().any(Option::is_some));
    assert!(!map.has_changes());
    let tick = session.tick(&mut doc.project, &mut map, false, Instant::now());
    assert_eq!(tick.unwrap(), None);
    assert!(session.can_undo() && session.can_redo());
    drop(session);

    // The redo step is in the file too.
    let (mut session, mut doc, mut map) = reopen(&path);
    assert!(texels(&map) == first, "the texels are not the same");
    assert!(session.redo(&mut doc, &mut map).unwrap());
    assert!(texels(&map) == second, "the texels are not the same");
    assert!(!session.can_redo());

    // The undo of the first stroke frees its tiles, and the world has no
    // raster layer again.
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert_eq!(map.allocated_tiles(), 0);
    assert!(texels(&map).iter().all(|&level| level == BASE));
    assert!(!session.can_undo());
    assert!(!session.undo(&mut doc, &mut map).unwrap());
    assert_eq!(doc.project.rasters(), []);

    // A new stroke removes the redo steps.
    assert!(session.can_redo());
    paint(&mut map, 0.0, 90.0, 0.1);
    session.flush(&mut doc.project, &mut map).unwrap();
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
    session.flush(&mut doc.project, &mut map).unwrap();
    let painted = texels(&map);
    let places = doc.project.layers()[2].id;
    doc.remove_layer(places);
    session.flush(&mut doc.project, &mut map).unwrap();
    paint(&mut map, -100.0, 5.0, 0.05);
    session.flush(&mut doc.project, &mut map).unwrap();
    let last = texels(&map);
    assert_eq!(doc.project.layers().len(), 2);

    // The edits go back in the reverse order.
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(texels(&map) == painted, "the texels are not the same");
    assert_eq!(doc.project.layers().len(), 2);
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(texels(&map) == painted, "the texels are not the same");
    assert_eq!(doc.project.layers(), layers);
    assert_eq!(doc.frame.entries.len(), entries);
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert_eq!(map.allocated_tiles(), 0);
    assert_eq!(doc.project.layers(), layers);
    assert!(!session.can_undo());

    for _ in 0..3 {
        assert!(session.redo(&mut doc, &mut map).unwrap());
    }
    assert!(texels(&map) == last, "the texels are not the same");
    assert_eq!(doc.project.layers().len(), 2);
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
    session.flush(&mut doc.project, &mut map).unwrap();
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
    });
    assert!(texels(&map) != first);
    drop(session);

    let (mut session, mut doc, mut map) = reopen(&path);
    assert!(texels(&map) == first, "the texels are not the same");
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn one_hold_of_a_control_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let (mut session, mut doc, mut map) = start(&path);
    // The file holds the band limits and the ramp of a painted heightmap.
    paint(&mut map, 45.0, 35.3, 0.2);
    session.flush(&mut doc.project, &mut map).unwrap();
    let bands = map.bands.clone();
    let stored = ProjectFile::open(&path).unwrap().1;

    let start = Instant::now();
    for i in 0..3 {
        map.bands.ramp.land[0] = [i as u8 + 1, 2, 3];
        let now = start + 2 * SAVE_DELAY * i;
        let tick = session.tick(&mut doc.project, &mut map, true, now);
        assert_eq!(tick.unwrap(), Some(SAVE_DELAY));
    }
    assert_eq!(ProjectFile::open(&path).unwrap().1, stored);
    // The save comes at the end of the hold, with no delay.
    doc.project.world.name = "Held".to_string();
    let tick = session.tick(&mut doc.project, &mut map, false, start);
    assert_eq!(tick.unwrap(), None);
    assert_eq!(ProjectFile::open(&path).unwrap().1, doc.project);

    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert_eq!(map.bands, bands);
    assert_eq!(doc.project, stored);
    let tick = session.tick(&mut doc.project, &mut map, false, start + SAVE_DELAY);
    assert_eq!(tick.unwrap(), None);
    assert!(session.can_redo());
    // The step before the hold is the stroke.
    assert!(session.undo(&mut doc, &mut map).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn a_save_after_one_stroke_writes_only_the_changed_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(2048, BASE);
    paint(&mut map, 45.0, 35.3, 0.4);
    paint(&mut map, -120.0, -40.0, 0.4);
    let start = Instant::now();
    let project = &mut doc.project;
    assert_eq!(
        session.tick(project, &mut map, false, start).unwrap(),
        Some(SAVE_DELAY)
    );
    // While a stroke waits for its texels, there is no save.
    let later = start + SAVE_DELAY;
    assert!(
        session
            .tick(project, &mut map, true, later)
            .unwrap()
            .is_some()
    );
    assert!(map.has_changes());
    assert_eq!(session.tick(project, &mut map, false, later).unwrap(), None);
    let painted = map.allocated_tiles();
    assert!(painted > 20, "{painted} tiles");
    assert_eq!(session.last_save.tiles_written, painted);

    // One small stroke on the painted ground.
    paint(&mut map, 45.0, 35.3, 0.01);
    let stroke: Vec<usize> = map.changed().collect();
    assert!((1..=4).contains(&stroke.len()), "{stroke:?}");
    assert_eq!(map.allocated_tiles(), painted);
    session.flush(project, &mut map).unwrap();
    assert_eq!(session.last_save.tiles_written, stroke.len());
    assert_eq!(session.last_save.tiles_deleted, 0);

    // A change of the bands writes the record and no tile.
    map.bands.add(77.0).unwrap();
    session.flush(project, &mut map).unwrap();
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
    paint(&mut map, 10.0, 10.0, 0.1);
    session.flush(&mut doc.project, &mut map).unwrap();
    drop(session);

    let (mut session, mut doc, loaded) = reopen(&path);
    assert_eq!(loaded.face_size(), 2048);
    assert_eq!(doc.project.rasters()[0].face_size, 2048);
    assert!(session.saves_map(&loaded));

    // A heightmap of another face size does not go to the file. The world
    // goes to the file, and the file keeps its tiles.
    let mut small = Heightmap::new(1024, meters_to_level(0.0));
    assert!(!session.saves_map(&small));
    paint(&mut small, -50.0, -50.0, 0.1);
    doc.project.world.name = "Other".to_string();
    session.flush(&mut doc.project, &mut small).unwrap();
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
    let mut map = Heightmap::new(1024, BASE);
    let start = Instant::now();
    session.flush(&mut doc.project, &mut map).unwrap();
    let mut tick = |doc: &mut Document, now| {
        session
            .tick(&mut doc.project, &mut map, false, now)
            .unwrap()
    };
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
    assert!(session.load_map().unwrap().is_none());
    let mut map = Heightmap::new(1024, BASE);
    let project = &mut next.project;
    assert!(
        session
            .tick(project, &mut map, false, now)
            .unwrap()
            .is_some()
    );
    session.flush(project, &mut map).unwrap();
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
fn a_new_world_has_no_raster_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);

    // A save of a world with no painted tile adds no raster.
    doc.project.world.name = "Aerth".to_string();
    let now = Instant::now();
    assert!(
        session
            .tick(&mut doc.project, &mut map, false, now)
            .unwrap()
            .is_some()
    );
    session.flush(&mut doc.project, &mut map).unwrap();
    assert_eq!(doc.project.rasters(), []);
    drop(session);
    let (file, saved) = ProjectFile::open(&path).unwrap();
    assert_eq!(saved.world.name, "Aerth");
    assert_eq!(saved.rasters(), []);
    drop(file);

    // The first stroke adds the heightmap as a raster.
    let mut doc = Document::empty();
    let mut session = Session::open(&path, &mut doc).unwrap();
    assert!(session.load_map().unwrap().is_none());
    paint(&mut map, 10.0, 20.0, 0.1);
    session.flush(&mut doc.project, &mut map).unwrap();
    assert_eq!(doc.project.rasters().len(), 1);
    assert_eq!(doc.project.rasters()[0].name, "Height");
    drop(session);
    let (_, _, loaded) = reopen(&path);
    assert!(texels(&loaded) == texels(&map));
}

#[test]
fn a_new_radius_keeps_each_longitude_and_latitude() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut map = Heightmap::new(1024, BASE);
    paint(&mut map, 10.0, 20.0, 0.1);
    session.flush(&mut doc.project, &mut map).unwrap();
    let layers = doc.project.layers().to_vec();
    let before = texels(&map);

    doc.set_radius_km(200.0);
    doc.project.world.units = Units::Imperial;
    assert_eq!(doc.project.layers(), layers);
    session.flush(&mut doc.project, &mut map).unwrap();
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
