//! The project file: a GeoPackage with extra `vale_` tables.
//!
//! Each vector layer is one GeoPackage feature table. A raster layer has no
//! feature table. The `vale_` tables hold the world, the layer order, the
//! attribute schema, the rasters, and the raster tiles, the undo log, and the
//! picture of the world. They are in
//! `gpkg_contents` with the data type `vale`. GDAL, and thus QGIS, lists a
//! table that is not in `gpkg_contents` as a layer, and it does not list a
//! table with a data type that it does not know.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read as _, Write as _};
use std::path::Path;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, params, params_from_iter};

use crate::tile::{self, TILE_SIZE};
use crate::undo::{self, UNDO_LIMIT};
use crate::{
    Feature, Field, FieldType, Geometry, GeometryKind, Hillshade, Layer, LayerId, LayerKind,
    LonLat, Project, Raster, RasterDisplay, RasterId, Rgb, Smoothing, Stop, StopLine, Units, Uuid,
    Value, World, clamp_feature_size, clamp_opacity,
};

/// The version of the `vale_` tables that this code writes.
pub const FORMAT_VERSION: u32 = 21;

/// One step for each format version. Step `n` changes a file of version `n`
/// to version `n + 1`. A new version adds a step and never changes an old one.
const MIGRATIONS: &[fn(&Transaction) -> rusqlite::Result<()>] = &[
    v1,
    v2,
    undo::v3,
    v4,
    v5,
    v6,
    v7,
    v8,
    v9,
    v10,
    v11,
    v12,
    v13,
    v14,
    v15,
    v16,
    v17,
    v18,
    v19,
    v20,
    v21,
];

/// `GPKG` in ASCII.
const APPLICATION_ID: i64 = 0x4750_4B47;
/// GeoPackage 1.3.
const GPKG_VERSION: i64 = 10300;
/// The row of the world sphere in `gpkg_spatial_ref_sys`.
const WORLD_SRS: i32 = 100_000;
/// The number of tiles that one step of a save or a load holds in memory.
const TILE_BATCH: usize = 256;

const WGS84: &str = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4326"]]"#;

/// An error from a project file.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    /// The file is not a Vale project.
    NotAProject,
    /// A newer version of the app wrote the file.
    NewerFormat {
        file: u32,
        app: u32,
    },
    /// The file is a Vale project, but a record in it is not valid.
    Corrupt(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "cannot write the project file: {e}"),
            StoreError::Sqlite(e) => write!(f, "project file error: {e}"),
            StoreError::NotAProject => write!(f, "the file is not a Vale project"),
            StoreError::NewerFormat { file, app } => write!(
                f,
                "the project has format version {file}, and this app reads up to version {app}"
            ),
            StoreError::Corrupt(what) => write!(f, "the project file is damaged: {what}"),
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

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::NotADatabase) => StoreError::NotAProject,
            _ => StoreError::Sqlite(e),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub(crate) fn corrupt(what: impl Into<String>) -> StoreError {
    StoreError::Corrupt(what.into())
}

/// The key of one raster tile inside its raster.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    /// Level 0 has the face size of the raster.
    pub level: u8,
    /// A number from 0 to 5.
    pub face: u8,
    /// The column and the row count tiles from the corner of the face.
    pub column: u32,
    pub row: u32,
}

/// The tiles of one raster that are not the same as in the file.
pub struct TileChanges<'a> {
    pub raster: RasterId,
    /// The file must hold no tile of the raster other than `put`.
    pub all: bool,
    /// The tiles to write. Each one has `TILE_SIZE` by `TILE_SIZE` pixels,
    /// row by row.
    pub put: Vec<(TileKey, &'a [u16])>,
    /// The tiles to remove.
    pub delete: Vec<TileKey>,
}

/// The raster tile rows that a save changed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveStats {
    pub tiles_written: usize,
    pub tiles_deleted: usize,
}

/// The tiles that an undo or a redo changed in the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    pub tiles: Vec<(RasterId, TileKey)>,
}

/// A small picture of the world, for a list of projects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    /// The pixels, row by row, with 4 bytes for each pixel.
    pub rgba: Vec<u8>,
}

/// The records of a project file that a list of projects shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The name of the world.
    pub name: String,
    pub thumbnail: Option<Thumbnail>,
}

/// Opens a file for a read. If a save of the file did not end, the file has
/// a journal, and a read-only connection cannot read it. The connection
/// then puts back the state from before that save.
fn open_for_read(path: &Path) -> Result<Connection, StoreError> {
    let open = |flags| -> rusqlite::Result<Connection> {
        let conn = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))?;
        Ok(conn)
    };
    match open(OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::ReadOnly) => {
            Ok(open(OpenFlags::SQLITE_OPEN_READ_WRITE)?)
        }
        other => Ok(other?),
    }
}

/// Reads the summary of a project file. This does not read the features, and
/// it does not migrate the file. A file of an older format version has no
/// thumbnail.
pub fn summary(path: &Path) -> Result<Summary, StoreError> {
    use rusqlite::OptionalExtension;
    let conn = open_for_read(path)?;
    let version = format_version(&conn)?;
    if version > FORMAT_VERSION {
        return Err(StoreError::NewerFormat {
            file: version,
            app: FORMAT_VERSION,
        });
    }
    let name = conn
        .query_row("SELECT name FROM vale_world", [], |r| r.get(0))
        .optional()?
        .ok_or_else(|| corrupt("no world record".to_string()))?;
    if version < 6 {
        return Ok(Summary {
            name,
            thumbnail: None,
        });
    }
    let row: Option<(u32, u32, Vec<u8>)> = conn
        .query_row("SELECT width, height, data FROM vale_thumbnail", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let thumbnail = match row {
        Some((width, height, data)) => {
            let mut rgba = Vec::new();
            let read = DeflateDecoder::new(data.as_slice()).read_to_end(&mut rgba);
            let len = u64::from(width) * u64::from(height) * 4;
            if read.is_err() || rgba.len() as u64 != len {
                return Err(corrupt("the pixels of the thumbnail".to_string()));
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

/// An open project file.
pub struct ProjectFile {
    conn: Connection,
    /// The project that the file holds.
    project: Project,
    undo_limit: u64,
    can_undo: bool,
    can_redo: bool,
    undo_bytes: u64,
}

impl ProjectFile {
    /// Writes a new project file. A file that is already at the path is replaced.
    pub fn create(path: &Path, project: &Project) -> Result<ProjectFile, StoreError> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let mut conn = Connection::open(path)?;
        migrate(&mut conn, 0)?;
        let tx = conn.transaction()?;
        write_edit(&tx, None, project, &[], 0)?;
        tx.commit()?;
        ProjectFile::with(conn, project.clone())
    }

    fn with(conn: Connection, project: Project) -> Result<ProjectFile, StoreError> {
        let mut file = ProjectFile {
            conn,
            project,
            undo_limit: UNDO_LIMIT,
            can_undo: false,
            can_redo: false,
            undo_bytes: 0,
        };
        file.read_log()?;
        Ok(file)
    }

    fn read_log(&mut self) -> Result<(), StoreError> {
        (self.can_undo, self.can_redo, self.undo_bytes) = undo::state(&self.conn)?;
        Ok(())
    }

    /// The project that the file holds.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Opens a project file and reads the project. A file of an older format
    /// version is migrated first.
    pub fn open(path: &Path) -> Result<(ProjectFile, Project), StoreError> {
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
            migrate(&mut conn, version)?;
        }
        let project = load(&conn)?;
        Ok((ProjectFile::with(conn, project.clone())?, project))
    }

    /// The name of the world of a project file. The call does not change
    /// the file.
    pub fn world_name(path: &Path) -> Result<String, StoreError> {
        let conn = open_for_read(path)?;
        format_version(&conn)?;
        Ok(conn.query_row("SELECT name FROM vale_world", [], |r| r.get(0))?)
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
            "INSERT OR REPLACE INTO vale_thumbnail (id, width, height, data)
             VALUES (1, ?1, ?2, ?3)",
            params![thumbnail.width, thumbnail.height, data],
        )?;
        Ok(())
    }

    /// Writes the style of each of these layers. This is not an edit, so it
    /// gets no undo step. An ID of no layer is skipped.
    pub fn set_styles(&mut self, styles: &[(LayerId, String)]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        for (id, style) in styles {
            tx.execute(
                "UPDATE vale_layer SET style = ?2 WHERE id = ?1",
                params![id.0, style],
            )?;
        }
        tx.commit()?;
        for (id, style) in styles {
            if let Some(layer) = self.project.layers.iter_mut().find(|l| l.id == *id) {
                layer.style = Some(style.clone());
            }
        }
        Ok(())
    }

    /// Writes the project as one edit. Each feature keeps its UUID, and each
    /// raster keeps its tiles.
    pub fn save(&mut self, project: &Project) -> Result<(), StoreError> {
        self.save_tiles(project, &[]).map(|_| ())
    }

    /// Writes the project and the changed raster tiles as one edit: one
    /// transaction with one undo step. Each other tile stays as it is. A
    /// raster that is not in the project loses its tiles, and so does a
    /// raster with a new face size. An edit that changes nothing makes no
    /// step. Any other edit removes the redo steps.
    pub fn save_tiles(
        &mut self,
        project: &Project,
        changes: &[TileChanges],
    ) -> Result<SaveStats, StoreError> {
        let tx = self.conn.transaction()?;
        let stats = write_edit(&tx, Some(&self.project), project, changes, self.undo_limit)?;
        tx.commit()?;
        self.project = project.clone();
        self.read_log()?;
        Ok(stats)
    }

    /// Writes the project in one transaction with no undo step, and removes
    /// each step of the log. Each raster keeps its tiles.
    pub fn save_base(&mut self, project: &Project) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        write_edit(&tx, None, project, &[], 0)?;
        undo::clear(&tx)?;
        tx.commit()?;
        self.project = project.clone();
        self.read_log()
    }

    /// True if the log has a step to undo.
    pub fn can_undo(&self) -> bool {
        self.can_undo
    }

    /// True if the log has a step to redo.
    pub fn can_redo(&self) -> bool {
        self.can_redo
    }

    /// Puts back the file from before the last edit, in one transaction.
    /// `None`: the log has no step to undo.
    pub fn undo(&mut self) -> Result<Option<Restored>, StoreError> {
        self.step(false)
    }

    /// Puts back the edit that the last `undo` took away.
    pub fn redo(&mut self) -> Result<Option<Restored>, StoreError> {
        self.step(true)
    }

    fn step(&mut self, redo: bool) -> Result<Option<Restored>, StoreError> {
        let tx = self.conn.transaction()?;
        let Some(step) = undo::next(&tx, redo)? else {
            return Ok(None);
        };
        let project = undo::swap_records(&tx, step, &self.project)?;
        write_records(&tx, &project)?;
        let tiles = undo::swap_tiles(&tx, step)?;
        remove_rasters(&tx, &project)?;
        undo::moved(&tx, step, !redo, self.undo_limit)?;
        tx.commit()?;
        self.project = project;
        self.read_log()?;
        Ok(Some(Restored { tiles }))
    }

    /// The size of the undo log, in bytes of compressed images.
    pub fn undo_bytes(&self) -> u64 {
        self.undo_bytes
    }

    /// The limit of the size of the undo log, in bytes.
    pub fn undo_limit(&self) -> u64 {
        self.undo_limit
    }

    /// Sets the limit of the size of the undo log. The next edit, undo, or
    /// redo removes the steps that are over the limit. A step that is larger
    /// than the limit goes.
    pub fn set_undo_limit(&mut self, bytes: u64) {
        self.undo_limit = bytes;
    }

    /// Reads one tile of a raster. `None`: the file has no such tile.
    pub fn load_tile(
        &self,
        raster: RasterId,
        key: TileKey,
    ) -> Result<Option<Vec<u16>>, StoreError> {
        use rusqlite::OptionalExtension;
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM vale_raster_tile WHERE raster_id = ?1 AND level = ?2
                   AND face = ?3 AND tile_column = ?4 AND tile_row = ?5",
                params![raster.0, key.level, key.face, key.column, key.row],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else {
            return Ok(None);
        };
        let pixels = tile::decode(&blob);
        Ok(Some(pixels.ok_or_else(|| {
            corrupt(format!("the pixels of raster tile {key:?}"))
        })?))
    }

    /// Reads each tile of a raster, in key order. `tile` gets `TILE_SIZE` by
    /// `TILE_SIZE` pixels, row by row.
    pub fn load_tiles(
        &self,
        raster: &Raster,
        mut tile: impl FnMut(TileKey, Vec<u16>),
    ) -> Result<(), StoreError> {
        let per_side = (raster.face_size as usize).div_ceil(TILE_SIZE) as u32;
        let mut rows = self.conn.prepare(
            "SELECT level, face, tile_column, tile_row, data FROM vale_raster_tile
             WHERE raster_id = ?1 ORDER BY level, face, tile_row, tile_column",
        )?;
        let mut rows = rows.query([raster.id.0])?;
        let mut batch: Vec<(TileKey, Vec<u8>)> = Vec::new();
        loop {
            let row = rows.next()?;
            if let Some(r) = row {
                let key: (i64, i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
                let inside = |n: i64, end: u32| (0..i64::from(end)).contains(&n);
                let (level, face, column, tile_row) = key;
                if level != 0
                    || !inside(face, 6)
                    || !inside(column, per_side)
                    || !inside(tile_row, per_side)
                {
                    return Err(corrupt(format!("raster tile key {key:?}")));
                }
                let key = TileKey {
                    level: 0,
                    face: face as u8,
                    column: column as u32,
                    row: tile_row as u32,
                };
                batch.push((key, r.get(4)?));
            }
            if batch.len() == TILE_BATCH || row.is_none() {
                let pixels = tile::parallel_map(&batch, |(_, blob)| tile::decode(blob));
                for ((key, _), pixels) in batch.drain(..).zip(pixels) {
                    let pixels = pixels
                        .ok_or_else(|| corrupt(format!("the pixels of raster tile {key:?}")))?;
                    tile(key, pixels);
                }
            }
            if row.is_none() {
                return Ok(());
            }
        }
    }
}

/// The format version of a file, or `NotAProject`.
fn format_version(conn: &Connection) -> Result<u32, StoreError> {
    let id: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'vale_meta')",
        [],
        |r| r.get(0),
    )?;
    if id != APPLICATION_ID || !has_meta {
        return Err(StoreError::NotAProject);
    }
    let text = meta(conn, "format_version")?;
    text.parse()
        .map_err(|_| corrupt(format!("format version {text:?}")))
}

fn meta(conn: &Connection, key: &str) -> Result<String, StoreError> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT value FROM vale_meta WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or_else(|| corrupt(format!("no {key} record")))
}

/// Adds the seed of the world.
fn v5(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_world ADD COLUMN seed INTEGER NOT NULL DEFAULT 0")
}

/// Runs each step from version `from` to `FORMAT_VERSION` in one transaction.
fn migrate(conn: &mut Connection, from: u32) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    for step in &MIGRATIONS[from as usize..] {
        step(&tx)?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO vale_meta (key, value) VALUES ('format_version', ?1)",
        [FORMAT_VERSION.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

/// The tables that GeoPackage requires, and the first `vale_` tables.
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
         INSERT INTO gpkg_spatial_ref_sys VALUES
           ('Undefined Cartesian SRS', -1, 'NONE', -1, 'undefined', NULL),
           ('Undefined geographic SRS', 0, 'NONE', 0, 'undefined', NULL);
         CREATE TABLE vale_meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL);
         CREATE TABLE vale_world (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           name TEXT NOT NULL,
           radius_km REAL NOT NULL);
         CREATE TABLE vale_layer (
           id INTEGER PRIMARY KEY,
           position INTEGER NOT NULL UNIQUE,
           name TEXT NOT NULL,
           kind TEXT NOT NULL,
           table_name TEXT NOT NULL UNIQUE,
           source TEXT);
         CREATE TABLE vale_field (
           layer_id INTEGER NOT NULL REFERENCES vale_layer(id),
           position INTEGER NOT NULL,
           name TEXT NOT NULL,
           column_name TEXT NOT NULL,
           kind TEXT NOT NULL,
           PRIMARY KEY (layer_id, position));
         CREATE TABLE gpkg_extensions (
           table_name TEXT,
           column_name TEXT,
           extension_name TEXT NOT NULL,
           definition TEXT NOT NULL,
           scope TEXT NOT NULL,
           CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name));
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           SELECT name, 'vale', name FROM sqlite_master
           WHERE type = 'table' AND name GLOB 'vale_*';
         INSERT INTO gpkg_extensions
           SELECT table_name, NULL, 'vale_project', 'Records of the Vale app', 'read-write'
           FROM gpkg_contents WHERE data_type = 'vale';"
    ))?;
    tx.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES
           ('WGS 84 geodetic', 4326, 'EPSG', 4326, ?1, NULL)",
        [WGS84],
    )?;
    Ok(())
}

/// The raster tables.
fn v2(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE vale_raster (
           id INTEGER PRIMARY KEY,
           position INTEGER NOT NULL,
           name TEXT NOT NULL,
           face_size INTEGER NOT NULL,
           unit TEXT NOT NULL,
           value_min REAL NOT NULL,
           value_max REAL NOT NULL,
           base INTEGER NOT NULL,
           limits TEXT NOT NULL,
           ramp_sea TEXT NOT NULL,
           ramp_land TEXT NOT NULL);
         CREATE TABLE vale_raster_tile (
           raster_id INTEGER NOT NULL REFERENCES vale_raster(id),
           level INTEGER NOT NULL,
           face INTEGER NOT NULL CHECK (face BETWEEN 0 AND 5),
           tile_column INTEGER NOT NULL,
           tile_row INTEGER NOT NULL,
           data BLOB NOT NULL,
           PRIMARY KEY (raster_id, level, face, tile_column, tile_row));
         INSERT INTO vale_meta (key, value) VALUES ('next_raster_id', '0');
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           VALUES ('vale_raster', 'vale', 'vale_raster'),
                  ('vale_raster_tile', 'vale', 'vale_raster_tile');
         INSERT INTO gpkg_extensions
           SELECT table_name, NULL, 'vale_project', 'Records of the Vale app', 'read-write'
           FROM gpkg_contents WHERE table_name IN ('vale_raster', 'vale_raster_tile');",
    )
}

/// The picture of the world.
fn v6(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE vale_thumbnail (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           width INTEGER NOT NULL,
           height INTEGER NOT NULL,
           data BLOB NOT NULL);
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           VALUES ('vale_thumbnail', 'vale', 'vale_thumbnail');
         INSERT INTO gpkg_extensions
           VALUES ('vale_thumbnail', NULL, 'vale_project', 'Records of the Vale app', 'read-write');",
    )
}

/// The visibility of a layer. The position of a layer becomes its place in
/// the draw order, and the layers of an older file get the order in which
/// the app drew them: polygons, then lines, then points.
fn v7(tx: &Transaction) -> rusqlite::Result<()> {
    const RANK: &str = "CASE kind WHEN 'polygon' THEN 0 WHEN 'line' THEN 1 ELSE 2 END";
    tx.execute_batch(&format!(
        "ALTER TABLE vale_layer ADD COLUMN visible INTEGER NOT NULL DEFAULT 1;
         CREATE TEMP TABLE vale_layer_order AS
           SELECT id, ROW_NUMBER() OVER (ORDER BY {RANK}, position) AS place FROM vale_layer;
         UPDATE vale_layer SET position = -(SELECT place FROM vale_layer_order o
           WHERE o.id = vale_layer.id);
         UPDATE vale_layer SET position = -position - 1;
         DROP TABLE vale_layer_order;"
    ))
}

/// The visibility of a raster.
fn v8(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN visible INTEGER NOT NULL DEFAULT 1")
}

/// A layer record for each raster, and the opacity of a raster. The layer has
/// the name and the visibility of the raster. The raster layers of an older
/// file go below its vector layers, in the order of the rasters. The undo
/// steps of an older file have no layer image for a raster, so they go.
fn v9(tx: &Transaction) -> rusqlite::Result<()> {
    const NEXT_ID: &str =
        "SELECT CAST(value AS INTEGER) FROM vale_meta WHERE key = 'next_layer_id'";
    const COUNT: &str = "SELECT COUNT(*) FROM vale_raster";
    tx.execute_batch(&format!(
        "ALTER TABLE vale_layer ADD COLUMN raster_id INTEGER REFERENCES vale_raster(id);
         ALTER TABLE vale_raster ADD COLUMN opacity REAL NOT NULL DEFAULT 1;
         UPDATE vale_layer SET position = -position - 1;
         UPDATE vale_layer SET position = -position - 1 + ({COUNT});
         CREATE TEMP TABLE vale_raster_order AS
           SELECT id, ROW_NUMBER() OVER (ORDER BY position, id) - 1 AS place FROM vale_raster;
         INSERT INTO vale_layer (id, position, name, kind, table_name, visible, raster_id)
           SELECT ({NEXT_ID}) + o.place, o.place, r.name, 'raster', 'vale_raster_' || r.id,
             r.visible, r.id
           FROM vale_raster r JOIN vale_raster_order o ON o.id = r.id;
         UPDATE vale_meta SET value = CAST(value AS INTEGER) + ({COUNT})
           WHERE key = 'next_layer_id';
         DROP TABLE vale_raster_order;
         ALTER TABLE vale_raster DROP COLUMN position;
         ALTER TABLE vale_raster DROP COLUMN name;
         ALTER TABLE vale_raster DROP COLUMN visible;
         DELETE FROM vale_undo_record;
         DELETE FROM vale_undo_tile;
         DELETE FROM vale_undo_step;"
    ))
}

/// The lines of the color stops. A stop of an older file has no line, and
/// its text and its undo images stay valid.
fn v12(_: &Transaction) -> rusqlite::Result<()> {
    Ok(())
}

/// The smoothing of a raster. The rasters of an older file get the default
/// smoothing, and so do their undo images.
fn v13(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "ALTER TABLE vale_raster ADD COLUMN smooth INTEGER NOT NULL DEFAULT 1;
         ALTER TABLE vale_raster ADD COLUMN feature_size REAL NOT NULL DEFAULT 2;",
    )
}

/// The hillshade of a raster. The rasters of an older file get the default
/// hillshade, and so do their undo images.
fn v15(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "ALTER TABLE vale_raster ADD COLUMN hillshade INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE vale_raster ADD COLUMN hillshade_opacity REAL NOT NULL DEFAULT 0.5;",
    )
}

/// The highlights, the light, the relief, and the shade color of the
/// hillshade. A file of version 15 can already have some of the columns,
/// and the step adds each column that it does not have.
fn v16(tx: &Transaction) -> rusqlite::Result<()> {
    const COLUMNS: [(&str, &str); 5] = [
        ("hillshade_highlights", "INTEGER NOT NULL DEFAULT 1"),
        ("hillshade_azimuth", "REAL NOT NULL DEFAULT 315"),
        ("hillshade_altitude", "REAL NOT NULL DEFAULT 45"),
        ("hillshade_exaggeration", "REAL NOT NULL DEFAULT 1"),
        ("hillshade_color", "TEXT NOT NULL DEFAULT '000000'"),
    ];
    let sql = "SELECT count(*) FROM pragma_table_info('vale_raster') WHERE name = ?1";
    for (name, kind) in COLUMNS {
        let count: i64 = tx.query_row(sql, [name], |r| r.get(0))?;
        if count == 0 {
            tx.execute_batch(&format!(
                "ALTER TABLE vale_raster ADD COLUMN {name} {kind};"
            ))?;
        }
    }
    Ok(())
}

/// The switch of the rivers of a raster. The rasters of an older file show
/// their rivers, and so do their undo images.
fn v17(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN rivers INTEGER NOT NULL DEFAULT 1")
}

/// The switch of the lines of the stops of a raster. The rasters of an older
/// file show their lines, and so do their undo images.
fn v18(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN lines INTEGER NOT NULL DEFAULT 1")
}

/// The default value of a field, and the whole number type. A column of an
/// older file with values of more than one type becomes a text column. A
/// file of version 18 can already have the default values.
fn v19(tx: &Transaction) -> rusqlite::Result<()> {
    let sql = "SELECT count(*) FROM pragma_table_info('vale_field') WHERE name = 'default_value'";
    let count: i64 = tx.query_row(sql, [], |r| r.get(0))?;
    if count == 0 {
        tx.execute_batch("ALTER TABLE vale_field ADD COLUMN default_value")?;
    }
    let mixed: Vec<(String, String)> = tx
        .prepare(
            "SELECT l.table_name, f.column_name FROM vale_field f
             JOIN vale_layer l ON l.id = f.layer_id WHERE f.kind = 'mixed'",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (table, column) in mixed {
        let (table, column) = (quote(&table), quote(&column));
        let cells: Vec<(i64, SqlValue)> = {
            let mut select = tx.prepare(&format!("SELECT fid, {column} FROM {table}"))?;
            // A cell that the old format cannot read stays as it is.
            let rows = select.query_map([], |r| {
                let cell = r.get_ref(1)?;
                let cell = match old_mixed(cell) {
                    Some(value) => write_value(FieldType::Text, &value),
                    None => SqlValue::from(cell),
                };
                Ok((r.get(0)?, cell))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let update = format!("UPDATE {table} SET {column} = ?1 WHERE fid = ?2");
        for (fid, cell) in cells {
            tx.execute(&update, params![cell, fid])?;
        }
    }
    tx.execute_batch("UPDATE vale_field SET kind = 'text' WHERE kind = 'mixed'")
}

/// The style of a layer. The layers of an older file have no style. A file
/// of version 19 can already have the column.
fn v20(tx: &Transaction) -> rusqlite::Result<()> {
    let sql = "SELECT count(*) FROM pragma_table_info('vale_layer') WHERE name = 'style'";
    let count: i64 = tx.query_row(sql, [], |r| r.get(0))?;
    if count == 0 {
        tx.execute_batch("ALTER TABLE vale_layer ADD COLUMN style TEXT")?;
    }
    Ok(())
}

/// The style of each layer holds the colors of its raster. The app moves the colors of the raster records into the styles when it opens an older file, so the step changes no table.
fn v21(_tx: &Transaction) -> rusqlite::Result<()> {
    Ok(())
}

/// The display of a raster. The rasters of an older file show bands.
fn v10(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN display TEXT NOT NULL DEFAULT 'bands'")
}

/// The color stops of a raster. The stops of an older raster have the
/// colors at the edges of its bands, and its band limits and ramps go. The
/// undo images of the rasters change, so the undo steps go.
fn v11(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN stops TEXT NOT NULL DEFAULT ''")?;
    let rows: Vec<(i64, String)> = {
        let mut select = tx.prepare(
            "SELECT id, unit, value_min, value_max, limits, ramp_sea, ramp_land FROM vale_raster",
        )?;
        select
            .query_map([], |r| {
                let stops = v10_stops(
                    &r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    &r.get::<_, String>(4)?,
                    &r.get::<_, String>(5)?,
                    &r.get::<_, String>(6)?,
                );
                Ok((r.get(0)?, v13_stops_text(&stops)))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, stops) in rows {
        tx.execute(
            "UPDATE vale_raster SET stops = ?1 WHERE id = ?2",
            params![stops, id],
        )?;
    }
    tx.execute_batch(
        "ALTER TABLE vale_raster DROP COLUMN limits;
         ALTER TABLE vale_raster DROP COLUMN ramp_sea;
         ALTER TABLE vale_raster DROP COLUMN ramp_land;
         DELETE FROM vale_undo_record;
         DELETE FROM vale_undo_tile;
         DELETE FROM vale_undo_step;",
    )
}

/// The stops at the edges of the bands of a format version 10 raster: one at
/// `min`, one at each limit, and one at `max`. A height in meters with values
/// on both sides of 0 has a sea split: 0 is a limit, and the bands below it
/// use the sea side of the ramp. Bands that the app of version 10 did not
/// take give no stops, and the app then uses the default of the range.
fn v10_stops(unit: &str, min: f64, max: f64, limits: &str, sea: &str, land: &str) -> Vec<V13Stop> {
    const MAX_BANDS: usize = 32;
    const MIN_BAND: f64 = 1.0;
    let parts = (parse_limits(limits), parse_colors(sea), parse_colors(land));
    let (Some(values), Some(sea), Some(land)) = parts else {
        return Vec::new();
    };
    if !(min.is_finite() && max.is_finite() && min < max) {
        return Vec::new();
    }
    let split = unit == "m" && min < 0.0 && 0.0 < max;
    if land.is_empty() || (split && sea.is_empty()) {
        return Vec::new();
    }
    let mut limits = if split { vec![0.0] } else { Vec::new() };
    let mut has_sea = false;
    for value in values {
        if split && value == 0.0 && !has_sea {
            has_sea = true;
            continue;
        }
        let free = value.is_finite()
            && limits.len() + 1 < MAX_BANDS
            && (min + MIN_BAND..=max - MIN_BAND).contains(&value)
            && limits.iter().all(|m: &f64| (m - value).abs() >= MIN_BAND);
        if !free {
            return Vec::new();
        }
        let index = limits.partition_point(|&m| m < value);
        limits.insert(index, value);
    }
    if has_sea != split {
        return Vec::new();
    }
    let sea_bands = if split {
        limits.partition_point(|&m| m < 0.0) + 1
    } else {
        0
    };
    let land_bands = limits.len() + 1 - sea_bands;
    // The colors at the bottom and at the top of band `i`.
    let edges = |i: usize| {
        let (j, k, ramp) = match i < sea_bands {
            true => (i, sea_bands, &sea),
            false => (i - sea_bands, land_bands, &land),
        };
        let at = |offset: f64| v10_blend(ramp, (j as f64 + offset) / k as f64);
        (at(0.0), at(1.0))
    };
    let bottom = edges(0).0;
    let mut stops = vec![V13Stop {
        value: min,
        below: bottom,
        above: bottom,
        line: StopLine::default(),
    }];
    for (i, &value) in limits.iter().enumerate() {
        stops.push(V13Stop {
            value,
            below: edges(i).1,
            above: edges(i + 1).0,
            line: StopLine::default(),
        });
    }
    let top = edges(limits.len()).1;
    stops.push(V13Stop {
        value: max,
        below: top,
        above: top,
        line: StopLine::default(),
    });
    stops
}

/// One color for each band of a raster. The stops of an older raster have
/// two colors, and each band takes one of them as the app of version 13
/// did. The undo images of the rasters change in the same way.
fn v14(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_raster ADD COLUMN colors TEXT NOT NULL DEFAULT ''")?;
    let rows: Vec<(i64, String, String)> = {
        let mut select =
            tx.prepare("SELECT id, unit, value_min, value_max, stops FROM vale_raster")?;
        select
            .query_map([], |r| {
                let old = v13_parse_stops(&r.get::<_, String>(4)?);
                let (stops, colors) =
                    v14_bands(&r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, &old);
                Ok((r.get(0)?, stops_text(&stops), colors_text(&colors)))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, stops, colors) in rows {
        tx.execute(
            "UPDATE vale_raster SET stops = ?1, colors = ?2 WHERE id = ?3",
            params![stops, colors, id],
        )?;
    }
    undo::v14(tx)
}

/// A stop of format version 13: a value, the color just below it, the color
/// just above it, and a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct V13Stop {
    pub value: f64,
    pub below: Rgb,
    pub above: Rgb,
    pub line: StopLine,
}

/// The stops and the band colors of format version 14 from the stops of
/// version 13. A height in meters with values on both sides of 0 has a sea
/// split. A band below the stop at sea level takes the color below its
/// upper stop, and any other band takes the color above its lower stop. The
/// stops at the ends lose their lines. A darker line keeps its color as a
/// line of one color if the band above its stop now has another color.
/// Stops that the app of version 13 did not take give no stops.
pub(crate) fn v14_bands(unit: &str, min: f64, max: f64, old: &[V13Stop]) -> (Vec<Stop>, Vec<Rgb>) {
    let split = unit == "m" && min < 0.0 && 0.0 < max;
    let sea = old.iter().position(|s| s.value == 0.0).filter(|_| split);
    let valid = old.len() >= 2
        && (!split || sea.is_some())
        && old
            .iter()
            .enumerate()
            .all(|(i, s)| s.below == s.above || Some(i) == sea);
    if !valid {
        return (Vec::new(), Vec::new());
    }
    let colors: Vec<Rgb> = (0..old.len() - 1)
        .map(|i| match sea {
            Some(sea) if i < sea => old[i + 1].below,
            _ => old[i].above,
        })
        .collect();
    let last = old.len() - 1;
    let stops = old
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut line = s.line;
            let base = if Some(i) == sea { s.below } else { s.above };
            if i == 0 || i == last {
                line = StopLine::default();
            } else if line.darker && base != colors[i] {
                let keep = 1.0 - line.darken;
                line.darker = false;
                line.color = base.map(|c| (f32::from(c) * keep).round() as u8);
            }
            Stop {
                value: s.value,
                line,
            }
        })
        .collect();
    (stops, colors)
}

/// The stops of format version 13 as text, as `v13_parse_stops` reads them.
fn v13_stops_text(stops: &[V13Stop]) -> String {
    let items: Vec<String> = stops
        .iter()
        .map(|s| {
            let mut item = match s.below == s.above {
                true => format!("{}:{}", s.value, hex(s.below)),
                false => format!("{}:{}/{}", s.value, hex(s.below), hex(s.above)),
            };
            if s.line != StopLine::default() {
                item += &format!("|{}", line_text(s.line));
            }
            item
        })
        .collect();
    items.join(",")
}

/// The stops of format version 13: `value:rrggbb`, or `value:rrggbb/rrggbb`
/// for a stop with two colors, each with an optional line as in
/// `stops_text`. A text that is not valid, or with values that are not in
/// ascending order, gives no stops.
pub(crate) fn v13_parse_stops(text: &str) -> Vec<V13Stop> {
    let stop = |item: &str| {
        let (item, line) = match item.split_once('|') {
            Some((item, line)) => (item, parse_line(line)?),
            None => (item, StopLine::default()),
        };
        let (value, colors) = item.split_once(':')?;
        let value: f64 = value.parse().ok().filter(|v: &f64| v.is_finite())?;
        let (below, above) = colors.split_once('/').unwrap_or((colors, colors));
        Some(V13Stop {
            value,
            below: parse_color(below)?,
            above: parse_color(above)?,
            line,
        })
    };
    let stops: Option<Vec<V13Stop>> = text
        .split(',')
        .filter(|s| !s.is_empty())
        .map(stop)
        .collect();
    let stops = stops.unwrap_or_default();
    match stops.windows(2).all(|w| w[0].value < w[1].value) {
        true => stops,
        false => Vec::new(),
    }
}

/// The color at `t` along colors with even spacing, as the app of format
/// version 10 blends it.
fn v10_blend(colors: &[Rgb], t: f64) -> Rgb {
    match colors {
        [] => [0; 3],
        [one] => *one,
        _ => {
            let x = t.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
            let i = (x as usize).min(colors.len() - 2);
            let f = x - i as f64;
            let (a, b) = (colors[i], colors[i + 1]);
            std::array::from_fn(|c| {
                (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * f).round() as u8
            })
        }
    }
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Writes one edit. With `old`, the project that the file holds, the edit
/// gets an undo step.
fn write_edit(
    tx: &Transaction,
    old: Option<&Project>,
    project: &Project,
    changes: &[TileChanges],
    undo_limit: u64,
) -> Result<SaveStats, StoreError> {
    let step = match old {
        Some(old) => {
            let step = undo::begin(tx)?;
            undo::keep_records(tx, step, old, project)?;
            Some(step)
        }
        None => None,
    };
    let mut stats = SaveStats {
        tiles_deleted: remove_old_tiles(tx, project, step)?,
        tiles_written: 0,
    };
    write_records(tx, project)?;
    remove_rasters(tx, project)?;
    for change in changes {
        write_tiles(tx, change, step, &mut stats)?;
    }
    if let Some(step) = step {
        undo::end(tx, step, undo_limit)?;
    }
    Ok(stats)
}

/// Writes the world, the raster records, and the layers. The record of a
/// raster that is not in the project stays.
fn write_records(tx: &Transaction, project: &Project) -> Result<(), StoreError> {
    clear(tx)?;
    write_world(tx, &project.world)?;
    // A raster layer refers to the row of its raster.
    write_rasters(tx, project)?;
    tx.execute(
        "INSERT OR REPLACE INTO vale_meta (key, value) VALUES ('next_layer_id', ?1)",
        [project.next_id.to_string()],
    )?;
    let mut tables = BTreeSet::new();
    for (position, layer) in project.layers.iter().enumerate() {
        match layer.kind {
            LayerKind::Vector(kind) => {
                let table = table_name(layer, &mut tables);
                write_layer(tx, position, layer, &table, None)?;
                write_features(tx, layer, kind, &table)?;
            }
            LayerKind::Raster(raster) => {
                if project.raster(raster).is_none() {
                    return Err(corrupt(format!("layer {:?} has no raster", layer.name)));
                }
                // No table has this name. A feature table cannot have it,
                // because the prefix is reserved.
                let table = format!("vale_raster_{}", raster.0);
                write_layer(tx, position, layer, &table, Some(raster))?;
            }
        }
    }
    Ok(())
}

/// Removes the world, the layer records, and the feature tables.
fn clear(tx: &Transaction) -> Result<(), StoreError> {
    let tables: Vec<String> = tx
        .prepare("SELECT table_name FROM vale_layer WHERE raster_id IS NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for table in tables {
        tx.execute(&format!("DROP TABLE IF EXISTS {}", quote(&table)), [])?;
    }
    tx.execute_batch(&format!(
        "DELETE FROM gpkg_geometry_columns;
         DELETE FROM gpkg_contents WHERE data_type = 'features';
         DELETE FROM gpkg_spatial_ref_sys WHERE srs_id = {WORLD_SRS};
         DELETE FROM vale_field;
         DELETE FROM vale_layer;
         DELETE FROM vale_world;"
    ))?;
    Ok(())
}

/// The display units of the world.
fn v4(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE vale_world ADD COLUMN units TEXT NOT NULL DEFAULT 'metric';")
}

/// The world as a geographic coordinate system on a sphere.
fn world_wkt(world: &World) -> String {
    let name = world.name.replace('"', "'");
    let radius_m = world.radius_km * 1000.0;
    format!(
        "GEOGCS[\"{name}\",DATUM[\"{name}\",SPHEROID[\"{name}\",{radius_m},0]],\
         PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433],\
         AXIS[\"Longitude\",EAST],AXIS[\"Latitude\",NORTH]]"
    )
}

fn write_world(tx: &Transaction, world: &World) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO vale_world (id, name, radius_km, units, seed) VALUES (1, ?1, ?2, ?3, ?4)",
        params![world.name, world.radius_km, world.units.id(), world.seed],
    )?;
    tx.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES (?1, ?2, 'NONE', ?2, ?3, NULL)",
        params![world.name, WORLD_SRS, world_wkt(world)],
    )?;
    Ok(())
}

/// A table name for the vector layer that no other layer has. SQLite compares
/// names without case, and it keeps some prefixes for itself and for
/// GeoPackage.
fn table_name(layer: &Layer, taken: &mut BTreeSet<String>) -> String {
    let mut name: String = layer
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let lower = name.to_lowercase();
    let reserved = ["gpkg_", "rtree_", "sqlite_", "vale_"];
    if name.is_empty() || reserved.iter().any(|p| lower.starts_with(p)) {
        name = format!("layer_{name}");
    }
    if taken.contains(&name.to_lowercase()) {
        name = format!("{name}_{}", layer.id.0);
    }
    while !taken.insert(name.to_lowercase()) {
        name.push('_');
    }
    name
}

/// A column name for each field. A name that is empty, or the same as an
/// earlier column without case, gets a number at its end.
fn column_names(fields: &[String]) -> Vec<String> {
    let mut taken: BTreeSet<String> = ["fid", "geom", "uuid"].map(String::from).into();
    fields
        .iter()
        .map(|field| {
            let base = if field.is_empty() { "field" } else { field };
            let mut name = base.to_string();
            let mut n = 0;
            while !taken.insert(name.to_lowercase()) {
                n += 1;
                name = format!("{base}_{n}");
            }
            name
        })
        .collect()
}

fn column_type(kind: FieldType) -> &'static str {
    match kind {
        FieldType::Text => "TEXT",
        FieldType::Integer => "INTEGER",
        FieldType::Real => "DOUBLE",
        FieldType::Bool => "BOOLEAN",
    }
}

/// The cell of a value of a field of this type.
fn write_value(kind: FieldType, value: &Value) -> SqlValue {
    match kind.convert(value) {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(b as i64),
        Value::Number(n) if kind == FieldType::Integer => SqlValue::Integer(n as i64),
        Value::Number(n) => SqlValue::Real(n),
        Value::Text(s) => SqlValue::Text(s),
    }
}

/// The value of a cell of a field of this type. A cell of another type
/// turns into a value of the type, as `FieldType::convert` gives it.
fn read_value(kind: FieldType, cell: ValueRef) -> Option<Value> {
    let value = match cell {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Number(i as f64),
        ValueRef::Real(n) => Value::Number(n),
        ValueRef::Text(s) => Value::Text(std::str::from_utf8(s).ok()?.into()),
        ValueRef::Blob(_) => return None,
    };
    Some(kind.convert(&value))
}

/// The `DEFAULT` clause of the column of a field, or nothing.
fn default_clause(field: &Field) -> String {
    match field.kind.convert(&field.default) {
        Value::Null => String::new(),
        Value::Bool(b) => format!(" DEFAULT {}", b as u8),
        Value::Number(n) if !n.is_finite() => String::new(),
        Value::Number(n) if field.kind == FieldType::Integer => format!(" DEFAULT {}", n as i64),
        Value::Number(n) => format!(" DEFAULT {n:?}"),
        Value::Text(s) => format!(" DEFAULT '{}'", s.replace('\'', "''")),
    }
}

/// The value of a cell of a column of a format before version 19 with
/// values of more than one type. The first character of each cell gives
/// the type.
fn old_mixed(cell: ValueRef) -> Option<Value> {
    Some(match cell {
        ValueRef::Null => Value::Null,
        ValueRef::Text(s) => {
            let s = std::str::from_utf8(s).ok()?;
            let rest = s.get(1..)?;
            match s.as_bytes()[0] {
                b'B' => Value::Bool(rest == "1"),
                b'N' => Value::Number(rest.parse().ok()?),
                b'T' => Value::Text(rest.into()),
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn geometry_type(kind: GeometryKind) -> &'static str {
    match kind {
        GeometryKind::Point => "MULTIPOINT",
        GeometryKind::Line => "MULTILINESTRING",
        GeometryKind::Polygon => "MULTIPOLYGON",
    }
}

/// Writes the record of a layer.
fn write_layer(
    tx: &Transaction,
    position: usize,
    layer: &Layer,
    table: &str,
    raster: Option<RasterId>,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO vale_layer
           (id, position, name, kind, table_name, source, visible, raster_id, style)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            layer.id.0,
            position as i64,
            layer.name,
            layer.kind.name(),
            table,
            layer.source,
            layer.visible,
            raster.map(|id| id.0),
            layer.style
        ],
    )?;
    Ok(())
}

/// Writes the feature table and the fields of a vector layer.
fn write_features(
    tx: &Transaction,
    layer: &Layer,
    kind: GeometryKind,
    table: &str,
) -> Result<(), StoreError> {
    let columns = column_names(&layer.field_names());
    let mut create = format!(
        "CREATE TABLE {} (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, geom {}, \
         uuid TEXT NOT NULL UNIQUE",
        quote(table),
        geometry_type(kind)
    );
    let mut insert = format!("INSERT INTO {} (fid, geom, uuid", quote(table));
    for (i, (column, field)) in columns.iter().zip(&layer.fields).enumerate() {
        tx.execute(
            "INSERT INTO vale_field
               (layer_id, position, name, column_name, kind, default_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                layer.id.0,
                i as i64,
                field.name,
                column,
                field.kind.id(),
                write_value(field.kind, &field.default)
            ],
        )?;
        create += &format!(
            ", {} {}{}",
            quote(column),
            column_type(field.kind),
            default_clause(field)
        );
        insert += &format!(", {}", quote(column));
    }
    create += ")";
    insert += ") VALUES (?, ?, ?";
    insert += &", ?".repeat(columns.len());
    insert += ")";
    tx.execute(&create, [])?;
    tx.execute(
        "INSERT INTO gpkg_contents
           (table_name, data_type, identifier, description, min_x, min_y, max_x, max_y, srs_id)
         VALUES (?1, 'features', ?1, ?2, -180, -90, 180, 90, ?3)",
        params![table, layer.name, WORLD_SRS],
    )?;
    tx.execute(
        "INSERT INTO gpkg_geometry_columns VALUES (?1, 'geom', ?2, ?3, 0, 0)",
        params![table, geometry_type(kind), WORLD_SRS],
    )?;

    let mut insert = tx.prepare(&insert)?;
    for (i, feature) in layer.features.iter().enumerate() {
        if feature.geometry.kind() != kind {
            return Err(corrupt(format!(
                "layer {:?} holds a {} geometry",
                layer.name,
                feature.geometry.kind().name()
            )));
        }
        let mut row = vec![
            SqlValue::Integer(i as i64 + 1),
            SqlValue::Blob(encode_geometry(&feature.geometry)),
            SqlValue::Text(feature.uuid().to_string()),
        ];
        for field in &layer.fields {
            let value = feature.attributes.get(&field.name).unwrap_or(&Value::Null);
            row.push(write_value(field.kind, value));
        }
        insert.execute(params_from_iter(row))?;
    }
    Ok(())
}

fn parse_color(hex: &str) -> Option<Rgb> {
    let n = u32::from_str_radix(hex, 16).ok()?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some([
        (n >> 16) as u8,
        (n >> 8) as u8,
        n as u8,
    ])
}

fn parse_colors(text: &str) -> Option<Vec<Rgb>> {
    text.split(',')
        .filter(|s| !s.is_empty())
        .map(parse_color)
        .collect()
}

fn parse_limits(text: &str) -> Option<Vec<f64>> {
    let numbers = text.split(',').filter(|s| !s.is_empty());
    numbers.map(|s| s.parse().ok()).collect()
}

fn hex([r, g, b]: Rgb) -> String {
    format!("{r:02x}{g:02x}{b:02x}")
}

/// The colors as `rrggbb`, with commas between them.
fn colors_text(colors: &[Rgb]) -> String {
    let items: Vec<String> = colors.iter().map(|&c| hex(c)).collect();
    items.join(",")
}

/// A line as `on:weight:rrggbb:color:darken`, with `off` for a line that is
/// off and `darker` for a line with the darker color of the band.
fn line_text(line: StopLine) -> String {
    let on = if line.on { "on" } else { "off" };
    let color = if line.darker { "darker" } else { "color" };
    let (weight, darken) = (line.weight, line.darken);
    format!("{on}:{weight}:{}:{color}:{darken}", hex(line.color))
}

/// The stops as values with commas between them. A stop with a line that is
/// not the default line adds `|` and the text of its line.
fn stops_text(stops: &[Stop]) -> String {
    let items: Vec<String> = stops
        .iter()
        .map(|s| match s.line == StopLine::default() {
            true => s.value.to_string(),
            false => format!("{}|{}", s.value, line_text(s.line)),
        })
        .collect();
    items.join(",")
}

fn parse_line(text: &str) -> Option<StopLine> {
    let mut parts = text.split(':');
    let on = match parts.next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    let weight: f32 = parts.next()?.parse().ok().filter(|w: &f32| w.is_finite())?;
    let color = parse_color(parts.next()?)?;
    let darker = match parts.next()? {
        "darker" => true,
        "color" => false,
        _ => return None,
    };
    let darken: f32 = parts.next()?.parse().ok().filter(|d: &f32| d.is_finite())?;
    let line = StopLine {
        on,
        weight,
        color,
        darker,
        darken,
    };
    parts.next().is_none().then_some(line)
}

/// The stops of `stops_text`. A text that is not valid, or with values that
/// are not finite and in ascending order, gives no stops.
fn parse_stops(text: &str) -> Vec<Stop> {
    let stop = |item: &str| {
        let (value, line) = match item.split_once('|') {
            Some((value, line)) => (value, parse_line(line)?),
            None => (item, StopLine::default()),
        };
        let value: f64 = value.parse().ok().filter(|v: &f64| v.is_finite())?;
        Some(Stop { value, line })
    };
    let stops: Option<Vec<Stop>> = text
        .split(',')
        .filter(|s| !s.is_empty())
        .map(stop)
        .collect();
    let stops = stops.unwrap_or_default();
    match stops.windows(2).all(|w| w[0].value < w[1].value) {
        true => stops,
        false => Vec::new(),
    }
}

/// The stops and the band colors of a raster. Colors that are not valid, or
/// not one fewer than the stops, give no stops and no colors.
fn parse_bands(stops: &str, colors: &str) -> (Vec<Stop>, Vec<Rgb>) {
    let stops = parse_stops(stops);
    match parse_colors(colors) {
        Some(colors) if colors.len() + 1 == stops.len() => (stops, colors),
        _ => (Vec::new(), Vec::new()),
    }
}

/// Removes the tiles of each raster that is no longer in the project or has
/// a new face size. A step keeps their images. Returns the tile count.
fn remove_old_tiles(
    tx: &Transaction,
    project: &Project,
    step: Option<i64>,
) -> Result<usize, StoreError> {
    let stored: Vec<(u32, u32)> = tx
        .prepare("SELECT id, face_size FROM vale_raster")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut removed = 0;
    for (id, face_size) in stored {
        let raster = project.raster(RasterId(id));
        if raster.is_none_or(|r| r.face_size != face_size) {
            if let Some(step) = step {
                undo::keep_raster_tiles(tx, step, id)?;
            }
            removed += tx.execute("DELETE FROM vale_raster_tile WHERE raster_id = ?1", [id])?;
        }
    }
    Ok(removed)
}

/// Removes the record of each raster that is not in the project. Such a
/// raster must have no tiles.
fn remove_rasters(tx: &Transaction, project: &Project) -> Result<(), StoreError> {
    let stored: Vec<u32> = tx
        .prepare("SELECT id FROM vale_raster")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in stored {
        if project.raster(RasterId(id)).is_none() {
            tx.execute("DELETE FROM vale_raster WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Writes the raster records.
fn write_rasters(tx: &Transaction, project: &Project) -> Result<(), StoreError> {
    for r in &project.rasters {
        let shade = r.hillshade.clamped();
        // A replaced row is a deleted row, and the tiles refer to the row.
        tx.execute(
            "INSERT INTO vale_raster (id, face_size, unit, value_min, value_max, base,
               opacity, display, stops, colors, smooth, feature_size, hillshade,
               hillshade_opacity, hillshade_highlights, hillshade_azimuth,
               hillshade_altitude, hillshade_exaggeration, hillshade_color, rivers, lines)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
               ?16, ?17, ?18, ?19, ?20, ?21)
             ON CONFLICT (id) DO UPDATE SET
               face_size = excluded.face_size, unit = excluded.unit,
               value_min = excluded.value_min, value_max = excluded.value_max,
               base = excluded.base,
               opacity = excluded.opacity, display = excluded.display,
               stops = excluded.stops, colors = excluded.colors,
               smooth = excluded.smooth,
               feature_size = excluded.feature_size,
               hillshade = excluded.hillshade,
               hillshade_opacity = excluded.hillshade_opacity,
               hillshade_highlights = excluded.hillshade_highlights,
               hillshade_azimuth = excluded.hillshade_azimuth,
               hillshade_altitude = excluded.hillshade_altitude,
               hillshade_exaggeration = excluded.hillshade_exaggeration,
               hillshade_color = excluded.hillshade_color,
               rivers = excluded.rivers, lines = excluded.lines",
            params![
                r.id.0,
                r.face_size,
                r.unit,
                r.value_min,
                r.value_max,
                r.base,
                f64::from(clamp_opacity(r.opacity)),
                r.display.id(),
                stops_text(&r.stops),
                colors_text(&r.colors),
                r.smoothing.on,
                f64::from(clamp_feature_size(r.smoothing.feature_size)),
                shade.on,
                f64::from(shade.opacity),
                shade.highlights,
                f64::from(shade.azimuth),
                f64::from(shade.altitude),
                f64::from(shade.exaggeration),
                hex(shade.color),
                r.rivers,
                r.lines,
            ],
        )?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO vale_meta (key, value) VALUES ('next_raster_id', ?1)",
        [project.next_raster_id.to_string()],
    )?;
    Ok(())
}

/// Writes the changed tiles of one raster. A step keeps the image of each
/// tile from before the change. A tile with the pixels that the file holds
/// is not a change.
fn write_tiles(
    tx: &Transaction,
    change: &TileChanges,
    step: Option<i64>,
    stats: &mut SaveStats,
) -> Result<(), StoreError> {
    let raster = change.raster.0;
    let mut keep = tx.prepare(undo::KEEP_TILE)?;
    if change.all {
        if let Some(step) = step {
            undo::keep_raster_tiles(tx, step, raster)?;
        }
        stats.tiles_deleted += tx.execute(
            "DELETE FROM vale_raster_tile WHERE raster_id = ?1",
            [raster],
        )?;
    }
    let mut delete = tx.prepare(
        "DELETE FROM vale_raster_tile WHERE raster_id = ?1 AND level = ?2 AND face = ?3
           AND tile_column = ?4 AND tile_row = ?5",
    )?;
    let mut keep_old = tx.prepare(undo::KEEP_OLD_TILE)?;
    for key in &change.delete {
        if let Some(step) = step {
            keep_old.execute(params![
                step, raster, key.level, key.face, key.column, key.row
            ])?;
        }
        stats.tiles_deleted +=
            delete.execute(params![raster, key.level, key.face, key.column, key.row])?;
    }
    let mut same = tx.prepare(
        "SELECT EXISTS (SELECT 1 FROM vale_raster_tile WHERE raster_id = ?1 AND level = ?2
           AND face = ?3 AND tile_column = ?4 AND tile_row = ?5 AND data = ?6)",
    )?;
    let mut put = tx.prepare(
        "INSERT INTO vale_raster_tile (raster_id, level, face, tile_column, tile_row, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT DO UPDATE SET data = excluded.data",
    )?;
    for batch in change.put.chunks(TILE_BATCH) {
        let blobs = tile::parallel_map(batch, |(_, pixels)| tile::encode(pixels));
        for ((key, _), blob) in batch.iter().zip(blobs) {
            let at = params![raster, key.level, key.face, key.column, key.row, blob];
            if same.query_row(at, |r| r.get(0))? {
                continue;
            }
            if let Some(step) = step {
                keep.execute(params![
                    step, raster, key.level, key.face, key.column, key.row
                ])?;
            }
            put.execute(at)?;
            stats.tiles_written += 1;
        }
    }
    Ok(())
}

fn load_rasters(conn: &Connection) -> Result<Vec<Raster>, StoreError> {
    let mut rows = conn.prepare(
        "SELECT id, face_size, unit, value_min, value_max, base, opacity, display, stops,
           colors, smooth, feature_size, hillshade, hillshade_opacity,
           hillshade_highlights, hillshade_azimuth, hillshade_altitude,
           hillshade_exaggeration, hillshade_color, rivers, lines
         FROM vale_raster ORDER BY id",
    )?;
    let mut rows = rows.query([])?;
    let mut rasters = Vec::new();
    while let Some(r) = rows.next()? {
        let id: u32 = r.get(0)?;
        let bad = |what: &str| corrupt(format!("the {what} of raster {id}"));
        let (face_size, base): (i64, i64) = (r.get(1)?, r.get(5)?);
        let text = |i: usize| r.get::<_, String>(i);
        let (stops, colors) = parse_bands(&text(8)?, &text(9)?);
        rasters.push(Raster {
            id: RasterId(id),
            opacity: clamp_opacity(r.get::<_, f64>(6)? as f32),
            display: RasterDisplay::from_id(&text(7)?).unwrap_or_default(),
            face_size: u32::try_from(face_size)
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| bad("face size"))?,
            unit: r.get(2)?,
            value_min: r.get(3)?,
            value_max: r.get(4)?,
            base: u16::try_from(base).map_err(|_| bad("base"))?,
            stops,
            colors,
            smoothing: Smoothing {
                on: r.get::<_, i64>(10)? != 0,
                feature_size: clamp_feature_size(r.get::<_, f64>(11)? as f32),
            },
            hillshade: Hillshade {
                on: r.get::<_, i64>(12)? != 0,
                opacity: r.get::<_, f64>(13)? as f32,
                highlights: r.get::<_, i64>(14)? != 0,
                azimuth: r.get::<_, f64>(15)? as f32,
                altitude: r.get::<_, f64>(16)? as f32,
                exaggeration: r.get::<_, f64>(17)? as f32,
                color: parse_color(&text(18)?).unwrap_or_default(),
            }
            .clamped(),
            rivers: r.get::<_, i64>(19)? != 0,
            lines: r.get::<_, i64>(20)? != 0,
        });
    }
    Ok(rasters)
}

fn load(conn: &Connection) -> Result<Project, StoreError> {
    use rusqlite::OptionalExtension;
    let (name, radius_km, units, seed) = conn
        .query_row(
            "SELECT name, radius_km, units, seed FROM vale_world",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| corrupt("no world record"))?;
    let units = Units::from_id(&units).ok_or_else(|| corrupt(format!("units {units:?}")))?;
    let world = World {
        name,
        radius_km,
        units,
        seed,
    };
    let next_id = meta(conn, "next_layer_id")?;
    let next_id = next_id
        .parse()
        .map_err(|_| corrupt(format!("next layer ID {next_id:?}")))?;
    let next_raster_id = meta(conn, "next_raster_id")?;
    let next_raster_id = next_raster_id
        .parse()
        .map_err(|_| corrupt(format!("next raster ID {next_raster_id:?}")))?;

    let rasters = load_rasters(conn)?;
    let mut layers = Vec::new();
    let mut rows = conn.prepare(
        "SELECT id, name, kind, table_name, source, visible, raster_id, style FROM vale_layer
         ORDER BY position",
    )?;
    let mut rows = rows.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(1)?;
        let kind: String = r.get(2)?;
        let raster: Option<u32> = r.get(6)?;
        let geometry = GeometryKind::from_name(&kind);
        let kind = match (kind.as_str(), geometry, raster) {
            (_, Some(geometry), None) => LayerKind::Vector(geometry),
            ("raster", _, Some(id)) => {
                let id = RasterId(id);
                let known = rasters.iter().any(|x| x.id == id);
                let twice = layers.iter().any(|l: &Layer| l.raster() == Some(id));
                if !known || twice {
                    return Err(corrupt(format!("the raster of layer {name:?}")));
                }
                LayerKind::Raster(id)
            }
            _ => return Err(corrupt(format!("layer kind {kind:?}"))),
        };
        let table: String = r.get(3)?;
        let mut layer = Layer {
            id: LayerId(r.get(0)?),
            name,
            kind,
            visible: r.get(5)?,
            source: r.get(4)?,
            fields: Vec::new(),
            features: Vec::new(),
            style: r.get(7)?,
        };
        if let LayerKind::Vector(geometry) = kind {
            load_features(conn, &mut layer, geometry, &table)?;
        }
        layers.push(layer);
    }
    let alone = |r: &Raster| !layers.iter().any(|l| l.raster() == Some(r.id));
    if let Some(raster) = rasters.iter().find(|r| alone(r)) {
        return Err(corrupt(format!("raster {} has no layer", raster.id.0)));
    }
    Ok(Project {
        world,
        layers,
        next_id,
        rasters,
        next_raster_id,
    })
}

fn load_features(
    conn: &Connection,
    layer: &mut Layer,
    kind: GeometryKind,
    table: &str,
) -> Result<(), StoreError> {
    let mut select = "SELECT geom, uuid".to_string();
    let mut fields = conn.prepare(
        "SELECT name, column_name, kind, default_value FROM vale_field WHERE layer_id = ?1
         ORDER BY position",
    )?;
    let mut fields = fields.query([layer.id.0])?;
    while let Some(r) = fields.next()? {
        let name: String = r.get(0)?;
        let column: String = r.get(1)?;
        let kind: String = r.get(2)?;
        let kind =
            FieldType::from_id(&kind).ok_or_else(|| corrupt(format!("field kind {kind:?}")))?;
        let default = read_value(kind, r.get_ref(3)?)
            .ok_or_else(|| corrupt(format!("the default value of field {name:?}")))?;
        layer.fields.push(Field {
            name,
            kind,
            default,
        });
        select += &format!(", {}", quote(&column));
    }
    select += &format!(" FROM {} ORDER BY fid", quote(table));

    let mut rows = conn.prepare(&select)?;
    let mut rows = rows.query([])?;
    while let Some(r) = rows.next()? {
        let geometry = decode_geometry(r.get_ref(0)?.as_blob().unwrap_or(&[]), kind)
            .ok_or_else(|| corrupt(format!("a geometry of layer {:?}", layer.name)))?;
        let uuid: String = r.get(1)?;
        let uuid = Uuid::parse_str(&uuid).map_err(|_| corrupt(format!("UUID {uuid:?}")))?;
        let mut attributes = BTreeMap::new();
        for (i, field) in layer.fields.iter().enumerate() {
            let value = read_value(field.kind, r.get_ref(i + 2)?)
                .ok_or_else(|| corrupt(format!("a value of field {:?}", field.name)))?;
            attributes.insert(field.name.clone(), value);
        }
        layer
            .features
            .push(Feature::with_uuid(uuid, geometry, attributes));
    }
    Ok(())
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_le_bytes());
}

/// Byte order, the type, and the part count of one WKB geometry.
fn put_head(out: &mut Vec<u8>, wkb_type: u32, count: usize) {
    out.push(1);
    out.extend_from_slice(&wkb_type.to_le_bytes());
    put_u32(out, count);
}

fn put_positions(out: &mut Vec<u8>, positions: &[LonLat]) {
    for p in positions {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
    }
}

/// A GeoPackage geometry: a header with no envelope, then little-endian WKB.
pub(crate) fn encode_geometry(geometry: &Geometry) -> Vec<u8> {
    let empty = match geometry {
        Geometry::Points(p) => p.is_empty(),
        Geometry::Lines(l) => l.is_empty(),
        Geometry::Polygons(p) => p.is_empty(),
    };
    let flags = if empty { 0x11 } else { 0x01 };
    let mut out = vec![b'G', b'P', 0, flags];
    out.extend_from_slice(&WORLD_SRS.to_le_bytes());
    match geometry {
        Geometry::Points(points) => {
            put_head(&mut out, 4, points.len());
            for p in points {
                out.push(1);
                out.extend_from_slice(&1u32.to_le_bytes());
                put_positions(&mut out, &[*p]);
            }
        }
        Geometry::Lines(lines) => {
            put_head(&mut out, 5, lines.len());
            for line in lines {
                put_head(&mut out, 2, line.len());
                put_positions(&mut out, line);
            }
        }
        Geometry::Polygons(polygons) => {
            put_head(&mut out, 6, polygons.len());
            for rings in polygons {
                put_head(&mut out, 3, rings.len());
                for ring in rings {
                    put_u32(&mut out, ring.len());
                    put_positions(&mut out, ring);
                }
            }
        }
    }
    out
}

/// Reads little-endian values from a blob.
pub(crate) struct Reader<'a>(pub(crate) &'a [u8]);

impl Reader<'_> {
    pub(crate) fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    pub(crate) fn f64(&mut self) -> Option<f64> {
        self.take().map(f64::from_le_bytes)
    }

    /// The part count of a geometry of this type.
    fn head(&mut self, wkb_type: u32) -> Option<usize> {
        (self.take::<1>()? == [1] && self.u32()? == wkb_type).then_some(())?;
        self.count()
    }

    /// A count that the rest of the blob can hold.
    fn count(&mut self) -> Option<usize> {
        let n = self.u32()? as usize;
        (n <= self.0.len()).then_some(n)
    }

    fn positions(&mut self, n: usize) -> Option<Vec<LonLat>> {
        (0..n).map(|_| Some([self.f64()?, self.f64()?])).collect()
    }
}

/// `None` for a blob that is not a little-endian XY geometry of this kind.
pub(crate) fn decode_geometry(blob: &[u8], kind: GeometryKind) -> Option<Geometry> {
    let mut r = Reader(blob);
    let [g, p, _version, flags] = r.take()?;
    (g == b'G' && p == b'P' && flags & 0x01 == 1).then_some(())?;
    let _srs = r.u32()?;
    let envelope = match (flags >> 1) & 0x07 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return None,
    };
    r.0 = r.0.get(envelope..)?;
    let geometry = match kind {
        GeometryKind::Point => {
            let n = r.head(4)?;
            let points = (0..n).map(|_| {
                (r.take::<1>()? == [1] && r.u32()? == 1).then_some(())?;
                Some([r.f64()?, r.f64()?])
            });
            Geometry::Points(points.collect::<Option<_>>()?)
        }
        GeometryKind::Line => {
            let n = r.head(5)?;
            let lines = (0..n).map(|_| {
                let n = r.head(2)?;
                r.positions(n)
            });
            Geometry::Lines(lines.collect::<Option<_>>()?)
        }
        GeometryKind::Polygon => {
            let n = r.head(6)?;
            let polygons = (0..n).map(|_| {
                let rings = r.head(3)?;
                (0..rings)
                    .map(|_| {
                        let n = r.count()?;
                        r.positions(n)
                    })
                    .collect::<Option<Vec<_>>>()
            });
            Geometry::Polygons(polygons.collect::<Option<_>>()?)
        }
    };
    r.0.is_empty().then_some(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    /// A project that uses each geometry kind, each value type, and names
    /// that need care in SQL.
    fn sample() -> Project {
        let mut p = Project::new(World {
            name: "Aerth \"the second\"".to_string(),
            radius_km: 4321.125,
            units: Units::Imperial,
            seed: 0xDEAD_BEEF,
        });
        p.add_layer(
            "Places".into(),
            GeometryKind::Point,
            Some("places.geojson".into()),
            vec![
                Feature::new(
                    Geometry::Points(vec![[10.5, -20.25], [179.999_999_999_9, 89.0]]),
                    attributes(&[
                        ("name", text("Oslo")),
                        ("Name", text("upper")),
                        ("fid", Value::Number(7.0)),
                        ("capital", Value::Bool(true)),
                        ("pop", Value::Number(0.1 + 0.2)),
                        ("any", Value::Number(1.0)),
                        ("", text("no name")),
                    ]),
                ),
                Feature::new(
                    Geometry::Points(vec![]),
                    attributes(&[
                        ("name", Value::Null),
                        ("capital", Value::Bool(false)),
                        ("any", text("N1")),
                        ("odd \"quote\"", Value::Number(f64::INFINITY)),
                    ]),
                ),
                Feature::new(
                    Geometry::Points(vec![[0.0, 0.0]]),
                    attributes(&[("any", Value::Bool(true)), ("nothing", Value::Null)]),
                ),
            ],
        );
        let gone = p.add_layer("Gone".into(), GeometryKind::Line, None, vec![]);
        p.add_layer(
            "Rivers & roads".into(),
            GeometryKind::Line,
            None,
            vec![
                Feature::new(
                    Geometry::Lines(vec![
                        vec![[0.0, 0.0], [1.0, 1.0]],
                        vec![[-5.0, 2.0]],
                        vec![],
                    ]),
                    attributes(&[("len", Value::Number(12.0))]),
                ),
                Feature::new(Geometry::Lines(vec![]), attributes(&[])),
            ],
        );
        p.add_layer(
            "Places".into(),
            GeometryKind::Polygon,
            None,
            vec![Feature::new(
                Geometry::Polygons(vec![
                    vec![
                        vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]],
                        vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]],
                    ],
                    vec![vec![
                        [-10.0, -10.0],
                        [-9.0, -10.0],
                        [-9.0, -9.0],
                        [-10.0, -10.0],
                    ]],
                ]),
                attributes(&[]),
            )],
        );
        p.add_layer("vale_layer".into(), GeometryKind::Polygon, None, vec![]);
        p.add_layer("".into(), GeometryKind::Point, None, vec![]);
        p.remove_layer(gone);
        p
    }

    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join("world.gpkg")
    }

    #[test]
    fn a_project_is_the_same_after_a_close_and_an_open() {
        let dir = tempfile::tempdir().unwrap();
        let project = sample();
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);

        // A layer that is added after the load gets an ID that was never used.
        let mut next = loaded.clone();
        let id = next.add_layer("New".into(), GeometryKind::Line, None, vec![]);
        assert!(project.layers().iter().all(|l| l.id < id));
        assert_eq!(id.0, project.next_id);

        file.save(&next).unwrap();
        drop(file);
        assert_eq!(ProjectFile::open(&path(&dir)).unwrap().1, next);
    }

    #[test]
    fn create_replaces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        drop(ProjectFile::create(&path(&dir), &sample()).unwrap());
        let empty = Project::new(World::default());
        drop(ProjectFile::create(&path(&dir), &empty).unwrap());
        assert_eq!(ProjectFile::open(&path(&dir)).unwrap().1, empty);
    }

    fn strings(conn: &Connection, sql: &str) -> Vec<String> {
        conn.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The parts of the GeoPackage standard that QGIS reads to list and open
    /// the feature tables.
    #[test]
    fn the_file_is_a_geopackage_with_the_feature_tables_only() {
        let dir = tempfile::tempdir().unwrap();
        let project = sample();
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();

        let pragma = |name: &str| -> i64 {
            conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(pragma("application_id"), 0x4750_4B47);
        assert_eq!(pragma("user_version"), 10300);
        assert_eq!(
            strings(
                &conn,
                "SELECT CAST(srs_id AS TEXT) FROM gpkg_spatial_ref_sys ORDER BY srs_id"
            ),
            ["-1", "0", "4326", "100000"]
        );
        let wkt: String = conn
            .query_row(
                "SELECT definition FROM gpkg_spatial_ref_sys WHERE srs_id = 100000",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            wkt.contains("SPHEROID[\"Aerth 'the second'\",4321125,0]"),
            "{wkt}"
        );

        let tables = [
            "Places",
            "Rivers___roads",
            "Places_3",
            "layer_vale_layer",
            "layer_",
        ];
        assert_eq!(
            strings(&conn, "SELECT table_name FROM vale_layer ORDER BY position"),
            tables
        );
        let mut sorted = tables.map(String::from);
        sorted.sort();
        // The contents table lists each feature table as features, and each
        // `vale_` table with a data type that GDAL does not list.
        assert_eq!(
            strings(
                &conn,
                "SELECT table_name FROM gpkg_contents
                 WHERE data_type = 'features' AND srs_id = 100000 ORDER BY table_name"
            ),
            sorted
        );
        assert_eq!(
            strings(
                &conn,
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT GLOB 'gpkg_*'
                   AND name NOT GLOB 'sqlite_*'
                   AND name NOT IN (SELECT table_name FROM gpkg_contents)"
            ),
            [] as [&str; 0]
        );
        assert_eq!(
            strings(
                &conn,
                "SELECT c.table_name FROM gpkg_contents c JOIN gpkg_extensions e USING (table_name)
                 WHERE c.data_type = 'vale' AND e.extension_name = 'vale_project'
                 ORDER BY c.table_name"
            ),
            [
                "vale_field",
                "vale_layer",
                "vale_meta",
                "vale_raster",
                "vale_raster_tile",
                "vale_thumbnail",
                "vale_undo_record",
                "vale_undo_step",
                "vale_undo_tile",
                "vale_world"
            ]
        );
        assert_eq!(
            strings(
                &conn,
                "SELECT table_name || ' ' || column_name || ' ' || geometry_type_name
                   || ' ' || srs_id || ' ' || z || ' ' || m
                 FROM gpkg_geometry_columns ORDER BY table_name"
            ),
            [
                "Places geom MULTIPOINT 100000 0 0",
                "Places_3 geom MULTIPOLYGON 100000 0 0",
                "Rivers___roads geom MULTILINESTRING 100000 0 0",
                "layer_ geom MULTIPOINT 100000 0 0",
                "layer_vale_layer geom MULTIPOLYGON 100000 0 0",
            ]
        );
        // The row ID is an integer primary key, and each column has a type.
        assert_eq!(
            strings(
                &conn,
                "SELECT name || ' ' || type || ' ' || pk FROM pragma_table_info('Places')"
            ),
            [
                "fid INTEGER 1",
                "geom MULTIPOINT 0",
                "uuid TEXT 0",
                "field TEXT 0",
                "Name TEXT 0",
                "any TEXT 0",
                "capital BOOLEAN 0",
                "fid_1 DOUBLE 0",
                "name_1 TEXT 0",
                "nothing TEXT 0",
                "odd \"quote\" DOUBLE 0",
                "pop DOUBLE 0",
            ]
        );
        for table in tables {
            let bad: i64 = conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM {} WHERE substr(geom, 1, 2) != CAST('GP' AS BLOB)",
                        quote(table)
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(bad, 0, "{table}");
        }
    }

    /// The schema of a layer that QGIS reads: the column types and the
    /// defaults of the fields.
    #[test]
    fn the_file_keeps_the_schema_of_a_layer_and_undo_puts_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let towns = project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        let point = Feature::new(Geometry::Points(vec![[1.0, 2.0]]), BTreeMap::new());
        assert!(project.add_feature(towns, point));
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();

        let fields = [
            Field::new("name", FieldType::Text),
            Field {
                default: Value::Number(0.0),
                ..Field::new("pop", FieldType::Integer)
            },
            Field::new("area", FieldType::Real),
            Field {
                default: Value::Bool(false),
                ..Field::new("capital", FieldType::Bool)
            },
        ];
        for field in fields {
            assert!(project.add_field(towns, field));
        }
        file.save(&project).unwrap();
        let earlier = project.clone();
        assert!(project.rename_field(towns, "pop", "population"));
        assert!(project.remove_field(towns, "area"));
        let point = Feature::new(Geometry::Points(vec![[3.0, 4.0]]), BTreeMap::new());
        assert!(project.add_feature(towns, point));
        file.save(&project).unwrap();
        drop(file);

        let conn = Connection::open(path(&dir)).unwrap();
        assert_eq!(
            strings(
                &conn,
                "SELECT name || ' ' || type || ' ' || coalesce(dflt_value, 'none')
                 FROM pragma_table_info('Towns')"
            ),
            [
                "fid INTEGER none",
                "geom MULTIPOINT none",
                "uuid TEXT none",
                "name TEXT none",
                "population INTEGER 0",
                "capital BOOLEAN 0",
            ]
        );
        assert_eq!(
            strings(
                &conn,
                "SELECT table_name FROM gpkg_contents WHERE data_type = 'features'"
            ),
            ["Towns"]
        );
        assert_eq!(
            strings(
                &conn,
                "SELECT quote(population) || ' ' || quote(capital) FROM \"Towns\" ORDER BY fid"
            ),
            ["NULL NULL", "0 0"]
        );
        drop(conn);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        let layer = loaded.layer(towns).unwrap();
        assert_eq!(layer.field_names(), ["name", "population", "capital"]);
        assert_eq!(layer.field("population").unwrap().kind, FieldType::Integer);
        assert_eq!(layer.field("capital").unwrap().default, Value::Bool(false));
        assert!(file.undo().unwrap().is_some());
        assert_eq!(file.project(), &earlier);
    }

    #[test]
    fn the_file_keeps_the_style_of_a_layer_and_undo_puts_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        project.layers[0].style = Some(r#"{"color":"red"}"#.into());
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let earlier = project.clone();
        project.layers[0].style = Some(r#"{"color":"blue"}"#.into());
        file.save(&project).unwrap();
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert!(file.undo().unwrap().is_some());
        assert_eq!(file.project(), &earlier);
    }

    #[test]
    fn set_styles_writes_the_style_with_no_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let towns = project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        let roads = project.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(!file.can_undo());
        file.set_styles(&[(roads, "{}".into())]).unwrap();
        assert!(!file.can_undo());
        let style = |p: &Project, id| p.layer(id).unwrap().style.clone();
        assert_eq!(style(file.project(), roads).as_deref(), Some("{}"));
        drop(file);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert!(!file.can_undo());
        assert_eq!(style(&loaded, towns), None);
        assert_eq!(style(&loaded, roads).as_deref(), Some("{}"));
    }

    #[test]
    fn a_layer_of_version_19_has_no_style() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let towns = project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch("ALTER TABLE vale_layer DROP COLUMN style")
            .unwrap();
        drop(conn);
        set_format_version(&dir, 19);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert_eq!(loaded.layer(towns).unwrap().style, None);
    }

    #[test]
    fn a_default_value_is_a_literal_of_its_column() {
        let field = |kind, default| Field {
            default,
            ..Field::new("x", kind)
        };
        let clause = |kind, value| default_clause(&field(kind, value));
        assert_eq!(clause(FieldType::Text, text("it's")), " DEFAULT 'it''s'");
        assert_eq!(clause(FieldType::Text, Value::Null), "");
        assert_eq!(
            clause(FieldType::Integer, Value::Number(-12.0)),
            " DEFAULT -12"
        );
        assert_eq!(clause(FieldType::Real, Value::Number(2.5)), " DEFAULT 2.5");
        let inf = Value::Number(f64::INFINITY);
        assert_eq!(clause(FieldType::Real, inf), "");
        assert_eq!(clause(FieldType::Bool, Value::Bool(true)), " DEFAULT 1");
    }

    #[test]
    fn a_mixed_column_of_version_18_becomes_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let point = |value: &str| {
            Feature::new(
                Geometry::Points(vec![]),
                attributes(&[("any", text(value)), ("n", Value::Number(1.0))]),
            )
        };
        let places = project.add_layer(
            "Places".into(),
            GeometryKind::Point,
            None,
            vec![point("B1"), point("N2.5"), point("Tx"), point("Zq")],
        );
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(
            "ALTER TABLE vale_field DROP COLUMN default_value;
             UPDATE vale_field SET kind = 'mixed' WHERE name = 'any';
             UPDATE \"Places\" SET \"any\" = NULL WHERE fid = 4;",
        )
        .unwrap();
        drop(conn);
        set_format_version(&dir, 18);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let layer = loaded.layer(places).unwrap();
        assert_eq!(layer.field("any").unwrap().kind, FieldType::Text);
        assert_eq!(layer.field("n").unwrap().kind, FieldType::Real);
        let any: Vec<&Value> = layer
            .features
            .iter()
            .map(|f| &f.attributes["any"])
            .collect();
        assert_eq!(any, [&text("true"), &text("2.5"), &text("x"), &Value::Null]);
        assert!(layer.fields.iter().all(|f| f.default == Value::Null));
    }

    #[test]
    fn a_uuid_stays_the_same_after_an_edit_and_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = sample();
        let before: Vec<Uuid> = project.layers[0]
            .features
            .iter()
            .map(Feature::uuid)
            .collect();
        assert_eq!(before.iter().collect::<BTreeSet<_>>().len(), 3);
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();

        let features = &mut project.layers[0].features;
        features[2].geometry = Geometry::Points(vec![[50.0, 60.0]]);
        features[2].attributes.insert("name".into(), text("Moved"));
        features.remove(0);
        project.world.radius_km = 1000.0;
        file.save(&project).unwrap();
        drop(file);

        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        let after: Vec<Uuid> = loaded.layers[0]
            .features
            .iter()
            .map(Feature::uuid)
            .collect();
        assert_eq!(after, before[1..]);

        let conn = Connection::open(path(&dir)).unwrap();
        assert_eq!(
            strings(&conn, "SELECT uuid FROM \"Places\" ORDER BY fid"),
            [before[1].to_string(), before[2].to_string()]
        );
    }

    fn set_format_version(dir: &tempfile::TempDir, version: u32) {
        let conn = Connection::open(path(dir)).unwrap();
        conn.execute(
            "UPDATE vale_meta SET value = ?1 WHERE key = 'format_version'",
            [version.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn the_file_has_a_format_version() {
        let dir = tempfile::tempdir().unwrap();
        drop(ProjectFile::create(&path(&dir), &sample()).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        assert_eq!(format_version(&conn).unwrap(), FORMAT_VERSION);
        assert_eq!(MIGRATIONS.len(), FORMAT_VERSION as usize);
        drop(conn);

        set_format_version(&dir, FORMAT_VERSION + 1);
        match ProjectFile::open(&path(&dir)) {
            Err(StoreError::NewerFormat { file, app }) => {
                assert_eq!((file, app), (FORMAT_VERSION + 1, FORMAT_VERSION));
            }
            other => panic!("{:?}", other.map(|x| x.1)),
        }
    }

    /// A file of an older version runs each later step, in order, in one
    /// transaction, and then has the new version.
    #[test]
    fn an_older_file_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(path(&dir)).unwrap();
        migrate(&mut conn, 0).unwrap();
        assert_eq!(format_version(&conn).unwrap(), FORMAT_VERSION);
        // No step runs again on a current file.
        migrate(&mut conn, FORMAT_VERSION).unwrap();
        assert_eq!(format_version(&conn).unwrap(), FORMAT_VERSION);
        // A step that fails leaves the file as it was.
        assert!(migrate(&mut conn, 0).is_err());
        assert_eq!(format_version(&conn).unwrap(), FORMAT_VERSION);
    }

    #[test]
    fn other_files_are_not_projects() {
        let dir = tempfile::tempdir().unwrap();
        let not = |r: Result<(ProjectFile, Project), StoreError>| {
            assert!(matches!(r, Err(StoreError::NotAProject)));
        };

        std::fs::write(path(&dir), "this is some text and not a database").unwrap();
        not(ProjectFile::open(&path(&dir)));

        std::fs::remove_file(path(&dir)).unwrap();
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
        drop(conn);
        not(ProjectFile::open(&path(&dir)));

        std::fs::remove_file(path(&dir)).unwrap();
        assert!(matches!(
            ProjectFile::open(&path(&dir)),
            Err(StoreError::Sqlite(_))
        ));
        assert!(!path(&dir).exists());
    }

    #[test]
    fn a_damaged_geometry_is_an_error() {
        let good = encode_geometry(&Geometry::Lines(vec![vec![[1.0, 2.0], [3.0, 4.0]]]));
        assert!(decode_geometry(&good, GeometryKind::Line).is_some());
        assert!(decode_geometry(&good, GeometryKind::Point).is_none());
        assert!(decode_geometry(&good[..good.len() - 1], GeometryKind::Line).is_none());
        let mut long = good.clone();
        long.push(0);
        assert!(decode_geometry(&long, GeometryKind::Line).is_none());
        // A part count that is larger than the blob does not allocate.
        let mut huge = good.clone();
        huge[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_geometry(&huge, GeometryKind::Line).is_none());

        // An envelope from another writer is skipped.
        let mut envelope = good[..8].to_vec();
        envelope[3] = 0x03;
        envelope.extend_from_slice(&[0; 32]);
        envelope.extend_from_slice(&good[8..]);
        assert_eq!(
            decode_geometry(&envelope, GeometryKind::Line),
            decode_geometry(&good, GeometryKind::Line)
        );
    }

    /// Ground that is different for each seed.
    fn pixels(seed: usize) -> Vec<u16> {
        (0..TILE_SIZE * TILE_SIZE)
            .map(|i| (1000 * seed + 5 * (i % TILE_SIZE) + (i / TILE_SIZE) * (seed + 1)) as u16)
            .collect()
    }

    fn key(face: u8, column: u32, row: u32) -> TileKey {
        TileKey {
            level: 0,
            face,
            column,
            row,
        }
    }

    /// A stop with no line.
    fn stop(value: f64) -> Stop {
        Stop {
            value,
            line: StopLine::default(),
        }
    }

    /// A project with a heightmap and a second raster of another face size.
    /// The two raster layers are below the vector layers.
    fn rasters() -> (Project, RasterId, RasterId) {
        let mut project = sample();
        let height = project.add_raster("Height".into(), 8192, "m".into(), -6000.0, 6000.0);
        let rain = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        *project.raster_mut(height).unwrap() = Raster {
            id: height,
            opacity: 1.0,
            display: RasterDisplay::Bands,
            face_size: 8192,
            unit: "m".into(),
            value_min: -6000.0,
            value_max: 6000.0,
            base: 19114,
            stops: vec![
                stop(-6000.0),
                Stop {
                    value: 1e-7,
                    line: StopLine {
                        on: true,
                        weight: 2.5,
                        color: [7, 8, 9],
                        darker: false,
                        darken: 0.25,
                    },
                },
                stop(1234.5678),
            ],
            colors: vec![[0, 1, 2], [250, 128, 7]],
            smoothing: Smoothing::default(),
            hillshade: Hillshade::default(),
            rivers: true,
            lines: true,
        };
        (project, height, rain)
    }

    fn put<'a>(
        raster: RasterId,
        tiles: &'a [(TileKey, Vec<u16>)],
        delete: Vec<TileKey>,
    ) -> TileChanges<'a> {
        TileChanges {
            raster,
            all: false,
            put: tiles.iter().map(|(key, p)| (*key, &p[..])).collect(),
            delete,
        }
    }

    fn tiles_of(file: &ProjectFile, raster: &Raster) -> Vec<(TileKey, Vec<u16>)> {
        let mut tiles = Vec::new();
        file.load_tiles(raster, |key, pixels| tiles.push((key, pixels)))
            .unwrap();
        tiles
    }

    #[test]
    fn two_rasters_are_the_same_after_a_close_and_an_open() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, rain) = rasters();
        // More tiles than one batch, and the same keys in both rasters.
        let high: Vec<_> = (0..300u32)
            .map(|i| {
                (
                    key((i % 6) as u8, 31 - i / 6 % 8, i / 48),
                    pixels(i as usize),
                )
            })
            .collect();
        let wet = vec![
            (key(0, 0, 0), pixels(7)),
            (key(5, 3, 3), vec![u16::MAX; 65536]),
        ];
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let changes = [put(height, &high, vec![]), put(rain, &wet, vec![])];
        let stats = file.save_tiles(&project, &changes).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (302, 0));
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(loaded.rasters().len(), 2);
        assert_eq!(loaded.raster(height).unwrap().face_size, 8192);
        assert_eq!(loaded.raster(rain).unwrap().face_size, 1000);
        let mut sorted = high.clone();
        sorted.sort_by_key(|(k, _)| (k.face, k.row, k.column));
        assert_eq!(tiles_of(&file, loaded.raster(height).unwrap()), sorted);
        assert_eq!(tiles_of(&file, loaded.raster(rain).unwrap()), wet);

        // A save of the project alone keeps the tiles.
        let mut next = loaded.clone();
        next.world.name = "Other".into();
        next.set_raster_stops(rain, vec![stop(5.0)], Vec::new());
        file.save(&next).unwrap();
        assert_eq!(tiles_of(&file, next.raster(rain).unwrap()), wet);

        // A raster that is removed loses its tiles, and the other keeps them.
        next.remove_raster(height);
        let stats = file.save_tiles(&next, &[]).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (0, 300));
        drop(file);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, next);
        assert_eq!(tiles_of(&file, loaded.raster(rain).unwrap()), wet);
        let id = next.add_raster("New".into(), 4096, String::new(), 0.0, 1.0);
        assert!(id > height && id > rain);

        // The tiles of one face size are not the tiles of another.
        next.raster_mut(rain).unwrap().face_size = 2000;
        assert_eq!(file.save_tiles(&next, &[]).unwrap().tiles_deleted, 2);
        assert_eq!(tiles_of(&file, next.raster(rain).unwrap()), []);
    }

    /// Each change of a tile row, from triggers on the tile table.
    fn log_tile_rows(conn: &Connection) {
        let trigger = |event: &str, row: &str| {
            format!(
                "CREATE TEMP TRIGGER log_{event} AFTER {event} ON main.vale_raster_tile BEGIN
                   INSERT INTO tile_log VALUES ('{event} ' || {row}.raster_id || ' '
                     || {row}.level || ' ' || {row}.face || ' ' || {row}.tile_column || ' '
                     || {row}.tile_row);
                 END;"
            )
        };
        conn.execute_batch(&format!(
            "CREATE TEMP TABLE tile_log (line TEXT); {} {} {}",
            trigger("insert", "NEW"),
            trigger("update", "NEW"),
            trigger("delete", "OLD")
        ))
        .unwrap();
    }

    fn take_log(conn: &Connection) -> Vec<String> {
        let lines = strings(conn, "SELECT line FROM tile_log ORDER BY rowid");
        conn.execute("DELETE FROM tile_log", []).unwrap();
        lines
    }

    #[test]
    fn a_save_writes_only_the_changed_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let painted: Vec<_> = (0..40u32)
            .map(|i| (key(2, i % 8, i / 8), pixels(i as usize)))
            .collect();
        let wet = vec![(key(2, 1, 0), pixels(99))];
        let changes = [put(height, &painted, vec![]), put(rain, &wet, vec![])];
        file.save_tiles(&project, &changes).unwrap();
        log_tile_rows(&file.conn);

        // One stroke changes two tiles, makes one, and takes one away.
        let stroke = vec![
            (key(2, 1, 0), pixels(200)),
            (key(2, 2, 0), pixels(201)),
            (key(3, 0, 0), pixels(202)),
        ];
        let changes = [put(height, &stroke, vec![key(2, 7, 4), key(4, 9, 9)])];
        let stats = file.save_tiles(&project, &changes).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (3, 1));
        assert_eq!(
            take_log(&file.conn),
            [
                "delete 0 0 2 7 4",
                "update 0 0 2 1 0",
                "update 0 0 2 2 0",
                "insert 0 0 3 0 0"
            ]
        );

        // A save with no changed tile writes no tile.
        assert_eq!(
            file.save_tiles(&project, &[]).unwrap(),
            SaveStats::default()
        );
        assert_eq!(take_log(&file.conn), [] as [&str; 0]);

        let mut expected: Vec<_> = painted
            .into_iter()
            .filter(|(k, _)| ![key(2, 7, 4), key(2, 1, 0), key(2, 2, 0)].contains(k))
            .chain(stroke)
            .collect();
        expected.sort_by_key(|(k, _)| (k.face, k.row, k.column));
        assert_eq!(tiles_of(&file, project.raster(height).unwrap()), expected);
        assert_eq!(tiles_of(&file, project.raster(rain).unwrap()), wet);

        // With `all`, the tiles of the raster are the tiles of the save.
        let fill = vec![(key(0, 0, 0), pixels(1))];
        let mut change = put(height, &fill, vec![]);
        change.all = true;
        let stats = file.save_tiles(&project, &[change]).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (1, 40));
        assert_eq!(tiles_of(&file, project.raster(height).unwrap()), fill);
        assert_eq!(tiles_of(&file, project.raster(rain).unwrap()), wet);
    }

    #[test]
    fn an_empty_world_gives_a_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let id = project.add_raster("Height".into(), 8192, "m".into(), -6000.0, 6000.0);
        let file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert_eq!(tiles_of(&file, project.raster(id).unwrap()), []);
        drop(file);
        let bytes = std::fs::metadata(path(&dir)).unwrap().len();
        assert!(bytes < 128 * 1024, "{bytes} bytes");

        // A tile of flat ground adds little.
        let (mut file, project) = ProjectFile::open(&path(&dir)).unwrap();
        let flat = vec![(key(0, 0, 0), vec![19114; 65536])];
        file.save_tiles(&project, &[put(id, &flat, vec![])])
            .unwrap();
        drop(file);
        let more = std::fs::metadata(path(&dir)).unwrap().len();
        assert!(more - bytes <= 8192, "{} bytes", more - bytes);
    }

    #[test]
    fn the_key_of_a_tile_has_a_level() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, _) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let tiles = vec![(key(0, 0, 0), pixels(1)), (key(5, 31, 31), pixels(2))];
        file.save_tiles(&project, &[put(height, &tiles, vec![])])
            .unwrap();
        assert_eq!(
            strings(
                &file.conn,
                "SELECT name FROM pragma_table_info('vale_raster_tile') WHERE pk > 0 ORDER BY pk"
            ),
            ["raster_id", "level", "face", "tile_column", "tile_row"]
        );
        assert_eq!(
            strings(
                &file.conn,
                "SELECT DISTINCT CAST(level AS TEXT) FROM vale_raster_tile"
            ),
            ["0"]
        );
        assert!(
            tiles_of(&file, project.raster(height).unwrap())
                .iter()
                .all(|(key, _)| key.level == 0)
        );
    }

    /// One layer of an older file: the ID, the position, the name, the kind,
    /// and the visibility.
    type OldLayer = (u32, i64, &'static str, GeometryKind, bool);

    /// One raster of an older file: the ID, the position, the name, and the
    /// visibility.
    type OldRaster = (u32, i64, &'static str, bool);

    /// Writes a file of format version 6, 7, or 8 with the steps of that
    /// version and SQL. Each layer has an empty feature table. A file of a
    /// version before 7 has no visibility of a layer, and a file of a
    /// version before 8 has no visibility of a raster.
    fn old_file(dir: &tempfile::TempDir, version: u32, layers: &[OldLayer], rasters: &[OldRaster]) {
        assert!((6..=8).contains(&version));
        let mut conn = Connection::open(path(dir)).unwrap();
        let tx = conn.transaction().unwrap();
        for step in &MIGRATIONS[..version as usize] {
            step(&tx).unwrap();
        }
        tx.execute_batch(&format!(
            "INSERT INTO vale_meta VALUES ('format_version', '{version}'), ('next_layer_id', '7');
             UPDATE vale_meta SET value = '4' WHERE key = 'next_raster_id';
             INSERT INTO vale_world (id, name, radius_km, units, seed)
               VALUES (1, 'Old', 1234.5, 'metric', 0);
             INSERT INTO gpkg_spatial_ref_sys
               VALUES ('Old', 100000, 'NONE', 100000, 'undefined', NULL);"
        ))
        .unwrap();
        for &(id, position, name, kind, visible) in layers {
            tx.execute(
                "INSERT INTO vale_layer (id, position, name, kind, table_name)
                 VALUES (?1, ?2, ?3, ?4, ?3)",
                params![id, position, name, kind.name()],
            )
            .unwrap();
            if version >= 7 {
                let hide = "UPDATE vale_layer SET visible = ?2 WHERE id = ?1";
                tx.execute(hide, params![id, visible]).unwrap();
            }
            let geometry = geometry_type(kind);
            tx.execute_batch(&format!(
                "CREATE TABLE {name} (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                   geom {geometry}, uuid TEXT NOT NULL UNIQUE);
                 INSERT INTO gpkg_contents (table_name, data_type, identifier, srs_id)
                   VALUES ('{name}', 'features', '{name}', 100000);
                 INSERT INTO gpkg_geometry_columns
                   VALUES ('{name}', 'geom', '{geometry}', 100000, 0, 0);"
            ))
            .unwrap();
        }
        for &(id, position, name, visible) in rasters {
            tx.execute(
                "INSERT INTO vale_raster (id, position, name, face_size, unit, value_min,
                   value_max, base, limits, ramp_sea, ramp_land)
                 VALUES (?1, ?2, ?3, 1000, 'mm', 0, 5000, 7, '250,1000', '000102', 'ffffff')",
                params![id, position, name],
            )
            .unwrap();
            if version >= 8 {
                let hide = "UPDATE vale_raster SET visible = ?2 WHERE id = ?1";
                tx.execute(hide, params![id, visible]).unwrap();
            }
        }
        tx.commit().unwrap();
    }

    #[test]
    fn a_file_of_version_6_gets_visible_layers_in_the_order_of_their_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let layers = [
            (0, 0, "Places", GeometryKind::Point, true),
            (1, 1, "Rivers", GeometryKind::Line, true),
            (2, 2, "Land", GeometryKind::Polygon, true),
            (3, 3, "Roads", GeometryKind::Line, true),
        ];
        old_file(&dir, 6, &layers, &[]);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let names: Vec<&str> = loaded.layers().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Land", "Rivers", "Roads", "Places"]);
        assert!(loaded.layers().iter().all(|l| l.visible));
        assert_eq!(
            strings(
                &file.conn,
                "SELECT CAST(position AS TEXT) FROM vale_layer ORDER BY position"
            ),
            ["0", "1", "2", "3"]
        );
    }

    /// A file of version 7 or 8 with three vector layers and two rasters.
    /// The order of the rasters is not the order of their IDs. Each raster
    /// has tiles, one layer has a feature, and the undo log has a step.
    /// After the migration, the raster layers are below the vector layers.
    fn old_rasters_become_layers(version: u32) {
        let dir = tempfile::tempdir().unwrap();
        let layers = [
            (0, 1, "Land", GeometryKind::Polygon, true),
            (2, 0, "Roads", GeometryKind::Line, false),
            (5, 2, "Towns", GeometryKind::Point, true),
        ];
        old_file(
            &dir,
            version,
            &layers,
            &[(0, 1, "Height", true), (3, 0, "Rain", false)],
        );
        let conn = Connection::open(path(&dir)).unwrap();
        let (height, rain) = (RasterId(0), RasterId(3));
        let high = vec![(key(0, 0, 0), pixels(1)), (key(2, 3, 3), pixels(2))];
        let wet = vec![(key(5, 3, 3), pixels(3))];
        for (raster, tiles) in [(height, &high), (rain, &wet)] {
            for (key, pixels) in tiles {
                conn.execute(
                    "INSERT INTO vale_raster_tile VALUES (?1, 0, ?2, ?3, ?4, ?5)",
                    params![
                        raster.0,
                        key.face,
                        key.column,
                        key.row,
                        tile::encode(pixels)
                    ],
                )
                .unwrap();
            }
        }
        let town = Feature::new(Geometry::Points(vec![[3.0, 4.0]]), attributes(&[]));
        conn.execute(
            "INSERT INTO Towns (geom, uuid) VALUES (?1, ?2)",
            params![encode_geometry(&town.geometry), town.uuid().to_string()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO vale_undo_step (id, redo, bytes) VALUES (1, 0, 2);
             INSERT INTO vale_undo_record VALUES (1, 'raster', 3, x'0102');
             INSERT INTO vale_undo_tile VALUES (1, 3, 0, 5, 3, 3, NULL);",
        )
        .unwrap();
        drop(conn);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let hidden = version >= 8;
        let rows = loaded.layers().iter();
        let rows: Vec<_> = rows
            .map(|l| (l.id.0, l.name.as_str(), l.kind, l.visible))
            .collect();
        assert_eq!(
            rows,
            [
                (7, "Rain", LayerKind::Raster(rain), !hidden),
                (8, "Height", LayerKind::Raster(height), true),
                (2, "Roads", LayerKind::Vector(GeometryKind::Line), false),
                (0, "Land", LayerKind::Vector(GeometryKind::Polygon), true),
                (5, "Towns", LayerKind::Vector(GeometryKind::Point), true),
            ]
        );
        assert_eq!(
            strings(
                &file.conn,
                "SELECT position || ' ' || table_name FROM vale_layer ORDER BY position"
            ),
            [
                "0 vale_raster_3",
                "1 vale_raster_0",
                "2 Roads",
                "3 Land",
                "4 Towns"
            ]
        );
        assert_eq!(meta(&file.conn, "next_layer_id").unwrap(), "9");
        assert_eq!(loaded.layers()[4].features, [town]);

        // The rasters keep their values and their tiles.
        let ids: Vec<RasterId> = loaded.rasters().iter().map(|r| r.id).collect();
        assert_eq!(ids, [height, rain]);
        for raster in loaded.rasters() {
            assert_eq!((raster.face_size, raster.unit.as_str()), (1000, "mm"));
            assert_eq!((raster.value_min, raster.value_max), (0.0, 5000.0));
            assert_eq!((raster.base, raster.opacity), (7, 1.0));
            assert_eq!(raster.display, RasterDisplay::Bands);
            let values: Vec<f64> = raster.stops.iter().map(|s| s.value).collect();
            assert_eq!(values, [0.0, 250.0, 1000.0, 5000.0]);
            assert_eq!(raster.colors, [[255; 3]; 3]);
        }
        assert_eq!(tiles_of(&file, loaded.raster(height).unwrap()), high);
        assert_eq!(tiles_of(&file, loaded.raster(rain).unwrap()), wet);
        assert_eq!(
            strings(
                &file.conn,
                "SELECT name FROM pragma_table_info('vale_raster')"
            ),
            [
                "id",
                "face_size",
                "unit",
                "value_min",
                "value_max",
                "base",
                "opacity",
                "display",
                "stops",
                "smooth",
                "feature_size",
                "colors",
                "hillshade",
                "hillshade_opacity",
                "hillshade_highlights",
                "hillshade_azimuth",
                "hillshade_altitude",
                "hillshade_exaggeration",
                "hillshade_color",
                "rivers",
                "lines"
            ]
        );

        // The undo log is empty.
        assert!(!file.can_undo() && !file.can_redo());
        assert_eq!((steps(&file), file.undo_bytes()), (0, 0));
        for table in ["vale_undo_record", "vale_undo_tile"] {
            let count = format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table}");
            assert_eq!(strings(&file.conn, &count), ["0"]);
        }

        // A new layer gets an ID that no layer has, and the file keeps an edit.
        let mut next = loaded.clone();
        let id = next.add_layer("New".into(), GeometryKind::Line, None, vec![]);
        assert_eq!(id, LayerId(9));
        next.move_layer(next.raster_layer(rain).unwrap().id, 3);
        file.save(&next).unwrap();
        drop(file);
        let (mut file, again) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(again, next);
        assert_eq!(tiles_of(&file, again.raster(rain).unwrap()), wet);
        file.undo().unwrap();
        assert_eq!(file.project().layers(), loaded.layers());
    }

    #[test]
    fn a_file_of_version_8_gets_raster_layers_below_the_vector_layers() {
        old_rasters_become_layers(8);
    }

    #[test]
    fn a_file_of_version_7_gets_raster_layers_below_the_vector_layers() {
        old_rasters_become_layers(7);
    }

    #[test]
    fn the_file_keeps_the_values_of_two_rasters() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let height = project.add_raster("Height".into(), 8192, "m".into(), -6000.0, 6000.0);
        let rain = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        let layer = |p: &Project, id| p.raster_layer(id).unwrap().id;
        project.set_layer_visible(layer(&project, rain), false);
        drop(ProjectFile::create(&path(&dir), &project).unwrap());

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        let values = |p: &Project, id| {
            let r = p.raster(id).unwrap();
            let layer = p.raster_layer(id).unwrap();
            (
                layer.name.clone(),
                layer.visible,
                r.unit.clone(),
                r.value_min,
                r.value_max,
                r.face_size,
            )
        };
        assert_eq!(
            values(&loaded, height),
            ("Height".into(), true, "m".into(), -6000.0, 6000.0, 8192)
        );
        assert_eq!(
            values(&loaded, rain),
            ("Rain".into(), false, "mm".into(), 0.0, 5000.0, 1000)
        );

        // A save keeps a change of each value.
        project.set_layer_visible(layer(&project, height), false);
        project.set_layer_visible(layer(&project, rain), true);
        project.rename_layer(layer(&project, rain), "Rainfall".into());
        project.raster_mut(rain).unwrap().value_max = 9000.0;
        file.save(&project).unwrap();
        drop(file);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(
            values(&loaded, height),
            ("Height".into(), false, "m".into(), -6000.0, 6000.0, 8192)
        );
        assert_eq!(
            values(&loaded, rain),
            ("Rainfall".into(), true, "mm".into(), 0.0, 9000.0, 1000)
        );
    }

    fn names(project: &Project) -> Vec<&str> {
        project.layers().iter().map(|l| l.name.as_str()).collect()
    }

    /// The kind and the ID of each record image of each step, in the order
    /// of the steps.
    fn images(file: &ProjectFile) -> Vec<String> {
        let sql = "SELECT group_concat(kind || ' ' || id, ', ') FROM
             (SELECT * FROM vale_undo_record ORDER BY step_id, kind, id) GROUP BY step_id";
        strings(&file.conn, sql)
    }

    #[test]
    fn an_undo_puts_back_the_order_of_a_raster_layer_and_a_vector_layer() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let land = project.add_layer("Land".into(), GeometryKind::Polygon, None, vec![]);
        let roads = project.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        let height = project.add_raster("Height".into(), 8192, "m".into(), -6000.0, 6000.0);
        let rain = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        let rain_layer = project.raster_layer(rain).unwrap().id;
        let wet = vec![(key(5, 3, 3), pixels(3))];
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        file.save_tiles(&project, &[put(rain, &wet, vec![])])
            .unwrap();
        file.save_base(&project).unwrap();
        let mut states = vec![content(&file)];
        assert_eq!(names(&project), ["Height", "Rain", "Land", "Roads"]);

        // The raster layer goes above a vector layer.
        project.move_layer(rain_layer, 2);
        file.save(&project).unwrap();
        states.push(content(&file));
        project.set_layer_visible(rain_layer, false);
        file.save(&project).unwrap();
        states.push(content(&file));
        // A vector layer goes below each raster layer.
        project.move_layer(roads, 0);
        file.save(&project).unwrap();
        states.push(content(&file));
        assert_eq!(names(&project), ["Roads", "Height", "Land", "Rain"]);
        // A change of the order or of the visibility keeps layer images only.
        assert_eq!(
            images(&file),
            [
                format!("layer {}, layer {}", land.0, rain_layer.0),
                format!("layer {}", rain_layer.0),
                "layer 0, layer 1, layer 2, layer 3".to_string(),
            ]
        );
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(content(&file), states[3]);
        assert_eq!(file.undo().unwrap().unwrap().tiles, []);
        assert_eq!(content(&file), states[2]);
        assert_eq!(names(file.project()), ["Height", "Land", "Rain", "Roads"]);
        assert!(!file.project().raster_layer(rain).unwrap().visible);
        file.undo().unwrap();
        assert_eq!(content(&file), states[1]);
        assert!(file.project().raster_layer(rain).unwrap().visible);
        file.undo().unwrap();
        assert_eq!(content(&file), states[0]);
        assert_eq!(names(file.project()), ["Height", "Rain", "Land", "Roads"]);
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(names(&loaded), ["Height", "Rain", "Land", "Roads"]);
        for state in &states[1..] {
            file.redo().unwrap();
            assert_eq!(&content(&file), state);
        }
        assert_eq!(names(file.project()), ["Roads", "Height", "Land", "Rain"]);
        assert_eq!(
            file.project().raster_layer(height).unwrap().kind,
            LayerKind::Raster(height)
        );
        assert_eq!(tiles_of(&file, file.project().raster(rain).unwrap()), wet);
    }

    #[test]
    fn an_undo_puts_back_a_removed_raster_layer_with_its_record_and_its_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let height = project.add_raster("Height".into(), 8192, "m".into(), -6000.0, 6000.0);
        project.add_layer("Land".into(), GeometryKind::Polygon, None, vec![]);
        let roads = project.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let empty = content(&file);
        const TILES: &str = "SELECT CAST(raster_id AS TEXT) FROM vale_raster_tile ORDER BY 1";
        const RECORDS: &str = "SELECT CAST(id AS TEXT) FROM vale_raster ORDER BY 1";
        const LAYERS: &str = "SELECT name || ' ' || COALESCE(raster_id, '-') FROM vale_layer
             ORDER BY position";

        // The step of a new raster layer holds the layer and the raster.
        let rain = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        let rain_layer = project.raster_layer(rain).unwrap().id;
        project.set_raster_stops(rain, vec![stop(250.0)], Vec::new());
        project.move_layer(rain_layer, 2);
        project.set_layer_visible(rain_layer, false);
        file.save(&project).unwrap();
        // The layer above the place of the raster layer has a new place.
        let step = format!(
            "layer {}, layer {}, raster {}",
            roads.0, rain_layer.0, rain.0
        );
        assert_eq!(images(&file), [step.as_str()]);
        let high = vec![(key(0, 0, 0), pixels(1))];
        let wet = vec![(key(5, 3, 3), pixels(3)), (key(2, 3, 3), pixels(4))];
        let changes = [put(height, &high, vec![]), put(rain, &wet, vec![])];
        file.save_tiles(&project, &changes).unwrap();
        let before = content(&file);
        assert_eq!(names(&project), ["Height", "Land", "Rain", "Roads"]);

        // The step of a removed raster layer holds the layer, the raster,
        // and the tiles.
        assert_eq!(
            project.remove_layer(rain_layer).unwrap().raster(),
            Some(rain)
        );
        let stats = file.save_tiles(&project, &[]).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (0, 2));
        // The step of the tiles has no record image.
        assert_eq!(images(&file), [step.as_str(); 2]);
        assert_eq!(steps(&file), 3);
        assert_eq!(strings(&file.conn, TILES), ["0"]);
        assert_eq!(strings(&file.conn, RECORDS), ["0"]);
        assert_eq!(
            strings(&file.conn, LAYERS),
            ["Height 0", "Land -", "Roads -"]
        );
        let after = content(&file);
        drop(file);

        let (mut file, _) = ProjectFile::open(&path(&dir)).unwrap();
        let mut restored = file.undo().unwrap().unwrap().tiles;
        restored.sort();
        assert_eq!(restored, [(rain, key(2, 3, 3)), (rain, key(5, 3, 3))]);
        assert_eq!(content(&file), before);
        let project = file.project();
        assert_eq!(names(project), ["Height", "Land", "Rain", "Roads"]);
        let layer = project.raster_layer(rain).unwrap();
        assert_eq!((layer.id, layer.visible), (rain_layer, false));
        assert_eq!(project.raster(rain).unwrap().stops, [stop(250.0)]);
        let mut sorted = wet.clone();
        sorted.sort_by_key(|(k, _)| (k.face, k.row, k.column));
        assert_eq!(tiles_of(&file, project.raster(rain).unwrap()), sorted);
        assert_eq!(strings(&file.conn, TILES), ["0", "1", "1"]);
        assert_eq!(strings(&file.conn, RECORDS), ["0", "1"]);
        assert_eq!(
            strings(&file.conn, LAYERS),
            ["Height 0", "Land -", "Rain 1", "Roads -"]
        );
        drop(file);

        // The file holds the layer after the undo, and a redo removes it.
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(names(&loaded), ["Height", "Land", "Rain", "Roads"]);
        file.redo().unwrap();
        assert_eq!(content(&file), after);
        assert!(file.project().raster(rain).is_none());
        assert!(file.project().layer(rain_layer).is_none());
        assert_eq!(strings(&file.conn, TILES), ["0"]);
        assert_eq!(strings(&file.conn, RECORDS), ["0"]);
        assert_eq!(
            strings(&file.conn, LAYERS),
            ["Height 0", "Land -", "Roads -"]
        );

        // An undo of each step takes away the new raster layer too.
        for _ in 0..3 {
            file.undo().unwrap();
        }
        assert_eq!(content(&file), empty);
        assert!(!file.can_undo());
        assert_eq!(strings(&file.conn, RECORDS), ["0"]);
        assert_eq!(
            strings(&file.conn, LAYERS),
            ["Height 0", "Land -", "Roads -"]
        );
    }

    #[test]
    fn the_file_keeps_the_opacity() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_opacity(rain, 0.25));
        file.save(&project).unwrap();
        // An opacity change keeps the image of the raster only.
        assert_eq!(images(&file), [format!("raster {}", rain.0)]);
        drop(file);

        let opacity = |p: &Project, id| p.raster(id).unwrap().opacity;
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(
            (opacity(&loaded, height), opacity(&loaded, rain)),
            (1.0, 0.25)
        );
        file.undo().unwrap();
        assert_eq!(opacity(file.project(), rain), 1.0);
        drop(file);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(opacity(&loaded, rain), 1.0);
        file.redo().unwrap();
        assert_eq!(file.project(), &project);
        drop(file);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(opacity(&loaded, rain), 0.25);

        // An opacity outside the range gets the nearest limit in the file
        // and after a load.
        file.conn
            .execute("UPDATE vale_raster SET opacity = 7 WHERE id = ?1", [rain.0])
            .unwrap();
        drop(file);
        let (mut file, mut loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(opacity(&loaded, rain), 1.0);
        loaded.raster_mut(height).unwrap().opacity = -3.0;
        file.save(&loaded).unwrap();
        assert_eq!(
            strings(
                &file.conn,
                "SELECT CAST(opacity AS TEXT) FROM vale_raster ORDER BY id"
            ),
            ["0.0", "1.0"]
        );
    }

    #[test]
    fn the_file_keeps_the_display_and_the_unit() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_display(rain, RasterDisplay::Grey));
        assert!(project.set_raster_unit(rain, "cm"));
        file.save(&project).unwrap();
        assert_eq!(images(&file), [format!("raster {}", rain.0)]);
        drop(file);

        let shown = |p: &Project, id| {
            let r = p.raster(id).unwrap();
            (r.display, r.unit.clone())
        };
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(shown(&loaded, height), (RasterDisplay::Bands, "m".into()));
        assert_eq!(shown(&loaded, rain), (RasterDisplay::Grey, "cm".into()));
        file.undo().unwrap();
        assert_eq!(
            shown(file.project(), rain),
            (RasterDisplay::Bands, "mm".into())
        );
        drop(file);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(shown(&loaded, rain), (RasterDisplay::Bands, "mm".into()));
        file.redo().unwrap();
        assert_eq!(file.project(), &project);
        drop(file);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(shown(&loaded, rain), (RasterDisplay::Grey, "cm".into()));

        // An unknown display shows bands.
        file.conn
            .execute(
                "UPDATE vale_raster SET display = 'sparkles' WHERE id = ?1",
                [rain.0],
            )
            .unwrap();
        drop(file);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded.raster(rain).unwrap().display, RasterDisplay::Bands);
    }

    /// Removes the switch of the lines of the rasters, as in a file before
    /// format version 18.
    const V17_DROP: &str = "ALTER TABLE vale_raster DROP COLUMN lines;";

    #[test]
    fn the_file_keeps_the_switch_of_the_lines_of_each_raster() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_lines(height, false));
        file.save(&project).unwrap();
        drop(file);

        let lines = |p: &Project, id| p.raster(id).unwrap().lines;
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert!(!lines(&loaded, height));
        assert!(lines(&loaded, rain));
        file.undo().unwrap();
        assert!(lines(file.project(), height));
        file.redo().unwrap();
        assert!(!lines(file.project(), height));
        drop(file);

        // A file before version 18 shows the lines of each raster.
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(V17_DROP).unwrap();
        drop(conn);
        set_format_version(&dir, 17);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert!(loaded.rasters().iter().all(|r| r.lines));
        file.undo().unwrap();
        assert!(lines(file.project(), height));
    }

    /// Removes the switches of the rivers and the lines of the rasters, as in
    /// a file before format version 17.
    const V16_DROP: &str = "ALTER TABLE vale_raster DROP COLUMN lines;
         ALTER TABLE vale_raster DROP COLUMN rivers;";

    #[test]
    fn the_file_keeps_the_switch_of_the_rivers_of_each_raster() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_rivers(height, false));
        file.save(&project).unwrap();
        drop(file);

        let rivers = |p: &Project, id| p.raster(id).unwrap().rivers;
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert!(!rivers(&loaded, height));
        assert!(rivers(&loaded, rain));
        file.undo().unwrap();
        assert!(rivers(file.project(), height));
        file.redo().unwrap();
        assert!(!rivers(file.project(), height));
        drop(file);

        // A file before version 17 shows the rivers of each raster.
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(V16_DROP).unwrap();
        drop(conn);
        set_format_version(&dir, 16);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert!(loaded.rasters().iter().all(|r| r.rivers));
        file.undo().unwrap();
        assert!(rivers(file.project(), height));
    }

    /// Removes the hillshade and the switches of the rivers and the lines of
    /// the rasters, as in a file before format version 15.
    const V14_DROP: &str = "ALTER TABLE vale_raster DROP COLUMN lines;
         ALTER TABLE vale_raster DROP COLUMN rivers;
         ALTER TABLE vale_raster DROP COLUMN hillshade;
         ALTER TABLE vale_raster DROP COLUMN hillshade_opacity;
         ALTER TABLE vale_raster DROP COLUMN hillshade_highlights;
         ALTER TABLE vale_raster DROP COLUMN hillshade_azimuth;
         ALTER TABLE vale_raster DROP COLUMN hillshade_altitude;
         ALTER TABLE vale_raster DROP COLUMN hillshade_exaggeration;
         ALTER TABLE vale_raster DROP COLUMN hillshade_color;";

    #[test]
    fn the_file_keeps_the_hillshade_of_each_raster() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        assert!(project.set_raster_hillshade_opacity(height, 0.75));
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_hillshade(height, true));
        assert!(project.set_raster_hillshade_highlights(height, false));
        assert!(project.set_raster_hillshade_azimuth(height, 90.0));
        assert!(project.set_raster_hillshade_altitude(height, 30.0));
        assert!(project.set_raster_hillshade_exaggeration(height, 2.5));
        assert!(project.set_raster_hillshade_color(height, [40, 20, 80]));
        file.save(&project).unwrap();
        assert!(project.set_raster_hillshade_opacity(rain, 0.25));
        file.save(&project).unwrap();
        drop(file);

        let hillshade = |p: &Project, id| p.raster(id).unwrap().hillshade;
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        let on = Hillshade {
            on: true,
            opacity: 0.75,
            highlights: false,
            azimuth: 90.0,
            altitude: 30.0,
            exaggeration: 2.5,
            color: [40, 20, 80],
        };
        let light = Hillshade {
            opacity: 0.25,
            ..Hillshade::default()
        };
        assert_eq!(hillshade(&loaded, height), on);
        assert_eq!(hillshade(&loaded, rain), light);

        // An undo puts back the old opacity, and then the old switches and
        // the old light, relief, and color.
        let off = Hillshade {
            opacity: 0.75,
            ..Hillshade::default()
        };
        file.undo().unwrap();
        assert_eq!(hillshade(file.project(), rain), Hillshade::default());
        assert_eq!(hillshade(file.project(), height), on);
        file.undo().unwrap();
        assert_eq!(hillshade(file.project(), height), off);
        drop(file);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(hillshade(&loaded, height), off);
        assert_eq!(hillshade(&loaded, rain), Hillshade::default());
        file.redo().unwrap();
        file.redo().unwrap();
        assert_eq!(file.project(), &project);
        drop(file);

        // A value outside its range in the file gets the nearest limit, and a
        // bad color gives black.
        let conn = Connection::open(path(&dir)).unwrap();
        let set = "UPDATE vale_raster SET hillshade_opacity = 7, hillshade_altitude = 120,
                     hillshade_exaggeration = 0, hillshade_color = 'zz' WHERE id = ?1";
        conn.execute(set, [rain.0]).unwrap();
        drop(conn);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        let read = hillshade(&loaded, rain);
        assert_eq!(read.opacity, 1.0);
        assert_eq!((read.altitude, read.exaggeration), (85.0, 0.25));
        assert_eq!(read.color, [0, 0, 0]);

        // A file before version 15 gives the default hillshade.
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(V14_DROP).unwrap();
        drop(conn);
        set_format_version(&dir, 14);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert!(
            loaded
                .rasters()
                .iter()
                .all(|r| r.hillshade == Hillshade::default())
        );
        // The undo images of version 15 stay valid.
        assert!(file.can_undo());
    }

    /// A file of version 15 with only some of the hillshade columns, as an
    /// early build of version 15 wrote it, gets the other columns.
    #[test]
    fn a_file_of_version_15_gets_the_missing_hillshade_columns() {
        let missing = [
            "hillshade_highlights",
            "hillshade_azimuth",
            "hillshade_altitude",
            "hillshade_exaggeration",
            "hillshade_color",
        ];
        for kept in [0, 1, 5] {
            let dir = tempfile::tempdir().unwrap();
            let (mut project, height, _) = rasters();
            assert!(project.set_raster_hillshade(height, true));
            assert!(project.set_raster_hillshade_opacity(height, 0.75));
            drop(ProjectFile::create(&path(&dir), &project).unwrap());
            let conn = Connection::open(path(&dir)).unwrap();
            conn.execute_batch(V16_DROP).unwrap();
            for name in &missing[kept..] {
                let drop_sql = format!("ALTER TABLE vale_raster DROP COLUMN {name}");
                conn.execute_batch(&drop_sql).unwrap();
            }
            drop(conn);
            set_format_version(&dir, 15);

            let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
            assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
            let shade = loaded.raster(height).unwrap().hillshade;
            let want = Hillshade {
                on: true,
                opacity: 0.75,
                ..Hillshade::default()
            };
            assert_eq!(shade, want, "{kept} kept");
        }
    }

    /// Removes the band colors and the smoothing of the rasters, as in a file
    /// before format version 13.
    const V12_DROP: &str = "ALTER TABLE vale_raster DROP COLUMN colors;
         ALTER TABLE vale_raster DROP COLUMN smooth;
         ALTER TABLE vale_raster DROP COLUMN feature_size;";

    #[test]
    fn the_file_keeps_the_smoothing_of_each_raster() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        assert!(project.set_raster_feature_size(height, 0.75));
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert!(project.set_raster_smoothing(height, false));
        file.save(&project).unwrap();
        assert!(project.set_raster_feature_size(rain, 6.5));
        file.save(&project).unwrap();
        drop(file);

        let smoothing = |p: &Project, id| p.raster(id).unwrap().smoothing;
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        let off = Smoothing {
            on: false,
            feature_size: 0.75,
        };
        let big = Smoothing {
            on: true,
            feature_size: 6.5,
        };
        assert_eq!(smoothing(&loaded, height), off);
        assert_eq!(smoothing(&loaded, rain), big);

        // An undo puts back the old size, and then the old switch.
        let on = Smoothing { on: true, ..off };
        file.undo().unwrap();
        assert_eq!(smoothing(file.project(), rain), Smoothing::default());
        assert_eq!(smoothing(file.project(), height), off);
        file.undo().unwrap();
        assert_eq!(smoothing(file.project(), height), on);
        drop(file);
        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(smoothing(&loaded, height), on);
        assert_eq!(smoothing(&loaded, rain), Smoothing::default());
        file.redo().unwrap();
        file.redo().unwrap();
        assert_eq!(file.project(), &project);
        drop(file);

        // A file before version 13 gives the default smoothing.
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(&format!("{V14_DROP} {V12_DROP}"))
            .unwrap();
        drop(conn);
        set_format_version(&dir, 12);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert!(
            loaded
                .rasters()
                .iter()
                .all(|r| r.smoothing == Smoothing::default())
        );
        // The undo images of version 13 stay valid.
        assert!(file.can_undo());
    }

    /// The band limits and the ramps of a raster before format version 11.
    const V10_COLUMNS: &str = "ALTER TABLE vale_raster ADD COLUMN limits TEXT NOT NULL DEFAULT '';
         ALTER TABLE vale_raster ADD COLUMN ramp_sea TEXT NOT NULL DEFAULT '';
         ALTER TABLE vale_raster ADD COLUMN ramp_land TEXT NOT NULL DEFAULT '';";

    #[test]
    fn the_rasters_of_a_file_of_version_9_show_bands() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        project.set_raster_opacity(rain, 0.5);
        // The height has no band limits, so it gets no stops.
        project.set_raster_stops(height, Vec::new(), Vec::new());
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(&format!(
            "ALTER TABLE vale_raster DROP COLUMN display;
             ALTER TABLE vale_raster DROP COLUMN stops;
             {V14_DROP}
             {V12_DROP}
             {V10_COLUMNS}"
        ))
        .unwrap();
        drop(conn);
        set_format_version(&dir, 9);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        assert!(
            loaded
                .rasters()
                .iter()
                .all(|r| r.display == RasterDisplay::Bands)
        );
        assert_eq!(loaded, project);
    }

    #[test]
    fn version_10_limits_and_ramps_become_stops() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, rain) = rasters();
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(&format!(
            "ALTER TABLE vale_raster DROP COLUMN stops;
             {V14_DROP}
             {V12_DROP}
             {V10_COLUMNS}"
        ))
        .unwrap();
        // The default bands of a height in meters.
        conn.execute(
            "UPDATE vale_raster SET limits = ?2, ramp_sea = ?3, ramp_land = ?4 WHERE id = ?1",
            params![
                height.0,
                "-4000,-2000,-1000,-200,0,200,500,1000,2000,3000,4000",
                "102c5c,bedef0",
                "6ca464,deca94,966c4c,f6f4f0",
            ],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO vale_undo_step (id, redo, bytes) VALUES (1, 0, 2);
             INSERT INTO vale_undo_record VALUES (1, 'raster', 0, x'0102');",
        )
        .unwrap();
        drop(conn);
        set_format_version(&dir, 10);

        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let raster = loaded.raster(height).unwrap();
        let values: Vec<f64> = raster.stops.iter().map(|s| s.value).collect();
        assert_eq!(
            values,
            [
                -6000.0, -4000.0, -2000.0, -1000.0, -200.0, 0.0, 200.0, 500.0, 1000.0, 2000.0,
                3000.0, 4000.0, 6000.0
            ]
        );
        // A band below sea level has the color at its top, and a band above
        // it has the color at its bottom.
        let colors = &raster.colors;
        assert_eq!(colors.len(), 12);
        assert_eq!(colors[0], [51, 80, 122]);
        assert_eq!(colors[4], [190, 222, 240]);
        assert_eq!(colors[5], [108, 164, 100]);
        // A raster with no limits gets the default of its range.
        assert!(loaded.raster(rain).unwrap().stops.is_empty());
        assert!(loaded.raster(rain).unwrap().colors.is_empty());
        let row = "SELECT stops || ' ' || colors FROM vale_raster WHERE id = 0";
        assert!(strings(&file.conn, row)[0].starts_with("-6000,-4000,"));
        assert!(strings(&file.conn, row)[0].contains(" 33507a,"));
        let count = "SELECT COUNT(*) FROM vale_undo_step";
        assert_eq!(
            file.conn
                .query_row(count, [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn the_v10_stops_need_valid_bands() {
        let stops = |unit, min, max, limits| v10_stops(unit, min, max, limits, "000000", "ffffff");
        // A range of heights without sea level as a limit.
        assert!(stops("m", -100.0, 100.0, "50").is_empty());
        assert!(stops("m", -100.0, 100.0, "0,0.5").is_empty());
        assert!(stops("m", 100.0, 100.0, "").is_empty());
        assert!(stops("mm", 0.0, 100.0, "x").is_empty());
        assert!(v10_stops("mm", 0.0, 100.0, "50", "", "").is_empty());
        // Without a sea split, all the bands use the land side.
        let rain = v10_stops("mm", 0.0, 100.0, "50", "", "000000,ffffff");
        let values: Vec<f64> = rain.iter().map(|s| s.value).collect();
        assert_eq!(values, [0.0, 50.0, 100.0]);
        assert_eq!(rain[1].below, [128, 128, 128]);
        assert_eq!(rain[1].above, [128, 128, 128]);
        // Above sea level only, 0 is a limit like any other.
        let land = stops("m", 0.0, 100.0, "1");
        assert_eq!(land.len(), 3);
    }

    #[test]
    fn stops_round_trip_as_text() {
        let mut stops = vec![stop(-0.1), stop(1e-7)];
        let text = stops_text(&stops);
        assert_eq!(text, "-0.1,0.0000001");
        assert_eq!(parse_stops(&text), stops);

        stops[0].line = StopLine {
            on: true,
            weight: 2.5,
            color: [16, 32, 48],
            darker: true,
            darken: 0.75,
        };
        stops[1].line.on = false;
        stops[1].line.weight = 0.25;
        let text = stops_text(&stops);
        let want = "-0.1|on:2.5:102030:darker:0.75,0.0000001|off:0.25:282828:color:0.5";
        assert_eq!(text, want);
        assert_eq!(parse_stops(&text), stops);
        assert_eq!(parse_stops(""), []);
        for bad in [
            "x",
            "inf",
            "NaN",
            "5:ffffff",
            "5,4",
            "5,5",
            "5,oops",
            "5|",
            "5|yes:1:ffffff:color:0.5",
            "5|on:inf:ffffff:color:0.5",
            "5|on:1:fffff:color:0.5",
            "5|on:1:ffffff",
            "5|on:1:ffffff:dark:0.5",
            "5|on:1:ffffff:color:NaN",
            "5|on:1:ffffff:color:0.5:1",
        ] {
            assert_eq!(parse_stops(bad), [], "{bad}");
        }

        let colors = [[0, 1, 2], [250, 128, 7]];
        assert_eq!(colors_text(&colors), "000102,fa8007");
        assert_eq!(
            parse_bands("0,1,2", "000102,FA8007"),
            (vec![stop(0.0), stop(1.0), stop(2.0)], colors.to_vec())
        );
        assert_eq!(parse_bands("", ""), (vec![], vec![]));
        // The colors must be valid, and one fewer than the stops.
        for (stops, colors) in [
            ("0,1", ""),
            ("0,1", "000102,000102"),
            ("0,1", "00010"),
            ("0", "000102"),
        ] {
            assert_eq!(
                parse_bands(stops, colors),
                (vec![], vec![]),
                "{stops} {colors}"
            );
        }
    }

    #[test]
    fn the_file_keeps_the_band_colors() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, _) = rasters();
        let before = project.raster(height).unwrap().clone();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let colors = vec![[9, 8, 7], [6, 5, 4]];
        assert!(project.set_raster_stops(height, before.stops.clone(), colors.clone()));
        file.save(&project).unwrap();
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(loaded.raster(height).unwrap().colors, colors);
        file.undo().unwrap();
        assert_eq!(file.project().raster(height).unwrap(), &before);
        drop(file);

        // A count of colors that does not match the stops gives no stops.
        let conn = Connection::open(path(&dir)).unwrap();
        let more = "UPDATE vale_raster SET colors = colors || ',000000' WHERE id = ?1";
        conn.execute(more, [height.0]).unwrap();
        drop(conn);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        let raster = loaded.raster(height).unwrap();
        assert!(raster.stops.is_empty() && raster.colors.is_empty());
    }

    #[test]
    fn the_file_and_the_undo_log_keep_the_stops() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, _) = rasters();
        let before = project.raster(height).unwrap().stops.clone();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let mut middle = Stop {
            value: 0.0,
            line: StopLine {
                on: true,
                weight: 3.0,
                color: [7, 8, 9],
                darker: true,
                darken: 0.25,
            },
        };
        let colors = vec![[1, 2, 3], [4, 5, 6]];
        let stops = |middle| vec![stop(-6000.0), middle, stop(6000.0)];
        assert!(project.set_raster_stops(height, stops(middle), colors.clone()));
        file.save(&project).unwrap();
        let lined = middle;
        middle.line.weight = 0.5;
        assert!(project.set_raster_stops(height, stops(middle), colors.clone()));
        file.save(&project).unwrap();
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(loaded.raster(height).unwrap().stops, stops(middle));
        file.undo().unwrap();
        assert_eq!(file.project().raster(height).unwrap().stops, stops(lined));
        file.undo().unwrap();
        assert_eq!(file.project().raster(height).unwrap().stops, before);
        drop(file);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded.raster(height).unwrap().stops, before);
    }

    /// A stop of format version 13 with two colors and no line.
    fn v13_stop(value: f64, below: Rgb, above: Rgb) -> V13Stop {
        V13Stop {
            value,
            below,
            above,
            line: StopLine::default(),
        }
    }

    #[test]
    fn version_13_stops_become_band_colors() {
        let (a, b, c, d, e) = ([1, 1, 1], [2, 2, 2], [200, 100, 50], [4, 4, 4], [5, 5, 5]);
        let darker = StopLine {
            on: true,
            darker: true,
            darken: 0.25,
            ..StopLine::default()
        };
        let mut sea = vec![
            v13_stop(-100.0, a, a),
            v13_stop(-50.0, b, b),
            v13_stop(0.0, c, d),
            v13_stop(50.0, e, e),
            v13_stop(100.0, a, a),
        ];
        sea[0].line = darker;
        sea[1].line = darker;
        sea[2].line = darker;
        sea[3].line = darker;
        sea[4].line.on = true;

        // A sea split: a band below sea level takes the color at its top.
        let (stops, colors) = v14_bands("m", -100.0, 100.0, &sea);
        assert_eq!(colors, [b, c, d, e]);
        let values: Vec<f64> = stops.iter().map(|s| s.value).collect();
        assert_eq!(values, [-100.0, -50.0, 0.0, 50.0, 100.0]);
        // The ends have no line.
        assert_eq!(stops[0].line, StopLine::default());
        assert_eq!(stops[4].line, StopLine::default());
        // A darker line keeps its old color where the band above it has
        // another color now.
        let custom = |color| StopLine {
            darker: false,
            color,
            ..darker
        };
        assert_eq!(stops[1].line, custom([2, 2, 2]));
        assert_eq!(stops[2].line, custom([150, 75, 38]));
        assert_eq!(stops[3].line, darker);

        // Land only, and a range of other values: a band takes the color at
        // its bottom.
        let land = [
            v13_stop(0.0, a, a),
            v13_stop(50.0, b, b),
            v13_stop(100.0, c, c),
        ];
        assert_eq!(v14_bands("m", 0.0, 100.0, &land).1, [a, b]);
        assert_eq!(v14_bands("mm", 0.0, 100.0, &land).1, [a, b]);
        let rain = [
            v13_stop(-100.0, a, a),
            v13_stop(0.0, b, b),
            v13_stop(100.0, c, c),
        ];
        assert_eq!(v14_bands("mm", -100.0, 100.0, &rain).1, [a, b]);

        // Stops that the old app did not take give no stops.
        let none = (vec![], vec![]);
        let no_sea = [
            v13_stop(-100.0, a, a),
            v13_stop(50.0, b, b),
            v13_stop(100.0, c, c),
        ];
        assert_eq!(v14_bands("m", -100.0, 100.0, &no_sea), none);
        let two = [
            v13_stop(0.0, a, a),
            v13_stop(50.0, b, c),
            v13_stop(100.0, c, c),
        ];
        assert_eq!(v14_bands("m", 0.0, 100.0, &two), none);
        assert_eq!(v14_bands("mm", -100.0, 100.0, &sea), none);
        assert_eq!(v14_bands("m", -100.0, 100.0, &sea[..1]), none);
        assert_eq!(v14_bands("m", -100.0, 100.0, &[]), none);

        // A file of version 13.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let id = project.add_raster("Height".into(), 1000, "m".into(), -100.0, 100.0);
        drop(ProjectFile::create(&path(&dir), &project).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(&format!(
            "{V14_DROP} ALTER TABLE vale_raster DROP COLUMN colors"
        ))
        .unwrap();
        let old = "UPDATE vale_raster SET stops = ?2 WHERE id = ?1";
        conn.execute(old, params![id.0, v13_stops_text(&sea)])
            .unwrap();
        drop(conn);
        set_format_version(&dir, 13);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let raster = loaded.raster(id).unwrap();
        assert_eq!((&raster.stops, &raster.colors), (&stops, &colors));
    }

    #[test]
    fn a_version_13_undo_image_keeps_its_band_colors() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let id = project.add_raster("Height".into(), 1000, "m".into(), -100.0, 100.0);
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        project.set_raster_opacity(id, 0.5);
        file.save(&project).unwrap();
        drop(file);

        // The image of the raster before the edit, with stops of version 13.
        let (a, b, c) = ([1, 1, 1], [2, 2, 2], [3, 3, 3]);
        let old = [
            v13_stop(-100.0, a, a),
            v13_stop(0.0, b, c),
            v13_stop(100.0, a, a),
        ];
        let mut before = Project::new(World::default());
        before.add_raster("Height".into(), 1000, "m".into(), -100.0, 100.0);
        let image = undo::image_v13(before.raster(id).unwrap(), &old);
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch(&format!(
            "{V14_DROP} ALTER TABLE vale_raster DROP COLUMN colors"
        ))
        .unwrap();
        let set = "UPDATE vale_undo_record SET data = ?1 WHERE kind = 'raster'";
        assert_eq!(conn.execute(set, [&image]).unwrap(), 1);
        drop(conn);
        set_format_version(&dir, 13);

        let (mut file, _) = ProjectFile::open(&path(&dir)).unwrap();
        let sizes = "SELECT bytes = (SELECT SUM(LENGTH(data)) FROM vale_undo_record) \
                     FROM vale_undo_step";
        assert!(
            file.conn
                .query_row(sizes, [], |r| r.get::<_, bool>(0))
                .unwrap()
        );
        file.undo().unwrap();
        let raster = file.project().raster(id).unwrap();
        assert_eq!(raster.opacity, 1.0);
        assert_eq!(raster.stops, [stop(-100.0), stop(0.0), stop(100.0)]);
        assert_eq!(raster.colors, [b, c]);
    }

    /// QGIS lists the tables of `gpkg_contents` that have features. A raster
    /// layer is not in that table, and it has no table of its own.
    #[test]
    fn a_raster_layer_has_no_feature_table() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        project.move_layer(project.raster_layer(rain).unwrap().id, 99);
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let tiles = vec![(key(0, 0, 0), pixels(1))];
        file.save_tiles(&project, &[put(height, &tiles, vec![])])
            .unwrap();
        // A save after the first one removes the tables of the layers.
        file.save(&project).unwrap();
        let conn = &file.conn;

        assert_eq!(
            strings(
                conn,
                "SELECT kind || ' ' || table_name || ' ' || raster_id FROM vale_layer
                 WHERE raster_id IS NOT NULL ORDER BY position"
            ),
            ["raster vale_raster_0 0", "raster vale_raster_1 1"]
        );
        let vectors = strings(
            conn,
            "SELECT table_name FROM vale_layer WHERE raster_id IS NULL ORDER BY table_name",
        );
        assert_eq!(vectors.len(), 5);
        assert_eq!(
            strings(
                conn,
                "SELECT table_name FROM gpkg_contents WHERE data_type = 'features'
                 ORDER BY table_name"
            ),
            vectors
        );
        assert_eq!(
            strings(
                conn,
                "SELECT table_name FROM gpkg_geometry_columns ORDER BY table_name"
            ),
            vectors
        );
        const RASTER_ROWS: &str = "(SELECT table_name FROM vale_layer WHERE raster_id IS NOT NULL)";
        for sql in [
            format!("SELECT table_name FROM gpkg_contents WHERE table_name IN {RASTER_ROWS}"),
            format!("SELECT name FROM sqlite_master WHERE name IN {RASTER_ROWS}"),
            "SELECT name FROM vale_field WHERE layer_id IN
               (SELECT id FROM vale_layer WHERE raster_id IS NOT NULL)"
                .to_string(),
            // Each table is in the contents table.
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT GLOB 'gpkg_*' AND name NOT GLOB 'sqlite_*'
               AND name NOT IN (SELECT table_name FROM gpkg_contents)"
                .to_string(),
        ] {
            assert_eq!(strings(conn, &sql), [] as [&str; 0], "{sql}");
        }
        // The tile table stays when the layer records go.
        assert_eq!(tiles_of(&file, project.raster(height).unwrap()), tiles);
        drop(file);
        assert_eq!(ProjectFile::open(&path(&dir)).unwrap().1, project);
    }

    #[test]
    fn a_removed_raster_leaves_no_tile_and_an_undo_puts_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, height, rain) = rasters();
        let high = vec![(key(0, 0, 0), pixels(1)), (key(2, 3, 4), pixels(2))];
        let wet = vec![(key(5, 3, 3), pixels(3))];
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let changes = [put(height, &high, vec![]), put(rain, &wet, vec![])];
        file.save_tiles(&project, &changes).unwrap();
        let before = content(&file);
        const TILES: &str = "SELECT CAST(raster_id AS TEXT) FROM vale_raster_tile ORDER BY 1";
        const RECORDS: &str = "SELECT CAST(id AS TEXT) FROM vale_raster ORDER BY 1";

        project.remove_raster(height);
        let stats = file.save_tiles(&project, &[]).unwrap();
        assert_eq!((stats.tiles_written, stats.tiles_deleted), (0, 2));
        assert_eq!(strings(&file.conn, TILES), ["1"]);
        assert_eq!(strings(&file.conn, RECORDS), ["1"]);
        let after = content(&file);
        drop(file);

        let (mut file, _) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(file.undo().unwrap().unwrap().tiles.len(), 2);
        assert_eq!(content(&file), before);
        assert_eq!(file.project().rasters()[0].id, height);
        assert_eq!(
            tiles_of(&file, file.project().raster(height).unwrap()),
            high
        );
        assert_eq!(strings(&file.conn, TILES), ["0", "0", "1"]);
        file.redo().unwrap();
        assert_eq!(content(&file), after);
        assert_eq!(strings(&file.conn, TILES), ["1"]);
        assert_eq!(strings(&file.conn, RECORDS), ["1"]);
    }

    #[test]
    fn the_file_keeps_the_order_the_name_and_the_visibility_and_undo_puts_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new(World::default());
        let ids: Vec<LayerId> = ["a", "b", "c"]
            .iter()
            .map(|n| project.add_layer(n.to_string(), GeometryKind::Line, None, vec![]))
            .collect();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let mut states = vec![project.clone()];
        project.move_layer(ids[2], 0);
        file.save(&project).unwrap();
        states.push(project.clone());
        project.set_layer_visible(ids[0], false);
        file.save(&project).unwrap();
        states.push(project.clone());
        project.rename_layer(ids[1], "Coast".into());
        file.save(&project).unwrap();
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert!(loaded == project);
        let names: Vec<&str> = loaded.layers().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "Coast"]);
        assert!(!loaded.layer(ids[0]).unwrap().visible);
        for state in states.iter().rev() {
            assert!(file.undo().unwrap().is_some());
            assert!(file.project() == state);
        }
    }

    #[test]
    fn a_file_of_version_4_gets_a_seed_of_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(path(&dir)).unwrap();
        let tx = conn.transaction().unwrap();
        for step in &MIGRATIONS[..4] {
            step(&tx).unwrap();
        }
        tx.execute_batch(
            "INSERT INTO vale_meta VALUES ('format_version', '4'), ('next_layer_id', '0');
             INSERT INTO vale_world VALUES (1, 'Old', 1234.5, 'imperial');",
        )
        .unwrap();
        tx.commit().unwrap();
        drop(conn);

        let (mut file, mut project) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(
            (project.world.seed, project.world.units),
            (0, Units::Imperial)
        );
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        project.world.seed = 41;
        file.save(&project).unwrap();
        drop(file);
        let (_, again) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(again.world.seed, 41);
    }

    #[test]
    fn the_summary_has_the_name_and_the_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, mut project) = {
            drop(ProjectFile::create(&path(&dir), &sample()).unwrap());
            ProjectFile::open(&path(&dir)).unwrap()
        };
        let name = project.world.name.clone();
        assert_eq!(
            summary(&path(&dir)).unwrap(),
            Summary {
                name,
                thumbnail: None
            }
        );

        let picture = |seed: u8| Thumbnail {
            width: 3,
            height: 2,
            rgba: (0..24).map(|i| i * seed).collect(),
        };
        file.set_thumbnail(&picture(1)).unwrap();
        file.set_thumbnail(&picture(2)).unwrap();
        // The thumbnail is not an edit.
        assert!(!file.can_undo());
        project.world.name = "Renamed".into();
        file.save(&project).unwrap();
        // The summary reads a file that the app has open.
        let found = summary(&path(&dir)).unwrap();
        assert_eq!(found.name, "Renamed");
        assert_eq!(found.thumbnail, Some(picture(2)));
        // An undo keeps the thumbnail.
        file.undo().unwrap();
        assert_eq!(summary(&path(&dir)).unwrap().thumbnail, Some(picture(2)));
        drop(file);

        // A file of version 5 has no thumbnail table.
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute_batch("DROP TABLE vale_thumbnail").unwrap();
        drop(conn);
        set_format_version(&dir, 5);
        assert_eq!(summary(&path(&dir)).unwrap().thumbnail, None);

        set_format_version(&dir, FORMAT_VERSION + 1);
        assert!(matches!(
            summary(&path(&dir)),
            Err(StoreError::NewerFormat { .. })
        ));
    }

    /// The system can stop the app in the middle of a save. The file then
    /// has a journal, and the list of projects still reads it.
    #[test]
    fn the_summary_reads_a_file_with_a_save_that_did_not_end() {
        let dir = tempfile::tempdir().unwrap();
        drop(ProjectFile::create(&path(&dir), &sample()).unwrap());
        let name = summary(&path(&dir)).unwrap().name;

        let conn = Connection::open(path(&dir)).unwrap();
        // A small cache writes the changed pages to the file before the
        // commit.
        conn.execute_batch(
            "PRAGMA cache_size = 10;
             BEGIN;
             UPDATE vale_world SET name = 'Half saved';
             CREATE TABLE filler (data BLOB);",
        )
        .unwrap();
        for _ in 0..50 {
            conn.execute("INSERT INTO filler VALUES (zeroblob(100000))", [])
                .unwrap();
        }
        let stopped = dir.path().join("stopped.gpkg");
        let journal = |file: &Path| {
            let mut name = file.as_os_str().to_owned();
            name.push("-journal");
            std::path::PathBuf::from(name)
        };
        std::fs::copy(path(&dir), &stopped).unwrap();
        std::fs::copy(journal(&path(&dir)), journal(&stopped)).unwrap();
        drop(conn);

        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY;
        let read_only = Connection::open_with_flags(&stopped, flags).unwrap();
        let read = read_only.query_row("SELECT name FROM vale_world", [], |_| Ok(()));
        assert!(
            read.is_err(),
            "a read-only connection does not read the file"
        );
        drop(read_only);

        assert_eq!(summary(&stopped).unwrap().name, name);
        assert_eq!(ProjectFile::world_name(&stopped).unwrap(), name);
        assert!(!journal(&stopped).exists());
    }

    #[test]
    fn a_file_of_version_1_gets_the_raster_tables() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(path(&dir)).unwrap();
        let tx = conn.transaction().unwrap();
        v1(&tx).unwrap();
        tx.execute_batch(
            "INSERT INTO vale_meta VALUES ('format_version', '1'), ('next_layer_id', '4');
             INSERT INTO vale_world VALUES (1, 'Old', 1234.5);",
        )
        .unwrap();
        tx.commit().unwrap();
        drop(conn);

        let (mut file, mut project) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(project.world.name, "Old");
        assert_eq!(project.rasters(), []);
        assert_eq!(project.world.units, Units::Metric);
        assert_eq!(format_version(&file.conn).unwrap(), FORMAT_VERSION);
        let id = project.add_raster("Height".into(), 4096, "m".into(), -6000.0, 6000.0);
        assert_eq!(id, RasterId(0));
        let tiles = vec![(key(1, 2, 3), pixels(1))];
        file.save_tiles(&project, &[put(id, &tiles, vec![])])
            .unwrap();
        drop(file);
        let (file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(tiles_of(&file, loaded.raster(id).unwrap()), tiles);
    }

    #[test]
    fn a_file_of_version_2_gets_metric_units() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(path(&dir)).unwrap();
        let tx = conn.transaction().unwrap();
        v1(&tx).unwrap();
        v2(&tx).unwrap();
        tx.execute_batch(
            "INSERT INTO vale_meta VALUES ('format_version', '2'), ('next_layer_id', '0');
             INSERT INTO vale_world VALUES (1, 'Old', 1234.5);",
        )
        .unwrap();
        tx.commit().unwrap();
        drop(conn);

        // The name of the world comes from a file of an older version.
        assert_eq!(ProjectFile::world_name(&path(&dir)).unwrap(), "Old");
        let (mut file, mut project) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(project.world.units, Units::Metric);
        assert_eq!(project.world.radius_km, 1234.5);
        project.world.units = Units::Imperial;
        file.save(&project).unwrap();
        drop(file);
        let (_, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(loaded.world.units, Units::Imperial);
    }

    #[test]
    fn unknown_units_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        drop(ProjectFile::create(&path(&dir), &sample()).unwrap());
        let conn = Connection::open(path(&dir)).unwrap();
        conn.execute("UPDATE vale_world SET units = 'cubits'", [])
            .unwrap();
        drop(conn);
        let error = ProjectFile::open(&path(&dir)).map(|_| ()).unwrap_err();
        assert!(error.to_string().contains("cubits"), "{error}");
    }

    #[test]
    fn a_damaged_tile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        let tiles = vec![(key(1, 2, 3), pixels(1))];
        let changes = [put(height, &tiles, vec![]), put(rain, &tiles, vec![])];
        file.save_tiles(&project, &changes).unwrap();
        let load = |file: &ProjectFile, id| {
            file.load_tiles(project.raster(id).unwrap(), |_, _| {})
                .map_err(|e| e.to_string())
        };
        assert_eq!(load(&file, height), Ok(()));

        file.conn
            .execute(
                "UPDATE vale_raster_tile SET data = x'0102' WHERE raster_id = 0",
                [],
            )
            .unwrap();
        assert!(load(&file, height).unwrap_err().contains("pixels"));
        // Column 4 is outside a face of 1,000 pixels.
        file.conn
            .execute(
                "UPDATE vale_raster_tile SET tile_column = 4 WHERE raster_id = 1",
                [],
            )
            .unwrap();
        assert!(load(&file, rain).unwrap_err().contains("key"));
        file.conn
            .execute("UPDATE vale_raster_tile SET tile_column = 3, level = 1", [])
            .unwrap();
        assert!(load(&file, rain).unwrap_err().contains("key"));
    }

    /// Ground that does not compress.
    fn noise(seed: u32) -> Vec<u16> {
        let mut n = seed.wrapping_mul(2_654_435_761) | 1;
        (0..TILE_SIZE * TILE_SIZE)
            .map(|_| {
                n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (n >> 16) as u16
            })
            .collect()
    }

    /// The project and each tile of each raster of a file. The project has
    /// no next IDs, because an undo does not put them back.
    type Content = (Project, Vec<Vec<(TileKey, Vec<u16>)>>);

    fn content(file: &ProjectFile) -> Content {
        let mut project = file.project().clone();
        (project.next_id, project.next_raster_id) = (0, 0);
        let tiles = project.rasters().iter().map(|r| tiles_of(file, r));
        (project.clone(), tiles.collect())
    }

    fn steps(file: &ProjectFile) -> i64 {
        let count = "SELECT COUNT(*) FROM vale_undo_step";
        file.conn.query_row(count, [], |r| r.get(0)).unwrap()
    }

    /// A file with three edits: a brush stroke, a vector edit, and an edit
    /// of both with a new world name. Returns the content after the creation
    /// and after each edit.
    fn three_edits(dir: &tempfile::TempDir) -> (ProjectFile, Vec<Content>) {
        let (mut project, height, rain) = rasters();
        let mut file = ProjectFile::create(&path(dir), &project).unwrap();
        let mut states = vec![content(&file)];

        let stroke = vec![(key(0, 0, 0), pixels(1)), (key(2, 3, 4), pixels(2))];
        let wet = vec![(key(5, 3, 3), pixels(3))];
        let changes = [put(height, &stroke, vec![]), put(rain, &wet, vec![])];
        file.save_tiles(&project, &changes).unwrap();
        states.push(content(&file));

        let places = project.layers()[2].id;
        let rivers = project.layers()[3].id;
        project.layers[2].features.remove(1);
        project.layers[2].features[0].geometry = Geometry::Points(vec![[1.0, 2.0]]);
        project.remove_layer(rivers);
        project.add_layer(
            "Towns".into(),
            GeometryKind::Point,
            None,
            vec![Feature::new(
                Geometry::Points(vec![[3.0, 4.0]]),
                attributes(&[("name", text("Bree"))]),
            )],
        );
        assert_eq!(project.layers()[2].id, places);
        file.save(&project).unwrap();
        states.push(content(&file));

        project.world.name = "Third".into();
        project.raster_mut(height).unwrap().base = 7;
        project.layers.swap(2, 4);
        let stroke = vec![(key(0, 0, 0), pixels(4)), (key(1, 1, 1), pixels(5))];
        let changes = [put(height, &stroke, vec![key(2, 3, 4)])];
        file.save_tiles(&project, &changes).unwrap();
        states.push(content(&file));
        (file, states)
    }

    #[test]
    fn an_undo_puts_back_the_radius_and_the_units_of_the_world() {
        let dir = tempfile::tempdir().unwrap();
        let first = sample();
        let mut project = first.clone();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        project.world.radius_km = 200.0;
        project.world.units = Units::Metric;
        file.save(&project).unwrap();
        assert!(file.undo().unwrap().is_some());
        assert!(file.project() == &first);
        assert!(file.redo().unwrap().is_some());
        assert!(file.project() == &project);
    }

    #[test]
    fn undo_works_after_a_close_and_an_open() {
        let dir = tempfile::tempdir().unwrap();
        let (file, states) = three_edits(&dir);
        drop(file);

        let (mut file, loaded) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(&loaded, file.project());
        assert_eq!(content(&file), states[3]);
        assert!(file.can_undo() && !file.can_redo());
        for state in states[..3].iter().rev() {
            assert!(file.undo().unwrap().is_some());
            assert_eq!(&content(&file), state);
        }
        assert!(!file.can_undo() && file.can_redo());
        assert_eq!(file.undo().unwrap(), None);
        drop(file);

        // The file holds the project from after the undo steps, and the redo
        // steps are there too.
        let (mut file, _) = ProjectFile::open(&path(&dir)).unwrap();
        assert_eq!(content(&file), states[0]);
        assert!(!file.can_undo() && file.can_redo());
        for state in &states[1..] {
            assert!(file.redo().unwrap().is_some());
            assert_eq!(&content(&file), state);
        }
        assert!(file.can_undo() && !file.can_redo());
        assert_eq!(file.redo().unwrap(), None);
        // Each feature keeps its UUID through an undo and a redo.
        assert_eq!(content(&file), states[3]);
    }

    #[test]
    fn brush_strokes_and_vector_edits_use_the_same_log() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, states) = three_edits(&dir);
        let (height, rain) = (RasterId(0), RasterId(1));
        // Each edit is one step of one table, in the order of the edits.
        let rows = |file: &ProjectFile, table: &str| {
            let sql = format!(
                "SELECT group_concat(n, ' ') FROM (SELECT COUNT(t.step_id) AS n
                 FROM vale_undo_step s LEFT JOIN {table} t ON t.step_id = s.id
                 GROUP BY s.id ORDER BY s.id)"
            );
            strings(&file.conn, &sql).remove(0)
        };
        assert_eq!(rows(&file, "vale_undo_tile"), "3 0 3");
        assert_eq!(rows(&file, "vale_undo_record"), "0 6 4");

        // An undo names the tiles that it changed.
        let restored = file.undo().unwrap().unwrap();
        let tiles = [key(0, 0, 0), key(1, 1, 1), key(2, 3, 4)].map(|k| (height, k));
        assert_eq!(restored.tiles, tiles);
        assert_eq!(file.load_tile(height, key(1, 1, 1)).unwrap(), None);
        assert_eq!(
            file.load_tile(height, key(2, 3, 4)).unwrap(),
            Some(pixels(2))
        );
        assert_eq!(file.undo().unwrap().unwrap().tiles, []);
        let restored = file.undo().unwrap().unwrap();
        assert_eq!(restored.tiles.len(), 3);
        assert!(restored.tiles.contains(&(rain, key(5, 3, 3))));
        assert_eq!(content(&file), states[0]);

        // A new edit removes the redo steps, and a save with no change keeps
        // them.
        file.redo().unwrap();
        assert_eq!(content(&file), states[1]);
        file.save_tiles(
            &states[1].0,
            &[put(height, &[(key(0, 0, 0), pixels(1))], vec![])],
        )
        .unwrap();
        file.save_tiles(&states[1].0, &[put(height, &[], vec![key(4, 4, 4)])])
            .unwrap();
        assert!(file.can_redo());
        assert_eq!(steps(&file), 3);
        let mut next = states[1].0.clone();
        next.world.radius_km = 99.0;
        next.world.seed = 7;
        file.save(&next).unwrap();
        assert!(!file.can_redo());
        assert_eq!(steps(&file), 2);
        file.undo().unwrap();
        assert_eq!(content(&file), states[1]);
    }

    #[test]
    fn an_undo_puts_back_a_raster_and_its_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, states) = three_edits(&dir);
        let (height, rain) = (RasterId(0), RasterId(1));
        let mut project = states[3].0.clone();
        project.remove_raster(height);
        project.raster_mut(rain).unwrap().face_size = 2000;
        let wet = vec![(key(0, 7, 7), pixels(8))];
        file.save_tiles(&project, &[put(rain, &wet, vec![])])
            .unwrap();
        let after = content(&file);
        assert_eq!(after.1, [wet]);

        assert_eq!(file.undo().unwrap().unwrap().tiles.len(), 4);
        assert_eq!(content(&file), states[3]);
        file.redo().unwrap();
        assert_eq!(content(&file), after);
        assert_eq!(
            strings(
                &file.conn,
                "SELECT DISTINCT CAST(raster_id AS TEXT) FROM vale_raster_tile"
            ),
            ["1"]
        );

        // An edit that replaces each tile of a raster.
        let ground = pixels(9);
        let all = TileChanges {
            raster: rain,
            all: true,
            put: vec![(key(1, 0, 0), &ground[..])],
            delete: vec![],
        };
        file.save_tiles(&project, &[all]).unwrap();
        file.undo().unwrap();
        assert_eq!(content(&file), after);
    }

    /// The files are copied while an edit is half in the file, as a stop of
    /// the app leaves them.
    #[test]
    fn a_stop_in_the_middle_of_an_edit_leaves_the_file_from_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, states) = three_edits(&dir);
        let before = std::fs::read(path(&dir)).unwrap();
        // With a small page cache, SQLite writes pages to the file before
        // the end of the transaction.
        file.conn.execute_batch("PRAGMA cache_size = 4").unwrap();

        let mut project = states[3].0.clone();
        project.world.name = "Half".into();
        project.remove_layer(project.layers()[2].id);
        let stroke: Vec<_> = (0..40).map(|i| (key(3, i, 0), noise(i))).collect();
        let tx = file.conn.transaction().unwrap();
        let changes = [put(RasterId(0), &stroke, vec![key(0, 0, 0)])];
        write_edit(&tx, Some(&file.project), &project, &changes, UNDO_LIMIT).unwrap();

        let stopped = tempfile::tempdir().unwrap();
        let journal = |dir: &tempfile::TempDir| dir.path().join("world.gpkg-journal");
        std::fs::copy(path(&dir), path(&stopped)).unwrap();
        std::fs::copy(journal(&dir), journal(&stopped)).unwrap();
        assert_ne!(std::fs::read(path(&stopped)).unwrap(), before);
        drop(tx);

        for dir in [&stopped, &dir] {
            let (mut file, _) = ProjectFile::open(&path(dir)).unwrap();
            assert_eq!(content(&file), states[3]);
            assert_eq!(steps(&file), 3);
            file.undo().unwrap();
            assert_eq!(content(&file), states[2]);
        }
    }

    #[test]
    fn the_undo_log_has_a_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (project, height, _) = rasters();
        let mut file = ProjectFile::create(&path(&dir), &project).unwrap();
        assert_eq!(file.undo_limit(), UNDO_LIMIT);
        let stroke = |file: &mut ProjectFile, seed: u32| {
            let tiles = vec![(key(0, 0, 0), noise(seed))];
            file.save_tiles(&project, &[put(height, &tiles, vec![])])
                .unwrap();
        };
        // The first stroke makes the tile, so its step holds no pixels.
        let tile = 2 * (TILE_SIZE * TILE_SIZE) as u64;
        for seed in 0..4 {
            stroke(&mut file, seed);
        }
        assert_eq!(steps(&file), 4);
        assert!(file.undo_bytes() > 3 * tile && file.undo_bytes() < 3 * tile + 4096);

        // The oldest steps go.
        file.set_undo_limit(2 * tile + 4096);
        stroke(&mut file, 4);
        assert_eq!(steps(&file), 2);
        assert!(file.undo_bytes() <= file.undo_limit());
        file.undo().unwrap();
        file.undo().unwrap();
        assert_eq!(
            file.load_tile(height, key(0, 0, 0)).unwrap(),
            Some(noise(2))
        );
        assert!(!file.can_undo());

        // The undo steps go before the redo steps, and the next redo goes
        // last.
        file.redo().unwrap();
        file.set_undo_limit(tile + 4096);
        file.undo().unwrap();
        assert_eq!(steps(&file), 1);
        file.redo().unwrap();
        assert_eq!(
            file.load_tile(height, key(0, 0, 0)).unwrap(),
            Some(noise(3))
        );
        assert!(!file.can_redo());

        // A step that is larger than the limit goes.
        file.set_undo_limit(100);
        stroke(&mut file, 5);
        assert_eq!((steps(&file), file.undo_bytes()), (0, 0));
        assert!(!file.can_undo());
    }

    #[test]
    fn a_damaged_undo_step_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, states) = three_edits(&dir);
        file.conn
            .execute(
                "UPDATE vale_undo_record SET data = x'0102' WHERE kind = 'world'",
                [],
            )
            .unwrap();
        assert!(matches!(file.undo(), Err(StoreError::Corrupt(_))));
        // The undo that failed changed nothing.
        assert_eq!(content(&file), states[3]);
    }

    #[test]
    fn a_base_save_makes_no_step_and_removes_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let (mut file, states) = three_edits(&dir);
        file.undo().unwrap();
        let mut project = file.project().clone();
        project.world.name = "Base".into();
        file.save_base(&project).unwrap();
        assert!(!file.can_undo() && !file.can_redo());
        assert_eq!((steps(&file), file.undo_bytes()), (0, 0));
        assert_eq!(content(&file).1, states[2].1);
        assert_eq!(file.project().world.name, "Base");
    }
}
