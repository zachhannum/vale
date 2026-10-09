//! Point label candidates.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::model::{CandidateKind, LabelClass, PointPlacement, PointPosition};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect};

/// One candidate per distinct listed position, in listed order.
pub(crate) fn candidates(
    position: Point,
    symbol_radius: f64,
    class: &LabelClass,
    pp: &PointPlacement,
    shaped: &ShapedText,
) -> Vec<Candidate> {
    let w = shaped.advance;
    let h = shaped.cap_height;
    let d = symbol_radius + pp.offset;
    let q = d * std::f64::consts::FRAC_1_SQRT_2;
    let (px, py) = (position.x, position.y);
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
