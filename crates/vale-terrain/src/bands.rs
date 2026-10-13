//! The value range, the band limits, and the color ramp of the preview.

use crate::cube::{ELEV_MAX, ELEV_MIN};

pub type Rgb = [u8; 3];

/// The elevation of sea level, in meters. It is a band limit of each range
/// that has values below it and above it.
pub const SEA_LEVEL: f64 = 0.0;
/// The largest number of bands.
pub const MAX_BANDS: usize = 32;
/// The least height of one band, in the unit of the layer.
pub const MIN_BAND: f64 = 1.0;
/// The number of bands on each side of sea level in `Bands::with_range`.
const SIDE_BANDS: usize = 4;

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
    /// The lower and upper value, in the unit of the layer.
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
    /// The values of level 0 and of the highest level.
    min: f64,
    max: f64,
    /// The limits in ascending order. With a sea split, sea level is one of
    /// them.
    limits: Vec<f64>,
    /// The bands have a sea split: sea level is a limit that stays, and the
    /// bands below it use the sea side of the ramp.
    split: bool,
    pub ramp: Ramp,
}

/// The bands of the height in meters.
impl Default for Bands {
    fn default() -> Bands {
        Bands {
            min: ELEV_MIN,
            max: ELEV_MAX,
            limits: vec![
                -4000.0, -2000.0, -1000.0, -200.0, SEA_LEVEL, 200.0, 500.0, 1000.0, 2000.0, 3000.0,
                4000.0,
            ],
            split: true,
            ramp: Ramp::default(),
        }
    }
}

impl Bands {
    /// The bands of a range from `min` to `max`, with limits at even spacing
    /// and the default ramp. With `sea`, the values are heights, and a range
    /// with values below and above sea level has a sea split. Such a range
    /// gets the same number of bands on each side of sea level. Returns
    /// `None` if the range is empty or not finite.
    pub fn with_range(min: f64, max: f64, sea: bool) -> Option<Bands> {
        let mut bands = Bands::empty(min, max, sea, Ramp::default())?;
        let sides: &[(f64, f64)] = if bands.sea().is_some() {
            &[(min, SEA_LEVEL), (SEA_LEVEL, max)]
        } else {
            &[(min, max)]
        };
        let count = 2 * SIDE_BANDS / sides.len();
        for &(from, to) in sides {
            for i in 1..count {
                // A range of less than `MIN_BAND` for each band gets fewer limits.
                bands.add(from + (to - from) * i as f64 / count as f64);
            }
        }
        Some(bands)
    }

    /// The height bands of a saved copy. Returns `None` if `add` does not
    /// take a limit, if sea level is not a limit, or if a side of the ramp is
    /// empty.
    pub fn from_parts(limits: &[f64], ramp: Ramp) -> Option<Bands> {
        Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, limits, ramp)
    }

    /// The bands of a saved copy with a range from `min` to `max`. Returns
    /// `None` if the range is empty or not finite, if `add` does not take a
    /// limit, or if the land side of the ramp is empty. With `sea`, the
    /// values are heights. A range of heights with a sea split also needs
    /// sea level as a limit and a sea side of the ramp.
    pub fn from_range_parts(
        min: f64,
        max: f64,
        sea: bool,
        limits: &[f64],
        ramp: Ramp,
    ) -> Option<Bands> {
        let mut bands = Bands::empty(min, max, sea, ramp)?;
        let split = bands.sea().is_some();
        if bands.ramp.land.is_empty() || (split && bands.ramp.sea.is_empty()) {
            return None;
        }
        let mut sea = false;
        for &value in limits {
            if split && value == SEA_LEVEL && !sea {
                sea = true;
            } else {
                bands.add(value)?;
            }
        }
        (sea == split).then_some(bands)
    }

    /// The bands of a range with no limits other than sea level.
    fn empty(min: f64, max: f64, sea: bool, ramp: Ramp) -> Option<Bands> {
        if !(min.is_finite() && max.is_finite() && min < max) {
            return None;
        }
        let split = sea && min < SEA_LEVEL && SEA_LEVEL < max;
        Some(Bands {
            min,
            max,
            limits: if split { vec![SEA_LEVEL] } else { Vec::new() },
            split,
            ramp,
        })
    }

    /// The value of level 0, in the unit of the layer.
    pub fn min(&self) -> f64 {
        self.min
    }

    /// The value of the highest level, in the unit of the layer.
    pub fn max(&self) -> f64 {
        self.max
    }

    /// The level of a value. A value out of the range gets the nearest end.
    pub fn level(&self, value: f64) -> u16 {
        (((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) * 65535.0).round() as u16
    }

    /// The value of a level.
    pub fn value(&self, level: u16) -> f64 {
        self.min + f64::from(level) / 65535.0 * (self.max - self.min)
    }

    /// The limits in ascending order. With a sea split, sea level is one of
    /// them.
    pub fn limits(&self) -> &[f64] {
        &self.limits
    }

    /// The index of sea level in `limits`. Only a range of heights with
    /// values on both sides of sea level has a sea split. All the bands of
    /// each other range use the land side of the ramp.
    pub fn sea(&self) -> Option<usize> {
        self.split.then(|| self.sea_index())
    }

    /// The number of limits below sea level. With a sea split, this is the
    /// index of sea level in `limits`.
    pub fn sea_index(&self) -> usize {
        self.limits.partition_point(|&m| m < SEA_LEVEL)
    }

    /// Adds a limit and returns its index. Returns `None` if the limit is
    /// less than `MIN_BAND` from another limit or from an end of the range,
    /// if it is not finite, or if the list is full.
    pub fn add(&mut self, value: f64) -> Option<usize> {
        let free = value.is_finite()
            && self.limits.len() + 1 < MAX_BANDS
            && (self.min + MIN_BAND..=self.max - MIN_BAND).contains(&value)
            && self.limits.iter().all(|m| (m - value).abs() >= MIN_BAND);
        if !free {
            return None;
        }
        let index = self.limits.partition_point(|&m| m < value);
        self.limits.insert(index, value);
        Some(index)
    }

    /// Moves a limit and returns its new value. The limit stays between the
    /// limits next to it or the ends of the range, `MIN_BAND` away from each,
    /// so its index does not change. Sea level does not move.
    pub fn move_to(&mut self, index: usize, value: f64) -> f64 {
        if Some(index) == self.sea() || value.is_nan() {
            return self.limits[index];
        }
        let below = if index == 0 {
            self.min
        } else {
            self.limits[index - 1]
        };
        let above = self.limits.get(index + 1).copied().unwrap_or(self.max);
        // A limit with less than two bands of room stays where it is.
        let (lo, hi) = (below + MIN_BAND, above - MIN_BAND);
        if lo <= hi {
            self.limits[index] = value.clamp(lo, hi);
        }
        self.limits[index]
    }

    /// Removes a limit. Returns `false` for sea level and for an index out of
    /// range.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.limits.len() || Some(index) == self.sea() {
            return false;
        }
        self.limits.remove(index);
        true
    }

    /// The bands from `min` to `max`, in ascending order. There is one more
    /// band than there are limits.
    ///
    /// The position of a band on the ramp comes from its index on its side of
    /// sea level, and not from its value.
    pub fn bands(&self) -> Vec<Band> {
        let sea = self.sea().map_or(0, |index| index + 1);
        let land = self.limits.len() + 1 - sea;
        (0..=self.limits.len())
            .map(|i| {
                let min = if i == 0 { self.min } else { self.limits[i - 1] };
                let max = self.limits.get(i).copied().unwrap_or(self.max);
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
    use crate::cube::{level_to_meters, meters_to_level};

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

    #[test]
    fn from_parts_gives_the_same_bands() {
        let mut bands = Bands::default();
        bands.add(123.5).unwrap();
        bands.remove(0);
        bands.ramp.land.push([9, 8, 7]);
        let copy = Bands::from_parts(bands.limits(), bands.ramp.clone());
        assert_eq!(copy, Some(bands));

        let ramp = Ramp::default;
        assert_eq!(Bands::from_parts(&[-100.0, 100.0], ramp()), None);
        assert_eq!(Bands::from_parts(&[0.0, 0.0], ramp()), None);
        assert_eq!(Bands::from_parts(&[0.0, 100.0, 100.5], ramp()), None);
        assert_eq!(Bands::from_parts(&[0.0, f64::NAN], ramp()), None);
        let empty = Ramp {
            sea: vec![],
            land: vec![[1, 2, 3]],
        };
        assert_eq!(Bands::from_parts(&[0.0], empty), None);
    }

    #[test]
    fn the_default_gives_the_levels_of_meters() {
        let bands = Bands::default();
        assert_eq!((bands.min(), bands.max()), (ELEV_MIN, ELEV_MAX));
        assert_eq!(bands.sea(), Some(bands.sea_index()));
        for meters in [
            -9000.0, ELEV_MIN, -1234.5, SEA_LEVEL, 0.1, 2500.0, ELEV_MAX, 9000.0,
        ] {
            assert_eq!(bands.level(meters), meters_to_level(meters), "{meters}");
        }
        for level in [0, 1, 32767, 32768, 65535] {
            assert_eq!(bands.value(level), level_to_meters(level), "{level}");
        }
        let same =
            Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, bands.limits(), Ramp::default());
        assert_eq!(same, Some(bands));
    }

    #[test]
    fn a_range_has_its_own_levels() {
        let bands = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(bands.level(0.0), 0);
        assert_eq!(bands.level(4000.0), 65535);
        assert_eq!(bands.level(2000.0), 32768);
        assert_eq!(bands.level(-50.0), 0);
        assert_eq!(bands.level(5000.0), 65535);
        assert_eq!(bands.value(0), 0.0);
        assert_eq!(bands.value(65535), 4000.0);
        // One level is about 0.06 of the unit.
        for value in [0.0, 1.0, 1234.5, 3999.0, 4000.0] {
            let back = bands.value(bands.level(value));
            assert!(
                (back - value).abs() <= 4000.0 / 65535.0 / 2.0 + 1e-9,
                "{value}"
            );
        }
        for level in [0, 1, 777, 40000, 65535] {
            assert_eq!(bands.level(bands.value(level)), level);
        }
    }

    #[test]
    fn a_range_with_no_sea_uses_the_land_ramp() {
        let bands = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let limits = [500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 3500.0];
        assert_eq!(bands.limits(), limits);
        let steps = bands.bands();
        assert_eq!(steps.len(), 8);
        assert_eq!((steps[0].min, steps[7].max), (0.0, 4000.0));
        for (j, band) in steps.iter().enumerate() {
            assert_eq!(band.color, bands.ramp.land((j as f64 + 0.5) / 8.0));
        }

        // Each limit moves and goes away, also the first one.
        let mut bands = bands;
        assert_eq!(bands.move_to(0, 100.0), 100.0);
        assert_eq!(bands.move_to(0, -100.0), MIN_BAND);
        assert_eq!(bands.move_to(6, 9000.0), 4000.0 - MIN_BAND);
        assert_eq!(bands.add(0.0), None);
        assert_eq!(bands.add(4000.0), None);
        while !bands.limits().is_empty() {
            assert!(bands.remove(0));
        }
        let steps = bands.bands();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].bottom, bands.ramp.land(0.0));
        assert_eq!(steps[0].top, bands.ramp.land(1.0));

        let below = Bands::with_range(-4000.0, 0.0, true).unwrap();
        assert_eq!(below.sea(), None);
        assert_eq!(below.bands()[0].bottom, below.ramp.land(0.0));
    }

    #[test]
    fn a_range_across_zero_that_is_not_a_height_has_one_ramp() {
        let bands = Bands::with_range(-60.0, 60.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let limits = [-45.0, -30.0, -15.0, 0.0, 15.0, 30.0, 45.0];
        assert_eq!(bands.limits(), limits);
        let steps = bands.bands();
        assert_eq!(steps.len(), 8);
        for (j, band) in steps.iter().enumerate() {
            assert_eq!(band.color, bands.ramp.land((j as f64 + 0.5) / 8.0));
        }
        // The limit at zero moves and goes away as each other limit.
        let mut bands = bands;
        assert_eq!(bands.move_to(3, 5.0), 5.0);
        assert!(bands.remove(3));
        assert_eq!(bands.limits().len(), 6);
        let ramp = bands.ramp.clone();
        let copy = Bands::from_range_parts(-60.0, 60.0, false, bands.limits(), ramp);
        assert_eq!(copy, Some(bands.clone()));
        // The same parts are not the bands of a height.
        let ramp = bands.ramp.clone();
        assert_eq!(
            Bands::from_range_parts(-60.0, 60.0, true, bands.limits(), ramp),
            None
        );
    }

    #[test]
    fn a_range_across_sea_level_has_a_sea_split() {
        let bands = Bands::with_range(-100.0, 300.0, true).unwrap();
        let limits = [-75.0, -50.0, -25.0, SEA_LEVEL, 75.0, 150.0, 225.0];
        assert_eq!(bands.limits(), limits);
        assert_eq!(bands.sea(), Some(3));
        let steps = bands.bands();
        assert_eq!(steps[3].top, bands.ramp.sea(1.0));
        assert_eq!(steps[4].bottom, bands.ramp.land(0.0));

        // A small range has fewer limits.
        let small = Bands::with_range(0.0, 4.0, true).unwrap();
        assert_eq!(small.limits(), [1.0, 2.0, 3.0]);

        assert_eq!(Bands::with_range(5.0, 5.0, true), None);
        assert_eq!(Bands::with_range(5.0, 1.0, true), None);
        assert_eq!(Bands::with_range(0.0, f64::INFINITY, true), None);
        assert_eq!(Bands::with_range(f64::NAN, 1.0, true), None);
    }

    #[test]
    fn from_range_parts_needs_sea_level_only_with_a_sea_split() {
        let ramp = Ramp::default;
        let land = Ramp {
            sea: vec![],
            land: vec![[1, 2, 3]],
        };
        let bands =
            Bands::from_range_parts(0.0, 4000.0, true, &[250.0, 1000.0], land.clone()).unwrap();
        assert_eq!(bands.limits(), [250.0, 1000.0]);
        assert_eq!((bands.min(), bands.max()), (0.0, 4000.0));
        assert_eq!(bands.level(4000.0), 65535);
        let none = Bands::from_range_parts(0.0, 4000.0, true, &[], ramp()).unwrap();
        assert!(none.limits().is_empty());

        // Sea level is an end of the range, and not a limit.
        assert_eq!(
            Bands::from_range_parts(0.0, 4000.0, true, &[0.0, 250.0], ramp()),
            None
        );
        assert_eq!(
            Bands::from_range_parts(0.0, 4000.0, true, &[5000.0], ramp()),
            None
        );
        assert_eq!(
            Bands::from_range_parts(4000.0, 0.0, true, &[250.0], ramp()),
            None
        );
        let empty = Ramp {
            sea: vec![[1, 2, 3]],
            land: vec![],
        };
        assert_eq!(
            Bands::from_range_parts(0.0, 4000.0, true, &[250.0], empty),
            None
        );

        assert!(Bands::from_range_parts(-10.0, 4000.0, true, &[0.0, 250.0], ramp()).is_some());
        assert_eq!(
            Bands::from_range_parts(-10.0, 4000.0, true, &[250.0], ramp()),
            None
        );
        assert_eq!(
            Bands::from_range_parts(-10.0, 4000.0, true, &[0.0, 250.0], land),
            None
        );
    }
}
