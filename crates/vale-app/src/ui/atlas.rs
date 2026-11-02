//! The canvas of the Atlas workspace: the open page and its map frames.
//! The iPad layout and the desktop layout use the same canvas.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect, pos2, vec2};
use kurbo::{Point, Vec2};
use vale_sphere::{Projection, ProjectionKind, ProjectionSpec};
use vale_store::{
    Border, BorderStyle, CoordFormat, EdgeLabels, FrameId, FrameRecord, FrameShape, Furniture,
    FurnitureId, FurnitureRecord, FurnitureText, Graticule, LayerId, LegendItem, MapId, Neatline,
    NorthArrowItem, PageId, PageRecord, PageUnits, Rgb, ScaleBarItem, TextItem,
};

use std::sync::Arc;

use vale_store::RasterId;

use super::pad::theme;
use super::{AppState, LayerRef};
use crate::atlas::{self as math, FrameTarget, Handle, PT_PER_MM, PagePreset, Vertex};
use crate::globe::backdrop::{Canvas, REDUCTION, Scene, SceneCallback};
use crate::globe::flat::FlatMesh;
use crate::globe::gpu::Mask;
use crate::globe::nav::Nav;
use crate::globe::{FrameDraw, FramePlace};
use crate::page_snap;
use crate::pipeline::Quality;
use crate::view::{View, ViewTransform};

pub mod measure;

/// The side of the square that takes the touch of a handle.
pub const HANDLE_TARGET: f32 = 44.0;
/// The radius of the circle of a handle.
const HANDLE_RADIUS: f32 = 6.0;
/// A wheel zoom is part of one gesture until the wheel rests this long, in
/// seconds.
const WHEEL_REST: f64 = 0.25;
/// Two taps less than this time apart are a double tap, in seconds.
const DOUBLE_TAP: f64 = 0.4;
/// The color of the paper of a page.
fn paper(page: &PageRecord) -> Color32 {
    let [r, g, b] = page.paper;
    Color32::from_rgb(r, g, b)
}
const ACCENT: Color32 = Color32::from_rgb(69, 196, 168);
/// The color of the snap guides.
const GUIDE: Color32 = Color32::from_rgb(240, 70, 160);
/// The smallest and the largest side of a page, in millimeters.
const PAGE_SIDE: (f64, f64) = (10.0, 2000.0);
/// The smallest and the largest zoom of the page, relative to the page
/// that fits the screen.
pub const PAGE_ZOOM: (f64, f64) = (0.25, 40.0);
/// The part of the page that stays on the screen, in points.
const PAGE_KEEP: f32 = 48.0;

/// The thing that the panel of the Atlas workspace shows.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AtlasSelection {
    #[default]
    None,
    Page,
    Frame(FrameId),
    /// An item of furniture.
    Item(FurnitureId),
}

/// The kind of a new item of furniture.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Legend,
    ScaleBar,
    NorthArrow,
}

impl ItemKind {
    pub const ALL: [ItemKind; 4] = [
        ItemKind::Text,
        ItemKind::Legend,
        ItemKind::ScaleBar,
        ItemKind::NorthArrow,
    ];

    pub fn of(item: &Furniture) -> ItemKind {
        match item {
            Furniture::Text(_) => ItemKind::Text,
            Furniture::Legend(_) => ItemKind::Legend,
            Furniture::ScaleBar(_) => ItemKind::ScaleBar,
            Furniture::NorthArrow(_) => ItemKind::NorthArrow,
        }
    }

    /// The name of the kind in the Add menu and in the title of its panel.
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Text => "Text",
            ItemKind::Legend => "Legend",
            ItemKind::ScaleBar => "Scale bar",
            ItemKind::NorthArrow => "North arrow",
        }
    }

    /// The width and the height of a new item, in millimeters.
    fn size(self) -> (f64, f64) {
        match self {
            ItemKind::Text => (80.0, 15.0),
            ItemKind::Legend => (50.0, 40.0),
            ItemKind::ScaleBar => (60.0, 12.0),
            ItemKind::NorthArrow => (12.0, 18.0),
        }
    }
}

/// A part of an item of furniture with its own text style.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextPart {
    /// The text of a text item.
    Text,
    /// The title of a legend.
    Title,
    /// The entries of a legend.
    Entries,
    /// The numbers of a scale bar.
    Numbers,
    /// The unit labels of a scale bar.
    Units,
    /// The letter N of a north arrow.
    Letter,
}

impl TextPart {
    /// The style of the part in an item, if the item has the part.
    pub fn of(self, item: &Furniture) -> Option<&FurnitureText> {
        match (self, item) {
            (TextPart::Text, Furniture::Text(i)) => Some(&i.style),
            (TextPart::Title, Furniture::Legend(i)) => Some(&i.title_style),
            (TextPart::Entries, Furniture::Legend(i)) => Some(&i.entry_style),
            (TextPart::Numbers, Furniture::ScaleBar(i)) => Some(&i.number_style),
            (TextPart::Units, Furniture::ScaleBar(i)) => Some(&i.unit_style),
            (TextPart::Letter, Furniture::NorthArrow(i)) => Some(&i.letter_style),
            _ => None,
        }
    }

    pub fn of_mut(self, item: &mut Furniture) -> Option<&mut FurnitureText> {
        match (self, item) {
            (TextPart::Text, Furniture::Text(i)) => Some(&mut i.style),
            (TextPart::Title, Furniture::Legend(i)) => Some(&mut i.title_style),
            (TextPart::Entries, Furniture::Legend(i)) => Some(&mut i.entry_style),
            (TextPart::Numbers, Furniture::ScaleBar(i)) => Some(&mut i.number_style),
            (TextPart::Units, Furniture::ScaleBar(i)) => Some(&mut i.unit_style),
            (TextPart::Letter, Furniture::NorthArrow(i)) => Some(&mut i.letter_style),
            _ => None,
        }
    }

    /// The page of the panel that holds the Text section of the part.
    /// `None`: the top of the panel.
    pub fn page(self) -> Option<ItemPage> {
        match self {
            TextPart::Text | TextPart::Letter => None,
            TextPart::Title | TextPart::Entries => Some(ItemPage::Text(self)),
            TextPart::Numbers => Some(ItemPage::Bar(BarPage::Numbers)),
            TextPart::Units => Some(ItemPage::Bar(BarPage::Units)),
        }
    }
}

/// A page of the panel of a scale bar.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BarPage {
    /// The gallery of the kinds of scale bar.
    Style,
    Units,
    Divisions,
    Bar,
    Marks,
    Numbers,
}

impl BarPage {
    pub const ALL: [BarPage; 6] = [
        BarPage::Style,
        BarPage::Units,
        BarPage::Divisions,
        BarPage::Bar,
        BarPage::Marks,
        BarPage::Numbers,
    ];

    /// The row that opens the page, and the title of the page.
    pub fn name(self) -> &'static str {
        match self {
            BarPage::Style => "Style",
            BarPage::Units => "Units",
            BarPage::Divisions => "Divisions",
            BarPage::Bar => "Bar",
            BarPage::Marks => "Marks",
            BarPage::Numbers => "Numbers",
        }
    }
}

/// A list of choices of a scale bar, on its own page.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BarPick {
    Unit,
    SecondUnit,
    UnitLabel,
    UnitPosition,
    Fit,
    MarkFrequency,
    NumberFrequency,
    Decimals,
}

impl BarPick {
    /// The row that opens the page, and the title of the page.
    pub fn name(self) -> &'static str {
        match self {
            BarPick::Unit => "Unit",
            BarPick::SecondUnit => "Second unit",
            BarPick::UnitLabel => "Unit label",
            BarPick::UnitPosition => "Label position",
            BarPick::Fit => "Fit",
            BarPick::MarkFrequency | BarPick::NumberFrequency => "Frequency",
            BarPick::Decimals => "Decimals",
        }
    }

    /// The page that holds the row of the choice.
    pub fn parent(self) -> BarPage {
        match self {
            BarPick::Unit | BarPick::SecondUnit | BarPick::UnitLabel | BarPick::UnitPosition => {
                BarPage::Units
            }
            BarPick::Fit => BarPage::Divisions,
            BarPick::MarkFrequency => BarPage::Marks,
            BarPick::NumberFrequency | BarPick::Decimals => BarPage::Numbers,
        }
    }
}

/// A page under the Grid labels page.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GridPage {
    Faces,
    Cases,
}

/// A page of the panel of an item of furniture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemPage {
    /// The frames of the page, to link the item to.
    Frame,
    /// The faces of the font of a text part.
    Face(TextPart),
    /// The cases of a text part.
    Case(TextPart),
    /// The text of a part of a legend: the title or the entries.
    Text(TextPart),
    /// The name of an entry of a legend, by its key.
    Entry(LayerId, String),
    Bar(BarPage),
    Pick(BarPick),
    /// The first fill of a scale bar (`false`) or the second fill (`true`).
    Fill(bool),
}

impl ItemPage {
    /// The page that the Back button goes to. `None`: the top of the panel.
    pub fn parent(&self) -> Option<ItemPage> {
        match self {
            ItemPage::Frame | ItemPage::Text(_) | ItemPage::Bar(_) => None,
            ItemPage::Face(part) | ItemPage::Case(part) => part.page(),
            ItemPage::Entry(..) => Some(ItemPage::Text(TextPart::Entries)),
            ItemPage::Pick(pick) => Some(ItemPage::Bar(pick.parent())),
            ItemPage::Fill(_) => Some(ItemPage::Bar(BarPage::Bar)),
        }
    }
}

/// The tool of a pen, a finger, and the mouse on the page.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AtlasTool {
    /// A tap selects, and a drag moves a frame or a handle.
    #[default]
    Select,
    /// A drag moves the map in a frame.
    Move,
    /// A tap in a frame adds a point to the path of the Measure tool, and a
    /// drag moves the page.
    Measure,
}

/// A tab of the panel of a frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FrameTab {
    #[default]
    Frame,
    Projection,
    Layers,
    Labels,
}

/// A page of the panel of a label class.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClassPage {
    /// The layers that the class can label.
    Layer,
    /// The placements of the feature type of the layer.
    Placement,
    /// The faces of the font family of the class.
    Style,
    /// The cases of the text.
    Case,
    /// A new condition of the filter.
    Condition,
    /// The ranked positions of a point class.
    Positions,
}

/// A segment of the panel of a label class.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ClassSegment {
    /// The text, the filter, and the font.
    #[default]
    Text,
    /// Where the labels go on their features.
    Placement,
    /// How the labels keep apart.
    Conflicts,
}

/// A color of a label class that the color picker edits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClassColor {
    Text,
    Halo,
    EdgeText,
    EdgeHalo,
    /// The text of a part of an item of furniture.
    ItemText(TextPart),
    /// The halo of a part of an item of furniture.
    ItemHalo(TextPart),
    /// The first fill of a scale bar.
    Fill1,
    Fill2,
    /// The outline of a scale bar.
    Outline,
    /// The arrow of a north arrow.
    Arrow,
    /// The background of the hatch of the first or the second fill of a
    /// scale bar.
    Background(bool),
    /// The paper of a page.
    Paper,
    /// The lines of the layout grid.
    LayoutGrid,
    /// The first or the second color of the border of a frame.
    Border(bool),
    /// The stroke of the border of a frame.
    BorderStroke,
    /// The text of the reference grid of a frame.
    GridText,
    /// The halo of the reference grid of a frame.
    GridHalo,
}

/// A row that a finger drags by its handle: a label class in the Labels
/// tab, or a position on the Positions page.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClassDrag {
    /// The place of the row in its list.
    pub from: usize,
    /// The distance from the top of the row to the finger.
    pub grab: f32,
}

/// The pan and the zoom of the page on the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PageView {
    /// The zoom, relative to the page that fits the screen.
    pub zoom: f64,
    /// The middle of the page from the middle of the page that fits, in
    /// points.
    pub offset: egui::Vec2,
}

impl Default for PageView {
    fn default() -> PageView {
        PageView {
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
}

/// The place of the page on the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PagePlace {
    pub rect: Rect,
    pub pts_per_mm: f64,
}

impl PagePlace {
    /// The page at its true proportions, as large as fits in `free`, in the
    /// middle of it.
    pub fn fit(page: &PageRecord, free: Rect) -> PagePlace {
        let (w, h) = (page.width_mm.max(1.0), page.height_mm.max(1.0));
        let scale = (f64::from(free.width()) / w)
            .min(f64::from(free.height()) / h)
            .max(1e-3);
        let size = vec2((w * scale) as f32, (h * scale) as f32);
        PagePlace {
            rect: Rect::from_center_size(free.center(), size),
            pts_per_mm: scale,
        }
    }

    /// The fitted page with a pan and a zoom.
    pub fn viewed(&self, view: PageView) -> PagePlace {
        if view == PageView::default() {
            return *self;
        }
        let size = self.rect.size() * view.zoom as f32;
        PagePlace {
            rect: Rect::from_center_size(self.rect.center() + view.offset, size),
            pts_per_mm: self.pts_per_mm * view.zoom,
        }
    }

    /// Pixels of the screen in one point of the page, with
    /// `pixels_per_point` pixels in one point of the screen.
    pub fn pixels_per_page_point(&self, pixels_per_point: f32) -> f32 {
        (self.pts_per_mm / PT_PER_MM) as f32 * pixels_per_point
    }

    pub fn target(&self) -> FrameTarget {
        FrameTarget {
            origin: Point::new(f64::from(self.rect.min.x), f64::from(self.rect.min.y)),
            pts_per_mm: self.pts_per_mm,
        }
    }

    /// A screen position to page millimeters.
    pub fn to_mm(&self, p: Pos2) -> Point {
        self.target()
            .from_target(Point::new(f64::from(p.x), f64::from(p.y)))
    }

    /// Page millimeters to a screen position.
    pub fn to_screen(&self, p: Point) -> Pos2 {
        let q = self.target().to_target(p);
        pos2(q.x as f32, q.y as f32)
    }

    /// A box in page millimeters to the screen.
    pub fn rect_to_screen(&self, r: kurbo::Rect) -> Rect {
        Rect::from_min_max(
            self.to_screen(Point::new(r.x0, r.y0)),
            self.to_screen(Point::new(r.x1, r.y1)),
        )
    }
}

/// The last compose of a frame.
/// A texture of a frame at the reduced size of the blurred backdrop, in
/// premultiplied bytes.
struct Thumb {
    serial: u64,
    size: [usize; 2],
    rgba: Vec<u8>,
}

impl Thumb {
    /// Averages `REDUCTION` by `REDUCTION` pixels of a premultiplied image.
    fn new(img: &vale_render::Rgba) -> Thumb {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = REDUCTION as usize;
        let size = [img.width.div_ceil(n), img.height.div_ceil(n)];
        let mut rgba = vec![0u8; size[0] * size[1] * 4];
        for y in 0..size[1] {
            for x in 0..size[0] {
                let (mut sum, mut count) = ([0u32; 4], 0);
                for sy in y * n..((y + 1) * n).min(img.height) {
                    for sx in x * n..((x + 1) * n).min(img.width) {
                        let at = (sy * img.width + sx) * 4;
                        for (total, byte) in sum.iter_mut().zip(&img.data[at..at + 4]) {
                            *total += u32::from(*byte);
                        }
                        count += 1;
                    }
                }
                for c in 0..4 {
                    rgba[(y * size[0] + x) * 4 + c] = (sum[c] / count.max(1)) as u8;
                }
            }
        }
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Thumb { serial, size, rgba }
    }
}

struct FrameImage {
    texture: egui::TextureHandle,
    /// A reduced copy for the blurred backdrop of the cards.
    thumb: Arc<Thumb>,
    /// The key of what the frame shows.
    content: u64,
    /// The key of the place of the frame and of its view.
    place: u64,
    /// The view of the frame on the screen at the compose.
    vt: ViewTransform,
    /// The texture on the screen at the compose.
    rect: Rect,
    /// The compose was at the final quality.
    settled: bool,
}

/// What a drag on the page changes.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Drag {
    /// A handle of a frame, with the distance from the pointer to the handle
    /// in millimeters.
    Handle(FrameId, Handle, Vec2),
    /// The whole frame, with the distance from the pointer to the top-left
    /// corner of its box in millimeters.
    Body(FrameId, Vec2),
    /// The map in the frame.
    Pan(FrameId),
    /// The page on the screen.
    Page,
    /// The map in the frame turns about the anchor. The angle of the pointer
    /// in the last frame, in degrees.
    Rotate(FrameId, f64),
    /// A handle of an item of furniture, with the distance from the pointer
    /// to the handle in millimeters.
    ItemHandle(FurnitureId, usize, Vec2),
    /// The whole item.
    Item(FurnitureId),
    /// Nothing.
    Idle,
}

/// What a gesture of two fingers changes.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Pinch {
    /// The map in the selected frame.
    Frame(FrameId),
    Page,
}

pub struct AtlasState {
    /// The open page.
    pub page: PageId,
    pub selection: AtlasSelection,
    /// The selected vertex of the selected polygon frame.
    pub vertex: Option<Vertex>,
    pub tool: AtlasTool,
    pub tab: FrameTab,
    /// The list of the page sizes of the panel of the page is open.
    pub sizes: bool,
    /// The list of the projections of the Projection tab is open.
    pub kinds: bool,
    /// The color picker of the graticule is open.
    pub graticule_color: bool,
    /// The color picker of the graticule showed in this frame.
    pub(crate) graticule_shown: bool,
    /// The color picker of the neatline is open.
    pub neatline_color: bool,
    /// The color picker of the neatline showed in this frame.
    pub(crate) neatline_shown: bool,
    /// The text of the name field of the custom projections.
    pub preset_name: String,
    /// The Add menu is open.
    pub add: bool,
    /// The Add menu in the last frame.
    pub add_rect: Option<Rect>,
    /// The panel of the selection in the last frame.
    pub panel_rect: Option<Rect>,
    /// The page on the screen in the last frame.
    pub place: Option<PagePlace>,
    /// The pan and the zoom of the open page.
    pub view: PageView,
    /// The page of `view`.
    view_of: Option<PageId>,
    /// Frames snap to the layout of the page.
    pub snap: bool,
    /// The guides of the snaps of the drag, in page millimeters.
    pub guides: Vec<page_snap::Guide>,
    /// Shift or a second finger locks the angle of the edges of a vertex.
    lock: bool,
    /// A second finger joined the drag of a vertex. The fingers do not pinch
    /// until all of them are up.
    held: bool,
    /// The page that fits the screen, and the canvas, in the last frame.
    fit: Option<(PagePlace, Rect)>,
    /// The handles of the selected frame in the last frame, with their
    /// touch areas.
    pub handles: Vec<(Handle, Rect)>,
    /// The count of the frame composes.
    pub composes: u64,
    /// The layer whose panel the Layers tab of this frame shows.
    pub layer: Option<(FrameId, LayerId)>,
    /// The page of the panel of `layer`.
    pub layer_page: Option<super::pad::Page>,
    /// The label class whose panel the Labels tab of this frame shows: its
    /// layer and its place in the classes of the layer.
    pub label: Option<(FrameId, LayerId, usize)>,
    /// The page of the panel of `label`.
    pub label_page: Option<ClassPage>,
    /// The segment of the panel of `label`.
    pub label_segment: ClassSegment,
    /// The color of the label class whose color picker is open.
    pub label_color: Option<ClassColor>,
    /// The color picker of the label class showed in this frame.
    pub(crate) label_color_shown: bool,
    /// The text of the Text field of the label class while it has the focus.
    pub label_text: String,
    /// The text of the Expression field of the filter of the label class.
    pub label_filter: String,
    /// The condition of the page of a new condition of the label class.
    pub label_draft: super::pad::filter::Draft,
    /// The label class that a finger drags in the Labels tab.
    pub label_drag: Option<ClassDrag>,
    /// The position that a finger drags on the Positions page.
    pub position_drag: Option<ClassDrag>,
    /// The frame whose Edge labels page the Frame tab shows.
    pub edge: Option<FrameId>,
    /// The set of edge labels that the page shows.
    pub edge_set: EdgeSet,
    /// The list of the faces of the font of the set is open.
    pub edge_styles: bool,
    /// The frame whose Grid labels page the Frame tab shows.
    pub grid: Option<FrameId>,
    /// The page of the faces or of the cases of the grid labels, in place of
    /// the Grid labels page.
    pub grid_page: Option<GridPage>,
    /// The panel shows the distance of each side of the border.
    pub border_sides: bool,
    /// The panel of this selection and tab showed in the last frame.
    pub(crate) shown: Option<(AtlasSelection, FrameTab)>,
    /// The page of the panel of the selected item of furniture.
    pub item_page: Option<ItemPage>,
    /// The thumbnails of the kinds of scale bar in the style gallery: the
    /// key of what each shows, and its texture.
    bar_previews: HashMap<vale_store::ScaleBarKind, (u64, egui::TextureHandle)>,
    /// The text of the name field of the entry page of a legend.
    pub entry_name: String,
    /// The selection of `handles`.
    handles_of: Option<AtlasSelection>,
    images: HashMap<FrameId, FrameImage>,
    /// The textures of the items of furniture.
    item_images: HashMap<FurnitureId, ItemImage>,
    /// The water of the frames that draw raster layers.
    unders: HashMap<FrameId, FrameImage>,
    /// The edge labels, the neatline, and the border of the frames. They
    /// stay in place while a gesture moves the map.
    decors: HashMap<FrameId, FrameImage>,
    /// The map of the frames in a margin around their shapes, at a lower
    /// resolution. It fills the shape while a gesture moves the map.
    spills: HashMap<FrameId, FrameImage>,
    /// The screen rectangles of the map, the spill, and the decorations of
    /// each frame in the last frame.
    painted: HashMap<FrameId, [Option<Rect>; 3]>,
    /// The shape of each frame as a mask for its raster layers: the key of
    /// what it shows, and the mask.
    masks: HashMap<FrameId, (u64, Arc<Mask>)>,
    mask_count: u64,
    /// The meshes of the projections of the maps, on a sphere of radius 1.
    meshes: Vec<(ProjectionSpec, Arc<FlatMesh>)>,
    drag: Option<Drag>,
    /// The pointer at the last step of the drag.
    drag_last: Pos2,
    /// What the two-finger gesture changes.
    pinch: Option<Pinch>,
    /// More than one finger touched the screen since the first finger came
    /// down. A drag of one finger then does nothing.
    fingers: bool,
    /// The time of the last wheel zoom.
    wheel_at: Option<f64>,
    /// The time and the place of the last tap.
    last_tap: Option<(f64, Pos2)>,
    /// Finds the taps of two and three fingers, which undo and redo.
    nav: Nav,
    /// The press of the Measure tool that can be a tap.
    measure_press: crate::measure::TapPress,
}

impl AtlasState {
    /// The layer whose panel the Layers tab of the selected frame shows.
    pub fn open_layer(&self) -> Option<LayerId> {
        match (self.selection, self.tab, self.layer) {
            (AtlasSelection::Frame(f), FrameTab::Layers, Some((of, layer))) if f == of => {
                Some(layer)
            }
            _ => None,
        }
    }

    /// The frame whose Grid labels page the Frame tab shows.
    pub fn open_grid(&self) -> Option<FrameId> {
        match (self.selection, self.tab, self.grid) {
            (AtlasSelection::Frame(f), FrameTab::Frame, Some(of)) if f == of => Some(f),
            _ => None,
        }
    }

    /// The frame whose Edge labels page the Frame tab shows.
    pub fn open_edge(&self) -> Option<FrameId> {
        match (self.selection, self.tab, self.edge) {
            (AtlasSelection::Frame(f), FrameTab::Frame, Some(of)) if f == of => Some(f),
            _ => None,
        }
    }

    /// The label class whose panel the Labels tab of the selected frame
    /// shows.
    pub fn open_class(&self) -> Option<(LayerId, usize)> {
        match (self.selection, self.tab, self.label) {
            (AtlasSelection::Frame(f), FrameTab::Labels, Some((of, layer, i))) if f == of => {
                Some((layer, i))
            }
            _ => None,
        }
    }

    /// The screen rectangle of the texture of each frame.
    pub fn texture_rects(&self) -> Vec<Rect> {
        self.images.values().map(|i| i.rect).collect()
    }

    /// The screen rectangle of the texture of frame `id`.
    pub fn texture_rect(&self, id: FrameId) -> Option<Rect> {
        self.images.get(&id).map(|i| i.rect)
    }

    /// The screen rectangles of the map, the spill, and the decorations of
    /// frame `id` in the last frame.
    pub fn painted_rects(&self, id: FrameId) -> [Option<Rect>; 3] {
        self.painted.get(&id).copied().unwrap_or_default()
    }
}

impl Default for AtlasState {
    fn default() -> AtlasState {
        AtlasState {
            page: PageId(0),
            selection: AtlasSelection::None,
            vertex: None,
            tool: AtlasTool::Select,
            tab: FrameTab::Frame,
            sizes: false,
            kinds: false,
            graticule_color: false,
            graticule_shown: false,
            neatline_color: false,
            neatline_shown: false,
            preset_name: String::new(),
            add: false,
            add_rect: None,
            panel_rect: None,
            place: None,
            view: PageView::default(),
            view_of: None,
            fit: None,
            handles: Vec::new(),
            composes: 0,
            layer: None,
            layer_page: None,
            label: None,
            label_page: None,
            label_segment: ClassSegment::Text,
            label_color: None,
            label_color_shown: false,
            label_text: String::new(),
            label_filter: String::new(),
            label_draft: Default::default(),
            label_drag: None,
            position_drag: None,
            edge: None,
            edge_set: EdgeSet::Meridians,
            edge_styles: false,
            grid: None,
            grid_page: None,
            border_sides: false,
            shown: None,
            item_page: None,
            bar_previews: HashMap::new(),
            entry_name: String::new(),
            handles_of: None,
            images: HashMap::new(),
            item_images: HashMap::new(),
            unders: HashMap::new(),
            decors: HashMap::new(),
            spills: HashMap::new(),
            painted: HashMap::new(),
            masks: HashMap::new(),
            mask_count: 0,
            meshes: Vec::new(),
            snap: true,
            guides: Vec::new(),
            lock: false,
            held: false,
            drag: None,
            drag_last: Pos2::ZERO,
            pinch: None,
            fingers: false,
            wheel_at: None,
            last_tap: None,
            nav: Nav::default(),
            measure_press: Default::default(),
        }
    }
}

impl AtlasState {
    /// The number of frames with a texture.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// True if the frame has a texture.
    pub fn has_image(&self, id: FrameId) -> bool {
        self.images.contains_key(&id)
    }

    /// The size of the texture of a frame, in pixels.
    pub fn image_size(&self, id: FrameId) -> Option<[usize; 2]> {
        self.images.get(&id).map(|image| image.texture.size())
    }

    /// The selected frame.
    pub fn frame(&self) -> Option<FrameId> {
        match self.selection {
            AtlasSelection::Frame(id) => Some(id),
            _ => None,
        }
    }

    /// The selected item of furniture.
    pub fn item(&self) -> Option<FurnitureId> {
        match self.selection {
            AtlasSelection::Item(id) => Some(id),
            _ => None,
        }
    }

    /// True if the item has a texture.
    /// The number of thumbnails of the style gallery of a scale bar.
    pub fn bar_preview_count(&self) -> usize {
        self.bar_previews.len()
    }

    pub fn has_item_image(&self, id: FurnitureId) -> bool {
        self.item_images.contains_key(&id)
    }

    /// The screen rectangle of the texture of an item.
    pub fn item_rect(&self, id: FurnitureId) -> Option<Rect> {
        self.item_images.get(&id).map(|i| i.rect)
    }
}

/// The texture of an item of furniture.
struct ItemImage {
    texture: egui::TextureHandle,
    /// The key of what the item shows.
    content: u64,
    /// The texture on the screen at the last paint.
    rect: Rect,
}

/// Makes the open page, the selection, and the selected vertex valid for
/// the atlas of the document.
pub fn sync(state: &mut AppState) {
    let atlas = &state.doc.project.atlas;
    let a = &mut state.atlas;
    if atlas.page(a.page).is_none() {
        a.page = atlas.pages[0].id;
        a.selection = AtlasSelection::None;
    }
    let page = atlas.page(a.page).expect("the open page is in the atlas");
    let frame = a
        .frame()
        .and_then(|id| page.frames.iter().find(|f| f.id == id));
    if a.frame().is_some() && frame.is_none() {
        a.selection = AtlasSelection::None;
    }
    let item = a
        .item()
        .is_some_and(|id| page.furniture.iter().any(|f| f.id == id));
    if a.item().is_some() && !item {
        a.selection = AtlasSelection::None;
    }
    if a.item().is_none() {
        a.item_page = None;
    }
    let vertex_ok = |(r, i): Vertex| match frame.map(|f| &f.shape) {
        Some(FrameShape::Polygon(rings)) => rings.get(r).is_some_and(|ring| i < ring.len()),
        _ => false,
    };
    if a.vertex.is_some_and(|v| !vertex_ok(v)) {
        a.vertex = None;
    }
    a.images.retain(|id, _| atlas.frame(*id).is_some());
    a.decors.retain(|id, _| atlas.frame(*id).is_some());
    a.spills.retain(|id, _| atlas.frame(*id).is_some());
    a.item_images
        .retain(|id, _| page.furniture.iter().any(|f| f.id == *id));
}

/// The place of the open page in the atlas order, from 0, and the number
/// of pages.
pub fn page_index(state: &AppState) -> (usize, usize) {
    let atlas = &state.doc.project.atlas;
    (
        atlas.page_index(state.atlas.page).unwrap_or(0),
        atlas.pages.len(),
    )
}

/// The open page.
pub fn open_page(state: &AppState) -> &PageRecord {
    let atlas = &state.doc.project.atlas;
    atlas.page(state.atlas.page).unwrap_or(&atlas.pages[0])
}

/// Opens the page before (`-1`) or after (`1`) the open page.
pub fn go_to_page(state: &mut AppState, step: isize) {
    let (index, count) = page_index(state);
    let next = index as isize + step;
    if (0..count as isize).contains(&next) {
        state.atlas.page = state.doc.project.atlas.pages[next as usize].id;
        if state.atlas.frame().is_some() || state.atlas.item().is_some() {
            state.atlas.selection = AtlasSelection::None;
        }
        state.atlas.vertex = None;
    }
}

/// The map of the selected frame, or the first map if no frame is
/// selected.
pub fn selected_map(state: &AppState) -> MapId {
    let frame = state.atlas.frame();
    let frame = frame.and_then(|id| state.doc.project.atlas.frame(id));
    frame.map_or_else(|| state.doc.first_map(), |f| f.map)
}

/// Adds a frame to the open page and selects it. The frame shows a new
/// map, or a map that no frame shows.
pub fn add_frame(state: &mut AppState) -> Option<FrameId> {
    let page = open_page(state).clone();
    let atlas = &mut state.doc.project.atlas;
    let map_id = atlas.unshown_map().unwrap_or_else(|| atlas.add_map());
    let map = atlas.map(map_id)?.clone();
    let projection = match state.pipeline.projection(&state.doc, map_id) {
        Ok(p) => p,
        Err(e) => {
            state.status = format!("{e:#}");
            return None;
        }
    };
    let frame = math::new_frame(&page, &map, projection);
    let id = state.doc.project.atlas.add_frame(page.id, frame)?;
    state.atlas.selection = AtlasSelection::Frame(id);
    state.atlas.vertex = None;
    state.atlas.tab = FrameTab::Frame;
    Some(id)
}

/// Changes a frame with the projection of its map. The frame changes only
/// if `edit` returns true. Returns that value.
pub fn edit_frame(
    state: &mut AppState,
    id: FrameId,
    edit: impl FnOnce(&mut FrameRecord, &Projection) -> bool,
) -> bool {
    let Some(mut frame) = state.doc.project.atlas.frame(id).cloned() else {
        return false;
    };
    let Ok(projection) = state.pipeline.projection(&state.doc, frame.map) else {
        return false;
    };
    if !edit(&mut frame, projection) {
        return false;
    }
    if let Some(slot) = state.doc.project.atlas.frame_mut(id) {
        *slot = frame;
    }
    true
}

/// Adds an item of furniture to the open page, inside the margin of the
/// page, and selects it. A legend, a scale bar, and a north arrow link to
/// the largest frame of the page.
pub fn add_item(state: &mut AppState, kind: ItemKind) -> Option<FurnitureId> {
    let page = open_page(state);
    let frame = largest_frame(page);
    let m = math::MARGIN_MM;
    let (w, h) = kind.size();
    let (w, h) = (
        w.min(page.width_mm - 2.0 * m).max(math::MIN_SIZE_MM),
        h.min(page.height_mm - 2.0 * m).max(math::MIN_SIZE_MM),
    );
    let (left, top) = (m, m);
    let (right, bottom) = (page.width_mm - m - w, page.height_mm - m - h);
    let (x, y, item) = match kind {
        ItemKind::Text => (left, top, Furniture::Text(TextItem::default())),
        ItemKind::Legend => {
            let item = LegendItem {
                frame,
                ..LegendItem::default()
            };
            (right, top, Furniture::Legend(item))
        }
        ItemKind::ScaleBar => {
            let item = ScaleBarItem {
                frame,
                unit: vale_store::ScaleUnit::for_units(state.doc.project.world.units),
                ..ScaleBarItem::default()
            };
            (left, bottom, Furniture::ScaleBar(item))
        }
        ItemKind::NorthArrow => {
            let item = NorthArrowItem {
                frame,
                ..NorthArrowItem::default()
            };
            (right, bottom, Furniture::NorthArrow(item))
        }
    };
    let page = page.id;
    let rect = [x, y, x + w, y + h];
    let id = state.doc.project.atlas.add_furniture(page, rect, item)?;
    state.atlas.selection = AtlasSelection::Item(id);
    state.atlas.vertex = None;
    state.atlas.item_page = None;
    Some(id)
}

/// The frame of a page with the largest area.
pub fn largest_frame(page: &PageRecord) -> Option<FrameId> {
    let area = |f: &FrameRecord| -> f64 {
        let rings = f.shape.rings();
        let ring_area = |ring: &Vec<[f64; 2]>| {
            let n = ring.len();
            let twice: f64 = (0..n)
                .map(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % n]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum();
            twice.abs() / 2.0
        };
        let mut rings = rings.iter();
        let outer = rings.next().map_or(0.0, ring_area);
        outer - rings.map(ring_area).sum::<f64>()
    };
    let largest = page.frames.iter().map(|f| (f.id, area(f)));
    let largest = largest.max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
    largest.map(|(id, _)| id)
}

/// Changes an item of furniture. The item changes only if `edit` returns
/// true.
pub fn edit_item(
    state: &mut AppState,
    id: FurnitureId,
    edit: impl FnOnce(&mut FurnitureRecord) -> bool,
) -> bool {
    let Some(mut rec) = state.doc.project.atlas.furniture(id).cloned() else {
        return false;
    };
    if !edit(&mut rec) || state.doc.project.atlas.furniture(id) == Some(&rec) {
        return false;
    }
    if let Some(slot) = state.doc.project.atlas.furniture_mut(id) {
        *slot = rec;
    }
    true
}

/// `edit_item` as one undo step.
pub fn item_step(
    state: &mut AppState,
    id: FurnitureId,
    edit: impl FnOnce(&mut FurnitureRecord) -> bool,
) {
    state.save_edit();
    if edit_item(state, id, edit) {
        state.save_edit();
    }
}

/// Changes the text style of a part of an item of furniture. With `step`,
/// the change is one undo step.
pub fn edit_text(
    state: &mut AppState,
    id: FurnitureId,
    part: TextPart,
    step: bool,
    edit: impl FnOnce(&mut FurnitureText),
) {
    let edit = |rec: &mut FurnitureRecord| match part.of_mut(&mut rec.item) {
        Some(style) => {
            edit(style);
            true
        }
        None => false,
    };
    if step {
        item_step(state, id, edit);
    } else {
        edit_item(state, id, edit);
    }
}

/// Changes the first fill or the second fill of a scale bar. With `step`,
/// the change is one undo step.
pub fn edit_fill(
    state: &mut AppState,
    id: FurnitureId,
    second: bool,
    step: bool,
    edit: impl FnOnce(&mut vale_store::FillPattern),
) {
    edit_bar(state, id, step, |b| {
        edit(if second { &mut b.fill2 } else { &mut b.fill1 });
    });
}

/// Changes a scale bar. With `step`, the change is one undo step.
pub fn edit_bar(
    state: &mut AppState,
    id: FurnitureId,
    step: bool,
    edit: impl FnOnce(&mut ScaleBarItem),
) {
    let edit = |rec: &mut FurnitureRecord| match &mut rec.item {
        Furniture::ScaleBar(bar) => {
            edit(bar);
            true
        }
        _ => false,
    };
    if step {
        item_step(state, id, edit);
    } else {
        edit_item(state, id, edit);
    }
}

/// The units of a scale bar as the page draws them, with the division
/// values that the bar picks. Nothing for a bar with no frame.
pub fn bar_rows(state: &mut AppState, id: FurnitureId) -> Vec<crate::scale_bar::BarRow> {
    let atlas = &state.doc.project.atlas;
    let Some(rec) = atlas.furniture(id).cloned() else {
        return Vec::new();
    };
    let Furniture::ScaleBar(bar) = &rec.item else {
        return Vec::new();
    };
    let Some(frame) = bar.frame.and_then(|f| atlas.frame(f)).cloned() else {
        return Vec::new();
    };
    let radius = state.doc.project.world.radius_km;
    let Ok(p) = state.pipeline.projection(&state.doc, frame.map) else {
        return Vec::new();
    };
    let Some(km_per_mm) = crate::furniture::scale_at_center(p, &frame, radius) else {
        return Vec::new();
    };
    let mm = crate::furniture::rect_of(&rec);
    let rect = kurbo::Rect::new(0.0, 0.0, mm.width() * PT_PER_MM, mm.height() * PT_PER_MM);
    let km_per_point = km_per_mm / PT_PER_MM;
    crate::scale_bar::draw(bar, rect, 1.0, km_per_point, &mut state.fonts).rows
}

/// The size of a thumbnail of the style gallery of a scale bar, in
/// millimeters.
pub const PREVIEW_MM: (f64, f64) = (60.0, 12.0);

/// The thumbnail of a scale bar in the style `kind`, `width` points wide
/// on the screen. The texture renders again when the bar changes.
pub fn bar_preview(
    ctx: &egui::Context,
    state: &mut AppState,
    bar: &ScaleBarItem,
    kind: vale_store::ScaleBarKind,
    width: f32,
) -> Option<egui::TextureId> {
    let ppp = ctx.pixels_per_point();
    let scale = f64::from(ppp * width) / (PREVIEW_MM.0 * PT_PER_MM);
    let shown = ScaleBarItem {
        frame: None,
        ..bar.clone()
    };
    let key = hash_of((&shown, kind, scale.to_bits()));
    if let Some((k, texture)) = state.atlas.bar_previews.get(&kind)
        && *k == key
    {
        return Some(texture.id());
    }
    let (w, h) = PREVIEW_MM;
    let img = crate::scale_bar::preview_rgba(&shown, kind, w, h, scale, &mut state.fonts).ok()?;
    let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
    let options = egui::TextureOptions::LINEAR;
    let texture = match state.atlas.bar_previews.remove(&kind) {
        Some((_, mut old)) => {
            old.set(image, options);
            old
        }
        None => ctx.load_texture(format!("bar-preview-{}", kind.id()), image, options),
    };
    let id = texture.id();
    state.atlas.bar_previews.insert(kind, (key, texture));
    Some(id)
}

/// Links an item of furniture to a frame.
pub fn set_item_frame(state: &mut AppState, id: FurnitureId, frame: Option<FrameId>) {
    edit_item(state, id, |rec| {
        rec.item.set_frame(frame);
        true
    });
}

/// Changes an entry of a legend. An entry with no change goes away.
pub fn edit_entry(
    state: &mut AppState,
    id: FurnitureId,
    key: (LayerId, String),
    edit: impl FnOnce(&mut vale_store::LegendEntry),
) {
    edit_item(state, id, |rec| {
        let Furniture::Legend(legend) = &mut rec.item else {
            return false;
        };
        let mut entry = legend.entries.get(&key).cloned().unwrap_or_default();
        edit(&mut entry);
        if entry == vale_store::LegendEntry::default() {
            legend.entries.remove(&key);
        } else {
            legend.entries.insert(key, entry);
        }
        true
    });
}

/// The rows of a legend, or nothing for a legend with no frame.
pub fn legend_rows(state: &mut AppState, legend: &LegendItem) -> Vec<crate::furniture::LegendRow> {
    let atlas = &state.doc.project.atlas;
    let Some(frame) = legend.frame.and_then(|f| atlas.frame(f)).cloned() else {
        return Vec::new();
    };
    match state.pipeline.projection(&state.doc, frame.map) {
        Ok(p) => crate::furniture::legend_rows(&state.doc, p, &frame, legend),
        Err(_) => Vec::new(),
    }
}

/// Deletes the selected item of furniture and clears the selection.
pub fn delete_item(state: &mut AppState) {
    let Some(id) = state.atlas.item() else {
        return;
    };
    state.doc.project.atlas.remove_furniture(id);
    state.atlas.selection = AtlasSelection::None;
    state.atlas.item_page = None;
    state.atlas.item_images.remove(&id);
}

/// The frames of the open page and their names: the name of the map, with
/// a number if more than one frame shows the map.
pub fn frame_names(state: &AppState) -> Vec<(FrameId, String)> {
    let atlas = &state.doc.project.atlas;
    let page = open_page(state);
    let name = |f: &FrameRecord| {
        atlas
            .map(f.map)
            .map_or_else(String::new, |m| m.name.clone())
    };
    let mut out = Vec::new();
    for (i, f) in page.frames.iter().enumerate() {
        let same = |g: &&FrameRecord| g.map == f.map;
        let count = page.frames.iter().filter(same).count();
        let text = if count > 1 {
            let n = page.frames[..=i].iter().filter(same).count();
            format!("{} {n}", name(f))
        } else {
            name(f)
        };
        out.push((f.id, text));
    }
    out
}

/// The top item of furniture of a page that holds a point in millimeters.
fn item_at(page: &PageRecord, p: Point) -> Option<FurnitureId> {
    let mut items = page.furniture.iter().rev();
    let found = items.find(|f| crate::furniture::rect_of(f).contains(p));
    found.map(|f| f.id)
}

/// The top frame of a page that holds a point in millimeters.
fn frame_at(page: &PageRecord, p: Point) -> Option<FrameId> {
    let mut frames = page.frames.iter().rev();
    frames.find(|f| math::contains(&f.shape, p)).map(|f| f.id)
}

/// The handle of the selected frame whose touch area holds a screen
/// position. The nearest handle wins.
fn handle_at(state: &AppState, pos: Pos2) -> Option<(Handle, Pos2)> {
    if state.atlas.handles_of != Some(state.atlas.selection) {
        return None;
    }
    // Inside a small item, the touch areas of the handles cover the body.
    // There a handle takes only a touch near its circle, so the body moves.
    let body = state.atlas.item().and_then(|id| {
        let rec = state.doc.project.atlas.furniture(id)?;
        Some(
            state
                .atlas
                .place?
                .rect_to_screen(crate::furniture::rect_of(rec)),
        )
    });
    let inside = body.is_some_and(|r| r.contains(pos));
    let near = state.atlas.handles.iter().filter(|(_, r)| {
        r.contains(pos) && (!inside || r.center().distance(pos) <= 2.0 * HANDLE_RADIUS)
    });
    let best = near.min_by(|(_, a), (_, b)| {
        let (da, db) = (a.center().distance(pos), b.center().distance(pos));
        da.total_cmp(&db)
    });
    best.map(|(h, r)| (*h, r.center()))
}

/// The selected frame if it is the top frame at a screen position.
fn selected_at(
    state: &AppState,
    page: &PageRecord,
    place: PagePlace,
    pos: Pos2,
) -> Option<FrameId> {
    let id = state.atlas.frame()?;
    (frame_at(page, place.to_mm(pos)) == Some(id)).then_some(id)
}

/// The frame whose map a gesture of two fingers and the wheel change: the
/// selected frame under a screen position. While the Measure tool is the
/// tool, the gestures change the page.
fn frame_gesture(
    state: &AppState,
    page: &PageRecord,
    place: PagePlace,
    pos: Pos2,
) -> Option<FrameId> {
    let measuring = state.atlas.tool == AtlasTool::Measure;
    selected_at(state, page, place, pos).filter(|_| !measuring)
}

/// Moves the page on the screen by `d` points.
fn pan_page(state: &mut AppState, d: egui::Vec2) {
    state.atlas.view.offset += d;
    keep_page(state);
}

/// Zooms the page about a screen position.
fn zoom_page(state: &mut AppState, at: Pos2, factor: f64) {
    let Some((fit, _)) = state.atlas.fit else {
        return;
    };
    let view = &mut state.atlas.view;
    let zoom = (view.zoom * factor).clamp(PAGE_ZOOM.0, PAGE_ZOOM.1);
    let f = (zoom / view.zoom) as f32;
    let center = fit.rect.center() + view.offset;
    view.offset = at + (center - at) * f - fit.rect.center();
    view.zoom = zoom;
    keep_page(state);
}

/// Moves the page so that a part of it stays on the canvas.
fn keep_page(state: &mut AppState) {
    let Some((fit, area)) = state.atlas.fit else {
        return;
    };
    let r = fit.viewed(state.atlas.view).rect;
    let keep = |lo: f32, hi: f32, min: f32, max: f32| {
        let k = PAGE_KEEP.min(hi - lo);
        (min + k - hi).max(0.0) + (max - k - lo).min(0.0)
    };
    state.atlas.view.offset += vec2(
        keep(r.left(), r.right(), area.left(), area.right()),
        keep(r.top(), r.bottom(), area.top(), area.bottom()),
    );
}

fn hash_of(value: impl std::fmt::Debug) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{value:?}").hash(&mut hasher);
    hasher.finish()
}

/// Draws the open page in the middle of `free` and takes the touches, the
/// pen, and the mouse in `area`.
pub fn canvas(ui: &mut egui::Ui, state: &mut AppState, area: Rect, free: Rect, back: Color32) {
    sync(state);
    let resp = ui.interact(
        area,
        ui.id().with("atlas-canvas"),
        egui::Sense::click_and_drag(),
    );
    state.canvas_size = (f64::from(area.width()), f64::from(area.height()));
    // The pen queue of the globe is not for the page.
    if let Some(queue) = &state.globe.pen.queue {
        queue.take();
    }
    if state.atlas.view_of != Some(state.atlas.page) {
        state.atlas.view = PageView::default();
        state.atlas.view_of = Some(state.atlas.page);
    }
    let fit = PagePlace::fit(open_page(state), free);
    state.atlas.fit = Some((fit, area));
    let place = fit.viewed(state.atlas.view);
    state.atlas.place = Some(place);
    super::globe::measure_keys(ui, state);
    input(ui, state, &resp, place);
    state.atlas.nav.track_taps(ui, area);
    match super::globe::history_input(ui, &mut state.atlas.nav) {
        Some(true) => state.redo(),
        Some(false) => state.undo(),
        None => {}
    }
    sync(state);
    let place = PagePlace::fit(open_page(state), free).viewed(state.atlas.view);
    state.atlas.place = Some(place);

    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, back);
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    painter.add(shadow.as_shape(place.rect, 0.0));
    painter.rect_filled(place.rect, 0.0, paper(open_page(state)));
    frames(ui, state, place, area);
    items(ui, state, place, area);
    layout_grid(ui, state, place, area);
    selection(ui, state, place);
    guides(ui, state, place, area);
    measure::overlay(ui, state, place, area);
}

/// True while a gesture moves the maps, so the frames show their last
/// textures.
fn gesture(ui: &egui::Ui, state: &AppState) -> bool {
    let (down, fingers, now) =
        ui.input(|i| (i.pointer.any_down(), i.multi_touch().is_some(), i.time));
    let wheel = state.atlas.wheel_at.is_some_and(|t| now - t < WHEEL_REST);
    down || fingers || wheel
}

fn input(ui: &egui::Ui, state: &mut AppState, resp: &egui::Response, place: PagePlace) {
    let page = open_page(state).clone();
    let (any_down, now, alt, shift) = ui.input(|i| {
        let m = i.modifiers;
        (i.pointer.any_down(), i.time, m.alt, m.shift)
    });
    let mut fingers = ui.input(|i| i.multi_touch());
    let touches = fingers.is_some();
    // A second finger on the canvas during the drag of a vertex locks the
    // angle of its edges, and it does not pinch.
    let vertex_drag = matches!(
        state.atlas.drag,
        Some(Drag::Handle(_, Handle::Vertex(_), _))
    );
    if vertex_drag && fingers.is_some() && !state.atlas.fingers {
        state.atlas.held = true;
    }
    state.atlas.lock = shift || (vertex_drag && state.atlas.held && fingers.is_some());
    if state.atlas.held {
        if !any_down && fingers.is_none() {
            state.atlas.held = false;
        }
        fingers = None;
    }
    if fingers.is_some() {
        state.atlas.fingers = true;
    } else {
        state.atlas.pinch = None;
        if !any_down {
            state.atlas.fingers = false;
        }
    }

    // Two fingers move and zoom the map of the selected frame below them.
    // Elsewhere, they move and zoom the page.
    if let Some(touch) = fingers {
        let pinch = match state.atlas.pinch {
            Some(pinch) => Some(pinch),
            None if resp.rect.contains(touch.center_pos) => {
                Some(match frame_gesture(state, &page, place, touch.center_pos) {
                    Some(id) => Pinch::Frame(id),
                    None => Pinch::Page,
                })
            }
            None => None,
        };
        state.atlas.pinch = pinch;
        state.atlas.drag = None;
        if pinch == Some(Pinch::Page) {
            pan_page(state, touch.translation_delta);
            zoom_page(state, touch.center_pos, f64::from(touch.zoom_delta));
        }
        if let Some(Pinch::Frame(id)) = pinch {
            let at = place.to_mm(touch.center_pos);
            let d = touch.translation_delta;
            let d = Vec2::new(f64::from(d.x), f64::from(d.y)) / place.pts_per_mm;
            let factor = f64::from(touch.zoom_delta);
            edit_frame(state, id, |frame, projection| {
                let panned = d != Vec2::ZERO && math::pan(frame, projection, d);
                let zoomed =
                    (factor - 1.0).abs() > 1e-6 && math::zoom(frame, projection, at, factor);
                panned || zoomed
            });
        }
    }

    // The wheel and a pinch of the trackpad zoom the map of the selected
    // frame, or the page, about the pointer.
    if !touches && let Some(pos) = resp.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
        let at = place.to_mm(pos);
        if (factor - 1.0).abs() > 1e-4 {
            match frame_gesture(state, &page, place, pos) {
                Some(id) => {
                    if edit_frame(state, id, |f, p| math::zoom(f, p, at, factor)) {
                        state.atlas.wheel_at = Some(now);
                    }
                }
                None => {
                    zoom_page(state, pos, factor);
                    state.atlas.wheel_at = Some(now);
                }
            }
        }
        state.cursor_lonlat = frame_at(&page, at).and_then(|id| {
            let frame = page.frames.iter().find(|f| f.id == id)?;
            let projection = state.pipeline.projection(&state.doc, frame.map).ok()?;
            let vt = math::frame_transform(projection, frame, place.target());
            let q = Point::new(f64::from(pos.x), f64::from(pos.y));
            projection.inverse(vt.from_page(q))
        });
    } else {
        state.cursor_lonlat = None;
    }
    if let Some(at) = state.atlas.wheel_at
        && now - at < WHEEL_REST
    {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(WHEEL_REST));
    }

    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        state.atlas.drag = origin.map(|pos| start_drag(state, &page, place, pos, alt));
        state.atlas.drag_last = origin.unwrap_or_default();
    }
    if resp.dragged()
        && fingers.is_none()
        && !state.atlas.fingers
        && let Some(drag) = state.atlas.drag
        && let Some(pos) = resp.interact_pointer_pos()
        // The release of the first finger can come after the second finger
        // is up, so a step with no move keeps the last position.
        && pos != state.atlas.drag_last
    {
        let d = pos - std::mem::replace(&mut state.atlas.drag_last, pos);
        if drag == Drag::Page {
            pan_page(state, d);
        } else {
            let d = Vec2::new(f64::from(d.x), f64::from(d.y)) / place.pts_per_mm;
            drag_to(state, drag, place, pos, d);
        }
    }
    if resp.drag_stopped() || !any_down || state.atlas.drag.is_none() {
        state.atlas.drag = None;
        state.atlas.guides.clear();
    }

    // A second quick tap on a vertex removes it. A second quick tap off the
    // page fits the page to the screen.
    if state.atlas.tool == AtlasTool::Measure {
        measure::input(ui, state, resp, &page, place);
    } else if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let last = state.atlas.last_tap.replace((now, pos));
        let again = last.is_some_and(|(t, p)| now - t < DOUBLE_TAP && p.distance(pos) < 12.0);
        match handle_at(state, pos) {
            Some((Handle::Vertex(v), _)) if again && state.atlas.vertex == Some(v) => {
                remove_vertex(state, v);
                state.atlas.last_tap = None;
            }
            _ => {
                tap(state, &page, place, pos);
                if again && !place.rect.contains(pos) {
                    state.atlas.view = PageView::default();
                    state.atlas.last_tap = None;
                }
            }
        }
    }

    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let delete =
        ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete && !typing && state.atlas.tool == AtlasTool::Select {
        if let Some(v) = state.atlas.vertex {
            remove_vertex(state, v);
        } else if state.atlas.frame().is_some() {
            delete_frame(state);
        } else if state.atlas.item().is_some() {
            delete_item(state);
        }
    }
}

/// Deletes the selected frame and clears the selection. The items of
/// furniture of the frame link to the largest frame that stays on the page.
pub fn delete_frame(state: &mut AppState) {
    let Some(id) = state.atlas.frame() else {
        return;
    };
    let atlas = &mut state.doc.project.atlas;
    let page = atlas.page_of_frame(id);
    atlas.remove_frame(id);
    if let Some(page) = page {
        let to = atlas.page(page).and_then(largest_frame);
        atlas.relink_furniture(page, id, to);
    }
    state.doc.project.atlas.prune_maps();
    state.atlas.selection = AtlasSelection::None;
    state.atlas.vertex = None;
    state.atlas.images.remove(&id);
    state.atlas.decors.remove(&id);
    state.atlas.spills.remove(&id);
}

/// Removes the selected vertex of the selected frame.
pub fn remove_vertex(state: &mut AppState, v: Vertex) -> bool {
    let Some(id) = state.atlas.frame() else {
        return false;
    };
    let removed = edit_frame(state, id, |f, _| math::remove_vertex(&mut f.shape, v));
    if removed {
        state.atlas.vertex = None;
    }
    removed
}

fn start_drag(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    pos: Pos2,
    alt: bool,
) -> Drag {
    if state.atlas.fingers {
        return Drag::Idle;
    }
    let at = place.to_mm(pos);
    let under = frame_at(page, at);
    match state.atlas.tool {
        AtlasTool::Move => match selected_at(state, page, place, pos) {
            Some(id) if alt => {
                let frame = page.frames.iter().find(|f| f.id == id);
                let anchor = frame.map_or(at, |f| math::anchor(&f.shape));
                Drag::Rotate(id, angle(anchor, at))
            }
            Some(id) => Drag::Pan(id),
            None => Drag::Page,
        },
        AtlasTool::Measure => Drag::Page,
        AtlasTool::Select => {
            if let (Some(id), Some((handle, center))) = (state.atlas.item(), handle_at(state, pos))
            {
                return match handle {
                    Handle::Resize(i) => Drag::ItemHandle(id, i, place.to_mm(center) - at),
                    _ => Drag::Idle,
                };
            }
            if let Some(id) = item_at(page, at) {
                state.atlas.selection = AtlasSelection::Item(id);
                state.atlas.vertex = None;
                return Drag::Item(id);
            }
            if let (Some(id), Some((handle, center))) = (state.atlas.frame(), handle_at(state, pos))
            {
                let grab = place.to_mm(center) - at;
                let handle = match handle {
                    Handle::Edge(v) => {
                        let mut new = None;
                        edit_frame(state, id, |f, _| {
                            new = math::insert_vertex(&mut f.shape, v);
                            new.is_some()
                        });
                        match new {
                            Some(v) => Handle::Vertex(v),
                            None => return Drag::Idle,
                        }
                    }
                    other => other,
                };
                if let Handle::Vertex(v) = handle {
                    state.atlas.vertex = Some(v);
                }
                return Drag::Handle(id, handle, grab);
            }
            match under {
                Some(id) => {
                    if state.atlas.frame() != Some(id) {
                        state.atlas.vertex = None;
                    }
                    state.atlas.selection = AtlasSelection::Frame(id);
                    let corner = page.frames.iter().find(|f| f.id == id);
                    let corner = corner.map_or(at, |f| math::shape_bbox(&f.shape).origin());
                    Drag::Body(id, corner - at)
                }
                None => Drag::Page,
            }
        }
    }
}

/// The snap targets of the open page for a drag of a frame. `None` if
/// snapping is off.
fn snap_targets(state: &AppState, moving: FrameId) -> Option<page_snap::Targets> {
    if !state.atlas.snap {
        return None;
    }
    let grid = &state.doc.project.atlas.grid;
    let spacing = grid.on.then_some(grid.spacing_mm);
    Some(page_snap::targets(open_page(state), moving, None, spacing))
}

/// The vertices of a shape other than `v`.
fn other_vertices(shape: &FrameShape, v: Vertex) -> Vec<Point> {
    let rings = shape.rings();
    let all = rings.iter().enumerate().flat_map(|(r, ring)| {
        let ring = ring.iter().enumerate();
        ring.map(move |(i, &[x, y])| ((r, i), Point::new(x, y)))
    });
    all.filter(|(at, _)| *at != v).map(|(_, p)| p).collect()
}

/// The vertices before and after a vertex in its ring.
fn neighbors(shape: &FrameShape, (r, i): Vertex) -> Vec<Point> {
    let rings = shape.rings();
    let Some(ring) = rings.get(r).filter(|ring| ring.len() > 1 && i < ring.len()) else {
        return Vec::new();
    };
    let n = ring.len();
    let at = |k: usize| Point::new(ring[k][0], ring[k][1]);
    vec![at((i + n - 1) % n), at((i + 1) % n)]
}

/// The angle of a point about a center, in degrees clockwise on the page.
fn angle(center: Point, p: Point) -> f64 {
    (p.y - center.y).atan2(p.x - center.x).to_degrees()
}

fn drag_to(state: &mut AppState, drag: Drag, place: PagePlace, pos: Pos2, d: Vec2) {
    let at = place.to_mm(pos);
    match drag {
        Drag::Handle(id, handle, grab) => {
            let to = at + grab;
            let targets = snap_targets(state, id);
            let reach = state.globe.snap.distance / place.pts_per_mm;
            let lock = state.atlas.lock;
            let mut guides = Vec::new();
            edit_frame(state, id, |f, _| match handle {
                Handle::Resize(i) => {
                    let mut raw = f.shape.clone();
                    math::resize(&mut raw, i, to);
                    let Some(t) = &targets else {
                        return math::resize(&mut f.shape, i, to);
                    };
                    let b = math::shape_bbox(&raw);
                    let (r, g) = page_snap::snap_rect(b, page_snap::Moving::Handle(i), t, reach);
                    guides = g;
                    let snapped = FrameShape::Rectangle([r.x0, r.y0, r.x1, r.y1]);
                    let corner = math::handles(&snapped)[i].1;
                    math::resize(&mut f.shape, i, corner)
                }
                Handle::Vertex(v) => {
                    let near = neighbors(&f.shape, v);
                    let targets = targets.clone().map(|mut t| {
                        t.align = other_vertices(&f.shape, v);
                        t
                    });
                    let (to, g) = match (&targets, lock && !near.is_empty()) {
                        (t, true) => page_snap::lock_angle(to, &near, t.as_ref(), reach),
                        (Some(t), false) => page_snap::snap_point(to, t, reach),
                        (None, false) => (to, Vec::new()),
                    };
                    guides = g;
                    math::move_vertex(&mut f.shape, v, to)
                }
                Handle::Edge(_) => false,
            });
            state.atlas.guides = guides;
        }
        Drag::Body(id, grab) => {
            let targets = snap_targets(state, id);
            let reach = state.globe.snap.distance / place.pts_per_mm;
            let mut guides = Vec::new();
            edit_frame(state, id, |f, _| {
                let b = math::shape_bbox(&f.shape);
                let raw = b.with_origin(at + grab);
                let r = match &targets {
                    Some(t) => {
                        let (r, g) = page_snap::snap_rect(raw, page_snap::Moving::Body, t, reach);
                        guides = g;
                        r
                    }
                    None => raw,
                };
                let d = r.origin() - b.origin();
                math::move_shape(&mut f.shape, d);
                d != Vec2::ZERO
            });
            state.atlas.guides = guides;
        }
        Drag::Pan(id) => {
            if d != Vec2::ZERO {
                edit_frame(state, id, |f, p| math::pan(f, p, d));
            }
        }
        Drag::Rotate(id, last) => {
            let Some(anchor) = state
                .doc
                .project
                .atlas
                .frame(id)
                .map(|f| math::anchor(&f.shape))
            else {
                return;
            };
            let now = angle(anchor, at);
            let degrees = (now - last + 180.0).rem_euclid(360.0) - 180.0;
            edit_frame(state, id, |f, p| math::rotate(f, p, anchor, degrees));
            state.atlas.drag = Some(Drag::Rotate(id, now));
        }
        Drag::ItemHandle(id, i, grab) => {
            edit_item(state, id, |rec| {
                let mut shape = FrameShape::Rectangle(rec.rect);
                let done = math::resize(&mut shape, i, at + grab);
                if let FrameShape::Rectangle(r) = shape {
                    rec.rect = r;
                }
                done
            });
        }
        Drag::Item(id) => {
            if d != Vec2::ZERO {
                edit_item(state, id, |rec| {
                    let mut shape = FrameShape::Rectangle(rec.rect);
                    math::move_shape(&mut shape, d);
                    if let FrameShape::Rectangle(r) = shape {
                        rec.rect = r;
                    }
                    true
                });
            }
        }
        Drag::Page | Drag::Idle => {}
    }
}

/// A tap selects a vertex of the selected frame, a frame, the page, or
/// nothing. A tap on a frame also selects the feature below it.
fn tap(state: &mut AppState, page: &PageRecord, place: PagePlace, pos: Pos2) {
    if state.atlas.tool == AtlasTool::Select
        && let Some((handle, _)) = handle_at(state, pos)
    {
        if let Handle::Vertex(v) = handle {
            state.atlas.vertex = Some(v);
        }
        return;
    }
    let at = place.to_mm(pos);
    state.atlas.vertex = None;
    state.atlas.sizes = false;
    if let Some(id) = item_at(page, at) {
        state.atlas.selection = AtlasSelection::Item(id);
        return;
    }
    let picked = match frame_at(page, at) {
        Some(id) => {
            state.atlas.selection = AtlasSelection::Frame(id);
            let frame = page.frames.iter().find(|f| f.id == id).cloned();
            let q = Point::new(f64::from(pos.x), f64::from(pos.y));
            frame.and_then(|f| {
                let target = place.target();
                state
                    .pipeline
                    .pick_frame(&state.doc, &f, target, q)
                    .ok()
                    .flatten()
            })
        }
        None if place.rect.contains(pos) => {
            state.atlas.selection = AtlasSelection::Page;
            None
        }
        None => {
            state.atlas.selection = AtlasSelection::None;
            None
        }
    };
    if picked != state.selection {
        state.selection = picked;
        if let Some(sel) = picked {
            state.select(Some(LayerRef::Vector(sel.layer)));
        }
    }
}

/// The raster layers that a map shows and that the globe draws, from the
/// bottom.
fn map_rasters(state: &AppState, map: MapId) -> Vec<RasterId> {
    let layers = state.doc.project.layers().iter();
    let on = layers.filter(|layer| state.doc.shown(map, layer.id));
    on.filter_map(|layer| layer.raster())
        .filter(|id| state.globe.draws(*id))
        .collect()
}

/// The image layers that a map shows, from the bottom.
fn map_images(state: &AppState, map: MapId) -> Vec<vale_store::ImageId> {
    let layers = state.doc.project.layers().iter();
    let on = layers.filter(|layer| state.doc.shown(map, layer.id));
    on.filter_map(|layer| layer.image())
        .filter(|id| state.globe.holds_image(*id))
        .collect()
}

/// A frame of the page to draw in this pass.
struct Placed {
    id: FrameId,
    vt: ViewTransform,
    /// The view of the decorations of the frame.
    fixed: ViewTransform,
    shown: Rect,
    /// The triangles of the shape on the screen. The map does not move out
    /// of them.
    inside: Vec<[Pos2; 3]>,
    /// The frame draws raster layers on the GPU.
    layered: bool,
}

/// A rectangle grown to whole physical pixels. A texture on whole pixels
/// stays sharp, so a line has the same width at each angle.
fn snap(rect: Rect, ppp: f32) -> Rect {
    let to_points = |v: f32| v / ppp;
    Rect::from_min_max(
        pos2(
            to_points((rect.min.x * ppp).floor()),
            to_points((rect.min.y * ppp).floor()),
        ),
        pos2(
            to_points((rect.max.x * ppp).ceil()),
            to_points((rect.max.y * ppp).ceil()),
        ),
    )
}

/// The pixels of the part of a frame on the canvas: the left, the top, the
/// width, and the height. The pixels cover `shown`.
fn pixel_rect(shown: Rect, ppp: f32) -> [u32; 4] {
    let (x0, y0) = ((shown.min.x * ppp).round(), (shown.min.y * ppp).round());
    let (x1, y1) = ((shown.max.x * ppp).round(), (shown.max.y * ppp).round());
    let side = |a: f32, b: f32| (b - a).clamp(1.0, 8192.0) as u32;
    [
        x0.max(0.0) as u32,
        y0.max(0.0) as u32,
        side(x0, x1),
        side(y0, y1),
    ]
}

/// The mesh of the projection of a map, on a sphere of radius 1.
fn mesh_of(state: &mut AppState, spec: ProjectionSpec) -> Option<Arc<FlatMesh>> {
    if let Some((_, mesh)) = state.atlas.meshes.iter().find(|(of, _)| *of == spec) {
        return Some(mesh.clone());
    }
    let projection = Projection::new(spec, 1.0).ok()?;
    let mesh = Arc::new(FlatMesh::new(&projection));
    if state.atlas.meshes.len() >= 8 {
        state.atlas.meshes.remove(0);
    }
    state.atlas.meshes.push((spec, mesh.clone()));
    Some(mesh)
}

/// The shape of a frame as pixels of coverage, for the target of the frame.
fn shape_mask(
    state: &mut AppState,
    frame: &FrameRecord,
    place: PagePlace,
    rect: [u32; 4],
    ppp: f32,
) -> Option<Arc<Mask>> {
    let [x, y, w, h] = rect;
    let key = hash_of((&frame.shape, place.target(), rect, ppp.to_bits()));
    if let Some((of, mask)) = state.atlas.masks.get(&frame.id)
        && *of == key
    {
        return Some(mask.clone());
    }
    let top_left = vec2(x as f32, y as f32) / ppp;
    let origin = place.rect.min - top_left;
    let target = FrameTarget {
        origin: Point::new(f64::from(origin.x), f64::from(origin.y)),
        pts_per_mm: place.pts_per_mm,
    };
    let size = (f64::from(w) / f64::from(ppp), f64::from(h) / f64::from(ppp));
    let list = crate::pipeline::Pipeline::frame_mask(frame, target, size);
    let image = vale_render::render_rgba(&list, f64::from(ppp)).ok()?;
    let alpha: Vec<u8> = image.data.chunks_exact(4).map(|px| px[3]).collect();
    let size = [image.width as u32, image.height as u32];
    state.atlas.mask_count += 1;
    let mask = Arc::new(Mask {
        id: state.atlas.mask_count,
        size,
        alpha,
    });
    state.atlas.masks.insert(frame.id, (key, mask.clone()));
    Some(mask)
}

/// Composes the frames that changed and paints each frame of the page.
fn frames(ui: &egui::Ui, state: &mut AppState, place: PagePlace, area: Rect) {
    let page = open_page(state).clone();
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let moving = gesture(ui, state);
    let primary = state.atlas.frame().or(page.frames.first().map(|f| f.id));
    let target = place.target();
    let painter = ui.painter_at(area);
    // The copies of the raster layers arrive on the GPU while the globe
    // advances.
    if state.globe.advance(std::time::Instant::now()) {
        ctx.request_repaint();
    }
    let radius = state.doc.project.world.radius_km * 1000.0;
    let mut placed: Vec<Placed> = Vec::new();
    let mut draws: Vec<FrameDraw> = Vec::new();
    let mut spilled = false;
    for frame in &page.frames {
        let Some(map) = state.doc.project.atlas.map(frame.map) else {
            continue;
        };
        let rasters = map_rasters(state, frame.map);
        let images = map_images(state, frame.map);
        let (raster_looks, image_looks) = crate::raster_style::map_looks(&state.doc, frame.map);
        let layered = !rasters.is_empty() || !images.is_empty();
        let content = hash_of((
            map,
            state.map_generation,
            frame.map,
            &frame.graticule,
            neatline_key(&frame.neatline),
            &frame.border,
            page.paper,
            ppp.to_bits(),
            state.selection,
            layered,
        ));
        // Only the part of the frame on the canvas has a texture.
        // The neatline reaches out of the shape by half its width, and a sharp
        // corner reaches further.
        // The edge labels outside the shape reach further.
        let neatline = if frame.neatline.on {
            frame.neatline.width
        } else {
            0.0
        };
        let labels = crate::edge_labels::outside_reach(frame, &mut state.fonts);
        let band = crate::frame_border::reach(frame);
        let reach = neatline.max(labels).max(band);
        let reach = (reach * f64::from(place.pts_per_mm) / PT_PER_MM) as f32;
        let shown = snap(
            place
                .rect_to_screen(math::shape_bbox(&frame.shape))
                .expand(reach)
                .intersect(area),
            ppp,
        );
        if !shown.is_positive() {
            continue;
        }
        let place_key = hash_of((
            &frame.shape,
            frame.center,
            frame.scale,
            frame.rotation,
            target,
            shown,
        ));
        let Ok(projection) = state.pipeline.projection(&state.doc, frame.map) else {
            continue;
        };
        let vt = math::frame_transform(projection, frame, target);
        let fresh = match state.atlas.images.get(&frame.id) {
            None => false,
            Some(image) if image.content != content => false,
            Some(image) if image.place != place_key || !image.settled => moving,
            Some(_) => true,
        };
        if !fresh {
            let quality = if moving && !state.headless {
                Quality::Interactive
            } else {
                Quality::Final
            };
            let keys = (content, place_key);
            let primary = primary == Some(frame.id);
            let at = (place, shown);
            compose(
                &ctx,
                state,
                frame,
                at,
                vt,
                keys,
                quality,
                (primary, layered),
            );
        }
        // One spill each frame, after its frame settles.
        let spill = state.atlas.spills.get(&frame.id);
        if fresh
            && !moving
            && !spilled
            && spill.is_none_or(|s| s.content != content || s.place != place_key)
        {
            compose_spill(&ctx, state, frame, place, area, vt, (content, place_key));
            spilled = true;
            ctx.request_repaint();
        }
        if layered {
            let spec = state.doc.projection(frame.map);
            let rect = pixel_rect(shown, ppp);
            let mesh = mesh_of(state, spec);
            let mask = shape_mask(state, frame, place, rect, ppp);
            if let Some(mesh) = mesh {
                let [x, y, ..] = rect;
                let center = vt.origin;
                let units = vt.view.scale * radius;
                let scale = frame.scale;
                draws.push(FrameDraw {
                    id: frame.id.0,
                    place: FramePlace {
                        center: [
                            center.x as f32 * ppp - x as f32,
                            center.y as f32 * ppp - y as f32,
                        ],
                        map_center: [
                            (vt.view.center.x / radius) as f32,
                            (vt.view.center.y / radius) as f32,
                        ],
                        units_to_pixels: (units * f64::from(ppp)) as f32,
                        rotation: vt.view.rotation,
                        points_per_radian: radius * crate::atlas::PT_PER_MM * 1000.0 / scale,
                        pixels_per_page_point: place.pixels_per_page_point(ppp),
                    },
                    rect,
                    mesh,
                    mask,
                    shown: rasters,
                    images,
                    raster_looks,
                    image_looks,
                });
            }
        }
        let inside = shape_triangles(frame, place);
        placed.push(Placed {
            id: frame.id,
            vt,
            fixed: fixed_view(vt, target),
            shown,
            inside,
            layered,
        });
    }
    let canvas = Canvas {
        background: paper(&page),
        pixels_per_point: ppp,
    };
    // The tiles of the detail maps arrive while the frames ask for them.
    state.want_frames(draws.clone());
    if state.globe.details_loading() {
        ctx.request_repaint();
    }
    let mut callbacks: HashMap<u32, _> = if draws.is_empty() {
        HashMap::new()
    } else {
        let painted = state.globe.paint_frames(canvas, draws);
        painted.unwrap_or_default().into_iter().collect()
    };
    if let Some(format) = state.globe.backdrop() {
        let mut items = Vec::new();
        for p in &placed {
            let by = p.layered.then(|| state.atlas.unders.get(&p.id)).flatten();
            let images = [
                (by, p.vt),
                (state.atlas.images.get(&p.id), p.vt),
                (state.atlas.decors.get(&p.id), p.fixed),
            ];
            for (image, vt) in images {
                let Some(image) = image else {
                    continue;
                };
                let corners = [image.rect.min, image.rect.max].map(|c| {
                    let q = vt.to_page(
                        image
                            .vt
                            .from_page(Point::new(f64::from(c.x), f64::from(c.y))),
                    );
                    pos2(q.x as f32, q.y as f32)
                });
                let rect = Rect::from_two_pos(corners[0], corners[1]);
                items.push((image.thumb.clone(), rect.intersect(p.shown)));
            }
        }
        let scene = scene_of(area, place.rect, &items, canvas);
        let callback = SceneCallback {
            format,
            scene: Arc::new(scene),
            canvas,
        };
        painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
            area, callback,
        ));
    }
    state.atlas.painted.clear();
    for p in placed {
        let moved = (state.atlas.images.get(&p.id)).is_some_and(|image| image.vt != p.vt);
        let spill = (state.atlas.spills.get(&p.id))
            .filter(|_| moved)
            .map(|image| paint_image(&painter, image, &p.vt, (&p.inside, p.shown)));
        let by = p.layered.then(|| state.atlas.unders.get(&p.id)).flatten();
        if let Some(image) = by {
            paint_image(&painter, image, &p.vt, (&p.inside, p.shown));
        }
        if let Some(callback) = callbacks.remove(&p.id.0) {
            painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
                area, callback,
            ));
        }
        let map = (state.atlas.images.get(&p.id))
            .map(|image| paint_image(&painter, image, &p.vt, (&p.inside, p.shown)));
        let decor = (state.atlas.decors.get(&p.id))
            .map(|image| paint_image(&painter, image, &p.fixed, (&[], p.shown)));
        state.atlas.painted.insert(p.id, [map, spill, decor]);
    }
}

/// A view that follows the page and the shape of a frame, but not the
/// center, the scale, or the rotation of its map.
fn fixed_view(vt: ViewTransform, target: FrameTarget) -> ViewTransform {
    ViewTransform {
        view: View {
            center: Point::ZERO,
            scale: target.pts_per_mm,
            rotation: 0.0,
        },
        ..vt
    }
}

/// The largest side of the texture of an item of furniture, in pixels.
const ITEM_PIXELS: f64 = 4096.0;

/// Draws the items of furniture of the open page over the frames. While a
/// gesture moves the page, an item shows its last texture.
fn items(ui: &egui::Ui, state: &mut AppState, place: PagePlace, area: Rect) {
    let page = open_page(state).clone();
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let moving = gesture(ui, state);
    let painter = ui.painter_at(area);
    let zoom = place.pts_per_mm / PT_PER_MM;
    for rec in &page.furniture {
        let mm = crate::furniture::rect_of(rec);
        let outer = mm.inflate(
            crate::furniture::RENDER_MARGIN_MM,
            crate::furniture::RENDER_MARGIN_MM,
        );
        let side = outer.width().max(outer.height()) * PT_PER_MM;
        let scale = (f64::from(ppp) * zoom).min(ITEM_PIXELS / side.max(1.0));
        let atlas = &state.doc.project.atlas;
        let frame = rec.item.frame().and_then(|f| atlas.frame(f));
        let map = frame.and_then(|f| atlas.map(f.map));
        let content = hash_of((
            &rec.item,
            mm.width().to_bits(),
            mm.height().to_bits(),
            frame,
            map,
            state.map_generation,
            scale.to_bits(),
        ));
        let old = state.atlas.item_images.get(&rec.id).map(|i| i.content);
        if old != Some(content) && !(moving && old.is_some()) {
            let rendered = crate::furniture::render(
                &mut state.pipeline,
                &state.doc,
                rec,
                &mut state.fonts,
                scale,
                crate::furniture::Purpose::Screen,
            );
            let (img, _) = match rendered {
                Ok(r) => r,
                Err(e) => {
                    state.status = format!("{e:#}");
                    continue;
                }
            };
            let image =
                egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
            let options = egui::TextureOptions::LINEAR;
            let texture = match state.atlas.item_images.remove(&rec.id) {
                Some(mut old) => {
                    old.texture.set(image, options);
                    old.texture
                }
                None => ctx.load_texture(format!("item-{}", rec.id.0), image, options),
            };
            let rect = Rect::NOTHING;
            state.atlas.item_images.insert(
                rec.id,
                ItemImage {
                    texture,
                    content,
                    rect,
                },
            );
        }
        if let Some(image) = state.atlas.item_images.get_mut(&rec.id) {
            image.rect = place.rect_to_screen(outer);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(image.texture.id(), image.rect, uv, Color32::WHITE);
        }
    }
}

/// Draws the canvas of the atlas at the reduced size of the blurred
/// backdrop: the background, the page, and the textures of the frames. The
/// raster layers of a frame are not in it.
fn scene_of(area: Rect, page: Rect, items: &[(Arc<Thumb>, Rect)], canvas: Canvas) -> Scene {
    let n = REDUCTION as f32;
    let k = canvas.pixels_per_point / n;
    let px = |v: f32| (v * canvas.pixels_per_point).round().max(1.0) as u32;
    let size = [
        px(area.width()).div_ceil(REDUCTION),
        px(area.height()).div_ceil(REDUCTION),
    ];
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    (
        size,
        area.min.x.to_bits(),
        area.min.y.to_bits(),
        k.to_bits(),
    )
        .hash(&mut hasher);
    for r in std::iter::once(&page).chain(items.iter().map(|(_, r)| r)) {
        [r.min.x, r.min.y, r.max.x, r.max.y]
            .map(f32::to_bits)
            .hash(&mut hasher);
    }
    for (thumb, _) in items {
        thumb.serial.hash(&mut hasher);
    }
    canvas.background.to_array().hash(&mut hasher);
    let [w, h] = size.map(|s| s as usize);
    let back = theme::CANVAS.to_array();
    let mut rgba: Vec<u8> = (0..w * h).flat_map(|_| back).collect();
    // The rectangle in pixels of the scene.
    let pixels = |r: Rect| {
        let at = |p: Pos2, up: bool| {
            let v = (p - area.min) * k;
            let round = |c: f32| if up { c.ceil() } else { c.floor() };
            [
                round(v.x).clamp(0.0, w as f32) as usize,
                round(v.y).clamp(0.0, h as f32) as usize,
            ]
        };
        (at(r.min, false), at(r.max, true))
    };
    let (from, to) = pixels(page);
    for y in from[1]..to[1] {
        for x in from[0]..to[0] {
            let at = (y * w + x) * 4;
            rgba[at..at + 4].copy_from_slice(&canvas.background.to_array());
        }
    }
    for (thumb, rect) in items {
        let (from, to) = pixels(*rect);
        let [tw, th] = thumb.size;
        let (width, height) = ((to[0] - from[0]).max(1), (to[1] - from[1]).max(1));
        for y in from[1]..to[1] {
            for x in from[0]..to[0] {
                let tx = ((x - from[0]) * tw / width).min(tw - 1);
                let ty = ((y - from[1]) * th / height).min(th - 1);
                let t = (ty * tw + tx) * 4;
                let over = 255 - u32::from(thumb.rgba[t + 3]);
                let at = (y * w + x) * 4;
                for c in 0..3 {
                    let v = u32::from(thumb.rgba[t + c]) + u32::from(rgba[at + c]) * over / 255;
                    rgba[at + c] = v.min(255) as u8;
                }
            }
        }
    }
    Scene {
        key: hasher.finish(),
        size,
        rgba,
    }
}

/// The resolution of a spill, as a part of the resolution of the screen.
const SPILL_RESOLUTION: f32 = 0.5;

/// Composes the spill of a frame: the map in the part of the shape on the
/// canvas and a margin of half its larger side.
fn compose_spill(
    ctx: &egui::Context,
    state: &mut AppState,
    frame: &FrameRecord,
    place: PagePlace,
    area: Rect,
    vt: ViewTransform,
    (content, place_key): (u64, u64),
) {
    let ppp = ctx.pixels_per_point() * SPILL_RESOLUTION;
    let zoom = place.pts_per_mm / PT_PER_MM;
    let inner = place
        .rect_to_screen(math::shape_bbox(&frame.shape))
        .intersect(area);
    if !inner.is_positive() {
        return;
    }
    let margin = 0.5 * inner.width().max(inner.height());
    let mut shown = inner.expand(margin);
    let most = 8192.0 / ppp;
    shown.max = shown.min + shown.size().min(vec2(most, most));
    let scale = f64::from(ppp) * zoom;
    let origin = place.rect.min - shown.min;
    let target = FrameTarget {
        origin: Point::new(f64::from(origin.x) / zoom, f64::from(origin.y) / zoom),
        pts_per_mm: PT_PER_MM,
    };
    let part = kurbo::Rect::new(
        0.0,
        0.0,
        f64::from(shown.width()) / zoom,
        f64::from(shown.height()) / zoom,
    );
    let list = state
        .pipeline
        .compose_frame_spill(&state.doc, frame, target, part);
    let img = list
        .map_err(|e| format!("{e:#}"))
        .and_then(|list| vale_render::render_rgba(&list, scale).map_err(|e| format!("{e}")));
    let img = match img {
        Ok(img) => img,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
    let options = egui::TextureOptions::LINEAR;
    let texture = match state.atlas.spills.remove(&frame.id) {
        Some(mut old) => {
            old.texture.set(image, options);
            old.texture
        }
        None => ctx.load_texture(format!("spill-{}", frame.id.0), image, options),
    };
    let size = vec2(img.width as f32, img.height as f32) / ppp;
    state.atlas.spills.insert(
        frame.id,
        FrameImage {
            texture,
            thumb: Arc::new(Thumb::new(&img)),
            content,
            place: place_key,
            vt,
            rect: Rect::from_min_size(shown.min, size),
            settled: true,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn compose(
    ctx: &egui::Context,
    state: &mut AppState,
    frame: &FrameRecord,
    (place, shown): (PagePlace, Rect),
    vt: ViewTransform,
    (content, place_key): (u64, u64),
    quality: Quality,
    (primary, layered): (bool, bool),
) {
    let ppp = ctx.pixels_per_point();
    let px = |points: f32| ((points * ppp).round() as usize).clamp(1, 8192);
    let (w, h) = (px(shown.width()), px(shown.height()));
    // The list is in points of paper, so a width in the style is the width
    // on the printed page, and the texture is rendered at the scale of the
    // zoom. It starts at the top-left corner of the part of the frame on the
    // canvas.
    let zoom = f64::from(place.pts_per_mm) / PT_PER_MM;
    let scale = f64::from(ppp) * zoom;
    let origin = place.rect.min - shown.min;
    let target = FrameTarget {
        origin: Point::new(f64::from(origin.x) / zoom, f64::from(origin.y) / zoom),
        pts_per_mm: PT_PER_MM,
    };
    let part = kurbo::Rect::new(
        0.0,
        0.0,
        f64::from(shown.width()) / zoom,
        f64::from(shown.height()) / zoom,
    );
    let selection = state.selection;
    // A frame with raster layers has the water below them and the rest
    // above them. The GPU draws them between the two.
    let composed = if layered {
        state.pipeline.compose_frame_over(
            &state.doc,
            frame,
            target,
            part,
            &mut state.fonts,
            quality,
            selection,
        )
    } else {
        state.pipeline.compose_frame_part(
            &state.doc,
            frame,
            target,
            part,
            &mut state.fonts,
            quality,
            selection,
        )
    };
    state.atlas.composes += 1;
    let mut composed = match composed {
        Ok(c) => c,
        Err(e) => {
            state.status = format!("{e:#}");
            return;
        }
    };
    let (width, height) = (w as f64 / scale, h as f64 / scale);
    composed.list.width = width;
    composed.list.height = height;
    let settled = quality == Quality::Final;
    let keys = (content, place_key);
    if layered {
        let under = state
            .pipeline
            .compose_frame_under(&state.doc, frame, target, part);
        match under {
            Ok(mut list) => {
                list.width = width;
                list.height = height;
                let images = &mut state.atlas.unders;
                match upload(
                    ctx,
                    images,
                    frame.id,
                    &list,
                    scale,
                    (shown, vt),
                    keys,
                    settled,
                ) {
                    Ok(()) => {}
                    Err(e) => state.status = e,
                }
            }
            Err(e) => state.status = format!("{e:#}"),
        }
    } else {
        state.atlas.unders.remove(&frame.id);
    }
    let mut map = composed.list.clone();
    let decor = vale_render::DisplayList {
        items: map.items.split_off(composed.decor),
        ..map.clone()
    };
    let images = &mut state.atlas.images;
    if let Err(e) = upload(
        ctx,
        images,
        frame.id,
        &map,
        scale,
        (shown, vt),
        keys,
        settled,
    ) {
        state.status = e;
        return;
    }
    let clips = |item: &vale_render::Item| {
        matches!(
            item,
            vale_render::Item::PushClip { .. } | vale_render::Item::PopClip
        )
    };
    if decor.items.iter().all(clips) {
        state.atlas.decors.remove(&frame.id);
    } else {
        let fixed = fixed_view(vt, place.target());
        let images = &mut state.atlas.decors;
        if let Err(e) = upload(
            ctx,
            images,
            frame.id,
            &decor,
            scale,
            (shown, fixed),
            keys,
            settled,
        ) {
            state.status = e;
            return;
        }
    }
    if primary {
        state.composed = Some(composed);
    }
}

/// Renders a list to the texture of a frame in `images`.
fn upload(
    ctx: &egui::Context,
    images: &mut HashMap<FrameId, FrameImage>,
    id: FrameId,
    list: &vale_render::DisplayList,
    scale: f64,
    (shown, vt): (Rect, ViewTransform),
    (content, place): (u64, u64),
    settled: bool,
) -> Result<(), String> {
    let ppp = ctx.pixels_per_point();
    let img = vale_render::render_rgba(list, scale).map_err(|e| format!("{e}"))?;
    let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
    let options = egui::TextureOptions::LINEAR;
    let texture = match images.remove(&id) {
        Some(mut old) => {
            old.texture.set(image, options);
            old.texture
        }
        None => ctx.load_texture(format!("frame-{}", id.0), image, options),
    };
    let size = vec2(img.width as f32, img.height as f32) / ppp;
    images.insert(
        id,
        FrameImage {
            texture,
            thumb: Arc::new(Thumb::new(&img)),
            content,
            place,
            vt,
            rect: Rect::from_min_size(shown.min, size),
            settled,
        },
    );
    Ok(())
}

/// Paints the texture of a frame, and returns the screen rectangle of the
/// painted part. While a gesture moves the map, the texture moves with the
/// view, and it shows only inside the triangles of `inside`. With no
/// triangles, the whole texture shows.
fn paint_image(
    painter: &egui::Painter,
    image: &FrameImage,
    vt: &ViewTransform,
    (inside, clip): (&[[Pos2; 3]], Rect),
) -> Rect {
    let moved = |p: Pos2| {
        let q = Point::new(f64::from(p.x), f64::from(p.y));
        let q = vt.to_page(image.vt.from_page(q));
        pos2(q.x as f32, q.y as f32)
    };
    let back = |p: Pos2| {
        let q = Point::new(f64::from(p.x), f64::from(p.y));
        let q = image.vt.to_page(vt.from_page(q));
        pos2(q.x as f32, q.y as f32)
    };
    let r = image.rect;
    let uv = |p: Pos2| {
        let q = back(p) - r.min;
        pos2(q.x / r.width(), q.y / r.height())
    };
    let quad = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ]
    .map(moved);
    let mut mesh = egui::Mesh::with_texture(image.texture.id());
    let mut fan = |points: &[Pos2]| {
        let base = mesh.vertices.len() as u32;
        for &pos in points {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: uv(pos),
                color: Color32::WHITE,
            });
        }
        for i in 2..points.len() as u32 {
            mesh.add_triangle(base, base + i - 1, base + i);
        }
    };
    // At rest, the whole texture shows, with the smooth edge of the shape.
    if inside.is_empty() || image.vt == *vt {
        fan(&quad);
    } else {
        for triangle in inside {
            fan(&clip_convex(triangle, &quad));
        }
    }
    let rect = mesh.calc_bounds();
    painter.with_clip_rect(clip).add(mesh);
    rect
}

/// The part of the convex polygon `poly` inside the convex polygon `by`.
fn clip_convex(poly: &[Pos2], by: &[Pos2]) -> Vec<Pos2> {
    let cross = |a: Pos2, b: Pos2, p: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
    let n = by.len();
    let turn = (0..n)
        .map(|i| cross(by[i], by[(i + 1) % n], by[(i + 2) % n]))
        .sum::<f32>();
    let mut out = poly.to_vec();
    for i in 0..n {
        let (a, b) = (by[i], by[(i + 1) % n]);
        let side = |p: Pos2| cross(a, b, p) * turn.signum();
        let input = std::mem::take(&mut out);
        for (k, &p) in input.iter().enumerate() {
            let q = input[(k + 1) % input.len()];
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                out.push(p + (q - p) * (sp / (sp - sq)));
            }
        }
    }
    out
}

/// The triangles of the shape of a frame on the screen.
fn shape_triangles(frame: &FrameRecord, place: PagePlace) -> Vec<[Pos2; 3]> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut starts = Vec::new();
    for (i, ring) in frame.shape.rings().iter().enumerate() {
        if i > 0 {
            starts.push(points.len() as u32);
        }
        points.extend(ring.iter().map(|&[x, y]| {
            let p = place.to_screen(Point::new(x, y));
            [f64::from(p.x), f64::from(p.y)]
        }));
    }
    let mut indices: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(points.iter().copied(), &starts, &mut indices);
    let at = |i: u32| {
        let [x, y] = points[i as usize];
        pos2(x as f32, y as f32)
    };
    (indices.as_chunks::<3>().0.iter())
        .map(|t| t.map(at))
        .collect()
}

/// Paints the outline of the selected page or frame and the handles of the
/// selected frame.
/// The lines of the layout grid on the page. A grid that is denser than 4
/// points on the screen does not show.
fn layout_grid(ui: &egui::Ui, state: &AppState, place: PagePlace, area: Rect) {
    let grid = &state.doc.project.atlas.grid;
    let step = grid.spacing_mm * place.pts_per_mm;
    if !grid.on || step < 4.0 {
        return;
    }
    let painter = ui.painter_at(area.intersect(place.rect));
    let [r, g, b] = grid.color;
    let alpha = (grid.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(r, g, b, alpha));
    let page = open_page(state);
    let (w, h) = (page.width_mm, page.height_mm);
    let mut x = grid.spacing_mm;
    while x < w {
        let a = place.to_screen(Point::new(x, 0.0));
        let b = place.to_screen(Point::new(x, h));
        painter.line_segment([a, b], stroke);
        x += grid.spacing_mm;
    }
    let mut y = grid.spacing_mm;
    while y < h {
        let a = place.to_screen(Point::new(0.0, y));
        let b = place.to_screen(Point::new(w, y));
        painter.line_segment([a, b], stroke);
        y += grid.spacing_mm;
    }
}

/// The guide lines of the snaps that hold.
fn guides(ui: &egui::Ui, state: &AppState, place: PagePlace, area: Rect) {
    let painter = ui.painter_at(area);
    let stroke = egui::Stroke::new(1.0, GUIDE);
    for guide in &state.atlas.guides {
        match *guide {
            page_snap::Guide::Line(a, b) => {
                painter.line_segment([place.to_screen(a), place.to_screen(b)], stroke);
            }
            page_snap::Guide::Point(p) => {
                painter.circle_stroke(place.to_screen(p), 11.0, egui::Stroke::new(2.0, GUIDE));
            }
        }
    }
}

fn selection(ui: &egui::Ui, state: &mut AppState, place: PagePlace) {
    state.atlas.handles.clear();
    state.atlas.handles_of = Some(state.atlas.selection);
    // The Measure tool does not edit the frames, so the selection does not
    // show.
    if state.atlas.tool == AtlasTool::Measure {
        return;
    }
    let painter = ui.painter();
    let stroke = egui::Stroke::new(2.0, ACCENT);
    match state.atlas.selection {
        AtlasSelection::None => {}
        AtlasSelection::Page => {
            let rect = place.rect.shrink(1.0);
            painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
        }
        AtlasSelection::Item(id) => {
            let Some(rec) = state.doc.project.atlas.furniture(id) else {
                return;
            };
            let rect = place.rect_to_screen(crate::furniture::rect_of(rec));
            painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
            let dark = egui::Stroke::new(1.5, Color32::from_rgb(11, 31, 26));
            let shape = FrameShape::Rectangle(rec.rect);
            for (handle, p) in math::handles(&shape) {
                let center = place.to_screen(p);
                painter.circle(center, HANDLE_RADIUS, ACCENT, dark);
                let size = vec2(HANDLE_TARGET, HANDLE_TARGET);
                let touch = Rect::from_center_size(center, size);
                state.atlas.handles.push((handle, touch));
            }
        }
        AtlasSelection::Frame(id) => {
            let Some(frame) = state.doc.project.atlas.frame(id) else {
                return;
            };
            for ring in frame.shape.rings() {
                let mut points: Vec<Pos2> = ring
                    .iter()
                    .map(|&[x, y]| place.to_screen(Point::new(x, y)))
                    .collect();
                if let Some(&first) = points.first() {
                    points.push(first);
                }
                if frame.neatline.on {
                    // The neatline shows between the dashes.
                    painter.extend(egui::Shape::dashed_line(&points, stroke, 6.0, 5.0));
                } else {
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
            let dark = egui::Stroke::new(1.5, Color32::from_rgb(11, 31, 26));
            let mut handles = Vec::new();
            for (handle, p) in math::handles(&frame.shape) {
                let center = place.to_screen(p);
                let on = matches!(handle, Handle::Vertex(v) if state.atlas.vertex == Some(v));
                match handle {
                    Handle::Edge(_) => {
                        let r = HANDLE_RADIUS - 1.0;
                        painter.circle(center, r, Color32::from_rgb(11, 31, 26), stroke);
                        let s = egui::Stroke::new(1.5, ACCENT);
                        painter.line_segment([center - vec2(3.0, 0.0), center + vec2(3.0, 0.0)], s);
                        painter.line_segment([center - vec2(0.0, 3.0), center + vec2(0.0, 3.0)], s);
                    }
                    _ => {
                        let r = if on {
                            HANDLE_RADIUS + 2.0
                        } else {
                            HANDLE_RADIUS
                        };
                        let fill = if on { Color32::WHITE } else { ACCENT };
                        painter.circle(center, r, fill, dark);
                    }
                }
                let size = vec2(HANDLE_TARGET, HANDLE_TARGET);
                handles.push((handle, Rect::from_center_size(center, size)));
            }
            state.atlas.handles = handles;
        }
    }
}

/// Makes the shape of a frame a polygon or a rectangle.
pub fn set_polygon(state: &mut AppState, id: FrameId, polygon: bool) {
    edit_frame(state, id, |f, _| {
        f.shape = if polygon {
            math::to_polygon(&f.shape)
        } else {
            math::to_rectangle(&f.shape)
        };
        true
    });
    state.atlas.vertex = None;
}

/// Adds a hole to a polygon frame.
pub fn add_hole(state: &mut AppState, id: FrameId) {
    edit_frame(state, id, |f, _| {
        math::add_hole(&mut f.shape);
        true
    });
}

/// Sets the scale of a frame, in the range of the scales.
pub fn set_scale(state: &mut AppState, id: FrameId, scale: f64) {
    let scale = scale.clamp(math::SCALE_RANGE.0, math::SCALE_RANGE.1);
    edit_frame(state, id, |f, _| {
        f.scale = scale;
        true
    });
}

/// Sets the longitude of the center of a frame, in degrees.
pub fn set_longitude(state: &mut AppState, id: FrameId, lon: f64) {
    let lon = (lon + 180.0).rem_euclid(360.0) - 180.0;
    edit_frame(state, id, |f, _| {
        f.center[0] = lon;
        true
    });
}

/// Sets the latitude of the center of a frame, in degrees. A latitude past
/// a pole does nothing.
pub fn set_latitude(state: &mut AppState, id: FrameId, lat: f64) {
    if lat.abs() <= 90.0 {
        edit_frame(state, id, |f, _| {
            f.center[1] = lat;
            true
        });
    }
}

/// Sets the rotation of the map in a frame, in degrees.
pub fn set_rotation(state: &mut AppState, id: FrameId, degrees: f64) {
    edit_frame(state, id, |f, _| {
        f.rotation = degrees;
        true
    });
}

/// The parts of a neatline as a hash key.
fn neatline_key(b: &Neatline) -> (bool, Rgb, u64) {
    (b.on, b.color, b.width.to_bits())
}

/// Turns the graticule of a frame on or off.
pub fn set_graticule(state: &mut AppState, id: FrameId, on: bool) {
    edit_frame(state, id, |f, _| {
        f.graticule.on = on;
        true
    });
}

/// Sets the graticule interval of a frame, in degrees. `None` makes the
/// interval follow the scale.
pub fn set_graticule_interval(state: &mut AppState, id: FrameId, interval: Option<f64>) {
    let interval = interval.filter(|i| i.is_finite() && *i > 0.0);
    edit_frame(state, id, |f, _| {
        f.graticule.interval = interval;
        true
    });
}

pub fn set_graticule_color(state: &mut AppState, id: FrameId, color: Rgb) {
    edit_frame(state, id, |f, _| {
        f.graticule.color = color;
        true
    });
}

/// The edge labels of one kind of graticule line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeSet {
    #[default]
    Meridians,
    Parallels,
}

impl EdgeSet {
    pub const ALL: [EdgeSet; 2] = [EdgeSet::Meridians, EdgeSet::Parallels];

    pub fn of(self, g: &Graticule) -> &EdgeLabels {
        match self {
            EdgeSet::Meridians => &g.meridians,
            EdgeSet::Parallels => &g.parallels,
        }
    }

    pub fn of_mut(self, g: &mut Graticule) -> &mut EdgeLabels {
        match self {
            EdgeSet::Meridians => &mut g.meridians,
            EdgeSet::Parallels => &mut g.parallels,
        }
    }
}

/// Changes a set of edge labels of a frame. The change is one undo step.
pub fn edit_edge_labels(
    state: &mut AppState,
    id: FrameId,
    set: EdgeSet,
    change: impl FnOnce(&mut EdgeLabels),
) {
    edit_frame(state, id, |f, _| {
        let labels = set.of_mut(&mut f.graticule);
        let before = labels.clone();
        change(labels);
        *labels != before
    });
}

pub fn set_graticule_format(state: &mut AppState, id: FrameId, format: CoordFormat) {
    edit_frame(state, id, |f, _| {
        f.graticule.format = format;
        true
    });
}

/// The graticule interval that a frame draws, and the interval from its
/// scale, in degrees. With the graticule off, the first is the second.
pub fn graticule_intervals(state: &mut AppState, id: FrameId) -> Option<(f64, f64)> {
    let frame = state.doc.project.atlas.frame(id)?.clone();
    let projection = state.pipeline.projection(&state.doc, frame.map).ok()?;
    let auto = math::auto_graticule_interval(projection, &frame);
    let drawn = math::graticule_interval(projection, &frame).unwrap_or(auto);
    Some((drawn, auto))
}

/// Sets the graticule line width of a frame, in points, rounded to 0.1.
pub fn set_graticule_width(state: &mut AppState, id: FrameId, width: f64) {
    if !width.is_finite() {
        return;
    }
    let range = Graticule::WIDTHS;
    let width = ((width.clamp(*range.start(), *range.end()) * 10.0).round() / 10.0)
        .clamp(*range.start(), *range.end());
    edit_frame(state, id, |f, _| {
        f.graticule.width = width;
        true
    });
}

/// The map of a frame.
fn map_of(state: &AppState, id: FrameId) -> Option<vale_store::MapId> {
    state.doc.project.atlas.frame(id).map(|f| f.map)
}

/// Changes the projection of the map of a frame.
pub fn edit_projection(state: &mut AppState, id: FrameId, edit: impl FnOnce(&mut ProjectionSpec)) {
    let Some(map) = map_of(state, id) else {
        return;
    };
    let before = state.doc.projection(map);
    let mut spec = before;
    edit(&mut spec);
    state.doc.set_projection(map, spec);
    if state.doc.projection(map).to_proj() != before.to_proj() {
        state.touch();
    }
}

/// Sets the kind of the projection of the map of a frame and keeps its
/// center.
pub fn set_projection_kind(state: &mut AppState, id: FrameId, kind: ProjectionKind) {
    edit_projection(state, id, |s| *s = s.with_kind(kind));
}

/// Sets the central meridian, or the longitude of the center.
pub fn set_projection_longitude(state: &mut AppState, id: FrameId, lon: f64) {
    edit_projection(state, id, |s| s.lon0 = lon);
}

/// Sets the latitude of origin, or the latitude of the center.
pub fn set_projection_latitude(state: &mut AppState, id: FrameId, lat: f64) {
    edit_projection(state, id, |s| s.lat0 = lat);
}

/// Sets the standard parallels of a conic projection.
pub fn set_parallels(state: &mut AppState, id: FrameId, lat1: f64, lat2: f64) {
    edit_projection(state, id, |s| (s.lat1, s.lat2) = (lat1, lat2));
}

/// Turns the oblique aspect on or off.
pub fn set_oblique(state: &mut AppState, id: FrameId, on: bool) {
    edit_projection(state, id, |s| {
        s.azimuth = on.then_some(s.azimuth.unwrap_or(OBLIQUE_AZIMUTH));
    });
}

/// Sets the azimuth of the center line of an oblique aspect.
pub fn set_azimuth(state: &mut AppState, id: FrameId, azimuth: f64) {
    edit_projection(state, id, |s| s.azimuth = Some(azimuth));
}

/// The azimuth of a new oblique aspect. A tilt of 45 degrees shows well.
pub const OBLIQUE_AZIMUTH: f64 = 45.0;

/// Saves the projection of the map of a frame under the typed name. Returns
/// false if the name is empty.
pub fn save_preset(state: &mut AppState, id: FrameId) -> bool {
    let Some(map) = map_of(state, id) else {
        return false;
    };
    let name = state.atlas.preset_name.trim().to_string();
    let saved = state.doc.save_preset(map, &name);
    if saved {
        state.atlas.preset_name = name;
    }
    saved
}

/// Gives the map of a frame the projection of a custom projection, and puts
/// its name in the name field.
pub fn apply_preset(state: &mut AppState, id: FrameId, name: &str) {
    let Some(map) = map_of(state, id) else {
        return;
    };
    let before = state.doc.projection(map).to_proj();
    if state.doc.apply_preset(map, name) {
        state.atlas.preset_name = name.to_string();
        if state.doc.projection(map).to_proj() != before {
            state.touch();
        }
    }
}

/// Deletes the custom projection with the name in the name field.
pub fn delete_preset(state: &mut AppState) {
    let name = state.atlas.preset_name.clone();
    if state.doc.remove_preset(&name) {
        state.atlas.preset_name.clear();
    }
}

/// The custom projection that has the projection of the map of a frame.
pub fn current_preset(state: &AppState, id: FrameId) -> Option<String> {
    let now = state.doc.projection(map_of(state, id)?).to_proj();
    let same = |p: &&vale_store::ProjectionPreset| {
        ProjectionSpec::from_proj(&p.projection).is_some_and(|s| s.to_proj() == now)
    };
    state
        .doc
        .presets()
        .iter()
        .find(same)
        .map(|p| p.name.clone())
}

/// Moves the center of the projection of the map of a frame to the center
/// of the frame. A projection that is not azimuthal keeps its latitude.
pub fn center_projection(state: &mut AppState, id: FrameId) {
    let Some(frame) = state.doc.project.atlas.frame(id) else {
        return;
    };
    let [lon, lat] = frame.center;
    let map = frame.map;
    let mut spec = state.doc.projection(map);
    spec.lon0 = lon;
    if spec.kind.is_azimuthal() || spec.azimuth.is_some() {
        spec.lat0 = lat;
    }
    state.doc.set_projection(map, spec);
    state.touch();
}

/// Changes the open page.
pub fn edit_page(state: &mut AppState, edit: impl FnOnce(&mut PageRecord)) {
    let id = open_page(state).id;
    if let Some(page) = state.doc.project.atlas.page_mut(id) {
        edit(page);
    }
}

/// Gives the open page the size of a preset and keeps its orientation.
pub fn set_page_preset(state: &mut AppState, preset: &PagePreset) {
    edit_page(state, |page| {
        let (w, h) = (preset.width_mm, preset.height_mm);
        let (w, h) = if page.width_mm > page.height_mm {
            (h, w)
        } else {
            (w, h)
        };
        page.width_mm = w;
        page.height_mm = h;
    });
}

/// Turns the open page between portrait and landscape.
pub fn turn_page(state: &mut AppState) {
    edit_page(state, |p| std::mem::swap(&mut p.width_mm, &mut p.height_mm));
}

/// A length of a page from the text of a field, in millimeters. A length
/// outside the sides of a page is `None`.
pub fn parse_length(text: &str, units: PageUnits) -> Option<f64> {
    let factor = match units {
        PageUnits::Mm => 1.0,
        PageUnits::In => 25.4,
    };
    super::pad::raster::parse_value(text)
        .map(|v| v * factor)
        .filter(|mm| (PAGE_SIDE.0..=PAGE_SIDE.1).contains(mm))
}

/// Adds a page after the open page, with its size, units, and paper, and
/// opens it with the page selected.
pub fn add_page(state: &mut AppState) {
    let page = open_page(state);
    let (id, w, h, units) = (page.id, page.width_mm, page.height_mm, page.units);
    let paper = page.paper;
    let new = state.doc.project.atlas.add_page(Some(id), w, h, units);
    if let Some(page) = state.doc.project.atlas.page_mut(new) {
        page.paper = paper;
    }
    state.atlas.page = new;
    state.atlas.selection = AtlasSelection::Page;
}

/// Deletes the open page and opens the page before it, or else the page
/// after it. The last page stays.
pub fn delete_page(state: &mut AppState) {
    let id = open_page(state).id;
    let atlas = &mut state.doc.project.atlas;
    let index = atlas.page_index(id).unwrap_or(0);
    if atlas.remove_page(id).is_some() {
        atlas.prune_maps();
        let open = index.saturating_sub(1).min(atlas.pages.len() - 1);
        state.atlas.page = atlas.pages[open].id;
        state.atlas.selection = AtlasSelection::Page;
    }
}

/// Gives the map of a frame a new name.
pub fn rename_map(state: &mut AppState, id: FrameId, name: &str) {
    let atlas = &mut state.doc.project.atlas;
    let Some(map) = atlas.frame(id).map(|f| f.map) else {
        return;
    };
    if let Some(record) = atlas.map_mut(map) {
        record.name = name.to_string();
    }
}

/// The canvas of the desktop layout, in the space between the panels. The
/// keys V and H choose the Select tool and the Move tool.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    if !typing {
        let (v, h) = ui.input(|i| (i.key_pressed(egui::Key::V), i.key_pressed(egui::Key::H)));
        if v {
            state.atlas.tool = AtlasTool::Select;
        }
        if h {
            state.atlas.tool = AtlasTool::Move;
        }
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let back = Color32::from_rgb(58, 62, 68);
            canvas(ui, state, area, area.shrink(24.0), back);
        });
}

/// Turns the neatline of a frame on or off.
pub fn set_neatline(state: &mut AppState, id: FrameId, on: bool) {
    edit_frame(state, id, |f, _| {
        f.neatline.on = on;
        true
    });
}

pub fn set_neatline_color(state: &mut AppState, id: FrameId, color: Rgb) {
    edit_frame(state, id, |f, _| {
        f.neatline.color = color;
        true
    });
}

/// Sets the neatline width of a frame, in points, rounded to 0.25.
pub fn set_neatline_width(state: &mut AppState, id: FrameId, width: f64) {
    if !width.is_finite() {
        return;
    }
    let range = Neatline::WIDTHS;
    let width = ((width.clamp(*range.start(), *range.end()) * 4.0).round() / 4.0)
        .clamp(*range.start(), *range.end());
    edit_frame(state, id, |f, _| {
        f.neatline.width = width;
        true
    });
}

/// Sets the paper color of the open page.
pub fn set_paper(state: &mut AppState, color: Rgb) {
    edit_page(state, |p| p.paper = color);
}

/// Changes the layout grid of the pages.
pub fn edit_grid(state: &mut AppState, change: impl FnOnce(&mut vale_store::LayoutGrid)) {
    change(&mut state.doc.project.atlas.grid);
}

/// Changes the border of a frame.
pub fn edit_border(state: &mut AppState, id: FrameId, change: impl FnOnce(&mut Border)) {
    edit_frame(state, id, |f, _| {
        let before = f.border.clone();
        change(&mut f.border);
        f.border != before
    });
}

/// Turns the border of a frame on or off. The reference grid turns off
/// with the border, because its labels sit in the band.
pub fn set_border(state: &mut AppState, id: FrameId, on: bool) {
    edit_border(state, id, |b| {
        b.on = on;
        b.grid &= on;
    });
}

pub fn set_border_style(state: &mut AppState, id: FrameId, style: BorderStyle) {
    edit_border(state, id, |b| b.style = style);
}

/// Sets the first color of the border of a frame, or the second color of a
/// graduated border.
pub fn set_border_color(state: &mut AppState, id: FrameId, second: bool, color: Rgb) {
    edit_border(state, id, |b| {
        if second {
            b.color2 = color;
        } else {
            b.color = color;
        }
    });
}

/// `v` in `range`, rounded to `step`.
fn stepped(v: f64, range: std::ops::RangeInclusive<f64>, step: f64) -> f64 {
    let (lo, hi) = (*range.start(), *range.end());
    ((v.clamp(lo, hi) / step).round() * step).clamp(lo, hi)
}

/// Sets the band width of the border of a frame, in millimeters, rounded
/// to 0.25.
pub fn set_border_width(state: &mut AppState, id: FrameId, width: f64) {
    if width.is_finite() {
        let width = stepped(width, Border::WIDTHS, 0.25);
        edit_border(state, id, |b| b.width_mm = width);
    }
}

/// Sets the distance of the border of a frame from its box, in
/// millimeters, rounded to 0.5: of one side, in the order top, right,
/// bottom, left, or of all sides for `None`.
pub fn set_border_distance(state: &mut AppState, id: FrameId, side: Option<usize>, distance: f64) {
    if distance.is_finite() {
        let distance = stepped(distance, Border::DISTANCES, 0.5);
        edit_border(state, id, |b| match side {
            Some(i) => b.distances_mm[i % 4] = distance,
            None => b.distances_mm = [distance; 4],
        });
    }
}

/// Sets the gap of the border of a frame around the shape, in
/// millimeters, rounded to 0.25.
pub fn set_border_gap(state: &mut AppState, id: FrameId, gap: f64) {
    if gap.is_finite() {
        let gap = stepped(gap, Border::GAPS, 0.25);
        edit_border(state, id, |b| b.gap_mm = gap);
    }
}

/// Sets the labels of the columns of the reference grid of a frame.
pub fn set_grid_columns(state: &mut AppState, id: FrameId, columns: vale_store::GridColumns) {
    edit_border(state, id, |b| b.grid_columns = columns);
}

/// Whether the panel shows the distance of each side of the border of a
/// frame: the user asked for it, or the sides differ.
pub fn border_by_side(state: &AppState, b: &Border) -> bool {
    let d = b.distances_mm;
    state.atlas.border_sides || d.iter().any(|x| *x != d[0])
}

/// Shows or hides the distance of each side of the border of a frame. To
/// hide them, all sides take the distance of the top.
pub fn set_border_by_side(state: &mut AppState, id: FrameId, on: bool) {
    state.atlas.border_sides = on;
    if !on {
        edit_border(state, id, |b| b.distances_mm = [b.distances_mm[0]; 4]);
    }
}

/// Turns the reference grid of a frame on or off. The border turns on with
/// the grid, because the labels sit in the band.
pub fn set_border_grid(state: &mut AppState, id: FrameId, on: bool) {
    edit_border(state, id, |b| {
        b.grid = on;
        b.on |= on;
    });
}

/// Changes the text style of the reference grid of a frame. With `step`,
/// the change is one undo step.
pub fn edit_grid_text(
    state: &mut AppState,
    id: FrameId,
    step: bool,
    edit: impl FnOnce(&mut FurnitureText),
) {
    if step {
        state.save_edit();
    }
    edit_border(state, id, |b| edit(&mut b.grid_text));
    if step {
        state.save_edit();
    }
}

/// Turns the stroke of the border of a frame on or off.
pub fn set_border_stroke(state: &mut AppState, id: FrameId, on: bool) {
    edit_border(state, id, |b| b.stroke = on);
}

pub fn set_border_stroke_color(state: &mut AppState, id: FrameId, color: Rgb) {
    edit_border(state, id, |b| b.stroke_color = color);
}

/// Sets the stroke width of the border of a frame, in points, rounded to
/// 0.25.
pub fn set_border_stroke_width(state: &mut AppState, id: FrameId, width: f64) {
    if width.is_finite() {
        let width = stepped(width, Border::STROKE_WIDTHS, 0.25);
        edit_border(state, id, |b| b.stroke_width = width);
    }
}
