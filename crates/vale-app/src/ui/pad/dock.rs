//! The dock of the iPad layout from 1,000 points: an opaque column at the
//! right edge, below the bar. It holds the panels of the workspace from top
//! to bottom, each with a title row. The open panels share the height, and
//! a bar with a grip between two open panels moves the line between them.
//! The edge handle at the left edge changes the width, and a drag of it to
//! the right edge closes the dock.

use eframe::egui::{self, Rect, Sense, pos2, vec2};

use super::geometry::{Rects, TOUCH, WidthClass};
use super::theme;
use super::widgets::{self, Controls};
use super::{Panel, panel_face};
use crate::dock::{DockState, HEIGHT_MIN, WIDTH_MIN};
use crate::ui::AppState;

/// The height of the title row of a panel.
pub const TITLE: f32 = TOUCH;
/// The height of the bar between two open panels.
pub const BAR: f32 = 14.0;
/// A drag of the edge handle this far past the narrowest width closes the
/// dock, and a drag this far from the right edge opens it.
pub const EDGE_DRAG: f32 = 40.0;
/// The touch area of the edge handle of the open dock. Most of it is over
/// the canvas, so that it leaves the rows of the panels free.
const HANDLE: (f32, f32) = (44.0, 88.0);
/// The part of the touch area of the edge handle over the dock.
const HANDLE_IN: f32 = 14.0;
/// The width of the touch area at the right edge of the canvas that opens
/// the closed dock.
const OPEN_WIDTH: f32 = 32.0;
/// The time of the move of the dock, in seconds.
const MOVE_TIME: f32 = 0.25;

/// The control name of the bar below `panel`.
pub fn bar_name(panel: Panel) -> String {
    format!("Resize {}", panel_face(panel).0)
}

/// The control name of the title row of `panel` in the dock. A tap folds
/// the panel.
pub fn title_name(panel: Panel) -> String {
    format!("{} panel", panel_face(panel).0)
}

/// The place of a panel in the dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub panel: Panel,
    pub title: Rect,
    /// The body of an open panel. `None`: the panel is folded.
    pub body: Option<Rect>,
    /// The bar below the panel, if it has open panels above and below.
    pub bar_below: Option<Rect>,
}

impl Slot {
    /// The title row and the body.
    pub fn rect(&self) -> Rect {
        self.body.map_or(self.title, |body| self.title.union(body))
    }
}

/// The heights of the bodies of the open panels, which share `room`. Each
/// keeps its part of the stored heights, and is at least `HEIGHT_MIN`.
pub fn share(stored: &[f32], room: f32) -> Vec<f32> {
    let n = stored.len();
    if n == 0 {
        return Vec::new();
    }
    if room <= HEIGHT_MIN * n as f32 {
        return vec![HEIGHT_MIN; n];
    }
    let mut out: Vec<Option<f32>> = vec![None; n];
    loop {
        let fixed: f32 = out.iter().flatten().sum();
        let free: f32 = (0..n)
            .filter(|&i| out[i].is_none())
            .map(|i| stored[i].max(1.0))
            .sum();
        let scale = (room - fixed) / free;
        let mut changed = false;
        for i in 0..n {
            if out[i].is_none() && stored[i].max(1.0) * scale < HEIGHT_MIN {
                out[i] = Some(HEIGHT_MIN);
                changed = true;
            }
        }
        if !changed {
            return (0..n)
                .map(|i| out[i].unwrap_or(stored[i].max(1.0) * scale))
                .collect();
        }
    }
}

/// True if the panel at `i` has a bar below it: it is open, and an open
/// panel is below it.
fn has_bar(dock: &DockState, i: usize) -> bool {
    !dock.panels[i].folded && dock.panels[i + 1..].iter().any(|p| !p.folded)
}

/// The places of the panels of `dock` in `rect`, moved up by `scroll`, and
/// the height of all panels.
pub fn slots(dock: &DockState, rect: Rect, scroll: f32) -> (Vec<Slot>, f32) {
    let n = dock.panels.len();
    let bars = (0..n).filter(|&i| has_bar(dock, i)).count();
    let stored: Vec<f32> = dock
        .panels
        .iter()
        .filter(|p| !p.folded)
        .map(|p| p.height)
        .collect();
    let room = rect.height() - TITLE * n as f32 - BAR * bars as f32;
    let mut heights = share(&stored, room).into_iter();
    let mut y = rect.top() - scroll;
    let mut out = Vec::with_capacity(n);
    for (i, p) in dock.panels.iter().enumerate() {
        let title = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), TITLE));
        y = title.bottom();
        let body = (!p.folded).then(|| {
            let h = heights.next().unwrap_or(HEIGHT_MIN);
            let body = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), h));
            y = body.bottom();
            body
        });
        let bar_below = has_bar(dock, i).then(|| {
            let bar = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), BAR));
            y = bar.bottom();
            bar
        });
        out.push(Slot {
            panel: p.panel,
            title,
            body,
            bar_below,
        });
    }
    (out, y + scroll - rect.top())
}

/// Moves the line of the bar below the panel at `i` by `dy`. Every open
/// panel keeps the height that it shows, from `slots`, and the nearest open
/// panels above and below the bar share the move. Each stays at least
/// `HEIGHT_MIN` high.
pub fn drag_bar(dock: &mut DockState, slots: &[Slot], i: usize, dy: f32) {
    for (p, slot) in dock.panels.iter_mut().zip(slots) {
        if let Some(body) = slot.body {
            p.height = body.height();
        }
    }
    let above = (0..=i.min(dock.panels.len().saturating_sub(1)))
        .rev()
        .find(|&k| !dock.panels[k].folded);
    let below = (i + 1..dock.panels.len()).find(|&k| !dock.panels[k].folded);
    let (Some(a), Some(b)) = (above, below) else {
        return;
    };
    let total = dock.panels[a].height + dock.panels[b].height;
    let top = (dock.panels[a].height + dy).clamp(HEIGHT_MIN, (total - HEIGHT_MIN).max(HEIGHT_MIN));
    dock.panels[a].height = top;
    dock.panels[b].height = total - top;
}

/// A drag of an edge handle: the width at its start, and the width that
/// the finger asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeDrag {
    start: f32,
    asked: f32,
}

/// The places of the dock in the frame, and its drags.
#[derive(Clone, Debug, Default)]
pub struct DockFrame {
    /// The places of the panels in this frame.
    pub slots: Vec<Slot>,
    /// The part of the dock that holds the panels, in this frame.
    pub rect: Option<Rect>,
    /// The distance that the panels are moved up, when they do not fit.
    pub scroll: f32,
    edge: Option<EdgeDrag>,
    /// The touch area of the edge handle in this frame: at the left edge
    /// of the open dock, or at the right edge of the canvas while the dock
    /// is closed. It is not in the list of the controls, because it lies
    /// over the canvas and the floating cards.
    pub handle: Option<Rect>,
    /// The panel that `show_card` places next. A panel in the dock goes to
    /// its slot, and a floating card of it has the Dock button.
    pub placing: Option<Panel>,
    /// The panels that showed in their slots in this frame.
    pub drawn: Vec<Panel>,
}

impl DockFrame {
    /// The place of `panel` in the dock in this frame.
    pub fn slot(&self, panel: Panel) -> Option<Slot> {
        self.slots.iter().find(|s| s.panel == panel).copied()
    }

    /// A finger drags an edge handle.
    pub fn dragging(&self) -> bool {
        self.edge.is_some()
    }
}

/// ⌥⌘I opens and closes the dock, from 1,000 points, while no text field
/// has the keyboard.
pub fn shortcut(ctx: &egui::Context, state: &mut AppState) {
    if !state.pad.dockable || ctx.egui_wants_keyboard_input() {
        return;
    }
    let keys = egui::KeyboardShortcut::new(
        egui::Modifiers::ALT | egui::Modifiers::COMMAND,
        egui::Key::I,
    );
    if ctx.input_mut(|i| i.consume_shortcut(&keys)) {
        state.pad.toggle_dock();
    }
}

/// The width of the dock for the layout. It moves to a new width in
/// `MOVE_TIME`, and at once in a run with no window and during a drag.
pub fn layout_width(ctx: &egui::Context, state: &AppState) -> f32 {
    let target = state.pad.open_dock().map_or(0.0, |dock| dock.width);
    let still = state.headless || state.pad.dock_frame.dragging();
    let time = if still { 0.0 } else { MOVE_TIME };
    let id = egui::Id::new(("pad-dock-width", state.workspace as u8));
    ctx.animate_value_with_time(id, target, time)
}

/// Shows an opaque part of the dock at `rect`, cut to `clip`, with the
/// border at the left edge of `clip`.
pub fn piece(
    ctx: &egui::Context,
    id: egui::Id,
    rect: Rect,
    clip: Rect,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let shown = rect.intersect(clip);
    if !shown.is_positive() {
        return;
    }
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(shown.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .sizing_pass(false)
        .show(ctx, |ui| {
            let (shown, _) = ui.allocate_exact_size(shown.size(), Sense::click_and_drag());
            ui.painter().rect_filled(shown, 0.0, theme::dock());
            let x = clip.left() + 0.5;
            ui.painter().line_segment(
                [pos2(x, shown.top()), pos2(x, shown.bottom())],
                egui::Stroke::new(1.0, theme::edge()),
            );
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
            ui.set_clip_rect(shown);
            theme::apply(&mut ui);
            add_contents(&mut ui);
        });
}

/// The line at the top of a panel of the dock that has no bar above it.
pub fn top_line(ui: &egui::Ui, rect: Rect) {
    ui.painter().line_segment(
        [
            pos2(rect.left(), rect.top() + 0.5),
            pos2(rect.right(), rect.top() + 0.5),
        ],
        egui::Stroke::new(1.0, theme::edge()),
    );
}

/// Lays out the dock and draws its bars, its empty space, and the edge
/// handle. The panels draw themselves at their slots after this, and
/// `finish` fills the slots that no panel drew.
pub fn draw(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let frame = &mut state.pad.dock_frame;
    frame.slots.clear();
    frame.rect = None;
    frame.placing = None;
    frame.drawn.clear();
    frame.handle = None;
    if rects.class != WidthClass::Wide {
        frame.edge = None;
        return;
    }
    let workspace = state.workspace;
    let Some(area) = rects.dock else {
        if !state.pad.docks.get(workspace).open {
            open_handle(ctx, state, rects);
        }
        return;
    };
    let open = state.pad.docks.get(workspace).open;
    // The panels keep the width of the dock while it moves in and out.
    let width = state.pad.docks.get(workspace).width.max(area.width());
    let bottom = rects.screen.bottom() - rects.bands.bottom;
    let content = Rect::from_min_size(area.min, vec2(width, bottom - area.top()));
    let content = Rect::from_min_max(
        content.min,
        pos2(
            content
                .right()
                .min(rects.screen.right() - rects.bands.right),
            content.bottom(),
        ),
    );
    let clip = Rect::from_min_max(area.min, pos2(area.right(), rects.screen.bottom()));
    let mut end = area.top();
    if open {
        let dock = state.pad.docks.get(workspace);
        let (_, height) = slots(dock, content, 0.0);
        let most = (height - content.height()).max(0.0);
        let scroll = state.pad.dock_frame.scroll.clamp(0.0, most);
        let (list, _) = slots(dock, content, scroll);
        state.pad.dock_frame.scroll = scroll;
        state.pad.dock_frame.rect = Some(content);
        for (i, slot) in list.iter().enumerate() {
            end = slot.bar_below.unwrap_or(slot.rect()).bottom();
            let Some(bar) = slot.bar_below else {
                continue;
            };
            let id = egui::Id::new(("pad-dock-bar", slot.panel.id()));
            let name = bar_name(slot.panel);
            let mut dy = 0.0;
            piece(ctx, id, bar, content, |ui| {
                let resp = widgets::hit(ui, controls, bar, &name, Sense::drag(), true, None);
                if resp.dragged() {
                    dy = resp.drag_delta().y;
                }
                let grip = Rect::from_center_size(bar.center(), vec2(36.0, 4.0));
                let color = if resp.dragged() {
                    theme::TEXT
                } else {
                    theme::MUTE.gamma_multiply(0.6)
                };
                ui.painter().rect_filled(grip, 2.0, color);
            });
            if dy != 0.0 {
                let dock = state.pad.docks.get_mut(workspace);
                drag_bar(dock, &list, i, dy);
            }
        }
        state.pad.dock_frame.slots = list;
    }
    // The space below the panels.
    let rest = Rect::from_min_max(pos2(area.left(), end.max(area.top())), area.max);
    piece(ctx, egui::Id::new("pad-dock-rest"), rest, clip, |_| {});
    if open {
        edge_handle(ctx, state, area);
    }
}

/// Draws the title row of each panel of the dock that did not show, so
/// that it can fold, and an empty body.
pub fn finish(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls) {
    let Some(content) = state.pad.dock_frame.rect else {
        return;
    };
    let slots = state.pad.dock_frame.slots.clone();
    for slot in slots {
        if state.pad.dock_frame.drawn.contains(&slot.panel) {
            continue;
        }
        let id = egui::Id::new(("pad-dock", slot.panel.id()));
        let mut fold = false;
        let mut scroll = 0.0;
        piece(ctx, id, slot.rect(), content, |ui| {
            top_line(ui, slot.rect());
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = panel_face(slot.panel).0;
            let name = title_name(slot.panel);
            let ends = widgets::TitleEnds::Fold {
                name: &name,
                folded: slot.body.is_none(),
            };
            let row = widgets::title_row_with(ui, controls, title, None, false, None, ends);
            fold = row.bar.clicked();
            scroll = row.bar.drag_delta().y;
        });
        title_input(state, slot.panel, fold, scroll);
        state.pad.panels.push((slot.panel, slot.rect()));
    }
}

/// A tap on the title row of a panel of the dock folds or opens it, and a
/// drag scrolls the dock.
pub fn title_input(state: &mut AppState, panel: Panel, tap: bool, drag: f32) {
    let workspace = state.workspace;
    if tap {
        let dock = state.pad.docks.get_mut(workspace);
        if let Some(p) = dock.panels.iter_mut().find(|p| p.panel == panel) {
            p.folded = !p.folded;
        }
    }
    state.pad.dock_frame.scroll -= drag;
}

/// The touch area of an edge handle from `left` to `right`, at the middle
/// from `top` to `bottom`.
fn handle_rect(left: f32, right: f32, top: f32, bottom: f32) -> Rect {
    let middle = (top + bottom) / 2.0;
    Rect::from_min_max(
        pos2(left, middle - HANDLE.1 / 2.0),
        pos2(right, middle + HANDLE.1 / 2.0),
    )
}

/// The grip at the left edge of the open dock. A drag changes the width,
/// and a drag past the narrowest width by more than `EDGE_DRAG` closes the
/// dock. The dock that closes keeps the width from before the drag.
fn edge_handle(ctx: &egui::Context, state: &mut AppState, area: Rect) {
    let x = area.left() + HANDLE_IN;
    let rect = handle_rect(x - HANDLE.0, x, area.top(), area.bottom());
    state.pad.dock_frame.handle = Some(rect);
    let workspace = state.workspace;
    let mut response = None;
    egui::Area::new(egui::Id::new("pad-dock-edge"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let resp = ui.interact(rect, ui.id().with("edge"), Sense::drag());
            let pill = Rect::from_center_size(pos2(area.left(), rect.center().y), vec2(10.0, 52.0));
            let active = resp.dragged();
            ui.painter().rect_filled(pill, 5.0, theme::bar());
            ui.painter().rect_stroke(
                pill,
                5.0,
                egui::Stroke::new(1.0, theme::MUTE.gamma_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
            let color = if active { theme::TEXT } else { theme::MUTE };
            let line = Rect::from_center_size(pill.center(), vec2(3.0, 32.0));
            ui.painter().rect_filled(line, 1.5, color);
            response = Some(resp);
        });
    let Some(resp) = response else {
        return;
    };
    let dock = state.pad.docks.get_mut(workspace);
    let frame = &mut state.pad.dock_frame;
    if resp.drag_started() || resp.dragged() && frame.edge.is_none() {
        frame.edge = Some(EdgeDrag {
            start: dock.width,
            asked: dock.width,
        });
    }
    if let Some(edge) = &mut frame.edge {
        edge.asked -= resp.drag_delta().x;
        dock.set_width(edge.asked);
        ctx.request_repaint();
    }
    if resp.drag_stopped()
        && let Some(edge) = frame.edge.take()
        && edge.asked < WIDTH_MIN - EDGE_DRAG
    {
        dock.open = false;
        dock.width = edge.start;
    }
}

/// The touch area at the right edge of the canvas while the dock is
/// closed. A drag to the left by `EDGE_DRAG` or more opens the dock.
fn open_handle(ctx: &egui::Context, state: &mut AppState, rects: &Rects) {
    let edge = rects.screen.right() - rects.bands.right;
    let rect = handle_rect(
        edge - OPEN_WIDTH,
        edge,
        rects.canvas.top(),
        rects.screen.bottom() - rects.bands.bottom,
    );
    state.pad.dock_frame.handle = Some(rect);
    let mut response = None;
    egui::Area::new(egui::Id::new("pad-dock-open"))
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let resp = ui.interact(rect, ui.id().with("open"), Sense::drag());
            if resp.dragged() {
                let pill = Rect::from_center_size(
                    pos2(rect.right() - 6.0, rect.center().y),
                    vec2(4.0, 36.0),
                );
                ui.painter().rect_filled(pill, 2.0, theme::MUTE);
            }
            response = Some(resp);
        });
    let Some(resp) = response else {
        return;
    };
    let frame = &mut state.pad.dock_frame;
    if resp.drag_started() || resp.dragged() && frame.edge.is_none() {
        frame.edge = Some(EdgeDrag {
            start: 0.0,
            asked: 0.0,
        });
    }
    if let Some(edge) = &mut frame.edge {
        edge.asked -= resp.drag_delta().x;
    }
    if resp.drag_stopped()
        && let Some(edge) = frame.edge.take()
        && edge.asked >= EDGE_DRAG
    {
        state.pad.toggle_dock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::DockPanel;
    use crate::ui::Workspace;

    fn dock(panels: &[(Panel, bool, f32)]) -> DockState {
        let mut dock = DockState::default();
        for &(panel, folded, height) in panels {
            dock.insert(
                Workspace::Globe,
                DockPanel {
                    panel,
                    folded,
                    height,
                },
            );
        }
        dock
    }

    fn rect(height: f32) -> Rect {
        Rect::from_min_size(pos2(800.0, 68.0), vec2(320.0, height))
    }

    #[test]
    fn the_open_panels_share_the_height_by_their_stored_heights() {
        let d = dock(&[
            (Panel::Layers, false, 200.0),
            (Panel::Attributes, false, 400.0),
            (Panel::Toolbox, true, 240.0),
        ]);
        let (s, height) = slots(&d, rect(766.0), 0.0);
        assert_eq!(s.len(), 3);
        // Three title rows and one bar.
        let room = 766.0 - 3.0 * TITLE - BAR;
        let layers = s[0].body.unwrap().height();
        let attributes = s[1].body.unwrap().height();
        assert!((layers + attributes - room).abs() < 0.01);
        assert!((attributes / layers - 2.0).abs() < 0.01);
        assert!(s[0].bar_below.is_some());
        assert_eq!(s[1].bar_below, None, "no open panel below");
        assert_eq!(s[2].body, None);
        assert!((s[2].title.bottom() - rect(766.0).bottom()).abs() < 0.01);
        assert!((height - 766.0).abs() < 0.01);
        assert_eq!(s[0].title.top(), 68.0);
        assert_eq!(s[0].body.unwrap().top(), s[0].title.bottom());
        assert_eq!(s[1].title.top(), s[0].bar_below.unwrap().bottom());
    }

    #[test]
    fn a_panel_is_at_least_the_smallest_height() {
        let d = dock(&[
            (Panel::Layers, false, 1000.0),
            (Panel::Attributes, false, 10.0),
        ]);
        let (s, _) = slots(&d, rect(500.0), 0.0);
        assert!((s[1].body.unwrap().height() - HEIGHT_MIN).abs() < 0.01);
        assert!(s[0].body.unwrap().height() >= HEIGHT_MIN);
    }

    #[test]
    fn panels_that_do_not_fit_make_the_dock_scroll() {
        let d = dock(&[
            (Panel::Layers, false, 240.0),
            (Panel::Attributes, false, 240.0),
            (Panel::Toolbox, false, 240.0),
        ]);
        let (s, height) = slots(&d, rect(400.0), 0.0);
        for slot in &s {
            assert_eq!(slot.body.unwrap().height(), HEIGHT_MIN);
        }
        assert!(height > 400.0);
        let (moved, _) = slots(&d, rect(400.0), 50.0);
        assert_eq!(moved[0].title.top(), s[0].title.top() - 50.0);
    }

    #[test]
    fn folded_panels_stack_their_title_rows() {
        let d = dock(&[(Panel::Layers, true, 240.0), (Panel::Toolbox, true, 240.0)]);
        let (s, height) = slots(&d, rect(600.0), 0.0);
        assert_eq!(height, 2.0 * TITLE);
        assert!(s.iter().all(|s| s.body.is_none() && s.bar_below.is_none()));
        assert_eq!(s[1].title.top(), s[0].title.bottom());
    }

    #[test]
    fn a_drag_of_a_bar_moves_the_line_between_the_open_panels() {
        let mut d = dock(&[
            (Panel::Layers, false, 300.0),
            (Panel::Attributes, true, 240.0),
            (Panel::Toolbox, false, 300.0),
        ]);
        let (s, _) = slots(&d, rect(700.0), 0.0);
        let before = s[0].body.unwrap().height();
        // The bar below Layers sits above the folded Attributes.
        assert!(s[0].bar_below.is_some());
        drag_bar(&mut d, &s, 0, 40.0);
        let (s, _) = slots(&d, rect(700.0), 0.0);
        assert!((s[0].body.unwrap().height() - before - 40.0).abs() < 0.01);
        drag_bar(&mut d, &s, 0, 1000.0);
        let (s, _) = slots(&d, rect(700.0), 0.0);
        assert!((s[2].body.unwrap().height() - HEIGHT_MIN).abs() < 0.01);
    }

    #[test]
    fn share_keeps_the_room() {
        let heights = share(&[100.0, 100.0, 100.0], 600.0);
        assert_eq!(heights, vec![200.0, 200.0, 200.0]);
        assert!(share(&[], 600.0).is_empty());
        assert_eq!(share(&[100.0, 100.0], 200.0), vec![HEIGHT_MIN; 2]);
    }
}
