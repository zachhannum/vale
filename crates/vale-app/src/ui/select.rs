//! The Select tool of the world workspace: the selection of features, the
//! move of a point, the delete, and the inspector that edits the attributes
//! of the selection.

use std::collections::HashSet;

use eframe::egui::{self, Pos2};
use vale_store::{Geometry, Layer, LayerId, Uuid, Value};

use super::AppState;
use super::new_feature::{FormField, can_take_focus};
use super::pad::theme;
use super::pad::widgets::{self, Controls};
use super::table::SHOW_IN_TABLE;
use crate::globe::math::{V3, dir_to_lonlat};
use crate::globe::select::{self, FeatureRef, SelectEvent, Shape};

/// The name of the Select tool.
pub const TOOL: &str = "Select";
/// The button that deletes the selected features.
pub const DELETE: &str = "Delete selection";
/// The command that merges the selected features into one feature.
pub const MERGE: &str = "Merge…";
/// The title of the list of Merge.
pub const MERGE_TITLE: &str = "Keep attributes of";
/// The name of the inspector, and of the button that opens it.
pub const ATTRIBUTES: &str = "Attributes";
/// The empty state of the inspector.
pub const NO_SELECTION: &str = "Select a feature to see its attributes.";
/// The text of the inspector for a feature of a layer with no fields.
pub const NO_FIELDS: &str = "The layer has no fields.";
/// The toggle of the inspector that gives a new feature the focus.
pub const FOCUS_NEW: &str = "Focus new features";
/// The command of the desktop View menu that shows the inspector.
pub const SHOW_ATTRIBUTES: &str = "Show attributes";
/// The command of the desktop View menu that hides the inspector.
pub const HIDE_ATTRIBUTES: &str = "Hide attributes";
const INSPECTOR: &str = "attributes";
/// The number of frames that ask for the focus in a new form. At the end of
/// its first frame, the tap that selects the feature takes the focus from
/// the field.
const FOCUS_FRAMES: u8 = 3;
/// The largest number of different values that the list of a field shows.
/// A field with more values has no list.
pub const CHOICES: usize = 50;
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
    /// The values of each field in the features of the layer.
    pub choices: Vec<Vec<String>>,
    /// The field with the open list of values.
    pub open: Option<String>,
    /// The undo count when the fields took their values.
    history: u64,
    /// The field that takes the focus when the form shows, and the number
    /// of frames that can still ask for the focus.
    focus: Option<(String, u8)>,
}

impl SelectionForm {
    /// True if a field of the form waits for the focus.
    pub(super) fn takes_focus(&self) -> bool {
        self.focus.is_some()
    }
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
            self.commit_new_feature();
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
        self.prune_feature_selection_to(self.select_layer().map(|l| l.id));
    }

    /// Keeps only the selected features that still exist in `layer`.
    pub(super) fn prune_feature_selection_to(&mut self, layer: Option<LayerId>) {
        if self.feature_selection.is_empty() {
            return;
        }
        let live: HashSet<(LayerId, Uuid)> = layer
            .and_then(|id| self.doc.layer(id))
            .into_iter()
            .flat_map(|l| l.features.iter().map(|f| (l.id, f.uuid())))
            .collect();
        self.feature_selection
            .retain(|f| live.contains(&(f.layer, f.uuid)));
        if self.feature_selection.is_empty() {
            self.attribute_focus = None;
        }
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
        self.commit_new_feature();
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
        self.commit_new_feature();
        self.save_edit();
        let selection = std::mem::take(&mut self.feature_selection);
        let uuids: Vec<Uuid> = selection.iter().map(|f| f.uuid).collect();
        if self.doc.project.remove_features(id, &uuids) > 0 {
            self.end_vector_edit(id);
        }
    }

    /// Two or more features of one layer are selected.
    pub fn can_merge(&self) -> bool {
        let mut layers = self.feature_selection.iter().map(|f| f.layer);
        let first = layers.next();
        self.feature_selection.len() >= 2 && layers.all(|l| Some(l) == first)
    }

    /// The selected features that Merge lists, the largest first, each with
    /// its name. The name is the value of the label field of the layer, or
    /// else "Feature" and the place in the list.
    pub fn merge_choices(&self) -> Vec<(Uuid, String)> {
        if !self.can_merge() {
            return Vec::new();
        }
        let id = self.feature_selection[0].layer;
        let Some(layer) = self.doc.layer(id) else {
            return Vec::new();
        };
        let field = (self.doc.entry(id)).and_then(|e| e.style.label.field.clone());
        let mut features: Vec<_> = (layer.features.iter())
            .filter(|f| self.feature_selection.iter().any(|s| s.uuid == f.uuid()))
            .map(|f| (crate::globe::merge::size(&f.geometry), f))
            .collect();
        features.sort_by(|a, b| b.0.total_cmp(&a.0));
        let name = |i: usize, f: &vale_store::Feature| {
            let value = field.as_ref().and_then(|n| f.attributes.get(n));
            let label = value
                .and_then(|v| v.label())
                .filter(|s| !s.trim().is_empty());
            label.unwrap_or_else(|| format!("Feature {}", i + 1))
        };
        (features.iter().enumerate())
            .map(|(i, (_, f))| (f.uuid(), name(i, f)))
            .collect()
    }

    /// Starts a merge of the selection. If all the selected features have
    /// the same attributes, the merge keeps the largest feature, and no list
    /// asks. Else the list of Merge opens.
    pub fn start_merge(&mut self) {
        let Some(&(largest, _)) = self.merge_choices().first() else {
            return;
        };
        let same = self
            .doc
            .layer(self.feature_selection[0].layer)
            .is_some_and(|l| {
                let selected = (l.features.iter())
                    .filter(|f| self.feature_selection.iter().any(|s| s.uuid == f.uuid()));
                let mut attributes = selected.map(|f| &f.attributes);
                let first = attributes.next();
                attributes.all(|a| Some(a) == first)
            });
        if same {
            self.merge_selection(largest);
        } else {
            self.merging = true;
        }
    }

    /// Joins the selected features into the feature `keep`, in one undo
    /// step. The feature keeps its UUID, its attributes, and its place in
    /// the draw order, and it becomes the selection.
    pub fn merge_selection(&mut self, keep: Uuid) {
        self.merging = false;
        if !self.can_merge() || !self.feature_selection.iter().any(|f| f.uuid == keep) {
            return;
        }
        let id = self.feature_selection[0].layer;
        let Some(layer) = self.doc.layer(id) else {
            return;
        };
        let selected = |uuid: Uuid| self.feature_selection.iter().any(|f| f.uuid == uuid);
        let geometries: Vec<&Geometry> = (layer.features.iter())
            .filter(|f| selected(f.uuid()))
            .map(|f| &f.geometry)
            .collect();
        let Some(merged) = crate::globe::merge::merge(&geometries) else {
            return;
        };
        self.commit_selection_form();
        self.selection_form = None;
        self.commit_new_feature();
        self.save_edit();
        let others: Vec<Uuid> = (self.feature_selection.iter())
            .map(|f| f.uuid)
            .filter(|&u| u != keep)
            .collect();
        let project = &mut self.doc.project;
        if project.set_geometry(id, keep, merged) {
            project.remove_features(id, &others);
            self.feature_selection = vec![FeatureRef {
                layer: id,
                uuid: keep,
            }];
            self.end_vector_edit(id);
        }
    }

    /// Opens the form of the one selected feature, or closes it. `shown`:
    /// the inspector shows.
    pub fn sync_selection_form(&mut self, shown: bool) {
        self.inspector_shown = shown;
        // A change of the tool ends the run of edits.
        if !shown || self.attribute_tool != self.globe.tool {
            self.attribute_focus = None;
            self.attribute_tool = self.globe.tool;
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
            // With "Focus new features", a new feature takes the focus in the
            // field that was edited last in its layer, or in the first field.
            // The field that was edited last takes the focus in the form of
            // the next feature of its layer that the Select tool selects.
            let new = self.new_feature.as_ref().is_some_and(|n| n.feature == f);
            let known = |name: &String| layer.fields.iter().any(|field| &field.name == name);
            let last =
                (self.last_field.as_ref()).filter(|(id, name)| *id == f.layer && known(name));
            let focus = match &mut self.attribute_focus {
                _ if new && self.tools.focus_new => last
                    .map(|(_, name)| name.clone())
                    .or_else(|| layer.fields.first().map(|field| field.name.clone()))
                    .map(|name| (name, FOCUS_FRAMES)),
                _ if new => None,
                Some((last, name)) if last.layer == f.layer && *last != f => {
                    *last = f;
                    Some((name.clone(), FOCUS_FRAMES))
                }
                _ => None,
            };
            let choices = layer.fields.iter().map(|f| choices(layer, &f.name));
            Some(SelectionForm {
                feature: f,
                fields: fields.collect(),
                choices: choices.collect(),
                open: None,
                history: self.history,
                focus,
            })
        });
    }

    /// Puts each changed value of the form in its feature. The change is
    /// one undo step, or a part of the undo step of a new feature.
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
        if self.new_feature.as_ref().is_some_and(|n| n.feature == f) {
            self.doc.project.set_attributes(f.layer, f.uuid, changed);
            self.pipeline.forget(f.layer);
            self.touch();
            return;
        }
        self.edit_attributes(f.layer, |project| {
            project.set_attributes(f.layer, f.uuid, changed)
        });
    }
}

/// The values of a field in the features of a layer, each one time and in
/// order. A field with more than `CHOICES` values gives none.
pub fn choices(layer: &Layer, name: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut values = Vec::new();
    for value in layer.features.iter().filter_map(|f| f.attributes.get(name)) {
        let Some(label) = value.label().filter(|l| !l.trim().is_empty()) else {
            continue;
        };
        if seen.insert(label) {
            if seen.len() > CHOICES {
                return Vec::new();
            }
            values.push(value);
        }
    }
    values.sort_by(|a, b| match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.total_cmp(b),
        (Value::Text(a), Value::Text(b)) => {
            (a.to_lowercase().cmp(&b.to_lowercase())).then_with(|| a.cmp(b))
        }
        _ => a.label().cmp(&b.label()),
    });
    values.iter().filter_map(|v| v.label()).collect()
}

/// The highlight of the selection and of the drag of the Select tool. The
/// selection shows in each tool.
pub fn paint(painter: &egui::Painter, state: &AppState) {
    let globe = &state.globe;
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    for f in &state.feature_selection {
        let Some(layer) = state.doc.layer(f.layer) else {
            continue;
        };
        let Some(feature) = layer.features.iter().find(|x| x.uuid() == f.uuid) else {
            continue;
        };
        paint_shape(painter, select::shape(globe, &feature.geometry));
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

/// Draws a selected feature in the accent color: a ring around each point,
/// a wide line on each line and each outline, and a handle at each end of a
/// line.
pub fn paint_shape(painter: &egui::Painter, shape: Shape) {
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    let line = egui::Stroke::new(3.0, theme::ACCENT);
    let dark = egui::Stroke::new(1.5, egui::Color32::from_rgb(11, 31, 26));
    for (_, pos) in &shape.points {
        painter.circle_stroke(*pos, 9.0, ring);
    }
    for run in shape.lines {
        painter.line(run, line);
    }
    // The ends of a line show as handles, so that the cut of a split shows
    // between two selected lines.
    for pos in shape.ends {
        painter.circle(pos, END, theme::ACCENT, dark);
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
/// Return goes to the next field, and on the last field it ends the edit.
/// `next`: the name of the next field.
fn read_field(
    resp: &egui::Response,
    focus: &mut Option<(String, u8)>,
    outcome: &mut Outcome,
    name: &str,
    next: Option<&String>,
) {
    if let Some((field, frames)) = focus
        && field == name
        && can_take_focus(&resp.ctx)
    {
        if *frames == 0 {
            *focus = None;
        } else {
            resp.request_focus();
            *frames -= 1;
        }
    }
    if resp.changed() {
        outcome.edited = Some(name.to_owned());
    }
    if !resp.lost_focus() {
        return;
    }
    outcome.commit = true;
    if resp.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        match next {
            Some(next) => *focus = Some((next.clone(), FOCUS_FRAMES)),
            None => outcome.end = true,
        }
    }
}

/// Puts a value of the list of a field in the field. The change is one
/// undo step.
fn pick(text: &mut String, choice: &str, outcome: &mut Outcome, name: &str) {
    choice.clone_into(text);
    outcome.commit = true;
    outcome.edited = Some(name.to_owned());
}

/// The menu of the values of a field on the desktop.
fn values_box(
    ui: &mut egui::Ui,
    name: &str,
    text: &mut String,
    choices: &[String],
    outcome: &mut Outcome,
) {
    let label = widgets::choices_name(name);
    let resp = egui::ComboBox::from_id_salt((INSPECTOR, name, "values"))
        .selected_text("")
        .width(0.0)
        .show_ui(ui, |ui| {
            for choice in choices {
                let on = text.trim() == choice;
                if ui.selectable_label(on, choice).clicked() {
                    pick(text, choice, outcome, name);
                }
            }
        })
        .response;
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &label));
    resp.on_hover_text(label);
}

/// What the inspector does after its frame.
#[derive(Default)]
struct Outcome {
    commit: bool,
    /// Return on the last field ends the edit.
    end: bool,
    show: bool,
    /// The field that the user edited.
    edited: Option<String>,
}

impl AppState {
    fn finish_inspector(&mut self, form: Option<SelectionForm>, outcome: Outcome) {
        if let (Some(name), Some(form)) = (outcome.edited, &form) {
            self.last_field = Some((form.feature.layer, name.clone()));
            self.attribute_focus = Some((form.feature, name));
        }
        self.selection_form = form;
        if outcome.commit {
            self.commit_selection_form();
        }
        if outcome.end {
            self.commit_new_feature();
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
            let names: Vec<String> = form.fields.iter().map(|f| f.name.clone()).collect();
            let (focus, open) = (&mut form.focus, &mut form.open);
            for (i, field) in form.fields.iter_mut().enumerate() {
                let valid = field.valid();
                let (name, text) = (&field.name, &mut field.text);
                let choices = &form.choices[i];
                let resp = if choices.is_empty() {
                    widgets::text_field(ui, controls, name, name, None, text, valid)
                } else {
                    let mut shown = open.as_ref() == Some(name);
                    let (resp, chosen) =
                        widgets::choice_field(ui, controls, name, text, valid, choices, &mut shown);
                    if let Some(chosen) = chosen {
                        pick(text, &choices[chosen], &mut outcome, name);
                    }
                    if shown {
                        *open = Some(name.clone());
                    } else if open.as_ref() == Some(name) {
                        *open = None;
                    }
                    resp
                };
                if resp.gained_focus() {
                    resp.scroll_to_me(None);
                }
                read_field(&resp, focus, &mut outcome, name, names.get(i + 1));
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

/// Takes the focus away from the field that has it, after a new feature.
/// The fields of each feature have the same IDs, so the field of the last
/// feature keeps the focus otherwise.
pub fn drop_focus(ctx: &egui::Context, state: &mut AppState) {
    if std::mem::take(&mut state.drop_focus) {
        ctx.memory_mut(|m| m.stop_text_input());
    }
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

/// The Edit menu of the desktop tool bar: Delete selection and Merge.
/// "Merge…" opens a window with a button for each selected feature, and a
/// click on a button merges the selection into that feature. Features with
/// the same attributes merge with no window.
pub fn edit_menu(ui: &mut egui::Ui, state: &mut AppState) {
    ui.menu_button("Edit", |ui| {
        let any = !state.feature_selection.is_empty();
        if ui.add_enabled(any, egui::Button::new(DELETE)).clicked() {
            state.delete_selection();
            ui.close();
        }
        if ui
            .add_enabled(state.can_merge(), egui::Button::new(MERGE))
            .clicked()
        {
            state.start_merge();
            ui.close();
        }
    });
    if !state.merging {
        return;
    }
    let choices = state.merge_choices();
    let mut open = !choices.is_empty();
    let mut keep = None;
    egui::Window::new("Merge")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
        .show(ui.ctx(), |ui| {
            ui.label(MERGE_TITLE);
            for (uuid, name) in &choices {
                if ui.button(name).clicked() {
                    keep = Some(*uuid);
                }
            }
        });
    state.merging = open;
    if let Some(uuid) = keep {
        state.merge_selection(uuid);
    }
}

/// The inspector of the desktop layout: a panel at the right.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    state.sync_selection_form(state.show_attributes);
    drop_focus(ui.ctx(), state);
    if !state.show_attributes {
        return;
    }
    let mut form = state.selection_form.take();
    let mut outcome = Outcome::default();
    let count = state.feature_selection.len();
    let mut focus_new = state.tools.focus_new;
    egui::Panel::right(INSPECTOR)
        .default_size(260.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(ATTRIBUTES);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut focus_new, FOCUS_NEW);
                });
            });
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
                    let names: Vec<String> = form.fields.iter().map(|f| f.name.clone()).collect();
                    let focus = &mut form.focus;
                    egui::Grid::new("attribute-fields")
                        .num_columns(2)
                        .show(ui, |ui| {
                            for (i, field) in form.fields.iter_mut().enumerate() {
                                ui.label(&field.name);
                                let color = (!field.valid()).then(|| ui.visuals().warn_fg_color);
                                let choices = &form.choices[i];
                                let resp = ui
                                    .horizontal(|ui| {
                                        let (name, text) = (&field.name, &mut field.text);
                                        let edit = egui::TextEdit::singleline(text)
                                            .id(egui::Id::new((INSPECTOR, name)))
                                            .text_color_opt(color);
                                        let resp = ui.add(edit);
                                        if choices.is_empty() {
                                            return resp;
                                        }
                                        values_box(ui, name, text, choices, &mut outcome);
                                        resp
                                    })
                                    .inner;
                                let (name, next) = (&field.name, names.get(i + 1));
                                read_field(&resp, focus, &mut outcome, name, next);
                                ui.end_row();
                            }
                        });
                }
                None => drop(ui.label(title(count))),
            }
            outcome.show = ui.button(SHOW_IN_TABLE).clicked();
        });
    state.tools.focus_new = focus_new;
    state.finish_inspector(form, outcome);
}
