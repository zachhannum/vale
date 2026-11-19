//! The area of the world at each level of a raster layer, for the page of
//! the color stops. A worker thread computes the areas again after a change
//! to the levels. A read never waits for the worker: it gets the old areas
//! until the new ones arrive.
//!
//! The areas inside the shape of a map frame come from a grid of cells on
//! the page, so a share is a share of the area of the frame.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use kurbo::Point;
use vale_sphere::{LonLat, Projection, ProjectionSpec};
use vale_store::{FrameId, FrameRecord, FrameShape, RasterId};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{Bands, Heightmap, level_areas};

use crate::atlas::{FrameTarget, contains, frame_transform, shape_bbox};

/// The number of level bins.
pub const AREA_BINS: usize = 4096;
/// The largest face that the worker reads, in texels.
const MOST_FACE: usize = 1024;

/// The number of cells of the grid of a frame along the long side of the
/// box of its shape.
const FRAME_CELLS: usize = 256;

/// The result of a job, and the running job.
struct Slot<T> {
    done: Option<Arc<T>>,
    job: Option<mpsc::Receiver<T>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            done: None,
            job: None,
        }
    }
}

impl<T> Slot<T> {
    /// Takes the result of a finished job. `block`: waits for it. True while
    /// the job runs.
    fn poll(&mut self, block: bool) -> bool {
        let Some(rx) = &self.job else {
            return false;
        };
        let msg = if block {
            rx.recv().map_err(|_| mpsc::TryRecvError::Disconnected)
        } else {
            rx.try_recv()
        };
        match msg {
            Ok(done) => {
                self.done = Some(Arc::new(done));
                self.job = None;
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                false
            }
        }
    }
}

#[derive(Default)]
struct Entry {
    /// The copy of the map that the areas come from, or that the running
    /// job reads.
    input: Option<Heightmap>,
    /// The area of each level bin, in steradians.
    bins: Slot<Vec<f64>>,
}

/// The level areas inside the shape of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameBins {
    /// The number of cells of the grid at each level bin.
    pub bins: Vec<f64>,
    /// The lowest and the highest level in the frame. With no cell, both
    /// are 0.
    pub lo: u16,
    pub hi: u16,
}

impl FrameBins {
    /// True if no cell of the grid shows the map.
    pub fn is_empty(&self) -> bool {
        self.bins.iter().all(|b| *b <= 0.0)
    }
}

/// The parts of a frame and of its map that set the levels in the frame.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameKey {
    shape: FrameShape,
    center: LonLat,
    scale: f64,
    rotation: f64,
    spec: ProjectionSpec,
    radius_m: f64,
}

impl FrameKey {
    pub fn new(frame: &FrameRecord, spec: ProjectionSpec, radius_m: f64) -> FrameKey {
        FrameKey {
            shape: frame.shape.clone(),
            center: frame.center,
            scale: frame.scale,
            rotation: frame.rotation,
            spec,
            radius_m,
        }
    }
}

#[derive(Default)]
struct FrameEntry {
    /// The frame and the copy of the map of the last job.
    key: Option<FrameKey>,
    input: Option<Heightmap>,
    bins: Slot<FrameBins>,
}

/// The level areas of the raster layers.
#[derive(Default)]
pub struct Areas {
    entries: HashMap<RasterId, Entry>,
    frames: HashMap<(FrameId, RasterId), FrameEntry>,
}

impl Areas {
    /// The area of each level bin of the layer `id`, whose levels are
    /// `live`. Starts a job if the levels changed since the last job.
    pub fn bins(&mut self, id: RasterId, live: &Heightmap) -> Option<Arc<Vec<f64>>> {
        let entry = self.entries.entry(id).or_default();
        let busy = entry.bins.poll(false);
        let changed = !entry.input.as_ref().is_some_and(|m| m.same_tiles(live));
        if changed && !busy {
            let map = live.snapshot();
            let input = map.snapshot();
            entry.bins.job = start(move || level_areas(&input, AREA_BINS, MOST_FACE));
            entry.input = Some(map);
        }
        entry.bins.done.clone()
    }

    /// The level areas of layer `id` inside the shape of frame `frame`.
    /// Starts a job if the frame or the levels changed since the last job,
    /// unless `wait`.
    pub fn frame_bins(
        &mut self,
        frame: FrameId,
        id: RasterId,
        key: FrameKey,
        live: &Heightmap,
        wait: bool,
    ) -> Option<Arc<FrameBins>> {
        let entry = self.frames.entry((frame, id)).or_default();
        let busy = entry.bins.poll(false);
        let changed = entry.key.as_ref() != Some(&key)
            || !entry.input.as_ref().is_some_and(|m| m.same_tiles(live));
        if changed && !busy && !wait {
            let map = live.snapshot();
            let (input, job) = (map.snapshot(), key.clone());
            entry.bins.job = start(move || frame_levels(&input, &job));
            entry.input = Some(map);
            entry.key = Some(key);
        }
        entry.bins.done.clone()
    }

    /// Takes the areas of the finished jobs. `block`: waits for each
    /// running job. True while a job runs.
    pub fn poll(&mut self, block: bool) -> bool {
        let mut busy = false;
        for entry in self.entries.values_mut() {
            busy |= entry.bins.poll(block);
        }
        for entry in self.frames.values_mut() {
            busy |= entry.bins.poll(block);
        }
        busy
    }

    /// Keeps only the layers for which `keep` is true.
    pub fn retain(&mut self, mut keep: impl FnMut(RasterId) -> bool) {
        self.entries.retain(|id, _| keep(*id));
        self.frames.retain(|(_, id), _| keep(*id));
    }
}

/// Starts a job on a worker thread.
fn start<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<mpsc::Receiver<T>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let _ = tx.send(work());
    };
    std::thread::Builder::new()
        .name("vale-areas".to_string())
        .spawn(run)
        .ok()
        .map(|_| rx)
}

/// The level areas inside the shape of a frame. The center of each cell of
/// a grid on the page that is inside the shape and on the map adds 1 to the
/// bin of its level.
pub fn frame_levels(map: &Heightmap, key: &FrameKey) -> FrameBins {
    let mut out = FrameBins {
        bins: vec![0.0; AREA_BINS],
        lo: u16::MAX,
        hi: 0,
    };
    let frame = FrameRecord {
        id: FrameId(0),
        map: vale_store::MapId(0),
        shape: key.shape.clone(),
        center: key.center,
        scale: key.scale,
        rotation: key.rotation,
        graticule: Default::default(),
        neatline: Default::default(),
        border: Default::default(),
    };
    let bbox = shape_bbox(&frame.shape);
    let step = bbox.width().max(bbox.height()) / FRAME_CELLS as f64;
    let Ok(projection) = Projection::new(key.spec, key.radius_m) else {
        out.lo = 0;
        return out;
    };
    if step > 0.0 {
        let vt = frame_transform(&projection, &frame, FrameTarget::MM);
        let (nx, ny) = (
            (bbox.width() / step).ceil() as usize,
            (bbox.height() / step).ceil() as usize,
        );
        for j in 0..ny {
            for i in 0..nx {
                let p = Point::new(
                    bbox.x0 + (i as f64 + 0.5) * step,
                    bbox.y0 + (j as f64 + 0.5) * step,
                );
                if !contains(&frame.shape, p) {
                    continue;
                }
                let Some([lon, lat]) = projection.inverse(vt.from_page(p)) else {
                    continue;
                };
                let level = map.sample(lonlat_to_dir(lon, lat));
                out.bins[(usize::from(level) * AREA_BINS) >> 16] += 1.0;
                out.lo = out.lo.min(level);
                out.hi = out.hi.max(level);
            }
        }
    }
    if out.is_empty() {
        (out.lo, out.hi) = (0, 0);
    }
    out
}

/// The place of a value in the bins, from 0 to the number of bins.
fn bin_place(bins: &[f64], bands: &Bands, value: f64) -> f64 {
    let t = (value - bands.min()) / (bands.max() - bands.min());
    t.clamp(0.0, 1.0) * bins.len() as f64
}

/// The area of the bins below a place. The bin at the place counts by the
/// part of it below the place.
fn area_below(bins: &[f64], place: f64) -> f64 {
    let whole = (place.floor() as usize).min(bins.len());
    let part = bins.get(whole).map_or(0.0, |b| b * (place - whole as f64));
    bins[..whole].iter().sum::<f64>() + part
}

/// The area of each band, from the lowest band.
pub fn band_areas(bins: &[f64], bands: &Bands) -> Vec<f64> {
    let below: Vec<f64> = (bands.stops().iter())
        .map(|s| area_below(bins, bin_place(bins, bands, s.value)))
        .collect();
    below.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect()
}

/// The area of each of `count` columns of even width in value, from the
/// lowest value.
pub fn columns(bins: &[f64], count: usize) -> Vec<f64> {
    columns_between(bins, count, 0.0, bins.len() as f64)
}

/// `columns` from the place `from` in the bins to the place `to`.
pub fn columns_between(bins: &[f64], count: usize, from: f64, to: f64) -> Vec<f64> {
    let below: Vec<f64> = (0..=count)
        .map(|c| area_below(bins, from + c as f64 / count as f64 * (to - from)))
        .collect();
    below.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect()
}

/// The share of `part` in `total`, as a whole percent. A share under one
/// percent is "<1%".
pub fn share_text(part: f64, total: f64) -> String {
    if part <= 0.0 || total <= 0.0 {
        return "0%".to_string();
    }
    let percent = 100.0 * part / total;
    if percent < 1.0 {
        "<1%".to_string()
    } else {
        format!("{}%", percent.round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands(values: &[f64]) -> Bands {
        let mut bands = Bands::with_range(0.0, 100.0, false).unwrap();
        while bands.stops().len() > 2 {
            bands.remove(1);
        }
        for v in values {
            bands.add(*v).unwrap();
        }
        bands
    }

    #[test]
    fn band_areas_split_the_bin_at_a_stop() {
        // Four bins of 25 each, and a stop in the middle of the second bin.
        let bins = [1.0, 2.0, 3.0, 4.0];
        let areas = band_areas(&bins, &bands(&[37.5]));
        assert_eq!(areas, vec![2.0, 8.0]);
        let areas = band_areas(&bins, &bands(&[25.0, 50.0, 75.0]));
        assert_eq!(areas, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn share_text_shows_under_one_percent() {
        assert_eq!(share_text(0.0, 10.0), "0%");
        assert_eq!(share_text(0.05, 10.0), "<1%");
        assert_eq!(share_text(0.1, 10.0), "1%");
        assert_eq!(share_text(5.0, 10.0), "50%");
        assert_eq!(share_text(1.0, 3.0), "33%");
    }

    /// A frame of 200 by 100 mm at 1:100,000,000 on an equirectangular map
    /// of the Earth.
    fn frame_key(center: LonLat) -> FrameKey {
        let spec = ProjectionSpec::new(vale_sphere::ProjectionKind::Equirectangular, 0.0, 0.0);
        FrameKey {
            shape: FrameShape::Rectangle([0.0, 0.0, 200.0, 100.0]),
            center,
            scale: 1e8,
            rotation: 0.0,
            spec,
            radius_m: 6_371_000.0,
        }
    }

    #[test]
    fn the_grid_of_a_frame_counts_the_cells_at_each_level() {
        let (north, south) = (40_000, 20_000);
        let mut map = Heightmap::new(64, 0);
        for face in 0..6 {
            for (x, y) in (0..64).flat_map(|y| (0..64).map(move |x| (x, y))) {
                let level = if map.texel_dir(face, x, y)[2] > 0.0 {
                    north
                } else {
                    south
                };
                map.set(face, x, y, level);
            }
        }
        let bin = |level: u16| (usize::from(level) * AREA_BINS) >> 16;

        // The frame shows 90° of latitude around the equator: one half of
        // its cells is north and one half is south.
        let got = frame_levels(&map, &frame_key([0.0, 0.0]));
        let cells = (FRAME_CELLS * FRAME_CELLS / 2) as f64;
        assert_eq!(got.bins.iter().sum::<f64>(), cells);
        assert_eq!(got.bins[bin(north)], cells / 2.0);
        assert_eq!(got.bins[bin(south)], cells / 2.0);
        assert_eq!((got.lo, got.hi), (south, north));

        // North of 15° only the high level shows.
        let got = frame_levels(&map, &frame_key([0.0, 60.0]));
        assert!(got.bins[bin(north)] > 0.0);
        assert_eq!(got.bins[bin(south)], 0.0);
        assert_eq!((got.lo, got.hi), (north, north));
    }

    #[test]
    fn columns_keep_the_total_area() {
        let bins: Vec<f64> = (0..AREA_BINS).map(|i| (i % 7) as f64).collect();
        let total: f64 = bins.iter().sum();
        for count in [1, 3, 154, 5000] {
            let cols = columns(&bins, count);
            assert_eq!(cols.len(), count);
            assert!((cols.iter().sum::<f64>() - total).abs() < 1e-6 * total);
        }
    }
}
