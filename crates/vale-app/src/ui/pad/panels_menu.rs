//! The Panels menu: a tile for each panel of the workspace, and the rows
//! "Close all" and "Reset layout". A tinted tile is an open panel. A tile
//! is off when its panel does nothing.

use eframe::egui::{self, Align2, Order, Rect, Sense, pos2, vec2};

use super::geometry::{self, PANELS_MENU_WIDTH, Rects};
use super::icons::{self, Icon};
use super::widgets::{self, Controls, HIGHLIGHT_INSET, ICON, PANEL_ROW};
use super::{PANELS, Panel, card_cells, has_strip, theme};
use crate::ui::atlas::{self, AtlasSelection, FrameTab};
use crate::ui::{AppState, Workspace, select};

pub const BRUSH: &str = "Brush";
pub const PAGES: &str = "Pages";
pub const CLOSE_ALL: &str = "Close all";
pub const RESET_LAYOUT: &str = "Reset layout";
pub const WORLD: &str = "World";
pub const SELECTION: &str = "Selection";
pub const ATLAS: &str = "Atlas";

/// The padding of the menu.
const PAD: f32 = 12.0;
const TILE_GAP: f32 = 8.0;
const COLUMNS: usize = 4;
/// The height of a tile.
const TILE: f32 = 78.0;
/// The height of the heading of a group.
const HEADING: f32 = 28.0;

/// A panel that a tile of the menu opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Panel(Panel),
    /// A tab of the panel of the selected frame.
    Frame(FrameTab),
    Pages,
    Maps,
}

/// A tile of the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub entry: Entry,
    pub name: &'static str,
    pub icon: Icon,
    pub open: bool,
    pub pinned: bool,
    pub enabled: bool,
}

/// A group of tiles under a heading.
pub struct Group {
    pub heading: &'static str,
    pub tiles: Vec<Tile>,
}

fn tile(entry: Entry, name: &'static str, icon: Icon, open: bool, enabled: bool) -> Tile {
    Tile {
        entry,
        name,
        icon,
        open: open && enabled,
        pinned: false,
        enabled,
    }
}

/// The tiles of the World workspace.
pub fn world(state: &AppState) -> Vec<Group> {
    let pad = &state.pad;
    let brush =
        (has_strip(&state.globe) || card_cells(&state.globe) > 0) && !state.globe.selecting();
    let panel = |panel: Panel, name, icon, enabled| {
        tile(Entry::Panel(panel), name, icon, pad.is_open(panel), enabled)
    };
    vec![Group {
        heading: WORLD,
        tiles: vec![
            panel(Panel::Layers, "Layers", Icon::Layers, true),
            panel(
                Panel::Attributes,
                select::ATTRIBUTES,
                Icon::Attributes,
                true,
            ),
            panel(Panel::Toolbox, super::TOOLBOX, Icon::Toolbox, true),
            panel(Panel::Brush, BRUSH, Icon::Brush, brush),
        ],
    }]
}

/// The tiles of the Atlas workspace.
pub fn atlas(state: &AppState) -> Vec<Group> {
    // While the Measure tool is the tool, its Brush panel is the panel.
    let measuring = state.measuring();
    let attributes = !measuring && state.pad.is_open(Panel::Attributes);
    let shown = !measuring && !attributes;
    let a = &state.atlas;
    let frame = !measuring && a.frame().is_some();
    let tab = |tab: FrameTab, name, icon| {
        tile(Entry::Frame(tab), name, icon, shown && a.tab == tab, frame)
    };
    let [frame_name, projection, layers, labels] = super::atlas::FRAME_TABS;
    vec![
        Group {
            heading: SELECTION,
            tiles: vec![
                tab(FrameTab::Frame, frame_name, Icon::Frame),
                tab(FrameTab::Projection, projection, Icon::Projection),
                tab(FrameTab::Layers, layers, Icon::Layers),
                tab(FrameTab::Labels, labels, Icon::Tag),
            ],
        },
        Group {
            heading: ATLAS,
            tiles: vec![
                tile(
                    Entry::Pages,
                    PAGES,
                    Icon::Pages,
                    shown && a.selection == AtlasSelection::Page,
                    !measuring,
                ),
                tile(
                    Entry::Maps,
                    super::atlas::MAPS,
                    Icon::Map,
                    shown && a.frames && a.selection == AtlasSelection::None,
                    !measuring,
                ),
                tile(
                    Entry::Panel(Panel::Attributes),
                    select::ATTRIBUTES,
                    Icon::Attributes,
                    attributes,
                    !measuring,
                ),
                tile(
                    Entry::Panel(Panel::Brush),
                    BRUSH,
                    Icon::Brush,
                    state.pad.brush,
                    measuring,
                ),
            ],
        },
    ]
}

/// Opens the panel of a tile, or closes it when it is open.
fn toggle(state: &mut AppState, rects: &Rects, tile: &Tile) {
    match (tile.entry, state.workspace) {
        (Entry::Panel(Panel::Attributes), Workspace::Atlas) => {
            if tile.open {
                state.pad.close(Panel::Attributes);
            } else {
                state.pad.open(Panel::Attributes);
                state.atlas.frames = false;
            }
        }
        (Entry::Panel(panel), _) => state.pad.toggle(panel, rects.one_panel),
        (Entry::Frame(_) | Entry::Pages, _) if tile.open => atlas::clear_selection(state),
        (Entry::Maps, _) if tile.open => state.atlas.frames = false,
        // The panel of the selection and the inspector share the panel
        // place, so the inspector closes.
        (entry, _) => {
            state.pad.close(Panel::Attributes);
            state.atlas.frames = false;
            match entry {
                Entry::Frame(tab) => state.atlas.tab = tab,
                Entry::Pages => {
                    atlas::clear_selection(state);
                    state.atlas.selection = AtlasSelection::Page;
                }
                _ => {
                    atlas::clear_selection(state);
                    state.atlas.frames = true;
                }
            }
        }
    }
}

/// Closes each panel of the workspace.
fn close_all(state: &mut AppState) {
    state.pad.brush = false;
    state.pad.right = None;
    if state.workspace == Workspace::Atlas {
        state.atlas.frames = false;
        if state.atlas.selection != AtlasSelection::None {
            atlas::clear_selection(state);
        }
    }
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
    widgets::card(
        ctx,
        None,
        "pad-panels-menu",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The menu covers the panels, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
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
        toggle(state, rects, &tile);
        state.pad.panels_menu = false;
    }
    if close {
        close_all(state);
        state.pad.panels_menu = false;
    }
    // The panels have no layout to reset yet.
    if reset {
        state.pad.panels_menu = false;
    }
}

/// A tile: the icon of the panel above its name. Returns true for a tap.
fn paint_tile(ui: &egui::Ui, controls: &mut Controls, rect: Rect, tile: &Tile) -> bool {
    let resp = widgets::hit(
        ui,
        controls,
        rect,
        tile.name,
        Sense::click(),
        tile.enabled,
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
    icons::paint(
        painter,
        icon,
        tile.icon,
        ICON,
        widgets::fade(color, tile.enabled),
    );
    let color = if tile.open { theme::TEXT } else { theme::MUTE };
    let at = pos2(rect.center().x, rect.bottom() - 18.0);
    let color = widgets::fade(color, tile.enabled);
    painter.text(at, Align2::CENTER_CENTER, tile.name, theme::hint(), color);
    if tile.pinned {
        let pin = Rect::from_center_size(rect.right_top() + vec2(-14.0, 14.0), vec2(14.0, 14.0));
        icons::paint(painter, pin, Icon::Pin, 14.0, theme::ACCENT);
    }
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
