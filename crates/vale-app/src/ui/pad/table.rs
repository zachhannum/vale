//! The attribute table in the iPad layout: a card along the bottom of the
//! canvas, between the tool strip and the right panel. The selection card
//! stays at the top, so the two show at the same time.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use vale_store::{FieldType, Uuid};

use super::geometry::{
    BRUSH_PANEL_WIDTH, GAP, MARGIN, PAD, PANEL_GAP, ROW, Rects, TOUCH, WidthClass,
};
use super::icons::Icon;
use super::theme;
use super::widgets::{self, BODY_PAD, Controls};
use crate::ui::AppState;
use crate::ui::point_form::{can_take_focus, keyboard_top};
use crate::ui::table::{
    ALL, CellTap, FILTER, SELECTED, cell_name, cell_text, column_width, count_text, empty_text,
    scroll_index, sort_name,
};

pub const CARD: &str = "attribute-table";
/// The name of the close button of the table.
pub const CLOSE: &str = "Close attribute table";
/// The height of a row and of the header.
pub const ROW_HEIGHT: f32 = TOUCH;
const HEIGHT: f32 = 300.0;
const FILTER_WIDTH: f32 = 220.0;
/// The least width of the card that holds the filter in the title row.
const ONE_ROW: f32 = 700.0;
const CELL_PAD: f32 = 12.0;

/// The rectangle of the card of the table.
pub fn rect(state: &AppState, rects: &Rects) -> Rect {
    let screen = rects.screen;
    let editing = state.table.as_ref().is_some_and(|t| t.edit.is_some());
    let mut bottom = rects.panel_bottom;
    if editing {
        bottom = bottom.min(keyboard_top(screen) - GAP);
    }
    let floor = rects.workspace.bottom() + GAP;
    let height = HEIGHT.min(0.4 * screen.height()).min(bottom - floor);
    let top = bottom - height;
    let edge_left = screen.left() + rects.bands.left + MARGIN;
    let edge_right = screen.right() - rects.bands.right - MARGIN;
    if rects.class == WidthClass::Compact {
        return Rect::from_min_max(pos2(edge_left, top), pos2(edge_right, bottom));
    }
    let mut left = rects.tools.right() + GAP;
    if let Some(brush) = rects.brush
        && brush.bottom() > top
    {
        left = left.max(brush.right() + GAP);
    }
    if state.pad.brush {
        left = left.max(rects.brush_panel.x + BRUSH_PANEL_WIDTH + PANEL_GAP);
    }
    let right = if state.pad.right.is_some() {
        rects.right_panel.x - PANEL_GAP
    } else {
        edge_right
    };
    Rect::from_min_max(pos2(left, top), pos2(right.max(left + 4.0 * TOUCH), bottom))
}

/// What the user did in the table in one frame.
#[derive(Default)]
struct Actions {
    close: bool,
    header: Option<String>,
    tap: Option<(Uuid, String)>,
    commit: bool,
    cancel: bool,
    mode: Option<bool>,
}

pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    state.sync_table();
    // A compact screen has room for one sheet. The table yields to it.
    if rects.class == WidthClass::Compact && state.pad.right.is_some() {
        return;
    }
    let Some(rows) = state.table_rows().map(<[usize]>::to_vec) else {
        return;
    };
    let rect = rect(state, rects);
    let selection = state.table_selection();
    let backdrop = state.globe.backdrop();
    let Some(table) = state.table.as_mut() else {
        return;
    };
    let Some(layer) = state.doc.project.layer(table.layer) else {
        return;
    };
    // The edited row shows above the keyboard.
    if table.edit.as_ref().is_some_and(|e| e.focus) {
        table.scroll_to = table.edit.as_ref().map(|e| e.feature);
    }
    let focus = can_take_focus(ctx);
    let mut act = Actions::default();
    let mut drawn = false;
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, backdrop, CARD, rect, Order::Middle, radius, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let one_row = rect.width() >= ONE_ROW;
        // The title row: the name, the count, the filter, the mode, and the
        // close button.
        let (row, _) = ui.allocate_exact_size(vec2(rect.width(), ROW), egui::Sense::hover());
        let close = Rect::from_min_size(
            pos2(row.right() - PAD - TOUCH, row.top() + PAD),
            vec2(TOUCH, TOUCH),
        );
        act.close =
            widgets::icon_button_at(ui, controls, close, CLOSE, Icon::Close, false, true).clicked();
        let items = [(ALL, true), (SELECTED, true)];
        let labels = [ALL, SELECTED];
        let seg_width = widgets::segments_width(ui, &labels);
        let seg = Rect::from_min_size(
            pos2(close.left() - seg_width, row.top()),
            vec2(seg_width, ROW),
        );
        let active = usize::from(table.selected_only);
        let child = ui.new_child(egui::UiBuilder::new().max_rect(seg));
        act.mode = widgets::segments(&child, controls, &items, active).map(|i| i == 1);
        let mut text_right = seg.left() - GAP;
        let valid = table.rows.error().is_none();
        let filter_row = if one_row {
            let field = Rect::from_min_size(
                pos2(seg.left() - GAP - FILTER_WIDTH, row.top() + PAD),
                vec2(FILTER_WIDTH, TOUCH),
            );
            text_right = field.left() - GAP;
            field
        } else {
            let (second, _) = ui.allocate_exact_size(vec2(rect.width(), ROW), egui::Sense::hover());
            second.shrink2(vec2(BODY_PAD - PAD, PAD))
        };
        let filter = &mut table.filter;
        widgets::field_at(
            ui,
            controls,
            filter_row,
            FILTER,
            None,
            filter,
            valid,
            theme::mono(),
        );
        if filter.is_empty() {
            let at = pos2(filter_row.left() + CELL_PAD, filter_row.center().y);
            let hint = "Filter, for example rank < 3";
            ui.painter()
                .text(at, Align2::LEFT_CENTER, hint, theme::body(), theme::MUTE);
        }
        let count = count_text(rows.len(), layer.features.len(), selection.len());
        let at = pos2(row.left() + BODY_PAD, row.center().y);
        let clip = Rect::from_min_max(row.min, pos2(text_right, row.bottom()));
        let painter = ui.painter().with_clip_rect(clip);
        let name = painter.text(
            at,
            Align2::LEFT_CENTER,
            &layer.name,
            theme::title(),
            theme::TEXT,
        );
        let at = pos2(name.right() + GAP, row.center().y);
        painter.text(at, Align2::LEFT_CENTER, count, theme::body(), theme::MUTE);
        ui.painter()
            .hline(rect.x_range(), ui.cursor().top(), (1.0, theme::divider()));

        let widths: Vec<f32> = layer
            .fields
            .iter()
            .map(|f| column_width(f.kind) + 2.0 * CELL_PAD)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 2.0 * PAD;
        let body = ui.available_rect_before_wrap();
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        if let Some(text) = empty_text(layer.fields.is_empty(), rows.len(), table) {
            header_row(&mut ui, controls, layer, table, &widths, &mut act);
            let at = pos2(
                body.left() + BODY_PAD,
                body.top() + ROW_HEIGHT + ROW_HEIGHT / 2.0,
            );
            let color = if valid { theme::MUTE } else { theme::WARN };
            ui.painter()
                .text(at, Align2::LEFT_CENTER, text, theme::body(), color);
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt("table-h")
            .auto_shrink([false, false])
            .show(&mut ui, |ui| {
                ui.set_min_width(total.max(body.width()));
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                header_row(ui, controls, layer, table, &widths, &mut act);
                let mut area = egui::ScrollArea::vertical()
                    .id_salt("table-v")
                    .auto_shrink([false, false]);
                if let Some(i) = scroll_index(table, &rows, layer) {
                    let view = ui.available_height();
                    let offset = i as f32 * ROW_HEIGHT - (view - ROW_HEIGHT) / 2.0;
                    area = area.vertical_scroll_offset(offset.max(0.0));
                }
                area.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for &i in &rows[range] {
                        let feature = &layer.features[i];
                        let uuid = feature.uuid();
                        let (row, _) = ui.allocate_exact_size(
                            vec2(total.max(body.width()), ROW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        if selection.contains(&uuid) {
                            let tint = theme::ACCENT.gamma_multiply(0.22);
                            let back = row.shrink2(vec2(PAD, widgets::HIGHLIGHT_INSET));
                            ui.painter().rect_filled(back, theme::CONTROL_RADIUS, tint);
                        }
                        let mut left = row.left() + PAD;
                        for (field, &w) in layer.fields.iter().zip(&widths) {
                            let cell =
                                Rect::from_min_size(pos2(left, row.top()), vec2(w, ROW_HEIGHT));
                            left += w;
                            let name = cell_name(&field.name, uuid);
                            let edit = table
                                .edit
                                .as_mut()
                                .filter(|e| e.feature == uuid && e.field.name == field.name);
                            if let Some(edit) = edit {
                                drawn = true;
                                let valid = edit.field.valid();
                                let font = font(field.kind);
                                let text = &mut edit.field.text;
                                let resp = widgets::field_at(
                                    ui, controls, cell, &name, None, text, valid, font,
                                );
                                if edit.focus && focus {
                                    resp.request_focus();
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
            });
    });
    table.scroll_to = None;
    // An edit whose row scrolled out of the view ends.
    if !drawn && table.edit.as_ref().is_some_and(|e| !e.focus) {
        act.commit = true;
    }
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
    if act.close {
        state.close_table();
    }
}

fn header_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    layer: &vale_store::Layer,
    table: &crate::ui::table::TableState,
    widths: &[f32],
    act: &mut Actions,
) {
    let width = ui
        .available_width()
        .max(widths.iter().sum::<f32>() + 2.0 * PAD);
    let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), egui::Sense::hover());
    let mut left = row.left() + PAD;
    for (field, &w) in layer.fields.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(left, row.top()), vec2(w, ROW_HEIGHT));
        left += w;
        let name = sort_name(&field.name);
        let resp = widgets::tap_area(ui, controls, cell, &name, true);
        if resp.clicked() {
            act.header = Some(field.name.clone());
        }
        let text = match table.arrow(&field.name) {
            Some(arrow) => format!("{} {arrow}", field.name),
            None => field.name.clone(),
        };
        let color = if table.arrow(&field.name).is_some() {
            theme::TEXT
        } else {
            theme::MUTE
        };
        let (align, at) = anchor(cell, field.kind);
        let painter = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
        painter.text(at, align, text, theme::body(), color);
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
