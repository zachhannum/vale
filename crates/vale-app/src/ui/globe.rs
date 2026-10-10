//! The globe canvas.

use std::time::Instant;

use eframe::egui;
use eframe::egui_wgpu;

use super::AppState;
use vale_terrain::{Mode, level_to_meters};

use crate::globe::backdrop::Canvas;
use crate::globe::brush::{FIXED_FLOW, Sample, pen_flow};
use crate::globe::math::dir_to_lonlat;
use crate::globe::nav::{Painting, on_canvas};
use crate::globe::{Globe, Tool, WorldView};
use crate::pen::{PenEvent, PenPhase, QUEUE_PEN};

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(22, 25, 31);

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| canvas(ui, state, BACKGROUND, false));
}

/// Starts a stroke, or starts to pick the flatten level.
fn press(globe: &mut Globe) {
    if globe.pick_level && globe.brush.mode == Mode::Flatten {
        globe.input.picking = true;
    } else {
        globe.pen_down();
    }
}

fn release(globe: &mut Globe) {
    if !std::mem::take(&mut globe.input.picking) {
        globe.pen_up();
    }
}

/// Reads the hover events of the pen queue. A hover event can arrive after
/// the pen comes down, so the pen does not hover while it is down.
fn hover_input(globe: &mut Globe, events: &[(egui::Pos2, PenEvent)]) {
    let pen = &mut globe.pen;
    for (pos, event) in events {
        match event.phase {
            PenPhase::Hover if !pen.down => {
                pen.hover = Some(*pos);
                pen.stats.hover(event);
            }
            PenPhase::Down | PenPhase::Up | PenPhase::Cancel | PenPhase::HoverEnd => {
                pen.down = event.phase == PenPhase::Down;
                pen.hover = None;
            }
            _ => {}
        }
    }
}

/// Reads the pen and the mouse for the brush. In the brush tool, a pen and
/// the primary mouse button paint. A touch with a force is a pen. With a pen
/// queue, the pen comes from the queue and not from egui.
fn brush_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    resp: &egui::Response,
    globe: &mut Globe,
    now: Instant,
) -> Painting {
    // The queue gives view points. A zoom of the UI changes the egui points.
    let zoom = ui.ctx().zoom_factor();
    let queue = globe.pen.queue.as_ref().map(|queue| queue.take());
    let from_queue = queue.is_some();
    let queue: Vec<(egui::Pos2, PenEvent)> = queue
        .unwrap_or_default()
        .into_iter()
        .map(|e| (egui::pos2(e.pos[0] / zoom, e.pos[1] / zoom), e))
        .collect();
    hover_input(globe, &queue);
    if globe.tool != Tool::Brush || globe.format.is_none() {
        if globe.input.pen.take().is_some() || std::mem::take(&mut globe.input.mouse) {
            release(globe);
            globe.pen.stats.up();
        }
        globe.input.pos = None;
        return Painting::default();
    }
    let time = ui.input(|i| i.time);
    let radius = globe.brush.radius(globe.radius());
    let sample = |globe: &mut Globe, pos: egui::Pos2, flow: f64| {
        let dir = globe.unproject(pos);
        globe.input.pos = Some(pos);
        if globe.input.picking {
            globe.pick(dir);
        } else {
            globe.pen_sample(Sample { dir, flow, radius }, now);
        }
    };
    for (pos, event) in &queue {
        let flow = pen_flow(event.force);
        let tilt = event.altitude.zip(event.azimuth);
        let ours = globe.input.pen == Some(QUEUE_PEN);
        match event.phase {
            PenPhase::Down => {
                let free = globe.input.pen.is_none() && !globe.input.mouse;
                if free && on_canvas(ui, rect, *pos) {
                    globe.input.pen = Some(QUEUE_PEN);
                    press(globe);
                    sample(globe, *pos, flow);
                    globe.pen.stats.sample(event.time, Some(event.force), tilt);
                }
            }
            PenPhase::Move | PenPhase::Up if ours => {
                sample(globe, *pos, flow);
                globe.pen.stats.sample(event.time, Some(event.force), tilt);
            }
            _ => {}
        }
        if ours && matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
            globe.input.pen = None;
            globe.input.pos = None;
            release(globe);
            globe.pen.stats.up();
        }
    }

    let mut touched = false;
    let events = ui.input(|i| i.events.clone());
    for event in &events {
        let egui::Event::Touch {
            id,
            phase,
            pos,
            force,
            ..
        } = event
        else {
            continue;
        };
        touched = true;
        if from_queue && force.is_some() {
            continue;
        }
        let flow = force.map_or(FIXED_FLOW, pen_flow);
        match phase {
            egui::TouchPhase::Start => {
                let free = globe.input.pen.is_none() && !globe.input.mouse;
                if force.is_some() && free && on_canvas(ui, rect, *pos) {
                    globe.input.pen = Some(id.0);
                    press(globe);
                    sample(globe, *pos, flow);
                    globe.pen.stats.sample(time, *force, None);
                }
            }
            egui::TouchPhase::Move => {
                if globe.input.pen == Some(id.0) {
                    sample(globe, *pos, flow);
                    globe.pen.stats.sample(time, *force, None);
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if globe.input.pen == Some(id.0) {
                    globe.input.pen = None;
                    globe.input.pos = None;
                    release(globe);
                    globe.pen.stats.up();
                }
            }
        }
    }

    let (pos, pressed, down, shift, moved) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.pointer.delta() != egui::Vec2::ZERO,
        )
    });
    // The first touch is also the egui pointer.
    let touched = touched || globe.nav.touching() || globe.input.pen.is_some();
    match pos {
        Some(pos) if globe.input.mouse && down => {
            if moved {
                sample(globe, pos, FIXED_FLOW);
                globe.pen.stats.sample(time, None, None);
            }
        }
        _ if globe.input.mouse => {
            globe.input.mouse = false;
            release(globe);
            globe.pen.stats.up();
        }
        Some(pos) if pressed && !shift && !touched && resp.contains_pointer() => {
            globe.input.mouse = true;
            press(globe);
            sample(globe, pos, FIXED_FLOW);
            globe.pen.stats.sample(time, None, None);
        }
        _ => {}
    }
    if globe.input.pen.is_none() {
        let hover = globe.pen.hover.filter(|pos| on_canvas(ui, rect, *pos));
        globe.input.pos = resp.hover_pos().or(hover);
    }
    Painting {
        pen: true,
        mouse: globe.input.mouse,
    }
}

/// Draws the globe in the free space of `ui` and reads its input. `cards`:
/// floating cards cover the canvas, and they can show a blurred copy of it.
pub(super) fn canvas(
    ui: &mut egui::Ui,
    state: &mut AppState,
    background: egui::Color32,
    cards: bool,
) {
    let now = Instant::now();
    let globe = &mut state.globe;
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    globe.rect = rect;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, background);
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return;
    }
    let painting = brush_input(ui, rect, &resp, globe, now);
    match globe.world_view {
        WorldView::Globe => globe.nav.update(ui, rect, &resp, &mut globe.view, painting),
        WorldView::Flat => {
            globe.flat.clamp(rect);
            globe.nav.update(ui, rect, &resp, &mut globe.flat, painting);
        }
    }
    let cursor = resp
        .hover_pos()
        .or(globe.input.pos)
        .and_then(|pos| globe.unproject(pos));
    state.cursor_lonlat = cursor.map(|dir| dir_to_lonlat(dir).into());
    state.cursor_meters = cursor.map(|dir| level_to_meters(globe.map.sample(dir)));

    let more = globe.advance(now);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let backdrop = (cards && globe.blur).then_some(Canvas {
        background,
        pixels_per_point,
    });
    match globe.callback(pixels_per_point, backdrop) {
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
    if let Some(pos) = globe.input.pos.filter(|_| globe.tool == Tool::Brush) {
        let globe_radius = globe.radius();
        let radius = globe.brush.radius(globe_radius);
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(170));
        match globe.world_view {
            WorldView::Globe => {
                painter.circle_stroke(pos, (radius * globe_radius) as f32, stroke);
            }
            // The stamp is round on the sphere, so its outline is not round
            // on the flat map.
            WorldView::Flat => {
                let center = globe.flat.unproject(rect, pos);
                let lines = center.map(|dir| globe.flat.outline(rect, dir, radius));
                for line in lines.unwrap_or_default() {
                    painter.line(line, stroke);
                }
            }
        }
    }
    if globe.nav.active() || more {
        ui.ctx().request_repaint();
    }
}
