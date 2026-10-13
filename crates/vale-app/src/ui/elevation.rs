//! The bands panel: the band limits and the color ramp of a raster layer.

use eframe::egui;
use vale_terrain::{Bands, Rgb};

use super::AppState;
use crate::globe::preview::Preview;
use crate::world;

/// The range of the factor of the width of a river line.
pub const RIVER_WIDTH: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The name of the bar for a screen reader and for the tests.
pub const BAR_LABEL: &str = "Band limits";
/// The button of the tool bar that opens the panel.
pub const PANEL: &str = "Bands";

const BAR_HEIGHT: f32 = 340.0;
const BAR_WIDTH: f32 = 56.0;
/// The space above and below the bar, so that a label at an end has room.
const BAR_PAD: f32 = 10.0;
/// A touch this near to a limit takes the limit, in points.
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

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.globe.preview.panel {
        return;
    }
    egui::Panel::right("elevation")
        .default_size(210.0)
        .show(ui, |ui| {
            let raster = state.shown_raster();
            let name = raster.map_or(PANEL.to_string(), |raster| raster.name.clone());
            let unit = state.shown_unit().to_string();
            ui.heading(name);
            let globe = &mut state.globe;
            controls(ui, &mut globe.map.bands, &mut globe.preview, &unit);
        });
}

/// The controls of the preview, and of the bands of one raster layer.
/// `unit` is the unit of the values of the layer. A height in meters has
/// rivers.
pub(super) fn controls(ui: &mut egui::Ui, bands: &mut Bands, preview: &mut Preview, unit: &str) {
    ui.checkbox(&mut preview.levels, "Levels");
    ui.checkbox(&mut preview.graticule, "Graticule");
    if unit == super::HEIGHT_UNIT {
        ui.checkbox(&mut preview.rivers, "Rivers");
        let width = egui::Slider::new(&mut preview.river_width, RIVER_WIDTH).text("River width");
        ui.add_enabled(preview.rivers, width);
    }
    ui.separator();
    limit_bar(ui, bands, preview, unit);
    selected_limit(ui, bands, preview, unit);
    ui.separator();
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

/// The bar with the bands and the limits. A tap on a free place adds a limit.
/// A tap on a limit selects it, and a drag moves it.
fn limit_bar(ui: &mut egui::Ui, bands: &mut Bands, preview: &mut Preview, unit: &str) {
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
        (off(m) <= GRAB).then_some(index)
    };
    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started()
        && let Some(pos) = press
    {
        preview.selected = near(bands, pos.y).filter(|i| Some(*i) != sea);
    }
    if resp.dragged()
        && let (Some(index), Some(pos)) = (preview.selected, resp.interact_pointer_pos())
        && index < bands.limits().len()
    {
        bands.move_to(index, value_of(pos.y));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        preview.selected = match near(bands, pos.y) {
            Some(index) => Some(index).filter(|i| Some(*i) != sea),
            None => bands.add(value_of(pos.y)),
        };
    }

    let painter = ui.painter_at(rect);
    for band in bands.bands() {
        let (top, bottom) = (y_of(band.max), y_of(band.min));
        let (high, low) = if preview.levels {
            (band.color, band.color)
        } else {
            (band.top, band.bottom)
        };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(egui::pos2(bar.left(), top), color32(high));
        mesh.colored_vertex(egui::pos2(bar.right(), top), color32(high));
        mesh.colored_vertex(egui::pos2(bar.right(), bottom), color32(low));
        mesh.colored_vertex(egui::pos2(bar.left(), bottom), color32(low));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
    let text = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    for (index, &meters) in bands.limits().iter().enumerate() {
        let y = y_of(meters);
        let selected = preview.selected == Some(index);
        let (width, color) = match (selected, Some(index) == sea) {
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
fn selected_limit(ui: &mut egui::Ui, bands: &mut Bands, preview: &mut Preview, unit: &str) {
    let span = bands.max() - bands.min();
    let count = bands.limits().len();
    let Some(index) = preview.selected.filter(|i| *i < count) else {
        preview.selected = None;
        ui.weak("Tap the bar to add a limit. Drag a limit to move it.");
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
            preview.selected = None;
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
        let rain = Bands::with_range(0.0, 4000.0).unwrap();
        assert_eq!(rain.sea(), None);
        for (value, fraction) in [(0.0, 0.0), (1000.0, 0.25), (2000.0, 0.5), (4000.0, 1.0)] {
            assert_eq!(bar_fraction(&rain, value), fraction);
            assert_eq!(bar_value(&rain, fraction), value);
        }
        assert_eq!(bar_fraction(&rain, -5.0), 0.0);
    }
}
