//! The project model and its GeoPackage file: a world, its layers, and their features.

use std::collections::{BTreeMap, BTreeSet};

pub use uuid::Uuid;

mod gpkg;
mod tile;
mod undo;

pub use gpkg::{
    FORMAT_VERSION, ProjectFile, Restored, SaveStats, StoreError, Summary, Thumbnail, TileChanges,
    TileKey, summary,
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
}

impl LayerKind {
    /// A lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            LayerKind::Vector(kind) => kind.name(),
            LayerKind::Raster(_) => "raster",
        }
    }
}

/// One row of the draw order: a set of features of one geometry kind, or a
/// raster. A raster layer has no source, no fields, and no features.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    /// The views draw the layer.
    pub visible: bool,
    /// The name of the imported file.
    pub source: Option<String>,
    /// Attribute names, sorted, no duplicates.
    pub fields: Vec<String>,
    /// A feature ID is its index here.
    pub features: Vec<Feature>,
}

impl Layer {
    /// The geometry kind of a vector layer.
    pub fn geometry(&self) -> Option<GeometryKind> {
        match self.kind {
            LayerKind::Vector(kind) => Some(kind),
            LayerKind::Raster(_) => None,
        }
    }

    /// The raster of a raster layer.
    pub fn raster(&self) -> Option<RasterId> {
        match self.kind {
            LayerKind::Vector(_) => None,
            LayerKind::Raster(id) => Some(id),
        }
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
    /// How much the raster hides the layers below it, from 0 to 1.
    pub opacity: f32,
    /// How the globe colors the values.
    pub display: RasterDisplay,
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
    /// The color stops of the ramp, in ascending order of value. An empty
    /// list gives the default bands of the range.
    pub stops: Vec<Stop>,
    /// How the globe smooths the values by the scale.
    pub smoothing: Smoothing,
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

/// A color stop of the ramp of a raster: a value, the color just below it,
/// and the color just above it. The two colors differ only at a hard stop,
/// for example at sea level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub value: f64,
    pub below: Rgb,
    pub above: Rgb,
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
    /// The line has the color of the stop, darker by `darken`, in place of
    /// `color`.
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
}

impl Default for World {
    fn default() -> Self {
        World {
            name: "New world".to_string(),
            radius_km: 6371.0,
            units: Units::Metric,
            seed: 0,
        }
    }
}

/// A world and its layers. Each raster has one layer, and each raster layer
/// has one raster.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub world: World,
    layers: Vec<Layer>,
    next_id: u32,
    /// In ID order.
    rasters: Vec<Raster>,
    next_raster_id: u32,
}

impl Project {
    /// A project with no layers.
    pub fn new(world: World) -> Self {
        Project {
            world,
            layers: Vec::new(),
            next_id: 0,
            rasters: Vec::new(),
            next_raster_id: 0,
        }
    }

    /// Adds a raster with no tiles, and its layer, and returns the ID of the
    /// raster. The layer goes directly above the top raster layer, or to the
    /// bottom of a project with no raster layer. `value_min` is the value of
    /// pixel 0, and `value_max` is the value of pixel 65,535. The layer is
    /// visible, and the raster is opaque and shows bands. Its base is zero,
    /// and its band limits, its ramp, and its stops are empty.
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
            smoothing: Smoothing::default(),
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

    /// Sets how the globe colors the values of a raster.
    pub fn set_raster_display(&mut self, id: RasterId, display: RasterDisplay) -> bool {
        self.raster_mut(id).map(|r| r.display = display).is_some()
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

    /// Sets the color stops of a raster.
    pub fn set_raster_stops(&mut self, id: RasterId, stops: Vec<Stop>) -> bool {
        self.raster_mut(id).map(|r| r.stops = stops).is_some()
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

    /// Adds a layer on top and returns its ID. Each feature gets each field
    /// of the layer, with `Null` for a field that it did not have.
    pub fn add_layer(
        &mut self,
        name: String,
        kind: GeometryKind,
        source: Option<String>,
        mut features: Vec<Feature>,
    ) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        let fields: BTreeSet<&String> = features.iter().flat_map(|f| f.attributes.keys()).collect();
        let fields: Vec<String> = fields.into_iter().cloned().collect();
        for f in &mut features {
            for name in &fields {
                if !f.attributes.contains_key(name) {
                    f.attributes.insert(name.clone(), Value::Null);
                }
            }
        }
        self.layers.push(Layer {
            id,
            name,
            kind: LayerKind::Vector(kind),
            visible: true,
            source,
            fields,
            features,
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

    /// Shows or hides a layer.
    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) -> bool {
        self.layer_mut(id).map(|l| l.visible = visible).is_some()
    }

    /// Adds a feature to a vector layer of the same geometry kind. The
    /// feature gets each field of the layer, with `Null` for a field that it
    /// did not have. A field that only the feature has becomes a field of the
    /// layer.
    pub fn add_feature(&mut self, id: LayerId, mut feature: Feature) -> bool {
        let Some(layer) = self.layer_mut(id) else {
            return false;
        };
        if layer.geometry() != Some(feature.geometry.kind()) {
            return false;
        }
        let new: Vec<String> = feature
            .attributes
            .keys()
            .filter(|k| layer.fields.binary_search(k).is_err())
            .cloned()
            .collect();
        for name in new {
            let i = layer.fields.binary_search(&name).unwrap_err();
            for f in &mut layer.features {
                f.attributes.insert(name.clone(), Value::Null);
            }
            layer.fields.insert(i, name);
        }
        for name in &layer.fields {
            if !feature.attributes.contains_key(name) {
                feature.attributes.insert(name.clone(), Value::Null);
            }
        }
        layer.features.push(feature);
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

    fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Removes a layer. Its ID is not used again. A raster layer takes its
    /// raster, and the next save removes the tiles of that raster from the
    /// file.
    pub fn remove_layer(&mut self, id: LayerId) -> Option<Layer> {
        let i = self.layers.iter().position(|l| l.id == id)?;
        let layer = self.layers.remove(i);
        if let Some(raster) = layer.raster() {
            self.rasters.retain(|r| r.id != raster);
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
        assert_eq!(p.layer(a).unwrap().fields, vec!["alpha", "mid", "zeta"]);
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
        assert!(p.add_feature(a, feature(&["alpha"])));
        let line = Feature::new(Geometry::Lines(vec![]), BTreeMap::new());
        assert!(!p.add_feature(a, line));
        assert!(!p.add_feature(LayerId(99), feature(&[])));
        let layer = p.layer(a).unwrap();
        assert_eq!(layer.fields, vec!["alpha", "mid"]);
        assert_eq!(layer.features.len(), 3);
        assert!(layer.features.iter().all(|f| f.attributes.len() == 2));

        let uuid = layer.features[1].uuid();
        let moved = Geometry::Points(vec![[5.0, 6.0]]);
        assert!(p.set_geometry(a, uuid, moved.clone()));
        assert!(!p.set_geometry(a, uuid, Geometry::Lines(vec![])));
        assert!(!p.set_geometry(a, Uuid::new_v4(), moved.clone()));
        let f = &p.layer(a).unwrap().features[1];
        assert_eq!((f.uuid(), &f.geometry), (uuid, &moved));
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

        assert!(p.remove_raster(ids[0]).is_some());
        assert!(p.remove_raster(ids[0]).is_none());
        assert!(p.raster_layer(ids[0]).is_none());
        assert!(!p.set_raster_opacity(ids[0], 0.5));
        assert!(!p.set_raster_display(ids[0], RasterDisplay::Grey));
        assert!(!p.set_raster_unit(ids[0], "m"));
        assert!(!p.set_raster_smoothing(ids[0], true));
        assert!(!p.set_raster_feature_size(ids[0], 1.0));
        let d = p.add_raster("d".into(), 1024, String::new(), 0.0, 1.0);
        assert!(ids.iter().all(|id| d > *id));
        let rasters: Vec<RasterId> = p.rasters().iter().map(|r| r.id).collect();
        assert_eq!(rasters, [ids[1], ids[2], d]);
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
}
