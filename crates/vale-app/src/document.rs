//! The document: project, map frame, and layer entries.

use std::path::Path;

use vale_import::ImportError;
use vale_import::geojson::{self, Imported};
use vale_sphere::ProjectionSpec;
use vale_store::{GeometryKind, Layer, LayerId, Project, World};
use vale_style::{Color, LayerStyle, MapStyle, Stroke};

use crate::view::View;

/// One layer in the drawing order.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerEntry {
    pub layer: LayerId,
    pub visible: bool,
    pub style: LayerStyle,
}

/// How the map looks: projection, view, layer order, and map style.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    pub projection: ProjectionSpec,
    /// `None` means fit the world at the next compose.
    pub view: Option<View>,
    /// Draw order. The first entry is at the bottom.
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

fn rank(kind: GeometryKind) -> u8 {
    match kind {
        GeometryKind::Polygon => 0,
        GeometryKind::Line => 1,
        GeometryKind::Point => 2,
    }
}

impl Document {
    pub fn empty() -> Self {
        Document {
            project: Project::new(World::default()),
            frame: MapFrame {
                projection: ProjectionSpec::default(),
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
        s.fill = Some(Color::rgb(244, 241, 232));
        s.stroke = Some(Stroke {
            color: Color::rgb(150, 160, 165),
            width: 0.6,
        });

        let s = &mut doc.entry_mut(rivers).unwrap().style;
        s.stroke = Some(Stroke {
            color: Color::rgb(90, 150, 200),
            width: 1.2,
        });
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.size = 11.0;
        s.label.italic = true;
        s.label.color = Color::rgb(40, 100, 160);

        let s = &mut doc.entry_mut(places).unwrap().style;
        s.fill = Some(Color::rgb(60, 60, 60));
        s.stroke = Some(Stroke {
            color: Color::rgb(255, 255, 255),
            width: 1.0,
        });
        s.point_radius = 2.5;
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.priority_field = Some("pop_max".to_string());
        s.label.size = 11.0;
        s.label.italic = false;
        s.label.color = Color::rgb(30, 30, 30);
        doc
    }

    pub fn add_imported(&mut self, imported: Imported, source: Option<&Path>) -> Vec<LayerId> {
        let mut ids = Vec::new();
        for l in imported.layers {
            let ordinal = self
                .frame
                .entries
                .iter()
                .filter(|e| {
                    self.project
                        .layer(e.layer)
                        .is_some_and(|x| x.kind == l.kind)
                })
                .count();
            let id =
                self.project
                    .add_layer(l.name, l.kind, source.map(Path::to_path_buf), l.features);
            let style = {
                let layer = self.project.layer(id).expect("layer was just added");
                LayerStyle::default_for(layer.kind, ordinal, &layer.fields)
            };
            let new_rank = rank(l.kind);
            let pos = self
                .frame
                .entries
                .iter()
                .rposition(|e| {
                    self.project
                        .layer(e.layer)
                        .is_some_and(|x| rank(x.kind) <= new_rank)
                })
                .map_or(0, |i| i + 1);
            self.frame.entries.insert(
                pos,
                LayerEntry {
                    layer: id,
                    visible: true,
                    style,
                },
            );
            ids.push(id);
        }
        ids
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

    /// `up` moves toward the top of the drawing. Returns false at the end.
    pub fn move_entry(&mut self, id: LayerId, up: bool) -> bool {
        let Some(i) = self.frame.entries.iter().position(|e| e.layer == id) else {
            return false;
        };
        let j = if up {
            if i + 1 >= self.frame.entries.len() {
                return false;
            }
            i + 1
        } else {
            if i == 0 {
                return false;
            }
            i - 1
        };
        self.frame.entries.swap(i, j);
        true
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

    /// Clamps to 100..=100000 and rescales the view so the map looks the same.
    pub fn set_radius_km(&mut self, km: f64) {
        let new = km.clamp(100.0, 100_000.0);
        let old = self.project.world.radius_km;
        if let Some(v) = &mut self.frame.view {
            v.center = kurbo::Point::new(v.center.x * new / old, v.center.y * new / old);
            v.scale *= old / new;
        }
        self.project.world.radius_km = new;
    }

    pub fn set_projection(&mut self, spec: ProjectionSpec) {
        let spec = spec.normalized();
        if spec != self.frame.projection {
            self.frame.projection = spec;
            self.frame.view = None;
        }
    }
}
