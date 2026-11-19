//! The area of the world at each level of a raster layer, for the page of
//! the color stops. A worker thread computes the areas again after a change
//! to the levels. A read never waits for the worker: it gets the old areas
//! until the new ones arrive.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use vale_store::RasterId;
use vale_terrain::{Bands, Heightmap, level_areas};

/// The number of level bins.
pub const AREA_BINS: usize = 4096;
/// The largest face that the worker reads, in texels.
const MOST_FACE: usize = 1024;

#[derive(Default)]
struct Entry {
    /// The copy of the map that the areas come from, or that the running
    /// job reads.
    input: Option<Heightmap>,
    /// The area of each level bin, in steradians.
    bins: Option<Arc<Vec<f64>>>,
    job: Option<mpsc::Receiver<Vec<f64>>>,
}

impl Entry {
    /// Takes the areas of a finished job. `block`: waits for it. True while
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
            Ok(bins) => {
                self.bins = Some(Arc::new(bins));
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

/// The level areas of the raster layers.
#[derive(Default)]
pub struct Areas {
    entries: HashMap<RasterId, Entry>,
}

impl Areas {
    /// The area of each level bin of the layer `id`, whose levels are
    /// `live`. Starts a job if the levels changed since the last job.
    pub fn bins(&mut self, id: RasterId, live: &Heightmap) -> Option<Arc<Vec<f64>>> {
        let entry = self.entries.entry(id).or_default();
        let busy = entry.poll(false);
        let changed = !entry.input.as_ref().is_some_and(|m| m.same_tiles(live));
        if changed && !busy {
            let map = live.snapshot();
            entry.job = start(map.snapshot());
            entry.input = Some(map);
        }
        entry.bins.clone()
    }

    /// Takes the areas of the finished jobs. `block`: waits for each
    /// running job. True while a job runs.
    pub fn poll(&mut self, block: bool) -> bool {
        let mut busy = false;
        for entry in self.entries.values_mut() {
            busy |= entry.poll(block);
        }
        busy
    }

    /// Keeps only the layers for which `keep` is true.
    pub fn retain(&mut self, mut keep: impl FnMut(RasterId) -> bool) {
        self.entries.retain(|id, _| keep(*id));
    }
}

/// Starts a job that computes the level areas on a worker thread.
fn start(map: Heightmap) -> Option<mpsc::Receiver<Vec<f64>>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let _ = tx.send(level_areas(&map, AREA_BINS, MOST_FACE));
    };
    std::thread::Builder::new()
        .name("vale-areas".to_string())
        .spawn(run)
        .ok()
        .map(|_| rx)
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
    let n = bins.len() as f64;
    let below: Vec<f64> = (0..=count)
        .map(|c| area_below(bins, c as f64 / count as f64 * n))
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
