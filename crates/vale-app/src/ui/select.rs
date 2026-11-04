//! The Select tool of the world workspace: the selection of features, the
//! move of a point, the delete, and the inspector that edits the attributes
//! of the selection.

use std::collections::HashSet;

use eframe::egui::{self, Pos2};
use vale_store::{Geometry, Layer, LayerId, Uuid};

use super::AppState;
use super::pad::theme;
use super::pad::widgets::{self, Controls};
use super::point_form::FormField;
use super::table::SHOW_IN_TABLE;
use crate::globe::math::{V3, dir_to_lonlat};
use crate::globe::select::{self, FeatureRef, SelectEvent};

/// The name of the Select tool.
pub const TOOL: &str = "Select";
/// The button that deletes the selected features.
pub const DELETE: &str = "Delete selection";
/// The name of the inspector, and of the button that opens it.
pub const ATTRIBUTES: &str = "Attributes";
/// The empty state of the inspector.
pub const NO_SELECTION: &str = "Select a feature to see its attributes.";
/// The text of the inspector for a feature of a layer with no fields.
pub const NO_FIELDS: &str = "The layer has no fields.";
/// The command of the desktop View menu that shows the inspector.
pub const SHOW_ATTRIBUTES: &str = "Show attributes";
/// The command of the desktop View menu that hides the inspector.
pub const HIDE_ATTRIBUTES: &str = "Hide attributes";
const INSPECTOR: &str = "attributes";
/// The number of frames that ask for the focus in a new form. At the end of
/// its first frame, the tap that selects the feature takes the focus from
/// the field.
const FOCUS_FRAMES: u8 = 3;
/// The radius of the handle at each end of a selected line.
const END: f32 = 5.0;

/// A drag of the Select tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectDrag {
    /// A box from `start` to `end`, in screen points.
    Box { start: Pos2, end: Pos2 },
    /// A move of one vertex of a point feature to `to`.
    Move {
        feature: FeatureRef,
        vertex: usize,
        to: Option<V3>,
    },
}

/// The fields of the one selected feature.
pub struct SelectionForm {
    pub feature: FeatureRef,
    pub fields: Vec<FormField>,
    /// The undo count when the fields took their values.
    history: u64,
    /// The field that takes the focus when the form shows, and the number
    /// of frames that can still ask for the focus.
    focus: Option<(String, u8)>,
}

/// The text of the inspector for a selection of `count` features.
pub fn title(count: usize) -> String {
    format!("{count} features")
}

impl AppState {
    /// The layer that the Select tool selects in: the selected vector
    /// layer, if it is visible.
    pub(super) fn select_layer(&self) -> Option<&Layer> {
        let id = self.selected_vector()?;
        self.doc.layer(id).filter(|l| l.visible)
    }

    fn select_layers(&self) -> Vec<&Layer> {
        self.select_layer().into_iter().collect()
    }

    /// Reads the taps and the drags of the Select tool.
    pub fn poll_select(&mut self) {
        let events = std::mem::take(&mut self.globe.select_press.events);
        self.prune_feature_selection();
        if !self.globe.selecting() {
            self.select_drag = None;
            return;
        }
        for event in events {
            match event {
                SelectEvent::Tap { pos, pointer } => {
                    let layers = self.select_layers();
                    let hit = select::hit(&self.globe, &layers, pos, pointer.reach());
                    self.set_feature_selection(hit.into_iter().collect());
                }
                SelectEvent::DragStart { pos, pointer } => {
                    let reach = pointer.reach();
                    let moving = self.feature_selection.iter().find_map(|f| {
                        let layer = self.doc.layer(f.layer)?;
                        let vertex = select::hit_vertex(&self.globe, layer, f.uuid, pos, reach)?;
                        Some((*f, vertex))
                    });
                    self.select_drag = Some(match moving {
                        Some((feature, vertex)) => SelectDrag::Move {
                            feature,
                            vertex,
                            to: None,
                        },
                        None => SelectDrag::Box {
                            start: pos,
                            end: pos,
                        },
                    });
                }
                SelectEvent::Drag { pos } => self.select_drag_to(pos),
                SelectEvent::DragEnd { pos } => {
                    self.select_drag_to(pos);
                    match self.select_drag.take() {
                        Some(SelectDrag::Box { start, end }) => {
                            let rect = select::drag_rect(start, end);
                            let found = select::in_box(&self.globe, &self.select_layers(), rect);
                            self.set_feature_selection(found);
                        }
                        Some(SelectDrag::Move {
                            feature,
                            vertex,
                            to: Some(to),
                        }) => self.move_vertex(feature, vertex, to),
                        _ => {}
                    }
                }
                SelectEvent::Cancel => self.select_drag = None,
            }
        }
    }

    fn select_drag_to(&mut self, pos: Pos2) {
        match &mut self.select_drag {
            Some(SelectDrag::Box { end, .. }) => *end = pos,
            Some(SelectDrag::Move { to, .. }) => *to = self.globe.unproject(pos),
            None => {}
        }
    }

    /// Replaces the selection of the Select tool.
    pub fn set_feature_selection(&mut self, selection: Vec<FeatureRef>) {
        if selection != self.feature_selection {
            self.commit_selection_form();
            if selection.is_empty() {
                self.attribute_focus = None;
            }
            self.feature_selection = selection;
        }
    }

    /// Keeps only the selected features that still exist in the layer of
    /// the Select tool.
    fn prune_feature_selection(&mut self) {
        if self.feature_selection.is_empty() {
            return;
        }
        let live: HashSet<(LayerId, Uuid)> = self
            .select_layer()
            .into_iter()
            .flat_map(|l| l.features.iter().map(|f| (l.id, f.uuid())))
            .collect();
        self.feature_selection
            .retain(|f| live.contains(&(f.layer, f.uuid)));
    }

    /// Moves one vertex of a point feature. The move is one undo step.
    fn move_vertex(&mut self, f: FeatureRef, vertex: usize, to: V3) {
        let Some(layer) = self.doc.layer(f.layer) else {
            return;
        };
        let feature = layer.features.iter().find(|x| x.uuid() == f.uuid);
        let Some(Geometry::Points(mut points)) = feature.map(|x| x.geometry.clone()) else {
            return;
        };
        let Some(point) = points.get_mut(vertex) else {
            return;
        };
        *point = dir_to_lonlat(to).into();
        self.commit_selection_form();
        self.commit_point_form();
        self.save_edit();
        let geometry = Geometry::Points(points);
        if self.doc.project.set_geometry(f.layer, f.uuid, geometry) {
            self.end_vector_edit(f.layer);
        }
    }

    /// Deletes the selected features in one undo step.
    pub fn delete_selection(&mut self) {
        let Some(id) = self.feature_selection.first().map(|f| f.layer) else {
            return;
        };
        self.commit_selection_form();
        self.selection_form = None;
        self.attribute_focus = None;
        self.commit_point_form();
        self.save_edit();
        let selection = std::mem::take(&mut self.feature_selection);
        let uuids: Vec<Uuid> = selection.iter().map(|f| f.uuid).collect();
        if self.doc.project.remove_features(id, &uuids) > 0 {
            self.end_vector_edit(id);
        }
    }

    /// Opens the form of the one selected feature, or closes it. `shown`:
    /// the inspector shows.
    pub fn sync_selection_form(&mut self, shown: bool) {
        if !shown {
            self.attribute_focus = None;
        }
        let one = match self.feature_selection.as_slice() {
            [one] if shown => Some(*one),
            _ => None,
        };
        let fresh = self
            .selection_form
            .as_ref()
            .is_some_and(|form| Some(form.feature) == one && form.history == self.history);
        if fresh {
            return;
        }
        if self
            .selection_form
            .as_ref()
            .is_some_and(|form| form.history == self.history)
        {
            self.commit_selection_form();
        }
        self.selection_form = one.and_then(|f| {
            let layer = self.doc.layer(f.layer)?;
            let feature = layer.features.iter().find(|x| x.uuid() == f.uuid)?;
            let fields = layer.fields.iter().map(|field| {
                let value = feature.attributes.get(&field.name);
                FormField::new(field, value.and_then(|v| v.label()).unwrap_or_default())
            });
            // The field that was edited last takes the focus in the form of
            // the next feature of its layer.
            let focus = match &mut self.attribute_focus {
                Some((last, name)) if last.layer == f.layer && *last != f => {
                    *last = f;
                    Some((name.clone(), FOCUS_FRAMES))
                }
                _ => None,
            };
            Some(SelectionForm {
                feature: f,
                fields: fields.collect(),
                history: self.history,
                focus,
            })
        });
    }

    /// Puts each changed value of the form in its feature. The change is
    /// one undo step.
    pub fn commit_selection_form(&mut self) {
        let Some(form) = &self.selection_form else {
            return;
        };
        let f = form.feature;
        let Some(feature) = self
            .doc
            .layer(f.layer)
            .and_then(|l| l.features.iter().find(|x| x.uuid() == f.uuid))
        else {
            return;
        };
        let changed: Vec<_> = form
            .fields
            .iter()
            .filter(|field| field.valid())
            .map(|field| (field.name.clone(), field.value()))
            .filter(|(name, value)| feature.attributes.get(name) != Some(value))
            .collect();
        if changed.is_empty() {
            return;
        }
        self.edit_attributes(f.layer, |project| {
            project.set_attributes(f.layer, f.uuid, changed)
        });
    }
}

/// The highlight of the selection and of the drag of the Select tool. The
/// selection shows in each tool.
pub fn paint(painter: &egui::Painter, state: &AppState) {
    let globe = &state.globe;
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    let line = egui::Stroke::new(3.0, theme::ACCENT);
    let dark = egui::Stroke::new(1.5, egui::Color32::from_rgb(11, 31, 26));
    for f in &state.feature_selection {
        let Some(layer) = state.doc.layer(f.layer) else {
            continue;
        };
        let Some(feature) = layer.features.iter().find(|x| x.uuid() == f.uuid) else {
            continue;
        };
        let shape = select::shape(globe, &feature.geometry);
        for (_, pos) in &shape.points {
            painter.circle_stroke(*pos, 9.0, ring);
        }
        for run in shape.lines {
            painter.line(run, line);
        }
        // The ends of a line show as handles, so that the cut of a split
        // shows between two selected lines.
        for pos in shape.ends {
            painter.circle(pos, END, theme::ACCENT, dark);
        }
    }
    match state.select_drag {
        Some(SelectDrag::Box { start, end }) => {
            let rect = select::drag_rect(start, end);
            let fill = theme::ACCENT.gamma_multiply(0.15);
            painter.rect(rect, 0.0, fill, ring, egui::StrokeKind::Inside);
        }
        Some(SelectDrag::Move { to: Some(to), .. }) => {
            if let Some(pos) = globe.project(to) {
                painter.circle_filled(pos, 4.0, theme::ACCENT);
                painter.circle_stroke(pos, 9.0, ring);
            }
        }
        _ => {}
    }
}

/// Reads the keys of the Select tool on the desktop: V chooses the tool,
/// Delete and Backspace delete the selection, and Escape clears it.
pub fn keys(ui: &egui::Ui, state: &mut AppState) {
    use egui::{Key, Modifiers};
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let key = |key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if key(Key::V) {
        state.globe.tool = crate::globe::Tool::Select;
    }
    if !state.globe.selecting() {
        return;
    }
    if key(Key::Delete) || key(Key::Backspace) {
        state.delete_selection();
    }
    if key(Key::Escape) {
        state.set_feature_selection(Vec::new());
    }
}

/// Reads a field of the inspector: its edit and its loss of the focus.
/// Returns true if the field lost the focus.
fn read_field(
    resp: &egui::Response,
    focus: &mut Option<(String, u8)>,
    remember: &mut Option<String>,
    name: &str,
) -> bool {
    if let Some((field, frames)) = focus
        && field == name
    {
        if *frames == 0 {
            *focus = None;
        } else {
            resp.request_focus();
            *frames -= 1;
        }
    }
    if resp.changed() {
        *remember = Some(name.to_owned());
    }
    resp.lost_focus()
}

/// What the inspector does after its frame.
#[derive(Default)]
struct Outcome {
    commit: bool,
    show: bool,
    /// The field that the user edited.
    edited: Option<String>,
}

impl AppState {
    fn finish_inspector(&mut self, form: Option<SelectionForm>, outcome: Outcome) {
        if let (Some(name), Some(form)) = (outcome.edited, &form) {
            self.attribute_focus = Some((form.feature, name));
        }
        self.selection_form = form;
        if outcome.commit {
            self.commit_selection_form();
        }
        if outcome.show {
            self.show_in_table();
        }
    }
}

/// The body of the Attributes panel of the iPad layout. With one selected
/// feature, it has a field for each attribute. With more than one, it
/// shows the count.
pub fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let count = state.feature_selection.len();
    if count == 0 {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_SELECTION).color(theme::MUTE));
        return;
    }
    let mut form = state.selection_form.take();
    let mut outcome = Outcome::default();
    match form.as_mut() {
        Some(form) => {
            if form.fields.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(NO_FIELDS).color(theme::MUTE));
            }
            let focus = &mut form.focus;
            for field in &mut form.fields {
                let valid = field.valid();
                let (name, text) = (&field.name, &mut field.text);
                let resp = widgets::text_field(ui, controls, name, name, None, text, valid);
                outcome.commit |= read_field(&resp, focus, &mut outcome.edited, name);
            }
        }
        None => {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(title(count)).color(theme::MUTE));
        }
    }
    ui.add_space(8.0);
    outcome.show = widgets::button_row(ui, controls, SHOW_IN_TABLE, true).clicked();
    state.finish_inspector(form, outcome);
}

/// Shows or hides the inspector of the desktop layout.
pub fn toggle_desktop(state: &mut AppState) {
    state.show_attributes = !state.show_attributes;
}

/// The View menu of the desktop tool bar, and its shortcut: ⌘I shows and
/// hides the inspector.
pub fn view_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::I);
    if ui.input_mut(|i| i.consume_shortcut(&shortcut)) {
        toggle_desktop(state);
    }
    ui.menu_button("View", |ui| {
        let name = if state.show_attributes {
            HIDE_ATTRIBUTES
        } else {
            SHOW_ATTRIBUTES
        };
        let button = egui::Button::new(name).shortcut_text(ui.ctx().format_shortcut(&shortcut));
        if ui.add(button).clicked() {
            toggle_desktop(state);
            ui.close();
        }
    });
}

/// The inspector of the desktop layout: a panel at the right.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    state.sync_selection_form(state.show_attributes);
    if !state.show_attributes {
        return;
    }
    let mut form = state.selection_form.take();
    let mut outcome = Outcome::default();
    let count = state.feature_selection.len();
    egui::Panel::right(INSPECTOR)
        .default_size(260.0)
        .show(ui, |ui| {
            ui.strong(ATTRIBUTES);
            ui.separator();
            if count == 0 {
                ui.weak(NO_SELECTION);
                return;
            }
            match form.as_mut() {
                Some(form) => {
                    if form.fields.is_empty() {
                        ui.weak(NO_FIELDS);
                    }
                    let focus = &mut form.focus;
                    egui::Grid::new("attribute-fields")
                        .num_columns(2)
                        .show(ui, |ui| {
                            for field in &mut form.fields {
                                ui.label(&field.name);
                                let color = (!field.valid()).then(|| ui.visuals().warn_fg_color);
                                let edit = egui::TextEdit::singleline(&mut field.text)
                                    .id(egui::Id::new((INSPECTOR, &field.name)))
                                    .text_color_opt(color);
                                let resp = ui.add(edit);
                                let name = &field.name;
                                outcome.commit |=
                                    read_field(&resp, focus, &mut outcome.edited, name);
                                ui.end_row();
                            }
                        });
                }
                None => drop(ui.label(title(count))),
            }
            outcome.show = ui.button(SHOW_IN_TABLE).clicked();
        });
    state.finish_inspector(form, outcome);
}
