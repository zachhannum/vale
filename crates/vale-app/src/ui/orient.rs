//! The toolbox operation "Orient lines downhill": each line of a layer gets
//! its first vertex at its higher end, so a taper is narrow at the top.

use vale_store::{Geometry, GeometryKind, LayerId, LonLat, RasterId, Uuid};
use vale_terrain::math::lonlat_to_dir;

use super::{AppState, is_height};

/// The name of the operation in the toolbox.
pub const ORIENT_DOWNHILL: &str = "Orient lines downhill";
/// What the operation does, below its name on its page.
pub const ORIENT_ABOUT: &str =
    "Reverses each line that runs uphill, so it starts at its higher end.";
/// The hint of the operation when the world has no line layer.
pub const NEEDS_LINES: &str = "Add a line layer.";
/// The hint of the operation when the world has no height layer.
pub const NEEDS_HEIGHT: &str = "Add a height layer.";

/// The layers that the user chose for "Orient lines downhill". `None`, or
/// a layer that is gone, gives the default layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrientInputs {
    pub lines: Option<LayerId>,
    pub height: Option<RasterId>,
}

/// The text of the features that the operation changes: "All", or the
/// number of selected features.
pub fn features_text(selected: usize) -> String {
    match selected {
        0 => "All".to_string(),
        n => format!("{n} selected"),
    }
}

/// The line with its parts and its vertices in the opposite order, if its
/// first vertex is lower than its last vertex. `level` gives the height at
/// a position. `None` for a line that runs downhill or on the level, and for
/// a point or a polygon.
pub fn downhill(g: &Geometry, level: impl Fn(LonLat) -> f64) -> Option<Geometry> {
    let Geometry::Lines(parts) = g else {
        return None;
    };
    let first = *parts.first()?.first()?;
    let last = *parts.last()?.last()?;
    if level(first) >= level(last) {
        return None;
    }
    let reversed = (parts.iter().rev())
        .map(|part| part.iter().rev().copied().collect())
        .collect();
    Some(Geometry::Lines(reversed))
}

impl AppState {
    /// The line layers of the world, with their names.
    pub fn line_layers(&self) -> Vec<(LayerId, String)> {
        (self.doc.project.layers().iter())
            .filter(|l| l.geometry() == Some(GeometryKind::Line))
            .map(|l| (l.id, l.name.clone()))
            .collect()
    }

    /// The height layers that the globe holds, with their names.
    pub fn held_heights(&self) -> Vec<(RasterId, String)> {
        let project = &self.doc.project;
        (project.layers().iter())
            .filter_map(|l| {
                let raster = project.raster(l.raster()?)?;
                let fit = is_height(raster) && self.globe.holds(raster.id);
                fit.then(|| (raster.id, l.name.clone()))
            })
            .collect()
    }

    /// The line layer of "Orient lines downhill": the chosen layer, else
    /// the selected layer if it is a line layer, else the first line layer.
    pub fn orient_lines(&self) -> Option<LayerId> {
        let lines = self.line_layers();
        let has = |id: Option<LayerId>| id.filter(|id| lines.iter().any(|(l, _)| l == id));
        (has(self.orient.lines))
            .or_else(|| has(self.selected_vector()))
            .or_else(|| lines.first().map(|(id, _)| *id))
    }

    /// The height layer of "Orient lines downhill": the chosen layer, else
    /// the first height layer that the globe holds.
    pub fn orient_height(&self) -> Option<RasterId> {
        let heights = self.held_heights();
        let chosen = self.orient.height;
        (chosen.filter(|id| heights.iter().any(|(h, _)| h == id)))
            .or_else(|| heights.first().map(|(id, _)| *id))
    }

    /// The number of selected features in layer `layer`.
    pub fn selected_in(&self, layer: LayerId) -> usize {
        (self.feature_selection.iter())
            .filter(|f| f.layer == layer)
            .count()
    }

    /// True if "Orient lines downhill" can run on its inputs.
    pub fn can_orient_downhill(&self) -> bool {
        self.orient_lines().is_some() && self.orient_height().is_some() && !self.globe.busy()
    }

    /// The hint of "Orient lines downhill" when it has no input.
    pub fn orient_hint(&self) -> Option<&'static str> {
        if self.orient_lines().is_none() {
            Some(NEEDS_LINES)
        } else if self.orient_height().is_none() {
            Some(NEEDS_HEIGHT)
        } else {
            None
        }
    }

    /// Runs "Orient lines downhill" from the toolbox on its inputs, and
    /// keeps the result for the note of the toolbox.
    pub fn run_orient_downhill(&mut self) {
        let (Some(layer), Some(height)) = (self.orient_lines(), self.orient_height()) else {
            return;
        };
        let n = self.orient_downhill(layer, height);
        self.toolbox_note = Some(match n {
            0 => "All lines run downhill.".to_string(),
            1 => "Reversed 1 line.".to_string(),
            n => format!("Reversed {n} lines."),
        });
    }

    /// Reverses each line of layer `layer` that runs uphill on height
    /// layer `height`, in one undo step. If the layer has selected
    /// features, only those change. Returns the number of reversed lines.
    pub fn orient_downhill(&mut self, layer: LayerId, height: RasterId) -> usize {
        let Some(lines) = self.doc.layer(layer) else {
            return 0;
        };
        if lines.geometry() != Some(GeometryKind::Line) || self.globe.busy() {
            return 0;
        }
        let Some(map) = self.globe.value_map(height) else {
            return 0;
        };
        let level = |p: LonLat| map.sample_bilinear(lonlat_to_dir(p[0], p[1]));
        let picked: Vec<Uuid> = (self.feature_selection.iter())
            .filter(|f| f.layer == layer)
            .map(|f| f.uuid)
            .collect();
        let changes: Vec<(Uuid, Geometry)> = (lines.features.iter())
            .filter(|f| picked.is_empty() || picked.contains(&f.uuid()))
            .filter_map(|f| Some((f.uuid(), downhill(&f.geometry, level)?)))
            .collect();
        if changes.is_empty() {
            return 0;
        }
        self.commit_selection_form();
        self.commit_new_feature();
        self.save_edit();
        let n = changes.len();
        for (uuid, geometry) in changes {
            self.doc.project.set_geometry(layer, uuid, geometry);
        }
        self.end_vector_edit(layer);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_that_runs_uphill_turns_around() {
        let level = |p: LonLat| p[0];
        let up = Geometry::Lines(vec![
            vec![[0.0, 0.0], [1.0, 0.0]],
            vec![[2.0, 0.0], [3.0, 1.0]],
        ]);
        let down = Geometry::Lines(vec![
            vec![[3.0, 1.0], [2.0, 0.0]],
            vec![[1.0, 0.0], [0.0, 0.0]],
        ]);
        assert_eq!(downhill(&up, level), Some(down.clone()));
        assert_eq!(downhill(&down, level), None);
        let flat = Geometry::Lines(vec![vec![[1.0, 0.0], [1.0, 5.0]]]);
        assert_eq!(downhill(&flat, level), None);
        assert_eq!(downhill(&Geometry::Points(vec![[0.0, 0.0]]), level), None);
    }
}
