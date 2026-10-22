//! The globe canvas.

use std::time::Instant;

use eframe::egui;
use eframe::egui_wgpu;
use vale_store::LayerId;

use super::AppState;
use vale_store::Geometry;
use vale_terrain::Mode;

use crate::document::Document;
use crate::globe::backdrop::Canvas;
use crate::globe::brush::{FIXED_FLOW, Sample, pen_flow, slerp};
use crate::globe::math::{angle, dir_to_lonlat, lonlat_to_dir};
use crate::globe::nav::{Painting, Tap, on_canvas};
use crate::globe::snap::Highlight;
use crate::globe::vector;
use crate::globe::view::GlobeView;
use crate::globe::{Globe, Paint, WorldView};
use crate::pen::{PenEvent, PenPhase, QUEUE_PEN};
use crate::ui::pad::theme;

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(22, 25, 31);
/// The largest angle between two drawn points of a highlight on the flat map.
const FLAT_STEP: f64 = std::f64::consts::PI / 180.0;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| canvas(ui, state, BACKGROUND, false));
}

/// Starts a stroke, or starts to pick the flatten level.
fn press(globe: &mut Globe) {
    if globe.lining() {
        globe.line_down();
    } else if globe.pick_level && globe.brush.mode == Mode::Flatten {
        globe.input.picking = true;
    } else {
        globe.pen_down();
    }
}

fn release(globe: &mut Globe) {
    if globe.line_preview().is_some() {
        globe.line_up();
    } else if !std::mem::take(&mut globe.input.picking) {
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

/// Reads the pen and the mouse for the brush and the line tool. In the
/// brush tool, a pen and the primary mouse button paint a raster layer that
/// the brush can paint. In the line tool, they draw a line in the selected
/// line layer. A touch with a force is a pen. With a pen queue, the pen
/// comes from the queue and not from egui.
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
    let brushing = globe.brushing() && globe.format.is_some();
    if !(brushing || globe.lining()) || globe.importing() {
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
        if globe.lining() {
            globe.line_sample(pos, dir);
        } else if globe.input.picking {
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

/// Reads the undo and redo commands. A tap of two fingers and Command+Z
/// undo. A tap of three fingers, Shift+Command+Z, and Command+Y redo. On
/// Windows and Linux, Ctrl is the Command key.
/// Returns `Some(true)` for a redo and `Some(false)` for an undo.
fn history_input(ui: &egui::Ui, globe: &mut Globe) -> Option<bool> {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let tap = globe.nav.take_tap();
    let typing = ui.ctx().egui_wants_keyboard_input();
    let key = |modifiers, key| {
        let shortcut = KeyboardShortcut::new(modifiers, key);
        !typing && ui.input_mut(|i| i.consume_shortcut(&shortcut))
    };
    // A shortcut without Shift also matches the keys with Shift, so the
    // shortcut with Shift comes first.
    let redo =
        key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || key(Modifiers::COMMAND, Key::Y);
    let undo = key(Modifiers::COMMAND, Key::Z);
    if redo || tap == Some(Tap::Three) {
        Some(true)
    } else if undo || tap == Some(Tap::Two) {
        Some(false)
    } else {
        None
    }
}

fn color(c: vale_style::Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Draws one vector layer over the globe.
/// Draws the target that a stroke snaps to or traces.
fn snap_highlight(
    painter: &egui::Painter,
    rect: egui::Rect,
    globe: &Globe,
    highlight: &Highlight,
    width: f32,
) {
    let [r, g, b, _] = theme::ACCENT.to_array();
    let color = egui::Color32::from_rgba_unmultiplied(r, g, b, 160);
    let stroke = egui::Stroke::new(width, color);
    let point = match globe.world_view {
        WorldView::Globe => {
            let view = &globe.view;
            let radius = view.radius(rect);
            let step = vector::step(radius * f64::from(painter.ctx().pixels_per_point()));
            let center = rect.center();
            for part in vector::clip_line(&view.rot, &highlight.span, step) {
                let to_screen = |p: &vector::P2| {
                    egui::pos2(
                        center.x + (p[0] * radius) as f32,
                        center.y - (p[1] * radius) as f32,
                    )
                };
                painter.line(part.iter().map(to_screen).collect(), stroke);
            }
            highlight.point.and_then(|p| view.project(rect, p))
        }
        WorldView::Flat => {
            let flat = &globe.flat;
            let mut part: Vec<egui::Pos2> = Vec::new();
            let mut parts = Vec::new();
            for pair in highlight.span.windows(2) {
                let n = (angle(pair[0], pair[1]) / FLAT_STEP).ceil().max(1.0) as usize;
                let start = usize::from(!part.is_empty());
                for i in start..=n {
                    let d = slerp(pair[0], pair[1], i as f64 / n as f64);
                    match flat.project(rect, d) {
                        Some(pos) => {
                            let jump = part.last().is_some_and(|last: &egui::Pos2| {
                                (pos.x - last.x).abs() > flat.map_width(rect) / 2.0
                            });
                            if jump {
                                parts.push(std::mem::take(&mut part));
                            }
                            part.push(pos);
                        }
                        None => parts.push(std::mem::take(&mut part)),
                    }
                }
            }
            parts.push(part);
            for part in parts.into_iter().filter(|p| p.len() > 1) {
                painter.line(part, stroke);
            }
            highlight.point.and_then(|p| flat.project(rect, p))
        }
    };
    if let Some(pos) = point {
        painter.circle_stroke(pos, 6.0, stroke);
    }
}

fn vector_layer(
    painter: &egui::Painter,
    rect: egui::Rect,
    doc: &Document,
    view: &GlobeView,
    id: LayerId,
) {
    let radius = view.radius(rect);
    let step = vector::step(radius * f64::from(painter.ctx().pixels_per_point()));
    let center = rect.center();
    let to_screen = |p: &vector::P2| {
        egui::pos2(
            center.x + (p[0] * radius) as f32,
            center.y - (p[1] * radius) as f32,
        )
    };
    let path = |part: &Vec<vector::P2>| part.iter().map(to_screen).collect::<Vec<_>>();
    let entries = doc.frame.entries.iter().filter(|e| e.layer == id);
    for entry in entries {
        let Some(layer) = doc.layer(entry.layer).filter(|l| l.visible) else {
            continue;
        };
        let fill = entry.style.fill.map(color);
        let stroke = entry
            .style
            .stroke
            .map(|s| egui::Stroke::new(s.width as f32, color(s.color)));
        for feature in &layer.features {
            match &feature.geometry {
                Geometry::Points(points) => {
                    for p in points {
                        if let Some(pos) = view.project(rect, lonlat_to_dir(p[0], p[1])) {
                            painter.circle(
                                pos,
                                entry.style.point_radius as f32,
                                fill.unwrap_or(egui::Color32::TRANSPARENT),
                                stroke.unwrap_or(egui::Stroke::NONE),
                            );
                        }
                    }
                }
                Geometry::Lines(lines) => {
                    let Some(stroke) = stroke else { continue };
                    for line in lines {
                        for part in vector::clip_line(&view.rot, &vector::line(line), step) {
                            painter.line(path(&part), stroke);
                        }
                    }
                }
                Geometry::Polygons(polygons) => {
                    for polygon in polygons {
                        let rings: Vec<_> = polygon
                            .iter()
                            .enumerate()
                            .map(|(i, ring)| vector::ring(ring, i > 0))
                            .collect();
                        if rings.first().is_none_or(|outer| outer.is_empty()) {
                            continue;
                        }
                        let rings: Vec<_> = rings.into_iter().filter(|r| !r.is_empty()).collect();
                        let clipped = vector::clip_polygon(&view.rot, &rings, step);
                        if let Some(fill) = fill {
                            let (points, indices) = vector::triangulate(&clipped.loops);
                            let vertices = points
                                .iter()
                                .map(|p| egui::epaint::Vertex {
                                    pos: to_screen(p),
                                    uv: egui::epaint::WHITE_UV,
                                    color: fill,
                                })
                                .collect();
                            let mesh = egui::Mesh {
                                indices,
                                vertices,
                                ..Default::default()
                            };
                            painter.add(mesh);
                        }
                        if let Some(stroke) = stroke {
                            for part in &clipped.outlines {
                                painter.line(path(part), stroke);
                            }
                        }
                    }
                }
            }
        }
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
    if let Some(rect) = canvas_input(ui, state, background) {
        paint_canvas(ui, state, rect, background, cards);
    }
}

/// Fills the free space of `ui` with the background and reads the input of
/// the globe. Returns the rectangle of the canvas, or `None` if it is too
/// small to draw.
pub(super) fn canvas_input(
    ui: &mut egui::Ui,
    state: &mut AppState,
    background: egui::Color32,
) -> Option<egui::Rect> {
    let now = Instant::now();
    if state.globe.lining() && state.globe.line_preview().is_none() {
        state.refresh_targets();
    }
    let globe = &mut state.globe;
    globe.world_km = state.doc.project.world.radius_km;
    globe.world_seed = state.doc.project.world.seed;
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    globe.rect = rect;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, background);
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return None;
    }
    let painting = brush_input(ui, rect, &resp, globe, now);
    match globe.world_view {
        WorldView::Globe => globe.nav.update(ui, rect, &resp, &mut globe.view, painting),
        WorldView::Flat => {
            globe.flat.clamp();
            globe.nav.update(ui, rect, &resp, &mut globe.flat, painting);
        }
    }
    state.poll_line();
    match history_input(ui, &mut state.globe) {
        Some(true) => state.redo(),
        Some(false) => state.undo(),
        None => {}
    }
    let globe = &mut state.globe;
    let cursor = resp
        .hover_pos()
        .or(globe.input.pos)
        .and_then(|pos| globe.unproject(pos));
    state.cursor_lonlat = cursor.map(|dir| dir_to_lonlat(dir).into());
    // The value is that of the raster layer of the readout.
    let shown = state.shown_raster().map(|raster| raster.id);
    let globe = &mut state.globe;
    let map = shown.and_then(|id| globe.display_map(id));
    let value = |(dir, map): (_, &vale_terrain::Heightmap)| map.bands.value(map.sample(dir));
    state.cursor_value = cursor.zip(map).map(value);
    Some(rect)
}

/// Paints the globe into `rect`. A change of the state before this call
/// shows in the same frame.
pub(super) fn paint_canvas(
    ui: &egui::Ui,
    state: &mut AppState,
    rect: egui::Rect,
    background: egui::Color32,
    cards: bool,
) {
    let painter = egui::Painter::new(ui.ctx().clone(), ui.layer_id(), rect);
    let layer = state.selected_vector();
    let globe = &mut state.globe;
    let more = globe.advance(Instant::now());
    let pixels_per_point = ui.ctx().pixels_per_point();
    // The passes of the raster layers and the shapes of the vector layers
    // go to the painter in the draw order. The flat view draws no vector
    // layer.
    let globe_view = globe.world_view == WorldView::Globe;
    let canvas = Canvas {
        background,
        pixels_per_point,
    };
    match globe.paint(canvas, cards && globe.blur) {
        Some(paints) => {
            for paint in paints {
                match paint {
                    Paint::Callback(callback) => {
                        painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
                    }
                    Paint::Vector(id) if globe_view => {
                        vector_layer(&painter, rect, &state.doc, &globe.view, id);
                    }
                    Paint::Vector(_) => {}
                }
            }
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
    if let Some(pos) = globe.input.pos.filter(|_| globe.brushing()) {
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
                let (flat, center) = (&globe.flat, globe.flat.unproject(rect, pos));
                let lines = center.map(|dir| flat.outline(rect, dir, radius));
                for line in lines.unwrap_or_default() {
                    painter.line(line, stroke);
                }
            }
        }
    }
    if let Some(line) = globe.line_preview() {
        // The stroke of the layer, or a plain white one.
        let entries = state.doc.frame.entries.iter();
        let style = entries
            .filter(|e| Some(e.layer) == layer)
            .find_map(|e| e.style.stroke);
        let stroke = style.map_or(egui::Stroke::new(2.0, egui::Color32::WHITE), |s| {
            egui::Stroke::new((s.width as f32).max(1.0), color(s.color))
        });
        if let Some(highlight) = globe.snap_highlight() {
            snap_highlight(&painter, rect, globe, &highlight, stroke.width + 4.0);
        }
        painter.line(line.to_vec(), stroke);
    }
    if globe.nav.active() || more {
        ui.ctx().request_repaint();
    }
}
