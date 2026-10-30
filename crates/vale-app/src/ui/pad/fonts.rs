//! The labels section of the panel of a vector layer.

use eframe::egui;
use vale_store::LayerId;

use super::note_text;
use super::raster::heading;
use super::widgets::{self, Controls};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::{Action, AppState, Note};

pub const LABELS: &str = "Labels";
/// The row that opens the font picker.
pub const FONT: &str = "Font";

/// The labels section: the font of the labels, and a warning if the device
/// has no such font.
pub fn section(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let label = &entry.style.label;
    let family = label.font.as_deref().unwrap_or(DEFAULT_FAMILY).to_string();
    let warning = fonts::missing(label.font.as_deref(), &state.font_families);
    let mut warning = warning.map(|t| Note::new(t, true, None));
    heading(ui, LABELS);
    if widgets::open_row(ui, controls, FONT, &family).clicked() {
        state.actions.push(Action::PickFont(id));
    }
    note_text(ui, None, &mut warning, None);
}
