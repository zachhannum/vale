//! The undo log of the project file.
//!
//! A step is one edit. It holds an image of each record and each raster tile
//! that the edit changed, from before the edit. An undo swaps each image with
//! the record or the tile in the file, so the step then holds the images from
//! after the edit, and a redo swaps them back. A record is the world, one
//! raster, or one vector layer with its features.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::gpkg::{Reader, StoreError, TileKey, corrupt, decode_geometry, encode_geometry};
use crate::{
    Feature, GeometryKind, Layer, LayerId, Project, Raster, RasterId, Units, Uuid, Value, World,
};

/// The default limit of the size of the log, in bytes of compressed images.
pub const UNDO_LIMIT: u64 = 256 << 20;

/// The codec of a record image: deflate.
const DEFLATE: u8 = 1;

const WORLD: &str = "world";
const RASTER: &str = "raster";
const LAYER: &str = "layer";

/// The log tables.
pub(crate) fn v3(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE vale_undo_step (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           redo INTEGER NOT NULL,
           bytes INTEGER NOT NULL);
         CREATE TABLE vale_undo_record (
           step_id INTEGER NOT NULL REFERENCES vale_undo_step(id),
           kind TEXT NOT NULL,
           id INTEGER NOT NULL,
           data BLOB,
           PRIMARY KEY (step_id, kind, id));
         CREATE TABLE vale_undo_tile (
           step_id INTEGER NOT NULL REFERENCES vale_undo_step(id),
           raster_id INTEGER NOT NULL,
           level INTEGER NOT NULL,
           face INTEGER NOT NULL,
           tile_column INTEGER NOT NULL,
           tile_row INTEGER NOT NULL,
           data BLOB,
           PRIMARY KEY (step_id, raster_id, level, face, tile_column, tile_row));
         INSERT INTO gpkg_contents (table_name, data_type, identifier)
           VALUES ('vale_undo_step', 'vale', 'vale_undo_step'),
                  ('vale_undo_record', 'vale', 'vale_undo_record'),
                  ('vale_undo_tile', 'vale', 'vale_undo_tile');
         INSERT INTO gpkg_extensions
           SELECT table_name, NULL, 'vale_project', 'Records of the Vale app', 'read-write'
           FROM gpkg_contents WHERE table_name GLOB 'vale_undo_*';",
    )
}

/// The SQL that keeps the image of one tile for a step. The image is NULL if
/// the file has no such tile. The first image of a tile in a step stays.
pub(crate) const KEEP_TILE: &str = "INSERT OR IGNORE INTO vale_undo_tile
       (step_id, raster_id, level, face, tile_column, tile_row, data)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6,
       (SELECT data FROM vale_raster_tile WHERE raster_id = ?2 AND level = ?3 AND face = ?4
          AND tile_column = ?5 AND tile_row = ?6))";

/// The SQL that keeps the image of one tile that the file holds.
pub(crate) const KEEP_OLD_TILE: &str = "INSERT OR IGNORE INTO vale_undo_tile
       (step_id, raster_id, level, face, tile_column, tile_row, data)
     SELECT ?1, raster_id, level, face, tile_column, tile_row, data FROM vale_raster_tile
     WHERE raster_id = ?2 AND level = ?3 AND face = ?4 AND tile_column = ?5 AND tile_row = ?6";

/// Starts a step and returns its ID.
pub(crate) fn begin(tx: &Transaction) -> Result<i64, StoreError> {
    tx.execute("INSERT INTO vale_undo_step (redo, bytes) VALUES (0, 0)", [])?;
    Ok(tx.last_insert_rowid())
}

/// Keeps the image of each record of `old` that is not the same in `new`.
pub(crate) fn keep_records(
    tx: &Transaction,
    step: i64,
    old: &Project,
    new: &Project,
) -> Result<(), StoreError> {
    let mut changed: Vec<(&str, u32)> = Vec::new();
    if old.world != new.world {
        changed.push((WORLD, 0));
    }
    let ids = |a: &Project, b: &Project| -> Vec<u32> {
        let rasters = a.rasters.iter().enumerate();
        let rasters = rasters.filter(|(i, r)| b.rasters.get(*i) != Some(r));
        rasters.map(|(_, r)| r.id.0).collect()
    };
    let mut rasters = ids(old, new);
    rasters.extend(ids(new, old));
    let ids = |a: &Project, b: &Project| -> Vec<u32> {
        let layers = a.layers.iter().enumerate();
        let layers = layers.filter(|(i, l)| b.layers.get(*i) != Some(l));
        layers.map(|(_, l)| l.id.0).collect()
    };
    let mut layers = ids(old, new);
    layers.extend(ids(new, old));
    changed.extend(rasters.into_iter().map(|id| (RASTER, id)));
    changed.extend(layers.into_iter().map(|id| (LAYER, id)));

    let mut keep = tx.prepare(
        "INSERT OR IGNORE INTO vale_undo_record (step_id, kind, id, data) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (kind, id) in changed {
        keep.execute(params![step, kind, id, image(old, kind, id)])?;
    }
    Ok(())
}

/// Keeps the image of each tile of a raster.
pub(crate) fn keep_raster_tiles(
    tx: &Transaction,
    step: i64,
    raster: u32,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT OR IGNORE INTO vale_undo_tile
           (step_id, raster_id, level, face, tile_column, tile_row, data)
         SELECT ?1, raster_id, level, face, tile_column, tile_row, data
         FROM vale_raster_tile WHERE raster_id = ?2",
        params![step, raster],
    )?;
    Ok(())
}

/// Ends the step of an edit. A step with no image goes, and the redo steps
/// stay. Any other step makes the redo steps invalid.
pub(crate) fn end(tx: &Transaction, step: i64, limit: u64) -> Result<(), StoreError> {
    let empty: bool = tx.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM vale_undo_record WHERE step_id = ?1)
           AND NOT EXISTS (SELECT 1 FROM vale_undo_tile WHERE step_id = ?1)",
        [step],
        |r| r.get(0),
    )?;
    if empty {
        return remove(tx, step);
    }
    let redo: Vec<i64> = tx
        .prepare("SELECT id FROM vale_undo_step WHERE redo = 1")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in redo {
        remove(tx, id)?;
    }
    measure(tx, step)?;
    trim(tx, limit)
}

/// The next step to undo, or with `redo`, the next step to redo.
pub(crate) fn next(tx: &Transaction, redo: bool) -> Result<Option<i64>, StoreError> {
    let sql = if redo {
        "SELECT MIN(id) FROM vale_undo_step WHERE redo = 1"
    } else {
        "SELECT MAX(id) FROM vale_undo_step WHERE redo = 0"
    };
    Ok(tx.query_row(sql, [], |r| r.get(0))?)
}

/// Swaps the record images of a step with the records of `current`. Returns
/// the project with the records of the step.
pub(crate) fn swap_records(
    tx: &Transaction,
    step: i64,
    current: &Project,
) -> Result<Project, StoreError> {
    let rows: Vec<(String, u32, Option<Vec<u8>>)> = tx
        .prepare("SELECT kind, id, data FROM vale_undo_record WHERE step_id = ?1")?
        .query_map([step], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut update = tx.prepare(
        "UPDATE vale_undo_record SET data = ?4 WHERE step_id = ?1 AND kind = ?2 AND id = ?3",
    )?;
    let mut next = current.clone();
    let mut rasters = Vec::new();
    let mut layers = Vec::new();
    for (kind, id, data) in rows {
        let bad = || corrupt(format!("the {kind} image of undo step {step}"));
        update.execute(params![step, kind, id, image(current, &kind, id)])?;
        let payload = match &data {
            Some(blob) => Some(unpack(blob).ok_or_else(bad)?),
            None => None,
        };
        let mut r = Reader(payload.as_deref().unwrap_or_default());
        match kind.as_str() {
            WORLD => {
                next.world = payload
                    .is_some()
                    .then(|| world(&mut r))
                    .flatten()
                    .ok_or_else(bad)?
            }
            RASTER => {
                next.rasters.retain(|x| x.id.0 != id);
                if payload.is_some() {
                    rasters.push(raster(&mut r).ok_or_else(bad)?);
                }
            }
            LAYER => {
                next.layers.retain(|x| x.id.0 != id);
                if payload.is_some() {
                    layers.push(layer(&mut r).ok_or_else(bad)?);
                }
            }
            _ => return Err(bad()),
        }
    }
    // Each record that did not change has its old place, so each image goes
    // to its old place when the images go in from the first place up.
    rasters.sort_by_key(|(position, _)| *position);
    for (position, raster) in rasters {
        let at = position.min(next.rasters.len());
        next.rasters.insert(at, raster);
    }
    layers.sort_by_key(|(position, _)| *position);
    for (position, layer) in layers {
        let at = position.min(next.layers.len());
        next.layers.insert(at, layer);
    }
    Ok(next)
}

/// Swaps the tile images of a step with the tiles of the file. Returns the
/// keys of the tiles.
pub(crate) fn swap_tiles(
    tx: &Transaction,
    step: i64,
) -> Result<Vec<(RasterId, TileKey)>, StoreError> {
    let keys: Vec<(u32, TileKey)> = tx
        .prepare(
            "SELECT raster_id, level, face, tile_column, tile_row FROM vale_undo_tile
             WHERE step_id = ?1 ORDER BY raster_id, level, face, tile_row, tile_column",
        )?
        .query_map([step], |r| {
            let key = TileKey {
                level: r.get(1)?,
                face: r.get(2)?,
                column: r.get(3)?,
                row: r.get(4)?,
            };
            Ok((r.get(0)?, key))
        })?
        .collect::<Result<_, _>>()?;
    const IMAGE: &str = "step_id = ?1 AND raster_id = ?2 AND level = ?3 AND face = ?4
        AND tile_column = ?5 AND tile_row = ?6";
    const TILE: &str = "raster_id = ?2 AND level = ?3 AND face = ?4
        AND tile_column = ?5 AND tile_row = ?6";
    let mut read = tx.prepare(&format!("SELECT data FROM vale_undo_tile WHERE {IMAGE}"))?;
    let mut keep = tx.prepare(&format!(
        "UPDATE vale_undo_tile SET data = (SELECT data FROM vale_raster_tile WHERE {TILE})
         WHERE {IMAGE}"
    ))?;
    let mut put = tx.prepare(
        "INSERT INTO vale_raster_tile (raster_id, level, face, tile_column, tile_row, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT DO UPDATE SET data = excluded.data",
    )?;
    let mut delete = tx.prepare(
        "DELETE FROM vale_raster_tile WHERE raster_id = ?1 AND level = ?2 AND face = ?3
           AND tile_column = ?4 AND tile_row = ?5",
    )?;
    for (raster, key) in &keys {
        let at = params![step, raster, key.level, key.face, key.column, key.row];
        let image: Option<Vec<u8>> = read.query_row(at, |r| r.get(0))?;
        keep.execute(at)?;
        match image {
            Some(blob) => put.execute(params![
                raster, key.level, key.face, key.column, key.row, blob
            ])?,
            None => delete.execute(params![raster, key.level, key.face, key.column, key.row])?,
        };
    }
    Ok(keys.into_iter().map(|(r, k)| (RasterId(r), k)).collect())
}

/// Ends an undo or a redo. The step is a redo step after an undo.
pub(crate) fn moved(tx: &Transaction, step: i64, redo: bool, limit: u64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE vale_undo_step SET redo = ?2 WHERE id = ?1",
        params![step, redo],
    )?;
    measure(tx, step)?;
    trim(tx, limit)
}

/// True if the log has an undo step, true if it has a redo step, and the
/// size of the images, in bytes.
pub(crate) fn state(conn: &rusqlite::Connection) -> Result<(bool, bool, u64), StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM vale_undo_step WHERE redo = 0),
                EXISTS (SELECT 1 FROM vale_undo_step WHERE redo = 1),
                (SELECT COALESCE(SUM(bytes), 0) FROM vale_undo_step)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}

/// Sets the size of a step from its images.
fn measure(tx: &Transaction, step: i64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE vale_undo_step SET bytes =
           (SELECT COALESCE(SUM(LENGTH(data)), 0) FROM vale_undo_record WHERE step_id = ?1)
           + (SELECT COALESCE(SUM(LENGTH(data)), 0) FROM vale_undo_tile WHERE step_id = ?1)
         WHERE id = ?1",
        [step],
    )?;
    Ok(())
}

/// Removes steps until the log is inside the limit. The oldest undo steps go
/// first. The redo steps go after them, and the next redo goes last.
fn trim(tx: &Transaction, limit: u64) -> Result<(), StoreError> {
    loop {
        let bytes: u64 = tx.query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM vale_undo_step",
            [],
            |r| r.get(0),
        )?;
        if bytes <= limit {
            return Ok(());
        }
        let step: Option<i64> = tx
            .query_row(
                "SELECT id FROM vale_undo_step
                 ORDER BY redo, CASE redo WHEN 0 THEN id ELSE -id END LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match step {
            Some(step) => remove(tx, step)?,
            None => return Ok(()),
        }
    }
}

/// Removes each step.
pub(crate) fn clear(tx: &Transaction) -> Result<(), StoreError> {
    tx.execute_batch(
        "DELETE FROM vale_undo_record; DELETE FROM vale_undo_tile; DELETE FROM vale_undo_step;",
    )?;
    Ok(())
}

fn remove(tx: &Transaction, step: i64) -> Result<(), StoreError> {
    tx.execute("DELETE FROM vale_undo_record WHERE step_id = ?1", [step])?;
    tx.execute("DELETE FROM vale_undo_tile WHERE step_id = ?1", [step])?;
    tx.execute("DELETE FROM vale_undo_step WHERE id = ?1", [step])?;
    Ok(())
}

/// The image of one record of a project. `None`: the project has no such
/// record.
fn image(project: &Project, kind: &str, id: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match kind {
        WORLD => put_world(&mut out, &project.world),
        RASTER => {
            let at = project.rasters.iter().position(|r| r.id.0 == id)?;
            put_raster(&mut out, at, &project.rasters[at]);
        }
        _ => {
            let at = project.layers.iter().position(|l| l.id.0 == id)?;
            put_layer(&mut out, at, &project.layers[at]);
        }
    }
    Some(pack(&out))
}

fn pack(payload: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(vec![DEFLATE], Compression::fast());
    encoder
        .write_all(payload)
        .and_then(|()| encoder.finish())
        .expect("a write to memory")
}

fn unpack(blob: &[u8]) -> Option<Vec<u8>> {
    let (&codec, stream) = blob.split_first()?;
    if codec != DEFLATE {
        return None;
    }
    let mut payload = Vec::new();
    DeflateDecoder::new(stream).read_to_end(&mut payload).ok()?;
    Some(payload)
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_le_bytes());
}

fn put_f64(out: &mut Vec<u8>, n: f64) {
    out.extend_from_slice(&n.to_le_bytes());
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len());
    out.extend_from_slice(bytes);
}

fn bytes<'a>(r: &mut Reader<'a>) -> Option<&'a [u8]> {
    let n = r.u32()? as usize;
    let (head, rest) = r.0.split_at_checked(n)?;
    r.0 = rest;
    Some(head)
}

fn string(r: &mut Reader) -> Option<String> {
    String::from_utf8(bytes(r)?.to_vec()).ok()
}

fn count(r: &mut Reader) -> Option<usize> {
    r.u32().map(|n| n as usize)
}

fn put_world(out: &mut Vec<u8>, world: &World) {
    put_bytes(out, world.name.as_bytes());
    put_f64(out, world.radius_km);
    put_bytes(out, world.units.id().as_bytes());
}

/// An image from format version 3 ends after the radius, and its world has
/// metric units.
fn world(r: &mut Reader) -> Option<World> {
    Some(World {
        name: string(r)?,
        radius_km: r.f64()?,
        units: match r.0.is_empty() {
            true => Units::Metric,
            false => Units::from_id(&string(r)?)?,
        },
    })
}

fn put_raster(out: &mut Vec<u8>, position: usize, raster: &Raster) {
    put_u32(out, position);
    put_u32(out, raster.id.0 as usize);
    put_bytes(out, raster.name.as_bytes());
    put_u32(out, raster.face_size as usize);
    put_bytes(out, raster.unit.as_bytes());
    put_f64(out, raster.value_min);
    put_f64(out, raster.value_max);
    put_u32(out, raster.base as usize);
    put_u32(out, raster.limits.len());
    for &limit in &raster.limits {
        put_f64(out, limit);
    }
    for ramp in [&raster.ramp_sea, &raster.ramp_land] {
        put_bytes(out, ramp.as_flattened());
    }
}

/// A raster and its place in the project.
fn raster(r: &mut Reader) -> Option<(usize, Raster)> {
    let position = count(r)?;
    let id = RasterId(r.u32()?);
    let name = string(r)?;
    let face_size = r.u32()?;
    let unit = string(r)?;
    let (value_min, value_max) = (r.f64()?, r.f64()?);
    let base = u16::try_from(r.u32()?).ok()?;
    let limits = (0..count(r)?).map(|_| r.f64()).collect::<Option<_>>()?;
    let mut ramp = || {
        let (colors, rest) = bytes(r)?.as_chunks::<3>();
        rest.is_empty().then(|| colors.to_vec())
    };
    let raster = Raster {
        id,
        name,
        face_size,
        unit,
        value_min,
        value_max,
        base,
        limits,
        ramp_sea: ramp()?,
        ramp_land: ramp()?,
    };
    Some((position, raster))
}

const KINDS: [GeometryKind; 3] = [
    GeometryKind::Point,
    GeometryKind::Line,
    GeometryKind::Polygon,
];

fn put_layer(out: &mut Vec<u8>, position: usize, layer: &Layer) {
    put_u32(out, position);
    put_u32(out, layer.id.0 as usize);
    put_bytes(out, layer.name.as_bytes());
    out.push(KINDS.iter().position(|k| *k == layer.kind).expect("a kind") as u8);
    out.push(layer.source.is_some() as u8);
    put_bytes(out, layer.source.as_deref().unwrap_or_default().as_bytes());
    put_u32(out, layer.fields.len());
    for field in &layer.fields {
        put_bytes(out, field.as_bytes());
    }
    put_u32(out, layer.features.len());
    for feature in &layer.features {
        out.extend_from_slice(feature.uuid().as_bytes());
        put_bytes(out, &encode_geometry(&feature.geometry));
        put_u32(out, feature.attributes.len());
        for (name, value) in &feature.attributes {
            put_bytes(out, name.as_bytes());
            match value {
                Value::Null => out.push(0),
                Value::Bool(b) => out.extend_from_slice(&[1, *b as u8]),
                Value::Number(n) => {
                    out.push(2);
                    put_f64(out, *n);
                }
                Value::Text(s) => {
                    out.push(3);
                    put_bytes(out, s.as_bytes());
                }
            }
        }
    }
}

/// A layer and its place in the project.
fn layer(r: &mut Reader) -> Option<(usize, Layer)> {
    let position = count(r)?;
    let id = LayerId(r.u32()?);
    let name = string(r)?;
    let [kind, has_source] = r.take()?;
    let kind = *KINDS.get(kind as usize)?;
    let source = string(r)?;
    let source = (has_source == 1).then_some(source);
    let fields = (0..count(r)?).map(|_| string(r)).collect::<Option<_>>()?;
    let mut features = Vec::new();
    for _ in 0..count(r)? {
        let uuid = Uuid::from_bytes(r.take()?);
        let geometry = decode_geometry(bytes(r)?, kind)?;
        let mut attributes = std::collections::BTreeMap::new();
        for _ in 0..count(r)? {
            let name = string(r)?;
            let [tag] = r.take()?;
            let value = match tag {
                0 => Value::Null,
                1 => Value::Bool(r.take::<1>()? == [1]),
                2 => Value::Number(r.f64()?),
                3 => Value::Text(string(r)?),
                _ => return None,
            };
            attributes.insert(name, value);
        }
        features.push(Feature::with_uuid(uuid, geometry, attributes));
    }
    let layer = Layer {
        id,
        name,
        kind,
        source,
        fields,
        features,
    };
    Some((position, layer))
}
