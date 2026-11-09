//! Cells with a flyout, and the cells of the tool strip. A cell shows the
//! current choice of its group, and the flyout at the right of the tool
//! column lists all choices of the group. A hold on the cell opens the
//! flyout. A hold on a cell of the tool strip first shows a tooltip with the
//! name of the tool. The owner of the cell decides what a tap does.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use eframe::egui_wgpu::wgpu;

use super::geometry::{self, Rects};
use super::icons::{self, Icon};
use super::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{HOLD_SLOP, HOLD_TIME, ModeGroup, theme};

/// A group of choices with a flyout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flyout {
    /// The modes of a brush cell of the tool strip.
    Brush(ModeGroup),
    /// The modes of the polygon tool, on the brush card.
    PolygonMode,
    /// The tools of the Vector cell of the tool strip.
    Vector,
}

impl Flyout {
    /// The ID of the card. Each flyout keeps its own scroll.
    fn id(self) -> String {
        format!("pad-flyout-{self:?}")
    }
}

/// One row of a flyout.
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub name: &'static str,
    pub icon: Icon,
    pub enabled: bool,
}

/// The time of a hold on a cell of the tool strip that opens the flyout of
/// its modes, in seconds. A shorter hold shows the tooltip.
pub const MODES_TIME: f64 = 0.8;
/// The text of the tooltip of a tool with modes.
pub const HOLD_HINT: &str = "Hold to change mode";

/// The tooltip of a cell of the tool strip that a finger holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    /// The name of the tool.
    pub name: String,
    /// The tool has modes, so the tooltip also shows `HOLD_HINT`.
    pub modes: bool,
    pub cell: Rect,
}

/// The flyouts of the iPad layout.
#[derive(Default)]
pub struct Flyouts {
    /// The open flyout.
    pub open: Option<Flyout>,
    /// The flyout in the last frame.
    pub rect: Option<Rect>,
    /// The tooltip of this frame.
    pub tip: Option<Tip>,
    /// The tooltip in the last frame.
    pub tip_rect: Option<Rect>,
    /// A hold on the cell with this name showed a tooltip or opened a
    /// flyout, so the end of the press is not a tap.
    held: Option<String>,
    /// The cells of this frame.
    cells: Vec<(Flyout, Rect)>,
}

impl Flyouts {
    /// Forgets the cells and the tooltip of the last frame.
    pub fn begin_frame(&mut self) {
        self.cells.clear();
        self.rect = None;
        self.tip = None;
        self.tip_rect = None;
    }

    /// Opens the flyout, or closes it when it is open.
    pub fn toggle(&mut self, key: Flyout) {
        self.open = (self.open != Some(key)).then_some(key);
    }
}

/// A tool cell with a mark in its corner. A hold opens the flyout. Returns
/// true for a tap that is not the end of a hold.
#[allow(clippy::too_many_arguments)]
pub fn cell(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    flyouts: &mut Flyouts,
    key: Flyout,
    name: &str,
    label: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> bool {
    let cell = widgets::tool_cell(ui, controls, name, label, icon, active, enabled);
    flyouts.cells.push((key, cell.rect));
    paint_mark(ui, cell.rect, active, enabled);
    hold(ui, flyouts, &cell, name, None, Some(key))
}

/// A cell of the tool strip: an icon with no name. A hold shows a tooltip
/// with `label`, the name of the tool. For a cell with `modes`, a longer
/// hold opens their flyout and hides the tooltip. Returns true for a tap
/// that is not the end of a hold.
#[allow(clippy::too_many_arguments)]
pub fn tool(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    flyouts: &mut Flyouts,
    modes: Option<Flyout>,
    name: &str,
    label: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> bool {
    let cell = widgets::icon_cell(ui, controls, name, icon, active, enabled);
    if let Some(key) = modes {
        flyouts.cells.push((key, cell.rect));
        paint_mark(ui, cell.rect, active, enabled);
    }
    hold(ui, flyouts, &cell, name, Some(label), modes)
}

/// Follows a press on a cell. With a `label`, a hold of `HOLD_TIME` shows
/// the tooltip, and a hold of `MODES_TIME` opens the flyout of `modes`.
/// With no label, a hold of `HOLD_TIME` opens the flyout. Returns true for
/// a tap that is not the end of a hold.
fn hold(
    ui: &egui::Ui,
    flyouts: &mut Flyouts,
    cell: &egui::Response,
    name: &str,
    label: Option<&str>,
    modes: Option<Flyout>,
) -> bool {
    let held = flyouts.held.as_deref() == Some(name);
    let tapped = cell.clicked() && !held;
    // A press that is too long for a click no longer counts as a press on
    // a control that takes clicks only, so a held press is followed here.
    let still = held
        && ui.input(|i| {
            i.pointer.primary_down()
                && i.pointer
                    .press_origin()
                    .is_some_and(|p| cell.rect.contains(p))
        });
    if !cell.is_pointer_button_down_on() && !still {
        if held {
            flyouts.held = None;
        }
        return tapped;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    let Some((time, _)) = press.filter(|&(_, moved)| moved <= HOLD_SLOP) else {
        return tapped;
    };
    let open_at = if label.is_some() {
        MODES_TIME
    } else {
        HOLD_TIME
    };
    let wait = |until: f64| {
        let wait = std::time::Duration::from_secs_f64((until - time).max(0.0));
        ui.ctx().request_repaint_after(wait);
    };
    if time < HOLD_TIME {
        wait(HOLD_TIME);
        return tapped;
    }
    if !held {
        flyouts.held = Some(name.to_owned());
        ui.ctx().request_repaint();
    }
    match (modes, label) {
        (Some(key), _) if time >= open_at => flyouts.open = Some(key),
        (modes, Some(label)) => {
            flyouts.tip = Some(Tip {
                name: label.to_owned(),
                modes: modes.is_some(),
                cell: cell.rect,
            });
            if modes.is_some() {
                wait(open_at);
            }
        }
        _ => {}
    }
    tapped
}

/// The width of the space between a tooltip and the tool strip.
const TIP_GAP: f32 = 8.0;
/// The height of a tooltip.
const TIP_HEIGHT: f32 = 40.0;

/// The tooltip of the held cell of the tool strip, at the right of the
/// strip, or above the strip in the row layout.
pub fn tooltip(
    ctx: &egui::Context,
    backdrop: Option<wgpu::TextureFormat>,
    flyouts: &mut Flyouts,
    rects: &Rects,
) {
    let Some(tip) = flyouts.tip.clone() else {
        return;
    };
    let width = |text: &str, font: egui::FontId| {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, theme::TEXT));
        galley.size().x.ceil()
    };
    let name = width(&tip.name, theme::body());
    let hint = if tip.modes {
        10.0 + width(HOLD_HINT, theme::hint())
    } else {
        0.0
    };
    let size = vec2(14.0 + name + hint + 14.0, TIP_HEIGHT);
    let rect = if rects.row {
        let bottom = rects.tools.top() - TIP_GAP;
        let screen = rects.screen.shrink2(vec2(geometry::MARGIN, 0.0));
        let left = (tip.cell.center().x - size.x / 2.0)
            .min(screen.right() - size.x)
            .max(screen.left());
        Rect::from_min_size(pos2(left, bottom - size.y), size)
    } else {
        let min = pos2(
            rects.tools.right() + TIP_GAP,
            tip.cell.center().y - size.y / 2.0,
        );
        Rect::from_min_size(min, size)
    };
    flyouts.tip_rect = Some(rect);
    let radius = theme::CONTROL_RADIUS.into();
    widgets::card(
        ctx,
        backdrop,
        "pad-tooltip",
        rect,
        Order::Tooltip,
        radius,
        |ui| {
            let painter = ui.painter();
            // A small point toward the cell.
            let point = if rects.row {
                let x = tip
                    .cell
                    .center()
                    .x
                    .clamp(rect.left() + 12.0, rect.right() - 12.0);
                vec![
                    pos2(x - 6.0, rect.bottom()),
                    pos2(x + 6.0, rect.bottom()),
                    pos2(x, rect.bottom() + 6.0),
                ]
            } else {
                let y = rect.center().y;
                vec![
                    pos2(rect.left(), y - 6.0),
                    pos2(rect.left(), y + 6.0),
                    pos2(rect.left() - 6.0, y),
                ]
            };
            let fill = theme::card(backdrop.is_some());
            painter.add(egui::Shape::convex_polygon(point, fill, egui::Stroke::NONE));
            let at = pos2(rect.left() + 14.0, rect.center().y);
            painter.text(
                at,
                Align2::LEFT_CENTER,
                &tip.name,
                theme::body(),
                theme::TEXT,
            );
            if tip.modes {
                let at = at + vec2(name + 10.0, 0.0);
                painter.text(
                    at,
                    Align2::LEFT_CENTER,
                    HOLD_HINT,
                    theme::hint(),
                    theme::MUTE,
                );
            }
        },
    );
}

/// The mark in the corner of a cell with a flyout.
fn paint_mark(ui: &egui::Ui, cell: Rect, active: bool, enabled: bool) {
    let color = widgets::fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
    let corner = cell.right_bottom() - vec2(6.0, 6.0);
    let points = vec![corner, corner - vec2(6.0, 0.0), corner - vec2(0.0, 6.0)];
    let shape = egui::Shape::convex_polygon(points, color, egui::Stroke::NONE);
    ui.painter().add(shape);
}

/// The open flyout, at the right of the tool column, next to its cell, or
/// above the row of its cell in the row layout. A tick marks the row of
/// `current`. Returns the row that the user chose. A choice or a press
/// outside the flyout and its cell closes it.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    backdrop: Option<wgpu::TextureFormat>,
    controls: &mut Controls,
    flyouts: &mut Flyouts,
    rects: &Rects,
    choices: &[Choice],
    current: Option<usize>,
) -> Option<usize> {
    let key = flyouts.open?;
    let Some(&(_, cell)) = flyouts.cells.iter().find(|(k, _)| *k == key) else {
        flyouts.open = None;
        return None;
    };
    let rows = choices.len() as f32;
    let full = 10.0 + rows * PANEL_ROW + 10.0;
    // In the row layout, the list is above its cell. Else it is at the
    // right of the strip.
    let rect = if rects.row {
        // A cell of the card of the tool has the list above the card.
        let card = rects.brush.filter(|card| card.contains_rect(cell));
        let bottom = card.unwrap_or(rects.tools).top() - geometry::GAP;
        let size = vec2(geometry::MODES_WIDTH, full.min(bottom - rects.column_top));
        let screen = rects.screen.shrink2(vec2(geometry::MARGIN, 0.0));
        let left = cell.left().min(screen.right() - size.x).max(screen.left());
        Rect::from_min_size(pos2(left, bottom - size.y), size)
    } else {
        let room = rects.panel_bottom - rects.tools.top();
        let size = vec2(geometry::MODES_WIDTH, full.min(room));
        let top = cell
            .top()
            .min(rects.panel_bottom - size.y)
            .max(rects.tools.top());
        Rect::from_min_size(pos2(rects.tools.right() + geometry::GAP, top), size)
    };
    flyouts.rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !cell.contains(pos)) {
        flyouts.open = None;
    }
    let mut chosen = None;
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        backdrop,
        &key.id(),
        rect,
        Order::Foreground,
        radius,
        |ui| {
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            ui.set_clip_rect(inner);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(&mut ui, |ui| {
                    for (index, choice) in choices.iter().enumerate() {
                        let (name, enabled) = (choice.name, choice.enabled);
                        let row = widgets::text_row(ui, controls, name, 32.0, false, enabled);
                        let at = pos2(row.rect.left() + 11.0, row.rect.center().y);
                        let icon_rect =
                            Rect::from_center_size(at, vec2(widgets::ICON, widgets::ICON));
                        let color = widgets::fade(theme::TEXT, enabled);
                        icons::paint(ui.painter(), icon_rect, choice.icon, widgets::ICON, color);
                        if current == Some(index) {
                            let tick = Rect::from_center_size(
                                pos2(row.rect.right() - 14.0, row.rect.center().y),
                                vec2(18.0, 18.0),
                            );
                            icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                        }
                        if row.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
        },
    );
    if chosen.is_some() {
        flyouts.open = None;
    }
    chosen
}
