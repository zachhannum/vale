//! The values of the raster layers under the features, for the `raster`
//! function of the expressions. The values of a vector layer are kept for
//! each raster layer that a style reads, and a worker thread computes them
//! again after a change to the raster or to a feature. A read never waits
//! for the worker: it gets the old value until the new one arrives.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use vale_sphere::LonLat;
use vale_sphere::measure::{area_center, line_center};
use vale_store::{Geometry, LayerId, RasterId};
use vale_style::expr::{RasterValues, Stat};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{FACES, Heightmap, TexelRect, Zonal, line_zonal, polygon_zonal, rects_meet};

use crate::document::Document;

/// The mean, the lowest, and the highest level under a feature.
type Levels = [f64; 3];

/// The texels that the levels of a feature come from. `None`: any change
/// to the raster computes the levels again.
type Rects = Option<[Option<TexelRect>; FACES]>;

/// The levels that a frozen copy computed, by layer, raster, and feature.
type Memo = HashMap<(LayerId, RasterId, usize), Option<Levels>>;

/// A raster layer by its name.
#[derive(Clone, Copy)]
struct Named {
    id: RasterId,
    /// The values of level 0 and of level 65,535.
    min: f64,
    max: f64,
}

impl Named {
    fn value(&self, level: f64) -> f64 {
        self.min + level / 65535.0 * (self.max - self.min)
    }
}

/// The result of a job: the index, the levels, and the texels of each
/// feature.
type Done = Vec<(usize, Option<Levels>, Rects)>;

/// The levels of the features of one vector layer under one raster layer.
struct Entry {
    /// The geometry of the features that the levels are for.
    geoms: Arc<Vec<Geometry>>,
    levels: Vec<Option<Levels>>,
    rects: Vec<Rects>,
    /// The features to compute again.
    pending: BTreeSet<usize>,
    job: Option<mpsc::Receiver<Done>>,
    /// The geometry is the same as the layer.
    checked: bool,
}

impl Entry {
    fn new(geoms: Vec<Geometry>) -> Entry {
        let n = geoms.len();
        Entry {
            geoms: Arc::new(geoms),
            levels: vec![None; n],
            rects: vec![None; n],
            pending: (0..n).collect(),
            job: None,
            checked: true,
        }
    }

    /// Marks the features that a change of the raster reaches. `None`: all
    /// texels can be different.
    fn raster_changed(&mut self, changed: Option<&[(usize, TexelRect)]>) {
        for (i, rects) in self.rects.iter().enumerate() {
            let meets = match (changed, rects) {
                (Some(changed), Some(rects)) => rects_meet(rects, changed),
                _ => true,
            };
            if meets {
                self.pending.insert(i);
            }
        }
    }

    /// Compares the geometry with the features of `layer`, and marks the
    /// features that changed.
    fn check(&mut self, layer: &vale_store::Layer) {
        if self.checked {
            return;
        }
        self.checked = true;
        let features = &layer.features;
        let same = |(g, f): (&Geometry, &vale_store::Feature)| *g == f.geometry;
        if self.geoms.len() == features.len() && self.geoms.iter().zip(features).all(same) {
            return;
        }
        let geoms: Vec<Geometry> = features.iter().map(|f| f.geometry.clone()).collect();
        for (i, g) in geoms.iter().enumerate() {
            if self.geoms.get(i) != Some(g) {
                self.pending.insert(i);
            }
        }
        self.levels.resize(geoms.len(), None);
        self.rects.resize(geoms.len(), None);
        self.pending.retain(|&i| i < geoms.len());
        self.geoms = Arc::new(geoms);
    }
}

/// The values of the raster layers under the features of the vector
/// layers.
#[derive(Default)]
pub struct RasterCache {
    names: HashMap<String, Named>,
    /// A snapshot of each raster layer that an entry reads.
    maps: HashMap<RasterId, Heightmap>,
    entries: HashMap<(LayerId, RasterId), Entry>,
    /// The pairs that a read found with no entry.
    wanted: Mutex<HashSet<(LayerId, RasterId)>>,
    /// A frozen copy computes a missing value at the read and keeps it here.
    memo: Option<Mutex<Memo>>,
    /// The count of the jobs that finished.
    arrived: u64,
    /// The results of the jobs stay in their channels, for the tests.
    pub held: bool,
}

impl RasterCache {
    /// The values for the features of vector layer `layer`.
    pub fn for_layer(&self, layer: LayerId) -> LayerRasters<'_> {
        LayerRasters { cache: self, layer }
    }

    /// The value of raster layer `name` under feature `index` of vector
    /// layer `layer`, in the unit of the raster layer. `None` if no raster
    /// layer has the name, or if the value is not computed yet.
    pub fn value_of(&self, layer: LayerId, index: usize, name: &str, stat: Stat) -> Option<f64> {
        self.read(layer, name, stat, index, None)
    }

    /// The names of the raster layers, sorted.
    pub fn raster_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.names.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// True if a raster layer has the name.
    pub fn has_raster(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    fn read(
        &self,
        layer: LayerId,
        name: &str,
        stat: Stat,
        index: usize,
        shape: Option<&Geometry>,
    ) -> Option<f64> {
        let named = *self.names.get(name)?;
        let key = (layer, named.id);
        let entry = self.entries.get(&key);
        let kept = entry.and_then(|e| e.levels.get(index).copied().flatten());
        let levels = match kept {
            Some(levels) => levels,
            None => {
                if entry.is_none() && self.memo.is_none() {
                    self.wanted.lock().ok()?.insert(key);
                }
                let (shape, map) = (shape?, self.maps.get(&named.id)?);
                match &self.memo {
                    Some(memo) => *(memo.lock().ok()?)
                        .entry((layer, named.id, index))
                        .or_insert_with(|| compute(map, shape).0)
                        .as_ref()?,
                    // A point is cheap, so it gets its value at once.
                    None if matches!(shape, Geometry::Points(_)) => compute(map, shape).0?,
                    None => return None,
                }
            }
        };
        let level = match stat {
            Stat::Mean => levels[0],
            Stat::Min => levels[1],
            Stat::Max => levels[2],
        };
        Some(named.value(level))
    }

    /// Follows the document and the rasters. `map_of` gives the heightmap of
    /// a raster layer. Starts a job for each kept value that a change of a
    /// raster or of a feature reaches. It does not wait for the jobs.
    pub fn update<'m>(
        &mut self,
        doc: &Document,
        map_of: impl Fn(RasterId) -> Option<&'m Heightmap>,
    ) {
        let project = &doc.project;
        self.names = (project.layers().iter())
            .filter_map(|layer| {
                let raster = project.raster(layer.raster()?)?;
                let named = Named {
                    id: raster.id,
                    min: raster.value_min,
                    max: raster.value_max,
                };
                Some((layer.name.clone(), named))
            })
            .collect();
        let rasters: HashSet<RasterId> = self.names.values().map(|n| n.id).collect();
        let vector = |id: LayerId| doc.layer(id).filter(|l| l.geometry().is_some());
        self.entries
            .retain(|(layer, raster), _| vector(*layer).is_some() && rasters.contains(raster));
        let wanted = self
            .wanted
            .get_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        for key in wanted {
            if let (Some(layer), true) = (vector(key.0), rasters.contains(&key.1)) {
                let geoms = layer.features.iter().map(|f| f.geometry.clone()).collect();
                self.entries.entry(key).or_insert_with(|| Entry::new(geoms));
            }
        }
        let used: HashSet<RasterId> = self.entries.keys().map(|k| k.1).collect();
        self.maps.retain(|id, _| used.contains(id));
        for id in used {
            let Some(live) = map_of(id) else { continue };
            let changed = match self.maps.get(&id) {
                None => Some(None),
                Some(old) => match live.changed_tiles(old) {
                    None => Some(None),
                    Some(rects) if rects.is_empty() => None,
                    Some(rects) => Some(Some(rects)),
                },
            };
            let Some(changed) = changed else { continue };
            self.maps.insert(id, live.snapshot());
            for (_, entry) in self.entries.iter_mut().filter(|(k, _)| k.1 == id) {
                entry.raster_changed(changed.as_deref());
            }
        }
        for ((layer, raster), entry) in &mut self.entries {
            if let Some(layer) = doc.layer(*layer) {
                entry.check(layer);
            }
            let Some(map) = self.maps.get(raster) else {
                continue;
            };
            if entry.job.is_some() || entry.pending.is_empty() {
                continue;
            }
            let indices: Vec<usize> = std::mem::take(&mut entry.pending).into_iter().collect();
            entry.job = start(map.snapshot(), entry.geoms.clone(), indices);
        }
    }

    /// Takes the values that the jobs finished. `block`: waits for each
    /// running job. True while a job runs.
    pub fn poll(&mut self, block: bool) -> bool {
        let mut busy = false;
        for entry in self.entries.values_mut() {
            let Some(rx) = &entry.job else { continue };
            if self.held {
                busy = true;
                continue;
            }
            let msg = if block {
                rx.recv().map_err(|_| mpsc::TryRecvError::Disconnected)
            } else {
                rx.try_recv()
            };
            match msg {
                Ok(done) => {
                    for (i, levels, rects) in done {
                        if i < entry.levels.len() {
                            entry.levels[i] = levels;
                            entry.rects[i] = rects;
                        }
                    }
                    entry.job = None;
                    self.arrived += 1;
                    busy |= !entry.pending.is_empty();
                }
                Err(mpsc::TryRecvError::Empty) => busy = true,
                Err(mpsc::TryRecvError::Disconnected) => entry.job = None,
            }
        }
        busy || self.wanted.get_mut().is_ok_and(|w| !w.is_empty())
    }

    /// Compares the kept geometry of layer `id` with the layer at the next
    /// update.
    pub fn forget(&mut self, id: LayerId) {
        for (_, entry) in self.entries.iter_mut().filter(|(k, _)| k.0 == id) {
            entry.checked = false;
        }
    }

    /// Compares the kept geometry of each layer with the layer at the next
    /// update. An undo can change any layer.
    pub fn uncheck(&mut self) {
        for entry in self.entries.values_mut() {
            entry.checked = false;
        }
    }

    /// A number that changes when a job finishes.
    pub fn arrived(&self) -> u64 {
        self.arrived
    }

    /// A copy with the values and the rasters of now, for an export. It
    /// starts no jobs, and it computes a missing value at the read.
    pub fn frozen(&self) -> RasterCache {
        let entries = (self.entries.iter())
            .map(|(key, e)| {
                let entry = Entry {
                    geoms: e.geoms.clone(),
                    levels: e.levels.clone(),
                    rects: e.rects.clone(),
                    pending: BTreeSet::new(),
                    job: None,
                    checked: true,
                };
                (*key, entry)
            })
            .collect();
        RasterCache {
            names: self.names.clone(),
            maps: (self.maps.iter())
                .map(|(id, m)| (*id, m.snapshot()))
                .collect(),
            entries,
            memo: Some(Mutex::default()),
            ..RasterCache::default()
        }
    }
}

/// The raster values of the features of one vector layer.
#[derive(Clone, Copy)]
pub struct LayerRasters<'a> {
    cache: &'a RasterCache,
    layer: LayerId,
}

impl RasterValues for LayerRasters<'_> {
    fn value(&self, layer: &str, stat: Stat, index: usize, shape: &Geometry) -> Option<f64> {
        (self.cache).read(self.layer, layer, stat, index, Some(shape))
    }

    fn has_layer(&self, layer: &str) -> bool {
        self.cache.has_raster(layer)
    }
}

/// Starts a job that computes the levels of the features at `indices` on a
/// worker thread.
fn start(
    map: Heightmap,
    geoms: Arc<Vec<Geometry>>,
    indices: Vec<usize>,
) -> Option<mpsc::Receiver<Done>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let done: Done = (indices.into_iter())
            .filter_map(|i| {
                let (levels, rects) = compute(&map, geoms.get(i)?);
                Some((i, levels, rects))
            })
            .collect();
        // The send fails when the app dropped the job.
        let _ = tx.send(done);
    };
    std::thread::Builder::new()
        .name("vale-raster-values".to_string())
        .spawn(run)
        .ok()
        .map(|_| rx)
}

fn sample(map: &Heightmap, p: LonLat) -> f64 {
    map.sample_bilinear(lonlat_to_dir(p[0], p[1]))
}

/// The mean, the lowest, and the highest of `values`.
fn stats(values: impl Iterator<Item = f64>) -> Option<Levels> {
    let (mut sum, mut n) = (0.0, 0usize);
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        sum += v;
        n += 1;
        min = min.min(v);
        max = max.max(v);
    }
    (n > 0).then(|| [sum / n as f64, min, max])
}

/// The levels of the parts of a feature as one: the mean weighted by the
/// area or the length of each part.
fn combine(parts: &[Zonal]) -> (Option<Levels>, Rects) {
    let weight: f64 = parts.iter().map(|z| z.weight).sum();
    let mean = if weight > 0.0 {
        parts.iter().map(|z| z.mean * z.weight).sum::<f64>() / weight
    } else {
        parts.iter().map(|z| z.mean).sum::<f64>() / parts.len() as f64
    };
    let min = parts.iter().map(|z| z.min).fold(f64::INFINITY, f64::min);
    let max = parts
        .iter()
        .map(|z| z.max)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut rects: [Option<TexelRect>; FACES] = [None; FACES];
    for (face, rect) in rects.iter_mut().enumerate() {
        for r in parts.iter().filter_map(|z| z.rects[face]) {
            *rect = Some(rect.map_or(r, |o| union(o, r)));
        }
    }
    (Some([mean, min, max]), Some(rects))
}

fn union(a: TexelRect, b: TexelRect) -> TexelRect {
    TexelRect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// The levels of a feature, and the texels that they come from. A line or
/// a polygon with no texel under it gets the level at its center.
fn compute(map: &Heightmap, geometry: &Geometry) -> (Option<Levels>, Rects) {
    let center = match geometry {
        Geometry::Points(points) => {
            return (stats(points.iter().map(|p| sample(map, *p))), None);
        }
        Geometry::Lines(lines) => {
            let parts: Vec<Zonal> = lines.iter().filter_map(|l| line_zonal(map, l)).collect();
            if !parts.is_empty() {
                return combine(&parts);
            }
            line_center(lines)
        }
        Geometry::Polygons(polygons) => {
            let parts: Vec<Zonal> = (polygons.iter())
                .filter_map(|p| polygon_zonal(map, p))
                .collect();
            if !parts.is_empty() {
                return combine(&parts);
            }
            area_center(polygons)
        }
    };
    let level = center.map(|c| sample(map, c));
    (level.map(|l| [l, l, l]), None)
}
