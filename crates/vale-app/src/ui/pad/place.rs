//! The section of the panel of a layer from a file that places the file on
//! the world, in the iPad layout.

use eframe::egui::{self, Sense, vec2};
use vale_store::PlaceMethod;

use super::widgets::{self, Controls, PANEL_ROW};
use super::{LayerRef, raster, theme};
use crate::ui::AppState;
use crate::ui::place_form::{self as form, Axis};

/// The highest picture of a tall file.
const PICTURE_HEIGHT: f32 = 240.0;
/// The radius of a numbered marker on the picture.
const MARKER: f32 = 12.0;

/// Shows the section if the layer came from a file.
pub fn section(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, of: LayerRef) {
    let Some(file) = form::open(state, ui.ctx(), of) else {
        return;
    };
    if let Some(source) = &file.source {
        widgets::value_row(ui, form::FILE, source);
    }
    raster::heading(ui, form::HEADING);
    let Some(place) = state.place_form.as_mut() else {
        return;
    };

    let space = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0));
    let rect = form::picture_rect(space, file.placement.size, PICTURE_HEIGHT);
    ui.allocate_exact_size(vec2(space.width(), rect.height() + 8.0), Sense::hover());
    let font = theme::small();
    form::paint_picture(ui.painter(), rect, place, 8.0, MARKER, font);
    if place.method == PlaceMethod::Points {
        let resp = widgets::drag_area(ui, controls, rect, form::PICTURE);
        form::picture_input(place, rect, &resp);
    }

    let names: Vec<String> = form::METHODS
        .iter()
        .map(|(_, label)| form::method_name(label))
        .collect();
    let labels = form::METHODS.map(|(_, label)| label);
    let active = form::METHODS.iter().position(|(m, _)| *m == place.method);
    if let Some(i) = widgets::segment_row(ui, controls, &names, &labels, active.unwrap_or(0)) {
        place.method = form::METHODS[i].0;
    }

    match place.method {
        PlaceMethod::Globe => {
            if form::not_wide(file.placement.size) {
                note(ui, &form::not_wide_text());
            }
        }
        PlaceMethod::Extents => {
            let axes = [Axis::Lon, Axis::Lon, Axis::Lat, Axis::Lat];
            for (i, label) in form::EDGES.iter().enumerate() {
                let valid = form::parse_degrees(&place.edges[i], axes[i]).is_some();
                let text = &mut place.edges[i];
                widgets::text_field(ui, controls, label, label, None, text, valid);
            }
        }
        PlaceMethod::Points => {
            for (i, label) in form::POINTS.iter().enumerate() {
                let lon = form::coordinate_name(i, Axis::Lon);
                let lat = form::coordinate_name(i, Axis::Lat);
                let [a, b] = &mut place.coordinates[i];
                let valid = [
                    form::parse_degrees(a, Axis::Lon).is_some(),
                    form::parse_degrees(b, Axis::Lat).is_some(),
                ];
                widgets::text_pair(ui, controls, [&lon, &lat], label, [a, b], valid);
            }
            if let Ok(placement) = place.candidate() {
                let text = form::extents_text(&placement.extents);
                widgets::muted_row(ui, form::EXTENTS, &text);
            }
        }
    }
    if let Err(hint) = place.candidate() {
        note(ui, hint);
    }
    let ready = place.ready().is_some();
    form::preview(state);
    ui.add_space(8.0);
    if widgets::button_row(ui, controls, form::PLACE, ready).clicked() {
        form::place(state);
    }
    ui.add_space(8.0);
}

/// A warning of one or two lines.
fn note(ui: &mut egui::Ui, text: &str) {
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::WARN);
    ui.add_space(4.0);
    ui.add(egui::Label::new(text).wrap().selectable(false));
    ui.add_space((PANEL_ROW - 40.0).max(4.0));
}
