//! The Selectable cell of the Select tool, and its list of the layers that
//! the tool selects in. The list stays open while you check layers, and a
//! touch outside the list closes it.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};

use super::geometry::Rects;
use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{PAD, theme};
use crate::ui::AppState;

pub const SELECTABLE: &str = "Selectable";
pub const ACTIVE_LAYER: &str = "Active layer";
pub const SELECT_ALL: &str = "Select all";
pub const DESELECT_ALL: &str = "Deselect all";
/// The value of the Active layer row with no active vector layer.
pub const NO_LAYER: &str = "None";

/// The name of the row of a layer in the list.
pub fn layer_row_name(name: &str) -> String {
    format!("{SELECTABLE} {name}")
}

/// The Selectable cell. A tap opens the list or closes it. Returns the
/// place of the cell.
pub(super) fn cell(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) -> Rect {
    let open = state.pad.selectable;
    let cell = widgets::tool_cell(
        ui,
        controls,
        SELECTABLE,
        SELECTABLE,
        Icon::Layers,
        open,
        true,
    );
    if cell.clicked() {
        state.pad.selectable = !open;
    }
    cell.rect
}

/// The name of the active vector layer, or `NO_LAYER`.
fn active_name(state: &AppState) -> String {
    (state.selected_vector())
        .and_then(|id| state.doc.layer(id))
        .map_or_else(|| NO_LAYER.to_owned(), |l| l.name.clone())
}

/// The list at the right of its cell, or above it in the row layout:
/// Active layer, Select all, Deselect all, and a row for each vector layer,
/// top first. A tick marks the Active layer row while the tool selects in
/// the active layer, and the row of each checked layer.
pub(super) fn menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    cell: Rect,
) {
    let layers: Vec<_> = (state.doc.project.layers().iter().rev())
        .filter(|l| l.geometry().is_some())
        .map(|l| (l.id, l.name.clone()))
        .collect();
    let active = active_name(state);
    let font = theme::body();
    let text_width = |t: &str| {
        ctx.fonts_mut(|f| f.layout_no_wrap(t.to_owned(), font.clone(), theme::TEXT))
            .size()
            .x
    };
    let widest = (layers.iter().map(|l| text_width(&l.1)))
        .chain([SELECT_ALL, DESELECT_ALL].map(text_width))
        .chain([text_width(ACTIVE_LAYER) + 16.0 + text_width(&active)])
        .fold(0.0, f32::max);
    let top = rects.column_top;
    let bottom = rects.panel_bottom;
    let left_edge = rects.screen.left() + rects.bands.left + PAD;
    let right_edge = rects.screen.right() - rects.bands.right - PAD;
    let width = (widest + 2.0 * widgets::BODY_PAD + 40.0)
        .clamp(220.0, 360.0)
        .min(right_edge - left_edge);
    let rows = 3 + layers.len();
    let full = 20.0 + rows as f32 * PANEL_ROW;
    // In the row layout, the list is above its cell.
    let rect = if rects.row {
        let height = full.min(cell.top() - 8.0 - top);
        let left = (cell.right() - width)
            .min(right_edge - width)
            .max(left_edge);
        Rect::from_min_size(pos2(left, cell.top() - 8.0 - height), vec2(width, height))
    } else {
        let height = full.min(bottom - top);
        let left = (cell.right() + 2.0 * PAD + 8.0)
            .min(right_edge - width)
            .max(left_edge);
        let y = cell.top().min(bottom - height).max(top);
        Rect::from_min_size(pos2(left, y), vec2(width, height))
    };
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|p| !rect.contains(p) && !cell.contains(p)) {
        state.pad.selectable = false;
        return;
    }
    let mut action = None;
    let checked: Vec<_> = (layers.iter())
        .map(|(id, _)| state.selectable_checked(*id))
        .collect();
    let in_active = matches!(state.selectable, crate::ui::select::Selectable::Active);
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-selectable",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(widgets::BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            ui.set_clip_rect(inner);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(&mut ui, |ui| {
                    let row = widgets::text_row(ui, controls, ACTIVE_LAYER, 0.0, false, true);
                    let at = pos2(row.rect.right() - 32.0, row.rect.center().y);
                    ui.painter().text(
                        at,
                        Align2::RIGHT_CENTER,
                        &active,
                        theme::body(),
                        theme::MUTE,
                    );
                    tick(ui, row.rect, in_active);
                    if row.clicked() {
                        action = Some(Action::Active);
                    }
                    if widgets::text_row(ui, controls, SELECT_ALL, 0.0, false, true).clicked() {
                        action = Some(Action::All);
                    }
                    if widgets::text_row(ui, controls, DESELECT_ALL, 0.0, false, true).clicked() {
                        action = Some(Action::None);
                    }
                    for ((id, name), on) in layers.iter().zip(&checked) {
                        let row_name = layer_row_name(name);
                        let row =
                            widgets::text_row_as(ui, controls, &row_name, name, 0.0, false, true);
                        tick(ui, row.rect, *on);
                        if row.clicked() {
                            action = Some(Action::Toggle(*id));
                        }
                    }
                });
        },
    );
    match action {
        Some(Action::Active) => state.set_selectable_active(),
        Some(Action::All) => state.select_all_layers(),
        Some(Action::None) => state.deselect_all_layers(),
        Some(Action::Toggle(id)) => state.toggle_selectable(id),
        None => {}
    }
}

enum Action {
    Active,
    All,
    None,
    Toggle(vale_store::LayerId),
}

/// The tick at the right end of a row that is on.
fn tick(ui: &egui::Ui, row: Rect, on: bool) {
    if on {
        let at = pos2(row.right() - 14.0, row.center().y);
        let rect = Rect::from_center_size(at, vec2(18.0, 18.0));
        icons::paint(ui.painter(), rect, Icon::Tick, 18.0, theme::ACCENT);
    }
}
