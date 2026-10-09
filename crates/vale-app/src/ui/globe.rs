//! The globe canvas.

use eframe::egui;
use eframe::egui_wgpu;

use super::AppState;
use crate::globe::math::dir_to_lonlat;

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(22, 25, 31);

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| canvas(ui, state));
}

fn canvas(ui: &mut egui::Ui, state: &mut AppState) {
    let globe = &mut state.globe;
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    globe.rect = rect;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BACKGROUND);
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return;
    }
    globe.nav.update(ui, rect, &resp, &mut globe.view);
    state.cursor_lonlat = resp
        .hover_pos()
        .and_then(|pos| globe.view.unproject(rect, pos))
        .map(|dir| dir_to_lonlat(dir).into());

    match globe.callback(ui.ctx().pixels_per_point()) {
        Some(callback) => {
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        }
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "The globe needs the wgpu renderer.",
                egui::FontId::default(),
                egui::Color32::GRAY,
            );
        }
    }
    if globe.nav.active() {
        ui.ctx().request_repaint();
    }
}
