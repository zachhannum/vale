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
use crate::globe::math::V3;
use crate::globe::math::{angle, dir_to_lonlat, lonlat_to_dir};
use crate::globe::nav::{Nav, Painting, Tap, on_canvas};
use crate::globe::select::Pointer;
use crate::globe::snap::Highlight;
use crate::globe::symbol::{GlobeScatter, SymbolMesh, append_missing, view_places};
use crate::globe::vector;
use crate::globe::view::GlobeView;
use crate::globe::{Globe, Paint, WorldView};
use crate::measure::{DENSIFY_DEG, MOUSE, Measure, TapPress};
use crate::pen::{PenEvent, PenPhase, QUEUE_PEN};
use crate::scatter::{ScatterCache, ScatterFill};
use crate::ui::pad::theme;
use vale_sphere::LonLat;
use vale_sphere::measure::densify_great_circle;
use vale_style::expr::Context;
use vale_style::{Paint as StylePaint, StrokeKind, scale_denominator};

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
    if globe.stroking() {
        globe.line_down();
    } else if globe.placing() {
        globe.point_down();
    } else if globe.pick_level && globe.brush.mode == Mode::Flatten {
        globe.input.picking = true;
    } else {
        globe.pen_down();
    }
}

fn release(globe: &mut Globe) {
    if globe.line_preview().is_some() {
        globe.line_up();
    } else if globe.point_pressed() {
        globe.point_up();
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
    if !(brushing || globe.editing() || globe.reshaping()) || globe.importing() {
        if globe.input.pen.take().is_some() || std::mem::take(&mut globe.input.mouse) {
            release(globe);
            globe.pen.stats.up();
        }
        globe.input.pos = None;
        if globe.measuring() && !globe.importing() {
            measure_input(ui, rect, resp, globe, &queue, from_queue);
        }
        if globe.selecting() && !globe.importing() {
            return select_input(ui, rect, resp, globe, &queue, from_queue);
        }
        return Painting::default();
    }
    let time = ui.input(|i| i.time);
    let radius = globe.brush.radius(globe.radius());
    let sample = |globe: &mut Globe, pos: egui::Pos2, flow: f64| {
        let dir = globe.unproject(pos);
        globe.input.pos = Some(pos);
        if globe.stroking() {
            globe.line_sample(pos, dir);
        } else if globe.placing() {
            globe.point_sample(dir);
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

/// Reads the taps of the Measure tool. A tap of a pen, a click, and a tap
/// of one finger add a point, and a drag moves the view.
fn measure_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    resp: &egui::Response,
    globe: &mut Globe,
    queue: &[(egui::Pos2, PenEvent)],
    from_queue: bool,
) {
    let mut tap = None;
    for (pos, event) in queue {
        let press = &mut globe.measure_press;
        match event.phase {
            PenPhase::Down if on_canvas(ui, rect, *pos) => press.down(QUEUE_PEN, *pos, true),
            PenPhase::Move => press.moved(QUEUE_PEN, *pos),
            PenPhase::Up => tap = press.up(QUEUE_PEN, *pos).or(tap),
            PenPhase::Cancel => press.cancel(QUEUE_PEN),
            _ => {}
        }
    }
    let touching = globe.nav.touching();
    let on = |pos| on_canvas(ui, rect, pos);
    let press = &mut globe.measure_press;
    tap = press_input(ui, resp, press, on, from_queue, touching).or(tap);
    if let Some(pos) = tap {
        globe.measure_tap(pos);
    }
    let down = globe.measure_press.is_down() || globe.nav.touching();
    let down = down || ui.input(|i| i.pointer.any_down());
    let hover = globe.pen.hover.filter(|pos| on_canvas(ui, rect, *pos));
    globe.input.pos = resp.hover_pos().or(hover).filter(|_| !down);
}

/// Reads the pen, the touches, and the mouse for the Select tool. A pen
/// and the mouse belong to the tool, and fingers still move the view.
fn select_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    resp: &egui::Response,
    globe: &mut Globe,
    queue: &[(egui::Pos2, PenEvent)],
    from_queue: bool,
) -> Painting {
    let on = |pos| on_canvas(ui, rect, pos);
    let press = &mut globe.select_press;
    for (pos, event) in queue {
        match event.phase {
            PenPhase::Down if on(*pos) => press.down(QUEUE_PEN, *pos, Pointer::Pen),
            PenPhase::Move => press.moved(QUEUE_PEN, *pos),
            PenPhase::Up => press.up(QUEUE_PEN, *pos),
            PenPhase::Cancel => press.cancel(QUEUE_PEN),
            _ => {}
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
        let pointer = if force.is_some() {
            Pointer::Pen
        } else {
            Pointer::Finger
        };
        match phase {
            egui::TouchPhase::Start if on(*pos) => press.down(id.0, *pos, pointer),
            egui::TouchPhase::Move => press.moved(id.0, *pos),
            egui::TouchPhase::End => press.up(id.0, *pos),
            egui::TouchPhase::Cancel => press.cancel(id.0),
            egui::TouchPhase::Start => {}
        }
    }
    // The first touch is also the egui pointer. A drag with Shift turns the
    // view.
    if !touched && !globe.nav.touching() {
        let (pos, pressed, released, shift) = ui.input(|i| {
            let p = &i.pointer;
            let (pressed, released) = (p.primary_pressed(), p.primary_released());
            (p.latest_pos(), pressed, released, i.modifiers.shift)
        });
        match pos {
            Some(pos) => {
                if pressed && !shift && resp.contains_pointer() {
                    press.down(MOUSE, pos, Pointer::Mouse);
                }
                press.moved(MOUSE, pos);
                if released {
                    press.up(MOUSE, pos);
                }
            }
            None if released => press.cancel(MOUSE),
            None => {}
        }
    }
    let hover = globe.pen.hover.filter(|pos| on(*pos));
    globe.input.pos = resp.hover_pos().or(hover);
    Painting {
        pen: true,
        mouse: globe.select_press.mouse(),
    }
}

/// Feeds the touches and the mouse of this frame to a press of the Measure
/// tool, and returns the position of a tap. A press starts where `on` is
/// true. With `skip_pen`, the pen queue has the touches of the pen.
/// `touching` is true while a finger or a pen is down on the canvas.
pub(super) fn press_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    press: &mut TapPress,
    on: impl Fn(egui::Pos2) -> bool,
    skip_pen: bool,
    touching: bool,
) -> Option<egui::Pos2> {
    let mut tap = None;
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
        if skip_pen && force.is_some() {
            continue;
        }
        match phase {
            egui::TouchPhase::Start if on(*pos) => press.down(id.0, *pos, force.is_some()),
            egui::TouchPhase::Move => press.moved(id.0, *pos),
            egui::TouchPhase::End => tap = press.up(id.0, *pos).or(tap),
            egui::TouchPhase::Cancel => press.cancel(id.0),
            egui::TouchPhase::Start => {}
        }
    }
    // The first touch is also the egui pointer.
    if !touched && !touching {
        let (pos, pressed, released, shift) = ui.input(|i| {
            let p = &i.pointer;
            let (pressed, released) = (p.primary_pressed(), p.primary_released());
            (p.latest_pos(), pressed, released, i.modifiers.shift)
        });
        match pos {
            Some(pos) => {
                if pressed && !shift && resp.contains_pointer() {
                    press.down(MOUSE, pos, false);
                }
                press.moved(MOUSE, pos);
                if released {
                    tap = press.up(MOUSE, pos).or(tap);
                }
            }
            None if released => press.cancel(MOUSE),
            None => {}
        }
    }
    tap
}

/// Reads the keys of the Measure tool in both workspaces. M chooses the
/// tool. While it is the tool, Return closes the path and Escape clears the
/// path.
pub(super) fn measure_keys(ui: &egui::Ui, state: &mut AppState) {
    use egui::{Key, Modifiers};
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let key = |key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if key(Key::M) && !state.measuring() {
        super::pad::toggle_measure(state);
    }
    if !state.measuring() {
        return;
    }
    if key(Key::Enter) {
        state.measure.close();
    }
    if key(Key::Escape) {
        state.measure.clear();
    }
}

/// Reads the undo and redo commands. A tap of two fingers and Command+Z
/// undo. A tap of three fingers, Shift+Command+Z, and Command+Y redo. On
/// Windows and Linux, Ctrl is the Command key.
/// Returns `Some(true)` for a redo and `Some(false)` for an undo.
pub(super) fn history_input(ui: &egui::Ui, nav: &mut Nav) -> Option<bool> {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let tap = nav.take_tap();
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

/// The parts on screen of a path of great-circle arcs. On the flat map, a
/// path breaks where it crosses the edge of the map.
pub fn screen_lines(
    globe: &Globe,
    rect: egui::Rect,
    pixels_per_point: f32,
    path: &[V3],
) -> Vec<Vec<egui::Pos2>> {
    match globe.world_view {
        WorldView::Globe => {
            let view = &globe.view;
            let radius = view.radius(rect);
            let step = vector::step(radius * f64::from(pixels_per_point));
            let center = rect.center();
            let to_screen = |p: &vector::P2| {
                egui::pos2(
                    center.x + (p[0] * radius) as f32,
                    center.y - (p[1] * radius) as f32,
                )
            };
            vector::clip_line(&view.rot, path, step)
                .iter()
                .map(|part| part.iter().map(to_screen).collect())
                .collect()
        }
        WorldView::Flat => {
            let flat = &globe.flat;
            let mut part: Vec<egui::Pos2> = Vec::new();
            let mut parts = Vec::new();
            for pair in path.windows(2) {
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
            parts.retain(|p| p.len() > 1);
            parts
        }
    }
}

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
    let ppp = painter.ctx().pixels_per_point();
    for part in screen_lines(globe, rect, ppp, &highlight.span) {
        painter.line(part, stroke);
    }
    if let Some(pos) = highlight.point.and_then(|p| globe.project(p)) {
        painter.circle_stroke(pos, 6.0, stroke);
    }
}

fn dirs(path: &[LonLat]) -> Vec<V3> {
    path.iter().map(|p| lonlat_to_dir(p[0], p[1])).collect()
}

/// The parts on screen of each segment of the path of the Measure tool.
pub fn measure_lines(
    globe: &Globe,
    rect: egui::Rect,
    pixels_per_point: f32,
    measure: &Measure,
) -> Vec<Vec<Vec<egui::Pos2>>> {
    let segment = |(a, b)| dirs(&densify_great_circle(&[a, b], DENSIFY_DEG, false));
    let lines = |(a, b)| screen_lines(globe, rect, pixels_per_point, &segment((a, b)));
    measure.segments().into_iter().map(lines).collect()
}

/// Draws the path of the Measure tool: the arcs, the points, the length of
/// each segment, and a dashed line from the last point to the cursor.
fn measure_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    globe: &Globe,
    measure: &Measure,
    world: &vale_store::World,
) {
    if measure.is_empty() {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let shadow = egui::Stroke::new(4.5, egui::Color32::from_black_alpha(110));
    let line = egui::Stroke::new(2.5, theme::ACCENT);
    let segments = measure_lines(globe, rect, ppp, measure);
    if let (false, Some(last), Some(pos)) = (measure.closed, measure.points.last(), globe.input.pos)
        && let Some(cursor) = globe.unproject(pos)
    {
        let path = densify_great_circle(&[last.pos, lonlat(cursor)], DENSIFY_DEG, false);
        let dash = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(200));
        for part in screen_lines(globe, rect, ppp, &dirs(&path)) {
            painter.extend(egui::Shape::dashed_line(&part, dash, 6.0, 4.0));
        }
    }
    for part in segments.iter().flatten() {
        painter.line(part.clone(), shadow);
    }
    for part in segments.iter().flatten() {
        painter.line(part.clone(), line);
    }
    let ends = measure.segments();
    let lengths = measure.segment_labels(world.radius_km, world.units);
    for ((parts, (a, b)), text) in segments.iter().zip(ends).zip(lengths) {
        let on_screen: f32 = parts
            .iter()
            .flat_map(|p| p.windows(2))
            .map(|w| w[0].distance(w[1]))
            .sum();
        let (a, b) = (lonlat_to_dir(a[0], a[1]), lonlat_to_dir(b[0], b[1]));
        let Some(mid) = globe.project(slerp(a, b, 0.5)) else {
            continue;
        };
        let font = egui::FontId::proportional(12.0);
        let galley = painter.layout_no_wrap(text, font, theme::TEXT);
        let pill = egui::Rect::from_center_size(mid, galley.size() + egui::vec2(12.0, 4.0));
        if on_screen < pill.width() + 8.0 {
            continue;
        }
        let fill = egui::Color32::from_black_alpha(190);
        painter.rect_filled(pill, pill.height() / 2.0, fill);
        painter.galley(pill.center() - galley.size() / 2.0, galley, theme::TEXT);
    }
    let dot = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160));
    for (i, point) in measure.points.iter().enumerate() {
        let Some(pos) = globe.project(lonlat_to_dir(point.pos[0], point.pos[1])) else {
            continue;
        };
        if i == 0 {
            painter.circle_stroke(pos, 8.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
        }
        painter.circle(pos, 4.0, egui::Color32::WHITE, dot);
    }
}

fn lonlat(dir: V3) -> LonLat {
    let (lon, lat) = dir_to_lonlat(dir);
    [lon, lat]
}

/// Draws one vector layer over the globe. The symbols go to one mesh, which
/// goes to the painter before the next shape that is not a symbol.
fn vector_layer(
    painter: &egui::Painter,
    rect: egui::Rect,
    doc: &Document,
    view: &GlobeView,
    id: LayerId,
    scatter: &mut ScatterCache,
) {
    let radius = view.radius(rect);
    let radius_m = doc.project.world.radius_km * 1000.0;
    let scale = scale_denominator(radius_m / radius);
    let step = vector::step(radius * f64::from(painter.ctx().pixels_per_point()));
    let center = rect.center();
    let to_screen = |p: &vector::P2| {
        egui::pos2(
            center.x + (p[0] * radius) as f32,
            center.y - (p[1] * radius) as f32,
        )
    };
    let point = |p: &vector::P2| {
        let q = to_screen(p);
        kurbo::Point::new(f64::from(q.x), f64::from(q.y))
    };
    let symbols = doc.symbols();
    let mut places = None;
    let mut pending = egui::Mesh::default();
    let flush = |pending: &mut egui::Mesh| {
        if !pending.is_empty() {
            painter.add(std::mem::take(pending));
        }
    };
    let entries = doc.frame.entries.iter().filter(|e| e.layer == id);
    for entry in entries {
        let Some(layer) = doc.layer(entry.layer).filter(|l| l.visible) else {
            continue;
        };
        let style = &entry.style.style;
        let constant = style.constant_paints(scale);
        let mut paints = Vec::new();
        let mut count = 0;
        for (index, feature) in layer.features.iter().enumerate() {
            let paints = match &constant {
                Some(c) => c.as_slice(),
                None => {
                    paints.clear();
                    let ctx = Context {
                        attributes: &feature.attributes,
                        scale,
                        geometry: feature.geometry.kind(),
                    };
                    style.paints(&ctx, &mut paints);
                    paints.as_slice()
                }
            };
            if paints.is_empty() {
                continue;
            }
            let strokes = paints.iter().filter_map(|p| match p {
                StylePaint::Stroke(s) => Some(*s),
                _ => None,
            });
            let seed = index as u64;
            match &feature.geometry {
                Geometry::Points(points) => {
                    for p in points {
                        let Some(pos) = view.project(rect, lonlat_to_dir(p[0], p[1])) else {
                            continue;
                        };
                        for paint in paints {
                            match paint {
                                StylePaint::Marker {
                                    radius,
                                    fill,
                                    stroke,
                                } => {
                                    flush(&mut pending);
                                    painter.circle(
                                        pos,
                                        *radius as f32,
                                        fill.map_or(egui::Color32::TRANSPARENT, color),
                                        stroke.map_or(egui::Stroke::NONE, |s| {
                                            egui::Stroke::new(s.width as f32, color(s.color))
                                        }),
                                    );
                                }
                                StylePaint::Symbol {
                                    symbol,
                                    size,
                                    rotation,
                                    tint,
                                } if *size > 0.0 => {
                                    let size = *size as f32;
                                    if rect.expand(size).contains(pos) {
                                        match symbols.get(symbol) {
                                            Some(s) => SymbolMesh::of(s).append(
                                                &mut pending,
                                                pos,
                                                size,
                                                *rotation as f32,
                                                *tint,
                                            ),
                                            None => append_missing(&mut pending, pos, size, *tint),
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Geometry::Lines(lines) => {
                    let parts: Vec<_> = lines
                        .iter()
                        .flat_map(|line| {
                            vector::clip_line_measured(&view.rot, &vector::line(line), step)
                        })
                        .map(|part| part.iter().map(|(p, f)| (point(p), *f)).collect())
                        .collect();
                    if !parts.is_empty() {
                        for stroke in strokes.clone() {
                            flush(&mut pending);
                            painter.extend(vector::stroke_shapes(&parts, &stroke, seed));
                        }
                    }
                }
                Geometry::Polygons(polygons) => {
                    let clipped: Vec<_> = polygons
                        .iter()
                        .filter_map(|polygon| {
                            let rings: Vec<_> = polygon
                                .iter()
                                .enumerate()
                                .map(|(i, ring)| vector::ring(ring, i > 0))
                                .collect();
                            if rings.first().is_none_or(|outer| outer.is_empty()) {
                                return None;
                            }
                            let rings: Vec<_> =
                                rings.into_iter().filter(|r| !r.is_empty()).collect();
                            Some(vector::clip_polygon(&view.rot, &rings, step))
                        })
                        .collect();
                    let mut triangles = vec![None; clipped.len()];
                    for paint in paints {
                        match paint {
                            StylePaint::Fill(c) => {
                                flush(&mut pending);
                                for (part, triangles) in clipped.iter().zip(&mut triangles) {
                                    let (points, indices) = triangles
                                        .get_or_insert_with(|| vector::triangulate(&part.loops));
                                    let vertices = points
                                        .iter()
                                        .map(|p| egui::epaint::Vertex {
                                            pos: to_screen(p),
                                            uv: egui::epaint::WHITE_UV,
                                            color: color(*c),
                                        })
                                        .collect();
                                    painter.add(egui::Mesh {
                                        indices: indices.clone(),
                                        vertices,
                                        ..Default::default()
                                    });
                                }
                            }
                            StylePaint::Stroke(stroke) => {
                                flush(&mut pending);
                                let mut stroke = *stroke;
                                // A polygon outline has no start and no end to taper.
                                if matches!(stroke.kind, StrokeKind::Taper { .. }) {
                                    stroke.kind = StrokeKind::Solid;
                                }
                                for part in &clipped {
                                    let outlines: Vec<_> = part
                                        .outlines
                                        .iter()
                                        .map(|o| o.iter().map(|p| (point(p), 0.0)).collect())
                                        .collect();
                                    painter.extend(vector::stroke_shapes(&outlines, &stroke, seed));
                                }
                            }
                            StylePaint::Scatter { symbol, tint, .. } => {
                                let Some(fill) = ScatterFill::of(paint) else {
                                    continue;
                                };
                                let places = places.get_or_insert_with(|| view_places(view, rect));
                                let mut on_globe = GlobeScatter {
                                    cache: scatter,
                                    view,
                                    rect,
                                    radius_m,
                                    places,
                                    count,
                                };
                                let instances =
                                    on_globe.instances(entry.layer, index, feature, &fill);
                                count = on_globe.count;
                                let mesh = symbols.get(symbol).map(SymbolMesh::of);
                                for i in instances {
                                    match &mesh {
                                        Some(m) => {
                                            m.append(&mut pending, i.pos, i.size, i.rotation, *tint)
                                        }
                                        None => append_missing(&mut pending, i.pos, i.size, *tint),
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    flush(&mut pending);
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
    if state.globe.snapping() && state.globe.line_preview().is_none() {
        state.refresh_targets();
    }
    let globe = &mut state.globe;
    globe.world_km = state.doc.project.world.radius_km;
    globe.world_seed = state.doc.project.world.seed;
    let world = &state.doc.project.world;
    globe.world_rain = vale_terrain::Rain {
        wind: world.wind_deg,
        shadow: world.rain_shadow.clamp(0.0, 1.0),
    };
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
    state.poll_point();
    state.poll_measure();
    state.poll_select();
    measure_keys(ui, state);
    super::select::keys(ui, state);
    match history_input(ui, &mut state.globe.nav) {
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
    let placed = state.point_form.as_ref().and_then(|form| {
        let layer = state.doc.project.layer(form.layer)?;
        let feature = layer.features.iter().find(|f| f.uuid() == form.feature)?;
        match &feature.geometry {
            Geometry::Points(points) => points.first().copied(),
            _ => None,
        }
    });
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
    let doc = &state.doc;
    let scatter = state.pipeline.scatter_cache_mut();
    scatter.retain(|id| doc.layer(id).is_some());
    match globe.paint(canvas, cards && globe.blur) {
        Some(paints) => {
            for paint in paints {
                match paint {
                    Paint::Callback(callback) => {
                        painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
                    }
                    Paint::Vector(id) if globe_view => {
                        vector_layer(&painter, rect, doc, &globe.view, id, scatter);
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
    // A basin takes its size from its outline, so it has no brush circle.
    let circle = globe.brushing() && globe.brush.mode != Mode::Basin;
    if let Some(pos) = globe.input.pos.filter(|_| circle) {
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
    // The outline of a basin and its main river show until the basin arrives.
    if let Some((outline, river)) = globe.basin_preview() {
        let shade = egui::Stroke::new(3.5, egui::Color32::from_black_alpha(90));
        let line = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(220));
        if outline.len() > 2 {
            painter.add(egui::Shape::closed_line(outline.clone(), shade));
            painter.add(egui::Shape::closed_line(outline, line));
        }
        if river.len() > 1 {
            painter.line(river.clone(), shade);
            painter.line(river, line);
        }
    }
    if let Some(outline) = globe.outline_preview().filter(|o| o.len() > 1) {
        let shade = egui::Stroke::new(3.5, egui::Color32::from_black_alpha(90));
        let line = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(220));
        painter.line(outline.clone(), shade);
        painter.line(outline, line);
    }
    // The stroke of the layer, or a plain white one.
    let entries = state.doc.frame.entries.iter();
    let style = entries
        .filter(|e| Some(e.layer) == layer)
        .find_map(|e| e.style.stroke());
    let stroke = style.map_or(egui::Stroke::new(2.0, egui::Color32::WHITE), |s| {
        egui::Stroke::new((s.width as f32).max(1.0), color(s.color))
    });
    if let Some(highlight) = globe.snap_highlight() {
        snap_highlight(&painter, rect, globe, &highlight, stroke.width + 4.0);
    }
    // A ring marks the point of the open form.
    if let Some(pos) = placed.and_then(|p| globe.project(lonlat_to_dir(p[0], p[1]))) {
        let ring = egui::Stroke::new(2.0, theme::ACCENT);
        painter.circle_stroke(pos, 9.0, ring);
    }
    measure_overlay(
        &painter,
        rect,
        globe,
        &state.measure,
        &state.doc.project.world,
    );
    if let Some(line) = globe.line_preview() {
        let mut line = line.to_vec();
        // The stroke of the polygon tool closes.
        if globe.filling() && line.len() > 2 {
            line.push(line[0]);
        }
        painter.line(line, stroke);
    }
    super::select::paint(&painter, state);
    if state.globe.nav.active() || more {
        ui.ctx().request_repaint();
    }
}
