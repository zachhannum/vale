//! The map canvas.

use std::time::Duration;

use eframe::egui;
use kurbo::Point;

use super::AppState;
use crate::pipeline::Quality;

const SETTLE_SECONDS: f64 = 0.25;

fn local(pos: egui::Pos2, rect: egui::Rect) -> Point {
    Point::new(f64::from(pos.x - rect.min.x), f64::from(pos.y - rect.min.y))
}

fn mark_view_change(state: &mut AppState, now: f64) {
    state.touch();
    state.last_view_change = Some(now);
}

/// Zooms about a page position. Used by the scroll wheel and by pinch.
pub fn zoom_at(state: &mut AppState, q: Point, factor: f64, now: f64) {
    let size = state.canvas_size;
    let Ok(fit) = state.pipeline.fit_scale(&state.doc, size) else {
        return;
    };
    let Ok(view) = state.pipeline.resolve_view(&state.doc, size) else {
        return;
    };
    state.doc.frame.view = Some(view.zoomed_at(q, factor, size.0, size.1, 0.5 * fit, 400.0 * fit));
    mark_view_change(state, now);
}

/// A click: selects the feature under `q`, or clears the selection.
pub fn click_at(state: &mut AppState, q: Point) {
    let size = state.canvas_size;
    state.selection = state.pipeline.pick(&state.doc, size, q).ok().flatten();
    if let Some(sel) = state.selection {
        state.selected_layer = Some(sel.layer);
    }
    state.touch();
}

/// A double click: centers the projection on the place under `q`.
pub fn center_on(state: &mut AppState, q: Point) {
    let size = state.canvas_size;
    let Ok(Some([lon, lat])) = state.pipeline.page_to_lonlat(&state.doc, size, q) else {
        return;
    };
    let mut spec = state.doc.frame.projection;
    spec.lon0 = lon;
    if spec.kind.is_azimuthal() {
        spec.lat0 = lat;
    }
    state.doc.set_projection(spec);
    state.status = format!("Centered the projection on {lon:.2}, {lat:.2}");
    state.touch();
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| canvas(ui, state));
}

fn canvas(ui: &mut egui::Ui, state: &mut AppState) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let size = (f64::from(rect.width()), f64::from(rect.height()));
    state.canvas_origin = (rect.min.x, rect.min.y);
    if size != state.canvas_size {
        state.canvas_size = size;
        state.touch();
    }
    if size.0 < 8.0 || size.1 < 8.0 {
        return;
    }
    let now = ui.input(|i| i.time);
    let mut view_changed = false;

    if resp.dragged() {
        let d = resp.drag_delta();
        if d != egui::Vec2::ZERO
            && let Ok(view) = state.pipeline.resolve_view(&state.doc, size)
        {
            state.doc.frame.view = Some(view.panned(f64::from(d.x), f64::from(d.y)));
            mark_view_change(state, now);
            view_changed = true;
        }
    }
    state.cursor_lonlat = None;
    if resp.hovered()
        && let Some(pos) = resp.hover_pos()
    {
        let q = local(pos, rect);
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
        if (factor - 1.0).abs() > 1e-4 {
            zoom_at(state, q, factor, now);
            view_changed = true;
        }
        state.cursor_lonlat = state
            .pipeline
            .page_to_lonlat(&state.doc, size, q)
            .ok()
            .flatten();
    }
    if resp.double_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        center_on(state, local(pos, rect));
    } else if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        click_at(state, local(pos, rect));
    }

    // Quality and timing.
    let mut quality = Quality::Final;
    if !state.headless {
        if view_changed {
            quality = Quality::Interactive;
        } else if let Some(t) = state.last_view_change {
            if now - t > SETTLE_SECONDS {
                if state.quality == Quality::Interactive {
                    state.touch();
                }
                state.last_view_change = None;
            } else {
                quality = state.quality;
            }
        }
        if state.last_view_change.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }

    let ppp = ui.ctx().pixels_per_point();
    let last_ppp = ui.data_mut(|d| d.get_temp::<f32>(egui::Id::new("map_ppp")));
    if last_ppp != Some(ppp) {
        ui.data_mut(|d| d.insert_temp(egui::Id::new("map_ppp"), ppp));
        state.touch();
    }

    if state.dirty || state.composed.is_none() {
        match state
            .pipeline
            .compose(&state.doc, size, &mut state.fonts, quality, state.selection)
        {
            Ok(composed) => {
                match vale_render::render_rgba(&composed.list, f64::from(ppp)) {
                    Ok(img) => {
                        let image = egui::ColorImage::from_rgba_premultiplied(
                            [img.width, img.height],
                            &img.data,
                        );
                        match &mut state.texture {
                            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                            None => {
                                state.texture = Some(ui.ctx().load_texture(
                                    "map",
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                        }
                    }
                    Err(e) => state.status = format!("{e}"),
                }
                state.composed = Some(composed);
                state.quality = quality;
            }
            Err(e) => state.status = format!("{e:#}"),
        }
        state.dirty = false;
    }

    if let Some(tex) = &state.texture {
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}
