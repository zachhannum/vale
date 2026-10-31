//! The map pipeline: query, clip, project, style, label, emit.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};

use anyhow::anyhow;
use kurbo::{Affine, BezPath, Circle, Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{DisplayList, FillRule, Item, Symbol, Symbols, stroke, symbol_items};
use vale_sphere::{
    ClipRect, LonLat, Projection, ProjectionSpec, auto_interval, graticule, graticule_in, wrap180,
};
use vale_store::{FrameRecord, Geometry, GeometryKind, Layer, LayerId, MapId, PageRecord, Value};
use vale_style::expr::Context;
use vale_style::{Extent, Paint, Stroke, StrokeKind, scale_denominator};

use crate::atlas::{self, FrameTarget, PT_PER_MM};
use crate::document::{Document, LayerEntry};
use crate::scatter::{self, ScatterCache, ScatterFill};
use crate::view::{View, ViewTransform};
use crate::{edge_labels, furniture, labels, pick};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Quality {
    Interactive,
    Final,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    pub layer: LayerId,
    pub feature: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnplacedInfo {
    pub layer: String,
    pub text: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelSummary {
    pub placed: usize,
    pub unplaced: Vec<UnplacedInfo>,
}

/// One visible layer in page space. Only features whose bounding box meets the page are present.
pub struct PageLayer<'a> {
    /// The layer with the style of the map.
    pub entry: Cow<'a, LayerEntry>,
    pub layer: &'a Layer,
    /// The geometry kind of the layer.
    pub kind: GeometryKind,
    pub features: Vec<PageFeature>,
    /// The scale denominator of the view.
    pub scale: f64,
}

impl PageLayer<'_> {
    /// The style context of feature `index` of `layer.features`.
    pub fn context(&self, index: usize) -> Context<'_> {
        static NONE: BTreeMap<String, Value> = BTreeMap::new();
        let attributes = self
            .layer
            .features
            .get(index)
            .map_or(&NONE, |f| &f.attributes);
        Context {
            attributes,
            scale: self.scale,
            geometry: self.kind,
        }
    }

    /// How far the paint of feature `index` reaches.
    pub fn extent(&self, index: usize) -> Extent {
        self.entry.style.style.extent(&self.context(index))
    }
}

pub struct PageFeature {
    /// Index in `layer.features`.
    pub index: usize,
    /// Point layers.
    pub points: Vec<Point>,
    /// Line layers, and the outlines of polygons.
    pub lines: Vec<Vec<Point>>,
    /// Polygon layers: closed rings for an even-odd fill.
    pub rings: Vec<Vec<Point>>,
    pub bbox: Rect,
}

/// A symbol that a map drew.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSymbol {
    pub layer: LayerId,
    /// Index in `layer.features`.
    pub feature: usize,
    /// The center in target points.
    pub center: Point,
    /// The box of the symbol in target points.
    pub bounds: Rect,
    /// The place on the sphere of a symbol of a scatter fill. `None` for a
    /// point symbol.
    pub lonlat: Option<LonLat>,
}

#[derive(Clone, Debug)]
pub struct Composed {
    pub list: DisplayList,
    /// The symbols of the point symbols and the scatter fills.
    pub symbols: Vec<PlacedSymbol>,
    /// The view that was used (the fitted one when the document had none).
    pub view: View,
    pub labels: LabelSummary,
    /// Ground distance of one point at the middle of the view.
    pub km_per_point: f64,
    pub graticule_step: Option<f64>,
}

/// The geometry of one feature in projected meters.
struct ProjFeature {
    points: Vec<Point>,
    lines: Vec<Vec<Point>>,
    rings: Vec<Vec<Point>>,
    bbox: Rect,
}

struct Cache {
    spec: ProjectionSpec,
    radius_m: f64,
    projection: Projection,
    outline: Vec<Point>,
    bounds: Rect,
    layers: HashMap<LayerId, Vec<Option<ProjFeature>>>,
    /// The graticule lines over the whole sphere, by interval. Only intervals of 1° or more.
    graticules: HashMap<u64, Vec<Vec<Point>>>,
    /// The graticule lines of the last window, for an interval under 1°.
    window_graticule: Option<WindowGraticule>,
}

struct WindowGraticule {
    interval: f64,
    window: ClipRect,
    lines: Vec<Vec<Point>>,
}

/// The most caches that a pipeline keeps.
const MAX_CACHES: usize = 3;

#[derive(Default)]
pub struct Pipeline {
    /// The most recently used cache first.
    caches: Vec<Cache>,
    scatter: ScatterCache,
}

/// The fonts that ship with the app.
pub fn fonts() -> anyhow::Result<Fonts> {
    crate::fonts::bundled()
}

fn extend_bbox(b: &mut Option<Rect>, p: Point) {
    *b = Some(match *b {
        None => Rect::new(p.x, p.y, p.x, p.y),
        Some(r) => Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y)),
    });
}

fn project_feature(projection: &Projection, geometry: &Geometry) -> Option<ProjFeature> {
    let mut points = Vec::new();
    let mut lines = Vec::new();
    let mut rings = Vec::new();
    match geometry {
        Geometry::Points(ps) => {
            points.extend(ps.iter().filter_map(|p| projection.forward(*p)));
        }
        Geometry::Lines(ls) => {
            for l in ls {
                lines.extend(projection.project_line(l));
            }
        }
        Geometry::Polygons(polys) => {
            for poly in polys {
                rings.extend(projection.project_polygon(poly));
                lines.extend(projection.project_outline_of(poly));
            }
        }
    }
    let mut bbox = None;
    for p in points
        .iter()
        .chain(lines.iter().flatten())
        .chain(rings.iter().flatten())
    {
        extend_bbox(&mut bbox, *p);
    }
    bbox.map(|bbox| ProjFeature {
        points,
        lines,
        rings,
        bbox,
    })
}

fn meets(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && a.x1 >= b.x0 && a.y0 <= b.y1 && a.y1 >= b.y0
}

fn map_pts(vt: &ViewTransform, pts: &[Point]) -> Vec<Point> {
    pts.iter().map(|p| vt.to_page(*p)).collect()
}

fn add_polyline(path: &mut BezPath, pts: &[Point], close: bool) {
    let mut it = pts.iter();
    let Some(first) = it.next() else { return };
    path.move_to(*first);
    for p in it {
        path.line_to(*p);
    }
    if close {
        path.close_path();
    }
}

fn polylines_path<'a>(parts: impl IntoIterator<Item = &'a Vec<Point>>, close: bool) -> BezPath {
    let mut path = BezPath::new();
    for part in parts {
        add_polyline(&mut path, part, close);
    }
    path
}

fn page_feature(vt: &ViewTransform, index: usize, f: &ProjFeature) -> PageFeature {
    PageFeature {
        index,
        points: map_pts(vt, &f.points),
        lines: f.lines.iter().map(|l| map_pts(vt, l)).collect(),
        rings: f.rings.iter().map(|l| map_pts(vt, l)).collect(),
        bbox: vt.page_bbox(f.bbox),
    }
}

/// The layers that a map draws, in draw order. `None` is the legacy map,
/// which draws the visible layers.
fn drawn_layers(doc: &Document, map: Option<MapId>) -> Vec<LayerId> {
    let entries = doc.frame.entries.iter().map(|e| e.layer);
    match map {
        None => entries.filter(|id| doc.visible(*id)).collect(),
        Some(map) => entries.filter(|id| doc.shown(map, *id)).collect(),
    }
}

/// The features of the layers that meet `area`, in page space, with the
/// style overrides of `map`.
fn build_page_layers<'a>(
    doc: &'a Document,
    map: Option<MapId>,
    cache: &Cache,
    vt: &ViewTransform,
    area: Rect,
    ids: &[LayerId],
    scale: f64,
) -> Vec<PageLayer<'a>> {
    let page = area.inflate(8.0, 8.0);
    let mut out = Vec::new();
    for entry in ids.iter().filter_map(|id| doc.map_entry(map, *id)) {
        let (Some(layer), Some(feats)) = (doc.layer(entry.layer), cache.layers.get(&entry.layer))
        else {
            continue;
        };
        // A raster layer is not on the map.
        let Some(kind) = layer.geometry() else {
            continue;
        };
        let features = feats
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.as_ref().map(|f| (i, f)))
            .filter(|(_, f)| meets(vt.page_bbox(f.bbox), page))
            .map(|(i, f)| page_feature(vt, i, f))
            .collect();
        out.push(PageLayer {
            entry,
            layer,
            kind,
            features,
            scale,
        });
    }
    out
}

/// Km per point at the center of the view.
fn km_per_point(cache: &Cache, vt: &ViewTransform) -> f64 {
    let p = cache
        .projection
        .inverse(vt.view.center)
        .unwrap_or([cache.spec.lon0, cache.spec.lat0]);
    let local = cache.projection.local_scale(p).unwrap_or(1.0);
    1.0 / (vt.view.scale * local) / 1000.0
}

/// The features of a layer that one paint draws. `order` is the index of the rule and the index of the symbolizer.
struct Batch {
    order: (usize, usize),
    paint: Paint,
    /// Indices in `PageLayer::features`.
    features: Vec<usize>,
}

/// The bits of a paint, for grouping.
fn paint_key(paint: &Paint) -> [u64; 8] {
    let color = |c: Option<vale_render::Color>| {
        c.map_or(1 << 32, |c| {
            u64::from(u32::from_be_bytes([c.r, c.g, c.b, c.a]))
        })
    };
    let stroke = |s: Option<vale_style::Stroke>| {
        s.map_or([1 << 32, 0], |s| [color(Some(s.color)), s.width.to_bits()])
    };
    let name = |s: &str| {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        h.finish()
    };
    match *paint {
        Paint::Fill(c) => [0, color(Some(c)), 0, 0, 0, 0, 0, 0],
        Paint::Stroke(s) => {
            let [c, w] = stroke(Some(s));
            let [k, a, b] = match s.kind {
                StrokeKind::Solid => [0, 0, 0],
                StrokeKind::Dashed { dash, gap } => [1, dash.to_bits(), gap.to_bits()],
                StrokeKind::Brush => [2, 0, 0],
                StrokeKind::Taper { start } => [3, start.to_bits(), 0],
            };
            [1, c, w, k, a, b, 0, 0]
        }
        Paint::Marker {
            radius,
            fill,
            stroke: st,
        } => {
            let [c, w] = stroke(st);
            [2, radius.to_bits(), color(fill), c, w, 0, 0, 0]
        }
        Paint::Symbol {
            ref symbol,
            size,
            rotation,
            tint,
        } => [
            3,
            name(symbol),
            size.to_bits(),
            rotation.to_bits(),
            color(tint),
            0,
            0,
            0,
        ],
        Paint::Scatter {
            ref symbol,
            size,
            spacing,
            size_jitter,
            rotation_jitter,
            tint,
        } => [
            4,
            name(symbol),
            size.to_bits(),
            spacing.to_bits(),
            size_jitter.to_bits(),
            rotation_jitter.to_bits(),
            color(tint),
            0,
        ],
    }
}

/// The batches of a layer, in draw order: rule by rule, and symbolizer by symbolizer. Features with the same paint share a batch, in the order of the features.
fn batches(pl: &PageLayer<'_>) -> Vec<Batch> {
    let style = &pl.entry.style.style;
    if let Some(paints) = style.constant_paints(pl.scale) {
        return paints
            .into_iter()
            .enumerate()
            .map(|(i, paint)| Batch {
                order: (0, i),
                paint,
                features: (0..pl.features.len()).collect(),
            })
            .collect();
    }
    let mut out: Vec<Batch> = Vec::new();
    let mut index: HashMap<(usize, usize, [u64; 8]), usize> = HashMap::new();
    let rules: Vec<_> = style
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.in_scale(pl.scale))
        .collect();
    for (i, f) in pl.features.iter().enumerate() {
        let ctx = pl.context(f.index);
        for (r, rule) in &rules {
            if !rule.filter.matches(&ctx) {
                continue;
            }
            for (k, symbolizer) in rule.symbolizers.iter().enumerate() {
                let Some(paint) = symbolizer.resolve(&ctx) else {
                    continue;
                };
                let at = *index.entry((*r, k, paint_key(&paint))).or_insert_with(|| {
                    out.push(Batch {
                        order: (*r, k),
                        paint,
                        features: Vec::new(),
                    });
                    out.len() - 1
                });
                out[at].features.push(i);
            }
        }
    }
    out.sort_by_key(|b| b.order);
    out
}

/// What the symbols of a map need.
struct SymbolDraw<'a> {
    symbols: &'a Symbols,
    scatter: &'a mut ScatterCache,
    projection: &'a Projection,
    vt: &'a ViewTransform,
    area: Rect,
    /// The nominal scale denominator of the view. It does not change with a
    /// pan. The spacing of a scatter fill follows it.
    scale: f64,
    /// Target points per point.
    unit: f64,
    /// The sphere radius in meters.
    radius: f64,
    /// Points on the sphere that `area` shows, made at the first scatter.
    view: Option<Vec<LonLat>>,
    /// The scatter symbols of the current layer.
    layer_count: usize,
    placed: Vec<PlacedSymbol>,
}

/// The box of a symbol in symbol units.
fn symbol_box(symbol: &Symbol) -> Rect {
    symbol
        .paths()
        .iter()
        .map(|p| p.path.bounding_box())
        .reduce(|a, b| a.union(b))
        .unwrap_or_default()
}

/// The radius of the dot that stands for a missing symbol, in symbol units.
const MISSING_DOT: f64 = 1.0 / 6.0;

impl SymbolDraw<'_> {
    fn view(&mut self) -> &[LonLat] {
        let (projection, vt, area) = (self.projection, self.vt, self.area);
        self.view.get_or_insert_with(|| {
            const N: usize = 9;
            let mut out = Vec::with_capacity(N * N);
            for i in 0..N {
                for j in 0..N {
                    let t = |k: usize| k as f64 / (N - 1) as f64;
                    let q = Point::new(
                        area.x0 + area.width() * t(i),
                        area.y0 + area.height() * t(j),
                    );
                    out.extend(projection.inverse(vt.from_page(q)));
                }
            }
            out
        })
    }

    /// Draws the instances of a symbol. A missing symbol draws a dot.
    fn push(
        &mut self,
        list: &mut DisplayList,
        name: &str,
        instances: Vec<(usize, Affine, Option<LonLat>)>,
        layer: LayerId,
        tint: Option<vale_render::Color>,
    ) {
        if instances.is_empty() {
            return;
        }
        let symbol = self.symbols.get(name);
        let unit_box = symbol.map_or(
            Rect::new(-MISSING_DOT, -MISSING_DOT, MISSING_DOT, MISSING_DOT),
            |s| symbol_box(s),
        );
        for (feature, a, lonlat) in &instances {
            self.placed.push(PlacedSymbol {
                layer,
                feature: *feature,
                center: *a * Point::ZERO,
                bounds: a.transform_rect_bbox(unit_box),
                lonlat: *lonlat,
            });
        }
        let affines: Vec<Affine> = instances.iter().map(|(_, a, _)| *a).collect();
        match symbol {
            Some(symbol) => list.items.extend(symbol_items(symbol, &affines, tint)),
            None => {
                let mut path = BezPath::new();
                for a in &affines {
                    path.extend(*a * Circle::new(Point::ZERO, MISSING_DOT).to_path(0.01));
                }
                list.push(Item::Fill {
                    path,
                    color: tint.unwrap_or(vale_render::Color::rgb(80, 80, 80)),
                    rule: FillRule::NonZero,
                });
            }
        }
    }
}

/// The affine that maps symbol units to target points: `size` points big,
/// turned `rotation` degrees clockwise, at `at`.
fn symbol_affine(at: Point, size: f64, rotation: f64) -> Affine {
    Affine::translate(at.to_vec2()) * Affine::rotate(rotation.to_radians()) * Affine::scale(size)
}

/// Draws one batch. A paint that does not fit the kind of the layer draws nothing.
fn draw_batch(list: &mut DisplayList, pl: &PageLayer<'_>, batch: &Batch, sd: &mut SymbolDraw<'_>) {
    let features = || batch.features.iter().map(|&i| &pl.features[i]);
    let lines = || polylines_path(features().flat_map(|f| &f.lines), false);
    match (pl.kind, batch.paint.clone()) {
        (GeometryKind::Polygon, Paint::Fill(color)) => {
            for f in features().filter(|f| !f.rings.is_empty()) {
                list.push(Item::Fill {
                    path: polylines_path(&f.rings, true),
                    color,
                    rule: FillRule::EvenOdd,
                });
            }
        }
        (GeometryKind::Polygon | GeometryKind::Line, Paint::Stroke(stroke)) => {
            let line = pl.kind == GeometryKind::Line;
            match stroke.kind {
                StrokeKind::Dashed { dash, gap } => {
                    let parts: Vec<_> = features()
                        .flat_map(|f| stroke::dash(&f.lines, dash, gap))
                        .collect();
                    push_stroke(list, polylines_path(&parts, false), &stroke, false);
                }
                StrokeKind::Taper { start } if line => {
                    for f in features().filter(|f| !f.lines.is_empty()) {
                        let pieces = stroke::taper(&f.lines, start * stroke.width, stroke.width);
                        list.push(Item::Fill {
                            path: stroke::taper_path(&pieces),
                            color: stroke.color,
                            rule: FillRule::NonZero,
                        });
                    }
                }
                StrokeKind::Brush => {
                    let mut items = Vec::new();
                    let mut bounds: Option<Rect> = None;
                    for f in features() {
                        for d in stroke::brush_dabs(&f.lines, stroke.width, f.index as u64) {
                            let circle = Circle::new(d.center, d.radius);
                            let b = circle.bounding_box();
                            bounds = Some(bounds.map_or(b, |r| r.union(b)));
                            let mut color = stroke.color;
                            color.a = (f32::from(color.a) * d.alpha).round() as u8;
                            items.push(Item::Fill {
                                path: circle.to_path(0.1),
                                color,
                                rule: FillRule::NonZero,
                            });
                        }
                    }
                    if let Some(bounds) = bounds {
                        list.push(Item::Rasterize { bounds, items });
                    }
                }
                StrokeKind::Solid | StrokeKind::Taper { .. } => {
                    push_stroke(list, lines(), &stroke, line);
                }
            }
        }
        (
            GeometryKind::Point,
            Paint::Marker {
                radius,
                fill,
                stroke,
            },
        ) if radius > 0.0 => {
            let circles = |r: f64| {
                let mut path = BezPath::new();
                for p in features().flat_map(|f| &f.points) {
                    path.extend(Circle::new(*p, r).to_path(0.1));
                }
                path
            };
            if let Some(color) = fill {
                list.push(Item::Fill {
                    path: circles(radius),
                    color,
                    rule: FillRule::NonZero,
                });
            }
            if let Some(stroke) = stroke {
                list.push(Item::Stroke {
                    path: circles(radius + stroke.width / 2.0),
                    color: stroke.color,
                    width: stroke.width,
                    round: true,
                });
            }
        }
        (
            GeometryKind::Point,
            Paint::Symbol {
                symbol,
                size,
                rotation,
                tint,
            },
        ) if size > 0.0 => {
            let size = size * sd.unit;
            let instances = features()
                .flat_map(|f| f.points.iter().map(move |p| (f.index, *p)))
                .map(|(i, p)| (i, symbol_affine(p, size, rotation), None))
                .collect();
            sd.push(list, &symbol, instances, pl.layer.id, tint);
        }
        (GeometryKind::Polygon, Paint::Scatter { symbol, tint, .. }) => {
            let Some(fill) = ScatterFill::of(&batch.paint) else {
                return;
            };
            let size = fill.size * sd.unit;
            let reach = size / 2.0;
            let shown = sd.area.inflate(reach, reach);
            let (scale, radius) = (sd.scale, sd.radius);
            let view = sd.view().to_vec();
            let mut instances = Vec::new();
            for f in features() {
                let Some(feature) = pl.layer.features.get(f.index) else {
                    continue;
                };
                let points =
                    sd.scatter
                        .get(pl.layer.id, f.index, feature, &fill, scale, radius, &view);
                for p in points.iter() {
                    if sd.layer_count >= scatter::MAX_POINTS {
                        break;
                    }
                    let Some(q) = sd.projection.forward(p.lonlat) else {
                        continue;
                    };
                    let q = sd.vt.to_page(q);
                    if !shown.contains(q) {
                        continue;
                    }
                    sd.layer_count += 1;
                    let a = symbol_affine(q, size * p.scale, p.rotation);
                    instances.push((f.index, a, Some(p.lonlat)));
                }
            }
            sd.push(list, &symbol, instances, pl.layer.id, tint);
        }
        _ => {}
    }
}

fn push_stroke(list: &mut DisplayList, path: BezPath, stroke: &Stroke, round: bool) {
    if !path.elements().is_empty() {
        list.push(Item::Stroke {
            path,
            color: stroke.color,
            width: stroke.width,
            round,
        });
    }
}

const SELECTION: vale_render::Color = vale_render::Color::rgb(230, 70, 30);

/// The color of the paper of a page.
pub const PAPER: vale_render::Color = vale_render::Color::rgb(255, 255, 255);

/// The graticule interval of a window view, in degrees. The lines are at
/// least 80 points apart. `None` when the graticule is off.
fn graticule_step(on: bool, view: &View, radius_m: f64) -> Option<f64> {
    let degrees_per_point = 1.0 / (view.scale * radius_m.to_radians());
    on.then(|| auto_interval(80.0 * degrees_per_point))
}

/// The smallest interval that has a graticule over the whole sphere.
const WORLD_GRATICULE_MIN: f64 = 1.0 - 1e-9;

/// The part of the sphere that `area` shows, padded by one interval. The
/// longitudes are unwrapped about `center_lon`, so they can go past ±180.
/// A pole inside the area gives all longitudes. The window is empty when
/// the area shows no part of the sphere.
pub(crate) fn graticule_window(
    projection: &Projection,
    vt: &ViewTransform,
    area: Rect,
    center_lon: f64,
    interval: f64,
) -> ClipRect {
    const N: usize = 17;
    let mut w = ClipRect {
        lon_min: f64::INFINITY,
        lon_max: f64::NEG_INFINITY,
        lat_min: f64::INFINITY,
        lat_max: f64::NEG_INFINITY,
    };
    for i in 0..N {
        for j in 0..N {
            let t = |k: usize| k as f64 / (N - 1) as f64;
            let q = Point::new(
                area.x0 + area.width() * t(i),
                area.y0 + area.height() * t(j),
            );
            let Some([lon, lat]) = projection.inverse(vt.from_page(q)) else {
                continue;
            };
            let lon = center_lon + wrap180(lon - center_lon);
            w.lon_min = w.lon_min.min(lon);
            w.lon_max = w.lon_max.max(lon);
            w.lat_min = w.lat_min.min(lat);
            w.lat_max = w.lat_max.max(lat);
        }
    }
    if w.lon_min > w.lon_max {
        return w;
    }
    w.lon_min -= interval;
    w.lon_max += interval;
    w.lat_min = (w.lat_min - interval).max(-90.0);
    w.lat_max = (w.lat_max + interval).min(90.0);
    let shows = |lat: f64| {
        projection
            .forward([center_lon, lat])
            .is_some_and(|p| area.contains(vt.to_page(p)))
    };
    for (pole, south) in [(90.0, false), (-90.0, true)] {
        if shows(pole) {
            if south {
                w.lat_min = -90.0;
            } else {
                w.lat_max = 90.0;
            }
            w.lon_min = center_lon - 180.0;
            w.lon_max = center_lon + 180.0;
        }
    }
    w
}

/// A closed path through rings of page millimeters, in target points.
fn shape_path(rings: &[Vec<[f64; 2]>], target: FrameTarget) -> BezPath {
    let rings: Vec<Vec<Point>> = rings
        .iter()
        .map(|r| {
            let points = r.iter().map(|&[x, y]| target.to_target(Point::new(x, y)));
            points.collect()
        })
        .collect();
    polylines_path(&rings, true)
}

impl Cache {
    /// Projects the graticule lines of an interval in the part of the
    /// sphere that `area` shows. An interval of 1° or more covers the
    /// whole sphere.
    fn ensure_graticule(
        &mut self,
        step: Option<f64>,
        vt: &ViewTransform,
        area: Rect,
        center_lon: f64,
    ) {
        let Some(step) = step else { return };
        if step >= WORLD_GRATICULE_MIN {
            if !self.graticules.contains_key(&step.to_bits()) {
                let lines = self.project_lines(&graticule(step));
                self.graticules.insert(step.to_bits(), lines);
            }
            return;
        }
        let window = graticule_window(&self.projection, vt, area, center_lon, step);
        if self
            .window_graticule
            .as_ref()
            .is_some_and(|w| w.interval == step && w.window == window)
        {
            return;
        }
        let lines = self.project_lines(&graticule_in(step, window));
        self.window_graticule = Some(WindowGraticule {
            interval: step,
            window,
            lines,
        });
    }

    fn project_lines(&self, lines: &[Vec<LonLat>]) -> Vec<Vec<Point>> {
        lines
            .iter()
            .flat_map(|l| self.projection.project_line(l))
            .collect()
    }

    /// The projected graticule lines of an interval that `ensure_graticule` made.
    fn graticule_lines(&self, step: f64) -> Option<&[Vec<Point>]> {
        if step >= WORLD_GRATICULE_MIN {
            self.graticules.get(&step.to_bits()).map(Vec::as_slice)
        } else {
            let w = self.window_graticule.as_ref()?;
            (w.interval == step).then_some(w.lines.as_slice())
        }
    }

    /// The longitude at the center of a view.
    fn center_lon(&self, vt: &ViewTransform) -> f64 {
        self.projection
            .inverse(vt.view.center)
            .map_or(self.spec.lon0, |p| p[0])
    }
}

/// The layers that `draw_map` drew, its symbols, and the ground size of one point.
struct DrawnMap<'a> {
    layers: Vec<PageLayer<'a>>,
    symbols: Vec<PlacedSymbol>,
    km_per_point: f64,
}

/// The scale of the symbols of a map.
#[derive(Copy, Clone)]
struct SymbolScale {
    /// The nominal scale denominator of the view.
    nominal: f64,
    /// Target points per point.
    unit: f64,
}

/// Draws a map in `area`: the background if it has one, the water, the
/// layers, the graticule, the outline, and the selection.
#[allow(clippy::too_many_arguments)]
fn draw_map<'a>(
    list: &mut DisplayList,
    doc: &'a Document,
    map: Option<MapId>,
    cache: &Cache,
    vt: &ViewTransform,
    area: Rect,
    background: bool,
    water: bool,
    ids: &[LayerId],
    graticule: Option<(&[Vec<Point>], Stroke)>,
    selection: Option<Selection>,
    scatter: &mut ScatterCache,
    symbol_scale: SymbolScale,
) -> DrawnMap<'a> {
    let style = &doc.frame.style;

    // 1 and 2: background and water.
    if background {
        list.push(Item::Fill {
            path: area.to_path(0.1),
            color: style.background,
            rule: FillRule::NonZero,
        });
    }
    let outline: Vec<Point> = map_pts(vt, &cache.outline);
    let outline_path = polylines_path([&outline], true);
    if water {
        list.push(Item::Fill {
            path: outline_path.clone(),
            color: style.water,
            rule: FillRule::NonZero,
        });
    }

    // 3: layers.
    let km_per_point = km_per_point(cache, vt);
    let scale = scale_denominator(km_per_point * 1000.0);
    let page_layers = build_page_layers(doc, map, cache, vt, area, ids, scale);
    let mut sd = SymbolDraw {
        symbols: doc.symbols(),
        scatter,
        projection: &cache.projection,
        vt,
        area,
        scale: symbol_scale.nominal,
        unit: symbol_scale.unit,
        radius: cache.radius_m,
        view: None,
        layer_count: 0,
        placed: Vec::new(),
    };
    for pl in &page_layers {
        sd.layer_count = 0;
        for batch in batches(pl) {
            draw_batch(list, pl, &batch, &mut sd);
        }
    }
    let symbols = sd.placed;

    // 4: graticule.
    if let Some((lines, stroke)) = graticule {
        let mut path = BezPath::new();
        for l in lines {
            let pts = map_pts(vt, l);
            let mut b = None;
            for p in &pts {
                extend_bbox(&mut b, *p);
            }
            if b.is_some_and(|b| meets(b, area)) {
                add_polyline(&mut path, &pts, false);
            }
        }
        if !path.elements().is_empty() {
            list.push(Item::Stroke {
                path,
                color: stroke.color,
                width: stroke.width,
                round: false,
            });
        }
    }

    // 5: outline.
    list.push(Item::Stroke {
        path: outline_path,
        color: style.outline.color,
        width: style.outline.width,
        round: false,
    });

    // 6: selection.
    if let Some(sel) = selection
        && let Some(pl) = page_layers.iter().find(|pl| pl.layer.id == sel.layer)
        && let Some(f) = pl.features.iter().find(|f| f.index == sel.feature)
    {
        let extent = pl.extent(f.index);
        let width = extent.stroke_width;
        let item = match pl.kind {
            GeometryKind::Point => {
                let mut path = BezPath::new();
                for p in &f.points {
                    path.extend(Circle::new(*p, extent.marker_radius + 3.0).to_path(0.1));
                }
                Item::Stroke {
                    path,
                    color: SELECTION,
                    width: 2.0,
                    round: true,
                }
            }
            GeometryKind::Line => Item::Stroke {
                path: polylines_path(&f.lines, false),
                color: SELECTION,
                width: width + 2.0,
                round: true,
            },
            GeometryKind::Polygon => Item::Stroke {
                path: polylines_path(&f.lines, false),
                color: SELECTION,
                width: 2.0,
                round: false,
            },
        };
        list.push(item);
    }

    DrawnMap {
        layers: page_layers,
        symbols,
        km_per_point,
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline::default()
    }

    /// Projects the features of a layer again at the next compose.
    pub fn forget(&mut self, id: LayerId) {
        for cache in &mut self.caches {
            cache.layers.remove(&id);
        }
        self.scatter.forget(id);
    }

    /// The scatters that the pipeline keeps.
    pub fn scatter_cache(&self) -> &ScatterCache {
        &self.scatter
    }

    pub fn scatter_cache_mut(&mut self) -> &mut ScatterCache {
        &mut self.scatter
    }

    /// The number of projections that the pipeline keeps.
    pub fn cache_count(&self) -> usize {
        self.caches.len()
    }

    /// Brings the projection of a map and the projected geometry of the
    /// layers `ids` up to date with the document.
    fn ensure(
        &mut self,
        doc: &Document,
        map: MapId,
        ids: &[LayerId],
    ) -> anyhow::Result<&mut Cache> {
        let spec = doc.projection(map);
        let radius_m = doc.project.world.radius_km * 1000.0;
        let found = self
            .caches
            .iter()
            .position(|c| c.spec == spec && c.radius_m == radius_m);
        match found {
            Some(i) => {
                let cache = self.caches.remove(i);
                self.caches.insert(0, cache);
            }
            None => {
                let projection =
                    Projection::new(spec, radius_m).map_err(|e| anyhow!("projection: {e}"))?;
                let outline = projection.outline();
                let bounds = projection.bounds();
                let cache = Cache {
                    spec,
                    radius_m,
                    projection,
                    outline,
                    bounds,
                    layers: HashMap::new(),
                    graticules: HashMap::new(),
                    window_graticule: None,
                };
                self.caches.insert(0, cache);
                self.caches.truncate(MAX_CACHES);
            }
        }
        let exists = |id: LayerId| doc.project.layers().iter().any(|l| l.id == id);
        self.scatter.retain(exists);
        let cache = &mut self.caches[0];
        cache.layers.retain(|id, _| exists(*id));
        for id in ids {
            if cache.layers.contains_key(id) {
                continue;
            }
            if let Some(layer) = doc.layer(*id) {
                let feats = layer
                    .features
                    .iter()
                    .map(|f| project_feature(&cache.projection, &f.geometry))
                    .collect();
                cache.layers.insert(*id, feats);
            }
        }
        Ok(cache)
    }

    pub fn resolve_view(&mut self, doc: &Document, size: (f64, f64)) -> anyhow::Result<View> {
        if let Some(v) = doc.frame.view {
            return Ok(v);
        }
        let cache = self.ensure(doc, doc.first_map(), &drawn_layers(doc, None))?;
        Ok(View::fit(cache.bounds, size.0, size.1, 24.0))
    }

    pub fn fit_scale(&mut self, doc: &Document, size: (f64, f64)) -> anyhow::Result<f64> {
        let cache = self.ensure(doc, doc.first_map(), &drawn_layers(doc, None))?;
        Ok(View::fit(cache.bounds, size.0, size.1, 24.0).scale)
    }

    /// The projected bounds of the projection of the first map, in meters.
    pub fn projection_bounds(&mut self, doc: &Document) -> anyhow::Result<Rect> {
        Ok(self.ensure(doc, doc.first_map(), &[])?.bounds)
    }

    /// The projection of a map.
    pub fn projection(&mut self, doc: &Document, map: MapId) -> anyhow::Result<&Projection> {
        Ok(&self.ensure(doc, map, &[])?.projection)
    }

    /// The projected position of a place, in meters. `None` when it is not visible.
    pub fn project_lonlat(&mut self, doc: &Document, p: LonLat) -> anyhow::Result<Option<Point>> {
        Ok(self
            .ensure(doc, doc.first_map(), &[])?
            .projection
            .forward(p))
    }

    pub fn lonlat_to_page(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        p: LonLat,
    ) -> anyhow::Result<Option<Point>> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform::new(view, size.0, size.1);
        Ok(self
            .ensure(doc, doc.first_map(), &[])?
            .projection
            .forward(p)
            .map(|q| vt.to_page(q)))
    }

    pub fn page_to_lonlat(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        q: Point,
    ) -> anyhow::Result<Option<LonLat>> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform::new(view, size.0, size.1);
        Ok(self
            .ensure(doc, doc.first_map(), &[])?
            .projection
            .inverse(vt.from_page(q)))
    }

    pub fn pick(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        at: Point,
    ) -> anyhow::Result<Option<Selection>> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform::new(view, size.0, size.1);
        let ids = drawn_layers(doc, None);
        let cache = self.ensure(doc, doc.first_map(), &ids)?;
        let scale = scale_denominator(km_per_point(cache, &vt) * 1000.0);
        let layers = build_page_layers(doc, None, cache, &vt, vt.page_rect(), &ids, scale);
        Ok(pick::pick(&layers, at))
    }

    /// The feature of a frame under the target point `at`. A point outside
    /// the shape of the frame picks nothing.
    pub fn pick_frame(
        &mut self,
        doc: &Document,
        frame: &FrameRecord,
        target: FrameTarget,
        at: Point,
    ) -> anyhow::Result<Option<Selection>> {
        if !atlas::contains(&frame.shape, target.from_target(at)) {
            return Ok(None);
        }
        let ids = drawn_layers(doc, Some(frame.map));
        let cache = self.ensure(doc, frame.map, &ids)?;
        let vt = atlas::frame_transform(&cache.projection, frame, target);
        let scale = scale_denominator(km_per_point(cache, &vt) * 1000.0);
        let area = frame_area(frame, target);
        let layers = build_page_layers(doc, Some(frame.map), cache, &vt, area, &ids, scale);
        Ok(pick::pick(&layers, at))
    }

    /// The map of the document at the size of a window, with a scale bar.
    pub fn compose(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
    ) -> anyhow::Result<Composed> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform::new(view, size.0, size.1);
        let page = vt.page_rect();
        let radius_m = doc.project.world.radius_km * 1000.0;
        let step = graticule_step(doc.frame.graticule, &view, radius_m);
        let ids = drawn_layers(doc, None);
        let cache = self.ensure(doc, doc.first_map(), &ids)?;
        let center_lon = cache.center_lon(&vt);
        cache.ensure_graticule(step, &vt, page, center_lon);
        let cache = &self.caches[0];
        let graticule = step
            .and_then(|s| cache.graticule_lines(s))
            .map(|lines| (lines, doc.frame.style.graticule));

        // The view scale is the nominal scale: it is the same after a pan.
        let symbol_scale = SymbolScale {
            nominal: scale_denominator(1.0 / view.scale),
            unit: 1.0,
        };
        let mut list = DisplayList::new(size.0, size.1);
        let drawn = draw_map(
            &mut list,
            doc,
            None,
            cache,
            &vt,
            page,
            true,
            true,
            &ids,
            graticule,
            selection,
            &mut self.scatter,
            symbol_scale,
        );

        // 7 to 9: scale bar, labels.
        let (bar_items, bar_rect) =
            furniture::scale_bar(page, drawn.km_per_point, doc.project.world.units, fonts);
        let mut label_summary = LabelSummary::default();
        if doc.frame.labels {
            let classes: Vec<_> = (drawn.layers.iter())
                .map(|pl| doc.label_classes(None, pl.layer.id))
                .collect();
            let (items, summary) = labels::place(
                &drawn.layers,
                &classes,
                page,
                fonts,
                quality,
                &[bar_rect],
                &drawn.symbols,
            );
            list.items.extend(items);
            label_summary = summary;
        }
        list.items.extend(bar_items);

        Ok(Composed {
            list,
            symbols: drawn.symbols,
            view,
            labels: label_summary,
            km_per_point: drawn.km_per_point,
            graticule_step: step,
        })
    }

    /// A map frame of a page, in a target. The list is the size of the page
    /// in the target. The frame draws the layers that its map shows, inside
    /// its shape, with no background.
    #[allow(clippy::too_many_arguments)]
    pub fn compose_frame(
        &mut self,
        doc: &Document,
        page: &PageRecord,
        frame: &FrameRecord,
        target: FrameTarget,
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
    ) -> anyhow::Result<Composed> {
        let page_box = Rect::from_points(
            target.to_target(Point::ZERO),
            target.to_target(Point::new(page.width_mm, page.height_mm)),
        );
        self.compose_frame_part(doc, frame, target, page_box, fonts, quality, selection)
    }

    /// The part of a map frame in `part`, a box in target points. The list
    /// reaches to the bottom-right corner of `part`. The layers and the
    /// labels are only the ones in `part`.
    #[allow(clippy::too_many_arguments)]
    pub fn compose_frame_part(
        &mut self,
        doc: &Document,
        frame: &FrameRecord,
        target: FrameTarget,
        part: Rect,
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
    ) -> anyhow::Result<Composed> {
        self.compose_frame_inner(doc, frame, target, part, fonts, quality, selection, true)
    }

    /// The water of a map frame, inside its shape. The raster layers of the
    /// frame go over it, and `compose_frame_over` goes over them.
    pub fn compose_frame_under(
        &mut self,
        doc: &Document,
        frame: &FrameRecord,
        target: FrameTarget,
        part: Rect,
    ) -> anyhow::Result<DisplayList> {
        let ids = drawn_layers(doc, Some(frame.map));
        let cache = self.ensure(doc, frame.map, &ids)?;
        let vt = atlas::frame_transform(&cache.projection, frame, target);
        let outline = polylines_path([&map_pts(&vt, &cache.outline)], true);
        let mut list = DisplayList::new(part.x1, part.y1);
        list.push(Item::PushClip {
            path: shape_path(&frame.shape.rings(), target),
            rule: FillRule::EvenOdd,
        });
        list.push(Item::Fill {
            path: outline,
            color: doc.frame.style.water,
            rule: FillRule::NonZero,
        });
        list.push(Item::PopClip);
        Ok(list)
    }

    /// A list that fills the shape of a map frame. A mask for the raster
    /// layers of the frame comes from it. The list is `size` points big.
    pub fn frame_mask(frame: &FrameRecord, target: FrameTarget, size: (f64, f64)) -> DisplayList {
        let mut list = DisplayList::new(size.0, size.1);
        list.push(Item::PushClip {
            path: shape_path(&frame.shape.rings(), target),
            rule: FillRule::EvenOdd,
        });
        list.push(Item::Fill {
            path: Rect::new(0.0, 0.0, size.0, size.1).to_path(0.1),
            color: PAPER,
            rule: FillRule::NonZero,
        });
        list.push(Item::PopClip);
        list
    }

    /// The part of a map frame over its raster layers: the same as
    /// `compose_frame_part` with no water.
    #[allow(clippy::too_many_arguments)]
    pub fn compose_frame_over(
        &mut self,
        doc: &Document,
        frame: &FrameRecord,
        target: FrameTarget,
        part: Rect,
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
    ) -> anyhow::Result<Composed> {
        self.compose_frame_inner(doc, frame, target, part, fonts, quality, selection, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn compose_frame_inner(
        &mut self,
        doc: &Document,
        frame: &FrameRecord,
        target: FrameTarget,
        part: Rect,
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
        water: bool,
    ) -> anyhow::Result<Composed> {
        let ids = drawn_layers(doc, Some(frame.map));
        let cache = self.ensure(doc, frame.map, &ids)?;
        let vt = atlas::frame_transform(&cache.projection, frame, target);
        let area = frame_area(frame, target).intersect(part);
        let step = atlas::graticule_interval(&cache.projection, frame);
        cache.ensure_graticule(step, &vt, area, frame.center[0]);
        let cache = &self.caches[0];
        let [r, g, b] = frame.graticule.color;
        let stroke = Stroke::solid(vale_render::Color::rgb(r, g, b), frame.graticule.width);
        let graticule = step
            .and_then(|s| cache.graticule_lines(s))
            .map(|lines| (lines, stroke));

        let mut list = DisplayList::new(part.x1, part.y1);
        list.push(Item::PushClip {
            path: shape_path(&frame.shape.rings(), target),
            rule: FillRule::EvenOdd,
        });
        // The frame scale is the nominal scale, in each target.
        let symbol_scale = SymbolScale {
            nominal: frame.scale,
            unit: target.pts_per_mm / PT_PER_MM,
        };
        let drawn = draw_map(
            &mut list,
            doc,
            Some(frame.map),
            cache,
            &vt,
            area,
            false,
            water,
            &ids,
            graticule,
            selection,
            &mut self.scatter,
            symbol_scale,
        );
        let edge = if step.is_some() {
            edge_labels::edge_labels(&cache.projection, frame, target, fonts)
        } else {
            Vec::new()
        };
        let mut label_summary = LabelSummary::default();
        if doc.frame.labels {
            let classes: Vec<_> = (drawn.layers.iter())
                .map(|pl| doc.label_classes(Some(frame.map), pl.layer.id))
                .collect();
            let obstacles: Vec<Rect> = (edge.iter())
                .filter(|l| !l.outside)
                .map(|l| edge_labels::quad_bounds(&l.quad))
                .collect();
            let (items, summary) = labels::place(
                &drawn.layers,
                &classes,
                area,
                fonts,
                quality,
                &obstacles,
                &drawn.symbols,
            );
            list.items.extend(items);
            label_summary = summary;
        }
        list.items.extend(edge_labels::items(&edge, false));
        list.push(Item::PopClip);
        if frame.border.on {
            let [r, g, b] = frame.border.color;
            list.push(Item::Stroke {
                path: shape_path(&frame.shape.rings(), target),
                color: vale_render::Color::rgb(r, g, b),
                width: frame.border.width,
                round: false,
            });
        }
        list.items.extend(edge_labels::items(&edge, true));

        Ok(Composed {
            list,
            symbols: drawn.symbols,
            view: vt.view,
            labels: label_summary,
            km_per_point: drawn.km_per_point,
            graticule_step: step,
        })
    }

    /// A whole page in points: the paper, each frame from the bottom frame to
    /// the top frame, and then the furniture.
    pub fn compose_page(
        &mut self,
        doc: &Document,
        page: &PageRecord,
        fonts: &mut Fonts,
        quality: Quality,
    ) -> anyhow::Result<DisplayList> {
        let target = FrameTarget {
            origin: Point::ZERO,
            pts_per_mm: PT_PER_MM,
        };
        let size = target.to_target(Point::new(page.width_mm, page.height_mm));
        let mut list = DisplayList::new(size.x, size.y);
        list.push(Item::Fill {
            path: Rect::new(0.0, 0.0, size.x, size.y).to_path(0.1),
            color: PAPER,
            rule: FillRule::NonZero,
        });
        for frame in &page.frames {
            let composed = self.compose_frame(doc, page, frame, target, fonts, quality, None)?;
            list.items.extend(composed.list.items);
        }
        for rec in &page.furniture {
            let items = furniture::items(self, doc, rec, target, fonts, furniture::Purpose::Export);
            list.items.extend(items);
        }
        Ok(list)
    }
}

/// The scale denominator that the rules of the layers of a frame see: the
/// scale at the center of the frame.
pub(crate) fn frame_rule_scale(projection: &Projection, frame: &FrameRecord) -> f64 {
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = atlas::frame_transform(projection, frame, target);
    let p = projection.inverse(vt.view.center).unwrap_or(frame.center);
    let local = projection.local_scale(p).unwrap_or(1.0);
    scale_denominator(1.0 / (vt.view.scale * local))
}

/// The items of a legend swatch of a rule in `rect`, in target points: a
/// rectangle for a polygon layer, a zigzag for a line layer, and a point at
/// the middle for a point layer. An expression draws its fallback. A brush
/// stroke draws as a solid stroke, and a scatter fill draws one symbol.
#[allow(clippy::too_many_arguments)]
pub(crate) fn swatch(
    doc: &Document,
    entry: &LayerEntry,
    layer: &Layer,
    kind: GeometryKind,
    rule: &vale_style::Rule,
    rect: Rect,
    unit: f64,
    projection: &Projection,
) -> Vec<Item> {
    let c = rect.center();
    let ring = vec![
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x1, rect.y1),
        Point::new(rect.x0, rect.y1),
        Point::new(rect.x0, rect.y0),
    ];
    let zigzag = (0..=3)
        .map(|i| {
            let y = if i % 2 == 0 { rect.y1 } else { rect.y0 };
            Point::new(rect.x0 + rect.width() * f64::from(i) / 3.0, y)
        })
        .collect();
    let feature = PageFeature {
        index: usize::MAX,
        points: vec![c],
        lines: match kind {
            GeometryKind::Polygon => vec![ring.clone()],
            GeometryKind::Line => vec![zigzag],
            GeometryKind::Point => vec![],
        },
        rings: vec![ring],
        bbox: rect,
    };
    let pl = PageLayer {
        entry: Cow::Borrowed(entry),
        layer,
        kind,
        features: vec![feature],
        scale: 0.0,
    };
    let vt = ViewTransform::new(
        View {
            center: Point::ZERO,
            scale: 1.0,
            rotation: 0.0,
        },
        rect.width(),
        rect.height(),
    );
    let mut scatter = ScatterCache::default();
    let mut sd = SymbolDraw {
        symbols: doc.symbols(),
        scatter: &mut scatter,
        projection,
        vt: &vt,
        area: rect,
        scale: 0.0,
        unit,
        radius: 1.0,
        view: None,
        layer_count: 0,
        placed: Vec::new(),
    };
    let mut list = DisplayList::new(rect.x1, rect.y1);
    for (k, symbolizer) in rule.symbolizers.iter().enumerate() {
        let paint = match symbolizer.fallback() {
            Some(Paint::Stroke(s)) if s.kind == StrokeKind::Brush => Paint::Stroke(Stroke {
                kind: StrokeKind::Solid,
                ..s
            }),
            Some(Paint::Scatter {
                symbol, size, tint, ..
            }) if kind == GeometryKind::Polygon => {
                let size = (size * unit).min(rect.height());
                let a = symbol_affine(c, size, 0.0);
                sd.push(&mut list, &symbol, vec![(0, a, None)], layer.id, tint);
                continue;
            }
            Some(paint) => paint,
            None => continue,
        };
        let batch = Batch {
            order: (0, k),
            paint,
            features: vec![0],
        };
        draw_batch(&mut list, &pl, &batch, &mut sd);
    }
    list.items
}

/// The box of the shape of a frame, in target points.
pub(crate) fn frame_area(frame: &FrameRecord, target: FrameTarget) -> Rect {
    let b = atlas::shape_bbox(&frame.shape);
    Rect::from_points(
        target.to_target(Point::new(b.x0, b.y0)),
        target.to_target(Point::new(b.x1, b.y1)),
    )
}
