//! The map pipeline: query, clip, project, style, label, emit.

use std::collections::{BTreeMap, HashMap};

use anyhow::anyhow;
use kurbo::{BezPath, Circle, Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{DisplayList, FillRule, Item};
use vale_sphere::{LonLat, Projection, ProjectionSpec, graticule, nice_step};
use vale_store::{FrameRecord, Geometry, GeometryKind, Layer, LayerId, MapId, PageRecord, Value};
use vale_style::expr::Context;
use vale_style::{Extent, Paint, scale_denominator};

use crate::atlas::{self, FrameTarget, PT_PER_MM};
use crate::document::{Document, LayerEntry};
use crate::view::{View, ViewTransform};
use crate::{furniture, labels, pick};

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
    pub entry: &'a LayerEntry,
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

#[derive(Clone, Debug)]
pub struct Composed {
    pub list: DisplayList,
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
    graticules: HashMap<u64, Vec<Vec<Point>>>,
}

/// The most caches that a pipeline keeps.
const MAX_CACHES: usize = 3;

#[derive(Default)]
pub struct Pipeline {
    /// The most recently used cache first.
    caches: Vec<Cache>,
}

/// Registers both Noto Sans files.
pub fn fonts() -> anyhow::Result<Fonts> {
    let mut fonts = Fonts::new();
    fonts.register(include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf").to_vec())?;
    fonts.register(include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf").to_vec())?;
    Ok(fonts)
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

/// The features of the layers that meet `area`, in page space.
fn build_page_layers<'a>(
    doc: &'a Document,
    cache: &Cache,
    vt: &ViewTransform,
    area: Rect,
    ids: &[LayerId],
    scale: f64,
) -> Vec<PageLayer<'a>> {
    let page = area.inflate(8.0, 8.0);
    let mut out = Vec::new();
    for entry in ids.iter().filter_map(|id| doc.entry(*id)) {
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
fn paint_key(paint: &Paint) -> [u64; 6] {
    let color = |c: Option<vale_render::Color>| {
        c.map_or(1 << 32, |c| {
            u64::from(u32::from_be_bytes([c.r, c.g, c.b, c.a]))
        })
    };
    let stroke = |s: Option<vale_style::Stroke>| {
        s.map_or([1 << 32, 0], |s| [color(Some(s.color)), s.width.to_bits()])
    };
    match *paint {
        Paint::Fill(c) => [0, color(Some(c)), 0, 0, 0, 0],
        Paint::Stroke(s) => {
            let [c, w] = stroke(Some(s));
            [1, c, w, 0, 0, 0]
        }
        Paint::Marker {
            radius,
            fill,
            stroke: st,
        } => {
            let [c, w] = stroke(st);
            [2, radius.to_bits(), color(fill), c, w, 0]
        }
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
    let mut index: HashMap<(usize, usize, [u64; 6]), usize> = HashMap::new();
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

/// Draws one batch. A paint that does not fit the kind of the layer draws nothing.
fn draw_batch(list: &mut DisplayList, pl: &PageLayer<'_>, batch: &Batch) {
    let features = || batch.features.iter().map(|&i| &pl.features[i]);
    let lines = || polylines_path(features().flat_map(|f| &f.lines), false);
    match (pl.kind, batch.paint) {
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
            let path = lines();
            if !path.elements().is_empty() {
                list.push(Item::Stroke {
                    path,
                    color: stroke.color,
                    width: stroke.width,
                    round: pl.kind == GeometryKind::Line,
                });
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
        _ => {}
    }
}

const SELECTION: vale_render::Color = vale_render::Color::rgb(230, 70, 30);

/// The color of the paper of a page.
pub const PAPER: vale_render::Color = vale_render::Color::rgb(255, 255, 255);

/// The graticule step of a view, in degrees. `None` when the graticule is off.
fn graticule_step(on: bool, view: &View, radius_m: f64) -> Option<f64> {
    let degrees_per_point = 1.0 / (view.scale * radius_m * std::f64::consts::PI / 180.0);
    on.then(|| nice_step(80.0 * degrees_per_point))
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
    /// Projects the graticule lines of a step.
    fn ensure_graticule(&mut self, step: Option<f64>) {
        let Some(step) = step else { return };
        if self.graticules.contains_key(&step.to_bits()) {
            return;
        }
        let lines: Vec<Vec<Point>> = graticule(step)
            .iter()
            .flat_map(|l| self.projection.project_line(l))
            .collect();
        self.graticules.insert(step.to_bits(), lines);
    }
}

/// The layers that `draw_map` drew, and the ground size of one point.
struct DrawnMap<'a> {
    layers: Vec<PageLayer<'a>>,
    km_per_point: f64,
}

/// Draws a map in `area`: the background if it has one, the water, the
/// layers, the graticule, the outline, and the selection.
#[allow(clippy::too_many_arguments)]
fn draw_map<'a>(
    list: &mut DisplayList,
    doc: &'a Document,
    cache: &Cache,
    vt: &ViewTransform,
    area: Rect,
    background: bool,
    water: bool,
    ids: &[LayerId],
    graticule_step: Option<f64>,
    selection: Option<Selection>,
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
    let page_layers = build_page_layers(doc, cache, vt, area, ids, scale);
    for pl in &page_layers {
        for batch in batches(pl) {
            draw_batch(list, pl, &batch);
        }
    }

    // 4: graticule.
    if let Some(step) = graticule_step
        && let Some(lines) = cache.graticules.get(&step.to_bits())
    {
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
                color: style.graticule.color,
                width: style.graticule.width,
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
        km_per_point,
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline { caches: Vec::new() }
    }

    /// Projects the features of a layer again at the next compose.
    pub fn forget(&mut self, id: LayerId) {
        for cache in &mut self.caches {
            cache.layers.remove(&id);
        }
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
                };
                self.caches.insert(0, cache);
                self.caches.truncate(MAX_CACHES);
            }
        }
        let cache = &mut self.caches[0];
        cache
            .layers
            .retain(|id, _| doc.project.layers().iter().any(|l| l.id == *id));
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
        let layers = build_page_layers(doc, cache, &vt, vt.page_rect(), &ids, scale);
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
        let layers = build_page_layers(doc, cache, &vt, area, &ids, scale);
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
        cache.ensure_graticule(step);
        let cache = &*cache;

        let mut list = DisplayList::new(size.0, size.1);
        let drawn = draw_map(
            &mut list, doc, cache, &vt, page, true, true, &ids, step, selection,
        );

        // 7 to 9: scale bar, labels.
        let (bar_items, bar_rect) =
            furniture::scale_bar(page, drawn.km_per_point, doc.project.world.units, fonts);
        let mut label_summary = LabelSummary::default();
        if doc.frame.labels {
            let (items, summary) = labels::place(&drawn.layers, page, fonts, quality, &[bar_rect]);
            list.items.extend(items);
            label_summary = summary;
        }
        list.items.extend(bar_items);

        Ok(Composed {
            list,
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
        let radius_m = doc.project.world.radius_km * 1000.0;
        let ids = drawn_layers(doc, Some(frame.map));
        let cache = self.ensure(doc, frame.map, &ids)?;
        let vt = atlas::frame_transform(&cache.projection, frame, target);
        let step = graticule_step(frame.graticule, &vt.view, radius_m);
        cache.ensure_graticule(step);
        let cache = &*cache;

        let mut list = DisplayList::new(part.x1, part.y1);
        let area = frame_area(frame, target).intersect(part);
        list.push(Item::PushClip {
            path: shape_path(&frame.shape.rings(), target),
            rule: FillRule::EvenOdd,
        });
        let drawn = draw_map(
            &mut list, doc, cache, &vt, area, false, water, &ids, step, selection,
        );
        let mut label_summary = LabelSummary::default();
        if doc.frame.labels {
            let (items, summary) = labels::place(&drawn.layers, area, fonts, quality, &[]);
            list.items.extend(items);
            label_summary = summary;
        }
        list.push(Item::PopClip);

        Ok(Composed {
            list,
            view: vt.view,
            labels: label_summary,
            km_per_point: drawn.km_per_point,
            graticule_step: step,
        })
    }

    /// A whole page in points: the paper, then each frame from the bottom
    /// frame to the top frame.
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
        Ok(list)
    }
}

/// The box of the shape of a frame, in target points.
fn frame_area(frame: &FrameRecord, target: FrameTarget) -> Rect {
    let b = atlas::shape_bbox(&frame.shape);
    Rect::from_points(
        target.to_target(Point::new(b.x0, b.y0)),
        target.to_target(Point::new(b.x1, b.y1)),
    )
}
