//! The state of a model run in the world file.
//!
//! `isotherm_run` holds the face size, the step, the model version, and the
//! month of the sums. `isotherm_run_grid` holds the temperature, the ice,
//! and the sums of the month, as deflated f64 values. `isotherm_field` holds
//! each filled month of each output, as deflated f32 values.

use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use isotherm_fields::{Field, Means, Output, Partial, cells};
use isotherm_sim::{MODEL_VERSION, RunState, STEPS_PER_YEAR};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::world_file::{StoreError, corrupt};

/// The run tables, for the migration step that adds them.
pub(crate) const RUN_TABLES: &str = "
    CREATE TABLE isotherm_run (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      face_size INTEGER NOT NULL,
      steps_per_year INTEGER NOT NULL,
      step INTEGER NOT NULL,
      model_version INTEGER NOT NULL,
      partial_month INTEGER NOT NULL,
      partial_count INTEGER NOT NULL);
    CREATE TABLE isotherm_run_grid (
      name TEXT PRIMARY KEY,
      data BLOB NOT NULL);
    CREATE TABLE isotherm_field (
      output TEXT NOT NULL,
      period INTEGER NOT NULL CHECK (period BETWEEN 0 AND 11),
      data BLOB NOT NULL,
      PRIMARY KEY (output, period));";

/// A second connection to a world file that only writes the run, for the
/// save when the app goes to the background.
pub struct RunFile {
    conn: Connection,
}

impl RunFile {
    pub fn open(path: impl AsRef<Path>) -> Result<RunFile, StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        Ok(RunFile { conn })
    }

    pub fn save_run(&mut self, state: &RunState) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        write_run(&tx, state)?;
        tx.commit()?;
        Ok(())
    }
}

/// Deletes the run.
pub(crate) fn clear_run(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DELETE FROM isotherm_run;
         DELETE FROM isotherm_run_grid;
         DELETE FROM isotherm_field;",
    )
}

/// Writes the run in place of the run of the file.
pub(crate) fn write_run(tx: &Transaction, state: &RunState) -> Result<(), StoreError> {
    clear_run(tx)?;
    let partial = state.means.partial();
    tx.execute(
        "INSERT INTO isotherm_run VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            state.face_size as i64,
            STEPS_PER_YEAR,
            state.step as i64,
            MODEL_VERSION,
            partial.month as i64,
            partial.count,
        ],
    )?;
    let mut grid = tx.prepare("INSERT INTO isotherm_run_grid VALUES (?1, ?2)")?;
    grid.execute(params!["temperature", deflate_f64(&state.temperature_k)?])?;
    grid.execute(params!["ice_m", deflate_f64(&state.ice_m)?])?;
    for (output, sum) in state.means.outputs().iter().zip(&partial.sums) {
        grid.execute(params![format!("sum:{}", output.key()), deflate_f64(sum)?])?;
    }
    let mut field = tx.prepare("INSERT INTO isotherm_field VALUES (?1, ?2, ?3)")?;
    for (output, f) in state.means.outputs().iter().zip(state.means.fields()) {
        for m in (0..12).filter(|&m| f.filled[m]) {
            field.execute(params![output.key(), m as i64, deflate_f32(&f.months[m])?])?;
        }
    }
    Ok(())
}

/// The run of the file. `None`: the file has no run, or a run of another
/// version of the model or of another year length.
pub(crate) fn read_run(conn: &Connection) -> Result<Option<RunState>, StoreError> {
    let head: Option<(i64, u32, i64, u32, i64, u32)> = conn
        .query_row(
            "SELECT face_size, steps_per_year, step, model_version, partial_month, partial_count
             FROM isotherm_run",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((n, steps_per_year, step, version, month, count)) = head else {
        return Ok(None);
    };
    if version != MODEL_VERSION || steps_per_year != STEPS_PER_YEAR {
        return Ok(None);
    }
    let n = usize::try_from(n).map_err(|_| corrupt("a negative face size"))?;
    let len = cells(n);
    let grid = |name: &str| -> Result<Vec<f64>, StoreError> {
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT data FROM isotherm_run_grid WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| corrupt(format!("no run grid {name}")))?;
        let values = inflate_f64(&blob)?;
        if values.len() != len {
            return Err(corrupt(format!("run grid {name} has the wrong size")));
        }
        Ok(values)
    };
    let outputs = Output::ALL;
    let mut fields = Vec::new();
    let mut sums = Vec::new();
    for output in outputs {
        let mut field = Field::new(n);
        let mut rows = conn.prepare("SELECT period, data FROM isotherm_field WHERE output = ?1")?;
        let rows = rows.query_map([output.key()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (m, blob) = row?;
            let values = inflate_f32(&blob)?;
            let m = m as usize;
            if m >= 12 || values.len() != len {
                return Err(corrupt("a run field has the wrong size"));
            }
            field.months[m] = values;
            field.filled[m] = true;
        }
        if field.filled.iter().all(|&f| f) {
            field.update_year();
        }
        fields.push(field);
        sums.push(grid(&format!("sum:{}", output.key()))?);
    }
    let partial = Partial {
        month: usize::try_from(month).map_err(|_| corrupt("a negative month"))?,
        count,
        sums,
    };
    let means = Means::from_parts(&outputs, fields, partial)
        .ok_or_else(|| corrupt("the run means do not match"))?;
    Ok(Some(RunState {
        face_size: n,
        step: u64::try_from(step).map_err(|_| corrupt("a negative step"))?,
        temperature_k: grid("temperature")?,
        ice_m: grid("ice_m")?,
        means,
    }))
}

fn deflate(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes)?;
    encoder.finish()
}

fn inflate(blob: &[u8]) -> Result<Vec<u8>, StoreError> {
    let mut bytes = Vec::new();
    DeflateDecoder::new(blob).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn deflate_f64(values: &[f64]) -> std::io::Result<Vec<u8>> {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    deflate(&bytes)
}

fn deflate_f32(values: &[f32]) -> std::io::Result<Vec<u8>> {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    deflate(&bytes)
}

fn inflate_f64(blob: &[u8]) -> Result<Vec<f64>, StoreError> {
    let bytes = inflate(blob)?;
    if bytes.len() % 8 != 0 {
        return Err(corrupt("a run grid has a partial value"));
    }
    Ok(bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&c| f64::from_le_bytes(c))
        .collect())
}

fn inflate_f32(blob: &[u8]) -> Result<Vec<f32>, StoreError> {
    let bytes = inflate(blob)?;
    if bytes.len() % 4 != 0 {
        return Err(corrupt("a run field has a partial value"));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| f32::from_le_bytes(c))
        .collect())
}
