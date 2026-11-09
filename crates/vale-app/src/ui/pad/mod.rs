//! The iPad layout of the world workspace: a full-screen canvas with
//! floating cards.

use std::collections::HashMap;

use eframe::egui::{self, Order, Pos2, Rect, pos2, vec2};
use vale_sphere::{LonLat, ProjectionKind, ProjectionSpec};
use vale_store::{GeometryKind, ImageId, Layer, LayerId, LayerKind, RasterId};
use vale_terrain::Mode;

pub use super::LayerRef;
use super::atlas::AtlasTool;
use super::raster_form::{self, RasterForm};
use super::select;
use super::world_form::{self, WorldForm};
use super::{Action, AppState, Note, SaveMark, Workspace, globe, is_height, panels};
use crate::globe::brush::{
    BASIN_DENSITY, CHANNEL_KM, EROSION, FLOODPLAIN_WIDTH_KM, FLOW, NOISE_GRAIN, NOISE_M,
    RANGE_SPACING_KM, RIDGE_CREST_M, RIDGE_MAGMA, RIDGE_OFFSET_KM, RIDGE_SEGMENT_KM,
    RIDGE_SPREAD_MM, SCALE_KM, SEED, SHAPE, STRENGTH_M, TALUS_DEG, TERRACE_STEP_M, WARP_KM,
    needs_height, shape_text,
};
use crate::globe::brush::{BrushSettings, brush_radius};
use crate::globe::combine::PolygonMode;
use crate::globe::line::{DEFAULT_SMOOTHING, LineSettings};
use crate::globe::snap;
use crate::globe::{Tool, WorldView};
use crate::session::{WorldFile, worlds_of};
use crate::world;

pub mod atlas;
pub mod browser;
pub mod cases;
pub mod chip_popover;
pub mod classify;
pub mod dock_ui;
pub mod edge_labels;
pub mod expression;
pub mod eyedropper;
pub mod fields;
pub mod filter;
pub mod flyout;
pub mod fonts;
pub mod furniture;
pub mod geometry;
pub mod grid_labels;
pub mod icons;
pub mod label_text;
pub mod labels;
pub mod number;
pub mod panels_menu;
pub mod panes;
pub mod point_card;
pub mod popover;
pub mod raster;
pub mod rules;
pub mod selectable;
pub mod swipe;
pub mod table;
pub mod text_style;
pub mod theme;
pub mod value;
pub mod widgets;

use flyout::{Choice, Flyout, Flyouts};
use geometry::{
    BrushCard, CELL, Input, MEASURE_CELLS, PAD, ROW, Rects, SELECT_CELLS, TOOLS, TOUCH,
    VERTEX_CELLS, WidthClass,
};
use icons::Icon;
use number::{Number, Scale};
use widgets::{BODY_PAD, Controls, PANEL_ROW};

/// The cell of the tool strip with the tools of the selected vector layer.
pub const VECTOR: &str = "Vector";
/// The tool that draws in a line layer.
pub const LINE: &str = "Line";
/// The tool that draws in a polygon layer.
pub const POLYGON: &str = "Polygon";
/// The tool that places points in a point layer.
pub const POINT: &str = "Point";
/// The tool that splits lines and polygons along a stroke.
pub const SPLIT: &str = "Split";
/// The tool that edits the vertices of lines and polygons.
pub const VERTEX: &str = "Vertex";
/// The switch of the Vertex tool that moves the vertices at one position in
/// all features together.
pub const MOVE_SHARED: &str = "Move shared vertices";
/// The button of the Vertex tool that deletes the selected vertices.
pub const DELETE_VERTEX: &str = "Delete vertex";
/// The button of the Vertex tool that smooths the selected part.
pub const SMOOTH_VERTICES: &str = "Smooth vertices";

/// A tool of the Vector cell.
pub struct VectorTool {
    pub tool: Tool,
    pub name: &'static str,
    pub icon: Icon,
    /// The kinds of the layers that the tool works in.
    pub kinds: &'static [GeometryKind],
    /// The tool draws new features, and the others edit features.
    pub draws: bool,
}

/// The tools of the Vector cell.
pub const VECTOR_TOOLS: [VectorTool; 5] = [
    VectorTool {
        tool: Tool::Line,
        name: LINE,
        icon: Icon::Line,
        kinds: &[GeometryKind::Line],
        draws: true,
    },
    VectorTool {
        tool: Tool::Polygon,
        name: POLYGON,
        icon: Icon::Polygon,
        kinds: &[GeometryKind::Polygon],
        draws: true,
    },
    VectorTool {
        tool: Tool::Point,
        name: POINT,
        icon: Icon::Point,
        kinds: &[GeometryKind::Point],
        draws: true,
    },
    VectorTool {
        tool: Tool::Vertex,
        name: VERTEX,
        icon: Icon::Vertex,
        kinds: &[GeometryKind::Line, GeometryKind::Polygon],
        draws: false,
    },
    VectorTool {
        tool: Tool::Split,
        name: SPLIT,
        icon: Icon::Split,
        kinds: &[GeometryKind::Line, GeometryKind::Polygon],
        draws: false,
    },
];

/// The tools of the Vector cell for a kind of layer: the tool that draws,
/// and then the tools that edit.
pub fn vector_tools(kind: GeometryKind) -> Vec<&'static VectorTool> {
    let of = |draws: bool| {
        (VECTOR_TOOLS.iter()).filter(move |t| t.draws == draws && t.kinds.contains(&kind))
    };
    of(true).chain(of(false)).collect()
}
/// The slider of the smoothing of the line tool and the polygon tool.
pub const SMOOTHING: &str = "Smoothing";
/// The button of the brush card that turns all snapping on and off.
pub const SNAP: &str = "Snap";
/// The button of the brush card that turns the terrain fit of the line
/// tool on and off.
pub const TERRAIN_FIT: &str = "Terrain fit";
/// The switch of the sheet that turns the terrain fit on and off.
pub const TERRAIN_FIT_SWITCH: &str = "Line terrain fit";
/// The button that makes a stroke from an outline reshape the feature.
pub const RESHAPE: &str = "Reshape";
/// The cell of the brush card that opens the flyout of the modes of the
/// polygon tool.
pub const POLYGON_MODE: &str = "Polygon mode";
/// The modes of the polygon tool.
pub const POLYGON_MODES: [(&str, Icon, PolygonMode); 3] = [
    ("New", Icon::Polygon, PolygonMode::New),
    ("Add", Icon::PolygonAdd, PolygonMode::Add),
    ("Subtract", Icon::PolygonSubtract, PolygonMode::Subtract),
];
/// The cell of the context strip that clears the selection.
pub const DESELECT: &str = "Deselect";
/// The name of the Reshape switch of the Brush panel.
pub const RESHAPE_SWITCH: &str = "Reshape switch";
/// The slider of the balance between the sketch and the terrain.
pub const TERRAIN_BALANCE: &str = "Terrain fit balance";
/// The cell of the tool strip that chooses the Measure tool.
pub const MEASURE: &str = "Measure";
/// The cell of the card of the Measure tool that clears the path.
pub const CLEAR_PATH: &str = "Clear path";
/// The switch of the sheet that turns all snapping on and off.
pub const SNAPPING: &str = "Snapping";
pub const SNAP_VERTICES: &str = "Snap to vertices";
pub const SNAP_EDGES: &str = "Snap to edges";
/// The slider of the snap distance, in points.
pub const SNAP_DISTANCE: &str = "Snap distance";
/// The cell of the tool strip with the modes that shape the height freely.
pub const SCULPT: &str = "Sculpt";
/// The cell of the tool strip with the modes that model the terrain. They
/// paint a height layer only.
pub const TERRAIN: &str = "Terrain";
/// The modes of the Sculpt cell.
pub const SCULPT_MODES: [(&str, Icon, Mode); 7] = [
    ("Raise", Icon::Raise, Mode::Raise),
    ("Lower", Icon::Lower, Mode::Lower),
    ("Smooth", Icon::Smooth, Mode::Smooth),
    ("Flatten", Icon::Flatten, Mode::Flatten),
    ("Warp", Icon::Warp, Mode::Warp),
    ("Push", Icon::Push, Mode::Push),
    ("Noise", Icon::Noise, Mode::Noise),
];
/// The modes of the Terrain cell.
pub const TERRAIN_MODES: [(&str, Icon, Mode); 11] = [
    ("Carve", Icon::Carve, Mode::Carve),
    ("Ridge", Icon::Ridge, Mode::Ridge),
    ("Deposit", Icon::Deposit, Mode::Deposit),
    ("Terrace", Icon::Terrace, Mode::Terrace),
    ("Shape", Icon::Shape, Mode::Shape),
    ("Erode", Icon::Erode, Mode::Erode),
    ("Weather", Icon::Weather, Mode::Weather),
    ("Uplift", Icon::Uplift, Mode::Uplift),
    (panels::OCEAN_RIDGE, Icon::OceanRidge, Mode::OceanRidge),
    (panels::RANGE, Icon::Range, Mode::Range),
    (panels::BASIN, Icon::Basin, Mode::Basin),
];

/// A cell of the tool strip that chooses the brush, and the group of the
/// modes in its flyout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModeGroup {
    Sculpt,
    Terrain,
}

impl ModeGroup {
    pub const ALL: [ModeGroup; 2] = [ModeGroup::Sculpt, ModeGroup::Terrain];

    /// The name of the cell.
    pub fn name(self) -> &'static str {
        match self {
            ModeGroup::Sculpt => SCULPT,
            ModeGroup::Terrain => TERRAIN,
        }
    }

    pub fn modes(self) -> &'static [(&'static str, Icon, Mode)] {
        match self {
            ModeGroup::Sculpt => &SCULPT_MODES,
            ModeGroup::Terrain => &TERRAIN_MODES,
        }
    }

    /// The group of a mode. The modes that need a height layer are Terrain.
    pub fn of(mode: Mode) -> ModeGroup {
        if needs_height(mode) {
            ModeGroup::Terrain
        } else {
            ModeGroup::Sculpt
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn first(self) -> Mode {
        self.modes()[0].2
    }
}

/// The name and the icon of a brush mode.
pub fn mode_face(mode: Mode) -> (&'static str, Icon) {
    ModeGroup::of(mode)
        .modes()
        .iter()
        .find(|(_, _, m)| *m == mode)
        .map_or(("Raise", Icon::Raise), |&(name, icon, _)| (name, icon))
}

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
/// The row that exports the heightmap as a PNG.
pub const EXPORT_PNG: &str = "Export PNG…";

/// The button of the Layers panel that opens the menu of the new layers.
pub const ADD_LAYER: &str = "Add layer";
/// The field of the panel of a vector layer with the name of the layer.
pub const LAYER_NAME: &str = "Layer name";
/// The slider of the panel of a raster layer that sets its opacity.
pub const OPACITY: &str = "Layer opacity";
/// The row of the menu of the new layers that opens the form of a new raster
/// layer.
pub const NEW_RASTER: &str = "Raster…";
/// The row of the menu of the new layers that adds an image layer from a
/// file.
pub const NEW_IMAGE: &str = "Image…";
/// The row of the menu of the new layers that adds vector layers from a
/// GeoJSON file.
pub const NEW_GEOJSON: &str = "GeoJSON…";
/// The row of the menu of the new layers that makes the height layer from a
/// heightmap file. It shows only in a world with no height layer.
pub const NEW_HEIGHTMAP: &str = "Heightmap…";
/// The heading of the rows of the menu of the new layers that make an empty
/// layer.
pub const NEW_SECTION: &str = "New";
/// The heading of the rows of the menu of the new layers that read a file.
pub const IMPORT_SECTION: &str = "Import";
/// The height of a heading of the menu of the new layers.
const MENU_HEADING: f32 = 28.0;
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
pub(crate) const HOLD_SLOP: f32 = 6.0;

/// A row of the layer list that a finger moved to the left, to show its
/// Delete button.
pub type Swipe<K = LayerId> = swipe::Swipe<K>;

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
    Attributes,
}

impl Panel {
    fn key(self) -> &'static str {
        match self {
            Panel::Brush => "pad-brush",
            Panel::Layers => "pad-layers",
            Panel::Toolbox => "pad-toolbox",
            Panel::Attributes => "pad-attributes",
        }
    }
}

/// A page of the panel of a layer. A raster layer has the pages of its
/// style, and a vector layer has the pages of its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Ramps,
    Stops,
    Hillshade,
    Blend,
    /// The field with this index.
    Field(usize),
    NewField,
    /// The rule with this index.
    Rule(usize),
    /// The symbols that the rule with this index can pick.
    Symbols(usize),
    /// The shapes of the marker of the rule with this index.
    MarkerShape(usize),
    /// The fill of the marker of the rule with this index.
    MarkerFill(usize),
    /// The attribute table of the layer.
    Table,
    /// The form that replaces the rules with a rule for each class.
    Classify,
    /// The labels of the colors of the ramp with this index.
    RampLabels(usize),
    /// The mask of the style of the layer.
    Mask,
    /// The label text of the layer.
    LabelText,
}

/// The state of the iPad layout.
#[derive(Default)]
pub struct PadState {
    /// The dock of each workspace.
    pub docks: panes::Docks,
    /// The workspace of the dock that the methods of the panels use.
    pub workspace: Workspace,
    /// What the dock does with a finger in the last frame.
    pub dock_drag: Option<dock_ui::DockDrag>,
    /// The dock of the World workspace in the last frame.
    pub dock_rect: Option<Rect>,
    /// The page of the panel of the layer of `layer` that the Layers panel
    /// shows.
    pub page: Option<Page>,
    /// The texts of the open page of a field.
    field: Option<fields::Draft>,
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
    /// The row of the layer list that a finger swiped.
    pub swipe: Option<Swipe>,
    /// The layer that the user renames in the layer list, and the text of
    /// its name field.
    pub rename: Option<(LayerId, String)>,
    /// The text of the name field of the panel of a vector layer.
    name: Option<(LayerRef, String)>,
    /// The list of the projections of the flat view is open.
    pub projections: bool,
    /// The list of the projections in the last frame.
    pub projections_rect: Option<Rect>,
    /// The view options are open.
    pub view_options: bool,
    /// The view options in the last frame.
    pub view_options_rect: Option<Rect>,
    /// The card of the center of the projection in the last frame.
    pub center_rect: Option<Rect>,
    /// The workspace menu is open.
    pub menu: bool,
    /// The Panels menu is open.
    pub panels_menu: bool,
    /// The Panels menu in the last frame.
    pub panels_menu_rect: Option<Rect>,
    /// The worlds of the open menu of the worlds. `None`: the menu is closed.
    pub worlds: Option<Vec<WorldFile>>,
    /// The menu of the worlds in the last frame.
    pub worlds_rect: Option<Rect>,
    /// The cells with a flyout, and the open flyout.
    pub flyouts: Flyouts,
    /// The list of the Selectable cell of the Select tool is open.
    pub selectable: bool,
    /// The last mode that was chosen in each group, by `ModeGroup::ALL`.
    /// `None`: the first mode of the group.
    group_modes: [Option<Mode>; 2],
    /// The last tool that was chosen in the Vector cell.
    vector_tool: Option<Tool>,
    /// The name of the tool that the Vector cell shows, while it is on.
    pub vector_face: Option<&'static str>,
    /// The distance that the sheet is pulled down from its open place.
    pub sheet_offset: f32,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The places of the cards in the last frame.
    pub rects: Option<Rects>,
    /// The pane of each open tab of the last frame, by its panel.
    pub panels: Vec<(Panel, Rect)>,
    /// The canvas had a tap that did not paint, since the last frame.
    pub canvas_tap: bool,
    /// A pen or a tool took the touch on the canvas.
    pub canvas_paint: bool,
    /// The workspace menu of the last frame.
    pub menu_rect: Option<Rect>,
    /// The label and the icon of each brush cell in the last frame, by
    /// `ModeGroup::ALL`.
    pub brush_faces: [Option<(&'static str, Icon)>; 2],
    /// The height of the contents of each panel, by the key of the panel.
    heights: HashMap<&'static str, f32>,
    /// The Style section of the panel of a raster layer.
    pub style: raster::StyleState,
    /// The Style section of the panel of a vector layer.
    pub rules: rules::RulesState,
}

impl PadState {
    /// The dock of the workspace.
    pub fn dock(&self) -> &panes::Dock {
        match self.workspace {
            Workspace::Globe => &self.docks.world,
            Workspace::Atlas => &self.docks.atlas,
        }
    }

    pub fn dock_mut(&mut self) -> &mut panes::Dock {
        match self.workspace {
            Workspace::Globe => &mut self.docks.world,
            Workspace::Atlas => &mut self.docks.atlas,
        }
    }

    /// The panel is a tab of a pane of the dock.
    pub fn is_open(&self, panel: Panel) -> bool {
        self.dock().is_open(panel)
    }

    /// The panel is the open tab of its pane.
    pub fn shows(&self, panel: Panel) -> bool {
        self.dock().shows(panel)
    }

    /// Shows a panel in the dock.
    pub fn open(&mut self, panel: Panel) {
        if !self.dock().shows(panel) {
            self.sheet_offset = 0.0;
        }
        self.dock_mut().open(panel);
        self.swipe = None;
    }

    pub fn close(&mut self, panel: Panel) {
        self.dock_mut().close(panel);
    }

    /// Closes a panel that shows, and else shows it.
    pub fn toggle(&mut self, panel: Panel) {
        if self.shows(panel) {
            self.close(panel);
        } else {
            self.open(panel);
        }
    }

    /// The pane that a sheet shows: the pane of the panel of the last
    /// focus, or else the last pane.
    pub fn sheet_pane(&self) -> Option<usize> {
        let dock = self.dock();
        let focus = dock.focus.and_then(|f| dock.find(f)).map(|(i, _)| i);
        focus.or_else(|| dock.panes.len().checked_sub(1))
    }

    /// The panel that a sheet shows.
    pub fn sheet_panel(&self) -> Option<Panel> {
        self.sheet_pane().map(|i| self.dock().panes[i].panel())
    }
}

pub const WORKSPACES: [&str; 2] = ["World", "Atlas"];
/// The Toolbox panel and its tile in the Panels menu.
pub const TOOLBOX: &str = "Toolbox";
/// The Layers panel and its tile in the Panels menu.
pub const LAYERS: &str = "Layers";
/// The button that opens the Panels menu.
pub const PANELS: &str = "Panels";
const VIEWS: [&str; 2] = ["Globe", "Flat"];
/// The action that makes the place at the middle of the canvas the center of
/// the projection of the flat view.
pub const RECENTER: &str = "Recenter";
/// The action that puts the center of the projection and the view back.
pub const RESET: &str = "Reset";
/// The control that opens the list of the projections.
pub const PROJECTION: &str = "Projection";
/// The button that opens the view options.
pub const VIEW_OPTIONS: &str = "View options";
/// The switch of the view options that shows the graticule.
pub const GRATICULE: &str = "Graticule";
/// The width that the button of the view options and its line add to the
/// view card.
const OPTIONS_LEAD: f32 = TOUCH + 9.0;
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

/// The text of the readout: the place and the value, or the result of the
/// Measure tool.
pub fn readout_line(state: &AppState) -> String {
    if let Some(text) = state.measure_readout() {
        return text;
    }
    let value = state.readout_value();
    readout_text(state.cursor_lonlat, value.as_deref())
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    state.pad.workspace = Workspace::Globe;
    widgets::set_backdrop(&ctx, state.globe.backdrop());
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
        // projections, a line, and the Recenter and Reset buttons. The
        // button of the view options is at the right.
        view_width: widgets::segments_width(ui, &VIEWS)
            + OPTIONS_LEAD
            + if flat { 3.0 * TOUCH + 9.0 + PAD } else { 0.0 },
        panel: has_sheet(state, class),
        sheet_offset: state.pad.sheet_offset,
        cells: card_cells(&state.globe),
        strip: has_strip(&state.globe),
        extra: extra_cells(state),
        tools: TOOLS,
        row: class == WidthClass::Compact,
        dock_width: state.pad.docks.world.width,
    };
    // A tap on the canvas closes the unpinned pane. A tap that closes the
    // keypad or the keyboard does only that.
    let typing = widgets::keypad_is_open(&ctx) || ctx.egui_wants_keyboard_input();
    if std::mem::take(&mut state.pad.canvas_tap) && class != WidthClass::Compact && !typing {
        state.pad.docks.world.tap_canvas();
        input.panel = has_sheet(state, class);
    }
    // The Brush panel holds the settings of the context strip.
    if !has_strip(&state.globe) && card_cells(&state.globe) == 0 || state.globe.selecting() {
        state.pad.close(Panel::Brush);
        input.panel = has_sheet(state, class);
    }
    let mut rects = geometry::layout(&input);
    state.pad.sheet_offset = input
        .sheet_offset
        .clamp(-rects.sheet_rise, rects.sheet_travel);

    let mut controls = std::mem::take(&mut state.pad.controls);
    controls.clear();
    state.pad.panels.clear();
    state.pad.flyouts.begin_frame();
    state.pad.menu_rect = None;
    state.pad.add_button = None;
    if !state.globe.brushing() || state.globe.brush.mode != Mode::Flatten {
        state.globe.pick_level = false;
    }
    let flyouts = &mut state.pad.flyouts;
    if !state.globe.paintable && matches!(flyouts.open, Some(Flyout::Brush(_))) {
        flyouts.open = None;
    }
    let one_tool = vector_kind(&state.globe).is_none_or(|k| vector_tools(k).len() < 2);
    if one_tool && flyouts.open == Some(Flyout::Vector) {
        flyouts.open = None;
    }

    top_row(&ctx, state, &mut controls, &rects);
    tool_strip(&ctx, state, &mut controls, &rects);
    brush_card(&ctx, state, &mut controls, &rects);
    // A button above can open or close a panel. The panels use the new state.
    input.panel = has_sheet(state, class);
    input.cells = card_cells(&state.globe);
    input.sheet_offset = state.pad.sheet_offset;
    rects = geometry::layout(&input);
    table::check(state);
    table::widen(state, &mut rects);
    expression::widen(state, &mut rects);
    state.sync_selection_form(state.pad.shows(Panel::Attributes));
    select::drop_focus(&ctx, state);
    dock_ui::show(&ctx, state, &mut controls, &rects);
    super::new_feature::keep_in_view(&ctx, state, &rects);
    expire_notes(&ctx, state);
    raster::band_card(&ctx, state, &mut controls, &rects);
    raster::shade_color_frame(&ctx, state);
    rules::picker_frame(&ctx, state);
    raster::reset_card(&ctx, state, &mut controls, &rects);
    raster::ramp_card(&ctx, state, &mut controls, &rects);
    readout(&ctx, ui, state, &rects);
    held_card(&ctx, state, &mut controls, &rects);
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
    state.pad.view_options_rect = None;
    if state.pad.view_options && rects.view.is_some() {
        view_options(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.view_options = false;
    }
    if state.pad.menu && class != WidthClass::Wide {
        menu(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.menu = false;
    }
    flyout::tooltip(&ctx, state.globe.backdrop(), &mut state.pad.flyouts, &rects);
    state.pad.panels_menu_rect = None;
    if state.pad.panels_menu {
        let groups = panels_menu::world(state);
        panels_menu::show(&ctx, state, &mut controls, &rects, &groups);
    }
    match state.pad.flyouts.open {
        Some(Flyout::Brush(group)) => brush_flyout(&ctx, state, &mut controls, &rects, group),
        Some(Flyout::PolygonMode) => polygon_flyout(&ctx, state, &mut controls, &rects),
        Some(Flyout::Vector) => vector_flyout(&ctx, state, &mut controls, &rects),
        None => {}
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
    super::region::pad(&ctx, state, &mut controls, &rects);
    dock_ui::handle(&ctx, state, &mut controls, &rects);
    if let Some(rect) = canvas.inner {
        globe::paint_canvas(ui, state, rect, theme::CANVAS);
    }

    state.pad.controls = controls;
    state.pad.rects = Some(rects);
}

/// True if a panel is open. On a compact screen, a panel is a sheet, and
/// the card of a new point takes the place of the sheet of the inspector.
fn has_sheet(state: &AppState, class: WidthClass) -> bool {
    let open = !state.pad.dock().panes.is_empty();
    open && !super::new_feature::point_card(state, class)
}

fn top_row(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-workspace",
        rects.workspace,
        rects,
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
                let items = [(WORKSPACES[0], true), (WORKSPACES[1], true)];
                if widgets::segments(ui, controls, &items, 0) == Some(1) {
                    state.workspace = Workspace::Atlas;
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
                // From the right: the button of the view options and a line,
                // and in the flat view Reset, Recenter, a line, and the
                // button of the list of projections.
                let card = ui.max_rect();
                let at = |right: f32| {
                    let min = pos2(right - TOUCH, card.top() + PAD);
                    Rect::from_min_size(min, vec2(TOUCH, TOUCH))
                };
                let options = at(card.right() - PAD);
                let divider = pos2(options.left() - 4.5, card.center().y);
                let line = Rect::from_center_size(divider, vec2(1.0, 24.0));
                ui.painter().rect_filled(line, 0.0, theme::divider());
                let open = state.pad.view_options;
                let button = widgets::icon_button_at(
                    ui,
                    controls,
                    options,
                    VIEW_OPTIONS,
                    Icon::Grid,
                    open,
                    true,
                );
                if button.clicked() {
                    state.pad.view_options = !open;
                    state.pad.projections = false;
                }
                if state.globe.world_view == WorldView::Flat {
                    let reset = at(options.left() - 9.0);
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
                        state.pad.view_options = false;
                    }
                    center_buttons(ui, state, controls, recenter, reset);
                }
            },
        );
    }
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-actions",
        rects.actions,
        rects,
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
            panels_button(ui, state, controls, at(2.0 * step + 9.0 + PAD));
        },
    );
}

/// The button that opens the Panels menu.
pub(super) fn panels_button(
    ui: &egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rect: Rect,
) {
    let open = state.pad.panels_menu;
    let button = widgets::icon_button_at(ui, controls, rect, PANELS, Icon::Panels, open, true);
    if button.clicked() {
        state.pad.panels_menu = !open;
    }
}

/// Chooses the Measure tool, or goes back from it to Move on the globe and
/// to Frame in the Atlas.
pub fn toggle_measure(state: &mut AppState) {
    let measuring = state.measuring();
    match state.workspace {
        Workspace::Globe => {
            state.globe.tool = if measuring {
                Tool::Navigate
            } else {
                Tool::Measure
            };
        }
        Workspace::Atlas => {
            state.atlas.tool = if measuring {
                AtlasTool::Frame
            } else {
                AtlasTool::Measure
            };
        }
    }
    state.pad.flyouts.open = None;
}

/// The tool has a context strip: the card below the tool strip with the
/// controls of the tool. The Measure tool has its own card.
fn has_strip(globe: &crate::globe::Globe) -> bool {
    globe.brushing() || globe.editing()
}

/// The number of cells of the card of a tool with a card of cells in the
/// place of the context strip.
fn card_cells(globe: &crate::globe::Globe) -> usize {
    if globe.measuring() {
        MEASURE_CELLS
    } else if globe.vertexing() {
        VERTEX_CELLS
    } else if globe.selecting() {
        SELECT_CELLS
    } else {
        0
    }
}

/// The cells that the context strip has in each tool: Deselect while
/// features are selected.
fn extra_cells(state: &AppState) -> usize {
    usize::from(!state.feature_selection.is_empty())
}

/// The cell that clears the selection.
fn deselect_cell(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    if state.feature_selection.is_empty() {
        return;
    }
    if widgets::tool_cell(ui, controls, DESELECT, DESELECT, Icon::Close, false, true).clicked() {
        state.set_feature_selection(Vec::new());
    }
}

/// The child of a card, inside the padding of the card. In a `row`, the
/// cells go from left to right.
fn padded_in(ui: &mut egui::Ui, row: bool) -> egui::Ui {
    let rect = ui.max_rect().shrink(PAD);
    let layout = if row {
        egui::Layout::left_to_right(egui::Align::Min)
    } else {
        egui::Layout::top_down(egui::Align::Min)
    };
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    ui
}

fn tool_strip(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    // An open sheet covers the row of the strip.
    if rects.tool_cells == 0 {
        return;
    }
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-tools",
        rects.tools,
        rects,
        |ui| {
            let mut ui = padded_in(ui, rects.row);
            let mut cells = |ui: &mut egui::Ui| {
                for group in ModeGroup::ALL {
                    brush_cell(ui, state, controls, group);
                }
                vector_cell(ui, state, controls);
                let globe = &state.globe;
                let pan = !globe.brushing()
                    && !globe.editing()
                    && !globe.measuring()
                    && !globe.selecting();
                for (name, label, icon, tool, active) in [
                    (
                        select::TOOL,
                        select::TOOL,
                        Icon::Select,
                        Tool::Select,
                        globe.selecting(),
                    ),
                    ("Pan", "Move", Icon::Move, Tool::Navigate, pan),
                    (
                        MEASURE,
                        MEASURE,
                        Icon::Measure,
                        Tool::Measure,
                        globe.measuring(),
                    ),
                ] {
                    let flyouts = &mut state.pad.flyouts;
                    if flyout::tool(ui, controls, flyouts, None, name, label, icon, active, true) {
                        state.globe.tool = tool;
                        state.pad.flyouts.open = None;
                    }
                }
            };
            if rects.tool_cells < TOOLS {
                egui::ScrollArea::new([rects.row, !rects.row])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut ui, cells);
            } else {
                cells(&mut ui);
            }
        },
    );
}

/// A brush cell. It shows the mode of the brush while that mode is in its
/// group, and else the last mode that was chosen in its group. A tap chooses
/// the brush in that mode, and a tap while the cell is on or a hold opens the
/// flyout of the modes of the group.
fn brush_cell(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, group: ModeGroup) {
    let globe = &mut state.globe;
    let current = ModeGroup::of(globe.brush.mode) == group;
    if current {
        state.pad.group_modes[group.index()] = Some(globe.brush.mode);
    }
    let mode = state.pad.group_modes[group.index()].unwrap_or(group.first());
    let (label, icon) = mode_face(mode);
    // With no raster layer to paint, the tool is Move.
    let enabled = globe.paintable && (group == ModeGroup::Sculpt || globe.is_height());
    let active = globe.brushing() && current;
    let name = group.name();
    let key = Flyout::Brush(group);
    let flyouts = &mut state.pad.flyouts;
    let tapped = flyout::tool(
        ui,
        controls,
        flyouts,
        Some(key),
        name,
        label,
        icon,
        active,
        enabled,
    );
    state.pad.brush_faces[group.index()] = Some((label, icon));
    if tapped {
        if active {
            flyouts.toggle(key);
        } else {
            globe.tool = Tool::Brush;
            globe.brush.mode = mode;
            flyouts.open = None;
        }
    }
}

/// The kind of the selected vector layer, if the globe can edit it.
fn vector_kind(globe: &crate::globe::Globe) -> Option<GeometryKind> {
    if globe.lineable {
        Some(GeometryKind::Line)
    } else if globe.fillable {
        Some(GeometryKind::Polygon)
    } else if globe.pointable {
        Some(GeometryKind::Point)
    } else {
        None
    }
}

/// The Vector cell. It shows the tool while the tool is in its list, and
/// else the last tool that was chosen in it, or the tool that draws. A tap
/// chooses that tool, and a tap while the cell is on or a hold opens the
/// flyout of the tools. With one tool, the cell has no flyout.
fn vector_cell(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let globe = &mut state.globe;
    let flyouts = &mut state.pad.flyouts;
    state.pad.vector_face = None;
    let Some(kind) = vector_kind(globe) else {
        widgets::icon_cell(ui, controls, VECTOR, Icon::Line, false, false);
        return;
    };
    let tools = vector_tools(kind);
    let current = tools.iter().find(|t| t.tool == globe.tool);
    if let Some(t) = current {
        state.pad.vector_tool = Some(t.tool);
    }
    let last = tools.iter().find(|t| Some(t.tool) == state.pad.vector_tool);
    let face = current.or(last).unwrap_or(&tools[0]);
    state.pad.vector_face = Some(face.name);
    let active = current.is_some();
    let modes = (tools.len() >= 2).then_some(Flyout::Vector);
    let tapped = flyout::tool(
        ui, controls, flyouts, modes, VECTOR, face.name, face.icon, active, true,
    );
    if tapped {
        if active && tools.len() >= 2 {
            flyouts.toggle(Flyout::Vector);
        } else {
            globe.tool = face.tool;
            flyouts.open = None;
        }
    }
}

/// The size of the brush on the ground, in the display units of the world.
pub fn brush_size_text(state: &AppState) -> String {
    let world = &state.doc.project.world;
    let globe = &state.globe;
    let km = globe.brush.radius_km(globe.radius(), world.radius_km);
    world::distance(km, world.units)
}

/// Shows the size of the brush on the ground with `control`, and sets the
/// size that it returns. During a drag, a circle shows the brush.
fn brush_size<'a>(
    state: &mut AppState,
    name: &'a str,
    control: impl FnOnce(Number<'a>) -> Option<f64>,
) {
    let world = &state.doc.project.world;
    let (world_km, units) = (world.radius_km, world.units);
    let globe_radius = state.globe.radius();
    let brush = &state.globe.brush;
    let range = brush.radius_km_range(globe_radius, world_km);
    let km = brush.radius_km(globe_radius, world_km);
    let (unit_km, symbol) = world::large_unit(units);
    let mut number = Number::new(name, km, range)
        .label("Size")
        .log()
        .shown(1.0 / unit_km, symbol)
        .text(move |km| world::distance(km, units))
        .preview(move |km| (km / world_km * globe_radius) as f32);
    if brush.lock.is_none() {
        let points = BrushSettings::default().size_points;
        number = number.default(brush_radius(points, globe_radius) * world_km);
    }
    if let Some(km) = control(number) {
        state.globe.brush.set_radius_km(km, globe_radius, world_km);
    }
}

/// A number from 0 to 1 in whole percents.
fn percent(name: &str, value: f64) -> Number<'_> {
    Number::new(name, value, 0.0..=1.0).percent()
}

fn brush_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(rect) = rects.brush else {
        return;
    };
    if rects.card_cells > 0 {
        cell_card(ctx, state, controls, rects);
        return;
    }
    // A tool with no context strip has a strip of the extra cells alone.
    if !has_strip(&state.globe) {
        widgets::edge_card(
            ctx,
            state.globe.backdrop(),
            "pad-brush-card",
            rect,
            rects,
            |ui| deselect_cell(&mut padded_in(ui, rects.row), state, controls),
        );
        return;
    }
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-brush-card",
        rect,
        rects,
        |ui| {
            let mut ui = padded_in(ui, rects.row);
            // In the row layout, a slider is a row as long as `length`.
            let row = rects.row;
            let slide = |number: Number<'_>, ui: &mut egui::Ui, controls: &mut Controls, length| {
                if row {
                    number.horizontal(ui, controls, length)
                } else {
                    number.vertical(ui, controls, length)
                }
            };
            // Each control shows while the tool can use it.
            let paint = state.globe.brushing();
            let line = state.globe.drawing();
            let stroke = state.globe.stroking();
            let vector = stroke || state.globe.placing();
            if let BrushCard::Full(height) = rects.brush_card {
                // The tools of a vector layer have the snap button. The line
                // tool, the polygon tool, and Split also have the smoothing.
                // The line tool and the polygon tool also have the reshape
                // button. The line tool also has the terrain fit button, and
                // the polygon tool has its modes.
                let fit = state.globe.lining();
                let fill = state.globe.filling();
                if vector {
                    if fill {
                        let mode = state.globe.line.polygon_mode;
                        let (label, icon, _) = POLYGON_MODES
                            .into_iter()
                            .find(|m| m.2 == mode)
                            .expect("a mode");
                        let key = Flyout::PolygonMode;
                        let flyouts = &mut state.pad.flyouts;
                        let on = mode != PolygonMode::New;
                        if flyout::cell(
                            &mut ui,
                            controls,
                            flyouts,
                            key,
                            POLYGON_MODE,
                            label,
                            icon,
                            on,
                            true,
                        ) {
                            flyouts.toggle(key);
                        }
                    }
                    if stroke {
                        let cells = [true, line, fit || fill].iter().filter(|&&c| c).count() as f32;
                        let cell = if row { CELL.0 } else { CELL.1 };
                        let length = 2.0 * height - cells * cell;
                        let smoothing = &mut state.globe.line.smoothing;
                        let number = percent(SMOOTHING, *smoothing).default(DEFAULT_SMOOTHING);
                        if let Some(value) = slide(number, &mut ui, controls, length) {
                            *smoothing = value;
                        }
                    }
                    let snap = &mut state.globe.snap.on;
                    let cell = widgets::tool_cell(
                        &mut ui,
                        controls,
                        SNAP,
                        SNAP,
                        Icon::Magnet,
                        *snap,
                        true,
                    );
                    if cell.clicked() {
                        *snap = !*snap;
                    }
                    if line {
                        // Reshape works in the New mode of the polygon tool.
                        let new = state.globe.line.polygon_mode == PolygonMode::New;
                        let enabled = !fill || new;
                        let on = &mut state.globe.line.reshape;
                        let cell = widgets::tool_cell(
                            &mut ui,
                            controls,
                            RESHAPE,
                            RESHAPE,
                            Icon::Reshape,
                            *on && enabled,
                            enabled,
                        );
                        if cell.clicked() {
                            *on = !*on;
                        }
                    }
                    if fit {
                        let terrain = state.globe.fit_layer.is_some();
                        let on = &mut state.globe.line.fit;
                        let active = *on && terrain;
                        let cell = widgets::tool_cell(
                            &mut ui,
                            controls,
                            TERRAIN_FIT,
                            "Fit",
                            Icon::Range,
                            active,
                            terrain,
                        );
                        if cell.clicked() {
                            *on = !*on;
                        }
                    }
                } else if paint {
                    // The outline sets the size of a basin, and a basin has no
                    // flow. The sliders keep their place in the card.
                    let on = state.globe.brush.mode != Mode::Basin;
                    brush_size(state, "Size", |n| {
                        slide(n.enabled(on), &mut ui, controls, height)
                    });
                    let flow = &mut state.globe.brush.flow;
                    let number = Number::new("Flow", *flow, FLOW).percent().enabled(on);
                    if let Some(value) = slide(number, &mut ui, controls, height) {
                        *flow = value;
                    }
                }
            }
            deselect_cell(&mut ui, state, controls);
            let open = state.pad.is_open(Panel::Brush);
            let button = widgets::tool_cell(
                &mut ui,
                controls,
                "Brush settings",
                "Brush",
                Icon::Brush,
                open,
                true,
            );
            if button.clicked() {
                state.pad.toggle(Panel::Brush);
            }
        },
    );
}

/// The card of cells of the Measure tool, the Vertex tool, or the Select
/// tool, in the place of the brush card.
pub(super) fn cell_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let Some(rect) = rects.brush.filter(|_| rects.card_cells > 0) else {
        return;
    };
    let mut merge_cell = None;
    let mut selectable_cell = None;
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-brush-card",
        rect,
        rects,
        |ui| {
            let mut ui = padded_in(ui, rects.row);
            let globe = state.workspace == Workspace::Globe;
            let vertexing = state.globe.vertexing() && globe;
            let selecting = state.globe.selecting() && globe;
            let own = if vertexing {
                VERTEX_CELLS
            } else if selecting {
                SELECT_CELLS
            } else {
                MEASURE_CELLS
            };
            let all = own + extra_cells(state);
            let mut cells = |ui: &mut egui::Ui| {
                if vertexing {
                    vertex_cells(ui, state, controls);
                } else if selecting {
                    selectable_cell = Some(selectable::cell(ui, state, controls));
                    merge_cell = select_cells(ui, state, controls);
                } else {
                    measure_cells(ui, state, controls);
                }
            };
            if rects.card_cells < all {
                egui::ScrollArea::new([rects.row, !rects.row])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut ui, cells);
            } else {
                cells(&mut ui);
            }
        },
    );
    match merge_cell {
        Some(cell) if state.merging => merge_menu(ctx, state, controls, rects, cell),
        _ => state.merging = false,
    }
    match selectable_cell {
        Some(cell) if state.pad.selectable => selectable::menu(ctx, state, controls, rects, cell),
        _ => state.pad.selectable = false,
    }
}

/// The list of Merge, at the right of its cell: one row for each selected
/// feature, the largest first. A tap on a row merges the selection into
/// that feature. A touch outside the list closes it.
fn merge_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    cell: Rect,
) {
    let choices = state.merge_choices();
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let font = theme::body();
    let widest = (choices.iter().map(|c| c.1.as_str()))
        .chain([select::MERGE_TITLE])
        .map(|t| ctx.fonts_mut(|f| f.layout_no_wrap(t.to_owned(), font.clone(), theme::TEXT)))
        .map(|g| g.size().x)
        .fold(0.0, f32::max);
    let screen = rects.screen.shrink(PAD);
    let width = (widest + 2.0 * widgets::BODY_PAD + 16.0).clamp(220.0, 360.0);
    let height = 20.0 + (choices.len() + 1) as f32 * widgets::PANEL_ROW;
    // In the row layout, the list is above its cell.
    let rect = if rects.row {
        let height = height.min(cell.top() - 8.0 - rects.column_top);
        let left = cell.right() - width;
        let left = left.min(screen.right() - width).max(screen.left());
        Rect::from_min_size(pos2(left, cell.top() - 8.0 - height), vec2(width, height))
    } else {
        let top = cell.top().min(screen.bottom() - height).max(screen.top());
        Rect::from_min_size(
            pos2(cell.right() + 2.0 * PAD + 8.0, top),
            vec2(width, height),
        )
    };
    if choices.is_empty() || pressed.is_some_and(|p| !rect.contains(p) && !cell.contains(p)) {
        state.merging = false;
        return;
    }
    let mut keep = None;
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-merge",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(widgets::BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let (title, _) = ui.allocate_exact_size(
                vec2(inner.width(), widgets::PANEL_ROW),
                egui::Sense::hover(),
            );
            ui.painter().text(
                title.left_center(),
                egui::Align2::LEFT_CENTER,
                select::MERGE_TITLE,
                theme::body(),
                theme::MUTE,
            );
            for (uuid, name) in &choices {
                if widgets::text_row(&mut ui, controls, name, 0.0, false, true).clicked() {
                    keep = Some(*uuid);
                }
            }
        },
    );
    if let Some(uuid) = keep {
        state.merge_selection(uuid);
    }
}

/// The cells of the card of the Vertex tool: the snap switch, the switch of
/// the move of shared vertices, Delete vertex, Smooth vertices, and the
/// Brush button, which opens the snapping settings.
fn vertex_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let snap = &mut state.globe.snap.on;
    if widgets::tool_cell(ui, controls, SNAP, SNAP, Icon::Magnet, *snap, true).clicked() {
        *snap = !*snap;
    }
    let shared = &mut state.globe.vertex.shared;
    if widgets::tool_cell(
        ui,
        controls,
        MOVE_SHARED,
        "Shared",
        Icon::Shared,
        *shared,
        true,
    )
    .clicked()
    {
        *shared = !*shared;
    }
    let can = state.can_delete_vertices();
    if widgets::tool_cell(
        ui,
        controls,
        DELETE_VERTEX,
        "Delete",
        Icon::Delete,
        false,
        can,
    )
    .clicked()
    {
        state.delete_vertices();
    }
    let can = state.can_smooth_vertices();
    if widgets::tool_cell(
        ui,
        controls,
        SMOOTH_VERTICES,
        "Smooth",
        Icon::Smooth,
        false,
        can,
    )
    .clicked()
    {
        state.smooth_vertices();
    }
    deselect_cell(ui, state, controls);
    brush_button(ui, state, controls);
}

/// The cells of the card of the Select tool after Selectable: Add, Delete
/// selection, Merge, Subtract, and Deselect. Returns the place of the Merge
/// cell.
fn select_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) -> Option<Rect> {
    let add = state.select_add;
    let name = select::ADD_TO_SELECTION;
    if widgets::tool_cell(ui, controls, name, "Add", Icon::Plus, add, true).clicked() {
        state.select_add = !add;
    }
    let (name, any) = (select::DELETE, !state.feature_selection.is_empty());
    if widgets::tool_cell(ui, controls, name, "Delete", Icon::Trash, false, any).clicked() {
        state.delete_selection();
    }
    let (open, can) = (state.merging, state.can_merge());
    let merge = widgets::tool_cell(ui, controls, select::MERGE, "Merge", Icon::Merge, open, can);
    if merge.clicked() {
        if open {
            state.merging = false;
        } else {
            state.start_merge();
        }
    }
    let (name, can) = (select::SUBTRACT, state.can_subtract());
    if widgets::tool_cell(ui, controls, name, name, Icon::Subtract, false, can).clicked() {
        state.subtract_selection();
    }
    deselect_cell(ui, state, controls);
    Some(merge.rect)
}

/// The cells of the card of the Measure tool: the snap switch, the Clear
/// path cell, and the Brush button, which opens the snapping settings.
fn measure_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let snap = &mut state.globe.snap.on;
    if widgets::tool_cell(ui, controls, SNAP, SNAP, Icon::Magnet, *snap, true).clicked() {
        *snap = !*snap;
    }
    let path = &mut state.measure;
    let filled = !path.is_empty();
    if widgets::tool_cell(
        ui,
        controls,
        CLEAR_PATH,
        "Clear",
        Icon::Close,
        false,
        filled,
    )
    .clicked()
    {
        path.clear();
    }
    deselect_cell(ui, state, controls);
    brush_button(ui, state, controls);
}

/// The Brush button of a card of cells, which opens the Brush panel.
fn brush_button(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let open = state.pad.is_open(Panel::Brush);
    let button = widgets::tool_cell(
        ui,
        controls,
        "Brush settings",
        "Brush",
        Icon::Brush,
        open,
        true,
    );
    if button.clicked() {
        state.pad.toggle(Panel::Brush);
    }
}

/// The title of a panel in its header and in its tile.
pub fn panel_name(state: &AppState, panel: Panel) -> &'static str {
    match panel {
        Panel::Brush if state.measuring() => MEASURE,
        Panel::Brush if state.globe.lining() => LINE,
        Panel::Brush if state.globe.filling() => POLYGON,
        Panel::Brush if state.globe.placing() => POINT,
        Panel::Brush if state.globe.splitting() => SPLIT,
        Panel::Brush if state.globe.vertexing() => VERTEX,
        Panel::Brush => panels_menu::BRUSH,
        Panel::Layers => LAYERS,
        Panel::Toolbox => TOOLBOX,
        Panel::Attributes => select::ATTRIBUTES,
    }
}

pub fn panel_icon(panel: Panel) -> Icon {
    match panel {
        Panel::Brush => Icon::Brush,
        Panel::Layers => Icon::Layers,
        Panel::Toolbox => Icon::Toolbox,
        Panel::Attributes => Icon::Attributes,
    }
}

/// The page of a panel that its pane shows, with the parts of its title.
pub(super) struct Head {
    /// The name of the page. The pane keeps the height of the page under
    /// this name.
    pub key: &'static str,
    pub panel: Panel,
    /// The title of the page.
    pub title: String,
    /// The name of the page that the back button opens. `None`: the page
    /// is the first page of the panel.
    pub back: Option<String>,
    /// The action button: its name, its icon, and its state.
    pub action: Option<(&'static str, Icon, bool)>,
    pub scroll: egui::scroll_area::ScrollSource,
    layer: Option<LayerRef>,
    page: Option<Page>,
}

/// Finds the page that a panel shows, and closes a page that is gone.
pub(super) fn panel_head(state: &mut AppState, panel: Panel) -> Head {
    let shown = state.pad.layer.and_then(|layer| state.layer_of(layer));
    let name = shown.map(|layer| layer.name.clone());
    let field_count = shown.map_or(0, |layer| layer.fields.len());
    let field_name = match state.pad.page {
        Some(Page::Field(i)) => shown.and_then(|layer| layer.fields.get(i)),
        _ => None,
    }
    .map(|field| field.name.clone());
    let rules = match state.pad.layer {
        Some(LayerRef::Vector(id)) => state.doc.entry(id).map(|e| &e.style.style.rules),
        _ => None,
    };
    let rule_count = rules.map_or(0, Vec::len);
    let rule_name = match state.pad.page {
        Some(Page::Rule(i) | Page::Symbols(i) | Page::MarkerShape(i) | Page::MarkerFill(i)) => {
            rules.and_then(|r| r.get(i)).map(|r| r.name.clone())
        }
        _ => None,
    };
    if name.is_none() {
        state.pad.layer = None;
    }
    let layer = state.pad.layer.filter(|_| panel == Panel::Layers);
    let fits = match (layer, state.pad.page) {
        (
            Some(LayerRef::Raster(_)),
            Some(Page::Ramps | Page::Stops | Page::Hillshade | Page::Blend | Page::RampLabels(_)),
        ) => true,
        (Some(LayerRef::Image(_)), Some(Page::Blend)) => true,
        (
            Some(LayerRef::Vector(_)),
            Some(
                Page::NewField
                | Page::Table
                | Page::Classify
                | Page::Ramps
                | Page::RampLabels(_)
                | Page::Mask
                | Page::LabelText,
            ),
        ) => true,
        (Some(LayerRef::Vector(_)), Some(Page::Field(i))) => i < field_count,
        (
            Some(LayerRef::Vector(_)),
            Some(Page::Rule(i) | Page::Symbols(i) | Page::MarkerShape(i) | Page::MarkerFill(i)),
        ) => i < rule_count,
        _ => false,
    };
    if panel == Panel::Layers && !fits {
        state.pad.page = None;
    }
    let page = state.pad.page.filter(|_| panel == Panel::Layers);
    if panel == Panel::Layers && !matches!(page, Some(Page::Field(_) | Page::NewField)) {
        state.pad.field = None;
    }
    // The panel of a raster layer or of an image layer has the controls of
    // the selected layer.
    if let Some(LayerRef::Raster(_) | LayerRef::Image(_)) = layer
        && state.selected_layer != layer
    {
        state.select(layer);
    }
    let name = name.as_deref().unwrap_or_default();
    let (key, title, back): (&'static str, &str, Option<&str>) = match (panel, layer) {
        (Panel::Brush, _) => (panel.key(), panel_name(state, panel), None),
        (Panel::Layers, Some(LayerRef::Raster(_))) if page == Some(Page::Ramps) => {
            ("pad-ramps", raster::COLOR_RAMPS, Some(raster::COLOR_STOPS))
        }
        (Panel::Layers, Some(LayerRef::Raster(_) | LayerRef::Vector(_)))
            if matches!(page, Some(Page::RampLabels(_))) =>
        {
            ("pad-ramp-labels", raster::LABELS, Some(raster::COLOR_RAMPS))
        }
        (Panel::Layers, Some(LayerRef::Raster(_))) if page == Some(Page::Stops) => {
            ("pad-stops", raster::COLOR_STOPS, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Raster(_))) if page == Some(Page::Hillshade) => {
            ("pad-hillshade", raster::HILLSHADE, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Raster(_))) if page == Some(Page::Blend) => {
            ("pad-blend", raster::BLEND, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Raster(_))) => ("pad-layer", name, Some("Layers")),
        (Panel::Layers, Some(LayerRef::Image(_))) if page == Some(Page::Blend) => {
            ("pad-blend", raster::BLEND, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Image(_))) => ("pad-image", name, Some("Layers")),
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::Table) => {
            ("pad-table", table::TITLE, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::Ramps) => {
            ("pad-ramps", raster::COLOR_RAMPS, Some(classify::TITLE))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::Classify) => {
            ("pad-classify", classify::TITLE, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::Mask) => {
            ("pad-mask", rules::MASK, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::LabelText) => {
            ("pad-label-text", label_text::LABEL_TEXT, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::NewField) => {
            ("pad-new-field", fields::NEW_FIELD, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::Field(_))) => (
            "pad-field",
            field_name.as_deref().unwrap_or_default(),
            Some(name),
        ),
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::Symbols(_))) => {
            ("pad-symbols", rules::SYMBOLS, rule_name.as_deref())
        }
        (Panel::Layers, Some(LayerRef::Vector(_)))
            if matches!(page, Some(Page::MarkerShape(_))) =>
        {
            (
                "pad-marker-shape",
                rules::MARKER_SHAPE,
                rule_name.as_deref(),
            )
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::MarkerFill(_))) => {
            ("pad-marker-fill", rules::FILL, rule_name.as_deref())
        }
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::Rule(_))) => (
            "pad-rule",
            rule_name.as_deref().unwrap_or_default(),
            Some(name),
        ),
        (Panel::Layers, Some(LayerRef::Vector(_))) => ("pad-vector", name, Some("Layers")),
        (Panel::Layers, None) => (panel.key(), "Layers", None),
        (Panel::Toolbox, _) => (panel.key(), TOOLBOX, None),
        (Panel::Attributes, _) => (panel.key(), select::ATTRIBUTES, None),
    };
    let list = panel == Panel::Layers && layer.is_none();
    if list && state.pad.reorder.is_none() {
        state.pad.name = None;
    }
    if !list {
        state.pad.reorder = None;
        state.pad.swipe = None;
    }
    let scroll = match state.pad.reorder {
        Some(_) => egui::scroll_area::ScrollSource::NONE,
        None => egui::scroll_area::ScrollSource::default(),
    };
    let action = match panel {
        Panel::Attributes => Some((select::FOCUS_NEW, Icon::TextCursor, state.tools.focus_new)),
        _ => list.then_some((ADD_LAYER, Icon::Plus, state.pad.add)),
    };
    Head {
        key,
        panel,
        title: title.to_owned(),
        back: back.map(str::to_owned),
        action,
        scroll,
        layer,
        page,
    }
}

/// Does what the user did with the back button and the action button of
/// a panel.
pub(super) fn panel_title(
    state: &mut AppState,
    head: &Head,
    back: bool,
    action: Option<&egui::Response>,
) {
    let (panel, layer, page) = (head.panel, head.layer, head.page);
    match action {
        Some(focus) if panel == Panel::Attributes => {
            if focus.clicked() {
                state.tools.focus_new = !state.tools.focus_new;
            }
        }
        Some(add) => {
            state.pad.add_button = Some(add.rect);
            if add.clicked() {
                state.pad.add = !state.pad.add;
            }
        }
        None => {}
    }
    if back && page == Some(Page::Ramps) {
        let vector = matches!(layer, Some(LayerRef::Vector(_)));
        state.pad.page = Some(if vector { Page::Classify } else { Page::Stops });
    } else if back && matches!(page, Some(Page::RampLabels(_))) {
        state.pad.page = Some(Page::Ramps);
    } else if let (true, Some(Page::Symbols(i) | Page::MarkerShape(i) | Page::MarkerFill(i))) =
        (back, page)
    {
        state.pad.page = Some(Page::Rule(i));
    } else if back && page.is_some() {
        state.pad.page = None;
        state.globe.preview.selected = None;
    } else if back {
        state.pad.layer = None;
    }
}

/// The rows of a panel between its title and its body, which do not
/// scroll. The table fills the page, and its rows have their own scroll.
pub(super) fn panel_top(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    head: &Head,
) {
    if head.panel == Panel::Layers && head.page == Some(Page::Table) {
        let size = ui.available_size();
        ui.allocate_ui(size, |ui| table::page(ui, state, controls));
    }
}

/// The body of a panel.
pub(super) fn panel_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    head: &Head,
) {
    let (panel, layer, page) = (head.panel, head.layer, head.page);
    match (panel, layer) {
        (Panel::Brush, _) => brush_body(ui, state, controls),
        (Panel::Layers, Some(LayerRef::Raster(id))) => match page {
            Some(Page::Ramps) => {
                raster::ramps_body(ui, state, controls, raster::RampUser::Raster(id))
            }
            Some(Page::RampLabels(i)) => raster::labels_body(ui, state, controls, i),
            Some(Page::Stops) => raster::stops_body(ui, state, controls, id),
            Some(Page::Hillshade) => {
                raster::hillshade_body(ui, state, controls, id);
            }
            Some(Page::Blend) => raster::blend_body(ui, state, controls, id),
            _ => raster_body(ui, state, controls, id),
        },
        (Panel::Layers, Some(LayerRef::Image(id))) => match page {
            Some(Page::Blend) => raster::image_blend_body(ui, state, controls, id),
            _ => image_body(ui, state, controls, id),
        },
        (Panel::Layers, Some(LayerRef::Vector(id))) => match page {
            Some(Page::NewField) => {
                fields::new_field_body(ui, state, controls, id);
            }
            Some(Page::Field(i)) => {
                fields::field_body(ui, state, controls, id, i);
            }
            Some(Page::Rule(i)) => rules::rule_body(ui, state, controls, id, i),
            Some(Page::Symbols(i)) => rules::symbols_body(ui, state, controls, id, i),
            Some(Page::MarkerShape(i)) => rules::shape_body(ui, state, controls, id, i),
            Some(Page::MarkerFill(i)) => rules::marker_fill_body(ui, state, controls, id, i),
            Some(Page::Classify) => classify::body(ui, state, controls, id),
            Some(Page::Ramps) => {
                raster::ramps_body(ui, state, controls, raster::RampUser::Classify(id))
            }
            Some(Page::RampLabels(i)) => raster::labels_body(ui, state, controls, i),
            Some(Page::Mask) => rules::mask_body(ui, state, controls, id),
            Some(Page::LabelText) => fonts::text_body(ui, state, controls, id),
            Some(Page::Table) => {}
            _ => vector_body(ui, state, controls, id),
        },
        (Panel::Layers, None) => layers_body(ui, state, controls),
        (Panel::Toolbox, _) => toolbox_body(ui, state),
        (Panel::Attributes, _) => select::body(ui, state, controls),
    }
}

/// Shows a panel as one card at the place of the dock. The Atlas shows its
/// panels so.
pub(super) fn show_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    panel: Panel,
) {
    let head = panel_head(state, panel);
    let title = CardTitle {
        title: &head.title,
        back: head.back.as_deref(),
        action: head.action,
    };
    let card = Card {
        key: head.key,
        place: (rects.dock.min, rects.dock.width()),
        head: title,
        scroll: head.scroll,
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        panel_title(state, &head, row.back, row.action.as_ref());
        if row.close {
            state.pad.close(panel);
        }
    };
    let top = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        panel_top(ui, state, controls, &head);
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        panel_body(ui, state, controls, &head);
    };
    let rect = show_card(ctx, state, controls, rects, card, on_title, top, body);
    state.pad.panels.push((panel, rect));
}

/// The title row of a panel: the title, the name of the panel that the back
/// button opens, and the action button.
pub struct CardTitle<'a> {
    pub title: &'a str,
    pub back: Option<&'a str>,
    pub action: Option<(&'a str, Icon, bool)>,
}

/// A panel of `show_card`.
pub struct Card<'a> {
    /// The name of the card. The card keeps its height under this name.
    pub key: &'static str,
    /// The top-left corner and the width of the card in the normal widths.
    pub place: (Pos2, f32),
    pub head: CardTitle<'a>,
    pub scroll: egui::scroll_area::ScrollSource,
}

/// Shows a panel: a card in the normal widths, and the sheet at the bottom
/// in the compact width. `on_title` gets the title row before the body
/// shows. `top` adds the rows between the title row and the body, which do
/// not scroll. Returns the rectangle of the panel.
#[allow(clippy::too_many_arguments)]
fn show_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    card: Card,
    on_title: impl FnOnce(&mut AppState, &widgets::TitleRow),
    top: impl FnOnce(&mut egui::Ui, &mut AppState, &mut Controls),
    body: impl FnOnce(&mut egui::Ui, &mut AppState, &mut Controls),
) -> Rect {
    let Card {
        key,
        place: (min, width),
        head,
        scroll,
    } = card;
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
            let most = (rects.dock.bottom() - min.y).max(ROW);
            let height = state.pad.heights.get(key).map_or(most, |h| h.min(most));
            let rect = Rect::from_min_size(min, vec2(width, height));
            (rect, geometry::edge_radius(rect, rects.screen))
        }
    };
    let sheet = rects.sheet.is_some();
    let id = if sheet { "pad-sheet" } else { key };
    let backdrop = state.globe.backdrop();
    let inner = geometry::edge_content(rect, rects.screen, rects.bands);
    widgets::card(ctx, backdrop, id, rect, Order::Middle, radius, |ui| {
        let layout = egui::Layout::top_down(egui::Align::Min);
        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.spacing_mut().item_spacing.y = 0.0;
        if sheet {
            let grabber =
                Rect::from_center_size(pos2(rect.center().x, rect.top() + 8.5), vec2(36.0, 5.0));
            ui.painter()
                .rect_filled(grabber, 3.0, theme::MUTE.gamma_multiply(0.6));
        }
        let row = widgets::title_row_with(ui, controls, head.title, head.back, sheet, head.action);
        on_title(state, &row);
        let low = sheet && state.pad.sheet_offset > -rects.sheet_rise + 0.5;
        ctx.data_mut(|d| d.insert_temp(low_id(), low));
        if sheet {
            if take_raise(ctx) {
                state.pad.sheet_offset = -rects.sheet_rise;
            }
            sheet_drag(ui, &mut state.pad, &row.bar, rects);
            if state.pad.sheet_offset >= rects.sheet_travel {
                return;
            }
        }
        let start = ui.cursor().top();
        let margin = egui::Margin::symmetric(BODY_PAD as i8, 0);
        egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            top(ui, state, controls);
        });
        let fixed = ui.cursor().top() - start;
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
                    body(ui, state, controls);
                });
            });
        state
            .pad
            .heights
            .insert(key, ROW + fixed + out.content_size.y);
    });
    rect
}

fn raise_id() -> egui::Id {
    egui::Id::new("pad-raise-sheet")
}

/// The panel is a sheet below its full height.
fn low_id() -> egui::Id {
    egui::Id::new("pad-sheet-low")
}

/// Raises the sheet to its full height in the next frame, for a field that
/// is under the keyboard. Returns true if the sheet was below its full
/// height.
pub fn raise_sheet(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.insert_temp(raise_id(), true));
    ctx.request_repaint();
    ctx.data(|d| d.get_temp(low_id())).unwrap_or(false)
}

fn take_raise(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(raise_id()))
        .unwrap_or(false)
}

/// Moves the sheet with a drag on its title row. At the end of the drag, the
/// sheet goes to the nearer of its two places. A tap changes the place.
fn sheet_drag(ui: &egui::Ui, pad: &mut PadState, bar: &egui::Response, rects: &Rects) {
    let (rise, travel) = (rects.sheet_rise, rects.sheet_travel);
    if bar.dragged() {
        pad.sheet_offset = (pad.sheet_offset + bar.drag_delta().y).clamp(-rise, travel);
        ui.ctx().request_repaint();
    }
    // The places of the sheet: the full height, the half height, and the
    // title row.
    let places = [-rise, 0.0, travel];
    let offset = pad.sheet_offset;
    pad.sheet_offset = if bar.drag_stopped() {
        let speed = ui.input(|i| i.pointer.velocity().y);
        if speed > 300.0 {
            places.into_iter().find(|p| *p > offset).unwrap_or(travel)
        } else if speed < -300.0 {
            places
                .into_iter()
                .rev()
                .find(|p| *p < offset)
                .unwrap_or(-rise)
        } else {
            let near = |p: &f32| (p - offset).abs();
            places
                .into_iter()
                .min_by(|a, b| near(a).total_cmp(&near(b)))
                .unwrap_or(0.0)
        }
    } else if bar.clicked() {
        if offset >= travel { 0.0 } else { travel }
    } else {
        return;
    };
}

fn brush_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let measuring = state.measuring();
    let line = state.globe.drawing();
    let stroke = state.globe.stroking();
    let vertexing = state.globe.vertexing();
    if stroke || measuring || state.globe.placing() || vertexing {
        // Smooth vertices smooths as the line tool does.
        if stroke || vertexing {
            let smoothing = &mut state.globe.line.smoothing;
            let number = percent("Line smoothing", *smoothing)
                .label(SMOOTHING)
                .default(DEFAULT_SMOOTHING);
            apply(number.slider(ui, controls), smoothing);
        }
        // The Measure tool in a map frame does not snap to color stops.
        let rasters = match state.workspace {
            Workspace::Atlas => Vec::new(),
            Workspace::Globe => panels::raster_layers(state),
        };
        let snap = &mut state.globe.snap;
        widgets::switch_row(ui, controls, SNAPPING, SNAPPING, &mut snap.on);
        widgets::switch_row(
            ui,
            controls,
            SNAP_VERTICES,
            SNAP_VERTICES,
            &mut snap.vertices,
        );
        widgets::switch_row(ui, controls, SNAP_EDGES, SNAP_EDGES, &mut snap.edges);
        for (id, name) in rasters {
            let mut on = snap.stops(id);
            let (control, label) = (panels::snap_stops_name(&name), format!("Stops of {name}"));
            if widgets::switch_row(ui, controls, &control, &label, &mut on) {
                snap.set_stops(id, on);
            }
        }
        let number = Number::new(SNAP_DISTANCE, snap.distance, snap::DISTANCE_RANGE)
            .label("Distance")
            .step(1.0)
            .unit("pt")
            .default(snap::DEFAULT_DISTANCE);
        apply(number.slider(ui, controls), &mut snap.distance);
        if line {
            let on = &mut state.globe.line.reshape;
            widgets::switch_row(ui, controls, RESHAPE_SWITCH, RESHAPE, on);
        }
        if state.globe.lining() {
            let terrain = state.globe.fit_layer.is_some();
            let line = &mut state.globe.line;
            let (name, label) = (TERRAIN_FIT_SWITCH, TERRAIN_FIT);
            widgets::switch_row_enabled(ui, controls, name, label, &mut line.fit, terrain);
            let number = percent(TERRAIN_BALANCE, line.terrain)
                .label("Balance")
                .default(LineSettings::default().terrain)
                .enabled(terrain && line.fit);
            apply(number.slider(ui, controls), &mut line.terrain);
        }
        return;
    }
    let unit = state.shown_unit().to_string();
    if state.globe.brush.mode == Mode::OceanRidge {
        ridge_hint_rows(ui, &state.globe);
    }
    // The outline sets the size of a basin.
    if state.globe.brush.mode == Mode::Basin {
        basin_rows(ui, &state.globe);
    } else {
        brush_size(state, "Brush size", |n| n.slider(ui, controls));
    }
    let world_km = state.doc.project.world.radius_km;
    let radius_km = state.globe.brush.radius_km(state.globe.radius(), world_km);
    let height = state.globe.is_height();
    let defaults = BrushSettings::default();
    let brush = &mut state.globe.brush;
    let world = &mut state.doc.project.world;
    // A height in meters as a value of the layer.
    let factor = brush.meters_value(1.0);
    let span = brush.value_range.1 - brush.value_range.0;
    let layer_text = |meters: f64| world::value_text(meters * factor, span, &unit);
    let meters = |name: &'static str, label: &'static str, value, range| {
        Number::new(name, value, range)
            .label(label)
            .shown(factor, &unit)
            .text(layer_text)
    };
    let km = |name: &'static str, label: &'static str, value, range| {
        Number::new(name, value, range).label(label).unit("km")
    };
    // The ocean ridge mode has no falloff and no strength, the range mode
    // has no falloff, and the basin mode has no falloff and no flow.
    let ridge = brush.mode == Mode::OceanRidge;
    if !matches!(brush.mode, Mode::OceanRidge | Mode::Range | Mode::Basin) {
        let number = percent("Brush hardness", brush.hardness)
            .label("Hardness")
            .default(defaults.hardness);
        apply(number.slider(ui, controls), &mut brush.hardness);
    }
    if brush.mode != Mode::Basin {
        let number = Number::new("Brush flow", brush.flow, FLOW)
            .label("Flow")
            .percent();
        apply(number.slider(ui, controls), &mut brush.flow);
    }
    if ridge {
        ridge_rows(ui, controls, brush, radius_km);
    } else if brush.mode == Mode::Warp {
        let number = km("Brush strength", "Strength", brush.warp_km, WARP_KM)
            .log()
            .default(defaults.warp_km);
        apply(number.slider(ui, controls), &mut brush.warp_km);
        let number = km("Brush scale", "Scale", brush.scale_km, SCALE_KM)
            .log()
            .default(defaults.scale_km);
        apply(number.slider(ui, controls), &mut brush.scale_km);
        seed_stepper(ui, controls, &mut world.seed);
    } else if brush.mode == Mode::Push {
        // The motion of the pen and the flow set the distance.
    } else if brush.mode == Mode::Noise {
        let number = meters("Brush raise", "Raise", brush.noise_raise_m, NOISE_M)
            .scale(Scale::Square)
            .default(defaults.noise_raise_m);
        apply(number.slider(ui, controls), &mut brush.noise_raise_m);
        let number = meters("Brush lower", "Lower", brush.noise_lower_m, NOISE_M)
            .scale(Scale::Square)
            .default(defaults.noise_lower_m);
        apply(number.slider(ui, controls), &mut brush.noise_lower_m);
    } else if brush.mode == Mode::Terrace {
        let value = brush.terrace_step_m;
        let number = meters("Brush step", "Step", value, TERRACE_STEP_M)
            .log()
            .default(defaults.terrace_step_m);
        apply(number.slider(ui, controls), &mut brush.terrace_step_m);
    } else if brush.mode == Mode::Shape {
        let number = Number::new("Brush shape", brush.shape, SHAPE)
            .label("Shape")
            .text(shape_text)
            .default(defaults.shape);
        apply(number.slider(ui, controls), &mut brush.shape);
    } else {
        let name = panels::strength_name(brush.mode);
        let number = meters("Brush strength", name, brush.strength_m, STRENGTH_M)
            .log()
            .default(defaults.strength_m);
        apply(number.slider(ui, controls), &mut brush.strength_m);
    }
    if brush.mode == Mode::Noise {
        let number = km("Brush scale", "Scale", brush.scale_km, SCALE_KM)
            .log()
            .default(defaults.scale_km);
        apply(number.slider(ui, controls), &mut brush.scale_km);
        let number = Number::new("Brush grain", brush.noise_grain, NOISE_GRAIN)
            .label("Grain")
            .text(|grain| format!("{:.1}×", 1.0 + grain));
        apply(number.slider(ui, controls), &mut brush.noise_grain);
        let ridged = &mut brush.noise_ridged;
        widgets::switch_row(ui, controls, "Brush ridged", "Ridged", ridged);
        if brush.noise_ridged {
            let inverted = &mut brush.noise_inverted;
            widgets::switch_row(ui, controls, "Brush invert", "Invert", inverted);
        }
        // The relief follows the height above sea level.
        if height {
            let number = percent("Brush relief", brush.noise_relief)
                .label("Relief")
                .default(defaults.noise_relief);
            apply(number.slider(ui, controls), &mut brush.noise_relief);
        }
        seed_stepper(ui, controls, &mut world.seed);
    }
    if brush.mode == Mode::Range {
        let value = brush.range_spacing_km;
        let number = km("Brush spacing", "Spacing", value, RANGE_SPACING_KM)
            .log()
            .default(defaults.range_spacing_km);
        apply(number.slider(ui, controls), &mut brush.range_spacing_km);
        seed_stepper(ui, controls, &mut world.seed);
    }
    if brush.mode == Mode::Basin {
        let value = brush.basin_density_km;
        let number = km("Brush density", "Density", value, BASIN_DENSITY)
            .log()
            .default(defaults.basin_density_km);
        apply(number.slider(ui, controls), &mut brush.basin_density_km);
        seed_stepper(ui, controls, &mut world.seed);
    }
    if matches!(brush.mode, Mode::Carve | Mode::Erode | Mode::Basin) {
        let name = "Brush channel width";
        let number = km(name, "Channel width", brush.channel_km, CHANNEL_KM).nice();
        apply(number.stepper(ui, controls), &mut brush.channel_km);
    }
    if brush.mode == Mode::Carve {
        let value = brush.floodplain_km;
        let number = km("Brush floodplain", "Floodplain", value, FLOODPLAIN_WIDTH_KM).step(1.0);
        apply(number.stepper(ui, controls), &mut brush.floodplain_km);
    }
    if matches!(brush.mode, Mode::Uplift | Mode::Range) {
        let number = Number::new("Brush erosion", brush.erosion, EROSION)
            .label("Erosion")
            .percent()
            .default(defaults.erosion);
        apply(number.slider(ui, controls), &mut brush.erosion);
    }
    if matches!(
        brush.mode,
        Mode::Erode | Mode::Uplift | Mode::Range | Mode::Basin
    ) {
        let mut degrees = world.wind_deg as f32;
        if widgets::compass_row(ui, controls, "Brush wind", "Wind", &mut degrees) {
            world.wind_deg = f64::from(degrees.round());
        }
        let shadow = world.rain_shadow.clamp(0.0, 1.0);
        let number = percent("Brush rain shadow", shadow).label("Rain shadow");
        apply(number.slider(ui, controls), &mut world.rain_shadow);
    }
    if matches!(brush.mode, Mode::Weather | Mode::Uplift) {
        let number = Number::new("Brush angle", brush.talus_deg, TALUS_DEG)
            .label("Angle")
            .unit("°")
            .nice();
        apply(number.stepper(ui, controls), &mut brush.talus_deg);
    }
    let range = brush.value_range;
    for (label, name, on, value) in [
        (
            "Floor",
            "Brush floor",
            &mut brush.floor_on,
            &mut brush.floor_m,
        ),
        (
            "Ceiling",
            "Brush ceiling",
            &mut brush.ceiling_on,
            &mut brush.ceiling_m,
        ),
    ] {
        widgets::switch_row(ui, controls, name, label, on);
        if *on {
            let name = format!("{name} level");
            let number = level(&name, *value, range, &unit).label("Level");
            apply(number.stepper(ui, controls), value);
        }
    }
    flatten_rows(ui, controls, &mut state.globe, &unit);
}

/// Sets `value` to a new value of a control.
fn apply(new: Option<f64>, value: &mut f64) {
    if let Some(new) = new {
        *value = new;
    }
}

/// The number of steps of a level of the brush. A height has steps of 50
/// meters.
const LIMIT_STEPS: f64 = 240.0;

/// A level of the layer that the brush paints, in the unit of the layer.
/// `range` has the values of the lowest and of the highest level.
fn level<'a>(name: &'a str, value: f64, (min, max): (f64, f64), unit: &'a str) -> Number<'a> {
    let span = max - min;
    Number::new(name, value.clamp(min, max), min..=max)
        .step(span / LIMIT_STEPS)
        .unit(unit)
        .text(move |value| world::value_text(value, span, unit))
}

/// The seed of the warp mode, the noise mode, the range mode, and the basin
/// mode. The seed is a property of the world, so each brush gets the same
/// pattern.
fn seed_stepper(ui: &mut egui::Ui, controls: &mut Controls, seed: &mut u32) {
    let range = f64::from(*SEED.start())..=f64::from(*SEED.end());
    let number = Number::new("Brush seed", f64::from(*seed), range)
        .label("Seed")
        .step(1.0);
    if let Some(value) = number.stepper(ui, controls) {
        *seed = value as u32;
    }
}

/// The settings of the ocean ridge mode, and the age of the crust at the
/// edge of the stroke. `radius_km` is the brush radius on the ground.
fn ridge_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    brush: &mut BrushSettings,
    radius_km: f64,
) {
    let number = Number::new("Brush crest", brush.ridge_crest_m, RIDGE_CREST_M)
        .label("Crest")
        .step(50.0)
        .unit("m");
    apply(number.stepper(ui, controls), &mut brush.ridge_crest_m);
    let number = Number::new("Brush spreading", brush.ridge_spread_mm, RIDGE_SPREAD_MM)
        .label("Spreading")
        .step(1.0)
        .unit("mm/yr");
    apply(number.stepper(ui, controls), &mut brush.ridge_spread_mm);
    let number = Number::new("Brush magma", brush.ridge_magma, RIDGE_MAGMA)
        .label("Magma")
        .percent()
        .text(panels::signed_percent)
        .default(0.0);
    apply(number.slider(ui, controls), &mut brush.ridge_magma);
    let number = Number::new("Brush segment", brush.ridge_segment_km, RIDGE_SEGMENT_KM)
        .label("Segment")
        .step(10.0)
        .unit("km");
    apply(number.stepper(ui, controls), &mut brush.ridge_segment_km);
    let number = Number::new("Brush offset", brush.ridge_offset_km, RIDGE_OFFSET_KM)
        .label("Offset")
        .step(1.0)
        .unit("km");
    apply(number.stepper(ui, controls), &mut brush.ridge_offset_km);
    raster::note(ui, &panels::edge_age_text(brush, radius_km));
}

/// The name of the switch that keeps the brush size on the ground.
pub const LOCK_SIZE: &str = "Lock size";
/// The name of the stepper of the level of the flatten mode.
pub const FLATTEN_LEVEL: &str = "Flatten level";
/// The name of the button that sets the flatten level at each stroke start.
pub const STROKE_START: &str = "Use stroke start";
/// The name of the switch that takes the flatten level from the globe.
pub const PICK_LEVEL: &str = "Pick from globe";

/// The lock of the size, and the level of the flatten mode.
fn flatten_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    globe: &mut crate::globe::Globe,
    unit: &str,
) {
    let globe_radius = globe.radius();
    let mut lock = globe.brush.lock.is_some();
    if widgets::switch_row(ui, controls, LOCK_SIZE, LOCK_SIZE, &mut lock) {
        globe.brush.set_lock(lock, globe_radius);
    }
    if globe.brush.mode != Mode::Flatten {
        globe.pick_level = false;
        return;
    }
    let brush = &mut globe.brush;
    match brush.flatten_level {
        Some(at) => {
            let number = level(FLATTEN_LEVEL, brush.value(at), brush.value_range, unit);
            if let Some(value) = number.stepper(ui, controls) {
                brush.flatten_level = Some(brush.level(value));
            }
            if widgets::button_row(ui, controls, STROKE_START, true).clicked() {
                brush.flatten_level = None;
            }
        }
        None => raster::note(ui, "Level at stroke start"),
    }
    let pick = &mut globe.pick_level;
    widgets::switch_row(ui, controls, PICK_LEVEL, PICK_LEVEL, pick);
}

/// The hint of the basin mode.
fn basin_rows(ui: &mut egui::Ui, globe: &crate::globe::Globe) {
    ui.add_space(8.0);
    let text = egui::RichText::new(panels::basin_hint(globe)).color(theme::MUTE);
    ui.add(egui::Label::new(text).selectable(false));
    ui.add_space(8.0);
}

/// The hint of a held ocean ridge. The panel of the ridge has no room for a
/// hint before the stroke on the smallest iPad.
fn ridge_hint_rows(ui: &mut egui::Ui, globe: &crate::globe::Globe) {
    if globe.holding().is_none() {
        return;
    }
    ui.add_space(8.0);
    let text = egui::RichText::new(panels::HELD_HINT).color(theme::MUTE);
    ui.add(egui::Label::new(text).selectable(false));
    ui.add_space(8.0);
}

/// The text of the Layers panel of a world with no layer.
pub const NO_LAYERS: &str = "No layers yet.";

/// The icon of the kind of a layer.
pub(super) fn layer_icon(layer: &Layer) -> Icon {
    match layer.kind {
        LayerKind::Raster(_) | LayerKind::Image(_) => Icon::Raster,
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
/// moves with the finger to a new place in the order. A swipe to the left
/// shows the Delete button of the row, and a long swipe deletes the layer.
/// The list of the layers, and below it the text of the import of an image
/// layer that runs or that failed, or of a GeoJSON import that failed.
fn layers_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    layer_list(ui, state, controls);
    image_note(ui, state, None);
}

/// Shows the text of the import of an image layer. `layer` is the image
/// layer of the panel, or `None` for the list of the layers, which shows the
/// progress and a failed import.
fn image_note(ui: &mut egui::Ui, state: &mut AppState, layer: Option<ImageId>) {
    let progress = layer.is_none().then(|| state.image_progress()).flatten();
    let ours = state
        .image_note
        .as_ref()
        .is_some_and(|(_, of)| *of == layer);
    let mut note = match state.image_note.take() {
        Some((note, _)) if ours => Some(note),
        other => {
            state.image_note = other;
            None
        }
    };
    note_text(ui, progress, &mut note, None);
    if ours {
        state.image_note = note.map(|note| (note, layer));
    }
}

fn layer_list(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
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
        state.pad.swipe = None;
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
        swipe_rows(ui, state, controls, &rows, area);
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
        widgets::layer_row(
            ui, controls, rect, icon, name, visible, on, false, false, 0.0, None,
        );
    }
    let row = &rows[from];
    let (icon, name, visible) = (row.icon, &row.name, Some(row.visible));
    let rect = slot(top);
    widgets::layer_row(
        ui, controls, rect, icon, name, visible, true, false, true, 0.0, None,
    );
    if !down {
        state.pad.reorder = None;
        if to != from {
            state.move_layer(reorder.layer, to);
        }
    }
}

/// The rows of the layer list that take touch. A sideways drag on a row
/// swipes it, and a drag up or down scrolls the list.
fn swipe_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rows: &[ListRow],
    area: Rect,
) {
    let place = |i: usize| {
        let top = area.top() + i as f32 * PANEL_ROW;
        Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW))
    };
    let selected = state.selected_layer;
    swipe::shut_outside(ui, &mut state.pad.swipe, |layer| {
        rows.iter().position(|row| row.id == layer).map(place)
    });
    for (i, row) in rows.iter().enumerate() {
        let (id, rect) = (row.id, place(i));
        if let Some((_, text)) = state.pad.rename.as_mut().filter(|r| r.0 == id) {
            // The row is the name field while the user renames the layer. The
            // name changes when the field loses the focus.
            let valid = !text.trim().is_empty();
            let field = ui
                .push_id(id.0, |ui| {
                    let font = theme::body();
                    widgets::field_at(ui, controls, rect, LAYER_NAME, None, text, valid, font)
                })
                .inner;
            if field.lost_focus() {
                let text = state.pad.rename.take().map(|r| r.1);
                state.doc.rename_layer(id, &text.unwrap_or_default());
            } else if !field.has_focus() {
                widgets::focus_all(ui, &field, &row.name);
            }
            continue;
        }
        let on = selected == Some(row.layer);
        let (icon, name, visible) = (row.icon, &row.name, Some(row.visible));
        let open = state.pad.swipe.filter(|s| s.row == id);
        let shift = swipe::shift(ui, state.pad.swipe, id);
        let out = ui
            .push_id(id.0, |ui| {
                widgets::layer_row(
                    ui, controls, rect, icon, name, visible, on, true, false, shift, None,
                )
            })
            .inner;
        if out.delete {
            state.pad.swipe = None;
            state.remove_layer(id);
            continue;
        }
        if out.rename {
            state.pad.swipe = None;
            state.pad.rename = Some((id, row.name.clone()));
            continue;
        }
        if out.eye {
            state.set_layer_visible(id, !row.visible);
        }
        if out.open {
            state.select(Some(row.layer));
            state.pad.layer = Some(row.layer);
            state.pad.swipe = None;
        }
        let Some(body) = out.body else { continue };
        if swipe::drag(ui, &mut state.pad.swipe, id, &body, widgets::RENAME_WIDTH) {
            continue;
        }
        if body.dragged() {
            ui.scroll_with_delta(vec2(0.0, body.drag_delta().y));
        }
        if body.clicked() {
            if open.is_some() {
                state.pad.swipe = None;
            } else {
                state.select(Some(row.layer));
            }
        }
        if !body.is_pointer_button_down_on() || body.dragged() || state.pad.swipe.is_some() {
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
    let rows = 3 + NEW_LAYERS.len() + usize::from(import);
    let height = 10.0 + 2.0 * MENU_HEADING + rows as f32 * PANEL_ROW + 10.0;
    let size = vec2(ADD_WIDTH, height);
    let pane = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let beside = pos2(
        rects.dock.left() - geometry::PANEL_GAP - size.x,
        pane.map_or(rects.dock.top(), |(_, rect)| rect.top()),
    );
    let min = if rects.sheet.is_none() && beside.x >= rects.screen.left() + geometry::MARGIN {
        beside
    } else {
        let left = (button.right() - size.x).max(rects.screen.left() + geometry::PANEL_GAP);
        // A short screen moves the menu up, out of the bottom band.
        let top = (rects.panel_bottom - size.y).min(button.bottom() + PAD);
        pos2(left, top.max(rects.screen.top() + rects.bands.top))
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
            menu_heading(ui, NEW_SECTION);
            for (name, kind) in NEW_LAYERS {
                if widgets::text_row(ui, controls, name, 0.0, false, true).clicked() {
                    let id = state.doc.add_layer(kind);
                    state.select(Some(LayerRef::Vector(id)));
                    state.pad.layer = Some(LayerRef::Vector(id));
                    state.pad.add = false;
                    state.touch();
                }
            }
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
            menu_heading(ui, IMPORT_SECTION);
            let free = state.image_import.is_none();
            if widgets::text_row(ui, controls, NEW_IMAGE, 0.0, false, free).clicked() {
                state.actions.push(Action::ImageLayerDialog);
                state.pad.add = false;
            }
            if widgets::text_row(ui, controls, NEW_GEOJSON, 0.0, false, true).clicked() {
                state.actions.push(Action::GeoJsonDialog);
                state.pad.add = false;
            }
            if import {
                let enabled = !state.globe.busy();
                let row = widgets::text_row(ui, controls, NEW_HEIGHTMAP, 0.0, false, enabled);
                if row.clicked() {
                    state.actions.push(Action::ImportHeightmapDialog);
                    state.pad.add = false;
                }
            }
        },
    );
}

/// The heading of a section of the menu of the new layers.
fn menu_heading(ui: &mut egui::Ui, text: &str) {
    let size = vec2(ui.available_width(), MENU_HEADING);
    let layout = egui::Layout::left_to_right(egui::Align::Max);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_size(size);
        let text = egui::RichText::new(text)
            .font(theme::small())
            .color(theme::MUTE);
        ui.label(text);
    });
}

/// The panel of a vector layer: its name, its kind, the number of its
/// features, its style, and its fields.
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
    if widgets::open_row(ui, controls, table::FEATURES, &features).clicked() {
        state.open_table(id);
    }
    rules::section(ui, state, controls, id);
    if kind != "Polygons" {
        fonts::section(ui, state, controls, id);
    }
    fields::list(ui, state, controls, id);
}

/// A note with no warning goes away after this time, in seconds.
pub const NOTE_TIME: f64 = 6.0;

/// Shows the text of the import or the export that runs, or of the last one
/// for the raster layer `layer` of the panel. A note with no warning goes
/// away after `NOTE_TIME`. `expire_notes` removes a note after its panel
/// closes.
fn note_text(
    ui: &mut egui::Ui,
    progress: Option<String>,
    note: &mut Option<Note>,
    layer: Option<RasterId>,
) {
    let ours = |n: &Note| n.layer.is_none() || n.layer == layer;
    if progress.is_none()
        && let Some(n) = note.as_mut().filter(|n| ours(n))
    {
        let (time, pass) = (ui.input(|i| i.time), ui.ctx().cumulative_pass_nr());
        let (start, _) = *n.shown.get_or_insert((time, pass));
        n.last_pass = pass;
        if !n.warning {
            let left = NOTE_TIME - (time - start);
            if left <= 0.0 {
                *note = None;
                return;
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }
    let (text, color) = match (progress, note.as_ref().filter(|n| ours(n))) {
        (Some(progress), _) => (progress, theme::MUTE),
        (None, Some(note)) if note.warning => (note.text.clone(), theme::WARN),
        (None, Some(note)) => (note.text.clone(), theme::MUTE),
        (None, None) => return,
    };
    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
}

/// Removes each note that a panel showed and that no panel showed in the
/// last pass, because the user left its panel.
fn expire_notes(ctx: &egui::Context, state: &mut AppState) {
    let pass = ctx.cumulative_pass_nr();
    let gone = |n: &Note| n.shown.is_some() && n.last_pass < pass;
    for note in [
        &mut state.import_note,
        &mut state.export_note,
        &mut state.ramp_note,
        &mut state.symbol_note,
    ] {
        if note.as_ref().is_some_and(gone) {
            *note = None;
        }
    }
    if state.image_note.as_ref().is_some_and(|(n, _)| gone(n)) {
        state.image_note = None;
    }
}

/// The row that imports a heightmap, and the text of the import below it.
/// `layer` is the raster layer of the panel.
fn import_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: Option<RasterId>,
) {
    let enabled = !state.globe.busy();
    if widgets::text_row(ui, controls, IMPORT_HEIGHTMAP, 0.0, false, enabled).clicked() {
        state.actions.push(Action::ImportHeightmapDialog);
    }
    let progress = state.globe.import_progress();
    note_text(ui, progress, &mut state.import_note, layer);
}

/// The row that exports the raster layer `layer` of the panel, and the text
/// of the export below it.
fn export_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, layer: RasterId) {
    let enabled = state.can_export();
    if widgets::text_row(ui, controls, EXPORT_GEOTIFF, 0.0, false, enabled).clicked() {
        state.actions.push(Action::ExportHeightmapDialog);
    }
    if widgets::text_row(ui, controls, EXPORT_PNG, 0.0, false, enabled).clicked() {
        state.actions.push(Action::ExportHeightmapPngDialog);
    }
    let progress = state.globe.export_progress();
    note_text(ui, progress, &mut state.export_note, Some(layer));
}

/// The panel of a raster layer: its name, the unit of a layer that is not a
/// height layer, its value range, the size of its pixels on the ground, its
/// opacity, the import into a height layer, the export, and the Style
/// section.
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
    let pixel_size = raster_form::pixel_size_text(raster.face_size, &state.doc.project.world);
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
    widgets::value_row(ui, "Pixel size", &pixel_size);
    let number = percent(OPACITY, f64::from(opacity)).label("Opacity");
    if let Some(opacity) = number.slider(ui, controls) {
        state.set_raster_opacity(id, opacity as f32);
    }
    if height {
        import_row(ui, state, controls, Some(id));
    }
    export_row(ui, state, controls, id);
    raster::style(ui, state, controls, id);
}

/// The panel of an image layer: its name, its size, its opacity, its blend
/// mode, and the text of its import.
fn image_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: ImageId) {
    let Some(image) = state.doc.project.image(id) else {
        return;
    };
    let (opacity, blend) = (image.opacity, image.blend);
    let size = format!("{} × {} px", image.width, image.height);
    let layer = state.doc.project.image_layer(id);
    let Some((layer, name)) = layer.map(|layer| (layer.id, layer.name.clone())) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;

    let of = LayerRef::Image(id);
    let mut text = match state.pad.name.take() {
        Some((layer, text)) if layer == of => text,
        _ => name.clone(),
    };
    let valid = !text.trim().is_empty();
    let field = widgets::text_field(ui, controls, LAYER_NAME, "Name", None, &mut text, valid);
    if field.changed() {
        state.doc.rename_layer(layer, &text);
    } else if !field.has_focus() {
        // The field shows the name again after an undo and after a blank text.
        text = name;
    }
    state.pad.name = Some((of, text));
    widgets::value_row(ui, "Size", &size);
    let number = percent(OPACITY, f64::from(opacity)).label("Opacity");
    if let Some(opacity) = number.slider(ui, controls) {
        state.set_image_opacity(id, opacity as f32);
    }
    let label = super::elevation::blend_label(blend);
    if widgets::open_row(ui, controls, raster::BLEND, label).clicked() {
        state.pad.page = Some(Page::Blend);
    }
    image_note(ui, state, Some(id));
}

fn toolbox_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("No tools yet.").color(theme::MUTE));
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
    let text = readout_line(state);
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::mono(), theme::MUTE);
    let mark = state.save_mark(std::time::Instant::now());
    let lead = if mark.is_some() { SAVE_MARK + 8.0 } else { 0.0 };
    let size = vec2(lead + galley.size().x.ceil() + 24.0, band.height());
    let rect = if rects.class == WidthClass::Compact && !rects.row {
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

/// The width of a button of the card of a held stroke, and the least width
/// of its state.
const HELD_BUTTON: f32 = 120.0;
const HELD_STATE: f32 = 132.0;

/// What the card of a basin or an ocean ridge shows.
#[derive(Clone, Copy, PartialEq)]
enum HeldCard {
    /// The outline of the next basin waits for its main river.
    Outline,
    /// A stroke is held, and the flag is true while it follows the settings.
    Held(bool),
}

/// The card of a held ocean ridge or basin, at the bottom of the canvas
/// above the readout: its state, and the Discard and Apply buttons. The
/// outline of the next basin shows the hint of its main river and the
/// Clear button. An open panel pushes the card to its right.
fn held_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let globe = &state.globe;
    let outline = globe.brushing() && globe.brush.mode == Mode::Basin;
    let card = match globe.holding() {
        Some(_) => HeldCard::Held(globe.held_updating()),
        None if outline && globe.basin_outline().is_some() => HeldCard::Outline,
        None => return,
    };
    let (text, buttons): (&str, &[(&str, egui::Color32)]) = match card {
        HeldCard::Outline => (
            panels::BASIN_RIVER_HINT,
            &[(panels::CLEAR_OUTLINE, theme::WARN)],
        ),
        HeldCard::Held(true) => (
            panels::UPDATING,
            &[
                (panels::DISCARD, theme::WARN),
                (panels::APPLY, theme::ACCENT),
            ],
        ),
        HeldCard::Held(false) => (
            panels::READY,
            &[
                (panels::DISCARD, theme::WARN),
                (panels::APPLY, theme::ACCENT),
            ],
        ),
    };
    let spinner = card == HeldCard::Held(true);
    let lead = if spinner { 24.0 } else { 0.0 };
    let text_width = ctx.fonts_mut(|fonts| {
        let galley = fonts.layout_no_wrap(text.to_string(), theme::body(), theme::MUTE);
        galley.size().x
    });
    let slot = HELD_STATE.max(lead + text_width.ceil() + 2.0 * BODY_PAD);
    let count = buttons.len() as f32;
    let size = vec2(
        slot + count * HELD_BUTTON + (count + 1.0) * PAD,
        PANEL_ROW + 2.0 * PAD,
    );
    let mut bottom = rects.screen.bottom() - rects.bands.bottom - geometry::GAP;
    for top in [rects.readout, rects.sheet]
        .into_iter()
        .flatten()
        .map(|r| r.top())
    {
        bottom = bottom.min(top - geometry::GAP);
    }
    let middle = rects.screen.center().x;
    let mut rect = Rect::from_min_size(pos2(middle - size.x / 2.0, bottom - size.y), size);
    let panels = &state.pad.panels;
    if let Some((_, panel)) = panels.iter().find(|(_, panel)| panel.intersects(rect)) {
        rect = rect.translate(vec2(panel.right() + geometry::GAP - rect.left(), 0.0));
    }
    let mut clicked = None;
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-held",
        rect,
        Order::Middle,
        theme::CARD_RADIUS.into(),
        |ui| {
            let middle = rect.center().y;
            let left = rect.left() + BODY_PAD;
            // A spinner shows while the stroke follows the settings.
            if spinner {
                let spin = Rect::from_center_size(pos2(left + 8.0, middle), vec2(16.0, 16.0));
                ui.put(spin, egui::Spinner::new().size(16.0).color(theme::MUTE));
            }
            ui.painter().text(
                pos2(left + lead, middle),
                egui::Align2::LEFT_CENTER,
                text,
                theme::body(),
                theme::MUTE,
            );
            for (i, &(name, fill)) in buttons.iter().enumerate() {
                let x = rect.left() + slot + PAD + i as f32 * (HELD_BUTTON + PAD);
                let place =
                    Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(HELD_BUTTON, PANEL_ROW));
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(place));
                if widgets::button_row_in(&mut ui, controls, name, true, fill).clicked() {
                    clicked = Some(name);
                }
            }
        },
    );
    let globe = &mut state.globe;
    match clicked {
        Some(panels::CLEAR_OUTLINE) => globe.set_basin_outline(None),
        Some(panels::DISCARD) => globe.discard_held(),
        Some(panels::APPLY) => globe.apply_held(),
        _ => {}
    }
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
    let top = if rects.row {
        rects.column_top
    } else {
        rects.tools.top()
    };
    let mut rect = Rect::from_min_size(pos2(middle - size.x / 2.0, top), size);
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

/// The view options: a card below the view switch with the switch of the
/// graticule.
fn view_options(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(view) = rects.view else {
        return;
    };
    let size = vec2(geometry::MENU_WIDTH, 10.0 + PANEL_ROW + 10.0);
    let min = pos2(view.right() - size.x, view.bottom() + PAD);
    let rect = Rect::from_min_size(min, size);
    state.pad.view_options_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !view.contains(pos)) {
        state.pad.view_options = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-view-options",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            // The card covers other cards, so its fill is opaque.
            ui.painter()
                .rect_filled(rect, radius, theme::card(false).to_opaque());
            let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let preview = &mut state.globe.preview;
            widgets::switch_row(
                &mut ui,
                controls,
                GRATICULE,
                GRATICULE,
                &mut preview.graticule,
            );
        },
    );
}

fn menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let compact = rects.class == WidthClass::Compact;
    let flat = state.globe.world_view == WorldView::Flat;
    let rows = match (compact, flat) {
        (true, true) => 7.0,
        (true, false) => 6.0,
        (false, _) => 2.0,
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
                let preview = &mut state.globe.preview;
                widgets::switch_row(ui, controls, GRATICULE, GRATICULE, &mut preview.graticule);
            }
            rule(ui);
            if widgets::text_row(ui, controls, WORKSPACES[1], 0.0, false, true).clicked() {
                state.workspace = Workspace::Atlas;
                state.pad.menu = false;
            }
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

/// The flyout of the modes of a brush cell.
fn brush_flyout(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    group: ModeGroup,
) {
    let globe = &mut state.globe;
    let choices: Vec<Choice> = group
        .modes()
        .iter()
        .map(|&(name, icon, mode)| Choice {
            name,
            icon,
            enabled: globe.is_height() || !needs_height(mode),
        })
        .collect();
    let current = group.modes().iter().position(|m| m.2 == globe.brush.mode);
    let backdrop = globe.backdrop();
    let flyouts = &mut state.pad.flyouts;
    if let Some(i) = flyout::show(ctx, backdrop, controls, flyouts, rects, &choices, current) {
        globe.tool = Tool::Brush;
        globe.brush.mode = group.modes()[i].2;
    }
}

/// The flyout of the tools of the Vector cell.
fn vector_flyout(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let Some(kind) = vector_kind(&state.globe) else {
        return;
    };
    let tools = vector_tools(kind);
    let choices: Vec<Choice> = (tools.iter())
        .map(|t| Choice {
            name: t.name,
            icon: t.icon,
            enabled: true,
        })
        .collect();
    let globe = &mut state.globe;
    let current = tools.iter().position(|t| t.tool == globe.tool);
    let backdrop = globe.backdrop();
    let flyouts = &mut state.pad.flyouts;
    if let Some(i) = flyout::show(ctx, backdrop, controls, flyouts, rects, &choices, current) {
        globe.tool = tools[i].tool;
    }
}

/// The flyout of the modes of the polygon tool.
fn polygon_flyout(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let choices = POLYGON_MODES.map(|(name, icon, _)| Choice {
        name,
        icon,
        enabled: true,
    });
    let current = POLYGON_MODES
        .iter()
        .position(|m| m.2 == state.globe.line.polygon_mode);
    let backdrop = state.globe.backdrop();
    let mode = &mut state.globe.line.polygon_mode;
    let flyouts = &mut state.pad.flyouts;
    if let Some(i) = flyout::show(ctx, backdrop, controls, flyouts, rects, &choices, current) {
        *mode = POLYGON_MODES[i].2;
    }
}
