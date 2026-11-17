//! The toolbox operation "Polygonize": a polygon for each band of a raster
//! layer, with the band limits of its color stops.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use vale_store::{Feature, Geometry, GeometryKind, LayerId, LonLat, RasterId, Value};
use vale_style::{Color, LayerStyle};
use vale_terrain::{BandRings, Bands, Heightmap, Trace, polygonize};

use super::{AppState, is_height};
use crate::classify::{self, Class, Step};
use crate::globe::polygon::sphere_polygons;

/// The name of the operation in the toolbox.
pub const POLYGONIZE: &str = "Polygonize";
/// What the operation does, below its name on its page.
pub const POLYGONIZE_ABOUT: &str =
    "Makes a polygon for each band of a raster layer, from its color stops.";
/// The hint of the operation when the world has no raster layer.
pub const NEEDS_RASTER: &str = "Add a raster layer.";
pub const INPUT: &str = "Input";
pub const LIMITS: &str = "Limits";
pub const SMOOTH: &str = "Smooth";
pub const SPLIT: &str = "Split into two layers at 0 m";
pub const ABOVE: &str = "Above";
pub const BELOW: &str = "Below";
/// The names of the output layers of a height layer.
pub const TOPOGRAPHY: &str = "Topography";
pub const BATHYMETRY: &str = "Bathymetry";

/// The width of the filter on the values at full Smooth, in texels.
const MAX_SIGMA: f64 = 2.0;
/// The largest distance of a removed vertex from the outline, in texels.
const TOLERANCE: f64 = 0.1;

/// The inputs and the parameters of "Polygonize", and its last run.
pub struct PolygonizeState {
    /// The raster layer that the user chose. `None`, or a layer that is
    /// gone, gives the default layer.
    pub raster: Option<RasterId>,
    /// The smoothing, from 0 to 1.
    pub smooth: f64,
    /// Split the output of a height layer at sea level.
    pub split: bool,
    pub above: String,
    pub below: String,
    pub last: Option<Run>,
}

impl Default for PolygonizeState {
    fn default() -> Self {
        PolygonizeState {
            raster: None,
            smooth: 0.6,
            split: true,
            above: TOPOGRAPHY.to_owned(),
            below: BATHYMETRY.to_owned(),
            last: None,
        }
    }
}

/// The record of a run: its input, and the layers that it made.
pub struct Run {
    pub raster: RasterId,
    /// A snapshot of the values. It shares the tiles that did not change.
    pub levels: Heightmap,
    pub bands: Bands,
    pub outputs: Vec<LayerId>,
    pub time: Duration,
}

/// One output layer: its name and its features.
pub struct Output {
    pub name: String,
    pub features: Vec<Feature>,
    /// The bands of the features, for the style.
    pub bands: Vec<usize>,
}

/// The names of the attributes of the lower limit and the upper limit of
/// a band.
pub fn limit_fields(height: bool) -> (&'static str, &'static str) {
    match height {
        true => ("elev_min", "elev_max"),
        false => ("value_min", "value_max"),
    }
}

/// The level of each inner stop of `bands`, as a fraction of a level.
pub fn limit_levels(bands: &Bands) -> Vec<f64> {
    let stops = bands.stops();
    let span = bands.max() - bands.min();
    (stops[1..stops.len() - 1].iter())
        .map(|s| (s.value - bands.min()) / span * 65535.0)
        .collect()
}

/// The polygons of one band.
fn band_polygons(band: &BandRings) -> Vec<Vec<Vec<LonLat>>> {
    if band.whole {
        let world = vec![
            [-180.0, -90.0],
            [180.0, -90.0],
            [180.0, 90.0],
            [-180.0, 90.0],
            [-180.0, -90.0],
        ];
        return vec![vec![world]];
    }
    sphere_polygons(&band.rings)
}

/// The output layers of a run: one feature for each band that has an
/// area. `split` is the index of the band above the split, if the output
/// splits, with the names of the layers above it and below it.
pub fn outputs(
    map: &Heightmap,
    bands: &Bands,
    height: bool,
    smooth: f64,
    names: (&str, Option<(usize, &str)>),
) -> Vec<Output> {
    let trace = Trace {
        sigma: smooth.clamp(0.0, 1.0) * MAX_SIGMA,
        tolerance: TOLERANCE,
    };
    let rings = polygonize(map, &limit_levels(bands), trace);
    let (min_field, max_field) = limit_fields(height);
    let stops = bands.stops();
    let (above, split) = names;
    let mut out = vec![Output {
        name: above.to_owned(),
        features: Vec::new(),
        bands: Vec::new(),
    }];
    if let Some((_, below)) = split {
        out.insert(
            0,
            Output {
                name: below.to_owned(),
                features: Vec::new(),
                bands: Vec::new(),
            },
        );
    }
    for (k, band) in rings.iter().enumerate() {
        let polygons = band_polygons(band);
        if polygons.is_empty() {
            continue;
        }
        let attributes = BTreeMap::from([
            (min_field.to_owned(), Value::Number(stops[k].value)),
            (max_field.to_owned(), Value::Number(stops[k + 1].value)),
        ]);
        let layer = match split {
            Some((sea, _)) if k >= sea => 1,
            _ => 0,
        };
        let layer = layer.min(out.len() - 1);
        out[layer]
            .features
            .push(Feature::new(Geometry::Polygons(polygons), attributes));
        out[layer].bands.push(k);
    }
    out
}

/// A rule for each band, with the color of the band.
pub fn band_rules(bands: &Bands, height: bool, which: &[usize]) -> Vec<vale_style::Rule> {
    let (field, _) = limit_fields(height);
    let stops = bands.stops();
    let classes: Vec<Class> = (which.iter())
        .map(|&k| {
            let lo = classify::number_text(stops[k].value);
            let hi = classify::number_text(stops[k + 1].value);
            let name = match bands.name(k) {
                "" => format!("{lo} – {hi}"),
                name => name.to_owned(),
            };
            Class {
                name,
                filter: format!("{field} = {lo}"),
                extra: false,
                value: None,
            }
        })
        .collect();
    let colors: Vec<Color> = (which.iter())
        .map(|&k| {
            let [r, g, b] = bands.colors()[k];
            Color::rgb(r, g, b)
        })
        .collect();
    let kind = GeometryKind::Polygon;
    let template = LayerStyle::default_for(kind, 0, &[]).style.rules[0].clone();
    classify::make_rules(&template, kind, &classes, &colors, Step::None)
}

/// The note of an output layer whose input changed, in the panel of the
/// layer, and its button.
pub const STALE_LAYER: &str = "Out of date. The input changed after the last run.";
pub const RUN_AGAIN: &str = "Run again";

/// The note of the outputs of a run whose input changed.
pub fn stale_text(names: &[String]) -> String {
    match names.len() {
        1 => format!("{} is out of date. This run replaces it.", names[0]),
        _ => format!(
            "{} are out of date. This run replaces them.",
            names.join(" and ")
        ),
    }
}

/// The text of the time of a run.
pub fn time_text(time: Duration) -> String {
    match time.as_secs_f64() {
        t if t < 1.0 => format!("{} ms", (t * 1000.0).round()),
        t => format!("{t:.1} s"),
    }
}

impl AppState {
    /// The raster layers that the globe holds, with their names.
    pub fn held_rasters(&self) -> Vec<(RasterId, String)> {
        let project = &self.doc.project;
        (project.layers().iter())
            .filter_map(|l| {
                let raster = project.raster(l.raster()?)?;
                self.globe
                    .holds(raster.id)
                    .then(|| (raster.id, l.name.clone()))
            })
            .collect()
    }

    /// The input of "Polygonize": the chosen layer, else the selected
    /// raster layer, else the first height layer, else the first raster
    /// layer.
    pub fn polygonize_input(&self) -> Option<RasterId> {
        let rasters = self.held_rasters();
        let has = |id: Option<RasterId>| id.filter(|id| rasters.iter().any(|(r, _)| r == id));
        let height = || {
            (rasters.iter())
                .find(|(id, _)| self.doc.project.raster(*id).is_some_and(is_height))
                .map(|(id, _)| *id)
        };
        (has(self.polygonize.raster))
            .or_else(|| has(self.selected_raster()))
            .or_else(height)
            .or_else(|| rasters.first().map(|(id, _)| *id))
    }

    /// The bands of a raster layer that the globe holds.
    fn raster_bands(&mut self, id: RasterId) -> Option<Bands> {
        self.globe.bands_mut(id).map(|b| b.clone())
    }

    /// The number of limits between the bands of the input of
    /// "Polygonize".
    pub fn polygonize_limits(&mut self) -> Option<usize> {
        let id = self.polygonize_input()?;
        let bands = self.raster_bands(id)?;
        Some(bands.stops().len().saturating_sub(2))
    }

    /// True if the input of "Polygonize" splits at sea level.
    pub fn polygonize_can_split(&mut self) -> bool {
        let Some(id) = self.polygonize_input() else {
            return false;
        };
        let height = self.doc.project.raster(id).is_some_and(is_height);
        height && self.raster_bands(id).is_some_and(|b| b.sea().is_some())
    }

    /// The output layers of the last run that the project still has.
    pub fn polygonize_outputs(&self) -> Vec<LayerId> {
        let Some(run) = &self.polygonize.last else {
            return Vec::new();
        };
        (run.outputs.iter())
            .filter(|id| self.doc.layer(**id).is_some())
            .copied()
            .collect()
    }

    /// True if the input of the last run changed after the run, and the
    /// project still has an output of the run.
    fn last_run_stale(&mut self) -> bool {
        let Some(raster) = self.polygonize.last.as_ref().map(|r| r.raster) else {
            return false;
        };
        let Some(bands) = self.raster_bands(raster) else {
            return false;
        };
        let Some(run) = &self.polygonize.last else {
            return false;
        };
        // The snapshot is of the whole heightmap or of the reduced one, as the
        // globe held it at the run.
        let maps = [self.globe.raster_map(raster), self.globe.value_map(raster)];
        let same = maps.iter().flatten().any(|m| m.same_tiles(&run.levels));
        let changed = !same || bands != run.bands;
        changed && !self.polygonize_outputs().is_empty()
    }

    /// The names of the outputs of the last run, if its input is the input
    /// of the page and it changed after the run. A new run replaces those
    /// layers.
    pub fn polygonize_stale(&mut self) -> Option<Vec<String>> {
        let raster = self.polygonize.last.as_ref()?.raster;
        if Some(raster) != self.polygonize_input() || !self.last_run_stale() {
            return None;
        }
        let names = (self.polygonize_outputs().iter())
            .filter_map(|id| Some(self.doc.layer(*id)?.name.clone()))
            .collect();
        Some(names)
    }

    /// True if layer `id` is an output of the last run, and the input of
    /// the run changed after it.
    pub fn is_stale_output(&mut self, id: LayerId) -> bool {
        self.polygonize_outputs().contains(&id) && self.last_run_stale()
    }

    /// Runs "Polygonize" again on the input of the last run, with the
    /// parameters of the page. The new layers replace the old outputs.
    pub fn run_polygonize_again(&mut self) {
        let Some(raster) = self.polygonize.last.as_ref().map(|r| r.raster) else {
            return;
        };
        self.polygonize.raster = Some(raster);
        self.run_polygonize();
    }

    /// True if "Polygonize" can run on its input.
    pub fn can_polygonize(&self) -> bool {
        self.polygonize_input().is_some() && !self.globe.busy()
    }

    /// Runs "Polygonize" from the toolbox, and keeps the result for the
    /// note of the toolbox.
    pub fn run_polygonize(&mut self) {
        let Some(ids) = self.polygonize() else {
            return;
        };
        let time = self
            .polygonize
            .last
            .as_ref()
            .map_or_else(Duration::default, |r| r.time);
        let names: Vec<String> = (ids.iter())
            .filter_map(|id| Some(self.doc.layer(*id)?.name.clone()))
            .collect();
        self.toolbox_note = Some(format!(
            "Made {} in {}.",
            names.join(" and "),
            time_text(time)
        ));
    }

    /// Makes the polygons of the bands of the input layer, as one undo
    /// step. The new layers replace the outputs of the last run on the same
    /// input. Returns the new layers.
    pub fn polygonize(&mut self) -> Option<Vec<LayerId>> {
        let raster = self.polygonize_input()?;
        if self.globe.busy() {
            return None;
        }
        let height = self.doc.project.raster(raster).is_some_and(is_height);
        let name = self.doc.project.raster_layer(raster)?.name.clone();
        let bands = self.raster_bands(raster)?;
        let split = self.polygonize.split && self.polygonize_can_split();
        let clean = |text: &str, default: &str| match text.trim() {
            "" => default.to_owned(),
            text => text.to_owned(),
        };
        let above = match (split, height) {
            (true, _) => clean(&self.polygonize.above, TOPOGRAPHY),
            (false, true) => TOPOGRAPHY.to_owned(),
            (false, false) => format!("{name} polygons"),
        };
        let below = clean(&self.polygonize.below, BATHYMETRY);
        let sea = bands.sea().filter(|_| split);
        // The trace reads the whole heightmap. The globe holds it for the
        // active layer, and the project file holds it for another layer.
        let loaded;
        let map = match self.globe.raster_map(raster) {
            Some(map) => map,
            None => {
                let record = self.doc.project.raster(raster)?;
                let session = self.session.as_ref()?;
                loaded = match session.load_map(record) {
                    Ok(map) => map,
                    Err(e) => {
                        self.status = format!("Could not read the raster layer: {e}");
                        return None;
                    }
                };
                &loaded
            }
        };
        let start = Instant::now();
        let layers = outputs(
            map,
            &bands,
            height,
            self.polygonize.smooth,
            (&above, sea.map(|s| (s, below.as_str()))),
        );
        let time = start.elapsed();
        // A loaded heightmap shares no tile with the globe, so the snapshot
        // to compare is of the reduced heightmap.
        let held = self
            .globe
            .raster_map(raster)
            .or(self.globe.value_map(raster));
        let levels = held.unwrap_or(map).snapshot();
        // The new layers take the places of the layers that they replace.
        let replaced: Vec<LayerId> = match &self.polygonize.last {
            Some(run) if run.raster == raster => self.polygonize_outputs(),
            _ => Vec::new(),
        };
        let place = (replaced.iter())
            .filter_map(|id| self.doc.project.layers().iter().position(|l| l.id == *id))
            .min();
        self.commit_selection_form();
        self.commit_new_feature();
        self.save_edit();
        for id in &replaced {
            self.drop_layer(*id);
        }
        let mut ids = Vec::new();
        for (i, layer) in layers.into_iter().enumerate() {
            let imported = vale_import::Imported {
                layers: vec![vale_import::ImportedLayer {
                    name: layer.name,
                    kind: GeometryKind::Polygon,
                    features: layer.features,
                }],
                skipped: 0,
            };
            let Some(&id) = self.doc.add_imported(imported, None).first() else {
                continue;
            };
            if let Some(place) = place {
                self.doc.move_layer(id, place + i);
            }
            if let Some(entry) = self.doc.entry_mut(id) {
                entry.style.style.rules = band_rules(&bands, height, &layer.bands);
            }
            ids.push(id);
        }
        self.sync_globe();
        self.touch();
        self.save_edit();
        self.polygonize.last = Some(Run {
            raster,
            levels,
            bands,
            outputs: ids.clone(),
            time,
        });
        Some(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_text_has_ms_below_a_second() {
        assert_eq!(time_text(Duration::from_millis(250)), "250 ms");
        assert_eq!(time_text(Duration::from_millis(1250)), "1.2 s");
    }
}
