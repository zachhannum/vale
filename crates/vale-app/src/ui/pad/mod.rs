//! The iPad layout of the world workspace: a full-screen canvas with
//! floating cards.

use std::collections::HashMap;

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_sphere::{LonLat, ProjectionKind, ProjectionSpec};
use vale_store::{GeometryKind, Layer, LayerId, LayerKind, RasterId};
use vale_terrain::Mode;

pub use super::LayerRef;
use super::raster_form::{self, RasterForm};
use super::world_form::{self, WorldForm};
use super::{Action, AppState, Note, SaveMark, Workspace, globe, is_height, panels};
use crate::globe::brush::{
    EROSION, FLOW, SCALE_KM, SEED, STRENGTH_M, TALUS_DEG, WARP_KM, needs_height,
};
use crate::globe::{Tool, WorldView};
use crate::session::{WorldFile, worlds_of};
use crate::world;

pub mod browser;
pub mod geometry;
pub mod icons;
pub mod raster;
pub mod theme;
pub mod widgets;

use geometry::{BrushCard, Input, PAD, ROW, Rects, TOOLS, TOUCH, WidthClass};
use icons::Icon;
use widgets::{BODY_PAD, Controls, PANEL_ROW};

/// The control of the tool strip that opens the menu of the other modes.
pub const MORE: &str = "More";
/// The modes of the menu of the More cell.
const MORE_MODES: [(&str, Icon, Mode); 7] = [
    ("Carve", Icon::Carve, Mode::Carve),
    ("Ridge", Icon::Ridge, Mode::Ridge),
    ("Deposit", Icon::Deposit, Mode::Deposit),
    ("Erode", Icon::Erode, Mode::Erode),
    ("Weather", Icon::Weather, Mode::Weather),
    ("Uplift", Icon::Uplift, Mode::Uplift),
    ("Warp", Icon::Warp, Mode::Warp),
];

/// The name of the row that opens the properties of the world.
pub const WORLD_PROPERTIES: &str = "World properties";
/// The row of the menu of the worlds that saves the open project.
pub const SAVE: &str = "Save";
/// The side of the save mark in the readout.
const SAVE_MARK: f32 = 14.0;
/// The button that opens the menu of the worlds.
pub const WORLDS: &str = "Worlds";
/// The row of the workspace menu that opens the menu of the worlds.
pub const WORLDS_ROW: &str = "Worlds…";
/// The width that the Worlds button and its line add to the workspace card.
const WORLDS_LEAD: f32 = TOUCH + 9.0;
/// The largest number of worlds that the menu shows with no scroll.
const WORLD_ROWS: usize = 6;
/// The name of the row that imports a heightmap into a height layer.
pub const IMPORT_HEIGHTMAP: &str = "Import heightmap…";
/// The name of the row that exports the heightmap.
pub const EXPORT_GEOTIFF: &str = "Export GeoTIFF…";

/// The button of the Layers panel that opens the menu of the new layers.
pub const ADD_LAYER: &str = "Add a layer";
/// The button of the panel of a vector layer that deletes the layer.
pub const DELETE_LAYER: &str = "Delete layer";
/// The field of the panel of a vector layer with the name of the layer.
pub const LAYER_NAME: &str = "Layer name";
/// The button of the panel of a raster layer that deletes the layer.
pub const DELETE_RASTER: &str = "Delete raster layer";
/// The slider of the panel of a raster layer that sets its opacity.
pub const OPACITY: &str = "Layer opacity";
/// The row of the menu of the new layers that opens the form of a new raster
/// layer.
pub const NEW_RASTER: &str = "Raster";
/// The rows of the menu of the new vector layers.
const NEW_LAYERS: [(&str, GeometryKind); 3] = [
    ("Lines", GeometryKind::Line),
    ("Points", GeometryKind::Point),
    ("Polygons", GeometryKind::Polygon),
];
/// The width of the menu of the new layers.
const ADD_WIDTH: f32 = 220.0;
/// The time that a finger holds a row of the layer list before the row
/// moves with the finger, in seconds.
pub const HOLD_TIME: f64 = 0.4;
/// The largest distance that a finger moves during the hold, in points.
const HOLD_SLOP: f32 = 6.0;

/// A row of the layer list that moves with a finger.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Reorder {
    pub layer: LayerId,
    /// The distance from the top of the row to the finger.
    grab: f32,
}

/// A panel that a button of the layout opens.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Panel {
    Brush,
    Layers,
    Toolbox,
}

impl Panel {
    fn key(self) -> &'static str {
        match self {
            Panel::Brush => "pad-brush",
            Panel::Layers => "pad-layers",
            Panel::Toolbox => "pad-toolbox",
        }
    }
}

/// The state of the iPad layout.
#[derive(Default)]
pub struct PadState {
    /// The Brush panel is open.
    pub brush: bool,
    /// The panel at the right: Layers, Toolbox, or none.
    pub right: Option<Panel>,
    /// The layer that the Layers panel shows. `None`: the panel shows the
    /// list of the layers.
    pub layer: Option<LayerRef>,
    /// The menu of the new layers is open.
    pub add: bool,
    /// The menu of the new layers in the last frame.
    pub add_rect: Option<Rect>,
    /// The button of this frame that opens the menu of the new layers.
    add_button: Option<Rect>,
    /// The row of the layer list that moves with a finger.
    pub reorder: Option<Reorder>,
    /// The text of the name field of the panel of a vector layer.
    name: Option<(LayerRef, String)>,
    /// The list of the projections of the flat view is open.
    pub projections: bool,
    /// The list of the projections in the last frame.
    pub projections_rect: Option<Rect>,
    /// The card of the center of the projection in the last frame.
    pub center_rect: Option<Rect>,
    /// The workspace menu is open.
    pub menu: bool,
    /// The worlds of the open menu of the worlds. `None`: the menu is closed.
    pub worlds: Option<Vec<WorldFile>>,
    /// The menu of the worlds in the last frame.
    pub worlds_rect: Option<Rect>,
    /// The menu of the More cell is open.
    pub more: bool,
    /// The distance that the sheet is pulled down from its open place.
    pub sheet_offset: f32,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The places of the cards in the last frame.
    pub rects: Option<Rects>,
    /// The open panels of the last frame.
    pub panels: Vec<(Panel, Rect)>,
    /// The workspace menu of the last frame.
    pub menu_rect: Option<Rect>,
    /// The menu of the More cell in the last frame.
    pub more_rect: Option<Rect>,
    /// The More cell of this frame.
    more_cell: Option<Rect>,
    /// The height of the contents of each panel, by the key of the panel.
    heights: HashMap<&'static str, f32>,
    /// The Style section of the panel of a raster layer.
    style: raster::StyleState,
}

impl PadState {
    pub fn is_open(&self, panel: Panel) -> bool {
        match panel {
            Panel::Brush => self.brush,
            other => self.right == Some(other),
        }
    }

    pub fn open(&mut self, panel: Panel) {
        match panel {
            Panel::Brush => self.brush = true,
            other => self.right = Some(other),
        }
        self.sheet_offset = 0.0;
    }

    pub fn close(&mut self, panel: Panel) {
        match panel {
            Panel::Brush => self.brush = false,
            _ => self.right = None,
        }
    }

    /// Opens a closed panel and closes an open panel. `one_panel`: the screen
    /// has room for one panel, so the other panel closes.
    fn toggle(&mut self, panel: Panel, one_panel: bool) {
        if self.is_open(panel) {
            self.close(panel);
            return;
        }
        if one_panel {
            self.brush = false;
            self.right = None;
        }
        self.open(panel);
    }
}

const WORKSPACES: [&str; 3] = ["World", "Maps", "Atlas"];
const VIEWS: [&str; 2] = ["Globe", "Flat"];
/// The action that makes the place at the middle of the canvas the center of
/// the projection of the flat view.
pub const RECENTER: &str = "Recenter";
/// The action that puts the center of the projection and the view back.
pub const RESET: &str = "Reset";
/// The control that opens the list of the projections.
pub const PROJECTION: &str = "Projection";
const WORLD_VIEWS: [WorldView; 2] = [WorldView::Globe, WorldView::Flat];

/// The text of the position readout.
/// `value` is the value of the raster layer at the place, with its unit.
pub fn readout_text(lonlat: Option<LonLat>, value: Option<&str>) -> String {
    let Some([lon, lat]) = lonlat else {
        return "–".to_owned();
    };
    let east = if lon < 0.0 { 'W' } else { 'E' };
    let north = if lat < 0.0 { 'S' } else { 'N' };
    let place = format!("{:.1}°{east} {:.1}°{north}", lon.abs(), lat.abs());
    match value {
        Some(value) => format!("{place} · {value}"),
        None => place,
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let screen = ctx.viewport_rect();
    // The globe paints after the panels, so a change in a panel shows in
    // the same frame.
    let canvas = ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        globe::canvas_input(ui, state, theme::CANVAS)
    });

    let class = WidthClass::of(screen.width());
    let flat = state.globe.world_view == WorldView::Flat;
    // The compact layout has no room for the Worlds button, so the
    // workspace menu has a row for it.
    let workspace_width = match class {
        WidthClass::Wide => WORLDS_LEAD + widgets::segments_width(ui, &WORKSPACES),
        WidthClass::Medium => WORLDS_LEAD + widgets::menu_button_width(ui, WORKSPACES[0]),
        WidthClass::Compact => widgets::menu_button_width(ui, WORKSPACES[0]),
    };
    let mut input = Input {
        screen,
        bands: geometry::Bands::new(screen, ctx.content_rect()),
        workspace_width,
        // In the flat view, the card also has the button of the list of
        // projections, a line, and the Recenter and Reset buttons.
        view_width: widgets::segments_width(ui, &VIEWS)
            + if flat { 3.0 * TOUCH + 9.0 + PAD } else { 0.0 },
        panel: state.pad.brush || state.pad.right.is_some(),
        sheet_offset: state.pad.sheet_offset,
    };
    let mut rects = geometry::layout(&input);
    if rects.one_panel && state.pad.brush && state.pad.right.is_some() {
        state.pad.right = None;
    }
    state.pad.sheet_offset = input.sheet_offset.clamp(0.0, rects.sheet_travel);

    let mut controls = std::mem::take(&mut state.pad.controls);
    controls.clear();
    state.pad.panels.clear();
    state.pad.menu_rect = None;
    state.pad.add_button = None;
    if !state.globe.brushing() || state.globe.brush.mode != Mode::Flatten {
        state.globe.pick_level = false;
    }
    if !state.globe.paintable {
        state.pad.more = false;
    }

    top_row(&ctx, state, &mut controls, &rects);
    tool_strip(&ctx, state, &mut controls, &rects);
    brush_card(&ctx, state, &mut controls, &rects);
    // A button above can open or close a panel. The panels use the new state.
    input.panel = state.pad.brush || state.pad.right.is_some();
    input.sheet_offset = state.pad.sheet_offset;
    rects = geometry::layout(&input);
    for panel in [Panel::Brush, Panel::Layers, Panel::Toolbox] {
        if state.pad.is_open(panel) {
            show_panel(&ctx, state, &mut controls, &rects, panel);
        }
    }
    readout(&ctx, ui, state, &rects);
    state.pad.center_rect = None;
    // With no view switch in the top row, the two buttons have their own card.
    if flat && rects.view.is_none() {
        center_card(&ctx, state, &mut controls, &rects);
    }
    state.pad.projections_rect = None;
    if state.pad.projections && flat {
        projections(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.projections = false;
    }
    if state.pad.menu && class != WidthClass::Wide {
        menu(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.menu = false;
    }
    state.pad.more_rect = None;
    if state.pad.more {
        more_menu(&ctx, state, &mut controls, &rects);
    }
    state.pad.add_rect = None;
    match state.pad.add_button {
        Some(button) if state.pad.add => add_menu(&ctx, state, &mut controls, &rects, button),
        _ => state.pad.add = false,
    }
    state.pad.worlds_rect = None;
    if state.pad.worlds.is_some() {
        worlds_menu(&ctx, state, &mut controls, &rects);
    }
    world_form::pad(&ctx, state, &mut controls, &rects);
    raster_form::pad(&ctx, state, &mut controls, &rects);
    if let Some(rect) = canvas.inner {
        globe::paint_canvas(ui, state, rect, theme::CANVAS, true);
    }

    state.pad.controls = controls;
    state.pad.rects = Some(rects);
}

fn top_row(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-workspace",
        rects.workspace,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let mut rest = card;
            if rects.class != WidthClass::Compact {
                let button = Rect::from_min_size(card.min + vec2(PAD, PAD), vec2(TOUCH, TOUCH));
                let open = state.pad.worlds.is_some();
                let worlds =
                    widgets::icon_button_at(ui, controls, button, WORLDS, Icon::World, open, true);
                if worlds.clicked() {
                    toggle_worlds(state);
                }
                let divider = pos2(button.right() + 4.5, card.center().y);
                let line = Rect::from_center_size(divider, vec2(1.0, 24.0));
                ui.painter().rect_filled(line, 0.0, theme::divider());
                rest.min.x += WORLDS_LEAD;
            }
            let ui = &ui.new_child(egui::UiBuilder::new().max_rect(rest));
            if rects.class == WidthClass::Wide {
                let items = [
                    (WORKSPACES[0], true),
                    (WORKSPACES[1], true),
                    (WORKSPACES[2], false),
                ];
                if widgets::segments(ui, controls, &items, 0) == Some(1) {
                    state.workspace = Workspace::Map;
                }
            } else {
                let open = state.pad.menu;
                if widgets::menu_button(ui, controls, "Workspace", WORKSPACES[0], open).clicked() {
                    state.pad.menu = !open;
                    state.pad.worlds = None;
                }
            }
        },
    );
    if let Some(view) = rects.view {
        widgets::card(
            ctx,
            state.globe.backdrop(),
            "pad-view",
            view,
            Order::Middle,
            radius,
            |ui| {
                let active = state.globe.world_view as usize;
                let items = [(VIEWS[0], true), (VIEWS[1], true)];
                if let Some(chosen) = widgets::segments(ui, controls, &items, active) {
                    state.globe.world_view = WORLD_VIEWS[chosen];
                }
                if state.globe.world_view == WorldView::Flat {
                    // From the right: Reset, Recenter, a line, and the
                    // button of the list of projections.
                    let card = ui.max_rect();
                    let at = |right: f32| {
                        let min = pos2(right - TOUCH, card.top() + PAD);
                        Rect::from_min_size(min, vec2(TOUCH, TOUCH))
                    };
                    let reset = at(card.right() - PAD);
                    let recenter = at(reset.left() - PAD);
                    let divider = pos2(recenter.left() - 4.5, card.center().y);
                    let line = Rect::from_center_size(divider, vec2(1.0, 24.0));
                    ui.painter().rect_filled(line, 0.0, theme::divider());
                    let open = state.pad.projections;
                    let list = at(recenter.left() - 9.0);
                    let button = widgets::icon_button_at(
                        ui,
                        controls,
                        list,
                        PROJECTION,
                        Icon::Down,
                        open,
                        true,
                    );
                    if button.clicked() {
                        state.pad.projections = !open;
                    }
                    center_buttons(ui, state, controls, recenter, reset);
                }
            },
        );
    }
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-actions",
        rects.actions,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let at = |x: f32| {
                let min = pos2(card.left() + PAD + x, card.top() + PAD);
                Rect::from_min_size(min, vec2(TOUCH, TOUCH))
            };
            let step = TOUCH + PAD;
            let can_undo = state.can_undo();
            let undo =
                widgets::icon_button_at(ui, controls, at(0.0), "Undo", Icon::Undo, false, can_undo);
            if undo.clicked() {
                state.undo();
            }
            let can_redo = state.can_redo();
            let redo = widgets::icon_button_at(
                ui,
                controls,
                at(step),
                "Redo",
                Icon::Redo,
                false,
                can_redo,
            );
            if redo.clicked() {
                state.redo();
            }
            let divider = pos2(card.left() + PAD + 2.0 * step + 4.5, card.center().y);
            let line = Rect::from_center_size(divider, vec2(1.0, 24.0));
            ui.painter().rect_filled(line, 0.0, theme::divider());
            let x = 2.0 * step + 9.0 + PAD;
            for (panel, name, icon, x) in [
                (Panel::Layers, "Layers", Icon::Layers, x),
                (Panel::Toolbox, "Toolbox", Icon::Toolbox, x + step),
            ] {
                let active = state.pad.is_open(panel);
                if widgets::icon_button_at(ui, controls, at(x), name, icon, active, true).clicked()
                {
                    state.pad.toggle(panel, rects.one_panel);
                }
            }
        },
    );
}

/// The child of a card, inside the padding of the card.
fn padded(ui: &mut egui::Ui) -> egui::Ui {
    let rect = ui.max_rect().shrink(PAD);
    let layout = egui::Layout::top_down(egui::Align::Min);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    ui
}

fn tool_strip(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-tools",
        rects.tools,
        Order::Middle,
        radius,
        |ui| {
            let mut ui = padded(ui);
            let mut cells = |ui: &mut egui::Ui| {
                let globe = &mut state.globe;
                // With no raster layer to paint, the tool is Move.
                let paint = globe.paintable;
                for (name, icon, mode) in [
                    ("Raise", Icon::Raise, Mode::Raise),
                    ("Lower", Icon::Lower, Mode::Lower),
                    ("Smooth", Icon::Smooth, Mode::Smooth),
                    ("Flatten", Icon::Flatten, Mode::Flatten),
                ] {
                    let active = globe.brushing() && globe.brush.mode == mode;
                    if widgets::tool_cell(ui, controls, name, name, icon, active, paint).clicked() {
                        globe.tool = Tool::Brush;
                        globe.brush.mode = mode;
                    }
                }
                // The cell shows the mode of the menu that the brush has.
                let chosen = MORE_MODES
                    .into_iter()
                    .find(|(_, _, mode)| globe.brushing() && globe.brush.mode == *mode);
                let (label, icon) =
                    chosen.map_or((MORE, Icon::More), |(name, icon, _)| (name, icon));
                let active = chosen.is_some();
                let more = widgets::tool_cell(ui, controls, MORE, label, icon, active, paint);
                if more.clicked() {
                    state.pad.more = !state.pad.more;
                }
                state.pad.more_cell = Some(more.rect);
                let active = !globe.brushing();
                if widgets::tool_cell(ui, controls, "Pan", "Move", Icon::Move, active, true)
                    .clicked()
                {
                    globe.tool = Tool::Navigate;
                }
            };
            if rects.tool_cells < TOOLS {
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut ui, cells);
            } else {
                cells(&mut ui);
            }
        },
    );
}

/// Shows a value as a slider with a logarithmic scale.
fn log_slider(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
    slider: impl FnOnce(&mut f64, &str) -> bool,
) -> Option<f64> {
    let (low, high) = (*range.start(), *range.end());
    let span = (high / low).ln();
    let mut fraction = if span > 0.0 {
        ((value / low).ln() / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    slider(&mut fraction, text).then(|| low * (fraction * span).exp())
}

/// The size of the brush on the ground, in the display units of the world.
pub fn brush_size_text(state: &AppState) -> String {
    let world = &state.doc.project.world;
    let globe = &state.globe;
    let km = globe.brush.radius_km(globe.radius(), world.radius_km);
    world::distance(km, world.units)
}

/// Shows the size of the brush on the ground as a slider.
fn size_slider(state: &mut AppState, slider: impl FnOnce(&mut f64, &str) -> bool) {
    let world_km = state.doc.project.world.radius_km;
    let globe = &state.globe;
    let globe_radius = globe.radius();
    let range = globe.brush.radius_km_range(globe_radius, world_km);
    let km = globe.brush.radius_km(globe_radius, world_km);
    let text = brush_size_text(state);
    if let Some(km) = log_slider(km, range, &text, slider) {
        let brush = &mut state.globe.brush;
        brush.set_radius_km(km, globe_radius, world_km);
    }
}

/// Shows a value from 0 to 1 as a slider with a value in percent.
fn percent_slider(
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    slider: impl FnOnce(&mut f64, &str) -> bool,
) {
    let (low, high) = (*range.start(), *range.end());
    let mut fraction = ((*value - low) / (high - low)).clamp(0.0, 1.0);
    let text = format!("{:.0}%", *value * 100.0);
    if slider(&mut fraction, &text) {
        *value = low + fraction * (high - low);
    }
}

fn brush_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(rect) = rects.brush else {
        return;
    };
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-brush-card",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let mut ui = padded(ui);
            let paint = state.globe.paintable;
            if let BrushCard::Full(height) = rects.brush_card {
                let mut slider = |name: &str, fraction: &mut f64, text: &str| {
                    widgets::vertical_slider(&mut ui, controls, name, height, fraction, text, paint)
                };
                size_slider(state, |fraction, text| slider("Size", fraction, text));
                percent_slider(&mut state.globe.brush.flow, FLOW, |fraction, text| {
                    slider("Flow", fraction, text)
                });
            }
            let open = state.pad.brush;
            let button = widgets::tool_cell(
                &mut ui,
                controls,
                "Brush settings",
                "Brush",
                Icon::Brush,
                open,
                paint,
            );
            if button.clicked() {
                state.pad.toggle(Panel::Brush, rects.one_panel);
            }
        },
    );
}

fn show_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    panel: Panel,
) {
    let shown = state.pad.layer.and_then(|layer| state.layer_of(layer));
    let name = shown.map(|layer| layer.name.clone());
    if name.is_none() {
        state.pad.layer = None;
    }
    let layer = state.pad.layer.filter(|_| panel == Panel::Layers);
    // The panel of a raster layer has the controls of the selected layer.
    if let Some(LayerRef::Raster(_)) = layer
        && state.selected_layer != layer
    {
        state.select(layer);
    }
    let name = name.as_deref().unwrap_or_default();
    let (key, title, back) = match (panel, layer) {
        (Panel::Brush, _) => (panel.key(), "Brush", None),
        (Panel::Layers, Some(LayerRef::Raster(_))) => ("pad-layer", name, Some("Layers")),
        (Panel::Layers, Some(LayerRef::Vector(_))) => ("pad-vector", name, Some("Layers")),
        (Panel::Layers, None) => (panel.key(), "Layers", None),
        (Panel::Toolbox, _) => (panel.key(), "Toolbox", None),
    };
    let list = panel == Panel::Layers && layer.is_none();
    if list && state.pad.reorder.is_none() {
        state.pad.name = None;
    }
    if !list {
        state.pad.reorder = None;
    }
    let scroll = match state.pad.reorder {
        Some(_) => egui::scroll_area::ScrollSource::NONE,
        None => egui::scroll_area::ScrollSource::default(),
    };
    let (rect, radius) = match rects.sheet {
        Some(sheet) => {
            let r = theme::SHEET_RADIUS as u8;
            let radius = egui::CornerRadius {
                nw: r,
                ne: r,
                sw: 0,
                se: 0,
            };
            (sheet, radius)
        }
        None => {
            let (min, width) = match panel {
                Panel::Brush => (rects.brush_panel, geometry::BRUSH_PANEL_WIDTH),
                _ => (rects.right_panel, geometry::RIGHT_PANEL_WIDTH),
            };
            let most = (rects.panel_bottom - min.y).max(ROW);
            let height = state.pad.heights.get(key).map_or(most, |h| h.min(most));
            let rect = Rect::from_min_size(min, vec2(width, height));
            (rect, theme::CARD_RADIUS.into())
        }
    };
    let sheet = rects.sheet.is_some();
    let id = if sheet { "pad-sheet" } else { key };
    state.pad.panels.push((panel, rect));
    widgets::card(
        ctx,
        state.globe.backdrop(),
        id,
        rect,
        Order::Middle,
        radius,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if sheet {
                let grabber = Rect::from_center_size(
                    pos2(rect.center().x, rect.top() + 8.5),
                    vec2(36.0, 5.0),
                );
                ui.painter()
                    .rect_filled(grabber, 3.0, theme::MUTE.gamma_multiply(0.6));
            }
            let action = list.then_some((ADD_LAYER, Icon::Plus, state.pad.add));
            let row = widgets::title_row_with(ui, controls, title, back, sheet, action);
            if let Some(add) = &row.action {
                state.pad.add_button = Some(add.rect);
                if add.clicked() {
                    state.pad.add = !state.pad.add;
                }
            }
            if row.back {
                state.pad.layer = None;
            }
            if row.close {
                state.pad.close(panel);
            }
            if sheet {
                sheet_drag(ui, &mut state.pad, &row.bar, rects.sheet_travel);
                if state.pad.sheet_offset >= rects.sheet_travel {
                    return;
                }
            }
            let bottom = if sheet {
                rect.bottom() - rects.bands.bottom
            } else {
                rect.bottom()
            };
            let most = (bottom - ui.cursor().top()).max(0.0);
            let out = egui::ScrollArea::vertical()
                .id_salt(key)
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
                        match (panel, layer) {
                            (Panel::Brush, _) => brush_body(ui, state, controls),
                            (Panel::Layers, Some(LayerRef::Raster(id))) => {
                                raster_body(ui, state, controls, id);
                            }
                            (Panel::Layers, Some(LayerRef::Vector(id))) => {
                                vector_body(ui, state, controls, id);
                            }
                            (Panel::Layers, None) => layers_body(ui, state, controls),
                            (Panel::Toolbox, _) => toolbox_body(ui, state),
                        }
                    });
                });
            state.pad.heights.insert(key, ROW + out.content_size.y);
        },
    );
}

/// Moves the sheet with a drag on its title row. At the end of the drag, the
/// sheet goes to the nearer of its two places. A tap changes the place.
fn sheet_drag(ui: &egui::Ui, pad: &mut PadState, bar: &egui::Response, travel: f32) {
    if bar.dragged() {
        pad.sheet_offset = (pad.sheet_offset + bar.drag_delta().y).clamp(0.0, travel);
        ui.ctx().request_repaint();
    }
    let down = if bar.drag_stopped() {
        let speed = ui.input(|i| i.pointer.velocity().y);
        if speed.abs() > 300.0 {
            speed > 0.0
        } else {
            pad.sheet_offset > travel / 2.0
        }
    } else if bar.clicked() {
        pad.sheet_offset < travel / 2.0
    } else {
        return;
    };
    pad.sheet_offset = if down { travel } else { 0.0 };
}

fn brush_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let unit = state.shown_unit().to_string();
    size_slider(state, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush size", "Size", fraction, text)
    });
    let brush = &mut state.globe.brush;
    percent_slider(&mut brush.hardness, 0.0..=1.0, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush hardness", "Hardness", fraction, text)
    });
    percent_slider(&mut brush.flow, FLOW, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush flow", "Flow", fraction, text)
    });
    if brush.mode == Mode::Warp {
        let text = format!("{} km", world::three_significant(brush.warp_km));
        let strength = log_slider(brush.warp_km, WARP_KM, &text, |fraction, text| {
            widgets::row_slider(ui, controls, "Brush strength", "Strength", fraction, text)
        });
        if let Some(km) = strength {
            brush.warp_km = km;
        }
        let text = format!("{} km", world::three_significant(brush.scale_km));
        let scale = log_slider(brush.scale_km, SCALE_KM, &text, |fraction, text| {
            widgets::row_slider(ui, controls, "Brush scale", "Scale", fraction, text)
        });
        if let Some(km) = scale {
            brush.scale_km = km;
        }
        let seed = &mut state.doc.project.world.seed;
        let high = f64::from(*SEED.end());
        let mut fraction = (f64::from(*seed) / high).min(1.0);
        let text = seed.to_string();
        if widgets::row_slider(ui, controls, "Brush seed", "Seed", &mut fraction, &text) {
            *seed = (fraction * high).round() as u32;
        }
    } else {
        let (min, max) = brush.value_range;
        let text = world::value_text(brush.strength_value(), max - min, &unit);
        let strength = log_slider(brush.strength_m, STRENGTH_M, &text, |fraction, text| {
            let name = panels::strength_name(brush.mode);
            widgets::row_slider(ui, controls, "Brush strength", name, fraction, text)
        });
        if let Some(meters) = strength {
            brush.strength_m = meters;
        }
    }
    if brush.mode == Mode::Uplift {
        percent_slider(&mut brush.erosion, EROSION, |fraction, text| {
            widgets::row_slider(ui, controls, "Brush erosion", "Erosion", fraction, text)
        });
    }
    if matches!(brush.mode, Mode::Weather | Mode::Uplift) {
        let text = format!("{}°", world::three_significant(brush.talus_deg));
        let angle = log_slider(brush.talus_deg, TALUS_DEG, &text, |fraction, text| {
            widgets::row_slider(ui, controls, "Brush angle", "Angle", fraction, text)
        });
        if let Some(degrees) = angle {
            brush.talus_deg = degrees;
        }
    }
    let range = brush.value_range;
    let floor = (&mut brush.floor_on, &mut brush.floor_m);
    limit_rows(ui, controls, "Floor", "Brush floor", floor, range, &unit);
    let ceiling = (&mut brush.ceiling_on, &mut brush.ceiling_m);
    limit_rows(
        ui,
        controls,
        "Ceiling",
        "Brush ceiling",
        ceiling,
        range,
        &unit,
    );
    ui.spacing_mut().item_spacing.y = 6.0;
    panels::brush_settings(ui, state);
}

/// The number of steps of the slider of a limit of the brush. A height has
/// steps of 50 meters.
const LIMIT_STEPS: f64 = 240.0;

/// The switch of a limit of the brush. While the switch is on, a slider
/// below it sets the value, in the unit of the layer that the brush paints.
/// `range` has the values of the lowest and of the highest level of the
/// layer.
fn limit_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    name: &str,
    (on, value): (&mut bool, &mut f64),
    (min, max): (f64, f64),
    unit: &str,
) {
    widgets::switch_row(ui, controls, name, label, on);
    if !*on {
        return;
    }
    let span = max - min;
    let mut fraction = ((*value - min) / span).clamp(0.0, 1.0);
    let text = world::value_text(value.clamp(min, max), span, unit);
    let name = format!("{name} level");
    if widgets::row_slider(ui, controls, &name, "Level", &mut fraction, &text) {
        *value = min + (fraction * LIMIT_STEPS).round() * span / LIMIT_STEPS;
    }
}

/// The text of the Layers panel of a world with no layer.
pub const NO_LAYERS: &str = "This world has no layers.";

/// The icon of the kind of a layer.
fn layer_icon(layer: &Layer) -> Icon {
    match layer.kind {
        LayerKind::Raster(_) => Icon::Raster,
        LayerKind::Vector(GeometryKind::Point) => Icon::Point,
        LayerKind::Vector(GeometryKind::Line) => Icon::Line,
        LayerKind::Vector(GeometryKind::Polygon) => Icon::Polygon,
    }
}

/// One row of the layer list.
struct ListRow {
    id: LayerId,
    layer: LayerRef,
    name: String,
    icon: Icon,
    visible: bool,
}

/// The list of the layers of each kind, in draw order. The first row is the
/// bottom of the drawing. A tap on a row selects the layer, and the eye
/// shows or hides it. A finger that holds a row lifts it, and the row then
/// moves with the finger to a new place in the order.
fn layers_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let layers = state.doc.project.layers().iter();
    let rows: Vec<ListRow> = layers
        .map(|l| ListRow {
            id: l.id,
            layer: LayerRef::of(l),
            name: l.name.clone(),
            icon: layer_icon(l),
            visible: l.visible,
        })
        .collect();
    if rows.is_empty() {
        state.pad.reorder = None;
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_LAYERS).color(theme::MUTE));
        return;
    }
    let size = vec2(ui.available_width(), rows.len() as f32 * PANEL_ROW);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let slot =
        |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW));
    let place = |i: usize| slot(area.top() + i as f32 * PANEL_ROW);
    let selected = state.selected_layer;
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));

    let lifted = state
        .pad
        .reorder
        .and_then(|r| Some((r, rows.iter().position(|row| row.id == r.layer)?)));
    let Some((reorder, from)) = lifted else {
        state.pad.reorder = None;
        for (i, row) in rows.iter().enumerate() {
            let (id, rect) = (row.id, place(i));
            let on = selected == Some(row.layer);
            let (icon, name, visible) = (row.icon, &row.name, Some(row.visible));
            let out = ui
                .push_id(id.0, |ui| {
                    widgets::layer_row(ui, controls, rect, icon, name, visible, on, true, false)
                })
                .inner;
            if out.eye {
                state.set_layer_visible(id, !row.visible);
            }
            if out.open {
                state.select(Some(row.layer));
                state.pad.layer = Some(row.layer);
            }
            let Some(body) = out.body else { continue };
            if body.clicked() {
                state.select(Some(row.layer));
            }
            if !body.is_pointer_button_down_on() {
                continue;
            }
            let press = ui.input(|i| {
                let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
                let pos = i.pointer.interact_pos()?;
                Some((i.time - start, origin.distance(pos), pos.y))
            });
            match press {
                Some((held, moved, y)) if moved <= HOLD_SLOP && held >= HOLD_TIME => {
                    state.select(Some(row.layer));
                    state.pad.reorder = Some(Reorder {
                        layer: id,
                        grab: y - rect.top(),
                    });
                    ui.ctx().request_repaint();
                }
                Some((held, moved, _)) if moved <= HOLD_SLOP => {
                    let wait = std::time::Duration::from_secs_f64(HOLD_TIME - held);
                    ui.ctx().request_repaint_after(wait);
                }
                _ => {}
            }
        }
        return;
    };

    let y = pointer.map_or(place(from).top() + reorder.grab, |p| p.y);
    let top = (y - reorder.grab).clamp(area.top(), area.bottom() - PANEL_ROW);
    let to = ((top - area.top()) / PANEL_ROW).round() as usize;
    let others = rows.iter().enumerate().filter(|(i, _)| *i != from);
    for (k, (_, row)) in others.enumerate() {
        // The rows make room at the place where the lifted row goes.
        let rect = place(if k < to { k } else { k + 1 });
        let on = selected == Some(row.layer);
        let (icon, name, visible) = (row.icon, &row.name, Some(row.visible));
        widgets::layer_row(ui, controls, rect, icon, name, visible, on, false, false);
    }
    let row = &rows[from];
    let (icon, name, visible) = (row.icon, &row.name, Some(row.visible));
    let rect = slot(top);
    widgets::layer_row(ui, controls, rect, icon, name, visible, true, false, true);
    if !down {
        state.pad.reorder = None;
        if to != from {
            state.move_layer(reorder.layer, to);
        }
    }
}

/// The menu of the new layers. A wide screen shows it at the left of the
/// Layers panel, and a narrow screen shows it below its button.
fn add_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    button: Rect,
) {
    // A world with no height layer gets one from an import.
    let import = !state.doc.project.rasters().iter().any(is_height);
    let rows = 1 + NEW_LAYERS.len() + usize::from(import);
    let size = vec2(ADD_WIDTH, 10.0 + rows as f32 * PANEL_ROW + 10.0);
    let beside = pos2(
        rects.right_panel.x - geometry::PANEL_GAP - size.x,
        rects.right_panel.y,
    );
    let min = if rects.sheet.is_none() && beside.x >= rects.screen.left() + geometry::MARGIN {
        beside
    } else {
        let left = (button.right() - size.x).max(rects.screen.left() + geometry::PANEL_GAP);
        pos2(left, button.bottom() + PAD)
    };
    let rect = Rect::from_min_size(min, size);
    state.pad.add_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !button.contains(pos)) {
        state.pad.add = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-add-layer",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The menu can cover a panel, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            // The arrow tells that the row opens a form.
            let raster = widgets::text_row(ui, controls, NEW_RASTER, 0.0, false, true);
            let arrow = Rect::from_center_size(
                pos2(raster.rect.right() - 10.0, raster.rect.center().y),
                vec2(18.0, 18.0),
            );
            icons::paint(ui.painter(), arrow, Icon::Open, 18.0, theme::MUTE);
            if raster.clicked() {
                state.raster_form = Some(RasterForm::open(state));
                state.pad.add = false;
            }
            for (name, kind) in NEW_LAYERS {
                if widgets::text_row(ui, controls, name, 0.0, false, true).clicked() {
                    let id = state.doc.add_layer(kind);
                    state.select(Some(LayerRef::Vector(id)));
                    state.pad.layer = Some(LayerRef::Vector(id));
                    state.pad.add = false;
                    state.touch();
                }
            }
            if import {
                let enabled = !state.globe.busy();
                let row = widgets::text_row(ui, controls, IMPORT_HEIGHTMAP, 0.0, false, enabled);
                if row.clicked() {
                    state.actions.push(Action::ImportHeightmapDialog);
                    state.pad.add = false;
                }
            }
        },
    );
}

/// The panel of a vector layer: its name, its kind, the number of its
/// features, and the button that deletes it.
fn vector_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let name = layer.name.clone();
    let kind = match layer.geometry() {
        Some(GeometryKind::Point) => "Points",
        Some(GeometryKind::Line) => "Lines",
        Some(GeometryKind::Polygon) => "Polygons",
        None => return,
    };
    let features = layer.features.len().to_string();
    ui.spacing_mut().item_spacing.y = 0.0;

    let of = LayerRef::Vector(id);
    let mut text = match state.pad.name.take() {
        Some((layer, text)) if layer == of => text,
        _ => name.clone(),
    };
    let valid = !text.trim().is_empty();
    let field = widgets::text_field(ui, controls, LAYER_NAME, "Name", None, &mut text, valid);
    if field.changed() {
        state.doc.rename_layer(id, &text);
    } else if !field.has_focus() {
        // The field shows the name again after an undo and after a blank text.
        text = name;
    }
    state.pad.name = Some((of, text));
    widgets::value_row(ui, "Type", kind);
    widgets::value_row(ui, "Features", &features);
    ui.add_space(6.0);
    if widgets::button_row_in(ui, controls, DELETE_LAYER, true, theme::WARN).clicked() {
        state.remove_layer(id);
    }
}

/// Shows the text of the import or the export that runs, or of the last one.
fn note_text(ui: &mut egui::Ui, progress: Option<String>, note: Option<&Note>) {
    let (text, color) = match (progress, note) {
        (Some(progress), _) => (progress, theme::MUTE),
        (None, Some(note)) if note.warning => (note.text.clone(), theme::WARN),
        (None, Some(note)) => (note.text.clone(), theme::MUTE),
        (None, None) => return,
    };
    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
}

/// The row that imports a heightmap, and the text of the import below it.
fn import_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let enabled = !state.globe.busy();
    if widgets::text_row(ui, controls, IMPORT_HEIGHTMAP, 0.0, false, enabled).clicked() {
        state.actions.push(Action::ImportHeightmapDialog);
    }
    let progress = state.globe.import_progress();
    note_text(ui, progress, state.import_note.as_ref());
}

/// The row that exports the selected raster layer, and the text of the
/// export below it.
fn export_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let enabled = state.can_export();
    if widgets::text_row(ui, controls, EXPORT_GEOTIFF, 0.0, false, enabled).clicked() {
        state.actions.push(Action::ExportHeightmapDialog);
    }
    let progress = state.globe.export_progress();
    note_text(ui, progress, state.export_note.as_ref());
}

/// The panel of a raster layer: its name, the unit of a layer that is not a
/// height layer, its value range, its face size, its opacity, the import
/// into a height layer, the export, the button that deletes the layer, and
/// the Style section.
fn raster_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (unit, height, opacity) = (raster.unit.clone(), is_height(raster), raster.opacity);
    let layer = state.doc.project.raster_layer(id);
    let name = layer.map_or(String::new(), |layer| layer.name.clone());
    let span = raster.value_max - raster.value_min;
    let range = format!(
        "{} to {}",
        world::value_text(raster.value_min, span, ""),
        world::value_text(raster.value_max, span, &unit)
    );
    let face_size = world::grouped(u64::from(raster.face_size));
    ui.spacing_mut().item_spacing.y = 0.0;

    let of = LayerRef::Raster(id);
    let mut text = match state.pad.name.take() {
        Some((layer, text)) if layer == of => text,
        _ => name.clone(),
    };
    let valid = !text.trim().is_empty();
    let field = widgets::text_field(ui, controls, LAYER_NAME, "Name", None, &mut text, valid);
    if field.changed() {
        state.rename_raster(id, &text);
    } else if !field.has_focus() {
        // The field shows the name again after an undo and after a blank text.
        text = name;
    }
    state.pad.name = Some((of, text));
    raster::unit_row(ui, state, controls, id);
    widgets::value_row(ui, "Values", &range);
    widgets::value_row(ui, "Face size", &face_size);
    let mut fraction = f64::from(opacity);
    let text = format!("{:.0}%", fraction * 100.0);
    if widgets::row_slider(ui, controls, OPACITY, "Opacity", &mut fraction, &text) {
        // The steps are whole percents.
        state.set_raster_opacity(id, ((fraction * 100.0).round() / 100.0) as f32);
    }
    if height {
        import_row(ui, state, controls);
    }
    export_row(ui, state, controls);
    // The button is above the Style section, which is longer than a short
    // panel.
    ui.add_space(6.0);
    let delete = widgets::button_row_in(ui, controls, DELETE_RASTER, true, theme::WARN);
    raster::style(ui, state, controls, id);
    if delete.clicked() {
        state.remove_raster(id);
    }
}

fn toolbox_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("The toolbox has no tools yet.").color(theme::MUTE));
    // Temporary controls, for the tests of the brush on the device.
    ui.checkbox(&mut state.globe.debug, "Debug");
    if state.globe.debug {
        // The frame time needs a new frame at each refresh of the screen.
        if !state.headless {
            ui.ctx().request_repaint();
        }
        panels::debug_controls(ui, state);
    }
}

/// The readout: the place and the elevation under the pointer. With a
/// project file, the save mark is at its left.
fn readout(ctx: &egui::Context, ui: &egui::Ui, state: &AppState, rects: &Rects) {
    let Some(band) = rects.readout else {
        return;
    };
    let value = state.readout_value();
    let text = readout_text(state.cursor_lonlat, value.as_deref());
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::mono(), theme::MUTE);
    let mark = state.save_mark(std::time::Instant::now());
    let lead = if mark.is_some() { SAVE_MARK + 8.0 } else { 0.0 };
    let size = vec2(lead + galley.size().x.ceil() + 24.0, band.height());
    let rect = if rects.class == WidthClass::Compact {
        Rect::from_min_size(band.min, size)
    } else {
        Rect::from_center_size(band.center(), size)
    };
    let radius = theme::CONTROL_RADIUS.into();
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-readout",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let left = rect.left() + 12.0;
            if let Some(mark) = mark {
                let center = pos2(left + SAVE_MARK / 2.0, rect.center().y);
                let at = Rect::from_center_size(center, vec2(SAVE_MARK, SAVE_MARK));
                // A test has no clock, so there the mark does not turn.
                save_mark(ui, at, mark, !state.headless);
            }
            let at = pos2(left + lead, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(at, galley, theme::MUTE);
        },
    );
}

/// The save mark: an arc that turns during a save, a tick when the file
/// holds each change, and a warning sign after a save that failed.
fn save_mark(ui: &egui::Ui, rect: Rect, mark: SaveMark, turn: bool) {
    let painter = ui.painter();
    let stroke = |color| egui::Stroke::new(1.4, color);
    match mark {
        SaveMark::Saving => {
            let start = if turn {
                ui.ctx().request_repaint();
                ui.input(|i| i.time) as f32 * std::f32::consts::TAU
            } else {
                0.0
            };
            let radius = rect.width() / 2.0 - 1.5;
            let points = (0..=18).map(|i| {
                let angle = start + i as f32 / 18.0 * 0.75 * std::f32::consts::TAU;
                rect.center() + radius * vec2(angle.cos(), angle.sin())
            });
            painter.add(egui::Shape::line(points.collect(), stroke(theme::MUTE)));
        }
        SaveMark::Saved => {
            let color = theme::MUTE.gamma_multiply(0.6);
            icons::paint(painter, rect, Icon::Tick, rect.width(), color);
        }
        SaveMark::Failed => {
            let (center, top) = (rect.center(), rect.top() + 1.0);
            let half = rect.width() / 2.0 - 0.5;
            let corners = [
                pos2(center.x, top),
                pos2(center.x + half, rect.bottom() - 1.0),
                pos2(center.x - half, rect.bottom() - 1.0),
            ];
            painter.add(egui::Shape::closed_line(
                corners.to_vec(),
                stroke(theme::WARN),
            ));
            let line = [pos2(center.x, top + 4.5), pos2(center.x, top + 8.0)];
            painter.line_segment(line, stroke(theme::WARN));
            painter.circle_filled(pos2(center.x, top + 10.2), 0.8, theme::WARN);
        }
    }
}

/// The Recenter and Reset buttons of the flat view, at the given places.
fn center_buttons(
    ui: &egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    recenter: Rect,
    reset: Rect,
) {
    let flat = &mut state.globe.flat;
    let can = flat.can_center_here();
    if widgets::icon_button_at(ui, controls, recenter, RECENTER, Icon::Center, false, can).clicked()
    {
        flat.center_here();
    }
    let can = flat.can_reset();
    if widgets::icon_button_at(ui, controls, reset, RESET, Icon::Reset, false, can).clicked() {
        flat.reset();
    }
}

/// The card of the Recenter and Reset buttons in the compact layout, below
/// the top row.
fn center_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let size = vec2(2.0 * TOUCH + 3.0 * PAD, ROW);
    let middle = rects.screen.center().x;
    let mut rect = Rect::from_min_size(pos2(middle - size.x / 2.0, rects.tools.top()), size);
    // An open panel pushes the card to its right. With no room there, the
    // card does not show.
    let panels = &state.pad.panels;
    let free = |rect: Rect| !panels.iter().any(|(_, panel)| panel.intersects(rect));
    if let Some((_, panel)) = panels.iter().find(|(_, panel)| panel.intersects(rect)) {
        rect = rect.translate(vec2(panel.right() + geometry::GAP - rect.left(), 0.0));
        if !free(rect) || rect.right() > rects.screen.right() - geometry::GAP {
            return;
        }
    }
    state.pad.center_rect = Some(rect);
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-center",
        rect,
        Order::Middle,
        theme::CARD_RADIUS.into(),
        |ui| {
            let at = |x: f32| {
                let min = pos2(rect.left() + PAD + x, rect.top() + PAD);
                Rect::from_min_size(min, vec2(TOUCH, TOUCH))
            };
            center_buttons(ui, state, controls, at(0.0), at(TOUCH + PAD));
        },
    );
}

/// The list of the projections of the flat view. It opens below the view
/// switch, or in the place of the workspace menu.
fn projections(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let rows = ProjectionKind::ALL.len() as f32;
    let size = vec2(geometry::MENU_WIDTH, 10.0 + rows * PANEL_ROW + 10.0);
    let min = match rects.view {
        Some(view) => pos2(view.center().x - size.x / 2.0, view.bottom() + PAD),
        None => rects.menu,
    };
    let rect = Rect::from_min_size(min, size);
    state.pad.projections_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let on_switch = |pos| rects.view.is_some_and(|view| view.contains(pos));
    if pressed.is_some_and(|pos| !rect.contains(pos) && !on_switch(pos)) {
        state.pad.projections = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-projections",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The list covers other cards, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let spec = state.globe.flat.spec();
            for kind in ProjectionKind::ALL {
                let row = widgets::text_row(ui, controls, kind.name(), 0.0, false, true);
                if kind == spec.kind {
                    let tick = Rect::from_center_size(
                        pos2(row.rect.right() - 14.0, row.rect.center().y),
                        vec2(18.0, 18.0),
                    );
                    icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                }
                if row.clicked() {
                    state.globe.flat.set_spec(ProjectionSpec { kind, ..spec });
                    state.pad.projections = false;
                }
            }
        },
    );
}

fn menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let compact = rects.class == WidthClass::Compact;
    let flat = state.globe.world_view == WorldView::Flat;
    let rows = match (compact, flat) {
        (true, true) => 7.0,
        (true, false) => 6.0,
        (false, _) => 3.0,
    };
    let rules = if compact { 2.0 } else { 1.0 };
    let height = 10.0 + rows * PANEL_ROW + rules * 13.0 + 10.0;
    let size = vec2(geometry::MENU_WIDTH, height);
    let rect = Rect::from_min_size(rects.menu, size);
    state.pad.menu_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !rects.workspace.contains(pos)) {
        state.pad.menu = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-menu",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The menu covers the tool strip, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if widgets::text_row(ui, controls, WORKSPACES[0], 0.0, true, true).clicked() {
                state.pad.menu = false;
            }
            if compact {
                for (label, view) in VIEWS.into_iter().zip(WORLD_VIEWS) {
                    let row = widgets::text_row(ui, controls, label, 16.0, false, true);
                    if state.globe.world_view == view {
                        let tick = Rect::from_center_size(
                            pos2(row.rect.right() - 14.0, row.rect.center().y),
                            vec2(18.0, 18.0),
                        );
                        icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                    }
                    if row.clicked() {
                        state.globe.world_view = view;
                        state.pad.menu = false;
                    }
                }
                if flat && widgets::text_row(ui, controls, PROJECTION, 32.0, false, true).clicked()
                {
                    state.pad.projections = true;
                    state.pad.menu = false;
                }
            }
            rule(ui);
            if widgets::text_row(ui, controls, WORKSPACES[1], 0.0, false, true).clicked() {
                state.workspace = Workspace::Map;
                state.pad.menu = false;
            }
            widgets::text_row(ui, controls, WORKSPACES[2], 0.0, false, false);
            if compact {
                rule(ui);
                if widgets::text_row(ui, controls, WORLDS_ROW, 0.0, false, true).clicked() {
                    state.pad.menu = false;
                    toggle_worlds(state);
                }
            }
        },
    );
}

/// A line between two groups of rows of a menu.
fn rule(ui: &mut egui::Ui) {
    let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), egui::Sense::hover());
    let line = Rect::from_center_size(rule.center(), vec2(rule.width(), 1.0));
    ui.painter().rect_filled(line, 0.0, theme::raise());
}

/// Opens or closes the menu of the worlds. The menu shows the worlds that
/// the folder of the projects has at this time.
pub fn toggle_worlds(state: &mut AppState) {
    state.pad.worlds = match state.pad.worlds {
        Some(_) => None,
        None => Some(worlds_of(state.projects.paths(state.files.as_ref()))),
    };
    state.pad.menu = false;
}

/// The name of a world in a row of a menu, at the length that the row holds.
fn row_name(name: &str) -> String {
    const LENGTH: usize = 26;
    if name.chars().count() <= LENGTH {
        return name.to_string();
    }
    let start: String = name.chars().take(LENGTH - 1).collect();
    format!("{}…", start.trim_end())
}

/// The menu of the worlds: the worlds of the folder of the projects, and the
/// rows that make a world and that open the properties of the open world.
/// The open world has a tick.
fn worlds_menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(list) = state.pad.worlds.take() else {
        return;
    };
    let shown = list.len().min(WORLD_ROWS) as f32 * PANEL_ROW;
    let rule_height = if list.is_empty() { 0.0 } else { 13.0 };
    let width = geometry::WORLDS_WIDTH.min(rects.screen.width() - 2.0 * geometry::MARGIN);
    // The row of the start screen needs a folder of projects, and the Save
    // row needs a project file.
    let rows = 2.0 + f32::from(state.projects.any()) + f32::from(state.session.is_some());
    let size = vec2(width, 10.0 + shown + rule_height + rows * PANEL_ROW + 10.0);
    let rect = Rect::from_min_size(rects.menu, size);
    state.pad.worlds_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let mut open =
        !pressed.is_some_and(|pos| !rect.contains(pos) && !rects.workspace.contains(pos));
    let current = state.session.as_ref().map(|s| s.path().to_path_buf());
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-worlds",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The menu covers the tool strip, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let mut rows = |ui: &mut egui::Ui| {
                for world in &list {
                    let name = row_name(&world.name);
                    let row = widgets::text_row(ui, controls, &name, 0.0, false, true);
                    if current.as_ref() == Some(&world.path) {
                        let tick = Rect::from_center_size(
                            pos2(row.rect.right() - 14.0, row.rect.center().y),
                            vec2(18.0, 18.0),
                        );
                        icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                    }
                    if row.clicked() {
                        state.actions.push(Action::OpenWorld(world.path.clone()));
                        open = false;
                    }
                }
            };
            if list.len() > WORLD_ROWS {
                egui::ScrollArea::vertical()
                    .max_height(shown)
                    .show(ui, |ui| {
                        ui.set_width(inner.width());
                        rows(ui)
                    });
            } else {
                rows(ui);
            }
            if !list.is_empty() {
                rule(ui);
            }
            if widgets::text_row(ui, controls, world_form::NEW_WORLD, 0.0, false, true).clicked() {
                state.world_form = Some(WorldForm::new_world());
                open = false;
            }
            if widgets::text_row(ui, controls, WORLD_PROPERTIES, 0.0, false, true).clicked() {
                state.world_form = Some(WorldForm::properties(&state.doc.project.world));
                open = false;
            }
            if state.session.is_some()
                && widgets::text_row(ui, controls, SAVE, 0.0, false, true).clicked()
            {
                state.actions.push(Action::Save);
                open = false;
            }
            if state.projects.any()
                && widgets::text_row(ui, controls, panels::ALL_WORLDS, 0.0, false, true).clicked()
            {
                state.actions.push(Action::ShowProjects);
                open = false;
            }
        },
    );
    state.pad.worlds = open.then_some(list);
}

/// The menu of the More cell: the brush modes that have no cell in the tool
/// strip. It opens at the right of the cell.
fn more_menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(cell) = state.pad.more_cell else {
        state.pad.more = false;
        return;
    };
    let rows = MORE_MODES.len() as f32;
    let size = vec2(geometry::MORE_WIDTH, 10.0 + rows * PANEL_ROW + 10.0);
    let top = cell
        .top()
        .min(rects.panel_bottom - size.y)
        .max(rects.tools.top());
    let min = pos2(rects.tools.right() + geometry::GAP, top);
    let rect = Rect::from_min_size(min, size);
    state.pad.more_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !cell.contains(pos)) {
        state.pad.more = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-more",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The menu covers the Brush panel, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let globe = &mut state.globe;
            for (name, _, mode) in MORE_MODES {
                let enabled = globe.is_height() || !needs_height(mode);
                let row = widgets::text_row(ui, controls, name, 0.0, false, enabled);
                if globe.brushing() && globe.brush.mode == mode {
                    let tick = Rect::from_center_size(
                        pos2(row.rect.right() - 14.0, row.rect.center().y),
                        vec2(18.0, 18.0),
                    );
                    icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                }
                if row.clicked() {
                    globe.tool = Tool::Brush;
                    globe.brush.mode = mode;
                    state.pad.more = false;
                }
            }
        },
    );
}
