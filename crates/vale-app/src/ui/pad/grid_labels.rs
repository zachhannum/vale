//! The Grid labels page of the Frame tab: the text of the labels of the
//! reference grid of a frame.

use eframe::egui::{self, Rect};
use vale_store::{FrameId, FurnitureText};

use super::atlas::{FRAME_TABS, panel_width};
use super::geometry::Rects;
use super::text_style::{self, TextEvent, TextSpec};
use super::widgets::{self, Controls};
use super::{Card, CardTitle, show_card};
use crate::ui::atlas::{self, AtlasSelection, ClassColor, GridPage};
use crate::ui::{Action, AppState, FontTarget};

pub const GRID_LABELS: &str = "Grid labels";
pub const SIZE: &str = "Grid label size";

/// The value of the Grid labels row of the Frame tab.
pub fn summary(text: &FurnitureText) -> String {
    format!("{} pt", text.size)
}

/// The Grid labels page of a frame, or the page of its faces or its cases,
/// in place of the Frame tab.
pub fn panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: FrameId,
) -> Option<Rect> {
    state.doc.project.atlas.frame(id)?;
    let (key, title, back) = match state.atlas.grid_page {
        None => ("pad-atlas-grid-labels", GRID_LABELS, FRAME_TABS[0]),
        Some(GridPage::Faces) => ("pad-atlas-grid-faces", "Style", GRID_LABELS),
        Some(GridPage::Cases) => ("pad-atlas-grid-cases", "Case", GRID_LABELS),
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head: CardTitle {
            title,
            back: Some(back),
            action: None,
        },
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            if state.atlas.grid_page.is_some() {
                state.atlas.grid_page = None;
            } else {
                state.atlas.grid = None;
            }
            state.atlas.label_color = None;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        body(ui, state, controls, id);
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(style) = (state.doc.project.atlas.frame(id)).map(|f| f.grid.text.clone()) else {
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
