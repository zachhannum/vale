//! Map furniture: text, legends, color keys, scale bars, and north arrows.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vale_labeler::{
    Caps, Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    PointPosition, TextStyle, place_labels,
};
use vale_render::{Color, DisplayList, FillRule, GlyphRun, Item, Rgba};
use vale_sphere::{LonLat, Projection, distance_km};
use vale_store::{
    FrameRecord, Furniture, FurnitureRecord, FurnitureText, LayerId, LegendItem, NorthArrowItem,
    TextAlign, TextCase, TextItem, Units,
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
pub(crate) const PAD: f64 = 3.0;
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
                ..Default::default()
            }),
            duplicate_distance: None,
            fallbacks: Default::default(),
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
        parallels: Vec::new(),
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
                ..Default::default()
            }),
            duplicate_distance: None,
            fallbacks: Default::default(),
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
        parallels: Vec::new(),
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

/// The labeler style of a furniture style, with the size in target points.
pub(crate) fn text_style(style: &FurnitureText, unit: f64, fonts: &mut Fonts) -> TextStyle {
    let size = style.size * unit;
    TextStyle {
        weight: style.weight,
        italic: style.italic,
        letter_spacing: style.letter_spacing * size,
        caps: match style.case {
            TextCase::SmallCaps => Caps::Small,
            TextCase::AllSmallCaps => Caps::AllSmall,
            _ => Caps::Normal,
        },
        ..TextStyle::new(crate::fonts::family(style.font.as_deref(), fonts), size)
    }
}

pub(crate) fn rgba([r, g, b, a]: [u8; 4]) -> Color {
    Color::rgba(r, g, b, a)
}

/// A furniture style in a target: the labeler style, the colors, the halo,
/// and the case. The halo and the case are the same as those of a label.
pub(crate) struct TextInk {
    pub style: TextStyle,
    pub color: Color,
    /// The color and the width of the halo, in target points.
    pub halo: Option<(Color, f64)>,
    pub case: TextCase,
}

/// The halos and the glyphs of some text. The halos go under all glyphs.
#[derive(Default)]
pub(crate) struct Painted {
    pub halos: Vec<Item>,
    pub glyphs: Vec<Item>,
}

impl Painted {
    pub fn into_items(self) -> impl Iterator<Item = Item> {
        self.halos.into_iter().chain(self.glyphs)
    }
}

impl TextInk {
    pub fn new(style: &FurnitureText, unit: f64, fonts: &mut Fonts) -> TextInk {
        TextInk {
            style: text_style(style, unit, fonts),
            color: rgba(style.color),
            halo: (style.halo).then(|| (rgba(style.halo_color), style.halo_width * unit)),
            case: style.case,
        }
    }

    /// The width of `text` in its case.
    pub fn advance(&self, text: &str, fonts: &mut Fonts) -> f64 {
        fonts.measure(&self.case.apply(text), &self.style).advance
    }

    /// One line of `text` in its case with the baseline start at the origin
    /// of `to`.
    pub fn line(&self, text: &str, to: Affine, fonts: &mut Fonts, out: &mut Painted) {
        let text = self.case.apply(text);
        let outlines = self.halo.is_some();
        let Some(line) = glyph_line(&text, &self.style, self.color, to, outlines, fonts) else {
            return;
        };
        if let Some((color, width)) = self.halo {
            out.halos.push(Item::Stroke {
                path: line.outlines,
                color,
                width,
                round: true,
            });
        }
        out.glyphs.extend(line.glyphs);
    }
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

/// The glyphs of `text`, the shown text of a text item, in `rect`, in
/// target points. A line that does not fit above the bottom of `rect` is
/// left out.
fn text_items(item: &TextItem, text: &str, rect: Rect, unit: f64, fonts: &mut Fonts) -> Vec<Item> {
    let ink = TextInk::new(&item.style, unit, fonts);
    let style = &ink.style;
    let metrics = fonts.measure("Hg", style);
    let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
    let step = LINE_HEIGHT * style.size;
    let mut out = Painted::default();
    let text = item.style.case.apply(text);
    for (k, line) in wrap(&text, style, rect.width(), fonts).iter().enumerate() {
        let baseline = rect.y0 + ascent + k as f64 * step;
        if baseline + descent > rect.y1 + 1e-9 {
            break;
        }
        let advance = fonts.measure(line, style).advance;
        let x = match item.align {
            TextAlign::Left => rect.x0,
            TextAlign::Center => rect.x0 + (rect.width() - advance) / 2.0,
            TextAlign::Right => rect.x1 - advance,
        };
        // The text is in its case already, and a case is the same twice.
        ink.line(line, Affine::translate((x, baseline)), fonts, &mut out);
    }
    out.into_items().collect()
}

/// The scale of `km_per_mm` ground kilometers per page millimeter as text.
/// The format `fraction` gives "1:25,000,000". The format `relative` gives
/// "1 cm = 250 km" for the page unit `cm` and "1 in = 40.0 mi" for `in`. The
/// numbers have three significant digits. `None` for an unknown format or
/// unit.
pub fn scale_value(format: &str, page_unit: &str, km_per_mm: f64, units: Units) -> Option<String> {
    match format {
        "fraction" => {
            const MM_PER_KM: f64 = 1e6;
            let n = significant(km_per_mm * MM_PER_KM).round().max(1.0);
            Some(format!("1:{}", world::grouped(n as u64)))
        }
        "relative" => {
            let (page_mm, page_symbol) = match page_unit {
                "cm" => (10.0, "cm"),
                "in" => (25.4, "in"),
                _ => return None,
            };
            let km = km_per_mm * page_mm;
            let (unit_km, symbol) = world::unit_for(km, units);
            let value = significant(km / unit_km);
            let value = if value >= 100.0 {
                world::grouped(value as u64)
            } else {
                world::three_significant(value)
            };
            Some(format!("1 {page_symbol} = {value} {symbol}"))
        }
        _ => None,
    }
}

/// `v` rounded to three significant digits.
fn significant(v: f64) -> f64 {
    if !(v.is_finite() && v > 0.0) {
        return 0.0;
    }
    let step = 10f64.powi(v.log10().floor() as i32 - 2);
    (v / step).round() * step
}

/// The date `days` days after 1970-01-01 as text. The format `long` gives
/// "November 8, 2026", and the format `iso` gives "2026-11-08". `None` for
/// an unknown format.
pub fn date_value(format: &str, days: i64) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let (y, m, d) = civil(days);
    match format {
        "long" => Some(format!("{} {d}, {y}", MONTHS[m as usize - 1])),
        "iso" => Some(format!("{y:04}-{m:02}-{d:02}")),
        _ => None,
    }
}

/// The year, the month, and the day of `days` days after 1970-01-01, in the
/// Gregorian calendar.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The values of the page functions for a text item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageSample {
    page_name: String,
    /// From 1.
    page_number: usize,
    page_count: usize,
    /// The name of the map of the frame. `None`: the item has no frame.
    map_name: Option<String>,
    /// Ground kilometers per page millimeter at the center of the frame.
    km_per_mm: Option<f64>,
    units: Units,
    /// Days after 1970-01-01.
    today: i64,
}

impl vale_style::expr::PageValues for PageSample {
    fn value(&self, name: &str, args: &[vale_style::expr::Value]) -> vale_style::expr::Value {
        use vale_style::expr::Value;
        let arg = |i: usize| match args.get(i) {
            Some(Value::Text(t)) => t.as_str(),
            _ => "",
        };
        let text = |t: Option<String>| t.map_or(Value::Null, Value::Text);
        match name {
            "scale" => {
                let unit = if arg(1).is_empty() { "cm" } else { arg(1) };
                let km = self.km_per_mm;
                text(km.and_then(|km| scale_value(arg(0), unit, km, self.units)))
            }
            "map_name" => text(self.map_name.clone()),
            "page_name" => Value::Text(self.page_name.clone()),
            "page_number" => Value::Number(self.page_number as f64),
            "page_count" => Value::Number(self.page_count as f64),
            "date" => text(date_value(arg(0), self.today)),
            _ => Value::Null,
        }
    }
}

/// The page values of a sample, and whether a frame value was asked for.
#[derive(Default)]
struct AtlasValues {
    sample: PageSample,
    asked_frame: std::cell::Cell<bool>,
}

impl vale_style::expr::PageValues for AtlasValues {
    fn value(&self, name: &str, args: &[vale_style::expr::Value]) -> vale_style::expr::Value {
        if matches!(name, "scale" | "map_name") {
            self.asked_frame.set(true);
        }
        self.sample.value(name, args)
    }
}

/// The text that a text item shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownText {
    pub text: String,
    /// The text uses a frame value, and the item has no frame.
    pub missing_frame: bool,
}

/// The template of `values` evaluated. A template that does not parse shows
/// as its text.
fn evaluate(template: &str, values: &AtlasValues) -> String {
    use vale_style::expr::{Context, Expr};
    static NONE: std::collections::BTreeMap<String, vale_style::expr::Value> =
        std::collections::BTreeMap::new();
    let Ok(expr) = Expr::parse_lenient(template) else {
        return template.to_string();
    };
    let ctx = Context {
        attributes: &NONE,
        scale: 0.0,
        geometry: vale_store::GeometryKind::Point,
        shape: None,
        radius_km: 0.0,
        page: Some(values),
        rasters: None,
        index: 0,
    };
    expr.eval(&ctx).label().unwrap_or_default()
}

/// Whether a text template uses a value of a frame: the scale or the map
/// name.
pub fn uses_frame(template: &str) -> bool {
    let values = AtlasValues::default();
    evaluate(template, &values);
    values.asked_frame.get()
}

/// The page values of the item `rec` on the day `today`, in days after
/// 1970-01-01. The frame values come from the frame of the item.
pub fn page_sample(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    today: i64,
) -> PageSample {
    let atlas = &doc.project.atlas;
    let index = (atlas.pages.iter())
        .position(|p| p.furniture.iter().any(|f| f.id == rec.id))
        .unwrap_or(0);
    let world = &doc.project.world;
    let mut sample = PageSample {
        page_name: atlas.pages.get(index).map_or(String::new(), |p| {
            crate::ui::pad::atlas::page_title(index, &p.name)
        }),
        page_number: index + 1,
        page_count: atlas.pages.len(),
        units: world.units,
        today,
        ..PageSample::default()
    };
    if let Some(frame) = rec.item.frame().and_then(|id| atlas.frame(id)) {
        sample.map_name = atlas.map(frame.map).map(|m| m.name.clone());
        if let Ok(projection) = pipeline.projection(doc, frame.map) {
            sample.km_per_mm = scale_at_center(projection, frame, world.radius_km);
        }
    }
    sample
}

/// The text of the text item `rec` on the day `today`, in days after
/// 1970-01-01.
pub fn shown_text(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    today: i64,
) -> ShownText {
    let Furniture::Text(item) = &rec.item else {
        return ShownText {
            text: String::new(),
            missing_frame: false,
        };
    };
    let values = AtlasValues {
        sample: page_sample(pipeline, doc, rec, today),
        ..AtlasValues::default()
    };
    let text = evaluate(&item.template, &values);
    let missing_frame = values.asked_frame.get() && values.sample.km_per_mm.is_none();
    ShownText {
        text,
        missing_frame,
    }
}

/// The days from 1970-01-01 to today, in UTC.
pub fn today() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

/// The box of an item in page millimeters.
pub fn rect_of(rec: &FurnitureRecord) -> Rect {
    let [x0, y0, x1, y1] = rec.rect;
    Rect::new(x0, y0, x1, y1).abs()
}

/// The items of one item of furniture in a target: vector paths and glyphs.
/// The screen and the export draw the same items, but for an item that
/// draws nothing in the export the screen draws a faint dashed box: an item
/// with no frame, a color key whose frame does not show its layer, and an
/// arrow that hides because north varies.
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
        let shown = shown_text(pipeline, doc, rec, today());
        let mut out = match purpose {
            Purpose::Screen if shown.missing_frame => placeholder(),
            _ => Vec::new(),
        };
        out.extend(text_items(item, &shown.text, rect, unit, fonts));
        return out;
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
                crate::scale_bar::draw(item, rect, unit, km_per_point, fonts).items
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
        Furniture::ColorKey(item) => match crate::color_key::data(doc, frame, item) {
            Some(data) => crate::color_key::draw(item, &data, rect, unit, fonts),
            None => placeholder(),
        },
    }
}

/// An item of furniture as a list in page points, and the box in page
/// millimeters that the list covers: the box of the item with
/// [`RENDER_MARGIN_MM`] around it.
pub fn list(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    fonts: &mut Fonts,
    purpose: Purpose,
) -> (DisplayList, Rect) {
    let rect = rect_of(rec).inflate(RENDER_MARGIN_MM, RENDER_MARGIN_MM);
    let target = FrameTarget {
        origin: Point::new(-rect.x0 * PT_PER_MM, -rect.y0 * PT_PER_MM),
        pts_per_mm: PT_PER_MM,
    };
    let mut list = DisplayList::new(rect.width() * PT_PER_MM, rect.height() * PT_PER_MM);
    list.items = items(pipeline, doc, rec, target, fonts, purpose);
    (list, rect)
}

/// An item of furniture as an image of `scale` pixels per page point, and
/// the box in page millimeters that the image covers.
pub fn render(
    pipeline: &mut Pipeline,
    doc: &Document,
    rec: &FurnitureRecord,
    fonts: &mut Fonts,
    scale: f64,
    purpose: Purpose,
) -> anyhow::Result<(Rgba, Rect)> {
    let (list, rect) = list(pipeline, doc, rec, fonts, purpose);
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
    const SAMPLES: usize = 17;
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
    let color = rgba(item.color);
    let to = Affine::translate(rect.center().to_vec2()) * Affine::rotate(angle.to_radians());
    let ink = TextInk::new(&item.letter_style, unit, fonts);
    let metrics = fonts.measure(&ink.case.apply("N"), &ink.style);
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
    let mut letter = Painted::default();
    ink.line("N", at, fonts, &mut letter);
    out.extend(letter.into_items());
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
    let (x, bottom) = (rect.x0 + pad, rect.y1 - pad + 1e-9);
    let mut y = rect.y0 + pad;
    let mut out = Vec::new();
    let mut text = Painted::default();
    // The height of a line of `ink` and the baseline in a line at `y`.
    let line = |ink: &TextInk, fonts: &mut Fonts| {
        let metrics = fonts.measure("Hg", &ink.style);
        let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
        let step = LINE_HEIGHT * ink.style.size;
        (step, move |y: f64| {
            y + (step - ascent - descent) / 2.0 + ascent
        })
    };
    if !legend.title.is_empty() {
        let ink = TextInk::new(&legend.title_style, unit, fonts);
        let (step, baseline) = line(&ink, fonts);
        if y + step > bottom {
            return out;
        }
        let to = Affine::translate((x, baseline(y)));
        ink.line(&legend.title, to, fonts, &mut text);
        y += step + 0.25 * ink.style.size;
    }
    let ink = TextInk::new(&legend.entry_style, unit, fonts);
    let (step, baseline) = line(&ink, fonts);
    let size = ink.style.size;
    let (width, height) = (1.8 * size, 0.9 * size);
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
        let to = Affine::translate((x + width + 0.5 * size, baseline(y)));
        ink.line(&row.label, to, fonts, &mut text);
        y += step;
    }
    out.extend(text.into_items());
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
    fn the_scale_value_has_three_significant_digits() {
        let value = |f, u, km| scale_value(f, u, km, Units::Metric).unwrap();
        assert_eq!(value("fraction", "", 25.034), "1:25,000,000");
        assert_eq!(value("fraction", "", 0.012_345_6), "1:12,300");
        assert_eq!(value("fraction", "", 1e-9), "1:1");
        assert_eq!(value("relative", "cm", 25.034), "1 cm = 250 km");
        assert_eq!(value("relative", "cm", 123.456), "1 cm = 1,230 km");
        assert_eq!(value("relative", "cm", 0.01), "1 cm = 100 m");
        let km = 40.0 * world::large_unit(Units::Imperial).0 / 25.4;
        let inch = scale_value("relative", "in", km, Units::Imperial);
        assert_eq!(inch.as_deref(), Some("1 in = 40.0 mi"));
        assert_eq!(scale_value("words", "cm", 1.0, Units::Metric), None);
        assert_eq!(scale_value("relative", "mm", 1.0, Units::Metric), None);
    }

    #[test]
    fn the_date_value_has_a_long_and_an_iso_format() {
        // 2026-11-08 is day 20,765.
        assert_eq!(date_value("long", 20_765).unwrap(), "November 8, 2026");
        assert_eq!(date_value("iso", 20_765).unwrap(), "2026-11-08");
        assert_eq!(date_value("iso", 0).unwrap(), "1970-01-01");
        assert_eq!(date_value("iso", 11_016).unwrap(), "2000-02-29");
        assert_eq!(date_value("short", 0), None);
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
                style: style(10.0),
                align,
                ..TextItem::default()
            };
            lines(&text_items(&item, "Title\nSecond\nThird", rect, 1.0, fonts))
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
