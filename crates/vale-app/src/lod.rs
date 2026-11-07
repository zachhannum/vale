//! The levels of detail of the vector layers, built off the UI thread. The
//! views draw the level for their scale. Edits, snapping, the attribute
//! table, pick, and export read the full geometry of the layer.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vale_sphere::Projection;
use vale_sphere::lod::{self, LEVELS, Shape, Simple};
use vale_store::{Geometry, Layer, LayerId};

/// A layer with fewer points draws its full geometry.
pub const MIN_POINTS: usize = 20_000;

/// The time from an edit of a layer to the start of its build. A drag
/// changes the layer in each frame, and it starts no build until it stops.
pub const QUIET: Duration = Duration::from_millis(500);

/// The geometry of one feature at a level.
enum Part {
    Same,
    Gone,
    Geometry(Geometry),
}

/// The features of a layer at one level.
pub struct Detail {
    pub index: usize,
    features: Vec<Part>,
}

impl Detail {
    fn new(level: lod::Level) -> Detail {
        let features = (level.features.into_iter())
            .map(|f| match f {
                Simple::Same => Part::Same,
                Simple::Gone => Part::Gone,
                Simple::Lines(lines) => Part::Geometry(Geometry::Lines(lines)),
                Simple::Polygons(polygons) => Part::Geometry(Geometry::Polygons(polygons)),
            })
            .collect();
        Detail {
            index: level.index,
            features,
        }
    }

    /// The number of points of the features at this level, with `full` for
    /// the features that keep their geometry.
    pub fn points(&self, full: &Layer) -> usize {
        (full.features.iter().enumerate())
            .filter_map(|(i, f)| geometry(Some(self), i, &f.geometry))
            .map(points)
            .sum()
    }
}

/// The geometry of feature `i` at `detail`, or `full` with no detail.
/// `None`: the feature does not draw at this level.
pub fn geometry<'a>(
    detail: Option<&'a Detail>,
    i: usize,
    full: &'a Geometry,
) -> Option<&'a Geometry> {
    match detail.and_then(|d| d.features.get(i)) {
        None | Some(Part::Same) => Some(full),
        Some(Part::Gone) => None,
        Some(Part::Geometry(g)) => Some(g),
    }
}

/// The address of a detail, for the keys of the caches. 0 is the full
/// geometry.
pub fn key(detail: Option<&Arc<Detail>>) -> usize {
    detail.map_or(0, |d| Arc::as_ptr(d) as usize)
}

pub fn points(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Points(p) => p.len(),
        Geometry::Lines(lines) => lines.iter().map(Vec::len).sum(),
        Geometry::Polygons(polygons) => polygons.iter().flatten().map(Vec::len).sum(),
    }
}

/// The size of one pixel in radians at `p`, for a projection with
/// `pixels` pixels per projected meter. The scale of a projection in
/// north-south can differ from its scale `s` in east-west. It is at most
/// the larger of `s` and `1 / s` in the projections of the app, so the
/// pixel uses that bound.
pub fn projected_pixel(projection: &Projection, p: [f64; 2], pixels: f64) -> Option<f64> {
    let s = projection.local_scale(p)?;
    Some(1.0 / (pixels * s.max(1.0 / s) * projection.radius_m()))
}

/// The levels of one layer.
struct Entry {
    /// The geometry that the build reads.
    snapshot: Arc<Vec<Geometry>>,
    levels: [Option<Arc<Detail>>; LEVELS],
    job: Option<mpsc::Receiver<Detail>>,
    /// The snapshot is the same as the layer. After an undo, the next use
    /// compares them.
    checked: bool,
}

/// The levels of the vector layers.
pub struct LodCache {
    entries: HashMap<LayerId, Entry>,
    builds: HashMap<LayerId, usize>,
    /// The time of the last edit of each layer that has no entry.
    edited: HashMap<LayerId, Instant>,
    /// A layer waits for the quiet time to end.
    waiting: bool,
    /// The count of the levels that arrived.
    arrived: u64,
    pub min_points: usize,
    pub quiet: Duration,
    /// The levels that the workers finish stay in their channels, for the
    /// tests.
    pub held: bool,
}

impl Default for LodCache {
    fn default() -> LodCache {
        LodCache {
            entries: HashMap::new(),
            builds: HashMap::new(),
            edited: HashMap::new(),
            waiting: false,
            arrived: 0,
            min_points: MIN_POINTS,
            quiet: QUIET,
            held: false,
        }
    }
}

impl LodCache {
    /// The level of `layer` to draw for a pixel of `pixel` radians. `None`:
    /// the full geometry. The first use starts the build of the levels.
    pub fn level(&mut self, layer: &Layer, pixel: f64) -> Option<Arc<Detail>> {
        let entry = self.entry(layer)?;
        let ready = std::array::from_fn(|l| entry.levels[l].is_some());
        let index = lod::choose(pixel, ready)?;
        entry.levels[index].clone()
    }

    /// The entry of `layer`, up to date or with a new build.
    fn entry(&mut self, layer: &Layer) -> Option<&mut Entry> {
        let fresh = self.entries.get_mut(&layer.id).is_some_and(|e| {
            if e.snapshot.len() != layer.features.len() {
                return false;
            }
            if !e.checked {
                e.checked = (e.snapshot.iter().zip(&layer.features)).all(|(g, f)| *g == f.geometry);
            }
            e.checked
        });
        if !fresh {
            self.entries.remove(&layer.id);
            let lines = (layer.features.iter()).any(|f| !matches!(f.geometry, Geometry::Points(_)));
            let count: usize = layer.features.iter().map(|f| points(&f.geometry)).sum();
            if !lines || count < self.min_points {
                return None;
            }
            if let Some(at) = self.edited.get(&layer.id) {
                if at.elapsed() < self.quiet {
                    self.waiting = true;
                    return None;
                }
                self.edited.remove(&layer.id);
            }
            let snapshot: Arc<Vec<Geometry>> =
                Arc::new(layer.features.iter().map(|f| f.geometry.clone()).collect());
            let job = start(snapshot.clone());
            *self.builds.entry(layer.id).or_default() += 1;
            self.entries.insert(
                layer.id,
                Entry {
                    snapshot,
                    levels: Default::default(),
                    job,
                    checked: true,
                },
            );
        }
        self.entries.get_mut(&layer.id)
    }

    /// Takes the levels that the workers finished. `block`: waits for each
    /// running build. True while a build runs or waits.
    pub fn poll(&mut self, block: bool) -> bool {
        let mut busy = std::mem::take(&mut self.waiting);
        for entry in self.entries.values_mut() {
            let Some(rx) = &entry.job else { continue };
            if self.held {
                busy = true;
                continue;
            }
            loop {
                let msg = if block {
                    rx.recv().map_err(|_| mpsc::TryRecvError::Disconnected)
                } else {
                    rx.try_recv()
                };
                match msg {
                    Ok(detail) => {
                        let index = detail.index;
                        entry.levels[index] = Some(Arc::new(detail));
                        self.arrived += 1;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        busy = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        entry.job = None;
                        break;
                    }
                }
            }
        }
        busy
    }

    /// Builds the levels of layer `id` again at its next use, after the
    /// quiet time. The app drops the levels of a running build.
    pub fn forget(&mut self, id: LayerId) {
        self.entries.remove(&id);
        self.edited.insert(id, Instant::now());
    }

    /// Compares each layer with its levels at its next use. An undo can
    /// change any layer.
    pub fn uncheck(&mut self) {
        for entry in self.entries.values_mut() {
            entry.checked = false;
        }
    }

    pub fn retain(&mut self, keep: impl Fn(LayerId) -> bool) {
        self.entries.retain(|id, _| keep(*id));
        self.edited.retain(|id, _| keep(*id));
    }

    /// A number that changes when a level arrives.
    pub fn arrived(&self) -> u64 {
        self.arrived
    }

    /// The number of builds of layer `id` since the start.
    pub fn builds(&self, id: LayerId) -> usize {
        self.builds.get(&id).copied().unwrap_or(0)
    }

    /// The indices of the ready levels of layer `id`.
    pub fn ready(&self, id: LayerId) -> Vec<usize> {
        self.entries.get(&id).map_or(Vec::new(), |e| {
            (0..LEVELS).filter(|&l| e.levels[l].is_some()).collect()
        })
    }
}

/// Starts the build of the levels of `snapshot` on a worker thread. The
/// worker sends each level when it is ready.
fn start(snapshot: Arc<Vec<Geometry>>) -> Option<mpsc::Receiver<Detail>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let shapes: Vec<Shape> = (snapshot.iter())
            .map(|g| match g {
                Geometry::Points(_) => Shape::Points,
                Geometry::Lines(lines) => Shape::Lines(lines),
                Geometry::Polygons(polygons) => Shape::Polygons(polygons),
            })
            .collect();
        let mut open = true;
        lod::build(&shapes, |level| {
            // The send fails when the app dropped the build.
            if open {
                open = tx.send(Detail::new(level)).is_ok();
            }
        });
    };
    std::thread::Builder::new()
        .name("vale-lod".to_string())
        .spawn(run)
        .ok()
        .map(|_| rx)
}
