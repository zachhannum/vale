//! The style panel of the desktop: the display, the band limits, and the
//! color ramp of a raster layer.

use eframe::egui;
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Rgb};

use super::{AppState, LayerRef};
use crate::globe::preview::Preview;
use crate::world;

/// The range of the factor of the width of a river line.
pub const RIVER_WIDTH: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The name of the bar for a screen reader and for the tests.
pub const BAR_LABEL: &str = "Band limits";
/// The button of the tool bar that opens the panel.
pub const PANEL: &str = "Layer style";

pub const BAR_HEIGHT: f32 = 340.0;
const BAR_WIDTH: f32 = 56.0;
/// The space above and below the bar, so that a label at an end has room.
const BAR_PAD: f32 = 10.0;
/// A pointer this near to a limit takes the limit, in points.
const GRAB: f32 = 14.0;
/// The number of the values of a range that a limit can land on when you
/// add or drag it. A limit of a height lands on a multiple of 10 meters.
const SNAPS: f64 = 1200.0;

/// The place of a value on the bar, from 0 at the lowest to 1 at the
/// highest. With a sea split, each side of sea level has a square root
/// scale, so the limits near sea level have room. With no sea split, the
/// scale is linear.
pub fn bar_fraction(bands: &Bands, value: f64) -> f64 {
    let (min, max) = (bands.min(), bands.max());
    if bands.sea().is_none() {
        return ((value - min) / (max - min)).clamp(0.0, 1.0);
    }
    let side = if value < 0.0 {
        -(value / min).sqrt()
    } else {
        (value / max).sqrt()
    };
    0.5 + 0.5 * side.clamp(-1.0, 1.0)
}

/// The value at a place on the bar. The inverse of `bar_fraction`.
pub fn bar_value(bands: &Bands, fraction: f64) -> f64 {
    let (min, max) = (bands.min(), bands.max());
    let fraction = fraction.clamp(0.0, 1.0);
    if bands.sea().is_none() {
        return min + fraction * (max - min);
    }
    let side = fraction * 2.0 - 1.0;
    let range = if side < 0.0 { min } else { max };
    side * side * range
}

/// The bar inside the rectangle of the widget.
fn bar_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(0.0, BAR_PAD),
        egui::pos2(rect.min.x + BAR_WIDTH, rect.max.y - BAR_PAD),
    )
}

/// The screen height of a value on the bar of a widget.
pub fn limit_y(rect: egui::Rect, bands: &Bands, value: f64) -> f32 {
    let bar = bar_rect(rect);
    bar.bottom() - bar_fraction(bands, value) as f32 * bar.height()
}

fn color32(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// The color of a value in the Grey display.
fn grey(bands: &Bands, value: f64) -> egui::Color32 {
    let level = (u32::from(bands.level(value)) * 255 / 65535) as u8;
    egui::Color32::from_gray(level)
}

/// The colors of a band at its top and at its bottom in a display.
pub fn band_colors(
    bands: &Bands,
    band: &vale_terrain::Band,
    display: RasterDisplay,
) -> (egui::Color32, egui::Color32) {
    match display {
        RasterDisplay::Bands => (color32(band.color), color32(band.color)),
        RasterDisplay::Gradient => (color32(band.top), color32(band.bottom)),
        RasterDisplay::Grey => (grey(bands, band.max), grey(bands, band.min)),
    }
}

/// The names of the displays, in the order of `RasterDisplay::ALL`.
pub const DISPLAYS: [&str; 3] = ["Bands", "Gradient", "Grey"];

/// The name of a display.
pub fn display_name(display: RasterDisplay) -> &'static str {
    let index = RasterDisplay::ALL.iter().position(|d| *d == display);
    DISPLAYS[index.unwrap_or(0)]
}

/// The selected band limit of a raster layer.
pub fn selected(preview: &Preview, id: Option<RasterId>) -> Option<usize> {
    let (of, index) = preview.selected?;
    (Some(of) == id).then_some(index)
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.globe.preview.panel {
        return;
    }
    egui::Panel::right("elevation")
        .default_size(210.0)
        .show(ui, |ui| {
            let raster = state.panel_raster().cloned();
            let layer = raster.as_ref().map(|raster| LayerRef::Raster(raster.id));
            let layer = layer.and_then(|layer| state.layer_of(layer));
            let name = layer.map_or(PANEL.to_string(), |layer| layer.name.clone());
            ui.heading(name);
            let Some(raster) = raster else {
                let mut preview = state.globe.preview;
                let (bands, unit) = (&mut state.globe.map.bands, super::HEIGHT_UNIT);
                view_controls(ui, &mut preview, true);
                display_choice(ui, &mut preview.display);
                ui.separator();
                let mut chosen = selected(&preview, None);
                limit_bar(ui, bands, &mut chosen, preview.display, unit, GRAB);
                selected_limit(ui, bands, &mut chosen, unit);
                ramp_buttons(ui, bands);
                state.globe.preview = preview;
                return;
            };
            let id = raster.id;
            let height = super::is_height(&raster);
            ui.horizontal(|ui| {
                ui.label("Unit");
                if height {
                    ui.label(&raster.unit);
                    return;
                }
                let mut unit = raster.unit.clone();
                let field = ui.add(egui::TextEdit::singleline(&mut unit).desired_width(80.0));
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, UNIT)
                });
                if field.changed() {
                    state.set_raster_unit(id, &unit);
                }
            });
            view_controls(ui, &mut state.globe.preview, height);
            ui.separator();
            ui.label("Style");
            let mut display = raster.display;
            if display_choice(ui, &mut display) {
                state.set_raster_display(id, display);
            }
            let unit = raster.unit.clone();
            let globe = &mut state.globe;
            let mut chosen = selected(&globe.preview, Some(id));
            if let Some(bands) = globe.bands_mut(id) {
                limit_bar(ui, bands, &mut chosen, display, &unit, GRAB);
                selected_limit(ui, bands, &mut chosen, &unit);
                ui.separator();
                ramp_buttons(ui, bands);
            }
            globe.preview.selected = chosen.map(|index| (id, index));
        });
}

/// The name of the unit field.
pub const UNIT: &str = "Unit";

/// The rivers of a height layer.
fn view_controls(ui: &mut egui::Ui, preview: &mut Preview, height: bool) {
    if height {
        ui.checkbox(&mut preview.rivers, "Rivers");
        let width = egui::Slider::new(&mut preview.river_width, RIVER_WIDTH).text("River width");
        ui.add_enabled(preview.rivers, width);
    }
}

/// The three displays as a row of buttons. Returns true if the user chose
/// another display.
fn display_choice(ui: &mut egui::Ui, display: &mut RasterDisplay) -> bool {
    let before = *display;
    ui.horizontal(|ui| {
        for (choice, name) in RasterDisplay::ALL.into_iter().zip(DISPLAYS) {
            ui.selectable_value(display, choice, name);
        }
    });
    *display != before
}

/// A button for each color of the ramp.
fn ramp_buttons(ui: &mut egui::Ui, bands: &mut Bands) {
    // A range with no sea split has one ramp.
    if bands.sea().is_none() {
        ui.label("Colors");
        ui.horizontal_wrapped(|ui| {
            for color in &mut bands.ramp.land {
                ui.color_edit_button_srgb(color);
            }
        });
        return;
    }
    ui.label("Land colors");
    ui.horizontal_wrapped(|ui| {
        for color in &mut bands.ramp.land {
            ui.color_edit_button_srgb(color);
        }
    });
    ui.label("Sea colors");
    ui.horizontal_wrapped(|ui| {
        for color in &mut bands.ramp.sea {
            ui.color_edit_button_srgb(color);
        }
    });
}

/// The bar with the bands and the limits, in the colors of `display`. A
/// tap on a free place adds a limit. A tap on a limit selects it, and a
/// drag moves it. A touch that is `grab` points or nearer to a limit takes
/// the limit.
pub(super) fn limit_bar(
    ui: &mut egui::Ui,
    bands: &mut Bands,
    selected: &mut Option<usize>,
    display: RasterDisplay,
    unit: &str,
    grab: f32,
) {
    let span = bands.max() - bands.min();
    let snap = span / SNAPS;
    let size = egui::vec2(ui.available_width(), BAR_HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, BAR_LABEL));
    let bar = bar_rect(rect);
    let scale = bands.clone();
    let y_of = |value: f64| limit_y(rect, &scale, value);
    let value_of = |y: f32| {
        let value = bar_value(&scale, f64::from((bar.bottom() - y) / bar.height()));
        (value / snap).round() * snap
    };

    let sea = bands.sea();
    // The limit under a touch.
    let near = |bands: &Bands, y: f32| {
        let off = |m: &f64| (y_of(*m) - y).abs();
        let limits = bands.limits().iter().enumerate();
        let (index, m) = limits.min_by(|a, b| off(a.1).total_cmp(&off(b.1)))?;
        (off(m) <= grab).then_some(index)
    };
    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started()
        && let Some(pos) = press
    {
        *selected = near(bands, pos.y).filter(|i| Some(*i) != sea);
    }
    if resp.dragged()
        && let (Some(index), Some(pos)) = (*selected, resp.interact_pointer_pos())
        && index < bands.limits().len()
    {
        bands.move_to(index, value_of(pos.y));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        *selected = match near(bands, pos.y) {
            Some(index) => Some(index).filter(|i| Some(*i) != sea),
            None => bands.add(value_of(pos.y)),
        };
    }

    let painter = ui.painter_at(rect);
    for band in bands.bands() {
        let (top, bottom) = (y_of(band.max), y_of(band.min));
        let (high, low) = band_colors(bands, &band, display);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(egui::pos2(bar.left(), top), high);
        mesh.colored_vertex(egui::pos2(bar.right(), top), high);
        mesh.colored_vertex(egui::pos2(bar.right(), bottom), low);
        mesh.colored_vertex(egui::pos2(bar.left(), bottom), low);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
    let text = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    for (index, &meters) in bands.limits().iter().enumerate() {
        let y = y_of(meters);
        let chosen = *selected == Some(index);
        let (width, color) = match (chosen, Some(index) == sea) {
            (true, _) => (3.0, strong),
            (false, true) => (2.0, text),
            (false, false) => (1.0, text),
        };
        let ends = [egui::pos2(bar.left(), y), egui::pos2(bar.right() + 8.0, y)];
        painter.line_segment(ends, egui::Stroke::new(width, color));
        let label = if Some(index) == sea {
            "Sea level".to_string()
        } else {
            world::value_text(meters, span, unit)
        };
        painter.text(
            egui::pos2(bar.right() + 12.0, y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }
}

/// The controls of the selected limit.
fn selected_limit(ui: &mut egui::Ui, bands: &mut Bands, selected: &mut Option<usize>, unit: &str) {
    let span = bands.max() - bands.min();
    let count = bands.limits().len();
    let Some(index) = selected.filter(|i| *i < count) else {
        *selected = None;
        ui.weak(BAR_HINT);
        return;
    };
    ui.horizontal(|ui| {
        let mut meters = bands.limits()[index];
        let value = egui::DragValue::new(&mut meters)
            .speed(span / SNAPS)
            .suffix(format!(" {unit}"));
        if ui.add(value).changed() {
            bands.move_to(index, meters);
        }
        if ui.button("Remove").clicked() && bands.remove(index) {
            *selected = None;
        }
    });
}

/// The hint below the bar when no limit is selected.
pub const BAR_HINT: &str = "Tap the bar to add a limit. Drag a limit to move it.";

/// A value in the middle of the tallest band on the bar, for a new limit.
/// It lands on a snap value.
pub fn new_limit(bands: &Bands) -> f64 {
    let place = |value: f64| bar_fraction(bands, value);
    let list = bands.bands();
    let tallest = list.iter().max_by(|a, b| {
        let size = |band: &&vale_terrain::Band| place(band.max) - place(band.min);
        size(a).total_cmp(&size(b))
    });
    let middle = tallest.map_or(0.5, |band| (place(band.min) + place(band.max)) / 2.0);
    let snap = (bands.max() - bands.min()) / SNAPS;
    (bar_value(bands, middle) / snap).round() * snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_scale_has_an_inverse() {
        let height = Bands::default();
        assert_eq!(bar_fraction(&height, height.min()), 0.0);
        assert_eq!(bar_fraction(&height, 0.0), 0.5);
        assert_eq!(bar_fraction(&height, height.max()), 1.0);
        // The scale of a height has more room near sea level.
        assert!(bar_fraction(&height, 1500.0) > 0.5 + 0.5 * 1500.0 / 6000.0);
        for meters in [-5000.0, -200.0, 0.0, 10.0, 1500.0, 6000.0] {
            let back = bar_value(&height, bar_fraction(&height, meters));
            assert!((back - meters).abs() < 1e-6);
        }
    }

    #[test]
    fn a_range_with_no_sea_split_has_a_linear_bar() {
        let rain = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(rain.sea(), None);
        for (value, fraction) in [(0.0, 0.0), (1000.0, 0.25), (2000.0, 0.5), (4000.0, 1.0)] {
            assert_eq!(bar_fraction(&rain, value), fraction);
            assert_eq!(bar_value(&rain, fraction), value);
        }
        assert_eq!(bar_fraction(&rain, -5.0), 0.0);
    }
}
