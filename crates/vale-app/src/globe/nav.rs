//! Globe navigation: fingers, a mouse, and a trackpad move the view.

use std::collections::BTreeMap;

use eframe::egui::{self, Pos2, Rect, Vec2};

use super::view::Camera;

/// The zoom factor of one point of wheel scroll, as an exponent.
const WHEEL_ZOOM: f64 = 0.002;

/// The longest tap, from the first finger down to the last finger up, in
/// seconds.
const TAP_SECONDS: f64 = 0.3;

/// The largest move of a finger in a tap, in points.
const TAP_SLOP: f32 = 10.0;

struct Finger {
    pos: Pos2,
    prev: Pos2,
    /// The place where the finger came down.
    start: Pos2,
}

/// A tap of more than one finger on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    Two,
    Three,
}

/// The fingers that are down can still be a tap.
struct Touching {
    /// The time when the first finger came down.
    start: f64,
    /// The largest number of fingers that were down together.
    count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Rotate,
    Twist,
}

/// The input that the brush takes and that does not move the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Painting {
    /// A pen paints.
    pub pen: bool,
    /// The primary mouse button paints.
    pub mouse: bool,
}

/// True if a screen position is on the canvas, with no other layer above.
pub fn on_canvas(ui: &egui::Ui, rect: Rect, pos: Pos2) -> bool {
    let layer = ui.layer_id();
    rect.contains(pos) && ui.ctx().layer_id_at(pos).is_none_or(|top| top == layer)
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
    painting: Painting,
    /// The touches that were a palm.
    pub palms: u32,
    /// The time of the input, in seconds.
    time: f64,
    touching: Option<Touching>,
    tap: Option<Tap>,
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

    /// True while a finger or a pen is down on the canvas.
    pub fn touching(&self) -> bool {
        !self.fingers.is_empty() || self.pen.is_some()
    }

    /// The number of pens and the number of fingers that are down on the
    /// canvas.
    pub fn counts(&self) -> (usize, usize) {
        let fingers = self.fingers.keys().filter(|id| Some(**id) != self.pen);
        (usize::from(self.pen.is_some()), fingers.count())
    }

    /// Records one touch event. A touch that starts off the canvas is not a finger
    /// of the globe. `pen` is true for a touch with a force. A pen that does not
    /// paint moves the globe as one finger does.
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
                if on_canvas && self.pen.is_some() {
                    self.palms += 1;
                }
                if !on_canvas || self.pen.is_some() {
                    return;
                }
                if pen {
                    self.pen = Some(id);
                    self.palms += self.fingers.len() as u32;
                    self.fingers.clear();
                    self.touching = None;
                    if self.painting.pen {
                        return;
                    }
                } else if self.fingers.is_empty() {
                    let (start, count) = (self.time, 0);
                    self.touching = Some(Touching { start, count });
                }
                let (prev, start) = (pos, pos);
                self.fingers.insert(id, Finger { pos, prev, start });
                if let Some(touching) = &mut self.touching {
                    touching.count = touching.count.max(self.fingers.len());
                }
            }
            egui::TouchPhase::Move => {
                if let Some(finger) = self.fingers.get_mut(&id) {
                    finger.pos = pos;
                    if (pos - finger.start).length() > TAP_SLOP {
                        self.touching = None;
                    }
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                self.fingers.remove(&id);
                if self.pen == Some(id) {
                    self.pen = None;
                }
                if phase == egui::TouchPhase::Cancel {
                    self.touching = None;
                }
                if self.fingers.is_empty()
                    && let Some(touching) = self.touching.take()
                    && self.time - touching.start <= TAP_SECONDS
                {
                    self.tap = match touching.count {
                        2 => Some(Tap::Two),
                        3 => Some(Tap::Three),
                        _ => None,
                    };
                }
            }
        }
    }

    /// Takes the tap that ended after the last call.
    pub fn take_tap(&mut self) -> Option<Tap> {
        self.tap.take()
    }

    /// True while two or more fingers can still be a tap. The view does not
    /// move in that time. If the fingers are not a tap, the view then moves
    /// to them in one step.
    fn waits_for_tap(&self) -> bool {
        self.touching
            .as_ref()
            .is_some_and(|t| t.count >= 2 && self.time - t.start <= TAP_SECONDS)
    }

    /// Moves the view with the fingers. One finger rotates. Two fingers rotate,
    /// zoom, and twist, and the place under their middle stays there.
    pub fn move_fingers(&mut self, view: &mut impl Camera, rect: Rect) {
        if self.waits_for_tap() {
            return;
        }
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
    fn pointer(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        resp: &egui::Response,
        view: &mut impl Camera,
    ) {
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
        let paints = self.painting.mouse;
        if pressed && resp.contains_pointer() && !paints {
            self.drag = Some(if shift { Drag::Twist } else { Drag::Rotate });
        }
        match (self.drag, pos) {
            (Some(drag), Some(pos)) if down && !paints => {
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

    /// Reads the input of one frame and moves the view. The input in
    /// `painting` belongs to the brush.
    pub fn update(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        resp: &egui::Response,
        view: &mut impl Camera,
        painting: Painting,
    ) {
        self.painting = painting;
        self.time = ui.input(|i| i.time);
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
                    let on_canvas = on_canvas(ui, rect, *pos);
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
        // while a finger or a pen is down.
        if touched || self.touching() {
            self.drag = None;
            self.move_fingers(view, rect);
        } else {
            self.pointer(ui, rect, resp, view);
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::TouchPhase::{Cancel, End, Move, Start};

    use super::*;
    use crate::globe::math::angle;
    use crate::globe::view::GlobeView;

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
        // The finger before the pen and the finger after it are palms.
        assert_eq!(nav.palms, 2);
        assert_eq!(nav.counts(), (1, 0));
        let after = view.project(rect(), place).unwrap();
        assert!((after - to).length() < 0.01, "{after:?}");
    }

    #[test]
    fn a_pen_that_paints_does_not_move_the_globe() {
        let mut nav = Nav {
            painting: Painting {
                pen: true,
                mouse: false,
            },
            ..Nav::default()
        };
        let mut view = GlobeView::centered(15.0, 25.0);
        let before = view;
        nav.touch(1, Start, Pos2::new(300.0, 250.0), true, true);
        // A palm.
        nav.touch(2, Start, Pos2::new(500.0, 400.0), true, false);
        nav.touch(1, Move, Pos2::new(380.0, 330.0), true, true);
        nav.touch(2, Move, Pos2::new(520.0, 300.0), true, false);
        nav.move_fingers(&mut view, rect());
        assert_eq!(view, before);
        assert!(nav.touching() && !nav.active());
    }

    /// Puts `count` fingers down, and lifts them after `seconds`.
    fn tap(nav: &mut Nav, count: u64, seconds: f64) -> Option<Tap> {
        let pos = |id: u64| Pos2::new(300.0 + 60.0 * id as f32, 250.0);
        for id in 0..count {
            nav.touch(id, Start, pos(id), true, false);
        }
        nav.time += seconds;
        for id in 0..count {
            nav.touch(id, End, pos(id), true, false);
        }
        nav.take_tap()
    }

    #[test]
    fn a_short_tap_of_two_or_three_fingers_is_a_tap() {
        let mut nav = Nav::default();
        assert_eq!(tap(&mut nav, 1, 0.1), None);
        assert_eq!(tap(&mut nav, 2, 0.1), Some(Tap::Two));
        assert_eq!(nav.take_tap(), None);
        assert_eq!(tap(&mut nav, 3, 0.1), Some(Tap::Three));
        assert_eq!(tap(&mut nav, 4, 0.1), None);
        assert_eq!(tap(&mut nav, 2, TAP_SECONDS + 0.1), None);
        assert!(!nav.active());
    }

    #[test]
    fn fingers_that_move_are_not_a_tap() {
        let mut nav = Nav::default();
        let (a, b) = (Pos2::new(300.0, 250.0), Pos2::new(400.0, 250.0));
        nav.touch(1, Start, a, true, false);
        nav.touch(2, Start, b, true, false);
        nav.touch(2, Move, b + Vec2::new(TAP_SLOP + 1.0, 0.0), true, false);
        // The finger comes back, and the fingers are still not a tap.
        nav.touch(2, Move, b, true, false);
        nav.touch(1, End, a, true, false);
        nav.touch(2, End, b, true, false);
        assert_eq!(nav.take_tap(), None);
    }

    #[test]
    fn fingers_next_to_a_pen_are_not_a_tap() {
        let mut nav = Nav::default();
        let (a, b) = (Pos2::new(300.0, 250.0), Pos2::new(400.0, 250.0));
        nav.touch(1, Start, a, true, false);
        nav.touch(2, Start, b, true, false);
        nav.touch(3, Start, Pos2::new(350.0, 300.0), true, true);
        for id in 1..=3 {
            nav.touch(id, End, a, true, false);
        }
        assert_eq!(nav.take_tap(), None);
        // A touch that iOS cancels is not a tap.
        nav.touch(1, Start, a, true, false);
        nav.touch(2, Start, b, true, false);
        nav.touch(1, Cancel, a, true, false);
        nav.touch(2, End, b, true, false);
        assert_eq!(nav.take_tap(), None);
    }

    #[test]
    fn the_view_waits_while_two_fingers_can_be_a_tap() {
        let mut nav = Nav::default();
        let mut view = GlobeView::centered(15.0, 25.0);
        let before = view;
        let (a, b) = (Pos2::new(300.0, 250.0), Pos2::new(400.0, 250.0));
        let place = view.unproject(rect(), a).unwrap();
        nav.touch(1, Start, a, true, false);
        nav.touch(2, Start, b, true, false);
        let small = Vec2::new(TAP_SLOP - 2.0, 0.0);
        nav.touch(1, Move, a + small, true, false);
        nav.touch(2, Move, b + small, true, false);
        nav.move_fingers(&mut view, rect());
        assert_eq!(view, before);
        // The fingers stay down, so they are not a tap. The view moves to them.
        nav.time += TAP_SECONDS + 0.1;
        nav.move_fingers(&mut view, rect());
        let after = view.project(rect(), place).unwrap();
        assert!((after - (a + small)).length() < 1.0, "{after:?}");
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
