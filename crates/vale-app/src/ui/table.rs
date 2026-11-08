//! The attribute table of the vector layers: the state of the table, the
//! edits of its cells, and the dock of the desktop layout. The project does
//! not save the state.

use std::collections::HashSet;

use eframe::egui;
use vale_store::{FieldType, FrameId, FrameRecord, LayerId, Uuid, Value};

use super::AppState;
use super::expr_help::{self, ExprHelp, ExprKind};
use super::new_feature::FormField;
use crate::globe::select::FeatureRef;
use crate::table::{RowKey, Rows, Sort};

/// The button of the page of a vector layer that opens the table.
pub const OPEN_TABLE: &str = "Open attribute table";
/// The button of the selection that opens the table of its layer.
pub const SHOW_IN_TABLE: &str = "Show in table";
/// The two items of the control that shows all rows or the selected rows.
pub const ALL: &str = "All";
pub const SELECTED: &str = "Selected";
/// The name of the filter field.
pub const FILTER: &str = "Filter";

/// The name of the header of a column, which sorts by its field.
pub fn sort_name(field: &str) -> String {
    format!("Sort by {field}")
}

/// The name of the cell of a field in the row of a feature.
pub fn cell_name(field: &str, uuid: Uuid) -> String {
    format!("{field} of {uuid}")
}

/// The width of a column of a field of this type.
pub fn column_width(kind: FieldType) -> f32 {
    match kind {
        FieldType::Text => 160.0,
        FieldType::Integer | FieldType::Real => 96.0,
        FieldType::Bool => 72.0,
    }
}

/// The edit of one cell.
pub struct CellEdit {
    pub feature: Uuid,
    pub field: FormField,
    /// The field takes the focus when it can.
    pub focus: bool,
}

pub struct TableState {
    pub layer: LayerId,
    pub sort: Option<Sort>,
    pub filter: String,
    /// Show only the selected rows.
    pub selected_only: bool,
    pub rows: Rows,
    pub edit: Option<CellEdit>,
    /// The row that the table scrolls to in the next frame.
    pub scroll_to: Option<Uuid>,
    /// The selection that the table saw in the last frame.
    seen: Vec<FeatureRef>,
    /// The rectangle of the table in the last frame.
    pub rect: Option<egui::Rect>,
    /// The scroll offset of the rows in the last frame.
    pub offset: f32,
}

impl TableState {
    fn new(layer: LayerId) -> TableState {
        TableState {
            layer,
            sort: None,
            filter: String::new(),
            selected_only: false,
            rows: Rows::default(),
            edit: None,
            scroll_to: None,
            seen: Vec::new(),
            rect: None,
            offset: 0.0,
        }
    }

    /// A tap on a header sorts up, then down, then not at all.
    pub fn tap_header(&mut self, field: &str) {
        self.sort = match self.sort.take() {
            Some(s) if s.field == field && s.ascending => Some(Sort {
                field: s.field,
                ascending: false,
            }),
            Some(s) if s.field == field => None,
            _ => Some(Sort {
                field: field.to_owned(),
                ascending: true,
            }),
        };
    }

    /// The direction of the sort by a field: true for up.
    pub fn sorted_up(&self, field: &str) -> Option<bool> {
        let sort = self.sort.as_ref().filter(|s| s.field == field)?;
        Some(sort.ascending)
    }
}

/// What a tap or a click on a cell does.
#[derive(Clone, Copy, PartialEq)]
pub enum CellTap {
    /// Selects the row alone, or starts the edit of a selected row.
    Plain,
    /// Adds the row to the selection, or removes it.
    Toggle,
    /// Adds the rows from the last selected row to this row.
    Range,
}

impl AppState {
    /// Opens the table of a vector layer and selects the layer.
    pub fn open_table(&mut self, layer: LayerId) {
        if self.doc.layer(layer).is_none() {
            return;
        }
        let pad_atlas =
            self.layout == super::Layout::Pad && self.workspace == super::Workspace::Atlas;
        let frame = self.table_frame(layer);
        if pad_atlas && frame.is_none() {
            return;
        }
        if self.selected_vector() != Some(layer) {
            self.select(Some(super::LayerRef::Vector(layer)));
        }
        let mut table = TableState::new(layer);
        table.scroll_to = self.first_selected_in(layer);
        table.seen = self.feature_selection.clone();
        self.table = Some(table);
        // In the iPad layout, the table is a page of the layer in the
        // Layers panel, or in the Layers tab of a frame in the Atlas.
        if let (true, Some(frame)) = (pad_atlas, frame) {
            let atlas = &mut self.atlas;
            atlas.selection = super::atlas::AtlasSelection::Frame(frame);
            atlas.tab = super::atlas::FrameTab::Layers;
            atlas.layer = Some((frame, layer));
            atlas.layer_page = Some(super::pad::Page::Table);
            // The frame panel shows the table, so the inspector closes.
            if self.pad.is_open(super::pad::Panel::Attributes) {
                self.pad.close(super::pad::Panel::Attributes);
            }
        } else if self.layout == super::Layout::Pad {
            self.pad.open(super::pad::Panel::Layers);
            self.pad.layer = Some(super::LayerRef::Vector(layer));
            self.pad.page = Some(super::pad::Page::Table);
        }
    }

    /// The first selected feature of `layer`.
    fn first_selected_in(&self, layer: LayerId) -> Option<Uuid> {
        let mut selection = self.feature_selection.iter();
        selection.find(|f| f.layer == layer).map(|f| f.uuid)
    }

    /// The frame whose Layers tab shows the table of `layer` in the Atlas:
    /// the selected frame, or else the top frame of the page whose map
    /// shows the layer.
    fn table_frame(&self, layer: LayerId) -> Option<FrameId> {
        if let Some(frame) = self.atlas.frame() {
            return Some(frame);
        }
        let page = super::atlas::open_page(self);
        let shows = |f: &&FrameRecord| self.doc.shown(f.map, layer);
        page.frames.iter().rev().find(shows).map(|f| f.id)
    }

    /// True if "Show in table" can open the table of the selection.
    pub fn can_show_in_table(&self) -> bool {
        let Some(f) = self.feature_selection.first() else {
            return false;
        };
        self.layout != super::Layout::Pad
            || self.workspace != super::Workspace::Atlas
            || self.table_frame(f.layer).is_some()
    }

    pub fn close_table(&mut self) {
        self.commit_cell();
        self.table = None;
    }

    /// Opens the table of the layer of the selection, at the first selected
    /// row.
    pub fn show_in_table(&mut self) {
        if let Some(f) = self.feature_selection.first().copied() {
            self.open_table(f.layer);
        }
    }

    /// Keeps the table on the selected vector layer, and scrolls it to a
    /// selection that the table did not make. A table of a deleted layer
    /// closes.
    pub fn sync_table(&mut self) {
        let Some(table) = &self.table else {
            return;
        };
        if self.doc.layer(table.layer).is_none() {
            self.table = None;
            return;
        }
        // On the desktop, the dock follows the selected vector layer. The
        // page of the iPad layout closes with its layer page.
        if let Some(id) = self.selected_vector()
            && id != table.layer
            && self.layout != super::Layout::Pad
        {
            self.commit_cell();
            self.open_table(id);
        }
        let selection = self.feature_selection.clone();
        let Some(table) = &mut self.table else {
            return;
        };
        if table.seen != selection {
            let first = selection.iter().find(|f| f.layer == table.layer);
            table.scroll_to = first.map(|f| f.uuid);
            table.seen = selection.clone();
            let gone = table
                .edit
                .as_ref()
                .is_some_and(|e| !selection.iter().any(|f| f.uuid == e.feature));
            if gone {
                self.commit_cell();
            }
        }
    }

    /// The rows of the table, built again if the layer, the sort, the
    /// filter, or the shown selection changed.
    pub fn table_rows(&mut self) -> Option<&[usize]> {
        let table = self.table.as_mut()?;
        let layer = self.doc.project.layer(table.layer)?;
        let selected = table.selected_only.then(|| {
            let ids = self.feature_selection.iter();
            ids.filter(|f| f.layer == table.layer)
                .map(|f| f.uuid)
                .collect()
        });
        let key = RowKey {
            layer: table.layer,
            revision: (self.line_edits, self.history),
            len: layer.features.len(),
            sort: table.sort.clone(),
            filter: table.filter.clone(),
            selected,
        };
        Some(table.rows.update(key, layer))
    }

    /// The selected features of the layer of the table.
    pub fn table_selection(&self) -> HashSet<Uuid> {
        let Some(table) = &self.table else {
            return HashSet::new();
        };
        let ids = self.feature_selection.iter();
        ids.filter(|f| f.layer == table.layer)
            .map(|f| f.uuid)
            .collect()
    }

    /// A tap or a click on the cell of `field` in the row of `uuid`.
    pub fn tap_cell(&mut self, uuid: Uuid, field: &str, tap: CellTap) {
        let Some(layer) = self.table.as_ref().map(|t| t.layer) else {
            return;
        };
        let editing = self
            .table
            .as_ref()
            .and_then(|t| t.edit.as_ref())
            .is_some_and(|e| e.feature == uuid && e.field.name == field);
        if editing {
            return;
        }
        self.commit_cell();
        let f = FeatureRef { layer, uuid };
        let selected = self.feature_selection.contains(&f);
        let selection = match tap {
            CellTap::Plain if selected => {
                self.start_edit(uuid, field);
                return;
            }
            CellTap::Plain => vec![f],
            CellTap::Toggle if selected => {
                let mut s = self.feature_selection.clone();
                s.retain(|x| *x != f);
                s
            }
            CellTap::Toggle => {
                let mut s = self.feature_selection.clone();
                s.push(f);
                s
            }
            CellTap::Range => self.range_to(f),
        };
        self.set_feature_selection(selection);
        if let Some(table) = &mut self.table {
            table.seen = self.feature_selection.clone();
        }
    }

    /// The selection with the rows from the last selected row to `f`, in
    /// the order of the table.
    fn range_to(&mut self, f: FeatureRef) -> Vec<FeatureRef> {
        let last = self.feature_selection.last().map(|x| x.uuid);
        let mut selection = self.feature_selection.clone();
        let (Some(layer), Some(rows)) = (
            self.doc.layer(f.layer).map(|l| l.id),
            self.table.as_ref().map(|t| t.rows.rows().to_vec()),
        ) else {
            return vec![f];
        };
        let Some(features) = self.doc.layer(layer).map(|l| &l.features) else {
            return vec![f];
        };
        let at = |uuid: Uuid| rows.iter().position(|&i| features[i].uuid() == uuid);
        let (Some(a), Some(b)) = (last.and_then(at), at(f.uuid)) else {
            return vec![f];
        };
        for &i in &rows[a.min(b)..=a.max(b)] {
            let x = FeatureRef {
                layer,
                uuid: features[i].uuid(),
            };
            if !selection.contains(&x) {
                selection.push(x);
            }
        }
        selection
    }

    fn start_edit(&mut self, uuid: Uuid, name: &str) {
        let Some(table) = &self.table else {
            return;
        };
        let Some(layer) = self.doc.layer(table.layer) else {
            return;
        };
        let Some(field) = layer.fields.iter().find(|f| f.name == name) else {
            return;
        };
        let Some(feature) = layer.features.iter().find(|f| f.uuid() == uuid) else {
            return;
        };
        let value = feature.attributes.get(name);
        let text = value.and_then(|v| v.label()).unwrap_or_default();
        let edit = CellEdit {
            feature: uuid,
            field: FormField::new(field, text),
            focus: true,
        };
        if let Some(table) = &mut self.table {
            table.edit = Some(edit);
        }
    }

    /// The number of rows that the edit of a cell sets: all selected rows
    /// if the row is one of them, else the row alone.
    pub fn edit_targets(&self, uuid: Uuid) -> Vec<Uuid> {
        let selection = self.table_selection();
        if selection.contains(&uuid) {
            selection.into_iter().collect()
        } else {
            vec![uuid]
        }
    }

    /// Ends the edit of a cell. A valid value goes to the row, or to all
    /// selected rows if the row is selected, in one undo step.
    pub fn commit_cell(&mut self) {
        let Some(table) = &mut self.table else {
            return;
        };
        let Some(edit) = table.edit.take() else {
            return;
        };
        let layer = table.layer;
        if !edit.field.valid() {
            return;
        }
        let targets = self.edit_targets(edit.feature);
        self.set_field_of(layer, &targets, &edit.field.name, &edit.field.value());
    }

    /// Ends the edit of a cell and drops its value.
    pub fn cancel_cell(&mut self) {
        if let Some(table) = &mut self.table {
            table.edit = None;
        }
    }

    /// Sets one field of the features with these UUIDs, in one undo step.
    /// No step is made if no value changes.
    pub fn set_field_of(&mut self, layer: LayerId, uuids: &[Uuid], name: &str, value: &Value) {
        let set: HashSet<Uuid> = uuids.iter().copied().collect();
        let Some(l) = self.doc.layer(layer) else {
            return;
        };
        let Some(field) = l.fields.iter().find(|f| f.name == name) else {
            return;
        };
        let value = field.kind.convert(value);
        let changes = l
            .features
            .iter()
            .any(|f| set.contains(&f.uuid()) && f.attributes.get(name) != Some(&value));
        if !changes {
            return;
        }
        self.edit_attributes(layer, |project| {
            project.set_attribute_of(layer, &set, name, &value) > 0
        });
    }

    /// Makes one undo step of a change of attributes. The table and the
    /// form of the selection both make their changes here. `change` gives
    /// true if it changed the layer.
    pub(super) fn edit_attributes(
        &mut self,
        layer: LayerId,
        change: impl FnOnce(&mut vale_store::Project) -> bool,
    ) {
        self.commit_new_feature();
        self.save_edit();
        if change(&mut self.doc.project) {
            // The form of the selection takes the new values.
            self.selection_form = None;
            self.end_vector_edit(layer);
        }
    }
}

/// Gives the field the focus, with all of its text selected, so that the
/// typed text takes the place of the old value.
pub fn focus_all(resp: &egui::Response, text: &str) {
    use egui::text::{CCursor, CCursorRange};
    resp.request_focus();
    let ctx = &resp.ctx;
    let mut edit = egui::TextEdit::load_state(ctx, resp.id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    edit.cursor.set_char_range(Some(all));
    edit.store(ctx, resp.id);
}

/// The arrow of the sorted column, a small triangle around `center`.
pub fn paint_arrow(painter: &egui::Painter, center: egui::Pos2, up: bool, color: egui::Color32) {
    let (w, h) = (4.5, 3.0);
    let tip = if up { -h } else { h };
    let points = vec![
        center + egui::vec2(-w, -tip),
        center + egui::vec2(w, -tip),
        center + egui::vec2(0.0, tip),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// The text of a value in a cell.
pub fn cell_text(value: Option<&Value>) -> String {
    value.and_then(|v| v.label()).unwrap_or_default()
}

/// The dock of the table below the canvas, in the desktop layout.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    state.sync_table();
    if state.table.is_none() {
        return;
    }
    let panel = egui::Panel::bottom("attribute-table")
        .resizable(true)
        .default_size(260.0)
        .show(ui, |ui| desktop_table(ui, state));
    if let Some(table) = &mut state.table {
        table.rect = Some(panel.response.rect);
    }
}

fn desktop_table(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(rows) = state.table_rows().map(<[usize]>::to_vec) else {
        return;
    };
    let selection = state.table_selection();
    let help = match &state.table {
        Some(table) => ExprHelp::for_layer(state, table.layer),
        None => return,
    };
    let Some(table) = state.table.as_mut() else {
        return;
    };
    let Some(layer) = state.doc.project.layer(table.layer) else {
        return;
    };
    let mut close = false;
    let mut tap = None;
    let mut header = None;
    let mut commit = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.strong(&layer.name);
        ui.weak(count_text(
            rows.len(),
            layer.features.len(),
            selection.len(),
        ));
        ui.separator();
        ui.label(FILTER);
        let color = table.rows.error().map(|_| ui.visuals().warn_fg_color);
        let id = egui::Id::new(("table", FILTER));
        let edit = egui::TextEdit::singleline(&mut table.filter)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .hint_text("name = 'Ardent'")
            .text_color_opt(color)
            .desired_width(220.0);
        ui.add(edit);
        expr_help::insert_menu(ui, &help, id, &mut table.filter);
        ui.selectable_value(&mut table.selected_only, false, ALL);
        ui.selectable_value(&mut table.selected_only, true, SELECTED);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button("Close").clicked();
        });
    });
    expr_help::desktop_line(ui, &help, &table.filter, ExprKind::Filter);
    ui.separator();
    let row_h = 22.0;
    let widths: Vec<f32> = layer.fields.iter().map(|f| column_width(f.kind)).collect();
    egui::ScrollArea::horizontal()
        .id_salt("table-h")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (field, &w) in layer.fields.iter().zip(&widths) {
                    let text = egui::RichText::new(&field.name).strong();
                    let button = egui::Button::new(text)
                        .frame(false)
                        .min_size(egui::vec2(w, row_h));
                    let resp = ui.add_sized([w, row_h], button);
                    if resp.clicked() {
                        header = Some(field.name.clone());
                    }
                    if let Some(up) = table.sorted_up(&field.name) {
                        let at = egui::pos2(resp.rect.right() - 10.0, resp.rect.center().y);
                        let color = ui.visuals().strong_text_color();
                        paint_arrow(ui.painter(), at, up, color);
                    }
                }
            });
            if let Some(message) = empty_text(layer.fields.is_empty(), rows.len(), table) {
                ui.weak(message);
                return;
            }
            let mut area = egui::ScrollArea::vertical()
                .id_salt("table-v")
                .auto_shrink([false, false]);
            if let Some(i) = scroll_index(table, &rows, layer) {
                let view = ui.available_height();
                area = area.vertical_scroll_offset((i as f32 * row_h - view / 2.0).max(0.0));
            }
            area.show_rows(ui, row_h, rows.len(), |ui, range| {
                for &i in &rows[range] {
                    let feature = &layer.features[i];
                    let uuid = feature.uuid();
                    let on = selection.contains(&uuid);
                    ui.horizontal(|ui| {
                        for (field, &w) in layer.fields.iter().zip(&widths) {
                            let size = egui::vec2(w, row_h);
                            let editing = table
                                .edit
                                .as_mut()
                                .filter(|e| e.feature == uuid && e.field.name == field.name);
                            if let Some(edit) = editing {
                                let color =
                                    (!edit.field.valid()).then(|| ui.visuals().warn_fg_color);
                                let text = egui::TextEdit::singleline(&mut edit.field.text)
                                    .id(egui::Id::new(("table-cell", &field.name)))
                                    .text_color_opt(color);
                                let resp = ui.add_sized(size, text);
                                if std::mem::take(&mut edit.focus) {
                                    focus_all(&resp, &edit.field.text);
                                }
                                if resp.lost_focus() {
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        cancel = true;
                                    } else {
                                        commit = true;
                                    }
                                }
                                continue;
                            }
                            let text = cell_text(feature.attributes.get(&field.name));
                            let label = egui::Button::selectable(on, text).min_size(size);
                            let resp = ui.add_sized(size, label);
                            if resp.clicked() {
                                let m = ui.input(|i| i.modifiers);
                                let kind = if m.command {
                                    CellTap::Toggle
                                } else if m.shift {
                                    CellTap::Range
                                } else {
                                    CellTap::Plain
                                };
                                tap = Some((uuid, field.name.clone(), kind));
                            }
                        }
                    });
                }
            });
        });
    table.scroll_to = None;
    if let Some(field) = header {
        table.tap_header(&field);
    }
    if cancel {
        state.cancel_cell();
    } else if commit {
        state.commit_cell();
    }
    if let Some((uuid, field, kind)) = tap {
        state.tap_cell(uuid, &field, kind);
    }
    if close {
        state.close_table();
    }
}

/// The count in the title row: the rows that show, and the selection.
pub fn count_text(rows: usize, features: usize, selected: usize) -> String {
    let shown = if rows == features {
        format!("{features}")
    } else {
        format!("{rows} of {features}")
    };
    if selected > 0 {
        format!("{shown} · {selected} selected")
    } else {
        shown
    }
}

/// The text of a table with no rows.
pub fn empty_text(no_fields: bool, rows: usize, table: &TableState) -> Option<String> {
    if no_fields {
        return Some("The layer has no fields.".into());
    }
    if rows > 0 {
        return None;
    }
    Some(match table.rows.error() {
        Some(error) => error.to_owned(),
        None if table.selected_only => "No selected rows match.".into(),
        None => "No rows match.".into(),
    })
}

/// The index in the rows of the row that the table scrolls to.
pub fn scroll_index(
    table: &TableState,
    rows: &[usize],
    layer: &vale_store::Layer,
) -> Option<usize> {
    let uuid = table.scroll_to?;
    rows.iter().position(|&i| layer.features[i].uuid() == uuid)
}
