//! The attribute table in the iPad layout: a page of the layer in the
//! Layers panel. The panel is wider while the page shows. The selection
//! card stays at the top, so the two show at the same time.

use eframe::egui::{self, Align2, Rect, pos2, vec2};
use vale_store::{FieldType, Uuid};

use super::geometry::{GAP, PAD, Rects, TOUCH};
use super::theme;
use super::widgets::{self, Controls, HIGHLIGHT_INSET, PANEL_ROW};
use super::{LayerRef, Page, Panel};
use crate::ui::expr_help::{ExprHelp, ExprKind};
use crate::ui::new_feature::{can_take_focus, keyboard_top};
use crate::ui::table::{
    ALL, CellTap, FILTER, SELECTED, cell_name, cell_text, column_width, count_text, empty_text,
    focus_all, paint_arrow, sort_name,
};
use crate::ui::{AppState, Workspace};

/// The title of the page.
pub const TITLE: &str = "Attribute table";
/// The row of the page of a vector layer that opens the table.
pub const FEATURES: &str = "Features";
/// The height of a row and of the header.
pub const ROW_HEIGHT: f32 = TOUCH;
/// The width of the Layers panel while it shows the table.
const WIDTH: f32 = 480.0;
const CELL_PAD: f32 = 12.0;

/// True if the Layers panel, or the Layers tab of a frame in the Atlas,
/// shows the table of the layer of the table.
pub fn showing(state: &AppState) -> bool {
    let Some(table) = &state.table else {
        return false;
    };
    if state.workspace == Workspace::Atlas {
        return state.atlas.open_layer() == Some(table.layer)
            && state.atlas.layer_page == Some(Page::Table);
    }
    state.pad.right == Some(Panel::Layers)
        && state.pad.page == Some(Page::Table)
        && state.pad.layer == Some(LayerRef::Vector(table.layer))
}

/// The table closes when its page closes.
pub fn check(state: &mut AppState) {
    if state.table.is_some() && !showing(state) {
        state.close_table();
    }
}

/// Makes the right panel wider while it shows the table. The panel stays
/// clear of the tool strip and of the cards beside it.
pub fn widen(state: &AppState, rects: &mut Rects) {
    if rects.sheet.is_some() || !showing(state) {
        return;
    }
    let right = super::geometry::float_right(rects);
    let normal = right - rects.right_panel.x;
    let room = right - rects.brush_panel.x;
    let width = WIDTH.min(room).max(normal);
    rects.right_panel.x = right - width;
    rects.right_panel.y = super::geometry::float_top(rects, right - width, right);
}

/// What the user did in the table in one frame.
#[derive(Default)]
struct Actions {
    header: Option<String>,
    tap: Option<(Uuid, String)>,
    commit: bool,
    cancel: bool,
    mode: Option<bool>,
}

/// The page: the filter and the mode, the header, and the rows. It fills
/// the panel, and only the rows scroll.
pub fn page(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    state.sync_table();
    let Some(rows) = state.table_rows().map(<[usize]>::to_vec) else {
        return;
    };
    let selection = state.table_selection();
    let screen = ui.ctx().content_rect();
    let focus = can_take_focus(ui.ctx());
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
    let mut act = Actions::default();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    // The filter, with the count at its right end, and the mode.
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (row, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let labels = [ALL, SELECTED];
    let seg_width = widgets::segments_width(ui, &labels);
    let seg = Rect::from_min_max(pos2(row.right() - seg_width, row.top()), row.max);
    let child = ui.new_child(egui::UiBuilder::new().max_rect(seg.translate(vec2(0.0, -PAD))));
    let items = [(ALL, true), (SELECTED, true)];
    let active = usize::from(table.selected_only);
    act.mode = widgets::segments(&child, controls, &items, active).map(|i| i == 1);
    let field = Rect::from_min_max(row.min, pos2(seg.left() - GAP / 2.0, row.bottom()));
    let field = field.shrink2(vec2(0.0, (PANEL_ROW - TOUCH) / 2.0));
    let valid = table.rows.error().is_none();
    let count = count_text(rows.len(), layer.features.len(), 0);
    let filter = &mut table.filter;
    let mono = theme::mono();
    widgets::field_at(
        ui,
        controls,
        field,
        FILTER,
        Some(&count),
        filter,
        valid,
        mono,
    );
    if filter.is_empty() {
        let at = pos2(field.left() + CELL_PAD, field.center().y);
        let painter = ui.painter();
        painter.text(at, Align2::LEFT_CENTER, FILTER, theme::body(), theme::MUTE);
    }
    super::expression::line(ui, &help, filter, ExprKind::Filter);
    ui.add_space(6.0);

    let widths: Vec<f32> = layer
        .fields
        .iter()
        .map(|f| column_width(f.kind) + 2.0 * CELL_PAD)
        .collect();
    let total = widths.iter().sum::<f32>().max(ui.available_width());
    let body = ui.available_rect_before_wrap();
    if let Some(text) = empty_text(layer.fields.is_empty(), rows.len(), table) {
        header_row(ui, controls, layer, table, &widths, total, &mut act);
        let at = pos2(body.left(), body.top() + ROW_HEIGHT * 1.5);
        let color = if valid { theme::MUTE } else { theme::WARN };
        ui.painter()
            .text(at, Align2::LEFT_CENTER, text, theme::body(), color);
        ui.allocate_space(ui.available_size());
    } else {
        let mut drawn = false;
        // The rows scroll in both directions, and the header moves sideways
        // with them. A drag of a finger on the rows scrolls them.
        let (head, _) =
            ui.allocate_exact_size(vec2(body.width(), ROW_HEIGHT), egui::Sense::hover());
        let top = ui.cursor().top();
        // A row shows if it is in the view, and above the keyboard while a
        // cell has the keyboard.
        let mut bottom = ui.clip_rect().bottom().min(body.bottom());
        if table.edit.is_some() {
            bottom = bottom.min(keyboard_top(screen) - GAP);
        }
        let mut area = egui::ScrollArea::both()
            .id_salt("table-rows")
            .auto_shrink([false, false]);
        let target = table
            .scroll_to
            .and_then(|uuid| rows.iter().position(|&i| layer.features[i].uuid() == uuid));
        if let Some(i) = target {
            let view = (bottom - top).max(ROW_HEIGHT);
            let y = i as f32 * ROW_HEIGHT;
            let offset = table.offset;
            // The table scrolls only as far as it must.
            let shown = y >= offset && y + ROW_HEIGHT <= offset + view;
            if !shown {
                let to = if y < offset { y } else { y + ROW_HEIGHT - view };
                area = area.vertical_scroll_offset(to.max(0.0));
            }
        }
        let out = area.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for &i in &rows[range] {
                let feature = &layer.features[i];
                let uuid = feature.uuid();
                let size = vec2(total, ROW_HEIGHT);
                let (row, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                if selection.contains(&uuid) {
                    let tint = theme::ACCENT.gamma_multiply(0.22);
                    let back = row.shrink2(vec2(0.0, HIGHLIGHT_INSET));
                    ui.painter().rect_filled(back, theme::CONTROL_RADIUS, tint);
                }
                let mut left = row.left();
                for (field, &w) in layer.fields.iter().zip(&widths) {
                    let cell = Rect::from_min_size(pos2(left, row.top()), vec2(w, ROW_HEIGHT));
                    left += w;
                    let name = cell_name(&field.name, uuid);
                    let edit = table
                        .edit
                        .as_mut()
                        .filter(|e| e.feature == uuid && e.field.name == field.name);
                    if let Some(edit) = edit {
                        drawn = true;
                        let valid = edit.field.valid();
                        let text = &mut edit.field.text;
                        let font = font(field.kind);
                        let resp =
                            widgets::field_at(ui, controls, cell, &name, None, text, valid, font);
                        if edit.focus && focus {
                            focus_all(&resp, &edit.field.text);
                            edit.focus = false;
                        }
                        if resp.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                act.cancel = true;
                            } else {
                                act.commit = true;
                            }
                        }
                        continue;
                    }
                    if widgets::tap_area(ui, controls, cell, &name, true).clicked() {
                        act.tap = Some((uuid, field.name.clone()));
                    }
                    let text = cell_text(feature.attributes.get(&field.name));
                    paint_cell(ui, cell, field.kind, &text);
                }
            }
        });
        table.offset = out.state.offset.y;
        let shift = vec2(-out.state.offset.x, 0.0);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(head.translate(shift)));
        ui.set_clip_rect(head.intersect(ui.clip_rect()));
        header_row(&mut ui, controls, layer, table, &widths, total, &mut act);
        // An edit whose row scrolled out of the view ends.
        if !drawn && table.edit.as_ref().is_some_and(|e| !e.focus) {
            act.commit = true;
        }
    }
    // The edited row stays in the view, above the keyboard.
    table.scroll_to = table.edit.as_ref().map(|e| e.feature);
    if let Some(field) = act.header {
        table.tap_header(&field);
    }
    if let Some(on) = act.mode {
        table.selected_only = on;
    }
    if act.cancel {
        state.cancel_cell();
    } else if act.commit {
        state.commit_cell();
    }
    if let Some((uuid, field)) = act.tap {
        state.tap_cell(uuid, &field, CellTap::Plain);
    }
}

fn header_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    layer: &vale_store::Layer,
    table: &crate::ui::table::TableState,
    widths: &[f32],
    total: f32,
    act: &mut Actions,
) {
    let (row, _) = ui.allocate_exact_size(vec2(total, ROW_HEIGHT), egui::Sense::hover());
    let mut left = row.left();
    for (field, &w) in layer.fields.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(left, row.top()), vec2(w, ROW_HEIGHT));
        left += w;
        let name = sort_name(&field.name);
        if widgets::tap_area(ui, controls, cell, &name, true).clicked() {
            act.header = Some(field.name.clone());
        }
        let sort = table.sort.as_ref().filter(|s| s.field == field.name);
        let color = if sort.is_some() {
            theme::TEXT
        } else {
            theme::MUTE
        };
        let painter = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
        let number = matches!(field.kind, FieldType::Integer | FieldType::Real);
        let galley = painter.layout_no_wrap(field.name.clone(), theme::body(), color);
        let arrow = if sort.is_some() { 14.0 } else { 0.0 };
        let left = if number {
            cell.right() - CELL_PAD - galley.size().x - arrow
        } else {
            cell.left() + CELL_PAD
        };
        let text_right = left + galley.size().x;
        let at = pos2(left, cell.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, color);
        if let Some(sort) = sort {
            let at = pos2(text_right + 9.0, cell.center().y);
            paint_arrow(&painter, at, sort.ascending, color);
        }
    }
    ui.painter()
        .hline(row.x_range(), row.bottom(), (1.0, theme::divider()));
}

/// Numbers are right-aligned, and the rest is left-aligned.
fn anchor(cell: Rect, kind: FieldType) -> (Align2, egui::Pos2) {
    match kind {
        FieldType::Integer | FieldType::Real => (
            Align2::RIGHT_CENTER,
            pos2(cell.right() - CELL_PAD, cell.center().y),
        ),
        _ => (
            Align2::LEFT_CENTER,
            pos2(cell.left() + CELL_PAD, cell.center().y),
        ),
    }
}

fn font(kind: FieldType) -> egui::FontId {
    match kind {
        FieldType::Integer | FieldType::Real => theme::mono(),
        _ => theme::body(),
    }
}

fn paint_cell(ui: &egui::Ui, cell: Rect, kind: FieldType, text: &str) {
    let color = match kind {
        FieldType::Bool => theme::MUTE,
        _ => theme::TEXT,
    };
    let (align, at) = anchor(cell, kind);
    let painter = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
    painter.text(at, align, text, font(kind), color);
}
