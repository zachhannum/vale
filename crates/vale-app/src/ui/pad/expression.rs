//! The line under an expression field of the iPad forms, the names of the
//! chips that insert into an expression, and the width of a panel with an
//! expression editor.

use eframe::egui::{self, Pos2, Sense, vec2};

use super::geometry::Rects;
use super::{LayerRef, Page, Panel, theme};
use crate::ui::AppState;
use crate::ui::expr_help::{ExprHelp, ExprKind, Line};

/// The width of a panel with an expression editor, if the screen has room.
pub const PANEL_WIDTH: f32 = 460.0;

/// True if the Layers panel shows a page with an expression editor: the
/// label text, a rule with its filter, or Classify with its value.
pub fn showing(state: &AppState) -> bool {
    state.pad.is_open(Panel::Layers)
        && matches!(state.pad.layer, Some(LayerRef::Vector(_)))
        && matches!(
            state.pad.page,
            Some(Page::LabelText | Page::Rule(_) | Page::Classify)
        )
}

/// The top left corner and the width of a right panel of width `normal`
/// that holds an expression editor. The panel is wider when the screen has
/// room, and stays clear of the tool strip and of the cards beside it. A
/// sheet keeps its width.
pub fn wide_place(rects: &Rects, normal: f32) -> (Pos2, f32) {
    let right = super::geometry::float_right(rects);
    let x = right - normal;
    if rects.sheet.is_some() {
        return (egui::pos2(x, rects.right_panel.y), normal);
    }
    let room = right - rects.brush_panel.x;
    let width = PANEL_WIDTH.min(room).max(normal);
    let top = super::geometry::float_top(rects, right - width, right);
    (egui::pos2(right - width, top), width)
}

/// Makes the right panel wider while it shows an expression editor.
pub fn widen(state: &AppState, rects: &mut Rects) {
    if !showing(state) {
        return;
    }
    let right = super::geometry::float_right(rects);
    let (at, _) = wide_place(rects, right - rects.right_panel.x);
    rects.right_panel = at;
}

/// The height of the line under an expression field.
const LINE: f32 = 20.0;

/// The control name of the chip that inserts `label`.
pub fn chip_name(label: &str) -> String {
    format!("Insert {label}")
}

/// The line under an expression field: the result in muted text, or the
/// message of the parser in the warning color. No line for blank text.
pub fn line(ui: &mut egui::Ui, help: &ExprHelp, text: &str, kind: ExprKind) {
    let (text, color) = match help.line(text, kind) {
        Some(Line::Preview(result)) => (result, theme::MUTE),
        Some(Line::Error(message)) => (message, theme::WARN),
        None => return,
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
    let text = egui::RichText::new(text).font(theme::small()).color(color);
    let place = rect.shrink2(vec2(12.0, 0.0));
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(place).layout(layout),
        |ui| {
            ui.add(egui::Label::new(text).selectable(false).truncate());
        },
    );
}
