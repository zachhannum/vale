//! The value range and the color stops of the preview.

use crate::cube::{ELEV_MAX, ELEV_MIN};

pub type Rgb = [u8; 3];

/// The elevation of sea level, in meters. It is a stop of each range of
/// heights that has values below it and above it.
pub const SEA_LEVEL: f64 = 0.0;
/// The largest number of bands.
pub const MAX_BANDS: usize = 32;
/// The least height of one band, in the unit of the layer.
pub const MIN_BAND: f64 = 1.0;
/// The number of bands on each side of sea level in `Bands::with_range`.
const SIDE_BANDS: usize = 4;

/// The default colors below sea level, from the deepest to the shallowest,
/// with even spacing over the bands.
const SEA_PALETTE: [Rgb; 2] = [[16, 44, 92], [190, 222, 240]];
/// The default colors above sea level, from the lowest to the highest, with
/// even spacing over the bands.
const LAND_PALETTE: [Rgb; 4] = [
    [108, 164, 100],
    [222, 202, 148],
    [150, 108, 76],
    [246, 244, 240],
];

/// One color stop. The color goes from `above` of a stop to `below` of the
/// next stop. Only the stop at sea level has two different colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// The value, in the unit of the layer.
    pub value: f64,
    /// The color just below the value.
    pub below: Rgb,
    /// The color at the value and just above it.
    pub above: Rgb,
    /// The line where the value crosses the stop.
    pub line: StopLine,
}

/// A line at a color stop. It keeps its values while it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopLine {
    pub on: bool,
    /// The weight, in points.
    pub weight: f32,
    pub color: Rgb,
    /// The line has the color of the stop, darker by `darken`, in place of
    /// `color`. The stop at sea level gives its sea color.
    pub darker: bool,
    /// The part of the light that the darker color takes away, from 0 to 1.
    pub darken: f32,
}

impl StopLine {
    /// The least and the largest weight, in points.
    pub const WEIGHTS: std::ops::RangeInclusive<f32> = 0.25..=8.0;
}

impl Default for StopLine {
    fn default() -> Self {
        StopLine {
            on: false,
            weight: 1.0,
            color: [40, 40, 40],
            darker: false,
            darken: 0.5,
        }
    }
}

/// A side of a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// One step of the preview, from one stop to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The lower and upper value, in the unit of the layer.
    pub min: f64,
    pub max: f64,
    /// The color of the whole band in the stepped preview. It is the color
    /// of the stop at the edge of the band nearer sea level: the bottom edge
    /// above sea level and with no sea split, and the top edge below sea
    /// level.
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
    /// The stops in ascending order. The first stop is at `min` and the last
    /// stop is at `max`.
    stops: Vec<Stop>,
    /// The bands have a sea split: sea level is a stop that stays, with a
    /// sea color below it and a land color above it.
    split: bool,
}

/// The bands of the height in meters.
impl Default for Bands {
    fn default() -> Bands {
        let limits = [
            -4000.0, -2000.0, -1000.0, -200.0, SEA_LEVEL, 200.0, 500.0, 1000.0, 2000.0, 3000.0,
            4000.0,
        ];
        Bands::from_palette(ELEV_MIN, ELEV_MAX, true, &limits)
    }
}

impl Bands {
    /// The bands of a range from `min` to `max`, with stops at even spacing
    /// and the default colors. With `sea`, the values are heights, and a
    /// range with values below and above sea level has a sea split. Such a
    /// range gets the same number of bands on each side of sea level.
    /// Returns `None` if the range is empty or not finite.
    pub fn with_range(min: f64, max: f64, sea: bool) -> Option<Bands> {
        if !(min.is_finite() && max.is_finite() && min < max) {
            return None;
        }
        let split = splits(min, max, sea);
        let sides: &[(f64, f64)] = if split {
            &[(min, SEA_LEVEL), (SEA_LEVEL, max)]
        } else {
            &[(min, max)]
        };
        let mut limits: Vec<f64> = if split { vec![SEA_LEVEL] } else { Vec::new() };
        let count = 2 * SIDE_BANDS / sides.len();
        for &(from, to) in sides {
            for i in 1..count {
                let value = from + (to - from) * i as f64 / count as f64;
                // A range of less than `MIN_BAND` for each band gets fewer stops.
                let free = (min + MIN_BAND..=max - MIN_BAND).contains(&value)
                    && limits.iter().all(|m| (m - value).abs() >= MIN_BAND);
                if free {
                    limits.push(value);
                }
            }
        }
        limits.sort_by(f64::total_cmp);
        Some(Bands::from_palette(min, max, split, &limits))
    }

    /// The bands of a saved copy with a range from `min` to `max`. With
    /// `sea`, the values are heights. An empty list of stops gives the
    /// stops of `with_range`.
    ///
    /// Returns `None` if the range is empty or not finite, or if the stops
    /// break a rule:
    /// - There are 2 to `MAX_BANDS + 1` stops.
    /// - The first stop is at `min`, and the last stop is at `max`.
    /// - The values ascend, with at least `MIN_BAND` between two stops.
    /// - With a sea split, a stop is at sea level.
    /// - Only the stop at sea level has two different colors.
    pub fn from_range_parts(min: f64, max: f64, sea: bool, stops: &[Stop]) -> Option<Bands> {
        if stops.is_empty() {
            return Bands::with_range(min, max, sea);
        }
        if !(min.is_finite() && max.is_finite() && min < max) {
            return None;
        }
        let split = splits(min, max, sea);
        let valid = (2..=MAX_BANDS + 1).contains(&stops.len())
            && stops[0].value == min
            && stops[stops.len() - 1].value == max
            && stops
                .windows(2)
                .all(|w| w[1].value - w[0].value >= MIN_BAND)
            && stops
                .iter()
                .all(|s| s.below == s.above || (split && s.value == SEA_LEVEL))
            && (!split || stops.iter().any(|s| s.value == SEA_LEVEL));
        valid.then(|| Bands {
            min,
            max,
            stops: stops.to_vec(),
            split,
        })
    }

    /// The bands with stops at `min`, at each of `limits`, and at `max`. The
    /// limits ascend, and with `split`, sea level is one of them. Each band
    /// takes its colors from the default palette by its index on its side
    /// of sea level.
    fn from_palette(min: f64, max: f64, split: bool, limits: &[f64]) -> Bands {
        let values: Vec<f64> = std::iter::once(min)
            .chain(limits.iter().copied())
            .chain(std::iter::once(max))
            .collect();
        let bands = values.len() - 1;
        let sea = if split {
            values.partition_point(|&v| v < SEA_LEVEL)
        } else {
            0
        };
        let land = bands - sea;
        let sea_color = |i: usize| blend(&SEA_PALETTE, i as f64 / sea as f64);
        let land_color = |i: usize| blend(&LAND_PALETTE, (i - sea) as f64 / land as f64);
        let stops = values
            .iter()
            .enumerate()
            .map(|(i, &value)| {
                // The top of the band below the stop, and the bottom of the
                // band above it.
                let below = match i {
                    0 => None,
                    _ if i <= sea => Some(sea_color(i)),
                    _ => Some(land_color(i)),
                };
                let above = match i {
                    _ if i == bands => None,
                    _ if i < sea => Some(sea_color(i)),
                    _ => Some(land_color(i)),
                };
                let below = below.or(above).unwrap_or_default();
                Stop {
                    value,
                    below,
                    above: above.unwrap_or(below),
                    line: StopLine::default(),
                }
            })
            .collect();
        Bands {
            min,
            max,
            stops,
            split,
        }
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

    /// The stops in ascending order, from `min` to `max`.
    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }

    /// The index of the stop at sea level. Only a range of heights with
    /// values on both sides of sea level has a sea split.
    pub fn sea(&self) -> Option<usize> {
        self.split
            .then(|| self.stops.partition_point(|s| s.value < SEA_LEVEL))
    }

    /// Whether the stop at `index` does not move: the two ends and sea level.
    pub fn fixed(&self, index: usize) -> bool {
        index == 0 || index + 1 == self.stops.len() || Some(index) == self.sea()
    }

    /// The color of the smooth preview at a value. A value out of the range
    /// gets the color of the nearest end.
    pub fn color_at(&self, value: f64) -> Rgb {
        if value.is_nan() || value <= self.min {
            return self.stops[0].above;
        }
        if value >= self.max {
            return self.stops[self.stops.len() - 1].below;
        }
        let i = self.stops.partition_point(|s| s.value <= value) - 1;
        let (a, b) = (self.stops[i], self.stops[i + 1]);
        mix(a.above, b.below, (value - a.value) / (b.value - a.value))
    }

    /// Adds a stop with the color of the gradient at its value, and returns
    /// its index. Returns `None` if the value is less than `MIN_BAND` from
    /// another stop, if it is not finite, or if the list is full.
    pub fn add(&mut self, value: f64) -> Option<usize> {
        let free = value.is_finite()
            && self.stops.len() <= MAX_BANDS
            && (self.min + MIN_BAND..=self.max - MIN_BAND).contains(&value)
            && self
                .stops
                .iter()
                .all(|s| (s.value - value).abs() >= MIN_BAND);
        if !free {
            return None;
        }
        let color = self.color_at(value);
        let index = self.stops.partition_point(|s| s.value < value);
        let stop = Stop {
            value,
            below: color,
            above: color,
            line: StopLine::default(),
        };
        self.stops.insert(index, stop);
        Some(index)
    }

    /// Moves a stop and returns its new value. The stop stays between the
    /// stops next to it, `MIN_BAND` away from each, so its index does not
    /// change. A fixed stop does not move.
    pub fn move_to(&mut self, index: usize, value: f64) -> f64 {
        if self.fixed(index) || value.is_nan() {
            return self.stops[index].value;
        }
        let lo = self.stops[index - 1].value + MIN_BAND;
        let hi = self.stops[index + 1].value - MIN_BAND;
        // A stop with less than two bands of room stays where it is.
        if lo <= hi {
            self.stops[index].value = value.clamp(lo, hi);
        }
        self.stops[index].value
    }

    /// Removes a stop. Returns `false` for a fixed stop and for an index out
    /// of range.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.stops.len() || self.fixed(index) {
            return false;
        }
        self.stops.remove(index);
        true
    }

    /// Sets a color of a stop. The stop at sea level takes the color on
    /// `side` only, and each other stop takes it on both sides. Returns
    /// `false` for an index out of range.
    pub fn set_color(&mut self, index: usize, side: Side, color: Rgb) -> bool {
        let sea = self.sea();
        let Some(stop) = self.stops.get_mut(index) else {
            return false;
        };
        match side {
            _ if Some(index) != sea => (stop.below, stop.above) = (color, color),
            Side::Below => stop.below = color,
            Side::Above => stop.above = color,
        }
        true
    }

    /// Sets the line of a stop. The weight stays in `StopLine::WEIGHTS`. Returns
    /// `false` for an index out of range.
    pub fn set_line(&mut self, index: usize, line: StopLine) -> bool {
        let Some(stop) = self.stops.get_mut(index) else {
            return false;
        };
        let (lo, hi) = (*StopLine::WEIGHTS.start(), *StopLine::WEIGHTS.end());
        let weight = if line.weight.is_nan() {
            lo
        } else {
            line.weight.clamp(lo, hi)
        };
        let darken = if line.darken.is_nan() {
            0.0
        } else {
            line.darken.clamp(0.0, 1.0)
        };
        stop.line = StopLine {
            weight,
            darken,
            ..line
        };
        true
    }

    /// The color of the line of a stop, or `None` for an index out of range.
    pub fn line_color(&self, index: usize) -> Option<Rgb> {
        let stop = self.stops.get(index)?;
        let line = stop.line;
        if !line.darker {
            return Some(line.color);
        }
        let base = if self.sea() == Some(index) {
            stop.below
        } else {
            stop.above
        };
        let keep = 1.0 - line.darken;
        Some(base.map(|c| (f32::from(c) * keep).round() as u8))
    }

    /// The bands with one band of each color, from the lowest band to the
    /// highest. With the same number of bands, the stops keep their values
    /// and their lines. Otherwise, the stops have even spacing on each side
    /// of sea level. The sea gets the first colors that are mostly blue, or
    /// if all or none are, the part of the bands of its part of the range.
    /// Returns `None` if there are no colors, if there are more than
    /// `MAX_BANDS`, or if the range has no room for the stops.
    pub fn painted(&self, colors: &[Rgb]) -> Option<Bands> {
        let n = colors.len();
        if n == 0 || n > MAX_BANDS || (self.split && n < 2) {
            return None;
        }
        let mut stops: Vec<Stop> = if n + 1 == self.stops.len() {
            self.stops.clone()
        } else {
            let sides: Vec<(f64, f64, usize)> = if self.split {
                let blue = colors
                    .iter()
                    .take_while(|c| c[2] > c[0] && c[2] >= c[1])
                    .count();
                let part = (SEA_LEVEL - self.min) / (self.max - self.min);
                let sea = if (1..n).contains(&blue) {
                    blue
                } else {
                    ((n as f64 * part).round() as usize).clamp(1, n - 1)
                };
                vec![(self.min, SEA_LEVEL, sea), (SEA_LEVEL, self.max, n - sea)]
            } else {
                vec![(self.min, self.max, n)]
            };
            let mut values = vec![self.min];
            for (from, to, count) in sides {
                let step = (to - from) / count as f64;
                values.extend((1..count).map(|i| from + step * i as f64));
                values.push(to);
            }
            values
                .into_iter()
                .map(|value| Stop {
                    value,
                    below: [0; 3],
                    above: [0; 3],
                    line: StopLine::default(),
                })
                .collect()
        };
        let sea = self
            .split
            .then(|| stops.partition_point(|s| s.value < SEA_LEVEL));
        for (i, stop) in stops.iter_mut().enumerate() {
            // Each band takes its color from the stop at its edge nearer sea
            // level.
            let (below, above) = match sea {
                Some(sea) if i == sea => (colors[i - 1], colors[i]),
                Some(sea) if i < sea => (colors[i.max(1) - 1], colors[i.max(1) - 1]),
                _ => (colors[i.min(n - 1)], colors[i.min(n - 1)]),
            };
            (stop.below, stop.above) = (below, above);
        }
        Bands::from_range_parts(self.min, self.max, self.split, &stops)
    }

    /// The bands from `min` to `max`, in ascending order. There is one band
    /// fewer than there are stops. Each stop gives its color to one band, so
    /// the top stop and, with a sea split, the bottom stop give none. The
    /// stop at sea level gives its two colors to the band on each side.
    pub fn bands(&self) -> Vec<Band> {
        let sea = self.sea();
        self.stops
            .windows(2)
            .enumerate()
            .map(|(i, w)| Band {
                min: w[0].value,
                max: w[1].value,
                color: match sea {
                    Some(sea) if i < sea => w[1].below,
                    _ => w[0].above,
                },
                bottom: w[0].above,
                top: w[1].below,
            })
            .collect()
    }
}

/// Whether a range has a sea split.
fn splits(min: f64, max: f64, sea: bool) -> bool {
    sea && min < SEA_LEVEL && SEA_LEVEL < max
}

/// The color at `t` from `a` to `b`.
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    std::array::from_fn(|c| {
        (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * t).round() as u8
    })
}

/// The color at `t` along colors with even spacing.
fn blend(colors: &[Rgb], t: f64) -> Rgb {
    let x = t.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
    let i = (x as usize).min(colors.len() - 2);
    mix(colors[i], colors[i + 1], x - i as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{level_to_meters, meters_to_level};

    fn values(bands: &Bands) -> Vec<f64> {
        bands.stops().iter().map(|s| s.value).collect()
    }

    fn sorted(bands: &Bands) -> bool {
        bands.stops().windows(2).all(|w| w[0].value < w[1].value)
    }

    fn stop(value: f64, color: Rgb) -> Stop {
        Stop {
            value,
            below: color,
            above: color,
            line: StopLine::default(),
        }
    }

    /// Stops with one grey color.
    fn plain(values: &[f64]) -> Vec<Stop> {
        values.iter().map(|&v| stop(v, [9; 3])).collect()
    }

    #[test]
    fn the_default_has_sea_level() {
        let bands = Bands::default();
        assert_eq!(bands.stops().len(), 13);
        let sea = bands.sea().unwrap();
        assert_eq!(sea, 5);
        assert_eq!(bands.stops()[sea].value, SEA_LEVEL);
        assert_eq!(
            (bands.stops()[0].value, bands.stops()[12].value),
            (ELEV_MIN, ELEV_MAX)
        );
        assert!(sorted(&bands));
    }

    #[test]
    fn the_default_keeps_the_old_band_edges() {
        let bands = Bands::default();
        let sea = [
            [16, 44, 92],
            [51, 80, 122],
            [86, 115, 151],
            [120, 151, 181],
            [155, 186, 210],
            [190, 222, 240],
        ];
        let land = [
            [108, 164, 100],
            [157, 180, 121],
            [206, 197, 141],
            [201, 175, 127],
            [171, 135, 97],
            [164, 127, 99],
            [205, 186, 170],
            [246, 244, 240],
        ];
        let stops = bands.stops();
        for (i, color) in sea.iter().enumerate().take(5) {
            assert_eq!((stops[i].below, stops[i].above), (*color, *color), "{i}");
        }
        assert_eq!((stops[5].below, stops[5].above), (sea[5], land[0]));
        for (j, color) in land.iter().enumerate().skip(1) {
            let s = stops[5 + j];
            assert_eq!((s.below, s.above), (*color, *color), "{j}");
        }
    }

    #[test]
    fn add_inserts_in_order() {
        let mut bands = Bands::default();
        assert_eq!(bands.add(100.0), Some(6));
        assert_eq!(bands.add(-5000.0), Some(1));
        assert_eq!(bands.add(5000.0), Some(14));
        assert!(sorted(&bands));
        assert_eq!(bands.stops().len(), 16);
        assert_eq!(bands.sea(), Some(6));

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
        assert_eq!(bands.add(201.0), Some(7));
        assert_eq!(bands.add(ELEV_MIN + MIN_BAND), Some(1));
    }

    #[test]
    fn add_rejects_a_full_list() {
        let mut bands = Bands::default();
        let mut meters = 5.0;
        while bands.bands().len() < MAX_BANDS {
            assert!(bands.add(meters).is_some());
            meters += 7.0;
        }
        assert_eq!(bands.stops().len(), MAX_BANDS + 1);
        assert_eq!(bands.add(meters), None);
        assert_eq!(bands.bands().len(), MAX_BANDS);
    }

    #[test]
    fn add_takes_the_color_of_the_gradient() {
        let stops = [stop(0.0, [0, 0, 0]), stop(100.0, [200, 100, 50])];
        let mut bands = Bands::from_range_parts(0.0, 100.0, false, &stops).unwrap();
        assert_eq!(bands.add(25.0), Some(1));
        assert_eq!(bands.stops()[1], stop(25.0, [50, 25, 13]));
        assert_eq!(bands.stops()[0], stops[0]);
        assert_eq!(bands.stops()[2], stops[1]);

        // The gradient does not change.
        let mut bands = Bands::default();
        let before: Vec<Rgb> = (-50..=50)
            .map(|i| bands.color_at(i as f64 * 97.0))
            .collect();
        let color = bands.color_at(1234.0);
        let index = bands.add(1234.0).unwrap();
        assert_eq!(bands.stops()[index], stop(1234.0, color));
        let after: Vec<Rgb> = (-50..=50)
            .map(|i| bands.color_at(i as f64 * 97.0))
            .collect();
        for (a, b) in before.iter().zip(&after) {
            for c in 0..3 {
                assert!(a[c].abs_diff(b[c]) <= 1, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn color_at_blends_between_stops() {
        let stops = [
            stop(0.0, [0, 0, 0]),
            stop(10.0, [100, 200, 51]),
            stop(30.0, [200, 0, 100]),
        ];
        let bands = Bands::from_range_parts(0.0, 30.0, false, &stops).unwrap();
        assert_eq!(bands.color_at(0.0), [0, 0, 0]);
        assert_eq!(bands.color_at(5.0), [50, 100, 26]);
        assert_eq!(bands.color_at(10.0), [100, 200, 51]);
        assert_eq!(bands.color_at(25.0), [175, 50, 88]);
        assert_eq!(bands.color_at(30.0), [200, 0, 100]);
        assert_eq!(bands.color_at(-5.0), [0, 0, 0]);
        assert_eq!(bands.color_at(f64::NAN), [0, 0, 0]);
        assert_eq!(bands.color_at(99.0), [200, 0, 100]);
    }

    #[test]
    fn the_step_color_is_the_color_of_one_stop() {
        let stops = [stop(0.0, [0, 0, 0]), stop(10.0, [101, 50, 3])];
        let bands = Bands::from_range_parts(0.0, 10.0, false, &stops).unwrap();
        let steps = bands.bands();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].bottom, [0, 0, 0]);
        assert_eq!(steps[0].top, [101, 50, 3]);
        assert_eq!(steps[0].color, [0, 0, 0]);
    }

    #[test]
    fn each_stop_colors_one_band() {
        let mut bands = Bands::default();
        let sea = bands.sea().unwrap();
        let last = bands.stops().len() - 1;
        let colors = |b: &Bands| b.bands().iter().map(|b| b.color).collect::<Vec<_>>();
        let red = [255, 0, 0];
        // Each stop and side, with the one band that it colors.
        let mut cases = vec![
            (sea, Side::Below, Some(sea - 1)),
            (sea, Side::Above, Some(sea)),
            (0, Side::Above, None),
            (last, Side::Below, None),
        ];
        cases.extend((1..sea).map(|i| (i, Side::Above, Some(i - 1))));
        cases.extend((sea + 1..last).map(|i| (i, Side::Above, Some(i))));
        for (index, side, band) in cases {
            let before = colors(&bands);
            let mut changed = bands.clone();
            changed.set_color(index, side, red);
            let after = colors(&changed);
            for (i, (a, b)) in before.iter().zip(&after).enumerate() {
                let want = if Some(i) == band { red } else { *a };
                assert_eq!(*b, want, "stop {index} {side:?}, band {i}");
            }
            bands = changed;
        }
    }

    #[test]
    fn move_to_stays_between_the_neighbors() {
        let mut bands = Bands::default();
        assert_eq!(bands.move_to(7, 350.0), 350.0);
        assert_eq!(bands.stops()[7].value, 350.0);
        assert_eq!(bands.bands()[6].max, 350.0);
        assert_eq!(bands.bands()[7].min, 350.0);

        assert_eq!(bands.move_to(7, 5000.0), 1000.0 - MIN_BAND);
        assert_eq!(bands.move_to(7, -5000.0), 200.0 + MIN_BAND);
        assert_eq!(bands.move_to(7, f64::INFINITY), 1000.0 - MIN_BAND);
        assert_eq!(bands.move_to(7, f64::NAN), 1000.0 - MIN_BAND);

        assert_eq!(bands.move_to(1, -9000.0), ELEV_MIN + MIN_BAND);
        assert_eq!(bands.move_to(11, 9000.0), ELEV_MAX - MIN_BAND);
        assert_eq!(bands.move_to(4, 500.0), SEA_LEVEL - MIN_BAND);
        assert_eq!(bands.stops().len(), 13);
        assert!(sorted(&bands));
    }

    #[test]
    fn the_ends_stay_and_take_colors() {
        let mut bands = Bands::default();
        let last = bands.stops().len() - 1;
        assert!(bands.fixed(0) && bands.fixed(last) && bands.fixed(5));
        assert!(!bands.fixed(1) && !bands.fixed(last - 1));
        assert_eq!(bands.move_to(0, 100.0), ELEV_MIN);
        assert_eq!(bands.move_to(last, 100.0), ELEV_MAX);
        assert!(!bands.remove(0));
        assert!(!bands.remove(last));
        assert_eq!(bands, Bands::default());

        assert!(bands.set_color(0, Side::Above, [1, 2, 3]));
        assert_eq!(bands.stops()[0], stop(ELEV_MIN, [1, 2, 3]));
        assert!(bands.set_color(last, Side::Below, [4, 5, 6]));
        assert_eq!(bands.stops()[last], stop(ELEV_MAX, [4, 5, 6]));
        assert!(bands.set_color(3, Side::Below, [7, 8, 9]));
        assert_eq!(bands.stops()[3], stop(-1000.0, [7, 8, 9]));
        assert!(!bands.set_color(last + 1, Side::Below, [7, 8, 9]));
        assert_eq!(bands.bands()[0].bottom, [1, 2, 3]);
        assert_eq!(bands.bands()[last - 1].top, [4, 5, 6]);
    }

    #[test]
    fn sea_level_has_two_colors_and_stays() {
        let mut bands = Bands::default();
        let sea = bands.sea().unwrap();
        assert_eq!(bands.move_to(sea, 100.0), SEA_LEVEL);
        assert_eq!(bands.move_to(sea, -100.0), SEA_LEVEL);
        assert!(!bands.remove(sea));
        assert_eq!(bands, Bands::default());

        assert!(bands.set_color(sea, Side::Below, [0, 0, 255]));
        assert!(bands.set_color(sea, Side::Above, [255, 0, 0]));
        let s = bands.stops()[sea];
        assert_eq!(
            (s.value, s.below, s.above),
            (SEA_LEVEL, [0, 0, 255], [255, 0, 0])
        );
        let steps = bands.bands();
        assert_eq!(steps[sea - 1].max, SEA_LEVEL);
        assert_eq!(steps[sea - 1].top, [0, 0, 255]);
        assert_eq!(steps[sea].min, SEA_LEVEL);
        assert_eq!(steps[sea].bottom, [255, 0, 0]);
        assert_eq!(bands.color_at(SEA_LEVEL), [255, 0, 0]);
        assert_eq!(bands.color_at(-1e-9), [0, 0, 255]);

        // A stop next to sea level takes the color on its side.
        let index = bands.add(-100.0).unwrap();
        assert_eq!(index, sea);
        let near = bands.stops()[index];
        assert_eq!(near.below, near.above);
        assert_eq!(bands.stops()[sea + 1].value, SEA_LEVEL);
        let copy = Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, bands.stops());
        assert_eq!(copy, Some(bands));
    }

    #[test]
    fn remove_joins_two_bands() {
        let mut bands = Bands::default();
        assert!(bands.remove(6));
        assert_eq!(bands.stops().len(), 12);
        assert!(!values(&bands).contains(&200.0));
        assert!(sorted(&bands));
        let steps = bands.bands();
        assert_eq!(steps.len(), 11);
        assert_eq!((steps[5].min, steps[5].max), (0.0, 500.0));

        assert!(bands.remove(1));
        assert_eq!(bands.sea(), Some(4));
        assert!(!bands.remove(4));
        assert!(!bands.remove(bands.stops().len()));
    }

    #[test]
    fn sea_level_is_the_only_inner_stop_that_stays() {
        let mut bands = Bands::default();
        let first = bands.stops()[0];
        let last = bands.stops()[12];
        let sea = bands.stops()[5];
        while bands.stops().len() > 3 {
            let index = (0..bands.stops().len()).find(|&i| !bands.fixed(i)).unwrap();
            assert!(bands.remove(index));
        }
        assert_eq!(values(&bands), [ELEV_MIN, SEA_LEVEL, ELEV_MAX]);
        let steps = bands.bands();
        assert_eq!((steps[0].min, steps[0].max), (ELEV_MIN, SEA_LEVEL));
        assert_eq!((steps[1].min, steps[1].max), (SEA_LEVEL, ELEV_MAX));
        assert_eq!(steps[0].bottom, first.above);
        assert_eq!(steps[0].top, sea.below);
        assert_eq!(steps[1].bottom, sea.above);
        assert_eq!(steps[1].top, last.below);
    }

    #[test]
    fn the_bands_cover_the_range() {
        let mut bands = Bands::default();
        bands.add(-3000.5).unwrap();
        bands.add(5999.0).unwrap();
        bands.move_to(3, -2500.0);
        bands.remove(8);
        let steps = bands.bands();
        assert_eq!(steps.len(), bands.stops().len() - 1);
        assert_eq!(steps[0].min, ELEV_MIN);
        assert_eq!(steps[steps.len() - 1].max, ELEV_MAX);
        for pair in steps.windows(2) {
            assert_eq!(pair[0].max, pair[1].min);
            if pair[0].max != SEA_LEVEL {
                assert_eq!(pair[0].top, pair[1].bottom);
            }
        }
        for band in &steps {
            assert!(band.max - band.min >= MIN_BAND);
        }
    }

    #[test]
    fn from_range_parts_gives_the_same_bands() {
        let mut bands = Bands::default();
        bands.add(123.5).unwrap();
        bands.remove(1);
        bands.set_color(3, Side::Above, [9, 8, 7]);
        let copy = Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, bands.stops());
        assert_eq!(copy, Some(bands));
    }

    #[test]
    fn from_range_parts_rejects_bad_stops() {
        let parts = |stops: &[Stop]| Bands::from_range_parts(0.0, 100.0, false, stops);
        assert!(parts(&plain(&[0.0, 50.0, 100.0])).is_some());
        // One stop.
        assert_eq!(parts(&plain(&[0.0])), None);
        // The ends are not at the ends of the range.
        assert_eq!(parts(&plain(&[1.0, 50.0, 100.0])), None);
        assert_eq!(parts(&plain(&[0.0, 50.0, 99.0])), None);
        assert_eq!(parts(&plain(&[0.0, 50.0, 100.0, 120.0])), None);
        // The values do not ascend, or are too near.
        assert_eq!(parts(&plain(&[0.0, 60.0, 40.0, 100.0])), None);
        assert_eq!(parts(&plain(&[0.0, 50.0, 50.0, 100.0])), None);
        assert_eq!(parts(&plain(&[0.0, 50.0, 50.5, 100.0])), None);
        assert_eq!(parts(&plain(&[0.0, 0.5, 100.0])), None);
        assert_eq!(parts(&plain(&[0.0, f64::NAN, 100.0])), None);
        // Too many stops.
        let many: Vec<f64> = (0..=MAX_BANDS).map(|i| i as f64 * 2.0).collect();
        let range = |values: &[f64]| {
            let max = values[values.len() - 1];
            Bands::from_range_parts(0.0, max, false, &plain(values))
        };
        assert!(range(&many).is_some());
        let many: Vec<f64> = (0..=MAX_BANDS + 1).map(|i| i as f64 * 2.0).collect();
        assert_eq!(range(&many), None);
        // A hard stop that is not at sea level.
        let mut hard = plain(&[0.0, 50.0, 100.0]);
        hard[1].above = [1, 2, 3];
        assert_eq!(parts(&hard), None);
        hard[1].value = SEA_LEVEL;
        assert_eq!(parts(&hard), None);
        // A bad range.
        assert_eq!(
            Bands::from_range_parts(100.0, 0.0, false, &plain(&[100.0, 0.0])),
            None
        );
        let nan = [stop(f64::NAN, [0; 3]), stop(1.0, [0; 3])];
        assert_eq!(Bands::from_range_parts(f64::NAN, 1.0, false, &nan), None);
    }

    #[test]
    fn the_default_gives_the_levels_of_meters() {
        let bands = Bands::default();
        assert_eq!((bands.min(), bands.max()), (ELEV_MIN, ELEV_MAX));
        for meters in [
            -9000.0, ELEV_MIN, -1234.5, SEA_LEVEL, 0.1, 2500.0, ELEV_MAX, 9000.0,
        ] {
            assert_eq!(bands.level(meters), meters_to_level(meters), "{meters}");
        }
        for level in [0, 1, 32767, 32768, 65535] {
            assert_eq!(bands.value(level), level_to_meters(level), "{level}");
        }
        let same = Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, bands.stops());
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
    fn a_range_with_no_sea_has_no_hard_stop() {
        let bands = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let expect = [
            0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 3500.0, 4000.0,
        ];
        assert_eq!(values(&bands), expect);
        for (j, s) in bands.stops().iter().enumerate() {
            assert_eq!(s.below, s.above, "{j}");
            assert_eq!(s.above, blend(&LAND_PALETTE, j as f64 / 8.0), "{j}");
        }
        let steps = bands.bands();
        assert_eq!(steps.len(), 8);
        assert_eq!((steps[0].min, steps[7].max), (0.0, 4000.0));

        // Each inner stop moves and goes away, also the first one.
        let mut bands = bands;
        assert_eq!(bands.move_to(1, 100.0), 100.0);
        assert_eq!(bands.move_to(1, -100.0), MIN_BAND);
        assert_eq!(bands.move_to(7, 9000.0), 4000.0 - MIN_BAND);
        assert_eq!(bands.add(0.0), None);
        assert_eq!(bands.add(4000.0), None);
        while bands.stops().len() > 2 {
            assert!(bands.remove(1));
        }
        let steps = bands.bands();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].bottom, LAND_PALETTE[0]);
        assert_eq!(steps[0].top, LAND_PALETTE[3]);

        let below = Bands::with_range(-4000.0, 0.0, true).unwrap();
        assert_eq!(below.sea(), None);
        assert_eq!(below.bands()[0].bottom, LAND_PALETTE[0]);
        assert!(below.stops().iter().all(|s| s.below == s.above));
    }

    #[test]
    fn a_range_across_zero_that_is_not_a_height_has_one_gradient() {
        let bands = Bands::with_range(-60.0, 60.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let expect = [-60.0, -45.0, -30.0, -15.0, 0.0, 15.0, 30.0, 45.0, 60.0];
        assert_eq!(values(&bands), expect);
        assert!(bands.stops().iter().all(|s| s.below == s.above));
        // The stop at zero moves and goes away as each other stop.
        let mut bands = bands;
        assert!(!bands.fixed(4));
        assert_eq!(bands.move_to(4, 5.0), 5.0);
        assert!(bands.remove(4));
        assert_eq!(bands.stops().len(), 8);
        let copy = Bands::from_range_parts(-60.0, 60.0, false, bands.stops());
        assert_eq!(copy, Some(bands.clone()));
        // The same parts are not the bands of a height.
        assert_eq!(
            Bands::from_range_parts(-60.0, 60.0, true, bands.stops()),
            None
        );
    }

    #[test]
    fn a_range_across_sea_level_has_a_sea_split() {
        let bands = Bands::with_range(-100.0, 300.0, true).unwrap();
        let expect = [
            -100.0, -75.0, -50.0, -25.0, SEA_LEVEL, 75.0, 150.0, 225.0, 300.0,
        ];
        assert_eq!(values(&bands), expect);
        assert_eq!(bands.sea(), Some(4));
        let steps = bands.bands();
        assert_eq!(steps[3].top, SEA_PALETTE[1]);
        assert_eq!(steps[4].bottom, LAND_PALETTE[0]);
        assert_eq!(steps[0].bottom, SEA_PALETTE[0]);
        assert_eq!(steps[7].top, LAND_PALETTE[3]);

        // A small range has fewer stops.
        let small = Bands::with_range(0.0, 4.0, true).unwrap();
        assert_eq!(values(&small), [0.0, 1.0, 2.0, 3.0, 4.0]);

        assert_eq!(Bands::with_range(5.0, 5.0, true), None);
        assert_eq!(Bands::with_range(5.0, 1.0, true), None);
        assert_eq!(Bands::with_range(0.0, f64::INFINITY, true), None);
        assert_eq!(Bands::with_range(f64::NAN, 1.0, true), None);
    }

    #[test]
    fn from_range_parts_needs_sea_level_only_with_a_sea_split() {
        let bands =
            Bands::from_range_parts(0.0, 4000.0, true, &plain(&[0.0, 250.0, 1000.0, 4000.0]))
                .unwrap();
        assert_eq!(values(&bands), [0.0, 250.0, 1000.0, 4000.0]);
        assert_eq!(bands.sea(), None);
        assert_eq!((bands.min(), bands.max()), (0.0, 4000.0));
        assert_eq!(bands.level(4000.0), 65535);

        // An empty list gives the stops of the range.
        let none = Bands::from_range_parts(0.0, 4000.0, true, &[]);
        assert_eq!(none, Bands::with_range(0.0, 4000.0, true));
        assert_eq!(Bands::from_range_parts(4000.0, 0.0, true, &[]), None);

        // Sea level is an end of the range, and not a hard stop.
        let mut hard = plain(&[0.0, 250.0, 4000.0]);
        hard[0].below = [1, 2, 3];
        assert_eq!(Bands::from_range_parts(0.0, 4000.0, true, &hard), None);

        let split = plain(&[-10.0, 0.0, 250.0, 4000.0]);
        let bands = Bands::from_range_parts(-10.0, 4000.0, true, &split).unwrap();
        assert_eq!(bands.sea(), Some(1));
        let mut hard = split.clone();
        hard[1].below = [1, 2, 3];
        assert!(Bands::from_range_parts(-10.0, 4000.0, true, &hard).is_some());
        assert_eq!(Bands::from_range_parts(-10.0, 4000.0, false, &hard), None);
        let land = plain(&[-10.0, 250.0, 4000.0]);
        assert_eq!(Bands::from_range_parts(-10.0, 4000.0, true, &land), None);
    }

    #[test]
    fn each_stop_keeps_its_own_line() {
        let mut bands = Bands::with_range(-1000.0, 4000.0, true).unwrap();
        assert!(bands.stops().iter().all(|s| s.line == StopLine::default()));
        let sea = bands.sea().unwrap();
        let coast = StopLine {
            on: true,
            weight: 3.0,
            color: [10, 20, 30],
            ..StopLine::default()
        };
        assert!(bands.set_line(sea, coast));
        assert_eq!(bands.stops()[sea].line, coast);
        assert_eq!(bands.stops()[sea + 1].line, StopLine::default());

        // The line moves with the stop, and a new stop has no line.
        let index = bands.add(-600.0).unwrap();
        assert!(index < sea + 1);
        assert_eq!(bands.stops()[sea + 1].line, coast);
        assert!(!bands.stops()[index].line.on);

        // The weight stays in its range.
        let heavy = StopLine {
            weight: 100.0,
            ..coast
        };
        assert!(bands.set_line(0, heavy));
        assert_eq!(bands.stops()[0].line.weight, *StopLine::WEIGHTS.end());
        assert!(!bands.set_line(99, coast));
    }

    #[test]
    fn a_darker_line_takes_the_color_of_its_stop() {
        let mut bands = Bands::with_range(-1000.0, 4000.0, true).unwrap();
        let sea = bands.sea().unwrap();
        let stop = bands.stops()[sea];
        assert_ne!(stop.below, stop.above);
        let line = StopLine {
            on: true,
            darker: true,
            darken: 0.5,
            ..StopLine::default()
        };
        bands.set_line(sea, line);
        let half = |c: Rgb| c.map(|c| (f32::from(c) * 0.5).round() as u8);
        assert_eq!(bands.line_color(sea), Some(half(stop.below)));
        let land = bands.stops()[sea + 1];
        bands.set_line(sea + 1, line);
        assert_eq!(bands.line_color(sea + 1), Some(half(land.above)));

        // The darker color follows the stop, and the amount stays in range.
        bands.set_color(sea + 1, Side::Above, [200, 100, 50]);
        assert_eq!(bands.line_color(sea + 1), Some([100, 50, 25]));
        bands.set_line(
            sea + 1,
            StopLine {
                darken: 3.0,
                ..line
            },
        );
        assert_eq!(bands.line_color(sea + 1), Some([0; 3]));
        bands.set_line(
            sea + 1,
            StopLine {
                darker: false,
                ..line
            },
        );
        assert_eq!(bands.line_color(sea + 1), Some(StopLine::default().color));
        assert_eq!(bands.line_color(99), None);
    }

    fn band_colors(bands: &Bands) -> Vec<Rgb> {
        bands.bands().iter().map(|b| b.color).collect()
    }

    fn greys(n: usize) -> Vec<Rgb> {
        (0..n).map(|i| [(i * 7) as u8; 3]).collect()
    }

    #[test]
    fn painted_with_the_same_count_keeps_the_stops() {
        let mut bands = Bands::default();
        let line = StopLine {
            on: true,
            ..StopLine::default()
        };
        bands.set_line(3, line);
        let colors = greys(bands.stops().len() - 1);
        let painted = bands.painted(&colors).unwrap();
        assert_eq!(values(&painted), values(&bands));
        assert_eq!(band_colors(&painted), colors);
        assert_eq!(painted.stops()[3].line, line);
    }

    #[test]
    fn painted_splits_the_bands_at_sea_level() {
        let bands = Bands::with_range(-1000.0, 3000.0, true).unwrap();
        let colors = greys(4);
        let painted = bands.painted(&colors).unwrap();
        assert_eq!(values(&painted), [-1000.0, 0.0, 1000.0, 2000.0, 3000.0]);
        assert_eq!(painted.sea(), Some(1));
        assert_eq!(band_colors(&painted), colors);
    }

    #[test]
    fn painted_gives_the_blue_colors_to_the_sea() {
        let bands = Bands::default();
        let colors = [
            [33, 113, 181],
            [199, 233, 192],
            [116, 196, 118],
            [35, 139, 69],
            [140, 81, 10],
        ];
        let painted = bands.painted(&colors).unwrap();
        let land = ELEV_MAX / 4.0;
        assert_eq!(
            values(&painted),
            [ELEV_MIN, 0.0, land, 2.0 * land, 3.0 * land, ELEV_MAX]
        );
        assert_eq!(band_colors(&painted), colors);
    }

    #[test]
    fn painted_with_no_split_has_even_spacing() {
        let bands = Bands::with_range(0.0, 100.0, false).unwrap();
        let colors = greys(5);
        let painted = bands.painted(&colors).unwrap();
        assert_eq!(values(&painted), [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(band_colors(&painted), colors);
    }

    #[test]
    fn painted_refuses_too_few_or_too_many_colors() {
        let bands = Bands::default();
        assert!(bands.painted(&[]).is_none());
        assert!(bands.painted(&greys(1)).is_none());
        assert!(bands.painted(&greys(MAX_BANDS + 1)).is_none());
        assert!(bands.painted(&greys(MAX_BANDS)).is_some());
        let small = Bands::with_range(0.0, 4.0, false).unwrap();
        assert!(small.painted(&greys(8)).is_none());
    }
}
