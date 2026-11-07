//! The line under an expression field of the iPad forms, and the names of
//! the chips that insert into an expression.

use eframe::egui::{self, Sense, vec2};

use super::theme;
use crate::ui::expr_help::{ExprHelp, ExprKind, Line};

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
