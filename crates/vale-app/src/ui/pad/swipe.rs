//! The swipe of a row of a list to the left, which shows the Delete button
//! of the row.

use std::hash::Hash;

use eframe::egui::{self, Pos2, Rect, Response, vec2};

use super::widgets;

/// The time that a row takes to snap open or shut, in seconds.
const SNAP_TIME: f32 = 0.15;

/// A row of a list that a finger moved to the left, to show its Delete
/// button. `K` identifies the row.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Swipe<K> {
    pub row: K,
    /// The distance of the row from its place, zero or less.
    pub offset: f32,
    /// The offset at the start of the drag.
    base: f32,
    /// The row moves with the finger. `false`: the row stays open.
    pub moving: bool,
}

/// Shuts the open row when a press lands outside it. `place` gives the
/// place of a row, or `None` for a row that is gone.
pub fn shut_outside<K: Copy>(
    ui: &egui::Ui,
    swipe: &mut Option<Swipe<K>>,
    place: impl Fn(K) -> Option<Rect>,
) {
    let Some(open) = *swipe else {
        return;
    };
    let at = place(open.row);
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.press_origin()));
    let inside = |p: Pos2| at.is_some_and(|r| r.expand2(vec2(8.0, 0.0)).contains(p));
    if at.is_none() || press.is_some_and(|p| !p.is_some_and(inside)) {
        *swipe = None;
    }
}

/// The distance of row `row` from its place in this frame. The row moves
/// with the finger, and it snaps open or shut.
pub fn shift<K: Copy + PartialEq + Hash + std::fmt::Debug>(
    ui: &egui::Ui,
    swipe: Option<Swipe<K>>,
    row: K,
) -> f32 {
    let swipe = swipe.filter(|s| s.row == row);
    let (target, moving) = swipe.map_or((0.0, false), |s| (s.offset, s.moving));
    let time = if moving { 0.0 } else { SNAP_TIME };
    let anim = ui.id().with(("swipe", row));
    ui.ctx().animate_value_with_time(anim, target, time)
}

/// Follows a sideways drag of the body of row `row`. A drag that starts
/// sideways moves the row, and the row opens or shuts when the finger lifts.
/// Returns true if the row moves with the finger, so that the body takes no
/// other touch.
pub fn drag<K: Copy + PartialEq>(
    ui: &egui::Ui,
    swipe: &mut Option<Swipe<K>>,
    row: K,
    body: &Response,
) -> bool {
    let moved = ui.input(|i| Some(i.pointer.interact_pos()? - i.pointer.press_origin()?));
    if body.drag_started() {
        let d = moved.unwrap_or_default();
        let base = swipe.filter(|s| s.row == row).map_or(0.0, |s| s.offset);
        *swipe = (d.x.abs() > d.y.abs()).then_some(Swipe {
            row,
            offset: base,
            base,
            moving: true,
        });
    }
    let Some(mut open) = swipe.filter(|s| s.row == row && s.moving) else {
        return false;
    };
    if body.dragged() {
        let dx = moved.map_or(0.0, |d| d.x);
        open.offset = (open.base + dx).clamp(-widgets::DELETE_WIDTH, 0.0);
        *swipe = Some(open);
    } else {
        // The finger is up: the row opens or shuts.
        let opened = open.offset < -widgets::DELETE_WIDTH / 2.0;
        *swipe = opened.then_some(Swipe {
            offset: -widgets::DELETE_WIDTH,
            moving: false,
            ..open
        });
    }
    ui.ctx().request_repaint();
    true
}
