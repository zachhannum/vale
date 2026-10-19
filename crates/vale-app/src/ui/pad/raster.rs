//! The Style section of the panel of a raster layer: the display, the
//! ramp, and the band limits.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Rgb};

use super::geometry::TOUCH;
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::{AppState, elevation};
use crate::world;

/// The name of the segmented control of the displays.
pub const SHOW_AS: &str = "Show as";
/// The name of the row that opens and closes the colors of the ramp.
pub const RAMP: &str = "Ramp";
/// The name of the field of the selected band limit.
pub const LIMIT: &str = "Band limit";
pub const ADD_LIMIT: &str = "Add a limit";
pub const REMOVE_LIMIT: &str = "Remove the limit";
/// The name of the unit field.
pub const UNIT: &str = "Unit";
pub const RIVERS: &str = "Rivers";
pub const RIVER_WIDTH: &str = "River width";

/// A touch this near to a band limit takes the limit, in points.
const GRAB: f32 = TOUCH / 2.0;
/// The height of the heading of a section.
const HEADING: f32 = 40.0;

/// The state of the Style section of the iPad layout.
#[derive(Default)]
pub struct StyleState {
    /// The text of the unit field while the user types.
    unit: Option<(RasterId, String)>,
    /// The text of the field of the selected limit while the user types.
    limit: Option<(RasterId, usize, String)>,
    /// The colors of the ramp of this layer are open.
    ramp: Option<RasterId>,
    /// The color of the ramp that the color picker edits: `true` for a sea
    /// color, and the index of the color.
    swatch: Option<(bool, usize)>,
}

/// The name of the swatch of a color of the ramp.
pub fn swatch_name(sea: bool, index: usize) -> String {
    let side = if sea { "Sea" } else { "Land" };
    format!("{side} color {}", index + 1)
}

/// The heading of a section of a panel.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING), Sense::hover());
    let at = pos2(rect.left(), rect.bottom() - 8.0);
    ui.painter()
        .text(at, Align2::LEFT_BOTTOM, text, theme::title(), theme::TEXT);
}

/// The unit row. A height layer has no unit row, because its unit cannot
/// change, and the row of its values shows the unit.
pub fn unit_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let unit = raster.unit.clone();
    if crate::ui::is_height(raster) {
        return;
    }
    let mut text = match state.pad.style.unit.take() {
        Some((of, text)) if of == id => text,
        _ => unit.clone(),
    };
    let valid = text.trim() != crate::ui::HEIGHT_UNIT;
    let field = widgets::text_field(ui, controls, UNIT, UNIT, None, &mut text, valid);
    if field.changed() {
        state.set_raster_unit(id, &text);
    } else if !field.has_focus() {
        text = unit;
    }
    state.pad.style.unit = Some((id, text));
}

/// The Style section: the display, the ramp, and the band limits, and then
/// the rivers of a height layer.
pub fn style(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (display, unit) = (raster.display, raster.unit.clone());
    let height = crate::ui::is_height(raster);

    heading(ui, "Style");
    let active = RasterDisplay::ALL.iter().position(|d| *d == display);
    let choice = widgets::choice_row(ui, controls, SHOW_AS, &elevation::DISPLAYS, active);
    if let Some(index) = choice {
        state.set_raster_display(id, RasterDisplay::ALL[index]);
    }
    let display = state.doc.project.raster(id).map_or(display, |r| r.display);

    let style = &mut state.pad.style;
    let globe = &mut state.globe;
    let mut selected = elevation::selected(&globe.preview, Some(id));
    let Some(bands) = globe.bands_mut(id) else {
        return;
    };
    if ramp_row(ui, controls, bands).clicked() {
        style.ramp = if style.ramp == Some(id) {
            None
        } else {
            Some(id)
        };
        style.swatch = None;
    }
    if style.ramp == Some(id) {
        ramp_colors(ui, controls, bands, &mut style.swatch);
    }

    heading(ui, "Band limits");
    let before = selected;
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(theme::MUTE);
        elevation::limit_bar(ui, bands, &mut selected, display, &unit, GRAB);
    });
    ui.add_space(8.0);
    if selected != before {
        style.limit = None;
    }
    let count = bands.limits().len();
    match selected.filter(|index| *index < count) {
        Some(index) => {
            limit_field(ui, controls, bands, &mut style.limit, id, index, &unit);
            if widgets::button_row_in(ui, controls, REMOVE_LIMIT, true, theme::raise()).clicked()
                && bands.remove(index)
            {
                selected = None;
                style.limit = None;
            }
        }
        None => selected = None,
    }
    let full = bands.limits().len() + 1 >= vale_terrain::MAX_BANDS;
    if widgets::button_row(ui, controls, ADD_LIMIT, !full).clicked() {
        selected = bands.add(elevation::new_limit(bands));
        style.limit = None;
    }
    let hint = if selected.is_some() {
        "Each raster layer has its own band limits."
    } else {
        elevation::BAR_HINT
    };
    note(ui, hint);

    let preview = &mut state.globe.preview;
    preview.selected = selected.map(|index| (id, index));
    if height {
        heading(ui, "Rivers");
        widgets::switch_row(ui, controls, RIVERS, RIVERS, &mut preview.rivers);
        let range = elevation::RIVER_WIDTH;
        let (low, high) = (f64::from(*range.start()), f64::from(*range.end()));
        let mut fraction = (f64::from(preview.river_width) - low) / (high - low);
        let text = format!("{:.2}×", preview.river_width);
        if widgets::row_slider(ui, controls, RIVER_WIDTH, "Width", &mut fraction, &text) {
            preview.river_width = (low + fraction * (high - low)) as f32;
        }
    }
}

/// A small muted text across the panel.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::MUTE);
    ui.add(egui::Label::new(text).wrap());
    ui.add_space(6.0);
}

fn color32(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// The Ramp row: the label, and the colors of the ramp from the lowest
/// value at the left to the highest at the right. A tap opens the colors.
fn ramp_row(ui: &mut egui::Ui, controls: &mut Controls, bands: &Bands) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = widgets::tap_area(ui, controls, rect, RAMP, true);
    let at = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, RAMP, theme::body(), theme::MUTE);
    let strip = Rect::from_min_max(
        pos2(rect.left() + 92.0, rect.center().y - 10.0),
        pos2(rect.right(), rect.center().y + 10.0),
    );
    let x_of = |value: f64| {
        let fraction = elevation::bar_fraction(bands, value) as f32;
        strip.left() + fraction * strip.width()
    };
    let mut mesh = egui::Mesh::default();
    for band in bands.bands() {
        let (left, right) = (x_of(band.min), x_of(band.max));
        let at = mesh.vertices.len() as u32;
        let (low, high) = (color32(band.bottom), color32(band.top));
        mesh.colored_vertex(pos2(left, strip.top()), low);
        mesh.colored_vertex(pos2(right, strip.top()), high);
        mesh.colored_vertex(pos2(right, strip.bottom()), high);
        mesh.colored_vertex(pos2(left, strip.bottom()), low);
        mesh.add_triangle(at, at + 1, at + 2);
        mesh.add_triangle(at, at + 2, at + 3);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    resp
}

/// The colors of the ramp as swatches. A tap on a swatch opens the color
/// picker below the swatches.
fn ramp_colors(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    swatch: &mut Option<(bool, usize)>,
) {
    let split = bands.sea().is_some();
    let sides: &[bool] = if split { &[true, false] } else { &[false] };
    for &sea in sides {
        let colors = if sea {
            &bands.ramp.sea
        } else {
            &bands.ramp.land
        };
        let label = match (split, sea) {
            (false, _) => "Colors",
            (true, true) => "Sea",
            (true, false) => "Land",
        };
        let row = ui.available_width();
        let per_row = ((row / TOUCH).floor() as usize).max(1);
        let rows = colors.len().div_ceil(per_row).max(1);
        let size = vec2(row, 22.0 + rows as f32 * TOUCH);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let at = pos2(rect.left(), rect.top() + 11.0);
        ui.painter()
            .text(at, Align2::LEFT_CENTER, label, theme::small(), theme::MUTE);
        for (index, color) in colors.iter().enumerate() {
            let (col, line) = (index % per_row, index / per_row);
            let min = pos2(
                rect.left() + col as f32 * TOUCH,
                rect.top() + 22.0 + line as f32 * TOUCH,
            );
            let place = Rect::from_min_size(min, vec2(TOUCH, TOUCH));
            let name = swatch_name(sea, index);
            let resp = widgets::tap_area(ui, controls, place, &name, true);
            let on = *swatch == Some((sea, index));
            let inner = place.shrink(widgets::HIGHLIGHT_INSET + 2.0);
            ui.painter()
                .rect_filled(inner, theme::CONTROL_RADIUS - 2.0, color32(*color));
            if on {
                let stroke = egui::Stroke::new(2.0, theme::TEXT);
                ui.painter().rect_stroke(
                    inner,
                    theme::CONTROL_RADIUS - 2.0,
                    stroke,
                    egui::StrokeKind::Outside,
                );
            }
            if resp.clicked() {
                *swatch = if on { None } else { Some((sea, index)) };
            }
        }
    }
    let Some((sea, index)) = *swatch else {
        return;
    };
    let colors = if sea {
        &mut bands.ramp.sea
    } else {
        &mut bands.ramp.land
    };
    let Some(color) = colors.get_mut(index) else {
        *swatch = None;
        return;
    };
    ui.add_space(8.0);
    ui.scope(|ui| {
        let width = ui.available_width();
        let spacing = ui.spacing_mut();
        spacing.slider_width = width;
        spacing.interact_size.y = TOUCH;
        let mut picked = color32(*color);
        let alpha = egui::color_picker::Alpha::Opaque;
        if egui::color_picker::color_picker_color32(ui, &mut picked, alpha) {
            *color = [picked.r(), picked.g(), picked.b()];
        }
    });
    ui.add_space(8.0);
}

/// The field of the selected limit. The value changes when the text is a
/// number.
fn limit_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    text: &mut Option<(RasterId, usize, String)>,
    id: RasterId,
    index: usize,
    unit: &str,
) {
    let span = bands.max() - bands.min();
    let value = bands.limits()[index];
    let shown = world::value_text(value, span, "");
    let mut edit = match text.take() {
        Some((of, at, edit)) if of == id && at == index => edit,
        _ => shown.clone(),
    };
    let parsed = parse_value(&edit);
    let resp = widgets::value_field(
        ui,
        controls,
        LIMIT,
        LIMIT,
        unit,
        &mut edit,
        parsed.is_some(),
    );
    if resp.changed()
        && let Some(value) = parse_value(&edit)
    {
        bands.move_to(index, value);
    } else if !resp.has_focus() {
        edit = shown;
    }
    *text = Some((id, index, edit));
}

/// A number as the user types it, with or without the separators of the
/// thousands, and with a hyphen or a minus sign.
pub fn parse_value(text: &str) -> Option<f64> {
    let clean: String = text
        .trim()
        .chars()
        .filter(|c| *c != ',' && *c != ' ')
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect();
    clean.parse().ok().filter(|v: &f64| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_value_can_have_separators_and_a_minus_sign() {
        assert_eq!(parse_value("1,800"), Some(1800.0));
        assert_eq!(parse_value(" \u{2212}1,000 "), Some(-1000.0));
        assert_eq!(parse_value("-3.5"), Some(-3.5));
        assert_eq!(parse_value("abc"), None);
        assert_eq!(parse_value("inf"), None);
    }
}
