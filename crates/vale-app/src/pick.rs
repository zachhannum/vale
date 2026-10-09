//! Hit testing.

use kurbo::Point;

use crate::pipeline::{PageFeature, PageLayer, Selection};
use vale_store::GeometryKind;

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

/// Even-odd test over all rings.
fn inside(rings: &[Vec<Point>], p: Point) -> bool {
    let mut crossings = 0;
    for ring in rings {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    crossings += 1;
                }
            }
        }
    }
    crossings % 2 == 1
}

fn distance(f: &PageFeature, kind: GeometryKind, at: Point) -> f64 {
    match kind {
        GeometryKind::Point => f
            .points
            .iter()
            .map(|p| p.distance(at))
            .fold(f64::INFINITY, f64::min),
        GeometryKind::Line => f
            .lines
            .iter()
            .flat_map(|l| l.windows(2))
            .map(|w| segment_distance(at, w[0], w[1]))
            .fold(f64::INFINITY, f64::min),
        GeometryKind::Polygon => {
            if inside(&f.rings, at) {
                0.0
            } else {
                f64::INFINITY
            }
        }
    }
}

/// The feature under `at`, if any. The top layer wins.
pub fn pick(layers: &[PageLayer<'_>], at: Point) -> Option<Selection> {
    for pl in layers.iter().rev() {
        let style = &pl.entry.style;
        let kind = pl.layer.kind;
        let reach = match kind {
            GeometryKind::Point => style.point_radius.max(3.0) + 3.0,
            GeometryKind::Line => (style.stroke.map_or(0.0, |s| s.width) / 2.0).max(1.0) + 3.0,
            GeometryKind::Polygon => 0.0,
        };
        let mut best: Option<(f64, usize)> = None;
        for f in &pl.features {
            if !f.bbox.inflate(8.0, 8.0).contains(at) {
                continue;
            }
            let d = distance(f, kind, at);
            let hit = if kind == GeometryKind::Polygon {
                d == 0.0
            } else {
                d <= reach
            };
            if hit && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, f.index));
                if kind == GeometryKind::Polygon {
                    break;
                }
            }
        }
        if let Some((_, feature)) = best {
            return Some(Selection {
                layer: pl.layer.id,
                feature,
            });
        }
    }
    None
}
