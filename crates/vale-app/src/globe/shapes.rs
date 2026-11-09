//! The shapes of the vector layers on the globe, kept between frames. Each
//! layer keeps the directions of its positions until an edit. It keeps its
//! parts on the screen for the last views, and the shapes that it gave to
//! the painter for the last draws.

use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::{egui, egui_wgpu};
use vale_store::{Geometry, Layer, LayerId};

use super::math::{V3, add, angle, length, lonlat_to_dir, scale};
use super::vector;
use super::view::GlobeView;
use crate::lod::{self, Detail};
use crate::mask::{MaskedFeature, MaskedLayer};
use crate::pipeline::Batch;
use crate::ui::globe::Cut;

/// The number of views and of draws that each layer keeps.
const KEEP: usize = 2;

/// The positions of a feature as directions.
pub enum Directions {
    Points(Vec<V3>),
    Lines(Vec<Vec<V3>>),
    /// The rings of each polygon, with no empty ring, and the outline of a
    /// masked polygon: its own outline cut by the mask.
    Polygons(Vec<Vec<Vec<V3>>>, Option<Vec<Vec<V3>>>),
}

/// A cap of the sphere that holds a feature or a part of it: all its
/// positions, its edges, and the inside of its polygons.
#[derive(Clone, Copy, Debug)]
pub struct Cap {
    pub center: V3,
    /// The angle from the center to the edge, less than a quarter turn.
    pub radius: f64,
}

impl Cap {
    /// The smallest cap around the mean of `points`. `None` if that cap is
    /// half of the sphere or more. A cap less than half of the sphere holds
    /// each short arc between two of its points.
    pub fn around(points: impl Iterator<Item = V3> + Clone) -> Option<Cap> {
        let sum = points.clone().fold([0.0; 3], add);
        let norm = length(sum);
        if norm < 1e-9 {
            return None;
        }
        let center = scale(sum, 1.0 / norm);
        let radius = points.map(|p| angle(center, p)).fold(0.0, f64::max);
        (radius < FRAC_PI_2 - 1e-6).then_some(Cap { center, radius })
    }

    /// The cap is all on the far side of the globe of `view` in `rect`, or
    /// all off `area` by more than `margin` points.
    pub fn hidden(
        &self,
        view: &GlobeView,
        rect: egui::Rect,
        area: egui::Rect,
        margin: f32,
    ) -> bool {
        let c = view.rot.mul_vec(self.center);
        if c[2].clamp(-1.0, 1.0).acos() - self.radius > FRAC_PI_2 {
            return true;
        }
        // The disc that holds the cap on the screen.
        let radius = view.radius(rect);
        let reach = (1.0 - self.radius.cos() + self.radius.sin()) * radius + f64::from(margin);
        let pos = view.view_to_screen(rect, c);
        f64::from(area.distance_to_pos(pos)) > reach
    }
}

/// A feature of a vector layer as directions, with its caps. `None`: no
/// cap holds the feature or the part, and it draws in each view.
pub struct Feature {
    pub directions: Directions,
    pub cap: Option<Cap>,
    /// The cap of each line or each polygon.
    pub parts: Vec<Option<Cap>>,
}

impl Feature {
    /// `None`: a feature that the level of detail leaves out.
    fn new(geometry: Option<&Geometry>, part: Option<&MaskedFeature>) -> Feature {
        let dirs = |line: &[[f64; 2]]| vector::line(line);
        let Some(geometry) = geometry else {
            return Feature {
                directions: Directions::Points(Vec::new()),
                cap: Some(Cap {
                    center: [0.0, 0.0, 1.0],
                    radius: 0.0,
                }),
                parts: Vec::new(),
            };
        };
        match geometry {
            Geometry::Points(points) => {
                let dirs: Vec<V3> = points.iter().map(|p| lonlat_to_dir(p[0], p[1])).collect();
                let cap = Cap::around(dirs.iter().copied());
                Feature {
                    directions: Directions::Points(dirs),
                    cap,
                    parts: Vec::new(),
                }
            }
            Geometry::Lines(lines) => {
                let lines: Vec<_> = part
                    .map_or(lines, |p| &p.lines)
                    .iter()
                    .map(|l| dirs(l))
                    .collect();
                let cap = Cap::around(lines.iter().flatten().copied());
                let parts = (lines.iter())
                    .map(|l| Cap::around(l.iter().copied()))
                    .collect();
                Feature {
                    directions: Directions::Lines(lines),
                    cap,
                    parts,
                }
            }
            Geometry::Polygons(polygons) => {
                let polygons: Vec<Vec<Vec<V3>>> = (part.map_or(polygons, |p| &p.polygons).iter())
                    .filter_map(|polygon| {
                        let rings: Vec<_> = (polygon.iter().enumerate())
                            .map(|(i, ring)| vector::ring(ring, i > 0))
                            .collect();
                        if rings.first().is_none_or(|outer| outer.is_empty()) {
                            return None;
                        }
                        Some(rings.into_iter().filter(|r| !r.is_empty()).collect())
                    })
                    .collect();
                let outlines = part.map(|p| p.lines.iter().map(|l| dirs(l)).collect::<Vec<_>>());
                let points = (polygons.iter().flatten().flatten())
                    .chain(outlines.iter().flatten().flatten())
                    .copied();
                // A ring splits the sphere in two. The side that holds the
                // far point of the cap is outside the cap.
                let holds = |cap: &Cap, rings: &[Vec<V3>]| {
                    !vector::contains(rings, scale(cap.center, -1.0))
                };
                let cap = Cap::around(points)
                    .filter(|cap| polygons.iter().all(|rings| holds(cap, rings)));
                let parts = (polygons.iter())
                    .map(|rings| {
                        Cap::around(rings.iter().flatten().copied()).filter(|cap| holds(cap, rings))
                    })
                    .collect();
                Feature {
                    directions: Directions::Polygons(polygons, outlines),
                    cap,
                    parts,
                }
            }
        }
    }
}

/// A feature of a vector layer on the screen.
pub enum OnScreen {
    /// The points on the near side of the globe.
    Points(Vec<egui::Pos2>),
    /// The parts of the lines, as `vector::stroke_shapes` takes them.
    Lines(Vec<Vec<(kurbo::Point, f64)>>),
    /// The parts of the polygons, their triangles, made at the first fill,
    /// and the outline of a masked polygon: its own outline cut by the mask.
    Polygons(
        Vec<vector::Clipped>,
        Vec<Option<(Vec<vector::P2>, Vec<u32>)>>,
        Option<Vec<Vec<(kurbo::Point, f64)>>>,
    ),
}

/// What the parts on the screen depend on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewKey {
    pub view: GlobeView,
    /// The globe fits this rectangle.
    pub rect: egui::Rect,
    /// The part of the screen that the layer draws in. It can be larger
    /// than `rect`.
    pub area: egui::Rect,
    pub pixels_per_point: f32,
    /// How far the paints of the layer reach out from the geometry, in
    /// points. A part that is farther off the screen is not on it.
    pub margin: f32,
}

impl ViewKey {
    /// `cap` is `None` or shows in the view.
    pub fn shows(&self, cap: &Option<Cap>) -> bool {
        cap.is_none_or(|cap| !cap.hidden(&self.view, self.rect, self.area, self.margin))
    }
}

/// What the shapes of a draw depend on, other than the features.
#[derive(PartialEq)]
pub(crate) struct DrawKey {
    pub view: ViewKey,
    pub clip: egui::Rect,
    /// The batches of each visible entry of the layer.
    pub batches: Vec<Vec<Batch>>,
    /// The address of the symbols.
    pub symbols: usize,
    pub format: Option<egui_wgpu::wgpu::TextureFormat>,
    pub cut: bool,
}

/// A shape of a draw on its way to the painter or to the cut.
#[derive(Clone)]
pub enum Out {
    Shape(egui::Shape),
    /// Shapes that go to the painter with an opacity.
    Faded(f32, Vec<egui::Shape>),
    /// The opaque shapes of a translucent stroke, which go to the painter
    /// as one group with the alpha.
    Group(f32, Vec<egui::Shape>),
    Cut(Cut),
}

/// What makes the directions of a layer out of date, other than an edit.
#[derive(Clone, Copy, PartialEq)]
struct Source {
    features: usize,
    address: usize,
    mask: Option<u64>,
    detail: usize,
}

impl Source {
    fn of(layer: &Layer, masked: Option<&MaskedLayer>, detail: Option<&Arc<Detail>>) -> Source {
        Source {
            features: layer.features.len(),
            address: layer.features.as_ptr() as usize,
            mask: masked.map(|m| m.generation),
            detail: lod::key(detail),
        }
    }
}

/// The kept shapes of one layer.
pub struct LayerShapes {
    source: Source,
    pub features: Vec<Feature>,
    /// The parts on the screen for each kept view. A feature that no draw
    /// needed has `None`. The latest view first.
    views: Vec<(ViewKey, Vec<Option<OnScreen>>)>,
    /// The latest draw first.
    draws: Vec<(DrawKey, Vec<Out>)>,
}

impl LayerShapes {
    /// The parts on the screen in `view`, kept or new.
    pub fn view(&mut self, key: ViewKey, work: &mut Work) -> (&[Feature], &mut [Option<OnScreen>]) {
        match self.views.iter().position(|(k, _)| *k == key) {
            Some(i) => {
                let kept = self.views.remove(i);
                self.views.insert(0, kept);
            }
            None => {
                work.views += 1;
                let screens = self.features.iter().map(|_| None).collect();
                self.views.insert(0, (key, screens));
                self.views.truncate(KEEP);
            }
        }
        (&self.features, &mut self.views[0].1)
    }

    /// The shapes of a kept draw with `key`.
    pub(crate) fn drawn(&mut self, key: &DrawKey) -> Option<&[Out]> {
        let i = self.draws.iter().position(|(k, _)| k == key)?;
        let kept = self.draws.remove(i);
        self.draws.insert(0, kept);
        Some(&self.draws[0].1)
    }

    /// Keeps the shapes of a draw.
    pub(crate) fn keep(&mut self, key: DrawKey, out: Vec<Out>) {
        self.draws.insert(0, (key, out));
        self.draws.truncate(KEEP);
    }
}

/// The geometry work for a layer since the start, for the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// The layer made the directions of its features.
    pub directions: usize,
    /// The layer made a new list of parts on the screen for a view.
    pub views: usize,
    /// The layer cut a feature at the edge of the globe and put it on the
    /// screen.
    pub clipped: usize,
    /// The layer made the shapes of a draw.
    pub draws: usize,
}

/// The kept shapes of the vector layers on the globe.
#[derive(Default)]
pub struct ShapeCache {
    layers: HashMap<LayerId, LayerShapes>,
    work: HashMap<LayerId, Work>,
}

impl ShapeCache {
    /// The kept shapes of `layer`, with directions that are up to date. A
    /// layer with no mask has the geometry of `detail`.
    pub fn layer(
        &mut self,
        layer: &Layer,
        masked: Option<&Arc<MaskedLayer>>,
        detail: Option<&Arc<Detail>>,
    ) -> (&mut LayerShapes, &mut Work) {
        let detail = detail.filter(|_| masked.is_none());
        let source = Source::of(layer, masked.map(Arc::as_ref), detail);
        let work = self.work.entry(layer.id).or_default();
        let shapes = self.layers.entry(layer.id).or_insert_with(|| LayerShapes {
            source,
            features: Vec::new(),
            views: Vec::new(),
            draws: Vec::new(),
        });
        if shapes.source != source || shapes.features.len() != layer.features.len() {
            work.directions += 1;
            shapes.source = source;
            shapes.features = (layer.features.iter().enumerate())
                .map(|(i, f)| {
                    let part = masked.and_then(|m| m.features.get(i)?.as_ref());
                    let geometry = lod::geometry(detail.map(Arc::as_ref), i, &f.geometry);
                    Feature::new(geometry, part)
                })
                .collect();
            shapes.views.clear();
            shapes.draws.clear();
        }
        (shapes, work)
    }

    /// Makes the shapes of layer `id` again at the next draw.
    pub fn forget(&mut self, id: LayerId) {
        self.layers.remove(&id);
    }

    pub fn retain(&mut self, keep: impl Fn(LayerId) -> bool) {
        self.layers.retain(|id, _| keep(*id));
    }

    /// The geometry work for layer `id` since the start.
    pub fn work(&self, id: LayerId) -> Work {
        self.work.get(&id).copied().unwrap_or_default()
    }
}

/// `f` of each item, in the order of `items`, on all cores. `cost` is the
/// work of an item. A small total of work stays on this thread.
pub fn par_map<T: Sync, R: Send>(
    items: &[T],
    cost: impl Fn(&T) -> usize,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    const SMALL: usize = 20_000;
    let total: usize = items.iter().map(&cost).sum();
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    if total < SMALL || threads < 2 {
        return items.iter().map(f).collect();
    }
    // The largest items go first, so that no thread waits for one large
    // item at the end.
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost(&items[i])));
    let next = AtomicUsize::new(0);
    let work = || {
        let mut mine = Vec::new();
        while let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
            mine.push((i, f(&items[i])));
        }
        mine
    };
    let done: Vec<Vec<(usize, R)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads).map(|_| s.spawn(work)).collect();
        (handles.into_iter())
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut out: Vec<Option<R>> = items.iter().map(|_| None).collect();
    for (i, r) in done.into_iter().flatten() {
        out[i] = Some(r);
    }
    out.into_iter().flatten().collect()
}
