//! The labels section of the panel of a vector layer, and its font page.

use eframe::egui;
use vale_store::LayerId;

use super::raster::heading;
use super::widgets::{self, Controls};
use super::{Page, note_text};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::inspector::ADD_FONT;
use crate::ui::{Action, AppState, Note};

pub const LABELS: &str = "Labels";
/// The row that opens the font page, and the title of the page.
pub const FONT: &str = "Font";

/// The labels section: the font of the labels, and a warning if the project
/// has no such font.
pub fn section(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let label = &entry.style.label;
    let family = label.font.as_deref().unwrap_or(DEFAULT_FAMILY).to_string();
    let mut warning = fonts::missing(label, &state.doc.project).map(|t| Note::new(t, true, None));
    heading(ui, LABELS);
    if widgets::open_row(ui, controls, FONT, &family).clicked() {
        state.pad.page = Some(Page::Fonts);
    }
    note_text(ui, None, &mut warning, None);
}

/// The font page: a row for each font of the app and of the project, with
/// the row of the current font on, the button that adds a font file, and the
/// text of the last addition. A tap on a row picks the font.
pub fn fonts_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.label.font.clone();
    let current = current.as_deref().unwrap_or(DEFAULT_FAMILY);
    let families = fonts::families(&state.doc.project);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let mut picked = None;
    for family in &families {
        let on = family == current;
        if widgets::option_row(ui, controls, family, family, &[], on).clicked() && !on {
            picked = Some(family.clone());
        }
    }
    ui.add_space(16.0);
    if widgets::button_row(ui, controls, ADD_FONT, true).clicked() {
        state.actions.push(Action::AddFontDialog);
    }
    ui.add_space(4.0);
    note_text(ui, None, &mut state.font_note, None);
    if let Some(family) = picked
        && let Some(entry) = state.doc.entry_mut(id)
    {
        fonts::set_font(&mut entry.style.label, &family);
        state.touch();
    }
}
