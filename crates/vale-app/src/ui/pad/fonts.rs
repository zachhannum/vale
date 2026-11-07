//! The labels section of the panel of a vector layer.

use eframe::egui;
use vale_store::LayerId;

use super::Page;
use super::label_text::{self, LABEL_TEXT};
use super::note_text;
use super::raster::heading;
use super::widgets::{self, Controls};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::expr_help::ExprHelp;
use crate::ui::{Action, AppState, FontTarget, Note};

pub const LABELS: &str = "Labels";
/// The row that opens the font picker.
pub const FONT: &str = "Font";

/// The labels section: the row that opens the label text, the font of the
/// labels, and a warning if the device has no such font.
pub fn section(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let label = &entry.style.label;
    let family = label.font.as_deref().unwrap_or(DEFAULT_FAMILY).to_string();
    let warning = fonts::missing(label.font.as_deref(), &state.font_families);
    let mut warning = warning.map(|t| Note::new(t, true, None));
    let text = short(&label.text);
    heading(ui, LABELS);
    if widgets::open_row(ui, controls, LABEL_TEXT, &text).clicked() {
        state.pad.page = Some(Page::LabelText);
    }
    if widgets::open_row(ui, controls, FONT, &family).clicked() {
        state.actions.push(Action::PickFont(FontTarget::Layer(id)));
    }
    note_text(ui, None, &mut warning, None);
}

/// The most characters of the label text in its row.
const SHORT: usize = 16;

/// The label text in its row, cut short with an ellipsis.
fn short(text: &str) -> String {
    if text.chars().count() <= SHORT {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(SHORT - 1).collect();
    out.push('…');
    out
}

/// Sets the label text of layer `id`.
fn set_text(state: &mut AppState, id: LayerId, text: String) {
    if let Some(entry) = state.doc.entry_mut(id)
        && entry.style.label.text != text
    {
        entry.style.label.text = text;
        state.touch();
    }
}

/// The Label text page of layer `id`.
pub fn text_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.label.text.clone();
    let help = ExprHelp::for_layer(state, id);
    let edit = label_text::editor(ui, controls, ("", LABEL_TEXT), &current, &help);
    if let Some(text) = edit.text {
        set_text(state, id, text);
    }
}
