//! The color key of a page: the color bands of one raster layer, as a strip
//! of boxes with the values at their edges or as a list of boxes with text,
//! with vector paths and glyphs.

use kurbo::{Affine, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{Color, FillRule, Item};
use vale_store::{ColorKeyItem, FrameRecord, KeyLayout, RasterDisplay, SecondUnit};
use vale_terrain::{Band, Bands, SEA_LEVEL};

use crate::atlas::PT_PER_MM;
use crate::document::Document;
use crate::furniture::{PAD, Painted, TextInk};
use crate::raster_style::apply_symbolizer;
use crate::session::bands_of;

/// The height of a line of text, as a multiple of the text size.
const LINE_HEIGHT: f64 = 1.2;
/// The number of steps of color in a box of a smooth display.
const SLICES: usize = 24;

/// One band of a color key.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    /// The key of the band in [`ColorKeyItem::bands`].
    pub key: String,
    /// The index of the band, from the lowest band.
    pub index: usize,
    pub band: Band,
    /// The name of the band in the style of its layer.
    pub name: String,
    /// The text of the band in a list: its name in the key, or else its
    /// name, or else its range.
    pub label: String,
    pub hidden: bool,
}

/// The bands of the layer of a color key in the map of its frame.
#[derive(Clone, Debug)]
pub struct KeyData {
    /// All bands, from the lowest band.
    pub rows: Vec<KeyRow>,
    pub bands: Bands,
    pub display: RasterDisplay,
    /// The unit word of the layer.
    pub unit: String,
    /// The index of the lowest band above sea level, for a strip that
    /// splits there.
    pub sea: Option<usize>,
}

/// The key of a band in [`ColorKeyItem::bands`]: its name, or the value of
/// its lower edge for a band with no name.
pub fn band_key(name: &str, min: f64) -> String {
    if name.is_empty() {
        number_text(min)
    } else {
        name.to_string()
    }
}

/// A value with at most two decimals, with no trailing zeros, and with a
/// comma between groups of thousands.
pub fn number_text(v: f64) -> String {
    let text = format!("{:.2}", (v * 100.0).round() / 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if grouped == "0" && frac.is_empty() {
        ""
    } else {
        sign
    };
    match frac {
        "" => format!("{sign}{grouped}"),
        _ => format!("{sign}{grouped}.{frac}"),
    }
}

/// The number of decimals of [`number_text`].
fn decimals(v: f64) -> usize {
    number_text(v).split_once('.').map_or(0, |(_, f)| f.len())
}

/// `v` rounded to `decimals`, as [`number_text`].
fn number_at(v: f64, decimals: usize) -> String {
    let p = 10f64.powi(decimals as i32);
    number_text((v * p).round() / p)
}

/// The range of band `index` of `count` bands in a list. The lowest band
/// and the highest band are open. A range with a negative end uses "to".
pub fn range_text(index: usize, count: usize, min: f64, max: f64) -> String {
    let (lo, hi) = (number_text(min), number_text(max));
    if count > 1 && index == 0 {
        format!("Under {hi}")
    } else if count > 1 && index + 1 == count {
        format!("Over {lo}")
    } else if min < 0.0 || max < 0.0 {
        format!("{lo} to {hi}")
    } else {
        format!("{lo}–{hi}")
    }
}

/// The word of a unit in a key.
pub fn unit_word(unit: &str) -> String {
    match unit {
        "m" => "meters".to_string(),
        "mm" => "millimeters".to_string(),
        _ => unit.to_string(),
    }
}

/// The common second unit of a layer unit: feet for meters, inches for
/// millimeters, and degrees Fahrenheit for degrees Celsius.
pub fn preset_unit(unit: &str) -> Option<SecondUnit> {
    let (name, factor, offset) = match unit {
        "m" => ("feet", 1.0 / 0.3048, 0.0),
        "mm" => ("inches", 1.0 / 25.4, 0.0),
        "°C" => ("°F", 1.8, 32.0),
        _ => return None,
    };
    Some(SecondUnit {
        name: name.to_string(),
        factor,
        offset,
    })
}

/// The bands of the layer of a key in the map of its frame. `None` if the
/// key has no layer, or if the map does not show it.
pub fn data(doc: &Document, frame: &FrameRecord, key: &ColorKeyItem) -> Option<KeyData> {
    let layer = key.layer?;
    if !doc.shown(frame.map, layer) {
        return None;
    }
    let raster = doc.project.layer(layer)?.raster()?;
    let mut record = doc.project.raster(raster)?.clone();
    if let Some(style) = doc.map_style(frame.map, layer)
        && let Some(s) = style.raster_symbolizer()
    {
        apply_symbolizer(&mut record, s, true);
    }
    let bands = bands_of(&record);
    let list = bands.bands();
    let count = list.len();
    let rows = (list.into_iter().enumerate())
        .map(|(index, band)| {
            let name = bands.name(index).to_string();
            let key_text = band_key(&name, band.min);
            let change = key.bands.get(&key_text);
            let own = change.and_then(|c| c.name.clone());
            let label = match own {
                Some(text) => text,
                None if !name.is_empty() && !key.names_in_boxes => name.clone(),
                None => range_text(index, count, band.min, band.max),
            };
            KeyRow {
                key: key_text,
                index,
                band,
                name,
                label,
                hidden: change.is_some_and(|c| c.hidden),
            }
        })
        .collect();
    let sea = (key.split_at_sea_level && crate::ui::is_height(&record))
        .then(|| bands.sea())
        .flatten();
    Some(KeyData {
        rows,
        display: record.display,
        unit: unit_word(&record.unit),
        sea,
        bands,
    })
}

/// The items of a key in `rect`, in target points. A part that does not fit
/// above the bottom of `rect` is left out.
pub fn draw(
    key: &ColorKeyItem,
    data: &KeyData,
    rect: Rect,
    unit: f64,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let pad = PAD * unit;
    let inner = Rect::new(rect.x0 + pad, rect.y0 + pad, rect.x1 - pad, rect.y1 - pad);
    let mut out = Vec::new();
    let mut text = Painted::default();
    let mut y = inner.y0;
    if !key.title.is_empty() {
        let ink = TextInk::new(&key.title_style, unit, fonts);
        let step = LINE_HEIGHT * ink.style.size;
        if y + step > inner.y1 + 1e-9 {
            return out;
        }
        let to = Affine::translate((inner.x0, baseline(&ink, y, fonts)));
        ink.line(&key.title, to, fonts, &mut text);
        y += step + 0.25 * ink.style.size;
    }
    let ink = TextInk::new(&key.value_style, unit, fonts);
    let mut draw = Draw {
        ink,
        unit,
        fonts,
        out: &mut out,
        text: &mut text,
    };
    let inner = Rect::new(inner.x0, y, inner.x1, inner.y1);
    match key.layout {
        KeyLayout::Strip => draw.strip(key, data, inner),
        KeyLayout::List => draw.list(key, data, inner),
    }
    out.extend(text.into_items());
    out
}

/// The baseline of a line of `ink` whose top is at `y`.
fn baseline(ink: &TextInk, y: f64, fonts: &mut Fonts) -> f64 {
    let metrics = fonts.measure("Hg", &ink.style);
    let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
    y + (LINE_HEIGHT * ink.style.size - ascent - descent) / 2.0 + ascent
}

fn fill(rect: Rect, color: Color) -> Item {
    Item::Fill {
        path: rect.to_path(0.1),
        color,
        rule: FillRule::NonZero,
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::rgb(r, g, b)
}

/// The color of the lines around the boxes.
fn line_color() -> Color {
    Color::rgba(70, 70, 70, 200)
}

/// One box of a strip: a band, or the whole range of a grey display.
struct Segment {
    min: f64,
    max: f64,
    band: Option<Band>,
}

/// A row of a strip: its boxes from the left, and whether each end has a
/// value.
struct StripRow {
    segments: Vec<Segment>,
    closed_start: bool,
    closed_end: bool,
    /// The values are depths, shown with no sign.
    depth: bool,
}

/// A value at an edge of a strip, and the x of the edge.
#[derive(Clone, Copy)]
struct Edge {
    x: f64,
    value: f64,
}

struct Draw<'a> {
    ink: TextInk,
    unit: f64,
    fonts: &'a mut Fonts,
    out: &'a mut Vec<Item>,
    text: &'a mut Painted,
}

impl Draw<'_> {
    /// The rows of a strip.
    fn strip_rows(data: &KeyData) -> Vec<StripRow> {
        if data.display == RasterDisplay::Grey {
            return vec![StripRow {
                segments: vec![Segment {
                    min: data.bands.min(),
                    max: data.bands.max(),
                    band: None,
                }],
                closed_start: true,
                closed_end: true,
                depth: false,
            }];
        }
        let segment = |r: &KeyRow| Segment {
            min: r.band.min,
            max: r.band.max,
            band: Some(r.band),
        };
        let shown = |range: std::ops::Range<usize>| -> Vec<Segment> {
            (data.rows[range].iter())
                .filter(|r| !r.hidden)
                .map(segment)
                .collect()
        };
        let n = data.rows.len();
        let rows = match data.sea {
            Some(sea) => vec![
                StripRow {
                    segments: shown(0..sea),
                    closed_start: false,
                    closed_end: true,
                    depth: true,
                },
                StripRow {
                    segments: shown(sea..n),
                    closed_start: true,
                    closed_end: false,
                    depth: false,
                },
            ],
            None => vec![StripRow {
                segments: shown(0..n),
                closed_start: false,
                closed_end: false,
                depth: false,
            }],
        };
        rows.into_iter()
            .filter(|r| !r.segments.is_empty())
            .collect()
    }

    /// The values at the edges of a row: each line between two boxes has
    /// the lower value of the box to its right, and a closed end has its
    /// value.
    fn edges(row: &StripRow, x0: f64, width: f64) -> Vec<Edge> {
        let m = row.segments.len();
        let mut edges = Vec::new();
        if row.closed_start {
            edges.push(Edge {
                x: x0,
                value: row.segments[0].min,
            });
        }
        for (k, s) in row.segments.iter().enumerate().skip(1) {
            edges.push(Edge {
                x: x0 + k as f64 * width,
                value: s.min,
            });
        }
        if row.closed_end {
            edges.push(Edge {
                x: x0 + m as f64 * width,
                value: row.segments[m - 1].max,
            });
        }
        edges
    }

    /// The text of a value of a row.
    fn value_text(row: &StripRow, v: f64) -> String {
        let v = if row.depth && v <= SEA_LEVEL { -v } else { v };
        number_text(v)
    }

    /// The text of a value of a row in the second unit, rounded to the
    /// decimals of the value in the first unit.
    fn second_text(row: &StripRow, v: f64, unit: &SecondUnit) -> String {
        let places = decimals(v);
        let c = unit.convert(v);
        let c = if row.depth && v <= SEA_LEVEL { -c } else { c };
        number_at(c, places)
    }

    fn strip(&mut self, key: &ColorKeyItem, data: &KeyData, inner: Rect) {
        let rows = Self::strip_rows(data);
        let Some(most) = rows.iter().map(|r| r.segments.len()).max() else {
            return;
        };
        let size = self.ink.style.size;
        let step = LINE_HEIGHT * size;
        let mm = self.unit * PT_PER_MM;
        let height = key.box_height.map_or(1.5 * size, |h| h * mm);
        // The end labels reach half their width past the ends of the strip.
        let mut reach: f64 = 0.0;
        for row in &rows {
            let m = row.segments.len();
            let ends = [
                row.closed_start.then(|| row.segments[0].min),
                row.closed_end.then(|| row.segments[m - 1].max),
            ];
            for v in ends.into_iter().flatten() {
                let mut w = self.ink.advance(&Self::value_text(row, v), self.fonts);
                if let Some(second) = &key.second_unit {
                    let t = Self::second_text(row, v, second);
                    w = w.max(self.ink.advance(&t, self.fonts));
                }
                reach = reach.max(w / 2.0);
            }
        }
        let fill = (inner.width() - 2.0 * reach).max(0.0) / most as f64;
        let mut width = key.box_width.map_or(fill, |w| (w * mm).min(fill));
        if width <= 0.0 {
            return;
        }
        // A unit at an open start reaches left of the first value, which is
        // one box in from the start.
        let space = 0.3 * size;
        let mut left = reach;
        for row in rows.iter().filter(|r| !r.closed_start && r.closed_end) {
            let first = row.segments[0].max;
            let mut units = vec![(Self::value_text(row, first), data.unit.clone())];
            if let Some(second) = &key.second_unit {
                units.push((Self::second_text(row, first, second), second.name.clone()));
            }
            for (text, unit) in units.iter().filter(|(_, u)| !u.is_empty()) {
                let w = self.ink.advance(text, self.fonts) / 2.0
                    + space
                    + self.ink.advance(unit, self.fonts);
                left = left.max(w - width);
            }
        }
        let x0 = inner.x0 + left;
        width = width.min((inner.x1 - reach - x0) / most as f64);
        if width <= 0.0 {
            return;
        }
        let mut y = inner.y0;
        for (i, row) in rows.iter().enumerate() {
            let row_height = step + height + if key.second_unit.is_some() { step } else { 0.0 };
            if i > 0 {
                y += 0.6 * size;
            }
            if y + row_height > inner.y1 + 1e-9 {
                return;
            }
            let top = y + step;
            self.boxes(row, data, x0, top, width, height);
            let edges = self.thinned(key, row, Self::edges(row, x0, width), width);
            let texts: Vec<String> = (edges.iter())
                .map(|e| Self::value_text(row, e.value))
                .collect();
            let line = top - 0.25 * size;
            self.edge_texts(row, &edges, &texts, &data.unit, line);
            if let Some(second) = &key.second_unit {
                let texts: Vec<String> = (edges.iter())
                    .map(|e| Self::second_text(row, e.value, second))
                    .collect();
                let below = baseline(&self.ink, top + height, self.fonts);
                self.edge_texts(row, &edges, &texts, &second.name, below);
            }
            y += row_height;
        }
    }

    /// The edges of a row whose values fit between their neighbors: every
    /// edge if the boxes are wide enough, or else every second or third
    /// edge, counted from a closed end.
    fn thinned(
        &mut self,
        key: &ColorKeyItem,
        row: &StripRow,
        edges: Vec<Edge>,
        width: f64,
    ) -> Vec<Edge> {
        let mut widest: f64 = 0.0;
        for e in &edges {
            widest = widest.max(
                self.ink
                    .advance(&Self::value_text(row, e.value), self.fonts),
            );
            if let Some(second) = &key.second_unit {
                let t = Self::second_text(row, e.value, second);
                widest = widest.max(self.ink.advance(&t, self.fonts));
            }
        }
        let space = 0.6 * self.ink.style.size;
        let every = ((widest + space) / width).ceil().max(1.0) as usize;
        if every == 1 || edges.is_empty() {
            return edges;
        }
        let anchor = if row.closed_start { 0 } else { edges.len() - 1 };
        (edges.iter().enumerate())
            .filter(|(i, _)| i.abs_diff(anchor) % every == 0)
            .map(|(_, e)| *e)
            .collect()
    }

    /// The boxes of a row, and the lines around them.
    fn boxes(&mut self, row: &StripRow, data: &KeyData, x0: f64, top: f64, width: f64, h: f64) {
        for (k, s) in row.segments.iter().enumerate() {
            let r = Rect::new(
                x0 + k as f64 * width,
                top,
                x0 + (k + 1) as f64 * width,
                top + h,
            );
            match (data.display, s.band) {
                (RasterDisplay::Bands, Some(band)) => self.out.push(fill(r, rgb(band.color))),
                (RasterDisplay::Gradient, Some(band)) => {
                    self.slices(r, |t| rgb(band.gradient_at(s.min + t * (s.max - s.min))));
                }
                _ => {
                    let bands = &data.bands;
                    self.slices(r, |t| {
                        let level = bands.level(s.min + t * (s.max - s.min));
                        let g = (u32::from(level) * 255 / 65535) as u8;
                        Color::rgb(g, g, g)
                    });
                }
            }
        }
        let width_pt = 0.3 * self.unit;
        for k in 0..row.segments.len() {
            let r = Rect::new(
                x0 + k as f64 * width,
                top,
                x0 + (k + 1) as f64 * width,
                top + h,
            );
            self.out.push(Item::Stroke {
                path: r.to_path(0.1),
                color: line_color(),
                width: width_pt,
                round: false,
            });
        }
    }

    /// Fills `r` from left to right with the colors of `color` at 0 to 1.
    fn slices(&mut self, r: Rect, color: impl Fn(f64) -> Color) {
        let w = r.width() / SLICES as f64;
        for i in 0..SLICES {
            // The slices overlap by a hair so that no seam shows.
            let x = r.x0 + i as f64 * w;
            let end = if i + 1 == SLICES { r.x1 } else { x + w * 1.05 };
            let t = (i as f64 + 0.5) / SLICES as f64;
            self.out.push(fill(Rect::new(x, r.y0, end, r.y1), color(t)));
        }
    }

    /// The values of a row at their edges, with the unit word next to the
    /// value nearest an open end: on the open side, or after the last value.
    fn edge_texts(
        &mut self,
        row: &StripRow,
        edges: &[Edge],
        texts: &[String],
        unit: &str,
        baseline: f64,
    ) {
        let space = 0.3 * self.ink.style.size;
        for (e, t) in edges.iter().zip(texts) {
            let w = self.ink.advance(t, self.fonts);
            let to = Affine::translate((e.x - w / 2.0, baseline));
            self.ink.line(t, to, self.fonts, self.text);
        }
        if unit.is_empty() {
            return;
        }
        let (Some(first), Some(last)) = (edges.first(), edges.last()) else {
            return;
        };
        let unit_w = self.ink.advance(unit, self.fonts);
        let at = if !row.closed_start && row.closed_end {
            let w = self.ink.advance(&texts[0], self.fonts);
            first.x - w / 2.0 - space - unit_w
        } else {
            let w = self.ink.advance(&texts[texts.len() - 1], self.fonts);
            last.x + w / 2.0 + space
        };
        let to = Affine::translate((at, baseline));
        self.ink.line(unit, to, self.fonts, self.text);
    }

    fn list(&mut self, key: &ColorKeyItem, data: &KeyData, inner: Rect) {
        let size = self.ink.style.size;
        let mm = self.unit * PT_PER_MM;
        let height = key.box_height.map_or(1.3 * size, |h| h * mm);
        // A row is as tall as its box or its text, whichever is taller, and
        // the box and the text are at its middle.
        let slot = height.max(LINE_HEIGHT * size);
        let gap = 0.6 * size;
        let shown: Vec<&KeyRow> = data.rows.iter().rev().filter(|r| !r.hidden).collect();
        let width = match key.box_width {
            Some(w) => w * mm,
            None => {
                let mut width = 2.6 * size;
                if key.names_in_boxes {
                    for r in &shown {
                        width = width.max(self.ink.advance(&r.name, self.fonts) + 0.8 * size);
                    }
                }
                width
            }
        };
        let mut y = inner.y0;
        for (i, row) in shown.iter().enumerate() {
            if i > 0 {
                y += gap;
            }
            if y + slot > inner.y1 + 1e-9 {
                return;
            }
            let top = y + (slot - height) / 2.0;
            let r = Rect::new(inner.x0, top, inner.x0 + width, top + height);
            let color = match data.display {
                RasterDisplay::Grey => {
                    let mid = (row.band.min + row.band.max) / 2.0;
                    let g = (u32::from(data.bands.level(mid)) * 255 / 65535) as u8;
                    [g, g, g]
                }
                _ => row.band.color,
            };
            self.out.push(fill(r, rgb(color)));
            self.out.push(Item::Stroke {
                path: r.to_path(0.1),
                color: line_color(),
                width: 0.3 * self.unit,
                round: false,
            });
            let line_top = y + (slot - LINE_HEIGHT * size) / 2.0;
            let base = baseline(&self.ink, line_top, self.fonts);
            if key.names_in_boxes && !row.name.is_empty() {
                let w = self.ink.advance(&row.name, self.fonts);
                let (ink_color, halo) = (self.ink.color, self.ink.halo.take());
                self.ink.color = contrast(color);
                let to = Affine::translate((r.center().x - w / 2.0, base));
                self.ink.line(&row.name, to, self.fonts, self.text);
                (self.ink.color, self.ink.halo) = (ink_color, halo);
            }
            let to = Affine::translate((r.x1 + 0.5 * size, base));
            self.ink.line(&row.label, to, self.fonts, self.text);
            y += slot;
        }
    }
}

/// Black or white, whichever reads better on `color`.
fn contrast([r, g, b]: [u8; 3]) -> Color {
    let luma = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    if luma > 140.0 {
        Color::rgb(30, 30, 30)
    } else {
        Color::rgb(250, 250, 250)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_text_groups_thousands_and_drops_trailing_zeros() {
        assert_eq!(number_text(0.0), "0");
        assert_eq!(number_text(-0.0), "0");
        assert_eq!(number_text(250.0), "250");
        assert_eq!(number_text(1000.0), "1,000");
        assert_eq!(number_text(13123.0), "13,123");
        assert_eq!(number_text(1234567.5), "1,234,567.5");
        assert_eq!(number_text(-400.0), "-400");
        assert_eq!(number_text(-4000.0), "-4,000");
        assert_eq!(number_text(2.50), "2.5");
        assert_eq!(number_text(0.125), "0.13");
    }

    #[test]
    fn range_text_names_the_ends() {
        assert_eq!(range_text(0, 3, 0.0, 250.0), "Under 250");
        assert_eq!(range_text(1, 3, 250.0, 500.0), "250–500");
        assert_eq!(range_text(2, 3, 2000.0, 4000.0), "Over 2,000");
        assert_eq!(range_text(0, 1, 0.0, 10.0), "0–10");
        assert_eq!(range_text(1, 3, -50.0, -25.0), "-50 to -25");
    }

    #[test]
    fn the_second_unit_keeps_the_decimals_of_the_first() {
        assert_eq!(number_at(3.28084 * 100.0, decimals(100.0)), "328");
        assert_eq!(number_at(3.28084 * 1.5, decimals(1.5)), "4.9");
    }
}
