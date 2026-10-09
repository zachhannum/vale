//! The elevation panel: the band limits and the color ramp of the heightmap.

use eframe::egui;
use vale_terrain::{Bands, ELEV_MAX, ELEV_MIN, Rgb};

use super::AppState;
use crate::globe::preview::Preview;

/// The name of the bar for a screen reader and for the tests.
pub const BAR_LABEL: &str = "Band limits";

const BAR_HEIGHT: f32 = 340.0;
const BAR_WIDTH: f32 = 56.0;
/// The space above and below the bar, so that a label at an end has room.
const BAR_PAD: f32 = 10.0;
/// A touch this near to a limit takes the limit, in points.
const GRAB: f32 = 14.0;
/// A limit that you add or drag lands on a multiple of this, in meters.
const SNAP: f64 = 10.0;

/// The place of an elevation on the bar, from 0 at the lowest to 1 at the
/// highest. Each side of sea level has a square root scale, so the limits
/// near sea level have room.
pub fn bar_fraction(meters: f64) -> f64 {
    let side = if meters < 0.0 {
        -(meters / ELEV_MIN).sqrt()
    } else {
        (meters / ELEV_MAX).sqrt()
    };
    0.5 + 0.5 * side.clamp(-1.0, 1.0)
}

/// The elevation at a place on the bar. The inverse of `bar_fraction`.
pub fn bar_meters(fraction: f64) -> f64 {
    let side = fraction.clamp(0.0, 1.0) * 2.0 - 1.0;
    let range = if side < 0.0 { ELEV_MIN } else { ELEV_MAX };
    side * side * range
}

/// The bar inside the rectangle of the widget.
fn bar_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(0.0, BAR_PAD),
        egui::pos2(rect.min.x + BAR_WIDTH, rect.max.y - BAR_PAD),
    )
}

/// The screen height of an elevation on the bar of a widget.
pub fn limit_y(rect: egui::Rect, meters: f64) -> f32 {
    let bar = bar_rect(rect);
    bar.bottom() - bar_fraction(meters) as f32 * bar.height()
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
            let globe = &mut state.globe;
            let (bands, preview) = (&mut globe.map.bands, &mut globe.preview);
            ui.heading("Elevation");
            ui.checkbox(&mut preview.levels, "Levels");
            ui.checkbox(&mut preview.graticule, "Graticule");
            ui.separator();
            limit_bar(ui, bands, preview);
            selected_limit(ui, bands, preview);
            ui.separator();
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
        });
}

/// The bar with the bands and the limits. A tap on a free place adds a limit.
/// A tap on a limit selects it, and a drag moves it.
fn limit_bar(ui: &mut egui::Ui, bands: &mut Bands, preview: &mut Preview) {
    let size = egui::vec2(ui.available_width(), BAR_HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, BAR_LABEL));
    let bar = bar_rect(rect);
    let y_of = |meters: f64| limit_y(rect, meters);
    let meters_of = |y: f32| {
        let meters = bar_meters(f64::from((bar.bottom() - y) / bar.height()));
        (meters / SNAP).round() * SNAP
    };

    let sea = bands.sea_index();
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
        preview.selected = near(bands, pos.y).filter(|i| *i != sea);
    }
    if resp.dragged()
        && let (Some(index), Some(pos)) = (preview.selected, resp.interact_pointer_pos())
        && index < bands.limits().len()
    {
        bands.move_to(index, meters_of(pos.y));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        preview.selected = match near(bands, pos.y) {
            Some(index) => Some(index).filter(|i| *i != sea),
            None => bands.add(meters_of(pos.y)),
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
        let (width, color) = match (selected, index == sea) {
            (true, _) => (3.0, strong),
            (false, true) => (2.0, text),
            (false, false) => (1.0, text),
        };
        let ends = [egui::pos2(bar.left(), y), egui::pos2(bar.right() + 8.0, y)];
        painter.line_segment(ends, egui::Stroke::new(width, color));
        let label = if index == sea {
            "Sea level".to_string()
        } else {
            format!("{meters:.0} m")
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
fn selected_limit(ui: &mut egui::Ui, bands: &mut Bands, preview: &mut Preview) {
    let count = bands.limits().len();
    let Some(index) = preview.selected.filter(|i| *i < count) else {
        preview.selected = None;
        ui.weak("Tap the bar to add a limit. Drag a limit to move it.");
        return;
    };
    ui.horizontal(|ui| {
        let mut meters = bands.limits()[index];
        let value = egui::DragValue::new(&mut meters).speed(SNAP).suffix(" m");
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
        assert_eq!(bar_fraction(ELEV_MIN), 0.0);
        assert_eq!(bar_fraction(0.0), 0.5);
        assert_eq!(bar_fraction(ELEV_MAX), 1.0);
        for meters in [-5000.0, -200.0, 0.0, 10.0, 1500.0, 6000.0] {
            assert!((bar_meters(bar_fraction(meters)) - meters).abs() < 1e-6);
        }
    }
}
