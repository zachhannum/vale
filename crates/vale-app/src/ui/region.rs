//! The command that makes a map from a region: a box drag on the globe
//! makes a map with a suggested projection, in a new frame on the open page.

use eframe::egui::{self, Order, Pos2, Rect, pos2, vec2};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{AppState, Workspace, atlas};
use crate::globe::Tool;
use crate::globe::region::region_of_box;
use crate::globe::select::{SelectEvent, drag_rect};

/// The command, in the Add menu of the atlas.
pub const MAP_FROM_REGION: &str = "Map from region";
/// The hint while the command waits for the drag.
pub const HINT: &str = "Drag a region on the globe.";
/// The button that ends the command on the desktop.
pub const CANCEL: &str = "Cancel";
/// The hint when the box holds no place on the globe.
const MISSED: &str = "The region is not on the globe.";
/// The least size of the box on each side, in points.
const LEAST: f32 = 8.0;
const WIDTH: f32 = 360.0;
const CARD: &str = "region-card";

/// The command while it waits for the drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionMode {
    /// The tool of the globe before the command.
    back: Tool,
    /// The box from its start to its end, in screen points.
    pub drag: Option<(Pos2, Pos2)>,
}

/// Starts the command: the globe workspace opens, and a pen or a mouse drag
/// makes the region.
pub fn start(state: &mut AppState) {
    if state.region.is_none() {
        state.region = Some(RegionMode {
            back: state.globe.tool,
            drag: None,
        });
    }
    state.globe.tool = Tool::Navigate;
    state.globe.region = true;
    state.workspace = Workspace::Globe;
}

/// Ends the command and opens the atlas.
pub fn cancel(state: &mut AppState) {
    end(state);
    state.workspace = Workspace::Atlas;
}

/// Ends the command, and puts back the tool of the globe.
fn end(state: &mut AppState) {
    if let Some(mode) = state.region.take()
        && state.globe.tool == Tool::Navigate
    {
        state.globe.tool = mode.back;
    }
    state.globe.region = false;
}

impl AppState {
    /// Reads the drag of the command. A choice of another tool or of the
    /// atlas ends the command.
    pub fn poll_region(&mut self) {
        let Some(mut mode) = self.region else {
            self.globe.region = false;
            return;
        };
        if self.globe.tool != Tool::Navigate || self.workspace != Workspace::Globe {
            end(self);
            return;
        }
        let events = std::mem::take(&mut self.globe.select_press.events);
        for event in events {
            match event {
                SelectEvent::DragStart { pos, .. } => mode.drag = Some((pos, pos)),
                SelectEvent::Drag { pos } => {
                    if let Some((_, end)) = &mut mode.drag {
                        *end = pos;
                    }
                }
                SelectEvent::DragEnd { pos } => {
                    let Some((start, _)) = mode.drag.take() else {
                        continue;
                    };
                    let rect = drag_rect(start, pos);
                    if rect.width() < LEAST || rect.height() < LEAST {
                        continue;
                    }
                    let Some(region) = region_of_box(&self.globe, rect) else {
                        self.status = MISSED.to_owned();
                        continue;
                    };
                    self.region = Some(mode);
                    if atlas::add_region_map(self, &region).is_some() {
                        cancel(self);
                        self.touch();
                    }
                    return;
                }
                SelectEvent::Tap { .. } => {}
                SelectEvent::Cancel => mode.drag = None,
            }
        }
        self.region = Some(mode);
    }
}

/// The box of the drag.
pub fn paint(painter: &egui::Painter, state: &AppState) {
    let Some(RegionMode {
        drag: Some((start, end)),
        ..
    }) = state.region
    else {
        return;
    };
    let ring = egui::Stroke::new(2.0, theme::ACCENT);
    let fill = theme::ACCENT.gamma_multiply(0.15);
    painter.rect(
        drag_rect(start, end),
        0.0,
        fill,
        ring,
        egui::StrokeKind::Inside,
    );
}

/// The card of the command in the iPad layout, near the top of the screen.
/// Its close button ends the command.
pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    if state.region.is_none() || state.workspace != Workspace::Globe {
        return;
    }
    let screen = rects.screen;
    let top = rects.workspace.bottom() + GAP;
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, top),
        vec2(width, ROW + PANEL_ROW),
    );
    let mut close = false;
    let radius = theme::CARD_RADIUS.into();
    let backdrop = state.globe.backdrop();
    widgets::card(ctx, backdrop, CARD, rect, Order::Foreground, radius, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        close = widgets::title_row(ui, controls, MAP_FROM_REGION, None, false).close;
        let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.add(egui::Label::new(egui::RichText::new(HINT).color(theme::MUTE)).truncate());
    });
    if close {
        cancel(state);
    }
}

/// The window of the command in the desktop layout. Escape ends the command.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    if state.region.is_none() || state.workspace != Workspace::Globe {
        return;
    }
    let mut close = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    egui::Window::new(MAP_FROM_REGION)
        .id(egui::Id::new(CARD))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 48.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong(MAP_FROM_REGION);
                ui.label(HINT);
                close |= ui.button(CANCEL).clicked();
            });
        });
    if close {
        cancel(state);
    }
}
