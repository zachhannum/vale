//! The world file and its undo log.
//!
//! An undo step holds the other state of the thing that it changed: a sea
//! level, a planet, or a whole terrain with its tiles. An undo or a redo
//! swaps that state with the state in the file. The steps that can be undone
//! have `done = 1`, and the steps that can be redone have `done = 0`.

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use isotherm_world::{Atmosphere, Heightmap, Moon, Orbit, Planet, Rotation, Star, Terrain, World};
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, params};

use crate::{run, tile};
use isotherm_sim::RunState;

/// The version of the `isotherm_` tables that this code writes.
pub const FORMAT_VERSION: u32 = 3;

/// One step for each format version. Step `n` changes a file of version `n`
/// to version `n + 1`. A new version adds a step and never changes an old one.
const MIGRATIONS: &[fn(&Transaction) -> rusqlite::Result<()>] = &[v1, v2, v3];

/// The most undo steps that the file keeps.
pub const UNDO_STEPS: usize = 100;
/// The most bytes of terrain tiles that the undo steps keep. The newest
/// terrain step stays, even if it is larger.
pub const UNDO_TERRAIN_BYTES: i64 = 256 << 20;

/// `GPKG` in ASCII.
const APPLICATION_ID: i64 = 0x4750_4B47;
/// GeoPackage 1.3.
const GPKG_VERSION: i64 = 10300;
const FACES: usize = 6;
/// The step id of a terrain while two terrains swap.
const SWAP_STEP: i64 = -1;

const WGS84: &str = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4326"]]"#;

/// An error from a world file.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    /// The file is not an Isotherm world.
    NotAWorld,
    /// A newer version of the app wrote the file.
    NewerFormat {
        file: u32,
        app: u32,
    },
    /// The file is an Isotherm world, but a record in it is not valid.
    Corrupt(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "cannot use the world file: {e}"),
            StoreError::Sqlite(e) => write!(f, "world file error: {e}"),
            StoreError::NotAWorld => write!(f, "the file is not an Isotherm world"),
            StoreError::NewerFormat { file, app } => write!(
                f,
                "the world has format version {file}, and this app reads up to version {app}"
            ),
            StoreError::Corrupt(what) => write!(f, "the world file is damaged: {what}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io(e) => Some(e),
            StoreError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> StoreError {
        StoreError::Io(e)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> StoreError {
        StoreError::Sqlite(e)
    }
}

pub(crate) fn corrupt(what: impl Into<String>) -> StoreError {
    StoreError::Corrupt(what.into())
}

/// What an undo or a redo put back in the file. The app puts the same thing
/// back in the world in memory.
pub enum Restored {
    SeaLevel(f64),
    Planet(Planet),
    Terrain(Box<Terrain>),
}

/// A small picture of the world for the start screen, with 4 bytes of RGBA
/// for each pixel, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What the start screen shows of a world file.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub name: String,
    pub thumbnail: Option<Thumbnail>,
}

/// Reads the name and the picture of a world file. This does not migrate or
/// change the file. A file of an older format version has no picture.
pub fn summary(path: impl AsRef<Path>) -> Result<Summary, StoreError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)?;
    let version = format_version(&conn)?;
    if version > FORMAT_VERSION {
        return Err(StoreError::NewerFormat {
            file: version,
            app: FORMAT_VERSION,
        });
    }
    let name = read_name(&conn)?;
    if version < 3 {
        return Ok(Summary {
            name,
            thumbnail: None,
        });
    }
    let row: Option<(u32, u32, Vec<u8>)> = conn
        .query_row(
            "SELECT width, height, data FROM isotherm_thumbnail",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let thumbnail = match row {
        Some((width, height, data)) => {
            let mut rgba = Vec::new();
            let read = DeflateDecoder::new(data.as_slice()).read_to_end(&mut rgba);
            let len = u64::from(width) * u64::from(height) * 4;
            if read.is_err() || rgba.len() as u64 != len {
                return Err(corrupt("the pixels of the thumbnail"));
            }
            Some(Thumbnail {
                width,
                height,
                rgba,
            })
        }
        None => None,
    };
    Ok(Summary { name, thumbnail })
}

fn read_name(conn: &Connection) -> Result<String, StoreError> {
    conn.query_row(
        "SELECT value FROM isotherm_meta WHERE key = 'name'",
        [],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| corrupt("no name record"))
}

/// An open world file. Each change is one transaction and one undo step.
pub struct WorldFile {
    conn: Connection,
    undo_steps: usize,
    redo_steps: usize,
}

impl WorldFile {
    /// Makes a new world file at `path`. The path must not exist.
    pub fn create(path: impl AsRef<Path>, world: &World) -> Result<WorldFile, StoreError> {
        let path = path.as_ref();
        if path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} exists", path.display()),
            )
            .into());
        }
        let mut conn = Connection::open(path)?;
        let tx = conn.transaction()?;
        migrate(&tx, 0)?;
        tx.execute(
            "INSERT INTO isotherm_meta (key, value) VALUES ('name', ?1)",
            [&world.name],
        )?;
        write_planet(&tx, &world.planet)?;
        write_terrain(&tx, &world.terrain)?;
        tx.commit()?;
        WorldFile::with_counts(conn)
    }

    /// Opens a world file, and changes a file of an older format version to
    /// the current version.
    pub fn open(path: impl AsRef<Path>) -> Result<(WorldFile, World), StoreError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} is not a file", path.display()),
            )
            .into());
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut conn = Connection::open_with_flags(path, flags)?;
        let version = format_version(&conn)?;
        if version > FORMAT_VERSION {
            return Err(StoreError::NewerFormat {
                file: version,
                app: FORMAT_VERSION,
            });
        }
        if version < FORMAT_VERSION {
            let tx = conn.transaction()?;
            migrate(&tx, version)?;
            tx.commit()?;
        }
        let world = World {
            name: read_name(&conn)?,
            planet: read_planet(&conn)?,
            terrain: read_terrain(&conn)?,
        };
        Ok((WorldFile::with_counts(conn)?, world))
    }

    fn with_counts(conn: Connection) -> Result<WorldFile, StoreError> {
        let mut file = WorldFile {
            conn,
            undo_steps: 0,
            redo_steps: 0,
        };
        file.count_steps()?;
        Ok(file)
    }

    fn count_steps(&mut self) -> Result<(), StoreError> {
        let count = |done: i64| -> rusqlite::Result<usize> {
            self.conn.query_row(
                "SELECT count(*) FROM isotherm_undo WHERE done = ?1",
                [done],
                |r| r.get(0),
            )
        };
        (self.undo_steps, self.redo_steps) = (count(1)?, count(0)?);
        Ok(())
    }

    /// Writes the planet and its moons, as one undo step.
    pub fn save_planet(&mut self, planet: &Planet) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let old = read_planet(&tx)?;
        push_step(&tx, "planet", None, Some(&planet_blob(&old)))?;
        write_planet(&tx, planet)?;
        run::clear_run(&tx)?;
        trim(&tx)?;
        tx.commit()?;
        self.count_steps()
    }

    /// Writes a new terrain in place of the terrain of the file, as one undo
    /// step. The step keeps the old tiles.
    pub fn replace_terrain(&mut self, terrain: &Terrain) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let step = push_step(&tx, "terrain", None, None)?;
        let row = TerrainRow::read(&tx, "isotherm_terrain", "id", 1)?;
        row.write(&tx, "isotherm_undo_terrain", "step_id", step)?;
        tx.execute(
            "INSERT INTO isotherm_undo_tile
               SELECT ?1, face, tile_column, tile_row, data FROM isotherm_tile",
            [step],
        )?;
        count_step_bytes(&tx, step)?;
        write_terrain(&tx, terrain)?;
        run::clear_run(&tx)?;
        trim(&tx)?;
        tx.commit()?;
        self.count_steps()
    }

    /// Writes a new sea level, as one undo step. `old_m` is the sea level
    /// that an undo puts back.
    pub fn set_sea_level(&mut self, old_m: f64, new_m: f64) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        push_step(&tx, "sea_level", Some(old_m), None)?;
        tx.execute("UPDATE isotherm_terrain SET sea_level_m = ?1", [new_m])?;
        run::clear_run(&tx)?;
        trim(&tx)?;
        tx.commit()?;
        self.count_steps()
    }

    /// Undoes the newest step. `None`: there is no step to undo.
    pub fn undo(&mut self) -> Result<Option<Restored>, StoreError> {
        self.swap(
            "SELECT id, kind FROM isotherm_undo WHERE done = 1 ORDER BY id DESC",
            0,
        )
    }

    /// Redoes the step that was undone last. `None`: there is no step to redo.
    pub fn redo(&mut self) -> Result<Option<Restored>, StoreError> {
        self.swap(
            "SELECT id, kind FROM isotherm_undo WHERE done = 0 ORDER BY id",
            1,
        )
    }

    /// Writes the state of a run in place of the saved run. A run is not an
    /// undo step.
    pub fn save_run(&mut self, state: &RunState) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        run::write_run(&tx, state)?;
        tx.commit()?;
        Ok(())
    }

    /// The saved run. `None`: the file has no run, or a run of another
    /// version of the model.
    pub fn load_run(&self) -> Result<Option<RunState>, StoreError> {
        run::read_run(&self.conn)
    }

    pub fn clear_run(&mut self) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        run::clear_run(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Writes the picture of the world. This is not an edit, so it gets no
    /// undo step.
    pub fn set_thumbnail(&mut self, thumbnail: &Thumbnail) -> Result<(), StoreError> {
        let len = u64::from(thumbnail.width) * u64::from(thumbnail.height) * 4;
        assert_eq!(thumbnail.rgba.len() as u64, len, "4 bytes for each pixel");
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&thumbnail.rgba)?;
        let data = encoder.finish()?;
        self.conn.execute(
            "INSERT OR REPLACE INTO isotherm_thumbnail (id, width, height, data)
             VALUES (1, ?1, ?2, ?3)",
            params![thumbnail.width, thumbnail.height, data],
        )?;
        Ok(())
    }

    /// Writes the name of the world. This gets no undo step.
    pub fn set_name(&mut self, name: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO isotherm_meta (key, value) VALUES ('name', ?1)",
            [name],
        )?;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        self.undo_steps > 0
    }

    pub fn can_redo(&self) -> bool {
        self.redo_steps > 0
    }

    /// Swaps the state of the first step of `select` with the state in the
    /// file, and gives the step the new `done`.
    fn swap(&mut self, select: &str, done: i64) -> Result<Option<Restored>, StoreError> {
        let tx = self.conn.transaction()?;
        let step: Option<(i64, String)> = tx
            .query_row(&format!("{select} LIMIT 1"), [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((id, kind)) = step else {
            return Ok(None);
        };
        run::clear_run(&tx)?;
        let restored = match kind.as_str() {
            "sea_level" => {
                let stored: f64 = tx.query_row(
                    "SELECT sea_level_m FROM isotherm_undo WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )?;
                let current: f64 =
                    tx.query_row("SELECT sea_level_m FROM isotherm_terrain", [], |r| r.get(0))?;
                tx.execute("UPDATE isotherm_terrain SET sea_level_m = ?1", [stored])?;
                tx.execute(
                    "UPDATE isotherm_undo SET sea_level_m = ?1 WHERE id = ?2",
                    params![current, id],
                )?;
                Restored::SeaLevel(stored)
            }
            "planet" => {
                let blob: Vec<u8> = tx.query_row(
                    "SELECT planet FROM isotherm_undo WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )?;
                let stored = planet_from_blob(&blob)?;
                let current = read_planet(&tx)?;
                write_planet(&tx, &stored)?;
                tx.execute(
                    "UPDATE isotherm_undo SET planet = ?1 WHERE id = ?2",
                    params![planet_blob(&current), id],
                )?;
                Restored::Planet(stored)
            }
            "terrain" => {
                let stored = TerrainRow::read(&tx, "isotherm_undo_terrain", "step_id", id)?;
                let current = TerrainRow::read(&tx, "isotherm_terrain", "id", 1)?;
                stored.write(&tx, "isotherm_terrain", "id", 1)?;
                current.write(&tx, "isotherm_undo_terrain", "step_id", id)?;
                tx.execute(
                    "UPDATE isotherm_undo_tile SET step_id = ?1 WHERE step_id = ?2",
                    [SWAP_STEP, id],
                )?;
                tx.execute(
                    "INSERT INTO isotherm_undo_tile
                       SELECT ?1, face, tile_column, tile_row, data FROM isotherm_tile",
                    [id],
                )?;
                tx.execute("DELETE FROM isotherm_tile", [])?;
                tx.execute(
                    "INSERT INTO isotherm_tile
                       SELECT face, tile_column, tile_row, data FROM isotherm_undo_tile
                       WHERE step_id = ?1",
                    [SWAP_STEP],
                )?;
                tx.execute(
                    "DELETE FROM isotherm_undo_tile WHERE step_id = ?1",
                    [SWAP_STEP],
                )?;
                count_step_bytes(&tx, id)?;
                Restored::Terrain(Box::new(read_terrain(&tx)?))
            }
            other => return Err(corrupt(format!("undo step kind {other:?}"))),
        };
        tx.execute(
            "UPDATE isotherm_undo SET done = ?1 WHERE id = ?2",
            [done, id],
        )?;
        tx.commit()?;
        self.count_steps()?;
        Ok(Some(restored))
    }
}

/// Adds a step that can be undone, after it drops the steps that can be
/// redone. Returns the id of the step.
fn push_step(
    tx: &Transaction,
    kind: &str,
    sea_level_m: Option<f64>,
    planet: Option<&[u8]>,
) -> rusqlite::Result<i64> {
    tx.execute_batch(
        "DELETE FROM isotherm_undo_tile
           WHERE step_id IN (SELECT id FROM isotherm_undo WHERE done = 0);
         DELETE FROM isotherm_undo_terrain
           WHERE step_id IN (SELECT id FROM isotherm_undo WHERE done = 0);
         DELETE FROM isotherm_undo WHERE done = 0;",
    )?;
    tx.execute(
        "INSERT INTO isotherm_undo (done, kind, sea_level_m, planet) VALUES (1, ?1, ?2, ?3)",
        params![kind, sea_level_m, planet],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Drops the oldest steps while the log has too many steps or too many bytes
/// of terrain. It runs after `push_step`, so each step can be undone.
fn trim(tx: &Transaction) -> rusqlite::Result<()> {
    let oldest_kept: Option<i64> = tx
        .query_row(
            "SELECT id FROM isotherm_undo ORDER BY id DESC LIMIT 1 OFFSET ?1",
            [UNDO_STEPS - 1],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = oldest_kept {
        drop_steps_before(tx, id)?;
    }
    loop {
        let (bytes, steps, oldest): (i64, i64, Option<i64>) = tx.query_row(
            "SELECT coalesce(sum(bytes), 0), count(*), min(step_id) FROM isotherm_undo_terrain",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        match oldest {
            Some(id) if bytes > UNDO_TERRAIN_BYTES && steps > 1 => drop_steps_before(tx, id + 1)?,
            _ => return Ok(()),
        }
    }
}

/// Drops each step with an id below `id`.
fn drop_steps_before(tx: &Transaction, id: i64) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM isotherm_undo_tile WHERE step_id < ?1", [id])?;
    tx.execute("DELETE FROM isotherm_undo_terrain WHERE step_id < ?1", [id])?;
    tx.execute("DELETE FROM isotherm_undo WHERE id < ?1", [id])?;
    Ok(())
}

fn count_step_bytes(tx: &Transaction, step: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE isotherm_undo_terrain SET bytes =
           (SELECT coalesce(sum(length(data)), 0) FROM isotherm_undo_tile WHERE step_id = ?1)
         WHERE step_id = ?1",
        [step],
    )?;
    Ok(())
}

/// The format version of a file, or `NotAWorld`.
fn format_version(conn: &Connection) -> Result<u32, StoreError> {
    let not_a_world = |e: rusqlite::Error| match e.sqlite_error_code() {
        Some(ErrorCode::NotADatabase) => StoreError::NotAWorld,
        _ => StoreError::Sqlite(e),
    };
    let id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(not_a_world)?;
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
           WHERE type = 'table' AND name = 'isotherm_meta')",
        [],
        |r| r.get(0),
    )?;
    if id != APPLICATION_ID || !has_meta {
        return Err(StoreError::NotAWorld);
    }
    let text: String = conn
        .query_row(
            "SELECT value FROM isotherm_meta WHERE key = 'format_version'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| corrupt("no format version"))?;
    text.parse()
        .map_err(|_| corrupt(format!("format version {text:?}")))
}

/// Runs each step from version `from` to `FORMAT_VERSION`.
fn migrate(tx: &Transaction, from: u32) -> Result<(), StoreError> {
    for step in &MIGRATIONS[from as usize..] {
        step(tx)?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO isotherm_meta (key, value) VALUES ('format_version', ?1)",
        [FORMAT_VERSION.to_string()],
    )?;
    Ok(())
}

/// The tables that GeoPackage requires, and the first `isotherm_` tables.
fn v1(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(&format!(
        "PRAGMA application_id = {APPLICATION_ID};
         PRAGMA user_version = {GPKG_VERSION};
         CREATE TABLE gpkg_spatial_ref_sys (
           srs_name TEXT NOT NULL,
           srs_id INTEGER PRIMARY KEY,
           organization TEXT NOT NULL,
           organization_coordsys_id INTEGER NOT NULL,
           definition TEXT NOT NULL,
           description TEXT);
         CREATE TABLE gpkg_contents (
           table_name TEXT NOT NULL PRIMARY KEY,
           data_type TEXT NOT NULL,
           identifier TEXT UNIQUE,
           description TEXT DEFAULT '',
           last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
           min_x DOUBLE,
           min_y DOUBLE,
           max_x DOUBLE,
           max_y DOUBLE,
           srs_id INTEGER,
           CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id)
             REFERENCES gpkg_spatial_ref_sys(srs_id));
         CREATE TABLE gpkg_geometry_columns (
           table_name TEXT NOT NULL,
           column_name TEXT NOT NULL,
           geometry_type_name TEXT NOT NULL,
           srs_id INTEGER NOT NULL,
           z TINYINT NOT NULL,
           m TINYINT NOT NULL,
           CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
           CONSTRAINT uk_gc_table_name UNIQUE (table_name),
           CONSTRAINT fk_gc_tn FOREIGN KEY (table_name)
             REFERENCES gpkg_contents(table_name),
           CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id)
             REFERENCES gpkg_spatial_ref_sys(srs_id));
         CREATE TABLE gpkg_extensions (
           table_name TEXT,
           column_name TEXT,
           extension_name TEXT NOT NULL,
           definition TEXT NOT NULL,
           scope TEXT NOT NULL,
           CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name));
         INSERT INTO gpkg_spatial_ref_sys VALUES
           ('Undefined Cartesian SRS', -1, 'NONE', -1, 'undefined', NULL),
           ('Undefined geographic SRS', 0, 'NONE', 0, 'undefined', NULL);
         CREATE TABLE isotherm_meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL);
         CREATE TABLE isotherm_planet (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           radius_m REAL NOT NULL,
           gravity REAL NOT NULL,
           luminosity_w REAL NOT NULL,
           temperature_k REAL NOT NULL,
           semi_major_m REAL NOT NULL,
           eccentricity REAL NOT NULL,
           perihelion_deg REAL NOT NULL,
           day_s REAL NOT NULL,
           tilt_deg REAL NOT NULL,
           surface_pressure_pa REAL NOT NULL,
           co2_ppm REAL NOT NULL);
         CREATE TABLE isotherm_moon (
           position INTEGER PRIMARY KEY,
           mass_kg REAL NOT NULL,
           distance_m REAL NOT NULL);
         CREATE TABLE isotherm_terrain (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           face_size INTEGER NOT NULL,
           tile_size INTEGER NOT NULL,
           base INTEGER NOT NULL,
           low_m REAL NOT NULL,
           high_m REAL NOT NULL,
           sea_level_m REAL NOT NULL);
         CREATE TABLE isotherm_tile (
           face INTEGER NOT NULL CHECK (face BETWEEN 0 AND 5),
           tile_column INTEGER NOT NULL,
           tile_row INTEGER NOT NULL,
           data BLOB NOT NULL,
           PRIMARY KEY (face, tile_column, tile_row));
         CREATE TABLE isotherm_undo (
           id INTEGER PRIMARY KEY,
           done INTEGER NOT NULL,
           kind TEXT NOT NULL,
           sea_level_m REAL,
           planet BLOB);
         CREATE TABLE isotherm_undo_terrain (
           step_id INTEGER PRIMARY KEY,
           face_size INTEGER NOT NULL,
           tile_size INTEGER NOT NULL,
           base INTEGER NOT NULL,
           low_m REAL NOT NULL,
           high_m REAL NOT NULL,
           sea_level_m REAL NOT NULL,
           bytes INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE isotherm_undo_tile (
           step_id INTEGER NOT NULL,
           face INTEGER NOT NULL,
           tile_column INTEGER NOT NULL,
           tile_row INTEGER NOT NULL,
           data BLOB NOT NULL,
           PRIMARY KEY (step_id, face, tile_column, tile_row));
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           SELECT name, 'isotherm', name FROM sqlite_master
           WHERE type = 'table' AND name GLOB 'isotherm_*';
         INSERT INTO gpkg_extensions
           SELECT table_name, NULL, 'isotherm_world', 'Records of the Isotherm app', 'read-write'
           FROM gpkg_contents WHERE data_type = 'isotherm';"
    ))?;
    tx.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES
           ('WGS 84 geodetic', 4326, 'EPSG', 4326, ?1, NULL)",
        [WGS84],
    )?;
    Ok(())
}

/// The mass of the star, the methane, and the sidereal day in place of the
/// solar day, in the planet and in the planets of the undo log. The tables
/// of a model run.
fn v2(tx: &Transaction) -> rusqlite::Result<()> {
    let old: Option<Vec<f64>> = tx
        .query_row(
            "SELECT radius_m, gravity, luminosity_w, temperature_k, semi_major_m,
               eccentricity, perihelion_deg, day_s, tilt_deg, surface_pressure_pa, co2_ppm
             FROM isotherm_planet",
            [],
            |r| (0..11).map(|i| r.get(i)).collect(),
        )
        .optional()?;
    tx.execute_batch(
        "ALTER TABLE isotherm_planet ADD COLUMN star_mass_kg REAL NOT NULL DEFAULT 0;
         ALTER TABLE isotherm_planet ADD COLUMN sidereal_day_s REAL NOT NULL DEFAULT 0;
         ALTER TABLE isotherm_planet ADD COLUMN ch4_ppm REAL NOT NULL DEFAULT 0;
         ALTER TABLE isotherm_planet DROP COLUMN day_s;",
    )?;
    tx.execute_batch(&format!(
        "{}
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           SELECT name, 'isotherm', name FROM sqlite_master
           WHERE type = 'table' AND name IN ('isotherm_run', 'isotherm_run_grid', 'isotherm_field');
         INSERT INTO gpkg_extensions
           SELECT table_name, NULL, 'isotherm_world', 'Records of the Isotherm app', 'read-write'
           FROM gpkg_contents
           WHERE table_name IN ('isotherm_run', 'isotherm_run_grid', 'isotherm_field');",
        run::RUN_TABLES
    ))?;
    if let Some(old) = old {
        let n = numbers_from_v1(&old);
        tx.execute(
            "UPDATE isotherm_planet SET star_mass_kg = ?1, sidereal_day_s = ?2, ch4_ppm = ?3",
            params![n[4], n[8], n[12]],
        )?;
    }
    let blobs: Vec<(i64, Vec<u8>)> = tx
        .prepare("SELECT id, planet FROM isotherm_undo WHERE planet IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, blob) in blobs {
        let (head, moons) = split_blob(&blob, 11)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let planet = planet_from_numbers(numbers_from_v1(&head), moons);
        tx.execute(
            "UPDATE isotherm_undo SET planet = ?1 WHERE id = ?2",
            params![planet_blob(&planet), id],
        )?;
    }
    Ok(())
}

/// The picture of the world for the start screen.
fn v3(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE isotherm_thumbnail (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           width INTEGER NOT NULL,
           height INTEGER NOT NULL,
           data BLOB NOT NULL
         );
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           VALUES ('isotherm_thumbnail', 'isotherm', 'isotherm_thumbnail');
         INSERT INTO gpkg_extensions
           VALUES ('isotherm_thumbnail', NULL, 'isotherm_world', 'Records of the Isotherm app',
             'read-write');",
    )
}

/// The numbers of a planet before its moons, in the order of the columns of
/// `isotherm_planet`.
fn planet_numbers(p: &Planet) -> [f64; PLANET_NUMBERS] {
    [
        p.radius_m,
        p.gravity,
        p.star.luminosity_w,
        p.star.temperature_k,
        p.star.mass_kg,
        p.orbit.semi_major_m,
        p.orbit.eccentricity,
        p.orbit.perihelion_deg,
        p.rotation.sidereal_day_s,
        p.rotation.tilt_deg,
        p.atmosphere.surface_pressure_pa,
        p.atmosphere.co2_ppm,
        p.atmosphere.ch4_ppm,
    ]
}

/// The count of the numbers of `planet_numbers`.
const PLANET_NUMBERS: usize = 13;

/// The columns of `isotherm_planet` in the order of `planet_numbers`.
const PLANET_COLUMNS: &str = "radius_m, gravity, luminosity_w, temperature_k, star_mass_kg,
    semi_major_m, eccentricity, perihelion_deg, sidereal_day_s, tilt_deg,
    surface_pressure_pa, co2_ppm, ch4_ppm";

fn planet_from_numbers(n: [f64; PLANET_NUMBERS], moons: Vec<Moon>) -> Planet {
    Planet {
        radius_m: n[0],
        gravity: n[1],
        star: Star {
            luminosity_w: n[2],
            temperature_k: n[3],
            mass_kg: n[4],
        },
        orbit: Orbit {
            semi_major_m: n[5],
            eccentricity: n[6],
            perihelion_deg: n[7],
        },
        rotation: Rotation {
            sidereal_day_s: n[8],
            tilt_deg: n[9],
        },
        atmosphere: Atmosphere {
            surface_pressure_pa: n[10],
            co2_ppm: n[11],
            ch4_ppm: n[12],
        },
        moons,
    }
}

/// The numbers of a planet of format version 1, which had the solar day in
/// place of the sidereal day, and no mass of the star and no methane. The
/// mass of the star and the methane get the values of the Earth preset.
fn numbers_from_v1(v1: &[f64]) -> [f64; PLANET_NUMBERS] {
    let earth = Planet::earth();
    let mut n = [
        v1[0],
        v1[1],
        v1[2],
        v1[3],
        earth.star.mass_kg,
        v1[4],
        v1[5],
        v1[6],
        0.0,
        v1[8],
        v1[9],
        v1[10],
        earth.atmosphere.ch4_ppm,
    ];
    let year = planet_from_numbers(n, Vec::new()).orbital_period_s();
    // The sidereal day is shorter than the solar day by one turn each year.
    n[8] = 1.0 / (1.0 / v1[7] + 1.0 / year);
    n
}

fn write_planet(tx: &Transaction, planet: &Planet) -> rusqlite::Result<()> {
    let n = planet_numbers(planet);
    tx.execute("DELETE FROM isotherm_planet", [])?;
    tx.execute(
        &format!(
            "INSERT INTO isotherm_planet (id, {PLANET_COLUMNS})
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        ),
        rusqlite::params_from_iter(n),
    )?;
    tx.execute("DELETE FROM isotherm_moon", [])?;
    for (position, moon) in planet.moons.iter().enumerate() {
        tx.execute(
            "INSERT INTO isotherm_moon (position, mass_kg, distance_m) VALUES (?1, ?2, ?3)",
            params![position as i64, moon.mass_kg, moon.distance_m],
        )?;
    }
    Ok(())
}

fn read_planet(conn: &Connection) -> Result<Planet, StoreError> {
    let numbers = conn
        .query_row(
            &format!("SELECT {PLANET_COLUMNS} FROM isotherm_planet"),
            [],
            |r| {
                let mut n = [0.0; PLANET_NUMBERS];
                for (i, value) in n.iter_mut().enumerate() {
                    *value = r.get(i)?;
                }
                Ok(n)
            },
        )
        .optional()?
        .ok_or_else(|| corrupt("no planet record"))?;
    let mut statement =
        conn.prepare("SELECT mass_kg, distance_m FROM isotherm_moon ORDER BY position")?;
    let moons = statement
        .query_map([], |r| {
            Ok(Moon {
                mass_kg: r.get(0)?,
                distance_m: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(planet_from_numbers(numbers, moons))
}

/// A planet as little-endian numbers: the numbers of `planet_numbers`, and
/// then the mass and the distance of each moon.
fn planet_blob(planet: &Planet) -> Vec<u8> {
    let moons = planet.moons.iter().flat_map(|m| [m.mass_kg, m.distance_m]);
    planet_numbers(planet)
        .into_iter()
        .chain(moons)
        .flat_map(f64::to_le_bytes)
        .collect()
}

fn planet_from_blob(blob: &[u8]) -> Result<Planet, StoreError> {
    let (head, moons) = split_blob(blob, PLANET_NUMBERS)?;
    Ok(planet_from_numbers(
        head.try_into().expect("13 numbers"),
        moons,
    ))
}

/// The numbers of a planet blob with a head of `head` numbers, and its moons.
fn split_blob(blob: &[u8], head: usize) -> Result<(Vec<f64>, Vec<Moon>), StoreError> {
    let bad = || corrupt("a planet of the undo log");
    if !blob.len().is_multiple_of(8) {
        return Err(bad());
    }
    let values: Vec<f64> = blob
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&b| f64::from_le_bytes(b))
        .collect();
    if values.len() < head || !(values.len() - head).is_multiple_of(2) {
        return Err(bad());
    }
    let (head, moons) = values.split_at(head);
    let moons = moons
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[mass_kg, distance_m]| Moon {
            mass_kg,
            distance_m,
        })
        .collect();
    Ok((head.to_vec(), moons))
}

/// The row of a terrain without its tiles.
struct TerrainRow {
    face_size: i64,
    tile_size: i64,
    base: i64,
    low_m: f64,
    high_m: f64,
    sea_level_m: f64,
}

impl TerrainRow {
    fn of(terrain: &Terrain) -> TerrainRow {
        TerrainRow {
            face_size: terrain.heights.face_size() as i64,
            tile_size: terrain.heights.tile_size() as i64,
            base: i64::from(terrain.heights.base()),
            low_m: terrain.low_m,
            high_m: terrain.high_m,
            sea_level_m: terrain.sea_level_m,
        }
    }

    fn read(conn: &Connection, table: &str, key: &str, id: i64) -> Result<TerrainRow, StoreError> {
        conn.query_row(
            &format!(
                "SELECT face_size, tile_size, base, low_m, high_m, sea_level_m
                 FROM {table} WHERE {key} = ?1"
            ),
            [id],
            |r| {
                Ok(TerrainRow {
                    face_size: r.get(0)?,
                    tile_size: r.get(1)?,
                    base: r.get(2)?,
                    low_m: r.get(3)?,
                    high_m: r.get(4)?,
                    sea_level_m: r.get(5)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| corrupt(format!("no terrain record in {table}")))
    }

    /// Writes the row in place of the row with the same key.
    fn write(&self, tx: &Transaction, table: &str, key: &str, id: i64) -> rusqlite::Result<()> {
        tx.execute(
            &format!(
                "INSERT INTO {table}
                   ({key}, face_size, tile_size, base, low_m, high_m, sea_level_m)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT ({key}) DO UPDATE SET
                   face_size = excluded.face_size, tile_size = excluded.tile_size,
                   base = excluded.base, low_m = excluded.low_m,
                   high_m = excluded.high_m, sea_level_m = excluded.sea_level_m"
            ),
            params![
                id,
                self.face_size,
                self.tile_size,
                self.base,
                self.low_m,
                self.high_m,
                self.sea_level_m
            ],
        )?;
        Ok(())
    }
}

fn write_terrain(tx: &Transaction, terrain: &Terrain) -> rusqlite::Result<()> {
    TerrainRow::of(terrain).write(tx, "isotherm_terrain", "id", 1)?;
    tx.execute("DELETE FROM isotherm_tile", [])?;
    let heights = &terrain.heights;
    let side = heights.tile_size();
    let mut insert = tx.prepare(
        "INSERT INTO isotherm_tile (face, tile_column, tile_row, data) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for index in heights.allocated() {
        let (face, column, row) = heights.tile_key(index);
        let texels = heights.tile(index).expect("an allocated tile");
        insert.execute(params![
            face as i64,
            column as i64,
            row as i64,
            tile::encode(texels, side)
        ])?;
    }
    Ok(())
}

fn read_terrain(conn: &Connection) -> Result<Terrain, StoreError> {
    let row = TerrainRow::read(conn, "isotherm_terrain", "id", 1)?;
    let size = |n: i64| usize::try_from(n).ok().filter(|&n| n > 0);
    let (Some(face_size), Some(side), Ok(base)) = (
        size(row.face_size),
        size(row.tile_size),
        u16::try_from(row.base),
    ) else {
        return Err(corrupt("the size or the base of the terrain"));
    };
    let mut heights = Heightmap::with_tile_size(face_size, side, base);
    let per_side = face_size.div_ceil(side);
    let mut statement =
        conn.prepare("SELECT face, tile_column, tile_row, data FROM isotherm_tile")?;
    let mut rows = statement.query([])?;
    while let Some(r) = rows.next()? {
        let key: (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let index = |n: i64, limit: usize| usize::try_from(n).ok().filter(|&n| n < limit);
        let (Some(face), Some(column), Some(tile_row)) = (
            index(key.0, FACES),
            index(key.1, per_side),
            index(key.2, per_side),
        ) else {
            return Err(corrupt(format!("terrain tile key {key:?}")));
        };
        let blob: Vec<u8> = r.get(3)?;
        let texels = tile::decode(&blob, side)
            .ok_or_else(|| corrupt(format!("the texels of terrain tile {key:?}")))?;
        heights.load_tile(face, column, tile_row, &texels);
    }
    Ok(Terrain {
        heights,
        low_m: row.low_m,
        high_m: row.high_m,
        sea_level_m: row.sea_level_m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a world file of format version 1, with one planet step in the
    /// undo log.
    fn v1_file(path: &Path, current: &Planet, undone: &Planet) {
        let v1_numbers = |p: &Planet| -> Vec<f64> {
            let n = planet_numbers(p);
            vec![
                n[0],
                n[1],
                n[2],
                n[3],
                n[5],
                n[6],
                n[7],
                p.solar_day_s(),
                n[9],
                n[10],
                n[11],
            ]
        };
        let mut conn = Connection::open(path).unwrap();
        let tx = conn.transaction().unwrap();
        v1(&tx).unwrap();
        tx.execute_batch(
            "INSERT INTO isotherm_meta (key, value) VALUES
               ('format_version', '1'), ('name', 'Old');",
        )
        .unwrap();
        tx.execute(
            "INSERT INTO isotherm_planet (id, radius_m, gravity, luminosity_w, temperature_k,
               semi_major_m, eccentricity, perihelion_deg, day_s, tilt_deg,
               surface_pressure_pa, co2_ppm)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params_from_iter(v1_numbers(current)),
        )
        .unwrap();
        for (position, moon) in current.moons.iter().enumerate() {
            tx.execute(
                "INSERT INTO isotherm_moon (position, mass_kg, distance_m) VALUES (?1, ?2, ?3)",
                params![position as i64, moon.mass_kg, moon.distance_m],
            )
            .unwrap();
        }
        let moons = undone.moons.iter().flat_map(|m| [m.mass_kg, m.distance_m]);
        let blob: Vec<u8> = v1_numbers(undone)
            .into_iter()
            .chain(moons)
            .flat_map(f64::to_le_bytes)
            .collect();
        push_step(&tx, "planet", None, Some(&blob)).unwrap();
        write_terrain(&tx, &Terrain::flat(4, -100.0, 100.0, 7)).unwrap();
        tx.commit().unwrap();
    }

    fn assert_close(a: &Planet, b: &Planet) {
        let (a_day, b_day) = (a.rotation.sidereal_day_s, b.rotation.sidereal_day_s);
        assert!((a_day - b_day).abs() < 1e-3, "{a_day} {b_day}");
        let mut b = b.clone();
        b.rotation.sidereal_day_s = a_day;
        assert_eq!(*a, b);
    }

    #[test]
    fn a_version_1_file_gets_the_sidereal_day_and_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.gpkg");
        let current = Planet::earth();
        let mut undone = Planet::earth();
        undone.rotation.sidereal_day_s = 40_000.0;
        undone.atmosphere.co2_ppm = 560.0;
        v1_file(&path, &current, &undone);

        let (mut file, world) = WorldFile::open(&path).unwrap();
        assert_eq!(world.name, "Old");
        assert_close(&world.planet, &current);
        assert!((world.planet.solar_day_s() - 86_400.0).abs() < 1.0);
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);

        let Some(Restored::Planet(planet)) = file.undo().unwrap() else {
            panic!("no planet step");
        };
        assert_close(&planet, &undone);
        drop(file);
        let (_, world) = WorldFile::open(&path).unwrap();
        assert_close(&world.planet, &undone);
    }
}
