//! "Orient rivers downhill": each line of a layer gets its first vertex at
//! its upstream end, so a taper is narrow at the source.

use vale_store::{Geometry, GeometryKind, LayerId, LonLat, RasterId, Uuid};
use vale_terrain::math::lonlat_to_dir;

use super::{AppState, is_height};

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
    /// The height layer that "Orient rivers downhill" reads: the first
    /// height layer that the globe holds.
    fn orient_height(&self) -> Option<RasterId> {
        let rasters = self.doc.project.rasters();
        let fit = |r: &&vale_store::Raster| is_height(r) && self.globe.holds(r.id);
        rasters.iter().find(fit).map(|r| r.id)
    }

    /// True if "Orient rivers downhill" can run on layer `layer`.
    pub fn can_orient_rivers(&self, layer: LayerId) -> bool {
        let lines = self.doc.layer(layer).and_then(|l| l.geometry()) == Some(GeometryKind::Line);
        lines && !self.globe.busy() && self.orient_height().is_some()
    }

    /// Reverses each line of layer `layer` that runs uphill, in one undo
    /// step. Returns the number of reversed lines.
    pub fn orient_rivers(&mut self, layer: LayerId) -> usize {
        if !self.can_orient_rivers(layer) {
            return 0;
        }
        let (Some(height), Some(lines)) = (self.orient_height(), self.doc.layer(layer)) else {
            return 0;
        };
        let Some(map) = self.globe.value_map(height) else {
            return 0;
        };
        let level = |p: LonLat| map.sample_bilinear(lonlat_to_dir(p[0], p[1]));
        let changes: Vec<(Uuid, Geometry)> = (lines.features.iter())
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
