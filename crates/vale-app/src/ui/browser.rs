//! The start screen: the list of the projects, and the dialogs that rename
//! and delete a project. The iPad layout draws its own screen from the same
//! state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Id, Modal, Sense, Vec2, vec2};

use super::world_form::{self, WorldForm};
use super::{Action, AppState, Layout, pad};
use crate::files::Files;
use crate::projects::{self, Entry, Locations};

/// The edge of the picture of a project, in points.
const PICTURE: f32 = 200.0;
/// The space between two cards, in points.
const GAP: f32 = 24.0;
/// The space between the window and the list, in points.
const MARGIN: i8 = 32;

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
}

impl BrowserState {
    /// Reads the projects folders again.
    pub fn reload(&mut self, locations: &Locations, files: &dyn Files) {
        self.entries = locations.list(files);
        self.textures.clear();
        self.menu = None;
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
    if state.layout == Layout::Pad {
        pad::browser::draw(ui, state);
        return;
    }
    // The test harness puts a margin around the UI. See `ui::draw`.
    let screen = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(MARGIN);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Worlds");
                ui.add_space(16.0);
                if ui.button(world_form::NEW_WORLD).clicked() {
                    state.world_form = Some(WorldForm::new_world());
                }
                if state.file_buttons && ui.button("Open file…").clicked() {
                    state.actions.push(Action::OpenProjectDialog);
                }
            });
            if !state.status.is_empty() {
                ui.label(&state.status);
            }
            ui.add_space(16.0);
            if state.browser.entries.is_empty() {
                ui.weak("No worlds yet. Make the first one with \"New world…\".");
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let entries = state.browser.entries.clone();
                    let columns = ((ui.available_width() + GAP) / (PICTURE + GAP)).floor();
                    for row in entries.chunks((columns as usize).max(1)) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for entry in row {
                                card(ui, state, entry);
                            }
                        });
                        ui.add_space(GAP);
                    }
                });
        });
    });
    dialogs(ui, state);
    world_form::desktop(ui.ctx(), state);
}

/// One project: the picture, the name, the date, and the buttons.
fn card(ui: &mut egui::Ui, state: &mut AppState, entry: &Entry) {
    let texture = state.browser.texture(ui.ctx(), entry);
    let readable = entry.error.is_none();
    let layout = egui::Layout::top_down(egui::Align::Min);
    ui.allocate_ui_with_layout(vec2(PICTURE, 0.0), layout, |ui| {
        ui.set_width(PICTURE);
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        let sense = if readable {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, picture) = ui.allocate_exact_size(Vec2::splat(PICTURE), sense);
        let label = format!("Open {}", entry.name);
        picture
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, readable, &label));
        let visuals = ui.style().interact(&picture);
        ui.painter().rect_filled(rect, 8.0, visuals.weak_bg_fill);
        match &texture {
            Some(texture) => {
                egui::Image::new(texture).paint_at(ui, rect);
            }
            // A file of an older version has no picture until it is open
            // one time.
            None => {
                let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
                ui.painter()
                    .circle_stroke(rect.center(), 0.45 * PICTURE, stroke);
            }
        }
        if picture.hovered() || picture.has_focus() {
            let stroke = ui.visuals().selection.stroke;
            let kind = egui::StrokeKind::Inside;
            ui.painter().rect_stroke(rect, 8.0, stroke, kind);
        }
        if picture.clicked() {
            state.actions.push(Action::OpenWorld(entry.path.clone()));
        }

        ui.strong(&entry.name);
        match (&entry.error, entry.modified) {
            (Some(error), _) => {
                let details = ui.link("Cannot read the file");
                if details.clicked() {
                    state
                        .browser
                        .show_problem(entry.path.clone(), error.clone());
                }
                details
            }
            (None, Some(time)) => ui.weak(projects::date_text(time)),
            (None, None) => ui.weak(""),
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.x = 6.0;
            let (path, name) = (entry.path.clone(), entry.name.clone());
            if ui
                .add_enabled(readable, egui::Button::new("Rename"))
                .clicked()
            {
                state.browser.ask_rename(path.clone(), name.clone());
            }
            if ui
                .add_enabled(readable, egui::Button::new("Copy"))
                .clicked()
            {
                state.actions.push(Action::CopyProject(path.clone()));
            }
            if ui.button("Delete").clicked() {
                state.browser.ask_delete(path, name);
            }
        });
    });
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
