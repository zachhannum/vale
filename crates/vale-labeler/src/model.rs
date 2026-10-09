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
}

impl Default for PointPlacement {
    fn default() -> Self {
        Self {
            positions: PointPosition::DEFAULT_ORDER.to_vec(),
            offset: 2.0,
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
    /// Largest angle between two neighbor glyphs, in radians.
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

/// Point or line placement rules.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    Point(PointPlacement),
    Line(LinePlacement),
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
}

/// Page-space geometry of a feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point { position: Point, symbol_radius: f64 },
    Line { path: Vec<Point>, stroke_width: f64 },
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
    /// Minimum distance between labels with the same class and text.
    pub duplicate_distance: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            seed: 0x5EED,
            improve: true,
            duplicate_distance: None,
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
