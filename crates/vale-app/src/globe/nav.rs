//! Globe navigation: fingers, a mouse, and a trackpad move the view.

use std::collections::BTreeMap;

use eframe::egui::{self, Pos2, Rect, Vec2};

use super::view::GlobeView;

/// The zoom factor of one point of wheel scroll, as an exponent.
const WHEEL_ZOOM: f64 = 0.002;

struct Finger {
    pos: Pos2,
    prev: Pos2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Rotate,
    Twist,
}

/// The input state of the globe between frames.
#[derive(Default)]
pub struct Nav {
    /// The fingers that came down on the canvas.
    fingers: BTreeMap<u64, Finger>,
    /// The pen that is down. Other touches are a palm while it is down.
    pen: Option<u64>,
    drag: Option<Drag>,
    /// The last scroll came from a trackpad and not from a wheel.
    trackpad: bool,
}

/// The angle from `a` to `b` in radians, from -π to π. Screen y points down,
/// so a positive angle is clockwise on screen.
fn turn(a: Vec2, b: Vec2) -> f64 {
    let mut d = f64::from(b.angle() - a.angle());
    if d > std::f64::consts::PI {
        d -= std::f64::consts::TAU;
    } else if d < -std::f64::consts::PI {
        d += std::f64::consts::TAU;
    }
    d
}

impl Nav {
    /// True while a finger or a mouse button holds the globe.
    pub fn active(&self) -> bool {
        !self.fingers.is_empty() || self.drag.is_some()
    }

    /// Records one touch event. A touch that starts off the canvas is not a finger
    /// of the globe. `pen` is true for a touch with a force, and a pen moves the
    /// globe as one finger does.
    pub fn touch(
        &mut self,
        id: u64,
        phase: egui::TouchPhase,
        pos: Pos2,
        on_canvas: bool,
        pen: bool,
    ) {
        match phase {
            egui::TouchPhase::Start => {
                if !on_canvas || self.pen.is_some() {
                    return;
                }
                if pen {
                    self.pen = Some(id);
                    self.fingers.clear();
                }
                self.fingers.insert(id, Finger { pos, prev: pos });
            }
            egui::TouchPhase::Move => {
                if let Some(finger) = self.fingers.get_mut(&id) {
                    finger.pos = pos;
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                self.fingers.remove(&id);
                if self.pen == Some(id) {
                    self.pen = None;
                }
            }
        }
    }

    /// Moves the view with the fingers. One finger rotates. Two fingers rotate,
    /// zoom, and twist, and the place under their middle stays there.
    pub fn move_fingers(&mut self, view: &mut GlobeView, rect: Rect) {
        let mut fingers = self.fingers.values();
        match (fingers.next(), fingers.next()) {
            (Some(a), None) => {
                if a.prev != a.pos {
                    view.gesture(rect, a.prev, a.pos, 1.0, 0.0);
                }
            }
            (Some(a), Some(b)) => {
                let (old, new) = (b.prev - a.prev, b.pos - a.pos);
                if old.length() > 1.0 && new.length() > 1.0 {
                    let scale = f64::from(new.length() / old.length());
                    let from = a.prev + old * 0.5;
                    let to = a.pos + new * 0.5;
                    view.gesture(rect, from, to, scale, -turn(old, new));
                }
            }
            _ => {}
        }
        for finger in self.fingers.values_mut() {
            finger.prev = finger.pos;
        }
    }

    /// Moves the view with the mouse and the trackpad.
    ///
    /// A drag with any button rotates. A drag with Shift twists about the middle
    /// of the canvas. The wheel zooms. On a trackpad, a scroll with two fingers
    /// rotates, a pinch zooms, and the rotate gesture twists.
    fn pointer(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response, view: &mut GlobeView) {
        let (pos, pressed, down, shift, delta, scroll, pinch, rotation) = ui.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.any_pressed(),
                i.pointer.any_down(),
                i.modifiers.shift,
                i.pointer.delta(),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.rotation_delta(),
            )
        });
        if pressed && resp.contains_pointer() {
            self.drag = Some(if shift { Drag::Twist } else { Drag::Rotate });
        }
        match (self.drag, pos) {
            (Some(drag), Some(pos)) if down => {
                if delta != Vec2::ZERO {
                    match drag {
                        Drag::Rotate => view.gesture(rect, pos - delta, pos, 1.0, 0.0),
                        Drag::Twist => {
                            let c = rect.center();
                            view.gesture(rect, c, c, 1.0, -turn(pos - delta - c, pos - c));
                        }
                    }
                }
            }
            _ => self.drag = None,
        }

        let Some(hover) = resp.hover_pos() else {
            return;
        };
        if scroll != Vec2::ZERO {
            if self.trackpad {
                view.gesture(rect, hover, hover + scroll, 1.0, 0.0);
            } else {
                // Shift turns a wheel scroll to the x axis.
                let factor = (f64::from(scroll.x + scroll.y) * WHEEL_ZOOM).exp();
                view.zoom_at(rect, hover, factor);
            }
        }
        if (pinch - 1.0).abs() > 1e-4 {
            view.zoom_at(rect, hover, f64::from(pinch));
        }
        if rotation != 0.0 {
            view.gesture(rect, hover, hover, 1.0, -f64::from(rotation));
        }
    }

    /// Reads the input of one frame and moves the view.
    pub fn update(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        resp: &egui::Response,
        view: &mut GlobeView,
    ) {
        let layer = ui.layer_id();
        let mut touched = false;
        let events = ui.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Touch {
                    id,
                    phase,
                    pos,
                    force,
                    ..
                } => {
                    touched = true;
                    let on_canvas = rect.contains(*pos)
                        && ui.ctx().layer_id_at(*pos).is_none_or(|top| top == layer);
                    self.touch(id.0, *phase, *pos, on_canvas, force.is_some());
                }
                // A trackpad scrolls in points. A wheel scrolls in lines or pages.
                egui::Event::MouseWheel { unit, .. } => {
                    self.trackpad = *unit == egui::MouseWheelUnit::Point;
                }
                _ => {}
            }
        }
        // The first finger is also the egui pointer, so the pointer path is off
        // while fingers are down.
        if touched || !self.fingers.is_empty() {
            self.drag = None;
            self.move_fingers(view, rect);
        } else {
            self.pointer(ui, rect, resp, view);
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::TouchPhase::{Move, Start};

    use super::*;
    use crate::globe::math::angle;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn one_finger_keeps_its_place() {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(15.0, 25.0);
        let (from, to) = (Pos2::new(300.0, 250.0), Pos2::new(380.0, 330.0));
        let place = view.unproject(rect(), from).unwrap();
        nav.touch(1, Start, from, true, false);
        nav.touch(1, Move, to, true, false);
        nav.move_fingers(&mut view, rect());
        let after = view.project(rect(), place).unwrap();
        assert!((after - to).length() < 0.01, "{after:?}");
    }

    #[test]
    fn a_touch_off_the_canvas_does_nothing() {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(15.0, 25.0);
        let before = view;
        nav.touch(1, Start, Pos2::new(300.0, 250.0), false, false);
        nav.touch(1, Move, Pos2::new(380.0, 330.0), false, false);
        nav.move_fingers(&mut view, rect());
        assert_eq!(view, before);
        assert!(!nav.active());
    }

    #[test]
    fn a_palm_does_not_pinch_while_the_pen_is_down() {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(15.0, 25.0);
        let (from, to) = (Pos2::new(300.0, 250.0), Pos2::new(380.0, 330.0));
        let place = view.unproject(rect(), from).unwrap();
        nav.touch(1, Start, Pos2::new(500.0, 400.0), true, false);
        nav.touch(2, Start, from, true, true);
        nav.touch(3, Start, Pos2::new(520.0, 420.0), true, false);
        nav.touch(2, Move, to, true, true);
        nav.touch(3, Move, Pos2::new(600.0, 300.0), true, false);
        nav.move_fingers(&mut view, rect());
        assert_eq!(view.zoom, 1.0);
        let after = view.project(rect(), place).unwrap();
        assert!((after - to).length() < 0.01, "{after:?}");
    }

    /// Two fingers move apart, turn, and shift. Returns how far the place under
    /// each finger ends from its finger, in points.
    fn pinch_and_twist(lat: f64) -> [f32; 2] {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(15.0, lat);
        let c = rect().center();
        let old = [c + Vec2::new(-40.0, 0.0), c + Vec2::new(40.0, 0.0)];
        // 1.5 times as far apart, turned by 30 degrees, and moved by 20, 10.
        let arm = Vec2::angled(30_f32.to_radians()) * 60.0;
        let shift = Vec2::new(20.0, 10.0);
        let new = [c - arm + shift, c + arm + shift];
        let places = old.map(|p| view.unproject(rect(), p).unwrap());
        for (id, pos) in old.into_iter().enumerate() {
            nav.touch(id as u64, Start, pos, true, false);
        }
        for (id, pos) in new.into_iter().enumerate() {
            nav.touch(id as u64, Move, pos, true, false);
        }
        nav.move_fingers(&mut view, rect());
        assert!((view.zoom - 1.5).abs() < 1e-6, "{}", view.zoom);
        let middle = view.project(rect(), places[0]).unwrap().to_vec2()
            + view.project(rect(), places[1]).unwrap().to_vec2();
        assert!((middle * 0.5 - (c + shift).to_vec2()).length() < 0.5);
        [0, 1].map(|i| (view.project(rect(), places[i]).unwrap() - new[i]).length())
    }

    #[test]
    fn two_fingers_keep_their_places() {
        for lat in [25.0, 0.0, -60.0] {
            let off = pinch_and_twist(lat);
            assert!(off[0] < 1.0 && off[1] < 1.0, "lat {lat}: {off:?}");
        }
    }

    #[test]
    fn two_fingers_keep_their_places_near_the_poles() {
        for lat in [89.0, 90.0, -89.5, -90.0] {
            let off = pinch_and_twist(lat);
            assert!(off[0] < 1.0 && off[1] < 1.0, "lat {lat}: {off:?}");
        }
    }

    #[test]
    fn a_twist_turns_the_globe_with_the_fingers() {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(0.0, 0.0);
        let c = rect().center();
        // The fingers turn clockwise on screen by a quarter turn about the middle.
        nav.touch(1, Start, c + Vec2::new(-50.0, 0.0), true, false);
        nav.touch(2, Start, c + Vec2::new(50.0, 0.0), true, false);
        nav.touch(1, Move, c + Vec2::new(0.0, -50.0), true, false);
        nav.touch(2, Move, c + Vec2::new(0.0, 50.0), true, false);
        nav.move_fingers(&mut view, rect());
        // North was up. Now it points to the right.
        let north = view.rot.mul_vec([0.0, 0.0, 1.0]);
        assert!(angle(north, [1.0, 0.0, 0.0]) < 1e-6, "{north:?}");
    }
}
