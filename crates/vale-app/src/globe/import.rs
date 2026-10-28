//! The import of an equirectangular image into a raster layer, on a worker
//! thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use vale_import::{Greyscale, raster};
use vale_terrain::{Equirect, FACES};

/// What an import read. The size is that of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    /// The file name with its extension.
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// The image is twice as wide as it is tall.
    pub two_to_one: bool,
}

impl Imported {
    /// The text that tells the user what the import did.
    pub fn message(&self) -> String {
        let Imported {
            name,
            width,
            height,
            ..
        } = self;
        if self.two_to_one {
            return format!("Imported {name} ({width} × {height})");
        }
        format!(
            "Imported {name} ({width} × {height}). The image is not 2:1, so it is stretched \
             to cover the globe."
        )
    }
}

/// The result of an import. The error is a text for the user.
pub type ImportResult = Result<Imported, String>;

enum Msg {
    /// The file is in memory.
    Read(Imported),
    /// The levels of one face. The faces arrive in order.
    Face(usize, Vec<u16>),
    Failed(String),
}

/// The file name of a path, with the extension.
pub(crate) fn file_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    name.to_string_lossy().into_owned()
}

/// What an import reads.
pub enum Source {
    /// A file that the worker reads. Its levels go into the layer as they are.
    Path(PathBuf),
    /// An image in memory, with its file name, and the line that maps its
    /// levels to the values of the layer.
    Image {
        name: String,
        image: Greyscale,
        mapping: Mapping,
    },
}

/// A straight line from the levels of an image to the values of a layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    /// The darkest level and the brightest level of the image.
    pub levels: (u16, u16),
    /// The layer values of the darkest level and the brightest level.
    pub values: (f64, f64),
    /// The range of the layer: the values of level 0 and of level 65,535.
    pub range: (f64, f64),
}

impl Mapping {
    /// The layer value of an image level. With one level in the image,
    /// each level gets the value of the darkest level.
    pub fn value(&self, level: u16) -> f64 {
        let ((d, b), (lo, hi)) = (self.levels, self.values);
        if d == b {
            return lo;
        }
        lo + (hi - lo) * (f64::from(level) - f64::from(d)) / (f64::from(b) - f64::from(d))
    }

    /// The layer level of each image level. A value out of the range of the
    /// layer gets the nearest end.
    pub fn table(&self) -> Vec<u16> {
        let (min, max) = self.range;
        (0..=u16::MAX)
            .map(|level| {
                let t = ((self.value(level) - min) / (max - min)).clamp(0.0, 1.0);
                // A NaN from an empty range becomes 0.
                (t * 65535.0).round() as u16
            })
            .collect()
    }
}

/// An image that a probe read.
#[derive(Clone, Debug)]
pub struct Probe {
    pub image: Greyscale,
    /// The darkest level and the brightest level of the image.
    pub levels: (u16, u16),
}

/// The read of an image file on a worker thread, before an import.
pub struct ProbeJob {
    rx: mpsc::Receiver<Result<Probe, String>>,
}

impl ProbeJob {
    pub fn start(path: PathBuf) -> ProbeJob {
        let (tx, rx) = mpsc::sync_channel(1);
        let run = move || {
            let probe = raster::read_file(&path)
                .map_err(|e| e.to_string())
                .and_then(|image| match image.range() {
                    Some(levels) => Ok(Probe { image, levels }),
                    None => Err(format!("{} has no pixels", file_name(&path))),
                });
            // The send fails when the app dropped the job.
            let _ = tx.send(probe);
        };
        let spawned = std::thread::Builder::new()
            .name("vale-probe".to_string())
            .spawn(run);
        // With no thread, the channel has no sender, and the probe fails.
        drop(spawned);
        ProbeJob { rx }
    }

    /// The result, or `None` while the worker reads. `block`: waits for it.
    pub fn poll(&self, block: bool) -> Option<Result<Probe, String>> {
        let msg = if block {
            self.rx.recv().ok()
        } else {
            match self.rx.try_recv() {
                Ok(msg) => Some(msg),
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => None,
            }
        };
        Some(msg.unwrap_or_else(|| Err("The read stopped".to_string())))
    }
}

/// An import that runs on a worker thread.
pub(super) struct Job {
    pub name: String,
    rx: mpsc::Receiver<Msg>,
    /// `None`: the worker still reads the file.
    pub info: Option<Imported>,
    /// The number of faces that the heightmap has.
    pub faces: usize,
}

/// One step of a job.
pub(super) enum Step {
    /// The worker has nothing yet.
    Wait,
    /// The file is in memory. The faces follow.
    Read,
    Face(usize, Vec<u16>),
    Failed(String),
}

impl Job {
    /// Starts the worker. It makes faces of `face_size` texels.
    pub fn start(source: Source, face_size: usize) -> Job {
        let name = match &source {
            Source::Path(path) => file_name(path),
            Source::Image { name, .. } => name.clone(),
        };
        // The channel has no buffer, so the worker holds one face at most.
        let (tx, rx) = mpsc::sync_channel(0);
        let worker_name = name.clone();
        let run = move || {
            let image = match source {
                Source::Path(path) => match raster::read_file(&path) {
                    Ok(image) => image,
                    Err(e) => {
                        let _ = tx.send(Msg::Failed(e.to_string()));
                        return;
                    }
                },
                Source::Image {
                    mut image, mapping, ..
                } => {
                    // The mapping clamps, so it comes before the reduction mixes levels.
                    let table = mapping.table();
                    for level in &mut image.levels {
                        *level = table[usize::from(*level)];
                    }
                    image
                }
            };
            let (width, height) = (image.width, image.height);
            let Some(src) = Equirect::new(width, height, image.levels) else {
                let _ = tx.send(Msg::Failed(format!("{worker_name} has no pixels")));
                return;
            };
            let info = Imported {
                name: worker_name,
                width,
                height,
                two_to_one: src.is_two_to_one(),
            };
            if tx.send(Msg::Read(info)).is_err() {
                return;
            }
            let src = src.reduced_for(face_size);
            for face in 0..FACES {
                // The send fails when the app dropped the job.
                if tx.send(Msg::Face(face, src.face(face, face_size))).is_err() {
                    return;
                }
            }
        };
        let spawned = std::thread::Builder::new()
            .name("vale-import".to_string())
            .spawn(run);
        // With no thread, the channel has no sender, and the job fails.
        drop(spawned);
        Job {
            name,
            rx,
            info: None,
            faces: 0,
        }
    }

    /// The text that tells the user how far the import is.
    pub fn progress(&self) -> String {
        match self.info {
            None => format!("Importing {}: reading file", self.name),
            Some(_) => {
                let face = (self.faces + 1).min(FACES);
                format!("Importing {}: face {face} of {FACES}", self.name)
            }
        }
    }

    /// Takes the next message of the worker. `block`: waits for it.
    pub fn next(&mut self, block: bool) -> Step {
        let msg = if block {
            self.rx.recv().ok()
        } else {
            match self.rx.try_recv() {
                Ok(msg) => Some(msg),
                Err(mpsc::TryRecvError::Empty) => return Step::Wait,
                Err(mpsc::TryRecvError::Disconnected) => None,
            }
        };
        match msg {
            Some(Msg::Read(info)) => {
                self.info = Some(info);
                Step::Read
            }
            Some(Msg::Face(face, levels)) => Step::Face(face, levels),
            Some(Msg::Failed(text)) => Step::Failed(text),
            None => Step::Failed(format!("Cannot import {}: import stopped", self.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mapping_is_a_straight_line_that_clamps() {
        let m = Mapping {
            levels: (1000, 3000),
            values: (0.0, 3000.0),
            range: (0.0, 4000.0),
        };
        let table = m.table();
        assert_eq!(table[1000], 0);
        assert_eq!(table[0], 0);
        assert_eq!(table[2000], (1500.0f64 / 4000.0 * 65535.0).round() as u16);
        assert_eq!(table[3000], (3000.0f64 / 4000.0 * 65535.0).round() as u16);
        // 5,000 is past the range of the layer.
        assert_eq!(table[3000 + 2000 * 2 / 3], 65535);
        assert_eq!(table[65535], 65535);
    }

    #[test]
    fn the_full_range_is_the_identity() {
        let m = Mapping {
            levels: (0, 65535),
            values: (-6000.0, 6000.0),
            range: (-6000.0, 6000.0),
        };
        let table = m.table();
        assert!(table.iter().enumerate().all(|(i, &v)| usize::from(v) == i));
    }

    #[test]
    fn an_image_with_one_level_maps_to_the_darkest_value() {
        let m = Mapping {
            levels: (500, 500),
            values: (10.0, 90.0),
            range: (0.0, 100.0),
        };
        let level = (0.1f64 * 65535.0).round() as u16;
        assert!(m.table().iter().all(|&v| v == level));
    }

    #[test]
    fn values_can_go_down() {
        let m = Mapping {
            levels: (0, 65535),
            values: (100.0, 0.0),
            range: (0.0, 100.0),
        };
        let table = m.table();
        assert_eq!((table[0], table[65535]), (65535, 0));
    }
}
