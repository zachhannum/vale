//! The UI: state, actions, and the draw function.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use vale_labeler::Fonts;
use vale_sphere::LonLat;
use vale_store::LayerId;

use crate::document::Document;
use crate::globe::import::ImportResult;
use crate::globe::{Globe, Tool};
use crate::headless;
use crate::pipeline::{Composed, Pipeline, Quality, Selection, fonts};
use crate::session::Session;

pub mod canvas;
pub mod elevation;
pub mod globe;
pub mod inspector;
pub mod pad;
pub mod panels;

/// A request that needs the host: a dialog or a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenDialog,
    OpenFiles(Vec<PathBuf>),
    ExportDialog(ExportFormat),
    Export(PathBuf),
    ImportHeightmapDialog,
    /// Imports this equirectangular image into the heightmap of the globe.
    ImportHeightmap(PathBuf),
}

/// The text of the last import of a heightmap.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    /// The import failed, or it changed the shape of the image.
    pub warning: bool,
}

/// True if the extension of the file is that of a PNG or TIFF image.
pub fn is_heightmap_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    ext.is_some_and(|e| ["png", "tif", "tiff"].contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Pdf,
}

/// A full-window view with its own canvas and panels.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Workspace {
    /// The world on a globe.
    #[default]
    Globe,
    /// The flat map of one map frame.
    Map,
}

/// The arrangement of the controls on the screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Layout {
    /// Docked panels, for a mouse.
    Desktop,
    /// A full-screen canvas with floating cards, for a finger and a pen.
    Pad,
}

impl Default for Layout {
    fn default() -> Layout {
        if cfg!(target_os = "ios") {
            Layout::Pad
        } else {
            Layout::Desktop
        }
    }
}

pub struct AppState {
    pub workspace: Workspace,
    pub layout: Layout,
    pub pad: pad::PadState,
    pub globe: Globe,
    pub doc: Document,
    pub pipeline: Pipeline,
    pub fonts: Fonts,
    pub composed: Option<Composed>,
    pub texture: Option<egui::TextureHandle>,
    /// The map must be composed again.
    pub dirty: bool,
    /// Quality of the last compose.
    pub quality: Quality,
    /// Egui time in seconds.
    pub last_view_change: Option<f64>,
    /// Size of the canvas in points.
    pub canvas_size: (f64, f64),
    /// Top-left corner of the canvas in screen points. Set by the canvas each frame.
    pub canvas_origin: (f32, f32),
    pub selection: Option<Selection>,
    pub selected_layer: Option<LayerId>,
    pub cursor_lonlat: Option<LonLat>,
    /// The elevation of the heightmap under the cursor, in meters.
    pub cursor_meters: Option<f64>,
    /// Last message, for example an import error.
    pub status: String,
    /// The text of the last import of a heightmap.
    pub import_note: Option<Note>,
    /// Requests that need the host (dialogs, files).
    pub actions: Vec<Action>,
    /// True: no timers, always `Quality::Final`.
    pub headless: bool,
    /// False: the tool bar has no buttons for GeoJSON files and for exports.
    pub file_buttons: bool,
    pub frames: u64,
    /// The open project file. `None`: the app saves nothing.
    pub session: Option<Session>,
    /// The edit count of the globe at the last save.
    saved_edits: u64,
}

impl AppState {
    pub fn new(doc: Document) -> anyhow::Result<Self> {
        let mut state = AppState {
            workspace: Workspace::default(),
            layout: Layout::default(),
            pad: pad::PadState::default(),
            globe: Globe::default(),
            doc,
            pipeline: Pipeline::new(),
            fonts: fonts()?,
            composed: None,
            texture: None,
            dirty: true,
            quality: Quality::Final,
            last_view_change: None,
            canvas_size: (0.0, 0.0),
            canvas_origin: (0.0, 0.0),
            selection: None,
            selected_layer: None,
            cursor_lonlat: None,
            cursor_meters: None,
            status: String::new(),
            import_note: None,
            actions: Vec::new(),
            headless: false,
            file_buttons: true,
            frames: 0,
            session: None,
            saved_edits: 0,
        };
        state.set_layout(Layout::default());
        Ok(state)
    }

    /// Sets the layout. In the iPad layout the pen paints, so the brush is
    /// the first tool.
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        if layout == Layout::Pad {
            self.globe.tool = Tool::Brush;
        }
    }

    /// True if the undo log of the project file has the edits. If not, the
    /// heightmap keeps the strokes in memory.
    fn file_log(&self) -> bool {
        let session = self.session.as_ref();
        session.is_some_and(|s| s.saves_map(&self.globe.map))
    }

    pub fn can_undo(&self) -> bool {
        match &self.session {
            Some(session) if self.file_log() => !self.globe.busy() && session.can_undo(),
            _ => self.globe.can_undo(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match &self.session {
            Some(session) if self.file_log() => !self.globe.busy() && session.can_redo(),
            _ => self.globe.can_redo(),
        }
    }

    /// Puts back the world from before the last edit.
    pub fn undo(&mut self) {
        self.step(false);
    }

    /// Puts back the last edit that `undo` took away.
    pub fn redo(&mut self) {
        self.step(true);
    }

    fn step(&mut self, redo: bool) {
        let able = if redo {
            self.can_redo()
        } else {
            self.can_undo()
        };
        if !able {
            return;
        }
        let file_log = self.file_log();
        let Some(session) = self.session.as_mut().filter(|_| file_log) else {
            if redo {
                self.globe.redo();
            } else {
                self.globe.undo();
            }
            return;
        };
        let map = &mut self.globe.map;
        let result = if redo {
            session.redo(&mut self.doc, map)
        } else {
            session.undo(&mut self.doc, map)
        };
        match result {
            Ok(_) => {
                let gone = |id| self.doc.layer(id).is_none();
                if self.selected_layer.is_some_and(gone) {
                    self.selected_layer = None;
                }
                if self.selection.is_some_and(|s| gone(s.layer)) {
                    self.selection = None;
                }
                // A layer can come back with other features under its ID.
                self.pipeline = Pipeline::new();
                self.touch();
            }
            Err(e) => {
                let what = if redo { "redo" } else { "undo" };
                self.status = format!("The {what} did not work: {e}");
            }
        }
    }

    /// Call this in each frame, after the changes of the frame. It saves the
    /// project a short time after a change, and at once after a stroke or an
    /// import. With `hold`, a control is held, and its changes wait. Returns
    /// the time to the next save.
    pub fn save_tick(&mut self, hold: bool, now: Instant) -> Option<Duration> {
        self.globe.set_memory_undo(!self.file_log());
        let session = self.session.as_mut()?;
        let globe = &mut self.globe;
        let project = &mut self.doc.project;
        let result = if globe.edits() != self.saved_edits {
            // The edit is one undo step, with no part of the next stroke.
            self.saved_edits = globe.edits();
            session.flush(project, &mut globe.map).map(|()| None)
        } else {
            let hold = hold || globe.busy();
            session.tick(project, &mut globe.map, hold, now)
        };
        match result {
            Ok(wait) => wait,
            Err(e) => {
                self.status = format!("The project did not save: {e}");
                None
            }
        }
    }

    /// Marks the map as needing a new compose.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Moves the result of an import of a heightmap to the status text.
    pub fn poll_import(&mut self) {
        if let Some(result) = self.globe.take_import_result() {
            self.note_import(result);
        }
    }

    /// Shows the result of an import of a heightmap.
    pub fn note_import(&mut self, result: ImportResult) {
        let note = match result {
            Ok(info) => Note {
                text: info.message(),
                warning: !info.two_to_one,
            },
            Err(text) => Note {
                text,
                warning: true,
            },
        };
        self.status = note.text.clone();
        self.import_note = Some(note);
    }

    /// Runs `OpenFiles`, `Export`, and `ImportHeightmap`. The dialog actions
    /// belong to the host.
    pub fn run_action(&mut self, action: Action) {
        match action {
            Action::OpenFiles(paths) => {
                for path in paths {
                    let had_layers = !self.doc.frame.entries.is_empty();
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    match self.doc.open_geojson(&path) {
                        Ok(ids) => {
                            if !had_layers {
                                self.doc.frame.view = None;
                            }
                            self.status = format!("Opened {name}: {} layers", ids.len());
                        }
                        Err(e) => self.status = format!("{name}: {e}"),
                    }
                }
                self.touch();
            }
            Action::Export(path) => {
                self.status = match headless::export(&self.doc, self.canvas_size, 2.0, &path) {
                    Ok(_) => format!("Exported {}", path.display()),
                    Err(e) => format!("{e:#}"),
                };
                self.touch();
            }
            Action::ImportHeightmap(path) => {
                self.import_note = None;
                self.globe.start_import(path);
                if self.headless {
                    self.globe.wait_import();
                }
                self.poll_import();
            }
            Action::OpenDialog | Action::ExportDialog(_) | Action::ImportHeightmapDialog => {}
        }
    }
}

/// Draws the whole UI.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    state.globe.stats.tick(std::time::Instant::now());
    state.poll_import();
    if state.layout == Layout::Pad && state.workspace == Workspace::Globe {
        pad::draw(ui, state);
        state.frames += 1;
        return;
    }
    // Fill the whole viewport. The test harness (egui_kittest) wraps the UI in an
    // 8 point outer margin, which would show as a dark border in screenshots. In the
    // real window this rectangle equals the rectangle of `ui`, so it changes nothing.
    let screen = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        panels::toolbar(ui, state);
        match state.workspace {
            Workspace::Globe => {
                panels::status(ui, state);
                panels::brush_debug(ui, state);
                panels::brush(ui, state);
                elevation::draw(ui, state);
                globe::draw(ui, state);
            }
            Workspace::Map => {
                panels::left(ui, state);
                inspector::draw(ui, state);
                panels::status(ui, state);
                canvas::draw(ui, state);
            }
        }
    });
    state.frames += 1;
}
