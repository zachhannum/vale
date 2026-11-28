use std::path::{Path, PathBuf};

use isotherm_store::{FORMAT_VERSION, Restored, StoreError, WorldFile};
use isotherm_world::{Heightmap, Moon, Planet, Terrain, World};
use rusqlite::Connection;

const TILE: usize = 4;

fn path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("world.gpkg")
}

fn planet() -> Planet {
    let mut planet = Planet::earth();
    planet.radius_m = 3_000_000.0;
    planet.atmosphere.co2_ppm = 900.0;
    planet.moons = vec![
        Moon {
            mass_kg: 1.0e22,
            distance_m: 2.0e8,
        },
        Moon {
            mass_kg: 3.0e21,
            distance_m: 5.0e8,
        },
    ];
    planet
}

/// A terrain of 10 texels on each side of a face, with tiles of 4 texels,
/// and texels that differ on three tiles.
fn terrain(seed: u16, low_m: f64, high_m: f64) -> Terrain {
    let mut heights = Heightmap::with_tile_size(10, TILE, 1000 + seed);
    let texels = |k: u16| -> Vec<u16> {
        (0..TILE * TILE)
            .map(|i| seed.wrapping_mul(97).wrapping_add(k * 300 + i as u16 * 7))
            .collect()
    };
    heights.load_tile(0, 0, 0, &texels(1));
    heights.load_tile(3, 2, 1, &texels(2));
    heights.load_tile(5, 1, 2, &texels(3));
    let mut terrain = Terrain::new(heights, low_m, high_m);
    terrain.sea_level_m = -100.0 + f64::from(seed);
    terrain
}

fn world() -> World {
    World {
        name: "Tethys".to_owned(),
        planet: planet(),
        terrain: terrain(1, -6000.0, 9000.0),
    }
}

fn tiles(terrain: &Terrain) -> Vec<((usize, usize, usize), Vec<u16>)> {
    let h = &terrain.heights;
    h.allocated()
        .map(|i| (h.tile_key(i), h.tile(i).unwrap().to_vec()))
        .collect()
}

fn assert_same_terrain(a: &Terrain, b: &Terrain) {
    assert_eq!(a.heights.face_size(), b.heights.face_size());
    assert_eq!(a.heights.tile_size(), b.heights.tile_size());
    assert_eq!(a.heights.base(), b.heights.base());
    assert_eq!((a.low_m, a.high_m), (b.low_m, b.high_m));
    assert_eq!(a.sea_level_m, b.sea_level_m);
    assert_eq!(tiles(a), tiles(b));
}

fn reopen(path: &Path) -> (WorldFile, World) {
    WorldFile::open(path).unwrap()
}

#[test]
fn a_world_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let original = world();
    WorldFile::create(path(&dir), &original).unwrap();
    let (file, opened) = reopen(&path(&dir));
    assert_eq!(opened.name, "Tethys");
    assert_eq!(opened.planet, original.planet);
    assert_eq!(opened.planet.moons.len(), 2);
    assert_same_terrain(&opened.terrain, &original.terrain);
    assert_eq!(tiles(&opened.terrain).len(), 3);
    assert!(!file.can_undo() && !file.can_redo());
}

#[test]
fn create_refuses_a_path_that_exists() {
    let dir = tempfile::tempdir().unwrap();
    WorldFile::create(path(&dir), &world()).unwrap();
    assert!(matches!(
        WorldFile::create(path(&dir), &world()),
        Err(StoreError::Io(_))
    ));
}

#[test]
fn sea_level_undo_and_redo_also_after_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = WorldFile::create(path(&dir), &world()).unwrap();
    file.set_sea_level(-99.0, 250.0).unwrap();
    file.set_sea_level(250.0, 400.0).unwrap();
    drop(file);

    let (mut file, opened) = reopen(&path(&dir));
    assert_eq!(opened.terrain.sea_level_m, 400.0);
    assert!(file.can_undo() && !file.can_redo());
    assert!(matches!(file.undo().unwrap(), Some(Restored::SeaLevel(m)) if m == 250.0));
    drop(file);

    let (mut file, opened) = reopen(&path(&dir));
    assert_eq!(opened.terrain.sea_level_m, 250.0);
    assert!(file.can_undo() && file.can_redo());
    assert!(matches!(file.undo().unwrap(), Some(Restored::SeaLevel(m)) if m == -99.0));
    assert!(!file.can_undo());
    assert!(file.undo().unwrap().is_none());
    drop(file);

    let (mut file, opened) = reopen(&path(&dir));
    assert_eq!(opened.terrain.sea_level_m, -99.0);
    assert!(matches!(file.redo().unwrap(), Some(Restored::SeaLevel(m)) if m == 250.0));
    assert!(matches!(file.redo().unwrap(), Some(Restored::SeaLevel(m)) if m == 400.0));
    assert!(file.redo().unwrap().is_none());
    drop(file);
    assert_eq!(reopen(&path(&dir)).1.terrain.sea_level_m, 400.0);
}

#[test]
fn a_new_step_drops_the_redo_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = WorldFile::create(path(&dir), &world()).unwrap();
    file.set_sea_level(-99.0, 10.0).unwrap();
    file.undo().unwrap();
    assert!(file.can_redo());
    file.set_sea_level(-99.0, 20.0).unwrap();
    assert!(!file.can_redo());
    assert!(matches!(file.undo().unwrap(), Some(Restored::SeaLevel(m)) if m == -99.0));
    assert!(!file.can_undo());
}

#[test]
fn replace_terrain_then_undo_restores_the_old_tiles_and_range() {
    let dir = tempfile::tempdir().unwrap();
    let original = world();
    let mut file = WorldFile::create(path(&dir), &original).unwrap();
    let new = terrain(7, 100.0, 4100.0);
    file.replace_terrain(&new).unwrap();
    drop(file);

    let (mut file, opened) = reopen(&path(&dir));
    assert_same_terrain(&opened.terrain, &new);
    let Some(Restored::Terrain(old)) = file.undo().unwrap() else {
        panic!("a terrain");
    };
    assert_same_terrain(&old, &original.terrain);
    drop(file);

    let (mut file, opened) = reopen(&path(&dir));
    assert_same_terrain(&opened.terrain, &original.terrain);
    let Some(Restored::Terrain(again)) = file.redo().unwrap() else {
        panic!("a terrain");
    };
    assert_same_terrain(&again, &new);
    drop(file);
    assert_same_terrain(&reopen(&path(&dir)).1.terrain, &new);
}

#[test]
fn save_planet_then_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = WorldFile::create(path(&dir), &world()).unwrap();
    file.save_planet(&Planet::earth()).unwrap();
    assert_eq!(reopen(&path(&dir)).1.planet, Planet::earth());
    let Some(Restored::Planet(old)) = file.undo().unwrap() else {
        panic!("a planet");
    };
    assert_eq!(old, planet());
    assert_eq!(reopen(&path(&dir)).1.planet, planet());
    let Some(Restored::Planet(new)) = file.redo().unwrap() else {
        panic!("a planet");
    };
    assert_eq!(new, Planet::earth());
}

#[test]
fn the_undo_log_keeps_at_most_the_step_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = WorldFile::create(path(&dir), &world()).unwrap();
    for i in 0..isotherm_store::UNDO_STEPS + 5 {
        file.set_sea_level(i as f64, i as f64 + 1.0).unwrap();
    }
    let mut undone = 0;
    while file.undo().unwrap().is_some() {
        undone += 1;
    }
    assert_eq!(undone, isotherm_store::UNDO_STEPS);
}

#[test]
fn a_newer_format_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    WorldFile::create(path(&dir), &world()).unwrap();
    let conn = Connection::open(path(&dir)).unwrap();
    conn.execute(
        "UPDATE isotherm_meta SET value = ?1 WHERE key = 'format_version'",
        [(FORMAT_VERSION + 1).to_string()],
    )
    .unwrap();
    drop(conn);
    match WorldFile::open(path(&dir)) {
        Err(StoreError::NewerFormat { file, app }) => {
            assert_eq!((file, app), (FORMAT_VERSION + 1, FORMAT_VERSION));
        }
        _ => panic!("the newer file opened"),
    }
}

#[test]
fn a_new_file_has_the_current_format_version() {
    let dir = tempfile::tempdir().unwrap();
    WorldFile::create(path(&dir), &world()).unwrap();
    let conn = Connection::open(path(&dir)).unwrap();
    let version: String = conn
        .query_row(
            "SELECT value FROM isotherm_meta WHERE key = 'format_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, FORMAT_VERSION.to_string());
    drop(conn);
    assert_eq!(reopen(&path(&dir)).1.name, "Tethys");
}

#[test]
fn a_file_that_is_not_a_world_is_refused() {
    let dir = tempfile::tempdir().unwrap();

    let plain = dir.path().join("plain.sqlite");
    Connection::open(&plain)
        .unwrap()
        .execute_batch("CREATE TABLE t (x INTEGER)")
        .unwrap();
    assert!(matches!(
        WorldFile::open(&plain),
        Err(StoreError::NotAWorld)
    ));

    let vale = dir.path().join("vale.gpkg");
    Connection::open(&vale)
        .unwrap()
        .execute_batch(
            "PRAGMA application_id = 1196444487;
             CREATE TABLE vale_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO vale_meta VALUES ('format_version', '43');",
        )
        .unwrap();
    assert!(matches!(WorldFile::open(&vale), Err(StoreError::NotAWorld)));

    let text = dir.path().join("text.gpkg");
    std::fs::write(
        &text,
        "not a database, but long enough to be read as a header ".repeat(4),
    )
    .unwrap();
    assert!(matches!(WorldFile::open(&text), Err(StoreError::NotAWorld)));
}

#[test]
fn the_file_is_a_geopackage() {
    let dir = tempfile::tempdir().unwrap();
    WorldFile::create(path(&dir), &world()).unwrap();
    let conn = Connection::open(path(&dir)).unwrap();
    let id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(id, 0x4750_4B47);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 10300);
    let listed: i64 = conn
        .query_row(
            "SELECT count(*) FROM gpkg_contents
             WHERE data_type = 'isotherm' AND table_name GLOB 'isotherm_*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name GLOB 'isotherm_*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(listed, tables);
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
}
