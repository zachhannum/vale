//! The atlas: the maps, and the pages with their map frames.
//!
//! A page coordinate is in millimeters from the top-left corner of the page,
//! with y down.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use crate::gpkg::{Reader, hex, parse_color};
pub use crate::scale_bar::ScaleBarItem;
use crate::{LayerId, LonLat, Rgb, Units};

/// The ID of a map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MapId(pub u32);

/// The ID of a page.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u32);

/// The ID of a map frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u32);

/// A map: a projection and the layers that it does not show.
#[derive(Clone, Debug, PartialEq)]
pub struct MapRecord {
    pub id: MapId,
    pub name: String,
    /// The PROJ string of the projection, with no radius.
    pub projection: String,
    /// The layers that the map does not show.
    pub hidden: BTreeSet<LayerId>,
    /// The style overrides of each layer, as the JSON text of `vale_style::Overrides`. A layer with no overrides has no entry.
    pub overrides: BTreeMap<LayerId, String>,
    /// The label classes of each layer in the map, as JSON text. A layer with no entry uses the label style of the layer. An entry with no class means that the layer has no labels in the map.
    pub labels: BTreeMap<LayerId, String>,
}

/// The units in which the app shows the size of a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageUnits {
    #[default]
    Mm,
    In,
}

impl PageUnits {
    pub const ALL: [PageUnits; 2] = [PageUnits::Mm, PageUnits::In];

    /// The name of the units in the project file.
    pub fn id(self) -> &'static str {
        match self {
            PageUnits::Mm => "mm",
            PageUnits::In => "in",
        }
    }

    pub fn from_id(id: &str) -> Option<PageUnits> {
        PageUnits::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// A page, its map frames, and its furniture.
#[derive(Clone, Debug, PartialEq)]
pub struct PageRecord {
    pub id: PageId,
    /// Free text. An empty name shows as the number of the page.
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub units: PageUnits,
    /// The color of the paper.
    pub paper: Rgb,
    /// From the bottom frame to the top frame.
    pub frames: Vec<FrameRecord>,
    /// From the bottom item to the top item.
    pub furniture: Vec<FurnitureRecord>,
}

/// The outline of a map frame, in page millimeters.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameShape {
    /// `[x0, y0, x1, y1]`.
    Rectangle([f64; 4]),
    /// Ring 0 is the outer ring, and each other ring is a hole. The fill
    /// rule is even-odd.
    Polygon(Vec<Vec<[f64; 2]>>),
}

impl FrameShape {
    /// The name of the kind of shape in the project file.
    pub fn kind_id(&self) -> &'static str {
        match self {
            FrameShape::Rectangle(_) => "rectangle",
            FrameShape::Polygon(_) => "polygon",
        }
    }

    /// The rings of the shape. A rectangle has one ring of 4 corners.
    pub fn rings(&self) -> Vec<Vec<[f64; 2]>> {
        match self {
            FrameShape::Rectangle([x0, y0, x1, y1]) => {
                vec![vec![[*x0, *y0], [*x1, *y0], [*x1, *y1], [*x0, *y1]]]
            }
            FrameShape::Polygon(rings) => rings.clone(),
        }
    }

    /// The shape of a kind and its rings, as `rings` gives them. `None`: the
    /// rings do not fit the kind.
    pub fn from_rings(kind: &str, rings: Vec<Vec<[f64; 2]>>) -> Option<FrameShape> {
        match kind {
            "rectangle" => match rings.as_slice() {
                [ring] if ring.len() == 4 => {
                    let ([x0, y0], [x1, y1]) = (ring[0], ring[2]);
                    Some(FrameShape::Rectangle([x0, y0, x1, y1]))
                }
                _ => None,
            },
            "polygon" if !rings.is_empty() => Some(FrameShape::Polygon(rings)),
            _ => None,
        }
    }
}

/// A view of a map on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRecord {
    pub id: FrameId,
    pub map: MapId,
    pub shape: FrameShape,
    /// The place at the middle of the frame.
    pub center: LonLat,
    /// N of the scale 1:N.
    pub scale: f64,
    /// Degrees clockwise.
    pub rotation: f64,
    pub graticule: Graticule,
    pub neatline: Neatline,
    pub border: Border,
}

/// The line along the edge of the shape of a map frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Neatline {
    pub on: bool,
    pub color: Rgb,
    /// The line width in points.
    pub width: f64,
}

impl Neatline {
    /// The line widths in points that the app offers.
    pub const WIDTHS: RangeInclusive<f64> = 0.25..=6.0;
}

impl Default for Neatline {
    fn default() -> Self {
        Neatline {
            on: true,
            color: [0x2b, 0x2b, 0x2b],
            width: 1.0,
        }
    }
}

/// A grid of lines on the pages that frames snap to. The grid shows only
/// on the screen, and an export leaves it out.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutGrid {
    pub on: bool,
    /// The distance between two lines.
    pub spacing_mm: f64,
    pub color: Rgb,
    /// From 0 to 1.
    pub opacity: f64,
}

impl LayoutGrid {
    /// The spacings in millimeters that the app offers.
    pub const SPACINGS: RangeInclusive<f64> = 1.0..=100.0;

    /// The grid as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "on": self.on,
            "spacing": self.spacing_mm,
            "color": hex(self.color),
            "opacity": self.opacity,
        })
        .to_string()
    }

    /// Reads [`LayoutGrid::to_json`]. A missing key gets its default. `None`
    /// if a value is bad.
    pub fn from_json(text: &str) -> Option<LayoutGrid> {
        let map = json_object(text)?;
        let mut g = LayoutGrid::default();
        read_flag(&map, "on", &mut g.on)?;
        read_number(&map, "spacing", &mut g.spacing_mm)?;
        read_color(&map, "color", &mut g.color)?;
        read_number(&map, "opacity", &mut g.opacity)?;
        (g.spacing_mm > 0.0 && (0.0..=1.0).contains(&g.opacity)).then_some(g)
    }
}

impl Default for LayoutGrid {
    fn default() -> Self {
        LayoutGrid {
            on: false,
            spacing_mm: 10.0,
            color: [0x3d, 0x8b, 0xfd],
            opacity: 0.35,
        }
    }
}

/// A band around a map frame, at a distance outside the bounding box of its
/// shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Border {
    pub on: bool,
    pub style: BorderStyle,
    pub color: Rgb,
    /// The second color of a graduated band.
    pub color2: Rgb,
    /// The width of the band in millimeters.
    pub width_mm: f64,
    /// The distance in millimeters from the bounding box to the band, at
    /// the top, the right, the bottom, and the left. A negative distance
    /// puts that side inside the box, and the shape covers the band there.
    pub distances_mm: [f64; 4],
    /// Where a part of the shape crosses the band, the space in millimeters
    /// between the shape and the band.
    pub gap_mm: f64,
    /// A line on the inner and the outer edge of the band.
    pub stroke: bool,
    pub stroke_color: Rgb,
    /// The stroke width in points.
    pub stroke_width: f64,
    /// The reference grid: a letter in the band for each interval of the
    /// meridians, and a number for each interval of the parallels.
    pub grid: bool,
    /// The text of the labels of the reference grid.
    pub grid_text: FurnitureText,
    /// The labels of the columns. The rows have the other kind.
    pub grid_columns: GridColumns,
}

/// The labels of the columns of a reference grid.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridColumns {
    /// A, B, C for the columns, and 1, 2, 3 for the rows.
    #[default]
    Letters,
    /// 1, 2, 3 for the columns, and A, B, C for the rows.
    Numbers,
}

impl GridColumns {
    pub const ALL: [GridColumns; 2] = [GridColumns::Letters, GridColumns::Numbers];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            GridColumns::Letters => "letters",
            GridColumns::Numbers => "numbers",
        }
    }

    pub fn from_id(id: &str) -> Option<GridColumns> {
        GridColumns::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// How a border band is colored.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    /// One color.
    #[default]
    Solid,
    /// Two colors, which change at each graticule line.
    Graduated,
}

impl BorderStyle {
    pub const ALL: [BorderStyle; 2] = [BorderStyle::Solid, BorderStyle::Graduated];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            BorderStyle::Solid => "solid",
            BorderStyle::Graduated => "graduated",
        }
    }

    pub fn from_id(id: &str) -> Option<BorderStyle> {
        BorderStyle::ALL.into_iter().find(|s| s.id() == id)
    }
}

impl Border {
    /// The band widths in millimeters that the app offers.
    pub const WIDTHS: RangeInclusive<f64> = 0.25..=10.0;
    /// The distances in millimeters that the app offers.
    pub const DISTANCES: RangeInclusive<f64> = -100.0..=30.0;
    /// The gaps in millimeters that the app offers.
    pub const GAPS: RangeInclusive<f64> = 0.0..=10.0;
    /// The stroke widths in points that the app offers.
    pub const STROKE_WIDTHS: RangeInclusive<f64> = 0.25..=4.0;

    /// The border as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "on": self.on,
            "style": self.style.id(),
            "color": hex(self.color),
            "color2": hex(self.color2),
            "width": self.width_mm,
            "distances": self.distances_mm,
            "gap": self.gap_mm,
            "grid_columns": self.grid_columns.id(),
            "stroke": self.stroke,
            "stroke_color": hex(self.stroke_color),
            "stroke_width": self.stroke_width,
            "grid": self.grid,
            "grid_text": self.grid_text.value(),
        })
        .to_string()
    }

    /// Reads [`Border::to_json`]. A missing key gets its default. `None` if
    /// a value is bad.
    pub fn from_json(text: &str) -> Option<Border> {
        let map = json_object(text)?;
        let mut b = Border::default();
        read_flag(&map, "on", &mut b.on)?;
        if let Some(v) = map.get("style") {
            b.style = BorderStyle::from_id(v.as_str()?)?;
        }
        read_color(&map, "color", &mut b.color)?;
        read_color(&map, "color2", &mut b.color2)?;
        read_number(&map, "width", &mut b.width_mm)?;
        if let Some(v) = map.get("distances") {
            let list = v.as_array().filter(|l| l.len() == 4)?;
            for (to, v) in b.distances_mm.iter_mut().zip(list) {
                *to = v.as_f64().filter(|x| x.is_finite())?;
            }
        }
        read_number(&map, "gap", &mut b.gap_mm)?;
        if let Some(v) = map.get("grid_columns") {
            b.grid_columns = GridColumns::from_id(v.as_str()?)?;
        }
        read_flag(&map, "stroke", &mut b.stroke)?;
        read_color(&map, "stroke_color", &mut b.stroke_color)?;
        read_number(&map, "stroke_width", &mut b.stroke_width)?;
        read_flag(&map, "grid", &mut b.grid)?;
        b.grid_text = b.grid_text.read_key(&map, "grid_text")?;
        (b.width_mm > 0.0 && b.stroke_width > 0.0 && b.gap_mm >= 0.0).then_some(b)
    }
}

impl Default for Border {
    fn default() -> Self {
        Border {
            on: false,
            style: BorderStyle::Solid,
            color: [0xc9, 0xaa, 0xa0],
            color2: [0xff, 0xff, 0xff],
            width_mm: 2.0,
            distances_mm: [8.0; 4],
            gap_mm: 1.0,
            stroke: false,
            stroke_color: [0x2b, 0x2b, 0x2b],
            stroke_width: 0.5,
            grid: false,
            grid_text: FurnitureText {
                color: [0x2b, 0x2b, 0x2b],
                ..FurnitureText::sized(5.0)
            },
            grid_columns: GridColumns::Letters,
        }
    }
}

/// The lines of latitude and longitude in a map frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Graticule {
    pub on: bool,
    /// Degrees between two lines. `None`: the frame picks the interval from
    /// its scale.
    pub interval: Option<f64>,
    pub color: Rgb,
    /// The line width in points.
    pub width: f64,
    pub format: CoordFormat,
    /// The labels where the meridians meet the edge of the frame.
    pub meridians: EdgeLabels,
    /// The labels where the parallels meet the edge of the frame.
    pub parallels: EdgeLabels,
}

/// The labels of one kind of graticule line at the edge of a map frame.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeLabels {
    pub on: bool,
    /// The font family. `None`: the default family of the app.
    pub font: Option<String>,
    pub weight: f32,
    pub italic: bool,
    /// The text size in points.
    pub size: f64,
    pub color: Rgb,
    /// 0 to 1.
    pub opacity: f64,
    pub halo: bool,
    pub halo_color: Rgb,
    /// The halo width in points.
    pub halo_width: f64,
    /// The labels are outside the shape of the frame, not inside it.
    pub outside: bool,
    pub edges: Edges,
    /// The distance from the edge to the text, in points.
    pub offset: f64,
    pub orientation: EdgeOrientation,
}

/// The sides of a frame that have edge labels. An edge of a polygon is on
/// the side that it faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// The direction of the text of an edge label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EdgeOrientation {
    /// Along the graticule line.
    #[default]
    Along,
    /// Level on the page.
    Horizontal,
}

impl EdgeOrientation {
    pub const ALL: [EdgeOrientation; 2] = [EdgeOrientation::Along, EdgeOrientation::Horizontal];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            EdgeOrientation::Along => "along",
            EdgeOrientation::Horizontal => "horizontal",
        }
    }

    pub fn from_id(id: &str) -> Option<EdgeOrientation> {
        EdgeOrientation::ALL.into_iter().find(|o| o.id() == id)
    }
}

impl EdgeLabels {
    /// The text sizes in points that the app offers.
    pub const SIZES: RangeInclusive<f64> = 4.0..=24.0;
    /// The halo widths in points that the app offers.
    pub const HALO_WIDTHS: RangeInclusive<f64> = 0.5..=8.0;
    /// The offsets in points that the app offers.
    pub const OFFSETS: RangeInclusive<f64> = 0.0..=20.0;

    /// The set as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "on": self.on,
            "font": self.font,
            "weight": self.weight,
            "italic": self.italic,
            "size": self.size,
            "color": hex(self.color),
            "opacity": self.opacity,
            "halo": self.halo,
            "halo_color": hex(self.halo_color),
            "halo_width": self.halo_width,
            "outside": self.outside,
            "edges": {
                "top": self.edges.top,
                "bottom": self.edges.bottom,
                "left": self.edges.left,
                "right": self.edges.right,
            },
            "offset": self.offset,
            "orientation": self.orientation.id(),
        })
        .to_string()
    }

    /// Reads [`EdgeLabels::to_json`]. A missing key gets its default, and
    /// an unknown key is ignored. `None` if a value is bad.
    pub fn from_json(text: &str) -> Option<EdgeLabels> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let map = value.as_object()?;
        let mut e = EdgeLabels::default();
        let flag = |key: &str, to: &mut bool| -> Option<()> {
            if let Some(v) = map.get(key) {
                *to = v.as_bool()?;
            }
            Some(())
        };
        let number = |key: &str, to: &mut f64| -> Option<()> {
            if let Some(v) = map.get(key) {
                *to = v.as_f64().filter(|x| x.is_finite())?;
            }
            Some(())
        };
        let color = |key: &str, to: &mut Rgb| -> Option<()> {
            if let Some(v) = map.get(key) {
                *to = parse_color(v.as_str()?)?;
            }
            Some(())
        };
        flag("on", &mut e.on)?;
        match map.get("font") {
            None | Some(serde_json::Value::Null) => {}
            Some(v) => e.font = Some(v.as_str()?.to_string()),
        }
        let mut weight = e.weight as f64;
        number("weight", &mut weight)?;
        e.weight = weight as f32;
        flag("italic", &mut e.italic)?;
        number("size", &mut e.size)?;
        color("color", &mut e.color)?;
        number("opacity", &mut e.opacity)?;
        flag("halo", &mut e.halo)?;
        color("halo_color", &mut e.halo_color)?;
        number("halo_width", &mut e.halo_width)?;
        flag("outside", &mut e.outside)?;
        if let Some(edges) = map.get("edges") {
            let edges = edges.as_object()?;
            for (key, to) in [
                ("top", &mut e.edges.top),
                ("bottom", &mut e.edges.bottom),
                ("left", &mut e.edges.left),
                ("right", &mut e.edges.right),
            ] {
                if let Some(v) = edges.get(key) {
                    *to = v.as_bool()?;
                }
            }
        }
        number("offset", &mut e.offset)?;
        if let Some(v) = map.get("orientation") {
            e.orientation = EdgeOrientation::from_id(v.as_str()?)?;
        }
        let valid = e.size > 0.0
            && e.halo_width > 0.0
            && e.offset >= 0.0
            && (0.0..=1.0).contains(&e.opacity)
            && e.weight > 0.0;
        valid.then_some(e)
    }
}

impl Default for EdgeLabels {
    fn default() -> Self {
        EdgeLabels {
            on: true,
            font: None,
            weight: 400.0,
            italic: false,
            size: 7.0,
            color: [0x4b, 0x52, 0x57],
            opacity: 1.0,
            halo: false,
            halo_color: [0xff, 0xff, 0xff],
            halo_width: 2.0,
            outside: false,
            edges: Edges {
                top: true,
                bottom: true,
                left: true,
                right: true,
            },
            offset: 2.0,
            orientation: EdgeOrientation::Along,
        }
    }
}

/// The text of a latitude or a longitude.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CoordFormat {
    /// Degrees, minutes, and seconds, for example 45°30′ N.
    #[default]
    Dms,
    /// Decimal degrees, for example 45.5° N.
    Decimal,
}

impl CoordFormat {
    pub const ALL: [CoordFormat; 2] = [CoordFormat::Dms, CoordFormat::Decimal];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            CoordFormat::Dms => "dms",
            CoordFormat::Decimal => "decimal",
        }
    }

    pub fn from_id(id: &str) -> Option<CoordFormat> {
        CoordFormat::ALL.into_iter().find(|f| f.id() == id)
    }
}

impl Graticule {
    /// The line widths in points that the app offers.
    pub const WIDTHS: RangeInclusive<f64> = 0.1..=2.0;
}

impl Default for Graticule {
    fn default() -> Self {
        Graticule {
            on: true,
            interval: None,
            color: [0x96, 0xa5, 0xaf],
            width: 0.5,
            format: CoordFormat::Dms,
            meridians: EdgeLabels::default(),
            parallels: EdgeLabels::default(),
        }
    }
}

/// The ID of an item of furniture.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FurnitureId(pub u32);

/// An item of furniture on a page: text, a legend, a scale bar, or a north
/// arrow.
#[derive(Clone, Debug, PartialEq)]
pub struct FurnitureRecord {
    pub id: FurnitureId,
    /// `[x0, y0, x1, y1]`.
    pub rect: [f64; 4],
    pub item: Furniture,
}

/// The kind of an item of furniture and its properties.
#[derive(Clone, Debug, PartialEq)]
pub enum Furniture {
    Text(TextItem),
    Legend(LegendItem),
    ScaleBar(ScaleBarItem),
    NorthArrow(NorthArrowItem),
}

/// The text style of a part of an item of furniture. It has the text
/// settings of a label class.
#[derive(Clone, Debug, PartialEq)]
pub struct FurnitureText {
    /// The font family. `None`: the default family of the app.
    pub font: Option<String>,
    pub weight: f32,
    pub italic: bool,
    /// The text size in points.
    pub size: f64,
    pub color: Rgb,
    pub case: TextCase,
    /// A fraction of `size`. 0.05 adds 5 % of the size between letters.
    pub letter_spacing: f64,
    /// 0 to 1, for the text and its halo.
    pub opacity: f64,
    pub halo: bool,
    pub halo_color: Rgb,
    /// The halo width in points.
    pub halo_width: f64,
}

/// How the case of a text changes.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextCase {
    #[default]
    Normal,
    Upper,
    Lower,
    Title,
    /// Small capitals for the lowercase letters. The labeler shapes them.
    SmallCaps,
    /// Small capitals for all letters. The labeler shapes them.
    AllSmallCaps,
}

impl TextCase {
    pub const ALL: [TextCase; 6] = [
        TextCase::Normal,
        TextCase::Upper,
        TextCase::Lower,
        TextCase::Title,
        TextCase::SmallCaps,
        TextCase::AllSmallCaps,
    ];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            TextCase::Normal => "Normal",
            TextCase::Upper => "Uppercase",
            TextCase::Lower => "Lowercase",
            TextCase::Title => "Title case",
            TextCase::SmallCaps => "Small caps",
            TextCase::AllSmallCaps => "All small caps",
        }
    }

    /// The text in this case. The small caps cases give the text unchanged.
    pub fn apply(self, text: &str) -> String {
        match self {
            TextCase::Upper => text.to_uppercase(),
            TextCase::Lower => text.to_lowercase(),
            TextCase::Title => title_case(text),
            TextCase::Normal | TextCase::SmallCaps | TextCase::AllSmallCaps => text.to_owned(),
        }
    }

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            TextCase::Normal => "normal",
            TextCase::Upper => "upper",
            TextCase::Lower => "lower",
            TextCase::Title => "title",
            TextCase::SmallCaps => "small_caps",
            TextCase::AllSmallCaps => "all_small_caps",
        }
    }

    pub fn from_id(id: &str) -> Option<TextCase> {
        TextCase::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// `s` with the first letter of each word in uppercase and the other
/// letters in lowercase.
pub fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// The horizontal alignment of text in its rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A box of free text, for example a title.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub text: String,
    pub style: FurnitureText,
    pub align: TextAlign,
}

/// A legend of the layers of the map of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendItem {
    /// `None`: the legend has no frame.
    pub frame: Option<FrameId>,
    pub title: String,
    pub title_style: FurnitureText,
    pub entry_style: FurnitureText,
    /// The changes to the entries, keyed by layer and rule name. An entry
    /// with no change has no key.
    pub entries: BTreeMap<(LayerId, String), LegendEntry>,
}

/// A change to one entry of a legend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegendEntry {
    /// `None`: the entry shows the name of the rule.
    pub name: Option<String>,
    pub hidden: bool,
}

/// A north arrow that points to the north of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct NorthArrowItem {
    /// `None`: the arrow has no frame.
    pub frame: Option<FrameId>,
    /// The arrow is not shown if the direction of north is not the same
    /// across the frame.
    pub hide_if_varies: bool,
    pub color: Rgb,
    /// The style of the letter N.
    pub letter_style: FurnitureText,
}

impl Furniture {
    /// The name of the kind in a project file.
    pub fn kind_id(&self) -> &'static str {
        match self {
            Furniture::Text(_) => "text",
            Furniture::Legend(_) => "legend",
            Furniture::ScaleBar(_) => "scale_bar",
            Furniture::NorthArrow(_) => "north_arrow",
        }
    }

    /// The frame of a legend, a scale bar, or a north arrow.
    pub fn frame(&self) -> Option<FrameId> {
        match self {
            Furniture::Text(_) => None,
            Furniture::Legend(i) => i.frame,
            Furniture::ScaleBar(i) => i.frame,
            Furniture::NorthArrow(i) => i.frame,
        }
    }

    /// Text has no frame, and it does not change.
    pub fn set_frame(&mut self, frame: Option<FrameId>) {
        match self {
            Furniture::Text(_) => {}
            Furniture::Legend(i) => i.frame = frame,
            Furniture::ScaleBar(i) => i.frame = frame,
            Furniture::NorthArrow(i) => i.frame = frame,
        }
    }

    /// The properties as JSON text, for a project file. The JSON does not
    /// hold the frame.
    pub fn to_json(&self) -> String {
        match self {
            Furniture::Text(i) => i.to_json(),
            Furniture::Legend(i) => i.to_json(),
            Furniture::ScaleBar(i) => i.to_json(),
            Furniture::NorthArrow(i) => i.to_json(),
        }
    }

    /// Reads [`Furniture::to_json`] of the kind `kind`, with the frame
    /// `frame`. `None` if the kind or a value is bad.
    pub fn from_json(kind: &str, text: &str, frame: Option<FrameId>) -> Option<Furniture> {
        let mut item = match kind {
            "text" => Furniture::Text(TextItem::from_json(text)?),
            "legend" => Furniture::Legend(LegendItem::from_json(text)?),
            "scale_bar" => Furniture::ScaleBar(ScaleBarItem::from_json(text)?),
            "north_arrow" => Furniture::NorthArrow(NorthArrowItem::from_json(text)?),
            _ => return None,
        };
        item.set_frame(frame);
        Some(item)
    }
}

pub(crate) type JsonMap = serde_json::Map<String, serde_json::Value>;

/// The JSON object of `text`.
pub(crate) fn json_object(text: &str) -> Option<JsonMap> {
    match serde_json::from_str(text).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

pub(crate) fn read_flag(map: &JsonMap, key: &str, to: &mut bool) -> Option<()> {
    if let Some(v) = map.get(key) {
        *to = v.as_bool()?;
    }
    Some(())
}

pub(crate) fn read_number(map: &JsonMap, key: &str, to: &mut f64) -> Option<()> {
    if let Some(v) = map.get(key) {
        *to = v.as_f64().filter(|x| x.is_finite())?;
    }
    Some(())
}

pub(crate) fn read_color(map: &JsonMap, key: &str, to: &mut Rgb) -> Option<()> {
    if let Some(v) = map.get(key) {
        *to = parse_color(v.as_str()?)?;
    }
    Some(())
}

pub(crate) fn read_string(map: &JsonMap, key: &str, to: &mut String) -> Option<()> {
    if let Some(v) = map.get(key) {
        *to = v.as_str()?.to_string();
    }
    Some(())
}

impl FurnitureText {
    /// The text sizes in points that the app offers.
    pub const SIZES: RangeInclusive<f64> = 4.0..=72.0;
    /// The letter spacings, as a fraction of the size, that the app offers.
    pub const LETTER_SPACINGS: RangeInclusive<f64> = 0.0..=1.0;
    /// The halo widths in points that the app offers.
    pub const HALO_WIDTHS: RangeInclusive<f64> = 0.5..=8.0;

    /// The default style with the size `size`.
    pub fn sized(size: f64) -> FurnitureText {
        FurnitureText {
            size,
            ..FurnitureText::default()
        }
    }

    /// Puts the keys of the style into `map`.
    pub(crate) fn put(&self, map: &mut JsonMap) {
        map.insert("font".into(), serde_json::json!(self.font));
        map.insert("weight".into(), serde_json::json!(self.weight));
        map.insert("italic".into(), self.italic.into());
        map.insert("size".into(), serde_json::json!(self.size));
        map.insert("color".into(), hex(self.color).into());
        map.insert("case".into(), self.case.id().into());
        map.insert(
            "letter_spacing".into(),
            serde_json::json!(self.letter_spacing),
        );
        map.insert("opacity".into(), serde_json::json!(self.opacity));
        map.insert("halo".into(), self.halo.into());
        map.insert("halo_color".into(), hex(self.halo_color).into());
        map.insert("halo_width".into(), serde_json::json!(self.halo_width));
    }

    /// The style as a JSON object.
    pub(crate) fn value(&self) -> serde_json::Value {
        let mut map = JsonMap::new();
        self.put(&mut map);
        serde_json::Value::Object(map)
    }

    /// Reads the keys of the style from `map` over `self`.
    pub(crate) fn read(mut self, map: &JsonMap) -> Option<FurnitureText> {
        match map.get("font") {
            None | Some(serde_json::Value::Null) => {}
            Some(v) => self.font = Some(v.as_str()?.to_string()),
        }
        let mut weight = self.weight as f64;
        read_number(map, "weight", &mut weight)?;
        self.weight = weight as f32;
        read_flag(map, "italic", &mut self.italic)?;
        read_number(map, "size", &mut self.size)?;
        read_color(map, "color", &mut self.color)?;
        if let Some(v) = map.get("case") {
            self.case = TextCase::from_id(v.as_str()?)?;
        }
        read_number(map, "letter_spacing", &mut self.letter_spacing)?;
        read_number(map, "opacity", &mut self.opacity)?;
        read_flag(map, "halo", &mut self.halo)?;
        read_color(map, "halo_color", &mut self.halo_color)?;
        read_number(map, "halo_width", &mut self.halo_width)?;
        let valid = self.size > 0.0
            && self.weight > 0.0
            && self.halo_width > 0.0
            && (0.0..=1.0).contains(&self.opacity);
        valid.then_some(self)
    }

    /// Reads the object under `key` of `map` over `self`. A missing key
    /// keeps `self`.
    pub(crate) fn read_key(self, map: &JsonMap, key: &str) -> Option<FurnitureText> {
        match map.get(key) {
            None => Some(self),
            Some(v) => self.read(v.as_object()?),
        }
    }
}

impl Default for FurnitureText {
    fn default() -> Self {
        FurnitureText {
            font: None,
            weight: 400.0,
            italic: false,
            size: 10.0,
            color: [0x2b, 0x2b, 0x2b],
            case: TextCase::Normal,
            letter_spacing: 0.0,
            opacity: 1.0,
            halo: false,
            halo_color: [0xff, 0xff, 0xff],
            halo_width: 1.5,
        }
    }
}

impl TextAlign {
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }

    pub fn from_id(id: &str) -> Option<TextAlign> {
        TextAlign::ALL.into_iter().find(|a| a.id() == id)
    }
}

impl TextItem {
    /// The item as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        let mut map = JsonMap::new();
        map.insert("text".into(), self.text.clone().into());
        self.style.put(&mut map);
        map.insert("align".into(), self.align.id().into());
        serde_json::Value::Object(map).to_string()
    }

    /// Reads [`TextItem::to_json`]. A missing key gets its default, and an
    /// unknown key is ignored. `None` if a value is bad.
    pub fn from_json(text: &str) -> Option<TextItem> {
        let map = json_object(text)?;
        let mut item = TextItem::default();
        read_string(&map, "text", &mut item.text)?;
        item.style = item.style.read(&map)?;
        if let Some(v) = map.get("align") {
            item.align = TextAlign::from_id(v.as_str()?)?;
        }
        Some(item)
    }
}

impl Default for TextItem {
    fn default() -> Self {
        TextItem {
            text: "Text".to_string(),
            style: FurnitureText::sized(18.0),
            align: TextAlign::Left,
        }
    }
}

impl LegendItem {
    /// The item with no frame as JSON text, for a project file. The entries
    /// are a list of objects with the keys `layer`, `rule`, `name`, and
    /// `hidden`.
    pub fn to_json(&self) -> String {
        let mut map = JsonMap::new();
        map.insert("title".into(), self.title.clone().into());
        map.insert("title_style".into(), self.title_style.value());
        map.insert("entry_style".into(), self.entry_style.value());
        let entries = self.entries.iter().map(|((layer, rule), e)| {
            serde_json::json!({
                "layer": layer.0,
                "rule": rule,
                "name": e.name,
                "hidden": e.hidden,
            })
        });
        map.insert("entries".into(), entries.collect());
        serde_json::Value::Object(map).to_string()
    }

    /// Reads [`LegendItem::to_json`]. A missing key gets its default, and
    /// an unknown key is ignored. `None` if a value is bad. The keys of a
    /// text style at the top of the object apply to the title and the
    /// entries.
    pub fn from_json(text: &str) -> Option<LegendItem> {
        let map = json_object(text)?;
        let mut item = LegendItem::default();
        read_string(&map, "title", &mut item.title)?;
        item.title_style = (item.title_style.read(&map)?).read_key(&map, "title_style")?;
        item.entry_style = (item.entry_style.read(&map)?).read_key(&map, "entry_style")?;
        if let Some(entries) = map.get("entries") {
            for entry in entries.as_array()? {
                let entry = entry.as_object()?;
                let layer = entry.get("layer")?.as_u64()?;
                let layer = LayerId(u32::try_from(layer).ok()?);
                let rule = entry.get("rule")?.as_str()?.to_string();
                let mut e = LegendEntry::default();
                match entry.get("name") {
                    None | Some(serde_json::Value::Null) => {}
                    Some(v) => e.name = Some(v.as_str()?.to_string()),
                }
                read_flag(entry, "hidden", &mut e.hidden)?;
                item.entries.insert((layer, rule), e);
            }
        }
        Some(item)
    }
}

impl Default for LegendItem {
    fn default() -> Self {
        LegendItem {
            frame: None,
            title: "Legend".to_string(),
            title_style: FurnitureText {
                weight: 700.0,
                ..FurnitureText::sized(11.0)
            },
            entry_style: FurnitureText::default(),
            entries: BTreeMap::new(),
        }
    }
}

impl NorthArrowItem {
    /// The item with no frame as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "hide_if_varies": self.hide_if_varies,
            "color": hex(self.color),
            "letter_style": self.letter_style.value(),
        })
        .to_string()
    }

    /// Reads [`NorthArrowItem::to_json`]. A missing key gets its default,
    /// and an unknown key is ignored. `None` if a value is bad.
    pub fn from_json(text: &str) -> Option<NorthArrowItem> {
        let map = json_object(text)?;
        let mut item = NorthArrowItem::default();
        read_flag(&map, "hide_if_varies", &mut item.hide_if_varies)?;
        read_color(&map, "color", &mut item.color)?;
        item.letter_style = item.letter_style.read_key(&map, "letter_style")?;
        Some(item)
    }
}

impl Default for NorthArrowItem {
    fn default() -> Self {
        NorthArrowItem {
            frame: None,
            hide_if_varies: true,
            color: [0x2b, 0x2b, 0x2b],
            letter_style: FurnitureText {
                weight: 600.0,
                ..FurnitureText::sized(9.0)
            },
        }
    }
}

/// A projection that the project keeps under a name. A custom projection is
/// a preset that you edit and save.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPreset {
    pub name: String,
    /// The PROJ string of the projection, with no radius.
    pub projection: String,
}

/// The maps and the pages. The atlas has at least one map and one page.
#[derive(Clone, Debug, PartialEq)]
pub struct Atlas {
    pub maps: Vec<MapRecord>,
    /// The custom projections, in the order in which they were first saved.
    pub presets: Vec<ProjectionPreset>,
    /// In atlas order.
    pub pages: Vec<PageRecord>,
    /// The layout grid of all pages.
    pub grid: LayoutGrid,
    pub(crate) next_map_id: u32,
    pub(crate) next_page_id: u32,
    pub(crate) next_frame_id: u32,
    pub(crate) next_furniture_id: u32,
}

/// The PROJ string of the Equal Earth projection at 0, 0.
pub const EQUAL_EARTH: &str = "+proj=eqearth +lon_0=0";

impl Atlas {
    /// An atlas with one map and one empty page. The map has an Equal Earth
    /// projection at 0, 0. The page is A4 landscape for metric units and
    /// Letter landscape for imperial units.
    pub fn new(units: Units) -> Atlas {
        let (width_mm, height_mm, units) = match units {
            Units::Metric => (297.0, 210.0, PageUnits::Mm),
            Units::Imperial => (279.4, 215.9, PageUnits::In),
        };
        Atlas {
            maps: vec![MapRecord {
                id: MapId(0),
                name: "Map".to_string(),
                projection: EQUAL_EARTH.to_string(),
                hidden: BTreeSet::new(),
                overrides: BTreeMap::new(),
                labels: BTreeMap::new(),
            }],
            presets: Vec::new(),
            pages: vec![PageRecord {
                id: PageId(0),
                name: String::new(),
                width_mm,
                height_mm,
                units,
                paper: [0xff, 0xff, 0xff],
                frames: Vec::new(),
                furniture: Vec::new(),
            }],
            grid: LayoutGrid::default(),
            next_map_id: 1,
            next_page_id: 1,
            next_frame_id: 0,
            next_furniture_id: 0,
        }
    }

    /// Adds an empty page directly after a page, or at the end. Returns its ID.
    pub fn add_page(
        &mut self,
        after: Option<PageId>,
        width_mm: f64,
        height_mm: f64,
        units: PageUnits,
    ) -> PageId {
        let id = PageId(self.next_page_id);
        self.next_page_id += 1;
        let at = after
            .and_then(|a| self.page_index(a))
            .map_or(self.pages.len(), |i| i + 1);
        let page = PageRecord {
            id,
            name: String::new(),
            width_mm,
            height_mm,
            units,
            paper: [0xff, 0xff, 0xff],
            frames: Vec::new(),
            furniture: Vec::new(),
        };
        self.pages.insert(at, page);
        id
    }

    /// Removes a page and its frames. `None`: no such page, or it is the
    /// last page.
    pub fn remove_page(&mut self, id: PageId) -> Option<PageRecord> {
        let i = self.page_index(id)?;
        (self.pages.len() > 1).then(|| self.pages.remove(i))
    }

    /// The place of a page in the atlas order.
    pub fn page_index(&self, id: PageId) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }

    pub fn page(&self, id: PageId) -> Option<&PageRecord> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: PageId) -> Option<&mut PageRecord> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// Puts a frame on top of the frames of a page, with a new ID in place
    /// of the ID of `frame`. `None`: no such page or no such map.
    pub fn add_frame(&mut self, page: PageId, mut frame: FrameRecord) -> Option<FrameId> {
        self.map(frame.map)?;
        let id = FrameId(self.next_frame_id);
        frame.id = id;
        self.page_mut(page)?.frames.push(frame);
        self.next_frame_id += 1;
        Some(id)
    }

    pub fn frame(&self, id: FrameId) -> Option<&FrameRecord> {
        self.pages
            .iter()
            .flat_map(|p| &p.frames)
            .find(|f| f.id == id)
    }

    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut FrameRecord> {
        let mut frames = self.pages.iter_mut().flat_map(|p| &mut p.frames);
        frames.find(|f| f.id == id)
    }

    /// The page that has a frame.
    pub fn page_of_frame(&self, id: FrameId) -> Option<PageId> {
        let page = self
            .pages
            .iter()
            .find(|p| p.frames.iter().any(|f| f.id == id));
        page.map(|p| p.id)
    }

    pub fn remove_frame(&mut self, id: FrameId) -> Option<FrameRecord> {
        self.pages.iter_mut().find_map(|p| {
            let i = p.frames.iter().position(|f| f.id == id)?;
            Some(p.frames.remove(i))
        })
    }

    /// Puts an item of furniture on top of the furniture of a page. `None`:
    /// no such page.
    pub fn add_furniture(
        &mut self,
        page: PageId,
        rect: [f64; 4],
        item: Furniture,
    ) -> Option<FurnitureId> {
        let id = FurnitureId(self.next_furniture_id);
        let record = FurnitureRecord { id, rect, item };
        self.page_mut(page)?.furniture.push(record);
        self.next_furniture_id += 1;
        Some(id)
    }

    pub fn furniture(&self, id: FurnitureId) -> Option<&FurnitureRecord> {
        self.pages
            .iter()
            .flat_map(|p| &p.furniture)
            .find(|f| f.id == id)
    }

    pub fn furniture_mut(&mut self, id: FurnitureId) -> Option<&mut FurnitureRecord> {
        let mut items = self.pages.iter_mut().flat_map(|p| &mut p.furniture);
        items.find(|f| f.id == id)
    }

    /// The page that has an item of furniture.
    pub fn page_of_furniture(&self, id: FurnitureId) -> Option<PageId> {
        let page = self
            .pages
            .iter()
            .find(|p| p.furniture.iter().any(|f| f.id == id));
        page.map(|p| p.id)
    }

    pub fn remove_furniture(&mut self, id: FurnitureId) -> Option<FurnitureRecord> {
        self.pages.iter_mut().find_map(|p| {
            let i = p.furniture.iter().position(|f| f.id == id)?;
            Some(p.furniture.remove(i))
        })
    }

    /// Links each item of a page that has the frame `from` to the frame `to`.
    pub fn relink_furniture(&mut self, page: PageId, from: FrameId, to: Option<FrameId>) {
        let Some(page) = self.page_mut(page) else {
            return;
        };
        for f in &mut page.furniture {
            if f.item.frame() == Some(from) {
                f.item.set_frame(to);
            }
        }
    }

    /// Saves a projection under a name. A preset with that name changes.
    pub fn save_preset(&mut self, name: &str, projection: &str) {
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(preset) => preset.projection = projection.to_string(),
            None => self.presets.push(ProjectionPreset {
                name: name.to_string(),
                projection: projection.to_string(),
            }),
        }
    }

    /// Removes the preset with this name. Returns true if there was one. A
    /// map keeps its projection.
    pub fn remove_preset(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }

    pub fn map(&self, id: MapId) -> Option<&MapRecord> {
        self.maps.iter().find(|m| m.id == id)
    }

    pub fn map_mut(&mut self, id: MapId) -> Option<&mut MapRecord> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    /// Adds a map with an Equal Earth projection at 0, 0 that shows each
    /// layer, after the last map. Its name is the first free name of "Map",
    /// "Map 2", "Map 3", and so on.
    pub fn add_map(&mut self) -> MapId {
        let name = self.free_name("Map");
        self.push_map(MapRecord {
            id: MapId(0),
            name,
            projection: EQUAL_EARTH.to_string(),
            hidden: BTreeSet::new(),
            overrides: BTreeMap::new(),
            labels: BTreeMap::new(),
        })
    }

    /// The first map that no frame shows.
    pub fn unshown_map(&self) -> Option<MapId> {
        let free = self
            .maps
            .iter()
            .find(|m| self.frames_of_map(m.id).next().is_none());
        free.map(|m| m.id)
    }

    /// Removes each map that no frame shows. If no frame shows a map, the
    /// first map stays.
    pub fn prune_maps(&mut self) {
        let shown: Vec<bool> = (self.maps.iter())
            .map(|m| self.frames_of_map(m.id).next().is_some())
            .collect();
        let keep = shown.iter().any(|&s| s).then_some(0);
        let mut i = 0;
        self.maps.retain(|_| {
            i += 1;
            shown[i - 1] || keep.is_none() && i == 1
        });
    }

    fn push_map(&mut self, mut map: MapRecord) -> MapId {
        map.id = MapId(self.next_map_id);
        self.next_map_id += 1;
        self.maps.push(map);
        MapId(self.next_map_id - 1)
    }

    /// `base`, or `base` with the first free number from 2.
    fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| self.maps.iter().any(|m| m.name == n);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .expect("a free name")
    }

    /// Each frame of a map, with its page, in atlas order.
    pub fn frames_of_map(&self, id: MapId) -> impl Iterator<Item = (&PageRecord, &FrameRecord)> {
        let frames = self
            .pages
            .iter()
            .flat_map(|p| p.frames.iter().map(move |f| (p, f)));
        frames.filter(move |(_, f)| f.map == id)
    }
}

/// The rings of a shape as a little-endian blob: the number of rings, then
/// for each ring the number of vertices and the x and y of each vertex.
pub(crate) fn encode_rings(rings: &[Vec<[f64; 2]>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(rings.len() as u32).to_le_bytes());
    for ring in rings {
        out.extend_from_slice(&(ring.len() as u32).to_le_bytes());
        for [x, y] in ring {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
    }
    out
}

/// `None`: the blob is not a whole set of rings.
pub(crate) fn decode_rings(blob: &[u8]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut r = Reader(blob);
    let rings = read_rings(&mut r)?;
    r.0.is_empty().then_some(rings)
}

pub(crate) fn read_rings(r: &mut Reader) -> Option<Vec<Vec<[f64; 2]>>> {
    let n = r.u32()? as usize;
    let mut rings = Vec::with_capacity(n.min(r.0.len() / 4));
    for _ in 0..n {
        let m = r.u32()? as usize;
        let mut ring = Vec::with_capacity(m.min(r.0.len() / 16));
        for _ in 0..m {
            ring.push([r.f64()?, r.f64()?]);
        }
        rings.push(ring);
    }
    Some(rings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_of_edge_labels_survives_json() {
        let set = EdgeLabels {
            font: Some("Noto Serif".into()),
            weight: 600.0,
            italic: true,
            size: 8.5,
            color: [10, 20, 30],
            opacity: 0.75,
            halo: true,
            halo_color: [200, 210, 220],
            halo_width: 1.5,
            outside: true,
            edges: Edges {
                top: true,
                bottom: false,
                left: true,
                right: false,
            },
            offset: 3.0,
            orientation: EdgeOrientation::Horizontal,
            ..EdgeLabels::default()
        };
        assert_eq!(EdgeLabels::from_json(&set.to_json()), Some(set));
        assert_eq!(EdgeLabels::from_json("{}"), Some(EdgeLabels::default()));
        let unknown = EdgeLabels::from_json(r#"{"size": 9, "future": 1}"#).unwrap();
        assert_eq!(unknown.size, 9.0);
        for bad in [
            "[]",
            r#"{"on": 1}"#,
            r#"{"offset": -1}"#,
            r#"{"opacity": 2}"#,
            r#"{"edges": {"top": "yes"}}"#,
        ] {
            assert_eq!(EdgeLabels::from_json(bad), None, "{bad}");
        }
    }

    fn frame(map: MapId) -> FrameRecord {
        FrameRecord {
            id: FrameId(99),
            map,
            shape: FrameShape::Rectangle([10.0, 10.0, 100.0, 80.0]),
            center: [12.0, -3.5],
            scale: 50_000_000.0,
            rotation: 15.0,
            graticule: Graticule::default(),
            neatline: Neatline::default(),
            border: Border::default(),
        }
    }

    #[test]
    fn a_saved_preset_replaces_the_preset_of_its_name() {
        let mut atlas = Atlas::new(Units::Metric);
        atlas.save_preset("Home", "+proj=laea +lon_0=1 +lat_0=2");
        atlas.save_preset("Far", "+proj=eqearth +lon_0=0");
        atlas.save_preset("Home", "+proj=laea +lon_0=3 +lat_0=4");
        assert_eq!(atlas.presets.len(), 2);
        assert_eq!(atlas.presets[0].projection, "+proj=laea +lon_0=3 +lat_0=4");
        assert!(atlas.remove_preset("Home"));
        assert!(!atlas.remove_preset("Home"));
        assert_eq!(atlas.presets.len(), 1);
    }

    #[test]
    fn a_new_atlas_has_one_map_and_one_page_in_the_units_of_the_world() {
        let metric = Atlas::new(Units::Metric);
        assert_eq!(metric.maps.len(), 1);
        assert_eq!(metric.maps[0].projection, EQUAL_EARTH);
        let page = &metric.pages[0];
        assert_eq!(
            (page.width_mm, page.height_mm, page.units),
            (297.0, 210.0, PageUnits::Mm)
        );
        let page = &Atlas::new(Units::Imperial).pages[0];
        assert_eq!(
            (page.width_mm, page.height_mm, page.units),
            (279.4, 215.9, PageUnits::In)
        );
    }

    #[test]
    fn pages_go_after_a_page_and_the_last_page_stays() {
        let mut atlas = Atlas::new(Units::Metric);
        let first = atlas.pages[0].id;
        let end = atlas.add_page(None, 100.0, 200.0, PageUnits::In);
        let middle = atlas.add_page(Some(first), 300.0, 400.0, PageUnits::Mm);
        let ids: Vec<PageId> = atlas.pages.iter().map(|p| p.id).collect();
        assert_eq!(ids, [first, middle, end]);
        assert_eq!(atlas.page(end).unwrap().width_mm, 100.0);
        assert_eq!(atlas.page_index(end), Some(2));

        assert_eq!(atlas.remove_page(middle).unwrap().id, middle);
        assert!(atlas.remove_page(middle).is_none());
        assert!(atlas.remove_page(first).is_some());
        assert!(atlas.remove_page(end).is_none());
        assert_eq!(atlas.pages.len(), 1);
        // The ID of a removed page is not used again.
        assert!(atlas.add_page(None, 1.0, 1.0, PageUnits::Mm) > end);
    }

    #[test]
    fn new_maps_get_free_names_and_new_ids() {
        let mut atlas = Atlas::new(Units::Metric);
        let first = atlas.maps[0].id;
        let second = atlas.add_map();
        let third = atlas.add_map();
        let names: Vec<&str> = atlas.maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Map", "Map 2", "Map 3"]);
        assert!(first < second && second < third);
        atlas.map_mut(second).unwrap().name = "Map 4".to_string();
        let fourth = atlas.add_map();
        assert_eq!(atlas.map(fourth).unwrap().name, "Map 2");
    }

    #[test]
    fn a_map_that_no_frame_shows_goes_away_but_the_last_map_stays() {
        let mut atlas = Atlas::new(Units::Metric);
        let shown = atlas.maps[0].id;
        let page = atlas.pages[0].id;
        assert_eq!(atlas.unshown_map(), Some(shown));
        let frame_id = atlas.add_frame(page, frame(shown)).unwrap();
        assert_eq!(atlas.unshown_map(), None);

        let free = atlas.add_map();
        assert_eq!(atlas.unshown_map(), Some(free));
        atlas.prune_maps();
        assert_eq!(atlas.maps.len(), 1);
        // The ID of a removed map is not used again.
        assert!(atlas.add_map() > free);

        atlas.remove_frame(frame_id);
        atlas.prune_maps();
        assert_eq!(atlas.maps.len(), 1);
        assert_eq!(atlas.maps[0].id, shown);
    }

    #[test]
    fn frames_go_on_a_page_and_come_off_it() {
        let mut atlas = Atlas::new(Units::Metric);
        let map = atlas.maps[0].id;
        let first = atlas.pages[0].id;
        let second = atlas.add_page(None, 297.0, 210.0, PageUnits::Mm);
        assert!(atlas.add_frame(first, frame(MapId(7))).is_none());
        assert!(atlas.add_frame(PageId(7), frame(map)).is_none());
        let a = atlas.add_frame(first, frame(map)).unwrap();
        let b = atlas.add_frame(second, frame(map)).unwrap();
        let c = atlas.add_frame(first, frame(map)).unwrap();
        assert_ne!(a, b);
        assert_eq!(atlas.frame(a).unwrap().id, a);
        let on_first: Vec<FrameId> = atlas
            .page(first)
            .unwrap()
            .frames
            .iter()
            .map(|f| f.id)
            .collect();
        assert_eq!(on_first, [a, c]);
        assert_eq!(atlas.page_of_frame(b), Some(second));
        let all: Vec<(PageId, FrameId)> = atlas
            .frames_of_map(map)
            .map(|(p, f)| (p.id, f.id))
            .collect();
        assert_eq!(all, [(first, a), (first, c), (second, b)]);

        atlas.frame_mut(c).unwrap().scale = 2.0;
        assert_eq!(atlas.frame(c).unwrap().scale, 2.0);
        assert_eq!(atlas.remove_frame(a).unwrap().id, a);
        assert!(atlas.frame(a).is_none());
        assert!(atlas.remove_frame(a).is_none());
        // The frames of a removed page go with it.
        atlas.remove_page(second);
        assert!(atlas.frame(b).is_none());
    }

    #[test]
    fn the_rings_of_a_shape_with_holes_round_trip() {
        let rings = vec![
            vec![
                [0.0, 0.0],
                [100.0, 0.0],
                [50.0, 30.0],
                [100.0, 80.0],
                [0.0, 80.0],
            ],
            vec![[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]],
            vec![[60.0, 60.0], [70.0, 60.0], [70.0, 70.0], [60.0, 70.5]],
        ];
        let blob = encode_rings(&rings);
        assert_eq!(decode_rings(&blob), Some(rings.clone()));
        assert_eq!(decode_rings(&blob[..blob.len() - 1]), None);
        let shape = FrameShape::from_rings("polygon", rings.clone()).unwrap();
        assert_eq!(shape, FrameShape::Polygon(rings));

        let rect = FrameShape::Rectangle([1.0, 2.0, 3.0, 4.0]);
        let back = FrameShape::from_rings(
            rect.kind_id(),
            decode_rings(&encode_rings(&rect.rings())).unwrap(),
        );
        assert_eq!(back, Some(rect));
        assert_eq!(
            FrameShape::from_rings("rectangle", vec![vec![[0.0, 0.0]]]),
            None
        );
        assert_eq!(FrameShape::from_rings("polygon", vec![]), None);
    }
    fn styled() -> FurnitureText {
        FurnitureText {
            font: Some("Noto Serif".into()),
            weight: 700.0,
            italic: true,
            size: 14.5,
            color: [10, 20, 30],
            case: TextCase::SmallCaps,
            letter_spacing: 0.08,
            opacity: 0.6,
            halo: true,
            halo_color: [250, 240, 230],
            halo_width: 2.5,
        }
    }

    /// Each item survives its JSON with the frame `frame`.
    fn round_trip(item: Furniture, frame: Option<FrameId>) {
        let text = item.to_json();
        let back = Furniture::from_json(item.kind_id(), &text, frame);
        assert_eq!(back, Some(item));
    }

    #[test]
    fn each_kind_of_furniture_survives_json() {
        let frame = Some(FrameId(4));
        round_trip(
            Furniture::Text(TextItem {
                text: "The North".into(),
                style: styled(),
                align: TextAlign::Center,
            }),
            None,
        );
        let mut entries = BTreeMap::new();
        let renamed = LegendEntry {
            name: Some("Towns".into()),
            hidden: false,
        };
        entries.insert((LayerId(2), "Large".into()), renamed);
        let hidden = LegendEntry {
            name: None,
            hidden: true,
        };
        entries.insert((LayerId(7), String::new()), hidden);
        let legend = LegendItem {
            frame,
            title: "Key".into(),
            title_style: styled(),
            entry_style: FurnitureText {
                case: TextCase::Title,
                ..FurnitureText::sized(7.0)
            },
            entries,
        };
        round_trip(Furniture::Legend(legend), frame);
        let bar = ScaleBarItem {
            frame,
            unit_style: styled(),
            second_unit: Some(crate::ScaleUnit::Feet),
            ..ScaleBarItem::default()
        };
        round_trip(Furniture::ScaleBar(bar), frame);
        let arrow = NorthArrowItem {
            frame,
            hide_if_varies: false,
            color: [200, 0, 0],
            letter_style: styled(),
        };
        round_trip(Furniture::NorthArrow(arrow), frame);
        assert_eq!(Furniture::from_json("compass", "{}", None), None);
    }

    #[test]
    fn a_missing_key_of_furniture_gets_its_default() {
        let text = TextItem::from_json("{}").unwrap();
        assert_eq!(text, TextItem::default());
        assert_eq!((text.text.as_str(), text.style.size), ("Text", 18.0));
        let legend = LegendItem::from_json(r#"{"future": 1}"#).unwrap();
        assert_eq!(legend, LegendItem::default());
        assert_eq!(legend.title, "Legend");
        assert_eq!(ScaleBarItem::from_json("{}"), Some(ScaleBarItem::default()));
        let arrow = NorthArrowItem::from_json("{}").unwrap();
        assert_eq!(arrow, NorthArrowItem::default());
        assert!(arrow.hide_if_varies);
        // The text keys of an older legend are the style of its title and
        // its entries.
        let old = r##"{"title": "Key", "size": 6, "weight": 400, "color": "102030"}"##;
        let legend = LegendItem::from_json(old).unwrap();
        let style = FurnitureText {
            color: [0x10, 0x20, 0x30],
            ..FurnitureText::sized(6.0)
        };
        assert_eq!(legend.title_style, style);
        assert_eq!(legend.entry_style, style);
        let text = TextItem::from_json(r#"{"size": 9}"#).unwrap();
        assert_eq!(text.style, FurnitureText::sized(9.0));
        for bad in [
            r#"{"case": "shout"}"#,
            r#"{"opacity": 1.5}"#,
            r#"{"halo_width": 0}"#,
            r#"{"halo": "yes"}"#,
        ] {
            assert_eq!(TextItem::from_json(bad), None, "{bad}");
        }
        assert_eq!(
            NorthArrowItem::from_json(r#"{"letter_style": {"size": -1}}"#),
            None
        );
        let entry = r#"{"entries": [{"layer": 3, "rule": "A"}]}"#;
        let legend = LegendItem::from_json(entry).unwrap();
        assert_eq!(
            legend.entries[&(LayerId(3), "A".into())],
            LegendEntry::default()
        );
        for bad in [
            "[]",
            r#"{"size": 0}"#,
            r#"{"align": "justify"}"#,
            r#"{"color": "red"}"#,
            r#"{"entries": [{"rule": "A"}]}"#,
            r#"{"text": 5}"#,
        ] {
            let none = TextItem::from_json(bad).is_none() || LegendItem::from_json(bad).is_none();
            assert!(none, "{bad}");
        }
    }

    #[test]
    fn each_case_changes_the_text() {
        let text = "the RIVER ode";
        assert_eq!(TextCase::Upper.apply(text), "THE RIVER ODE");
        assert_eq!(TextCase::Lower.apply(text), "the river ode");
        assert_eq!(TextCase::Title.apply(text), "The River Ode");
        assert_eq!(TextCase::SmallCaps.apply(text), text);
        for c in TextCase::ALL {
            assert_eq!(TextCase::from_id(c.id()), Some(c));
        }
    }

    #[test]
    fn furniture_relinks_on_its_page_only() {
        let mut atlas = Atlas::new(Units::Metric);
        let page = atlas.pages[0].id;
        let other = atlas.add_page(None, 100.0, 100.0, PageUnits::Mm);
        let linked = |frame| {
            Furniture::ScaleBar(ScaleBarItem {
                frame,
                ..ScaleBarItem::default()
            })
        };
        let rect = [1.0, 2.0, 30.0, 12.0];
        let a = atlas
            .add_furniture(page, rect, linked(Some(FrameId(1))))
            .unwrap();
        let b = atlas
            .add_furniture(other, rect, linked(Some(FrameId(1))))
            .unwrap();
        let text = Furniture::Text(TextItem::default());
        let c = atlas.add_furniture(page, rect, text).unwrap();
        assert_eq!(atlas.add_furniture(PageId(99), rect, linked(None)), None);
        assert_eq!(atlas.page_of_furniture(b), Some(other));
        atlas.relink_furniture(page, FrameId(1), Some(FrameId(2)));
        assert_eq!(atlas.furniture(a).unwrap().item.frame(), Some(FrameId(2)));
        assert_eq!(atlas.furniture(b).unwrap().item.frame(), Some(FrameId(1)));
        atlas.furniture_mut(b).unwrap().rect[0] = 5.0;
        assert_eq!(atlas.remove_furniture(b).unwrap().rect[0], 5.0);
        assert_eq!(atlas.furniture(b), None);
        assert_eq!(atlas.page(page).unwrap().furniture.len(), 2);
        assert_eq!(atlas.furniture(c).unwrap().item.frame(), None);
    }
}
