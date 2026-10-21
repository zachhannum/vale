//! The style panel of the desktop: the display and the color stops of a
//! raster layer.

use eframe::egui;
use vale_store::{Raster, RasterDisplay, RasterId, Smoothing};
use vale_terrain::{Bands, Rgb, Side, StopLine};

use super::gradient::{self, DESKTOP};
use super::{Action, AppState, LayerRef, pad};
use crate::globe::preview::Preview;
use crate::world;

/// The range of the factor of the width of a river line.
pub const RIVER_WIDTH: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The button of the tool bar that opens the panel.
pub const PANEL: &str = "Layer style";

/// The number of the values of a range that a stop can land on when you
/// add or drag it. A stop of a height lands on a multiple of 10 meters.
pub const SNAPS: f64 = 1200.0;

/// The place of a value on the bar, from 0 at the lowest to 1 at the
/// highest. With a sea split, each side of sea level has a square root
/// scale, so the stops near sea level have room. With no sea split, the
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

/// The selected color stop of a raster layer.
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
                gradient::editor(
                    ui,
                    None,
                    &DESKTOP,
                    bands,
                    &mut chosen,
                    preview.display,
                    unit,
                );
                selected_stop(ui, bands, &mut chosen, unit);
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
            smoothing(ui, state, &raster);
            let unit = raster.unit.clone();
            let globe = &mut state.globe;
            let mut chosen = selected(&globe.preview, Some(id));
            if let Some(bands) = globe.bands_mut(id) {
                gradient::editor(ui, None, &DESKTOP, bands, &mut chosen, display, &unit);
                selected_stop(ui, bands, &mut chosen, &unit);
                if reset_button(ui, bands, height) {
                    chosen = None;
                }
            }
            globe.preview.selected = chosen.map(|index| (id, index));
            let import = ui.button(pad::raster::IMPORT_COLORS);
            if import
                .on_hover_text(pad::raster::IMPORT_COLORS_HINT)
                .clicked()
            {
                state.actions.push(Action::ImportColorsDialog(id));
            }
            if let Some(note) = state.colors_note.as_ref().filter(|n| n.layer == Some(id)) {
                let color = if note.warning {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(egui::RichText::new(&note.text).color(color));
            }
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

/// The switch of the smoothing of a raster layer and its minimum feature
/// size.
fn smoothing(ui: &mut egui::Ui, state: &mut AppState, raster: &Raster) {
    let mut smoothing = raster.smoothing;
    if ui
        .checkbox(&mut smoothing.on, pad::raster::SMOOTHING)
        .changed()
    {
        state.set_raster_smoothing(raster.id, smoothing.on);
    }
    let size = egui::Slider::new(
        &mut smoothing.feature_size,
        0.0..=Smoothing::FEATURE_SIZE_MAX,
    )
    .step_by(0.25)
    .suffix(" pt")
    .text(pad::raster::FEATURE_SIZE);
    if ui.add_enabled(smoothing.on, size).changed() {
        state.set_raster_feature_size(raster.id, smoothing.feature_size);
    }
}

/// The button that puts back the default color stops. Its menu asks
/// first. Returns true after a reset.
fn reset_button(ui: &mut egui::Ui, bands: &mut Bands, height: bool) -> bool {
    let Some(default) = Bands::with_range(bands.min(), bands.max(), height) else {
        return false;
    };
    let changed = default.stops() != bands.stops();
    let mut reset = false;
    ui.add_enabled_ui(changed, |ui| {
        ui.menu_button(format!("{}…", pad::raster::RESET_STOPS), |ui| {
            ui.label("Put back the default color stops? You cannot undo this.");
            if ui.button(pad::raster::RESET).clicked() {
                reset = true;
                ui.close();
            }
        });
    });
    if reset {
        *bands = default;
    }
    reset
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

/// The controls of the selected stop: its value, its colors, and a button
/// that removes it.
fn selected_stop(ui: &mut egui::Ui, bands: &mut Bands, selected: &mut Option<usize>, unit: &str) {
    let span = bands.max() - bands.min();
    let Some(index) = selected.filter(|i| *i < bands.stops().len()) else {
        *selected = None;
        ui.weak(gradient::HINT);
        return;
    };
    let stop = bands.stops()[index];
    let fixed = bands.fixed(index);
    ui.horizontal(|ui| {
        if fixed {
            ui.label(world::value_text(stop.value, span, unit));
        } else {
            let mut value = stop.value;
            let field = egui::DragValue::new(&mut value)
                .speed(span / SNAPS)
                .suffix(format!(" {unit}"));
            if ui.add(field).changed() {
                bands.move_to(index, value);
            }
        }
        let sides: &[(Side, &str)] = if bands.sea() == Some(index) {
            &[(Side::Below, "Sea"), (Side::Above, "Land")]
        } else {
            &[(Side::Above, "")]
        };
        for &(side, label) in sides {
            let mut color = match side {
                Side::Below => stop.below,
                Side::Above => stop.above,
            };
            if !label.is_empty() {
                ui.label(label);
            }
            if ui.color_edit_button_srgb(&mut color).changed() {
                bands.set_color(index, side, color);
            }
        }
        if !fixed && ui.button("Remove").clicked() && bands.remove(index) {
            *selected = None;
        }
    });
    let mut line = stop.line;
    ui.horizontal(|ui| {
        let mut changed = ui.checkbox(&mut line.on, pad::raster::LINE).changed();
        let weight = egui::DragValue::new(&mut line.weight)
            .range(StopLine::WEIGHTS)
            .speed(0.05)
            .suffix(" pt");
        changed |= ui.add(weight).changed();
        if changed && bands.stops().get(index).is_some() {
            bands.set_line(index, line);
        }
    });
    ui.horizontal(|ui| {
        let mut changed = ui.checkbox(&mut line.darker, pad::raster::DARKER).changed();
        if line.darker {
            let darken = egui::DragValue::new(&mut line.darken)
                .range(0.0..=1.0)
                .speed(0.01)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                .custom_parser(|t| {
                    t.trim_end_matches('%')
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 100.0)
                });
            changed |= ui.add(darken).changed();
        } else {
            changed |= ui.color_edit_button_srgb(&mut line.color).changed();
        }
        if changed && bands.stops().get(index).is_some() {
            bands.set_line(index, line);
        }
    });
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
