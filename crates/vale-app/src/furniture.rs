//! Map furniture: text, legends, scale bars, and north arrows.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    PointPosition, TextStyle, place_labels,
};
use vale_render::{Color, DisplayList, FillRule, GlyphRun, Item, Rgba};
use vale_sphere::{LonLat, Projection, distance_km};
use vale_store::{
    FrameRecord, Furniture, FurnitureRecord, FurnitureText, LayerId, LegendItem, NorthArrowItem,
    ScaleBarItem, TextAlign, TextItem, Units,
};

use crate::atlas::{self, FrameTarget, PT_PER_MM, frame_transform};
use crate::document::Document;
use crate::pipeline::{self, Pipeline};
use crate::world;

/// What the furniture is drawn for. On the screen, an item that draws
/// nothing in an export draws a faint dashed box, so it stays selectable.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    Screen,
    Export,
}

/// The space between the box of an item and its content, in points.
const PAD: f64 = 3.0;
/// The largest change of the direction of north in a frame, in degrees,
/// for an arrow that hides when north varies.
pub const MAX_VARIATION: f64 = 10.0;
/// The space around an item that [`render`] draws, in millimeters.
pub const RENDER_MARGIN_MM: f64 = 2.0;

/// The height of a line of text, as a multiple of the text size.
const LINE_HEIGHT: f64 = 1.2;

/// The largest of 1, 2, 5 times a power of ten that is at most `v`.
pub fn nice(v: f64) -> f64 {
    let p = 10f64.powf(v.log10().floor());
    [5.0, 2.0, 1.0]
        .iter()
        .map(|m| m * p)
        .find(|c| *c <= v * (1.0 + 1e-12))
        .unwrap_or(p)
}

/// The scale bar and the rectangle that it covers.
pub fn scale_bar(
    page: Rect,
    km_per_point: f64,
    units: Units,
    fonts: &mut Fonts,
) -> (Vec<Item>, Rect) {
    if !(km_per_point.is_finite() && km_per_point > 0.0)
        || page.width() < 240.0
        || page.height() < 120.0
    {
        return (vec![], Rect::ZERO);
    }
    let (unit_km, symbol) = world::unit_for(140.0 * km_per_point, units);
    let value = nice(140.0 * km_per_point / unit_km);
    let length = value * unit_km / km_per_point;
    let text = format!("{} {symbol}", value.round() as i64);
    let (x0, y) = (page.x0 + 22.0, page.y1 - 22.0);
    let x1 = x0 + length;
    let ink = Color::rgb(40, 40, 40);

    let mut path = BezPath::new();
    path.move_to((x0, y - 5.0));
    path.line_to((x0, y));
    path.line_to((x1, y));
    path.line_to((x1, y - 5.0));

    let input = LabelInput {
        bounds: page,
        classes: vec![LabelClass {
            name: "scale".to_string(),
            style: TextStyle::new(crate::fonts::DEFAULT_FAMILY, 10.0),
            priority: 0,
            margin: 0.0,
            placement: Placement::Point(PointPlacement {
                positions: vec![PointPosition::TopRight],
                offset: 0.0,
            }),
            duplicate_distance: None,
        }],
        features: vec![Feature {
            id: 0,
            class: 0,
            text,
            priority: 0.0,
            geometry: Geometry::Point {
                position: Point::new(x0, y - 7.0),
                symbol_radius: 0.0,
            },
        }],
        obstacles: vec![],
        options: Options {
            seed: 1,
            improve: false,
        },
    };
    let labeling = place_labels(fonts, &input);

    let mut covered = Rect::new(x0, y - 5.0, x1, y);
    let mut glyphs = Vec::new();
    if let Some(label) = labeling.placed.first() {
        covered = covered.union(label.bounds);
        for run in &label.glyph_runs {
            glyphs.push(Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
                text: label.text.clone(),
                color: ink,
            }));
        }
    }
    let backing = covered.inflate(8.0, 8.0);
    let mut items = vec![
        Item::Fill {
            path: backing.to_path(0.1),
            color: Color::rgba(255, 255, 255, 200),
            rule: FillRule::NonZero,
        },
        Item::Stroke {
            path,
            color: ink,
            width: 1.5,
            round: false,
        },
    ];
    items.extend(glyphs);
    (items, backing)
}

/// One line of shaped text.
pub(crate) struct GlyphLine {
    pub glyphs: Vec<Item>,
    /// The outlines of the glyphs, if they were asked for.
    pub outlines: BezPath,
}

/// The glyphs of one line of `text` with the baseline start at the origin of
/// `to`. `None` if the text has no glyphs.
pub(crate) fn glyph_line(
    text: &str,
    style: &TextStyle,
    color: Color,
    to: Affine,
    outlines: bool,
    fonts: &mut Fonts,
) -> Option<GlyphLine> {
    let advance = fonts.measure(text, style).advance;
    let room = 4.0 * style.size + advance;
    let input = LabelInput {
        bounds: Rect::new(-room, -room, room, room),
        classes: vec![LabelClass {
            name: "text".to_string(),
            style: style.clone(),
            priority: 0,
            margin: 0.0,
            placement: Placement::Point(PointPlacement {
                positions: vec![PointPosition::TopRight],
                offset: 0.0,
            }),
            duplicate_distance: None,
        }],
        features: vec![Feature {
            id: 0,
            class: 0,
            text: text.to_string(),
            priority: 0.0,
            geometry: Geometry::Point {
                position: Point::ZERO,
                symbol_radius: 0.0,
            },
        }],
        obstacles: vec![],
        options: Options {
            seed: 1,
            improve: false,
        },
    };
    let labeling = place_labels(fonts, &input);
    let label = labeling.placed.first()?;
    let start = (label.glyph_runs.iter())
        .flat_map(|r| r.glyphs.first())
        .map(|g| g.transform.translation())
        .next()?;
    let to = to * Affine::translate(-start);
    let glyphs = (label.glyph_runs.iter())
        .map(|run| {
            Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: (run.glyphs.iter())
                    .map(|g| (g.id, to * g.transform))
                    .collect(),
                text: label.text.clone(),
                color,
            })
        })
        .collect();
    let outlines = if outlines {
        to * label.outlines()
    } else {
        BezPath::new()
    };
    Some(GlyphLine { glyphs, outlines })
}

/// The text style of furniture, with the size in target points.
pub(crate) fn text_style(style: &FurnitureText, unit: f64, fonts: &mut Fonts) -> TextStyle {
    TextStyle {
        weight: style.weight,
        italic: style.italic,
        ..TextStyle::new(
            crate::fonts::family(style.font.as_deref(), fonts),
            style.size * unit,
        )
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::rgb(r, g, b)
}

/// The lines of `text`: one paragraph for each line break, with the words
/// wrapped at `width`. A word wider than `width` has a line of its own.
pub fn wrap(text: &str, style: &TextStyle, width: f64, fonts: &mut Fonts) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
                continue;
            }
            let longer = format!("{line} {word}");
            if fonts.measure(&longer, style).advance <= width {
                line = longer;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            }
        }
        lines.push(line);
    }
    lines
}

/// The glyphs of a text item in `rect`, in target points. A line that does
/// not fit above the bottom of `rect` is left out.
fn text_items(item: &TextItem, rect: Rect, unit: f64, fonts: &mut Fonts) -> Vec<Item> {
    let style = text_style(&item.style, unit, fonts);
    let color = rgb(item.style.color);
    let metrics = fonts.measure("Hg", &style);
    let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
    let step = LINE_HEIGHT * style.size;
    let mut out = Vec::new();
    for (k, line) in wrap(&item.text, &style, rect.width(), fonts)
        .iter()
        .enumerate()
    {
        let baseline = rect.y0 + ascent + k as f64 * step;
        if baseline + descent > rect.y1 + 1e-9 {
            break;
        }
        let advance = fonts.measure(line, &style).advance;
        let x = match item.align {
            TextAlign::Left => rect.x0,
            TextAlign::Center => rect.x0 + (rect.width() - advance) / 2.0,
            TextAlign::Right => rect.x1 - advance,
        };
        let to = Affine::translate((x, baseline));
        if let Some(glyphs) = glyph_line(line, &style, color, to, false, fonts) {
            out.extend(glyphs.glyphs);
        }
    }
    out
}

/// The box of an item in page millimeters.
pub fn rect_of(rec: &FurnitureRecord) -> Rect {
    let [x0, y0, x1, y1] = rec.rect;
    Rect::new(x0, y0, x1, y1).abs()
}

/// The items of one item of furniture in a target: vector paths and glyphs.
/// The screen and the export draw the same items, but for an item that
/// draws nothing in the export the screen draws a faint dashed box: an item
/// with no frame, and an arrow that hides because north varies.
pub fn items(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    target: FrameTarget,
    fonts: &mut Fonts,
    purpose: Purpose,
) -> Vec<Item> {
    let mm = rect_of(rec);
    let rect = Rect::from_points(
        target.to_target(Point::new(mm.x0, mm.y0)),
        target.to_target(Point::new(mm.x1, mm.y1)),
    );
    let unit = target.pts_per_mm / PT_PER_MM;
    let placeholder = || match purpose {
        Purpose::Screen => placeholder(rect, unit),
        Purpose::Export => Vec::new(),
    };
    if let Furniture::Text(item) = &rec.item {
        return text_items(item, rect, unit, fonts);
    }
    let Some(frame) = (rec.item.frame()).and_then(|id| doc.project.atlas.frame(id)) else {
        return placeholder();
    };
    let Ok(projection) = pipeline.projection(doc, frame.map) else {
        return placeholder();
    };
    let world = &doc.project.world;
    match &rec.item {
        Furniture::Text(_) => Vec::new(),
        Furniture::ScaleBar(item) => match scale_at_center(projection, frame, world.radius_km) {
            Some(km_per_mm) => {
                let km_per_point = km_per_mm / target.pts_per_mm;
                scale_bar_items(item, rect, unit, km_per_point, world.units, fonts)
            }
            None => placeholder(),
        },
        Furniture::NorthArrow(item) => match north(projection, frame) {
            Some((_, variation)) if item.hide_if_varies && variation > MAX_VARIATION => {
                placeholder()
            }
            Some((angle, _)) => north_items(item, rect, unit, angle, fonts),
            None => placeholder(),
        },
        Furniture::Legend(item) => legend_items(doc, projection, frame, item, rect, unit, fonts),
    }
}

/// An item of furniture as an image of `scale` pixels per page point, and
/// the box in page millimeters that the image covers: the box of the item
/// with [`RENDER_MARGIN_MM`] around it.
pub fn render(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    fonts: &mut Fonts,
    scale: f64,
    purpose: Purpose,
) -> anyhow::Result<(Rgba, Rect)> {
    let rect = rect_of(rec).inflate(RENDER_MARGIN_MM, RENDER_MARGIN_MM);
    let target = FrameTarget {
        origin: Point::new(-rect.x0 * PT_PER_MM, -rect.y0 * PT_PER_MM),
        pts_per_mm: PT_PER_MM,
    };
    let mut list = DisplayList::new(rect.width() * PT_PER_MM, rect.height() * PT_PER_MM);
    list.items = items(pipeline, doc, rec, target, fonts, purpose);
    Ok((vale_render::render_rgba(&list, scale)?, rect))
}

/// A faint dashed box.
fn placeholder(rect: Rect, unit: f64) -> Vec<Item> {
    let ring = vec![
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x1, rect.y1),
        Point::new(rect.x0, rect.y1),
        Point::new(rect.x0, rect.y0),
    ];
    let mut path = BezPath::new();
    for part in vale_render::stroke::dash(&[ring], 3.0 * unit, 2.0 * unit) {
        let mut points = part.iter();
        if let Some(first) = points.next() {
            path.move_to(*first);
            points.for_each(|p| path.line_to(*p));
        }
    }
    vec![Item::Stroke {
        path,
        color: Color::rgba(120, 120, 120, 140),
        width: 0.75 * unit,
        round: false,
    }]
}

/// Ground kilometers per page millimeter along the horizontal of the page at
/// the center of a frame. `None` if the center is not on the globe.
pub fn scale_at_center(
    projection: &Projection,
    frame: &FrameRecord,
    radius_km: f64,
) -> Option<f64> {
    const HALF: f64 = 5.0;
    let vt = frame_transform(projection, frame, FrameTarget::MM);
    let c = atlas::anchor(&frame.shape);
    let a = projection.inverse(vt.from_page(c - Vec2::new(HALF, 0.0)))?;
    let b = projection.inverse(vt.from_page(c + Vec2::new(HALF, 0.0)))?;
    let km = distance_km(a, b, radius_km) / (2.0 * HALF);
    (km.is_finite() && km > 0.0).then_some(km)
}

/// The ground distance of one centimeter on the page, for example
/// "1 cm = 25.0 km".
pub fn scale_text(km_per_mm: f64, units: Units) -> String {
    format!("1 cm = {}", world::distance(10.0 * km_per_mm, units))
}

/// A nice value as text: a whole number with groups of three digits, or a
/// decimal number with no extra zeros.
fn value_text(value: f64) -> String {
    if value >= 1.0 {
        world::grouped(value.round() as u64)
    } else {
        let decimals = (-value.log10().floor()).max(0.0) as usize;
        format!("{value:.decimals$}")
    }
}

/// The text and the length in target points of the longest nice scale bar
/// that fits in `room` target points.
fn bar_length(room: f64, km_per_point: f64, units: Units) -> Option<(String, f64)> {
    let most = room * km_per_point;
    if !(most.is_finite() && most > 0.0) {
        return None;
    }
    let (unit_km, symbol) = world::unit_for(most, units);
    let value = nice(most / unit_km);
    let length = value * unit_km / km_per_point;
    Some((format!("{} {symbol}", value_text(value)), length))
}

fn scale_bar_items(
    item: &ScaleBarItem,
    rect: Rect,
    unit: f64,
    km_per_point: f64,
    units: Units,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let pad = PAD * unit;
    let Some((text, length)) = bar_length(rect.width() - 2.0 * pad, km_per_point, units) else {
        return Vec::new();
    };
    let style = text_style(&item.style, unit, fonts);
    let color = rgb(item.style.color);
    let width = unit;
    let tick = 0.4 * style.size;
    let (x0, y) = (rect.x0 + pad, rect.y1 - pad - width / 2.0);
    let x1 = x0 + length;
    let mut path = BezPath::new();
    path.move_to((x0, y - tick));
    path.line_to((x0, y));
    path.line_to((x1, y));
    path.line_to((x1, y - tick));
    let mut out = vec![Item::Stroke {
        path,
        color,
        width,
        round: false,
    }];
    let advance = fonts.measure(&text, &style).advance;
    let baseline = y - tick - 0.3 * style.size;
    let to = Affine::translate(((x0 + x1 - advance) / 2.0, baseline));
    if let Some(line) = glyph_line(&text, &style, color, to, false, fonts) {
        out.extend(line.glyphs);
    }
    out
}

/// The direction of north at a place, in degrees clockwise from the top of
/// the page.
fn north_at(projection: &Projection, vt: &crate::view::ViewTransform, p: LonLat) -> Option<f64> {
    const STEP: f64 = 0.01;
    let (q, sign) = if p[1] + STEP <= 90.0 {
        ([p[0], p[1] + STEP], 1.0)
    } else {
        ([p[0], p[1] - STEP], -1.0)
    };
    let a = vt.to_page(projection.forward(p)?);
    let b = vt.to_page(projection.forward(q)?);
    let d = (b - a) * sign;
    (d.hypot() > 1e-12).then(|| d.x.atan2(-d.y).to_degrees())
}

/// The direction of north at the center of a frame, in degrees clockwise
/// from the top of the page, and the largest difference from it in the
/// frame. The difference comes from a grid of points over the box of the
/// frame, with the corners and the middle of the edges, and a place near a
/// pole has a difference of 180°. `None` if the center is not on the globe.
pub fn north(projection: &Projection, frame: &FrameRecord) -> Option<(f64, f64)> {
    const NEAR_POLE: f64 = 90.0 - 0.01;
    // An odd number, so the grid has the middle of each edge.
    const SAMPLES: usize = 5;
    if frame.center[1].abs() > NEAR_POLE {
        return Some((0.0, 180.0));
    }
    let vt = frame_transform(projection, frame, FrameTarget::MM);
    let angle = north_at(projection, &vt, frame.center)?;
    let b = atlas::shape_bbox(&frame.shape);
    let c = b.center();
    let mut variation: f64 = 0.0;
    let t = |k: usize| k as f64 / (SAMPLES - 1) as f64;
    for i in 0..SAMPLES {
        for j in 0..SAMPLES {
            let p = Point::new(b.x0 + b.width() * t(i), b.y0 + b.height() * t(j));
            // A point a bit inside the box, so a point on the edge of a
            // rectangle is inside the shape.
            let p = c + (p - c) * 0.99;
            if p == c || !atlas::contains(&frame.shape, p) {
                continue;
            }
            let Some(lonlat) = projection.inverse(vt.from_page(p)) else {
                continue;
            };
            if lonlat[1].abs() > NEAR_POLE {
                variation = 180.0;
                continue;
            }
            if let Some(a) = north_at(projection, &vt, lonlat) {
                let d = (a - angle).rem_euclid(360.0);
                variation = variation.max(d.min(360.0 - d));
            }
        }
    }
    Some((angle, variation))
}

/// An arrow and the letter N, turned `angle` degrees clockwise.
fn north_items(
    item: &NorthArrowItem,
    rect: Rect,
    unit: f64,
    angle: f64,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let side = rect.width().min(rect.height()) - 2.0 * PAD * unit;
    if side <= 0.0 {
        return Vec::new();
    }
    let color = rgb(item.color);
    let to = Affine::translate(rect.center().to_vec2()) * Affine::rotate(angle.to_radians());
    let style = TextStyle::new(crate::fonts::DEFAULT_FAMILY, 0.3 * side);
    let metrics = fonts.measure("N", &style);
    let cap = if metrics.cap_height > 0.0 {
        metrics.cap_height
    } else {
        0.7 * metrics.ascent
    };
    let top = -side / 2.0;
    let tip = top + cap + 0.1 * side;
    let bottom = side / 2.0;
    let half = 0.18 * side;
    let notch = bottom - 0.25 * (bottom - tip);
    let outline = BezPath::from_vec(vec![
        kurbo::PathEl::MoveTo(Point::new(0.0, tip)),
        kurbo::PathEl::LineTo(Point::new(half, bottom)),
        kurbo::PathEl::LineTo(Point::new(0.0, notch)),
        kurbo::PathEl::LineTo(Point::new(-half, bottom)),
        kurbo::PathEl::ClosePath,
    ]);
    let left = BezPath::from_vec(vec![
        kurbo::PathEl::MoveTo(Point::new(0.0, tip)),
        kurbo::PathEl::LineTo(Point::new(0.0, notch)),
        kurbo::PathEl::LineTo(Point::new(-half, bottom)),
        kurbo::PathEl::ClosePath,
    ]);
    let mut out = vec![
        Item::Fill {
            path: to * left,
            color,
            rule: FillRule::NonZero,
        },
        Item::Stroke {
            path: to * outline,
            color,
            width: 0.75 * unit,
            round: false,
        },
    ];
    let at = to * Affine::translate((-metrics.advance / 2.0, top + cap));
    if let Some(line) = glyph_line("N", &style, color, at, false, fonts) {
        out.extend(line.glyphs);
    }
    out
}

/// One entry of a legend.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendRow {
    pub layer: LayerId,
    /// The name of the rule, empty for a rule with no name. The layer and the
    /// name are the key of [`LegendItem::entries`].
    pub rule: String,
    /// The place of the rule in the style of the layer in the map.
    pub index: usize,
    /// The text of the entry.
    pub label: String,
    pub hidden: bool,
}

/// The entries of a legend: one for each rule of each vector layer that the
/// map of the frame shows, from the top layer down, with only the rules at
/// the scale of the frame. The list has the hidden entries too. The text of
/// an entry is its name in the legend, or else the name of the layer for a
/// layer with one rule, or else the name of the rule.
pub fn legend_rows(
    doc: &Document,
    projection: &Projection,
    frame: &FrameRecord,
    legend: &LegendItem,
) -> Vec<LegendRow> {
    let scale = pipeline::frame_rule_scale(projection, frame);
    let mut out = Vec::new();
    for id in doc.frame.entries.iter().rev().map(|e| e.layer) {
        let Some(layer) = doc.layer(id) else {
            continue;
        };
        if !doc.shown(frame.map, id) || layer.geometry().is_none() {
            continue;
        }
        let Some(entry) = doc.map_entry(Some(frame.map), id) else {
            continue;
        };
        let rules = &entry.style.style.rules;
        let rules: Vec<_> = (rules.iter().enumerate())
            .filter(|(_, r)| r.in_scale(scale))
            .collect();
        let one = rules.len() == 1;
        for (index, rule) in rules {
            let change = legend.entries.get(&(id, rule.name.clone()));
            let label = match change.and_then(|c| c.name.clone()) {
                Some(name) => name,
                None if one || rule.name.is_empty() => layer.name.clone(),
                None => rule.name.clone(),
            };
            out.push(LegendRow {
                layer: id,
                rule: rule.name.clone(),
                index,
                label,
                hidden: change.is_some_and(|c| c.hidden),
            });
        }
    }
    out
}

/// The title and the entries of a legend that are not hidden. An entry has
/// a swatch of its rule and its text. A line that does not fit above the
/// bottom of `rect` is left out.
fn legend_items(
    doc: &Document,
    projection: &Projection,
    frame: &FrameRecord,
    legend: &LegendItem,
    rect: Rect,
    unit: f64,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let pad = PAD * unit;
    let style = text_style(&legend.style, unit, fonts);
    let color = rgb(legend.style.color);
    let metrics = fonts.measure("Hg", &style);
    let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
    let step = LINE_HEIGHT * style.size;
    let (x, bottom) = (rect.x0 + pad, rect.y1 - pad + 1e-9);
    let mut y = rect.y0 + pad;
    let mut out = Vec::new();
    let baseline = |y: f64| y + (step - ascent - descent) / 2.0 + ascent;
    if !legend.title.is_empty() {
        if y + step > bottom {
            return out;
        }
        let to = Affine::translate((x, baseline(y)));
        if let Some(line) = glyph_line(&legend.title, &style, color, to, false, fonts) {
            out.extend(line.glyphs);
        }
        y += step + 0.25 * style.size;
    }
    let (width, height) = (1.8 * style.size, 0.9 * style.size);
    for row in legend_rows(doc, projection, frame, legend) {
        if row.hidden {
            continue;
        }
        if y + step > bottom {
            break;
        }
        let (Some(layer), Some(entry)) = (
            doc.layer(row.layer),
            doc.map_entry(Some(frame.map), row.layer),
        ) else {
            continue;
        };
        let (Some(kind), Some(rule)) = (layer.geometry(), entry.style.style.rules.get(row.index))
        else {
            continue;
        };
        let top = y + (step - height) / 2.0;
        let swatch = Rect::new(x, top, x + width, top + height);
        out.extend(pipeline::swatch(
            doc, &entry, layer, kind, rule, swatch, unit, projection,
        ));
        let to = Affine::translate((x + width + 0.5 * style.size, baseline(y)));
        if let Some(line) = glyph_line(&row.label, &style, color, to, false, fonts) {
            out.extend(line.glyphs);
        }
        y += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(size: f64) -> FurnitureText {
        FurnitureText {
            size,
            ..FurnitureText::default()
        }
    }

    /// The text and the start of the first glyph of each line.
    fn lines(items: &[Item]) -> Vec<(String, Point)> {
        (items.iter())
            .filter_map(|i| match i {
                Item::Glyphs(run) => Some((run.text.clone(), run.glyphs[0].1 * Point::ZERO)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn text_wraps_at_the_width_and_at_line_breaks() {
        let mut fonts = crate::pipeline::fonts().unwrap();
        let s = text_style(&style(10.0), 1.0, &mut fonts);
        let width = fonts.measure("three four", &s).advance + 0.5;
        let lines = wrap("one two three four\nfive", &s, width, &mut fonts);
        assert_eq!(lines, ["one two", "three four", "five"]);
        let long = wrap("unbreakable", &s, 1.0, &mut fonts);
        assert_eq!(long, ["unbreakable"]);
    }

    #[test]
    fn text_aligns_in_its_box_and_leaves_out_lines_below_it() {
        let mut fonts = crate::pipeline::fonts().unwrap();
        let s = text_style(&style(10.0), 1.0, &mut fonts);
        let advance = fonts.measure("Title", &s).advance;
        let rect = Rect::new(100.0, 50.0, 300.0, 80.0);
        let start = |align: TextAlign, fonts: &mut Fonts| {
            let item = TextItem {
                text: "Title\nSecond\nThird".to_string(),
                style: style(10.0),
                align,
            };
            lines(&text_items(&item, rect, 1.0, fonts))
        };
        let left = start(TextAlign::Left, &mut fonts);
        // Two lines of 12 points fit in 30 points, and the third does not.
        let texts: Vec<_> = left.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["Title", "Second"]);
        assert!((left[1].1.y - left[0].1.y - 12.0).abs() < 1e-6);
        let x = |lines: &[(String, Point)]| lines[0].1.x;
        assert!((x(&left) - 100.0).abs() < 1.5, "{}", x(&left));
        let center = start(TextAlign::Center, &mut fonts);
        assert!((x(&center) - (200.0 - advance / 2.0)).abs() < 1.5);
        let right = start(TextAlign::Right, &mut fonts);
        assert!((x(&right) - (300.0 - advance)).abs() < 1.5);
    }
}
