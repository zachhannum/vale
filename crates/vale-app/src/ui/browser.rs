//! The state of the start screen, and the dialogs that rename and delete a
//! project. Each layout draws its own screen from this state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use eframe::egui::{self, Id, Modal};

use super::{Action, AppState, Layout, desk, pad};
use crate::files::{Availability, Files, Version};
use crate::projects::{self, Entry, Locations};

/// The title of the dialog with the error of a file.
pub const PROBLEM_TITLE: &str = "The app cannot read this file";
/// The button that opens a file that the list cannot read. The open can
/// repair a file that the app did not close.
pub const TRY_OPEN: &str = "Try to open";

/// The name of the file and the whole error.
pub fn problem_text(path: &Path, error: &str) -> String {
    let file = path.file_name().unwrap_or(path.as_os_str());
    format!("{}: {error}", file.to_string_lossy())
}

/// A dialog that waits for an answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Rename {
        path: PathBuf,
        /// The text of the field.
        name: String,
        /// The field had the focus one time.
        focused: bool,
    },
    Delete {
        path: PathBuf,
        name: String,
    },
    /// The whole error of a file that the app cannot read.
    Problem {
        path: PathBuf,
        error: String,
    },
    /// The versions of a file that two devices changed.
    Versions {
        path: PathBuf,
        name: String,
        /// The time of the version of this device.
        modified: Option<SystemTime>,
        others: Vec<Version>,
    },
}

/// The title of the dialog with the versions of a file.
pub const VERSIONS_TITLE: &str = "Pick a version";
pub const VERSIONS_TEXT: &str =
    "Two devices changed this world. Pick the version to keep. The other versions go away.";

/// The name of the button that keeps a version.
pub fn keep_name(device: &str, modified: Option<SystemTime>) -> String {
    match modified {
        Some(time) => format!("Keep {device}, {}", projects::time_text(time)),
        None => format!("Keep {device}"),
    }
}

/// The name of this device in the list of the versions.
pub fn this_device(pad: bool) -> &'static str {
    if pad { "this iPad" } else { "this computer" }
}

/// The buttons of the dialog with the versions: the name, and the version
/// that the button keeps.
pub fn version_buttons(
    modified: Option<SystemTime>,
    others: &[Version],
    pad: bool,
) -> Vec<(String, Option<usize>)> {
    let mut buttons = vec![(keep_name(this_device(pad), modified), None)];
    let other = |(index, version): (usize, &Version)| {
        (keep_name(&version.device, version.modified), Some(index))
    };
    buttons.extend(others.iter().enumerate().map(other));
    buttons
}

/// The line below the name of a project, and `true` if the line is a
/// warning. `pad`: the device is an iPad.
pub fn info_line(entry: &Entry, pad: bool, locations: bool) -> (String, bool) {
    let (text, warning) = match (&entry.error, entry.availability) {
        (Some(_), _) => return ("Cannot read the file".to_string(), true),
        (None, Availability::InCloud) => ("Not downloaded".to_string(), false),
        (None, Availability::Downloading) => ("Downloading…".to_string(), false),
        (None, Availability::Here) if !entry.conflicts.is_empty() => {
            (format!("{} versions", entry.conflicts.len() + 1), true)
        }
        (None, Availability::Here) => {
            let date = entry.modified.map(projects::date_text);
            (date.unwrap_or_default(), false)
        }
    };
    let text = match (locations, text.is_empty()) {
        (false, _) => text,
        (true, true) => entry.location.label(pad).to_string(),
        (true, false) => format!("{} · {text}", entry.location.label(pad)),
    };
    (text, warning)
}

/// Acts on a tap on a project that the app can read.
pub fn open_entry(state: &mut AppState, entry: &Entry) {
    if entry.availability != Availability::Here {
        state.actions.push(Action::Download(entry.path.clone()));
    } else if !entry.conflicts.is_empty() {
        state.browser.dialog = Some(Dialog::Versions {
            path: entry.path.clone(),
            name: entry.name.clone(),
            modified: entry.modified,
            others: entry.conflicts.clone(),
        });
    } else {
        state.actions.push(Action::OpenWorld(entry.path.clone()));
    }
}

/// The open menu of the actions of one project, in the iPad layout.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionsMenu {
    pub path: PathBuf,
    /// The button that opened the menu.
    pub button: egui::Rect,
}

#[derive(Default)]
pub struct BrowserState {
    pub menu: Option<ActionsMenu>,
    /// The projects of the folder at the last read.
    pub entries: Vec<Entry>,
    /// The pictures of the entries that the screen showed.
    textures: HashMap<PathBuf, egui::TextureHandle>,
    pub dialog: Option<Dialog>,
    /// The project that opens after its download.
    pub pending: Option<PathBuf>,
    /// The time of the last read of the folders while a download runs.
    pub polled: Option<Instant>,
    /// The recent worlds at the last read: the path and the name.
    pub recent: Vec<(PathBuf, String)>,
}

impl BrowserState {
    /// Reads the projects folders again.
    pub fn reload(&mut self, locations: &Locations, files: &dyn Files) {
        self.entries = locations.list(files);
        self.textures.clear();
        self.menu = None;
        let recent = locations.state_dir().map(projects::recent);
        let name = |path: &PathBuf| {
            let entry = self
                .entries
                .iter()
                .find(|e| std::path::absolute(&e.path).is_ok_and(|absolute| absolute == *path));
            entry.map_or_else(|| projects::entry(path).name, |e| e.name.clone())
        };
        self.recent = recent
            .unwrap_or_default()
            .into_iter()
            .map(|path| {
                let name = name(&path);
                (path, name)
            })
            .collect();
    }

    /// The count of the pictures that the screen showed.
    pub fn picture_count(&self) -> usize {
        self.textures.len()
    }

    /// Opens the dialog that asks for a new name of a project.
    pub fn ask_rename(&mut self, path: PathBuf, name: String) {
        self.dialog = Some(Dialog::Rename {
            path,
            name,
            focused: false,
        });
    }

    /// Opens the dialog that asks before a project is removed.
    pub fn ask_delete(&mut self, path: PathBuf, name: String) {
        self.dialog = Some(Dialog::Delete { path, name });
    }

    /// Opens the dialog with the whole error of a file.
    pub fn show_problem(&mut self, path: PathBuf, error: String) {
        self.dialog = Some(Dialog::Problem { path, error });
    }

    /// The picture of an entry as a texture.
    pub fn texture(&mut self, ctx: &egui::Context, entry: &Entry) -> Option<egui::TextureHandle> {
        let picture = entry.thumbnail.as_ref()?;
        let load = || {
            let size = [picture.width as usize, picture.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &picture.rgba);
            let name = format!("project {}", entry.path.display());
            ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
        };
        Some(
            self.textures
                .entry(entry.path.clone())
                .or_insert_with(load)
                .clone(),
        )
    }
}

/// Draws the start screen.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    match state.layout {
        Layout::Pad => pad::browser::draw(ui, state),
        Layout::Desktop => desk::start::draw(ui, state),
    }
}

/// Draws the open dialog. The editor calls this too, for the File menu.
pub fn dialogs(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(dialog) = &mut state.browser.dialog else {
        return;
    };
    let ctx = ui.ctx().clone();
    let mut close = false;
    let mut action = None;
    match dialog {
        Dialog::Problem { path, error } => {
            let modal = Modal::new(Id::new("project_problem")).show(&ctx, |ui| {
                ui.set_width(360.0);
                ui.heading(PROBLEM_TITLE);
                ui.label(problem_text(path, error));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if ui.button(TRY_OPEN).clicked() {
                        action = Some(Action::OpenWorld(path.clone()));
                    }
                });
            });
            close |= modal.should_close();
        }
        Dialog::Versions {
            path,
            name,
            modified,
            others,
        } => {
            let modal = Modal::new(Id::new("project_versions")).show(&ctx, |ui| {
                ui.set_width(360.0);
                ui.heading(format!("{VERSIONS_TITLE}: {name}"));
                ui.label(VERSIONS_TEXT);
                ui.add_space(8.0);
                for (text, keep) in version_buttons(*modified, others, false) {
                    if ui.button(text).clicked() {
                        action = Some(Action::KeepVersion(path.clone(), keep));
                    }
                }
                ui.add_space(8.0);
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
            close |= modal.should_close();
        }
        Dialog::Delete { path, name } => {
            let modal = Modal::new(Id::new("delete_project")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.heading(format!("Delete {name}?"));
                ui.label("The world and its undo steps go away. You cannot get them back.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Delete world").clicked() {
                        action = Some(Action::DeleteProject(path.clone()));
                    }
                });
            });
            close |= modal.should_close();
        }
        Dialog::Rename {
            path,
            name,
            focused,
        } => {
            let modal = Modal::new(Id::new("rename_project")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.heading("Rename the world");
                let field = egui::TextEdit::singleline(name).desired_width(f32::INFINITY);
                let field = ui.add(field);
                if !*focused {
                    *focused = true;
                    field.request_focus();
                }
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    let valid = !name.trim().is_empty();
                    let save = ui.add_enabled(valid, egui::Button::new("Save name"));
                    if valid && (save.clicked() || enter) {
                        action = Some(Action::RenameProject(path.clone(), name.clone()));
                    }
                });
            });
            close |= modal.should_close();
        }
    }
    if let Some(action) = action {
        state.actions.push(action);
        close = true;
    }
    if close {
        state.browser.dialog = None;
    }
}
