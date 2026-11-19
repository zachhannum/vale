//! The value range and the color stops of the preview.

use std::ops::Range;

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

/// One stop: the edge between two bands, or an end of the range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// The value, in the unit of the layer.
    pub value: f64,
    /// The line where the value crosses the stop. The stops at the ends
    /// have no line.
    pub line: StopLine,
}

/// A line at a color stop. It keeps its values while it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopLine {
    pub on: bool,
    /// The weight, in points.
    pub weight: f32,
    pub color: Rgb,
    /// The line has the color of the band above the stop, darker by
    /// `darken`, in place of `color`.
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

/// The bands that a ramp of colors colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    All,
    /// The bands below sea level.
    Sea,
    /// The bands above sea level.
    Land,
}

/// The colors of a ramp and what the bands take from them.
#[derive(Clone, Copy, Debug)]
pub struct RampColors<'a> {
    pub colors: &'a [Rgb],
    /// One label for each color, or none.
    pub labels: &'a [String],
    /// The colors are for classes with no order, so they do not blend, and
    /// they do not split at sea level.
    pub qualitative: bool,
    /// The number of colors below sea level. `None`: the colors that are
    /// mostly blue at the start of the ramp.
    pub sea: Option<usize>,
}

impl RampColors<'_> {
    /// The number of colors below sea level, if the ramp splits there.
    fn sea_colors(&self) -> Option<usize> {
        if self.qualitative {
            return None;
        }
        let n = self.colors.len();
        match self.sea {
            Some(sea) => (1..n).contains(&sea).then_some(sea),
            None => sea_colors(self.colors),
        }
    }
}

/// One step of the preview, from one stop to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The lower and upper value, in the unit of the layer.
    pub min: f64,
    pub max: f64,
    /// The color of the band. The smooth preview has this color at the
    /// middle of the band.
    pub color: Rgb,
    /// The colors at `min` and at `max` in the smooth preview. At an end of
    /// the range and at sea level, the edge has the color of the band.
    pub bottom: Rgb,
    pub top: Rgb,
}

impl Band {
    /// The color of the smooth preview at a value in the band. It goes from
    /// `bottom` to `color` at the middle, and on to `top`.
    pub fn gradient_at(&self, value: f64) -> Rgb {
        let mid = (self.min + self.max) / 2.0;
        if value < mid {
            mix(
                self.bottom,
                self.color,
                (value - self.min) / (mid - self.min),
            )
        } else {
            mix(self.color, self.top, (value - mid) / (self.max - mid))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bands {
    /// The values of level 0 and of the highest level.
    min: f64,
    max: f64,
    /// The stops in ascending order. The first stop is at `min` and the last
    /// stop is at `max`.
    stops: Vec<Stop>,
    /// The color of each band, from the lowest band. Band `i` runs from stop
    /// `i` to stop `i + 1`.
    colors: Vec<Rgb>,
    /// The name of each band, from the lowest band, or none.
    names: Vec<String>,
    /// The bands have a sea split: sea level is a stop that stays.
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
    /// stops of `with_range`. The stops at the ends lose their lines.
    ///
    /// Returns `None` if the range is empty or not finite, or if the stops
    /// break a rule:
    /// - There are 2 to `MAX_BANDS + 1` stops.
    /// - The first stop is at `min`, and the last stop is at `max`.
    /// - The values ascend, with at least `MIN_BAND` between two stops.
    /// - With a sea split, a stop is at sea level.
    /// - There is one color for each band.
    pub fn from_range_parts(
        min: f64,
        max: f64,
        sea: bool,
        stops: &[Stop],
        colors: &[Rgb],
    ) -> Option<Bands> {
        if stops.is_empty() {
            return Bands::with_range(min, max, sea);
        }
        if !(min.is_finite() && max.is_finite() && min < max) {
            return None;
        }
        let split = splits(min, max, sea);
        let valid = (2..=MAX_BANDS + 1).contains(&stops.len())
            && colors.len() + 1 == stops.len()
            && stops[0].value == min
            && stops[stops.len() - 1].value == max
            && stops
                .windows(2)
                .all(|w| w[1].value - w[0].value >= MIN_BAND)
            && (!split || stops.iter().any(|s| s.value == SEA_LEVEL));
        if !valid {
            return None;
        }
        let mut stops = stops.to_vec();
        let last = stops.len() - 1;
        stops[0].line = StopLine::default();
        stops[last].line = StopLine::default();
        Some(Bands {
            min,
            max,
            stops,
            colors: colors.to_vec(),
            names: Vec::new(),
            split,
        })
    }

    /// The bands with stops at `min`, at each of `limits`, and at `max`. The
    /// limits ascend, and with `split`, sea level is one of them. Each band
    /// takes its color from the default palette by its index on its side
    /// of sea level.
    fn from_palette(min: f64, max: f64, split: bool, limits: &[f64]) -> Bands {
        let stops: Vec<Stop> = std::iter::once(min)
            .chain(limits.iter().copied())
            .chain(std::iter::once(max))
            .map(|value| Stop {
                value,
                line: StopLine::default(),
            })
            .collect();
        let bands = stops.len() - 1;
        let sea = if split {
            stops.partition_point(|s| s.value < SEA_LEVEL)
        } else {
            0
        };
        let land = bands - sea;
        let colors = (0..bands)
            .map(|i| {
                if i < sea {
                    blend(&SEA_PALETTE, (i + 1) as f64 / sea as f64)
                } else {
                    blend(&LAND_PALETTE, (i - sea) as f64 / land as f64)
                }
            })
            .collect();
        Bands {
            min,
            max,
            stops,
            colors,
            names: Vec::new(),
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

    /// The color of each band, from the lowest band.
    pub fn colors(&self) -> &[Rgb] {
        &self.colors
    }

    /// The index of the stop at sea level. Only a range of heights with
    /// values on both sides of sea level has a sea split.
    pub fn sea(&self) -> Option<usize> {
        self.split
            .then(|| self.stops.partition_point(|s| s.value < SEA_LEVEL))
    }

    /// Whether the stop at `index` is an end of the range.
    fn end(&self, index: usize) -> bool {
        index == 0 || index + 1 == self.stops.len()
    }

    /// Whether the stop at `index` does not move: the two ends and sea level.
    pub fn fixed(&self, index: usize) -> bool {
        self.end(index) || Some(index) == self.sea()
    }

    /// The color of the smooth preview at a value. A value out of the range
    /// gets the color of the nearest end.
    pub fn color_at(&self, value: f64) -> Rgb {
        if value.is_nan() || value <= self.min {
            return self.colors[0];
        }
        if value >= self.max {
            return self.colors[self.colors.len() - 1];
        }
        let i = self.stops.partition_point(|s| s.value <= value) - 1;
        self.band(i).gradient_at(value)
    }

    /// Adds a stop, and returns its index. The stop splits a band, and the
    /// two parts keep the color and the name of the band. Returns `None` if the value is
    /// less than `MIN_BAND` from another stop, if it is not finite, or if
    /// the list is full.
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
        let index = self.stops.partition_point(|s| s.value < value);
        let stop = Stop {
            value,
            line: StopLine::default(),
        };
        self.stops.insert(index, stop);
        self.colors.insert(index, self.colors[index - 1]);
        if !self.names.is_empty() {
            self.names.insert(index, self.names[index - 1].clone());
        }
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

    /// Removes a stop. The bands below it and above it join, with the color
    /// and the name of the band below. Returns `false` for a fixed stop and for an index
    /// out of range.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.stops.len() || self.fixed(index) {
            return false;
        }
        self.stops.remove(index);
        self.colors.remove(index);
        if !self.names.is_empty() {
            self.names.remove(index);
        }
        true
    }

    /// The name of each band, from the lowest band, or none.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The name of a band. A band with no name, and an index out of range,
    /// give an empty name.
    pub fn name(&self, band: usize) -> &str {
        self.names.get(band).map_or("", String::as_str)
    }

    /// Sets the name of a band. Returns `false` for an index out of range.
    pub fn set_name(&mut self, band: usize, name: &str) -> bool {
        if band >= self.colors.len() {
            return false;
        }
        if self.names.is_empty() {
            self.names = vec![String::new(); self.colors.len()];
        }
        self.names[band] = name.to_owned();
        self.drop_blank_names();
        true
    }

    /// The bands with the names `names`. Names that are not one for each
    /// band, or that are all empty, give no names.
    pub fn with_names(mut self, names: &[String]) -> Bands {
        self.names = if names.len() == self.colors.len() {
            names.to_vec()
        } else {
            Vec::new()
        };
        self.drop_blank_names();
        self
    }

    fn drop_blank_names(&mut self) {
        if self.names.iter().all(|n| n.is_empty()) {
            self.names.clear();
        }
    }

    /// The names of the bands as a list with one name for each band.
    fn padded_names(&self) -> Vec<String> {
        match self.names.is_empty() {
            true => vec![String::new(); self.colors.len()],
            false => self.names.clone(),
        }
    }

    /// Sets the color of a band. Returns `false` for an index out of range.
    pub fn set_color(&mut self, band: usize, color: Rgb) -> bool {
        let Some(c) = self.colors.get_mut(band) else {
            return false;
        };
        *c = color;
        true
    }

    /// Sets the line of a stop. The weight stays in `StopLine::WEIGHTS`.
    /// Returns `false` for a stop at an end and for an index out of range.
    pub fn set_line(&mut self, index: usize, line: StopLine) -> bool {
        if index >= self.stops.len() || self.end(index) {
            return false;
        }
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
        self.stops[index].line = StopLine {
            weight,
            darken,
            ..line
        };
        true
    }

    /// The color of the line of a stop, or `None` for a stop at an end and
    /// for an index out of range.
    pub fn line_color(&self, index: usize) -> Option<Rgb> {
        if index >= self.stops.len() || self.end(index) {
            return None;
        }
        let line = self.stops[index].line;
        if !line.darker {
            return Some(line.color);
        }
        let keep = 1.0 - line.darken;
        Some(self.colors[index].map(|c| (f32::from(c) * keep).round() as u8))
    }

    /// The bands that a ramp colors. With a sea split, a ramp with no
    /// colors that are mostly blue is a land ramp, and a ramp with only such
    /// colors is a sea ramp.
    pub fn part(&self, colors: &[Rgb]) -> Part {
        if !self.split {
            return Part::All;
        }
        match colors.iter().filter(|c| is_blue(**c)).count() {
            0 => Part::Land,
            blue if blue == colors.len() => Part::Sea,
            _ => Part::All,
        }
    }

    /// The indices of the bands of a part.
    pub fn part_bands(&self, part: Part) -> Range<usize> {
        let m = self.colors.len();
        match (part, self.sea()) {
            (Part::Sea, Some(sea)) => 0..sea,
            (Part::Land, Some(sea)) => sea..m,
            _ => 0..m,
        }
    }

    /// The bands with one band of each color, from the lowest band to the
    /// highest, in the part of the ramp. The bands of the other part do not
    /// change. With the same number of bands, and with bands that cross sea
    /// level the same number below it, the stops keep their values and
    /// their lines. Otherwise, the stops of the part have even spacing.
    /// A ramp for all bands with a sea split gives the sea its first colors
    /// that are mostly blue, or if there are none, the part of the bands of
    /// its part of the range. Returns `None` if there are no colors, if there
    /// are too many bands, or if the range has no room for the stops.
    pub fn painted(&self, colors: &[Rgb]) -> Option<Bands> {
        self.painted_part(colors, &[], self.part(colors))
    }

    /// `painted` for a part that the user chose. With no sea split, each
    /// part is all bands. `labels` has one label for each color, or none.
    /// The new bands of the part take the labels of their colors as their
    /// names.
    pub fn painted_part(&self, colors: &[Rgb], labels: &[String], part: Part) -> Option<Bands> {
        let all = 0..self.colors.len();
        self.painted_in(colors, labels, part, all, self.min, self.max)
    }

    /// `painted_part` for the bands of `bands` in the part. The ramp spreads
    /// its colors over the values from `lo` to `hi`, with even spacing. The
    /// values stay between the stops at the two ends of these bands, so the
    /// first and the last new band reach out to those stops. With a sea
    /// split, the values of the land stay at sea level or above, and the
    /// values of the sea stay at sea level or below. Bands that cross sea
    /// level split the colors as `painted` does, with even spacing on each
    /// side from `lo` to sea level and from sea level to `hi`. The stops and
    /// the bands out of these bands do not change. Returns `None` if no band
    /// is in both, if `lo` is not less than `hi`, or for the reasons of
    /// `painted`.
    pub fn painted_in(
        &self,
        colors: &[Rgb],
        labels: &[String],
        part: Part,
        bands: Range<usize>,
        lo: f64,
        hi: f64,
    ) -> Option<Bands> {
        self.painted_split(colors, labels, part, bands, lo, hi, sea_colors(colors))
    }

    /// The part that a ramp colors by default: `part` of its colors, or all
    /// bands for a ramp with a sea level of its own.
    pub fn ramp_part(&self, ramp: &RampColors) -> Part {
        match ramp.sea {
            Some(_) if self.split && ramp.sea_colors().is_some() => Part::All,
            _ => self.part(ramp.colors),
        }
    }

    /// `stretched_in` for a ramp, with its sea colors below sea level.
    pub fn stretched_ramp(
        &self,
        ramp: &RampColors,
        part: Part,
        bands: Range<usize>,
    ) -> Option<Bands> {
        let (colors, labels) = (ramp.colors, ramp.labels);
        self.stretched_split(
            colors,
            labels,
            part,
            bands,
            ramp.qualitative,
            ramp.sea_colors(),
        )
    }

    /// `painted_in` for a ramp, with `count` new bands in the place of one
    /// band for each color. The colors are `count` even steps along the
    /// ramp, or the colors in order for a qualitative ramp, and each band
    /// takes the label of its nearest color. With bands that cross sea
    /// level, the sea colors and the land colors of the ramp each get a
    /// share of `count`, at least one, in the ratio of their numbers of
    /// colors.
    pub fn painted_count(
        &self,
        ramp: &RampColors,
        part: Part,
        bands: Range<usize>,
        (lo, hi): (f64, f64),
        count: usize,
    ) -> Option<Bands> {
        let (colors, labels, qualitative) = (ramp.colors, ramp.labels, ramp.qualitative);
        let n = colors.len();
        let r = self.window(part, bands.clone());
        let crosses = self.sea().is_some_and(|sea| r.start < sea && sea < r.end);
        let blue = ramp.sea_colors().filter(|_| crosses && count >= 2);
        // The colors of each side of the ramp, and the number of new bands
        // of the side.
        let sides: Vec<(Range<usize>, usize)> = match blue {
            Some(b) => {
                let sea = ((count * b) as f64 / n as f64).round() as usize;
                let sea = sea.clamp(1, count - 1);
                vec![(0..b, sea), (b..n, count - sea)]
            }
            None => vec![(0..n, count)],
        };
        let labels = match labels.len() == n {
            true => labels.to_vec(),
            false => vec![String::new(); n],
        };
        let (mut new_colors, mut new_labels) = (Vec::new(), Vec::new());
        for (side, k) in &sides {
            let (side, k) = (side.clone(), *k);
            new_colors.extend(sample_ramp(&colors[side.clone()], k, qualitative));
            let picks = sample_indices(side.len(), k, qualitative);
            new_labels.extend(picks.map(|i| labels[side.start + i].clone()));
        }
        let blue = blue.map(|_| sides[0].1);
        self.painted_split(&new_colors, &new_labels, part, bands, lo, hi, blue)
    }

    /// `painted_in` with the first `blue` colors below sea level, for bands
    /// that cross it. With no `blue`, the colors split in the ratio of the
    /// values below and above sea level.
    #[allow(clippy::too_many_arguments)]
    fn painted_split(
        &self,
        colors: &[Rgb],
        labels: &[String],
        part: Part,
        bands: Range<usize>,
        lo: f64,
        hi: f64,
        blue: Option<usize>,
    ) -> Option<Bands> {
        let n = colors.len();
        let r = self.window(part, bands);
        if n == 0 || n > MAX_BANDS || r.is_empty() {
            return None;
        }
        let blue = blue.filter(|b| (1..n).contains(b));
        let sea = self.sea().filter(|sea| r.start < *sea && *sea < r.end);
        if n == r.len() && sea.is_none_or(|sea| blue.is_none_or(|b| r.start + b == sea)) {
            return self.stretched_split(colors, labels, part, r, false, blue);
        }
        let (bottom, top) = (self.stops[r.start].value, self.stops[r.end].value);
        let (lo, hi) = (lo.max(bottom), hi.min(top));
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return None;
        }
        let labels = match labels.len() == n {
            true => labels.to_vec(),
            false => vec![String::new(); n],
        };
        let fresh = |value: f64| Stop {
            value,
            line: StopLine::default(),
        };
        // The values between `from` and `to` for `count` even bands.
        let inner = |from: f64, to: f64, count: usize| {
            let step = (to - from) / count as f64;
            (1..count).map(move |i| from + step * i as f64)
        };
        let crosses = self.sea().is_some_and(|sea| r.start < sea && sea < r.end);
        let sides: Vec<(f64, f64, usize)> = if crosses {
            if n < 2 {
                return None;
            }
            let part = (SEA_LEVEL - lo) / (hi - lo);
            let sea = blue.unwrap_or_else(|| ((n as f64 * part).round() as usize).clamp(1, n - 1));
            vec![(lo, SEA_LEVEL, sea), (SEA_LEVEL, hi, n - sea)]
        } else {
            vec![(lo, hi, n)]
        };
        let mut values = Vec::new();
        for (i, (from, to, count)) in sides.into_iter().enumerate() {
            if i > 0 {
                values.push(from);
            }
            values.extend(inner(from, to, count));
        }
        let stops: Vec<Stop> = self.stops[..=r.start]
            .iter()
            .copied()
            .chain(values.into_iter().map(fresh))
            .chain(self.stops[r.end..].iter().copied())
            .collect();
        let old = &self.colors;
        let all = [&old[..r.start], colors, &old[r.end..]].concat();
        let names = self.padded_names();
        let names = [&names[..r.start], &labels[..], &names[r.end..]].concat();
        Bands::from_range_parts(self.min, self.max, self.split, &stops, &all)
            .map(|b| b.with_names(&names))
    }

    /// The bands with the same stops, values, and lines, and the colors of
    /// a ramp over the bands of its part. With more bands than colors, the
    /// first band takes the first color, the last band takes the last
    /// color, and the bands between take the blend at their place along
    /// the ramp. With fewer bands, each band takes the color at its middle
    /// along the ramp, and a color drops out. The bands of the other part
    /// do not change. Returns `None` if there are no colors.
    pub fn stretched(&self, colors: &[Rgb]) -> Option<Bands> {
        self.stretched_part(colors, &[], self.part(colors))
    }

    /// `stretched` for a part that the user chose. With no sea split, each
    /// part is all bands. `labels` has one label for each color, or none.
    /// If a label is not empty, each band of the part takes the label of
    /// its color as its name. Otherwise, the bands keep their names.
    pub fn stretched_part(&self, colors: &[Rgb], labels: &[String], part: Part) -> Option<Bands> {
        self.stretched_in(colors, labels, part, 0..self.colors.len(), false)
    }

    /// `stretched_part` for the bands of `bands` in the part. The ramp
    /// spans these bands, and the other bands do not change. The colors of
    /// a `qualitative` ramp do not blend: with more bands than colors, the
    /// colors repeat in order. Returns `None` if there are no colors or if
    /// no band is in both.
    ///
    /// With bands that cross sea level, a ramp that is not `qualitative` and
    /// that starts with colors that are mostly blue puts those colors on the
    /// bands below sea level and its other colors on the bands above it.
    pub fn stretched_in(
        &self,
        colors: &[Rgb],
        labels: &[String],
        part: Part,
        bands: Range<usize>,
        qualitative: bool,
    ) -> Option<Bands> {
        let blue = sea_colors(colors).filter(|_| !qualitative);
        self.stretched_split(colors, labels, part, bands, qualitative, blue)
    }

    /// `stretched_in` with the first `blue` colors below sea level.
    fn stretched_split(
        &self,
        colors: &[Rgb],
        labels: &[String],
        part: Part,
        bands: Range<usize>,
        qualitative: bool,
        blue: Option<usize>,
    ) -> Option<Bands> {
        let n = colors.len();
        let range = self.window(part, bands);
        if n == 0 || range.is_empty() {
            return None;
        }
        let sea = self
            .sea()
            .filter(|sea| range.start < *sea && *sea < range.end);
        if let (Some(sea), Some(b)) = (sea, blue.filter(|b| (1..n).contains(b))) {
            let side = |from: usize, to: usize| match labels.len() == n {
                true => labels[from..to].to_vec(),
                false => Vec::new(),
            };
            let below = self.stretched_split(
                &colors[..b],
                &side(0, b),
                Part::Sea,
                range.start..sea,
                qualitative,
                None,
            )?;
            return below.stretched_split(
                &colors[b..],
                &side(b, n),
                Part::Land,
                sea..range.end,
                qualitative,
                None,
            );
        }
        let m = range.len();
        let labeled = labels.len() == n && labels.iter().any(|l| !l.is_empty());
        let mut all = self.colors.clone();
        let mut names = self.padded_names();
        for (k, i) in range.enumerate() {
            let c = if n >= m {
                (2 * k + 1) * n / (2 * m)
            } else if qualitative {
                k % n
            } else {
                (k * (n - 1) + (m - 1) / 2) / (m - 1)
            };
            all[i] = if n > 1 && n < m && !qualitative {
                blend(colors, k as f64 / (m - 1) as f64)
            } else {
                colors[c]
            };
            if labeled {
                names[i] = labels[c].clone();
            }
        }
        Bands::from_range_parts(self.min, self.max, self.split, &self.stops, &all)
            .map(|b| b.with_names(&names))
    }

    /// The bands with the inner stops moved so that they span the values
    /// from `lo` to `hi`, with the same relative spacing. The ends of the
    /// range, the colors, the names, and the lines do not change. With a
    /// sea split, sea level stays, and each side moves on its own:
    /// - If `lo` is below sea level and `hi` is above it, the sea goes to
    ///   the values from `lo` to sea level, and the land goes to the values
    ///   from sea level to `hi`.
    /// - If `lo` is at sea level or above, only the land moves, to the
    ///   values from `lo` to `hi`.
    /// - If `hi` is at sea level or below, only the sea moves, to the
    ///   values from `lo` to `hi`.
    ///
    /// The values out of the range go to its nearest end. Returns `None` if
    /// `lo` is not less than `hi`, if a value is not finite, or if two stops
    /// come closer than `MIN_BAND`.
    pub fn fitted_to(&self, lo: f64, hi: f64) -> Option<Bands> {
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return None;
        }
        let (lo, hi) = (lo.max(self.min), hi.min(self.max));
        // The map of the values from `a..b` to `c..d`.
        let map = |a: f64, b: f64, c: f64, d: f64| move |v: f64| c + (v - a) * (d - c) / (b - a);
        let sea = self.sea();
        let mut stops = self.stops.clone();
        for (i, stop) in stops.iter_mut().enumerate() {
            if self.fixed(i) {
                continue;
            }
            let v = stop.value;
            stop.value = match sea {
                None => map(self.min, self.max, lo, hi)(v),
                Some(s) => {
                    let below = i < s;
                    if lo < SEA_LEVEL && SEA_LEVEL < hi {
                        match below {
                            true => map(self.min, SEA_LEVEL, lo, SEA_LEVEL)(v),
                            false => map(SEA_LEVEL, self.max, SEA_LEVEL, hi)(v),
                        }
                    } else if lo >= SEA_LEVEL && !below {
                        map(SEA_LEVEL, self.max, lo, hi)(v)
                    } else if hi <= SEA_LEVEL && below {
                        map(self.min, SEA_LEVEL, lo, hi)(v)
                    } else {
                        v
                    }
                }
            };
        }
        Bands::from_range_parts(self.min, self.max, self.split, &stops, &self.colors)
            .map(|b| b.with_names(&self.names))
    }

    /// The bands of `bands` that are in a part.
    fn window(&self, part: Part, bands: Range<usize>) -> Range<usize> {
        let p = self.part_bands(part);
        bands.start.max(p.start)..bands.end.min(p.end)
    }

    /// The band at `i`, from stop `i` to stop `i + 1`.
    fn band(&self, i: usize) -> Band {
        let (lo, hi) = (self.stops[i].value, self.stops[i + 1].value);
        let color = self.colors[i];
        let width = |j: usize| self.stops[j + 1].value - self.stops[j].value;
        let hard = |stop: usize| self.end(stop) || Some(stop) == self.sea();
        // The edge color is on the line from the middle of one band to the
        // middle of the next.
        let edge = |below: usize| {
            let (a, b) = (width(below), width(below + 1));
            mix(self.colors[below], self.colors[below + 1], a / (a + b))
        };
        Band {
            min: lo,
            max: hi,
            color,
            bottom: if hard(i) { color } else { edge(i - 1) },
            top: if hard(i + 1) { color } else { edge(i) },
        }
    }

    /// The bands from `min` to `max`, in ascending order. There is one band
    /// fewer than there are stops.
    pub fn bands(&self) -> Vec<Band> {
        (0..self.colors.len()).map(|i| self.band(i)).collect()
    }
}

/// Whether a color is mostly blue.
fn is_blue(c: Rgb) -> bool {
    c[2] > c[0] && c[2] >= c[1]
}

/// The number of colors that are mostly blue at the start of a ramp, if
/// the ramp also has other colors.
fn sea_colors(colors: &[Rgb]) -> Option<usize> {
    let blue = colors.iter().take_while(|c| is_blue(**c)).count();
    (1..colors.len()).contains(&blue).then_some(blue)
}

/// The index of the color of a ramp of `n` colors for each of `count` new
/// colors: the nearest color along the ramp, or the colors in order for a
/// `qualitative` ramp.
fn sample_indices(n: usize, count: usize, qualitative: bool) -> impl Iterator<Item = usize> {
    let last = n.saturating_sub(1);
    (0..count).map(move |k| match (qualitative, count) {
        (true, _) => k % n.max(1),
        (false, 1) => last / 2,
        (false, _) => (k * last + (count - 1) / 2) / (count - 1),
    })
}

/// `count` colors of a ramp: even steps from its first color to its last
/// color, or its colors in order, again from the first color after the
/// last, for a `qualitative` ramp. One color is the middle of the ramp.
pub fn sample_ramp(colors: &[Rgb], count: usize, qualitative: bool) -> Vec<Rgb> {
    if colors.is_empty() {
        return Vec::new();
    }
    if qualitative {
        return colors.iter().copied().cycle().take(count).collect();
    }
    if colors.len() == 1 {
        return vec![colors[0]; count];
    }
    (0..count)
        .map(|k| match count {
            1 => blend(colors, 0.5),
            _ => blend(colors, k as f64 / (count - 1) as f64),
        })
        .collect()
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

    fn stop(value: f64) -> Stop {
        Stop {
            value,
            line: StopLine::default(),
        }
    }

    /// Stops at the values.
    fn plain(values: &[f64]) -> Vec<Stop> {
        values.iter().map(|&v| stop(v)).collect()
    }

    /// The bands at the values, with one grey color.
    fn grey(min: f64, max: f64, sea: bool, values: &[f64]) -> Option<Bands> {
        let colors = vec![[9; 3]; values.len().saturating_sub(1)];
        Bands::from_range_parts(min, max, sea, &plain(values), &colors)
    }

    fn copy(bands: &Bands) -> Option<Bands> {
        Bands::from_range_parts(
            bands.min(),
            bands.max(),
            bands.sea().is_some(),
            bands.stops(),
            bands.colors(),
        )
    }

    fn band_colors(bands: &Bands) -> Vec<Rgb> {
        bands.bands().iter().map(|b| b.color).collect()
    }

    const BLUE: Rgb = [0, 0, 200];

    fn greys(n: usize) -> Vec<Rgb> {
        (0..n).map(|i| [(i * 7) as u8; 3]).collect()
    }

    /// The colors of `m` bands that blend the colors of a ramp from the
    /// first band to the last.
    fn spread(colors: &[Rgb], m: usize) -> Vec<Rgb> {
        (0..m)
            .map(|k| blend(colors, k as f64 / (m - 1) as f64))
            .collect()
    }

    #[test]
    fn the_default_has_sea_level() {
        let bands = Bands::default();
        assert_eq!(bands.stops().len(), 13);
        assert_eq!(bands.colors().len(), 12);
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
    fn the_default_keeps_the_old_band_colors() {
        // The colors of the bands before each band had its own color.
        let old = [
            [51, 80, 122],
            [86, 115, 151],
            [120, 151, 181],
            [155, 186, 210],
            [190, 222, 240],
            [108, 164, 100],
            [157, 180, 121],
            [206, 197, 141],
            [201, 175, 127],
            [171, 135, 97],
            [164, 127, 99],
            [205, 186, 170],
        ];
        assert_eq!(Bands::default().colors(), old);
        assert_eq!(band_colors(&Bands::default()), old);
        let split = Bands::with_range(-100.0, 300.0, true).unwrap();
        assert_eq!(
            split.colors(),
            [
                [60, 89, 129],
                [103, 133, 166],
                [147, 178, 203],
                [190, 222, 240],
                [108, 164, 100],
                [194, 193, 136],
                [186, 155, 112],
                [174, 142, 117],
            ]
        );
        let land = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(
            land.colors(),
            [
                [108, 164, 100],
                [151, 178, 118],
                [194, 193, 136],
                [213, 190, 139],
                [186, 155, 112],
                [159, 120, 85],
                [174, 142, 117],
                [210, 193, 179],
            ]
        );
    }

    #[test]
    fn each_band_has_one_color() {
        let mut bands = Bands::default();
        let red = [255, 0, 0];
        for band in 0..bands.colors().len() {
            let before = band_colors(&bands);
            assert!(bands.set_color(band, red));
            let after = band_colors(&bands);
            for (i, (a, b)) in before.iter().zip(&after).enumerate() {
                let want = if i == band { red } else { *a };
                assert_eq!(*b, want, "band {band}, band {i}");
            }
            assert_eq!(bands.colors(), after);
        }
        assert!(!bands.set_color(bands.colors().len(), red));
    }

    #[test]
    fn gradient_blends_between_middles_flat_at_ends_hard_at_sea() {
        // Bands of 10, 30, 10 and 20 with sea level at 0.
        let stops = plain(&[-40.0, -30.0, 0.0, 10.0, 30.0]);
        let colors = [[0, 0, 0], [80, 160, 40], [200, 0, 100], [0, 100, 200]];
        let bands = Bands::from_range_parts(-40.0, 30.0, true, &stops, &colors).unwrap();
        assert_eq!(bands.sea(), Some(2));
        let steps = bands.bands();

        // Flat from the bottom end to the middle of the lowest band.
        for v in [-50.0, -40.0, -37.0, -35.0] {
            assert_eq!(bands.color_at(v), [0, 0, 0], "{v}");
        }
        assert_eq!(bands.color_at(f64::NAN), [0, 0, 0]);
        assert_eq!(steps[0].bottom, [0, 0, 0]);
        // From the middle at -35 to the middle at -15: the edge at -30 is a
        // quarter of the way.
        assert_eq!(steps[0].top, [20, 40, 10]);
        assert_eq!(steps[1].bottom, [20, 40, 10]);
        assert_eq!(bands.color_at(-30.0), [20, 40, 10]);
        assert_eq!(bands.color_at(-25.0), [40, 80, 20]);
        assert_eq!(bands.color_at(-15.0), [80, 160, 40]);
        // Flat from the middle to sea level, then a hard edge.
        assert_eq!(steps[1].top, [80, 160, 40]);
        assert_eq!(bands.color_at(-1e-9), [80, 160, 40]);
        assert_eq!(steps[2].bottom, [200, 0, 100]);
        assert_eq!(bands.color_at(SEA_LEVEL), [200, 0, 100]);
        assert_eq!(bands.color_at(5.0), [200, 0, 100]);
        // From the middle at 5 to the middle at 20: the edge at 10 is a third
        // of the way.
        assert_eq!(steps[2].top, [133, 33, 133]);
        assert_eq!(bands.color_at(20.0), [0, 100, 200]);
        // Flat from the middle of the top band to the top end.
        for v in [20.0, 25.0, 30.0, 99.0] {
            assert_eq!(bands.color_at(v), [0, 100, 200], "{v}");
        }
        assert_eq!(steps[3].top, [0, 100, 200]);
        assert_eq!(steps[3].gradient_at(30.0), [0, 100, 200]);
    }

    #[test]
    fn the_ends_have_no_line() {
        let mut bands = Bands::default();
        let last = bands.stops().len() - 1;
        let line = StopLine {
            on: true,
            ..StopLine::default()
        };
        assert!(!bands.set_line(0, line));
        assert!(!bands.set_line(last, line));
        assert_eq!(bands.line_color(0), None);
        assert_eq!(bands.line_color(last), None);
        assert_eq!(bands, Bands::default());

        // A saved copy with lines at the ends loses them.
        let mut stops = bands.stops().to_vec();
        stops[0].line = line;
        stops[last].line = line;
        stops[3].line = line;
        let saved =
            Bands::from_range_parts(ELEV_MIN, ELEV_MAX, true, &stops, bands.colors()).unwrap();
        assert_eq!(saved.stops()[0].line, StopLine::default());
        assert_eq!(saved.stops()[last].line, StopLine::default());
        assert_eq!(saved.stops()[3].line, line);
    }

    #[test]
    fn add_inserts_in_order() {
        let mut bands = Bands::default();
        assert_eq!(bands.add(100.0), Some(6));
        assert_eq!(bands.add(-5000.0), Some(1));
        assert_eq!(bands.add(5000.0), Some(14));
        assert!(sorted(&bands));
        assert_eq!(bands.stops().len(), 16);
        assert_eq!(bands.colors().len(), 15);
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
    fn add_splits_a_band_with_its_color() {
        let mut bands = Bands::default();
        let before = bands.colors().to_vec();
        // 1234 is in band 8, from 1000 to 2000.
        let index = bands.add(1234.0).unwrap();
        assert_eq!(index, 9);
        assert_eq!(bands.stops()[index], stop(1234.0));
        let mut want = before.clone();
        want.insert(9, before[8]);
        assert_eq!(bands.colors(), want);
        let steps = bands.bands();
        assert_eq!((steps[8].min, steps[8].max), (1000.0, 1234.0));
        assert_eq!((steps[9].min, steps[9].max), (1234.0, 2000.0));
        assert_eq!((steps[8].color, steps[9].color), (before[8], before[8]));

        // A stop next to sea level splits the band on its side.
        let index = bands.add(-100.0).unwrap();
        assert_eq!(index, 5);
        assert_eq!(bands.colors()[5], before[4]);
        assert_eq!(bands.stops()[6].value, SEA_LEVEL);
        assert_eq!(copy(&bands), Some(bands));
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
    fn the_ends_and_sea_level_stay() {
        let mut bands = Bands::default();
        let last = bands.stops().len() - 1;
        let sea = bands.sea().unwrap();
        assert!(bands.fixed(0) && bands.fixed(last) && bands.fixed(sea));
        assert!(!bands.fixed(1) && !bands.fixed(last - 1));
        assert_eq!(bands.move_to(0, 100.0), ELEV_MIN);
        assert_eq!(bands.move_to(last, 100.0), ELEV_MAX);
        assert_eq!(bands.move_to(sea, 100.0), SEA_LEVEL);
        assert_eq!(bands.move_to(sea, -100.0), SEA_LEVEL);
        assert!(!bands.remove(0));
        assert!(!bands.remove(last));
        assert!(!bands.remove(sea));
        assert_eq!(bands, Bands::default());
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
    fn remove_keeps_the_lower_color() {
        let mut bands = Bands::default();
        let before = bands.colors().to_vec();
        assert!(bands.remove(8));
        let mut want = before.clone();
        want.remove(8);
        assert_eq!(bands.colors(), want);
        let joined = bands.bands()[7];
        assert_eq!((joined.min, joined.max), (500.0, 2000.0));
        assert_eq!(joined.color, before[7]);
    }

    #[test]
    fn sea_level_is_the_only_inner_stop_that_stays() {
        let mut bands = Bands::default();
        while bands.stops().len() > 3 {
            let index = (0..bands.stops().len()).find(|&i| !bands.fixed(i)).unwrap();
            assert!(bands.remove(index));
        }
        assert_eq!(values(&bands), [ELEV_MIN, SEA_LEVEL, ELEV_MAX]);
        let steps = bands.bands();
        assert_eq!((steps[0].min, steps[0].max), (ELEV_MIN, SEA_LEVEL));
        assert_eq!((steps[1].min, steps[1].max), (SEA_LEVEL, ELEV_MAX));
        // Each removal keeps the lower color: the deepest sea band and the
        // first land band.
        let old = Bands::default().colors().to_vec();
        assert_eq!(bands.colors(), [old[0], old[5]]);
        for step in steps {
            assert_eq!((step.bottom, step.top), (step.color, step.color));
        }
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
        bands.set_color(3, [9, 8, 7]);
        bands.set_line(
            5,
            StopLine {
                on: true,
                ..StopLine::default()
            },
        );
        assert_eq!(copy(&bands), Some(bands));
    }

    #[test]
    fn from_range_parts_rejects_bad_stops() {
        let parts = |values: &[f64]| grey(0.0, 100.0, false, values);
        assert!(parts(&[0.0, 50.0, 100.0]).is_some());
        // One stop.
        assert_eq!(parts(&[0.0]), None);
        // The ends are not at the ends of the range.
        assert_eq!(parts(&[1.0, 50.0, 100.0]), None);
        assert_eq!(parts(&[0.0, 50.0, 99.0]), None);
        assert_eq!(parts(&[0.0, 50.0, 100.0, 120.0]), None);
        // The values do not ascend, or are too near.
        assert_eq!(parts(&[0.0, 60.0, 40.0, 100.0]), None);
        assert_eq!(parts(&[0.0, 50.0, 50.0, 100.0]), None);
        assert_eq!(parts(&[0.0, 50.0, 50.5, 100.0]), None);
        assert_eq!(parts(&[0.0, 0.5, 100.0]), None);
        assert_eq!(parts(&[0.0, f64::NAN, 100.0]), None);
        // Too many stops.
        let many: Vec<f64> = (0..=MAX_BANDS).map(|i| i as f64 * 2.0).collect();
        let range = |values: &[f64]| grey(0.0, values[values.len() - 1], false, values);
        assert!(range(&many).is_some());
        let many: Vec<f64> = (0..=MAX_BANDS + 1).map(|i| i as f64 * 2.0).collect();
        assert_eq!(range(&many), None);
        // A wrong number of colors.
        let stops = plain(&[0.0, 50.0, 100.0]);
        for n in [0, 1, 3] {
            let colors = vec![[1, 2, 3]; n];
            assert_eq!(
                Bands::from_range_parts(0.0, 100.0, false, &stops, &colors),
                None,
                "{n}"
            );
        }
        // A bad range.
        assert_eq!(grey(100.0, 0.0, false, &[100.0, 0.0]), None);
        assert_eq!(grey(f64::NAN, 1.0, false, &[f64::NAN, 1.0]), None);
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
        assert_eq!(copy(&bands), Some(bands));
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
    fn a_range_with_no_sea_has_one_gradient() {
        let bands = Bands::with_range(0.0, 4000.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let expect = [
            0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 3500.0, 4000.0,
        ];
        assert_eq!(values(&bands), expect);
        for (j, c) in bands.colors().iter().enumerate() {
            assert_eq!(*c, blend(&LAND_PALETTE, j as f64 / 8.0), "{j}");
        }
        let steps = bands.bands();
        assert_eq!(steps.len(), 8);
        assert_eq!((steps[0].min, steps[7].max), (0.0, 4000.0));
        for pair in steps.windows(2) {
            assert_eq!(pair[0].top, pair[1].bottom);
        }

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
        assert_eq!(bands.colors(), [LAND_PALETTE[0]]);

        let below = Bands::with_range(-4000.0, 0.0, true).unwrap();
        assert_eq!(below.sea(), None);
        assert_eq!(below.colors()[0], LAND_PALETTE[0]);
    }

    #[test]
    fn a_range_across_zero_that_is_not_a_height_has_one_gradient() {
        let bands = Bands::with_range(-60.0, 60.0, false).unwrap();
        assert_eq!(bands.sea(), None);
        let expect = [-60.0, -45.0, -30.0, -15.0, 0.0, 15.0, 30.0, 45.0, 60.0];
        assert_eq!(values(&bands), expect);
        // The stop at zero moves and goes away as each other stop.
        let mut bands = bands;
        assert!(!bands.fixed(4));
        assert_eq!(bands.move_to(4, 5.0), 5.0);
        assert!(bands.remove(4));
        assert_eq!(bands.stops().len(), 8);
        assert_eq!(copy(&bands), Some(bands.clone()));
        // The same parts are not the bands of a height.
        assert_eq!(
            Bands::from_range_parts(-60.0, 60.0, true, bands.stops(), bands.colors()),
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
        let bands = grey(0.0, 4000.0, true, &[0.0, 250.0, 1000.0, 4000.0]).unwrap();
        assert_eq!(values(&bands), [0.0, 250.0, 1000.0, 4000.0]);
        assert_eq!(bands.sea(), None);
        assert_eq!((bands.min(), bands.max()), (0.0, 4000.0));
        assert_eq!(bands.level(4000.0), 65535);

        // An empty list gives the stops of the range.
        let none = Bands::from_range_parts(0.0, 4000.0, true, &[], &[]);
        assert_eq!(none, Bands::with_range(0.0, 4000.0, true));
        assert_eq!(Bands::from_range_parts(4000.0, 0.0, true, &[], &[]), None);

        let split = grey(-10.0, 4000.0, true, &[-10.0, 0.0, 250.0, 4000.0]).unwrap();
        assert_eq!(split.sea(), Some(1));
        assert_eq!(grey(-10.0, 4000.0, true, &[-10.0, 250.0, 4000.0]), None);
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
        assert!(bands.set_line(1, heavy));
        assert_eq!(bands.stops()[1].line.weight, *StopLine::WEIGHTS.end());
        assert!(!bands.set_line(99, coast));
    }

    #[test]
    fn a_darker_line_takes_the_color_of_the_band_above() {
        let mut bands = Bands::with_range(-1000.0, 4000.0, true).unwrap();
        let sea = bands.sea().unwrap();
        let line = StopLine {
            on: true,
            darker: true,
            darken: 0.5,
            ..StopLine::default()
        };
        let half = |c: Rgb| c.map(|c| (f32::from(c) * 0.5).round() as u8);
        let land = bands.colors()[sea + 1];
        bands.set_line(sea + 1, line);
        assert_eq!(bands.line_color(sea + 1), Some(half(land)));

        // The darker color follows the band, and the amount stays in range.
        bands.set_color(sea + 1, [200, 100, 50]);
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

    #[test]
    fn a_darker_coast_takes_the_first_land_band() {
        let mut bands = Bands::default();
        let sea = bands.sea().unwrap();
        let line = StopLine {
            on: true,
            darker: true,
            darken: 0.5,
            ..StopLine::default()
        };
        assert!(bands.set_line(sea, line));
        assert_eq!(bands.line_color(sea), Some([54, 82, 50]));
        bands.set_color(sea - 1, [0, 0, 0]);
        bands.set_color(sea, [100, 60, 20]);
        assert_eq!(bands.line_color(sea), Some([50, 30, 10]));
    }

    #[test]
    fn a_ramp_gives_one_color_per_band() {
        let bands = Bands::default();
        for n in [1, 3, 7, 12] {
            let ramp = greys(n);
            let part = Part::Land;
            let range = bands.part_bands(part);
            for result in [
                bands.painted_part(&ramp, &[], part).unwrap(),
                bands.stretched_part(&ramp, &[], part).unwrap(),
            ] {
                assert_eq!(result.colors().len(), result.stops().len() - 1);
                assert_eq!(band_colors(&result), result.colors());
                // The sea does not change.
                let sea = result.sea().unwrap();
                assert_eq!(result.colors()[..sea], bands.colors()[..range.start]);
            }
        }
        let painted = bands.painted_part(&greys(3), &[], Part::Land).unwrap();
        let sea = painted.sea().unwrap();
        assert_eq!(painted.colors()[sea..], greys(3));
    }

    #[test]
    fn a_land_ramp_changes_only_the_land() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        assert_eq!(bands.part(&greys(3)), Part::Land);
        let painted = bands.painted(&greys(3)).unwrap();
        assert_eq!(painted.stops()[..=sea], bands.stops()[..=sea]);
        let third = ELEV_MAX / 3.0;
        assert_eq!(values(&painted)[sea..], [0.0, third, 2.0 * third, ELEV_MAX]);
        let old = band_colors(&bands);
        assert_eq!(band_colors(&painted), [&old[..sea], &greys(3)].concat());
    }

    #[test]
    fn a_land_ramp_and_a_sea_ramp_color_one_layer() {
        let bands = Bands::default();
        let deep: Vec<Rgb> = (0..4).map(|i| [10, 20 * i, 200]).collect();
        let painted = bands.painted(&greys(3)).unwrap();
        assert_eq!(painted.part(&deep), Part::Sea);
        let both = painted.painted(&deep).unwrap();
        let half = ELEV_MIN / 4.0;
        assert_eq!(
            values(&both)[..5],
            [ELEV_MIN, 3.0 * half, 2.0 * half, half, 0.0]
        );
        // The land does not change.
        let sea = bands.sea().unwrap();
        assert_eq!(both.stops()[4..], painted.stops()[sea..]);
        assert_eq!(band_colors(&both), [&deep[..], &greys(3)].concat());
    }

    #[test]
    fn a_stretched_land_ramp_keeps_the_sea() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        let stretched = bands.stretched(&greys(2)).unwrap();
        assert_eq!(values(&stretched), values(&bands));
        assert_eq!(stretched.colors()[..sea], bands.colors()[..sea]);
        assert_eq!(band_colors(&stretched)[sea..], spread(&greys(2), 7));
    }

    /// The names `names`, as owned strings.
    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_split_keeps_the_name_on_both_parts() {
        let mut bands = Bands::with_range(0.0, 100.0, false).unwrap();
        let n = bands.colors().len();
        assert!(bands.names().is_empty());
        assert!(bands.set_name(0, "Low"));
        assert!(!bands.set_name(n, "Out"));
        let index = bands.add(bands.stops()[1].value / 2.0).unwrap();
        assert_eq!(index, 1);
        assert_eq!(
            (bands.name(0), bands.name(1), bands.name(2)),
            ("Low", "Low", "")
        );
        assert_eq!(bands.names().len(), bands.colors().len());
        // A band with no names stays with no names after a split.
        let mut plain = Bands::with_range(0.0, 100.0, false).unwrap();
        plain.add(1.0).unwrap();
        assert!(plain.names().is_empty());
    }

    #[test]
    fn a_join_keeps_the_name_of_the_lower_band() {
        let mut bands = Bands::with_range(0.0, 100.0, false).unwrap();
        bands.set_name(0, "Low");
        bands.set_name(1, "High");
        assert!(bands.remove(1));
        assert_eq!(bands.name(0), "Low");
        assert_eq!(bands.names().len(), bands.colors().len());
        // Names that are all empty are no names.
        bands.set_name(0, "");
        assert!(bands.names().is_empty());
        let wrong = bands.clone().with_names(&owned(&["One"]));
        assert!(wrong.names().is_empty());
    }

    #[test]
    fn a_labeled_ramp_names_the_bands_it_colors() {
        let plain = Bands::with_range(0.0, 100.0, false).unwrap();
        let labels = owned(&["Af", "", "Cfb"]);
        let painted = plain.painted_part(&greys(3), &labels, Part::All).unwrap();
        assert_eq!(painted.names(), labels);
        // With the same number of bands, each band takes the label of its
        // color.
        let n = plain.colors().len();
        let many: Vec<String> = (0..n).map(|i| format!("C{i}")).collect();
        let stretched = plain.stretched_part(&greys(n), &many, Part::All).unwrap();
        assert_eq!(stretched.names(), many);
        // With fewer colors than bands, a label repeats.
        let two = plain
            .stretched_part(&greys(2), &owned(&["A", "B"]), Part::All)
            .unwrap();
        assert_eq!(two.name(0), "A");
        assert_eq!(two.name(n - 1), "B");
    }

    #[test]
    fn an_unlabeled_ramp_keeps_the_names_when_the_stops_stay() {
        let mut plain = Bands::with_range(0.0, 100.0, false).unwrap();
        plain.set_name(1, "Mid");
        let stretched = plain.stretched_part(&greys(2), &[], Part::All).unwrap();
        assert_eq!(stretched.names(), plain.names());
        let blank = owned(&["", ""]);
        let stretched = plain.stretched_part(&greys(2), &blank, Part::All).unwrap();
        assert_eq!(stretched.names(), plain.names());
        // New stops give the bands no names.
        let painted = plain.painted_part(&greys(3), &[], Part::All).unwrap();
        assert!(painted.names().is_empty());
    }

    #[test]
    fn a_land_ramp_keeps_the_sea_names() {
        let mut bands = Bands::default();
        let sea = bands.sea().unwrap();
        bands.set_name(0, "Deep");
        bands.set_name(sea, "Shore");
        let labels = owned(&["Plain", "Hill", "Peak"]);
        let painted = bands.painted_part(&greys(3), &labels, Part::Land).unwrap();
        assert_eq!(painted.name(0), "Deep");
        assert_eq!(painted.names()[sea..], labels);
        let stretched = bands
            .stretched_part(&greys(3), &labels, Part::Land)
            .unwrap();
        assert_eq!(stretched.name(0), "Deep");
        assert_ne!(stretched.name(sea), "Shore");
    }

    #[test]
    fn a_chosen_part_overrides_the_colors() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        // Grey is a land color, but the user chose the sea.
        let painted = bands.painted_part(&greys(2), &[], Part::Sea).unwrap();
        assert_eq!(values(&painted)[..3], [ELEV_MIN, ELEV_MIN / 2.0, 0.0]);
        assert_eq!(painted.stops()[3..], bands.stops()[sea + 1..]);
        assert_eq!(band_colors(&painted)[..2], greys(2));
        let stretched = bands.stretched_part(&greys(2), &[], Part::Sea).unwrap();
        assert_eq!(stretched.colors()[sea..], bands.colors()[sea..]);
        // With no sea split, a part is all bands.
        let plain = Bands::with_range(0.0, 100.0, false).unwrap();
        let all = plain.painted_part(&greys(2), &[], Part::Land).unwrap();
        assert_eq!(values(&all), [0.0, 50.0, 100.0]);
    }

    #[test]
    fn painted_with_the_same_count_keeps_the_stops() {
        let mut bands = Bands::default();
        let line = StopLine {
            on: true,
            ..StopLine::default()
        };
        bands.set_line(3, line);
        // One blue color for each sea band makes a ramp for all bands.
        let mut colors = greys(bands.stops().len() - 1);
        colors[..bands.sea().unwrap()].fill(BLUE);
        let painted = bands.painted(&colors).unwrap();
        assert_eq!(values(&painted), values(&bands));
        assert_eq!(band_colors(&painted), colors);
        assert_eq!(painted.stops()[3].line, line);
    }

    #[test]
    fn painted_splits_the_bands_at_sea_level() {
        let bands = Bands::with_range(-1000.0, 3000.0, true).unwrap();
        let mut colors = greys(4);
        colors[3] = BLUE;
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
    fn stretched_keeps_the_stops_and_takes_the_color_at_each_middle() {
        let mut bands = Bands::with_range(0.0, 100.0, false).unwrap();
        bands.set_line(
            2,
            StopLine {
                on: true,
                ..StopLine::default()
            },
        );
        let before = bands.stops().to_vec();
        let more = bands.stretched(&greys(4)).unwrap();
        assert_eq!(more.stops(), before);
        assert_eq!(band_colors(&more), spread(&greys(4), 8));
        let fewer = bands.stretched(&greys(16)).unwrap();
        let g = greys(16);
        assert_eq!(
            band_colors(&fewer),
            [g[1], g[3], g[5], g[7], g[9], g[11], g[13], g[15]]
        );
        assert!(bands.stretched(&[]).is_none());
    }

    #[test]
    fn painted_refuses_too_few_or_too_many_colors() {
        let bands = Bands::default();
        assert!(bands.painted(&[]).is_none());
        assert!(bands.painted(&greys(MAX_BANDS + 1)).is_none());
        // The sea keeps its bands, so the land has no room for so many.
        assert!(bands.painted(&greys(MAX_BANDS)).is_none());
        let plain = Bands::with_range(0.0, 100.0, false).unwrap();
        assert!(plain.painted(&greys(MAX_BANDS)).is_some());
        let small = Bands::with_range(0.0, 4.0, false).unwrap();
        assert!(small.painted(&greys(8)).is_none());
    }

    #[test]
    fn a_ramp_with_fewer_colors_than_bands_blends_its_colors() {
        let plain = Bands::with_range(0.0, 100.0, false).unwrap();
        let m = plain.colors().len();
        assert!(m > 3);
        let ramp = [[0, 0, 0], [100, 100, 100], [200, 200, 200]];
        let fit = plain.stretched_part(&ramp, &[], Part::All).unwrap();
        let colors = fit.colors();
        assert_eq!(colors[0], ramp[0]);
        assert_eq!(colors[m - 1], ramp[2]);
        // No two bands next to each other have the same color.
        assert!(colors.windows(2).all(|w| w[0][0] < w[1][0]), "{colors:?}");
        // A qualitative ramp does not blend, and its colors repeat in order.
        let fit = plain
            .stretched_in(&ramp, &[], Part::All, 0..m, true)
            .unwrap();
        let cycle: Vec<Rgb> = (0..m).map(|k| ramp[k % 3]).collect();
        assert_eq!(fit.colors(), cycle);
        // One color colors all bands.
        let one = plain.stretched_part(&ramp[..1], &[], Part::All).unwrap();
        assert!(one.colors().iter().all(|&c| c == ramp[0]));
    }

    #[test]
    fn stretched_in_colors_only_the_window() {
        let bands = Bands::default();
        let fit = bands
            .stretched_in(&greys(2), &[], Part::Land, 7..10, false)
            .unwrap();
        assert_eq!(fit.stops(), bands.stops());
        assert_eq!(fit.colors()[7..10], spread(&greys(2), 3));
        assert_eq!(fit.colors()[..7], bands.colors()[..7]);
        assert_eq!(fit.colors()[10..], bands.colors()[10..]);
        // The window is in the part, so a sea window colors nothing.
        assert!(
            bands
                .stretched_in(&greys(2), &[], Part::Land, 0..5, false)
                .is_none()
        );
    }

    #[test]
    fn painted_in_keeps_the_stops_out_of_the_window() {
        let bands = Bands::default();
        let made = bands
            .painted_in(&greys(4), &[], Part::Land, 7..10, 600.0, 1800.0)
            .unwrap();
        // The end bands reach out to the stops at 500 m and 3000 m.
        assert_eq!(made.stops()[..=7], bands.stops()[..=7]);
        assert_eq!(values(&made)[7..12], [500.0, 900.0, 1200.0, 1500.0, 3000.0]);
        assert_eq!(made.stops()[11..], bands.stops()[10..]);
        let old = bands.colors();
        assert_eq!(made.colors(), [&old[..7], &greys(4), &old[10..]].concat());
    }

    #[test]
    fn painted_in_across_sea_level_splits_the_colors() {
        let bands = Bands::default();
        let made = bands
            .painted_in(&greys(4), &[], Part::All, 3..8, -600.0, 600.0)
            .unwrap();
        assert_eq!(values(&made)[3..8], [-1000.0, -300.0, 0.0, 300.0, 1000.0]);
        assert_eq!(made.sea(), Some(5));
        assert_eq!(made.stops()[7..], bands.stops()[8..]);
        assert_eq!(made.colors()[3..7], greys(4));
        // One color can not cover both sides.
        let one = bands.painted_in(&greys(1), &[], Part::All, 3..8, -600.0, 600.0);
        assert!(one.is_none());
    }

    #[test]
    fn painted_in_refuses_too_many_bands_and_an_empty_range() {
        let bands = Bands::default();
        let n = bands.colors().len();
        let many = greys(MAX_BANDS - n + 2);
        assert!(
            bands
                .painted_in(&many, &[], Part::Land, 7..8, 500.0, 1000.0)
                .is_none()
        );
        let fits = greys(MAX_BANDS - n + 1);
        assert!(
            bands
                .painted_in(&fits, &[], Part::Land, 7..8, 500.0, 1000.0)
                .is_some()
        );
        let empty = bands.painted_in(&greys(2), &[], Part::Land, 7..10, 900.0, 900.0);
        assert!(empty.is_none());
    }

    #[test]
    fn painted_in_keeps_a_land_ramp_above_sea_level() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        let land = bands
            .painted_in(&greys(2), &[], Part::Land, 0..12, -5000.0, 2000.0)
            .unwrap();
        assert_eq!(land.stops()[..=sea], bands.stops()[..=sea]);
        assert_eq!(values(&land)[sea..], [0.0, 1000.0, ELEV_MAX]);
        let water = bands
            .painted_in(&greys(2), &[], Part::Sea, 0..12, -3000.0, 2000.0)
            .unwrap();
        assert_eq!(values(&water)[..3], [ELEV_MIN, -1500.0, 0.0]);
    }

    #[test]
    fn fitted_to_moves_the_inner_stops_into_the_window() {
        let mut bands = grey(0.0, 1000.0, false, &[0.0, 100.0, 500.0, 1000.0]).unwrap();
        let line = StopLine {
            on: true,
            ..StopLine::default()
        };
        bands.set_line(1, line);
        bands.set_color(2, BLUE);
        bands.set_name(1, "Hills");
        let fitted = bands.fitted_to(200.0, 400.0).unwrap();
        assert_eq!(values(&fitted), [0.0, 220.0, 300.0, 1000.0]);
        assert_eq!(fitted.colors(), bands.colors());
        assert_eq!(fitted.names(), bands.names());
        assert_eq!(fitted.stops()[1].line, line);
    }

    #[test]
    fn fitted_to_keeps_sea_level_for_a_window_across_it() {
        let bands = grey(
            -1000.0,
            3000.0,
            true,
            &[-1000.0, -500.0, 0.0, 1500.0, 3000.0],
        )
        .unwrap();
        let fitted = bands.fitted_to(-200.0, 600.0).unwrap();
        assert_eq!(values(&fitted), [-1000.0, -100.0, 0.0, 300.0, 3000.0]);
    }

    #[test]
    fn fitted_to_moves_only_the_land_for_a_window_on_land() {
        let bands = grey(
            -1000.0,
            3000.0,
            true,
            &[-1000.0, -500.0, 0.0, 1500.0, 3000.0],
        )
        .unwrap();
        let fitted = bands.fitted_to(1000.0, 2000.0).unwrap();
        assert_eq!(values(&fitted), [-1000.0, -500.0, 0.0, 1500.0, 3000.0]);
        let fitted = bands.fitted_to(200.0, 1400.0).unwrap();
        assert_eq!(values(&fitted), [-1000.0, -500.0, 0.0, 800.0, 3000.0]);
    }

    #[test]
    fn fitted_to_moves_only_the_sea_for_a_window_in_the_sea() {
        let bands = grey(
            -1000.0,
            3000.0,
            true,
            &[-1000.0, -500.0, 0.0, 1500.0, 3000.0],
        )
        .unwrap();
        let fitted = bands.fitted_to(-800.0, -400.0).unwrap();
        assert_eq!(values(&fitted), [-1000.0, -600.0, 0.0, 1500.0, 3000.0]);
    }

    #[test]
    fn fitted_to_rejects_a_bad_window() {
        let bands = grey(0.0, 1000.0, false, &[0.0, 100.0, 500.0, 1000.0]).unwrap();
        assert!(bands.fitted_to(400.0, 400.0).is_none());
        assert!(bands.fitted_to(500.0, 400.0).is_none());
        assert!(bands.fitted_to(f64::NAN, 400.0).is_none());
        assert!(bands.fitted_to(0.0, f64::INFINITY).is_none());
        // The two inner stops would be 0.4 apart.
        assert!(bands.fitted_to(400.0, 401.0).is_none());
    }

    /// A ramp that is not qualitative and has no sea level of its own.
    fn ramp_of<'a>(colors: &'a [Rgb], labels: &'a [String]) -> RampColors<'a> {
        RampColors {
            colors,
            labels,
            qualitative: false,
            sea: None,
        }
    }

    #[test]
    fn a_ramp_with_its_own_sea_level_splits_there() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        let all = 0..bands.colors().len();
        // Teal sea colors are not mostly blue.
        let teal = [[23, 70, 66], [42, 107, 106], [90, 163, 154]];
        let colors = [&teal[..], &greys(4)].concat();
        let ramp = RampColors {
            sea: Some(3),
            ..ramp_of(&colors, &[])
        };
        assert_eq!(bands.part(&colors), Part::Land);
        assert_eq!(bands.ramp_part(&ramp), Part::All);
        let fitted = bands.stretched_ramp(&ramp, Part::All, all.clone()).unwrap();
        assert_eq!(fitted.colors()[0], teal[0]);
        assert_eq!(fitted.colors()[sea - 1], teal[2]);
        assert!(fitted.colors()[sea..].iter().all(|c| c[0] == c[1] && c[1] == c[2]));
        let (lo, hi) = (bands.min(), bands.max());
        let made = bands
            .painted_count(&ramp, Part::All, all, (lo, hi), 7)
            .unwrap();
        assert_eq!(made.sea(), Some(3));
        assert_eq!(made.colors()[..3], teal);
    }

    /// A ramp with `sea` colors that are mostly blue and then `land` colors
    /// that are not.
    fn sea_and_land(sea: usize, land: usize) -> Vec<Rgb> {
        let blues = (0..sea).map(|i| [10, 10 * i as u8, 200]);
        let greens = (0..land).map(|i| [40 + 5 * i as u8, 150, 10]);
        blues.chain(greens).collect()
    }

    #[test]
    fn stretched_puts_the_blue_colors_on_the_sea_bands() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        let m = bands.colors().len();
        // Far fewer blue colors than sea bands, and far more land colors
        // than land bands.
        let colors = sea_and_land(2, 20);
        let fitted = bands.stretched(&colors).unwrap();
        assert_eq!(values(&fitted), values(&bands));
        assert!(fitted.colors()[..sea].iter().all(|c| is_blue(*c)));
        assert!(fitted.colors()[sea..].iter().all(|c| !is_blue(*c)));
        assert_eq!(fitted.colors()[0], colors[0]);
        assert_eq!(fitted.colors()[sea - 1], colors[1]);
        assert_eq!(fitted.colors()[m - 1], colors[20]);
    }

    #[test]
    fn a_qualitative_stretch_ignores_sea_level() {
        let bands = Bands::default();
        let colors = sea_and_land(2, 2);
        let all = 0..bands.colors().len();
        let fitted = bands
            .stretched_in(&colors, &[], Part::All, all, true)
            .unwrap();
        let repeated: Vec<Rgb> = colors
            .iter()
            .copied()
            .cycle()
            .take(fitted.colors().len())
            .collect();
        assert_eq!(fitted.colors(), repeated);
    }

    #[test]
    fn painted_count_makes_that_many_bands_split_at_sea_level() {
        let bands = Bands::default();
        let colors = sea_and_land(4, 4);
        let all = 0..bands.colors().len();
        let (lo, hi) = (bands.min(), bands.max());
        let painted = bands
            .painted_count(&ramp_of(&colors, &[]), Part::All, all, (lo, hi), 10)
            .unwrap();
        assert_eq!(painted.colors().len(), 10);
        let sea = painted.sea().unwrap();
        assert_eq!(sea, 5);
        assert_eq!(painted.stops()[sea].value, 0.0);
        assert!(painted.colors()[..sea].iter().all(|c| is_blue(*c)));
        assert!(painted.colors()[sea..].iter().all(|c| !is_blue(*c)));
        // Each side runs from its first color to its last color.
        assert_eq!(painted.colors()[0], colors[0]);
        assert_eq!(painted.colors()[sea - 1], colors[3]);
        assert_eq!(painted.colors()[sea], colors[4]);
        assert_eq!(painted.colors()[9], colors[7]);
        let land = ELEV_MAX / 5.0;
        assert_eq!(
            values(&painted)[sea..],
            [0.0, land, 2.0 * land, 3.0 * land, 4.0 * land, ELEV_MAX]
        );
    }

    #[test]
    fn painted_count_on_the_land_keeps_the_sea() {
        let bands = Bands::default();
        let sea = bands.sea().unwrap();
        let all = 0..bands.colors().len();
        let (lo, hi) = (bands.min(), bands.max());
        let painted = bands
            .painted_count(&ramp_of(&greys(8), &[]), Part::Land, all, (lo, hi), 3)
            .unwrap();
        assert_eq!(painted.colors().len(), sea + 3);
        assert_eq!(painted.stops()[..=sea], bands.stops()[..=sea]);
        assert_eq!(painted.colors()[..sea], bands.colors()[..sea]);
        let g = greys(8);
        assert_eq!(painted.colors()[sea..], [g[0], blend(&g, 0.5), g[7]]);
    }

    #[test]
    fn painted_count_gives_each_band_the_label_of_its_nearest_color() {
        let bands = Bands::with_range(0.0, 100.0, false).unwrap();
        let labels = owned(&["Low", "Mid", "High"]);
        let all = 0..bands.colors().len();
        let painted = bands
            .painted_count(
                &ramp_of(&greys(3), &labels),
                Part::All,
                all,
                (0.0, 100.0),
                5,
            )
            .unwrap();
        assert_eq!(painted.colors().len(), 5);
        let names: Vec<&str> = (0..5).map(|i| painted.name(i)).collect();
        assert_eq!(names, ["Low", "Mid", "Mid", "High", "High"]);
    }

    #[test]
    fn sample_ramp_keeps_the_ends() {
        let g = greys(4);
        let six = sample_ramp(&g, 6, false);
        assert_eq!((six[0], six[5]), (g[0], g[3]));
        assert_eq!(sample_ramp(&g, 1, false), [blend(&g, 0.5)]);
        assert_eq!(
            sample_ramp(&g, 6, true),
            [g[0], g[1], g[2], g[3], g[0], g[1]]
        );
    }
}
