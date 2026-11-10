//! The dock of a workspace on the screen: the panes in one card on
//! the right edge. On a compact screen, the pane of the last focus is a
//! sheet. A long press on a tab opens the menu of its panel.

use eframe::egui::{self, Align2, Order, Pos2, Rect, Sense, Stroke, pos2, vec2};

use super::geometry::{self, PAD, ROW, Rects, TOUCH, WidthClass};
use super::icons::{self, Icon};
use super::panes::{self, BAR, Drop};
use super::widgets::{self, BODY_PAD, Controls, ICON};
use super::{HOLD_SLOP, HOLD_TIME, point_card, popover, theme};
use super::{Head, Panel, panel_body, panel_head, panel_icon, panel_name, panel_title, panel_top};
use crate::ui::AppState;

/// The handle that changes the width of the dock.
pub const DOCK_WIDTH: &str = "Dock width";
/// The height of the part of a bar that takes a drag.
const BAR_TOUCH: f32 = TOUCH;
/// The extra height of the last bar, so its touch area is in the dock.
const FOOT: f32 = (BAR_TOUCH - BAR) / 2.0;
/// The size of the handle at the left edge of the dock.
pub const HANDLE: (f32, f32) = (TOUCH, 120.0);
/// The padding at each side of the name of a tab.
const TAB_PAD: f32 = 12.0;
/// The time of the slide of the dock.
pub const SLIDE_TIME: f32 = 0.25;
/// A drag of the width handle this far past the narrowest dock collapses
/// the dock.
pub const COLLAPSE_DRAG: f32 = 40.0;
/// The tab at the right edge that shows a collapsed dock.
pub const SHOW_DOCK: &str = "Show dock";
/// The text of a drag of the width handle that collapses the dock.
pub const HIDE_DOCK: &str = "Hide dock";
/// The touch area of the tab of a collapsed dock.
const EDGE_TAB: (f32, f32) = (TOUCH, 72.0);
/// The row of the menu of a tab that moves the tab to a new pane.
pub const MOVE_TO_NEW_PANE: &str = "Move to new pane";
/// The width of the menu of a tab.
const MENU_WIDTH: f32 = 240.0;

/// What a finger moves in the dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockDrag {
    /// The pane with this index, by the empty part of its header.
    Pane { pane: usize, pos: Pos2 },
    /// A tab, or a pane with one tab.
    Tab { panel: Panel, pos: Pos2 },
    /// The handle of the width.
    Width,
}

/// The control name of the empty part of the header of a pane.
pub fn move_name(name: &str) -> String {
    format!("Move {name}")
}

/// The control name of a tab.
pub fn tab_name(name: &str) -> String {
    format!("{name} tab")
}

/// The row of the menu of a tab, and the button of a sheet, that close a
/// panel.
pub fn close_name(name: &str) -> String {
    format!("Close {name}")
}

/// The menu of a tab, which a long press opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneMenu {
    pub panel: Panel,
    /// The tab or the title that the menu points at.
    pub anchor: Rect,
    /// The menu is the menu of a sheet, which has no Move row.
    sheet: bool,
    /// The press that opened the menu goes on. Its release is not a tap.
    held: bool,
}

/// The places of a pane in the last frame, for a drop.
struct Placed {
    rect: Rect,
    /// The part of the header that holds the tabs.
    tabs: Rect,
    /// The middle of each tab, along the header.
    centers: Vec<f32>,
}

/// What a pane shows of its controls.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Dock,
    Sheet,
}

/// What the user did with the header of a pane.
struct HeaderOut {
    /// The Close button of a sheet.
    close: bool,
    /// The tab that the user chose.
    focus: Option<usize>,
    /// The empty part of the header.
    bar: Option<egui::Response>,
    /// The tabs of a sheet, which move the sheet with a drag.
    tab_drags: Vec<egui::Response>,
    /// A drag of a tab, or of the header, that started or that ended.
    drag: Option<(DockDrag, DragPhase)>,
    tabs: Rect,
    centers: Vec<f32>,
}

#[derive(Clone, Copy, PartialEq)]
enum DragPhase {
    Moving,
    Stopped,
}

pub fn show(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    state.pad.dock_rect = None;
    if let Some(menu) = &mut state.pad.pane_menu
        && !ctx.input(|i| i.pointer.any_down() || i.pointer.any_released())
    {
        menu.held = false;
    }
    if state.pad.dock().panes.is_empty() {
        state.pad.dock_drag = None;
        state.pad.pane_menu = None;
        return;
    }
    if rects.class == WidthClass::Compact {
        state.pad.dock_drag = None;
        if crate::ui::new_feature::point_card(state, rects.class) {
            let rect = point_card::show(ctx, state, controls, rects);
            state.pad.panels.push((Panel::Attributes, rect));
        } else if let Some(sheet) = rects.sheet {
            show_sheet(ctx, state, controls, rects, sheet);
        }
    } else {
        let slide = slide(ctx, state);
        if slide < 1.0 {
            show_dock(ctx, state, controls, rects, slide);
        }
        if state.pad.dock().collapsed {
            state.pad.pane_menu = None;
            if !state.pad.panels_menu {
                edge_tab(ctx, state, controls, rects);
            }
        }
    }
    pane_menu(ctx, state, controls);
}

/// How far the dock is off the right edge, from 0 on the screen to 1 off
/// it.
fn slide(ctx: &egui::Context, state: &AppState) -> f32 {
    let collapsed = state.pad.dock().collapsed;
    let id = egui::Id::new(("pad-dock-slide", state.pad.workspace as u8));
    let t = ctx.animate_bool_with_time(id, collapsed, SLIDE_TIME);
    if state.headless {
        f32::from(u8::from(collapsed))
    } else {
        t
    }
}

/// The tab at the middle of the right edge that shows a collapsed dock,
/// with a tap or a drag to the left.
fn edge_tab(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let right = rects.screen.right() - rects.bands.right;
    let rect = Rect::from_min_size(
        pos2(right - EDGE_TAB.0, rects.dock.center().y - EDGE_TAB.1 / 2.0),
        vec2(EDGE_TAB.0, EDGE_TAB.1),
    );
    let visible = Rect::from_min_max(
        pos2(right - 16.0, rect.top() + 8.0),
        pos2(rects.screen.right(), rect.bottom() - 8.0),
    );
    let mut show = false;
    egui::Area::new(egui::Id::new("pad-dock-edge-tab"))
        .order(Order::Middle)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let resp = widgets::drag_area(ui, controls, rect, SHOW_DOCK);
            let r = theme::CONTROL_RADIUS as u8;
            let radius = egui::CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            };
            let painter = ui.painter();
            let fill = if resp.is_pointer_button_down_on() {
                theme::card(false).to_opaque().blend(theme::raise())
            } else {
                theme::card(false).to_opaque()
            };
            painter.add(theme::shadow().as_shape(visible, r));
            painter.rect_filled(visible, radius, fill);
            let pill = Rect::from_center_size(
                pos2(visible.left() + 8.0, visible.center().y),
                vec2(4.0, 28.0),
            );
            painter.rect_filled(pill, 2.0, theme::MUTE.gamma_multiply(0.7));
            let left = ui.input(|i| {
                let origin = i.pointer.press_origin()?;
                Some(origin.x - i.pointer.interact_pos()?.x)
            });
            show = resp.clicked() || (resp.dragged() && left.is_some_and(|d| d >= 2.0 * HOLD_SLOP));
        });
    if show {
        state.pad.dock_mut().collapsed = false;
    }
}

/// The menu of a tab after a long press: "Close <panel>", and in a dock
/// pane with more than one tab, "Move to new pane".
fn pane_menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls) {
    let Some(menu) = state.pad.pane_menu else {
        return;
    };
    let Some((i, _)) = state.pad.dock().find(menu.panel) else {
        state.pad.pane_menu = None;
        return;
    };
    let split = !menu.sheet && state.pad.dock().panes[i].tabs.len() > 1;
    let close = close_name(panel_name(state, menu.panel));
    let (mut closed, mut moved) = (false, false);
    let shown = popover::show(ctx, "pad-pane-menu", menu.anchor, MENU_WIDTH, |ui| {
        closed = widgets::text_row(ui, controls, &close, 0.0, false, true).clicked();
        if split {
            popover::divider(ui);
            moved = widgets::text_row(ui, controls, MOVE_TO_NEW_PANE, 0.0, false, true).clicked();
        }
    });
    if closed {
        state.pad.close(menu.panel);
    }
    if moved {
        state.pad.dock_mut().split_tab(menu.panel);
    }
    if closed || moved || (shown.closed && !menu.held) {
        state.pad.pane_menu = None;
    }
}

/// True if a press on `resp` is a long press: it lasts `HOLD_TIME` and
/// moves no more than `HOLD_SLOP`.
fn long_press(ui: &egui::Ui, resp: &egui::Response) -> bool {
    // The frame of the release is not part of the press.
    if !resp.is_pointer_button_down_on() || !ui.input(|i| i.pointer.primary_down()) {
        return false;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    match press {
        Some((time, moved)) if moved <= HOLD_SLOP && time >= HOLD_TIME => true,
        Some((time, moved)) if moved <= HOLD_SLOP => {
            let wait = std::time::Duration::from_secs_f64(HOLD_TIME - time);
            ui.ctx().request_repaint_after(wait);
            false
        }
        _ => false,
    }
}

fn show_dock(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    slide: f32,
) {
    let room = rects.dock;
    state.pad.dock_mut().keep_in(room.height());
    let mut heads: Vec<Head> = (0..state.pad.dock().panes.len())
        .map(|i| {
            let panel = state.pad.dock().panes[i].panel();
            panel_head(state, panel)
        })
        .collect();
    // A pane shows its content, up to the height that the user gave it.
    let natural: Vec<f32> = (heads.iter())
        .map(|head| {
            state
                .pad
                .heights
                .get(head.key)
                .copied()
                .unwrap_or(room.height())
        })
        .collect();
    let wanted: Vec<f32> = (state.pad.dock().panes.iter().zip(&natural))
        .map(|(pane, &natural)| pane.height.map_or(natural, |h| h.min(natural)))
        .collect();
    let space = room.height() - FOOT;
    let heights = panes::fit(&wanted, space);
    let total = heights.iter().sum::<f32>() + BAR * heights.len() as f32 + FOOT;
    let rect = Rect::from_min_size(room.min, vec2(room.width(), total.min(room.height())));
    // A collapsing dock slides off the right edge.
    let rect = rect.translate(vec2((slide * room.width()).round(), 0.0));
    let radius = geometry::edge_radius(rect, rects.screen);
    let names = rect.width() - rects.bands.right >= panes::NAMES_WIDTH;
    let backdrop = state.globe.backdrop();
    let mut placed = Vec::new();
    let mut drag = None;
    let mut bars = Vec::new();
    let mut actions: Vec<(usize, HeaderOut)> = Vec::new();
    widgets::card(
        ctx,
        backdrop,
        "pad-dock",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let mut top = rect.top();
            for (i, head) in heads.iter_mut().enumerate() {
                let pane_rect =
                    Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), heights[i]));
                let inner = geometry::edge_content(pane_rect, rects.screen, rects.bands);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min))
                        .id_salt(("pad-pane", i)),
                );
                child.set_clip_rect(inner.intersect(rect));
                let out = pane(&mut child, state, controls, i, head, inner, names);
                placed.push(Placed {
                    rect: pane_rect,
                    tabs: out.tabs,
                    centers: out.centers.clone(),
                });
                state.pad.panels.push((head.panel, pane_rect));
                if out.drag.is_some() {
                    drag = out.drag;
                }
                actions.push((i, out));
                // Each pane has a bar below it that sets its height.
                let bar = Rect::from_min_size(
                    pos2(rect.left(), pane_rect.bottom()),
                    vec2(rect.width(), BAR),
                );
                bars.push(bar);
                top = bar.bottom();
            }
            // The bars take a drag after the panes, so they win at the edges.
            let last = bars.len().saturating_sub(1);
            for (i, bar) in bars.iter().enumerate() {
                let line = geometry::edge_content(*bar, rects.screen, rects.bands);
                if i < last {
                    ui.painter().hline(
                        line.x_range(),
                        bar.center().y,
                        Stroke::new(1.0, theme::divider()),
                    );
                }
                let pill = Rect::from_center_size(bar.center(), vec2(36.0, 4.0));
                let pill = pill.translate(vec2((line.center().x - bar.center().x).round(), 0.0));
                let touch = Rect::from_center_size(bar.center(), vec2(bar.width(), BAR_TOUCH));
                let sense = Sense::click_and_drag();
                let resp = ui.interact(touch, ui.id().with(("pad-bar", i)), sense);
                let color = if resp.dragged() {
                    theme::ACCENT
                } else {
                    theme::MUTE.gamma_multiply(0.6)
                };
                ui.painter().rect_filled(pill, 2.0, color);
                if resp.dragged() && resp.drag_delta().y != 0.0 {
                    let delta = resp.drag_delta().y;
                    let dock = state.pad.dock_mut();
                    dock.resize(i, delta, &heights, &natural, space);
                    ctx.request_repaint();
                }
                if resp.double_clicked() {
                    let dock = state.pad.dock_mut();
                    dock.grow(i, &heights, &natural, space);
                }
            }
        },
    );
    if !state.pad.dock().collapsed {
        state.pad.dock_rect = Some(rect);
    }
    for (i, out) in actions.into_iter().rev() {
        if let Some(tab) = out.focus {
            state.pad.dock_mut().focus(i, tab);
        }
    }
    match drag {
        Some((drag, DragPhase::Moving)) => {
            state.pad.dock_drag = Some(drag);
            paint_drop(ctx, state, rect, &placed, drag);
            ctx.request_repaint();
        }
        // A drag that did not move is no drop.
        Some((_, DragPhase::Stopped)) if state.pad.dock_drag.is_none() => {}
        Some((drag, DragPhase::Stopped)) => {
            // The finger is gone at the end of the drag, so the drop is at
            // its last place.
            let drag = match (drag, state.pad.dock_drag) {
                (DockDrag::Pane { pane, .. }, Some(DockDrag::Pane { pos, .. })) => {
                    DockDrag::Pane { pane, pos }
                }
                (DockDrag::Tab { panel, .. }, Some(DockDrag::Tab { pos, .. })) => {
                    DockDrag::Tab { panel, pos }
                }
                (drag, _) => drag,
            };
            if let Some(target) = drop_of(state.pad.dock(), rect, &placed, drag) {
                let dock = state.pad.dock_mut();
                match (drag, target) {
                    (DockDrag::Pane { pane, .. }, Target::Gap(at)) => {
                        dock.move_pane(pane, at);
                    }
                    (DockDrag::Tab { panel, .. }, Target::Gap(at)) => {
                        dock.move_tab(panel, Drop::Split { at });
                    }
                    (DockDrag::Tab { panel, .. }, Target::Tabs(pane, index)) => {
                        let drop = Drop::Merge { pane, index };
                        dock.move_tab(panel, drop);
                    }
                    _ => {}
                }
            }
            state.pad.dock_drag = None;
        }
        None if state.pad.dock_drag != Some(DockDrag::Width) => state.pad.dock_drag = None,
        None => {}
    }
}

/// Where a drag ends.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// The place before the pane with this index.
    Gap(usize),
    /// The tab row of the pane with this index, before the tab with this
    /// index.
    Tabs(usize, usize),
}

/// The target of a drag at its present place. A drop outside the dock
/// does nothing.
fn drop_of(dock: &panes::Dock, rect: Rect, placed: &[Placed], drag: DockDrag) -> Option<Target> {
    let (pos, from) = match drag {
        DockDrag::Pane { pane, pos } => (pos, Some(pane)),
        DockDrag::Tab { panel, pos } => (pos, dock.find(panel).map(|(i, _)| i)),
        DockDrag::Width => return None,
    };
    let area = rect.expand2(vec2(0.0, ROW));
    if !(area.x_range().contains(pos.x) && area.y_range().contains(pos.y)) {
        return None;
    }
    if let DockDrag::Tab { .. } = drag {
        for (i, p) in placed.iter().enumerate() {
            if Some(i) != from && p.tabs.contains(pos) {
                let index = p.centers.iter().filter(|c| **c < pos.x).count();
                return Some(Target::Tabs(i, index));
            }
        }
    }
    let at = placed.iter().filter(|p| p.rect.center().y < pos.y).count();
    Some(Target::Gap(at))
}

/// The ghost of the pane or the tab under the finger, and the line or the
/// outline of its drop target.
fn paint_drop(
    ctx: &egui::Context,
    state: &AppState,
    rect: Rect,
    placed: &[Placed],
    drag: DockDrag,
) {
    let (pos, name, icon) = match drag {
        DockDrag::Pane { pane, pos } => {
            let panel = state.pad.dock().panes[pane].panel();
            (pos, panel_name(state, panel), panel_icon(panel))
        }
        DockDrag::Tab { panel, pos } => (pos, panel_name(state, panel), panel_icon(panel)),
        DockDrag::Width => return,
    };
    let target = drop_of(state.pad.dock(), rect, placed, drag);
    let painter = ctx.layer_painter(egui::LayerId::new(
        Order::Foreground,
        "pad-dock-drop".into(),
    ));
    match target {
        Some(Target::Gap(at)) => {
            let y = match placed.get(at) {
                Some(p) if at == 0 => p.rect.top() + 2.0,
                Some(p) => p.rect.top() - BAR / 2.0,
                None => placed.last().map_or(rect.top(), |p| p.rect.bottom() - 2.0),
            };
            let x = rect.x_range().shrink(BODY_PAD);
            painter.hline(x, y, Stroke::new(3.0, theme::ACCENT));
        }
        Some(Target::Tabs(i, _)) => {
            let tabs = placed[i].tabs;
            painter.rect_stroke(
                tabs,
                theme::CONTROL_RADIUS,
                Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        None => {}
    }
    let ghost = Rect::from_min_size(pos - vec2(40.0, ROW / 2.0), vec2(220.0, ROW));
    painter.rect_filled(
        ghost,
        theme::CARD_RADIUS,
        theme::card(false).gamma_multiply(0.9),
    );
    let icon_rect = Rect::from_center_size(
        pos2(ghost.left() + BODY_PAD + ICON / 2.0, ghost.center().y),
        vec2(ICON, ICON),
    );
    icons::paint(&painter, icon_rect, icon, ICON, theme::TEXT);
    let at = pos2(icon_rect.right() + 10.0, ghost.center().y);
    painter.text(at, Align2::LEFT_CENTER, name, theme::title(), theme::TEXT);
}

/// The handle at the middle of the left edge of the dock, and the label of
/// the width during its drag. A card or a menu that covers the place of the
/// handle hides it.
pub fn handle(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(dock) = state.pad.dock_rect else {
        return;
    };
    // The touch area of the handle is at the left of the dock, so it covers
    // no control of a pane. Its grip is on the edge of the dock.
    let rect = Rect::from_min_size(
        pos2(dock.left() - HANDLE.0, dock.center().y - HANDLE.1 / 2.0),
        vec2(HANDLE.0, HANDLE.1),
    );
    let dragging = state.pad.dock_drag == Some(DockDrag::Width);
    // A control under the handle hides it. A control of a pane touches its
    // right edge, and that is no overlap.
    if !dragging
        && controls
            .iter()
            .any(|c| c.rect.intersect(rect).is_positive())
    {
        return;
    }
    let most =
        (rects.screen.right() - rects.bands.right - rects.column_right).max(panes::MIN_WIDTH);
    egui::Area::new(egui::Id::new("pad-dock-handle"))
        .order(Order::Middle)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let resp = widgets::drag_area(ui, controls, rect, DOCK_WIDTH);
            let active = resp.dragged();
            // The width that the finger asks for, which can be below the
            // narrowest dock.
            if !active {
                // The collapsed dock keeps the width of the start of the
                // drag.
                if let Some((start, width)) = state.pad.width_drag.take()
                    && resp.drag_stopped()
                    && width <= panes::MIN_WIDTH - COLLAPSE_DRAG
                {
                    let dock = state.pad.dock_mut();
                    dock.collapsed = true;
                    dock.width = start;
                }
            }
            // A grip on a dark capsule shows over a white page and over the
            // dark canvas.
            let center = pos2(dock.left(), rect.center().y);
            let capsule = Rect::from_center_size(center, vec2(12.0, 64.0));
            let back = theme::card(false).to_opaque();
            ui.painter().rect_filled(capsule, 6.0, back);
            let pill = Rect::from_center_size(center, vec2(4.0, 48.0));
            let color = if active { theme::ACCENT } else { theme::TEXT };
            ui.painter().rect_filled(pill, 2.0, color);
            if active {
                let (start, width) = (state.pad.width_drag)
                    .unwrap_or((state.pad.dock().width, state.pad.dock().width));
                let width = width - resp.drag_delta().x;
                state.pad.width_drag = Some((start, width));
                let dock = state.pad.dock_mut();
                dock.set_width(width);
                dock.width = dock.width.min(most);
                let text = if width <= panes::MIN_WIDTH - COLLAPSE_DRAG {
                    HIDE_DOCK.to_owned()
                } else {
                    format!("{} pt", dock.width.round())
                };
                state.pad.dock_drag = Some(DockDrag::Width);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    Order::Foreground,
                    "pad-dock-width".into(),
                ));
                let galley = painter.layout_no_wrap(text, theme::body(), theme::TEXT);
                let size = galley.size() + vec2(24.0, 14.0);
                let label = Rect::from_min_size(
                    pos2(rect.left() - 8.0 - size.x, rect.center().y - size.y / 2.0),
                    size,
                );
                painter.rect_filled(label, theme::CONTROL_RADIUS, theme::card(false).to_opaque());
                painter.galley(label.center() - galley.size() / 2.0, galley, theme::TEXT);
                ctx.request_repaint();
            } else if state.pad.dock_drag == Some(DockDrag::Width) {
                state.pad.dock_drag = None;
            }
        });
}

/// The sheet of a compact screen: the pane of the last focus, with its
/// tabs and its Close button, and with no drag of its tabs.
fn show_sheet(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    rect: Rect,
) {
    let Some(i) = state.pad.sheet_pane() else {
        return;
    };
    let panel = state.pad.dock().panes[i].panel();
    let mut head = panel_head(state, panel);
    let r = theme::SHEET_RADIUS as u8;
    let radius = egui::CornerRadius {
        nw: r,
        ne: r,
        sw: 0,
        se: 0,
    };
    let backdrop = state.globe.backdrop();
    let inner = geometry::edge_content(rect, rects.screen, rects.bands);
    let mut out = None;
    widgets::card(
        ctx,
        backdrop,
        "pad-sheet",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let grabber =
                Rect::from_center_size(pos2(rect.center().x, rect.top() + 8.5), vec2(36.0, 5.0));
            ui.painter()
                .rect_filled(grabber, 3.0, theme::MUTE.gamma_multiply(0.6));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            let body = Rect::from_min_max(
                inner.min,
                pos2(inner.right(), rect.bottom() - rects.bands.bottom),
            );
            out = Some(pane_in_sheet(
                &mut child, state, controls, rects, i, &mut head, body,
            ));
        },
    );
    state.pad.panels.push((panel, rect));
    if let Some(out) = out {
        let dock = state.pad.dock_mut();
        if out.close {
            dock.close_pane(i);
            state.pad.pane_menu = None;
        } else if let Some(tab) = out.focus {
            dock.focus(i, tab);
        }
    }
}

fn pane_in_sheet(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    i: usize,
    head: &mut Head,
    body: Rect,
) -> HeaderOut {
    ui.spacing_mut().item_spacing.y = 0.0;
    let header = Rect::from_min_size(body.min, vec2(body.width(), ROW));
    let out = header_row(ui, state, controls, i, head, header, Mode::Sheet, true);
    ui.advance_cursor_after_rect(header);
    let low = state.pad.sheet_offset > -rects.sheet_rise + 0.5;
    ui.ctx().data_mut(|d| d.insert_temp(super::low_id(), low));
    if super::take_raise(ui.ctx()) {
        state.pad.sheet_offset = -rects.sheet_rise;
    }
    if let Some(bar) = &out.bar {
        super::sheet_drag(ui, &mut state.pad, bar, rects);
    }
    for tab in out
        .tab_drags
        .iter()
        .filter(|t| t.dragged() || t.drag_stopped())
    {
        super::sheet_drag(ui, &mut state.pad, tab, rects);
    }
    if state.pad.sheet_offset >= rects.sheet_travel {
        return out;
    }
    contents(ui, state, controls, head, body.bottom());
    out
}

/// A pane of the dock: its header, its back row, and its body.
#[allow(clippy::too_many_arguments)]
fn pane(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    i: usize,
    head: &mut Head,
    rect: Rect,
    names: bool,
) -> HeaderOut {
    ui.spacing_mut().item_spacing.y = 0.0;
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), ROW));
    let out = header_row(ui, state, controls, i, head, header, Mode::Dock, names);
    ui.advance_cursor_after_rect(header);
    contents(ui, state, controls, head, rect.bottom());
    out
}

/// The back row, the rows that do not scroll, and the body of a pane, down
/// to `bottom`. Keeps the height of the content.
fn contents(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    head: &mut Head,
    bottom: f32,
) {
    let start = ui.cursor().top();
    if let Some(back) = &head.back {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        let button =
            Rect::from_min_size(pos2(row.left() + PAD, row.top() + PAD), vec2(TOUCH, TOUCH));
        let name = format!("Back to {back}");
        let tap = widgets::icon_button_at(ui, controls, button, &name, Icon::Back, false, true);
        let clip = Rect::from_min_max(pos2(button.right() + 4.0, row.top()), row.max);
        ui.painter()
            .with_clip_rect(clip.intersect(ui.clip_rect()))
            .text(
                pos2(clip.left(), row.center().y),
                Align2::LEFT_CENTER,
                &head.title,
                theme::title(),
                theme::TEXT,
            );
        if tap.clicked() {
            match head.view.as_mut().and_then(|v| v.back.take()) {
                Some(back) => back(state),
                None => panel_title(state, head, true, None),
            }
        }
    }
    let margin = egui::Margin::symmetric(BODY_PAD as i8, 0);
    let top_start = ui.cursor().top();
    egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.set_max_height((bottom - ui.cursor().top()).max(0.0));
        match head.view.as_mut().and_then(|v| v.top.take()) {
            Some(top) => top(ui, state, controls),
            None => panel_top(ui, state, controls, head),
        }
    });
    let fixed = ui.cursor().top() - top_start;
    let most = (bottom - ui.cursor().top()).max(0.0);
    let scroll = match state.pad.dock_drag {
        Some(_) => egui::scroll_area::ScrollSource::NONE,
        None => head.scroll,
    };
    let out = egui::ScrollArea::vertical()
        .id_salt(("pad-pane", head.key))
        .scroll_source(scroll)
        .max_height(most)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let margin = egui::Margin {
                left: BODY_PAD as i8,
                right: BODY_PAD as i8,
                top: 0,
                bottom: 10,
            };
            egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                match head.view.as_mut().and_then(|v| v.body.take()) {
                    Some(body) => body(ui, state, controls),
                    None => panel_body(ui, state, controls, head),
                }
            });
        });
    let back = top_start - start;
    state
        .pad
        .heights
        .insert(head.key, ROW + back + fixed + out.content_size.y);
}

/// The width of a tab, with or without its name.
fn tab_width(ui: &egui::Ui, name: &str, named: bool) -> f32 {
    if !named {
        return TOUCH;
    }
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), theme::body(), theme::TEXT);
    TAB_PAD + ICON + 8.0 + galley.size().x.ceil() + TAB_PAD
}

/// The header of a pane: the title or the tabs, and the action button. A
/// sheet has the Close button too. A long press on a tab or on the title
/// opens the menu of its panel.
#[allow(clippy::too_many_arguments)]
fn header_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    i: usize,
    head: &Head,
    rect: Rect,
    mode: Mode,
    names: bool,
) -> HeaderOut {
    let pane = state.pad.dock().panes[i].clone();
    let name = panel_name(state, pane.panel());
    // The press that opened a menu does not choose or move a tab.
    let holding = state.pad.pane_menu.is_some_and(|m| m.held);
    let mut menu = None;
    let button = |x: f32| Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(TOUCH, TOUCH));
    let mut left_of = rect.right() - PAD;
    let mut close = false;
    if mode == Mode::Sheet {
        let close_rect = button(left_of - TOUCH);
        left_of = close_rect.left();
        let tap = widgets::icon_button_at(
            ui,
            controls,
            close_rect,
            &close_name(name),
            Icon::Close,
            false,
            true,
        );
        close = tap.clicked();
    }
    if let Some((action, icon, active)) = head.action {
        let at = button(left_of - TOUCH);
        left_of = at.left();
        let resp = widgets::icon_button_at(ui, controls, at, action, icon, active, true);
        panel_title(state, head, false, Some(&resp));
    }
    let tabs_area = Rect::from_min_max(
        pos2(rect.left() + PAD, rect.top()),
        pos2(left_of, rect.bottom()),
    );
    let drag_sense = Sense::click_and_drag();
    let mut tab_drags = Vec::new();
    let mut focus = None;
    let mut drag = None;
    let mut centers = Vec::new();
    let mut end = tabs_area.left();
    let mut title_rect = tabs_area;
    let painter = ui
        .painter()
        .with_clip_rect(tabs_area.intersect(ui.clip_rect()));
    if pane.tabs.len() == 1 {
        let icon = Rect::from_center_size(
            pos2(rect.left() + BODY_PAD + ICON / 2.0, rect.center().y),
            vec2(ICON, ICON),
        );
        icons::paint(&painter, icon, panel_icon(pane.panel()), ICON, theme::MUTE);
        let at = pos2(icon.right() + 10.0, rect.center().y);
        let title = painter.text(at, Align2::LEFT_CENTER, name, theme::title(), theme::TEXT);
        title_rect = Rect::from_min_max(
            pos2(rect.left(), rect.top() + PAD),
            pos2(title.right().min(left_of), rect.bottom() - PAD),
        );
        if let Some(subtitle) = &head.subtitle {
            let at = pos2(left_of - 8.0, rect.center().y);
            let galley = painter.layout_no_wrap(subtitle.clone(), theme::body(), theme::MUTE);
            // The subtitle shows only where it does not cover the title.
            if at.x - galley.size().x > title.right() + 12.0 {
                let min = pos2(at.x - galley.size().x, at.y - galley.size().y / 2.0);
                painter.galley(min, galley, theme::MUTE);
            }
        }
        centers.push(tabs_area.center().x);
    } else {
        // Tabs that do not fit drop their names: first the names of the
        // tabs behind, then the name of the open tab.
        let labels: Vec<&str> = pane.tabs.iter().map(|p| panel_name(state, *p)).collect();
        let total = |names: bool, open: bool| -> f32 {
            (labels.iter().enumerate())
                .map(|(t, l)| tab_width(ui, l, (t == pane.active && open) || names))
                .sum()
        };
        let names = names && total(true, true) <= tabs_area.width();
        let open_name = total(names, true) <= tabs_area.width();
        for (t, panel) in pane.tabs.iter().enumerate() {
            let tab_label = labels[t];
            let active = t == pane.active;
            let named = (active && open_name) || names;
            let width = tab_width(ui, tab_label, named);
            let tab = Rect::from_min_size(pos2(end, rect.top() + PAD), vec2(width, TOUCH));
            end = tab.right();
            centers.push(tab.center().x);
            let resp = widgets::hit(
                ui,
                controls,
                tab.intersect(tabs_area),
                &tab_name(tab_label),
                drag_sense,
                true,
                Some(active),
            );
            if active {
                painter.rect_filled(
                    tab.shrink(widgets::HIGHLIGHT_INSET),
                    theme::CONTROL_RADIUS,
                    theme::raise(),
                );
            }
            let color = if active { theme::TEXT } else { theme::MUTE };
            let icon_x = if named {
                tab.left() + TAB_PAD + ICON / 2.0
            } else {
                tab.center().x
            };
            let icon = Rect::from_center_size(pos2(icon_x, tab.center().y), vec2(ICON, ICON));
            icons::paint(&painter, icon, panel_icon(*panel), ICON, color);
            if named {
                let at = pos2(icon.right() + 8.0, tab.center().y);
                painter.text(at, Align2::LEFT_CENTER, tab_label, theme::body(), color);
            }
            if holding {
                continue;
            }
            if long_press(ui, &resp) {
                menu = Some((*panel, tab.intersect(tabs_area)));
                continue;
            }
            if resp.clicked() {
                focus = Some(t);
            }
            if mode == Mode::Sheet {
                tab_drags.push(resp);
                continue;
            }
            drag = drag.or_else(|| {
                tab_drag(
                    ui,
                    &resp,
                    DockDrag::Tab {
                        panel: *panel,
                        pos: Pos2::ZERO,
                    },
                )
            });
        }
    }
    // The empty part of the header. In a pane with one tab, it holds the
    // title too.
    let empty = Rect::from_min_max(
        pos2(end.max(rect.left()), rect.top() + PAD),
        pos2(left_of, rect.bottom() - PAD),
    );
    let empty = if pane.tabs.len() == 1 {
        Rect::from_min_max(pos2(rect.left(), rect.top() + PAD), empty.max)
    } else {
        empty
    };
    let bar = (empty.width() >= TOUCH).then(|| {
        let sense = match mode {
            Mode::Dock => Sense::drag(),
            Mode::Sheet => Sense::click_and_drag(),
        };
        widgets::hit(ui, controls, empty, &move_name(name), sense, true, None)
    });
    if let Some(bar) = &bar
        && !holding
    {
        if pane.tabs.len() == 1 && long_press(ui, bar) {
            menu = Some((pane.panel(), title_rect));
        } else if mode == Mode::Dock {
            let kind = if pane.tabs.len() == 1 {
                DockDrag::Tab {
                    panel: pane.panel(),
                    pos: Pos2::ZERO,
                }
            } else {
                DockDrag::Pane {
                    pane: i,
                    pos: Pos2::ZERO,
                }
            };
            drag = drag.or_else(|| tab_drag(ui, bar, kind));
        }
    }
    if let Some((panel, anchor)) = menu {
        state.pad.pane_menu = Some(PaneMenu {
            panel,
            anchor,
            sheet: mode == Mode::Sheet,
            held: true,
        });
        ui.ctx().request_repaint();
    }
    let tabs = Rect::from_min_max(
        tabs_area.min,
        pos2(
            end.max(tabs_area.left() + TOUCH).min(left_of),
            tabs_area.max.y,
        ),
    );
    let tabs = if pane.tabs.len() == 1 {
        tabs_area
    } else {
        tabs
    };
    HeaderOut {
        close,
        focus,
        bar: if holding { None } else { bar },
        tab_drags,
        drag,
        tabs,
        centers,
    }
}

/// The drag of a response, with the place of the finger.
fn tab_drag(ui: &egui::Ui, resp: &egui::Response, kind: DockDrag) -> Option<(DockDrag, DragPhase)> {
    // A press that stays in place is no drag, even when it is long.
    let moved = ui.input(|i| {
        let origin = i.pointer.press_origin()?;
        Some(origin.distance(i.pointer.interact_pos()?))
    });
    let phase = if resp.drag_stopped() {
        DragPhase::Stopped
    } else if resp.dragged() && moved.is_some_and(|m| m > HOLD_SLOP) {
        DragPhase::Moving
    } else {
        return None;
    };
    let pos = ui
        .ctx()
        .pointer_interact_pos()
        .or(resp.interact_pointer_pos())
        .unwrap_or(resp.rect.center());
    let kind = match kind {
        DockDrag::Pane { pane, .. } => DockDrag::Pane { pane, pos },
        DockDrag::Tab { panel, .. } => DockDrag::Tab { panel, pos },
        DockDrag::Width => DockDrag::Width,
    };
    Some((kind, phase))
}
