//! The export of the heightmap to an equirectangular GeoTIFF file, on a
//! worker thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;

use vale_import::geotiff::{self, GeoTiff};
use vale_store::{Raster, World};
use vale_terrain::{EquirectGrid, Heightmap};

use super::import::file_name;

/// What an export wrote. The size is that of the image.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    /// The file name with its extension.
    pub name: String,
    pub path: PathBuf,
    pub width: usize,
    pub height: usize,
}

impl Exported {
    /// The text that tells the user what the export did.
    pub fn message(&self) -> String {
        let Exported {
            name,
            width,
            height,
            ..
        } = self;
        format!("Exported {name} ({width} x {height})")
    }
}

/// The result of an export. The error is a text for the user.
pub type ExportResult = Result<Exported, String>;

/// Writes the heightmap to a GeoTIFF file. The image has 4 pixels across
/// for each texel along one side of a face, and it is half as tall. The call
/// returns after the file is complete. `progress` gets the count of the rows
/// that are done.
///
/// With no raster record, the levels are from -6000 m to 6000 m.
pub fn write_heightmap(
    map: &Heightmap,
    world: &World,
    raster: Option<&Raster>,
    path: &Path,
    progress: &(dyn Fn(u32) + Sync),
) -> ExportResult {
    let grid = EquirectGrid::for_face_size(map.face_size());
    let (width, height) = (grid.width(), grid.height());
    let tags = GeoTiff::new(world, raster, width as u32, height as u32);
    let rows = |row: usize, out: &mut [u16]| map.equirect_row(&grid, row, out);
    geotiff::write_file(path, &tags, &rows, progress).map_err(|e| e.to_string())?;
    Ok(Exported {
        name: file_name(path),
        path: path.to_path_buf(),
        width,
        height,
    })
}

/// An export that runs on a worker thread. A dropped job writes its file to
/// the end.
pub(super) struct Job {
    pub name: String,
    rx: mpsc::Receiver<ExportResult>,
    /// The count of the rows that the file holds.
    rows: Arc<AtomicU32>,
    /// The count of the rows of the image.
    height: u32,
}

impl Job {
    /// Starts the worker. `map` is a snapshot, so a later edit of the
    /// heightmap does not change the file.
    pub fn start(map: Heightmap, world: World, raster: Option<Raster>, path: PathBuf) -> Job {
        let name = file_name(&path);
        let height = 2 * map.face_size() as u32;
        let rows = Arc::new(AtomicU32::new(0));
        let (tx, rx) = mpsc::sync_channel(1);
        let done = rows.clone();
        let run = move || {
            let progress = |rows: u32| done.store(rows, Ordering::Relaxed);
            let result = write_heightmap(&map, &world, raster.as_ref(), &path, &progress);
            let _ = tx.send(result);
        };
        let spawned = std::thread::Builder::new()
            .name("vale-export".to_string())
            .spawn(run);
        // With no thread, the channel has no sender, and the job fails.
        drop(spawned);
        Job {
            name,
            rx,
            rows,
            height,
        }
    }

    /// The text that tells the user how far the export is.
    pub fn progress(&self) -> String {
        let rows = u64::from(self.rows.load(Ordering::Relaxed));
        let percent = (rows * 100 / u64::from(self.height.max(1))).min(100);
        format!("Exporting {}: {percent}%", self.name)
    }

    /// The result of the worker, or `None` while the worker runs. `block`:
    /// waits for the result.
    pub fn result(&mut self, block: bool) -> Option<ExportResult> {
        let stopped = || Err(format!("cannot export {}: the export stopped", self.name));
        if block {
            return Some(self.rx.recv().unwrap_or_else(|_| stopped()));
        }
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(stopped()),
        }
    }
}
