//! The Vertex tool of the world workspace: the handles of the vertices of
//! one line or polygon, the move, the insert, the delete, and the smooth.

use eframe::egui::{self, Pos2};
use vale_store::{Geometry, LonLat, Uuid};

use super::AppState;
use super::pad::geometry::TOUCH;
use super::pad::theme;
use crate::globe::math::V3;
use crate::globe::select::{self, FeatureRef, Pointer};
use crate::globe::vertex::{self, VertexEvent};

/// The distance in points at which a pointer takes a handle. The target of
/// a handle is a touch of 44 points.
pub const HANDLE_REACH: f32 = TOUCH / 2.0;
/// The radius of a handle on the screen.
const HANDLE: f32 = 4.5;
/// The radius of a selected handle on the screen.
const SELECTED: f32 = 6.0;

/// A drag of the Vertex tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VertexDrag {
    /// A box from `start` to `end`, in screen points.
    Box { start: Pos2, end: Pos2 },
    /// A move of the vertex at `from` to the snapped position `to`, with
    /// the stored position of a vertex that it snaps to. With `group`, the
    /// other selected vertices move with it.
    Move {
        feature: FeatureRef,
        from: LonLat,
        to: Option<(V3, Option<LonLat>)>,
        group: bool,
    },
}

impl AppState {
    /// The feature that the Vertex tool edits: the one selected line or
    /// polygon of the selected layer.
    pub fn vertex_feature(&self) -> Option<(FeatureRef, &Geometry)> {
        let [f] = self.feature_selection[..] else {
            return None;
        };
        let layer = self.select_layer().filter(|l| l.id == f.layer)?;
        let feature = layer.features.iter().find(|x| x.uuid() == f.uuid)?;
        match feature.geometry {
            Geometry::Points(_) => None,
            _ => Some((f, &feature.geometry)),
        }
    }

    /// Puts the screen positions of the handles in the globe, so the press
    /// knows a touch on a handle as it starts.
    pub fn refresh_vertex_handles(&mut self) {
        let handles = match self.vertex_feature() {
            Some((_, g)) if self.globe.vertexing() => vertex::handles(&self.globe, g),
            _ => Vec::new(),
        };
        self.globe.vertex.handles = handles.into_iter().map(|h| h.1).collect();
    }

    /// Reads the taps and the drags of the Vertex tool.
    pub fn poll_vertex(&mut self) {
        let events = std::mem::take(&mut self.globe.vertex.press.events);
        if !self.globe.vertexing() {
            self.vertex_drag = None;
            self.vertex_selection.clear();
            return;
        }
        // An undo can take the selected vertices away.
        match self.vertex_feature() {
            Some((_, g)) => {
                let g = g.clone();
                self.vertex_selection.retain(|&v| vertex::has_vertex(&g, v));
            }
            None => self.vertex_selection.clear(),
        }
        for event in events {
            match event {
                VertexEvent::Tap { pos, pointer } => self.vertex_tap(pos, pointer),
                VertexEvent::DragStart { pos, .. } => {
                    let grabbed = self.vertex_feature().and_then(|(f, g)| {
                        let handles = vertex::handles(&self.globe, g);
                        Some((f, vertex::hit_handle(&handles, pos, HANDLE_REACH)?))
                    });
                    self.vertex_drag = Some(match grabbed {
                        Some((feature, from)) => {
                            // A selected vertex moves the selection, and
                            // another vertex moves alone.
                            let selected = self.vertex_selection.iter();
                            let group = selected.clone().any(|&v| vertex::same(v, from))
                                && self.vertex_selection.len() > 1;
                            if !group {
                                self.vertex_selection = vec![from];
                            }
                            VertexDrag::Move {
                                feature,
                                from,
                                to: None,
                                group,
                            }
                        }
                        None => VertexDrag::Box {
                            start: pos,
                            end: pos,
                        },
                    });
                }
                VertexEvent::Drag { pos } => self.vertex_drag_to(pos),
                VertexEvent::DragEnd { pos } => {
                    self.vertex_drag_to(pos);
                    match self.vertex_drag.take() {
                        Some(VertexDrag::Box { start, end }) => {
                            let rect = select::drag_rect(start, end);
                            self.vertex_selection = match self.vertex_feature() {
                                Some((_, g)) => {
                                    vertex::in_box(&vertex::handles(&self.globe, g), rect)
                                }
                                None => Vec::new(),
                            };
                        }
                        Some(VertexDrag::Move {
                            feature,
                            from,
                            to: Some(to),
                            group,
                        }) => {
                            let moves = self.drag_moves(from, to, group);
                            let froms: Vec<LonLat> = moves.iter().map(|m| m.0).collect();
                            let scale = self.globe.radius();
                            let op = |g: &Geometry| vertex::move_vertices(g, &moves, scale);
                            if self.edit_vertices(feature, &froms, op) {
                                self.vertex_selection = moves.iter().map(|m| m.1).collect();
                            }
                        }
                        _ => {}
                    }
                }
                VertexEvent::Cancel => self.vertex_drag = None,
            }
        }
    }

    /// A tap selects a handle, adds a vertex on an edge, or chooses the
    /// feature to edit.
    fn vertex_tap(&mut self, pos: Pos2, pointer: Pointer) {
        let reach = pointer.reach();
        if let Some((f, g)) = self.vertex_feature() {
            let handles = vertex::handles(&self.globe, g);
            if let Some(v) = vertex::hit_handle(&handles, pos, HANDLE_REACH) {
                self.vertex_selection = vec![v];
                return;
            }
            if let Some((a, b, at)) = vertex::hit_edge(&self.globe, g, pos, reach) {
                let scale = self.globe.radius();
                let op = |g: &Geometry| vertex::insert_vertex(g, a, b, at, scale);
                if self.edit_vertices(f, &[a, b], op) {
                    self.vertex_selection = vec![at];
                }
                return;
            }
        }
        let layers: Vec<_> = self.select_layer().into_iter().collect();
        let hit = select::hit(&self.globe, &layers, pos, reach);
        let editable = hit.filter(|h| {
            let layer = layers.iter().find(|l| l.id == h.layer);
            let feature = layer.and_then(|l| l.features.iter().find(|x| x.uuid() == h.uuid));
            feature.is_some_and(|x| !matches!(x.geometry, Geometry::Points(_)))
        });
        match editable {
            Some(h) => {
                if self.feature_selection != [h] {
                    self.set_feature_selection(vec![h]);
                }
                self.vertex_selection.clear();
            }
            None if !self.vertex_selection.is_empty() => self.vertex_selection.clear(),
            None => self.set_feature_selection(Vec::new()),
        }
    }

    fn vertex_drag_to(&mut self, pos: Pos2) {
        match &mut self.vertex_drag {
            Some(VertexDrag::Box { end, .. }) => *end = pos,
            Some(VertexDrag::Move {
                feature, from, to, ..
            }) => {
                let (from, uuid) = (*from, feature.uuid);
                *to = (self.globe.unproject(pos)).map(|d| self.globe.vertex_snap(d, from, uuid));
            }
            None => {}
        }
    }

    /// The moves of a drag of the vertex at `from` to `to`: the vertex, and
    /// with `group`, the other selected vertices.
    fn drag_moves(
        &self,
        from: LonLat,
        to: (V3, Option<LonLat>),
        group: bool,
    ) -> Vec<(LonLat, LonLat)> {
        let group = if group {
            &self.vertex_selection[..]
        } else {
            &[]
        };
        vertex::group_moves(from, to, group)
    }

    /// The features that an edit of the feature `f` changes: `f`, and with
    /// "Move shared vertices" on, each feature of the layer with a vertex at
    /// one of the positions `shared`.
    fn vertex_targets(&self, f: FeatureRef, shared: &[LonLat]) -> Vec<Uuid> {
        let mut out = vec![f.uuid];
        if !self.globe.vertex.shared {
            return out;
        }
        let Some(layer) = self.doc.layer(f.layer) else {
            return out;
        };
        for &at in shared {
            for uuid in vertex::sharing(layer, at, f.uuid) {
                if !out.contains(&uuid) {
                    out.push(uuid);
                }
            }
        }
        out
    }

    /// Applies `op` to the feature `f` and to the features that share the
    /// positions `shared`, in one undo step. A feature where `op` gives
    /// `None` keeps its geometry. If `f` keeps its geometry, nothing
    /// changes. Returns true if the layer changed.
    fn edit_vertices(
        &mut self,
        f: FeatureRef,
        shared: &[LonLat],
        op: impl Fn(&Geometry) -> Option<Geometry>,
    ) -> bool {
        let targets = self.vertex_targets(f, shared);
        let Some(layer) = self.doc.layer(f.layer) else {
            return false;
        };
        let changes: Vec<(Uuid, Geometry)> = targets
            .iter()
            .filter_map(|&uuid| {
                let feature = layer.features.iter().find(|x| x.uuid() == uuid)?;
                Some((uuid, op(&feature.geometry)?))
            })
            .collect();
        if changes.first().is_none_or(|c| c.0 != f.uuid) {
            return false;
        }
        self.commit_selection_form();
        self.commit_point_form();
        self.save_edit();
        let mut changed = false;
        for (uuid, geometry) in changes {
            changed |= self.doc.project.set_geometry(f.layer, uuid, geometry);
        }
        if changed {
            self.end_vector_edit(f.layer);
        }
        changed
    }

    /// True if "Delete vertex" can delete the selected vertices.
    pub fn can_delete_vertices(&self) -> bool {
        self.globe.vertexing() && !self.vertex_selection.is_empty()
    }

    /// True if "Smooth vertices" can smooth the selected vertices.
    pub fn can_smooth_vertices(&self) -> bool {
        self.globe.vertexing() && self.vertex_selection.len() >= 2
    }

    /// Deletes the selected vertices in one undo step.
    pub fn delete_vertices(&mut self) {
        let Some((f, _)) = self.vertex_feature() else {
            return;
        };
        let selected = self.vertex_selection.clone();
        let scale = self.globe.radius();
        let op = |g: &Geometry| vertex::delete_vertices(g, &selected, scale);
        if self.edit_vertices(f, &selected, op) {
            self.vertex_selection.clear();
        }
    }

    /// Smooths the part of the edited feature between the selected vertices
    /// in one undo step.
    pub fn smooth_vertices(&mut self) {
        let Some((f, _)) = self.vertex_feature() else {
            return;
        };
        let selected = self.vertex_selection.clone();
        let (scale, strength) = (self.globe.radius(), self.globe.line.smoothing);
        let op = |g: &Geometry| vertex::smooth_vertices(g, &selected, scale, strength);
        self.edit_vertices(f, &[], op);
    }
}

/// The handles of the edited feature, the drag of the Vertex tool, and the
/// features after the move that the drag makes.
pub fn paint(painter: &egui::Painter, state: &AppState) {
    let globe = &state.globe;
    if !globe.vertexing() {
        return;
    }
    let Some((f, geometry)) = state.vertex_feature() else {
        return;
    };
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    let dark = egui::Stroke::new(1.5, egui::Color32::from_rgb(11, 31, 26));
    let mut shown = geometry.clone();
    let mut selected = state.vertex_selection.clone();
    if let Some(VertexDrag::Move {
        from,
        to: Some((dir, exact)),
        group,
        ..
    }) = state.vertex_drag
    {
        let moves = state.drag_moves(from, (dir, exact), group);
        let froms: Vec<LonLat> = moves.iter().map(|m| m.0).collect();
        let scale = globe.radius();
        let line = egui::Stroke::new(3.0, theme::ACCENT);
        let layer = state.doc.layer(f.layer);
        for uuid in state.vertex_targets(f, &froms) {
            let feature = layer.and_then(|l| l.features.iter().find(|x| x.uuid() == uuid));
            let Some(moved) =
                feature.and_then(|x| vertex::move_vertices(&x.geometry, &moves, scale))
            else {
                continue;
            };
            for run in select::shape(globe, &moved).lines {
                painter.line(run, line);
            }
            if uuid == f.uuid {
                shown = moved;
            }
        }
        selected = moves.iter().map(|m| m.1).collect();
        if exact.is_some()
            && let Some(pos) = globe.project(dir)
        {
            painter.circle_stroke(pos, 9.0, ring);
        }
    }
    for (v, pos) in vertex::handles(globe, &shown) {
        if selected.iter().any(|&s| vertex::same(s, v)) {
            painter.circle(pos, SELECTED, egui::Color32::WHITE, ring);
        } else {
            painter.circle(pos, HANDLE, theme::ACCENT, dark);
        }
    }
    if let Some(VertexDrag::Box { start, end }) = state.vertex_drag {
        let rect = select::drag_rect(start, end);
        let fill = theme::ACCENT.gamma_multiply(0.15);
        painter.rect(rect, 0.0, fill, ring, egui::StrokeKind::Inside);
    }
}

/// Reads the keys of the Vertex tool on the desktop: A chooses the tool,
/// Delete and Backspace delete the selected vertices, and Escape clears
/// them.
pub fn keys(ui: &egui::Ui, state: &mut AppState) {
    use egui::{Key, Modifiers};
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let key = |key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if key(Key::A) && (state.globe.lineable || state.globe.fillable) {
        state.globe.tool = crate::globe::Tool::Vertex;
    }
    if !state.globe.vertexing() {
        return;
    }
    if key(Key::Delete) || key(Key::Backspace) {
        state.delete_vertices();
    }
    if key(Key::Escape) {
        state.vertex_selection.clear();
    }
}
