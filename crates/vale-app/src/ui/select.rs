//! The Select tool of the world workspace: the selection of features, the
//! move of a point, the delete, and the card that edits the attributes.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Order, Pos2, Rect, pos2, vec2};
use vale_store::{Geometry, Layer, LayerId, Uuid};

use super::AppState;
use super::pad::geometry::{GAP, MARGIN, ROW, Rects};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::point_form::{FormField, keyboard_top};
use crate::globe::math::{V3, dir_to_lonlat};
use crate::globe::select::{self, FeatureRef, SelectEvent};

/// The name of the Select tool.
pub const TOOL: &str = "Select";
/// The button that deletes the selected features.
pub const DELETE: &str = "Delete selection";
const WIDTH: f32 = 360.0;
const BOTTOM: f32 = 12.0;
const CARD: &str = "selection-card";

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
}

/// The title of the card of a selection of `count` features.
pub fn title(count: usize) -> String {
    format!("{count} features")
}

impl AppState {
    /// The vector layers that the Select tool can select in, top first.
    fn select_layers(&self) -> Vec<&Layer> {
        let layers = self.doc.project.layers().iter().rev();
        layers
            .filter(|l| l.visible && l.geometry().is_some())
            .collect()
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
            self.feature_selection = selection;
        }
    }

    /// Keeps only the selected features that still exist.
    fn prune_feature_selection(&mut self) {
        if self.feature_selection.is_empty() {
            return;
        }
        let mut live: HashMap<LayerId, HashSet<Uuid>> = HashMap::new();
        let doc = &self.doc;
        self.feature_selection.retain(|f| {
            let uuids = live.entry(f.layer).or_insert_with(|| {
                let layer = doc.layer(f.layer);
                let features = layer.into_iter().flat_map(|l| &l.features);
                features.map(|feature| feature.uuid()).collect()
            });
            uuids.contains(&f.uuid)
        });
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

    /// Deletes the selected features of all layers in one undo step.
    pub fn delete_selection(&mut self) {
        if self.feature_selection.is_empty() {
            return;
        }
        self.commit_selection_form();
        self.selection_form = None;
        self.commit_point_form();
        self.save_edit();
        let selection = std::mem::take(&mut self.feature_selection);
        let mut by_layer: Vec<(LayerId, Vec<Uuid>)> = Vec::new();
        for f in selection {
            match by_layer.iter_mut().find(|(id, _)| *id == f.layer) {
                Some((_, uuids)) => uuids.push(f.uuid),
                None => by_layer.push((f.layer, vec![f.uuid])),
            }
        }
        let mut last = None;
        for (id, uuids) in by_layer {
            if self.doc.project.remove_features(id, &uuids) > 0 {
                self.pipeline.forget(id);
                last = Some(id);
            }
        }
        if let Some(id) = last {
            self.end_vector_edit(id);
        }
    }

    /// Opens the form of the one selected feature, or closes it.
    fn sync_selection_form(&mut self) {
        let one = match self.feature_selection.as_slice() {
            [one] if self.globe.selecting() => Some(*one),
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
            Some(SelectionForm {
                feature: f,
                fields: fields.collect(),
                history: self.history,
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
        self.commit_point_form();
        self.save_edit();
        if self.doc.project.set_attributes(f.layer, f.uuid, changed) {
            self.end_vector_edit(f.layer);
        }
    }
}

/// The highlight of the selection and of the drag of the Select tool.
pub fn paint(painter: &egui::Painter, state: &AppState) {
    let globe = &state.globe;
    if !globe.selecting() {
        return;
    }
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    let line = egui::Stroke::new(3.0, theme::ACCENT);
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

/// The title of the card or the window, and the count of the selection.
fn heading(state: &AppState) -> String {
    match &state.selection_form {
        Some(form) => state
            .doc
            .layer(form.feature.layer)
            .map_or_else(String::new, |l| l.name.clone()),
        None => title(state.feature_selection.len()),
    }
}

/// The card of the selection in the iPad layout, near the top of the
/// screen. It shows the fields of one selected feature, and the delete
/// button.
pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    state.sync_selection_form();
    if !state.globe.selecting() || state.feature_selection.is_empty() {
        return;
    }
    let title = heading(state);
    let mut form = state.selection_form.take();
    let screen = rects.screen;
    let top = rects.workspace.bottom() + GAP;
    let room = keyboard_top(screen) - GAP - top;
    let fields = form.as_ref().map_or(0, |f| f.fields.len()) as f32;
    let least = ROW + PANEL_ROW + BOTTOM;
    let height = (ROW + (fields + 1.0) * PANEL_ROW + BOTTOM).min(room.max(least));
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, top),
        vec2(width, height),
    );
    let (mut close, mut delete, mut commit) = (false, false, false);
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, CARD, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        ui.spacing_mut().item_spacing.y = 0.0;
        close = widgets::title_row(ui, controls, &title, None, false).close;
        let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        egui::ScrollArea::vertical().show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for field in form.iter_mut().flat_map(|f| &mut f.fields) {
                let valid = field.valid();
                let (name, text) = (&field.name, &mut field.text);
                let resp = widgets::text_field(ui, controls, name, name, None, text, valid);
                commit |= resp.lost_focus();
            }
            delete = widgets::button_row_in(ui, controls, DELETE, true, theme::WARN).clicked();
        });
    });
    state.selection_form = form;
    if commit {
        state.commit_selection_form();
    }
    if delete {
        state.delete_selection();
    } else if close {
        state.set_feature_selection(Vec::new());
    }
}

/// The window of the selection in the desktop layout.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    state.sync_selection_form();
    if !state.globe.selecting() || state.feature_selection.is_empty() {
        return;
    }
    let title = heading(state);
    let mut form = state.selection_form.take();
    let (mut delete, mut commit) = (false, false);
    egui::Window::new("Selection")
        .id(egui::Id::new(CARD))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, vec2(-12.0, 48.0))
        .show(ctx, |ui| {
            ui.strong(&title);
            egui::Grid::new("selection-fields")
                .num_columns(2)
                .show(ui, |ui| {
                    for field in form.iter_mut().flat_map(|f| &mut f.fields) {
                        ui.label(&field.name);
                        let color = (!field.valid()).then(|| ui.visuals().warn_fg_color);
                        let edit = egui::TextEdit::singleline(&mut field.text)
                            .id(egui::Id::new((CARD, &field.name)))
                            .text_color_opt(color);
                        commit |= ui.add(edit).lost_focus();
                        ui.end_row();
                    }
                });
            delete = ui.button(DELETE).clicked();
        });
    state.selection_form = form;
    if commit {
        state.commit_selection_form();
    }
    if delete {
        state.delete_selection();
    }
}
