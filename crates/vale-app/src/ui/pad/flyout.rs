//! Cells with a flyout. A cell shows the current choice of its group, and
//! the flyout at the right of the tool column lists all choices of the
//! group. A hold on the cell opens the flyout. The owner of the cell
//! decides what a tap does.

use eframe::egui::{self, Order, Rect, pos2, vec2};
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
    /// A line above the row divides it from the rows before.
    pub divider: bool,
}

/// The height of a divider between two rows of a flyout.
const DIVIDER: f32 = 9.0;

/// The flyouts of the iPad layout.
#[derive(Default)]
pub struct Flyouts {
    /// The open flyout.
    pub open: Option<Flyout>,
    /// The flyout in the last frame.
    pub rect: Option<Rect>,
    /// A hold on this cell opened the flyout, so the end of the press is
    /// not a tap.
    held: Option<Flyout>,
    /// The cells of this frame.
    cells: Vec<(Flyout, Rect)>,
}

impl Flyouts {
    /// Forgets the cells of the last frame.
    pub fn begin_frame(&mut self) {
        self.cells.clear();
        self.rect = None;
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
    let held = flyouts.held == Some(key);
    let tapped = cell.clicked() && !held;
    if !cell.is_pointer_button_down_on() {
        if held {
            flyouts.held = None;
        }
        return tapped;
    }
    if held {
        return tapped;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    match press {
        Some((held, moved)) if moved <= HOLD_SLOP && held >= HOLD_TIME => {
            flyouts.held = Some(key);
            flyouts.open = Some(key);
            ui.ctx().request_repaint();
        }
        Some((held, moved)) if moved <= HOLD_SLOP => {
            let wait = std::time::Duration::from_secs_f64(HOLD_TIME - held);
            ui.ctx().request_repaint_after(wait);
        }
        _ => {}
    }
    tapped
}

/// The mark in the corner of a cell with a flyout.
fn paint_mark(ui: &egui::Ui, cell: Rect, active: bool, enabled: bool) {
    let color = widgets::fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
    let corner = cell.right_bottom() - vec2(6.0, 6.0);
    let points = vec![corner, corner - vec2(6.0, 0.0), corner - vec2(0.0, 6.0)];
    let shape = egui::Shape::convex_polygon(points, color, egui::Stroke::NONE);
    ui.painter().add(shape);
}

/// The open flyout, at the right of the tool column, next to its cell. A
/// tick marks the row of `current`. Returns the row that the user chose. A
/// choice or a press outside the flyout and its cell closes it.
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
    let dividers = choices.iter().filter(|c| c.divider).count() as f32;
    let room = rects.panel_bottom - rects.tools.top();
    let size = vec2(
        geometry::MODES_WIDTH,
        (10.0 + rows * PANEL_ROW + dividers * DIVIDER + 10.0).min(room),
    );
    let top = cell
        .top()
        .min(rects.panel_bottom - size.y)
        .max(rects.tools.top());
    let min = pos2(rects.tools.right() + geometry::GAP, top);
    let rect = Rect::from_min_size(min, size);
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
                        if choice.divider {
                            let (line, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), DIVIDER),
                                egui::Sense::hover(),
                            );
                            let y = line.center().y;
                            let stroke = egui::Stroke::new(1.0, theme::divider());
                            ui.painter().hline(line.x_range(), y, stroke);
                        }
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
