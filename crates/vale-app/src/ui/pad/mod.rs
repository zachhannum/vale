//! The iPad layout of the world workspace: a full-screen canvas with
//! floating cards.

use std::collections::HashMap;

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_sphere::LonLat;
use vale_store::{CollectionId, GeometryKind, ImageId, Layer, LayerId, LayerKind, MapId, RasterId};
use vale_terrain::Mode;

pub use super::LayerRef;
use super::atlas::{AtlasTool, FrameTab};
use super::export_form;
use super::raster_form::{self, RasterForm};
use super::select;
use super::world_form::{self, WorldForm};
use super::{Action, AppState, Note, SaveMark, Workspace, globe, is_height, panels};
use crate::globe::Tool;
use crate::globe::brush::{
    ABYSSAL_RATE_MM, BASIN_AGE, BASIN_DENSITY, BASIN_LAKES, BRANCHING, CHANNEL_KM, EROSION,
    FLATTEN_RATE, FLOODPLAIN_WIDTH_KM, FLOOR_AGE_MYR, FLOW, HILL_SCALE, HOTSPOT_HEIGHT,
    HOTSPOT_SPEED, MARGIN_SHELF_M, NOISE_GRAIN, NOISE_M, PLATEAU_HEIGHT, RANGE_SPACING_KM,
    RIDGE_CREST_M, RIDGE_HILLS, RIDGE_MAGMA, RIDGE_OFFSET_KM, RIDGE_SEGMENT_KM, RIDGE_SPREAD_MM,
    SCALE_KM, SEAMOUNT_DENSITY, SEAMOUNT_HEIGHT_M, SEED, SHAPE, STRENGTH_M, TALUS_DEG,
    TERRACE_STEP_M, WARP_KM, needs_height, shape_text,
};
use crate::globe::brush::{BrushSettings, brush_radius};
use crate::globe::combine::PolygonMode;
use crate::globe::line::{DEFAULT_SMOOTHING, LineSettings};
use crate::globe::snap;
use crate::globe::sweep;
use crate::session::{WorldFile, worlds_of};
use crate::strip::{self, Slider};
use crate::world;

pub mod atlas;
pub mod browser;
pub mod cases;
pub mod chip_popover;
pub mod classify;
pub mod collection;
pub mod collection_choice;
pub mod copy_style;
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
pub mod label_text;
pub mod labels;
pub mod list;
pub mod manage;
pub mod map_card;
pub mod number;
pub mod panels_menu;
pub mod panes;
pub mod point_card;
pub mod popover;
pub mod ramp_editor;
pub mod raster;
pub mod readout;
pub mod rules;
pub mod selectable;
pub mod stops;
pub mod table;
pub mod text_style;
pub mod toolbox;
pub mod value;
pub mod widgets;

pub use vale_shell::pad::{icons, motion, theme};

use flyout::{Choice, Flyout, Flyouts};
use geometry::{
    BrushCard, ColumnInput, Input, MEASURE_CELLS, PAD, ROW, Rects, SELECT_CELLS, TOOLS, TOUCH,
    VERTEX_CELLS, WidthClass,
};
use icons::Icon;
use number::{Number, Scale};
use widgets::{BODY_PAD, Controls, PANEL_ROW};

/// The tool that moves and zooms the view, in both workspaces.
pub const PAN: &str = "Pan";
/// The cell of the tool strip with the tools of the selected vector layer.
pub const VECTOR: &str = "Vector";
/// The tool that draws in a line layer.
pub const LINE: &str = "Line";
/// The tool that draws in a polygon layer.
pub const POLYGON: &str = "Polygon";
/// The tool that paints in a polygon layer with a round brush.
pub const POLYGON_BRUSH: &str = "Polygon brush";
/// The slider of the radius of the Polygon brush.
pub const POLYGON_BRUSH_SIZE: &str = "Size";
/// The slider of the radius of the Polygon brush in the Brush panel.
pub const POLYGON_BRUSH_PANEL_SIZE: &str = "Polygon brush size";
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
pub const VECTOR_TOOLS: [VectorTool; 6] = [
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
        tool: Tool::PolygonBrush,
        name: POLYGON_BRUSH,
        icon: Icon::PolygonBrush,
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
/// The switch that makes a reshape of a polygon also reshape the polygons
/// that share its border.
pub const SHARED_BORDERS_SWITCH: &str = "Shared borders switch";
/// The slider of the balance between the sketch and the terrain.
pub const TERRAIN_BALANCE: &str = "Terrain fit balance";
/// The cell of the tool strip that chooses the Measure tool.
pub const MEASURE: &str = "Measure";
/// The cell of the card of the Measure tool that clears the path.
pub const CLEAR_PATH: &str = "Clear path";
/// The switch of the sheet that turns all snapping on and off.
pub const SNAPPING: &str = "Snapping";
/// The slider of the snap distance, in points.
pub const SNAP_DISTANCE: &str = "Snap distance";
/// The cell of the tool strip with the modes that shape the height freely.
pub const SCULPT: &str = "Sculpt";
/// The cell of the tool strip with the modes that model the terrain. They
/// paint a height layer only.
pub const TERRAIN: &str = "Terrain";
/// The modes of the Sculpt cell.
pub const SCULPT_MODES: [(&str, Icon, Mode); 8] = [
    ("Raise", Icon::Raise, Mode::Raise),
    ("Lower", Icon::Lower, Mode::Lower),
    ("Smooth", Icon::Smooth, Mode::Smooth),
    ("Flatten", Icon::Flatten, Mode::Flatten),
    ("Paint", Icon::Paint, Mode::Paint),
    ("Warp", Icon::Warp, Mode::Warp),
    ("Push", Icon::Push, Mode::Push),
    ("Noise", Icon::Noise, Mode::Noise),
];
/// The modes of the Terrain cell.
pub const TERRAIN_MODES: [(&str, Icon, Mode); 16] = [
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
    (panels::SEAMOUNTS, Icon::Seamounts, Mode::Seamounts),
    (
        panels::ABYSSAL_HILLS,
        Icon::AbyssalHills,
        Mode::AbyssalHills,
    ),
    (panels::MARGIN, Icon::Margin, Mode::Margin),
    (panels::HOTSPOT, Icon::Hotspot, Mode::Hotspot),
    (panels::PLATEAU, Icon::Plateau, Mode::Plateau),
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
/// The width of the menu of the new layers while it shows the picker of
/// its Collection row.
const PICK_WIDTH: f32 = 320.0;
pub use list::{HOLD_SLOP, HOLD_TIME};

/// A panel that a button of the layout opens.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Panel {
    Brush,
    /// The snap settings, and the snap targets of the active layer.
    Snap,
    /// The layers of the world, or in the Atlas the layers of the map of
    /// the selected frame.
    Layers,
    Toolbox,
    Attributes,
    /// The settings of the selected map frame or item of the page.
    Properties,
    /// The projection of the map of the selected frame.
    Projection,
    /// The label classes of the map of the selected frame.
    Labels,
    /// The open page of the atlas.
    Pages,
}

impl Panel {
    fn key(self) -> &'static str {
        match self {
            Panel::Brush => "pad-brush",
            Panel::Snap => "pad-snap",
            Panel::Layers => "pad-layers",
            Panel::Toolbox => "pad-toolbox",
            Panel::Attributes => "pad-attributes",
            Panel::Properties => "pad-properties",
            Panel::Projection => "pad-projection",
            Panel::Labels => "pad-labels",
            Panel::Pages => "pad-pages",
        }
    }

    /// The tab of the panel of a frame of the desktop layout that holds the
    /// same rows.
    pub fn frame_tab(self) -> Option<FrameTab> {
        match self {
            Panel::Properties => Some(FrameTab::Frame),
            Panel::Projection => Some(FrameTab::Projection),
            Panel::Layers => Some(FrameTab::Layers),
            Panel::Labels => Some(FrameTab::Labels),
            _ => None,
        }
    }
}

impl From<FrameTab> for Panel {
    fn from(tab: FrameTab) -> Panel {
        match tab {
            FrameTab::Frame => Panel::Properties,
            FrameTab::Projection => Panel::Projection,
            FrameTab::Layers => Panel::Layers,
            FrameTab::Labels => Panel::Labels,
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
    /// The rules that the Style section selects.
    Rules,
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
    /// The editor of a new ramp, which `AppState::ramp_draft` holds.
    RampEditor,
    /// The mask of the style of the layer.
    Mask,
    /// The label text of the layer.
    LabelText,
    /// The height layer of the Flow lines of the layer, from the rule with
    /// this index, or from the selected rules.
    FlowHeight(Option<usize>),
    /// The dash pattern of the rule with this index, or of the selected rules.
    DashPattern(Option<usize>),
    /// The collections of the layer.
    Collections,
    /// The maps and the map styles that the globe style can copy from.
    CopyStyle,
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
    /// The dock in the last frame.
    pub dock_rect: Option<Rect>,
    /// The open menu of a tab.
    pub pane_menu: Option<dock_ui::PaneMenu>,
    /// The width of the dock at the start of a drag of the width handle,
    /// and the width that the drag asks for, which can be below the
    /// narrowest dock.
    pub width_drag: Option<(f32, f32)>,
    /// The selection of the Atlas in the last frame.
    pub atlas_selection: Option<crate::ui::atlas::AtlasSelection>,
    /// The tab of the frame of the Atlas in the last frame.
    pub atlas_tab: Option<FrameTab>,
    /// The page of the panel of the layer of `layer` that the Layers panel
    /// shows.
    pub page: Option<Page>,
    /// The page and the search of the Toolbox panel.
    pub toolbox: toolbox::ToolboxState,
    /// The texts of the open page of a field.
    field: Option<fields::Draft>,
    /// The layer that the Layers panel shows. `None`: the panel shows the
    /// list of the layers.
    pub layer: Option<LayerRef>,
    /// The menu of the new layers is open.
    pub add: bool,
    /// The choice of the Collection row of the menu of the new layers.
    pub add_collection: Option<crate::ui::collection_choice::CollectionChoice>,
    /// The menu of the new layers shows the picker of its Collection row.
    pub add_picking: bool,
    /// The picker of the Collection row of a form.
    pub collection_picker: collection_choice::Picker,
    /// The menu of the new layers in the last frame.
    pub add_rect: Option<Rect>,
    /// The button of this frame that opens the menu of the new layers.
    add_button: Option<Rect>,
    /// The cards of the checked collections in the Layers panel.
    pub cards: collection::Cards,
    /// The collection popover of the Layers panel is open.
    pub collections_menu: bool,
    /// The button of this frame that opens the collection popover.
    pub collections_button: Option<Rect>,
    /// The card of the collection menu in the last frame.
    pub collections_rect: Option<Rect>,
    /// The gestures of the list of the collections in the collection menu.
    pub collection_list: list::ListState<CollectionId>,
    /// The Maps side of the collection menu shows.
    pub maps_side: bool,
    /// The gestures of the list of the maps in the collection menu.
    pub map_list: list::ListState<MapId>,
    /// The gestures of the list of the Collections page of a layer.
    pub layer_collections: list::ListState<CollectionId>,
    /// The text of the search field of the Collections page of a layer.
    pub collection_search: String,
    /// The pages of the collections.
    pub manage: manage::ManageState,
    /// The list of the fields of a vector layer. A key is a layer and the
    /// index of a field.
    pub field_list: list::ListState<(LayerId, usize)>,
    /// The text of the name field of the panel of a vector layer.
    name: Option<(LayerRef, String)>,
    /// The list of the projections of the flat view is open.
    pub projections: bool,
    /// The list of the projections in the last frame.
    pub projections_rect: Option<Rect>,
    /// The card of the View button and the scale bar of the readout in the
    /// last frame.
    pub view_rect: Option<Rect>,
    /// The View button of the readout in the last frame.
    view_button: Option<Rect>,
    /// The View popover is open.
    pub view_menu: bool,
    /// The View popover in the last frame.
    pub view_menu_rect: Option<Rect>,
    /// The card of the place above the readout in the last frame.
    pub place_rect: Option<Rect>,
    /// The capsule of the scale bar of a compact screen in the last frame.
    pub scale_rect: Option<Rect>,
    /// A finger, the pen, or the mouse button is down on the canvas.
    pub canvas_down: bool,
    /// The pen hovers over the canvas.
    pub pen_hover: bool,
    /// The last time that `canvas_down` or `pen_hover` was true.
    last_down: Option<std::time::Instant>,
    /// The banner of the last frame: a save that failed, or a raster layer
    /// that does not fit in the memory.
    pub banner: Option<Rect>,
    /// The card of the map on the globe in the last frame.
    pub map_card: Option<Rect>,
    /// The label at the end of the scale bar of the readout in the last
    /// frame.
    pub readout_scale: Option<String>,
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
    /// The last kind that was chosen in the Furniture cell of the Atlas.
    furniture_kind: Option<crate::ui::atlas::ItemKind>,
    /// The name of the tool that the Vector cell shows, while it is on.
    pub vector_face: Option<&'static str>,
    /// The distance that the sheet is pulled down from its open place.
    pub sheet_offset: f32,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The places of the cards in the last frame.
    pub rects: Option<Rects>,
    /// The region that the system reserves around a camera at the top of
    /// the screen, in points.
    pub camera: Option<Rect>,
    /// The folding region of a partially folded device, in points.
    pub fold: Option<Rect>,
    /// The pane of each open tab of the last frame, by its panel.
    pub panels: Vec<(Panel, Rect)>,
    /// The workspace menu of the last frame.
    pub menu_rect: Option<Rect>,
    /// The label and the icon of each brush cell in the last frame, by
    /// `ModeGroup::ALL`.
    pub brush_faces: [Option<(&'static str, Icon)>; 2],
    /// The height of the contents of each panel, by the key of the panel.
    heights: HashMap<&'static str, f32>,
    /// The key of a page that opens at the top in the next frame.
    scroll_top: Option<&'static str>,
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

    /// The panel is the open tab of its pane. On a compact screen, it is
    /// the panel of the sheet.
    pub fn shows(&self, panel: Panel) -> bool {
        match self.rects.is_some_and(|r| r.class == WidthClass::Compact) {
            true => self.sheet_panel() == Some(panel),
            false => self.dock().shows(panel),
        }
    }

    /// Shows a panel in the dock.
    pub fn open(&mut self, panel: Panel) {
        if !self.dock().shows(panel) {
            self.sheet_offset = 0.0;
        }
        self.dock_mut().open(panel);
        for list in self.cards.lists.values_mut() {
            list.open = None;
        }
    }

    pub fn close(&mut self, panel: Panel) {
        self.dock_mut().close(panel);
    }

    /// Closes a panel that shows, and else shows it. A panel in a folded
    /// pane shows.
    pub fn toggle(&mut self, panel: Panel) {
        if self.shows(panel) && !self.dock().folded(panel) {
            self.close(panel);
        } else {
            self.open(panel);
        }
    }

    /// The panel that a sheet shows: the panel of the last focus, or else
    /// the open tab of the last pane.
    pub fn sheet_panel(&self) -> Option<Panel> {
        self.dock().sheet_panel()
    }
}

pub const WORKSPACES: [&str; 2] = ["World", "Atlas"];
/// The Toolbox panel and its tile in the Panels menu.
pub const TOOLBOX: &str = "Toolbox";
/// The Layers panel and its tile in the Panels menu.
pub const LAYERS: &str = "Layers";
/// The button that opens the Panels menu.
pub const PANELS: &str = "Panels";
/// The button that hides and shows the dock.
pub const DOCK: &str = "Dock";
/// The button of the readout that opens the View popover.
pub const VIEW: &str = "View";
/// The segments of the globe and of the flat view, in the View popover.
pub const VIEWS: [&str; 2] = ["Globe", "Flat"];
/// The action that makes the place at the middle of the canvas the center of
/// the projection of the flat view.
pub const RECENTER: &str = "Recenter";
/// The action that puts the center of the projection and the view back.
pub const RESET: &str = "Reset view";
/// The row that opens the list of the projections.
pub const PROJECTION: &str = "Projection";
/// The switch of the View popover that shows the graticule.
pub const GRATICULE: &str = "Graticule";
/// The width of the space of a divider in a row of buttons.
pub(super) const DIVIDER: f32 = 9.0;

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
    // the same frame. The dock floats over the canvas.
    let bands = geometry::Bands::of(&ctx);
    let canvas = ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        globe::canvas_input(ui, state, theme::CANVAS)
    });

    let class = WidthClass::of(screen.width());
    pin_sheet(&mut state.pad);
    let mut input = Input {
        screen,
        bands,
        workspace_width: workspace_width(ui, class, WORKSPACES[0]),
        panel: has_sheet(state, class),
        sheet_offset: state.pad.sheet_offset,
        cells: card_cells(&state.globe),
        strip: has_strip(&state.globe),
        extra: extra_cells(state),
        tools: TOOLS,
        row: class == WidthClass::Compact,
        column: column_input(state),
        dock_width: state.pad.docks.world.width,
        camera: state.pad.camera,
        fold: state.pad.fold,
        glass: state.globe.backdrop().is_some(),
        keyboard: ctx.egui_wants_keyboard_input(),
        top_view: class == WidthClass::Compact,
    };
    let mut rects = geometry::layout(&input);
    state.pad.sheet_offset = input
        .sheet_offset
        .clamp(-rects.sheet_rise, rects.sheet_travel);
    if input.panel {
        input.sheet_offset = state.pad.sheet_offset;
        rects = geometry::layout(&input);
    }

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
    if rects.row {
        let closing = closing_sheet(&ctx, &input, &rects);
        bottom_bar(&ctx, state, &mut controls, &rects, &mut footer, closing);
    } else {
        rail_closing(&ctx, state, &rects, closing_sheet(&ctx, &input, &rects));
        tool_column(&ctx, state, &mut controls, &rects);
    }
    // A button above can open or close a panel. The panels use the new state.
    input.panel = has_sheet(state, class);
    input.cells = card_cells(&state.globe);
    input.column = column_input(state);
    input.sheet_offset = state.pad.sheet_offset;
    rects = geometry::layout(&input);
    if input.panel {
        input.sheet_offset = shown_sheet_offset(&ctx, state.pad.sheet_offset, rects.sheet_closed);
        rects = geometry::layout(&input);
    }
    table::check(state);
    table::widen(state, &mut rects);
    popover::set_layout(&ctx, &rects);
    expression::widen(state, &mut rects);
    state.sync_selection_form(state.pad.shows(Panel::Attributes));
    select::drop_focus(&ctx, state);
    state.pad.collections_rect = None;
    state.pad.collections_button = None;
    dock_ui::show(&ctx, state, &mut controls, &rects, &mut footer);
    super::new_feature::keep_in_view(&ctx, state, &rects);
    expire_notes(&ctx, state);
    raster::band_card(&ctx, state, &mut controls, &rects);
    raster::shade_color_frame(&ctx, state);
    rules::picker_frame(&ctx, state);
    raster::reset_card(&ctx, state, &mut controls, &rects);
    raster::ramp_card(&ctx, state, &mut controls, &rects, None);
    readout::readout(&ctx, state, &mut controls, &rects);
    held_card(&ctx, state, &mut controls, &rects);
    readout::view_menu(&ctx, state, &mut controls, &rects);
    if state.pad.menu && class == WidthClass::Compact {
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
        // The cells of the Atlas are not here.
        Some(Flyout::Frame | Flyout::Furniture) => state.pad.flyouts.open = None,
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
    crate::ui::export_form::pad(&ctx, state, &mut controls, &rects);
    super::region::pad(&ctx, state, &mut controls, &rects);
    readout::banner(&ctx, state, &rects);
    map_card::show(&ctx, state, &mut controls, &rects);
    dock_ui::handle(&ctx, state, &mut controls, &rects);
    if let Some(rect) = canvas.inner {
        globe::paint_canvas(ui, state, rect, theme::CANVAS);
    }

    state.pad.controls = controls;
    state.pad.rects = Some(rects);
}

/// A sheet at its full height or at its title row stays there when the
/// rows of the tools change, and with them the places of the sheet.
pub(super) fn pin_sheet(pad: &mut PadState) {
    if let Some(r) = pad.rects.filter(|r| r.sheet.is_some()) {
        if pad.sheet_offset <= -r.sheet_rise + 0.5 {
            pad.sheet_offset = f32::NEG_INFINITY;
        } else if pad.sheet_offset >= r.sheet_travel - 0.5 {
            pad.sheet_offset = f32::INFINITY;
        }
    }
}

/// True if a panel is open. On a compact screen, a panel is a sheet, and
/// the card of a new point takes the place of the sheet of the inspector.
fn has_sheet(state: &AppState, class: WidthClass) -> bool {
    let open = !state.pad.dock().panes.is_empty();
    open && !super::new_feature::point_card(state, class)
}

/// The width of the workspace card: the Worlds button, a divider, and the
/// buttons of the two workspaces. The compact layout has the button of the
/// workspace menu, which holds the Worlds row.
pub(super) fn workspace_width(ui: &egui::Ui, class: WidthClass, current: &str) -> f32 {
    match class {
        WidthClass::Compact => widgets::menu_button_width(ui, current),
        _ => WORLDS_LEAD + 2.0 * TOUCH + 2.0 * PAD,
    }
}

/// The workspace card at the left end of the top row.
pub(super) fn workspace_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let current = state.workspace;
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-workspace",
        rects.workspace,
        rects,
        |ui| {
            let card = ui.max_rect();
            if rects.class == WidthClass::Compact {
                let label = match current {
                    Workspace::Globe => WORKSPACES[0],
                    Workspace::Atlas => WORKSPACES[1],
                };
                let open = state.pad.menu;
                let menu = widgets::menu_button(ui, controls, "Workspace", label, open);
                if menu.clicked() {
                    state.pad.menu = !open;
                    state.pad.worlds = None;
                }
                save_mark(ui, state, menu.rect);
                return;
            }
            let at = |x: f32| {
                Rect::from_min_size(pos2(card.left() + x, card.top() + PAD), vec2(TOUCH, TOUCH))
            };
            let open = state.pad.worlds.is_some();
            let worlds =
                widgets::icon_button_at(ui, controls, at(PAD), WORLDS, Icon::Folder, open, true);
            if worlds.clicked() {
                toggle_worlds(state);
            }
            save_mark(ui, state, worlds.rect);
            let line = Rect::from_center_size(
                pos2(card.left() + PAD + TOUCH + 4.5, card.center().y),
                vec2(1.0, 24.0),
            );
            ui.painter().rect_filled(line, 0.0, theme::divider());
            let workspaces = [
                (WORKSPACES[0], Icon::World, Workspace::Globe),
                (WORKSPACES[1], Icon::Book, Workspace::Atlas),
            ];
            for (i, (name, icon, workspace)) in workspaces.into_iter().enumerate() {
                let rect = at(WORLDS_LEAD + PAD + i as f32 * TOUCH);
                let flyouts = &mut state.pad.flyouts;
                let on = current == workspace;
                let place = flyout::Place::Below;
                if flyout::button(
                    ui,
                    controls,
                    flyouts,
                    rect,
                    (name, name),
                    icon,
                    on,
                    true,
                    place,
                ) {
                    state.workspace = workspace;
                }
            }
        },
    );
}

/// The names of the states of the save mark.
pub const SAVING: &str = "Saving";
pub const NOT_SAVED: &str = "Not saved";
/// The side of the save mark.
const SAVE_MARK: f32 = 14.0;

/// The save mark at the bottom-right corner of a button: a spinner while a
/// change waits, and a warning sign after a save that failed. When the file
/// holds each change, the button has no mark.
fn save_mark(ui: &mut egui::Ui, state: &AppState, button: Rect) {
    let mark = state.save_mark(std::time::Instant::now());
    let Some(mark) = mark.filter(|&m| m != SaveMark::Saved) else {
        return;
    };
    let center = button.right_bottom() - vec2(SAVE_MARK / 2.0 + 2.0, SAVE_MARK / 2.0 + 2.0);
    let rect = Rect::from_center_size(center, vec2(SAVE_MARK, SAVE_MARK));
    let painter = ui.painter();
    painter.circle_filled(center, SAVE_MARK / 2.0 + 1.0, theme::menu());
    let name = match mark {
        // A test has no clock, so there the mark does not turn.
        SaveMark::Saving if !state.headless => {
            let spin = egui::Spinner::new()
                .size(SAVE_MARK - 4.0)
                .color(theme::MUTE);
            ui.put(rect.shrink(2.0), spin);
            SAVING
        }
        SaveMark::Saving | SaveMark::Saved => SAVING,
        SaveMark::Failed => {
            readout::paint_warning(painter, rect.shrink(1.0), theme::WARN);
            NOT_SAVED
        }
    };
    let id = ui.id().with("save-mark");
    let resp = ui.interact(rect, id, egui::Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name));
}

fn top_row(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    workspace_card(ctx, state, controls, rects);
    widgets::edge_card(
        ctx,
        state.globe.backdrop(),
        "pad-actions",
        rects.actions,
        rects,
        |ui| {
            let card = ui.max_rect();
            let inset = geometry::actions_inset(rects.class);
            let at = |x: f32| {
                let min = pos2(card.left() + inset + x, card.top() + PAD);
                Rect::from_min_size(min, vec2(TOUCH, TOUCH))
            };
            let step = TOUCH + PAD;
            let divider = |ui: &egui::Ui, x: f32| {
                let at = pos2(card.left() + inset + x + 4.5, card.center().y);
                let line = Rect::from_center_size(at, vec2(1.0, 24.0));
                ui.painter().rect_filled(line, 0.0, theme::divider());
            };
            let mut x = 0.0;
            // A compact screen has the View button at the left end, before
            // a divider. It has no readout card.
            if rects.class == WidthClass::Compact {
                let view = at(0.0);
                let flat = state.globe.world_view == crate::globe::WorldView::Flat;
                readout::view_button(ui, state, controls, view, flat);
                state.pad.view_rect = Some(view);
                state.pad.view_button = Some(view);
                divider(ui, step);
                x = step + geometry::DIVIDER_WIDTH;
            }
            let can_undo = state.can_undo();
            let undo =
                widgets::icon_button_at(ui, controls, at(x), "Undo", Icon::Undo, false, can_undo);
            if undo.clicked() {
                state.undo();
            }
            let can_redo = state.can_redo();
            let redo = widgets::icon_button_at(
                ui,
                controls,
                at(x + step),
                "Redo",
                Icon::Redo,
                false,
                can_redo,
            );
            if redo.clicked() {
                state.redo();
            }
            divider(ui, x + 2.0 * step);
            let x = x + 2.0 * step + geometry::DIVIDER_WIDTH;
            panels_button(ui, state, controls, at(x));
            if rects.class != WidthClass::Compact {
                dock_button(ui, state, controls, at(x + step));
            }
        },
    );
}

/// The button that hides and shows the dock. It has the accent color while
/// the dock shows, and it is off while the dock has no panes.
pub(super) fn dock_button(
    ui: &egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rect: Rect,
) {
    let dock = state.pad.dock();
    let enabled = !dock.panes.is_empty();
    let shown = enabled && !dock.collapsed;
    let sense = egui::Sense::click();
    let resp = widgets::hit(ui, controls, rect, DOCK, sense, enabled, Some(shown));
    let start = widgets::press_start(ui);
    let painter = ui.painter();
    let inner = rect.shrink(widgets::HIGHLIGHT_INSET);
    let radius = widgets::control_radius(inner);
    if shown {
        let fill = theme::ACCENT.gamma_multiply(0.3);
        painter.rect_filled(inner, radius, fill);
    }
    if resp.is_pointer_button_down_on() {
        painter.rect_filled(inner, radius, theme::raise());
    }
    let color = if shown { theme::ACCENT } else { theme::MUTE };
    let color = widgets::fade(color, enabled);
    icons::paint(painter, rect, Icon::Dock, widgets::ICON, color);
    widgets::press_end(ui, &resp, start);
    if resp.clicked() {
        let dock = state.pad.dock_mut();
        dock.collapsed = !dock.collapsed;
    }
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

/// Chooses the Measure tool, or goes back from it to Pan.
pub fn toggle_measure(state: &mut AppState) {
    let measuring = state.measuring();
    match state.workspace {
        Workspace::Globe => {
            state.globe.tool = if measuring { Tool::Pan } else { Tool::Measure };
        }
        Workspace::Atlas => {
            state.atlas.tool = if measuring {
                AtlasTool::Pan
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

/// The cells of the tool strip, in a row that fills `ui`.
fn strip_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let mut ui = row_in(ui, rects.grid, 0);
    if rects.tool_cells < TOOLS {
        egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut ui, |ui| tool_cells(ui, state, controls));
    } else {
        tool_cells(&mut ui, state, controls);
    }
}

/// A row of cells on the columns of `grid`, from column `start`: a child
/// of `ui` with the cells from left to right, with the space between two
/// columns. Without a grid, the cells are `PAD` inside `ui`.
pub(super) fn row_in(ui: &mut egui::Ui, grid: Option<geometry::Grid>, start: usize) -> egui::Ui {
    let Some(grid) = grid else {
        return padded_in(ui, true);
    };
    // The cells of the rows of a phone are icons alone, as in the tool
    // column. `footer` puts the style back.
    let style = match grid.segment {
        true => widgets::CellStyle::Segment,
        false => widgets::CellStyle::Icon,
    };
    widgets::set_cell_style(ui.ctx(), style);
    widgets::set_segment_width(ui.ctx(), grid.cell_width());
    let outer = ui.max_rect();
    let rect = Rect::from_min_max(
        pos2(grid.cell_left(start), outer.top() + PAD),
        pos2(outer.right(), outer.bottom() - PAD),
    );
    let layout = egui::Layout::left_to_right(egui::Align::Min);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    ui.spacing_mut().item_spacing = vec2(grid.spacing(), 0.0);
    ui
}

/// The floating card of the rows of a compact screen with no sheet: the
/// rows of the tools at the place of the footer of the sheet, so a sheet
/// grows up from the card and no cell moves. `closing` is the place of a
/// sheet that closes: the card is there, with no panel, until it reaches
/// the place of the rows.
pub(super) fn bottom_bar(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    footer: dock_ui::Footer<'_>,
    closing: Option<Rect>,
) {
    let Some(bar) = rects.bar.filter(|_| rects.sheet.is_none()) else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(bar_pass(), pass));
    // The card has the id of the sheet, so a sheet that opens or closes
    // is the same card.
    let radius = rects.sheet_radius();
    let backdrop = state.globe.backdrop();
    widgets::card(
        ctx,
        backdrop,
        "pad-sheet",
        closing.unwrap_or(bar),
        Order::Middle,
        radius,
        |ui| {
            if let Some(sheet) = closing {
                dock_ui::sheet_chrome(ui, sheet, Some(bar));
            }
            footer(ui, state, controls, rects);
        },
    );
}

fn bar_pass() -> egui::Id {
    egui::Id::new("pad-bar-pass")
}

/// True if `bottom_bar` showed the card of the tools in this pass.
pub(super) fn bar_shown(ctx: &egui::Context) -> bool {
    let pass: Option<u64> = ctx.data(|d| d.get_temp(bar_pass()));
    pass == Some(ctx.cumulative_pass_nr())
}

/// The sheet beside the rail as it closes, with no panel, down to its
/// title row.
pub(super) fn rail_closing(
    ctx: &egui::Context,
    state: &AppState,
    rects: &Rects,
    closing: Option<Rect>,
) {
    let Some(sheet) = closing else {
        return;
    };
    let radius = rects.sheet_radius();
    let backdrop = state.globe.backdrop();
    widgets::card(
        ctx,
        backdrop,
        "pad-sheet",
        sheet,
        Order::Middle,
        radius,
        |ui| {
            dock_ui::sheet_chrome(ui, sheet, None);
        },
    );
}

/// The place of a sheet that closes. It follows the spring of the sheet
/// down to the floating card of the rows, or beside the rail down to its
/// title row. None if no sheet closes.
pub(super) fn closing_sheet(ctx: &egui::Context, input: &Input, rects: &Rects) -> Option<Rect> {
    if input.panel || (rects.bar.is_none() && !rects.rail) {
        return None;
    }
    let id = sheet_spring();
    let pass = ctx.cumulative_pass_nr();
    let seen: Option<u64> = ctx.data(|d| d.get_temp(id.with("seen")));
    if seen.is_none_or(|p| p + 1 < pass) {
        return None;
    }
    let offset = motion::points(ctx, id, rects.sheet_closed, motion::SNAPPY);
    if offset >= rects.sheet_closed - 0.5 {
        return None;
    }
    ctx.data_mut(|d| d.insert_temp(id.with("seen"), pass));
    let input = Input {
        panel: true,
        sheet_offset: offset,
        ..*input
    };
    geometry::layout(&input).sheet
}

/// A child of `ui` at `rect`, for a row of the footer of the sheet.
pub(super) fn row_at(ui: &mut egui::Ui, rect: Rect) -> egui::Ui {
    let layout = egui::Layout::top_down(egui::Align::Min);
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

/// The rows of the tools in the footer of the sheet: the controls of the
/// tool above the tool strip.
pub(super) fn footer(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    if let Some(rect) = rects.brush {
        if rects.card_cells > 0 {
            cell_card_in(ui, state, controls, rects);
        } else {
            brush_cells(&mut row_at(ui, rect), state, controls, rects);
        }
    }
    strip_cells(&mut row_at(ui, rects.tools), state, controls, rects);
    widgets::set_cell_style(ui.ctx(), widgets::CellStyle::default());
}

/// The cells of the tools: Sculpt, Terrain, Vector, Select, Move, and
/// Measure.
fn tool_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    for group in ModeGroup::ALL {
        brush_cell(ui, state, controls, group);
    }
    vector_cell(ui, state, controls);
    let globe = &state.globe;
    let pan = !globe.brushing() && !globe.editing() && !globe.measuring() && !globe.selecting();
    for (name, label, icon, tool, active) in [
        (
            select::TOOL,
            select::TOOL,
            Icon::Select,
            Tool::Select,
            globe.selecting(),
        ),
        (PAN, PAN, Icon::Hand, Tool::Pan, pan),
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
}

/// The contents of the tool column for the tool of the globe.
fn column_input(state: &AppState) -> ColumnInput {
    let globe = &state.globe;
    let (cells, sliders, brush_button) = if card_cells(globe) > 0 {
        (card_cells(globe), 0, false)
    } else if has_strip(globe) {
        let (cells, sliders) = strip_counts(state);
        (cells, sliders, true)
    } else {
        (0, 0, false)
    };
    // The cell of the polygon modes is above the slider.
    let strip = card_cells(globe) == 0 && has_strip(globe);
    let vector = globe.stroking() || globe.placing();
    ColumnInput {
        tools: TOOLS,
        cells: cells + extra_cells(state),
        lead: usize::from(strip && vector && globe.filling()),
        sliders,
        brush_button,
    }
}

/// The number of cells and of sliders that `strip_controls` adds.
fn strip_counts(state: &AppState) -> (usize, usize) {
    let globe = &state.globe;
    if globe.sweeping() {
        // The mode cell, and the Smoothing slider and the Size slider.
        (1, 2)
    } else if globe.stroking() || globe.placing() {
        let cells = 1
            + usize::from(globe.filling())
            + usize::from(globe.drawing())
            + usize::from(globe.lining());
        (cells, usize::from(globe.stroking()))
    } else if globe.brushing() {
        (0, strip_sliders(state).len())
    } else {
        (0, 0)
    }
}

/// Shows the tool column card at `rects.tools` with the contents that
/// `add_contents` adds from the top, as icon cells. The contents scroll if
/// they are longer than the card.
pub(super) fn column_card(
    ctx: &egui::Context,
    backdrop: Option<eframe::egui_wgpu::wgpu::TextureFormat>,
    rects: &Rects,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let rect = rects.tools;
    // The rail reaches into the bottom band, and its controls stay above it.
    let content = rects.tools_content();
    let scroll = rects.column_content > content.height() + 0.5;
    let inset = geometry::COLUMN_INSET;
    let gap = rects.column_gap.filter(|_| !scroll);
    let card_gap = gap.map(|g| egui::Rangef::new(g.min + inset, g.max - inset));
    widgets::split_edge_card(ctx, backdrop, "pad-tools", rect, card_gap, rects, |ui| {
        let mut inner = ui.max_rect();
        inner.max.y = inner.max.y.min(content.bottom());
        let inner = inner.shrink(geometry::COLUMN_INSET);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.set_clip_rect(content.shrink2(vec2(0.0, geometry::COLUMN_INSET)));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        widgets::with_cell_style(&mut ui, widgets::CellStyle::Icon, |ui| {
            if scroll {
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, add_contents);
            } else {
                widgets::set_column_gap(ui.ctx(), gap);
                add_contents(ui);
                widgets::set_column_gap(ui.ctx(), None);
            }
        });
    });
}

/// The line between the tools and the controls of the tool in the tool
/// column.
pub(super) fn column_divider(ui: &mut egui::Ui) {
    widgets::skip_fold(ui, geometry::COLUMN_DIVIDER);
    let size = vec2(ui.available_width(), geometry::COLUMN_DIVIDER);
    let (rule, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let line = Rect::from_center_size(rule.center(), vec2(rule.width() - 4.0, 1.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
}

/// The tool column: the tool cells, and below a divider the controls of
/// the tool.
fn tool_column(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let mut merge_cell = None;
    let mut selectable_cell = None;
    let backdrop = state.globe.backdrop();
    let context = rects.column_context;
    // In the rail, the controls of the tool are above the tools, so the
    // tool cells at the bottom never move.
    column_card(ctx, backdrop, rects, |ui| {
        if !rects.rail {
            tool_cells(ui, state, controls);
        }
        if !context {
            if rects.rail {
                tool_cells(ui, state, controls);
            }
            return;
        }
        if !rects.rail {
            column_divider(ui);
        }
        if state.globe.vertexing() {
            vertex_cells(ui, state, controls);
        } else if state.globe.selecting() {
            selectable_cell = Some(selectable::cell(ui, state, controls));
            merge_cell = select_cells(ui, state, controls);
        } else if state.globe.measuring() {
            measure_cells(ui, state, controls);
        } else if has_strip(&state.globe) {
            let slide = match rects.brush_card {
                BrushCard::Full(track) => Slide::Column(track),
                BrushCard::Bare(track) if rects.rail => {
                    Slide::Bare(track, geometry::RAIL_SLIDER_GAP)
                }
                BrushCard::Bare(track) => Slide::Bare(track, geometry::SLIDER_GAP),
                _ => Slide::Column(0.0),
            };
            strip_controls(ui, state, controls, slide);
            deselect_cell(ui, state, controls);
            brush_button(ui, state, controls);
        } else {
            deselect_cell(ui, state, controls);
        }
        if rects.rail {
            column_divider(ui);
            tool_cells(ui, state, controls);
        }
    });
    match merge_cell {
        Some(cell) if state.merging => merge_menu(ctx, state, controls, rects, cell),
        _ => state.merging = false,
    }
    match selectable_cell {
        Some(cell) if state.pad.selectable => selectable::menu(ctx, state, controls, rects, cell),
        _ => state.pad.selectable = false,
    }
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
    let mut number = Number::new(name, km, range.clone())
        .label("Size")
        .log()
        .shown(1.0 / unit_km, symbol)
        .text(move |km| world::distance(km, units))
        .preview(move |km| (km / world_km * globe_radius) as f32);
    if brush.size_is_fixed() {
        number = number.default(*range.end());
    } else if brush.lock.is_none() {
        let points = BrushSettings::default().size_points;
        number = number.default(brush_radius(points, globe_radius) * world_km);
    }
    if let Some(km) = control(number) {
        state.globe.brush.set_radius_km(km, globe_radius, world_km);
    }
}

/// A number from 0 to 1 in whole percents.
/// The slider of the radius of the Polygon brush, in points. During a
/// drag, a circle shows the brush.
fn polygon_brush_size(state: &AppState, name: &'static str) -> Number<'static> {
    let (lo, hi) = (*sweep::SIZE.start(), *sweep::SIZE.end());
    let size = f64::from(state.globe.line.brush_size);
    Number::new(name, size, f64::from(lo)..=f64::from(hi))
        .log()
        .step(1.0)
        .unit("pt")
        .default(f64::from(sweep::DEFAULT_SIZE))
        .preview(|points| points as f32)
}

fn percent(name: &str, value: f64) -> Number<'_> {
    Number::new(name, value, 0.0..=1.0).percent()
}

/// The controls of the tool, in a row that fills `ui`. A tool with no
/// context strip has the extra cells alone.
fn brush_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let mut ui = row_in(ui, rects.row_grid, 0);
    if !has_strip(&state.globe) {
        deselect_cell(&mut ui, state, controls);
        return;
    }
    if let BrushCard::Full(pitch) = rects.brush_card {
        let spans = rects.slider_spans;
        strip_controls(&mut ui, state, controls, Slide::Row { pitch, spans });
    }
    deselect_cell(&mut ui, state, controls);
    brush_button(&mut ui, state, controls);
}

/// The length of the sliders of the controls of the brush and of the
/// vector tools.
#[derive(Clone, Copy)]
enum Slide {
    /// The row layout: each slider spans whole columns of this pitch, the
    /// first slider the first number of columns and each later slider the
    /// second number.
    Row { pitch: f32, spans: [usize; 2] },
    /// The tool column, with tracks of this length. Zero: the Brush panel
    /// holds the sliders.
    Column(f32),
    /// The tool column, with tracks of this length and no values and no
    /// names, and this space above and below each track in all.
    Bare(f32, f32),
}

/// The controls of the brush and of the vector tools. Each control shows
/// while the tool can use it.
fn strip_controls(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, slide: Slide) {
    // `slide` adds slider number `i` of the row.
    let slide =
        |number: Number<'_>, ui: &mut egui::Ui, controls: &mut Controls, i: usize| match slide {
            Slide::Row { pitch, spans } => {
                let k = spans[i.min(1)].max(1);
                let width = (k - 1) as f32 * pitch + geometry::ROW_CELL;
                number.horizontal_bare(ui, controls, width)
            }
            Slide::Column(track) if track > 0.0 => {
                number.vertical(ui, controls, track + geometry::SLIDER_TEXT)
            }
            Slide::Column(_) => None,
            Slide::Bare(track, space) => number.vertical_bare(ui, controls, track + space, space),
        };
    let paint = state.globe.brushing();
    let line = state.globe.drawing();
    let stroke = state.globe.stroking();
    let vector = stroke || state.globe.placing();
    // The tools of a vector layer have the snap button. The line tool, the
    // polygon tool, and Split also have the smoothing. The line tool and
    // the polygon tool also have the reshape button. The line tool also
    // has the terrain fit button, and the polygon tool has its modes.
    let fit = state.globe.lining();
    let fill = state.globe.filling();
    // The Polygon brush has the modes, the smoothing, and the size, and it
    // does not snap.
    let sweep = state.globe.sweeping();
    if vector {
        if fill || sweep {
            let mode = state.globe.line.polygon_mode;
            let (label, icon, _) = POLYGON_MODES
                .into_iter()
                .find(|m| m.2 == mode)
                .expect("a mode");
            let key = Flyout::PolygonMode;
            let flyouts = &mut state.pad.flyouts;
            let on = mode != PolygonMode::New;
            if flyout::cell(
                ui,
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
        if sweep {
            let smoothing = &mut state.globe.line.smoothing;
            let number = percent(SMOOTHING, *smoothing).default(DEFAULT_SMOOTHING);
            if let Some(value) = slide(number, ui, controls, 0) {
                *smoothing = value;
            }
            if let Some(value) = slide(
                polygon_brush_size(state, POLYGON_BRUSH_SIZE),
                ui,
                controls,
                1,
            ) {
                state.globe.line.brush_size = value as f32;
            }
            return;
        }
        if stroke {
            let smoothing = &mut state.globe.line.smoothing;
            let number = percent(SMOOTHING, *smoothing).default(DEFAULT_SMOOTHING);
            if let Some(value) = slide(number, ui, controls, 0) {
                *smoothing = value;
            }
        }
        snap_cell(ui, state, controls);
        if line {
            // Reshape works in the New mode of the polygon tool.
            let new = state.globe.line.polygon_mode == PolygonMode::New;
            let enabled = !fill || new;
            let on = &mut state.globe.line.reshape;
            let cell = widgets::tool_cell(
                ui,
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
                ui,
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
        for (i, slider) in strip_sliders(state).into_iter().enumerate() {
            let mut control = |number: Number<'_>, ui: &mut egui::Ui, controls: &mut Controls| {
                slide(number, ui, controls, i)
            };
            brush_slider(ui, controls, state, slider, &mut Place::Strip(&mut control));
        }
    }
}

/// The sliders of the brush that the strip shows.
fn strip_sliders(state: &AppState) -> Vec<Slider> {
    let globe = &state.globe;
    state.tools.pins.strip(globe.brush.mode, globe.is_height())
}

/// The card of cells in the footer of the sheet, at `rects.brush`.
pub(super) fn cell_card_in(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let Some(rect) = rects.brush.filter(|_| rects.card_cells > 0) else {
        return;
    };
    let cells = card_cells_in(&mut row_at(ui, rect), state, controls, rects);
    cell_menus(&ui.ctx().clone(), state, controls, rects, cells);
}

/// The cells of the card of cells, in a row or a column that fills `ui`.
/// Returns the places of the Merge cell and the Selectable cell.
fn card_cells_in(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) -> (Option<Rect>, Option<Rect>) {
    let mut merge_cell = None;
    let mut selectable_cell = None;
    let mut ui = if rects.row {
        row_in(ui, rects.row_grid, 0)
    } else {
        padded_in(ui, false)
    };
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
    (merge_cell, selectable_cell)
}

/// The lists of the Merge cell and the Selectable cell, while they are open.
fn cell_menus(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    (merge_cell, selectable_cell): (Option<Rect>, Option<Rect>),
) {
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
    let height = 2.0 * widgets::menu_inset().y + (choices.len() + 1) as f32 * widgets::PANEL_ROW;
    // In the row layout, the list is above its cell.
    let rect = if rects.row {
        let height = height.min(cell.top() - 8.0 - rects.column_top);
        let left = cell.right() - width;
        let left = left.min(screen.right() - width).max(screen.left());
        Rect::from_min_size(pos2(left, cell.top() - 8.0 - height), vec2(width, height))
    } else {
        let top = cell.top().min(screen.bottom() - height).max(screen.top());
        // The list of a cell of the rail is at the left of the cell.
        let x = match rects.rail {
            true => cell.left() - 2.0 * PAD - 8.0 - width,
            false => cell.right() + 2.0 * PAD + 8.0,
        };
        Rect::from_min_size(pos2(x, top), vec2(width, height))
    };
    let rect = geometry::off_fold(rect, cell, rects.fold, screen);
    if choices.is_empty() || pressed.is_some_and(|p| !rect.contains(p) && !cell.contains(p)) {
        state.merging = false;
        return;
    }
    let mut keep = None;
    let radius = theme::CARD_RADIUS.into();
    let backdrop = state.globe.backdrop();
    widgets::card_from(
        ctx,
        backdrop,
        "pad-merge",
        rect,
        Order::Foreground,
        radius,
        cell.center().clamp(rect.min, rect.max),
        |ui| {
            let inner = rect.shrink2(widgets::menu_inset());
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
            let rows: Vec<list::Row<usize>> = (choices.iter().enumerate())
                .map(|(i, (_, name))| list::Row::new(i, name.clone()))
                .collect();
            let mut taps = list::ListState::default();
            let spec = list::Spec::default();
            let events = list::show(
                &mut ui,
                controls,
                &mut taps,
                spec,
                &rows,
                |ui, c, cell, i| {
                    let look = list::Look {
                        text: &rows[i].name,
                        ..list::Look::default()
                    };
                    list::draw(ui, c, cell, &look)
                },
            );
            for event in events {
                if let list::Event::Tap(i) = event {
                    keep = Some(choices[i].0);
                }
            }
        },
    );
    if let Some(uuid) = keep {
        state.merge_selection(uuid);
    }
}

/// The cells of the card of the Vertex tool: Snap, the switch of the move
/// of shared vertices, Delete vertex, Smooth vertices, and the Brush button.
fn vertex_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    snap_cell(ui, state, controls);
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

/// The cells of the card of the Measure tool: Snap, the Clear path cell,
/// and the Brush button.
pub(super) fn measure_cells(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    snap_cell(ui, state, controls);
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

/// The Snap cell of a card. A tap turns snapping on and off, and a hold
/// opens the Snap panel.
fn snap_cell(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let on = state.globe.snap.on;
    let cell = widgets::tool_cell_with(ui, controls, SNAP, SNAP, Icon::Magnet, on, true, false);
    match flyout::press(ui, &mut state.pad.flyouts, &cell, SNAP) {
        flyout::Press::Tap => state.globe.snap.on = !on,
        flyout::Press::Long => state.pad.open(Panel::Snap),
        flyout::Press::None => {}
    }
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
        Panel::Brush if !brush_settings(state) => panels_menu::BRUSH,
        Panel::Brush if state.measuring() => MEASURE,
        Panel::Brush if state.globe.lining() => LINE,
        Panel::Brush if state.globe.filling() => POLYGON,
        Panel::Brush if state.globe.sweeping() => POLYGON_BRUSH,
        Panel::Brush if state.globe.placing() => POINT,
        Panel::Brush if state.globe.splitting() => SPLIT,
        Panel::Brush if state.globe.vertexing() => VERTEX,
        Panel::Brush => panels_menu::BRUSH,
        Panel::Snap => SNAP,
        Panel::Layers => LAYERS,
        Panel::Toolbox => TOOLBOX,
        Panel::Attributes => select::ATTRIBUTES,
        Panel::Properties => atlas::PROPERTIES,
        Panel::Projection => atlas::FRAME_TABS[1],
        Panel::Labels => atlas::FRAME_TABS[3],
        Panel::Pages => panels_menu::PAGES,
    }
}

pub fn panel_icon(panel: Panel) -> Icon {
    match panel {
        Panel::Brush => Icon::Brush,
        Panel::Snap => Icon::Magnet,
        Panel::Layers => Icon::Layers,
        Panel::Toolbox => Icon::Toolbox,
        Panel::Attributes => Icon::Attributes,
        Panel::Properties => Icon::Properties,
        Panel::Projection => Icon::Projection,
        Panel::Labels => Icon::Tag,
        Panel::Pages => Icon::Pages,
    }
}

/// The rows of a page of a panel that the page made itself.
pub(super) type Rows = Box<dyn FnOnce(&mut egui::Ui, &mut AppState, &mut Controls)>;
/// What the back button of a page does.
pub(super) type Back = Box<dyn FnOnce(&mut AppState)>;

/// A page of a panel of the Atlas: what its back button does, its rows that
/// do not scroll, and its body. Each part runs one time.
pub(super) struct View {
    pub back: Option<Back>,
    pub top: Option<Rows>,
    pub body: Option<Rows>,
}

/// What a tap on the button with this name does.
pub(super) type ButtonAction = Box<dyn FnOnce(&mut AppState, &'static str)>;

/// The icon buttons at the right of the back row of a page, with their
/// names, and what a tap on one of them does.
pub(super) struct PageButtons {
    pub buttons: Vec<(&'static str, Icon)>,
    pub act: ButtonAction,
}

/// The parts of a page of a panel of the Atlas.
pub(super) struct PageParts {
    pub key: &'static str,
    pub title: String,
    pub back: Option<String>,
    pub scroll: egui::scroll_area::ScrollSource,
}

impl PageParts {
    pub fn new(key: &'static str, title: &str, back: Option<&str>) -> PageParts {
        PageParts {
            key,
            title: title.to_owned(),
            back: back.map(str::to_owned),
            scroll: egui::scroll_area::ScrollSource::default(),
        }
    }

    /// The head of the page in the pane of `panel`.
    pub fn head(
        self,
        panel: Panel,
        back: impl FnOnce(&mut AppState) + 'static,
        top: impl FnOnce(&mut egui::Ui, &mut AppState, &mut Controls) + 'static,
        body: impl FnOnce(&mut egui::Ui, &mut AppState, &mut Controls) + 'static,
    ) -> Head {
        Head {
            key: self.key,
            panel,
            title: self.title,
            back: self.back,
            subtitle: None,
            action: None,
            scroll: self.scroll,
            layer: None,
            page: None,
            buttons: None,
            view: Some(View {
                back: Some(Box::new(back)),
                top: Some(Box::new(top)),
                body: Some(Box::new(body)),
            }),
        }
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
    /// The text at the right of the title of a pane with one tab, such as
    /// the name of the map of the selected frame.
    pub subtitle: Option<String>,
    layer: Option<LayerRef>,
    page: Option<Page>,
    /// The buttons of the back row.
    pub buttons: Option<PageButtons>,
    /// The parts of a page of a panel of the Atlas.
    pub view: Option<View>,
}

/// Finds the page that a panel shows, and closes a page that is gone.
pub(super) fn panel_head(state: &mut AppState, panel: Panel) -> Head {
    if state.pad.workspace == Workspace::Atlas
        && let Some(head) = atlas::head(state, panel)
    {
        return head;
    }
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
        Some(
            Page::Rule(i)
            | Page::Symbols(i)
            | Page::MarkerShape(i)
            | Page::MarkerFill(i)
            | Page::FlowHeight(Some(i))
            | Page::DashPattern(Some(i)),
        ) => rules.and_then(|r| r.get(i)).map(|r| r.name.clone()),
        Some(Page::Rules | Page::FlowHeight(None) | Page::DashPattern(None)) => {
            match state.pad.layer {
                Some(LayerRef::Vector(id)) => Some(rules::selection_title(state, id)),
                _ => None,
            }
        }
        _ => None,
    };
    if name.is_none() {
        state.pad.layer = None;
    }
    let layer = state.pad.layer.filter(|_| panel == Panel::Layers);
    let manage_head = match (panel, layer) {
        (Panel::Layers, None) => manage::head(state),
        _ => None,
    };
    let layer_back = manage::layer_back(state);
    let fits = match (layer, state.pad.page) {
        (Some(_), Some(Page::Collections | Page::CopyStyle)) => true,
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
        (
            Some(LayerRef::Vector(id)),
            Some(Page::Rules | Page::FlowHeight(None) | Page::DashPattern(None)),
        ) => !state.pad.rules.picked(id).is_empty(),
        (
            Some(LayerRef::Vector(_)),
            Some(Page::FlowHeight(Some(i)) | Page::DashPattern(Some(i))),
        ) => i < rule_count,
        (Some(LayerRef::Raster(_) | LayerRef::Vector(_)), Some(Page::RampEditor)) => {
            state.ramp_draft.is_some()
        }
        _ => false,
    };
    if panel == Panel::Layers && !fits {
        if state.pad.page == Some(Page::RampEditor) {
            state.ramp_draft = None;
        }
        state.pad.page = None;
    }
    let page = state.pad.page.filter(|_| panel == Panel::Layers);
    if panel == Panel::Layers && page != Some(Page::Ramps) {
        state.pad.style.ramp_list = list::ListState::default();
    }
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
        (Panel::Layers, Some(_)) if page == Some(Page::Collections) => {
            ("pad-collections", collection::COLLECTIONS, Some(name))
        }
        (Panel::Layers, Some(_)) if page == Some(Page::CopyStyle) => {
            ("pad-copy-style", copy_style::TITLE, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Raster(_))) if page == Some(Page::Ramps) => {
            ("pad-ramps", raster::COLOR_RAMPS, Some(raster::COLOR_STOPS))
        }
        (Panel::Layers, Some(LayerRef::Raster(_) | LayerRef::Vector(_)))
            if matches!(page, Some(Page::RampLabels(_))) =>
        {
            ("pad-ramp-labels", raster::LABELS, Some(raster::COLOR_RAMPS))
        }
        (Panel::Layers, Some(LayerRef::Raster(_) | LayerRef::Vector(_)))
            if page == Some(Page::RampEditor) =>
        {
            let back = ramp_editor::back_page(state).1;
            ("pad-ramp-editor", ramp_editor::title(state), Some(back))
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
        (Panel::Layers, Some(LayerRef::Raster(_))) => (
            "pad-layer",
            name,
            Some(layer_back.as_deref().unwrap_or("Layers")),
        ),
        (Panel::Layers, Some(LayerRef::Image(_))) if page == Some(Page::Blend) => {
            ("pad-blend", raster::BLEND, Some(name))
        }
        (Panel::Layers, Some(LayerRef::Image(_))) => (
            "pad-image",
            name,
            Some(layer_back.as_deref().unwrap_or("Layers")),
        ),
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
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::FlowHeight(_))) => {
            ("pad-flow-height", rules::HEIGHT_LAYER, rule_name.as_deref())
        }
        (Panel::Layers, Some(LayerRef::Vector(_)))
            if matches!(page, Some(Page::DashPattern(_))) =>
        {
            ("pad-dash-pattern", rules::PATTERN, rule_name.as_deref())
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
        (Panel::Layers, Some(LayerRef::Vector(_))) if page == Some(Page::Rules) => (
            "pad-rules",
            rule_name.as_deref().unwrap_or_default(),
            Some(name),
        ),
        (Panel::Layers, Some(LayerRef::Vector(_))) if matches!(page, Some(Page::Rule(_))) => (
            "pad-rule",
            rule_name.as_deref().unwrap_or_default(),
            Some(name),
        ),
        (Panel::Layers, Some(LayerRef::Vector(_))) => (
            "pad-vector",
            name,
            Some(layer_back.as_deref().unwrap_or("Layers")),
        ),
        (Panel::Layers, None) => match &manage_head {
            Some((key, title, back)) => (*key, title.as_str(), Some(back.as_str())),
            None => (panel.key(), "Layers", None),
        },
        (Panel::Toolbox, _) => toolbox::head(state.pad.toolbox.page),
        (Panel::Attributes, _) => (panel.key(), select::ATTRIBUTES, None),
        (_, _) => (panel.key(), panel_name(state, panel), None),
    };
    let list = panel == Panel::Layers && layer.is_none() && manage_head.is_none();
    if list && !state.pad.cards.lifted() {
        state.pad.name = None;
    }
    // The head of each pane comes here, so another pane must not end a
    // gesture in the layer list.
    let list_shown = state.pad.shows(Panel::Layers)
        && state.pad.layer.is_none()
        && state.pad.manage.page.is_none();
    if !list_shown {
        state.pad.cards.shut();
        state.pad.cards.lift = None;
        state.pad.collections_menu = false;
    }
    let scroll = egui::scroll_area::ScrollSource::default();
    let mode = state.globe.brush.mode;
    let action = match panel {
        Panel::Attributes => Some((select::FOCUS_NEW, Icon::TextCursor, state.tools.focus_new)),
        // The button shows while the pins of the mode are not its default
        // pins.
        Panel::Brush => {
            (brush_settings(state) && state.globe.brushing() && state.tools.pins.is_changed(mode))
                .then_some((RESET_PINS, Icon::ResetPins, false))
        }
        _ => list.then_some((ADD_LAYER, Icon::Plus, state.pad.add)),
    };
    let subtitle = match panel {
        Panel::Attributes => select::layer_name(state),
        _ => None,
    };
    let buttons = match (layer, page) {
        (Some(LayerRef::Vector(id)), None) => {
            let rule_buttons = rules::selection_buttons(state, id, rule_count);
            let rule_buttons = rule_buttons.map(|buttons| PageButtons {
                buttons,
                act: Box::new(move |state, name| rules::selection_action(state, id, name)),
            });
            rule_buttons.or_else(|| {
                let buttons = fields::selection_buttons(state, id)?;
                Some(PageButtons {
                    buttons,
                    act: Box::new(move |state, name| fields::selection_action(state, id, name)),
                })
            })
        }
        (Some(LayerRef::Raster(id)), Some(Page::Ramps)) => {
            let user = raster::RampUser::Raster(id);
            raster::ramp_buttons(state).map(|buttons| PageButtons {
                buttons,
                act: Box::new(move |state, name| raster::ramp_group(state, user, name)),
            })
        }
        (Some(LayerRef::Vector(id)), Some(Page::Ramps)) => {
            let user = raster::RampUser::Classify(id);
            raster::ramp_buttons(state).map(|buttons| PageButtons {
                buttons,
                act: Box::new(move |state, name| raster::ramp_group(state, user, name)),
            })
        }
        (None, None) if list => {
            let picked = state.pad.cards.picked().len();
            let live = collection::live_rows(&state.doc.project);
            let mut count: Vec<LayerId> = live.into_iter().flat_map(|(_, rows)| rows).collect();
            count.sort();
            count.dedup();
            let count = count.len();
            let buttons = list::group_buttons(picked, count, &[], Some(DELETE_LAYERS));
            buttons.map(|buttons| PageButtons {
                buttons,
                act: Box::new(layer_group),
            })
        }
        _ => None,
    };
    Head {
        key,
        panel,
        title: title.to_owned(),
        back: back.map(str::to_owned),
        subtitle,
        action,
        scroll,
        layer,
        page,
        buttons,
        view: None,
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
    if panel == Panel::Toolbox {
        if back {
            toolbox::back(state);
        }
        return;
    }
    match action {
        Some(reset) if panel == Panel::Brush => {
            if reset.clicked() {
                state.tools.pins.reset(state.globe.brush.mode);
            }
        }
        Some(focus) if panel == Panel::Attributes => {
            if focus.clicked() {
                state.tools.focus_new = !state.tools.focus_new;
            }
        }
        Some(add) => {
            state.pad.add_button = Some(add.rect);
            if add.clicked() {
                state.pad.add = !state.pad.add;
                // The menu opens on the default collection.
                state.pad.add_collection = None;
                state.pad.add_picking = false;
            }
        }
        None => {}
    }
    if back && page == Some(Page::Ramps) {
        let vector = matches!(layer, Some(LayerRef::Vector(_)));
        state.pad.page = Some(if vector { Page::Classify } else { Page::Stops });
    } else if back && matches!(page, Some(Page::RampLabels(_))) {
        state.pad.page = Some(Page::Ramps);
    } else if back && page == Some(Page::RampEditor) {
        ramp_editor::cancel(state);
    } else if let (true, Some(Page::Symbols(i) | Page::MarkerShape(i) | Page::MarkerFill(i))) =
        (back, page)
    {
        state.pad.page = Some(Page::Rule(i));
    } else if let (true, Some(Page::FlowHeight(rule) | Page::DashPattern(rule))) = (back, page) {
        state.pad.page = Some(rule.map_or(Page::Rules, Page::Rule));
    } else if back && page.is_some() {
        state.pad.page = None;
        state.globe.preview.selected = None;
    } else if back && panel == Panel::Layers && layer.is_none() {
        manage::back(state);
    } else if back {
        state.pad.layer = None;
    }
}

/// The collection popover of the map of the Layers tab in the Atlas
/// workspace, while it is open.
pub(super) fn map_collections(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    head: &Head,
) {
    if head.panel != Panel::Layers || state.workspace != Workspace::Atlas {
        return;
    }
    let map = atlas::layers_map(state);
    match (
        state.pad.collections_menu,
        state.pad.collections_button,
        map,
    ) {
        (true, Some(button), Some(map)) => {
            let view = collection::View::Map(map);
            collection::popover(ui, state, controls, button, view);
        }
        _ => state.pad.collections_menu = false,
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
    if head.panel == Panel::Layers
        && head.layer.is_none()
        && state.pad.manage.page.is_none()
        && state.workspace == Workspace::Globe
    {
        match (state.pad.collections_menu, state.pad.collections_button) {
            (true, Some(button)) => {
                collection::popover(ui, state, controls, button, collection::View::Globe);
            }
            _ => state.pad.collections_menu = false,
        }
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
    if panel == Panel::Layers
        && page == Some(Page::Collections)
        && let Some(id) = layer.and_then(|l| layer_id(state, l))
    {
        collection::page_body(ui, state, controls, id);
        return;
    }
    if panel == Panel::Layers
        && page == Some(Page::CopyStyle)
        && let Some(id) = layer.and_then(|l| layer_id(state, l))
    {
        copy_style::page_body(ui, state, controls, id);
        return;
    }
    match (panel, layer) {
        (Panel::Brush, _) => brush_body(ui, state, controls),
        (Panel::Snap, _) => snap_body(ui, state, controls),
        (Panel::Layers, Some(LayerRef::Raster(id))) => match page {
            Some(Page::Ramps) => {
                raster::ramps_body(ui, state, controls, raster::RampUser::Raster(id))
            }
            Some(Page::RampLabels(i)) => raster::labels_body(ui, state, controls, i),
            Some(Page::RampEditor) => ramp_editor::body(ui, state, controls),
            Some(Page::Stops) => raster::stops_body(ui, state, controls, id, None),
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
            Some(Page::Rules) => rules::selection_body(ui, state, controls, id),
            Some(Page::Symbols(i)) => rules::symbols_body(ui, state, controls, id, i),
            Some(Page::MarkerShape(i)) => rules::shape_body(ui, state, controls, id, i),
            Some(Page::MarkerFill(i)) => rules::marker_fill_body(ui, state, controls, id, i),
            Some(Page::Classify) => classify::body(ui, state, controls, id),
            Some(Page::Ramps) => {
                raster::ramps_body(ui, state, controls, raster::RampUser::Classify(id))
            }
            Some(Page::RampLabels(i)) => raster::labels_body(ui, state, controls, i),
            Some(Page::RampEditor) => ramp_editor::body(ui, state, controls),
            Some(Page::Mask) => rules::mask_body(ui, state, controls, id),
            Some(Page::FlowHeight(_)) => rules::flow_height_body(ui, state, controls, id),
            Some(Page::DashPattern(rule)) => {
                rules::dash_pattern_body(ui, state, controls, id, rule);
            }
            Some(Page::LabelText) => fonts::text_body(ui, state, controls, id),
            Some(Page::Table) => {}
            _ => vector_body(ui, state, controls, id),
        },
        (Panel::Layers, None) if state.pad.manage.page.is_some() => {
            manage::body(ui, state, controls)
        }
        (Panel::Layers, None) => layers_body(ui, state, controls),
        (Panel::Toolbox, _) => toolbox::body(ui, state, controls),
        (Panel::Attributes, _) => select::body(ui, state, controls),
        _ => {}
    }
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
/// The spring of the offset of the sheet.
fn sheet_spring() -> egui::Id {
    egui::Id::new("pad-sheet-offset")
}

/// The offset of the sheet on the screen. It follows `offset` on a spring.
/// A sheet that the last frame did not show rises from the offset
/// `closed`: the floating card of the rows, or else its title row.
fn shown_sheet_offset(ctx: &egui::Context, offset: f32, closed: f32) -> f32 {
    let id = sheet_spring();
    let pass = ctx.cumulative_pass_nr();
    let seen: Option<u64> = ctx.data(|d| d.get_temp(id.with("seen")));
    ctx.data_mut(|d| d.insert_temp(id.with("seen"), pass));
    if seen.is_none_or(|p| p + 1 < pass) {
        motion::set(ctx, id, closed.max(offset));
    }
    motion::points(ctx, id, offset, motion::SNAPPY)
}

fn sheet_drag(ui: &egui::Ui, pad: &mut PadState, bar: &egui::Response, rects: &Rects) {
    let (rise, travel) = (rects.sheet_rise, rects.sheet_travel);
    if bar.dragged() {
        pad.sheet_offset = (pad.sheet_offset + bar.drag_delta().y).clamp(-rise, travel);
        motion::set(ui.ctx(), sheet_spring(), pad.sheet_offset);
        ui.ctx().request_repaint();
    }
    // The places of the sheet: the full height, the half height, and the
    // title row.
    let places = [-rise, 0.0, travel];
    let offset = pad.sheet_offset;
    pad.sheet_offset = if bar.drag_stopped() {
        let speed = ui.input(|i| i.pointer.velocity().y);
        motion::kick(ui.ctx(), sheet_spring(), offset, speed);
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

/// The hint of the Brush panel for a tool with no settings.
pub const NO_SETTINGS: &str = "This tool has no settings.";
/// The hint of the Brush panel for a tool whose only settings are in the
/// Snap panel.
pub const SNAP_HINT: &str = "Touch and hold Snap to change snap settings.";
/// The hint of the Snap panel with no active layer, or with an image layer.
pub const NO_SNAP_LAYER: &str = "Select a layer to choose snap targets.";
/// The hint of the Snap panel in the Atlas workspace.
pub const MAP_SNAP: &str = "In a map frame, Measure snaps to the layers of the map.";
/// The labels of the toggles of the snap targets of a layer.
pub const SNAP_VERTICES: &str = "Vertices";
pub const SNAP_EDGES: &str = "Edges";
/// The label of the toggle of the points of a point layer. It sets the
/// vertices of the targets.
pub const SNAP_POINTS: &str = "Points";
pub const SNAP_STOPS: &str = "Color stops";

/// The name of the toggle of a type of snap target, `label`, in the layer
/// `layer`.
pub fn snap_target_name(label: &str, layer: &str) -> String {
    format!("{label} of {layer}")
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).color(theme::MUTE));
}

/// A visible layer that can be a snap target.
pub(crate) struct SnapLayer {
    pub id: LayerId,
    pub name: String,
    pub icon: Icon,
    pub kind: SnapKind,
}

/// The kind of a layer that can be a snap target.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapKind {
    /// A line or a polygon layer: its vertices and its edges.
    Shapes,
    /// A point layer: its points, which are the vertices of a target.
    Points,
    /// A raster layer. Its color stops can be targets if it shows bands.
    Raster { bands: bool },
}

impl SnapKind {
    /// The labels of the toggles of the kind, each with the type that it sets.
    pub fn toggles(self) -> &'static [(&'static str, SnapType)] {
        match self {
            SnapKind::Shapes => &[
                (SNAP_VERTICES, SnapType::Vertices),
                (SNAP_EDGES, SnapType::Edges),
            ],
            SnapKind::Points => &[(SNAP_POINTS, SnapType::Vertices)],
            SnapKind::Raster { .. } => &[(SNAP_STOPS, SnapType::Stops)],
        }
    }

    /// The toggles can change: false for a raster that does not show bands.
    pub fn enabled(self) -> bool {
        self != SnapKind::Raster { bands: false }
    }
}

/// A type of snap target.
#[derive(Clone, Copy)]
pub(crate) enum SnapType {
    Vertices,
    Edges,
    Stops,
}

impl SnapType {
    pub fn of(self, types: &mut vale_store::SnapTypes) -> &mut bool {
        match self {
            SnapType::Vertices => &mut types.vertices,
            SnapType::Edges => &mut types.edges,
            SnapType::Stops => &mut types.stops,
        }
    }
}

/// The visible vector and raster layers that the globe draws: the active
/// layer first, then the others in the drawn order, from the bottom.
pub(crate) fn snap_layers(state: &AppState) -> Vec<SnapLayer> {
    let project = &state.doc.project;
    let mut layers: Vec<SnapLayer> = (project.drawn_layers().into_iter())
        .filter(|l| l.visible)
        .filter_map(|l| {
            let kind = match l.raster() {
                Some(id) => SnapKind::Raster {
                    bands: project.raster(id)?.display == vale_store::RasterDisplay::Bands,
                },
                None if l.geometry()? == GeometryKind::Point => SnapKind::Points,
                None => SnapKind::Shapes,
            };
            Some(SnapLayer {
                id: l.id,
                name: l.name.clone(),
                icon: layer_icon(l),
                kind,
            })
        })
        .collect();
    let active = state.active_layer_id();
    if let Some(at) = layers.iter().position(|l| Some(l.id) == active) {
        let first = layers.remove(at);
        layers.insert(0, first);
    }
    layers
}

/// The types of target `target` in a list of snap targets.
pub(crate) fn snap_types(
    targets: &[(LayerId, vale_store::SnapTypes)],
    target: LayerId,
) -> vale_store::SnapTypes {
    let none = vale_store::SnapTypes {
        vertices: false,
        edges: false,
        stops: false,
    };
    (targets.iter().find(|(id, _)| *id == target)).map_or(none, |(_, t)| *t)
}

/// The Snap panel: the Snapping switch, the snap distance, and a row for
/// each visible layer with the toggles of its types of snap target of the
/// active layer.
fn snap_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let snap = &mut state.globe.snap;
    widgets::switch_row(ui, controls, SNAPPING, SNAPPING, &mut snap.on);
    let number = Number::new(SNAP_DISTANCE, snap.distance, snap::DISTANCE_RANGE)
        .label("Distance")
        .step(1.0)
        .unit("pt")
        .default(snap::DEFAULT_DISTANCE);
    apply(number.slider(ui, controls), &mut snap.distance);
    if state.workspace == Workspace::Atlas {
        muted(ui, MAP_SNAP);
        return;
    }
    let Some(active) = state.active_layer_id() else {
        muted(ui, NO_SNAP_LAYER);
        return;
    };
    let layers = snap_layers(state);
    if !layers.is_empty() {
        rule(ui);
    }
    let targets = state.doc.project.snap_targets(active);
    // The toggles take the touches at the right of a row.
    let toggles = |l: &SnapLayer| {
        let pills = l.kind.toggles().iter();
        let width = |(label, _): &(&str, _)| widgets::pill_width(ui, label) + widgets::PILL_GAP;
        pills.map(width).sum::<f32>() + 8.0
    };
    let rows: Vec<list::Row<LayerId>> = (layers.iter())
        .map(|l| list::Row {
            trailing: toggles(l),
            ..list::Row::new(l.id, l.name.clone())
        })
        .collect();
    let mut changed = Vec::new();
    let mut list = list::ListState::default();
    list::show(
        ui,
        controls,
        &mut list,
        list::Spec::default(),
        &rows,
        |ui, controls, cell, i| {
            let layer = &layers[i];
            let mut types = snap_types(&targets, layer.id);
            let before = types;
            snap_row(ui, controls, cell, layer, layer.id == active, &mut types);
            if types != before {
                changed.push((layer.id, types));
            }
            None
        },
    );
    for (id, types) in changed {
        state.set_snap_target(active, id, types);
    }
}

/// The switch of the Brush panel for one finger that paints and draws.
pub const FINGER_PAINT: &str = "Paint with finger";

/// A row of the Snap panel: the icon of the kind of the layer, its name,
/// and at the right the toggles of its types of target. The `active` layer
/// has the accent color.
fn snap_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    cell: &list::Cell,
    layer: &SnapLayer,
    active: bool,
    types: &mut vale_store::SnapTypes,
) {
    let rect = cell.rect;
    let enabled = layer.kind.enabled();
    let mut right = rect.right_center();
    for (label, kind) in layer.kind.toggles().iter().rev() {
        let name = snap_target_name(label, &layer.name);
        let on = kind.of(types);
        let (left, _) = widgets::pill_toggle(ui, controls, right, &name, label, on, enabled);
        right.x = left - widgets::PILL_GAP;
    }
    let color = if active { theme::ACCENT } else { theme::TEXT };
    let painter = &cell.painter;
    let kind = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(18.0, 18.0));
    let icon = if active { theme::ACCENT } else { theme::MUTE };
    icons::paint(
        painter,
        kind,
        layer.icon,
        18.0,
        widgets::fade(icon, enabled),
    );
    let left = rect.left() + 28.0;
    let width = (right.x - 8.0 - left).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        layer.name.clone(),
        theme::body(),
        widgets::fade(color, enabled),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = painter.layout_job(job);
    let at = pos2(left, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, color);
}

/// The tool has settings in the Brush panel.
pub(super) fn brush_settings(state: &AppState) -> bool {
    match state.workspace {
        Workspace::Atlas => state.measuring(),
        Workspace::Globe => {
            (has_strip(&state.globe) || card_cells(&state.globe) > 0) && !state.globe.selecting()
        }
    }
}

fn brush_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    tool_rows(ui, state, controls);
    let globe = &state.globe;
    let draws = globe.brushing() || globe.stroking() || globe.placing();
    if state.workspace == Workspace::Globe && draws {
        let mut on = state.finger_paints();
        if widgets::switch_row(ui, controls, FINGER_PAINT, FINGER_PAINT, &mut on) {
            state.tools.finger_paints = Some(on);
        }
    }
}

/// The rows of the Brush panel for the settings of the tool.
fn tool_rows(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    if !brush_settings(state) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_SETTINGS).color(theme::MUTE));
        return;
    }
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
        if state.globe.sweeping() {
            let number = polygon_brush_size(state, POLYGON_BRUSH_PANEL_SIZE).label("Size");
            if let Some(value) = number.slider(ui, controls) {
                state.globe.line.brush_size = value as f32;
            }
            return;
        }
        if !(stroke || vertexing) {
            muted(ui, SNAP_HINT);
        }
        if line {
            let on = &mut state.globe.line.reshape;
            widgets::switch_row(ui, controls, RESHAPE_SWITCH, RESHAPE, on);
        }
        if state.globe.filling() {
            let line = &mut state.globe.line;
            let (on, enabled) = (&mut line.reshape_shared, line.reshape);
            let (name, label) = (SHARED_BORDERS_SWITCH, "Shared borders");
            widgets::switch_row_enabled(ui, controls, name, label, on, enabled);
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
    let mode = state.globe.brush.mode;
    if mode == Mode::OceanRidge {
        ridge_hint_rows(ui, &state.globe);
    }
    if mode == Mode::Margin {
        hint_rows(ui, panels::MARGIN_HINT);
    }
    if mode == Mode::Hotspot {
        hint_rows(ui, panels::HOTSPOT_HINT);
    }
    // The outline sets the size of a basin.
    if mode == Mode::Basin {
        basin_rows(ui, &state.globe);
    }
    let world_km = state.doc.project.world.radius_km;
    let radius_km = state.globe.brush.radius_km(state.globe.radius(), world_km);
    let row = |ui: &mut egui::Ui, controls: &mut Controls, state: &mut AppState, slider| {
        brush_slider(ui, controls, state, slider, &mut Place::Panel);
    };
    for slider in [Slider::Size, Slider::Hardness, Slider::Flow] {
        if strip::sliders(mode, true).contains(&slider) {
            row(ui, controls, state, slider);
        }
    }
    match mode {
        Mode::OceanRidge => {
            let brush = &mut state.globe.brush;
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
            row(ui, controls, state, Slider::Magma);
            row(ui, controls, state, Slider::Hills);
            row(ui, controls, state, Slider::Scale);
            let brush = &mut state.globe.brush;
            widgets::switch_row(ui, controls, FIT_TO_MAP, "Fit to map", &mut brush.ridge_fit);
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
        Mode::Warp => {
            row(ui, controls, state, Slider::Strength);
            row(ui, controls, state, Slider::Scale);
            seed_stepper(ui, controls, &mut state.doc.project.world.seed);
        }
        // The motion of the pen and the flow set the distance of a push.
        Mode::Push => {}
        Mode::Flatten => row(ui, controls, state, Slider::Rate),
        Mode::AbyssalHills => {
            row(ui, controls, state, Slider::Rate);
            row(ui, controls, state, Slider::Hills);
            row(ui, controls, state, Slider::Scale);
            let fit = &mut state.globe.brush.abyssal_fit;
            widgets::switch_row(ui, controls, FIT_TO_MAP, "Fit to map", fit);
            seed_stepper(ui, controls, &mut state.doc.project.world.seed);
        }
        Mode::Hotspot => {
            row(ui, controls, state, Slider::Strength);
            let brush = &mut state.globe.brush;
            let number = Number::new(HOTSPOT_SPEED_NAME, brush.hotspot_speed_mm, HOTSPOT_SPEED)
                .label("Plate speed")
                .step(1.0)
                .default(BrushSettings::default().hotspot_speed_mm)
                .unit("mm/yr");
            apply(number.stepper(ui, controls), &mut brush.hotspot_speed_mm);
            raster::note(ui, &panels::hotspot_age_text(brush));
            let reefs = &mut brush.hotspot_reefs;
            widgets::switch_row(ui, controls, HOTSPOT_REEFS, "Reefs", reefs);
            row(ui, controls, state, Slider::Age);
            seed_stepper(ui, controls, &mut state.doc.project.world.seed);
        }
        Mode::Margin => {
            let brush = &mut state.globe.brush;
            let number = Number::new(MARGIN_SHELF, brush.margin_shelf_m, MARGIN_SHELF_M)
                .label("Shelf depth")
                .step(10.0)
                .unit("m");
            apply(number.stepper(ui, controls), &mut brush.margin_shelf_m);
            seed_stepper(ui, controls, &mut state.doc.project.world.seed);
        }
        Mode::Paint => {
            row(ui, controls, state, Slider::Opacity);
            row(ui, controls, state, Slider::Value);
        }
        Mode::Noise => {
            for slider in [Slider::Raise, Slider::Lower, Slider::Scale, Slider::Grain] {
                row(ui, controls, state, slider);
            }
            let brush = &mut state.globe.brush;
            let ridged = &mut brush.noise_ridged;
            widgets::switch_row(ui, controls, "Brush ridged", "Ridged", ridged);
            if brush.noise_ridged {
                let inverted = &mut brush.noise_inverted;
                widgets::switch_row(ui, controls, "Brush invert", "Invert", inverted);
            }
            // The relief follows the height above sea level.
            if state.globe.is_height() {
                row(ui, controls, state, Slider::Relief);
            }
            seed_stepper(ui, controls, &mut state.doc.project.world.seed);
        }
        Mode::Terrace => row(ui, controls, state, Slider::Step),
        Mode::Shape => row(ui, controls, state, Slider::Shape),
        mode if strip::sliders(mode, true).contains(&Slider::Strength) => {
            row(ui, controls, state, Slider::Strength);
        }
        _ => {}
    }
    if mode == Mode::Range {
        row(ui, controls, state, Slider::Spacing);
        seed_stepper(ui, controls, &mut state.doc.project.world.seed);
    }
    if mode == Mode::Seamounts {
        row(ui, controls, state, Slider::Density);
        raster::note(ui, SEAMOUNT_DENSITY_NOTE);
        row(ui, controls, state, Slider::Age);
        seed_stepper(ui, controls, &mut state.doc.project.world.seed);
    }
    if mode == Mode::Plateau {
        row(ui, controls, state, Slider::Age);
        seed_stepper(ui, controls, &mut state.doc.project.world.seed);
    }
    if mode == Mode::Basin {
        for slider in [Slider::Density, Slider::Age, Slider::Lakes] {
            row(ui, controls, state, slider);
        }
        seed_stepper(ui, controls, &mut state.doc.project.world.seed);
    }
    let brush = &mut state.globe.brush;
    if matches!(mode, Mode::Carve | Mode::Erode) {
        let name = "Brush channel width";
        let number = Number::new(name, brush.channel_km, CHANNEL_KM)
            .label("Channel width")
            .unit("km")
            .nice();
        apply(number.stepper(ui, controls), &mut brush.channel_km);
    }
    if mode == Mode::Carve {
        let value = brush.floodplain_km;
        let number = Number::new("Brush floodplain", value, FLOODPLAIN_WIDTH_KM)
            .label("Floodplain")
            .unit("km")
            .step(1.0);
        apply(number.stepper(ui, controls), &mut brush.floodplain_km);
    }
    if matches!(
        mode,
        Mode::Uplift | Mode::Range | Mode::Basin | Mode::Margin | Mode::Hotspot
    ) {
        row(ui, controls, state, Slider::Erosion);
    }
    if matches!(
        mode,
        Mode::Erode | Mode::Uplift | Mode::Margin | Mode::Hotspot
    ) {
        row(ui, controls, state, Slider::Branching);
    }
    if matches!(mode, Mode::Erode | Mode::Uplift | Mode::Range | Mode::Basin) {
        let world = &mut state.doc.project.world;
        let mut degrees = world.wind_deg as f32;
        if widgets::compass_row(ui, controls, "Brush wind", "Wind", &mut degrees) {
            world.wind_deg = f64::from(degrees.round());
        }
        row(ui, controls, state, Slider::RainShadow);
    }
    let brush = &mut state.globe.brush;
    if matches!(mode, Mode::Weather | Mode::Uplift) {
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

/// Where `brush_slider` adds a slider: as a row of the Brush panel with a
/// pin, or in the strip with the control that the closure adds.
pub(super) enum Place<'f> {
    Panel,
    Strip(&'f mut dyn for<'n> FnMut(Number<'n>, &mut egui::Ui, &mut Controls) -> Option<f64>),
}

/// The name of the switch of the ocean ridge mode and of the abyssal hills
/// mode that makes the hills at least as large as the map needs.
pub const FIT_TO_MAP: &str = "Brush fit to map";
/// The note under the density of the seamounts mode.
pub const SEAMOUNT_DENSITY_NOTE: &str = "Density per 10,000 km²";

/// The name of the button in the header of the Brush panel that gives the
/// mode its default pins.
pub const RESET_PINS: &str = "Reset pins";

/// The name of a slider of the Brush panel.
pub fn slider_name(slider: Slider) -> &'static str {
    match slider {
        Slider::Size => "Brush size",
        Slider::Hardness => "Brush hardness",
        Slider::Flow => "Brush flow",
        Slider::Strength => "Brush strength",
        Slider::Scale => "Brush scale",
        Slider::Rate => "Brush rate",
        Slider::Opacity => "Brush opacity",
        Slider::Value => PAINT_VALUE,
        Slider::Raise => "Brush raise",
        Slider::Lower => "Brush lower",
        Slider::Grain => "Brush grain",
        Slider::Relief => "Brush relief",
        Slider::Step => "Brush step",
        Slider::Shape => "Brush shape",
        Slider::Spacing => "Brush spacing",
        Slider::Density => "Brush density",
        Slider::Age => "Brush age",
        Slider::Lakes => "Brush lakes",
        Slider::Erosion => "Brush erosion",
        Slider::Branching => "Brush branching",
        Slider::RainShadow => "Brush rain shadow",
        Slider::Magma => "Brush magma",
        Slider::Hills => "Brush hills",
    }
}

/// The label of a slider of the brush in `mode`.
pub fn slider_label(mode: Mode, slider: Slider) -> &'static str {
    match slider {
        Slider::Size => "Size",
        Slider::Hardness => "Hardness",
        Slider::Flow => "Flow",
        Slider::Strength => panels::strength_name(mode),
        Slider::Scale => "Scale",
        Slider::Rate => "Rate",
        Slider::Opacity => "Opacity",
        Slider::Value => "Value",
        Slider::Raise => "Raise",
        Slider::Lower => "Lower",
        Slider::Grain => "Grain",
        Slider::Relief => "Relief",
        Slider::Step => "Step",
        Slider::Shape => "Shape",
        Slider::Spacing => "Spacing",
        Slider::Density => "Density",
        Slider::Age => "Age",
        Slider::Lakes => "Lakes",
        Slider::Erosion => "Erosion",
        Slider::Branching => "Branching",
        Slider::RainShadow => "Rain shadow",
        Slider::Magma => "Magma",
        Slider::Hills => "Hills",
    }
}

/// The name of a strip slider of the brush in `mode`.
pub fn strip_name(mode: Mode, slider: Slider) -> String {
    format!("Strip {}", slider_label(mode, slider).to_lowercase())
}

/// Adds a slider of the brush at `place`, and sets the setting that it
/// changes. A tap on the label of a slider of the panel pins it to the strip
/// or unpins it.
pub(super) fn brush_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &mut AppState,
    slider: Slider,
    place: &mut Place<'_>,
) {
    let mode = state.globe.brush.mode;
    let label = slider_label(mode, slider);
    let strip_name = strip_name(mode, slider);
    let name = match place {
        Place::Panel => slider_name(slider),
        Place::Strip(_) => strip_name.as_str(),
    };
    let pinned = state.tools.pins.is_pinned(mode, slider);
    let mut toggled = false;
    let mut show = |number: Number<'_>, ui: &mut egui::Ui, controls: &mut Controls| match place {
        Place::Panel => {
            let (value, tap) = number.pin(pinned).slider_pin(ui, controls);
            toggled |= tap;
            value
        }
        Place::Strip(control) => control(number, ui, controls),
    };
    if slider == Slider::Size {
        brush_size(state, name, |number| show(number, ui, controls));
    } else {
        let unit = state.shown_unit().to_string();
        let defaults = BrushSettings::default();
        let brush = &mut state.globe.brush;
        let world = &mut state.doc.project.world;
        // A height in meters as a value of the layer.
        let factor = brush.meters_value(1.0);
        let span = brush.value_range.1 - brush.value_range.0;
        let layer_text = |meters: f64| world::value_text(meters * factor, span, &unit);
        let meters = |value, range| {
            Number::new(name, value, range)
                .label(label)
                .shown(factor, &unit)
                .text(layer_text)
        };
        let km = |value, range| Number::new(name, value, range).label(label).unit("km");
        let fraction = |value, range| Number::new(name, value, range).label(label).percent();
        let mut set =
            |number: Number<'_>, value: &mut f64| apply(show(number, ui, controls), value);
        match slider {
            Slider::Size => {}
            Slider::Hardness => {
                let number = fraction(brush.hardness, 0.0..=1.0).default(defaults.hardness);
                set(number, &mut brush.hardness);
            }
            Slider::Flow => set(fraction(brush.flow, FLOW), &mut brush.flow),
            Slider::Strength if mode == Mode::Warp => {
                let number = km(brush.warp_km, WARP_KM).log().default(defaults.warp_km);
                set(number, &mut brush.warp_km);
            }
            Slider::Strength if mode == Mode::Hotspot => {
                let number = meters(brush.hotspot_height_m, HOTSPOT_HEIGHT)
                    .log()
                    .default(defaults.hotspot_height_m);
                set(number, &mut brush.hotspot_height_m);
            }
            Slider::Strength if mode == Mode::Plateau => {
                let number = meters(brush.plateau_height_m, PLATEAU_HEIGHT)
                    .log()
                    .default(defaults.plateau_height_m);
                set(number, &mut brush.plateau_height_m);
            }
            Slider::Strength if mode == Mode::Seamounts => {
                let number = meters(brush.seamount_height_m, SEAMOUNT_HEIGHT_M)
                    .log()
                    .default(defaults.seamount_height_m);
                set(number, &mut brush.seamount_height_m);
            }
            Slider::Strength => {
                let number = meters(brush.strength_m, STRENGTH_M)
                    .log()
                    .default(defaults.strength_m);
                set(number, &mut brush.strength_m);
            }
            Slider::Scale if matches!(mode, Mode::OceanRidge | Mode::AbyssalHills) => {
                let value = match mode {
                    Mode::OceanRidge => &mut brush.ridge_scale,
                    _ => &mut brush.abyssal_scale,
                };
                let number = fraction(*value, HILL_SCALE).log().default(1.0);
                set(number, value);
            }
            Slider::Scale => {
                let number = km(brush.scale_km, SCALE_KM)
                    .log()
                    .default(defaults.scale_km);
                set(number, &mut brush.scale_km);
            }
            Slider::Rate if mode == Mode::AbyssalHills => {
                let number = Number::new(name, brush.abyssal_rate_mm, ABYSSAL_RATE_MM)
                    .label(label)
                    .unit("mm/yr")
                    .step(1.0)
                    .default(defaults.abyssal_rate_mm);
                set(number, &mut brush.abyssal_rate_mm);
            }
            Slider::Rate => {
                let number =
                    fraction(brush.flatten_rate, FLATTEN_RATE).default(defaults.flatten_rate);
                set(number, &mut brush.flatten_rate);
            }
            Slider::Opacity => {
                let number = fraction(brush.opacity, 0.0..=1.0).default(defaults.opacity);
                set(number, &mut brush.opacity);
            }
            Slider::Value => {
                let value = brush.value(brush.paint_level);
                let number = level(name, value, brush.value_range, &unit).label(label);
                if let Some(value) = show(number, ui, controls) {
                    brush.paint_level = brush.level(value);
                }
            }
            Slider::Raise => {
                let number = meters(brush.noise_raise_m, NOISE_M)
                    .scale(Scale::Square)
                    .default(defaults.noise_raise_m);
                set(number, &mut brush.noise_raise_m);
            }
            Slider::Lower => {
                let number = meters(brush.noise_lower_m, NOISE_M)
                    .scale(Scale::Square)
                    .default(defaults.noise_lower_m);
                set(number, &mut brush.noise_lower_m);
            }
            Slider::Grain => {
                let number = Number::new(name, brush.noise_grain, NOISE_GRAIN)
                    .label(label)
                    .text(|grain| format!("{:.1}×", 1.0 + grain));
                set(number, &mut brush.noise_grain);
            }
            Slider::Relief => {
                let number = fraction(brush.noise_relief, 0.0..=1.0).default(defaults.noise_relief);
                set(number, &mut brush.noise_relief);
            }
            Slider::Step => {
                let number = meters(brush.terrace_step_m, TERRACE_STEP_M)
                    .log()
                    .default(defaults.terrace_step_m);
                set(number, &mut brush.terrace_step_m);
            }
            Slider::Shape => {
                let number = Number::new(name, brush.shape, SHAPE)
                    .label(label)
                    .text(shape_text)
                    .default(defaults.shape);
                set(number, &mut brush.shape);
            }
            Slider::Spacing => {
                let number = km(brush.range_spacing_km, RANGE_SPACING_KM)
                    .log()
                    .default(defaults.range_spacing_km);
                set(number, &mut brush.range_spacing_km);
            }
            Slider::Density if mode == Mode::Seamounts => {
                let number = Number::new(name, brush.seamount_density, SEAMOUNT_DENSITY)
                    .label(label)
                    .text(|density| format!("{density:.0}"))
                    .log()
                    .default(defaults.seamount_density);
                set(number, &mut brush.seamount_density);
            }
            Slider::Density => {
                let number = km(brush.basin_density_km, BASIN_DENSITY)
                    .log()
                    .default(defaults.basin_density_km);
                set(number, &mut brush.basin_density_km);
            }
            Slider::Age if matches!(mode, Mode::Seamounts | Mode::Hotspot | Mode::Plateau) => {
                let (age, default) = match mode {
                    Mode::Hotspot => (&mut brush.hotspot_age_myr, defaults.hotspot_age_myr),
                    Mode::Plateau => (&mut brush.plateau_age_myr, defaults.plateau_age_myr),
                    _ => (&mut brush.seamount_age_myr, defaults.seamount_age_myr),
                };
                let number = Number::new(name, *age, FLOOR_AGE_MYR)
                    .label(label)
                    .unit("Myr")
                    .text(|age| format!("{age:.0} Myr"))
                    .default(default);
                set(number, age);
            }
            Slider::Age => {
                let number = fraction(brush.basin_age, BASIN_AGE).default(defaults.basin_age);
                set(number, &mut brush.basin_age);
            }
            Slider::Lakes => {
                let number = fraction(brush.basin_lakes, BASIN_LAKES).default(defaults.basin_lakes);
                set(number, &mut brush.basin_lakes);
            }
            Slider::Erosion => {
                let number = fraction(brush.erosion, EROSION).default(defaults.erosion);
                set(number, &mut brush.erosion);
            }
            Slider::Branching => {
                let number = fraction(brush.branching, BRANCHING).default(defaults.branching);
                set(number, &mut brush.branching);
            }
            Slider::RainShadow => {
                let number = fraction(world.rain_shadow.clamp(0.0, 1.0), 0.0..=1.0);
                set(number, &mut world.rain_shadow);
            }
            Slider::Magma => {
                let number = fraction(brush.ridge_magma, RIDGE_MAGMA)
                    .text(panels::signed_percent)
                    .default(0.0);
                set(number, &mut brush.ridge_magma);
            }
            Slider::Hills if mode == Mode::AbyssalHills => {
                let number =
                    fraction(brush.abyssal_hills, RIDGE_HILLS).default(defaults.abyssal_hills);
                set(number, &mut brush.abyssal_hills);
            }
            Slider::Hills => {
                let number = fraction(brush.ridge_hills, RIDGE_HILLS).default(defaults.ridge_hills);
                set(number, &mut brush.ridge_hills);
            }
        }
    }
    if toggled {
        state.tools.pins.toggle(mode, slider);
    }
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

/// The name of the switch that keeps the brush size on the ground.
pub const LOCK_SIZE: &str = "Lock size";
/// The name of the stepper of the level of the flatten mode.
pub const FLATTEN_LEVEL: &str = "Flatten level";
/// The name of the button that sets the flatten level at each stroke start.
pub const STROKE_START: &str = "Use stroke start";
/// The name of the switch that takes the flatten level from the globe.
pub const PICK_LEVEL: &str = "Pick from globe";
/// The name of the slider of the value of the paint mode.
pub const PAINT_VALUE: &str = "Brush value";

/// The lock of the size, and the level of the flatten mode.
fn flatten_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    globe: &mut crate::globe::Globe,
    unit: &str,
) {
    let globe_radius = globe.radius();
    let mut lock = globe.brush.lock.is_some();
    if !globe.brush.size_is_fixed()
        && widgets::switch_row(ui, controls, LOCK_SIZE, LOCK_SIZE, &mut lock)
    {
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

/// A row with a hint.
fn hint_rows(ui: &mut egui::Ui, hint: &str) {
    ui.add_space(8.0);
    let text = egui::RichText::new(hint).color(theme::MUTE);
    ui.add(egui::Label::new(text).selectable(false));
    ui.add_space(8.0);
}

/// The name of the shelf depth stepper of the margin mode.
pub const MARGIN_SHELF: &str = "Brush shelf depth";
/// The name of the plate speed stepper of the hotspot track mode.
pub const HOTSPOT_SPEED_NAME: &str = "Brush plate speed";
/// The name of the reefs switch of the hotspot track mode.
pub const HOTSPOT_REEFS: &str = "Brush reefs";

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

/// The list of the layers of each kind, in draw order. The first row is the
/// bottom of the drawing. A tap on a row selects the layer, and the eye
/// shows or hides it. The list has each gesture of [`list`]. Below the list
/// is the text of the import of an image layer that runs or that failed,
/// or of a GeoJSON import that failed.
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

/// The ID of the layer of a panel.
fn layer_id(state: &AppState, layer: LayerRef) -> Option<LayerId> {
    let project = &state.doc.project;
    match layer {
        LayerRef::Vector(id) => Some(id),
        LayerRef::Raster(id) => project.raster_layer(id).map(|l| l.id),
        LayerRef::Image(id) => project.image_layer(id).map(|l| l.id),
    }
}

fn layer_list(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    if state.doc.project.layers().is_empty() {
        state.pad.cards = collection::Cards::default();
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_LAYERS).color(theme::MUTE));
        return;
    }
    collection::cards(ui, state, controls, collection::View::Globe);
}

/// The name of the header button that deletes the selected layers.
pub const DELETE_LAYERS: &str = "Delete layers";

/// Does the action of a header button of the selected layers. "Select
/// all" selects each row that takes touch, in each card.
fn layer_group(state: &mut AppState, name: &'static str) {
    let picked = state.pad.cards.picked();
    let live = collection::live_rows(&state.doc.project);
    let cards = &mut state.pad.cards;
    match name {
        list::SELECT_ALL => {
            for (id, rows) in live {
                cards.lists.entry(id).or_default().picked = rows;
            }
        }
        list::DESELECT_ALL => cards.lists.values_mut().for_each(|l| l.picked.clear()),
        DELETE_LAYERS => {
            cards.lists.clear();
            state.remove_layers(&picked);
        }
        _ => {}
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
    let mut size = if state.pad.add_picking {
        let picker = collection_choice::picker_height(&state.doc.project);
        let height = 2.0 * widgets::menu_inset().y + ROW + picker;
        let room = (rects.panel_bottom - rects.screen.top() - rects.bands.top).max(ROW * 3.0);
        vec2(PICK_WIDTH, height.min(room))
    } else {
        let rows = 4 + NEW_LAYERS.len() + usize::from(import);
        let height = 2.0 * widgets::menu_inset().y + 2.0 * MENU_HEADING + rows as f32 * PANEL_ROW;
        vec2(ADD_WIDTH, height)
    };
    let pane = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let beside = pos2(
        rects.dock.left() - geometry::PANEL_GAP - size.x,
        pane.map_or(rects.dock.top(), |(_, rect)| rect.top()),
    );
    let min = if rects.sheet.is_none() && beside.x >= rects.screen.left() + geometry::MARGIN {
        beside
    } else if rects.class == WidthClass::Compact {
        // On a compact screen, the menu is a popover of its button.
        let (place, _) = popover::place(button, size, popover::room(ctx, button));
        size = place.size();
        place.min
    } else {
        // A short screen moves the menu up, out of the bottom band. A menu
        // that then covers its button goes to the left of the button.
        let below = button.bottom() + PAD;
        let top = (rects.panel_bottom - size.y).min(below);
        let top = top.max(rects.screen.top() + rects.bands.top);
        let right = if top < below {
            button.left() - PAD
        } else {
            button.right()
        };
        let left = (right - size.x).max(rects.screen.left() + geometry::PANEL_GAP);
        // A screen that is still too short scrolls the rows.
        size.y = size.y.min(rects.panel_bottom - top);
        pos2(left, top)
    };
    let rect = geometry::off_fold(
        Rect::from_min_size(min, size),
        button,
        rects.fold,
        rects.screen,
    );
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
    let backdrop = state.globe.backdrop();
    widgets::card_from(
        ctx,
        backdrop,
        "pad-add-layer",
        rect,
        Order::Foreground,
        radius,
        button.center().clamp(rect.min, rect.max),
        |ui| {
            let inner = rect.shrink2(widgets::menu_inset());
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if state.pad.add_picking {
                add_menu_picker(ui, state, controls);
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("pad-add-layer-rows")
                .show(ui, |ui| add_menu_rows(ui, state, controls, import));
        },
    );
}

/// The rows of the menu of the new layers: the Collection row, the rows of
/// the new layers, and the rows of the imports.
fn add_menu_rows(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, import: bool) {
    let choice = state.pad.add_collection.clone();
    if collection_choice::row(ui, state, controls, choice.as_ref()) {
        state.pad.add_picking = true;
    }
    let valid = state.collection_choice_is_valid(choice.as_ref());
    menu_heading(ui, NEW_SECTION);
    for (name, kind) in NEW_LAYERS {
        if widgets::text_row(ui, controls, name, 0.0, false, valid).clicked() {
            state.add_vector_layer(kind, choice.as_ref());
            state.pad.add = false;
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
        let mut form = RasterForm::open(state);
        form.collection = choice.clone();
        state.raster_form = Some(form);
        state.pad.add = false;
    }
    menu_heading(ui, IMPORT_SECTION);
    let free = state.image_import.is_none() && valid;
    if widgets::text_row(ui, controls, NEW_IMAGE, 0.0, false, free).clicked() {
        state.import_collection = choice.clone();
        state.actions.push(Action::ImageLayerDialog);
        state.pad.add = false;
    }
    if widgets::text_row(ui, controls, NEW_GEOJSON, 0.0, false, valid).clicked() {
        state.import_collection = choice.clone();
        state.actions.push(Action::GeoJsonDialog);
        state.pad.add = false;
    }
    if import {
        let enabled = !state.globe.busy() && valid;
        let row = widgets::text_row(ui, controls, NEW_HEIGHTMAP, 0.0, false, enabled);
        if row.clicked() {
            state.import_collection = choice.clone();
            state.actions.push(Action::ImportHeightmapDialog);
            state.pad.add = false;
        }
    }
}

/// The picker of the Collection row of the menu of the new layers. The
/// list scrolls if the menu is too short for it.
fn add_menu_picker(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let title = widgets::title_row(
        ui,
        controls,
        collection_choice::COLLECTION,
        Some(ADD_LAYER),
        false,
    );
    if title.close {
        state.pad.add = false;
    }
    let default = state.default_layer_collection();
    let mut picked = false;
    egui::ScrollArea::vertical()
        .id_salt("pad-add-collection")
        .show(ui, |ui| {
            let pad = &mut state.pad;
            picked = collection_choice::picker(
                ui,
                &state.doc.project,
                default,
                controls,
                &mut pad.collection_picker,
                &mut pad.add_collection,
            );
        });
    if title.back || picked {
        state.pad.add_picking = false;
    }
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
    collection::open_row(ui, state, controls, id);
    if crate::ui::budget::too_large(ui.ctx(), id) {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(crate::ui::budget::TOO_LARGE_ABOUT).color(theme::WARN));
        ui.add_space(8.0);
    }
    if state.is_stale_output(id) {
        use crate::ui::polygonize::{RUN_AGAIN, STALE_LAYER};
        ui.add_space(8.0);
        ui.label(egui::RichText::new(STALE_LAYER).color(theme::MUTE));
        ui.add_space(8.0);
        if widgets::button_row(ui, controls, RUN_AGAIN, state.can_polygonize()).clicked() {
            let old = state.doc.layer(id).map(|l| l.name.clone());
            state.run_polygonize_again();
            // The panel shows the new layer of the same name.
            let outputs = state.polygonize_outputs();
            let named = |o: &&LayerId| state.doc.layer(**o).map(|l| l.name.clone()) == old;
            let same = outputs.iter().find(named);
            if let Some(&new) = same.or(outputs.last()) {
                state.pad.layer = Some(LayerRef::Vector(new));
            }
        }
        ui.add_space(8.0);
    }
    if widgets::open_row(ui, controls, table::FEATURES, &features).clicked() {
        state.open_table(id);
    }
    let current = vale_style::scale_denominator(
        state.doc.project.world.radius_km * 1000.0 / state.globe.radius(),
    );
    rules::section(ui, state, controls, id, current);
    copy_style::open_row(ui, state, controls);
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
    let layer = layer.map(|layer| layer.id);
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
    if let Some(layer) = layer {
        collection::open_row(ui, state, controls, layer);
    }
    let number = percent(OPACITY, f64::from(opacity)).label("Opacity");
    if let Some(opacity) = number.slider(ui, controls) {
        state.set_raster_opacity(id, opacity as f32);
    }
    if height {
        import_row(ui, state, controls, Some(id));
    }
    export_row(ui, state, controls, id);
    raster::style(ui, state, controls, id, None);
    copy_style::open_row(ui, state, controls);
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
    collection::open_row(ui, state, controls, layer);
    let number = percent(OPACITY, f64::from(opacity)).label("Opacity");
    if let Some(opacity) = number.slider(ui, controls) {
        state.set_image_opacity(id, opacity as f32);
    }
    let label = super::elevation::blend_label(blend);
    if widgets::open_row(ui, controls, raster::BLEND, label).clicked() {
        state.pad.page = Some(Page::Blend);
    }
    copy_style::open_row(ui, state, controls);
    image_note(ui, state, Some(id));
}

/// The width of a button of the card of a held stroke, and the least width
/// of its state.
const HELD_BUTTON: f32 = 120.0;
const HELD_STATE: f32 = 132.0;

/// What the card of a basin or an ocean ridge shows.
#[derive(Clone, Copy, PartialEq)]
enum HeldCard {
    /// The outline of the next basin waits for its first river.
    Outline,
    /// A stroke is held, and the flag is true while it follows the settings.
    Held(bool),
}

/// The card of a held ocean ridge or basin, at the bottom of the canvas
/// above the readout: its state, and the Discard and Apply buttons. A held
/// basin with 2 rivers or more also has the Undo river button. The outline
/// of the next basin shows the hint of its first river and the Clear
/// button. An open panel pushes the card to its right. On a narrow screen,
/// the card leaves out the text of the state, and the buttons get narrower.
fn held_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let globe = &state.globe;
    let outline = globe.brushing() && globe.brush.mode == Mode::Basin;
    let card = match globe.holding() {
        Some(_) => HeldCard::Held(globe.held_updating()),
        None if outline && globe.basin_outline().is_some() => HeldCard::Outline,
        None => return,
    };
    let mut buttons = vec![(panels::DISCARD, theme::WARN)];
    if globe.basin_rivers() >= 2 {
        buttons.push((panels::UNDO_RIVER, theme::ACCENT));
    }
    buttons.push((panels::APPLY, theme::ACCENT));
    let text = match card {
        HeldCard::Outline => {
            buttons = vec![(panels::CLEAR_OUTLINE, theme::WARN)];
            panels::BASIN_RIVER_HINT
        }
        HeldCard::Held(true) => panels::UPDATING,
        HeldCard::Held(false) => panels::READY,
    };
    let spinner = card == HeldCard::Held(true);
    let lead = if spinner { 24.0 } else { 0.0 };
    let text_width = ctx.fonts_mut(|fonts| {
        let galley = fonts.layout_no_wrap(text.to_string(), theme::body(), theme::MUTE);
        galley.size().x
    });
    let mut slot = HELD_STATE.max(lead + text_width.ceil() + 2.0 * BODY_PAD);
    let count = buttons.len() as f32;
    let room = rects.screen.width() - 2.0 * geometry::GAP;
    let width = |slot: f32, button: f32| slot + count * button + (count + 1.0) * PAD;
    let mut text = Some(text);
    if width(slot, HELD_BUTTON) > room {
        text = None;
        slot = if spinner { lead + BODY_PAD } else { 0.0 };
    }
    let button = HELD_BUTTON.min((room - width(slot, 0.0)) / count).floor();
    let size = vec2(width(slot, button), PANEL_ROW + 2.0 * PAD);
    let mut bottom = rects.screen.bottom() - rects.bands.bottom - geometry::GAP;
    for top in [rects.readout, rects.sheet, rects.bar]
        .into_iter()
        .flatten()
        .map(|r| r.top())
    {
        bottom = bottom.min(top - geometry::GAP);
    }
    let middle = geometry::canvas_part(rects.screen, rects.fold).center().x;
    let mut rect = Rect::from_min_size(pos2(middle - size.x / 2.0, bottom - size.y), size);
    let panels = &state.pad.panels;
    if let Some((_, panel)) = panels.iter().find(|(_, panel)| panel.intersects(rect)) {
        rect = rect.translate(vec2(panel.right() + geometry::GAP - rect.left(), 0.0));
    }
    let mut clicked = None;
    // The card grows from the place where the pen left the canvas.
    let hold = ctx
        .input(|i| i.pointer.latest_pos())
        .unwrap_or(rect.center_top());
    widgets::card_from(
        ctx,
        state.globe.backdrop(),
        "pad-held",
        rect,
        Order::Middle,
        theme::CARD_RADIUS.into(),
        hold.clamp(rect.min, rect.max),
        |ui| {
            let middle = rect.center().y;
            let left = rect.left() + BODY_PAD;
            // A spinner shows while the stroke follows the settings.
            if spinner {
                let spin = Rect::from_center_size(pos2(left + 8.0, middle), vec2(16.0, 16.0));
                ui.put(spin, egui::Spinner::new().size(16.0).color(theme::MUTE));
            }
            if let Some(text) = text {
                ui.painter().text(
                    pos2(left + lead, middle),
                    egui::Align2::LEFT_CENTER,
                    text,
                    theme::body(),
                    theme::MUTE,
                );
            }
            for (i, &(name, fill)) in buttons.iter().enumerate() {
                let x = rect.left() + slot + PAD + i as f32 * (button + PAD);
                let place = Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(button, PANEL_ROW));
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
        Some(panels::UNDO_RIVER) => globe.undo_river(),
        _ => {}
    }
}

/// The workspace menu of the compact layout: the two workspaces, the Worlds
/// row, and the Export row.
fn menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let height = 2.0 * widgets::menu_inset().y + 4.0 * PANEL_ROW + 3.0 * 13.0;
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
    let backdrop = state.globe.backdrop();
    widgets::card_from(
        ctx,
        backdrop,
        "pad-menu",
        rect,
        Order::Foreground,
        radius,
        rects.workspace.center().clamp(rect.min, rect.max),
        |ui| {
            let inner = rect.shrink2(widgets::menu_inset());
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if widgets::text_row(ui, controls, WORKSPACES[0], 0.0, true, true).clicked() {
                state.pad.menu = false;
            }
            rule(ui);
            if widgets::text_row(ui, controls, WORKSPACES[1], 0.0, false, true).clicked() {
                state.workspace = Workspace::Atlas;
                state.pad.menu = false;
            }
            rule(ui);
            if widgets::text_row(ui, controls, WORLDS_ROW, 0.0, false, true).clicked() {
                state.pad.menu = false;
                toggle_worlds(state);
            }
            rule(ui);
            if export_form_row(ui, state, controls) {
                state.pad.menu = false;
            }
        },
    );
}

/// The row of a menu that opens the export form, with the share icon.
/// Returns true if the user tapped it.
pub(super) fn export_form_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
) -> bool {
    let row = widgets::text_row(ui, controls, export_form::EXPORT, 0.0, false, true);
    let icon = Rect::from_center_size(
        pos2(row.rect.right() - 12.0, row.rect.center().y),
        vec2(20.0, 20.0),
    );
    icons::paint(ui.painter(), icon, Icon::Share, 20.0, theme::MUTE);
    if row.clicked() {
        state.export_form = Some(export_form::ExportForm::new(state));
    }
    row.clicked()
}

/// A line between two groups of rows of a menu.
fn rule(ui: &mut egui::Ui) {
    let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), egui::Sense::hover());
    let line = Rect::from_center_size(rule.center(), vec2(rule.width(), 1.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
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
    let Some(worlds) = state.pad.worlds.take() else {
        return;
    };
    let shown = worlds.len().min(WORLD_ROWS) as f32 * PANEL_ROW;
    let rule_height = if worlds.is_empty() { 0.0 } else { 13.0 };
    let width = geometry::WORLDS_WIDTH.min(rects.screen.width() - 2.0 * geometry::MARGIN);
    // The row of the start screen needs a folder of projects, and the Save
    // row needs a project file.
    let rows = 2.0 + f32::from(state.projects.any()) + f32::from(state.session.is_some());
    // The Export row is last, after a divider. On a compact screen, it is in
    // the workspace menu.
    let export = rects.class != WidthClass::Compact;
    let (rows, rule_height) = match export {
        true => (rows + 1.0, rule_height + 13.0),
        false => (rows, rule_height),
    };
    let size = vec2(
        width,
        2.0 * widgets::menu_inset().y + shown + rule_height + rows * PANEL_ROW,
    );
    let rect = Rect::from_min_size(rects.menu, size);
    let room = Rect::from_x_y_ranges(rects.screen.x_range(), rects.menu.y..=rects.panel_bottom);
    let rect = geometry::off_fold(rect, rects.workspace, rects.fold, room);
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
    let backdrop = state.globe.backdrop();
    widgets::card_from(
        ctx,
        backdrop,
        "pad-worlds",
        rect,
        Order::Foreground,
        radius,
        rects.workspace.center().clamp(rect.min, rect.max),
        |ui| {
            let inner = rect.shrink2(widgets::menu_inset());
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let ui = &mut ui;
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let mut rows = |ui: &mut egui::Ui| {
                let rows: Vec<list::Row<usize>> = (worlds.iter().enumerate())
                    .map(|(i, world)| list::Row::new(i, row_name(&world.name)))
                    .collect();
                let mut state_list = list::ListState::default();
                let spec = list::Spec::default();
                let events = list::show(
                    ui,
                    controls,
                    &mut state_list,
                    spec,
                    &rows,
                    |ui, controls, cell, i| {
                        if current.as_ref() == Some(&worlds[i].path) {
                            let tick = Rect::from_center_size(
                                pos2(cell.rect.right() - 14.0, cell.rect.center().y),
                                vec2(18.0, 18.0),
                            );
                            icons::paint(&cell.painter, tick, Icon::Tick, 18.0, theme::ACCENT);
                        }
                        let look = list::Look {
                            text: &rows[i].name,
                            ..list::Look::default()
                        };
                        list::draw(ui, controls, cell, &look)
                    },
                );
                for event in events {
                    if let list::Event::Tap(i) = event {
                        state
                            .actions
                            .push(Action::OpenWorld(worlds[i].path.clone()));
                        open = false;
                    }
                }
            };
            if worlds.len() > WORLD_ROWS {
                egui::ScrollArea::vertical()
                    .max_height(shown)
                    .show(ui, |ui| {
                        ui.set_width(inner.width());
                        rows(ui)
                    });
            } else {
                rows(ui);
            }
            if !worlds.is_empty() {
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
            if export {
                rule(ui);
                if export_form_row(ui, state, controls) {
                    open = false;
                }
            }
        },
    );
    state.pad.worlds = open.then_some(worlds);
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
