//! Public input and output types.

use crate::collide::OrientedBox;
use kurbo::{Affine, BezPath, Point, Rect};
use parley::FontData;

// ---------- input ----------

/// Font and spacing for a label class.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Family name as registered, for example "Noto Sans".
    pub family: String,
    /// Font size in page units.
    pub size: f64,
    /// Font weight; 400.0 is regular.
    pub weight: f32,
    /// Use the italic face.
    pub italic: bool,
    /// Extra space after each glyph, in page units.
    pub letter_spacing: f64,
    /// Small capitals.
    pub caps: Caps,
}

/// Which letters show as small capitals.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Caps {
    /// No small capitals.
    #[default]
    Normal,
    /// Lowercase letters show as small capitals.
    Small,
    /// All letters show as small capitals.
    AllSmall,
}

impl TextStyle {
    /// Regular weight, upright, no letter spacing.
    pub fn new(family: impl Into<String>, size: f64) -> Self {
        Self {
            family: family.into(),
            size,
            weight: 400.0,
            italic: false,
            letter_spacing: 0.0,
            caps: Caps::Normal,
        }
    }
}

/// Where a point label sits relative to its symbol.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointPosition {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    Right,
    Left,
    Top,
    Bottom,
}

impl PointPosition {
    /// The default preference order.
    pub const DEFAULT_ORDER: [PointPosition; 8] = [
        PointPosition::TopRight,
        PointPosition::TopLeft,
        PointPosition::BottomRight,
        PointPosition::BottomLeft,
        PointPosition::Right,
        PointPosition::Left,
        PointPosition::Top,
        PointPosition::Bottom,
    ];
}

/// Placement rules for point features.
#[derive(Clone, Debug, PartialEq)]
pub struct PointPlacement {
    /// Preference order; the first is best.
    pub positions: Vec<PointPosition>,
    /// Gap between the symbol edge and the text.
    pub offset: f64,
    /// The text follows the parallels of [`LabelInput::parallels`], straight or curved.
    pub graticule: bool,
    /// Text that follows the parallels reads from west to east, also where it is upside down.
    pub no_flip: bool,
}

impl Default for PointPlacement {
    fn default() -> Self {
        Self {
            positions: PointPosition::DEFAULT_ORDER.to_vec(),
            offset: 2.0,
            graticule: false,
            no_flip: false,
        }
    }
}

/// Which side of a line the text sits on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LineSide {
    Above,
    Below,
    Centered,
}

/// Placement rules for line features.
#[derive(Clone, Debug, PartialEq)]
pub struct LinePlacement {
    /// Preference order of sides.
    pub sides: Vec<LineSide>,
    /// Gap between the stroke edge and the text.
    pub offset: f64,
    /// Distance between repeated labels; `None` gives one label per feature.
    pub repeat_distance: Option<f64>,
    /// The angle between two neighbor glyphs, in radians, above which a candidate costs more.
    /// A candidate that turns more than 1.6 times this angle is not used.
    pub max_glyph_turn: f64,
    /// Smoothing window, in multiples of the font size.
    pub smoothing: f64,
}

impl Default for LinePlacement {
    fn default() -> Self {
        Self {
            sides: vec![LineSide::Above, LineSide::Below],
            offset: 2.0,
            repeat_distance: None,
            max_glyph_turn: 0.30,
            smoothing: 1.5,
        }
    }
}

/// How a polygon label follows its shape.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum PolygonMode {
    /// Curved text in a long shape, and horizontal text in a compact shape.
    #[default]
    Auto,
    /// Text that curves along the long axis of every shape. Where no curved text fits, the
    /// text is horizontal.
    Curved,
    /// Horizontal text.
    Horizontal,
    /// Text along a parallel of [`LabelInput::parallels`] through the shape, at the latitude
    /// with the most room. Where no such text fits, the text is horizontal.
    Parallel,
}

/// A zone of a polygon label. Inside the polygon, the zones are the cells of a three by three
/// grid over the bounds of the shape. Outside it, a zone is a direction from the anchor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Zone {
    TopLeft,
    Top,
    TopRight,
    Left,
    /// Inside only.
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Zone {
    /// The zones inside a polygon, row by row from the top.
    pub const INSIDE: [Zone; 9] = [
        Zone::TopLeft,
        Zone::Top,
        Zone::TopRight,
        Zone::Left,
        Zone::Center,
        Zone::Right,
        Zone::BottomLeft,
        Zone::Bottom,
        Zone::BottomRight,
    ];

    /// The zones outside a polygon, row by row from the top.
    pub const OUTSIDE: [Zone; 8] = [
        Zone::TopLeft,
        Zone::Top,
        Zone::TopRight,
        Zone::Left,
        Zone::Right,
        Zone::BottomLeft,
        Zone::Bottom,
        Zone::BottomRight,
    ];

    /// The column and the row of the zone, each from -1 to 1. The row grows down the page.
    pub fn offset(self) -> (i32, i32) {
        let i = self as i32;
        (i % 3 - 1, i / 3 - 1)
    }
}

/// The rank of each zone of a polygon label. Rank 1 is best, and a higher rank costs more.
/// Rank 0 blocks the zone. Zones apply to horizontal text, and only a zone outside the
/// polygon gets a leader line.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PolygonZones {
    /// In the order of [`Zone::INSIDE`].
    pub inside: [u8; 9],
    /// In the order of [`Zone::OUTSIDE`].
    pub outside: [u8; 8],
}

impl PolygonZones {
    /// The rank of `zone` inside the polygon.
    pub fn inside(&self, zone: Zone) -> u8 {
        self.inside[zone as usize]
    }

    /// The rank of `zone` outside the polygon. The center has rank 0.
    pub fn outside(&self, zone: Zone) -> u8 {
        Zone::OUTSIDE
            .iter()
            .position(|z| *z == zone)
            .map_or(0, |i| self.outside[i])
    }
}

impl Default for PolygonZones {
    /// All zones inside have rank 1, and all zones outside are blocked.
    fn default() -> Self {
        Self {
            inside: [1; 9],
            outside: [0; 8],
        }
    }
}

/// The point of a polygon where the leader line of a label outside the polygon starts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeaderAnchor {
    /// The centroid of the shape, which can be outside a concave shape.
    Center,
    /// The visual center, which is always inside the shape.
    #[default]
    Inside,
    /// The point of the outline nearest to the label.
    Outline,
}

/// Placement rules for polygon features.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonPlacement {
    pub mode: PolygonMode,
    /// The most extra space between two letters, in multiples of the font size. Zero keeps the
    /// letters together.
    pub max_spread: f64,
    /// The angle between two neighbor glyphs of a curved label, in radians, above which a
    /// candidate costs more. A candidate that turns more than 1.6 times this angle is not used.
    pub max_glyph_turn: f64,
    /// Horizontal text follows the parallels of [`LabelInput::parallels`], straight or curved.
    pub graticule: bool,
    /// Text that follows the parallels reads from west to east, also where it is upside down.
    pub no_flip: bool,
    pub zones: PolygonZones,
    pub anchor: LeaderAnchor,
    /// A label can cover the holes of the polygon, for example the islands of a lake.
    pub over_holes: bool,
    /// A label avoids the other polygon features, as it avoids holes.
    pub avoid_other_polygons: bool,
    /// Only the largest part of a polygon with many parts gets a label. Otherwise each part
    /// gets one, and a part where the label does not fit gets none.
    pub largest_part_only: bool,
    /// The distance between repeated labels of a large polygon; `None` gives one label per part.
    pub repeat_distance: Option<f64>,
    /// The distance that a label can go past the outline, in page units.
    pub overrun: f64,
    /// A part of the visible polygon with less area than this gets no label, in square page
    /// units.
    pub min_area: f64,
}

impl Default for PolygonPlacement {
    fn default() -> Self {
        Self {
            mode: PolygonMode::Auto,
            max_spread: 1.0,
            max_glyph_turn: 0.2,
            graticule: false,
            no_flip: false,
            zones: PolygonZones::default(),
            anchor: LeaderAnchor::default(),
            over_holes: false,
            avoid_other_polygons: false,
            largest_part_only: true,
            repeat_distance: None,
            overrun: 0.0,
            min_area: 0.0,
        }
    }
}

/// Point, line, or polygon placement rules.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    Point(PointPlacement),
    Line(LinePlacement),
    Polygon(PolygonPlacement),
}

/// A way to place a label that does not fit. Removal comes after the last fallback of a class.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Fallback {
    /// Smaller font sizes, one step at a time, down to [`Fallbacks::min_size`].
    SmallerFont,
    /// The words on two or more horizontal lines, up to [`Fallbacks::max_lines`].
    Stack,
    /// The words of [`Fallbacks::abbreviations`] in their short form.
    Abbreviate,
    /// A point label farther from its symbol, or a polygon label outside the shape, with a
    /// leader line. A line label has no leader line.
    Leader,
}

impl Fallback {
    /// All fallbacks in the default order.
    pub const ALL: [Fallback; 4] = [
        Fallback::SmallerFont,
        Fallback::Stack,
        Fallback::Abbreviate,
        Fallback::Leader,
    ];
}

/// The fallbacks of a label class and their options.
#[derive(Clone, Debug, PartialEq)]
pub struct Fallbacks {
    /// The fallbacks to try, in order. Each one starts again from the label of the class.
    pub order: Vec<Fallback>,
    /// The smallest font size of [`Fallback::SmallerFont`], in page units.
    pub min_size: f64,
    /// The step between two font sizes, in page units.
    pub size_step: f64,
    /// The most lines of [`Fallback::Stack`].
    pub max_lines: usize,
    /// Pairs of a word and its short form, for example "Mount" and "Mt.". A word matches as a
    /// whole word, with the same case.
    pub abbreviations: Vec<(String, String)>,
    /// The extra distance of a point label with a leader line from its symbol, in page units.
    pub leader_distance: f64,
}

impl Default for Fallbacks {
    /// No fallbacks, so a label that does not fit is removed.
    fn default() -> Self {
        Self {
            order: Vec::new(),
            min_size: 6.0,
            size_step: 1.0,
            max_lines: 2,
            abbreviations: Vec::new(),
            leader_distance: 12.0,
        }
    }
}

/// A group of labels that share style and rules.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelClass {
    pub name: String,
    pub style: TextStyle,
    /// Higher is placed first.
    pub priority: i32,
    /// Collision boxes grow by this on every side.
    pub margin: f64,
    pub placement: Placement,
    /// Minimum distance between labels of this class with the same text.
    /// `None` keeps all of them.
    pub duplicate_distance: Option<f64>,
    pub fallbacks: Fallbacks,
}

/// Page-space geometry of a feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point {
        position: Point,
        symbol_radius: f64,
    },
    Line {
        path: Vec<Point>,
        stroke_width: f64,
    },
    /// Closed rings with the even-odd fill rule, so holes and separate parts need no order.
    /// A ring inside an odd number of other rings is a hole.
    Polygon {
        rings: Vec<Vec<Point>>,
    },
}

/// One feature to label.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    /// The caller's stable ID, returned unchanged.
    pub id: u64,
    /// Index into [`LabelInput::classes`].
    pub class: usize,
    pub text: String,
    /// Order inside a class; higher goes first.
    pub priority: f64,
    pub geometry: Geometry,
}

/// How an obstacle affects labels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ObstacleKind {
    /// Labels may not overlap it.
    Hard,
    /// Overlap adds `cost`.
    Soft { cost: f64 },
}

/// A rectangle that labels avoid.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Obstacle {
    pub rect: Rect,
    pub kind: ObstacleKind,
}

/// Engine options.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Seed for the local search.
    pub seed: u64,
    /// Run the seeded improvement search after the greedy pass.
    pub improve: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            seed: 0x5EED,
            improve: true,
        }
    }
}

/// Everything `place_labels` needs.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelInput {
    /// Every label must lie inside.
    pub bounds: Rect,
    pub classes: Vec<LabelClass>,
    pub features: Vec<Feature>,
    pub obstacles: Vec<Obstacle>,
    /// Lines of constant latitude in page space, each from west to east. Labels that follow
    /// the graticule use the nearest one, so the lines must be close together, for example
    /// one each degree, and cross the whole page. With no lines, the parallels are horizontal.
    pub parallels: Vec<Vec<Point>>,
    pub options: Options,
}

// ---------- output ----------

/// The candidate that was chosen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CandidateKind {
    Point(PointPosition),
    /// `start` is the arc length on the smoothed line.
    Line {
        side: LineSide,
        start: f64,
    },
    /// Inside a polygon. `curved` text follows the long axis or a parallel; other text is
    /// horizontal or follows the parallels.
    Polygon {
        curved: bool,
    },
}

/// One glyph with its transform from glyph-local space to page space.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub id: u32,
    pub transform: Affine,
}

/// Glyphs that share a font and size.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedGlyphRun {
    /// Font bytes (shared blob) and collection index.
    pub font: FontData,
    pub font_size: f32,
    /// Variable-font position; empty for static fonts.
    pub normalized_coords: Vec<i16>,
    pub glyphs: Vec<PlacedGlyph>,
}

/// A line from a feature to its label.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Leader {
    /// The end on the feature.
    pub anchor: Point,
    /// The end on the edge of the ink bounds of the label.
    pub end: Point,
}

/// A label that was placed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLabel {
    pub feature: u64,
    pub class: usize,
    /// 0 for the first label of a feature.
    pub repeat: u32,
    pub text: String,
    pub kind: CandidateKind,
    pub glyph_runs: Vec<PlacedGlyphRun>,
    /// The collision boxes that were used.
    pub boxes: Vec<OrientedBox>,
    /// Axis-aligned bounds of `boxes`.
    pub bounds: Rect,
    pub cost: f64,
    pub leader: Option<Leader>,
    /// The fallback that placed the label, or `None` if the label of the class fits.
    pub fallback: Option<Fallback>,
}

impl PlacedLabel {
    /// All glyph outlines in page space.
    pub fn outlines(&self) -> BezPath {
        crate::text::label_outlines(self)
    }
}

/// Why a label was not placed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnplacedReason {
    EmptyText,
    InvalidClass,
    GeometryMismatch,
    FontNotFound,
    MissingGlyphs,
    OutOfBounds,
    Obstructed,
    LineTooShort,
    TooCurved,
    /// The label does not fit inside the visible part of the polygon.
    AreaTooSmall,
    /// The visible part of the polygon has less area than the class allows.
    BelowMinimumArea,
    Collision,
}

/// A label that was not placed, with the reason.
#[derive(Clone, Debug, PartialEq)]
pub struct UnplacedLabel {
    pub feature: u64,
    pub class: usize,
    pub repeat: u32,
    pub text: String,
    pub reason: UnplacedReason,
}

/// The result of `place_labels`.
#[derive(Clone, Debug, PartialEq)]
pub struct Labeling {
    /// Sorted by (feature ID, repeat).
    pub placed: Vec<PlacedLabel>,
    /// Sorted by (feature ID, repeat).
    pub unplaced: Vec<UnplacedLabel>,
    /// The energy defined in T7.
    pub total_cost: f64,
}
