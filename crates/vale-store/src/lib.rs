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

/// A set of features of one geometry kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: GeometryKind,
    /// The views draw the layer.
    pub visible: bool,
    /// The name of the imported file.
    pub source: Option<String>,
    /// Attribute names, sorted, no duplicates.
    pub fields: Vec<String>,
    /// A feature ID is its index here.
    pub features: Vec<Feature>,
}

/// The identity of a raster. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RasterId(pub u32);

/// The record of one raster layer: a cube map of 16-bit values. The project
/// file holds its tiles, and the project in memory does not.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub id: RasterId,
    pub name: String,
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
    /// The band limits, as values in ascending order.
    pub limits: Vec<f64>,
    /// The colors of the ramp below sea level.
    pub ramp_sea: Vec<Rgb>,
    /// The colors of the ramp above sea level.
    pub ramp_land: Vec<Rgb>,
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

/// A world and its layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub world: World,
    layers: Vec<Layer>,
    next_id: u32,
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

    /// Adds a raster with no tiles and returns its ID. Its unit is empty, and
    /// its range, its base, its band limits, and its ramp are zero or empty.
    pub fn add_raster(&mut self, name: String, face_size: u32) -> RasterId {
        let id = RasterId(self.next_raster_id);
        self.next_raster_id += 1;
        self.rasters.push(Raster {
            id,
            name,
            face_size,
            unit: String::new(),
            value_min: 0.0,
            value_max: 0.0,
            base: 0,
            limits: Vec::new(),
            ramp_sea: Vec::new(),
            ramp_land: Vec::new(),
        });
        id
    }

    /// The rasters in the order they were added.
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

    /// Removes a raster. Its ID is not used again. The next save removes its
    /// tiles from the file.
    pub fn remove_raster(&mut self, id: RasterId) -> Option<Raster> {
        let i = self.rasters.iter().position(|r| r.id == id)?;
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
            kind,
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

    fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Removes a layer. Its ID is not used again.
    pub fn remove_layer(&mut self, id: LayerId) -> Option<Layer> {
        let i = self.layers.iter().position(|l| l.id == id)?;
        Some(self.layers.remove(i))
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
        let names =
            |p: &Project| -> Vec<String> { p.layers().iter().map(|l| l.name.clone()).collect() };
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
    fn kinds() {
        assert_eq!(Geometry::Lines(vec![]).kind().name(), "line");
        assert_eq!(Geometry::Polygons(vec![]).kind().name(), "polygon");
        assert_eq!(Geometry::Points(vec![]).kind().name(), "point");
        assert_eq!(World::default().radius_km, 6371.0);
    }
}
