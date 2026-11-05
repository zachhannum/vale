//! The raster mask of a layer in a map frame, on the page.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use kurbo::{BezPath, Point, Rect};

use crate::atlas::FrameTarget;
use crate::mask::trace::trace;

/// The region of a raster mask in one map frame, in page millimeters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageMask {
    /// Rings that do not cross, from `trace`.
    rings: Vec<Vec<Point>>,
    /// The box of each ring.
    boxes: Vec<Rect>,
}

impl PageMask {
    /// A mask from rings in page millimeters that do not cross.
    pub fn new(rings: Vec<Vec<Point>>) -> Self {
        let boxes = rings.iter().map(|r| bbox(r)).collect();
        PageMask { rings, boxes }
    }

    /// The mask of the samples of `grid` with a value in `lo..hi`. `at`
    /// maps page millimeters to grid pixels: pixel (i, j) covers the target
    /// points from (i, j) to (i + 1, j + 1). NaN is outside.
    pub fn from_grid(
        grid: &[f32],
        width: usize,
        height: usize,
        lo: f32,
        hi: f32,
        at: FrameTarget,
    ) -> Self {
        let rings = trace(grid, width, height, lo, hi)
            .into_iter()
            .map(|r| r.into_iter().map(|p| at.from_target(p)).collect())
            .collect();
        PageMask::new(rings)
    }

    /// The range `lo..hi` of the limits of a raster mask. `None` has no
    /// limit.
    pub fn range(min: Option<f64>, max: Option<f64>) -> (f32, f32) {
        (
            min.map_or(f32::NEG_INFINITY, |v| v as f32),
            max.map_or(f32::INFINITY, |v| v as f32),
        )
    }

    /// The rings in page millimeters.
    pub fn rings(&self) -> &[Vec<Point>] {
        &self.rings
    }

    /// The mask as a closed path in target points, for a nonzero clip.
    pub fn path(&self, target: FrameTarget) -> BezPath {
        let mut path = BezPath::new();
        for ring in &self.rings {
            for (i, p) in ring.iter().enumerate() {
                let q = target.to_target(*p);
                if i == 0 {
                    path.move_to(q);
                } else {
                    path.line_to(q);
                }
            }
            path.close_path();
        }
        path
    }

    /// The part of `rings`, even-odd polygons in target points with the box
    /// `bbox`, inside the mask.
    pub fn clip(&self, rings: &[Vec<Point>], bbox: Rect, target: FrameTarget) -> Vec<Vec<Point>> {
        let near = |b: &Rect| {
            let (p0, p1) = (Point::new(b.x0, b.y0), Point::new(b.x1, b.y1));
            let b = Rect::from_points(target.to_target(p0), target.to_target(p1));
            b.x0 <= bbox.x1 && b.x1 >= bbox.x0 && b.y0 <= bbox.y1 && b.y1 >= bbox.y0
        };
        // A ring away from the box adds nothing inside the box.
        let clip: Vec<Vec<[f64; 2]>> = (self.rings.iter().zip(&self.boxes))
            .filter(|(_, b)| near(b))
            .map(|(r, _)| {
                (r.iter())
                    .map(|p| {
                        let q = target.to_target(*p);
                        [q.x, q.y]
                    })
                    .collect()
            })
            .collect();
        if clip.is_empty() {
            return Vec::new();
        }
        let subject: Vec<Vec<[f64; 2]>> = (rings.iter())
            .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
            .collect();
        // The rings of the mask do not cross, so the even-odd rule gives
        // the nonzero region of the mask.
        subject
            .overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd)
            .into_iter()
            .flatten()
            .map(|r| r.into_iter().map(|[x, y]| Point::new(x, y)).collect())
            .collect()
    }
}

fn bbox(ring: &[Point]) -> Rect {
    let mut it = ring.iter();
    let Some(first) = it.next() else {
        return Rect::ZERO;
    };
    it.fold(Rect::from_points(*first, *first), |r, p| r.union_pt(*p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    }

    fn area(rings: &[Vec<Point>]) -> f64 {
        let mut path = BezPath::new();
        for r in rings {
            path.move_to(r[0]);
            r[1..].iter().for_each(|p| path.line_to(*p));
            path.close_path();
        }
        kurbo::Shape::area(&path).abs()
    }

    #[test]
    fn a_grid_mask_maps_to_the_page() {
        // 4 × 2 pixels, the left half inside, at 2 pixels per millimeter
        // with the page at (10, 0).
        let grid = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];
        let at = FrameTarget {
            origin: Point::new(10.0, 0.0),
            pts_per_mm: 2.0,
        };
        let mask = PageMask::from_grid(&grid, 4, 2, 0.0, f32::INFINITY, at);
        assert_eq!(mask.rings().len(), 1);
        let b = bbox(&mask.rings()[0]);
        assert!(
            (b.x0 + 5.0).abs() < 1e-9 && (b.x1 + 4.0).abs() < 1e-9,
            "{b:?}"
        );
        assert!(b.y0.abs() < 1e-9 && (b.y1 - 1.0).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn a_clip_keeps_the_part_inside() {
        let mask = PageMask::new(vec![square(0.0, 0.0, 10.0, 10.0)]);
        let target = FrameTarget {
            origin: Point::ZERO,
            pts_per_mm: 2.0,
        };
        let rings = vec![square(10.0, 0.0, 30.0, 20.0)];
        let bbox = Rect::new(10.0, 0.0, 30.0, 20.0);
        let part = mask.clip(&rings, bbox, target);
        assert!((area(&part) - 200.0).abs() < 1e-3, "{}", area(&part));
        let away = vec![square(50.0, 50.0, 60.0, 60.0)];
        assert!(
            mask.clip(&away, Rect::new(50.0, 50.0, 60.0, 60.0), target)
                .is_empty()
        );
    }
}
