//! The undo log of the project file.
//!
//! A step is one edit. It holds an image of each record and each raster tile
//! that the edit changed, from before the edit. An undo swaps each image with
//! the record or the tile in the file, so the step then holds the images from
//! after the edit, and a redo swaps them back. A record is the world, one
//! raster, one image, one layer with its features, or the atlas. The step of a new raster layer or
//! of a removed one holds the image of the layer and the image of the raster.
//! The same is true of an image layer. A step does not hold the tiles of an
//! image, because they do not change. They stay in the file while a step
//! holds the record of the image.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::atlas::{
    Atlas, Border, CoordFormat, EdgeLabels, FrameId, FrameRecord, FrameShape, Furniture,
    FurnitureId, FurnitureRecord, Graticule, LayoutGrid, MapId, MapRecord, Neatline, PageId,
    PageRecord, PageUnits, ProjectionPreset, encode_rings, read_rings,
};
use crate::gpkg::{
    Reader, StoreError, TileKey, V13Stop, corrupt, decode_geometry, encode_geometry,
    remove_image_tiles, v14_bands,
};
use crate::{
    BlendMode, Feature, Field, FieldType, GeometryKind, Hillshade, Image, ImageId, Layer, LayerId,
    LayerKind, Project, Raster, RasterDisplay, RasterId, Smoothing, Stop, StopLine, Units, Uuid,
    Value, World, clamp_feature_size, clamp_opacity, infer_fields,
};

/// The default limit of the size of the log, in bytes of compressed images.
pub const UNDO_LIMIT: u64 = 256 << 20;

/// The codec of a record image: deflate.
const DEFLATE: u8 = 1;

const WORLD: &str = "world";
const RASTER: &str = "raster";
const IMAGE: &str = "image";
const LAYER: &str = "layer";
const ATLAS: &str = "atlas";

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
    if old.atlas != new.atlas {
        changed.push((ATLAS, 0));
    }
    let ids = |a: &Project, b: &Project| -> Vec<u32> {
        let rasters = a.rasters.iter().filter(|r| b.raster(r.id) != Some(r));
        rasters.map(|r| r.id.0).collect()
    };
    let mut rasters = ids(old, new);
    rasters.extend(ids(new, old));
    let ids = |a: &Project, b: &Project| -> Vec<u32> {
        let images = a.images.iter().filter(|i| b.image(i.id) != Some(i));
        images.map(|i| i.id.0).collect()
    };
    let mut images = ids(old, new);
    images.extend(ids(new, old));
    let ids = |a: &Project, b: &Project| -> Vec<u32> {
        let layers = a.layers.iter().enumerate();
        let layers = layers.filter(|(i, l)| b.layers.get(*i) != Some(l));
        layers.map(|(_, l)| l.id.0).collect()
    };
    let mut layers = ids(old, new);
    layers.extend(ids(new, old));
    changed.extend(rasters.into_iter().map(|id| (RASTER, id)));
    changed.extend(images.into_iter().map(|id| (IMAGE, id)));
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
                    next.rasters.push(raster(&mut r).ok_or_else(bad)?);
                }
            }
            IMAGE => {
                next.images.retain(|x| x.id.0 != id);
                if payload.is_some() {
                    next.images.push(read_image(&mut r).ok_or_else(bad)?);
                }
            }
            LAYER => {
                next.layers.retain(|x| x.id.0 != id);
                if payload.is_some() {
                    layers.push(layer(&mut r).ok_or_else(bad)?);
                }
            }
            ATLAS => {
                let image = payload.is_some().then(|| atlas(&mut r)).flatten();
                let (maps, pages, presets, grid) = image.ok_or_else(bad)?;
                let atlas = &mut next.atlas;
                (atlas.maps, atlas.pages, atlas.presets, atlas.grid) = (maps, pages, presets, grid);
            }
            _ => return Err(bad()),
        }
    }
    next.rasters.sort_by_key(|r| r.id);
    next.images.sort_by_key(|i| i.id);
    // Each layer that did not change has its old place, so each image goes
    // to its old place when the images go in from the first place up.
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
fn measure(tx: &Transaction, step: i64) -> rusqlite::Result<()> {
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
            remove_image_tiles(tx)?;
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
            None => {
                remove_image_tiles(tx)?;
                return Ok(());
            }
        }
    }
}

/// Removes each step.
pub(crate) fn clear(tx: &Transaction) -> Result<(), StoreError> {
    tx.execute_batch(
        "DELETE FROM vale_undo_record; DELETE FROM vale_undo_tile; DELETE FROM vale_undo_step;",
    )?;
    remove_image_tiles(tx)?;
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
        RASTER => put_raster(&mut out, project.raster(RasterId(id))?),
        IMAGE => put_image(&mut out, project.image(ImageId(id))?),
        ATLAS => put_atlas(&mut out, &project.atlas),
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
    out.extend_from_slice(&world.seed.to_le_bytes());
    put_f64(out, world.wind_deg);
    put_f64(out, world.rain_shadow);
}

/// An image from format version 3 ends after the radius, and its world has
/// metric units. An image from format version 4 ends after the units. The
/// seed of each is zero. An image from a format version before 32 ends after
/// the seed, and its world has a west wind and no rain shadow.
fn world(r: &mut Reader) -> Option<World> {
    Some(World {
        name: string(r)?,
        radius_km: r.f64()?,
        units: match r.0.is_empty() {
            true => Units::Metric,
            false => Units::from_id(&string(r)?)?,
        },
        seed: if r.0.is_empty() { 0 } else { r.u32()? },
        wind_deg: if r.0.is_empty() { 270.0 } else { r.f64()? },
        rain_shadow: if r.0.is_empty() { 0.0 } else { r.f64()? },
    })
}

/// The image of the atlas holds the maps, the pages, the custom
/// projections, the style overrides and the label classes of the maps, and
/// the furniture of the pages, and not the counters of the IDs. An image that
/// ends after the projections has no overrides. An image that ends after the
/// overrides has no label classes. An image that ends after the label
/// classes has no furniture. An image that ends after the furniture has white
/// paper, and frames with no border. An image that ends after the borders
/// has the default layout grid.
fn put_atlas(out: &mut Vec<u8>, atlas: &Atlas) {
    put_u32(out, atlas.maps.len());
    for map in &atlas.maps {
        put_u32(out, map.id.0 as usize);
        put_bytes(out, map.name.as_bytes());
        put_bytes(out, map.projection.as_bytes());
        put_u32(out, map.hidden.len());
        for layer in &map.hidden {
            put_u32(out, layer.0 as usize);
        }
    }
    put_u32(out, atlas.pages.len());
    for page in &atlas.pages {
        put_u32(out, page.id.0 as usize);
        put_bytes(out, page.name.as_bytes());
        put_f64(out, page.width_mm);
        put_f64(out, page.height_mm);
        put_bytes(out, page.units.id().as_bytes());
        put_u32(out, page.frames.len());
        for frame in &page.frames {
            put_u32(out, frame.id.0 as usize);
            put_u32(out, frame.map.0 as usize);
            put_bytes(out, frame.shape.kind_id().as_bytes());
            out.extend_from_slice(&encode_rings(&frame.shape.rings()));
            put_f64(out, frame.center[0]);
            put_f64(out, frame.center[1]);
            put_f64(out, frame.scale);
            put_f64(out, frame.rotation);
            let graticule = &frame.graticule;
            out.push(graticule.on as u8);
            match graticule.interval {
                Some(interval) => {
                    out.push(1);
                    put_f64(out, interval);
                }
                None => out.push(0),
            }
            out.extend_from_slice(&graticule.color);
            put_f64(out, graticule.width);
            put_bytes(out, graticule.format.id().as_bytes());
            put_bytes(out, graticule.meridians.to_json().as_bytes());
            put_bytes(out, graticule.parallels.to_json().as_bytes());
            let neatline = &frame.neatline;
            out.push(neatline.on as u8);
            out.extend_from_slice(&neatline.color);
            put_f64(out, neatline.width);
        }
    }
    put_u32(out, atlas.presets.len());
    for preset in &atlas.presets {
        put_bytes(out, preset.name.as_bytes());
        put_bytes(out, preset.projection.as_bytes());
    }
    let overrides: Vec<_> = (atlas.maps.iter())
        .flat_map(|m| {
            m.overrides
                .iter()
                .map(move |(layer, text)| (m.id, layer, text))
        })
        .collect();
    put_u32(out, overrides.len());
    for (map, layer, text) in overrides {
        put_u32(out, map.0 as usize);
        put_u32(out, layer.0 as usize);
        put_bytes(out, text.as_bytes());
    }
    let labels: Vec<_> = (atlas.maps.iter())
        .flat_map(|m| {
            m.labels
                .iter()
                .map(move |(layer, text)| (m.id, layer, text))
        })
        .collect();
    put_u32(out, labels.len());
    for (map, layer, text) in labels {
        put_u32(out, map.0 as usize);
        put_u32(out, layer.0 as usize);
        put_bytes(out, text.as_bytes());
    }
    let furniture: Vec<_> = (atlas.pages.iter())
        .flat_map(|p| p.furniture.iter().map(move |f| (p.id, f)))
        .collect();
    put_u32(out, furniture.len());
    for (page, f) in furniture {
        put_u32(out, page.0 as usize);
        put_u32(out, f.id.0 as usize);
        for x in f.rect {
            put_f64(out, x);
        }
        put_bytes(out, f.item.kind_id().as_bytes());
        match f.item.frame() {
            Some(frame) => {
                out.push(1);
                put_u32(out, frame.0 as usize);
            }
            None => out.push(0),
        }
        put_bytes(out, f.item.to_json().as_bytes());
    }
    put_u32(out, atlas.pages.len());
    for page in &atlas.pages {
        put_u32(out, page.id.0 as usize);
        out.extend_from_slice(&page.paper);
    }
    let frames: Vec<_> = atlas.pages.iter().flat_map(|p| &p.frames).collect();
    put_u32(out, frames.len());
    for frame in frames {
        put_u32(out, frame.id.0 as usize);
        put_bytes(out, frame.border.to_json().as_bytes());
    }
    put_bytes(out, atlas.grid.to_json().as_bytes());
}

fn flag(r: &mut Reader) -> Option<bool> {
    match r.take::<1>()? {
        [0] => Some(false),
        [1] => Some(true),
        _ => None,
    }
}

type AtlasImage = (
    Vec<MapRecord>,
    Vec<PageRecord>,
    Vec<ProjectionPreset>,
    LayoutGrid,
);

fn atlas(r: &mut Reader) -> Option<AtlasImage> {
    let mut maps = Vec::new();
    for _ in 0..count(r)? {
        let id = MapId(r.u32()?);
        let name = string(r)?;
        let projection = string(r)?;
        let mut hidden = std::collections::BTreeSet::new();
        for _ in 0..count(r)? {
            hidden.insert(LayerId(r.u32()?));
        }
        maps.push(MapRecord {
            id,
            name,
            projection,
            hidden,
            overrides: std::collections::BTreeMap::new(),
            labels: std::collections::BTreeMap::new(),
        });
    }
    let mut pages = Vec::new();
    for _ in 0..count(r)? {
        let id = PageId(r.u32()?);
        let name = string(r)?;
        let (width_mm, height_mm) = (r.f64()?, r.f64()?);
        let units = PageUnits::from_id(&string(r)?)?;
        let mut frames = Vec::new();
        for _ in 0..count(r)? {
            let id = FrameId(r.u32()?);
            let map = MapId(r.u32()?);
            let kind = string(r)?;
            let shape = FrameShape::from_rings(&kind, read_rings(r)?)?;
            frames.push(FrameRecord {
                id,
                map,
                shape,
                center: [r.f64()?, r.f64()?],
                scale: r.f64()?,
                rotation: r.f64()?,
                graticule: Graticule {
                    on: flag(r)?,
                    interval: if flag(r)? { Some(r.f64()?) } else { None },
                    color: r.take::<3>()?,
                    width: r.f64()?,
                    format: CoordFormat::from_id(&string(r)?)?,
                    meridians: EdgeLabels::from_json(&string(r)?)?,
                    parallels: EdgeLabels::from_json(&string(r)?)?,
                },
                neatline: Neatline {
                    on: flag(r)?,
                    color: r.take::<3>()?,
                    width: r.f64()?,
                },
                border: Border::default(),
            });
        }
        pages.push(PageRecord {
            id,
            name,
            width_mm,
            height_mm,
            units,
            paper: [0xff, 0xff, 0xff],
            frames,
            furniture: Vec::new(),
        });
    }
    let mut presets = Vec::new();
    for _ in 0..count(r)? {
        presets.push(ProjectionPreset {
            name: string(r)?,
            projection: string(r)?,
        });
    }
    if !r.0.is_empty() {
        for _ in 0..count(r)? {
            let id = MapId(r.u32()?);
            let layer = LayerId(r.u32()?);
            let text = string(r)?;
            let map = maps.iter_mut().find(|m| m.id == id)?;
            map.overrides.insert(layer, text);
        }
    }
    if !r.0.is_empty() {
        for _ in 0..count(r)? {
            let id = MapId(r.u32()?);
            let layer = LayerId(r.u32()?);
            let text = string(r)?;
            let map = maps.iter_mut().find(|m| m.id == id)?;
            map.labels.insert(layer, text);
        }
    }
    if !r.0.is_empty() {
        for _ in 0..count(r)? {
            let page = PageId(r.u32()?);
            let id = FurnitureId(r.u32()?);
            let rect = [r.f64()?, r.f64()?, r.f64()?, r.f64()?];
            let kind = string(r)?;
            let frame = if flag(r)? {
                Some(FrameId(r.u32()?))
            } else {
                None
            };
            let item = Furniture::from_json(&kind, &string(r)?, frame)?;
            let page = pages.iter_mut().find(|p| p.id == page)?;
            page.furniture.push(FurnitureRecord { id, rect, item });
        }
    }
    if !r.0.is_empty() {
        for _ in 0..count(r)? {
            let id = PageId(r.u32()?);
            let paper = r.take::<3>()?;
            pages.iter_mut().find(|p| p.id == id)?.paper = paper;
        }
        for _ in 0..count(r)? {
            let id = FrameId(r.u32()?);
            let border = Border::from_json(&string(r)?)?;
            let mut frames = pages.iter_mut().flat_map(|p| &mut p.frames);
            let frame = frames.find(|f| f.id == id)?;
            frame.border = border;
        }
    }
    let mut grid = LayoutGrid::default();
    if !r.0.is_empty() {
        grid = LayoutGrid::from_json(&string(r)?)?;
    }
    r.0.is_empty().then_some((maps, pages, presets, grid))
}

/// The raster images of the log get one color for each band, as the
/// rasters of the file do.
pub(crate) fn v14(tx: &Transaction) -> rusqlite::Result<()> {
    let rows: Vec<(i64, u32, Vec<u8>)> = tx
        .prepare(
            "SELECT step_id, id, data FROM vale_undo_record
             WHERE kind = ?1 AND data IS NOT NULL",
        )?
        .query_map([RASTER], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut update = tx.prepare(
        "UPDATE vale_undo_record SET data = ?4 WHERE step_id = ?1 AND kind = ?2 AND id = ?3",
    )?;
    let mut steps = std::collections::BTreeSet::new();
    for (step, id, blob) in rows {
        // A damaged image stays, and an undo of its step gives an error.
        let payload = unpack(&blob);
        let Some(old) = payload.and_then(|p| raster_v13(&mut Reader(&p))) else {
            continue;
        };
        let mut out = Vec::new();
        put_raster(&mut out, &old);
        update.execute(params![step, RASTER, id, pack(&out)])?;
        steps.insert(step);
    }
    for step in steps {
        measure(tx, step)?;
    }
    Ok(())
}

fn put_raster(out: &mut Vec<u8>, raster: &Raster) {
    put_u32(out, raster.id.0 as usize);
    put_u32(out, raster.face_size as usize);
    put_bytes(out, raster.unit.as_bytes());
    put_f64(out, raster.value_min);
    put_f64(out, raster.value_max);
    put_u32(out, raster.base as usize);
    out.extend_from_slice(&raster.opacity.to_le_bytes());
    put_bytes(out, raster.display.id().as_bytes());
    put_u32(out, raster.stops.len());
    for stop in &raster.stops {
        put_f64(out, stop.value);
        out.push(stop.line.on as u8);
        out.extend_from_slice(&stop.line.weight.to_le_bytes());
        out.extend_from_slice(&stop.line.color);
        out.push(stop.line.darker as u8);
        out.extend_from_slice(&stop.line.darken.to_le_bytes());
    }
    put_u32(out, raster.colors.len());
    for color in &raster.colors {
        out.extend_from_slice(color);
    }
    out.extend_from_slice(&raster.smoothing.feature_size.to_le_bytes());
    out.push(raster.smoothing.on as u8);
    out.extend_from_slice(&raster.hillshade.opacity.to_le_bytes());
    out.push(raster.hillshade.on as u8);
    out.push(raster.hillshade.highlights as u8);
    out.extend_from_slice(&raster.hillshade.azimuth.to_le_bytes());
    out.extend_from_slice(&raster.hillshade.altitude.to_le_bytes());
    out.extend_from_slice(&raster.hillshade.exaggeration.to_le_bytes());
    out.extend_from_slice(&raster.hillshade.color);
    out.push(raster.rivers as u8);
    out.push(raster.lines as u8);
    put_bytes(out, raster.blend.id().as_bytes());
}

fn put_image(out: &mut Vec<u8>, image: &Image) {
    put_u32(out, image.id.0 as usize);
    put_u32(out, image.width as usize);
    put_u32(out, image.height as usize);
    put_u32(out, image.levels as usize);
    out.extend_from_slice(&image.opacity.to_le_bytes());
    put_bytes(out, image.blend.id().as_bytes());
}

/// An unknown blend mode blends as normal.
fn read_image(r: &mut Reader) -> Option<Image> {
    let image = Image {
        id: ImageId(r.u32()?),
        width: r.u32()?,
        height: r.u32()?,
        levels: r.u32()?,
        opacity: clamp_opacity(f32::from_le_bytes(r.take()?)),
        blend: BlendMode::from_id(&string(r)?).unwrap_or_default(),
    };
    r.0.is_empty().then_some(image)
}

/// A raster image of format version 13 with the stops `old`.
#[cfg(test)]
fn put_raster_v13(out: &mut Vec<u8>, raster: &Raster, old: &[V13Stop]) {
    put_u32(out, raster.id.0 as usize);
    put_u32(out, raster.face_size as usize);
    put_bytes(out, raster.unit.as_bytes());
    put_f64(out, raster.value_min);
    put_f64(out, raster.value_max);
    put_u32(out, raster.base as usize);
    out.extend_from_slice(&raster.opacity.to_le_bytes());
    put_bytes(out, raster.display.id().as_bytes());
    put_u32(out, old.len());
    for stop in old {
        put_f64(out, stop.value);
        out.extend_from_slice(&stop.below);
        out.extend_from_slice(&stop.above);
    }
    for stop in old {
        out.push(stop.line.on as u8);
        out.extend_from_slice(&stop.line.weight.to_le_bytes());
        out.extend_from_slice(&stop.line.color);
        out.push(stop.line.darker as u8);
        out.extend_from_slice(&stop.line.darken.to_le_bytes());
    }
    out.extend_from_slice(&raster.smoothing.feature_size.to_le_bytes());
    out.push(raster.smoothing.on as u8);
}

/// The packed raster image of format version 13 with the stops `old`.
#[cfg(test)]
pub(crate) fn image_v13(raster: &Raster, old: &[V13Stop]) -> Vec<u8> {
    let mut out = Vec::new();
    put_raster_v13(&mut out, raster, old);
    pack(&out)
}

fn line(r: &mut Reader) -> Option<StopLine> {
    let [on]: [u8; 1] = r.take()?;
    let weight = f32::from_le_bytes(r.take()?);
    let color = r.take()?;
    let [darker]: [u8; 1] = r.take()?;
    Some(StopLine {
        on: on != 0,
        weight,
        color,
        darker: darker != 0,
        darken: f32::from_le_bytes(r.take()?),
    })
}

fn smoothing(r: &mut Reader) -> Option<Smoothing> {
    let feature_size = clamp_feature_size(f32::from_le_bytes(r.take()?));
    let [on]: [u8; 1] = r.take()?;
    Some(Smoothing {
        on: on != 0,
        feature_size,
    })
}

/// The length of the hillshade at the end of a raster image.
#[cfg(test)]
const HILLSHADE_LEN: usize = 4 + 2 + 3 * 4 + 3;

/// An image of an early build of version 15 ends after the switch, or after
/// the highlights, and gets the defaults of the rest.
fn hillshade(r: &mut Reader) -> Option<Hillshade> {
    let mut h = Hillshade {
        opacity: f32::from_le_bytes(r.take()?),
        ..Hillshade::default()
    };
    let [on]: [u8; 1] = r.take()?;
    h.on = on != 0;
    if !r.0.is_empty() {
        let [highlights]: [u8; 1] = r.take()?;
        h.highlights = highlights != 0;
    }
    if !r.0.is_empty() {
        h.azimuth = f32::from_le_bytes(r.take()?);
        h.altitude = f32::from_le_bytes(r.take()?);
        h.exaggeration = f32::from_le_bytes(r.take()?);
        h.color = r.take()?;
    }
    Some(h.clamped())
}

/// An unknown display gives bands. An image of format version 14 ends after
/// the smoothing, and gets the default hillshade. An image of a version
/// before 17 has no switch of the rivers, and shows them. An image of a
/// version before 18 has no switch of the lines, and shows them. An image of
/// a version before 23 has no blend mode, and blends as normal.
fn raster(r: &mut Reader) -> Option<Raster> {
    let id = RasterId(r.u32()?);
    let face_size = r.u32()?;
    let unit = string(r)?;
    let (value_min, value_max) = (r.f64()?, r.f64()?);
    let base = u16::try_from(r.u32()?).ok()?;
    let opacity = clamp_opacity(f32::from_le_bytes(r.take()?));
    let display = RasterDisplay::from_id(&string(r)?).unwrap_or_default();
    let stops = (0..count(r)?)
        .map(|_| {
            Some(Stop {
                value: r.f64()?,
                line: line(r)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let colors = (0..count(r)?)
        .map(|_| r.take())
        .collect::<Option<Vec<_>>>()?;
    let smoothing = smoothing(r)?;
    let hillshade = match r.0.is_empty() {
        true => Hillshade::default(),
        false => hillshade(r)?,
    };
    let mut rivers = true;
    if !r.0.is_empty() {
        let [on]: [u8; 1] = r.take()?;
        rivers = on != 0;
    }
    let mut lines = true;
    if !r.0.is_empty() {
        let [on]: [u8; 1] = r.take()?;
        lines = on != 0;
    }
    let blend = match r.0.is_empty() {
        true => BlendMode::Normal,
        false => BlendMode::from_id(&string(r)?).unwrap_or_default(),
    };
    r.0.is_empty().then_some(Raster {
        id,
        opacity,
        display,
        blend,
        face_size,
        unit,
        value_min,
        value_max,
        base,
        stops,
        colors,
        smoothing,
        hillshade,
        rivers,
        lines,
    })
}

/// A raster image of format version 11, 12, or 13, with its stops and band
/// colors as `gpkg::v14_bands` gives them. An unknown display gives bands.
/// An image of version 11 has no lines. An image of version 11 or 12 has no
/// smoothing, and gets the default. Each gets the default hillshade.
fn raster_v13(r: &mut Reader) -> Option<Raster> {
    let id = RasterId(r.u32()?);
    let face_size = r.u32()?;
    let unit = string(r)?;
    let (value_min, value_max) = (r.f64()?, r.f64()?);
    let base = u16::try_from(r.u32()?).ok()?;
    let opacity = clamp_opacity(f32::from_le_bytes(r.take()?));
    let display = RasterDisplay::from_id(&string(r)?).unwrap_or_default();
    let mut old = (0..count(r)?)
        .map(|_| {
            Some(V13Stop {
                value: r.f64()?,
                below: r.take()?,
                above: r.take()?,
                line: StopLine::default(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let mut smooth = Smoothing::default();
    if !r.0.is_empty() {
        for stop in &mut old {
            stop.line = line(r)?;
        }
    }
    if !r.0.is_empty() {
        smooth = smoothing(r)?;
    }
    let (stops, colors) = v14_bands(&unit, value_min, value_max, &old);
    r.0.is_empty().then_some(Raster {
        id,
        opacity,
        display,
        face_size,
        unit,
        value_min,
        value_max,
        base,
        stops,
        colors,
        smoothing: smooth,
        hillshade: Hillshade::default(),
        rivers: true,
        lines: true,
        blend: BlendMode::Normal,
    })
}

const KINDS: [GeometryKind; 3] = [
    GeometryKind::Point,
    GeometryKind::Line,
    GeometryKind::Polygon,
];

/// The kind byte of the image of a raster layer. The ID of the raster comes
/// after it.
const RASTER_KIND: u8 = KINDS.len() as u8;
/// The kind byte of the image of an image layer. The ID of the image comes
/// after it.
const IMAGE_KIND: u8 = RASTER_KIND + 1;

fn put_layer(out: &mut Vec<u8>, position: usize, layer: &Layer) {
    put_u32(out, position);
    put_u32(out, layer.id.0 as usize);
    put_bytes(out, layer.name.as_bytes());
    match layer.kind {
        LayerKind::Vector(kind) => {
            out.push(KINDS.iter().position(|k| *k == kind).expect("a kind") as u8);
        }
        LayerKind::Raster(raster) => {
            out.push(RASTER_KIND);
            put_u32(out, raster.0 as usize);
        }
        LayerKind::Image(image) => {
            out.push(IMAGE_KIND);
            put_u32(out, image.0 as usize);
        }
    }
    out.push(layer.source.is_some() as u8);
    put_bytes(out, layer.source.as_deref().unwrap_or_default().as_bytes());
    put_u32(out, layer.fields.len());
    for field in &layer.fields {
        put_bytes(out, field.name.as_bytes());
    }
    put_u32(out, layer.features.len());
    for feature in &layer.features {
        out.extend_from_slice(feature.uuid().as_bytes());
        put_bytes(out, &encode_geometry(&feature.geometry));
        put_u32(out, feature.attributes.len());
        for (name, value) in &feature.attributes {
            put_bytes(out, name.as_bytes());
            put_value(out, value);
        }
    }
    out.push(layer.visible as u8);
    for field in &layer.fields {
        let kind = FieldType::ALL.iter().position(|k| *k == field.kind);
        out.push(kind.expect("a field type") as u8);
        put_value(out, &field.default);
    }
    out.push(layer.style.is_some() as u8);
    put_bytes(out, layer.style.as_deref().unwrap_or_default().as_bytes());
}

fn put_value(out: &mut Vec<u8>, value: &Value) {
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

fn value(r: &mut Reader) -> Option<Value> {
    let [tag] = r.take()?;
    Some(match tag {
        0 => Value::Null,
        1 => Value::Bool(r.take::<1>()? == [1]),
        2 => Value::Number(r.f64()?),
        3 => Value::Text(string(r)?),
        _ => return None,
    })
}

/// A layer and its place in the project. An image of a format before
/// version 20 ends before the style, and an image before version 19 ends
/// after the visibility. Its fields get their types as an
/// import gives them, and the values turn into values of those types.
fn layer(r: &mut Reader) -> Option<(usize, Layer)> {
    let position = count(r)?;
    let id = LayerId(r.u32()?);
    let name = string(r)?;
    let kind = match r.take()? {
        [RASTER_KIND] => LayerKind::Raster(RasterId(r.u32()?)),
        [IMAGE_KIND] => LayerKind::Image(ImageId(r.u32()?)),
        [kind] => LayerKind::Vector(*KINDS.get(kind as usize)?),
    };
    let [has_source] = r.take()?;
    let source = string(r)?;
    let source = (has_source == 1).then_some(source);
    let names: Vec<String> = (0..count(r)?).map(|_| string(r)).collect::<Option<_>>()?;
    let mut features = Vec::new();
    for _ in 0..count(r)? {
        let uuid = Uuid::from_bytes(r.take()?);
        let LayerKind::Vector(kind) = kind else {
            return None;
        };
        let geometry = decode_geometry(bytes(r)?, kind)?;
        let mut attributes = std::collections::BTreeMap::new();
        for _ in 0..count(r)? {
            let name = string(r)?;
            attributes.insert(name, value(r)?);
        }
        features.push(Feature::with_uuid(uuid, geometry, attributes));
    }
    let visible = r.take::<1>()? == [1];
    let fields = match r.0.is_empty() {
        true => {
            let inferred = infer_fields(&mut features);
            names
                .into_iter()
                .map(|name| {
                    let kind = inferred.iter().find(|f| f.name == name).map(|f| f.kind);
                    Field::new(name, kind.unwrap_or(FieldType::Text))
                })
                .collect()
        }
        false => names
            .into_iter()
            .map(|name| {
                let [kind] = r.take()?;
                Some(Field {
                    name,
                    kind: *FieldType::ALL.get(kind as usize)?,
                    default: value(r)?,
                })
            })
            .collect::<Option<_>>()?,
    };
    let style = match r.0.is_empty() {
        true => None,
        false => {
            let [has_style] = r.take()?;
            let style = string(r)?;
            (has_style == 1).then_some(style)
        }
    };
    let layer = Layer {
        id,
        name,
        kind,
        visible,
        source,
        fields,
        features,
        style,
    };
    r.0.is_empty().then_some((position, layer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{
        EdgeOrientation, LegendEntry, LegendItem, NorthArrowItem, ScaleBarItem, TextItem,
    };

    #[test]
    fn an_atlas_image_keeps_the_frames() {
        let mut before = Atlas::new(Units::Metric);
        before.maps[0].hidden.insert(LayerId(3));
        before.maps[0].name = "Wide".into();
        let second_map = before.add_map();
        let text = r#"{"/label/size":20.0}"#.to_string();
        before.maps[0].overrides.insert(LayerId(3), text.clone());
        before
            .map_mut(second_map)
            .unwrap()
            .overrides
            .insert(LayerId(4), text);
        let classes = r#"[{"name":"Towns"}]"#.to_string();
        before.maps[0].labels.insert(LayerId(3), classes);
        before.maps[0].labels.insert(LayerId(5), "[]".into());
        let first = before.pages[0].id;
        let second = before.add_page(Some(first), 420.0, 297.0, PageUnits::In);
        before.page_mut(second).unwrap().name = "North".into();
        before.page_mut(second).unwrap().paper = [0xf3, 0xef, 0xdf];
        let frame = |shape, graticule: Graticule| FrameRecord {
            id: FrameId(0),
            map: MapId(0),
            shape,
            center: [-12.5, 33.25],
            scale: 25_000_000.0,
            rotation: 271.5,
            neatline: if graticule.on {
                Neatline::default()
            } else {
                Neatline {
                    on: false,
                    color: [0xaa, 0x10, 0x20],
                    width: 3.5,
                }
            },
            border: Border {
                on: !graticule.on,
                style: crate::atlas::BorderStyle::Graduated,
                width_mm: 3.5,
                stroke: true,
                grid: true,
                ..Border::default()
            },
            graticule,
        };
        let manual = Graticule {
            on: false,
            interval: Some(7.5),
            color: [0x12, 0x34, 0x56],
            width: 1.25,
            format: CoordFormat::Decimal,
            parallels: EdgeLabels {
                outside: true,
                orientation: EdgeOrientation::Horizontal,
                ..EdgeLabels::default()
            },
            ..Graticule::default()
        };
        let rect = FrameShape::Rectangle([10.0, 10.0, 200.0, 150.0]);
        let ring = vec![
            [0.0, 0.0],
            [90.0, 0.0],
            [45.0, 20.0],
            [90.0, 60.0],
            [0.0, 60.0],
        ];
        let hole = vec![[5.0, 5.0], [15.0, 5.0], [15.0, 15.0]];
        let polygon = FrameShape::Polygon(vec![ring, hole]);
        before
            .add_frame(second, frame(rect, Graticule::default()))
            .unwrap();
        before.add_frame(second, frame(polygon, manual)).unwrap();

        let linked = before.pages[1].frames[1].id;
        let legend = LegendItem {
            frame: Some(linked),
            entries: [((LayerId(3), "Large".to_string()), LegendEntry::default())].into(),
            ..LegendItem::default()
        };
        let items = [
            Furniture::Text(TextItem::default()),
            Furniture::Legend(legend),
            Furniture::ScaleBar(ScaleBarItem::default()),
            Furniture::NorthArrow(NorthArrowItem {
                frame: Some(linked),
                ..NorthArrowItem::default()
            }),
        ];
        for (i, item) in items.into_iter().enumerate() {
            let rect = [i as f64, 2.0, 30.5, 40.0];
            let page = if i == 0 { first } else { second };
            before.add_furniture(page, rect, item).unwrap();
        }

        before.save_preset("Home", "+proj=laea +lon_0=10 +lat_0=50");
        before.grid = LayoutGrid {
            on: true,
            spacing_mm: 12.5,
            color: [0x10, 0x20, 0x30],
            opacity: 0.5,
        };
        let mut out = Vec::new();
        put_atlas(&mut out, &before);
        let image = atlas(&mut Reader(&out)).unwrap();
        assert_eq!(
            image,
            (
                before.maps.clone(),
                before.pages.clone(),
                before.presets.clone(),
                before.grid.clone(),
            )
        );
        out.pop();
        assert_eq!(atlas(&mut Reader(&out)), None);
    }

    #[test]
    fn an_atlas_image_without_overrides_reads() {
        let before = Atlas::new(Units::Metric);
        let mut out = Vec::new();
        put_atlas(&mut out, &before);
        // The paper of one page, no frames, and the layout grid.
        out.truncate(out.len() - 15 - 4 - before.grid.to_json().len());
        // An image of an older build ends after the projections.
        assert_eq!(out.split_off(out.len() - 12), [0; 12]);
        let image = atlas(&mut Reader(&out)).unwrap();
        assert_eq!(image.0, before.maps);
    }

    #[test]
    fn an_atlas_image_without_label_classes_reads() {
        let mut before = Atlas::new(Units::Metric);
        let text = r#"{"/label/size":20.0}"#.to_string();
        before.maps[0].overrides.insert(LayerId(3), text);
        let mut out = Vec::new();
        put_atlas(&mut out, &before);
        // The paper of one page, no frames, and the layout grid.
        out.truncate(out.len() - 15 - 4 - before.grid.to_json().len());
        // An image of an older build ends after the overrides.
        assert_eq!(out.split_off(out.len() - 8), [0; 8]);
        let image = atlas(&mut Reader(&out)).unwrap();
        assert_eq!(image.0, before.maps);
    }

    #[test]
    fn an_atlas_image_without_furniture_reads() {
        let mut before = Atlas::new(Units::Metric);
        before.maps[0].labels.insert(LayerId(3), "[]".into());
        let mut out = Vec::new();
        put_atlas(&mut out, &before);
        out.truncate(out.len() - 4 - before.grid.to_json().len());
        // An image of an older build ends after the label classes. The rest
        // is the furniture count, one page with white paper, and no frames.
        let tail = out.split_off(out.len() - 19);
        assert_eq!(tail[..8], [0, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(tail[12..], [0xff, 0xff, 0xff, 0, 0, 0, 0]);
        let image = atlas(&mut Reader(&out)).unwrap();
        assert_eq!((image.0, image.1), (before.maps, before.pages));
    }

    #[test]
    fn an_atlas_image_without_paper_and_borders_reads() {
        let mut before = Atlas::new(Units::Metric);
        let page = before.pages[0].id;
        let frame = FrameRecord {
            id: FrameId(0),
            map: MapId(0),
            shape: FrameShape::Rectangle([10.0, 10.0, 100.0, 80.0]),
            center: [0.0, 0.0],
            scale: 1e7,
            rotation: 0.0,
            graticule: Graticule::default(),
            neatline: Neatline::default(),
            border: Border::default(),
        };
        let id = before.add_frame(page, frame).unwrap();
        let mut out = Vec::new();
        put_atlas(&mut out, &before);
        // An image of an older build ends after the furniture.
        let frame = before.frame(id).unwrap();
        let border = frame.border.to_json();
        let grid = before.grid.to_json();
        let tail = 4 + 7 + 4 + 4 + (4 + border.len()) + (4 + grid.len());
        out.truncate(out.len() - tail);
        let image = atlas(&mut Reader(&out)).unwrap();
        assert_eq!(image.1, before.pages);
    }

    #[test]
    fn a_world_image_keeps_the_seed_the_wind_and_the_rain_shadow() {
        let before = World {
            name: "Aerth".to_string(),
            radius_km: 4321.125,
            units: Units::Imperial,
            seed: 0xDEAD_BEEF,
            wind_deg: 90.0,
            rain_shadow: 0.6,
        };
        let mut out = Vec::new();
        put_world(&mut out, &before);
        assert_eq!(world(&mut Reader(&out)), Some(before));
    }

    #[test]
    fn a_world_image_of_version_31_gets_a_west_wind_and_no_rain_shadow() {
        let mut out = Vec::new();
        put_bytes(&mut out, b"Old");
        put_f64(&mut out, 1234.5);
        put_bytes(&mut out, Units::Imperial.id().as_bytes());
        out.extend_from_slice(&7u32.to_le_bytes());
        let old = world(&mut Reader(&out)).unwrap();
        assert_eq!((old.seed, old.wind_deg, old.rain_shadow), (7, 270.0, 0.0));
    }

    #[test]
    fn a_layer_image_keeps_the_visibility() {
        let mut project = Project::new(World::default());
        let id = project.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        project.set_layer_visible(id, false);
        let hidden = project.layer(id).unwrap();
        let mut out = Vec::new();
        put_layer(&mut out, 0, hidden);
        assert_eq!(layer(&mut Reader(&out)), Some((0, hidden.clone())));
        out.pop();
        assert_eq!(layer(&mut Reader(&out)), None);
    }

    #[test]
    fn a_layer_image_keeps_the_style() {
        const STYLE: &str = r#"{"width":2}"#;
        let mut project = Project::new(World::default());
        project.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        project.layers[0].style = Some(STYLE.into());
        let styled = &project.layers[0];
        let mut out = Vec::new();
        put_layer(&mut out, 0, styled);
        assert_eq!(layer(&mut Reader(&out)), Some((0, styled.clone())));
        // An image of version 19 ends before the style.
        out.truncate(out.len() - 5 - STYLE.len());
        let old = Layer {
            style: None,
            ..styled.clone()
        };
        assert_eq!(layer(&mut Reader(&out)), Some((0, old)));
    }

    #[test]
    fn a_layer_image_keeps_the_type_and_the_default_of_each_field() {
        let mut project = Project::new(World::default());
        let id = project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        let fields = [
            Field {
                default: Value::Text("?".into()),
                ..Field::new("name", FieldType::Text)
            },
            Field {
                default: Value::Number(3.0),
                ..Field::new("pop", FieldType::Integer)
            },
            Field::new("area", FieldType::Real),
            Field {
                default: Value::Bool(true),
                ..Field::new("capital", FieldType::Bool)
            },
        ];
        for field in fields {
            assert!(project.add_field(id, field));
        }
        let point = Feature::new(crate::Geometry::Points(vec![]), Default::default());
        assert!(project.add_feature(id, point));
        let towns = project.layer(id).unwrap();
        let mut out = Vec::new();
        put_layer(&mut out, 0, towns);
        assert_eq!(layer(&mut Reader(&out)), Some((0, towns.clone())));
        // An image that ends inside the schema is not valid.
        out.pop();
        assert_eq!(layer(&mut Reader(&out)), None);
    }

    #[test]
    fn a_layer_image_of_version_18_gets_the_types_of_an_import() {
        let attributes = |pairs: [(&str, Value); 3]| {
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
        };
        let point = |pairs| Feature::new(crate::Geometry::Points(vec![]), attributes(pairs));
        let mut project = Project::new(World::default());
        let id = project.add_layer(
            "Towns".into(),
            GeometryKind::Point,
            None,
            vec![
                point([
                    ("any", Value::Number(1.0)),
                    ("flag", Value::Bool(true)),
                    ("pop", Value::Number(2.0)),
                ]),
                point([
                    ("any", Value::Bool(false)),
                    ("flag", Value::Null),
                    ("pop", Value::Number(0.5)),
                ]),
            ],
        );
        let towns = project.layer(id).unwrap();
        // An old image ends after the visibility, and its values have
        // their own types.
        let mut old = towns.clone();
        old.features[0]
            .attributes
            .insert("any".into(), Value::Number(1.0));
        old.features[1]
            .attributes
            .insert("any".into(), Value::Bool(false));
        let mut out = Vec::new();
        put_layer(&mut out, 2, &old);
        // Each field has a type byte and a null default, and the style
        // has a presence byte and a length.
        out.truncate(out.len() - 2 * old.fields.len() - 5);
        assert_eq!(layer(&mut Reader(&out)), Some((2, towns.clone())));
        let kinds: Vec<FieldType> = towns.fields.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [FieldType::Text, FieldType::Bool, FieldType::Real]);
    }

    #[test]
    fn the_image_of_a_raster_layer_keeps_the_id_of_its_raster() {
        let mut project = Project::new(World::default());
        project.add_raster("Height".into(), 1000, "m".into(), 0.0, 1.0);
        let id = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        let rain = project.raster_layer(id).unwrap().id;
        project.set_layer_visible(rain, false);
        let hidden = project.layer(rain).unwrap();
        assert_eq!(hidden.kind, LayerKind::Raster(id));
        let mut out = Vec::new();
        put_layer(&mut out, 1, hidden);
        assert_eq!(layer(&mut Reader(&out)), Some((1, hidden.clone())));
    }

    /// A raster with values, an opacity, a display, smoothing, stops, and
    /// band colors that are not the defaults.
    fn rain() -> Raster {
        let mut project = Project::new(World::default());
        let id = project.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 5000.0);
        let rain = project.raster_mut(id).unwrap();
        rain.opacity = 0.375;
        rain.display = RasterDisplay::Grey;
        rain.smoothing = Smoothing {
            on: false,
            feature_size: 3.25,
        };
        rain.stops = vec![
            Stop {
                value: 0.0,
                line: StopLine::default(),
            },
            Stop {
                value: 250.5,
                line: StopLine {
                    on: true,
                    weight: 2.5,
                    color: [10, 11, 12],
                    darker: true,
                    darken: 0.25,
                },
            },
            Stop {
                value: 5000.0,
                line: StopLine::default(),
            },
        ];
        rain.colors = vec![[1, 2, 3], [4, 5, 6]];
        rain.rivers = false;
        rain.lines = false;
        rain.clone()
    }

    #[test]
    fn a_raster_image_keeps_the_values_and_the_opacity() {
        let mut rain = rain();
        rain.hillshade = Hillshade {
            on: true,
            opacity: 0.375,
            highlights: false,
            azimuth: 45.0,
            altitude: 60.0,
            exaggeration: 3.0,
            color: [30, 60, 90],
        };
        rain.blend = BlendMode::SoftLight;
        let mut out = Vec::new();
        put_raster(&mut out, &rain);
        assert_eq!(raster(&mut Reader(&out)), Some(rain.clone()));
        // An image of version 22 ends after the switch of the lines, and it
        // blends as normal.
        out.truncate(out.len() - (4 + rain.blend.id().len()));
        let normal = Raster {
            blend: BlendMode::Normal,
            ..rain.clone()
        };
        assert_eq!(raster(&mut Reader(&out)), Some(normal.clone()));
        let rain = normal;
        // An image of version 17 ends after the switch of the rivers, and it
        // shows the lines.
        out.pop();
        let lines = Raster {
            lines: true,
            ..rain.clone()
        };
        assert_eq!(raster(&mut Reader(&out)), Some(lines.clone()));
        // An image of version 16 ends after the hillshade, and it shows the
        // rivers.
        out.pop();
        let shown = Raster {
            rivers: true,
            ..lines
        };
        assert_eq!(raster(&mut Reader(&out)), Some(shown));
        out.pop();
        assert_eq!(raster(&mut Reader(&out)), None);

        // An image of version 14 ends after the smoothing, and it gets the
        // default hillshade.
        out.truncate(out.len() + 1 - HILLSHADE_LEN);
        let old = raster(&mut Reader(&out)).unwrap();
        assert_eq!(old.hillshade, Hillshade::default());
        assert_eq!(old.smoothing, rain.smoothing);
    }

    #[test]
    fn a_raster_image_of_an_early_build_gets_the_default_light() {
        let mut rain = rain();
        rain.hillshade = Hillshade {
            on: true,
            opacity: 0.375,
            highlights: false,
            azimuth: 90.0,
            ..Hillshade::default()
        };
        let mut full = Vec::new();
        put_raster(&mut full, &rain);
        full.truncate(full.len() - 2 - (4 + rain.blend.id().len()));
        let light = HILLSHADE_LEN - 4 - 2;
        // The image ends after the highlights.
        let out = &full[..full.len() - light];
        let shade = raster(&mut Reader(out)).unwrap().hillshade;
        let want = Hillshade {
            azimuth: Hillshade::AZIMUTH_DEFAULT,
            ..rain.hillshade
        };
        assert_eq!(shade, want);
        // The image ends after the switch.
        let out = &full[..full.len() - light - 1];
        let shade = raster(&mut Reader(out)).unwrap().hillshade;
        assert!(shade.highlights);
        assert_eq!((shade.on, shade.opacity), (true, 0.375));
        // An image that ends inside the light is not valid.
        assert_eq!(raster(&mut Reader(&full[..full.len() - 1])), None);
    }

    #[test]
    fn an_undo_image_keeps_the_band_colors() {
        let mut rain = rain();
        rain.colors = vec![[255, 0, 128], [7, 7, 7]];
        let mut out = Vec::new();
        put_raster(&mut out, &rain);
        assert_eq!(raster(&mut Reader(&out)).unwrap().colors, rain.colors);
        // An image with a color that ends early is not valid.
        out.truncate(out.len() - 2 - HILLSHADE_LEN - 5 - 1);
        assert_eq!(raster(&mut Reader(&out)), None);
    }

    #[test]
    fn a_raster_image_of_version_13_gets_band_colors() {
        let rain = rain();
        let old = [
            V13Stop {
                value: 0.0,
                below: [1, 2, 3],
                above: [1, 2, 3],
                line: StopLine::default(),
            },
            V13Stop {
                value: 250.5,
                below: [4, 5, 6],
                above: [4, 5, 6],
                line: rain.stops[1].line,
            },
            V13Stop {
                value: 5000.0,
                below: [7, 8, 9],
                above: [7, 8, 9],
                line: StopLine::default(),
            },
        ];
        let mut out = Vec::new();
        put_raster_v13(&mut out, &rain, &old);
        let shown = Raster {
            rivers: true,
            lines: true,
            ..rain.clone()
        };
        assert_eq!(raster_v13(&mut Reader(&out)), Some(shown));
        out.pop();
        assert_eq!(raster_v13(&mut Reader(&out)), None);

        // An image of version 12 ends after the lines, and it gets the
        // default smoothing.
        out.truncate(out.len() + 1 - 5);
        let v12 = raster_v13(&mut Reader(&out)).unwrap();
        assert_eq!(v12.smoothing, Smoothing::default());
        assert_eq!((&v12.stops, &v12.colors), (&rain.stops, &rain.colors));

        // An image of version 11 ends after the stops, and its stops have
        // no line.
        out.truncate(out.len() - 3 * 13);
        let v11 = raster_v13(&mut Reader(&out)).unwrap();
        assert!(v11.stops.iter().all(|s| s.line == StopLine::default()));
        assert_eq!(v11.colors, rain.colors);
        assert_eq!(v11.smoothing, Smoothing::default());

        // With no stops, an image of version 12 is one of version 13 with
        // no smoothing.
        let mut out = Vec::new();
        put_raster_v13(&mut out, &rain, &[]);
        assert!(!raster_v13(&mut Reader(&out)).unwrap().smoothing.on);
        out.truncate(out.len() - 5);
        let old = raster_v13(&mut Reader(&out)).unwrap();
        assert_eq!(old.smoothing, Smoothing::default());
        assert!(old.stops.is_empty() && old.colors.is_empty());
    }

    #[test]
    fn a_world_image_of_version_3_gets_a_seed_of_zero() {
        let mut out = Vec::new();
        put_bytes(&mut out, b"Old");
        put_f64(&mut out, 1234.5);
        let old = world(&mut Reader(&out)).unwrap();
        assert_eq!(
            (old.name.as_str(), old.radius_km, old.seed),
            ("Old", 1234.5, 0)
        );
    }
}
