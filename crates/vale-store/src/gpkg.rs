//! The project file: a GeoPackage with extra `vale_` tables.
//!
//! Each vector layer is one GeoPackage feature table. The `vale_` tables hold
//! the world, the layer order, the attribute schema, the rasters, and the
//! raster tiles. They are in
//! `gpkg_contents` with the data type `vale`. GDAL, and thus QGIS, lists a
//! table that is not in `gpkg_contents` as a layer, and it does not list a
//! table with a data type that it does not know.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, params, params_from_iter};

use crate::tile::{self, TILE_SIZE};
use crate::{
    Feature, Geometry, GeometryKind, Layer, LayerId, LonLat, Project, Raster, RasterId, Rgb, Uuid,
    Value, World,
};

/// The version of the `vale_` tables that this code writes.
pub const FORMAT_VERSION: u32 = 2;

/// One step for each format version. Step `n` changes a file of version `n`
/// to version `n + 1`. A new version adds a step and never changes an old one.
const MIGRATIONS: &[fn(&Transaction) -> rusqlite::Result<()>] = &[v1, v2];

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

fn corrupt(what: impl Into<String>) -> StoreError {
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

/// An open project file.
pub struct ProjectFile {
    conn: Connection,
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
        let mut file = ProjectFile { conn };
        file.save(project)?;
        Ok(file)
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
        Ok((ProjectFile { conn }, project))
    }

    /// Writes the project in one transaction. Each feature keeps its UUID,
    /// and each raster keeps its tiles.
    pub fn save(&mut self, project: &Project) -> Result<(), StoreError> {
        self.save_tiles(project, &[]).map(|_| ())
    }

    /// Writes the project and the changed raster tiles in one transaction.
    /// Each other tile stays as it is. A raster that is not in the project
    /// loses its tiles, and so does a raster with a new face size.
    pub fn save_tiles(
        &mut self,
        project: &Project,
        changes: &[TileChanges],
    ) -> Result<SaveStats, StoreError> {
        let tx = self.conn.transaction()?;
        clear(&tx)?;
        write_world(&tx, &project.world)?;
        tx.execute(
            "INSERT OR REPLACE INTO vale_meta (key, value) VALUES ('next_layer_id', ?1)",
            [project.next_id.to_string()],
        )?;
        let mut tables = BTreeSet::new();
        for (position, layer) in project.layers.iter().enumerate() {
            let table = table_name(layer, &mut tables);
            write_layer(&tx, position, layer, &table)?;
        }
        let mut stats = write_rasters(&tx, project)?;
        for change in changes {
            write_tiles(&tx, change, &mut stats)?;
        }
        tx.commit()?;
        Ok(stats)
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

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Removes the world, the layer records, and the feature tables.
fn clear(tx: &Transaction) -> Result<(), StoreError> {
    let tables: Vec<String> = tx
        .prepare("SELECT table_name FROM vale_layer")?
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
        "INSERT INTO vale_world (id, name, radius_km) VALUES (1, ?1, ?2)",
        params![world.name, world.radius_km],
    )?;
    tx.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES (?1, ?2, 'NONE', ?2, ?3, NULL)",
        params![world.name, WORLD_SRS, world_wkt(world)],
    )?;
    Ok(())
}

/// A table name for the layer that no other layer has. SQLite compares names
/// without case, and it keeps some prefixes for itself and for GeoPackage.
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

/// How the values of one field are stored in its column.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FieldKind {
    Bool,
    Number,
    Text,
    /// Values of more than one type, or a number that is not finite. The
    /// column is text, and the first character of each cell gives the type.
    Mixed,
}

impl FieldKind {
    const ALL: [FieldKind; 4] = [
        FieldKind::Bool,
        FieldKind::Number,
        FieldKind::Text,
        FieldKind::Mixed,
    ];

    fn name(self) -> &'static str {
        match self {
            FieldKind::Bool => "bool",
            FieldKind::Number => "number",
            FieldKind::Text => "text",
            FieldKind::Mixed => "mixed",
        }
    }

    fn column_type(self) -> &'static str {
        match self {
            FieldKind::Bool => "BOOLEAN",
            FieldKind::Number => "DOUBLE",
            FieldKind::Text | FieldKind::Mixed => "TEXT",
        }
    }

    fn of(layer: &Layer, field: &str) -> FieldKind {
        let mut kind = None;
        for value in layer
            .features
            .iter()
            .filter_map(|f| f.attributes.get(field))
        {
            let this = match value {
                Value::Null => continue,
                Value::Bool(_) => FieldKind::Bool,
                Value::Number(n) if n.is_finite() => FieldKind::Number,
                Value::Number(_) => FieldKind::Mixed,
                Value::Text(_) => FieldKind::Text,
            };
            match kind {
                None => kind = Some(this),
                Some(k) if k == this => {}
                Some(_) => return FieldKind::Mixed,
            }
        }
        kind.unwrap_or(FieldKind::Text)
    }

    fn write(self, value: &Value) -> SqlValue {
        match (self, value) {
            (_, Value::Null) => SqlValue::Null,
            (FieldKind::Mixed, Value::Bool(b)) => SqlValue::Text(format!("B{}", *b as u8)),
            (FieldKind::Mixed, Value::Number(n)) => SqlValue::Text(format!("N{n:?}")),
            (FieldKind::Mixed, Value::Text(s)) => SqlValue::Text(format!("T{s}")),
            (_, Value::Bool(b)) => SqlValue::Integer(*b as i64),
            (_, Value::Number(n)) => SqlValue::Real(*n),
            (_, Value::Text(s)) => SqlValue::Text(s.clone()),
        }
    }

    fn read(self, cell: ValueRef) -> Option<Value> {
        Some(match (self, cell) {
            (_, ValueRef::Null) => Value::Null,
            (FieldKind::Bool, ValueRef::Integer(i)) => Value::Bool(i != 0),
            (FieldKind::Number, ValueRef::Real(n)) => Value::Number(n),
            (FieldKind::Number, ValueRef::Integer(i)) => Value::Number(i as f64),
            (FieldKind::Text, ValueRef::Text(s)) => {
                Value::Text(std::str::from_utf8(s).ok()?.into())
            }
            (FieldKind::Mixed, ValueRef::Text(s)) => {
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
}

fn geometry_type(kind: GeometryKind) -> &'static str {
    match kind {
        GeometryKind::Point => "MULTIPOINT",
        GeometryKind::Line => "MULTILINESTRING",
        GeometryKind::Polygon => "MULTIPOLYGON",
    }
}

fn write_layer(
    tx: &Transaction,
    position: usize,
    layer: &Layer,
    table: &str,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO vale_layer (id, position, name, kind, table_name, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            layer.id.0,
            position as i64,
            layer.name,
            layer.kind.name(),
            table,
            layer.source
        ],
    )?;
    let columns = column_names(&layer.fields);
    let kinds: Vec<FieldKind> = layer
        .fields
        .iter()
        .map(|f| FieldKind::of(layer, f))
        .collect();
    let mut create = format!(
        "CREATE TABLE {} (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, geom {}, \
         uuid TEXT NOT NULL UNIQUE",
        quote(table),
        geometry_type(layer.kind)
    );
    let mut insert = format!("INSERT INTO {} (fid, geom, uuid", quote(table));
    for (i, (column, kind)) in columns.iter().zip(&kinds).enumerate() {
        tx.execute(
            "INSERT INTO vale_field (layer_id, position, name, column_name, kind)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![layer.id.0, i as i64, layer.fields[i], column, kind.name()],
        )?;
        create += &format!(", {} {}", quote(column), kind.column_type());
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
        params![table, geometry_type(layer.kind), WORLD_SRS],
    )?;

    let mut insert = tx.prepare(&insert)?;
    for (i, feature) in layer.features.iter().enumerate() {
        if feature.geometry.kind() != layer.kind {
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
        for (field, kind) in layer.fields.iter().zip(&kinds) {
            let value = feature.attributes.get(field).unwrap_or(&Value::Null);
            row.push(kind.write(value));
        }
        insert.execute(params_from_iter(row))?;
    }
    Ok(())
}

fn colors_text(colors: &[Rgb]) -> String {
    let hex: Vec<String> = colors
        .iter()
        .map(|[r, g, b]| format!("{r:02x}{g:02x}{b:02x}"))
        .collect();
    hex.join(",")
}

fn parse_colors(text: &str) -> Option<Vec<Rgb>> {
    let color = |hex: &str| {
        let n = u32::from_str_radix(hex, 16).ok()?;
        (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some([
            (n >> 16) as u8,
            (n >> 8) as u8,
            n as u8,
        ])
    };
    text.split(',')
        .filter(|s| !s.is_empty())
        .map(color)
        .collect()
}

fn limits_text(limits: &[f64]) -> String {
    let numbers: Vec<String> = limits.iter().map(f64::to_string).collect();
    numbers.join(",")
}

fn parse_limits(text: &str) -> Option<Vec<f64>> {
    let numbers = text.split(',').filter(|s| !s.is_empty());
    numbers.map(|s| s.parse().ok()).collect()
}

/// Writes the raster records. The tiles of a raster stay, but not if the
/// raster is no longer in the project or its face size is new.
fn write_rasters(tx: &Transaction, project: &Project) -> Result<SaveStats, StoreError> {
    let stored: Vec<(u32, u32)> = tx
        .prepare("SELECT id, face_size FROM vale_raster")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut stats = SaveStats::default();
    for (id, face_size) in stored {
        let raster = project.raster(RasterId(id));
        if raster.is_none_or(|r| r.face_size != face_size) {
            stats.tiles_deleted +=
                tx.execute("DELETE FROM vale_raster_tile WHERE raster_id = ?1", [id])?;
        }
        if raster.is_none() {
            tx.execute("DELETE FROM vale_raster WHERE id = ?1", [id])?;
        }
    }
    for (position, r) in project.rasters.iter().enumerate() {
        // A replaced row is a deleted row, and the tiles refer to the row.
        tx.execute(
            "INSERT INTO vale_raster (id, position, name, face_size, unit, value_min,
               value_max, base, limits, ramp_sea, ramp_land)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (id) DO UPDATE SET
               position = excluded.position, name = excluded.name,
               face_size = excluded.face_size, unit = excluded.unit,
               value_min = excluded.value_min, value_max = excluded.value_max,
               base = excluded.base, limits = excluded.limits,
               ramp_sea = excluded.ramp_sea, ramp_land = excluded.ramp_land",
            params![
                r.id.0,
                position as i64,
                r.name,
                r.face_size,
                r.unit,
                r.value_min,
                r.value_max,
                r.base,
                limits_text(&r.limits),
                colors_text(&r.ramp_sea),
                colors_text(&r.ramp_land),
            ],
        )?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO vale_meta (key, value) VALUES ('next_raster_id', ?1)",
        [project.next_raster_id.to_string()],
    )?;
    Ok(stats)
}

fn write_tiles(
    tx: &Transaction,
    change: &TileChanges,
    stats: &mut SaveStats,
) -> Result<(), StoreError> {
    let raster = change.raster.0;
    if change.all {
        stats.tiles_deleted += tx.execute(
            "DELETE FROM vale_raster_tile WHERE raster_id = ?1",
            [raster],
        )?;
    }
    let mut delete = tx.prepare(
        "DELETE FROM vale_raster_tile WHERE raster_id = ?1 AND level = ?2 AND face = ?3
           AND tile_column = ?4 AND tile_row = ?5",
    )?;
    for key in &change.delete {
        stats.tiles_deleted +=
            delete.execute(params![raster, key.level, key.face, key.column, key.row])?;
    }
    let mut put = tx.prepare(
        "INSERT INTO vale_raster_tile (raster_id, level, face, tile_column, tile_row, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT DO UPDATE SET data = excluded.data",
    )?;
    for batch in change.put.chunks(TILE_BATCH) {
        let blobs = tile::parallel_map(batch, |(_, pixels)| tile::encode(pixels));
        for ((key, _), blob) in batch.iter().zip(blobs) {
            put.execute(params![
                raster, key.level, key.face, key.column, key.row, blob
            ])?;
            stats.tiles_written += 1;
        }
    }
    Ok(())
}

fn load_rasters(conn: &Connection) -> Result<Vec<Raster>, StoreError> {
    let mut rows = conn.prepare(
        "SELECT id, name, face_size, unit, value_min, value_max, base, limits, ramp_sea,
           ramp_land
         FROM vale_raster ORDER BY position, id",
    )?;
    let mut rows = rows.query([])?;
    let mut rasters = Vec::new();
    while let Some(r) = rows.next()? {
        let name: String = r.get(1)?;
        let bad = |what: &str| corrupt(format!("the {what} of raster {name:?}"));
        let (face_size, base): (i64, i64) = (r.get(2)?, r.get(6)?);
        let text = |i: usize| r.get::<_, String>(i);
        rasters.push(Raster {
            id: RasterId(r.get(0)?),
            face_size: u32::try_from(face_size)
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| bad("face size"))?,
            unit: r.get(3)?,
            value_min: r.get(4)?,
            value_max: r.get(5)?,
            base: u16::try_from(base).map_err(|_| bad("base"))?,
            limits: parse_limits(&text(7)?).ok_or_else(|| bad("band limits"))?,
            ramp_sea: parse_colors(&text(8)?).ok_or_else(|| bad("ramp"))?,
            ramp_land: parse_colors(&text(9)?).ok_or_else(|| bad("ramp"))?,
            name: name.clone(),
        });
    }
    Ok(rasters)
}

fn load(conn: &Connection) -> Result<Project, StoreError> {
    use rusqlite::OptionalExtension;
    let world = conn
        .query_row("SELECT name, radius_km FROM vale_world", [], |r| {
            Ok(World {
                name: r.get(0)?,
                radius_km: r.get(1)?,
            })
        })
        .optional()?
        .ok_or_else(|| corrupt("no world record"))?;
    let next_id = meta(conn, "next_layer_id")?;
    let next_id = next_id
        .parse()
        .map_err(|_| corrupt(format!("next layer ID {next_id:?}")))?;
    let next_raster_id = meta(conn, "next_raster_id")?;
    let next_raster_id = next_raster_id
        .parse()
        .map_err(|_| corrupt(format!("next raster ID {next_raster_id:?}")))?;

    let mut layers = Vec::new();
    let mut rows = conn
        .prepare("SELECT id, name, kind, table_name, source FROM vale_layer ORDER BY position")?;
    let mut rows = rows.query([])?;
    while let Some(r) = rows.next()? {
        let kind: String = r.get(2)?;
        let kind = GeometryKind::from_name(&kind)
            .ok_or_else(|| corrupt(format!("layer kind {kind:?}")))?;
        let table: String = r.get(3)?;
        let mut layer = Layer {
            id: LayerId(r.get(0)?),
            name: r.get(1)?,
            kind,
            source: r.get(4)?,
            fields: Vec::new(),
            features: Vec::new(),
        };
        load_features(conn, &mut layer, &table)?;
        layers.push(layer);
    }
    Ok(Project {
        world,
        layers,
        next_id,
        rasters: load_rasters(conn)?,
        next_raster_id,
    })
}

fn load_features(conn: &Connection, layer: &mut Layer, table: &str) -> Result<(), StoreError> {
    let mut select = "SELECT geom, uuid".to_string();
    let mut kinds = Vec::new();
    let mut fields = conn.prepare(
        "SELECT name, column_name, kind FROM vale_field WHERE layer_id = ?1 ORDER BY position",
    )?;
    let mut fields = fields.query([layer.id.0])?;
    while let Some(r) = fields.next()? {
        let column: String = r.get(1)?;
        let kind: String = r.get(2)?;
        let kind = FieldKind::ALL
            .into_iter()
            .find(|k| k.name() == kind)
            .ok_or_else(|| corrupt(format!("field kind {kind:?}")))?;
        layer.fields.push(r.get(0)?);
        kinds.push(kind);
        select += &format!(", {}", quote(&column));
    }
    select += &format!(" FROM {} ORDER BY fid", quote(table));

    let mut rows = conn.prepare(&select)?;
    let mut rows = rows.query([])?;
    while let Some(r) = rows.next()? {
        let geometry = decode_geometry(r.get_ref(0)?.as_blob().unwrap_or(&[]), layer.kind)
            .ok_or_else(|| corrupt(format!("a geometry of layer {:?}", layer.name)))?;
        let uuid: String = r.get(1)?;
        let uuid = Uuid::parse_str(&uuid).map_err(|_| corrupt(format!("UUID {uuid:?}")))?;
        let mut attributes = BTreeMap::new();
        for (i, (field, kind)) in layer.fields.iter().zip(&kinds).enumerate() {
            let value = kind
                .read(r.get_ref(i + 2)?)
                .ok_or_else(|| corrupt(format!("a value of field {field:?}")))?;
            attributes.insert(field.clone(), value);
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
fn encode_geometry(geometry: &Geometry) -> Vec<u8> {
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
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn f64(&mut self) -> Option<f64> {
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
fn decode_geometry(blob: &[u8], kind: GeometryKind) -> Option<Geometry> {
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
                "odd \"quote\" TEXT 0",
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

    /// A project with a heightmap and a second raster of another face size.
    fn rasters() -> (Project, RasterId, RasterId) {
        let mut project = sample();
        let height = project.add_raster("Height".into(), 8192);
        let rain = project.add_raster("Rain".into(), 1000);
        *project.raster_mut(height).unwrap() = Raster {
            id: height,
            name: "Height".into(),
            face_size: 8192,
            unit: "m".into(),
            value_min: -6000.0,
            value_max: 6000.0,
            base: 19114,
            limits: vec![-4000.0, -0.1, 0.0, 1e-7, 1234.5678],
            ramp_sea: vec![[0, 1, 2], [250, 128, 7]],
            ramp_land: vec![[255, 255, 255]],
        };
        project.raster_mut(rain).unwrap().unit = "mm".into();
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
        next.raster_mut(rain).unwrap().limits = vec![5.0];
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
        let id = next.add_raster("New".into(), 4096);
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
        let id = project.add_raster("Height".into(), 8192);
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
        assert_eq!(format_version(&file.conn).unwrap(), 2);
        let id = project.add_raster("Height".into(), 4096);
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

        file.conn
            .execute(
                "UPDATE vale_raster SET ramp_sea = '00ff0g' WHERE id = 0",
                [],
            )
            .unwrap();
        drop(file);
        match ProjectFile::open(&path(&dir)) {
            Err(StoreError::Corrupt(what)) => assert!(what.contains("ramp"), "{what}"),
            other => panic!("{:?}", other.map(|x| x.1)),
        }
    }
}
