//! The Style section of the panel of a raster layer: the display and the
//! color stops.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Rgb, Side};

use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::{AppState, elevation, gradient};
use crate::world;

/// The name of the segmented control of the displays.
pub const SHOW_AS: &str = "Show as";
/// The name of the field of the selected stop.
pub const STOP_VALUE: &str = "Stop value";
/// The name of the swatch of a stop.
pub const COLOR: &str = "Color";
/// The names of the swatches of the stop at sea level.
pub const SEA_COLOR: &str = "Sea";
pub const LAND_COLOR: &str = "Land";
pub const REMOVE_STOP: &str = "Remove the stop";
/// The name of the unit field.
pub const UNIT: &str = "Unit";
pub const RIVERS: &str = "Rivers";
pub const RIVER_WIDTH: &str = "River width";

/// The height of the heading of a section.
const HEADING: f32 = 40.0;
/// The place of the swatches in the Color row.
const SWATCHES: f32 = 92.0;

/// The state of the Style section of the iPad layout.
#[derive(Default)]
pub struct StyleState {
    /// The text of the unit field while the user types.
    unit: Option<(RasterId, String)>,
    /// The text of the field of the selected stop while the user types.
    value: Option<(RasterId, usize, String)>,
    /// The color of a stop that the color picker edits.
    swatch: Option<(RasterId, usize, Side)>,
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

/// The Style section: the display and the color stops, and then the
/// rivers of a height layer.
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
    let before = selected;
    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(theme::MUTE);
        let m = &gradient::PAD;
        gradient::editor(ui, Some(controls), m, bands, &mut selected, display, &unit);
    });
    if selected != before {
        style.value = None;
        style.swatch = None;
    }
    match selected {
        Some(index) => stop_rows(ui, controls, bands, style, id, index, &unit, &mut selected),
        None => note(ui, gradient::HINT),
    }

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

/// The rows of the selected stop: its value, and its colors with the
/// button that removes it. A tap on a swatch opens the color picker below.
#[allow(clippy::too_many_arguments)]
fn stop_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    style: &mut StyleState,
    id: RasterId,
    index: usize,
    unit: &str,
    selected: &mut Option<usize>,
) {
    let fixed = bands.fixed(index);
    if fixed {
        let span = bands.max() - bands.min();
        let value = bands.stops()[index].value;
        widgets::value_row(ui, "Value", &world::value_text(value, span, unit));
    } else {
        value_field(ui, controls, bands, &mut style.value, id, index, unit);
    }

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let y = rect.center().y;
    let at = pos2(rect.left(), y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, "Color", theme::body(), theme::MUTE);
    let stop = bands.stops()[index];
    let sides: &[(Side, &str)] = if bands.sea() == Some(index) {
        &[(Side::Below, SEA_COLOR), (Side::Above, LAND_COLOR)]
    } else {
        &[(Side::Above, COLOR)]
    };
    let mut x = rect.left() + SWATCHES;
    for &(side, name) in sides {
        let place = Rect::from_min_size(pos2(x, y - TOUCH / 2.0), vec2(TOUCH, TOUCH));
        let resp = widgets::tap_area(ui, controls, place, name, true);
        let color = match side {
            Side::Below => stop.below,
            Side::Above => stop.above,
        };
        let open = style.swatch == Some((id, index, side));
        let inner = place.shrink(widgets::HIGHLIGHT_INSET + 2.0);
        let radius = theme::CONTROL_RADIUS - 2.0;
        let painter = ui.painter();
        painter.rect_filled(inner, radius, color32(color));
        let stroke = match open {
            true => egui::Stroke::new(2.0, theme::TEXT),
            false => egui::Stroke::new(1.0, theme::divider()),
        };
        painter.rect_stroke(inner, radius, stroke, egui::StrokeKind::Outside);
        x = place.right() + 4.0;
        if sides.len() > 1 {
            let at = pos2(x, y);
            let label = painter.text(at, Align2::LEFT_CENTER, name, theme::small(), theme::MUTE);
            x = label.right() + 16.0;
        }
        if resp.clicked() {
            style.swatch = if open { None } else { Some((id, index, side)) };
        }
    }
    if !fixed {
        let place = Rect::from_min_size(
            pos2(rect.right() - TOUCH, y - TOUCH / 2.0),
            vec2(TOUCH, TOUCH),
        );
        let resp = widgets::tap_area(ui, controls, place, REMOVE_STOP, true);
        if resp.is_pointer_button_down_on() {
            let back = place.shrink(widgets::HIGHLIGHT_INSET);
            ui.painter()
                .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
        }
        icons::paint(ui.painter(), place, Icon::Close, 22.0, theme::WARN);
        if resp.clicked() && bands.remove(index) {
            *selected = None;
            style.value = None;
            style.swatch = None;
            return;
        }
    }

    let Some((_, _, side)) = style.swatch.filter(|s| s.0 == id && s.1 == index) else {
        return;
    };
    let color = match side {
        Side::Below => stop.below,
        Side::Above => stop.above,
    };
    ui.add_space(8.0);
    let mut picked = color;
    if widgets::color_picker(ui, controls, COLOR, &mut picked) {
        bands.set_color(index, side, picked);
    }
    ui.add_space(8.0);
}

/// The field of the value of the selected stop. The stop moves when the
/// text is a number.
fn value_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    text: &mut Option<(RasterId, usize, String)>,
    id: RasterId,
    index: usize,
    unit: &str,
) {
    let span = bands.max() - bands.min();
    let value = bands.stops()[index].value;
    let shown = world::value_text(value, span, "");
    let mut edit = match text.take() {
        Some((of, at, edit)) if of == id && at == index => edit,
        _ => shown.clone(),
    };
    let parsed = parse_value(&edit);
    let resp = widgets::value_field(
        ui,
        controls,
        STOP_VALUE,
        "Value",
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

    #[test]
    fn a_hex_code_gives_its_color() {
        use super::widgets::{hex_text, parse_hex};
        assert_eq!(parse_hex("#1A2b3C"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(parse_hex(" ff0000 "), Some([255, 0, 0]));
        assert_eq!(parse_hex("ff00"), None);
        assert_eq!(parse_hex("gg0000"), None);
        assert_eq!(parse_hex("ÿÿÿ"), None);
        assert_eq!(hex_text([1, 171, 255]), "01ABFF");
    }
}
