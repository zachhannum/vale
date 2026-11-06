//! The globe canvas.

use std::sync::Arc;
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
use crate::globe::flat::FlatView;
use crate::globe::math::V3;
use crate::globe::math::{angle, dir_to_lonlat, lonlat_to_dir};
use crate::globe::nav::{Nav, Painting, TAP_SECONDS, TAP_SLOP, Tap, on_canvas};
use crate::globe::select::Pointer;
use crate::globe::shapes::{Directions, DrawKey, OnScreen, Out, ViewKey};
use crate::globe::snap::Highlight;
use crate::globe::stroke_group;
use crate::globe::symbol::{GlobeScatter, SymbolMesh, append_missing, view_places};
use crate::globe::vector;
use crate::globe::view::GlobeView;
use crate::globe::{FingerPress, Globe, Paint, RasterMask, WorldView};
use crate::mask::MaskedFeature;
use crate::measure::{DENSIFY_DEG, MOUSE, Measure, TapPress};
use crate::pen::{PenEvent, PenPhase, QUEUE_PEN};
use crate::pipeline::Pipeline;
use crate::scatter::ScatterFill;
use crate::ui::pad::theme;
use vale_render::hatch::offset_ring;
use vale_sphere::LonLat;
use vale_sphere::measure::densify_great_circle;
use vale_style::expr::Context;
use vale_style::{Hatch, MarkerShape, Paint as StylePaint, StrokeKind, scale_denominator};

use crate::pipeline;

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

/// Starts the stroke of the finger that waits, with the places that it
/// touched. The finger no longer moves the view.
fn finger_down(globe: &mut Globe, time: f64, sample: &impl Fn(&mut Globe, egui::Pos2, f64)) {
    let Some(finger) = globe.input.finger.take() else {
        return;
    };
    globe.nav.forget(finger.id);
    globe.input.pen = Some(finger.id);
    press(globe);
    for pos in finger.path {
        sample(globe, pos, FIXED_FLOW);
    }
    globe.pen.stats.sample(time, None, None);
}

/// Reads one touch on a device with no pen. One finger paints with no
/// pressure. It waits until it moves, holds, or lifts, because a second
/// finger in that time gives both fingers to the view. A second finger
/// also ends a stroke and gives both fingers to the view. `started` is true
/// if a finger came down earlier in the frame, before the view reads it.
#[allow(clippy::too_many_arguments)]
fn finger_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    globe: &mut Globe,
    started: &mut bool,
    id: u64,
    phase: egui::TouchPhase,
    pos: egui::Pos2,
    time: f64,
    sample: &impl Fn(&mut Globe, egui::Pos2, f64),
) {
    let waits = globe.input.finger.as_ref().is_some_and(|f| f.id == id);
    match phase {
        egui::TouchPhase::Start => {
            let alone = !std::mem::replace(started, true) && globe.nav.counts().1 == 0;
            if globe.input.finger.take().is_some() {
                return;
            }
            if let Some(painter) = globe.input.pen.take() {
                let last = globe.input.pos.take().unwrap_or(pos);
                release(globe);
                globe.pen.stats.up();
                globe.nav.adopt(painter, last);
                return;
            }
            if alone && !globe.input.mouse && on_canvas(ui, rect, pos) {
                let path = vec![pos];
                globe.input.finger = Some(FingerPress {
                    id,
                    start: time,
                    path,
                });
            }
        }
        egui::TouchPhase::Move if waits => {
            let finger = globe.input.finger.as_mut().unwrap();
            finger.path.push(pos);
            if (pos - finger.path[0]).length() > TAP_SLOP {
                finger_down(globe, time, sample);
            }
        }
        egui::TouchPhase::Move => {
            if globe.input.pen == Some(id) {
                sample(globe, pos, FIXED_FLOW);
                globe.pen.stats.sample(time, None, None);
            }
        }
        egui::TouchPhase::End | egui::TouchPhase::Cancel => {
            if waits {
                if phase == egui::TouchPhase::End {
                    finger_down(globe, time, sample);
                } else {
                    globe.input.finger = None;
                }
            }
            if globe.input.pen == Some(id) {
                globe.input.pen = None;
                globe.input.pos = None;
                release(globe);
                globe.pen.stats.up();
            }
        }
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
/// comes from the queue and not from egui. On a device with no pen, one
/// finger takes the place of the pen.
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
    if !(brushing || globe.stroking() || globe.placing()) || globe.importing() {
        if globe.input.pen.take().is_some() || std::mem::take(&mut globe.input.mouse) {
            release(globe);
            globe.pen.stats.up();
        }
        globe.input.pos = None;
        globe.input.finger = None;
        if globe.measuring() && !globe.importing() {
            measure_input(ui, rect, resp, globe, &queue, from_queue);
        }
        if (globe.selecting() || globe.region) && !globe.importing() {
            return select_input(ui, rect, resp, globe, &queue, from_queue);
        }
        if globe.vertexing() && !globe.importing() {
            return vertex_input(ui, rect, resp, globe, &queue, from_queue);
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

    let no_pen = globe.nav.no_pen;
    let mut started = false;
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
        if no_pen {
            let started = &mut started;
            finger_input(ui, rect, globe, started, id.0, *phase, *pos, time, &sample);
            continue;
        }
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

    if globe
        .input
        .finger
        .as_ref()
        .is_some_and(|f| time - f.start > TAP_SECONDS)
    {
        finger_down(globe, time, &sample);
    }

    let (pos, pressed, down, shift, option, moved) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.modifiers.alt,
            i.pointer.delta() != egui::Vec2::ZERO,
        )
    });
    // Shift and Option choose the mode of the polygon tool, so they start
    // a stroke and do not turn the globe.
    let keys = globe.filling();
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
        Some(pos) if pressed && (keys || !shift) && !touched && resp.contains_pointer() => {
            globe.input.mouse = true;
            press(globe);
            sample(globe, pos, FIXED_FLOW);
            globe.pen.stats.sample(time, None, None);
        }
        _ => {}
    }
    if keys {
        globe.line_keys(shift, option);
    }
    if globe.input.pen.is_none() {
        let hover = globe.pen.hover.filter(|pos| on_canvas(ui, rect, *pos));
        globe.input.pos = resp.hover_pos().or(hover);
    }
    Painting {
        pen: true,
        mouse: globe.input.mouse,
        finger: globe.input.pen.filter(|_| no_pen),
        fingers: no_pen,
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
/// and the mouse belong to the tool, and fingers still move the view. A
/// finger that holds still as Shift does not move the view.
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
    press.time = ui.input(|i| i.time);
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
    // The first touch is also the egui pointer. A click with Shift adds to
    // the selection, and a drag with Shift turns the view.
    if !touched && !globe.nav.touching() {
        let (pos, pressed, released, shift) = ui.input(|i| {
            let p = &i.pointer;
            let (pressed, released) = (p.primary_pressed(), p.primary_released());
            (p.latest_pos(), pressed, released, i.modifiers.shift)
        });
        match pos {
            Some(pos) => {
                if pressed && resp.contains_pointer() {
                    if shift {
                        press.shift_down(MOUSE, pos);
                    } else {
                        press.down(MOUSE, pos, Pointer::Mouse);
                    }
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
    let held = globe.select_press.held();
    if let Some(id) = held {
        globe.nav.forget(id);
    }
    Painting {
        pen: true,
        mouse: globe.select_press.mouse(),
        finger: held,
        fingers: held.is_some(),
    }
}

/// Reads the pen, the touches, and the mouse for the Vertex tool. A pen and
/// the mouse belong to the tool. A finger that starts on a handle drags
/// it, and other fingers move the view.
fn vertex_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    resp: &egui::Response,
    globe: &mut Globe,
    queue: &[(egui::Pos2, PenEvent)],
    from_queue: bool,
) -> Painting {
    let on = |pos| on_canvas(ui, rect, pos);
    let handles = std::mem::take(&mut globe.vertex.handles);
    let on_handle = |pos: egui::Pos2| {
        let reach = super::vertex::HANDLE_REACH;
        handles.iter().any(|h| h.distance(pos) <= reach)
    };
    let press = &mut globe.vertex.press;
    for (pos, event) in queue {
        match event.phase {
            PenPhase::Down if on(*pos) => {
                press.down(QUEUE_PEN, *pos, Pointer::Pen, on_handle(*pos))
            }
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
            egui::TouchPhase::Start if on(*pos) => press.down(id.0, *pos, pointer, on_handle(*pos)),
            egui::TouchPhase::Move => press.moved(id.0, *pos),
            egui::TouchPhase::End => press.up(id.0, *pos),
            egui::TouchPhase::Cancel => press.cancel(id.0),
            egui::TouchPhase::Start => {}
        }
    }
    // The first touch is also the egui pointer, and the view does not have
    // the finger on a handle. A drag with Shift turns the view.
    if !touched && !globe.nav.touching() && press.claimed_finger().is_none() {
        let (pos, pressed, released, shift) = ui.input(|i| {
            let p = &i.pointer;
            let (pressed, released) = (p.primary_pressed(), p.primary_released());
            (p.latest_pos(), pressed, released, i.modifiers.shift)
        });
        match pos {
            Some(pos) => {
                if pressed && !shift && resp.contains_pointer() {
                    press.down(MOUSE, pos, Pointer::Mouse, on_handle(pos));
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
    globe.vertex.handles = handles;
    let hover = globe.pen.hover.filter(|pos| on(*pos));
    globe.input.pos = resp.hover_pos().or(hover);
    Painting {
        pen: true,
        mouse: globe.vertex.press.mouse(),
        finger: globe.vertex.press.claimed_finger(),
        fingers: false,
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

/// Reads the keys of a held ocean ridge or basin. Return applies it, and
/// Escape discards it.
fn held_keys(ui: &egui::Ui, state: &mut AppState) {
    use egui::{Key, Modifiers};
    if ui.ctx().egui_wants_keyboard_input() || state.globe.holding().is_none() {
        return;
    }
    let key = |key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if key(Key::Enter) {
        state.globe.apply_held();
    }
    if key(Key::Escape) {
        state.globe.discard_held();
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

/// Paints a marker at each of `points`: the fills, then the hatches, then the
/// strokes, as the page draws them. The stroke is outside the fill, as in the
/// pipeline.
fn paint_markers(
    sink: &mut Sink,
    points: &[egui::Pos2],
    radius: f64,
    shape: MarkerShape,
    fill: Option<vale_style::Color>,
    hatch: Option<Hatch>,
    stroke: Option<vale_style::Stroke>,
) {
    if radius <= 0.0 || points.is_empty() {
        return;
    }
    let to_pos = |p: &kurbo::Point| egui::pos2(p.x as f32, p.y as f32);
    let center = |pos: egui::Pos2| kurbo::Point::new(f64::from(pos.x), f64::from(pos.y));
    let outline = |pos| pipeline::marker_outline(shape, center(pos), radius);
    match (fill, hatch) {
        (Some(c), None) if shape == MarkerShape::Circle => {
            for &pos in points {
                sink.add(egui::Shape::circle_filled(pos, radius as f32, color(c)));
            }
        }
        (Some(c), None) => {
            // Each shape is star-shaped around its center, so a fan from the center fills it.
            let c = color(c);
            for &pos in points {
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(pos, c);
                let outline = outline(pos);
                for p in &outline {
                    mesh.colored_vertex(to_pos(p), c);
                }
                let n = outline.len() as u32;
                for i in 0..n {
                    mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
                }
                sink.add(mesh);
            }
        }
        (Some(c), Some(h)) => {
            let (opaque, alpha) = opaque(c);
            let line = egui::Stroke::new(h.line_width as f32, opaque);
            let shapes = points
                .iter()
                .flat_map(|&pos| pipeline::marker_hatch(&outline(pos), &h, center(pos)))
                .map(|(a, b)| egui::Shape::line_segment([to_pos(&a), to_pos(&b)], line))
                .collect();
            let paint = vale_style::Stroke::solid(c, h.line_width);
            sink.stroke(paint, (shapes, alpha));
        }
        (None, _) => {}
    }
    if let Some(s) = stroke {
        let (opaque, alpha) = opaque(s.color);
        let line = egui::Stroke::new(s.width as f32, opaque);
        let shapes = points
            .iter()
            .map(|&pos| {
                if shape == MarkerShape::Circle {
                    egui::Shape::circle_stroke(pos, (radius + s.width / 2.0) as f32, line)
                } else {
                    let ring = offset_ring(&outline(pos), s.width / 2.0);
                    let ring = ring.iter().map(to_pos).collect();
                    egui::epaint::PathShape::closed_line(ring, line).into()
                }
            })
            .collect();
        sink.stroke(s, (shapes, alpha));
    }
}

/// The color `c` with full alpha, and the alpha of `c` from 0 to 1.
fn opaque(c: vale_style::Color) -> (egui::Color32, f32) {
    (
        egui::Color32::from_rgb(c.r, c.g, c.b),
        f32::from(c.a) / 255.0,
    )
}

/// A shape that a raster mask cuts.
#[derive(Clone)]
pub enum Cut {
    Shape(egui::Shape),
    /// The opaque shapes of a translucent stroke, which draw as one group
    /// with the alpha.
    Group(f32, Vec<egui::Shape>),
}

/// The shapes of a vector layer on their way to the painter, in draw order.
/// The symbols wait in one mesh, and the translucent strokes of one paint
/// wait in one group, until a shape of another kind comes. With a cut, the
/// fills, the strokes, and the groups go to the list of shapes that a raster
/// mask cuts. The sink keeps the shapes, and `paint` sends them on.
struct Sink {
    /// The format of the render target. `None`: no wgpu renderer, and a
    /// translucent stroke goes to the painter shape by shape.
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    /// The shapes go to a cut.
    cut: bool,
    symbols: egui::Mesh,
    /// The paint of the open group, its alpha, and its opaque shapes.
    group: Option<(vale_style::Stroke, f32, Vec<egui::Shape>)>,
    out: Vec<Out>,
}

impl Sink {
    fn new(format: Option<egui_wgpu::wgpu::TextureFormat>, cut: bool) -> Self {
        Sink {
            format,
            cut,
            symbols: egui::Mesh::default(),
            group: None,
            out: Vec::new(),
        }
    }

    /// Sends the waiting symbols or the open group on.
    fn flush(&mut self) {
        if !self.symbols.is_empty() {
            let symbols = std::mem::take(&mut self.symbols);
            self.out.push(Out::Shape(symbols.into()));
        }
        if let Some((_, alpha, shapes)) = self.group.take() {
            if self.cut {
                self.out.push(Out::Cut(Cut::Group(alpha, shapes)));
            } else if self.format.is_some() {
                self.out.push(Out::Group(alpha, shapes));
            }
        }
    }

    /// Sends the open group on, if there is one.
    fn end_group(&mut self) {
        if self.group.is_some() {
            self.flush();
        }
    }

    fn add(&mut self, shape: impl Into<egui::Shape>) {
        self.flush();
        let shape = shape.into();
        self.out.push(match self.cut {
            true => Out::Cut(Cut::Shape(shape)),
            false => Out::Shape(shape),
        });
    }

    /// Sends the shapes that `paint` gives to the painter, also with a cut.
    fn uncut(&mut self, paint: impl FnOnce(&mut Self)) {
        let cut = std::mem::replace(&mut self.cut, false);
        paint(self);
        self.flush();
        self.cut = cut;
    }

    /// The mesh of the waiting symbols.
    fn symbols(&mut self) -> &mut egui::Mesh {
        self.end_group();
        &mut self.symbols
    }

    /// Paints the shapes of a stroke with the paint `paint`, as
    /// `vector::stroke_shapes` gives them.
    fn stroke(&mut self, paint: vale_style::Stroke, (shapes, alpha): (Vec<egui::Shape>, f32)) {
        if alpha >= 1.0 {
            self.flush();
            let cut = self.cut;
            self.out.extend(shapes.into_iter().map(|shape| match cut {
                true => Out::Cut(Cut::Shape(shape)),
                false => Out::Shape(shape),
            }));
            return;
        }
        if self.format.is_none() && !self.cut {
            self.flush();
            self.out.push(Out::Faded(alpha, shapes));
            return;
        }
        match &mut self.group {
            Some((open, _, group)) if *open == paint => group.extend(shapes),
            _ => {
                self.flush();
                self.group = Some((paint, alpha, shapes));
            }
        }
    }

    /// The shapes, in draw order.
    fn finish(mut self) -> Vec<Out> {
        self.flush();
        self.out
    }
}

/// Sends the shapes of a sink to the painter and to the cut.
fn paint_out(
    painter: &egui::Painter,
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    mut cut: Option<&mut Vec<Cut>>,
    out: &[Out],
) {
    for out in out {
        match out {
            Out::Shape(shape) => {
                painter.add(shape.clone());
            }
            Out::Faded(alpha, shapes) => {
                let mut painter = painter.clone();
                painter.multiply_opacity(*alpha);
                painter.extend(shapes.iter().cloned());
            }
            Out::Group(alpha, shapes) => {
                let Some(format) = format else { continue };
                let clip = painter.clip_rect();
                let shapes = shapes.clone();
                let shape = stroke_group::group_shape(painter.ctx(), format, shapes, *alpha, clip);
                if let Some(shape) = shape {
                    painter.add(shape);
                }
            }
            Out::Cut(shape) => {
                if let Some(cut) = &mut cut {
                    cut.push(shape.clone());
                }
            }
        }
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

/// Draws one vector layer over the globe, in the draw order of the page:
/// rule by rule and symbolizer by symbolizer, and the features of one paint
/// together. The symbols go to one mesh, which goes to the painter before the
/// next shape that is not a symbol. The translucent strokes of one paint go
/// to one group. A layer with a mask draws the parts of its features inside
/// the mask.
pub fn vector_layer(
    painter: &egui::Painter,
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    rect: egui::Rect,
    doc: &Document,
    view: &GlobeView,
    id: LayerId,
    pipeline: &mut Pipeline,
) {
    vector_layer_cut(painter, format, None, rect, doc, view, id, pipeline);
}

/// Draws one vector layer over the globe, as `vector_layer`. With `cut`,
/// the fills and the strokes go to `cut`, and the other shapes go to the
/// painter. The layer keeps its shapes between frames, and a draw with no
/// change to the view, the layer, or its style sends the kept shapes again.
#[expect(clippy::too_many_arguments)]
pub fn vector_layer_cut(
    painter: &egui::Painter,
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    cut: Option<&mut Vec<Cut>>,
    rect: egui::Rect,
    doc: &Document,
    view: &GlobeView,
    id: LayerId,
    pipeline: &mut Pipeline,
) {
    let Some(layer) = doc.layer(id) else {
        return;
    };
    let masked = pipeline.masked(doc, id);
    let radius = view.radius(rect);
    let radius_m = doc.project.world.radius_km * 1000.0;
    let scale = scale_denominator(radius_m / radius);
    let pixels_per_point = painter.ctx().pixels_per_point();
    let features = &layer.features;
    let entries: Vec<_> = (doc.frame.entries.iter())
        .filter(|e| e.layer == id && layer.visible)
        .collect();
    let batches = (entries.iter())
        .map(|entry| {
            pipeline::style_batches(&entry.style.style, scale, features.len(), |i| Context {
                attributes: &features[i].attributes,
                scale,
                geometry: features[i].geometry.kind(),
            })
        })
        .collect();
    let view_key = ViewKey {
        view: *view,
        rect,
        pixels_per_point,
    };
    let key = DrawKey {
        view: view_key,
        clip: painter.clip_rect(),
        batches,
        symbols: Arc::as_ptr(doc.symbols()) as usize,
        format,
        cut: cut.is_some(),
    };
    let (scatter, cache) = pipeline.globe_caches_mut();
    let (shapes, work) = cache.layer(layer, masked.as_ref());
    if let Some(out) = shapes.drawn(&key) {
        paint_out(painter, format, cut, out);
        return;
    }
    work.draws += 1;
    let step = vector::step(radius * f64::from(pixels_per_point));
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
    let measured = |lines: &[Vec<V3>]| -> Vec<Vec<(kurbo::Point, f64)>> {
        (lines.iter())
            .flat_map(|line| vector::clip_line_measured(&view.rot, line, step))
            .map(|part| part.iter().map(|(p, f)| (point(p), *f)).collect())
            .collect()
    };
    let on_screen = |directions: &Directions| match directions {
        Directions::Points(points) => OnScreen::Points(
            points
                .iter()
                .filter_map(|d| view.project(rect, *d))
                .collect(),
        ),
        Directions::Lines(lines) => OnScreen::Lines(measured(lines)),
        Directions::Polygons(polygons, outlines) => {
            let clipped: Vec<_> = (polygons.iter())
                .map(|rings| vector::clip_polygon(&view.rot, rings, step))
                .collect();
            let triangles = vec![None; clipped.len()];
            let outlines = outlines.as_ref().map(|lines| {
                (lines.iter())
                    .flat_map(|line| vector::clip_line(&view.rot, line, step))
                    .map(|o| o.iter().map(|p| (point(p), 0.0)).collect())
                    .collect()
            });
            OnScreen::Polygons(clipped, triangles, outlines)
        }
    };
    let symbols = doc.symbols();
    let mut places = None;
    let mut sink = Sink::new(format, key.cut);
    let (dirs, screens) = shapes.view(view_key, work);
    for (entry, batches) in entries.iter().zip(&key.batches) {
        let mut count = 0;
        for batch in batches {
            let paint = &batch.paint;
            let extent = vale_style::extent_of(std::slice::from_ref(paint));
            let margin = (extent.stroke_width + extent.marker_radius) as f32 + 2.0;
            for &index in &batch.features {
                let feature = &features[index];
                let hidden = dirs[index]
                    .cap
                    .is_some_and(|cap| cap.hidden(view, rect, margin));
                if hidden {
                    continue;
                }
                let screen = screens[index].get_or_insert_with(|| {
                    work.clipped += 1;
                    on_screen(&dirs[index].directions)
                });
                let seed = index as u64;
                match (screen, paint) {
                    (OnScreen::Points(_), StylePaint::Marker { .. }) => {}
                    (
                        OnScreen::Points(points),
                        StylePaint::Symbol {
                            symbol,
                            size,
                            rotation,
                            tint,
                        },
                    ) if *size > 0.0 => {
                        let size = *size as f32;
                        for &pos in points.iter().filter(|p| rect.expand(size).contains(**p)) {
                            match symbols.get(symbol) {
                                Some(s) => SymbolMesh::of(s).append(
                                    sink.symbols(),
                                    pos,
                                    size,
                                    *rotation as f32,
                                    *tint,
                                ),
                                None => append_missing(sink.symbols(), pos, size, *tint),
                            }
                        }
                    }
                    (OnScreen::Lines(parts), StylePaint::Stroke(stroke)) if !parts.is_empty() => {
                        sink.stroke(*stroke, vector::stroke_shapes(parts, stroke, seed));
                    }
                    (OnScreen::Polygons(clipped, triangles, _), StylePaint::Fill(c)) => {
                        for (part, triangles) in clipped.iter().zip(triangles) {
                            let (points, indices) =
                                triangles.get_or_insert_with(|| vector::triangulate(&part.loops));
                            let vertices = points
                                .iter()
                                .map(|p| egui::epaint::Vertex {
                                    pos: to_screen(p),
                                    uv: egui::epaint::WHITE_UV,
                                    color: color(*c),
                                })
                                .collect();
                            sink.add(egui::Mesh {
                                indices: indices.clone(),
                                vertices,
                                ..Default::default()
                            });
                        }
                    }
                    (OnScreen::Polygons(clipped, _, outlines), StylePaint::Stroke(stroke)) => {
                        let mut stroke = *stroke;
                        // A polygon outline has no start and no end to taper.
                        if matches!(stroke.kind, StrokeKind::Taper { .. }) {
                            stroke.kind = StrokeKind::Solid;
                        }
                        if let Some(outlines) = outlines {
                            sink.stroke(stroke, vector::stroke_shapes(outlines, &stroke, seed));
                            continue;
                        }
                        for part in clipped.iter() {
                            let outlines: Vec<_> = part
                                .outlines
                                .iter()
                                .map(|o| o.iter().map(|p| (point(p), 0.0)).collect())
                                .collect();
                            let shapes = vector::stroke_shapes(&outlines, &stroke, seed);
                            sink.stroke(stroke, shapes);
                        }
                    }
                    (OnScreen::Polygons(..), StylePaint::Scatter { symbol, tint, .. }) => {
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
                        let instances = on_globe.instances(entry.layer, index, feature, &fill);
                        count = on_globe.count;
                        let mesh = symbols.get(symbol).map(SymbolMesh::of);
                        for i in instances {
                            match &mesh {
                                Some(m) => {
                                    m.append(sink.symbols(), i.pos, i.size, i.rotation, *tint)
                                }
                                None => append_missing(sink.symbols(), i.pos, i.size, *tint),
                            }
                        }
                    }
                    _ => {}
                }
            }
            // The markers of a paint draw as on the page: the fills of all
            // points, then their hatches, then their strokes.
            if let StylePaint::Marker {
                radius,
                shape,
                fill,
                hatch,
                stroke,
            } = paint
            {
                let points: Vec<egui::Pos2> = batch
                    .features
                    .iter()
                    .filter_map(|&i| match &screens[i] {
                        Some(OnScreen::Points(points)) => Some(points),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                // A mask does not cut the points.
                sink.uncut(|sink| {
                    paint_markers(sink, &points, *radius, *shape, *fill, *hatch, *stroke);
                });
            }
            // Two batches with the same paint are two layers on the page too.
            sink.end_group();
        }
    }
    let out = sink.finish();
    paint_out(painter, format, cut, &out);
    shapes.keep(key, out);
}

/// The geometry of a feature in the flat view: the rings of the fill, the
/// outline, and the triangles of the fill, made at the first fill.
struct FlatScreen {
    fill: Option<Vec<Vec<vector::P2>>>,
    outline: Vec<Vec<(kurbo::Point, f64)>>,
    triangles: Option<(Vec<vector::P2>, Vec<u32>)>,
}

/// Draws the fills and the strokes of one vector layer over the flat view,
/// in the draw order of `vector_layer`. The points, the symbols, and the
/// scatter fills do not draw there. A layer with a mask draws the parts of
/// its features inside the mask.
pub fn flat_vector_layer(
    painter: &egui::Painter,
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    rect: egui::Rect,
    doc: &Document,
    flat: &FlatView,
    id: LayerId,
    pipeline: &mut Pipeline,
) {
    flat_vector_layer_cut(painter, format, None, rect, doc, flat, id, pipeline);
}

/// Draws one vector layer over the flat view, as `flat_vector_layer`. With
/// `cut`, the shapes go to `cut`.
#[expect(clippy::too_many_arguments)]
pub fn flat_vector_layer_cut(
    painter: &egui::Painter,
    format: Option<egui_wgpu::wgpu::TextureFormat>,
    cut: Option<&mut Vec<Cut>>,
    rect: egui::Rect,
    doc: &Document,
    flat: &FlatView,
    id: LayerId,
    pipeline: &mut Pipeline,
) {
    let (Some(entry), Some(layer)) = (doc.entry(id), doc.layer(id).filter(|l| l.visible)) else {
        return;
    };
    let masked = pipeline.masked(doc, id);
    let projection = flat.projection();
    let radius_m = doc.project.world.radius_km * 1000.0;
    let scale = scale_denominator(radius_m / flat.scale(rect));
    let to_screen = |p: &vale_sphere::Point| flat.screen(rect, *p);
    let measured = |lines: Vec<Vec<vale_sphere::Point>>| -> Vec<Vec<(kurbo::Point, f64)>> {
        (lines.into_iter())
            .map(|line| {
                let length: f64 = line.windows(2).map(|w| w[0].distance(w[1])).sum();
                let mut along = 0.0;
                (0..line.len())
                    .map(|i| {
                        if i > 0 {
                            along += line[i - 1].distance(line[i]);
                        }
                        let q = to_screen(&line[i]);
                        let f = if length > 0.0 { along / length } else { 0.0 };
                        (kurbo::Point::new(f64::from(q.x), f64::from(q.y)), f)
                    })
                    .collect()
            })
            .collect()
    };
    let on_screen = |geometry: &Geometry, part: Option<&MaskedFeature>| match geometry {
        Geometry::Points(_) => None,
        Geometry::Lines(lines) => {
            let lines = part.map_or(lines, |p| &p.lines);
            let outline = (lines.iter())
                .flat_map(|l| projection.project_line(l))
                .collect();
            Some(FlatScreen {
                fill: None,
                outline: measured(outline),
                triangles: None,
            })
        }
        Geometry::Polygons(polygons) => {
            let rings: Vec<Vec<vector::P2>> = (part.map_or(polygons, |p| &p.polygons).iter())
                .flat_map(|p| projection.project_polygon(p))
                .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
                .collect();
            // The outline of a masked polygon is its own outline cut by the
            // mask.
            let outline: Vec<_> = match part {
                Some(p) => (p.lines.iter())
                    .flat_map(|l| projection.project_line(l))
                    .collect(),
                None => (polygons.iter())
                    .flat_map(|p| projection.project_outline_of(p))
                    .collect(),
            };
            Some(FlatScreen {
                fill: Some(rings),
                outline: measured(outline),
                triangles: None,
            })
        }
    };
    let features = &layer.features;
    let batches = pipeline::style_batches(&entry.style.style, scale, features.len(), |i| Context {
        attributes: &features[i].attributes,
        scale,
        geometry: features[i].geometry.kind(),
    });
    let mut screens: Vec<Option<Option<FlatScreen>>> = features.iter().map(|_| None).collect();
    let mut sink = Sink::new(format, cut.is_some());
    for batch in &batches {
        for &index in &batch.features {
            let feature = &features[index];
            let part = (masked.as_ref()).and_then(|m| m.features.get(index)?.as_ref());
            let screen = screens[index].get_or_insert_with(|| on_screen(&feature.geometry, part));
            let Some(screen) = screen else {
                continue;
            };
            let seed = index as u64;
            match &batch.paint {
                StylePaint::Fill(c) => {
                    let Some(rings) = &screen.fill else { continue };
                    let (points, indices) = screen.triangles.get_or_insert_with(|| {
                        vector::triangulate(&vector::even_odd_loops(rings.clone()))
                    });
                    let vertices = (points.iter())
                        .map(|p| egui::epaint::Vertex {
                            pos: to_screen(&vale_sphere::Point::new(p[0], p[1])),
                            uv: egui::epaint::WHITE_UV,
                            color: color(*c),
                        })
                        .collect();
                    sink.add(egui::Mesh {
                        indices: indices.clone(),
                        vertices,
                        ..Default::default()
                    });
                }
                StylePaint::Stroke(stroke) if !screen.outline.is_empty() => {
                    let mut stroke = *stroke;
                    // A polygon outline has no start and no end to taper.
                    if screen.fill.is_some() && matches!(stroke.kind, StrokeKind::Taper { .. }) {
                        stroke.kind = StrokeKind::Solid;
                    }
                    sink.stroke(
                        stroke,
                        vector::stroke_shapes(&screen.outline, &stroke, seed),
                    );
                }
                _ => {}
            }
        }
        sink.end_group();
    }
    paint_out(painter, format, cut, &sink.finish());
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
        held_buttons(ui, state, rect);
    }
}

/// The state and the Discard and Apply buttons of a held ocean ridge or
/// basin, at the bottom of the canvas. The outline of the next basin shows
/// the hint of its main river and the Clear button there.
fn held_buttons(ui: &egui::Ui, state: &mut AppState, rect: egui::Rect) {
    use super::panels::{APPLY, BASIN_RIVER_HINT, CLEAR_OUTLINE, DISCARD, READY, UPDATING};
    let globe = &mut state.globe;
    let outline = globe.brushing() && globe.brush.mode == Mode::Basin;
    let outline = outline && globe.basin_outline().is_some();
    if globe.holding().is_none() && !outline {
        return;
    }
    let at = rect.center_bottom() - egui::vec2(0.0, 24.0);
    egui::Area::new(egui::Id::new("held-buttons"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if globe.holding().is_none() {
                        ui.label(BASIN_RIVER_HINT);
                        if ui.button(CLEAR_OUTLINE).clicked() {
                            globe.set_basin_outline(None);
                        }
                        return;
                    }
                    // A spinner shows while the stroke follows the settings.
                    if globe.held_updating() {
                        ui.add(egui::Spinner::new());
                        ui.label(UPDATING);
                    } else {
                        ui.label(READY);
                    }
                    if ui.button(DISCARD).clicked() {
                        globe.discard_held();
                    }
                    if ui.button(APPLY).clicked() {
                        globe.apply_held();
                    }
                });
            });
        });
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
    state.refresh_vertex_handles();
    let globe = &mut state.globe;
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
    state.poll_region();
    state.poll_select();
    state.poll_vertex();
    measure_keys(ui, state);
    held_keys(ui, state);
    super::select::keys(ui, state);
    super::vertex::keys(ui, state);
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

/// The raster mask of each vector layer whose mask is a raster layer that
/// the globe holds.
fn raster_masks(doc: &Document, globe: &Globe) -> Vec<(LayerId, RasterMask)> {
    let project = &doc.project;
    let layers = project.layers();
    let raster_of = |id: LayerId| {
        layers
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| match l.kind {
                vale_store::LayerKind::Raster(raster) => Some(raster),
                _ => None,
            })
    };
    (doc.frame.entries.iter())
        .filter_map(|entry| {
            let vale_style::Mask::Raster { layer, min, max } = entry.style.style.mask? else {
                return None;
            };
            let raster = raster_of(layer).filter(|&r| globe.holds(r))?;
            let smoothing = project.raster(raster)?.smoothing;
            let mask = RasterMask {
                raster,
                min,
                max,
                smoothing,
            };
            Some((entry.layer, mask))
        })
        .collect()
}

/// The meshes of `cuts` in their order, each with the clip rectangle `clip`
/// in points and its alpha. The meshes read no texture, so a disc is a
/// polygon and not a picture of the font atlas.
pub fn tessellate(
    ctx: &egui::Context,
    clip: egui::Rect,
    cuts: Vec<Cut>,
) -> Vec<(egui::Rect, egui::Mesh, f32)> {
    let ppp = ctx.pixels_per_point();
    let options = ctx.tessellation_options(|o| *o);
    let mut out = Vec::new();
    let mut shapes = Vec::new();
    let push = |out: &mut Vec<_>, shapes: Vec<egui::Shape>, alpha| {
        let mesh = stroke_group::tessellate(shapes, ppp, options, clip);
        if !mesh.is_empty() {
            out.push((clip, mesh, alpha));
        }
    };
    for cut in cuts {
        match cut {
            Cut::Shape(shape) => shapes.push(shape),
            Cut::Group(alpha, group) => {
                if !shapes.is_empty() {
                    push(&mut out, std::mem::take(&mut shapes), 1.0);
                }
                push(&mut out, group, alpha);
            }
        }
    }
    if !shapes.is_empty() {
        push(&mut out, shapes, 1.0);
    }
    out
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
    // go to the painter in the draw order.
    let globe_view = globe.world_view == WorldView::Globe;
    let canvas = Canvas {
        background,
        pixels_per_point,
    };
    let doc = &state.doc;
    let pipeline = &mut state.pipeline;
    (pipeline.scatter_cache_mut()).retain(|id| doc.layer(id).is_some());
    globe.set_raster_masks(raster_masks(doc, globe));
    let format = globe.format;
    match globe.paint(canvas, cards && globe.blur) {
        Some(paints) => {
            for paint in paints {
                match paint {
                    Paint::Callback(callback) => {
                        painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
                    }
                    Paint::Vector(id) if globe_view => {
                        vector_layer(&painter, format, rect, doc, &globe.view, id, pipeline);
                    }
                    Paint::Vector(id) => {
                        let flat = &globe.flat;
                        flat_vector_layer(&painter, format, rect, doc, flat, id, pipeline);
                    }
                    Paint::Masked(id, pass) => {
                        // The cut shapes go below the other shapes of the
                        // layer.
                        let slot = painter.add(egui::Shape::Noop);
                        let mut shapes = Vec::new();
                        let cut = Some(&mut shapes);
                        let (view, flat) = (&globe.view, &globe.flat);
                        if globe_view {
                            vector_layer_cut(&painter, format, cut, rect, doc, view, id, pipeline);
                        } else {
                            flat_vector_layer_cut(
                                &painter, format, cut, rect, doc, flat, id, pipeline,
                            );
                        }
                        let meshes = tessellate(ui.ctx(), painter.clip_rect(), shapes);
                        let callback = pass.callback(pixels_per_point, meshes);
                        let callback = egui_wgpu::Callback::new_paint_callback(rect, callback);
                        painter.set(slot, callback);
                    }
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
    // A basin takes its size from its outline, and the pen draws the center
    // line of an ocean ridge, so they have no brush circle.
    let line = matches!(globe.brush.mode, Mode::Basin | Mode::OceanRidge);
    let circle = globe.brushing() && !line;
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
    if let Some(axis) = globe.ridge_preview().filter(|a| a.len() > 1) {
        let shade = egui::Stroke::new(3.5, egui::Color32::from_black_alpha(90));
        let line = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(220));
        painter.line(axis.clone(), shade);
        painter.line(axis, line);
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
    measure_overlay(
        &painter,
        rect,
        globe,
        &state.measure,
        &state.doc.project.world,
    );
    if let Some(line) = globe.line_preview() {
        let mut line = line.to_vec();
        let reshape = globe.reshape_preview();
        // The stroke of the polygon tool closes, and a stroke that reshapes
        // stays open.
        if globe.filling() && reshape.is_none() && line.len() > 2 {
            line.push(line[0]);
        }
        // The outline that the stroke reshapes shows in the accent color.
        let layer = layer.and_then(|id| state.doc.layer(id));
        let feature = layer.and_then(|l| l.features.iter().find(|f| Some(f.uuid()) == reshape));
        if let Some(feature) = feature {
            let outline = egui::Stroke::new(3.0, theme::ACCENT);
            for run in crate::globe::select::shape(globe, &feature.geometry).lines {
                painter.line(run, outline);
            }
        }
        // A cut is not layer data, so it draws as a sketch.
        if globe.splitting() {
            painter.line(
                line.clone(),
                egui::Stroke::new(3.5, egui::Color32::from_black_alpha(90)),
            );
            painter.line(
                line,
                egui::Stroke::new(1.5, egui::Color32::from_white_alpha(220)),
            );
        } else {
            painter.line(line, stroke);
        }
    }
    super::select::paint(&painter, state);
    super::vertex::paint(&painter, state);
    super::region::paint(&painter, state);
    if state.globe.nav.active() || more {
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_style::{Color, Stroke};

    const FORMAT: Option<egui_wgpu::wgpu::TextureFormat> =
        Some(egui_wgpu::wgpu::TextureFormat::Rgba8Unorm);

    /// The shapes that `vector_layer` gives for a line layer with `stroke`.
    fn line_shapes(
        stroke: Stroke,
        format: Option<egui_wgpu::wgpu::TextureFormat>,
    ) -> Vec<egui::Shape> {
        let text = r#"{"type":"MultiLineString","coordinates":[[[-30,0],[0,0],[0,30]],[[10,-10],[30,-10]]]}"#;
        layer_shapes(text, format, |style| {
            style.set_fill(None);
            style.set_stroke(Some(stroke));
        })
    }

    /// The shapes that `vector_layer` gives for the GeoJSON `text` with the
    /// style that `set` makes.
    fn layer_shapes(
        text: &str,
        format: Option<egui_wgpu::wgpu::TextureFormat>,
        set: impl FnOnce(&mut vale_style::LayerStyle),
    ) -> Vec<egui::Shape> {
        let mut doc = Document::empty();
        let id = doc.add_imported(
            vale_import::geojson::read_str("shapes", text).unwrap(),
            None,
        )[0];
        set(&mut doc.entry_mut(id).unwrap().style);
        let view = GlobeView::centered(0.0, 0.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0));
        let mut pipeline = Pipeline::new();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            vector_layer(&painter, format, rect, &doc, &view, id, &mut pipeline);
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|s| s.shape).collect()
    }

    #[test]
    fn a_translucent_stroke_goes_to_one_group_and_an_opaque_stroke_goes_directly() {
        let format = FORMAT;
        let translucent = Stroke::solid(Color::rgba(0, 0, 255, 128), 6.0);
        let shapes = line_shapes(translucent, format);
        // Both lines share the paint, so they share one group.
        assert_eq!(shapes.len(), 1);
        assert_eq!(callbacks(&shapes), 1);
        let opaque = Stroke::solid(Color::rgb(0, 0, 255), 6.0);
        let shapes = line_shapes(opaque, format);
        assert_eq!(shapes.len(), 2);
        assert_eq!(callbacks(&shapes), 0);
        // Without wgpu, the translucent lines go to the painter with the
        // alpha in their color.
        let shapes = line_shapes(translucent, None);
        assert_eq!(callbacks(&shapes), 0);
        assert!(shapes.iter().all(|s| matches!(
            s,
            egui::Shape::Path(p) if matches!(
                p.stroke.color,
                egui::epaint::ColorMode::Solid(c) if c.a() == 128
            )
        )));
    }

    /// Three squares in a row, each one a feature.
    const SQUARES: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[-30,-5],[-20,-5],[-20,5],[-30,5],[-30,-5]]]}},
        {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[-5,-5],[5,-5],[5,5],[-5,5],[-5,-5]]]}},
        {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[20,-5],[30,-5],[30,5],[20,5],[20,-5]]]}}
    ]}"#;

    fn callbacks(shapes: &[egui::Shape]) -> usize {
        shapes
            .iter()
            .filter(|s| matches!(s, egui::Shape::Callback(_)))
            .count()
    }

    #[test]
    fn the_translucent_strokes_of_a_polygon_layer_go_to_one_group_after_the_fills() {
        let shapes = layer_shapes(SQUARES, FORMAT, |style| {
            style.set_fill(Some(Color::rgb(255, 0, 0)));
            style.set_stroke(Some(Stroke::solid(Color::rgba(0, 0, 255, 128), 4.0)));
        });
        assert_eq!(shapes.len(), 4, "{shapes:?}");
        assert!(
            shapes[..3]
                .iter()
                .all(|s| matches!(s, egui::Shape::Mesh(_)))
        );
        assert!(matches!(shapes[3], egui::Shape::Callback(_)));
    }

    #[test]
    fn the_globe_draws_the_fills_of_a_layer_before_its_strokes() {
        let shapes = layer_shapes(SQUARES, FORMAT, |style| {
            style.set_fill(Some(Color::rgb(255, 0, 0)));
            style.set_stroke(Some(Stroke::solid(Color::rgb(0, 0, 255), 4.0)));
        });
        let kinds: Vec<_> = shapes
            .iter()
            .map(|s| match s {
                egui::Shape::Mesh(_) => "fill",
                egui::Shape::Path(_) => "stroke",
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            ["fill", "fill", "fill", "stroke", "stroke", "stroke"]
        );
    }

    #[test]
    fn the_translucent_hatches_and_strokes_of_markers_go_to_one_group_each() {
        use vale_style::expr::Property;
        use vale_style::{Hatch, MarkerShape, Rule, StrokeSymbolizer, Style, Symbolizer};
        let text = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[-20,0]}},
            {"type":"Feature","properties":{},"geometry":{"type":"MultiPoint","coordinates":[[0,0],[20,0]]}}
        ]}"#;
        let marker = |fill, stroke| {
            layer_shapes(text, FORMAT, |style| {
                style.style = Style {
                    rules: vec![Rule::new(
                        "markers",
                        vec![Symbolizer::Marker {
                            radius: Property::constant(10.0),
                            shape: MarkerShape::Square,
                            fill: Some(Property::constant(fill)),
                            hatch: Some(Hatch {
                                cross: true,
                                angle: 45.0,
                                spacing: 3.0,
                                line_width: 1.0,
                            }),
                            stroke: Some(StrokeSymbolizer::constant(Stroke::solid(stroke, 2.0))),
                        }],
                    )],
                    mask: None,
                };
            })
        };
        let translucent = marker(Color::rgba(255, 0, 0, 128), Color::rgba(0, 0, 255, 128));
        assert_eq!(translucent.len(), 2, "{translucent:?}");
        assert_eq!(callbacks(&translucent), 2);
        // Opaque markers draw all hatch lines, then the three strokes.
        let opaque = marker(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        assert_eq!(callbacks(&opaque), 0);
        let strokes = opaque
            .iter()
            .rposition(|s| !matches!(s, egui::Shape::Path(p) if p.closed))
            .unwrap();
        assert_eq!(opaque.len() - strokes - 1, 3);
        assert!(
            opaque[..=strokes]
                .iter()
                .all(|s| matches!(s, egui::Shape::LineSegment { .. }))
        );
    }
}
