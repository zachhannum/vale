//! The map pipeline: query, clip, project, style, label, emit.

use std::collections::{BTreeMap, HashMap};

use anyhow::anyhow;
use kurbo::{BezPath, Circle, Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{DisplayList, FillRule, Item};
use vale_sphere::{LonLat, Projection, ProjectionSpec, graticule, nice_step};
use vale_store::{Geometry, GeometryKind, Layer, LayerId, Value};
use vale_style::expr::Context;
use vale_style::{Extent, Paint, scale_denominator};

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

#[derive(Default)]
pub struct Pipeline {
    cache: Option<Cache>,
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

fn page_bbox(vt: &ViewTransform, b: Rect) -> Rect {
    let p = vt.to_page(Point::new(b.x0, b.y0));
    let q = vt.to_page(Point::new(b.x1, b.y1));
    Rect::new(p.x.min(q.x), p.y.min(q.y), p.x.max(q.x), p.y.max(q.y))
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
        bbox: page_bbox(vt, f.bbox),
    }
}

fn build_page_layers<'a>(
    doc: &'a Document,
    cache: &Cache,
    vt: &ViewTransform,
    scale: f64,
) -> Vec<PageLayer<'a>> {
    let page = vt.page_rect().inflate(8.0, 8.0);
    let mut out = Vec::new();
    for entry in doc.frame.entries.iter().filter(|e| doc.visible(e.layer)) {
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
            .filter(|(_, f)| meets(page_bbox(vt, f.bbox), page))
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

/// Km per point at the middle of the view.
fn km_per_point(cache: &Cache, vt: &ViewTransform) -> f64 {
    let middle = Point::new(vt.width / 2.0, vt.height / 2.0);
    let p = cache
        .projection
        .inverse(vt.from_page(middle))
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
                let paint = symbolizer.resolve(&ctx);
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

impl Pipeline {
    pub fn new() -> Self {
        Pipeline { cache: None }
    }

    /// Projects the features of a layer again at the next compose.
    pub fn forget(&mut self, id: LayerId) {
        if let Some(cache) = &mut self.cache {
            cache.layers.remove(&id);
        }
    }

    /// Brings the projection and the projected geometry up to date with the document.
    fn ensure(&mut self, doc: &Document) -> anyhow::Result<&mut Cache> {
        let spec = doc.frame.projection.normalized();
        let radius_m = doc.project.world.radius_km * 1000.0;
        let stale = self
            .cache
            .as_ref()
            .is_none_or(|c| c.spec != spec || c.radius_m != radius_m);
        if stale {
            let projection =
                Projection::new(spec, radius_m).map_err(|e| anyhow!("projection: {e}"))?;
            let outline = projection.outline();
            let bounds = projection.bounds();
            self.cache = Some(Cache {
                spec,
                radius_m,
                projection,
                outline,
                bounds,
                layers: HashMap::new(),
                graticules: HashMap::new(),
            });
        }
        let cache = self.cache.as_mut().expect("cache was just made");
        cache
            .layers
            .retain(|id, _| doc.project.layers().iter().any(|l| l.id == *id));
        for entry in doc.frame.entries.iter().filter(|e| doc.visible(e.layer)) {
            if cache.layers.contains_key(&entry.layer) {
                continue;
            }
            if let Some(layer) = doc.layer(entry.layer) {
                let feats = layer
                    .features
                    .iter()
                    .map(|f| project_feature(&cache.projection, &f.geometry))
                    .collect();
                cache.layers.insert(entry.layer, feats);
            }
        }
        Ok(cache)
    }

    pub fn resolve_view(&mut self, doc: &Document, size: (f64, f64)) -> anyhow::Result<View> {
        if let Some(v) = doc.frame.view {
            return Ok(v);
        }
        let cache = self.ensure(doc)?;
        Ok(View::fit(cache.bounds, size.0, size.1, 24.0))
    }

    pub fn fit_scale(&mut self, doc: &Document, size: (f64, f64)) -> anyhow::Result<f64> {
        let cache = self.ensure(doc)?;
        Ok(View::fit(cache.bounds, size.0, size.1, 24.0).scale)
    }

    /// The projected position of a place, in meters. `None` when it is not visible.
    pub fn project_lonlat(&mut self, doc: &Document, p: LonLat) -> anyhow::Result<Option<Point>> {
        Ok(self.ensure(doc)?.projection.forward(p))
    }

    pub fn lonlat_to_page(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        p: LonLat,
    ) -> anyhow::Result<Option<Point>> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform {
            view,
            width: size.0,
            height: size.1,
        };
        Ok(self
            .ensure(doc)?
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
        let vt = ViewTransform {
            view,
            width: size.0,
            height: size.1,
        };
        Ok(self.ensure(doc)?.projection.inverse(vt.from_page(q)))
    }

    pub fn pick(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        at: Point,
    ) -> anyhow::Result<Option<Selection>> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform {
            view,
            width: size.0,
            height: size.1,
        };
        let cache = self.ensure(doc)?;
        let scale = scale_denominator(km_per_point(cache, &vt) * 1000.0);
        let layers = build_page_layers(doc, cache, &vt, scale);
        Ok(pick::pick(&layers, at))
    }

    pub fn compose(
        &mut self,
        doc: &Document,
        size: (f64, f64),
        fonts: &mut Fonts,
        quality: Quality,
        selection: Option<Selection>,
    ) -> anyhow::Result<Composed> {
        let view = self.resolve_view(doc, size)?;
        let vt = ViewTransform {
            view,
            width: size.0,
            height: size.1,
        };
        let page = vt.page_rect();
        let style = &doc.frame.style;
        let radius_m = doc.project.world.radius_km * 1000.0;

        // The graticule step depends on the view, so make its lines before borrowing the cache.
        let degrees_per_point = 1.0 / (view.scale * radius_m * std::f64::consts::PI / 180.0);
        let graticule_step = doc
            .frame
            .graticule
            .then(|| nice_step(80.0 * degrees_per_point));
        let cache = self.ensure(doc)?;
        if let Some(step) = graticule_step
            && !cache.graticules.contains_key(&step.to_bits())
        {
            let lines: Vec<Vec<Point>> = graticule(step)
                .iter()
                .flat_map(|l| cache.projection.project_line(l))
                .collect();
            cache.graticules.insert(step.to_bits(), lines);
        }
        let cache = &*cache;

        let mut list = DisplayList::new(size.0, size.1);

        // 1 and 2: background and water.
        list.push(Item::Fill {
            path: page.to_path(0.1),
            color: style.background,
            rule: FillRule::NonZero,
        });
        let outline: Vec<Point> = map_pts(&vt, &cache.outline);
        let outline_path = polylines_path([&outline], true);
        list.push(Item::Fill {
            path: outline_path.clone(),
            color: style.water,
            rule: FillRule::NonZero,
        });

        // 3: layers.
        let km_per_point = km_per_point(cache, &vt);
        let scale = scale_denominator(km_per_point * 1000.0);
        let page_layers = build_page_layers(doc, cache, &vt, scale);
        for pl in &page_layers {
            for batch in batches(pl) {
                draw_batch(&mut list, pl, &batch);
            }
        }

        // 4: graticule.
        if let Some(step) = graticule_step
            && let Some(lines) = cache.graticules.get(&step.to_bits())
        {
            let mut path = BezPath::new();
            for l in lines {
                let pts = map_pts(&vt, l);
                let mut b = None;
                for p in &pts {
                    extend_bbox(&mut b, *p);
                }
                if b.is_some_and(|b| meets(b, page)) {
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

        // 7 to 9: scale bar, labels.
        let (bar_items, bar_rect) =
            furniture::scale_bar(page, km_per_point, doc.project.world.units, fonts);
        let mut label_summary = LabelSummary::default();
        if doc.frame.labels {
            let (items, summary) = labels::place(&page_layers, page, fonts, quality, &[bar_rect]);
            list.items.extend(items);
            label_summary = summary;
        }
        list.items.extend(bar_items);

        Ok(Composed {
            list,
            view,
            labels: label_summary,
            km_per_point,
            graticule_step,
        })
    }
}
