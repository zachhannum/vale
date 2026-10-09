//! The UI: state, actions, and the draw function.

use std::path::PathBuf;

use eframe::egui;
use vale_labeler::Fonts;
use vale_sphere::LonLat;
use vale_store::LayerId;

use crate::document::Document;
use crate::globe::Globe;
use crate::headless;
use crate::pipeline::{Composed, Pipeline, Quality, Selection, fonts};

pub mod canvas;
pub mod globe;
pub mod inspector;
pub mod panels;

/// A request that needs the host: a dialog or a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenDialog,
    OpenFiles(Vec<PathBuf>),
    ExportDialog(ExportFormat),
    Export(PathBuf),
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

pub struct AppState {
    pub workspace: Workspace,
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
    /// Last message, for example an import error.
    pub status: String,
    /// Requests that need the host (dialogs, files).
    pub actions: Vec<Action>,
    /// True: no timers, always `Quality::Final`.
    pub headless: bool,
    /// False: the tool bar has no buttons that need a file dialog.
    pub file_buttons: bool,
    pub frames: u64,
}

impl AppState {
    pub fn new(doc: Document) -> anyhow::Result<Self> {
        Ok(AppState {
            workspace: Workspace::default(),
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
            status: String::new(),
            actions: Vec::new(),
            headless: false,
            file_buttons: true,
            frames: 0,
        })
    }

    /// Marks the map as needing a new compose.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Runs `OpenFiles` and `Export`. The dialog actions belong to the host.
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
            Action::OpenDialog | Action::ExportDialog(_) => {}
        }
    }
}

/// Draws the whole UI.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
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
