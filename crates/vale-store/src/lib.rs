//! The project model and its GeoPackage file: a world, its layers, and their features.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

pub use uuid::Uuid;

mod atlas;
mod gpkg;
mod image_tile;
mod scale_bar;
mod tile;
mod undo;

pub use atlas::{
    Atlas, Border, CoordFormat, EdgeLabels, EdgeOrientation, Edges, FrameId, FrameRecord,
    FrameShape, Furniture, FurnitureId, FurnitureRecord, FurnitureText, Graticule, LegendEntry,
    LegendItem, MapId, MapRecord, NorthArrowItem, PageId, PageRecord, PageUnits, ProjectionPreset,
    TextAlign, TextCase, TextItem, title_case,
};
pub use gpkg::{
    FORMAT_VERSION, ProjectFile, Restored, SaveStats, StoreError, Summary, Thumbnail, TileChanges,
    TileKey, summary,
};
pub use image_tile::{IMAGE_TILE_SIZE, decode_tile, encode_tile, image_levels, level_size};
pub use scale_bar::{
    ScaleBarItem, ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, TickFrequency, UnitLabelStyle,
    UnitPosition,
};
pub use tile::TILE_SIZE;
pub use undo::UNDO_LIMIT;

/// A color as red, green, and blue.
pub type Rgb = [u8; 3];

/// A position as `[lon, lat]` in degrees.
pub type LonLat = [f64; 2];

/// An attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    /// The number, for `Number` only.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Text for a label. `Null` and blank text give `None`.
    pub fn label(&self) -> Option<String> {
        match self {
            Value::Null => None,
            Value::Bool(b) => Some(b.to_string()),
            Value::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    Some(format!("{}", *n as i64))
                } else {
                    Some(format!("{n}"))
                }
            }
            Value::Text(s) => {
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            }
        }
    }
}

/// The largest whole number that a field holds exactly.
const WHOLE_MAX: f64 = 9_007_199_254_740_992.0;

/// The type of the values of a field.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FieldType {
    Text,
    /// A whole number. Its values are `Value::Number` with a whole value.
    Integer,
    /// A decimal number.
    Real,
    Bool,
}

impl FieldType {
    pub const ALL: [FieldType; 4] = [
        FieldType::Text,
        FieldType::Integer,
        FieldType::Real,
        FieldType::Bool,
    ];

    /// The name of the type in the UI.
    pub fn label(self) -> &'static str {
        match self {
            FieldType::Text => "Text",
            FieldType::Integer => "Whole number",
            FieldType::Real => "Decimal number",
            FieldType::Bool => "True or false",
        }
    }

    /// The name of the type in the project file.
    pub(crate) fn id(self) -> &'static str {
        match self {
            FieldType::Text => "text",
            FieldType::Integer => "integer",
            FieldType::Real => "number",
            FieldType::Bool => "bool",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<FieldType> {
        FieldType::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The value of this type that the text gives. Text that is not a value
    /// of the type gives `None`.
    pub fn parse(self, text: &str) -> Option<Value> {
        let number = || text.trim().parse::<f64>().ok().filter(|n| !n.is_nan());
        match self {
            FieldType::Text => Some(Value::Text(text.to_string())),
            FieldType::Integer => number()
                .filter(|n| n.fract() == 0.0 && n.abs() <= WHOLE_MAX)
                .map(Value::Number),
            FieldType::Real => number().map(Value::Number),
            FieldType::Bool => match text.trim().to_lowercase().as_str() {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            },
        }
    }

    /// The value of this type that is nearest to the value. A value that has
    /// no such value gives `Null`.
    pub fn convert(self, value: &Value) -> Value {
        let number = |n: f64| match self {
            FieldType::Integer => {
                let n = n.round();
                (n.abs() <= WHOLE_MAX).then_some(Value::Number(n))
            }
            _ => (!n.is_nan()).then_some(Value::Number(n)),
        };
        let converted = match (self, value) {
            (_, Value::Null) => None,
            (FieldType::Text, Value::Text(_)) => Some(value.clone()),
            (FieldType::Text, _) => value.label().map(Value::Text),
            (FieldType::Bool, Value::Bool(_)) => Some(value.clone()),
            (FieldType::Bool, Value::Number(n)) => Some(Value::Bool(*n != 0.0)),
            (FieldType::Bool, Value::Text(s)) => self.parse(s),
            (_, Value::Bool(b)) => Some(Value::Number(*b as u8 as f64)),
            (_, Value::Number(n)) => number(*n),
            (_, Value::Text(s)) => FieldType::Real.parse(s).and_then(|v| number(v.as_f64()?)),
        };
        converted.unwrap_or(Value::Null)
    }
}

/// One field of the attribute schema of a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub kind: FieldType,
    /// The value of the field in a new feature.
    pub default: Value,
}

impl Field {
    /// A field with no default value.
    pub fn new(name: impl Into<String>, kind: FieldType) -> Field {
        Field {
            name: name.into(),
            kind,
            default: Value::Null,
        }
    }
}

/// The fields of an import, sorted by name. Each type is the type of all the
/// values of its field. A field with values of more than one type, or with
/// no values, is text. Each value turns into a value of the type of its
/// field, and each feature gets each field.
pub(crate) fn infer_fields(features: &mut [Feature]) -> Vec<Field> {
    let names: BTreeSet<&String> = features.iter().flat_map(|f| f.attributes.keys()).collect();
    let fields: Vec<Field> = names
        .into_iter()
        .map(|name| {
            let mut kinds = features
                .iter()
                .filter_map(|f| match f.attributes.get(name)? {
                    Value::Null => None,
                    Value::Bool(_) => Some(FieldType::Bool),
                    Value::Number(_) => Some(FieldType::Real),
                    Value::Text(_) => Some(FieldType::Text),
                });
            let first = kinds.next().unwrap_or(FieldType::Text);
            let kind = match kinds.all(|k| k == first) {
                true => first,
                false => FieldType::Text,
            };
            Field::new(name.clone(), kind)
        })
        .collect();
    for f in features {
        for field in &fields {
            let value = f.attributes.get(&field.name).unwrap_or(&Value::Null);
            let value = field.kind.convert(value);
            f.attributes.insert(field.name.clone(), value);
        }
    }
    fields
}

/// The kind of geometry that a layer holds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GeometryKind {
    Point,
    Line,
    Polygon,
}

impl GeometryKind {
    /// A lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            GeometryKind::Point => "point",
            GeometryKind::Line => "line",
            GeometryKind::Polygon => "polygon",
        }
    }

    /// The kind with this lower-case name.
    pub fn from_name(name: &str) -> Option<GeometryKind> {
        [
            GeometryKind::Point,
            GeometryKind::Line,
            GeometryKind::Polygon,
        ]
        .into_iter()
        .find(|k| k.name() == name)
    }
}

/// The geometry of one feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Points(Vec<LonLat>),
    Lines(Vec<Vec<LonLat>>),
    /// Polygons, then rings (outer ring first), then positions.
    Polygons(Vec<Vec<Vec<LonLat>>>),
}

impl Geometry {
    /// The kind of this geometry.
    pub fn kind(&self) -> GeometryKind {
        match self {
            Geometry::Points(_) => GeometryKind::Point,
            Geometry::Lines(_) => GeometryKind::Line,
            Geometry::Polygons(_) => GeometryKind::Polygon,
        }
    }
}

/// A geometry with attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub geometry: Geometry,
    pub attributes: BTreeMap<String, Value>,
    uuid: Uuid,
}

impl Feature {
    /// A feature with a new UUID.
    pub fn new(geometry: Geometry, attributes: BTreeMap<String, Value>) -> Feature {
        Feature::with_uuid(Uuid::new_v4(), geometry, attributes)
    }

    pub(crate) fn with_uuid(
        uuid: Uuid,
        geometry: Geometry,
        attributes: BTreeMap<String, Value>,
    ) -> Feature {
        Feature {
            geometry,
            attributes,
            uuid,
        }
    }

    /// The stable identity. An edit, a save, and a load keep it.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
}

/// The identity of a layer. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u32);

/// What a layer holds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LayerKind {
    /// Features of one geometry kind.
    Vector(GeometryKind),
    /// The raster with this ID.
    Raster(RasterId),
    /// The image with this ID.
    Image(ImageId),
}

impl LayerKind {
    /// A lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            LayerKind::Vector(kind) => kind.name(),
            LayerKind::Raster(_) => "raster",
            LayerKind::Image(_) => "image",
        }
    }
}

/// One row of the draw order: a set of features of one geometry kind, a
/// raster, or an image. A raster layer and an image layer have no source, no
/// fields, and no features.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    /// The views draw the layer.
    pub visible: bool,
    /// The name of the imported file.
    pub source: Option<String>,
    /// The attribute schema, in order. No two fields have the same name.
    /// Each feature has a value for each field, and each value is `Null` or
    /// a value of the type of its field.
    pub fields: Vec<Field>,
    /// A feature ID is its index here.
    pub features: Vec<Feature>,
    /// The style of the layer as JSON. `vale-store` does not read it. `None`
    /// means the default style.
    pub style: Option<String>,
}

impl Layer {
    /// The geometry kind of a vector layer.
    pub fn geometry(&self) -> Option<GeometryKind> {
        match self.kind {
            LayerKind::Vector(kind) => Some(kind),
            LayerKind::Raster(_) | LayerKind::Image(_) => None,
        }
    }

    /// The raster of a raster layer.
    pub fn raster(&self) -> Option<RasterId> {
        match self.kind {
            LayerKind::Raster(id) => Some(id),
            LayerKind::Vector(_) | LayerKind::Image(_) => None,
        }
    }

    /// The image of an image layer.
    pub fn image(&self) -> Option<ImageId> {
        match self.kind {
            LayerKind::Image(id) => Some(id),
            LayerKind::Vector(_) | LayerKind::Raster(_) => None,
        }
    }

    /// The field with this name.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The index of the field with this name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// The names of the fields, in order.
    pub fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.name.clone()).collect()
    }

    /// True if a field can have this name. The name is not blank, and it is
    /// not `fid`, `geom`, or `uuid`. No field other than `except` has the
    /// name. The check ignores case.
    pub fn name_free(&self, name: &str, except: Option<&str>) -> bool {
        let lower = name.to_lowercase();
        !name.trim().is_empty()
            && !["fid", "geom", "uuid"].contains(&lower.as_str())
            && !self
                .fields
                .iter()
                .any(|f| Some(f.name.as_str()) != except && f.name.to_lowercase() == lower)
    }
}

/// The identity of a raster. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RasterId(pub u32);

/// The record of one raster: a cube map of 16-bit values. Its layer has the
/// name and the visibility. The project file holds its tiles, and the project
/// in memory does not.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub id: RasterId,
    /// How much the raster hides the layers below it, from 0 to 1. A copy of the style of the layer.
    pub opacity: f32,
    /// How the globe colors the values. A copy of the style of the layer.
    pub display: RasterDisplay,
    /// How the raster mixes with the layers below it. A copy of the style of the layer.
    pub blend: BlendMode,
    /// The number of pixels along the edge of a face.
    pub face_size: u32,
    /// The unit of the values, for example `m`.
    pub unit: String,
    /// The value of pixel 0.
    pub value_min: f64,
    /// The value of pixel 65,535.
    pub value_max: f64,
    /// The pixel of each place that has no tile.
    pub base: u16,
    /// The stops at the edges of the bands, in ascending order of value. A
    /// copy of the style of the layer. An empty list gives the default bands
    /// of the range.
    pub stops: Vec<Stop>,
    /// The color of each band, from the lowest band up. There is one color
    /// fewer than stops. Band `i` runs from stop `i` to stop `i + 1`. A copy
    /// of the style of the layer.
    pub colors: Vec<Rgb>,
    /// How the globe smooths the values by the scale.
    pub smoothing: Smoothing,
    /// The shade of the slopes over the colors.
    pub hillshade: Hillshade,
    /// The globe draws the rivers of a height raster.
    pub rivers: bool,
    /// The globe draws the lines of the stops.
    pub lines: bool,
}

/// The identity of an image. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(pub u32);

/// The record of one image: an equirectangular picture of the full globe in
/// straight RGBA8, with no value range and no color stops. Its layer has the
/// name and the visibility. The project file holds the tiles of its pyramid,
/// and the project in memory does not. The tiles do not change after the
/// import.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub id: ImageId,
    /// The size of level 0, in pixels.
    pub width: u32,
    pub height: u32,
    /// The number of levels of the pyramid, as `image_levels` gives it.
    pub levels: u32,
    /// How much the image hides the layers below it, from 0 to 1, on top of
    /// the alpha of each pixel. A copy of the style of the layer.
    pub opacity: f32,
    /// How the image mixes with the layers below it. A copy of the style of the layer.
    pub blend: BlendMode,
}

/// The hillshade of a height layer. It keeps its opacity and its highlights
/// while it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hillshade {
    pub on: bool,
    /// How much the shade hides the colors, from 0 to 1.
    pub opacity: f32,
    /// The slopes toward the light get lighter. Without the highlights, only
    /// the slopes away from the light change.
    pub highlights: bool,
    /// The direction of the light, in degrees clockwise from north, from 0 up
    /// to 360.
    pub azimuth: f32,
    /// The height of the light, in degrees above the ground.
    pub altitude: f32,
    /// It makes the slopes steeper, on top of the change by the zoom.
    pub exaggeration: f32,
    /// The shade color. The slopes away from the light blend toward it.
    pub color: Rgb,
}

impl Hillshade {
    pub const OPACITY_DEFAULT: f32 = 0.5;
    pub const AZIMUTH_DEFAULT: f32 = 315.0;
    pub const ALTITUDE_DEFAULT: f32 = 45.0;
    pub const EXAGGERATION_DEFAULT: f32 = 1.0;
    pub const ALTITUDES: RangeInclusive<f32> = 5.0..=85.0;
    pub const EXAGGERATIONS: RangeInclusive<f32> = 0.25..=4.0;

    /// The hillshade with each value in its range. A value that is not a
    /// number gives the default. An infinite azimuth also gives the default.
    pub fn clamped(self) -> Hillshade {
        let azimuth = match self.azimuth.is_finite() {
            true => self.azimuth.rem_euclid(360.0),
            false => Hillshade::AZIMUTH_DEFAULT,
        };
        // The remainder of a value just below 0 rounds up to 360.
        let azimuth = if azimuth < 360.0 { azimuth } else { 0.0 };
        let clamp = |value: f32, range: RangeInclusive<f32>, default: f32| match value.is_nan() {
            true => default,
            false => value.clamp(*range.start(), *range.end()),
        };
        Hillshade {
            opacity: clamp_opacity(self.opacity),
            azimuth,
            altitude: clamp(
                self.altitude,
                Hillshade::ALTITUDES,
                Hillshade::ALTITUDE_DEFAULT,
            ),
            exaggeration: clamp(
                self.exaggeration,
                Hillshade::EXAGGERATIONS,
                Hillshade::EXAGGERATION_DEFAULT,
            ),
            ..self
        }
    }
}

impl Default for Hillshade {
    fn default() -> Self {
        Hillshade {
            on: false,
            opacity: Hillshade::OPACITY_DEFAULT,
            highlights: true,
            azimuth: Hillshade::AZIMUTH_DEFAULT,
            altitude: Hillshade::ALTITUDE_DEFAULT,
            exaggeration: Hillshade::EXAGGERATION_DEFAULT,
            color: [0, 0, 0],
        }
    }
}

/// The smoothing of the values of a raster by the scale. It keeps its size
/// while it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoothing {
    pub on: bool,
    /// The minimum feature size, in points. 0 gives the full detail.
    pub feature_size: f32,
}

impl Smoothing {
    /// The largest feature size, in points.
    pub const FEATURE_SIZE_MAX: f32 = 8.0;
    pub const FEATURE_SIZE_DEFAULT: f32 = 2.0;
}

impl Default for Smoothing {
    fn default() -> Self {
        Smoothing {
            on: true,
            feature_size: Smoothing::FEATURE_SIZE_DEFAULT,
        }
    }
}

/// A feature size from 0 to the largest size. A value that is not finite
/// gives the default size.
pub(crate) fn clamp_feature_size(points: f32) -> f32 {
    if points.is_finite() {
        points.clamp(0.0, Smoothing::FEATURE_SIZE_MAX)
    } else {
        Smoothing::FEATURE_SIZE_DEFAULT
    }
}

/// A stop at the edge of two bands of a raster.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub value: f64,
    /// The line where the values cross the stop.
    pub line: StopLine,
}

/// A line at a color stop. It keeps its weight and color while it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopLine {
    pub on: bool,
    /// The weight, in points.
    pub weight: f32,
    pub color: Rgb,
    /// The line has the color of the band above the stop, darker by
    /// `darken`, in place of `color`.
    pub darker: bool,
    /// The part of the light that the darker color takes away, from 0 to 1.
    pub darken: f32,
}

impl Default for StopLine {
    fn default() -> Self {
        StopLine {
            on: false,
            weight: 1.0,
            color: [40, 40, 40],
            darker: false,
            darken: 0.5,
        }
    }
}

/// An opacity from 0 to 1. A value that is not a number gives 1.
pub(crate) fn clamp_opacity(opacity: f32) -> f32 {
    if opacity.is_nan() {
        1.0
    } else {
        opacity.clamp(0.0, 1.0)
    }
}

/// How the globe colors the values of a raster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RasterDisplay {
    /// A color for each band between two band limits.
    #[default]
    Bands,
    /// A smooth ramp of colors.
    Gradient,
    /// Black at the lowest value and white at the highest.
    Grey,
}

impl RasterDisplay {
    pub const ALL: [RasterDisplay; 3] = [
        RasterDisplay::Bands,
        RasterDisplay::Gradient,
        RasterDisplay::Grey,
    ];

    /// The name of the display in the project file.
    pub fn id(self) -> &'static str {
        match self {
            RasterDisplay::Bands => "bands",
            RasterDisplay::Gradient => "gradient",
            RasterDisplay::Grey => "grey",
        }
    }

    pub fn from_id(id: &str) -> Option<RasterDisplay> {
        RasterDisplay::ALL.into_iter().find(|d| d.id() == id)
    }
}

/// How a raster layer or an image layer mixes its colors with the layers below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl BlendMode {
    pub const ALL: [BlendMode; 12] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ];

    /// The name of the mode in the project file.
    pub fn id(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Multiply => "multiply",
            BlendMode::Screen => "screen",
            BlendMode::Overlay => "overlay",
            BlendMode::Darken => "darken",
            BlendMode::Lighten => "lighten",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::HardLight => "hard-light",
            BlendMode::SoftLight => "soft-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
        }
    }

    pub fn from_id(id: &str) -> Option<BlendMode> {
        BlendMode::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The place of the mode in `ALL`, for the shader.
    pub fn index(self) -> u32 {
        BlendMode::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }
}

/// The units in which the app shows a distance on the ground.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Units {
    /// Kilometers and meters.
    #[default]
    Metric,
    /// Miles and feet.
    Imperial,
}

impl Units {
    pub const ALL: [Units; 2] = [Units::Metric, Units::Imperial];

    /// The name of the units in the project file.
    pub fn id(self) -> &'static str {
        match self {
            Units::Metric => "metric",
            Units::Imperial => "imperial",
        }
    }

    pub fn from_id(id: &str) -> Option<Units> {
        Units::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// The world: a sphere with a name, a radius, display units, and a seed. The
/// radius is in kilometers for each choice of units.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub name: String,
    pub radius_km: f64,
    pub units: Units,
    /// The seed of the noise of the world.
    pub seed: u32,
    /// The compass direction that the wind comes from, in degrees. 0 is
    /// north, and 270 is west.
    pub wind_deg: f64,
    /// How much rain the relief stops on the side away from the wind, from 0
    /// to 1.
    pub rain_shadow: f64,
}

impl Default for World {
    fn default() -> Self {
        World {
            name: "New world".to_string(),
            radius_km: 6371.0,
            units: Units::Metric,
            seed: 0,
            wind_deg: 270.0,
            rain_shadow: 0.0,
        }
    }
}

/// A world, its layers, and its atlas. Each raster has one layer, and each
/// raster layer has one raster. The same is true of the images.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub world: World,
    layers: Vec<Layer>,
    next_id: u32,
    /// In ID order.
    rasters: Vec<Raster>,
    next_raster_id: u32,
    /// In ID order.
    images: Vec<Image>,
    next_image_id: u32,
    pub atlas: Atlas,
    symbols: Vec<SymbolRecord>,
}

/// A point symbol that the user adds to the project, as the text of an SVG
/// file. Each symbol has a unique name.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolRecord {
    pub name: String,
    pub svg: String,
}

impl Project {
    /// A project with no layers.
    pub fn new(world: World) -> Self {
        Project {
            atlas: Atlas::new(world.units),
            world,
            layers: Vec::new(),
            next_id: 0,
            rasters: Vec::new(),
            next_raster_id: 0,
            symbols: Vec::new(),
            images: Vec::new(),
            next_image_id: 0,
        }
    }

    /// Adds a raster with no tiles, and its layer, and returns the ID of the
    /// raster. The layer goes directly above the top raster layer, or to the
    /// bottom of a project with no raster layer. `value_min` is the value of
    /// pixel 0, and `value_max` is the value of pixel 65,535. The layer is
    /// visible, and the raster is opaque and shows bands. Its base is zero,
    /// and it has no stops and no colors.
    pub fn add_raster(
        &mut self,
        name: String,
        face_size: u32,
        unit: String,
        value_min: f64,
        value_max: f64,
    ) -> RasterId {
        let id = RasterId(self.next_raster_id);
        self.next_raster_id += 1;
        self.rasters.push(Raster {
            id,
            opacity: 1.0,
            display: RasterDisplay::Bands,
            face_size,
            unit,
            value_min,
            value_max,
            base: 0,
            stops: Vec::new(),
            colors: Vec::new(),
            smoothing: Smoothing::default(),
            hillshade: Hillshade::default(),
            rivers: true,
            lines: true,
            blend: BlendMode::Normal,
        });
        let top = self.layers.iter().rposition(|l| l.raster().is_some());
        let layer = Layer {
            id: LayerId(self.next_id),
            name,
            kind: LayerKind::Raster(id),
            visible: true,
            source: None,
            fields: Vec::new(),
            features: Vec::new(),
            style: None,
        };
        self.next_id += 1;
        self.layers.insert(top.map_or(0, |i| i + 1), layer);
        id
    }

    /// The rasters in ID order.
    pub fn rasters(&self) -> &[Raster] {
        &self.rasters
    }

    /// The raster with this ID.
    pub fn raster(&self, id: RasterId) -> Option<&Raster> {
        self.rasters.iter().find(|r| r.id == id)
    }

    /// The raster with this ID. The ID must stay as it is. If the face size
    /// changes, the next save removes the tiles of the old size.
    pub fn raster_mut(&mut self, id: RasterId) -> Option<&mut Raster> {
        self.rasters.iter_mut().find(|r| r.id == id)
    }

    /// The layer of the raster with this ID.
    pub fn raster_layer(&self, id: RasterId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.raster() == Some(id))
    }

    /// Sets the opacity of a raster. A value outside 0 to 1 gets the nearest
    /// limit.
    pub fn set_raster_opacity(&mut self, id: RasterId, opacity: f32) -> bool {
        let raster = self.raster_mut(id);
        raster.map(|r| r.opacity = clamp_opacity(opacity)).is_some()
    }

    /// Sets how a raster mixes with the layers below it.
    pub fn set_raster_blend(&mut self, id: RasterId, blend: BlendMode) -> bool {
        self.raster_mut(id).map(|r| r.blend = blend).is_some()
    }

    /// Sets how the globe colors the values of a raster.
    pub fn set_raster_display(&mut self, id: RasterId, display: RasterDisplay) -> bool {
        self.raster_mut(id).map(|r| r.display = display).is_some()
    }

    /// Turns the rivers of a raster on or off.
    pub fn set_raster_rivers(&mut self, id: RasterId, on: bool) -> bool {
        self.raster_mut(id).map(|r| r.rivers = on).is_some()
    }

    /// Turns the lines of the stops of a raster on or off.
    pub fn set_raster_lines(&mut self, id: RasterId, on: bool) -> bool {
        self.raster_mut(id).map(|r| r.lines = on).is_some()
    }

    /// Turns the smoothing of a raster on or off.
    pub fn set_raster_smoothing(&mut self, id: RasterId, on: bool) -> bool {
        self.raster_mut(id).map(|r| r.smoothing.on = on).is_some()
    }

    /// Sets the minimum feature size of the smoothing of a raster, in
    /// points. A value outside 0 to the largest size gets the nearest limit.
    pub fn set_raster_feature_size(&mut self, id: RasterId, points: f32) -> bool {
        let size = clamp_feature_size(points);
        self.raster_mut(id)
            .map(|r| r.smoothing.feature_size = size)
            .is_some()
    }

    /// Turns the hillshade of a raster on or off.
    pub fn set_raster_hillshade(&mut self, id: RasterId, on: bool) -> bool {
        self.raster_mut(id).map(|r| r.hillshade.on = on).is_some()
    }

    /// Sets the opacity of the hillshade of a raster. A value outside 0 to 1
    /// gets the nearest limit.
    pub fn set_raster_hillshade_opacity(&mut self, id: RasterId, opacity: f32) -> bool {
        let opacity = clamp_opacity(opacity);
        self.raster_mut(id)
            .map(|r| r.hillshade.opacity = opacity)
            .is_some()
    }

    /// Turns the highlights of the hillshade of a raster on or off.
    pub fn set_raster_hillshade_highlights(&mut self, id: RasterId, on: bool) -> bool {
        self.raster_mut(id)
            .map(|r| r.hillshade.highlights = on)
            .is_some()
    }

    /// Sets the direction of the light of the hillshade of a raster. An
    /// azimuth outside 0 to 360 turns into the same direction.
    pub fn set_raster_hillshade_azimuth(&mut self, id: RasterId, azimuth: f32) -> bool {
        self.raster_mut(id)
            .map(|r| {
                r.hillshade = Hillshade {
                    azimuth,
                    ..r.hillshade
                }
                .clamped()
            })
            .is_some()
    }

    /// Sets the height of the light of the hillshade of a raster. A value
    /// outside the range gets the nearest limit.
    pub fn set_raster_hillshade_altitude(&mut self, id: RasterId, altitude: f32) -> bool {
        self.raster_mut(id)
            .map(|r| {
                r.hillshade = Hillshade {
                    altitude,
                    ..r.hillshade
                }
                .clamped()
            })
            .is_some()
    }

    /// Sets the exaggeration of the hillshade of a raster. A value outside the
    /// range gets the nearest limit.
    pub fn set_raster_hillshade_exaggeration(&mut self, id: RasterId, exaggeration: f32) -> bool {
        self.raster_mut(id)
            .map(|r| {
                r.hillshade = Hillshade {
                    exaggeration,
                    ..r.hillshade
                }
                .clamped()
            })
            .is_some()
    }

    /// Sets the shade color of the hillshade of a raster.
    pub fn set_raster_hillshade_color(&mut self, id: RasterId, color: Rgb) -> bool {
        self.raster_mut(id)
            .map(|r| r.hillshade.color = color)
            .is_some()
    }

    /// Sets the stops and the band colors of a raster.
    pub fn set_raster_stops(&mut self, id: RasterId, stops: Vec<Stop>, colors: Vec<Rgb>) -> bool {
        let raster = self.raster_mut(id);
        raster
            .map(|r| (r.stops, r.colors) = (stops, colors))
            .is_some()
    }

    /// Sets the unit of the values of a raster.
    pub fn set_raster_unit(&mut self, id: RasterId, unit: &str) -> bool {
        let raster = self.raster_mut(id);
        raster.map(|r| r.unit = unit.to_string()).is_some()
    }

    /// Removes a raster and its layer. Its ID is not used again. The next
    /// save removes its tiles from the file.
    pub fn remove_raster(&mut self, id: RasterId) -> Option<Raster> {
        let i = self.rasters.iter().position(|r| r.id == id)?;
        self.layers.retain(|l| l.raster() != Some(id));
        Some(self.rasters.remove(i))
    }

    /// Adds an image of `width` by `height` pixels with no tiles, and its
    /// layer on top, and returns the ID of the image. The layer is visible,
    /// and the image is opaque and blends as normal.
    pub fn add_image(&mut self, name: String, width: u32, height: u32) -> ImageId {
        let id = ImageId(self.next_image_id);
        self.next_image_id += 1;
        self.images.push(Image {
            id,
            width,
            height,
            levels: image_levels(width, height),
            opacity: 1.0,
            blend: BlendMode::Normal,
        });
        self.layers.push(Layer {
            id: LayerId(self.next_id),
            name,
            kind: LayerKind::Image(id),
            visible: true,
            source: None,
            fields: Vec::new(),
            features: Vec::new(),
            style: None,
        });
        self.next_id += 1;
        id
    }

    /// The ID that the next `add_image` gives.
    pub fn next_image_id(&self) -> ImageId {
        ImageId(self.next_image_id)
    }

    /// The images in ID order.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// The image with this ID.
    pub fn image(&self, id: ImageId) -> Option<&Image> {
        self.images.iter().find(|i| i.id == id)
    }

    /// The image with this ID. The ID and the size must stay as they are.
    pub fn image_mut(&mut self, id: ImageId) -> Option<&mut Image> {
        self.images.iter_mut().find(|i| i.id == id)
    }

    /// The layer of the image with this ID.
    pub fn image_layer(&self, id: ImageId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.image() == Some(id))
    }

    /// Sets the opacity of an image. A value outside 0 to 1 gets the nearest
    /// limit, and NaN gives 1.
    pub fn set_image_opacity(&mut self, id: ImageId, opacity: f32) -> bool {
        let image = self.image_mut(id);
        image.map(|i| i.opacity = clamp_opacity(opacity)).is_some()
    }

    /// Sets how an image mixes with the layers below it.
    pub fn set_image_blend(&mut self, id: ImageId, blend: BlendMode) -> bool {
        self.image_mut(id).map(|i| i.blend = blend).is_some()
    }

    /// Removes an image and its layer. Its ID is not used again. The file
    /// removes its tiles when no undo step holds the image.
    pub fn remove_image(&mut self, id: ImageId) -> Option<Image> {
        let i = self.images.iter().position(|x| x.id == id)?;
        self.layers.retain(|l| l.image() != Some(id));
        Some(self.images.remove(i))
    }

    /// Adds a layer on top and returns its ID. Each attribute name of the
    /// features is a field, in the order of the names. All the values of a
    /// field give its type. A field with values of more than one type is
    /// text. Each feature gets each field, with `Null` for a field that it
    /// did not have.
    pub fn add_layer(
        &mut self,
        name: String,
        kind: GeometryKind,
        source: Option<String>,
        mut features: Vec<Feature>,
    ) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        let fields = infer_fields(&mut features);
        self.layers.push(Layer {
            id,
            name,
            kind: LayerKind::Vector(kind),
            visible: true,
            source,
            fields,
            features,
            style: None,
        });
        id
    }

    /// The layer with this ID.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// The layers in draw order. The first layer is at the bottom.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// The symbols that the user added, in the order of addition.
    pub fn symbols(&self) -> &[SymbolRecord] {
        &self.symbols
    }

    /// Adds a symbol and returns its name. The name is trimmed, and an empty
    /// name becomes "Symbol". If a symbol of the project or a name in
    /// `reserved` has the name, the symbol gets the first free name of the
    /// form "oak 2", "oak 3", and so on. A symbol is not an edit, so it gets
    /// no undo step.
    pub fn add_symbol(&mut self, name: &str, svg: String, reserved: &[&str]) -> String {
        let base = match name.trim() {
            "" => "Symbol",
            name => name,
        };
        let taken = |n: &str| reserved.contains(&n) || self.symbols.iter().any(|s| s.name == n);
        let mut name = base.to_string();
        let mut n = 2;
        while taken(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        self.symbols.push(SymbolRecord {
            name: name.clone(),
            svg,
        });
        name
    }

    /// Moves a layer to this index of the draw order. An index past the end
    /// is the top.
    pub fn move_layer(&mut self, id: LayerId, to: usize) -> bool {
        let Some(from) = self.layers.iter().position(|l| l.id == id) else {
            return false;
        };
        let layer = self.layers.remove(from);
        let to = to.min(self.layers.len());
        self.layers.insert(to, layer);
        true
    }

    /// Gives a layer a new name.
    pub fn rename_layer(&mut self, id: LayerId, name: String) -> bool {
        self.layer_mut(id).map(|l| l.name = name).is_some()
    }

    /// Sets the style JSON of a layer.
    pub fn set_layer_style(&mut self, id: LayerId, style: Option<String>) -> bool {
        self.layer_mut(id).map(|l| l.style = style).is_some()
    }

    /// Shows or hides a layer.
    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) -> bool {
        self.layer_mut(id).map(|l| l.visible = visible).is_some()
    }

    /// Adds a feature to a vector layer of the same geometry kind. The
    /// feature gets the default value of each field that it does not have.
    /// Each value turns into a value of the type of its field. An attribute
    /// that is not a field of the layer is ignored.
    pub fn add_feature(&mut self, id: LayerId, mut feature: Feature) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        if layer.geometry() != Some(feature.geometry.kind()) {
            return false;
        }
        let mut given = std::mem::take(&mut feature.attributes);
        for field in &layer.fields {
            let value = match given.remove(&field.name) {
                Some(value) => field.kind.convert(&value),
                None => field.default.clone(),
            };
            feature.attributes.insert(field.name.clone(), value);
        }
        layer.features.push(feature);
        true
    }

    /// Adds a field at the end of the schema of a vector layer. Each feature
    /// gets `Null` in the field. The default value turns into a value of the
    /// type of the field. The name must be free.
    pub fn add_field(&mut self, id: LayerId, mut field: Field) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        if layer.geometry().is_none() || !layer.name_free(&field.name, None) {
            return false;
        }
        field.default = field.kind.convert(&field.default);
        for f in &mut layer.features {
            f.attributes.insert(field.name.clone(), Value::Null);
        }
        layer.fields.push(field);
        true
    }

    /// Gives a field a new name. Each feature keeps its value. The new name
    /// must be free.
    pub fn rename_field(&mut self, id: LayerId, old: &str, new: &str) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        let Some(i) = layer.field_index(old) else {
            return false;
        };
        if old == new || !layer.name_free(new, Some(old)) {
            return false;
        }
        layer.fields[i].name = new.to_string();
        for f in &mut layer.features {
            let value = f.attributes.remove(old).unwrap_or(Value::Null);
            f.attributes.insert(new.to_string(), value);
        }
        true
    }

    /// Removes a field and its values.
    pub fn remove_field(&mut self, id: LayerId, name: &str) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        let Some(i) = layer.field_index(name) else {
            return false;
        };
        layer.fields.remove(i);
        for f in &mut layer.features {
            f.attributes.remove(name);
        }
        true
    }

    /// Gives a field a new type. Each value and the default value turn into
    /// values of the new type, as `FieldType::convert` gives them.
    pub fn set_field_type(&mut self, id: LayerId, name: &str, kind: FieldType) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        let Some(i) = layer.field_index(name) else {
            return false;
        };
        let field = &mut layer.fields[i];
        if field.kind == kind {
            return false;
        }
        field.kind = kind;
        field.default = kind.convert(&field.default);
        for f in &mut layer.features {
            if let Some(value) = f.attributes.get_mut(name) {
                *value = kind.convert(value);
            }
        }
        true
    }

    /// Sets the default value of a field. The value turns into a value of
    /// the type of the field.
    pub fn set_field_default(&mut self, id: LayerId, name: &str, value: Value) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        let Some(i) = layer.field_index(name) else {
            return false;
        };
        let field = &mut layer.fields[i];
        let value = field.kind.convert(&value);
        if field.default == value {
            return false;
        }
        field.default = value;
        true
    }

    /// Gives the feature with this UUID a new geometry of the same kind. The
    /// feature keeps its UUID and its attributes.
    pub fn set_geometry(&mut self, id: LayerId, uuid: Uuid, geometry: Geometry) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        if layer.geometry() != Some(geometry.kind()) {
            return false;
        }
        let feature = layer.features.iter_mut().find(|f| f.uuid == uuid);
        feature.map(|f| f.geometry = geometry).is_some()
    }

    /// Sets attribute values of the feature with this UUID. Each value turns
    /// into a value of the type of its field. A name that is not a field of
    /// the layer is ignored.
    pub fn set_attributes(
        &mut self,
        id: LayerId,
        uuid: Uuid,
        values: impl IntoIterator<Item = (String, Value)>,
    ) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        let Some(feature) = layer.features.iter_mut().find(|f| f.uuid == uuid) else {
            return false;
        };
        for (name, value) in values {
            if let Some(field) = layer.fields.iter().find(|f| f.name == name) {
                feature.attributes.insert(name, field.kind.convert(&value));
            }
        }
        true
    }

    fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Removes a layer. Its ID is not used again. A raster layer takes its
    /// raster, and the next save removes the tiles of that raster from the
    /// file. An image layer takes its image, and the file removes the tiles
    /// of the image when no undo step holds the image.
    pub fn remove_layer(&mut self, id: LayerId) -> Option<Layer> {
        let i = self.layers.iter().position(|l| l.id == id)?;
        let layer = self.layers.remove(i);
        for map in &mut self.atlas.maps {
            map.hidden.remove(&id);
            map.overrides.remove(&id);
            map.labels.remove(&id);
        }
        if let Some(raster) = layer.raster() {
            self.rasters.retain(|r| r.id != raster);
        }
        if let Some(image) = layer.image() {
            self.images.retain(|i| i.id != image);
        }
        Some(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(keys: &[&str]) -> Feature {
        Feature::new(
            Geometry::Points(vec![[0.0, 0.0]]),
            keys.iter().map(|k| (k.to_string(), Value::Null)).collect(),
        )
    }

    #[test]
    fn value_labels() {
        assert_eq!(Value::Null.label(), None);
        assert_eq!(Value::Bool(true).label(), Some("true".into()));
        assert_eq!(Value::Number(832.0).label(), Some("832".into()));
        assert_eq!(Value::Number(1.5).label(), Some("1.5".into()));
        assert_eq!(Value::Text("  ".into()).label(), None);
        assert_eq!(Value::Text("".into()).label(), None);
        assert_eq!(Value::Text("Oslo".into()).label(), Some("Oslo".into()));
        assert_eq!(Value::Number(2.0).as_f64(), Some(2.0));
        assert_eq!(Value::Text("2".into()).as_f64(), None);
    }

    #[test]
    fn ids_and_fields() {
        let mut p = Project::new(World::default());
        let a = p.add_layer(
            "a".into(),
            GeometryKind::Point,
            None,
            vec![feature(&["zeta", "alpha"]), feature(&["mid", "alpha"])],
        );
        let b = p.add_layer("b".into(), GeometryKind::Point, None, vec![]);
        assert!(b > a);
        assert_eq!(p.layer(a).unwrap().field_names(), ["alpha", "mid", "zeta"]);
        assert_eq!(p.layers().len(), 2);
        let features = &p.layer(a).unwrap().features;
        assert!(features.iter().all(|f| f.attributes.len() == 3));
        assert_ne!(features[0].uuid(), features[1].uuid());
    }

    #[test]
    fn added_features_and_new_geometry() {
        let mut p = Project::new(World::default());
        let a = p.add_layer(
            "a".into(),
            GeometryKind::Point,
            None,
            vec![feature(&["mid"])],
        );
        assert!(p.add_feature(a, feature(&[])));
        // An attribute that is not a field is ignored.
        assert!(p.add_feature(a, feature(&["alpha"])));
        let line = Feature::new(Geometry::Lines(vec![]), BTreeMap::new());
        assert!(!p.add_feature(a, line));
        assert!(!p.add_feature(LayerId(99), feature(&[])));
        let layer = p.layer(a).unwrap();
        assert_eq!(layer.field_names(), ["mid"]);
        assert_eq!(layer.features.len(), 3);
        assert!(layer.features.iter().all(|f| f.attributes.len() == 1));

        let uuid = layer.features[1].uuid();
        let moved = Geometry::Points(vec![[5.0, 6.0]]);
        assert!(p.set_geometry(a, uuid, moved.clone()));
        assert!(!p.set_geometry(a, uuid, Geometry::Lines(vec![])));
        assert!(!p.set_geometry(a, Uuid::new_v4(), moved.clone()));
        let f = &p.layer(a).unwrap().features[1];
        assert_eq!((f.uuid(), &f.geometry), (uuid, &moved));

        let values = [
            ("mid".to_string(), Value::Number(2.0)),
            ("beta".to_string(), Value::Number(1.0)),
        ];
        assert!(p.set_attributes(a, uuid, values.clone()));
        assert!(!p.set_attributes(a, Uuid::new_v4(), values));
        let f = &p.layer(a).unwrap().features[1];
        // The value turns into a value of the type of the field.
        assert_eq!(f.attributes["mid"], Value::Text("2".into()));
        assert!(!f.attributes.contains_key("beta"));
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    #[test]
    fn text_parses_as_a_value_of_each_type() {
        use FieldType::*;
        assert_eq!(Text.parse(" a b "), Some(text(" a b ")));
        assert_eq!(Integer.parse(" 42 "), Some(Value::Number(42.0)));
        assert_eq!(Integer.parse("-7"), Some(Value::Number(-7.0)));
        assert_eq!(
            Integer.parse("9007199254740992"),
            Some(Value::Number(WHOLE_MAX))
        );
        for bad in ["2.5", "9007199254740994", "inf", "NaN", "x", ""] {
            assert_eq!(Integer.parse(bad), None, "{bad}");
        }
        assert_eq!(Real.parse("2.5"), Some(Value::Number(2.5)));
        assert_eq!(Real.parse("-inf"), Some(Value::Number(f64::NEG_INFINITY)));
        assert_eq!(Real.parse("NaN"), None);
        assert_eq!(Real.parse("x"), None);
        assert_eq!(Bool.parse("TRUE"), Some(Value::Bool(true)));
        assert_eq!(Bool.parse(" False"), Some(Value::Bool(false)));
        assert_eq!(Bool.parse("yes"), None);
        assert_eq!(Bool.parse("1"), None);
        assert_eq!(FieldType::ALL.map(FieldType::label)[1], "Whole number");
        for kind in FieldType::ALL {
            assert_eq!(FieldType::from_id(kind.id()), Some(kind));
        }
    }

    #[test]
    fn a_value_converts_to_each_type() {
        use FieldType::*;
        let n = Value::Number;
        for kind in FieldType::ALL {
            assert_eq!(kind.convert(&Value::Null), Value::Null);
        }
        assert_eq!(Text.convert(&Value::Bool(true)), text("true"));
        assert_eq!(Text.convert(&n(3.0)), text("3"));
        assert_eq!(Text.convert(&n(2.5)), text("2.5"));
        assert_eq!(Text.convert(&text("")), text(""));
        assert_eq!(Integer.convert(&Value::Bool(true)), n(1.0));
        assert_eq!(Integer.convert(&n(2.5)), n(3.0));
        assert_eq!(Integer.convert(&n(-2.4)), n(-2.0));
        assert_eq!(Integer.convert(&n(f64::INFINITY)), Value::Null);
        assert_eq!(Integer.convert(&text(" 2.4 ")), n(2.0));
        assert_eq!(Integer.convert(&text("x")), Value::Null);
        assert_eq!(Real.convert(&Value::Bool(false)), n(0.0));
        assert_eq!(Real.convert(&text("1.25")), n(1.25));
        assert_eq!(Real.convert(&n(f64::NAN)), Value::Null);
        assert_eq!(Real.convert(&text("")), Value::Null);
        assert_eq!(Bool.convert(&n(0.0)), Value::Bool(false));
        assert_eq!(Bool.convert(&n(-3.0)), Value::Bool(true));
        assert_eq!(Bool.convert(&text("true")), Value::Bool(true));
        assert_eq!(Bool.convert(&text("maybe")), Value::Null);
    }

    #[test]
    fn an_import_gets_a_type_for_each_field() {
        let mut p = Project::new(World::default());
        let point = |pairs: &[(&str, Value)]| {
            let attributes = pairs.iter().map(|(k, v)| (k.to_string(), v.clone()));
            Feature::new(Geometry::Points(vec![]), attributes.collect())
        };
        let a = p.add_layer(
            "a".into(),
            GeometryKind::Point,
            None,
            vec![
                point(&[
                    ("flag", Value::Bool(true)),
                    ("pop", Value::Number(3.0)),
                    ("any", Value::Number(1.5)),
                    ("none", Value::Null),
                ]),
                point(&[
                    ("flag", Value::Null),
                    ("pop", Value::Number(0.5)),
                    ("any", Value::Bool(false)),
                    ("name", text("Oslo")),
                ]),
            ],
        );
        let layer = p.layer(a).unwrap();
        let kinds: Vec<(&str, FieldType)> = layer
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("any", FieldType::Text),
                ("flag", FieldType::Bool),
                ("name", FieldType::Text),
                ("none", FieldType::Text),
                ("pop", FieldType::Real),
            ]
        );
        assert!(layer.fields.iter().all(|f| f.default == Value::Null));
        let any: Vec<&Value> = layer
            .features
            .iter()
            .map(|f| &f.attributes["any"])
            .collect();
        assert_eq!(any, [&text("1.5"), &text("false")]);
        assert_eq!(layer.features[0].attributes["name"], Value::Null);
    }

    #[test]
    fn the_schema_of_a_layer_changes() {
        let mut p = Project::new(World::default());
        let a = p.add_layer("a".into(), GeometryKind::Point, None, vec![]);
        let point = || Feature::new(Geometry::Points(vec![]), BTreeMap::new());
        assert!(p.add_feature(a, point()));
        let pop = Field {
            default: text("12.6"),
            ..Field::new("pop", FieldType::Integer)
        };
        assert!(p.add_field(a, Field::new("name", FieldType::Text)));
        assert!(p.add_field(a, pop));
        // A name that is not free is refused.
        for name in ["NAME", " ", "", "FID", "geom", "Uuid"] {
            assert!(!p.add_field(a, Field::new(name, FieldType::Text)), "{name}");
        }
        let layer = p.layer(a).unwrap();
        assert_eq!(layer.field_names(), ["name", "pop"]);
        assert_eq!(layer.field("pop").unwrap().default, Value::Number(13.0));
        assert_eq!(layer.field_index("pop"), Some(1));
        assert_eq!(layer.features[0].attributes["pop"], Value::Null);
        assert!(layer.name_free("Pop", Some("pop")));
        assert!(!layer.name_free("Pop", Some("name")));

        // A new feature gets the default value of each field it does not have.
        let mut given = point();
        given.attributes.insert("name".into(), Value::Bool(true));
        assert!(p.add_feature(a, given));
        let f = &p.layer(a).unwrap().features[1];
        assert_eq!(f.attributes["name"], text("true"));
        assert_eq!(f.attributes["pop"], Value::Number(13.0));

        assert!(p.rename_field(a, "pop", "population"));
        assert!(!p.rename_field(a, "pop", "x"));
        assert!(!p.rename_field(a, "name", "Population"));
        assert!(!p.rename_field(a, "name", "name"));
        assert!(p.rename_field(a, "name", "Name"));
        let layer = p.layer(a).unwrap();
        assert_eq!(layer.field_names(), ["Name", "population"]);
        assert_eq!(
            layer.features[1].attributes.keys().collect::<Vec<_>>(),
            ["Name", "population"]
        );

        assert!(p.set_field_type(a, "population", FieldType::Bool));
        assert!(!p.set_field_type(a, "population", FieldType::Bool));
        assert!(!p.set_field_type(a, "pop", FieldType::Text));
        let layer = p.layer(a).unwrap();
        assert_eq!(
            layer.field("population").unwrap().default,
            Value::Bool(true)
        );
        assert_eq!(
            layer.features[1].attributes["population"],
            Value::Bool(true)
        );
        assert_eq!(layer.features[0].attributes["population"], Value::Null);

        assert!(p.set_field_default(a, "Name", Value::Number(5.0)));
        assert!(!p.set_field_default(a, "Name", text("5")));
        assert!(!p.set_field_default(a, "x", Value::Null));
        assert_eq!(
            p.layer(a).unwrap().field("Name").unwrap().default,
            text("5")
        );

        assert!(p.remove_field(a, "Name"));
        assert!(!p.remove_field(a, "Name"));
        let layer = p.layer(a).unwrap();
        assert_eq!(layer.field_names(), ["population"]);
        assert!(layer.features.iter().all(|f| f.attributes.len() == 1));

        // A raster layer has no fields.
        let rain = p.add_raster("Rain".into(), 1000, "mm".into(), 0.0, 1.0);
        let rain = p.raster_layer(rain).unwrap().id;
        assert!(!p.add_field(rain, Field::new("x", FieldType::Text)));
        assert!(!p.add_field(LayerId(99), Field::new("x", FieldType::Text)));
    }

    #[test]
    fn removed_ids_are_not_reused() {
        let mut p = Project::new(World::default());
        let a = p.add_layer("a".into(), GeometryKind::Line, None, vec![]);
        assert!(p.remove_layer(a).is_some());
        assert!(p.layer(a).is_none());
        assert!(p.remove_layer(a).is_none());
        let b = p.add_layer("b".into(), GeometryKind::Line, None, vec![]);
        assert_ne!(a, b);
    }

    #[test]
    fn move_rename_and_hide() {
        let mut p = Project::new(World::default());
        let ids: Vec<LayerId> = ["a", "b", "c"]
            .iter()
            .map(|n| p.add_layer(n.to_string(), GeometryKind::Line, None, vec![]))
            .collect();
        assert!(p.move_layer(ids[0], 2));
        assert_eq!(names(&p), ["b", "c", "a"]);
        assert!(p.move_layer(ids[0], 0));
        assert_eq!(names(&p), ["a", "b", "c"]);
        assert!(p.move_layer(ids[1], 99));
        assert_eq!(names(&p), ["a", "c", "b"]);
        assert!(p.rename_layer(ids[2], "Coast".into()));
        assert_eq!(names(&p), ["a", "Coast", "b"]);
        assert!(p.layer(ids[0]).unwrap().visible);
        assert!(p.set_layer_visible(ids[0], false));
        assert!(!p.layer(ids[0]).unwrap().visible);
        assert!(p.remove_layer(ids[0]).is_some());
        assert!(!p.move_layer(ids[0], 0));
        assert!(!p.rename_layer(ids[0], "x".into()));
        assert!(!p.set_layer_visible(ids[0], true));
    }

    #[test]
    fn a_raster_gets_its_values_and_its_layer() {
        let mut p = Project::new(World::default());
        let ids: Vec<RasterId> = ["a", "b", "c"]
            .iter()
            .map(|n| p.add_raster(n.to_string(), 4096, "mm".into(), 0.0, 5000.0))
            .collect();
        let a = p.raster(ids[0]).unwrap();
        assert_eq!((a.face_size, a.unit.as_str()), (4096, "mm"));
        assert_eq!((a.value_min, a.value_max, a.opacity), (0.0, 5000.0, 1.0));
        let layer = p.raster_layer(ids[0]).unwrap();
        assert_eq!((layer.name.as_str(), layer.visible), ("a", true));
        assert_eq!(layer.kind, LayerKind::Raster(ids[0]));
        assert_eq!((layer.raster(), layer.geometry()), (Some(ids[0]), None));
        assert_eq!(layer.kind.name(), "raster");
        assert!(layer.source.is_none() && layer.fields.is_empty() && layer.features.is_empty());

        assert!(p.set_raster_opacity(ids[1], 0.25));
        assert_eq!(p.raster(ids[1]).unwrap().opacity, 0.25);
        for (given, kept) in [(7.0, 1.0), (-1.0, 0.0), (f32::NAN, 1.0)] {
            assert!(p.set_raster_opacity(ids[1], given));
            assert_eq!(p.raster(ids[1]).unwrap().opacity, kept);
        }
        assert_eq!(p.raster(ids[1]).unwrap().display, RasterDisplay::Bands);
        assert!(p.set_raster_display(ids[1], RasterDisplay::Gradient));
        assert_eq!(p.raster(ids[1]).unwrap().display, RasterDisplay::Gradient);
        assert!(p.set_raster_unit(ids[1], "cm"));
        assert_eq!(p.raster(ids[1]).unwrap().unit, "cm");
        assert_eq!(p.raster(ids[1]).unwrap().smoothing, Smoothing::default());
        assert!(p.set_raster_smoothing(ids[1], false));
        assert!(!p.raster(ids[1]).unwrap().smoothing.on);
        let max = Smoothing::FEATURE_SIZE_MAX;
        for (given, kept) in [
            (3.5, 3.5),
            (0.0, 0.0),
            (20.0, max),
            (-1.0, 0.0),
            (f32::NAN, 2.0),
            (f32::INFINITY, 2.0),
        ] {
            assert!(p.set_raster_feature_size(ids[1], given));
            assert_eq!(p.raster(ids[1]).unwrap().smoothing.feature_size, kept);
        }
        assert_eq!(p.raster(ids[1]).unwrap().hillshade, Hillshade::default());
        assert!(p.set_raster_hillshade(ids[1], true));
        assert!(p.raster(ids[1]).unwrap().hillshade.on);
        for (given, kept) in [(0.25, 0.25), (7.0, 1.0), (-1.0, 0.0), (f32::NAN, 1.0)] {
            assert!(p.set_raster_hillshade_opacity(ids[1], given));
            assert_eq!(p.raster(ids[1]).unwrap().hillshade.opacity, kept);
        }
        assert!(p.raster(ids[1]).unwrap().hillshade.on);
        assert!(p.raster(ids[1]).unwrap().hillshade.highlights);
        assert!(p.set_raster_hillshade_highlights(ids[1], false));
        assert!(!p.raster(ids[1]).unwrap().hillshade.highlights);
        let shade = |p: &Project| p.raster(ids[1]).unwrap().hillshade;
        for (given, kept) in [
            (-45.0, 315.0),
            (720.0, 0.0),
            (f32::NAN, 315.0),
            (90.0, 90.0),
        ] {
            assert!(p.set_raster_hillshade_azimuth(ids[1], given));
            assert_eq!(shade(&p).azimuth, kept);
        }
        for (given, kept) in [(0.0, 5.0), (90.0, 85.0), (30.0, 30.0)] {
            assert!(p.set_raster_hillshade_altitude(ids[1], given));
            assert_eq!(shade(&p).altitude, kept);
        }
        for (given, kept) in [(0.0, 0.25), (10.0, 4.0), (f32::NAN, 1.0), (2.0, 2.0)] {
            assert!(p.set_raster_hillshade_exaggeration(ids[1], given));
            assert_eq!(shade(&p).exaggeration, kept);
        }
        assert!(p.set_raster_hillshade_color(ids[1], [10, 20, 30]));
        assert_eq!(shade(&p).color, [10, 20, 30]);

        assert!(p.remove_raster(ids[0]).is_some());
        assert!(p.remove_raster(ids[0]).is_none());
        assert!(p.raster_layer(ids[0]).is_none());
        assert!(!p.set_raster_opacity(ids[0], 0.5));
        assert!(!p.set_raster_display(ids[0], RasterDisplay::Grey));
        assert!(!p.set_raster_unit(ids[0], "m"));
        assert!(!p.set_raster_smoothing(ids[0], true));
        assert!(!p.set_raster_feature_size(ids[0], 1.0));
        assert!(!p.set_raster_hillshade(ids[0], true));
        assert!(!p.set_raster_hillshade_opacity(ids[0], 0.5));
        assert!(!p.set_raster_hillshade_highlights(ids[0], false));
        assert!(!p.set_raster_hillshade_azimuth(ids[0], 90.0));
        assert!(!p.set_raster_hillshade_altitude(ids[0], 30.0));
        assert!(!p.set_raster_hillshade_exaggeration(ids[0], 2.0));
        assert!(!p.set_raster_hillshade_color(ids[0], [1, 2, 3]));
        let d = p.add_raster("d".into(), 1024, String::new(), 0.0, 1.0);
        assert!(ids.iter().all(|id| d > *id));
        let rasters: Vec<RasterId> = p.rasters().iter().map(|r| r.id).collect();
        assert_eq!(rasters, [ids[1], ids[2], d]);
    }

    #[test]
    fn an_image_gets_its_layer_on_top() {
        let mut p = Project::new(World::default());
        let towns = p.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        let a = p.add_image("Plates".into(), 1000, 501);
        let b = p.add_image("Coasts".into(), 200, 100);
        assert!(b > a);
        let order: Vec<LayerKind> = p.layers().iter().map(|l| l.kind).collect();
        let towns = p.layer(towns).unwrap().kind;
        assert_eq!(order, [towns, LayerKind::Image(a), LayerKind::Image(b)]);
        let layer = p.image_layer(a).unwrap();
        assert_eq!((layer.name.as_str(), layer.visible), ("Plates", true));
        assert_eq!(
            (layer.image(), layer.raster(), layer.geometry()),
            (Some(a), None, None)
        );
        assert_eq!(layer.kind.name(), "image");
        let image = p.image(a).unwrap();
        assert_eq!((image.width, image.height, image.levels), (1000, 501, 3));
        assert_eq!((image.opacity, image.blend), (1.0, BlendMode::Normal));

        assert!(p.set_image_opacity(a, 0.25));
        assert!(p.set_image_blend(a, BlendMode::Multiply));
        assert_eq!(p.image(a).unwrap().opacity, 0.25);
        assert_eq!(p.image(a).unwrap().blend, BlendMode::Multiply);
        assert!(p.set_image_opacity(a, 3.0));
        assert_eq!(p.image(a).unwrap().opacity, 1.0);

        let id = p.image_layer(b).unwrap().id;
        assert_eq!(p.remove_layer(id).unwrap().image(), Some(b));
        assert!(p.image(b).is_none());
        assert!(p.remove_image(a).is_some());
        assert!(p.image_layer(a).is_none() && p.remove_image(a).is_none());
        assert!(!p.set_image_opacity(a, 0.5) && !p.set_image_blend(a, BlendMode::Screen));
        assert!(p.add_image("c".into(), 10, 10) > b);
    }

    fn names(p: &Project) -> Vec<&str> {
        p.layers().iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn a_raster_layer_is_a_layer_in_the_one_order() {
        let mut p = Project::new(World::default());
        let roads = p.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        let height = p.add_raster("Height".into(), 4096, "m".into(), -6000.0, 6000.0);
        assert_eq!(names(&p), ["Height", "Roads"]);
        let towns = p.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
        let rain = p.add_raster("Rain".into(), 1024, "mm".into(), 0.0, 5000.0);
        assert_eq!(names(&p), ["Height", "Rain", "Roads", "Towns"]);
        let layer = |p: &Project, id| p.raster_layer(id).unwrap().id;
        let (height_layer, rain_layer) = (layer(&p, height), layer(&p, rain));
        let ids: BTreeSet<LayerId> = p.layers().iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(p.layer(rain_layer).unwrap().raster(), Some(rain));
        assert_eq!(p.layer(roads).unwrap().geometry(), Some(GeometryKind::Line));

        // A layer of each kind moves to each place.
        assert!(p.move_layer(rain_layer, 99));
        assert_eq!(names(&p), ["Height", "Roads", "Towns", "Rain"]);
        assert!(p.move_layer(towns, 0));
        assert_eq!(names(&p), ["Towns", "Height", "Roads", "Rain"]);
        // A new raster layer goes directly above the top raster layer.
        let wind = p.add_raster("Wind".into(), 1024, "m/s".into(), 0.0, 100.0);
        assert_eq!(names(&p), ["Towns", "Height", "Roads", "Rain", "Wind"]);
        assert!(p.move_layer(height_layer, 2));
        assert_eq!(names(&p), ["Towns", "Roads", "Height", "Rain", "Wind"]);
        // The order of the layers is not the order of the rasters.
        assert!(p.move_layer(layer(&p, wind), 0));
        let rasters: Vec<RasterId> = p.rasters().iter().map(|r| r.id).collect();
        assert_eq!(rasters, [height, rain, wind]);

        assert!(p.set_layer_visible(rain_layer, false));
        assert!(!p.raster_layer(rain).unwrap().visible);
        assert!(p.raster_layer(height).unwrap().visible);
        assert!(p.rename_layer(rain_layer, "Rainfall".into()));
        assert_eq!(p.raster_layer(rain).unwrap().name, "Rainfall");
    }

    #[test]
    fn a_removed_raster_layer_takes_its_raster() {
        let mut p = Project::new(World::default());
        let roads = p.add_layer("Roads".into(), GeometryKind::Line, None, vec![]);
        let height = p.add_raster("Height".into(), 4096, "m".into(), -6000.0, 6000.0);
        let rain = p.add_raster("Rain".into(), 1024, "mm".into(), 0.0, 5000.0);
        let rain_layer = p.raster_layer(rain).unwrap().id;

        let removed = p.remove_layer(rain_layer).unwrap();
        assert_eq!(removed.raster(), Some(rain));
        assert!(p.raster(rain).is_none() && p.raster_layer(rain).is_none());
        assert!(p.remove_raster(rain).is_none());
        assert_eq!(names(&p), ["Height", "Roads"]);

        let height_layer = p.raster_layer(height).unwrap().id;
        assert_eq!(p.remove_raster(height).unwrap().id, height);
        assert!(p.layer(height_layer).is_none());
        assert!(p.remove_layer(height_layer).is_none());
        assert_eq!(names(&p), ["Roads"]);
        assert_eq!(p.rasters(), []);

        // A removed vector layer takes no raster.
        let wind = p.add_raster("Wind".into(), 1024, "m/s".into(), 0.0, 100.0);
        assert!(p.remove_layer(roads).is_some());
        assert!(p.raster(wind).is_some());
        assert_eq!(names(&p), ["Wind"]);
    }

    #[test]
    fn kinds() {
        assert_eq!(Geometry::Lines(vec![]).kind().name(), "line");
        assert_eq!(Geometry::Polygons(vec![]).kind().name(), "polygon");
        assert_eq!(Geometry::Points(vec![]).kind().name(), "point");
        assert_eq!(World::default().radius_km, 6371.0);
    }

    #[test]
    fn add_symbol_makes_unique_names() {
        let mut project = Project::new(World::default());
        let svg = || "<svg/>".to_string();
        assert_eq!(project.add_symbol(" oak ", svg(), &[]), "oak");
        assert_eq!(project.add_symbol("oak", svg(), &[]), "oak 2");
        assert_eq!(project.add_symbol("oak", svg(), &[]), "oak 3");
        assert_eq!(project.add_symbol("  ", svg(), &[]), "Symbol");
        assert_eq!(project.add_symbol("", svg(), &[]), "Symbol 2");
        let reserved = ["Circle", "Circle 2"];
        assert_eq!(project.add_symbol("Circle", svg(), &reserved), "Circle 3");
        let names: Vec<&str> = project.symbols().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["oak", "oak 2", "oak 3", "Symbol", "Symbol 2", "Circle 3"]
        );
    }
}
