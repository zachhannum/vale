//! The Grid labels page of the Frame tab: the text of the labels of the
//! reference grid of a frame.

use eframe::egui;
use vale_store::{FrameId, FurnitureText};

use super::atlas::FRAME_TABS;
use super::text_style::{self, TextEvent, TextSpec};
use super::widgets::Controls;
use super::{Head, PageParts, Panel};
use crate::ui::atlas::{self, ClassColor, GridPage};
use crate::ui::{Action, AppState, FontTarget};

pub const GRID_LABELS: &str = "Grid labels";
pub const SIZE: &str = "Grid label size";

/// The value of the Grid labels row of the Frame tab.
pub fn summary(text: &FurnitureText) -> String {
    format!("{} pt", text.size)
}

/// The Grid labels page of a frame, or the page of its faces or its cases,
/// in the pane of the Frame panel.
pub(super) fn head(state: &mut AppState, id: FrameId) -> Option<Head> {
    state.doc.project.atlas.frame(id)?;
    let (key, title, back) = match state.atlas.grid_page {
        None => ("pad-atlas-grid-labels", GRID_LABELS, FRAME_TABS[0]),
        Some(GridPage::Faces) => ("pad-atlas-grid-faces", "Style", GRID_LABELS),
        Some(GridPage::Cases) => ("pad-atlas-grid-cases", "Case", GRID_LABELS),
    };
    let back_to = |state: &mut AppState| {
        if state.atlas.grid_page.is_some() {
            state.atlas.grid_page = None;
        } else {
            state.atlas.grid = None;
        }
        state.atlas.label_color = None;
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = move |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        body(ui, state, controls, id);
    };
    let parts = PageParts::new(key, title, Some(back));
    Some(parts.head(Panel::Frame, back_to, top, body))
}

fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(style) = (state.doc.project.atlas.frame(id)).map(|f| f.border.grid_text.clone())
    else {
        return;
    };
    match state.atlas.grid_page {
        Some(GridPage::Faces) => {
            let current = vale_labeler::Face {
                weight: style.weight,
                italic: style.italic,
            };
            let font = style.font.as_deref();
            if let Some(f) = text_style::face_rows(ui, state, controls, font, current) {
                atlas::edit_grid_text(state, id, true, |s| {
                    s.weight = f.weight;
                    s.italic = f.italic;
                });
                state.atlas.grid_page = None;
            }
        }
        Some(GridPage::Cases) => {
            if let Some(case) = text_style::case_rows(ui, controls, style.case) {
                atlas::edit_grid_text(state, id, true, |s| s.case = case);
                state.atlas.grid_page = None;
            }
        }
        None => {
            let range = |r: std::ops::RangeInclusive<f64>| (*r.start(), *r.end());
            let spec = TextSpec {
                size_name: SIZE,
                size_step: 0.5,
                sizes: range(FurnitureText::SIZES),
                spacings: range(FurnitureText::LETTER_SPACINGS),
                halo_widths: range(FurnitureText::HALO_WIDTHS),
                colors: (ClassColor::GridText, ClassColor::GridHalo),
            };
            match text_style::section(ui, state, controls, &style, spec) {
                Some(TextEvent::PickFont) => {
                    let target = FontTarget::Grid(id);
                    state.actions.push(Action::PickFont(target));
                }
                Some(TextEvent::OpenFaces) => state.atlas.grid_page = Some(GridPage::Faces),
                Some(TextEvent::OpenCases) => state.atlas.grid_page = Some(GridPage::Cases),
                Some(TextEvent::Edit(edit)) => {
                    atlas::edit_grid_text(state, id, edit.step(), |s| edit.apply(s));
                }
                None => {}
            }
        }
    }
}
