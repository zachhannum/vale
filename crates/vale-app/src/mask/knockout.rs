//! The holes that the markers and the point symbols of a point layer make
//! in a markers mask.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use kurbo::{BezPath, Point, Rect};
use vale_render::Symbols;
use vale_render::hatch::offset_ring;
use vale_style::{MarkerShape, Paint};

use crate::pipeline::{MISSING_DOT, marker_outline, symbol_box};

/// The shape that a point draws, for its footprint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// A marker of `radius` with a stroke of `stroke` width outside it, in
    /// points.
    Marker {
        shape: MarkerShape,
        radius: f64,
        stroke: f64,
    },
    /// A point symbol with the box `bounds` in symbol units, `size` points
    /// big, turned `rotation` degrees clockwise.
    Symbol {
        bounds: Rect,
        size: f64,
        rotation: f64,
    },
}

impl Mark {
    /// The mark of a resolved paint of a point layer. `None` for a paint
    /// that draws nothing at a point.
    pub fn of(paint: &Paint, symbols: &Symbols) -> Option<Mark> {
        match *paint {
            Paint::Marker {
                radius,
                shape,
                stroke,
                ..
            } if radius > 0.0 => Some(Mark::Marker {
                shape,
                radius,
                stroke: stroke.map_or(0.0, |s| s.width.max(0.0)),
            }),
            Paint::Symbol {
                ref symbol,
                size,
                rotation,
                ..
            } if size > 0.0 => {
                let missing = Rect::new(-MISSING_DOT, -MISSING_DOT, MISSING_DOT, MISSING_DOT);
                let bounds = symbols.get(symbol).map_or(missing, |s| symbol_box(s));
                Some(Mark::Symbol {
                    bounds,
                    size,
                    rotation,
                })
            }
            _ => None,
        }
    }
}

/// The ring around `mark` at `center`, grown by `gap` points. `unit` is the
/// number of target units per point. A marker reaches to the outer edge of
/// its stroke.
pub fn footprint(mark: Mark, center: Point, unit: f64, gap: f64) -> Vec<Point> {
    let gap = gap.max(0.0) * unit;
    match mark {
        Mark::Marker {
            shape: MarkerShape::Circle,
            radius,
            stroke,
        } => marker_outline(MarkerShape::Circle, center, (radius + stroke) * unit + gap),
        Mark::Marker {
            shape,
            radius,
            stroke,
        } => {
            let ring = marker_outline(shape, center, radius * unit);
            let grow = stroke * unit + gap;
            if grow > 0.0 {
                offset_ring(&ring, grow)
            } else {
                ring
            }
        }
        Mark::Symbol {
            bounds,
            size,
            rotation,
        } => {
            let size = size * unit;
            let b = bounds.inflate(gap / size, gap / size);
            let (sin, cos) = rotation.to_radians().sin_cos();
            [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1)]
                .into_iter()
                .map(|(x, y)| {
                    let (x, y) = (x * size, y * size);
                    Point::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
                })
                .collect()
        }
    }
}

/// The clip of `area` with a hole for each footprint, for the even-odd rule.
/// Footprints that overlap make one hole.
pub fn knockout_path(area: Rect, rings: &[Vec<Point>]) -> BezPath {
    let mut path = BezPath::new();
    let mut ring_path = |ring: &[Point]| {
        for (i, p) in ring.iter().enumerate() {
            if i == 0 {
                path.move_to(*p);
            } else {
                path.line_to(*p);
            }
        }
        path.close_path();
    };
    ring_path(&[
        Point::new(area.x0, area.y0),
        Point::new(area.x1, area.y0),
        Point::new(area.x1, area.y1),
        Point::new(area.x0, area.y1),
    ]);
    // Each ring turns the same way, so that the nonzero rule gives the union.
    let subject: Vec<Vec<[f64; 2]>> = (rings.iter())
        .filter(|r| r.len() >= 3)
        .map(|r| {
            let mut r: Vec<[f64; 2]> = r.iter().map(|p| [p.x, p.y]).collect();
            if signed_area(&r) < 0.0 {
                r.reverse();
            }
            r
        })
        .collect();
    if subject.is_empty() {
        return path;
    }
    let none: Vec<Vec<[f64; 2]>> = Vec::new();
    for ring in subject
        .overlay(&none, OverlayRule::Subject, FillRule::NonZero)
        .into_iter()
        .flatten()
    {
        let ring: Vec<Point> = ring.into_iter().map(|[x, y]| Point::new(x, y)).collect();
        ring_path(&ring);
    }
    path
}

fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let ([x0, y0], [x1, y1]) = (ring[i], ring[(i + 1) % n]);
            x0 * y1 - x1 * y0
        })
        .sum::<f64>()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_footprint_of_a_marker_reaches_its_stroke_and_the_gap() {
        let c = Point::new(10.0, 20.0);
        let circle = Mark::Marker {
            shape: MarkerShape::Circle,
            radius: 4.0,
            stroke: 1.5,
        };
        let ring = footprint(circle, c, 1.0, 2.0);
        assert!(ring.len() >= 32);
        assert!(ring.iter().all(|p| near(p.distance(c), 7.5)));
        // Two target units per point double each length.
        let ring = footprint(circle, c, 2.0, 2.0);
        assert!(ring.iter().all(|p| near(p.distance(c), 15.0)));

        let square = Mark::Marker {
            shape: MarkerShape::Square,
            radius: 4.0,
            stroke: 1.5,
        };
        let ring = footprint(square, c, 1.0, 2.0);
        let bbox = ring
            .iter()
            .fold(Rect::from_points(c, c), |r, p| r.union_pt(*p));
        assert!(near(bbox.width(), 15.0) && near(bbox.height(), 15.0));
        assert!(near(bbox.center().x, c.x) && near(bbox.center().y, c.y));
        let ring = footprint(square, c, 2.0, 2.0);
        let bbox = ring
            .iter()
            .fold(Rect::from_points(c, c), |r, p| r.union_pt(*p));
        assert!(near(bbox.width(), 30.0));
    }

    #[test]
    fn the_footprint_of_a_symbol_is_its_turned_box() {
        let mark = Mark::Symbol {
            bounds: Rect::new(-0.5, -0.25, 0.5, 0.25),
            size: 10.0,
            rotation: 90.0,
        };
        let c = Point::new(100.0, 50.0);
        let ring = footprint(mark, c, 1.0, 1.0);
        assert_eq!(ring.len(), 4);
        let bbox = ring
            .iter()
            .fold(Rect::from_points(ring[0], ring[0]), |r, p| r.union_pt(*p));
        // The box is 12 by 7 points with the gap, and stands upright after a
        // quarter turn.
        assert!(near(bbox.width(), 7.0), "{bbox:?}");
        assert!(near(bbox.height(), 12.0), "{bbox:?}");
        assert!(near(bbox.center().x, c.x) && near(bbox.center().y, c.y));
    }

    #[test]
    fn overlapping_footprints_make_one_hole() {
        let mark = Mark::Marker {
            shape: MarkerShape::Circle,
            radius: 5.0,
            stroke: 0.0,
        };
        let area = Rect::new(0.0, 0.0, 100.0, 100.0);
        let rings = [
            footprint(mark, Point::new(40.0, 50.0), 1.0, 0.0),
            footprint(mark, Point::new(46.0, 50.0), 1.0, 0.0),
            footprint(mark, Point::new(80.0, 50.0), 1.0, 0.0),
        ];
        let path = knockout_path(area, &rings);
        let closes = path
            .elements()
            .iter()
            .filter(|e| matches!(e, kurbo::PathEl::ClosePath))
            .count();
        // The area and two holes.
        assert_eq!(closes, 3);
        // Under the even-odd rule, the middle of the overlap and the
        // middle of each circle are outside the clip.
        let inside = |p: Point| path.winding(p) % 2 != 0;
        assert!(inside(Point::new(10.0, 10.0)));
        assert!(!inside(Point::new(43.0, 50.0)));
        assert!(!inside(Point::new(80.0, 50.0)));
        assert!(inside(Point::new(60.0, 50.0)));
    }
}
