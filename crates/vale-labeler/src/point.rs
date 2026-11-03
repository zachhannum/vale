//! Point label candidates.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::graticule::{Parallels, upright};
use crate::line::{along, parallel};
use crate::model::{CandidateKind, LabelClass, PointPlacement, PointPosition};
use crate::polygon::{follow, glyph_boxes};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect};

/// One candidate per distinct listed position, in listed order. If the class follows the
/// graticule, "right" is the reading direction along the parallel through the point, and "top"
/// is the side of the parallel above the text.
pub(crate) fn candidates(
    position: Point,
    symbol_radius: f64,
    class: &LabelClass,
    pp: &PointPlacement,
    shaped: &ShapedText,
    parallels: &Parallels,
) -> Vec<Candidate> {
    let w = shaped.advance;
    let h = shaped.cap_height;
    let d = symbol_radius + pp.offset;
    let q = d * std::f64::consts::FRAC_1_SQRT_2;
    let (px, py) = if pp.graticule {
        (0.0, 0.0)
    } else {
        (position.x, position.y)
    };
    let through = pp.graticule.then(|| {
        let t = parallels.through(position);
        let (curve, map) = upright(t.curve, t.at - w / 2.0, t.at + w / 2.0, pp.no_flip);
        (curve, map(t.at))
    });
    let mut seen: Vec<PointPosition> = Vec::new();
    let mut out = Vec::new();
    for &pos in &pp.positions {
        if seen.contains(&pos) {
            continue;
        }
        let cost = seen.len() as f64;
        seen.push(pos);
        let (left, top) = match pos {
            PointPosition::TopRight => (px + q, py - q - h),
            PointPosition::TopLeft => (px - q - w, py - q - h),
            PointPosition::BottomRight => (px + q, py + q),
            PointPosition::BottomLeft => (px - q - w, py + q),
            PointPosition::Right => (px + d, py - h / 2.0),
            PointPosition::Left => (px - d - w, py - h / 2.0),
            PointPosition::Top => (px - w / 2.0, py - d - h),
            PointPosition::Bottom => (px - w / 2.0, py + d),
        };
        let origin = Point::new(left, top + h);
        if let Some((curve, at)) = &through {
            // `origin` is the start of the baseline relative to the point, along and below
            // the curve.
            let base = parallel(curve, -origin.y);
            let start = along(curve, &base, at + origin.x);
            if start < 0.0 || start + w > base.length() {
                continue;
            }
            let transforms = follow(&base, start, shaped, 0.0).transforms;
            let (boxes, bounds) = glyph_boxes(shaped, &transforms, class.margin);
            out.push(Candidate {
                kind: CandidateKind::Point(pos),
                cost,
                transforms,
                bounds,
                boxes,
            });
            continue;
        }
        let transforms: Vec<Affine> = shaped
            .glyphs()
            .map(|(_, g)| Affine::translate((origin.x + g.x, origin.y + g.y)))
            .collect();
        let ink = Rect::new(
            shaped.ink_bounds.x0 + origin.x,
            shaped.ink_bounds.y0 + origin.y,
            shaped.ink_bounds.x1 + origin.x,
            shaped.ink_bounds.y1 + origin.y,
        );
        let b = OrientedBox::from_rect(ink).inflate(class.margin);
        out.push(Candidate {
            kind: CandidateKind::Point(pos),
            cost,
            transforms,
            bounds: b.aabb(),
            boxes: vec![b],
        });
    }
    out
}
