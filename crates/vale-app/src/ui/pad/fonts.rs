//! The labels section of the panel of a vector layer.

use eframe::egui;
use vale_store::LayerId;
use vale_style::expr::{TextPart, split};

use super::Page;
use super::label_parts::{self, LABEL_TEXT};
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

/// The Label text page of layer `id`: the text builder.
pub fn text_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.label.text.clone();
    let help = ExprHelp::for_layer(state, id);
    let mut typed = std::mem::take(&mut state.pad.label_text);
    let drag = &mut state.pad.part_drag;
    let built = label_parts::list(ui, controls, LABEL_TEXT, &current, &mut typed, drag, &help);
    state.pad.label_text = typed;
    if let Some(text) = built.text {
        set_text(state, id, text);
    }
    if let Some(n) = built.open {
        state.pad.page = Some(Page::Part(n));
    }
    if built.add {
        state.pad.page = Some(Page::AddPart);
    }
}

/// The page of a new part of the label text of layer `id`. A text part
/// opens its page.
pub fn add_part_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
) {
    let (Some(layer), Some(entry)) = (state.doc.layer(id), state.doc.entry(id)) else {
        return;
    };
    let fields = label_parts::fields_of(layer);
    let current = entry.style.label.text.clone();
    let Some(part) = label_parts::add_page(ui, controls, &fields) else {
        return;
    };
    let text_part = matches!(part, TextPart::Text(_));
    let (text, n) = label_parts::append(&current, part);
    set_text(state, id, text);
    state.pad.page = Some(if text_part {
        Page::Part(n)
    } else {
        Page::LabelText
    });
}

/// The page of part `n` of the label text of layer `id`.
pub fn part_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    n: usize,
) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.label.text.clone();
    let parts = split(&current);
    let Some(part) = parts.get(n) else {
        return;
    };
    let number = super::labels::number_field(state, id, part);
    let help = ExprHelp::for_layer(state, id);
    if let Some((part, _)) = label_parts::part_page(ui, controls, part, number, &help)
        && let Some(text) = label_parts::replace(&current, n, part)
    {
        set_text(state, id, text);
    }
}
