//! The placement of a layer from a file on the globe, and the move of a
//! layer to a new placement.

use std::path::Path;

use vale_import::geojson::{self, ImportedLayer};
use vale_store::{Extents, Geometry, Layer, LayerId, LonLat, Placement};

use crate::globe::import::file_name;
use crate::ui::{AppState, LayerRef, Note};

/// A placement of a layer that the world view shows before it is stored.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PlaceDraft {
    pub layer: LayerId,
    pub placement: Placement,
}

/// The extents of a layer. A layer with no placement covers the globe.
pub fn layer_extents(layer: &Layer) -> Extents {
    layer.placement.map_or(Extents::GLOBE, |p| p.extents)
}

/// True for the extensions of a GeoJSON file.
pub fn is_vector_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    ext.is_some_and(|e| e.eq_ignore_ascii_case("geojson") || e.eq_ignore_ascii_case("json"))
}

/// The place in `to` of a point in `from`. The map is linear in each axis,
/// and north goes to north.
pub fn map_point(p: LonLat, from: &Extents, to: &Extents) -> LonLat {
    let x = (p[0] - from.west) / (from.east - from.west);
    let y = (p[1] - from.south) / (from.north - from.south);
    [
        to.west + x * (to.east - to.west),
        to.south + y * (to.north - to.south),
    ]
}

/// The geometry with each point moved from `from` to `to`.
pub fn map_geometry(geometry: &Geometry, from: &Extents, to: &Extents) -> Geometry {
    let line = |l: &Vec<LonLat>| l.iter().map(|p| map_point(*p, from, to)).collect();
    match geometry {
        Geometry::Points(points) => Geometry::Points(line(points)),
        Geometry::Lines(lines) => Geometry::Lines(lines.iter().map(line).collect()),
        Geometry::Polygons(polygons) => {
            let polygon = |rings: &Vec<Vec<LonLat>>| rings.iter().map(line).collect();
            Geometry::Polygons(polygons.iter().map(polygon).collect())
        }
    }
}

/// The frame of the coordinates of a vector file: the globe if each
/// coordinate is a longitude and a latitude, or else the bounding box. A
/// box with no width or no height gets one unit.
pub fn file_frame(layers: &[ImportedLayer]) -> Extents {
    let mut b = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut add = |p: &LonLat| {
        b = [
            b[0].min(p[0]),
            b[1].max(p[0]),
            b[2].min(p[1]),
            b[3].max(p[1]),
        ];
    };
    for feature in layers.iter().flat_map(|l| &l.features) {
        match &feature.geometry {
            Geometry::Points(points) => points.iter().for_each(&mut add),
            Geometry::Lines(lines) => lines.iter().flatten().for_each(&mut add),
            Geometry::Polygons(p) => p.iter().flatten().flatten().for_each(&mut add),
        }
    }
    let g = Extents::GLOBE;
    let in_degrees = g.west <= b[0] && b[1] <= g.east && g.south <= b[2] && b[3] <= g.north;
    if in_degrees || b[0] > b[1] {
        return g;
    }
    let grow = |lo: f64, hi: f64| {
        if hi > lo {
            (lo, hi)
        } else {
            (lo - 0.5, lo + 0.5)
        }
    };
    let ((west, east), (south, north)) = (grow(b[0], b[1]), grow(b[2], b[3]));
    Extents {
        west,
        east,
        south,
        north,
    }
}

impl AppState {
    /// Adds the layers of a GeoJSON file as one undo step, and selects the
    /// top one. A file in other units than degrees fills the globe.
    pub fn import_vector(&mut self, path: &Path) {
        let name = file_name(path);
        let mut imported = match geojson::read_file_raw(path) {
            Ok(imported) => imported,
            Err(e) => {
                self.note_image(
                    Note::new(format!("Cannot import {name}: {e}"), true, None),
                    None,
                );
                return;
            }
        };
        let frame = file_frame(&imported.layers);
        if frame != Extents::GLOBE {
            for feature in imported.layers.iter_mut().flat_map(|l| &mut l.features) {
                feature.geometry = map_geometry(&feature.geometry, &frame, &Extents::GLOBE);
            }
        }
        let size = [frame.east - frame.west, frame.north - frame.south];
        self.save_edit();
        if self.doc.frame.entries.is_empty() {
            self.doc.frame.view = None;
        }
        let ids = self.doc.add_imported(imported, Some(path));
        for id in &ids {
            let placement = Some(Placement::globe(size));
            self.doc.project.set_layer_placement(*id, placement);
        }
        if let Some(&top) = ids.last() {
            self.select(Some(LayerRef::Vector(top)));
            self.pad.layer = Some(LayerRef::Vector(top));
        }
        self.doc.store_styles();
        self.touch();
        self.save_edit();
        let text = format!("Added {name}: {} layers", ids.len());
        self.note_image(Note::new(text, false, None), None);
    }

    /// Moves a layer to a new placement as one undo step. The points of a
    /// vector layer move from the old extents to the new extents. Returns
    /// `false` if the layer is not in the project or the extents are not
    /// valid.
    pub fn place_layer(&mut self, id: LayerId, placement: Placement) -> bool {
        let Some(layer) = self.doc.layer(id) else {
            return false;
        };
        if !placement.extents.is_valid() {
            return false;
        }
        let from = layer_extents(layer);
        let to = placement.extents;
        let moved: Vec<_> = match layer.geometry() {
            Some(_) if from != to => layer
                .features
                .iter()
                .map(|f| (f.uuid(), map_geometry(&f.geometry, &from, &to)))
                .collect(),
            _ => Vec::new(),
        };
        self.save_edit();
        let project = &mut self.doc.project;
        for (uuid, geometry) in moved {
            project.set_geometry(id, uuid, geometry);
        }
        project.set_layer_placement(id, Some(placement));
        self.pipeline.forget(id);
        self.sync_globe();
        self.touch();
        self.save_edit();
        true
    }

    /// Sets or clears the draft. The world view draws the layer of the draft
    /// at its extents while they are valid.
    pub fn set_place_draft(&mut self, draft: Option<PlaceDraft>) {
        self.place_draft = draft;
    }

    /// The layer of the draft and its extents, if the extents are valid and
    /// the layer is in the project.
    pub fn draft_extents(&self) -> Option<(LayerId, Extents)> {
        let draft = self.place_draft?;
        self.doc.layer(draft.layer)?;
        let extents = draft.placement.extents;
        extents.is_valid().then_some((draft.layer, extents))
    }

    /// Stores the placement of the draft and clears the draft. Returns
    /// `false` if no draft is valid. A draft that is not valid stays.
    pub fn commit_place_draft(&mut self) -> bool {
        if self.draft_extents().is_none() {
            return false;
        }
        let Some(draft) = self.place_draft.take() else {
            return false;
        };
        self.place_layer(draft.layer, draft.placement)
    }

    /// Clears the draft. The layer stays at its stored placement.
    pub fn cancel_place_draft(&mut self) {
        self.place_draft = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_maps_linearly_with_north_up() {
        let from = Extents {
            west: 0.0,
            east: 2000.0,
            south: 0.0,
            north: 1000.0,
        };
        let to = Extents::GLOBE;
        assert_eq!(map_point([0.0, 1000.0], &from, &to), [-180.0, 90.0]);
        assert_eq!(map_point([1000.0, 500.0], &from, &to), [0.0, 0.0]);
        assert_eq!(map_point([2000.0, 0.0], &from, &to), [180.0, -90.0]);
    }

    #[test]
    fn a_path_ends_in_geojson_or_json() {
        assert!(is_vector_path(Path::new("a/b.geojson")));
        assert!(is_vector_path(Path::new("b.JSON")));
        assert!(!is_vector_path(Path::new("b.png")));
    }
}
