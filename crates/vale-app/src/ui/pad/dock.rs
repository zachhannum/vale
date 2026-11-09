//! The dock of the iPad layout from 1,000 points: a blurred column at the
//! right edge, from the bar to the bottom edge of the screen. It holds the
//! groups of panels of the workspace from top to bottom. A group of one
//! panel has a title row, and a stack of panels has a row of tabs. The open
//! groups share the height, and a bar with a grip between two open groups
//! moves the line between them. A drag of a title row or a tab moves a panel
//! to another place in the dock, or into a stack. The edge handle at the
//! left edge changes the width, and a drag of it to the right edge closes
//! the dock.

use eframe::egui::{self, Pos2, Rect, Response, Sense, pos2, vec2};

use super::geometry::{PAD, ROW, Rects, TOUCH, WidthClass};
use super::icons::{self, Icon};
use super::theme;
use super::widgets::{self, Controls};
use super::{HOLD_SLOP, HOLD_TIME, Panel, panel_face};
use crate::dock::{DockState, DropAt, HEIGHT_MIN, WIDTH_MIN};
use crate::ui::AppState;

/// The height of the title row of a panel.
pub const TITLE: f32 = ROW;
/// The height of the bar between two open panels.
pub const BAR: f32 = 14.0;
/// The touch area of the bar reaches this far into the body above it and
/// into the title row below it, so that it is as high as a touch target.
pub const GRIP_REACH: f32 = (TOUCH - BAR) / 2.0;
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
/// The size of the card that follows the finger while a panel moves.
const GHOST: (f32, f32) = (200.0, 44.0);
/// A dragged panel this close to the top or the bottom of the dock scrolls
/// the dock.
const SCROLL_EDGE: f32 = 40.0;
/// The distance that the dock scrolls in each frame of a drag at its edge.
const SCROLL_STEP: f32 = 8.0;

/// The control name of the bar below `panel`.
pub fn bar_name(panel: Panel) -> String {
    format!("Resize {}", panel_face(panel).0)
}

/// The control name of the title row of `panel` in the dock, or of its tab
/// in a stack. A tap folds the group, and a tap on a tab that is not active
/// makes its panel active.
pub fn title_name(panel: Panel) -> String {
    format!("{} panel", panel_face(panel).0)
}

/// The place of a group in the dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The index of the group.
    pub group: usize,
    /// The active panel of the group.
    pub panel: Panel,
    /// The number of panels of the group. A stack has more than one.
    pub tabs: usize,
    /// The title row or the row of tabs.
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

/// True if the group at `i` has a bar below it: it is open, and an open
/// group is below it.
fn has_bar(dock: &DockState, i: usize) -> bool {
    !dock.groups[i].folded && dock.groups[i + 1..].iter().any(|g| !g.folded)
}

/// The places of the groups of `dock` in `rect`, moved up by `scroll`, and
/// the height of all groups.
pub fn slots(dock: &DockState, rect: Rect, scroll: f32) -> (Vec<Slot>, f32) {
    let n = dock.groups.len();
    let bars = (0..n).filter(|&i| has_bar(dock, i)).count();
    let stored: Vec<f32> = dock
        .groups
        .iter()
        .filter(|g| !g.folded)
        .map(|g| g.height)
        .collect();
    let room = rect.height() - TITLE * n as f32 - BAR * bars as f32;
    let mut heights = share(&stored, room).into_iter();
    let mut y = rect.top() - scroll;
    let mut out = Vec::with_capacity(n);
    for (i, g) in dock.groups.iter().enumerate() {
        let title = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), TITLE));
        y = title.bottom();
        let body = (!g.folded).then(|| {
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
            group: i,
            panel: g.active_panel(),
            tabs: g.panels.len(),
            title,
            body,
            bar_below,
        });
    }
    (out, y + scroll - rect.top())
}

/// Moves the line of the bar below the group at `i` by `dy`. Every open
/// group keeps the height that it shows, from `slots`, and the nearest open
/// groups above and below the bar share the move. Each stays at least
/// `HEIGHT_MIN` high.
pub fn drag_bar(dock: &mut DockState, slots: &[Slot], i: usize, dy: f32) {
    for (g, slot) in dock.groups.iter_mut().zip(slots) {
        if let Some(body) = slot.body {
            g.height = body.height();
        }
    }
    let groups = &mut dock.groups;
    let above = (0..=i.min(groups.len().saturating_sub(1)))
        .rev()
        .find(|&k| !groups[k].folded);
    let below = (i + 1..groups.len()).find(|&k| !groups[k].folded);
    let (Some(a), Some(b)) = (above, below) else {
        return;
    };
    let total = groups[a].height + groups[b].height;
    let top = (groups[a].height + dy).clamp(HEIGHT_MIN, (total - HEIGHT_MIN).max(HEIGHT_MIN));
    groups[a].height = top;
    groups[b].height = total - top;
}

/// A panel that moves with a finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelDrag {
    pub panel: Panel,
    /// The whole group of the panel moves: the drag started on the title
    /// row of a group of one panel, or on the free part of a row of tabs.
    pub whole: bool,
    /// The finger.
    pub pos: Pos2,
    /// The place where a drop puts the panel. `None`: the drop changes
    /// nothing.
    pub target: Option<DropAt>,
}

/// The place in the dock where a drop of a dragged panel at `pos` puts it:
/// on a title row or a row of tabs, a new tab of that group, and elsewhere
/// a group of its own between the groups. A whole stack goes only between
/// the groups. `None`: the drop changes nothing.
pub fn drop_at(dock: &DockState, slots: &[Slot], drag: &PanelDrag, pos: Pos2) -> Option<DropAt> {
    let source = dock.group_of(drag.panel)?;
    let stack = drag.whole && source.is_stack();
    let over = slots.iter().find(|s| s.title.y_range().contains(pos.y));
    let at = match over {
        Some(slot) if !stack => DropAt::Stack(slot.group),
        _ => {
            let k = slots.iter().filter(|s| s.rect().center().y < pos.y).count();
            DropAt::Between(slots.get(k).map_or(dock.groups.len(), |s| s.group))
        }
    };
    let ok = match (stack, at) {
        (true, DropAt::Between(t)) => dock.can_move_group(drag.panel, t),
        (true, DropAt::Stack(_)) => false,
        (false, at) => dock.can_drop(drag.panel, at),
    };
    ok.then_some(at)
}

/// The line between the groups that shows a drop at `Between(k)`.
fn line_y(slots: &[Slot], k: usize) -> Option<f32> {
    if let Some(slot) = slots.iter().find(|s| s.group == k) {
        let above = k.checked_sub(1).and_then(|i| slots.get(i)?.bar_below);
        return Some(above.map_or(slot.title.top(), |bar| bar.center().y));
    }
    let last = slots.last()?;
    (k == last.group + 1).then(|| last.bar_below.unwrap_or(last.rect()).bottom())
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
    /// The panel that moves with a finger.
    pub drag: Option<PanelDrag>,
    /// A panel dropped in this frame, so the release is not a tap.
    dropped: bool,
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

/// Shows a blurred part of the dock at `rect`, cut to `clip`, with the
/// border at the left edge of `clip`. The parts of the dock do not overlap,
/// so that they look like one blurred area.
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
    let backdrop = widgets::backdrop(ctx);
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(shown.min)
        .movable(false)
        // The dock paints past the safe area, to the bottom of the screen.
        .constrain_to(ctx.viewport_rect())
        .constrain(false)
        .fade_in(false)
        .sizing_pass(false)
        .show(ctx, |ui| {
            let (shown, _) = ui.allocate_exact_size(shown.size(), Sense::click_and_drag());
            let (order, radius) = (egui::Order::Middle, egui::CornerRadius::ZERO);
            widgets::glass(ui.painter(), backdrop, order, shown, radius, false);
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
    frame.dropped = false;
    let workspace = state.workspace;
    let open = state.pad.docks.get(workspace).open;
    if rects.class != WidthClass::Wide || rects.dock.is_none() || !open {
        state.pad.dock_frame.drag = None;
    }
    if rects.class != WidthClass::Wide {
        state.pad.dock_frame.edge = None;
        return;
    }
    let Some(area) = rects.dock else {
        if !open {
            open_handle(ctx, state, rects);
        }
        return;
    };
    release(ctx, state);
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
        follow(ctx, state, &list, content);
        for (i, slot) in list.iter().enumerate() {
            end = slot.bar_below.unwrap_or(slot.rect()).bottom();
            let Some(bar) = slot.bar_below else {
                continue;
            };
            let id = egui::Id::new(("pad-dock-bar", slot.group));
            let name = bar_name(slot.panel);
            let mut dy = 0.0;
            // The touch area of the bar goes on into the panels at each side.
            let touch = bar.expand2(vec2(0.0, GRIP_REACH)).intersect(content);
            if touch.height() >= TOUCH {
                controls.push(widgets::Control {
                    name: name.clone(),
                    rect: touch,
                });
            }
            piece(ctx, id, bar, content, |ui| {
                let resp = widgets::hit(ui, &mut Vec::new(), bar, &name, Sense::drag(), true, None);
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
    // The space below the panels, and the band of the home indicator below a
    // dock that scrolls.
    let top = end.max(area.top()).min(bottom);
    let rest = Rect::from_min_max(pos2(area.left(), top), area.max);
    piece(ctx, egui::Id::new("pad-dock-rest"), rest, clip, |_| {});
    if open {
        edge_handle(ctx, state, area);
    }
}

/// Drops the dragged panel when the finger leaves the screen: at its
/// target, or nowhere if the finger left the dock.
fn release(ctx: &egui::Context, state: &mut AppState) {
    let Some(drag) = state.pad.dock_frame.drag else {
        return;
    };
    if ctx.input(|i| i.pointer.any_down()) {
        return;
    }
    let dock = state.pad.docks.get_mut(state.workspace);
    match drag.target {
        Some(DropAt::Between(t)) if drag.whole => {
            dock.move_group(drag.panel, t);
        }
        Some(at) => {
            dock.move_panel(drag.panel, at);
        }
        None => {}
    }
    state.pad.dock_frame.drag = None;
    state.pad.dock_frame.dropped = true;
}

/// Moves the dragged panel with the finger, finds its target in `slots`,
/// and scrolls the dock while the finger is at its top or bottom.
fn follow(ctx: &egui::Context, state: &mut AppState, slots: &[Slot], content: Rect) {
    let Some(mut drag) = state.pad.dock_frame.drag else {
        return;
    };
    if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
        drag.pos = pos;
    }
    let dock = state.pad.docks.get(state.workspace);
    drag.target = content
        .contains(drag.pos)
        .then(|| drop_at(dock, slots, &drag, drag.pos))
        .flatten();
    if content.x_range().contains(drag.pos.x) {
        if drag.pos.y < content.top() + SCROLL_EDGE {
            state.pad.dock_frame.scroll -= SCROLL_STEP;
        } else if drag.pos.y > content.bottom() - SCROLL_EDGE {
            state.pad.dock_frame.scroll += SCROLL_STEP;
        }
    }
    state.pad.dock_frame.drag = Some(drag);
    ctx.request_repaint();
}

/// The title row of a group in the dock, or its row of tabs. `title`,
/// `back`, and `action` are those of the active panel, and a folded group
/// shows no action. A tap folds the
/// group, a tap on a tab that is not active makes its panel active, and a
/// drag or a hold starts a move.
pub fn head(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    slot: Slot,
    title: &str,
    back: Option<&str>,
    action: Option<(&str, Icon, bool)>,
) -> widgets::TitleRow {
    let folded = slot.body.is_none();
    // The actions act on the body, so a folded group has none.
    let action = action.filter(|_| !folded);
    // The bar above the group, and the strip of the row that its touch area
    // reaches into.
    let slots = &state.pad.dock_frame.slots;
    let above = (slots.iter().position(|s| s.group == slot.group))
        .and_then(|k| k.checked_sub(1))
        .filter(|&k| slots[k].bar_below.is_some());
    let strip = above.map(|_| slot.title.with_max_y(slot.title.top() + GRIP_REACH));
    let panels = match state.pad.docks.get(state.workspace).group_of(slot.panel) {
        Some(group) if group.is_stack() => group.panels.clone(),
        _ => {
            let name = title_name(slot.panel);
            let ends = widgets::TitleEnds::Fold {
                name: &name,
                folded,
                icon: panel_face(slot.panel).1,
                grip: strip,
            };
            let row = widgets::title_row_with(ui, controls, title, back, false, action, ends);
            if grab(ui, state, &row.bar, slot.panel, true) {
                toggle_fold(state, slot.panel);
            }
            grip(state, row.grip.as_ref(), above, slot.panel);
            return row;
        }
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let bar = ui.interact(rect, ui.id().with("pad-dock-tabs"), Sense::click_and_drag());
    let strip = strip.map(|strip| widgets::grip_strip(ui, strip));
    let chevron = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top()), vec2(24.0, ROW));
    let face = if folded { Icon::Open } else { Icon::Down };
    icons::paint(ui.painter(), chevron, face, 18.0, theme::MUTE);
    let button = |x: f32| Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(TOUCH, TOUCH));
    let mut end = rect.right() - PAD;
    let action = action.map(|(name, icon, active)| {
        let at = button(end - TOUCH);
        end = at.left();
        widgets::icon_button_at(ui, controls, at, name, icon, active, true)
    });
    let back = back.is_some_and(|parent| {
        let at = button(end - TOUCH);
        end = at.left();
        let name = format!("Back to {parent}");
        widgets::icon_button_at(ui, controls, at, &name, Icon::Back, false, true).clicked()
    });
    // The first icon lines up with the icon of a title row.
    let left = rect.left() + 28.0;
    let room = end - left - TOUCH * panels.len() as f32;
    let width = widgets::text_width(ui, title, theme::title());
    // The active tab holds the title if it has room for some of it.
    let extra = (width + 9.0).min(room.max(0.0));
    let extra = if extra < 48.0 { 0.0 } else { extra };
    let mut x = left;
    let mut tapped = None;
    for &panel in &panels {
        let active = panel == slot.panel;
        let w = TOUCH + if active { extra } else { 0.0 };
        let tab = Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(w, TOUCH));
        x = tab.right();
        let name = title_name(panel);
        let sense = Sense::click_and_drag();
        let resp = widgets::hit(ui, controls, tab, &name, sense, true, Some(active));
        if active || resp.is_pointer_button_down_on() {
            let fill = tab.shrink(widgets::HIGHLIGHT_INSET);
            ui.painter()
                .rect_filled(fill, theme::CONTROL_RADIUS, theme::raise());
        }
        let color = if active { theme::TEXT } else { theme::MUTE };
        let icon = Rect::from_min_size(tab.min, vec2(TOUCH, TOUCH));
        icons::paint(
            ui.painter(),
            icon,
            panel_face(panel).1,
            widgets::ICON,
            color,
        );
        if active && extra > 0.0 {
            let clip = Rect::from_min_max(
                pos2(icon.right() - 2.0, tab.top()),
                pos2(tab.right() - 7.0, tab.bottom()),
            );
            ui.painter().with_clip_rect(clip).text(
                clip.left_center(),
                egui::Align2::LEFT_CENTER,
                title,
                theme::title(),
                theme::TEXT,
            );
        }
        if grab(ui, state, &resp, panel, false) {
            tapped = Some(panel);
        }
    }
    if grab(ui, state, &bar, slot.panel, true) {
        toggle_fold(state, slot.panel);
    }
    grip(state, strip.as_ref(), above, slot.panel);
    match tapped {
        Some(panel) if panel == slot.panel => toggle_fold(state, panel),
        Some(panel) => {
            let workspace = state.workspace;
            state.pad.docks.get_mut(workspace).show(workspace, panel);
        }
        None => {}
    }
    widgets::TitleRow {
        close: false,
        back,
        dock: false,
        action,
        bar,
        grip: strip,
    }
}

/// The strip at the bottom of the body of `slot` that is a part of the
/// touch area of the bar below it. It is above the body, so a drag there
/// moves the bar.
pub fn body_grip(ui: &egui::Ui, state: &mut AppState, slot: Slot) {
    let (Some(body), Some(_)) = (slot.body, slot.bar_below) else {
        return;
    };
    let strip = body.with_min_y(body.bottom() - GRIP_REACH);
    let resp = ui.interact(strip, ui.id().with("pad-grip-body"), Sense::drag());
    if resp.dragged() && state.pad.dock_frame.drag.is_none() {
        let dy = resp.drag_delta().y;
        let slots = state.pad.dock_frame.slots.clone();
        drag_bar(
            state.pad.docks.get_mut(state.workspace),
            &slots,
            slot.group,
            dy,
        );
    }
}

/// A drag of the strip of a title row moves the bar above it, the group
/// at `above`, and a tap on it folds the group of `panel`.
fn grip(state: &mut AppState, strip: Option<&Response>, above: Option<usize>, panel: Panel) {
    let (Some(strip), Some(i)) = (strip, above) else {
        return;
    };
    if strip.dragged() && state.pad.dock_frame.drag.is_none() {
        let dy = strip.drag_delta().y;
        let slots = state.pad.dock_frame.slots.clone();
        drag_bar(state.pad.docks.get_mut(state.workspace), &slots, i, dy);
    }
    if strip.clicked() {
        toggle_fold(state, panel);
    }
}

/// Folds or opens the group of `panel`.
fn toggle_fold(state: &mut AppState, panel: Panel) {
    let dock = state.pad.docks.get_mut(state.workspace);
    if let Some((g, _)) = dock.position(panel) {
        dock.groups[g].folded = !dock.groups[g].folded;
    }
}

/// Starts a move of `panel` on a drag or a hold of `resp`, and scrolls the
/// dock on a scroll over it. Returns true on a tap.
fn grab(ui: &egui::Ui, state: &mut AppState, resp: &Response, panel: Panel, whole: bool) -> bool {
    let frame = &mut state.pad.dock_frame;
    if resp.hovered() {
        frame.scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    if frame.drag.is_some() || frame.dropped {
        return false;
    }
    let held = resp.is_pointer_button_down_on() && !resp.dragged() && {
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
    };
    if resp.drag_started() || held {
        let pos = resp.interact_pointer_pos().unwrap_or(resp.rect.center());
        frame.drag = Some(PanelDrag {
            panel,
            whole,
            pos,
            target: None,
        });
        ui.ctx().request_repaint();
        return false;
    }
    resp.clicked()
}

/// Draws the title row of each group of the dock whose panel did not show,
/// so that it can fold and move, and an empty body. Then draws the panel
/// that moves with a finger, and where it goes.
pub fn finish(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls) {
    let Some(content) = state.pad.dock_frame.rect else {
        return;
    };
    let slots = state.pad.dock_frame.slots.clone();
    for slot in &slots {
        if state.pad.dock_frame.drawn.contains(&slot.panel) {
            continue;
        }
        let id = egui::Id::new(("pad-dock", slot.panel.id()));
        piece(ctx, id, slot.rect(), content, |ui| {
            top_line(ui, slot.rect());
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = panel_face(slot.panel).0;
            head(ui, state, controls, *slot, title, None, None);
            body_grip(ui, state, *slot);
        });
        state.pad.panels.push((slot.panel, slot.rect()));
    }
    if let Some(drag) = state.pad.dock_frame.drag {
        drop_marks(ctx, &slots, content, &drag);
    }
}

/// Draws where a drop of `drag` puts the panel, and the lifted card of the
/// panel above the finger.
fn drop_marks(ctx: &egui::Context, slots: &[Slot], content: Rect, drag: &PanelDrag) {
    let order = egui::Order::Foreground;
    let painter = ctx.layer_painter(egui::LayerId::new(order, egui::Id::new("pad-dock-drop")));
    let stroke = egui::Stroke::new(2.0, theme::ACCENT);
    match drag.target {
        Some(DropAt::Stack(g)) => {
            if let Some(slot) = slots.iter().find(|s| s.group == g) {
                let rect = slot.title.shrink(widgets::HIGHLIGHT_INSET);
                let painter = painter.with_clip_rect(content);
                painter.rect_stroke(
                    rect,
                    theme::CONTROL_RADIUS,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Some(DropAt::Between(k)) => {
            if let Some(y) = line_y(slots, k) {
                let y = y.clamp(content.top() + 1.5, content.bottom() - 1.5);
                let line = Rect::from_min_max(
                    pos2(content.left() + 8.0, y - 1.5),
                    pos2(content.right() - 8.0, y + 1.5),
                );
                painter.rect_filled(line, 1.5, theme::ACCENT);
            }
        }
        None => {}
    }
    let (name, icon) = panel_face(drag.panel);
    let rect = Rect::from_center_size(drag.pos - vec2(0.0, 30.0), vec2(GHOST.0, GHOST.1));
    egui::Area::new(egui::Id::new("pad-dock-ghost"))
        .order(order)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .interactable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let radius = egui::CornerRadius::same(theme::CONTROL_RADIUS as u8);
            let backdrop = widgets::backdrop(ui.ctx());
            widgets::glass(ui.painter(), backdrop, order, rect, radius, true);
            ui.painter().rect_filled(rect, radius, theme::raise());
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.0, theme::divider()),
                egui::StrokeKind::Inside,
            );
            let at = Rect::from_min_size(rect.min, vec2(TOUCH, TOUCH));
            icons::paint(ui.painter(), at, icon, widgets::ICON, theme::TEXT);
            let clip = Rect::from_min_max(
                pos2(at.right() - 4.0, rect.top()),
                pos2(rect.right() - 10.0, rect.bottom()),
            );
            ui.painter().with_clip_rect(clip).text(
                clip.left_center(),
                egui::Align2::LEFT_CENTER,
                name,
                theme::title(),
                theme::TEXT,
            );
        });
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
            ui.painter().rect_filled(pill, 5.0, theme::handle());
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
    use crate::dock::DropAt;
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
    fn a_stack_has_one_slot_with_its_active_panel() {
        let mut d = dock(&[
            (Panel::Layers, false, 240.0),
            (Panel::Attributes, false, 240.0),
            (Panel::Toolbox, false, 240.0),
        ]);
        d.move_panel(Panel::Toolbox, DropAt::Stack(0));
        d.activate(Panel::Layers);
        let (s, height) = slots(&d, rect(700.0), 0.0);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].group, s[0].panel, s[0].tabs), (0, Panel::Layers, 2));
        assert_eq!(
            (s[1].group, s[1].panel, s[1].tabs),
            (1, Panel::Attributes, 1)
        );
        assert!(s[0].bar_below.is_some());
        let room = 700.0 - 2.0 * TITLE - BAR;
        let bodies = s[0].body.unwrap().height() + s[1].body.unwrap().height();
        assert!((bodies - room).abs() < 0.01);
        assert!((height - 700.0).abs() < 0.01);
    }

    #[test]
    fn a_drop_goes_into_a_title_row_or_between_the_groups() {
        let d = dock(&[
            (Panel::Layers, false, 240.0),
            (Panel::Attributes, false, 240.0),
            (Panel::Toolbox, false, 240.0),
        ]);
        let (s, _) = slots(&d, rect(700.0), 0.0);
        let drag = |panel, whole| PanelDrag {
            panel,
            whole,
            pos: Pos2::ZERO,
            target: None,
        };
        let x = 900.0;
        let toolbox = drag(Panel::Toolbox, true);
        let at = |y: f32| drop_at(&d, &s, &toolbox, pos2(x, y));
        assert_eq!(at(s[0].title.center().y), Some(DropAt::Stack(0)));
        assert_eq!(at(s[1].title.center().y), Some(DropAt::Stack(1)));
        assert_eq!(at(s[2].title.center().y), None, "its own title row");
        assert_eq!(at(s[0].rect().top() + 60.0), Some(DropAt::Between(0)));
        assert_eq!(
            at(s[0].bar_below.unwrap().center().y),
            Some(DropAt::Between(1))
        );
        assert_eq!(at(s[2].rect().bottom() - 5.0), None, "its own place");
        assert_eq!(line_y(&s, 1), Some(s[0].bar_below.unwrap().center().y));
        assert_eq!(line_y(&s, 0), Some(s[0].title.top()));
        assert_eq!(line_y(&s, 3), Some(s[2].rect().bottom()));

        // A whole stack goes only between the groups, and a tab of it also
        // into another group.
        let mut d = d.clone();
        d.move_panel(Panel::Attributes, DropAt::Stack(0));
        let (s, _) = slots(&d, rect(700.0), 0.0);
        let stack = drag(Panel::Layers, true);
        let y = s[1].title.center().y;
        // Above the Toolbox group is just below the stack: its own place.
        assert_eq!(drop_at(&d, &s, &stack, pos2(x, y)), None);
        let y = s[1].rect().bottom() - 5.0;
        assert_eq!(
            drop_at(&d, &s, &stack, pos2(x, y)),
            Some(DropAt::Between(2))
        );
        let tab = drag(Panel::Attributes, false);
        let y = s[1].title.center().y;
        assert_eq!(drop_at(&d, &s, &tab, pos2(x, y)), Some(DropAt::Stack(1)));
        let y = s[0].title.center().y;
        assert_eq!(drop_at(&d, &s, &tab, pos2(x, y)), None, "its own stack");
    }

    #[test]
    fn share_keeps_the_room() {
        let heights = share(&[100.0, 100.0, 100.0], 600.0);
        assert_eq!(heights, vec![200.0, 200.0, 200.0]);
        assert!(share(&[], 600.0).is_empty());
        assert_eq!(share(&[100.0, 100.0], 200.0), vec![HEIGHT_MIN; 2]);
    }
}
