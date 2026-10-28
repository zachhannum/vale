//! The scatter fill: symbols inside a polygon, fixed on the ground.
//!
//! The positions depend on the feature, the paint, the sphere radius, and the
//! nominal scale `1:D` of the view. `D` must not change with a pan. A change
//! of `D` changes the level of the lattice only at a power of two, so a zoom
//! adds or removes symbols, and a symbol never moves.

use std::collections::HashMap;
use std::sync::Arc;

use kurbo::{Point, Rect};
use vale_sphere::{
    LonLat, ScatterParams, ScatterPlane, ScatterPoint, min_distance, scatter, seed_from,
};
use vale_store::{Feature, Geometry, LayerId};
use vale_style::{METERS_PER_POINT, Paint};

/// The most symbols of one feature, and of one layer in one view.
pub const MAX_POINTS: usize = 30_000;

/// The most scatter points that a cache keeps over all its entries.
const MAX_CACHED_POINTS: usize = 2_000_000;

/// The inputs of a scatter fill that set the positions of its symbols.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScatterFill {
    /// The longer side of a symbol, in points.
    pub size: f64,
    /// The minimum distance between two symbol centers, in points.
    pub spacing: f64,
    pub size_jitter: f64,
    pub rotation_jitter: f64,
}

impl ScatterFill {
    /// The fill of a `Paint::Scatter`.
    pub fn of(paint: &Paint) -> Option<ScatterFill> {
        match *paint {
            Paint::Scatter {
                size,
                spacing,
                size_jitter,
                rotation_jitter,
                ..
            } => Some(ScatterFill {
                size,
                spacing,
                size_jitter,
                rotation_jitter,
            }),
            _ => None,
        }
    }
}

/// The ground distance in meters of `points` points at the scale 1:`scale`.
pub fn ground(points: f64, scale: f64) -> f64 {
    points * METERS_PER_POINT * scale
}

/// All rings of the polygons of a geometry. Other geometries have none.
pub fn rings(geometry: &Geometry) -> Vec<Vec<LonLat>> {
    match geometry {
        Geometry::Polygons(polys) => polys.iter().flatten().cloned().collect(),
        _ => Vec::new(),
    }
}

/// The plane and the parameters of the scatter of a feature at the scale
/// 1:`scale`, on a sphere of `radius` meters. `None` when the fill has no
/// size or no spacing.
pub fn setup(
    feature: &Feature,
    rings: &[Vec<LonLat>],
    fill: &ScatterFill,
    scale: f64,
    radius: f64,
) -> Option<(ScatterPlane, ScatterParams)> {
    let spacing = ground(fill.spacing, scale);
    if !(fill.size > 0.0 && spacing > 0.0 && spacing.is_finite() && radius > 0.0) {
        return None;
    }
    let plane = ScatterPlane::for_rings(rings, radius);
    // Half the symbol size in proportion to the real minimum distance. This
    // is at least half the ground size of a symbol, and it changes only with
    // the level.
    let ratio = (fill.size / fill.spacing).min(1.0);
    let inset = ratio * min_distance(&plane, spacing) / 2.0;
    let params = ScatterParams {
        seed: seed_from(feature.uuid().as_bytes()),
        spacing,
        inset,
        size_jitter: fill.size_jitter,
        rotation_jitter: fill.rotation_jitter,
        max_points: MAX_POINTS,
    };
    Some((plane, params))
}

/// The window of a scatter in the plane. `None` is the whole polygon, when
/// the whole polygon is not too big for the spacing. Else it is the box of
/// the `view` points with a margin, snapped to a grid so that a small pan
/// keeps it. With no `view` points it is the whole polygon, and the cost
/// guard of the scatter makes it sparser.
pub fn window(
    plane: &ScatterPlane,
    rings: &[Vec<LonLat>],
    params: &ScatterParams,
    view: &[LonLat],
) -> Option<Rect> {
    let bounds = bbox(rings.iter().flatten().filter_map(|p| plane.forward(*p)))?;
    let cell = 2.0 * min_distance(plane, params.spacing);
    let cells = |r: Rect| (r.width() / cell + 2.0) * (r.height() / cell + 2.0);
    if cells(bounds) <= 4.0 * params.max_points as f64 {
        return None;
    }
    let seen = bbox(view.iter().filter_map(|p| plane.forward(*p)))?;
    let side = seen.width().max(seen.height());
    if side.is_nan() || side <= 0.0 {
        return None;
    }
    let grid = 2f64.powf((side / 4.0).log2().floor());
    let r = seen.inflate(side / 4.0, side / 4.0);
    Some(Rect::new(
        (r.x0 / grid).floor() * grid,
        (r.y0 / grid).floor() * grid,
        (r.x1 / grid).ceil() * grid,
        (r.y1 / grid).ceil() * grid,
    ))
}

fn bbox(points: impl Iterator<Item = Point>) -> Option<Rect> {
    points
        .map(|p| Rect::from_points(p, p))
        .reduce(|a, b| a.union(b))
}

/// The scatter of a polygon feature at the scale 1:`scale`, on a sphere of
/// `radius` meters. `view` holds points on the sphere that the view shows,
/// for the window of a big polygon. The symbols that a view shows do not
/// depend on `view`, unless the view itself holds too many symbols.
pub fn scatter_feature(
    feature: &Feature,
    fill: &ScatterFill,
    scale: f64,
    radius: f64,
    view: &[LonLat],
) -> Vec<ScatterPoint> {
    let rings = rings(&feature.geometry);
    let Some((plane, params)) = setup(feature, &rings, fill, scale, radius) else {
        return Vec::new();
    };
    let window = window(&plane, &rings, &params, view);
    scatter(&plane, &rings, &params, window)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    layer: LayerId,
    feature: usize,
    uuid: vale_store::Uuid,
    radius: u64,
    level: u64,
    inset: u64,
    jitter: [u64; 2],
    window: Option<[u64; 4]>,
}

/// The scatters of the features of the layers, by the inputs that set them.
#[derive(Default)]
pub struct ScatterCache {
    entries: HashMap<Key, Arc<Vec<ScatterPoint>>>,
    points: usize,
}

impl ScatterCache {
    /// The scatter of feature `index` of a layer, as `scatter_feature` gives it.
    #[allow(clippy::too_many_arguments)]
    pub fn get(
        &mut self,
        layer: LayerId,
        index: usize,
        feature: &Feature,
        fill: &ScatterFill,
        scale: f64,
        radius: f64,
        view: &[LonLat],
    ) -> Arc<Vec<ScatterPoint>> {
        let rings = rings(&feature.geometry);
        let Some((plane, params)) = setup(feature, &rings, fill, scale, radius) else {
            return Arc::default();
        };
        let window = window(&plane, &rings, &params, view);
        let key = Key {
            layer,
            feature: index,
            uuid: feature.uuid(),
            radius: radius.to_bits(),
            level: min_distance(&plane, params.spacing).to_bits(),
            inset: params.inset.to_bits(),
            jitter: [
                params.size_jitter.to_bits(),
                params.rotation_jitter.to_bits(),
            ],
            window: window.map(|w| [w.x0, w.y0, w.x1, w.y1].map(f64::to_bits)),
        };
        if let Some(points) = self.entries.get(&key) {
            return points.clone();
        }
        let points = Arc::new(scatter(&plane, &rings, &params, window));
        if self.points + points.len() > MAX_CACHED_POINTS {
            self.clear();
        }
        self.points += points.len();
        self.entries.insert(key, points.clone());
        points
    }

    /// Drops the scatters of a layer.
    pub fn forget(&mut self, layer: LayerId) {
        self.retain(|id| id != layer);
    }

    /// Keeps the scatters of the layers that `keep` gives.
    pub fn retain(&mut self, mut keep: impl FnMut(LayerId) -> bool) {
        self.entries.retain(|k, _| keep(k.layer));
        self.points = self.entries.values().map(|p| p.len()).sum();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.points = 0;
    }

    /// The number of scatters that the cache keeps.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
