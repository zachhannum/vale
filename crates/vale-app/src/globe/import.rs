//! The import of an equirectangular image into the heightmap, on a worker
//! thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use vale_import::raster;
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
            return format!("Imported {name} ({width} x {height}).");
        }
        format!(
            "Imported {name}. The image is {width} x {height}. A full-globe image is twice as \
             wide as it is tall, so the app stretched the image over the globe."
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
pub(super) fn file_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    name.to_string_lossy().into_owned()
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
    pub fn start(path: PathBuf, face_size: usize) -> Job {
        let name = file_name(&path);
        // The channel has no buffer, so the worker holds one face at most.
        let (tx, rx) = mpsc::sync_channel(0);
        let worker_name = name.clone();
        let run = move || {
            let image = match raster::read_file(&path) {
                Ok(image) => image,
                Err(e) => {
                    let _ = tx.send(Msg::Failed(e.to_string()));
                    return;
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
            None => format!("Importing {}: reading the file", self.name),
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
            None => Step::Failed(format!("cannot import {}: the import stopped", self.name)),
        }
    }
}
