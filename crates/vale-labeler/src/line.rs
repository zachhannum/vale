//! Line label candidates: text that follows a smoothed copy of the line.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::geom::{Measured, clip_polyline_to_rect, smooth, wrap_angle};
use crate::model::{CandidateKind, LabelClass, LinePlacement, LineSide, UnplacedReason};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect};
use std::f64::consts::PI;

const EPS: f64 = 1e-9;

/// One entry per repeat slot.
pub(crate) fn slots(
    path: &[Point],
    stroke_width: f64,
    class: &LabelClass,
    lp: &LinePlacement,
    shaped: &ShapedText,
    bounds: Rect,
) -> Vec<Result<Vec<Candidate>, UnplacedReason>> {
    let size = class.style.size as f64;
    let need = shaped.advance + size;

    // Clip, smooth, and keep the pieces that are long enough.
    let pieces: Vec<Measured> = clip_polyline_to_rect(path, bounds)
        .iter()
        .map(|p| Measured::new(smooth(p, 0.25 * size, lp.smoothing * size)))
        .filter(|m| m.length() >= need && m.pts.len() >= 2)
        .collect();
    if pieces.is_empty() {
        return vec![Err(UnplacedReason::LineTooShort)];
    }

    // Slots as (piece index, start, end).
    let mut slot_ranges: Vec<(usize, f64, f64)> = Vec::new();
    match lp.repeat_distance {
        Some(r) if r > 0.0 && r.is_finite() => {
            for (pi, m) in pieces.iter().enumerate() {
                let len = m.length();
                let max_n = (len / need).floor().max(1.0);
                let n = (len / r).floor().clamp(1.0, max_n) as usize;
                let w = len / n as f64;
                for k in 0..n {
                    slot_ranges.push((pi, k as f64 * w, (k + 1) as f64 * w));
                }
            }
        }
        _ => {
            let mut best = 0;
            for (i, m) in pieces.iter().enumerate() {
                if m.length() > pieces[best].length() {
                    best = i;
                }
            }
            slot_ranges.push((best, 0.0, pieces[best].length()));
        }
    }

    slot_ranges
        .into_iter()
        .map(|(pi, a, b)| slot_candidates(&pieces[pi], a, b, stroke_width, class, lp, shaped))
        .collect()
}

fn slot_candidates(
    m: &Measured,
    a: f64,
    b: f64,
    stroke_width: f64,
    class: &LabelClass,
    lp: &LinePlacement,
    shaped: &ShapedText,
) -> Result<Vec<Candidate>, UnplacedReason> {
    let size = class.style.size as f64;
    let adv = shaped.advance;
    let cap = shaped.cap_height;
    let mid = 0.5 * (a + b);
    let half = 0.5 * (b - a);

    let mut deltas = vec![0.0];
    for k in 1..=6 {
        deltas.push(k as f64 * size);
        deltas.push(-(k as f64) * size);
    }

    let mut out: Vec<Candidate> = Vec::new();
    let mut rejected_for_turn = false;
    for delta in deltas {
        let s0 = mid + delta - adv / 2.0;
        if s0 < a + 0.5 * size - EPS || s0 + adv > b - 0.5 * size + EPS {
            continue;
        }
        let p0 = m.point_at(s0);
        let p1 = m.point_at(s0 + adv);
        let forward = p1.x >= p0.x;
        let mut seen: Vec<LineSide> = Vec::new();
        for &side in &lp.sides {
            if seen.contains(&side) {
                continue;
            }
            let side_index = seen.len() as f64;
            seen.push(side);

            // Line points at the start and end of each glyph, in reading direction.
            let mut ends: Vec<(Point, Point)> = Vec::new();
            for (_, g) in shaped.glyphs() {
                let (sa, sb) = if forward {
                    (s0 + g.x, s0 + g.x + g.advance)
                } else {
                    (s0 + adv - g.x, s0 + adv - g.x - g.advance)
                };
                ends.push((m.point_at(sa), m.point_at(sb)));
            }
            let fallback = if forward {
                (p1.y - p0.y).atan2(p1.x - p0.x)
            } else {
                (p0.y - p1.y).atan2(p0.x - p1.x)
            };
            let mut angles: Vec<f64> = Vec::with_capacity(ends.len());
            for (pa, pb) in &ends {
                let th = if pa == pb {
                    angles.last().copied().unwrap_or(fallback)
                } else {
                    (pb.y - pa.y).atan2(pb.x - pa.x)
                };
                angles.push(th);
            }
            let mut turn_sum = 0.0;
            let mut too_sharp = false;
            for w in angles.windows(2) {
                let t = wrap_angle(w[1] - w[0]).abs();
                if t > lp.max_glyph_turn {
                    too_sharp = true;
                    break;
                }
                turn_sum += t;
            }
            if too_sharp {
                rejected_for_turn = true;
                continue;
            }

            let d = match side {
                LineSide::Above => stroke_width / 2.0 + lp.offset,
                LineSide::Centered => -cap / 2.0,
                LineSide::Below => -(stroke_width / 2.0 + lp.offset + cap),
            };
            let mut transforms = Vec::with_capacity(ends.len());
            let mut boxes = Vec::new();
            for ((_, g), ((pa, _), th)) in shaped.glyphs().zip(ends.iter().zip(&angles)) {
                let (sin, cos) = th.sin_cos();
                let anchor = Point::new(pa.x + sin * d, pa.y - cos * d);
                let tf = Affine::translate(anchor.to_vec2())
                    * Affine::rotate(*th)
                    * Affine::translate((0.0, g.y));
                if let Some(bb) = g.bbox {
                    boxes.push(OrientedBox::from_local_rect(bb, tf).inflate(class.margin));
                }
                transforms.push(tf);
            }
            let mut bounds: Option<Rect> = None;
            for bx in &boxes {
                let r = bx.aabb();
                bounds = Some(bounds.map_or(r, |u| u.union(r)));
            }
            let cost = side_index + 4.0 * delta.abs() / half + 6.0 * turn_sum / PI;
            out.push(Candidate {
                kind: CandidateKind::Line { side, start: s0 },
                cost,
                transforms,
                boxes,
                bounds: bounds.unwrap_or(Rect::ZERO),
            });
        }
    }
    if out.is_empty() && rejected_for_turn {
        return Err(UnplacedReason::TooCurved);
    }
    Ok(out)
}
