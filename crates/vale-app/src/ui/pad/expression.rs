//! The expression field of the iPad forms.

use eframe::egui::{self, Response, Sense, vec2};

use super::theme;
use super::widgets::{self, Controls};
use crate::ui::expr_help::{self, ExprHelp, ExprKind, Line};

/// The height of the line under an expression field.
const LINE: f32 = 20.0;

/// The control name of the chip that inserts `label`.
pub fn chip_name(label: &str) -> String {
    format!("Insert {label}")
}

/// A field for an expression, in the mono font. While the field has the
/// focus, two rows of chips under it insert the fields of the layer and the
/// functions at the cursor. Under that, a line shows the result for the
/// feature of `help`, or the message of the parser.
///
/// A tap on a chip takes the focus from the field in the frame of the tap,
/// so the field shows the chips if it had the focus at the start of the
/// frame, and takes the focus back after an insert.
pub fn field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &mut String,
    help: &ExprHelp,
    kind: ExprKind,
) -> Response {
    let id = ui.id().with(name);
    let had_focus = ui.memory(|m| m.has_focus(id));
    let valid = help.valid(text);
    let mut resp = widgets::code_field(ui, controls, name, name, text, valid);
    if had_focus || resp.has_focus() {
        // The fields and the functions have a row each, so that a layer
        // with many fields does not push the functions out of reach.
        let (calls, fields): (Vec<_>, Vec<_>) = help.chips().into_iter().partition(|c| c.call);
        for (row, chips) in [("fields", fields), ("functions", calls)] {
            if chips.is_empty() {
                continue;
            }
            let named: Vec<(String, String)> = chips
                .iter()
                .map(|c| (chip_name(&c.label), c.label.clone()))
                .collect();
            if let Some(i) = widgets::chip_row(ui, controls, id.with(row), &named) {
                expr_help::insert_at_cursor(ui.ctx(), id, text, &chips[i]);
                resp.request_focus();
                resp.mark_changed();
            }
        }
    }
    line(ui, help, text, kind);
    resp
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
