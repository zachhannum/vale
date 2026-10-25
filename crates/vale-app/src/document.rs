//! The document: project, map frame, and layer entries.

use std::path::Path;

use vale_import::ImportError;
use vale_import::geojson::{self, Imported};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{
    Field, FieldType, GeometryKind, Layer, LayerId, MapId, MapRecord, Project, Units, World,
};
use vale_style::{Color, LayerStyle, MapStyle, Stroke};

use crate::view::View;
use crate::world;

/// One layer in the drawing order.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerEntry {
    pub layer: LayerId,
    pub style: LayerStyle,
}

/// How the map looks: view, layer styles, and map style. The projection is
/// on the map of the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    /// `None` means fit the world at the next compose.
    pub view: Option<View>,
    /// One entry for each vector layer of the project, in the order of the
    /// project. The first entry is at the bottom.
    pub entries: Vec<LayerEntry>,
    pub graticule: bool,
    pub labels: bool,
    pub style: MapStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub project: Project,
    pub frame: MapFrame,
}

/// The style that a layer holds. A style that does not parse is the default style.
fn stored_style(layer: &Layer) -> Option<LayerStyle> {
    let text = layer.style.as_deref()?;
    LayerStyle::from_json(text)
        .inspect_err(|e| eprintln!("The style of layer {:?} is not valid: {e}", layer.name))
        .ok()
}

fn rank(kind: GeometryKind) -> u8 {
    match kind {
        GeometryKind::Polygon => 0,
        GeometryKind::Line => 1,
        GeometryKind::Point => 2,
    }
}

/// The rank of a layer in the order of an import. A raster layer draws
/// below each imported layer.
fn layer_rank(layer: &Layer) -> u8 {
    layer.geometry().map_or(0, rank)
}

impl Document {
    pub fn empty() -> Self {
        Document {
            project: Project::new(World::default()),
            frame: MapFrame {
                view: None,
                entries: Vec::new(),
                graticule: true,
                labels: true,
                style: MapStyle::default(),
            },
        }
    }

    /// The bundled Natural Earth 110m world.
    pub fn sample() -> Self {
        let mut doc = Document::empty();
        doc.project.world = World {
            name: "Earth (sample)".to_string(),
            radius_km: 6371.0,
            units: Units::Metric,
            seed: 0,
        };
        let sources: [(&str, &str); 3] = [
            (
                "Land",
                include_str!("../../../data/natural-earth/110m/ne_110m_land.geojson"),
            ),
            (
                "Rivers",
                include_str!(
                    "../../../data/natural-earth/110m/ne_110m_rivers_lake_centerlines.geojson"
                ),
            ),
            (
                "Places",
                include_str!(
                    "../../../data/natural-earth/110m/ne_110m_populated_places_simple.geojson"
                ),
            ),
        ];
        let mut ids = Vec::new();
        for (name, text) in sources {
            let imported = geojson::read_str(name, text).expect("bundled sample data is valid");
            ids.extend(doc.add_imported(imported, None));
        }
        let (land, rivers, places) = (ids[0], ids[1], ids[2]);

        let s = &mut doc.entry_mut(land).unwrap().style;
        s.set_fill(Some(Color::rgb(244, 241, 232)));
        s.set_stroke(Some(Stroke {
            color: Color::rgb(150, 160, 165),
            width: 0.6,
        }));

        let s = &mut doc.entry_mut(rivers).unwrap().style;
        s.set_stroke(Some(Stroke {
            color: Color::rgb(90, 150, 200),
            width: 1.2,
        }));
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.size = 11.0;
        s.label.italic = true;
        s.label.color = Color::rgb(40, 100, 160);

        let s = &mut doc.entry_mut(places).unwrap().style;
        s.set_fill(Some(Color::rgb(60, 60, 60)));
        s.set_stroke(Some(Stroke {
            color: Color::rgb(255, 255, 255),
            width: 1.0,
        }));
        s.set_point_radius(2.5);
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.priority_field = Some("pop_max".to_string());
        s.label.size = 11.0;
        s.label.italic = false;
        s.label.color = Color::rgb(30, 30, 30);
        doc
    }

    /// Adds each layer above the layers of its kind and of the kinds that
    /// draw below it. Polygons draw below lines, and lines draw below points.
    pub fn add_imported(&mut self, imported: Imported, source: Option<&Path>) -> Vec<LayerId> {
        let mut ids = Vec::new();
        for l in imported.layers {
            let file = source
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned());
            let new_rank = rank(l.kind);
            let to = self
                .project
                .layers()
                .iter()
                .rposition(|x| layer_rank(x) <= new_rank)
                .map_or(0, |i| i + 1);
            let id = self.project.add_layer(l.name, l.kind, file, l.features);
            self.project.move_layer(id, to);
            self.sync_entries();
            ids.push(id);
        }
        ids
    }

    /// Adds an empty layer on top. Its name is the kind and the first number
    /// that no layer of the project has, for example "Lines 2". A point
    /// layer gets a `name` text field, and its points get labels from it.
    pub fn add_layer(&mut self, kind: GeometryKind) -> LayerId {
        let noun = match kind {
            GeometryKind::Point => "Points",
            GeometryKind::Line => "Lines",
            GeometryKind::Polygon => "Polygons",
        };
        let name = (1..)
            .map(|n| format!("{noun} {n}"))
            .find(|name| self.project.layers().iter().all(|l| &l.name != name))
            .expect("a free number");
        let id = self.project.add_layer(name, kind, None, Vec::new());
        if kind == GeometryKind::Point {
            self.project
                .add_field(id, Field::new("name", FieldType::Text));
        }
        self.sync_entries();
        id
    }

    /// Puts the entries in the order of the vector layers of the project. A
    /// vector layer with no entry gets one with the default style. A raster
    /// layer has no entry.
    fn sync_entries(&mut self) {
        let mut old = std::mem::take(&mut self.frame.entries);
        let mut counts = [0; 3];
        for layer in self.project.layers() {
            let Some(kind) = layer.geometry() else {
                continue;
            };
            let ordinal = &mut counts[rank(kind) as usize];
            let entry = match old.iter().position(|e| e.layer == layer.id) {
                Some(i) => old.swap_remove(i),
                None => LayerEntry {
                    layer: layer.id,
                    style: stored_style(layer).unwrap_or_else(|| {
                        LayerStyle::default_for(kind, *ordinal, &layer.field_names())
                    }),
                },
            };
            *ordinal += 1;
            self.frame.entries.push(entry);
        }
    }

    /// Replaces the project. An entry stays if the new project has a layer
    /// with its ID and its kind. Each other layer gets a new entry.
    pub fn set_project(&mut self, project: Project) {
        let projection = self.projection();
        let old = std::mem::replace(&mut self.project, project);
        if old.world.radius_km != self.project.world.radius_km || projection != self.projection() {
            self.frame.view = None;
        }
        let kind = |p: &Project, id| p.layer(id).map(|l| l.kind);
        let new = &self.project;
        self.frame
            .entries
            .retain(|e| kind(new, e.layer).is_some() && kind(new, e.layer) == kind(&old, e.layer));
        for entry in &mut self.frame.entries {
            let Some(layer) = new.layer(entry.layer) else {
                continue;
            };
            if layer
                .style
                .as_ref()
                .is_some_and(|t| *t != entry.style.to_json())
                && let Some(style) = stored_style(layer)
            {
                entry.style = style;
            }
        }
        self.sync_entries();
    }

    /// The style of each vector layer whose project layer does not hold it, as JSON.
    pub fn changed_styles(&self) -> Vec<(LayerId, String)> {
        self.frame
            .entries
            .iter()
            .filter_map(|e| {
                let stored = self.project.layer(e.layer)?.style.as_deref();
                let json = e.style.to_json();
                (stored != Some(json.as_str())).then_some((e.layer, json))
            })
            .collect()
    }

    /// Writes each changed style into the project.
    pub fn store_styles(&mut self) {
        for (id, json) in self.changed_styles() {
            self.project.set_layer_style(id, Some(json));
        }
    }

    pub fn open_geojson(&mut self, path: &Path) -> Result<Vec<LayerId>, ImportError> {
        let imported = geojson::read_file(path)?;
        Ok(self.add_imported(imported, Some(path)))
    }

    /// Removes the entry and the layer.
    pub fn remove_layer(&mut self, id: LayerId) -> bool {
        self.frame.entries.retain(|e| e.layer != id);
        self.project.remove_layer(id).is_some()
    }

    /// Moves a layer of the map past the next layer of the map. `up` moves
    /// toward the top of the drawing. Returns false at the end.
    pub fn move_entry(&mut self, id: LayerId, up: bool) -> bool {
        let entries = &self.frame.entries;
        let Some(i) = entries.iter().position(|e| e.layer == id) else {
            return false;
        };
        let next = if up {
            i.checked_add(1)
        } else {
            i.checked_sub(1)
        };
        let Some(next) = next.and_then(|i| entries.get(i)).map(|e| e.layer) else {
            return false;
        };
        let layers = self.project.layers();
        let to = layers.iter().position(|l| l.id == next);
        to.is_some_and(|to| self.move_layer(id, to))
    }

    /// Moves a layer one place in the draw order of the layers of each kind.
    /// `up` moves toward the top of the drawing. Returns false at the end.
    pub fn move_by_one(&mut self, id: LayerId, up: bool) -> bool {
        let layers = self.project.layers();
        let Some(i) = layers.iter().position(|l| l.id == id) else {
            return false;
        };
        if (up && i + 1 >= layers.len()) || (!up && i == 0) {
            return false;
        }
        self.move_layer(id, if up { i + 1 } else { i - 1 })
    }

    /// Moves a layer to this index of the draw order. Index 0 is the bottom.
    pub fn move_layer(&mut self, id: LayerId, to: usize) -> bool {
        let moved = self.project.move_layer(id, to);
        self.sync_entries();
        moved
    }

    /// Gives a layer a new name. A blank name changes nothing.
    pub fn rename_layer(&mut self, id: LayerId, name: &str) -> bool {
        let name = name.trim();
        !name.is_empty() && self.project.rename_layer(id, name.to_string())
    }

    /// Gives a field of a vector layer a new name. The label field and the
    /// priority field of the style keep the field.
    pub fn rename_field(&mut self, id: LayerId, old: &str, new: &str) -> bool {
        if !self.project.rename_field(id, old, new) {
            return false;
        }
        if let Some(entry) = self.entry_mut(id) {
            let label = &mut entry.style.label;
            for field in [&mut label.field, &mut label.priority_field] {
                if field.as_deref() == Some(old) {
                    *field = Some(new.to_string());
                }
            }
        }
        true
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) -> bool {
        self.project.set_layer_visible(id, visible)
    }

    pub fn visible(&self, id: LayerId) -> bool {
        self.project.layer(id).is_some_and(|l| l.visible)
    }

    pub fn entry(&self, id: LayerId) -> Option<&LayerEntry> {
        self.frame.entries.iter().find(|e| e.layer == id)
    }

    pub fn entry_mut(&mut self, id: LayerId) -> Option<&mut LayerEntry> {
        self.frame.entries.iter_mut().find(|e| e.layer == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.project.layer(id)
    }

    /// Clamps to `world::RADIUS_KM` and rescales the view so the map looks the same.
    pub fn set_radius_km(&mut self, km: f64) {
        let new = km.clamp(*world::RADIUS_KM.start(), *world::RADIUS_KM.end());
        let old = self.project.world.radius_km;
        if let Some(v) = &mut self.frame.view {
            v.center = kurbo::Point::new(v.center.x * new / old, v.center.y * new / old);
            v.scale *= old / new;
        }
        self.project.world.radius_km = new;
    }

    /// The map of every frame.
    pub fn map_id(&self) -> MapId {
        self.map().id
    }

    fn map(&self) -> &MapRecord {
        &self.project.atlas.maps[0]
    }

    /// The projection of the map. An unknown kind is the default projection.
    pub fn projection(&self) -> ProjectionSpec {
        let map = self.map();
        match ProjectionKind::from_id(&map.projection) {
            Some(kind) => ProjectionSpec {
                kind,
                lon0: map.lon0,
                lat0: map.lat0,
            }
            .normalized(),
            None => ProjectionSpec::default(),
        }
    }

    pub fn set_projection(&mut self, spec: ProjectionSpec) {
        let spec = spec.normalized();
        if spec != self.projection() {
            let map = &mut self.project.atlas.maps[0];
            map.projection = spec.kind.id().to_string();
            map.lon0 = spec.lon0;
            map.lat0 = spec.lat0;
            self.frame.view = None;
        }
    }

    /// True if a map shows a layer.
    pub fn shown(&self, map: MapId, layer: LayerId) -> bool {
        let map = self.project.atlas.map(map);
        map.is_some_and(|m| !m.hidden.contains(&layer))
    }

    /// Shows or hides a layer in a map. Returns false if nothing changed.
    pub fn set_shown(&mut self, map: MapId, layer: LayerId, on: bool) -> bool {
        let Some(map) = self.project.atlas.map_mut(map) else {
            return false;
        };
        if on {
            map.hidden.remove(&layer)
        } else {
            map.hidden.insert(layer)
        }
    }
}
