//! The Panels menu: a tile for each panel of the workspace, and the rows
//! "Close all" and "Reset layout". A tinted tile is an open panel.

use eframe::egui::{self, Align2, Order, Rect, Sense, pos2, vec2};

use super::geometry::{self, PANELS_MENU_WIDTH, Rects};
use super::icons::{self, Icon};
use super::widgets::{self, Controls, HIGHLIGHT_INSET, ICON, PANEL_ROW};
use super::{PANELS, Panel, theme};
use crate::ui::{AppState, Workspace, select};

pub const BRUSH: &str = "Brush";
pub const PAGES: &str = "Pages";
pub const CLOSE_ALL: &str = "Close all";
pub const RESET_LAYOUT: &str = "Reset layout";
pub const WORLD: &str = "World";
pub const SELECTION: &str = "Selection";
pub const ATLAS: &str = "Atlas";
/// The name of the control of the Snap tile, which differs from the name
/// of the Snap cell of the cards.
pub const SNAP_TILE: &str = "Snap panel";

/// The padding of the menu.
const PAD: f32 = 12.0;
const TILE_GAP: f32 = 8.0;
const COLUMNS: usize = 4;
/// The height of a tile.
const TILE: f32 = 78.0;
/// The height of the heading of a group.
const HEADING: f32 = 28.0;

/// A tile of the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub entry: Panel,
    /// The name on the tile.
    pub name: &'static str,
    /// The name of the control.
    pub control: &'static str,
    pub icon: Icon,
    pub open: bool,
}

/// A group of tiles under a heading.
pub struct Group {
    pub heading: &'static str,
    pub tiles: Vec<Tile>,
}

/// The tiles of the World workspace.
pub fn world(state: &AppState) -> Vec<Group> {
    let panel = |entry: Panel, name, icon| Tile {
        entry,
        name,
        control: if entry == Panel::Snap {
            SNAP_TILE
        } else {
            name
        },
        icon,
        open: state.pad.is_open(entry),
    };
    vec![Group {
        heading: WORLD,
        tiles: vec![
            panel(Panel::Layers, "Layers", Icon::Layers),
            panel(Panel::Attributes, select::ATTRIBUTES, Icon::Attributes),
            panel(Panel::Toolbox, super::TOOLBOX, Icon::Toolbox),
            panel(Panel::Brush, BRUSH, Icon::Brush),
            panel(Panel::Snap, super::SNAP, Icon::Magnet),
        ],
    }]
}

/// The tiles of the Atlas workspace.
pub fn atlas(state: &AppState) -> Vec<Group> {
    let panel = |entry: Panel, name, icon| Tile {
        entry,
        name,
        control: if entry == Panel::Snap {
            SNAP_TILE
        } else {
            name
        },
        icon,
        open: state.pad.is_open(entry),
    };
    let [_, projection, layers, labels] = super::atlas::FRAME_TABS;
    vec![
        Group {
            heading: SELECTION,
            tiles: vec![
                panel(
                    Panel::Properties,
                    super::atlas::PROPERTIES,
                    Icon::Properties,
                ),
                panel(Panel::Projection, projection, Icon::Projection),
                panel(Panel::Layers, layers, Icon::Layers),
                panel(Panel::Labels, labels, Icon::Tag),
            ],
        },
        Group {
            heading: ATLAS,
            tiles: vec![
                panel(Panel::Pages, PAGES, Icon::Pages),
                panel(Panel::Attributes, select::ATTRIBUTES, Icon::Attributes),
                panel(Panel::Brush, BRUSH, Icon::Brush),
                panel(Panel::Snap, super::SNAP, Icon::Magnet),
            ],
        },
    ]
}

/// The height of the menu with these groups.
fn height(groups: &[Group]) -> f32 {
    let tiles: f32 = groups
        .iter()
        .map(|g| {
            let rows = g.tiles.len().div_ceil(COLUMNS) as f32;
            HEADING + rows * TILE + (rows - 1.0).max(0.0) * TILE_GAP + TILE_GAP
        })
        .sum();
    PAD + tiles + PANEL_ROW + PAD
}

/// The menu, on the right edge below the top row. A tap outside the menu
/// and outside the Panels button closes it.
pub fn show(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    groups: &[Group],
) {
    let collapsed = state.pad.dock().collapsed;
    let size = vec2(PANELS_MENU_WIDTH + rects.bands.right, height(groups));
    let rect = Rect::from_min_size(pos2(rects.screen.right() - size.x, rects.column_top), size);
    state.pad.panels_menu_rect = Some(rect);
    let button = controls.iter().find(|c| c.name == PANELS).map(|c| c.rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let outside = |pos: egui::Pos2| !rect.contains(pos) && !button.is_some_and(|b| b.contains(pos));
    if pressed.is_some_and(outside) {
        state.pad.panels_menu = false;
    }
    let radius = geometry::edge_radius(rect, rects.screen);
    let inner = geometry::edge_content(rect, rects.screen, rects.bands).shrink(PAD);
    let mut chosen = None;
    let mut close = false;
    let mut reset = false;
    let backdrop = state.globe.backdrop();
    widgets::card(
        ctx,
        backdrop,
        "pad-panels-menu",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            let width = (inner.width() - (COLUMNS - 1) as f32 * TILE_GAP) / COLUMNS as f32;
            let mut top = inner.top();
            for group in groups {
                let at = pos2(inner.left() + HIGHLIGHT_INSET, top + HEADING / 2.0);
                let heading = group.heading.to_uppercase();
                ui.painter()
                    .text(at, Align2::LEFT_CENTER, heading, theme::hint(), theme::MUTE);
                top += HEADING;
                for (index, tile) in group.tiles.iter().enumerate() {
                    let (row, column) = (index / COLUMNS, index % COLUMNS);
                    let min = pos2(
                        inner.left() + column as f32 * (width + TILE_GAP),
                        top + row as f32 * (TILE + TILE_GAP),
                    );
                    let cell = Rect::from_min_size(min, vec2(width, TILE));
                    if paint_tile(ui, controls, cell, tile) {
                        chosen = Some(*tile);
                    }
                }
                let rows = group.tiles.len().div_ceil(COLUMNS) as f32;
                top += rows * TILE + (rows - 1.0).max(0.0) * TILE_GAP + TILE_GAP;
            }
            let half = (inner.width() - TILE_GAP) / 2.0;
            let left = Rect::from_min_size(pos2(inner.left(), top), vec2(half, PANEL_ROW));
            let right = left.translate(vec2(half + TILE_GAP, 0.0));
            close = text_button(ui, controls, left, CLOSE_ALL);
            reset = text_button(ui, controls, right, RESET_LAYOUT);
        },
    );
    if let Some(tile) = chosen {
        // A tile of an open panel of a collapsed dock shows the panel.
        if collapsed && tile.open {
            state.pad.open(tile.entry);
            state.pad.dock_mut().collapsed = false;
        } else {
            state.pad.toggle(tile.entry);
        }
        state.pad.panels_menu = false;
    }
    if close {
        state.pad.dock_mut().close_all();
        state.pad.panels_menu = false;
    }
    if reset {
        state.pad.dock_mut().reset();
        // A selected frame opens its Frame panel.
        if state.workspace == Workspace::Atlas && state.atlas.frame().is_some() {
            state.pad.dock_mut().follow_frame(Panel::Properties);
        }
        state.pad.panels_menu = false;
    }
}

/// A tile: the icon of the panel above its name. Returns true for a tap.
fn paint_tile(ui: &egui::Ui, controls: &mut Controls, rect: Rect, tile: &Tile) -> bool {
    let resp = widgets::hit(
        ui,
        controls,
        rect,
        tile.control,
        Sense::click(),
        true,
        Some(tile.open),
    );
    let fill = if tile.open {
        theme::ACCENT.gamma_multiply(0.3)
    } else {
        theme::track()
    };
    let fill = if resp.is_pointer_button_down_on() {
        fill.blend(theme::raise())
    } else {
        fill
    };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CONTROL_RADIUS, fill);
    let color = if tile.open {
        theme::ACCENT
    } else {
        theme::MUTE
    };
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + 26.0), vec2(ICON, ICON));
    icons::paint(painter, icon, tile.icon, ICON, color);
    let color = if tile.open { theme::TEXT } else { theme::MUTE };
    let at = pos2(rect.center().x, rect.bottom() - 18.0);
    painter.text(at, Align2::CENTER_CENTER, tile.name, theme::hint(), color);
    resp.clicked()
}

/// A button with a text in the middle. Returns true for a tap.
fn text_button(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str) -> bool {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let fill = if resp.is_pointer_button_down_on() {
        theme::raise()
    } else {
        theme::track()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CONTROL_RADIUS, fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        theme::TEXT,
    );
    resp.clicked()
}
