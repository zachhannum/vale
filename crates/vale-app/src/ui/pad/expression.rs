//! The line under an expression field of the iPad forms, the names of the
//! chips that insert into an expression, and the width of a panel with an
//! expression editor.

use eframe::egui::{self, Pos2, Rect, Sense, vec2};

use super::geometry::{Rects, WidthClass};
use super::{LayerRef, Page, Panel, theme};
use crate::ui::AppState;
use crate::ui::expr_help::{ExprHelp, ExprKind, Line};

/// The width of a panel with an expression editor, if the screen has room.
pub const PANEL_WIDTH: f32 = 460.0;

/// True if the Layers panel shows a page with an expression editor: the
/// label text, a rule with its filter, or Classify with its value.
pub fn showing(state: &AppState) -> bool {
    state.pad.shows(Panel::Layers)
        && matches!(state.pad.layer, Some(LayerRef::Vector(_)))
        && matches!(
            state.pad.page,
            Some(Page::LabelText | Page::Rule(_) | Page::Classify)
        )
}

/// The dock, at least `width` wide inside the band of the system, if the
/// screen has room. It stays clear of the tool strip and of the cards
/// beside it.
pub fn wider(rects: &Rects, width: f32) -> Rect {
    let mut dock = rects.dock;
    let left = (rects.screen.right() - rects.bands.right - width).max(rects.column_right);
    dock.min.x = dock.min.x.min(left);
    dock
}

/// The top left corner and the width of a panel that holds an expression
/// editor. The panel is wider when the screen has room. A sheet keeps its
/// width.
pub fn wide_place(rects: &Rects) -> (Pos2, f32) {
    let dock = match rects.class {
        WidthClass::Compact => rects.dock,
        _ => wider(rects, PANEL_WIDTH),
    };
    (dock.min, dock.width())
}

/// Makes the dock wider while it shows an expression editor.
pub fn widen(state: &AppState, rects: &mut Rects) {
    if showing(state) && rects.class != WidthClass::Compact {
        rects.dock = wider(rects, PANEL_WIDTH);
    }
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
