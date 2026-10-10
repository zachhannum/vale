//! The band limits and the color ramp of the elevation preview.

use crate::cube::{ELEV_MAX, ELEV_MIN};

pub type Rgb = [u8; 3];

/// The elevation of sea level, in meters. It is always a band limit.
pub const SEA_LEVEL: f64 = 0.0;
/// The largest number of bands.
pub const MAX_BANDS: usize = 32;
/// The least height of one band, in meters.
pub const MIN_BAND: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    /// Color stops below sea level, from the deepest to the shallowest, with
    /// even spacing.
    pub sea: Vec<Rgb>,
    /// Color stops above sea level, from the lowest to the highest, with even
    /// spacing.
    pub land: Vec<Rgb>,
}

impl Default for Ramp {
    fn default() -> Ramp {
        Ramp {
            sea: vec![[16, 44, 92], [190, 222, 240]],
            land: vec![
                [108, 164, 100],
                [222, 202, 148],
                [150, 108, 76],
                [246, 244, 240],
            ],
        }
    }
}

impl Ramp {
    /// The color below sea level at `t`, from 0 at the deepest stop to 1 at
    /// the shallowest.
    pub fn sea(&self, t: f64) -> Rgb {
        blend(&self.sea, t)
    }

    /// The color above sea level at `t`, from 0 at the lowest stop to 1 at
    /// the highest.
    pub fn land(&self, t: f64) -> Rgb {
        blend(&self.land, t)
    }
}

/// The color at `t` along stops with even spacing. An empty list gives black.
fn blend(stops: &[Rgb], t: f64) -> Rgb {
    match stops {
        [] => [0; 3],
        [one] => *one,
        _ => {
            let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
            let i = (x as usize).min(stops.len() - 2);
            let f = x - i as f64;
            let (a, b) = (stops[i], stops[i + 1]);
            std::array::from_fn(|c| {
                (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * f).round() as u8
            })
        }
    }
}

/// One step of the preview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The lower and upper elevation, in meters.
    pub min: f64,
    pub max: f64,
    /// The color of the whole band in the stepped preview.
    pub color: Rgb,
    /// The colors at `min` and at `max` in the smooth preview.
    pub bottom: Rgb,
    pub top: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bands {
    /// The limits in ascending order. Sea level is one of them.
    limits: Vec<f64>,
    pub ramp: Ramp,
}

impl Default for Bands {
    fn default() -> Bands {
        Bands {
            limits: vec![
                -4000.0, -2000.0, -1000.0, -200.0, SEA_LEVEL, 200.0, 500.0, 1000.0, 2000.0, 3000.0,
                4000.0,
            ],
            ramp: Ramp::default(),
        }
    }
}

impl Bands {
    /// The limits in meters, in ascending order. Sea level is one of them.
    pub fn limits(&self) -> &[f64] {
        &self.limits
    }

    /// The index of sea level in `limits`.
    pub fn sea_index(&self) -> usize {
        self.limits.partition_point(|&m| m < SEA_LEVEL)
    }

    /// Adds a limit and returns its index. Returns `None` if the limit is
    /// less than `MIN_BAND` from another limit or from an end of the
    /// elevation range, if it is not finite, or if the list is full.
    pub fn add(&mut self, meters: f64) -> Option<usize> {
        let free = meters.is_finite()
            && self.limits.len() + 1 < MAX_BANDS
            && (ELEV_MIN + MIN_BAND..=ELEV_MAX - MIN_BAND).contains(&meters)
            && self.limits.iter().all(|m| (m - meters).abs() >= MIN_BAND);
        if !free {
            return None;
        }
        let index = self.limits.partition_point(|&m| m < meters);
        self.limits.insert(index, meters);
        Some(index)
    }

    /// Moves a limit and returns its new elevation. The limit stays between
    /// the limits next to it or the ends of the elevation range, `MIN_BAND`
    /// away from each, so its index does not change. Sea level does not move.
    pub fn move_to(&mut self, index: usize, meters: f64) -> f64 {
        if index == self.sea_index() || meters.is_nan() {
            return self.limits[index];
        }
        let below = if index == 0 {
            ELEV_MIN
        } else {
            self.limits[index - 1]
        };
        let above = self.limits.get(index + 1).copied().unwrap_or(ELEV_MAX);
        // Two neighbors can be less than two bands apart only after a change
        // to the constants, so the limit then stays where it is.
        let (lo, hi) = (below + MIN_BAND, above - MIN_BAND);
        if lo <= hi {
            self.limits[index] = meters.clamp(lo, hi);
        }
        self.limits[index]
    }

    /// Removes a limit. Returns `false` for sea level and for an index out of
    /// range.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.limits.len() || index == self.sea_index() {
            return false;
        }
        self.limits.remove(index);
        true
    }

    /// The bands from `ELEV_MIN` to `ELEV_MAX`, in ascending order. There is
    /// one more band than there are limits.
    ///
    /// The position of a band on the ramp comes from its index on its side of
    /// sea level, and not from its elevation.
    pub fn bands(&self) -> Vec<Band> {
        let sea = self.sea_index() + 1;
        let land = self.limits.len() + 1 - sea;
        (0..=self.limits.len())
            .map(|i| {
                let min = if i == 0 { ELEV_MIN } else { self.limits[i - 1] };
                let max = self.limits.get(i).copied().unwrap_or(ELEV_MAX);
                let (j, k) = if i < sea { (i, sea) } else { (i - sea, land) };
                let at = |offset: f64| {
                    let t = (j as f64 + offset) / k as f64;
                    if i < sea {
                        self.ramp.sea(t)
                    } else {
                        self.ramp.land(t)
                    }
                };
                Band {
                    min,
                    max,
                    color: at(0.5),
                    bottom: at(0.0),
                    top: at(1.0),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(bands: &Bands) -> bool {
        bands.limits().windows(2).all(|w| w[0] < w[1])
    }

    #[test]
    fn the_default_has_sea_level() {
        let bands = Bands::default();
        assert_eq!(bands.limits().len(), 11);
        assert_eq!(bands.limits()[bands.sea_index()], SEA_LEVEL);
        assert!(sorted(&bands));
    }

    #[test]
    fn add_inserts_in_order() {
        let mut bands = Bands::default();
        assert_eq!(bands.add(100.0), Some(5));
        assert_eq!(bands.add(-5000.0), Some(0));
        assert_eq!(bands.add(5000.0), Some(13));
        assert!(sorted(&bands));
        assert_eq!(bands.limits().len(), 14);
        assert_eq!(bands.sea_index(), 5);

        let steps = bands.bands();
        assert_eq!(steps.len(), 15);
        assert_eq!((steps[6].min, steps[6].max), (0.0, 100.0));
        assert_eq!((steps[7].min, steps[7].max), (100.0, 200.0));
    }

    #[test]
    fn add_rejects_bad_limits() {
        let mut bands = Bands::default();
        let before = bands.clone();
        for meters in [
            200.0,
            SEA_LEVEL,
            200.5,
            199.5,
            ELEV_MIN,
            ELEV_MAX,
            ELEV_MIN + 0.5,
            ELEV_MAX - 0.5,
            ELEV_MIN - 100.0,
            ELEV_MAX + 100.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(bands.add(meters), None, "{meters}");
        }
        assert_eq!(bands, before);
        assert_eq!(bands.add(201.0), Some(6));
        assert_eq!(bands.add(ELEV_MIN + MIN_BAND), Some(0));
    }

    #[test]
    fn add_rejects_a_full_list() {
        let mut bands = Bands::default();
        let mut meters = 5.0;
        while bands.limits().len() + 1 < MAX_BANDS {
            assert!(bands.add(meters).is_some());
            meters += 7.0;
        }
        assert_eq!(bands.bands().len(), MAX_BANDS);
        assert_eq!(bands.add(meters), None);
        assert_eq!(bands.bands().len(), MAX_BANDS);
    }

    #[test]
    fn move_to_stays_between_the_neighbors() {
        let mut bands = Bands::default();
        assert_eq!(bands.move_to(6, 350.0), 350.0);
        assert_eq!(bands.limits()[6], 350.0);
        assert_eq!(bands.bands()[6].max, 350.0);
        assert_eq!(bands.bands()[7].min, 350.0);

        assert_eq!(bands.move_to(6, 5000.0), 1000.0 - MIN_BAND);
        assert_eq!(bands.move_to(6, -5000.0), 200.0 + MIN_BAND);
        assert_eq!(bands.move_to(6, f64::INFINITY), 1000.0 - MIN_BAND);
        assert_eq!(bands.move_to(6, f64::NAN), 1000.0 - MIN_BAND);

        assert_eq!(bands.move_to(0, -9000.0), ELEV_MIN + MIN_BAND);
        assert_eq!(bands.move_to(10, 9000.0), ELEV_MAX - MIN_BAND);
        assert_eq!(bands.move_to(3, 500.0), SEA_LEVEL - MIN_BAND);
        assert_eq!(bands.limits().len(), 11);
        assert!(sorted(&bands));
    }

    #[test]
    fn sea_level_stays() {
        let mut bands = Bands::default();
        let sea = bands.sea_index();
        assert_eq!(bands.move_to(sea, 100.0), SEA_LEVEL);
        assert_eq!(bands.move_to(sea, -100.0), SEA_LEVEL);
        assert!(!bands.remove(sea));
        assert_eq!(bands, Bands::default());
    }

    #[test]
    fn remove_joins_two_bands() {
        let mut bands = Bands::default();
        assert!(bands.remove(5));
        assert_eq!(bands.limits().len(), 10);
        assert!(!bands.limits().contains(&200.0));
        assert!(sorted(&bands));
        let steps = bands.bands();
        assert_eq!(steps.len(), 11);
        assert_eq!((steps[5].min, steps[5].max), (0.0, 500.0));

        assert!(bands.remove(0));
        assert_eq!(bands.sea_index(), 3);
        assert!(!bands.remove(3));
        assert!(!bands.remove(bands.limits().len()));
    }

    #[test]
    fn sea_level_is_the_only_limit_that_stays() {
        let mut bands = Bands::default();
        while bands.limits().len() > 1 {
            let index = if bands.sea_index() == 0 { 1 } else { 0 };
            assert!(bands.remove(index));
        }
        assert_eq!(bands.limits(), [SEA_LEVEL]);
        let steps = bands.bands();
        assert_eq!((steps[0].min, steps[0].max), (ELEV_MIN, SEA_LEVEL));
        assert_eq!((steps[1].min, steps[1].max), (SEA_LEVEL, ELEV_MAX));
        assert_eq!(steps[0].bottom, bands.ramp.sea(0.0));
        assert_eq!(steps[0].top, bands.ramp.sea(1.0));
        assert_eq!(steps[1].color, bands.ramp.land(0.5));
    }

    #[test]
    fn each_side_has_its_own_ramp() {
        let mut bands = Bands {
            ramp: Ramp {
                sea: vec![[0, 0, 40], [0, 0, 255]],
                land: vec![[255, 0, 0], [0, 255, 0]],
            },
            ..Bands::default()
        };
        bands.add(-2.0).unwrap();
        bands.add(2.0).unwrap();
        let sea = bands.sea_index();
        let steps = bands.bands();
        for (i, band) in steps.iter().enumerate() {
            for color in [band.color, band.bottom, band.top] {
                if i <= sea {
                    assert_eq!((color[0], color[1]), (0, 0), "band {i}");
                    assert!(color[2] >= 40, "band {i}");
                } else {
                    assert_eq!(color[2], 0, "band {i}");
                    let sum = u16::from(color[0]) + u16::from(color[1]);
                    assert!(sum.abs_diff(255) <= 1, "band {i}");
                }
            }
        }
        assert_eq!(steps[sea].max, SEA_LEVEL);
        assert_eq!(steps[sea + 1].min, SEA_LEVEL);
        assert_eq!(steps[sea].top, [0, 0, 255]);
        assert_eq!(steps[sea + 1].bottom, [255, 0, 0]);
        assert_ne!(steps[sea].color, steps[sea + 1].color);
    }

    #[test]
    fn the_ramp_position_comes_from_the_band_index() {
        let mut bands = Bands::default();
        bands.add(1.0).unwrap();
        bands.add(2.0).unwrap();
        let steps = bands.bands();
        let land = &steps[bands.sea_index() + 1..];
        let k = land.len() as f64;
        for (j, band) in land.iter().enumerate() {
            let j = j as f64;
            assert_eq!(band.bottom, bands.ramp.land(j / k));
            assert_eq!(band.color, bands.ramp.land((j + 0.5) / k));
            assert_eq!(band.top, bands.ramp.land((j + 1.0) / k));
        }
        assert_ne!(land[0].color, land[1].color);
        assert_ne!(land[1].color, land[2].color);
        for pair in steps.windows(2) {
            if pair[0].max != SEA_LEVEL {
                assert_eq!(pair[0].top, pair[1].bottom);
            }
        }
    }

    #[test]
    fn the_bands_cover_the_range() {
        let mut bands = Bands::default();
        bands.add(-3000.5).unwrap();
        bands.add(5999.0).unwrap();
        bands.move_to(2, -2500.0);
        bands.remove(7);
        let steps = bands.bands();
        assert_eq!(steps.len(), bands.limits().len() + 1);
        assert_eq!(steps[0].min, ELEV_MIN);
        assert_eq!(steps[steps.len() - 1].max, ELEV_MAX);
        for pair in steps.windows(2) {
            assert_eq!(pair[0].max, pair[1].min);
        }
        for band in &steps {
            assert!(band.max - band.min >= MIN_BAND);
        }
    }

    #[test]
    fn the_ramp_blends_between_stops() {
        let ramp = Ramp {
            sea: vec![[0, 0, 0], [100, 200, 51]],
            land: vec![[0, 0, 0], [100, 100, 100], [200, 0, 100]],
        };
        assert_eq!(ramp.sea(0.0), [0, 0, 0]);
        assert_eq!(ramp.sea(0.5), [50, 100, 26]);
        assert_eq!(ramp.sea(1.0), [100, 200, 51]);
        assert_eq!(ramp.sea(-1.0), [0, 0, 0]);
        assert_eq!(ramp.sea(2.0), [100, 200, 51]);
        assert_eq!(ramp.land(0.5), [100, 100, 100]);
        assert_eq!(ramp.land(0.75), [150, 50, 100]);
        assert_eq!(ramp.land(1.0), [200, 0, 100]);

        let short = Ramp {
            sea: Vec::new(),
            land: vec![[1, 2, 3]],
        };
        assert_eq!(short.sea(0.3), [0, 0, 0]);
        assert_eq!(short.land(0.3), [1, 2, 3]);
    }
}
