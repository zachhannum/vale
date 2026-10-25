//! The style panel of the desktop: the display and the color stops of a
//! raster layer.

use eframe::egui;
use vale_store::{Hillshade, Raster, RasterDisplay, RasterId, Smoothing};
use vale_terrain::{Bands, Rgb, StopLine};

use super::gradient::{self, DESKTOP};
use super::pad::raster::RampChoice;
use super::{Action, AppState, LayerRef, pad};
use crate::globe::preview::Preview;
use crate::world;

/// The range of the factor of the width of a river line.
pub const RIVER_WIDTH: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The most height of the list of the stops.
const STOP_LIST: f32 = 240.0;

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

/// The color of a value in a band in a display.
pub fn band_color_at(
    bands: &Bands,
    band: &vale_terrain::Band,
    display: RasterDisplay,
    value: f64,
) -> egui::Color32 {
    let value = value.clamp(band.min, band.max);
    match display {
        RasterDisplay::Bands => color32(band.color),
        RasterDisplay::Gradient => color32(band.gradient_at(value)),
        RasterDisplay::Grey => grey(bands, value),
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
                ui.checkbox(&mut preview.rivers, pad::raster::RIVERS);
                let on = preview.rivers;
                river_width(ui, &mut preview, on);
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
                stop_list(ui, bands, &mut chosen, unit);
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
            if height {
                rivers(ui, state, &raster);
            }
            ui.separator();
            ui.label("Style");
            let mut display = raster.display;
            if display_choice(ui, &mut display) {
                state.set_raster_display(id, display);
            }
            smoothing(ui, state, &raster);
            let mut lines = raster.lines;
            if ui.checkbox(&mut lines, pad::raster::BAND_LINES).changed() {
                state.set_raster_lines(id, lines);
            }
            if height {
                hillshade(ui, state, &raster);
            }
            let unit = raster.unit.clone();
            let globe = &mut state.globe;
            let mut chosen = selected(&globe.preview, Some(id));
            if let Some(bands) = globe.bands_mut(id) {
                gradient::editor(ui, None, &DESKTOP, bands, &mut chosen, display, &unit);
                stop_list(ui, bands, &mut chosen, &unit);
                selected_stop(ui, bands, &mut chosen, &unit);
                if reset_button(ui, bands, height) {
                    chosen = None;
                }
            }
            globe.preview.selected = chosen.map(|index| (id, index));
            ui.menu_button(pad::raster::COLOR_RAMPS, |ui| {
                let import = ui.button(pad::raster::IMPORT_RAMP);
                if import
                    .on_hover_text(pad::raster::IMPORT_RAMP_HINT)
                    .clicked()
                {
                    state.actions.push(Action::ImportRampDialog(id));
                    ui.close();
                }
                ramp_menus(ui, state, id);
            });
            if let Some(note) = state.ramp_note.as_ref().filter(|n| n.layer == Some(id)) {
                let color = if note.warning {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(egui::RichText::new(&note.text).color(color));
            }
        });
}

/// One menu for each color ramp, with the ways to apply it, its reverse,
/// and its delete.
fn ramp_menus(ui: &mut egui::Ui, state: &mut AppState, id: RasterId) {
    state.open_ramps();
    let Some(layer) = state.globe.bands_mut(id).map(|b| b.clone()) else {
        return;
    };
    let split = layer.sea().is_some();
    let mut chosen = None;
    for (index, ramp) in state.ramps.list().iter().enumerate() {
        let colors = ramp.colors.len();
        // The part that the user chose stays in the memory of egui.
        let key = egui::Id::new(("ramp-part", id, index));
        let picked: Option<usize> = ui.data(|d| d.get_temp(key));
        let part = picked.map_or(layer.part(&ramp.colors), |i| pad::raster::PARTS[i]);
        let (bands, kind) = pad::raster::ramp_fit(&layer, part);
        ui.menu_button(format!("{} ({colors})", ramp.name), |ui| {
            if split {
                ui.horizontal(|ui| {
                    ui.label(pad::raster::RAMP_PART);
                    for (i, name) in pad::raster::PART_NAMES.iter().enumerate() {
                        let on = pad::raster::PARTS[i] == part;
                        if ui.selectable_label(on, *name).clicked() {
                            ui.data_mut(|d| d.insert_temp(key, i));
                        }
                    }
                });
                ui.separator();
            }
            let mut choose = |ui: &mut egui::Ui, text: &str, choice: RampChoice| {
                if ui.button(text).clicked() {
                    chosen = Some((index, part, choice));
                    // The menu stays open after a reverse.
                    if choice != RampChoice::Reverse {
                        ui.close();
                    }
                }
            };
            if colors == bands {
                choose(ui, pad::raster::APPLY_RAMP, RampChoice::Apply(true));
            } else {
                let keep = format!("Fit to {bands} {kind}bands");
                choose(ui, &keep, RampChoice::Apply(true));
                let fresh = format!("Make {colors} {kind}bands");
                choose(ui, &fresh, RampChoice::Apply(false));
            }
            ui.separator();
            choose(ui, pad::raster::REVERSE_RAMP, RampChoice::Reverse);
            choose(ui, pad::raster::DELETE_RAMP, RampChoice::Delete);
        });
    }
    let Some((index, part, choice)) = chosen else {
        return;
    };
    let result = match choice {
        RampChoice::Apply(keep) => state.apply_ramp(id, index, keep, part),
        RampChoice::Reverse => state.ramps.reverse(index).map_err(|e| e.to_string()),
        RampChoice::Delete => state.ramps.remove(index).map_err(|e| e.to_string()),
    };
    if let Err(text) = result {
        state.ramp_note = Some(super::Note::new(text, true, Some(id)));
    }
}

/// The name of the unit field.
pub const UNIT: &str = "Unit";

/// The switch of the rivers of a height layer, and the width of all rivers.
fn rivers(ui: &mut egui::Ui, state: &mut AppState, raster: &Raster) {
    let mut on = raster.rivers;
    if ui.checkbox(&mut on, pad::raster::RIVERS).changed() {
        state.set_raster_rivers(raster.id, on);
    }
    river_width(ui, &mut state.globe.preview, on);
}

/// The slider of the width of the rivers.
fn river_width(ui: &mut egui::Ui, preview: &mut Preview, enabled: bool) {
    let width = egui::Slider::new(&mut preview.river_width, RIVER_WIDTH).text("River width");
    ui.add_enabled(enabled, width);
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

/// The switch of the hillshade of a height layer, its opacity, and its
/// highlights.
fn hillshade(ui: &mut egui::Ui, state: &mut AppState, raster: &Raster) {
    let mut hillshade = raster.hillshade;
    if ui
        .checkbox(&mut hillshade.on, pad::raster::HILLSHADE)
        .changed()
    {
        state.set_raster_hillshade(raster.id, hillshade.on);
    }
    let mut percent = (hillshade.opacity * 100.0).round();
    let opacity = egui::Slider::new(&mut percent, 0.0..=100.0)
        .step_by(1.0)
        .suffix(" %")
        .text(pad::raster::HILLSHADE_OPACITY);
    if ui.add_enabled(hillshade.on, opacity).changed() {
        state.set_raster_hillshade_opacity(raster.id, percent / 100.0);
    }
    let highlights = egui::Checkbox::new(&mut hillshade.highlights, "Highlights");
    if ui.add_enabled(hillshade.on, highlights).changed() {
        state.set_raster_hillshade_highlights(raster.id, hillshade.highlights);
    }
    let azimuth = egui::Slider::new(&mut hillshade.azimuth, 0.0..=360.0)
        .step_by(1.0)
        .suffix("\u{b0}")
        .text(pad::raster::HILLSHADE_AZIMUTH);
    if ui.add_enabled(hillshade.on, azimuth).changed() {
        state.set_raster_hillshade_azimuth(raster.id, hillshade.azimuth);
    }
    let altitude = egui::Slider::new(&mut hillshade.altitude, Hillshade::ALTITUDES)
        .step_by(1.0)
        .suffix("\u{b0}")
        .text(pad::raster::HILLSHADE_ALTITUDE);
    if ui.add_enabled(hillshade.on, altitude).changed() {
        state.set_raster_hillshade_altitude(raster.id, hillshade.altitude);
    }
    let relief = egui::Slider::new(&mut hillshade.exaggeration, Hillshade::EXAGGERATIONS)
        .logarithmic(true)
        .suffix("\u{d7}")
        .text(pad::raster::HILLSHADE_RELIEF);
    if ui.add_enabled(hillshade.on, relief).changed() {
        state.set_raster_hillshade_exaggeration(raster.id, hillshade.exaggeration);
    }
    ui.add_enabled_ui(hillshade.on, |ui| {
        ui.horizontal(|ui| {
            if ui.color_edit_button_srgb(&mut hillshade.color).changed() {
                state.set_raster_hillshade_color(raster.id, hillshade.color);
            }
            ui.label(pad::raster::HILLSHADE_COLOR);
        });
    });
}

/// The button that puts back the default color stops. Its menu asks
/// first. Returns true after a reset.
fn reset_button(ui: &mut egui::Ui, bands: &mut Bands, height: bool) -> bool {
    let Some(default) = Bands::with_range(bands.min(), bands.max(), height) else {
        return false;
    };
    let changed = default != *bands;
    let mut reset = false;
    ui.add_enabled_ui(changed, |ui| {
        ui.menu_button(format!("{}…", pad::raster::RESET_STOPS), |ui| {
            ui.label("Reset to the default color stops? This cannot be undone.");
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

/// The name of the color button and of the value field of the selected
/// band.
pub const BAND_COLOR: &str = "Band color";
pub const BAND_VALUE: &str = "Lower edge";
/// The button that removes the stop at the lower edge of the selected band.
pub const REMOVE: &str = "Remove stop";

/// The list of the bands, with the highest band first. A row shows the
/// color of the band, the value of its lower edge, and the line at that
/// edge. A click on a row selects the band, and a second click clears the
/// selection.
fn stop_list(ui: &mut egui::Ui, bands: &Bands, selected: &mut Option<usize>, unit: &str) {
    let span = bands.max() - bands.min();
    let scroll = egui::ScrollArea::vertical()
        .id_salt("stop-list")
        .max_height(STOP_LIST);
    scroll.show(ui, |ui| {
        let stops = bands.stops();
        for (index, color) in bands.colors().iter().enumerate().rev() {
            ui.horizontal(|ui| {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, color32(*color));
                let mut text = world::value_text(stops[index].value, span, unit);
                if bands.sea() == Some(index) {
                    text.push_str("  Sea level");
                }
                let on = *selected == Some(index);
                let row = ui.selectable_label(on, text);
                let name = gradient::band_row(index);
                row.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name)
                });
                if row.clicked() {
                    *selected = (!on).then_some(index);
                }
                if let Some(line) = bands.line_color(index).filter(|_| stops[index].line.on) {
                    let (mark, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
                    let width = stops[index].line.weight.clamp(1.0, 4.0);
                    let stroke = egui::Stroke::new(width, color32(line));
                    ui.painter().hline(mark.x_range(), mark.center().y, stroke);
                }
            });
        }
    });
}

/// The controls of the selected band, as on the card of a band: its color,
/// the value of its lower edge, the line at that edge, and the button that
/// removes that stop. The lowest band has only its color.
fn selected_stop(ui: &mut egui::Ui, bands: &mut Bands, selected: &mut Option<usize>, unit: &str) {
    let span = bands.max() - bands.min();
    let Some(index) = selected.filter(|i| *i < bands.colors().len()) else {
        *selected = None;
        ui.weak(gradient::HINT);
        return;
    };
    let stop = bands.stops()[index];
    let fixed = bands.fixed(index);
    let lowest = index == 0;
    ui.horizontal(|ui| {
        let mut color = bands.colors()[index];
        let button = ui.color_edit_button_srgb(&mut color);
        button.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, BAND_COLOR)
        });
        if button.changed() {
            bands.set_color(index, color);
        }
        if lowest {
            return;
        }
        if fixed {
            ui.label(world::value_text(stop.value, span, unit));
        } else {
            let mut value = stop.value;
            let field = egui::DragValue::new(&mut value)
                .speed(span / SNAPS)
                .suffix(format!(" {unit}"));
            let field = ui.add(field);
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, BAND_VALUE)
            });
            if field.changed() {
                bands.move_to(index, value);
            }
            if ui.button(REMOVE).clicked() && bands.remove(index) {
                *selected = None;
            }
        }
    });
    if lowest || selected.is_none() {
        return;
    }
    let mut line = stop.line;
    ui.horizontal(|ui| {
        let mut changed = ui.checkbox(&mut line.on, pad::raster::LINE).changed();
        let weight = egui::DragValue::new(&mut line.weight)
            .range(StopLine::WEIGHTS)
            .speed(0.05)
            .suffix(" pt");
        changed |= ui.add(weight).changed();
        if changed {
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
        if changed {
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
