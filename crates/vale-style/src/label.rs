//! The label classes of a map.

use serde_json::{Map, Value as Json, json};
use vale_store::GeometryKind;

use crate::expr::field_ref;
use crate::{Color, LabelStyle, StyleError};

/// The version of the label class JSON that this build writes.
pub const CLASSES_VERSION: u64 = 1;

/// Where a label goes relative to its feature.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LabelPlacement {
    /// The best of the ranked positions around a point.
    Around,
    /// Along a line, on the side that fits.
    AboveOrBelow,
    LineAbove,
    LineBelow,
    OnLine,
    /// Polygons. Curved along the long axis of a long shape, else straight at the visual center.
    BestFit,
    /// Polygons. Curved along the long axis.
    Curved,
    /// Polygons. Straight, at the visual center.
    Horizontal,
    /// Polygons. Along a parallel, at the latitude with the most room.
    Parallel,
}

const POINT_PLACEMENTS: [LabelPlacement; 1] = [LabelPlacement::Around];

const LINE_PLACEMENTS: [LabelPlacement; 4] = [
    LabelPlacement::AboveOrBelow,
    LabelPlacement::LineAbove,
    LabelPlacement::LineBelow,
    LabelPlacement::OnLine,
];

const POLYGON_PLACEMENTS: [LabelPlacement; 4] = [
    LabelPlacement::BestFit,
    LabelPlacement::Curved,
    LabelPlacement::Parallel,
    LabelPlacement::Horizontal,
];

const ALL_PLACEMENTS: [LabelPlacement; 9] = [
    LabelPlacement::Around,
    LabelPlacement::AboveOrBelow,
    LabelPlacement::LineAbove,
    LabelPlacement::LineBelow,
    LabelPlacement::OnLine,
    LabelPlacement::BestFit,
    LabelPlacement::Curved,
    LabelPlacement::Horizontal,
    LabelPlacement::Parallel,
];

impl LabelPlacement {
    /// The placements of a feature type. The first is the default.
    pub fn choices(kind: GeometryKind) -> &'static [LabelPlacement] {
        match kind {
            GeometryKind::Point => &POINT_PLACEMENTS,
            GeometryKind::Line => &LINE_PLACEMENTS,
            GeometryKind::Polygon => &POLYGON_PLACEMENTS,
        }
    }

    pub fn default_for(kind: GeometryKind) -> Option<LabelPlacement> {
        LabelPlacement::choices(kind).first().copied()
    }

    /// True if a feature of `kind` can have this placement.
    pub fn fits(self, kind: GeometryKind) -> bool {
        LabelPlacement::choices(kind).contains(&self)
    }

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LabelPlacement::Around => "Around point",
            LabelPlacement::AboveOrBelow => "Above or below",
            LabelPlacement::LineAbove => "Above line",
            LabelPlacement::LineBelow => "Below line",
            LabelPlacement::OnLine => "On line",
            LabelPlacement::BestFit => "Best fit",
            LabelPlacement::Curved => "Curved",
            LabelPlacement::Horizontal => "Horizontal",
            LabelPlacement::Parallel => "Along parallel",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LabelPlacement::Around => "around",
            LabelPlacement::AboveOrBelow => "above_or_below",
            LabelPlacement::LineAbove => "line_above",
            LabelPlacement::LineBelow => "line_below",
            LabelPlacement::OnLine => "on_line",
            LabelPlacement::BestFit => "best_fit",
            LabelPlacement::Curved => "curved",
            LabelPlacement::Horizontal => "horizontal",
            LabelPlacement::Parallel => "parallel",
        }
    }

    fn from_key(key: &str) -> Option<LabelPlacement> {
        ALL_PLACEMENTS.into_iter().find(|p| p.key() == key)
    }

    /// True for the placements of polygons.
    pub fn is_polygon(self) -> bool {
        POLYGON_PLACEMENTS.contains(&self)
    }
}

/// A position of a point label relative to its symbol.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LabelPosition {
    TopRight,
    Right,
    BottomRight,
    Top,
    Bottom,
    TopLeft,
    Left,
    BottomLeft,
}

impl LabelPosition {
    /// The default rank, best first. It equals the default order of the labeler.
    pub const DEFAULT_ORDER: [LabelPosition; 8] = [
        LabelPosition::TopRight,
        LabelPosition::TopLeft,
        LabelPosition::BottomRight,
        LabelPosition::BottomLeft,
        LabelPosition::Right,
        LabelPosition::Left,
        LabelPosition::Top,
        LabelPosition::Bottom,
    ];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LabelPosition::TopRight => "Top right",
            LabelPosition::Right => "Right",
            LabelPosition::BottomRight => "Bottom right",
            LabelPosition::Top => "Top",
            LabelPosition::Bottom => "Bottom",
            LabelPosition::TopLeft => "Top left",
            LabelPosition::Left => "Left",
            LabelPosition::BottomLeft => "Bottom left",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LabelPosition::TopRight => "top_right",
            LabelPosition::Right => "right",
            LabelPosition::BottomRight => "bottom_right",
            LabelPosition::Top => "top",
            LabelPosition::Bottom => "bottom",
            LabelPosition::TopLeft => "top_left",
            LabelPosition::Left => "left",
            LabelPosition::BottomLeft => "bottom_left",
        }
    }

    fn from_key(key: &str) -> Option<LabelPosition> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .find(|p| p.key() == key)
    }
}

/// The point of a polygon where the leader line of a label outside the polygon starts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeaderAnchor {
    /// The centroid of the shape.
    Center,
    /// A point that is always inside the shape.
    #[default]
    Inside,
    /// The point of the outline nearest to the label.
    Outline,
}

impl LeaderAnchor {
    pub const ALL: [LeaderAnchor; 3] = [
        LeaderAnchor::Center,
        LeaderAnchor::Inside,
        LeaderAnchor::Outline,
    ];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LeaderAnchor::Center => "Center",
            LeaderAnchor::Inside => "Always inside",
            LeaderAnchor::Outline => "Nearest edge",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LeaderAnchor::Center => "center",
            LeaderAnchor::Inside => "inside",
            LeaderAnchor::Outline => "outline",
        }
    }

    fn from_key(key: &str) -> Option<LeaderAnchor> {
        LeaderAnchor::ALL.into_iter().find(|a| a.key() == key)
    }
}

/// The default rank of each zone inside a polygon. Rank 1 is best, and rank 0 blocks the zone.
pub const ZONES_INSIDE: [u8; 9] = [1; 9];

/// The default rank of each zone outside a polygon. All are blocked.
pub const ZONES_OUTSIDE: [u8; 8] = [0; 8];

/// The default distance between the repeats of a polygon label, in points.
pub const POLYGON_REPEAT_DISTANCE: f64 = 400.0;

/// The mark at the anchor end of a leader line.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeaderEnd {
    #[default]
    None,
    Dot,
    Arrow,
}

impl LeaderEnd {
    pub const ALL: [LeaderEnd; 3] = [LeaderEnd::None, LeaderEnd::Dot, LeaderEnd::Arrow];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LeaderEnd::None => "None",
            LeaderEnd::Dot => "Dot",
            LeaderEnd::Arrow => "Arrow",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LeaderEnd::None => "none",
            LeaderEnd::Dot => "dot",
            LeaderEnd::Arrow => "arrow",
        }
    }

    fn from_key(key: &str) -> Option<LeaderEnd> {
        LeaderEnd::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// How the leader line of a label outside its feature looks. Distances are in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LeaderStyle {
    /// False draws no line. The label keeps its place.
    pub show: bool,
    /// `None` uses the color of the text.
    pub color: Option<Color>,
    pub width: f64,
    /// From 0 to 1, times the opacity of the text.
    pub opacity: f64,
    pub dashed: bool,
    pub dash: f64,
    pub dash_gap: f64,
    /// A wider line in the halo color under the line.
    pub halo: bool,
    /// `None` uses the halo color of the text.
    pub halo_color: Option<Color>,
    /// The width of the halo on each side of the line.
    pub halo_width: f64,
    pub end: LeaderEnd,
    /// The diameter of a dot, or the length of an arrow.
    pub end_size: f64,
    /// The space between the line and the label.
    pub label_gap: f64,
    /// The space between the line and its anchor.
    pub anchor_gap: f64,
}

impl Default for LeaderStyle {
    fn default() -> Self {
        LeaderStyle {
            show: true,
            color: None,
            width: 0.75,
            opacity: 1.0,
            dashed: false,
            dash: 3.0,
            dash_gap: 2.0,
            halo: false,
            halo_color: None,
            halo_width: 1.0,
            end: LeaderEnd::None,
            end_size: 3.0,
            label_gap: 1.0,
            anchor_gap: 0.0,
        }
    }
}

impl LeaderStyle {
    fn to_json(self) -> Json {
        json!({
            "show": self.show,
            "color": self.color.map(|c| c.to_hex()),
            "width": self.width,
            "opacity": self.opacity,
            "dashed": self.dashed,
            "dash": self.dash,
            "dash_gap": self.dash_gap,
            "halo": self.halo,
            "halo_color": self.halo_color.map(|c| c.to_hex()),
            "halo_width": self.halo_width,
            "end": self.end.key(),
            "end_size": self.end_size,
            "label_gap": self.label_gap,
            "anchor_gap": self.anchor_gap,
        })
    }

    /// Missing keys take the default.
    fn from_json(j: Option<&Json>) -> LeaderStyle {
        let d = LeaderStyle::default();
        let Some(m) = j.and_then(Json::as_object) else {
            return d;
        };
        let number =
            |key: &str, fallback: f64| m.get(key).and_then(Json::as_f64).unwrap_or(fallback);
        let flag =
            |key: &str, fallback: bool| m.get(key).and_then(Json::as_bool).unwrap_or(fallback);
        let color = |key: &str| m.get(key).and_then(Json::as_str).and_then(Color::from_hex);
        LeaderStyle {
            show: flag("show", d.show),
            color: color("color"),
            width: number("width", d.width),
            opacity: number("opacity", d.opacity),
            dashed: flag("dashed", d.dashed),
            dash: number("dash", d.dash),
            dash_gap: number("dash_gap", d.dash_gap),
            halo: flag("halo", d.halo),
            halo_color: color("halo_color"),
            halo_width: number("halo_width", d.halo_width),
            end: text(m, "end")
                .as_deref()
                .and_then(LeaderEnd::from_key)
                .unwrap_or_default(),
            end_size: number("end_size", d.end_size),
            label_gap: number("label_gap", d.label_gap),
            anchor_gap: number("anchor_gap", d.anchor_gap),
        }
    }
}

/// One position in the rank of a point class. A blocked position is off.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PositionRank {
    pub position: LabelPosition,
    pub on: bool,
}

impl PositionRank {
    /// All positions in the default order, all on.
    pub fn defaults() -> Vec<PositionRank> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .map(|position| PositionRank { position, on: true })
            .collect()
    }

    /// All positions in the default order, with only `only` on.
    fn only(only: LabelPosition) -> Vec<PositionRank> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .map(|position| PositionRank {
                position,
                on: position == only,
            })
            .collect()
    }
}

/// The default offset of a placement, in points.
pub fn offset_for(placement: LabelPlacement) -> f64 {
    if placement == LabelPlacement::Around {
        2.0
    } else {
        4.0
    }
}

/// The default distance between the repeats of a line label, in points.
pub const REPEAT_DISTANCE: f64 = 600.0;

/// The default curve limit of a line label, in radians between glyphs.
pub const CURVE_LIMIT: f64 = 0.30;

/// The default curve limit of a polygon label, in radians between glyphs.
pub const POLYGON_CURVE_LIMIT: f64 = 0.2;

/// The default curve limit of a placement.
pub fn curve_limit_for(placement: LabelPlacement) -> f64 {
    if placement.is_polygon() {
        POLYGON_CURVE_LIMIT
    } else {
        CURVE_LIMIT
    }
}

/// The default spread of a polygon label, in multiples of the font size.
pub const SPREAD: f64 = 1.0;

/// The default smoothing window, in multiples of the font size.
pub const SMOOTHING: f64 = 1.5;

/// The default buffer around a label, in points.
pub const BUFFER: f64 = 1.0;

/// The default distance between two labels with the same text, in points.
pub const DUPLICATE_DISTANCE: f64 = 300.0;

pub use vale_store::TextCase;

/// The default color of the halo.
pub const HALO_COLOR: Color = Color::rgba(255, 255, 255, 217);

/// The default width of the halo, in points.
pub const HALO_WIDTH: f64 = 2.2;

/// The default letter spacing of a feature type, as a fraction of `size`. Line labels get 0.5 pt, and polygon labels get 0.15 of the size.
fn letter_spacing_for(kind: GeometryKind, size: f64) -> f64 {
    match kind {
        GeometryKind::Line if size > 0.0 => 0.5 / size,
        GeometryKind::Polygon => 0.15,
        _ => 0.0,
    }
}

/// One class of labels of a layer on a map.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelClass {
    pub name: String,
    /// An expression. Empty text gives no label.
    pub text: String,
    /// A filter. Empty text matches all features.
    pub filter: String,
    /// A font family. `None` is the default font.
    pub font: Option<String>,
    /// Points.
    pub size: f64,
    pub color: Color,
    pub italic: bool,
    /// The font weight, from 100 to 900. 400 is regular.
    pub weight: f32,
    pub case: TextCase,
    /// A fraction of `size`. 0.05 adds 5 % of the size between letters.
    pub letter_spacing: f64,
    /// The opacity of the text, from 0 to 1.
    pub opacity: f64,
    pub halo: bool,
    pub halo_color: Color,
    /// Points.
    pub halo_width: f64,
    /// Higher is placed first.
    pub priority: i32,
    /// A numeric attribute. Higher is placed first within the class.
    pub priority_field: Option<String>,
    pub placement: LabelPlacement,
    /// The distance from the feature, in points.
    pub offset: f64,
    /// The ranked positions of a point label, best first.
    pub positions: Vec<PositionRank>,
    /// Lines. True repeats the label along a long line.
    pub repeat: bool,
    /// Lines. The distance between the repeats, in points.
    pub repeat_distance: f64,
    /// Lines and polygons. The largest turn of the text between two glyphs, in radians.
    pub curve_limit: f64,
    /// Polygons. The most extra space between two letters, in multiples of `size`.
    pub spread: f64,
    /// Points, and polygons with straight text. True makes the text follow the parallels.
    pub graticule: bool,
    /// True lets text that follows the parallels read west to east also where it is upside down.
    pub no_flip: bool,
    /// Polygons. The rank of each zone inside the shape, row by row from the top left. Rank 1
    /// is best, and rank 0 blocks the zone.
    pub zones_inside: [u8; 9],
    /// Polygons. The rank of each zone outside the shape, row by row from the top left, with
    /// no center.
    pub zones_outside: [u8; 8],
    /// Polygons. The start of the leader line of a label outside the shape.
    pub anchor: LeaderAnchor,
    /// Polygons. The look of the leader line of a label outside the shape.
    pub leader: LeaderStyle,
    /// Polygons. True lets a label cover the holes of the shape.
    pub over_holes: bool,
    /// Polygons. True keeps a label off the other labeled polygons.
    pub avoid_polygons: bool,
    /// Polygons. True labels only the largest part of a shape with many parts.
    pub largest_part: bool,
    /// Polygons. True repeats the label in a large shape.
    pub polygon_repeat: bool,
    /// Polygons. The distance between the repeats, in points.
    pub polygon_repeat_distance: f64,
    /// Polygons. The distance that a label can go past the outline, in points.
    pub overrun: f64,
    /// Polygons. A shape with less area gets no label, in square points.
    pub min_area: f64,
    /// Lines. The smoothing window of the line, in multiples of `size`.
    pub smoothing: f64,
    /// The margin around the label that other labels keep free, in points.
    pub buffer: f64,
    /// True keeps labels with the same text apart.
    pub remove_duplicates: bool,
    /// The smallest distance between two labels with the same text, in points.
    pub duplicate_distance: f64,
}

impl LabelClass {
    /// The class that gives the labels of `style`. `None` if the labels are off, there is no field, or the feature type has no placements.
    pub fn from_style(
        name: &str,
        style: &LabelStyle,
        kind: GeometryKind,
        priority: i32,
    ) -> Option<LabelClass> {
        if !style.enabled {
            return None;
        }
        let field = style.field.as_deref()?;
        Some(LabelClass {
            name: name.to_owned(),
            text: field_ref(field),
            filter: String::new(),
            font: style.font.clone(),
            size: style.size,
            color: style.color,
            italic: style.italic,
            weight: 400.0,
            case: TextCase::Normal,
            letter_spacing: letter_spacing_for(kind, style.size),
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority,
            priority_field: style.priority_field.clone(),
            ..LabelClass::placement_defaults(LabelPlacement::default_for(kind)?)
        })
    }

    /// A class with the default text and placement options of `placement`.
    fn placement_defaults(placement: LabelPlacement) -> LabelClass {
        LabelClass {
            name: String::new(),
            text: String::new(),
            filter: String::new(),
            font: None,
            size: 11.0,
            color: Color::rgb(0, 0, 0),
            italic: false,
            weight: 400.0,
            case: TextCase::Normal,
            letter_spacing: 0.0,
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority: 0,
            priority_field: None,
            placement,
            offset: offset_for(placement),
            positions: PositionRank::defaults(),
            repeat: true,
            repeat_distance: REPEAT_DISTANCE,
            curve_limit: curve_limit_for(placement),
            spread: SPREAD,
            graticule: false,
            no_flip: false,
            zones_inside: ZONES_INSIDE,
            zones_outside: ZONES_OUTSIDE,
            anchor: LeaderAnchor::default(),
            leader: LeaderStyle::default(),
            over_holes: false,
            avoid_polygons: false,
            largest_part: true,
            polygon_repeat: false,
            polygon_repeat_distance: POLYGON_REPEAT_DISTANCE,
            overrun: 0.0,
            min_area: 0.0,
            smoothing: SMOOTHING,
            buffer: BUFFER,
            remove_duplicates: true,
            duplicate_distance: DUPLICATE_DISTANCE,
        }
    }

    /// A new class that labels each feature with its `name` field. `None` if the feature type has no placements.
    pub fn new(name: &str, kind: GeometryKind, priority: i32) -> Option<LabelClass> {
        let placement = LabelPlacement::default_for(kind)?;
        let line = kind == GeometryKind::Line;
        let polygon = kind == GeometryKind::Polygon;
        let size = 11.0;
        Some(LabelClass {
            name: name.to_owned(),
            text: "name".to_owned(),
            filter: String::new(),
            font: None,
            size,
            color: if line {
                Color::rgb(40, 100, 160)
            } else if polygon {
                Color::rgb(0x5a, 0x55, 0x4c)
            } else {
                Color::rgb(0x2b, 0x2a, 0x26)
            },
            italic: line,
            weight: 400.0,
            case: if polygon {
                TextCase::Upper
            } else {
                TextCase::Normal
            },
            letter_spacing: letter_spacing_for(kind, size),
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority,
            priority_field: None,
            remove_duplicates: false,
            ..LabelClass::placement_defaults(placement)
        })
    }
}

/// The JSON text of `classes`. The same classes give the same text.
pub fn classes_to_json(classes: &[LabelClass]) -> String {
    let classes: Vec<Json> = classes
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "text": c.text,
                "filter": c.filter,
                "font": c.font,
                "size": c.size,
                "color": c.color.to_hex(),
                "italic": c.italic,
                "weight": c.weight,
                "case": c.case.id(),
                "letter_spacing": c.letter_spacing,
                "opacity": c.opacity,
                "halo": c.halo,
                "halo_color": c.halo_color.to_hex(),
                "halo_width": c.halo_width,
                "priority": c.priority,
                "priority_field": c.priority_field,
                "placement": c.placement.key(),
                "offset": c.offset,
                "positions": c
                    .positions
                    .iter()
                    .map(|r| json!({ "position": r.position.key(), "on": r.on }))
                    .collect::<Vec<_>>(),
                "repeat": c.repeat,
                "repeat_distance": c.repeat_distance,
                "curve_limit": c.curve_limit,
                "spread": c.spread,
                "graticule": c.graticule,
                "no_flip": c.no_flip,
                "zones_inside": c.zones_inside,
                "zones_outside": c.zones_outside,
                "anchor": c.anchor.key(),
                "leader": c.leader.to_json(),
                "over_holes": c.over_holes,
                "avoid_polygons": c.avoid_polygons,
                "largest_part": c.largest_part,
                "polygon_repeat": c.polygon_repeat,
                "polygon_repeat_distance": c.polygon_repeat_distance,
                "overrun": c.overrun,
                "min_area": c.min_area,
                "smoothing": c.smoothing,
                "buffer": c.buffer,
                "remove_duplicates": c.remove_duplicates,
                "duplicate_distance": c.duplicate_distance,
            })
        })
        .collect();
    json!({ "version": CLASSES_VERSION, "classes": classes }).to_string()
}

/// The classes of a JSON text. The reader ignores unknown keys.
pub fn classes_from_json(text: &str) -> Result<Vec<LabelClass>, StyleError> {
    let root: Json = serde_json::from_str(text).map_err(|e| StyleError::Json(e.to_string()))?;
    let root = root.as_object().ok_or_else(|| invalid("not an object"))?;
    match root.get("version").and_then(Json::as_u64) {
        Some(1..=CLASSES_VERSION) => {}
        v => return Err(StyleError::Version(v)),
    }
    root.get("classes")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("no classes"))?
        .iter()
        .map(read_class)
        .collect()
}

fn invalid(what: &str) -> StyleError {
    StyleError::Invalid(what.to_owned())
}

fn text(m: &Map<String, Json>, key: &str) -> Option<String> {
    m.get(key).and_then(Json::as_str).map(str::to_owned)
}

/// The zone ranks at `key`, or `fallback` if the list is missing or has the wrong length.
fn ranks<const N: usize>(m: &Map<String, Json>, key: &str, fallback: [u8; N]) -> [u8; N] {
    let Some(list) = m.get(key).and_then(Json::as_array) else {
        return fallback;
    };
    let ranks: Vec<u8> = list
        .iter()
        .filter_map(|r| r.as_u64().and_then(|r| u8::try_from(r).ok()))
        .collect();
    ranks.try_into().unwrap_or(fallback)
}

fn read_class(j: &Json) -> Result<LabelClass, StyleError> {
    let m = j
        .as_object()
        .ok_or_else(|| invalid("a label class is not an object"))?;
    let color = m
        .get("color")
        .and_then(Json::as_str)
        .and_then(Color::from_hex)
        .ok_or_else(|| invalid("a label class has no valid color"))?;
    let key = text(m, "placement").unwrap_or_default();
    // Older files have one side of a point as the placement.
    let (placement, side) = match key.as_str() {
        "right" => (LabelPlacement::Around, Some(LabelPosition::Right)),
        "left" => (LabelPlacement::Around, Some(LabelPosition::Left)),
        "above" => (LabelPlacement::Around, Some(LabelPosition::Top)),
        "below" => (LabelPlacement::Around, Some(LabelPosition::Bottom)),
        key => (
            LabelPlacement::from_key(key)
                .ok_or_else(|| invalid("a label class has no valid placement"))?,
            None,
        ),
    };
    let positions = match (side, m.get("positions").and_then(Json::as_array)) {
        (Some(side), _) => PositionRank::only(side),
        (None, Some(list)) => list
            .iter()
            .filter_map(|r| {
                let position = r
                    .get("position")
                    .and_then(Json::as_str)
                    .and_then(LabelPosition::from_key)?;
                let on = r.get("on").and_then(Json::as_bool).unwrap_or(true);
                Some(PositionRank { position, on })
            })
            .collect(),
        (None, None) => PositionRank::defaults(),
    };
    let priority = m
        .get("priority")
        .and_then(Json::as_i64)
        .and_then(|p| i32::try_from(p).ok())
        .unwrap_or(0);
    let number = |key: &str, fallback: f64| m.get(key).and_then(Json::as_f64).unwrap_or(fallback);
    let flag = |key: &str, fallback: bool| m.get(key).and_then(Json::as_bool).unwrap_or(fallback);
    Ok(LabelClass {
        name: text(m, "name").unwrap_or_default(),
        text: text(m, "text").unwrap_or_default(),
        filter: text(m, "filter").unwrap_or_default(),
        font: text(m, "font"),
        size: number("size", 11.0),
        color,
        italic: flag("italic", false),
        weight: number("weight", 400.0) as f32,
        case: text(m, "case")
            .as_deref()
            .and_then(TextCase::from_id)
            .unwrap_or(TextCase::Normal),
        letter_spacing: number("letter_spacing", 0.0),
        opacity: number("opacity", 1.0),
        halo: flag("halo", true),
        halo_color: m
            .get("halo_color")
            .and_then(Json::as_str)
            .and_then(Color::from_hex)
            .unwrap_or(HALO_COLOR),
        halo_width: number("halo_width", HALO_WIDTH),
        priority,
        priority_field: text(m, "priority_field"),
        placement,
        offset: number("offset", offset_for(placement)),
        positions,
        repeat: flag("repeat", true),
        repeat_distance: number("repeat_distance", REPEAT_DISTANCE),
        curve_limit: number("curve_limit", curve_limit_for(placement)),
        spread: number("spread", SPREAD),
        graticule: flag("graticule", false),
        no_flip: flag("no_flip", false),
        zones_inside: ranks(m, "zones_inside", ZONES_INSIDE),
        zones_outside: ranks(m, "zones_outside", ZONES_OUTSIDE),
        anchor: text(m, "anchor")
            .as_deref()
            .and_then(LeaderAnchor::from_key)
            .unwrap_or_default(),
        leader: LeaderStyle::from_json(m.get("leader")),
        over_holes: flag("over_holes", false),
        avoid_polygons: flag("avoid_polygons", false),
        largest_part: flag("largest_part", true),
        polygon_repeat: flag("polygon_repeat", false),
        polygon_repeat_distance: number("polygon_repeat_distance", POLYGON_REPEAT_DISTANCE),
        overrun: number("overrun", 0.0),
        min_area: number("min_area", 0.0),
        smoothing: number("smoothing", SMOOTHING),
        buffer: number("buffer", BUFFER),
        remove_duplicates: flag("remove_duplicates", true),
        duplicate_distance: number("duplicate_distance", DUPLICATE_DISTANCE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(field: &str) -> LabelStyle {
        LabelStyle {
            enabled: true,
            field: Some(field.to_owned()),
            priority_field: Some("pop".to_owned()),
            size: 13.0,
            color: Color::rgb(10, 20, 30),
            italic: true,
            font: Some("Serif".to_owned()),
        }
    }

    #[test]
    fn a_label_class_survives_json() {
        let mut a = LabelClass::new("Towns", GeometryKind::Point, 2).unwrap();
        a.filter = "pop > 1000".to_owned();
        a.font = Some("Serif".to_owned());
        a.priority_field = Some("pop".to_owned());
        a.size = 14.5;
        a.color = Color::rgba(1, 2, 3, 128);
        a.italic = true;
        a.weight = 700.0;
        a.case = TextCase::AllSmallCaps;
        a.letter_spacing = 0.05;
        a.opacity = 0.75;
        a.halo = false;
        a.halo_color = Color::rgba(0, 0, 0, 100);
        a.halo_width = 1.5;
        a.offset = 3.5;
        a.positions = vec![
            PositionRank {
                position: LabelPosition::Left,
                on: true,
            },
            PositionRank {
                position: LabelPosition::TopRight,
                on: false,
            },
        ];
        a.repeat = false;
        a.repeat_distance = 250.0;
        a.curve_limit = 0.5;
        a.smoothing = 2.0;
        a.buffer = 3.0;
        a.remove_duplicates = false;
        a.duplicate_distance = 120.0;
        a.graticule = true;
        a.no_flip = true;
        let mut b = LabelClass::new("Rivers", GeometryKind::Line, -1).unwrap();
        b.placement = LabelPlacement::LineBelow;
        let mut c = LabelClass::new("Seas", GeometryKind::Polygon, 1).unwrap();
        c.spread = 2.5;
        c.curve_limit = 0.1;
        let mut d = LabelClass::new("Lakes", GeometryKind::Polygon, 0).unwrap();
        d.placement = LabelPlacement::Horizontal;
        let mut e = LabelClass::new("Bays", GeometryKind::Polygon, 0).unwrap();
        e.placement = LabelPlacement::Curved;
        let mut f = LabelClass::new("Plains", GeometryKind::Polygon, 0).unwrap();
        f.placement = LabelPlacement::Parallel;
        f.no_flip = true;
        let mut g = LabelClass::new("Moors", GeometryKind::Polygon, 0).unwrap();
        g.graticule = true;
        let mut h = LabelClass::new("Isles", GeometryKind::Polygon, 0).unwrap();
        h.zones_inside = [2, 3, 2, 0, 1, 0, 4, 5, 4];
        h.zones_outside = [1, 2, 3, 0, 4, 5, 6, 7];
        h.anchor = LeaderAnchor::Outline;
        h.over_holes = true;
        h.avoid_polygons = true;
        h.largest_part = false;
        h.polygon_repeat = true;
        h.polygon_repeat_distance = 250.0;
        h.overrun = 3.0;
        h.min_area = 120.0;
        h.leader = LeaderStyle {
            show: false,
            color: Some(Color::rgb(200, 10, 20)),
            width: 1.5,
            opacity: 0.5,
            dashed: true,
            dash: 5.0,
            dash_gap: 3.0,
            halo: true,
            halo_color: Some(Color::rgb(250, 250, 240)),
            halo_width: 2.0,
            end: LeaderEnd::Arrow,
            end_size: 4.0,
            label_gap: 2.5,
            anchor_gap: 1.5,
        };
        let classes = vec![a, b, c, d, e, f, g, h];
        let text = classes_to_json(&classes);
        assert_eq!(classes_from_json(&text).unwrap(), classes);
        assert_eq!(classes_to_json(&classes_from_json(&text).unwrap()), text);

        let sparse = r##"{"version":1,"extra":1,"classes":[{"color":"#102030","placement":"on_line","x":2}]}"##;
        let read = classes_from_json(sparse).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].text, "");
        assert_eq!(read[0].font, None);
        assert!(read[0].halo);
        assert_eq!(read[0].halo_color, HALO_COLOR);
        assert_eq!(read[0].halo_width, 2.2);
        assert_eq!(read[0].weight, 400.0);
        assert_eq!(read[0].case, TextCase::Normal);
        assert_eq!(read[0].letter_spacing, 0.0);
        assert_eq!(read[0].opacity, 1.0);
        assert_eq!(read[0].placement, LabelPlacement::OnLine);
        assert_eq!(read[0].offset, 4.0);
        assert_eq!(read[0].positions, PositionRank::defaults());
        assert!(read[0].repeat);
        assert_eq!(read[0].repeat_distance, 600.0);
        assert_eq!(read[0].curve_limit, 0.30);
        assert_eq!(read[0].spread, 1.0);
        assert_eq!(read[0].smoothing, 1.5);
        assert_eq!(read[0].buffer, 1.0);
        assert!(read[0].remove_duplicates);
        assert_eq!(read[0].duplicate_distance, 300.0);
        assert!(!read[0].graticule);
        assert!(!read[0].no_flip);
        assert_eq!(read[0].zones_inside, [1; 9]);
        assert_eq!(read[0].zones_outside, [0; 8]);
        assert_eq!(read[0].anchor, LeaderAnchor::Inside);
        assert!(!read[0].over_holes && !read[0].avoid_polygons && !read[0].polygon_repeat);
        assert!(read[0].largest_part);
        assert_eq!(read[0].polygon_repeat_distance, 400.0);
        assert_eq!((read[0].overrun, read[0].min_area), (0.0, 0.0));
        assert_eq!(read[0].leader, LeaderStyle::default());
        let leader = r##"{"version":1,"classes":[{"color":"#102030","placement":"best_fit","leader":{"width":2,"end":"dot"}}]}"##;
        let leader = classes_from_json(leader).unwrap()[0].leader;
        assert_eq!((leader.width, leader.end), (2.0, LeaderEnd::Dot));
        assert!(leader.show);
        assert_eq!(leader.color, None);
        let point = r##"{"version":1,"classes":[{"color":"#102030","placement":"around"}]}"##;
        assert_eq!(classes_from_json(point).unwrap()[0].offset, 2.0);
        let polygon = r##"{"version":1,"classes":[{"color":"#102030","placement":"best_fit"}]}"##;
        let polygon = &classes_from_json(polygon).unwrap()[0];
        assert_eq!(polygon.placement, LabelPlacement::BestFit);
        assert_eq!(polygon.spread, 1.0);
        assert_eq!(polygon.curve_limit, 0.2);
        assert_eq!(
            classes_from_json(r#"{"version":9,"classes":[]}"#),
            Err(StyleError::Version(Some(9)))
        );
    }

    #[test]
    fn the_default_class_reproduces_the_label_style() {
        let c = LabelClass::from_style("Default", &style("name"), GeometryKind::Point, 5).unwrap();
        assert_eq!(c.name, "Default");
        assert_eq!(c.text, "name");
        assert_eq!(c.filter, "");
        assert_eq!(c.font.as_deref(), Some("Serif"));
        assert_eq!(c.size, 13.0);
        assert_eq!(c.color, Color::rgb(10, 20, 30));
        assert!(c.italic);
        assert_eq!(c.weight, 400.0);
        assert_eq!(c.case, TextCase::Normal);
        assert_eq!(c.letter_spacing, 0.0);
        assert_eq!(c.opacity, 1.0);
        assert!(c.halo);
        assert_eq!(c.halo_color, Color::rgba(255, 255, 255, 217));
        assert_eq!(c.halo_width, 2.2);
        assert_eq!(c.priority, 5);
        assert_eq!(c.priority_field.as_deref(), Some("pop"));
        assert_eq!(c.placement, LabelPlacement::Around);
        assert_eq!(c.offset, 2.0);
        assert_eq!(c.positions.len(), 8);
        assert!(c.positions.iter().all(|r| r.on));
        assert_eq!(c.positions[0].position, LabelPosition::TopRight);
        assert!(c.repeat);
        assert_eq!(c.repeat_distance, 600.0);
        assert_eq!(c.curve_limit, 0.30);
        assert_eq!(c.smoothing, 1.5);
        assert_eq!(c.buffer, 1.0);
        assert!(c.remove_duplicates);
        assert_eq!(c.duplicate_distance, 300.0);

        let line = LabelClass::from_style("D", &style("old name"), GeometryKind::Line, 0).unwrap();
        assert_eq!(line.text, "\"old name\"");
        assert_eq!(line.placement, LabelPlacement::AboveOrBelow);
        assert_eq!(line.letter_spacing, 0.5 / 13.0);
        assert_eq!(line.offset, 4.0);
        let new = LabelClass::new("R", GeometryKind::Line, 0).unwrap();
        assert_eq!(new.offset, 4.0);
        assert_eq!(new.repeat_distance, 600.0);
        assert_eq!(new.letter_spacing * new.size, 0.5);
        assert_eq!(new.halo_width, 2.2);
        let point = LabelClass::new("T", GeometryKind::Point, 0).unwrap();
        assert_eq!(point.letter_spacing, 0.0);
        assert!(!new.remove_duplicates);
        assert!(!point.remove_duplicates);
        assert_eq!(point.offset, 2.0);
        assert_eq!(point.positions, PositionRank::defaults());
        assert_eq!(field_ref("end"), "\"end\"");
        assert_eq!(field_ref("a\"b"), "\"a\"\"b\"");

        let mut off = style("name");
        off.enabled = false;
        assert_eq!(
            LabelClass::from_style("D", &off, GeometryKind::Point, 0),
            None
        );
        let mut none = style("name");
        none.field = None;
        assert_eq!(
            LabelClass::from_style("D", &none, GeometryKind::Point, 0),
            None
        );
        let polygon =
            LabelClass::from_style("D", &style("name"), GeometryKind::Polygon, 0).unwrap();
        assert_eq!(polygon.placement, LabelPlacement::BestFit);
        assert_eq!(polygon.curve_limit, 0.2);
        assert_eq!(polygon.spread, 1.0);
    }

    #[test]
    fn each_case_changes_the_text() {
        let text = "the river ODE";
        assert_eq!(TextCase::Normal.apply(text), text);
        assert_eq!(TextCase::Upper.apply(text), "THE RIVER ODE");
        assert_eq!(TextCase::Lower.apply(text), "the river ode");
        assert_eq!(TextCase::Title.apply(text), "The River Ode");
        assert_eq!(TextCase::SmallCaps.apply(text), text);
        assert_eq!(TextCase::AllSmallCaps.apply(text), text);
        for c in TextCase::ALL {
            assert_eq!(TextCase::from_id(c.id()), Some(c));
        }
        assert_eq!(TextCase::AllSmallCaps.id(), "all_small_caps");
        assert_eq!(TextCase::Title.name(), "Title case");
    }

    #[test]
    fn the_placements_match_the_feature_type() {
        use GeometryKind::{Line, Point, Polygon};
        assert_eq!(LabelPlacement::choices(Point), &[LabelPlacement::Around]);
        assert_eq!(LabelPlacement::choices(Line).len(), 4);
        assert_eq!(
            LabelPlacement::choices(Polygon),
            &[
                LabelPlacement::BestFit,
                LabelPlacement::Curved,
                LabelPlacement::Parallel,
                LabelPlacement::Horizontal
            ]
        );
        assert_eq!(
            LabelPlacement::default_for(Point),
            Some(LabelPlacement::Around)
        );
        assert_eq!(
            LabelPlacement::default_for(Line),
            Some(LabelPlacement::AboveOrBelow)
        );
        assert_eq!(
            LabelPlacement::default_for(Polygon),
            Some(LabelPlacement::BestFit)
        );
        for p in ALL_PLACEMENTS {
            let fits = [p.fits(Point), p.fits(Line), p.fits(Polygon)];
            assert_eq!(fits.iter().filter(|f| **f).count(), 1);
            assert_eq!(p.fits(Polygon), p.is_polygon());
            assert_eq!(LabelPlacement::from_key(p.key()), Some(p));
        }
        assert_eq!(LabelPlacement::BestFit.name(), "Best fit");
        assert_eq!(LabelPlacement::BestFit.key(), "best_fit");
        assert_eq!(LabelPlacement::Curved.name(), "Curved");
        assert_eq!(LabelPlacement::Curved.key(), "curved");
        assert_eq!(LabelPlacement::Horizontal.name(), "Horizontal");
        assert_eq!(LabelPlacement::Horizontal.key(), "horizontal");
        assert_eq!(LabelPlacement::Parallel.name(), "Along parallel");
        assert_eq!(LabelPlacement::Parallel.key(), "parallel");
        assert_eq!(LabelPlacement::AboveOrBelow.name(), "Above or below");
        assert_eq!(LabelPlacement::Around.name(), "Around point");
        let area = LabelClass::new("A", Polygon, 0).unwrap();
        assert_eq!(area.placement, LabelPlacement::BestFit);
        assert_eq!(area.curve_limit, 0.2);
        assert_eq!(area.spread, 1.0);
        assert_eq!(area.case, TextCase::Upper);
        assert_eq!(area.letter_spacing, 0.15);
        for p in LabelPosition::DEFAULT_ORDER {
            assert_eq!(LabelPosition::from_key(p.key()), Some(p));
        }
        assert_eq!(LabelPosition::TopRight.name(), "Top right");
        assert_eq!(LabelPosition::BottomLeft.key(), "bottom_left");
    }

    #[test]
    fn a_point_placement_of_one_side_reads_as_a_ranked_position() {
        for (key, side) in [
            ("right", LabelPosition::Right),
            ("left", LabelPosition::Left),
            ("above", LabelPosition::Top),
            ("below", LabelPosition::Bottom),
        ] {
            let text = format!(
                r##"{{"version":1,"classes":[{{"color":"#102030","placement":"{key}"}}]}}"##
            );
            let c = &classes_from_json(&text).unwrap()[0];
            assert_eq!(c.placement, LabelPlacement::Around);
            assert_eq!(c.offset, 2.0);
            assert_eq!(c.positions.len(), 8);
            let on: Vec<_> = c.positions.iter().filter(|r| r.on).collect();
            assert_eq!(on.len(), 1);
            assert_eq!(on[0].position, side);
        }
    }
}
