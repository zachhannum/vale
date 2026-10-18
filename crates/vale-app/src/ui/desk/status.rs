//! The status bar: the position readout, the status text, the scale, and
//! the zoom.

use eframe::egui::{self, Align, Layout, Stroke};

use super::dock::grouped;
use super::theme;
use crate::globe::WorldView;
use crate::ui::{AppState, Workspace, pad};
use crate::world;

/// The zoom of the canvas, as a fraction. 1 shows the whole world or map.
pub fn zoom(state: &mut AppState) -> Option<f64> {
    match state.workspace {
        Workspace::Globe => Some(match state.globe.world_view {
            WorldView::Globe => state.globe.view.zoom,
            WorldView::Flat => state.globe.flat.zoom,
        }),
        Workspace::Map => {
            let scale = state.composed.as_ref()?.view.scale;
            let fit = state
                .pipeline
                .fit_scale(&state.doc, state.canvas_size)
                .ok()?;
            Some(scale / fit)
        }
    }
}

/// The text of a zoom, for example "100%".
pub fn zoom_text(zoom: f64) -> String {
    format!("{:.0}%", zoom * 100.0)
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let frame = egui::Frame::NONE
        .fill(theme::glass(state.translucent))
        .inner_margin(egui::Margin::symmetric(12, 0));
    egui::Panel::bottom("desk-status")
        .exact_size(theme::STATUS)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.clip_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter().hline(rect.x_range(), rect.top() + 0.5, stroke);
            ui.spacing_mut().item_spacing.x = 18.0;
            let font = egui::FontId::monospace(11.0);
            let text = |text: String| {
                egui::RichText::new(text)
                    .font(font.clone())
                    .color(theme::MUTE)
            };
            let value = match state.workspace {
                Workspace::Globe => state.readout_value(),
                Workspace::Map => None,
            };
            let readout = pad::readout_text(state.cursor_lonlat, value.as_deref());
            let progress = state.globe.import_progress();
            let progress = progress.or_else(|| state.globe.export_progress());
            let message = progress.unwrap_or_else(|| state.status.clone());
            let scale = match state.workspace {
                Workspace::Globe => state
                    .shown_raster()
                    .map(|r| format!("Face {}", grouped(r.face_size as usize))),
                Workspace::Map => state.composed.as_ref().map(|c| {
                    let units = state.doc.project.world.units;
                    format!("1 pt = {}", world::distance(c.km_per_point, units))
                }),
            };
            let zoom = zoom(state).map(zoom_text);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(text(readout));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(zoom) = zoom {
                        ui.label(text(zoom));
                    }
                    if let Some(scale) = scale {
                        ui.label(text(scale));
                    }
                    if !message.is_empty() {
                        let label = egui::Label::new(text(message)).truncate();
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(label);
                        });
                    }
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_is_a_percent() {
        assert_eq!(zoom_text(1.0), "100%");
        assert_eq!(zoom_text(0.474), "47%");
        assert_eq!(zoom_text(2.5), "250%");
    }
}
