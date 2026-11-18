//! The toolbox operation "Polygonize": a polygon for each band of a raster
//! layer. The limits of the bands are the color stops of the layer, or the
//! multiples of an interval.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use vale_store::{Feature, Geometry, GeometryKind, LayerId, LonLat, RasterId, Value};
use vale_style::{Color, LayerStyle};
use vale_terrain::{Bands, Contours, Heightmap, Rgb, Trace, polygonize};

use super::{AppState, is_height};
use crate::classify::{self, Class, Step};
use crate::globe::polygon::sphere_polygons;

/// The name of the operation in the toolbox.
pub const POLYGONIZE: &str = "Polygonize";
/// What the operation does, below its name on its page.
pub const POLYGONIZE_ABOUT: &str = "Makes a polygon for each band of a raster layer.";
/// The hint of the operation when the world has no raster layer.
pub const NEEDS_RASTER: &str = "Add a raster layer.";
pub const INPUT: &str = "Input";
pub const BREAKS: &str = "Breaks";
pub const COLOR_STOPS: &str = "Color stops";
pub const INTERVAL: &str = "Interval";
pub const BASE: &str = "Base";
pub const SMOOTH: &str = "Smooth";
pub const SIMPLIFY: &str = "Simplify";
/// The hint of breaks that give too many bands.
pub const TOO_MANY_BANDS: &str = "Too many bands. Use a larger interval.";
pub const SPLIT: &str = "Split into two layers at 0 m";
pub const ABOVE: &str = "Above";
pub const BELOW: &str = "Below";
/// The names of the output layers of a height layer.
pub const TOPOGRAPHY: &str = "Topography";
pub const BATHYMETRY: &str = "Bathymetry";

/// The width of the filter on the values at full Smooth, in texels.
const MAX_SIGMA: f64 = 2.0;
/// The range of Simplify: the largest distance of a removed vertex from the
/// outline, in texels.
pub const SIMPLIFY_RANGE: std::ops::RangeInclusive<f64> = 0.5..=16.0;
/// The most bands of a run.
pub const MAX_RUN_BANDS: usize = 256;

/// Where the limits of the bands come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Breaks {
    /// The color stops of the layer, with the colors and the names of its
    /// bands.
    Stops,
    /// Each multiple of `step` from `base`. A band has the color of the
    /// layer at its middle.
    Interval { step: f64, base: f64 },
}

/// The bands of a run: their limits, colors, and names.
#[derive(Clone, Debug, PartialEq)]
pub struct RunBands {
    /// The values of level 0 and of the highest level.
    min: f64,
    max: f64,
    /// The limits from `min` to `max`. Band `i` runs from limit `i` to
    /// limit `i + 1`.
    limits: Vec<f64>,
    colors: Vec<Rgb>,
    names: Vec<String>,
    /// The index of the limit at sea level, for the split.
    sea: Option<usize>,
}

impl RunBands {
    /// The bands of a layer with the bands `bands` and the breaks `breaks`.
    /// `None` for an interval that is not positive, or that gives more than
    /// `MAX_RUN_BANDS` bands.
    pub fn of(bands: &Bands, breaks: Breaks) -> Option<RunBands> {
        let (min, max) = (bands.min(), bands.max());
        let Breaks::Interval { step, base } = breaks else {
            return Some(RunBands {
                min,
                max,
                limits: bands.stops().iter().map(|s| s.value).collect(),
                colors: bands.colors().to_vec(),
                names: bands.names().to_vec(),
                sea: bands.sea(),
            });
        };
        if !(step > 0.0 && step.is_finite() && base.is_finite()) {
            return None;
        }
        let first = ((min - base) / step).floor() + 1.0;
        if (max - min) / step > MAX_RUN_BANDS as f64 {
            return None;
        }
        let mut limits = vec![min];
        let mut k = first;
        loop {
            let v = base + k * step;
            if v >= max {
                break;
            }
            if v > min {
                limits.push(v);
            }
            k += 1.0;
        }
        // The split at sea level needs a limit there.
        let split = bands.sea().is_some();
        if split && !limits.contains(&vale_terrain::SEA_LEVEL) {
            let at = limits.partition_point(|v| *v < vale_terrain::SEA_LEVEL);
            limits.insert(at, vale_terrain::SEA_LEVEL);
        }
        limits.push(max);
        if limits.len() - 1 > MAX_RUN_BANDS {
            return None;
        }
        let colors = (limits.windows(2))
            .map(|w| bands.color_at((w[0] + w[1]) / 2.0))
            .collect();
        let sea = split.then(|| limits.partition_point(|v| *v < vale_terrain::SEA_LEVEL));
        Some(RunBands {
            min,
            max,
            limits,
            colors,
            names: Vec::new(),
            sea,
        })
    }

    /// The number of bands.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    fn name(&self, band: usize) -> &str {
        self.names.get(band).map_or("", String::as_str)
    }
}

/// The inputs and the parameters of "Polygonize", and its last run.
pub struct PolygonizeState {
    /// The raster layer that the user chose. `None`, or a layer that is
    /// gone, gives the default layer.
    pub raster: Option<RasterId>,
    /// The smoothing, from 0 to 1.
    pub smooth: f64,
    /// The largest distance of a removed vertex from the outline, in
    /// texels.
    pub simplify: f64,
    pub breaks: Breaks,
    /// Split the output of a height layer at sea level.
    pub split: bool,
    pub above: String,
    pub below: String,
    pub last: Option<Run>,
    /// The run that the worker does.
    pub job: Option<Job>,
}

impl Default for PolygonizeState {
    fn default() -> Self {
        PolygonizeState {
            raster: None,
            smooth: 0.6,
            simplify: *SIMPLIFY_RANGE.start(),
            breaks: Breaks::Stops,
            split: true,
            above: TOPOGRAPHY.to_owned(),
            below: BATHYMETRY.to_owned(),
            last: None,
            job: None,
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

/// The level of each inner limit of `bands`, as a fraction of a level.
pub fn limit_levels(bands: &RunBands) -> Vec<f64> {
    let limits = &bands.limits;
    let span = bands.max - bands.min;
    (limits[1..limits.len() - 1].iter())
        .map(|v| (v - bands.min) / span * 65535.0)
        .collect()
}

/// The polygons of one band.
fn band_polygons(contours: &Contours, band: usize) -> Vec<Vec<Vec<LonLat>>> {
    if contours.is_whole(band) {
        let world = vec![
            [-180.0, -90.0],
            [180.0, -90.0],
            [180.0, 90.0],
            [-180.0, 90.0],
            [-180.0, -90.0],
        ];
        return vec![vec![world]];
    }
    sphere_polygons(contours.band_rings(band))
}

/// The names of the output layers: the layer above the split, and the
/// index of the band above the split with the name of the layer below it,
/// if the output splits.
#[derive(Clone, Debug)]
pub struct Names {
    pub above: String,
    pub split: Option<(usize, String)>,
}

/// The number of threads that make the polygons. Each thread holds the
/// copies of the rings of one band.
const MAKE_THREADS: usize = 4;

/// The share of the progress of a run that the trace takes. The polygons
/// in longitude and latitude take the rest.
const TRACE_SHARE: f64 = 0.6;

/// The output layers of a run: one feature for each band that has an
/// area. `progress` gets the share of the work that is done, from 0 to 1.
/// `None` if `stop` became true.
pub fn outputs(
    map: &Heightmap,
    bands: &RunBands,
    height: bool,
    trace: Trace,
    names: &Names,
    progress: &(dyn Fn(f64) + Sync),
    stop: &AtomicBool,
) -> Option<Vec<Output>> {
    let traced = |share: f64| {
        progress(share * TRACE_SHARE);
        !stop.load(Ordering::Relaxed)
    };
    let contours = polygonize(map, &limit_levels(bands), trace, &traced)?;
    // The polygons of the bands, on all cores. Each thread takes the next
    // band that no thread has.
    let count = contours.bands_len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let polygons: Vec<Mutex<Vec<Vec<Vec<LonLat>>>>> =
        (0..count).map(|_| Mutex::new(Vec::new())).collect();
    std::thread::scope(|s| {
        for _ in 0..MAKE_THREADS.min(count) {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= count || stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let made = band_polygons(&contours, k);
                    *polygons[k].lock().expect("polygons") = made;
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(TRACE_SHARE + (1.0 - TRACE_SHARE) * finished as f64 / count as f64);
                }
            });
        }
    });
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    drop(contours);
    let (min_field, max_field) = limit_fields(height);
    let limits = &bands.limits;
    let layer = |name: &str| Output {
        name: name.to_owned(),
        features: Vec::new(),
        bands: Vec::new(),
    };
    let mut out = vec![layer(&names.above)];
    if let Some((_, below)) = &names.split {
        out.insert(0, layer(below));
    }
    for (k, polygons) in polygons.into_iter().enumerate() {
        let polygons = polygons.into_inner().expect("polygons");
        if polygons.is_empty() {
            continue;
        }
        let attributes = BTreeMap::from([
            (min_field.to_owned(), Value::Number(limits[k])),
            (max_field.to_owned(), Value::Number(limits[k + 1])),
        ]);
        let index = match names.split {
            Some((sea, _)) if k >= sea => 1,
            _ => 0,
        };
        let index = index.min(out.len() - 1);
        out[index]
            .features
            .push(Feature::new(Geometry::Polygons(polygons), attributes));
        out[index].bands.push(k);
    }
    Some(out)
}

/// A rule for each band, with the color of the band.
pub fn band_rules(bands: &RunBands, height: bool, which: &[usize]) -> Vec<vale_style::Rule> {
    let (field, _) = limit_fields(height);
    let limits = &bands.limits;
    let classes: Vec<Class> = (which.iter())
        .map(|&k| {
            let lo = classify::number_text(limits[k]);
            let hi = classify::number_text(limits[k + 1]);
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
            let [r, g, b] = bands.colors[k];
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

/// A distance on the ground, in meters or in kilometers.
pub fn distance_text(meters: f64) -> String {
    if meters < 1000.0 {
        format!("{} m", crate::color_key::number_text(meters.round()))
    } else {
        format!(
            "{} km",
            crate::color_key::number_text((meters / 100.0).round() / 10.0)
        )
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

    /// The bands of a run of "Polygonize" on its input with its breaks.
    /// `None` with no input, or with breaks that give no bands.
    pub fn polygonize_bands(&mut self) -> Option<RunBands> {
        let id = self.polygonize_input()?;
        RunBands::of(&self.raster_bands(id)?, self.polygonize.breaks)
    }

    /// The size of one texel of the input of "Polygonize" on the ground, in
    /// meters, at the middle of a face.
    pub fn polygonize_texel_m(&self) -> Option<f64> {
        let id = self.polygonize_input()?;
        let face = self.doc.project.raster(id)?.face_size;
        let radius_m = self.doc.project.world.radius_km * 1000.0;
        Some(std::f64::consts::FRAC_PI_2 * radius_m / f64::from(face))
    }

    /// The unit of the values of the input of "Polygonize", with a space
    /// before it, or nothing.
    pub fn polygonize_unit(&self) -> &'static str {
        let height = (self.polygonize_input())
            .and_then(|id| self.doc.project.raster(id))
            .is_some_and(is_height);
        if height { " m" } else { "" }
    }

    /// Breaks at an interval of about a tenth of the range of the input,
    /// rounded to 1, 2, or 5 times a power of ten, from 0.
    pub fn default_interval(&mut self) -> Breaks {
        let (min, max) = self.polygonize_range().unwrap_or((0.0, 1000.0));
        let rough = ((max - min) / 10.0).max(1e-9);
        let power = 10f64.powf(rough.log10().floor());
        let step = [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|m| m * power)
            .find(|s| *s >= rough)
            .unwrap_or(10.0 * power);
        Breaks::Interval { step, base: 0.0 }
    }

    /// A value range that the breaks of "Polygonize" can cover: the range
    /// of the bands of its input.
    pub fn polygonize_range(&mut self) -> Option<(f64, f64)> {
        let id = self.polygonize_input()?;
        let bands = self.raster_bands(id)?;
        Some((bands.min(), bands.max()))
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
    pub fn can_polygonize(&mut self) -> bool {
        let idle = self.polygonize.job.is_none() && !self.globe.busy();
        idle && self.polygonize_bands().is_some()
    }

    /// Runs "Polygonize" from the toolbox, and keeps the result for the
    /// note of the toolbox.
    pub fn run_polygonize(&mut self) {
        if self.polygonize.job.is_some() || self.globe.busy() {
            return;
        }
        let Some(job) = self.start_polygonize() else {
            return;
        };
        self.polygonize.job = Some(job);
        self.toolbox_note = None;
        if self.headless {
            self.poll_polygonize(true);
        }
    }

    /// Stops the run of "Polygonize" that the worker does.
    pub fn cancel_polygonize(&mut self) {
        if let Some(job) = &self.polygonize.job {
            job.stop.store(true, Ordering::Relaxed);
        }
    }

    /// The progress of the run of "Polygonize", from 0 to 1, and its text.
    /// `None` when no run works.
    pub fn polygonize_progress(&self) -> Option<(f32, String)> {
        let job = self.polygonize.job.as_ref()?;
        let share = job.share();
        let text = match job.phase() {
            Phase::Read => "Reading the layer…".to_owned(),
            Phase::Trace => format!("Tracing the bands… {}%", (share * 100.0).round()),
            Phase::Make => format!("Making the polygons… {}%", (share * 100.0).round()),
        };
        Some((share, text))
    }

    /// Takes the result of the worker of "Polygonize", if it is done, and
    /// adds its layers. Call it in each frame. `block`: waits for the
    /// worker.
    pub fn poll_polygonize(&mut self, block: bool) {
        let Some(job) = &self.polygonize.job else {
            return;
        };
        // A stroke that is not done keeps the layers for a later frame.
        if self.globe.busy() && !block {
            return;
        }
        let result = match block {
            true => job.rx.recv().ok(),
            false => match job.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => None,
            },
        };
        let job = self.polygonize.job.take().expect("a job");
        match result {
            Some(Ok(Some(layers))) => {
                let time = job.start.elapsed();
                let ids = self.add_polygonize_layers(job, layers, time);
                let names: Vec<String> = (ids.iter())
                    .filter_map(|id| Some(self.doc.layer(*id)?.name.clone()))
                    .collect();
                self.toolbox_note = Some(format!(
                    "Made {} in {}.",
                    names.join(" and "),
                    time_text(time)
                ));
                crate::log_file::write(&format!(
                    "polygonize: {:?}; {}",
                    time,
                    crate::log_file::memory()
                ));
            }
            Some(Ok(None)) => self.toolbox_note = Some(STOPPED.to_owned()),
            Some(Err(e)) => self.toolbox_note = Some(e),
            None => {
                self.toolbox_note = Some(format!("Could not polygonize: {}", "the run stopped"))
            }
        }
    }

    /// Runs "Polygonize" on its input and waits for the result. Returns the
    /// new layers.
    pub fn polygonize(&mut self) -> Option<Vec<LayerId>> {
        let job = self.start_polygonize()?;
        self.polygonize.job = Some(job);
        self.poll_polygonize(true);
        Some(self.polygonize_outputs())
    }

    /// Starts the worker of "Polygonize" on its input.
    fn start_polygonize(&mut self) -> Option<Job> {
        let raster = self.polygonize_input()?;
        if self.globe.busy() {
            return None;
        }
        let height = self.doc.project.raster(raster).is_some_and(is_height);
        let name = self.doc.project.raster_layer(raster)?.name.clone();
        let input = self.raster_bands(raster)?;
        let bands = RunBands::of(&input, self.polygonize.breaks)?;
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
        let names = Names {
            above,
            split: bands.sea.filter(|_| split).map(|s| (s, below)),
        };
        // The worker reads the whole heightmap: a snapshot of the tiles that
        // the globe holds for the active layer, or the tiles of the project
        // file for another layer.
        let source = match self.globe.raster_map(raster) {
            Some(map) => Source::Map(Box::new(map.snapshot())),
            None => {
                self.save_edit();
                let record = self.doc.project.raster(raster)?.clone();
                let database = self.session.as_ref()?.database().to_path_buf();
                Source::File(database, record)
            }
        };
        // The snapshot to compare later is of the heightmap that the globe
        // holds, whole or reduced.
        let held = self
            .globe
            .raster_map(raster)
            .or(self.globe.value_map(raster));
        let levels = held?.snapshot();
        Some(Job::start(
            raster,
            source,
            Params {
                input,
                bands,
                height,
                trace: Trace {
                    sigma: self.polygonize.smooth.clamp(0.0, 1.0) * MAX_SIGMA,
                    tolerance: self.polygonize.simplify,
                },
                names,
            },
            levels,
        ))
    }

    /// Adds the output layers of a run, as one undo step. They replace the
    /// outputs of the last run on the same input. Returns the new layers.
    fn add_polygonize_layers(
        &mut self,
        job: Job,
        layers: Vec<Output>,
        time: Duration,
    ) -> Vec<LayerId> {
        let (raster, bands, height) = (job.raster, job.bands, job.height);
        let input = job.input;
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
            levels: job.levels,
            bands: input,
            outputs: ids.clone(),
            time,
        });
        ids
    }
}

/// The note of a run that the user stopped.
pub const STOPPED: &str = "Stopped.";
pub const CANCEL: &str = "Cancel";

/// Where the worker gets the heightmap.
enum Source {
    Map(Box<Heightmap>),
    File(PathBuf, vale_store::Raster),
}

/// The parameters of a run.
struct Params {
    /// The bands of the input layer.
    input: Bands,
    bands: RunBands,
    height: bool,
    trace: Trace,
    names: Names,
}

/// The step of a run that the worker does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Read,
    Trace,
    Make,
}

type JobResult = Result<Option<Vec<Output>>, String>;

/// A run of "Polygonize" on a worker thread.
pub struct Job {
    raster: RasterId,
    input: Bands,
    bands: RunBands,
    height: bool,
    levels: Heightmap,
    start: Instant,
    rx: mpsc::Receiver<JobResult>,
    /// The share of the work that is done, in thousandths.
    done: Arc<AtomicU32>,
    /// The worker reads the file while this is true.
    reading: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Job {
    fn start(raster: RasterId, source: Source, params: Params, levels: Heightmap) -> Job {
        let (tx, rx) = mpsc::sync_channel(1);
        let done = Arc::new(AtomicU32::new(0));
        let reading = Arc::new(AtomicBool::new(matches!(source, Source::File(..))));
        let stop = Arc::new(AtomicBool::new(false));
        let (input, bands, height) = (params.input.clone(), params.bands.clone(), params.height);
        let worker = {
            let (done, reading, stop) = (done.clone(), reading.clone(), stop.clone());
            move || {
                let map = match source {
                    Source::Map(map) => *map,
                    Source::File(database, record) => {
                        match crate::session::Session::read_map(&database, &record, &stop) {
                            Ok(Some(map)) => map,
                            Ok(None) => {
                                let _ = tx.send(Ok(None));
                                return;
                            }
                            Err(e) => {
                                let _ = tx.send(Err(format!("Could not read the layer: {e}")));
                                return;
                            }
                        }
                    }
                };
                reading.store(false, Ordering::Relaxed);
                let progress = |share: f64| {
                    let share = (share.clamp(0.0, 1.0) * 1000.0) as u32;
                    done.fetch_max(share, Ordering::Relaxed);
                };
                let p = &params;
                let out = outputs(
                    &map, &p.bands, p.height, p.trace, &p.names, &progress, &stop,
                );
                let _ = tx.send(Ok(out));
            }
        };
        let spawned = std::thread::Builder::new()
            .name("vale-polygonize".to_string())
            .spawn(worker);
        // With no thread, the channel has no sender, and the job fails.
        drop(spawned);
        Job {
            raster,
            input,
            bands,
            height,
            levels,
            start: Instant::now(),
            rx,
            done,
            reading,
            stop,
        }
    }

    /// The share of the work that is done, from 0 to 1.
    pub fn share(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn phase(&self) -> Phase {
        if self.reading.load(Ordering::Relaxed) {
            Phase::Read
        } else if f64::from(self.share()) < TRACE_SHARE {
            Phase::Trace
        } else {
            Phase::Make
        }
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
