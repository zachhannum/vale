//! The import of an image file into an image layer, on a worker thread, and
//! the tiles of the images of a project with no file.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;

use vale_import::pyramid::PyramidBuilder;
use vale_import::rgba::RowReader;
use vale_store::{ImageId, encode_tile};

use crate::globe::import::file_name;

/// The PNG of each tile of the images of a project with no file, by the
/// image, the level, the column, and the row.
pub type ImageTiles = HashMap<(ImageId, u32, u32, u32), Vec<u8>>;

/// One tile of an image: the level, the column, the row, and the PNG.
pub type Tile = (u32, u32, u32, Vec<u8>);

/// The rows that the worker reads at one time.
const STRIP_ROWS: u32 = 256;
/// The tiles that wait for the app. The worker stops while the channel is
/// full.
const CHANNEL_TILES: usize = 32;
/// The most tiles of one step.
const STEP_TILES: usize = 64;

/// What an import read from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageInfo {
    /// The file name with its extension.
    pub name: String,
    /// The file name with no extension, for the name of the layer.
    pub stem: String,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
}

/// The end of the warning about a file that is not twice as wide as it is
/// high.
pub const NOT_WIDE: &str = "is not 2:1, so it is stretched to cover the globe";

impl ImageInfo {
    /// The text that tells the user what the import did, and `true` if the
    /// text has a warning.
    pub fn message(&self) -> (String, bool) {
        let ImageInfo {
            name,
            width,
            height,
            ..
        } = self;
        let mut text = format!("Imported {name} ({width} × {height})");
        let wide = *width == 2 * *height;
        if !wide {
            text.push_str(&format!(". The image {NOT_WIDE}"));
        }
        if !self.has_alpha {
            text.push_str(". The image has no alpha channel, so it has no transparent parts");
        }
        let warning = !wide || !self.has_alpha;
        if warning {
            text.push('.');
        }
        (text, warning)
    }
}

enum Msg {
    Read(ImageInfo),
    Tile(Tile),
    /// The worker read this number of rows.
    Rows(u32),
    Done,
    Failed(String),
}

/// One step of an import.
pub enum ImportStep {
    /// The worker has nothing new.
    Wait,
    /// Tiles for the store.
    Tiles(Vec<Tile>),
    /// Each tile arrived.
    Done(ImageInfo),
    Failed(String),
}

/// An import of an image file that runs on a worker thread. The worker
/// reads the file in strips and makes the tile pyramid, so the main memory
/// never holds the whole image.
pub struct ImageImport {
    /// The ID of the image that the tiles are for.
    pub id: ImageId,
    /// The file name with its extension.
    pub name: String,
    rx: mpsc::Receiver<Msg>,
    info: Option<ImageInfo>,
    rows: u32,
    /// The worker sent its last tile.
    finished: bool,
}

impl ImageImport {
    /// Starts the worker for the image `id`.
    pub fn start(path: PathBuf, id: ImageId) -> ImageImport {
        let name = file_name(&path);
        let (tx, rx) = mpsc::sync_channel(CHANNEL_TILES);
        let worker_name = name.clone();
        let run = move || {
            if let Err(text) = read(&path, worker_name, &tx) {
                let _ = tx.send(Msg::Failed(text));
            }
        };
        let spawned = std::thread::Builder::new()
            .name("vale-image-import".to_string())
            .spawn(run);
        // With no thread, the channel has no sender, and the import fails.
        drop(spawned);
        ImageImport {
            id,
            name,
            rx,
            info: None,
            rows: 0,
            finished: false,
        }
    }

    /// The text that tells the user how far the import is.
    pub fn progress(&self) -> String {
        match &self.info {
            Some(info) if info.height > 0 => {
                let percent = u64::from(self.rows) * 100 / u64::from(info.height);
                format!("Importing {}: {percent}%", self.name)
            }
            _ => format!("Importing {}: reading file", self.name),
        }
    }

    /// Takes the next messages of the worker. `block`: waits for one.
    pub fn next(&mut self, block: bool) -> ImportStep {
        if self.finished {
            return self.done();
        }
        let mut tiles = Vec::new();
        loop {
            let msg = if block && tiles.is_empty() {
                self.rx.recv().ok()
            } else {
                match self.rx.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(mpsc::TryRecvError::Empty) if tiles.is_empty() => return ImportStep::Wait,
                    Err(mpsc::TryRecvError::Empty) => return ImportStep::Tiles(tiles),
                    Err(mpsc::TryRecvError::Disconnected) => None,
                }
            };
            match msg {
                Some(Msg::Read(info)) => self.info = Some(info),
                Some(Msg::Rows(rows)) => self.rows = rows,
                Some(Msg::Tile(tile)) => {
                    tiles.push(tile);
                    if tiles.len() >= STEP_TILES {
                        return ImportStep::Tiles(tiles);
                    }
                }
                // The tiles before the end go to the store first, and the
                // end is the next step.
                Some(Msg::Done) if !tiles.is_empty() => {
                    self.finished = true;
                    return ImportStep::Tiles(tiles);
                }
                Some(Msg::Done) => return self.done(),
                Some(Msg::Failed(text)) => return ImportStep::Failed(text),
                None if !tiles.is_empty() => return ImportStep::Tiles(tiles),
                None => {
                    let text = format!("Cannot import {}: import stopped", self.name);
                    return ImportStep::Failed(text);
                }
            }
        }
    }

    fn done(&mut self) -> ImportStep {
        match self.info.take() {
            Some(info) => ImportStep::Done(info),
            None => ImportStep::Failed(format!("Cannot import {}: no image", self.name)),
        }
    }
}

/// Reads the file and sends its tiles. Returns the text of an error.
fn read(path: &std::path::Path, name: String, tx: &mpsc::SyncSender<Msg>) -> Result<(), String> {
    let mut reader = RowReader::open(path).map_err(|e| e.to_string())?;
    let (width, height) = (reader.width(), reader.height());
    if width == 0 || height == 0 {
        return Err(format!("{name} has no pixels"));
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let info = ImageInfo {
        stem: stem.unwrap_or_else(|| name.clone()),
        name,
        width,
        height,
        has_alpha: reader.has_alpha(),
    };
    if tx.send(Msg::Read(info)).is_err() {
        return Ok(());
    }
    // A send fails when the app dropped the import.
    let stopped = Rc::new(Cell::new(false));
    let sink = {
        let (tx, stopped) = (tx.clone(), stopped.clone());
        move |level, column, row, w, h, rgba: &[u8]| {
            let png = encode_tile(w, h, rgba);
            if tx.send(Msg::Tile((level, column, row, png))).is_err() {
                stopped.set(true);
            }
        }
    };
    let mut builder = PyramidBuilder::new(width, height, sink);
    let mut strip = Vec::new();
    let mut rows = 0;
    loop {
        let n = reader
            .next_strip(STRIP_ROWS, &mut strip)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        builder.push_strip(&strip, n);
        rows += n;
        if stopped.get() || tx.send(Msg::Rows(rows)).is_err() {
            return Ok(());
        }
    }
    builder.finish();
    let _ = tx.send(Msg::Done);
    Ok(())
}

/// Multiplies the colors of straight RGBA8 pixels by their alpha.
pub fn premultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for c in &mut pixel[..3] {
            *c = ((u16::from(*c) * alpha + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_colors_scale_by_the_alpha() {
        let mut rgba = [255, 128, 0, 128, 10, 20, 30, 0, 9, 8, 7, 255];
        premultiply(&mut rgba);
        assert_eq!(rgba, [128, 64, 0, 128, 0, 0, 0, 0, 9, 8, 7, 255]);
    }

    #[test]
    fn the_message_warns_of_a_shape_that_is_not_2_to_1_and_of_no_alpha() {
        let info = |width, has_alpha| ImageInfo {
            name: "plates.png".into(),
            stem: "plates".into(),
            width,
            height: 100,
            has_alpha,
        };
        assert_eq!(
            info(200, true).message(),
            ("Imported plates.png (200 × 100)".into(), false)
        );
        assert!(info(300, true).message().1);
        let (text, warning) = info(200, false).message();
        assert!(warning && text.contains("no alpha"), "{text}");
    }
}
