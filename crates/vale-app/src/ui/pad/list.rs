//! The list of rows of the iPad layout. Each list draws through [`show`],
//! and it turns on only the gestures that its data supports:
//!
//! - A swipe to the right selects a row or deselects it ([`Spec::select`]).
//! - A swipe to the left shows the Rename and Delete buttons of a row
//!   ([`Row::rename`], [`Row::delete`]).
//! - A hold lifts a row, and the row moves with the finger to a new place
//!   ([`Spec::reorder`]).
//!
//! A drag that no gesture takes scrolls the list. The caller draws the
//! content of each row, or uses [`draw`].

use std::hash::Hash;

use eframe::egui::{self, Align2, Rect, Response, pos2, vec2};

use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::motion;
use super::theme;
use super::widgets::{self, Controls, DELETE_WIDTH, HIGHLIGHT_INSET, ICON, PANEL_ROW, RENAME};

pub use super::selectable::{DESELECT_ALL, SELECT_ALL};

/// The time that a finger holds a row before the row lifts, in seconds.
pub const HOLD_TIME: f64 = 0.4;
/// The largest distance that a finger moves during the hold, in points.
pub const HOLD_SLOP: f32 = 6.0;
/// The distance that a swipe to the right moves a row, at most.
pub const SWIPE_SELECT: f32 = 56.0;
/// The band at the top and the bottom of the visible list in which a
/// lifted row scrolls the list.
const EDGE: f32 = 40.0;
/// The scroll of one frame with the finger at the edge, in points.
const EDGE_SPEED: f32 = 12.0;
/// The name of the eye of [`draw`] in [`Trail::Control`].
pub const EYE: &str = "eye";

/// A row that a swipe to the left opened or opens.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Open<K> {
    pub row: K,
    /// The distance of the row from its place, zero or less.
    pub offset: f32,
    /// The offset at the start of the drag.
    base: f32,
    /// The row moves with the finger. `false`: the row stays open.
    pub moving: bool,
}

/// A row that moves with a finger.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Lift<K> {
    pub key: K,
    /// The place of the row in the list when it lifted.
    pub from: usize,
    /// The distance from the top of the row to the finger.
    grab: f32,
}

/// The state of a list. `K` identifies a row.
#[derive(Clone, Debug, PartialEq)]
pub struct ListState<K> {
    /// The row that a swipe to the left opened or opens.
    pub open: Option<Open<K>>,
    /// The row that a swipe to the right moves, and its distance.
    pub mark: Option<(K, f32)>,
    /// The selected rows, in the order of the list.
    pub picked: Vec<K>,
    pub lift: Option<Lift<K>>,
    /// The row that the user renames, and the text of its name field.
    pub rename: Option<(K, String)>,
}

impl<K> Default for ListState<K> {
    fn default() -> Self {
        ListState {
            open: None,
            mark: None,
            picked: Vec::new(),
            lift: None,
            rename: None,
        }
    }
}

impl<K: Copy + PartialEq> ListState<K> {
    pub fn is_picked(&self, key: K) -> bool {
        self.picked.contains(&key)
    }

    /// Ends each gesture, and keeps the selected rows.
    pub fn shut(&mut self) {
        self.open = None;
        self.mark = None;
        self.lift = None;
        self.rename = None;
    }
}

/// The gestures of a list.
#[derive(Copy, Clone)]
pub struct Spec<'a> {
    /// A swipe to the right selects a row.
    pub select: bool,
    /// A hold lifts a row.
    pub reorder: bool,
    /// The name of the name field of a row that the user renames.
    pub field: &'a str,
    /// True for a name that a rename applies.
    pub valid: &'a dyn Fn(&str) -> bool,
}

/// True for a name with a character that is not a space.
pub fn not_blank(name: &str) -> bool {
    !name.trim().is_empty()
}

impl Default for Spec<'_> {
    fn default() -> Self {
        Spec {
            select: false,
            reorder: false,
            field: "Name",
            valid: &not_blank,
        }
    }
}

/// A row of a list.
#[derive(Clone, Debug)]
pub struct Row<K> {
    pub key: K,
    /// The name of the control of the row.
    pub label: String,
    /// The text that Rename edits. The buttons behind the row have this
    /// name after their own.
    pub name: String,
    /// The row has the highlight of a chosen row.
    pub on: bool,
    pub rename: bool,
    pub delete: bool,
    /// The names of more buttons behind the row, at the left of Rename.
    pub actions: &'static [&'static str],
    /// A swipe to the right selects the row, in a list with
    /// [`Spec::select`].
    pub select: bool,
    /// The width at the right of the row that the controls of the caller
    /// take, from the right edge of the highlight.
    pub trailing: f32,
    /// The rows of a list with [`Spec::reorder`] are `PANEL_ROW` tall.
    pub height: f32,
}

impl<K> Row<K> {
    /// A row with no button behind it and no trailing control.
    pub fn new(key: K, name: impl Into<String>) -> Self {
        let name = name.into();
        Row {
            key,
            label: name.clone(),
            name,
            on: false,
            rename: false,
            delete: false,
            actions: &[],
            select: true,
            trailing: 0.0,
            height: PANEL_ROW,
        }
    }

    /// The width of the buttons behind the row.
    fn behind(&self) -> f32 {
        self.buttons() as f32 * DELETE_WIDTH
    }

    /// The number of buttons behind the row.
    fn buttons(&self) -> usize {
        usize::from(self.rename) + usize::from(self.delete) + self.actions.len()
    }
}

/// A row as the caller draws it.
pub struct Cell {
    /// The row at its place, before a swipe moves it.
    pub rect: Rect,
    /// A painter that leaves out the part of the row that a swipe to the
    /// left moved off.
    pub painter: egui::Painter,
    /// The move of the row by a swipe: less than zero to the left, more
    /// than zero to the right. The content moves only to the right.
    pub shift: f32,
    /// The controls of the row take touch.
    pub live: bool,
    pub lifted: bool,
    pub picked: bool,
    /// The place of the controls at the right of the row.
    pub trailing: Rect,
}

/// A control of the caller that the user tapped.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Trail {
    /// The arrow that opens the row.
    Open,
    /// Another control, by name.
    Control(&'static str),
}

/// What the user did with a list.
#[derive(Clone, Debug, PartialEq)]
pub enum Event<K> {
    Tap(K),
    Open(K),
    Control(K, &'static str),
    /// A hold lifted the row.
    Lift(K),
    Rename(K, String),
    Delete(K),
    /// A button of [`Row::actions`], by name.
    Action(K, &'static str),
    /// The lifted row went from one place to another.
    Move {
        key: K,
        from: usize,
        to: usize,
    },
}

fn lift_id() -> egui::Id {
    egui::Id::new("pad-list-lifted")
}

/// True if a list had a lifted row in this frame or in the last one. The
/// scroll area of the list does not scroll then.
pub fn lifted(ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_pass_nr();
    let at = ctx.data(|d| d.get_temp::<u64>(lift_id()));
    at.is_some_and(|at| at + 1 >= now)
}

fn set_lifted(ctx: &egui::Context) {
    let now = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(lift_id(), now));
}

/// The gesture that a sideways drag of a row starts.
#[derive(Copy, Clone, PartialEq)]
enum Claim {
    Open,
    Mark,
}

/// Draws the rows one below the other, each [`Row::height`] tall, and
/// follows the gestures of `spec`. `cell` draws the content of row `i` and returns the
/// control of the caller that the user tapped.
pub fn show<K>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &mut ListState<K>,
    spec: Spec,
    rows: &[Row<K>],
    cell: impl FnMut(&mut egui::Ui, &mut Controls, &Cell, usize) -> Option<Trail>,
) -> Vec<Event<K>>
where
    K: Copy + PartialEq + Hash + std::fmt::Debug + Send + Sync + 'static,
{
    // Two lists with the same keys in one parent have different IDs.
    let salt = (std::any::type_name::<K>(), spec.field);
    ui.push_id(salt, |ui| show_rows(ui, controls, state, spec, rows, cell))
        .inner
}

fn show_rows<K>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &mut ListState<K>,
    spec: Spec,
    rows: &[Row<K>],
    mut cell: impl FnMut(&mut egui::Ui, &mut Controls, &Cell, usize) -> Option<Trail>,
) -> Vec<Event<K>>
where
    K: Copy + PartialEq + Hash + std::fmt::Debug + Send + Sync + 'static,
{
    let mut events = Vec::new();
    let place_of = |key: K| rows.iter().position(|r| r.key == key);
    state.picked.retain(|k| place_of(*k).is_some());
    if !spec.select {
        state.picked.clear();
        state.mark = None;
    }
    if state.mark.is_some_and(|m| place_of(m.0).is_none()) {
        state.mark = None;
    }
    if state
        .rename
        .as_ref()
        .is_some_and(|r| place_of(r.0).is_none())
    {
        state.rename = None;
    }
    let tops: Vec<f32> = (rows.iter())
        .scan(0.0, |y, r| {
            *y += r.height;
            Some(*y - r.height)
        })
        .collect();
    let size = vec2(ui.available_width(), rows.iter().map(|r| r.height).sum());
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let slot =
        |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW));
    // Each row moves to its place on a spring, as after a reorder.
    let (list_id, ctx) = (ui.id(), ui.ctx().clone());
    let spring = |key: K, target: f32, height: f32| {
        let y = motion::points(&ctx, place_spring(list_id, key), target, motion::SNAPPY);
        Rect::from_min_size(
            pos2(area.left(), area.top() + y),
            vec2(area.width(), height),
        )
    };
    // A spring steps once in a frame, so each row asks for one place: its
    // place in the list, or its place while another row is lifted.
    let lift = state.lift.filter(|_| spec.reorder);
    let lift = lift.and_then(|l| Some((l, place_of(l.key)?)));
    let Some((lift, from)) = lift else {
        state.lift = None;
        let places: Vec<Rect> = (rows.iter().zip(&tops))
            .map(|(r, top)| spring(r.key, *top, r.height))
            .collect();
        let place = |i: usize| places[i];
        rows_live(
            ui,
            controls,
            state,
            spec,
            rows,
            place,
            &mut cell,
            &mut events,
        );
        if state.lift.is_some() {
            set_lifted(ui.ctx());
        }
        return events;
    };
    set_lifted(ui.ctx());
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
    let y = pointer.map_or(area.top() + tops[from] + lift.grab, |p| p.y);
    let top = (y - lift.grab).clamp(area.top(), area.bottom() - PANEL_ROW);
    let to = ((top - area.top()) / PANEL_ROW).round() as usize;
    let others = rows.iter().enumerate().filter(|(i, _)| *i != from);
    for (k, (i, row)) in others.enumerate() {
        // The rows make room at the place where the lifted row goes.
        let at = if k < to { k } else { k + 1 };
        let rect = spring(row.key, at as f32 * PANEL_ROW, PANEL_ROW);
        paint_still(ui, controls, state, row, rect, false, i, &mut cell);
    }
    motion::set(ui.ctx(), place_spring(list_id, lift.key), top - area.top());
    paint_still(
        ui,
        controls,
        state,
        &rows[from],
        slot(top),
        true,
        from,
        &mut cell,
    );
    if !down {
        state.clear_for_change();
        if to != from {
            events.push(Event::Move {
                key: lift.key,
                from,
                to,
            });
        }
        return events;
    }
    // A finger near the top or the bottom of the visible list scrolls it.
    if let Some(p) = pointer {
        let clip = ui.clip_rect();
        let up = EDGE - (p.y - clip.top());
        let down = EDGE - (clip.bottom() - p.y);
        let delta = if up > 0.0 && area.top() < clip.top() {
            EDGE_SPEED * (up / EDGE).min(1.0)
        } else if down > 0.0 && area.bottom() > clip.bottom() {
            -EDGE_SPEED * (down / EDGE).min(1.0)
        } else {
            0.0
        };
        if delta != 0.0 {
            let none = egui::style::ScrollAnimation::none();
            ui.scroll_with_delta_animation(vec2(0.0, delta), none);
        }
    }
    ui.ctx().request_repaint();
    events
}

/// A row that takes no touch, while another row is lifted.
#[allow(clippy::too_many_arguments)]
fn paint_still<K: Copy + PartialEq>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &ListState<K>,
    row: &Row<K>,
    rect: Rect,
    lifted: bool,
    i: usize,
    cell: &mut impl FnMut(&mut egui::Ui, &mut Controls, &Cell, usize) -> Option<Trail>,
) {
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    let painter = ui.painter().clone();
    if lifted {
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    let picked = state.is_picked(row.key);
    paint_back(&painter, highlight, picked, row.on || lifted);
    let at = Cell {
        rect,
        painter,
        shift: 0.0,
        live: false,
        lifted,
        picked,
        trailing: trailing_rect(rect, row.trailing),
    };
    cell(ui, controls, &at, i);
}

/// The tint of a selected row, or the highlight of a row that is chosen
/// or pressed.
fn paint_back(painter: &egui::Painter, highlight: Rect, picked: bool, raised: bool) {
    if picked {
        painter.rect_filled(highlight, 5.0, theme::ACCENT.gamma_multiply(0.25));
    } else if raised {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
}

fn trailing_rect(rect: Rect, width: f32) -> Rect {
    let edge = rect.right() + 8.0;
    Rect::from_min_max(pos2(edge - width, rect.top()), pos2(edge, rect.bottom()))
}

/// The rows that take touch.
#[allow(clippy::too_many_arguments)]
fn rows_live<K>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &mut ListState<K>,
    spec: Spec,
    rows: &[Row<K>],
    place: impl Fn(usize) -> Rect,
    cell: &mut impl FnMut(&mut egui::Ui, &mut Controls, &Cell, usize) -> Option<Trail>,
    events: &mut Vec<Event<K>>,
) where
    K: Copy + PartialEq + Hash + std::fmt::Debug + Send + Sync + 'static,
{
    shut_outside(ui, state, |key| {
        rows.iter().position(|r| r.key == key).map(&place)
    });
    let list_id = ui.id();
    for (i, row) in rows.iter().enumerate() {
        let (key, rect) = (row.key, place(i));
        if let Some((_, text)) = state.rename.as_mut().filter(|r| r.0 == key) {
            // The row is the name field. The name changes when the field
            // loses the focus.
            let valid = (spec.valid)(text);
            let field = ui
                .push_id(("list-field", key), |ui| {
                    let font = theme::body();
                    widgets::field_at(ui, controls, rect, spec.field, None, text, valid, font)
                })
                .inner;
            if field.lost_focus() {
                let text = state.rename.take().map(|r| r.1).unwrap_or_default();
                if (spec.valid)(&text) && text != row.name {
                    events.push(Event::Rename(key, text));
                }
            } else if !field.has_focus() {
                widgets::focus_all(ui, &field, &row.name);
            }
            continue;
        }
        let width = row.behind();
        let opened = state.open.filter(|o| o.row == key);
        let target = match (opened, state.mark) {
            (Some(o), _) => (o.offset, o.moving),
            (None, Some((m, dx))) if m == key => (dx, true),
            _ => (0.0, false),
        };
        let anim = shift_spring(list_id, key);
        let shift = if target.1 {
            motion::set(ui.ctx(), anim, target.0);
            target.0
        } else {
            motion::points(ui.ctx(), anim, target.0, motion::SNAPPY)
        };
        let out = ui
            .push_id(("list-row", key), |ui| {
                paint_row(
                    ui,
                    controls,
                    row,
                    rect,
                    shift,
                    state.is_picked(key),
                    i,
                    cell,
                )
            })
            .inner;
        if out.delete {
            events.push(Event::Delete(key));
            state.clear_for_change();
            continue;
        }
        if out.rename {
            state.open = None;
            state.rename = Some((key, row.name.clone()));
            continue;
        }
        if let Some(action) = out.action {
            state.open = None;
            events.push(Event::Action(key, action));
            continue;
        }
        if let Some(trail) = out.trail {
            state.open = None;
            events.push(match trail {
                Trail::Open => Event::Open(key),
                Trail::Control(name) => Event::Control(key, name),
            });
        }
        let body = &out.body;
        if follow(ui, state, spec, rows, row, body, width) {
            continue;
        }
        if body.clicked() {
            if opened.is_some() {
                state.open = None;
            } else {
                events.push(Event::Tap(key));
            }
        }
        if spec.reorder && hold(ui, state, body) {
            ui.ctx().set_dragged_id(body.id);
            state.lift = Some(Lift {
                key,
                from: i,
                grab: ui
                    .input(|i| i.pointer.interact_pos())
                    .map_or(0.0, |p| p.y - rect.top()),
            });
            state.open = None;
            state.picked.clear();
            events.push(Event::Lift(key));
            ui.ctx().request_repaint();
        }
    }
}

/// The spring of the sideways offset of the row `key`.
fn shift_spring<K: Hash + std::fmt::Debug>(list: egui::Id, key: K) -> egui::Id {
    list.with(("list-shift", key))
}

/// The spring of the place of the row `key` in the list.
fn place_spring<K: Hash + std::fmt::Debug>(list: egui::Id, key: K) -> egui::Id {
    list.with(("list-place", key))
}

impl<K> ListState<K> {
    /// Ends each gesture and the selection after a change that can give
    /// the rows new keys.
    fn clear_for_change(&mut self) {
        self.open = None;
        self.mark = None;
        self.lift = None;
        self.rename = None;
        self.picked.clear();
    }
}

/// Shuts the open row when a press lands outside it. `place` gives the
/// place of a row, or `None` for a row that is gone.
fn shut_outside<K: Copy>(
    ui: &egui::Ui,
    state: &mut ListState<K>,
    place: impl Fn(K) -> Option<Rect>,
) {
    let Some(open) = state.open else {
        return;
    };
    let at = place(open.row);
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.press_origin()));
    let inside = |p: egui::Pos2| at.is_some_and(|r| r.expand2(vec2(8.0, 0.0)).contains(p));
    if at.is_none() || press.is_some_and(|p| !p.is_some_and(inside)) {
        state.open = None;
    }
}

/// Follows a sideways drag of a row. A drag to the left opens the buttons
/// behind the row, and a drag to the right selects the row or deselects
/// it. Returns true while the row moves with the finger.
fn follow<K: Copy + PartialEq + Hash + std::fmt::Debug>(
    ui: &egui::Ui,
    state: &mut ListState<K>,
    spec: Spec,
    rows: &[Row<K>],
    row: &Row<K>,
    body: &Response,
    width: f32,
) -> bool {
    let key = row.key;
    let is_open = state.open.is_some_and(|o| o.row == key);
    let mut claim = None;
    let took = widgets::take_drag(ui, body, |m| {
        if !widgets::sideways(m) {
            return false;
        }
        claim = if width > 0.0 && (m.x < 0.0 || is_open) {
            Some(Claim::Open)
        } else if spec.select && row.select && m.x > 0.0 {
            Some(Claim::Mark)
        } else {
            None
        };
        claim.is_some()
    });
    let moved = ui.input(|i| Some(i.pointer.interact_pos()? - i.pointer.press_origin()?));
    let dx = moved.map_or(0.0, |d| d.x);
    match claim {
        Some(Claim::Open) => {
            let base = state
                .open
                .filter(|o| o.row == key)
                .map_or(0.0, |o| o.offset);
            state.mark = None;
            state.open = Some(Open {
                row: key,
                offset: base,
                base,
                moving: true,
            });
        }
        Some(Claim::Mark) => {
            state.open = None;
            state.mark = Some((key, 0.0));
        }
        None => {}
    }
    if let Some(mut open) = state.open.filter(|o| o.row == key && o.moving) {
        if body.dragged() || took {
            open.offset = (open.base + dx).clamp(-width, 0.0);
            state.open = Some(open);
        } else {
            // The finger is up: the row opens or shuts, and it springs on
            // with the speed of the finger. A swipe past half the buttons,
            // or past one button, opens the row.
            let opened = open.offset < -(width / 2.0).min(DELETE_WIDTH);
            let speed = ui.input(|i| i.pointer.velocity().x);
            motion::kick(ui.ctx(), shift_spring(ui.id(), key), open.offset, speed);
            state.open = opened.then_some(Open {
                offset: -width,
                moving: false,
                ..open
            });
        }
        ui.ctx().request_repaint();
        return true;
    }
    if let Some((_, shift)) = state.mark.filter(|m| m.0 == key) {
        if body.dragged() || took {
            state.mark = Some((key, dx.clamp(0.0, SWIPE_SELECT)));
        } else {
            // The finger is up: a swipe past the middle selects the row or
            // deselects it.
            if shift >= SWIPE_SELECT / 2.0 {
                if state.is_picked(key) {
                    state.picked.retain(|k| *k != key);
                } else {
                    state.picked.push(key);
                    let place = |k: &K| rows.iter().position(|r| r.key == *k);
                    state.picked.sort_by_key(place);
                }
            }
            state.mark = None;
        }
        ui.ctx().request_repaint();
        return true;
    }
    false
}

/// True when a press on `body` became a hold of `HOLD_TIME` in place,
/// with no row open.
fn hold<K>(ui: &egui::Ui, state: &ListState<K>, body: &Response) -> bool {
    if !body.is_pointer_button_down_on() || body.dragged() {
        return false;
    }
    if state.open.is_some() || state.mark.is_some() || state.lift.is_some() {
        return false;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    match press {
        Some((held, moved)) if moved <= HOLD_SLOP && held >= HOLD_TIME => true,
        Some((held, moved)) if moved <= HOLD_SLOP => {
            let wait = std::time::Duration::from_secs_f64(HOLD_TIME - held);
            ui.ctx().request_repaint_after(wait);
            false
        }
        _ => false,
    }
}

/// What the user did with one live row.
struct RowOut {
    body: Response,
    delete: bool,
    rename: bool,
    action: Option<&'static str>,
    trail: Option<Trail>,
}

/// A row that takes touch, moved `shift` by a swipe.
#[allow(clippy::too_many_arguments)]
fn paint_row<K>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    row: &Row<K>,
    rect: Rect,
    shift: f32,
    picked: bool,
    i: usize,
    cell: &mut impl FnMut(&mut egui::Ui, &mut Controls, &Cell, usize) -> Option<Trail>,
) -> RowOut {
    let edge = rect.right() + 8.0;
    let body_right = if row.trailing > 0.0 {
        edge - row.trailing
    } else {
        rect.right()
    };
    let right = body_right
        .min(edge + shift.min(0.0))
        .max(rect.left() + TOUCH);
    let body_rect = Rect::from_min_max(rect.min, pos2(right, rect.bottom()));
    let body = widgets::hit(
        ui,
        controls,
        body_rect,
        &row.label,
        egui::Sense::click(),
        true,
        Some(row.on || picked),
    );
    let (mut delete, mut rename, mut action) = (false, false, None);
    if shift < 0.0 {
        // Delete is at the right edge, then Rename, then the actions.
        let mut slot = 0;
        let count = row.buttons();
        if row.delete {
            let name = widgets::delete_name(&row.name);
            let last = slot + 1 == count;
            delete = behind(ui, controls, rect, shift, slot, last, "Delete", &name);
            slot += 1;
        }
        if row.rename {
            let name = format!("{RENAME} {}", row.name);
            let last = slot + 1 == count;
            rename = behind(ui, controls, rect, shift, slot, last, RENAME, &name);
            slot += 1;
        }
        for &text in row.actions {
            let name = format!("{text} {}", row.name);
            let last = slot + 1 == count;
            if behind(ui, controls, rect, shift, slot, last, text, &name) {
                action = Some(text);
            }
            slot += 1;
        }
    }
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    let left = rect.left() - 8.0;
    let clip = Rect::from_x_y_ranges(left..=edge + shift.min(0.0), rect.y_range());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(clip));
    let pressed = body.is_pointer_button_down_on();
    let highlight = highlight.translate(vec2(shift, 0.0));
    paint_back(&painter, highlight, picked, row.on || pressed);
    let at = Cell {
        rect,
        painter,
        shift,
        live: shift == 0.0,
        lifted: false,
        picked,
        trailing: trailing_rect(rect, row.trailing),
    };
    let trail = cell(ui, controls, &at, i);
    RowOut {
        body,
        delete,
        rename,
        action,
        trail,
    }
}

/// The space at the left of each button behind a row.
const BUTTON_GAP: f32 = 6.0;

/// A button behind a row that a swipe moves `shift` to the left. Slot 0 is
/// at the right edge. The `last` button grows to fill the open part of
/// the row. The button takes touch when the row is open past it. Returns
/// true if the user taps it.
#[allow(clippy::too_many_arguments)]
fn behind(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    shift: f32,
    slot: usize,
    last: bool,
    text: &str,
    name: &str,
) -> bool {
    let edge = rect.right() + 8.0;
    let right = edge - slot as f32 * DELETE_WIDTH;
    let reveal = edge + shift;
    if reveal >= right {
        return false;
    }
    let left = if last {
        reveal
    } else {
        reveal.max(right - DELETE_WIDTH)
    };
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    // The buttons have a gap between them and at the left of the first one.
    let back = Rect::from_x_y_ranges(left + BUTTON_GAP..=right, highlight.y_range());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(back));
    let color = match text {
        "Delete" => theme::WARN,
        RENAME => theme::MUTE,
        _ => theme::ACCENT,
    };
    painter.rect_filled(back, 5.0, color);
    let at = pos2(right - (DELETE_WIDTH - BUTTON_GAP) / 2.0, rect.center().y);
    painter.text(
        at,
        Align2::CENTER_CENTER,
        text,
        theme::body(),
        theme::CANVAS,
    );
    if -shift < (slot + 1) as f32 * DELETE_WIDTH - 0.5 {
        return false;
    }
    let area = Rect::from_min_max(
        pos2(right - DELETE_WIDTH, rect.center().y - TOUCH / 2.0),
        pos2(right, rect.center().y + TOUCH / 2.0),
    );
    widgets::tap_area(ui, controls, area, name, true).clicked()
}

/// The content of a row that [`draw`] paints.
#[derive(Clone, Copy, Default)]
pub struct Look<'a> {
    /// The icon at the left, such as the kind of a layer.
    pub icon: Option<Icon>,
    pub text: &'a str,
    /// A gray text at the right of the text.
    pub note: Option<&'a str>,
    /// The note is a warning, in the warning color.
    pub warn: bool,
    /// A check column at the left. `Some(true)`: the row has the check.
    pub check: Option<bool>,
    /// The eye that shows or hides the row, at the right.
    pub eye: Option<bool>,
    /// The arrow that opens the row, at the far right.
    pub open: bool,
}

impl Look<'_> {
    /// The width of the controls at the right, for [`Row::trailing`].
    pub fn trailing(&self) -> f32 {
        (usize::from(self.eye.is_some()) + usize::from(self.open)) as f32 * TOUCH
    }
}

/// Paints the content of a row: the check, the icon, the text, the note,
/// the eye, and the open arrow. The eye has the name "Hide <text>" or
/// "Show <text>", and the arrow "Open <text>".
pub fn draw(ui: &mut egui::Ui, controls: &mut Controls, cell: &Cell, look: &Look) -> Option<Trail> {
    let rect = cell.rect;
    let painter = &cell.painter;
    let moved = vec2(cell.shift.max(0.0), 0.0);
    let mut x = rect.left() + moved.x;
    if let Some(on) = look.check {
        let mark = Rect::from_center_size(pos2(x + 18.0, rect.center().y), vec2(18.0, 18.0));
        if on {
            icons::paint(painter, mark, Icon::Tick, 18.0, theme::ACCENT);
        }
        x += 38.0;
    }
    if let Some(icon) = look.icon {
        let kind = Rect::from_center_size(pos2(x + 10.0, rect.center().y), vec2(18.0, 18.0));
        icons::paint(painter, kind, icon, 18.0, theme::MUTE);
        x += 28.0;
    }
    let shown = look.eye != Some(false);
    let color = match look.check {
        Some(false) => theme::MUTE,
        _ => widgets::fade(theme::TEXT, shown),
    };
    let trailing = cell.trailing.translate(moved);
    let mut name_right = if look.trailing() > 0.0 {
        trailing.left() - 4.0
    } else {
        rect.right() + moved.x - 4.0
    };
    if let Some(note) = look.note {
        let color = if look.warn { theme::WARN } else { theme::MUTE };
        let galley = painter.layout_no_wrap(note.to_owned(), theme::body(), color);
        let width = galley.size().x.ceil();
        let place = Rect::from_min_max(
            pos2(name_right - width - 2.0, rect.top()),
            pos2(name_right, rect.bottom()),
        );
        name_right = place.left() - 8.0;
        // The note is a label, so a screen reader and a test can read it.
        let text = egui::RichText::new(note).font(theme::body()).color(color);
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        let builder = egui::UiBuilder::new().max_rect(place).layout(layout);
        let mut child = ui.new_child(builder);
        child.set_clip_rect(painter.clip_rect());
        child.add(egui::Label::new(text).selectable(false));
    }
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(name_right, rect.bottom()));
    painter
        .with_clip_rect(painter.clip_rect().intersect(clip))
        .text(
            clip.left_center(),
            Align2::LEFT_CENTER,
            look.text,
            theme::body(),
            color,
        );
    if cell.shift < 0.0 {
        return None;
    }
    let edge = trailing.right();
    let open_rect = Rect::from_center_size(
        pos2(edge - TOUCH / 2.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    let eye_rect = match look.open {
        true => open_rect.translate(vec2(-TOUCH, 0.0)),
        false => open_rect,
    };
    let eye_icon = if shown { Icon::Eye } else { Icon::EyeOff };
    let mut trail = None;
    if cell.live {
        if look.eye.is_some() {
            let name = widgets::eye_name(look.text, shown);
            let eye = widgets::icon_button_at(ui, controls, eye_rect, &name, eye_icon, false, true);
            if eye.clicked() {
                trail = Some(Trail::Control(EYE));
            }
        }
        if look.open {
            let name = format!("Open {}", look.text);
            let open =
                widgets::icon_button_at(ui, controls, open_rect, &name, Icon::Open, false, true);
            if open.clicked() {
                trail = Some(Trail::Open);
            }
        }
    } else {
        if look.eye.is_some() {
            icons::paint(painter, eye_rect, eye_icon, ICON, theme::MUTE);
        }
        if look.open {
            icons::paint(painter, open_rect, Icon::Open, ICON, theme::MUTE);
        }
    }
    trail
}

/// The buttons of the header of a list with `picked` of its `count` rows
/// selected: "Select all" while a row is not selected, the `extras`, the
/// `delete` button, and "Deselect all". `None`: no row is selected.
pub fn group_buttons(
    picked: usize,
    count: usize,
    extras: &[(&'static str, Icon)],
    delete: Option<&'static str>,
) -> Option<Vec<(&'static str, Icon)>> {
    if picked == 0 {
        return None;
    }
    let mut buttons = Vec::new();
    if picked < count {
        buttons.push((SELECT_ALL, Icon::Tick));
    }
    buttons.extend_from_slice(extras);
    if let Some(delete) = delete {
        buttons.push((delete, Icon::Trash));
    }
    buttons.push((DESELECT_ALL, Icon::Close));
    Some(buttons)
}

/// What a button of [`group_buttons`] asks the caller to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Group<K> {
    /// Delete these rows, in the order of the list.
    Delete(Vec<K>),
    /// The extra button with this name.
    Extra(&'static str),
}

/// Does the action of the button `name` of [`group_buttons`]. "Select all"
/// and "Deselect all" change the selection of `state`, and return `None`.
/// `keys` are the keys of the rows, in the order of the list.
pub fn group_action<K: Copy + PartialEq>(
    state: &mut ListState<K>,
    keys: &[K],
    name: &'static str,
    delete: Option<&str>,
) -> Option<Group<K>> {
    match name {
        SELECT_ALL => {
            state.picked = keys.to_vec();
            None
        }
        DESELECT_ALL => {
            state.picked.clear();
            None
        }
        _ if Some(name) == delete => {
            let picked = std::mem::take(&mut state.picked);
            state.clear_for_change();
            Some(Group::Delete(picked))
        }
        _ => Some(Group::Extra(name)),
    }
}

/// A list of choices with no gestures: a row for each choice, with the
/// check on the choices that are on. `choices` are the text of each row
/// and its check. A row has the name `label(text)`. Returns the index of
/// the row that the user tapped.
pub fn options(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    choices: &[(&str, bool)],
    label: impl Fn(&str) -> String,
) -> Option<usize> {
    let rows: Vec<Row<usize>> = (choices.iter().enumerate())
        .map(|(i, (text, on))| Row {
            label: label(text),
            on: *on,
            ..Row::new(i, *text)
        })
        .collect();
    let mut state = ListState::default();
    let events = show(
        ui,
        controls,
        &mut state,
        Spec::default(),
        &rows,
        |ui, c, cell, i| {
            let look = Look {
                text: choices[i].0,
                check: Some(choices[i].1),
                ..Look::default()
            };
            draw(ui, c, cell, &look)
        },
    );
    events.into_iter().find_map(|e| match e {
        Event::Tap(i) => Some(i),
        _ => None,
    })
}
