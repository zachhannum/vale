//! The page of a layer in the World workspace that copies the style of the
//! layer in a map or a map style into the globe style.

use eframe::egui;
use vale_store::LayerId;

use super::widgets::{self, Controls};
use crate::document::Target;
use crate::ui::AppState;

pub const COPY_STYLE_FROM: &str = "Copy style from…";
pub const TITLE: &str = "Copy style from";
pub const MAPS: &str = "Maps";
pub const MAP_STYLES: &str = "Map styles";

/// The name of the row of a map in the page.
pub fn map_row(name: &str) -> String {
    format!("Map {name}")
}

/// The name of the row of a map style in the page.
pub fn map_style_row(name: &str) -> String {
    format!("Map style {name}")
}

/// The row of a layer panel that opens the page.
pub(super) fn open_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    if widgets::button_row(ui, controls, COPY_STYLE_FROM, true).clicked() {
        state.pad.page = Some(super::Page::CopyStyle);
    }
}

/// The page: a row for each map and each map style. A tap on a row makes a
/// copy of the style of the layer there the globe style, and the panel of
/// the layer comes back. The row of a style that is the globe style has a
/// check.
pub(super) fn page_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let atlas = &state.doc.project.atlas;
    let maps = (atlas.maps.iter())
        .map(|m| (Target::Map(m.id), map_row(&m.name), m.name.clone()))
        .collect();
    let styles = (atlas.map_styles.iter())
        .map(|s| (Target::Style(s.id), map_style_row(&s.name), s.name.clone()))
        .collect();
    let mut picked = None;
    for (heading, rows) in [(MAPS, maps), (MAP_STYLES, styles)] {
        let rows: Vec<(Target, String, String)> = rows;
        if rows.is_empty() {
            continue;
        }
        widgets::list_heading(ui, heading);
        for (target, name, title) in rows {
            let same = !state.differs_from_globe(target, layer);
            if widgets::option_row(ui, controls, &name, &title, &[], same).clicked() && !same {
                picked = Some(target);
            }
        }
    }
    if let Some(target) = picked {
        state.copy_to_globe_style(target, layer);
        state.pad.page = None;
    }
}
