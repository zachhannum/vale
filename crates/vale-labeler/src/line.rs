//! Line label candidates: text that follows a smoothed copy of the line.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::geom::{Measured, clip_polyline_to_rect, smooth, wrap_angle};
use crate::model::{CandidateKind, LabelClass, LinePlacement, LineSide, UnplacedReason};
use crate::text::{ShapedGlyph, ShapedText, band_middle};
use kurbo::{Affine, Point, Rect};
use std::f64::consts::PI;

const EPS: f64 = 1e-9;

/// The hard limit of the turn between neighbor glyphs, as a multiple of `max_glyph_turn`.
const TURN_CAP: f64 = 1.6;

/// The cost of a candidate that turns more than `max_glyph_turn` at some glyph.
/// It is larger than the other costs of a line candidate, so a straighter candidate goes first.
const OVER_TURN_COST: f64 = 20.0;

/// The extra cost of each `max_glyph_turn` of turn above the limit.
const OVER_TURN_WEIGHT: f64 = 10.0;

/// How much closer two neighbor letters may get where they turn more than `max_glyph_turn`,
/// times the font size.
const SQUEEZE: f64 = 0.05;

/// A gap wider than this times the font size holds a word space.
const WORD_SPACE: f64 = 0.2;

/// The smallest gap across a word space on a curve, as a fraction of the gap in a straight run.
const WORD_GAP: f64 = 0.6;

/// How much wider the gap between neighbor glyphs may get on a curve, times the font size.
const SPREAD: f64 = 0.3;

/// Ink boxes shrink by this times the font size before the test for letters that are not
/// neighbors and overlap.
const SHRINK: f64 = 0.1;

/// The step between candidate positions, as a multiple of the font size.
const STEP: f64 = 0.25;

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

    // Positions along the whole slot, from the middle out.
    let step = STEP * size;
    let mut deltas = vec![0.0];
    let mut k = 1.0;
    while k * step <= half + EPS {
        deltas.push(k * step);
        deltas.push(-k * step);
        k += 1.0;
    }
    let limit = lp.max_glyph_turn;
    let turn_cap = TURN_CAP * limit;

    // The baselines, by side and reading direction.
    let mut baselines: Vec<((LineSide, bool), Measured)> = Vec::new();
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

            // The glyphs follow the baseline, a parallel copy of the line, so the letter
            // spacing stays the same on both sides of a bend.
            let d = match side {
                LineSide::Above => stroke_width / 2.0 + lp.offset,
                LineSide::Centered => -cap / 2.0,
                LineSide::Below => -(stroke_width / 2.0 + lp.offset + cap),
            };
            let base = match baselines.iter().position(|(k, _)| *k == (side, forward)) {
                Some(i) => &baselines[i].1,
                None => {
                    baselines.push(((side, forward), parallel(m, if forward { d } else { -d })));
                    &baselines.last().unwrap().1
                }
            };
            let start = along(m, base, s0 + adv / 2.0) - adv / 2.0;
            if start < 0.0 || start + adv > base.length() {
                continue;
            }

            // Baseline points at the start and end of each glyph, in reading direction.
            let mut ends: Vec<(Point, Point)> = Vec::new();
            for (_, g) in shaped.glyphs() {
                let (sa, sb) = if forward {
                    (start + g.x, start + g.x + g.advance)
                } else {
                    (start + adv - g.x, start + adv - g.x - g.advance)
                };
                ends.push((base.point_at(sa), base.point_at(sb)));
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
            let mut over = 0.0;
            let mut too_sharp = false;
            for w in angles.windows(2) {
                let t = wrap_angle(w[1] - w[0]).abs();
                if t > turn_cap {
                    too_sharp = true;
                    break;
                }
                turn_sum += t;
                over = f64::max(over, t - limit);
            }
            if too_sharp {
                rejected_for_turn = true;
                continue;
            }

            let mut transforms = Vec::with_capacity(ends.len());
            let mut boxes = Vec::new();
            // The last glyph with ink, and its angle.
            let mut prev: Option<(Ink, f64)> = None;
            let mut crowded = false;
            // Shrunk ink boxes, to find letters that are not neighbors and overlap.
            let mut inks: Vec<OrientedBox> = Vec::new();
            for ((_, g), ((pa, _), th)) in shaped.glyphs().zip(ends.iter().zip(&angles)) {
                let tf = Affine::translate(pa.to_vec2())
                    * Affine::rotate(*th)
                    * Affine::translate((0.0, g.y));
                if let Some(bb) = g.bbox {
                    boxes.push(OrientedBox::from_local_rect(bb, tf).inflate(class.margin));
                    let ink = (tf, g);
                    if let Some((p, pth)) = prev {
                        let over = wrap_angle(th - pth).abs() > limit;
                        crowded |= squeezed(p, ink, size, over);
                    }
                    prev = Some((ink, *th));
                    let k = (SHRINK * size).min(0.4 * bb.width()).min(0.4 * bb.height());
                    let shrunk = OrientedBox::from_local_rect(
                        Rect::new(bb.x0 + k, bb.y0 + k, bb.x1 - k, bb.y1 - k),
                        tf,
                    );
                    let n = inks.len();
                    crowded |= n >= 2 && inks[..n - 1].iter().any(|o| o.intersects(&shrunk));
                    inks.push(shrunk);
                }
                transforms.push(tf);
            }
            if crowded {
                rejected_for_turn = true;
                continue;
            }
            let mut bounds: Option<Rect> = None;
            for bx in &boxes {
                let r = bx.aabb();
                bounds = Some(bounds.map_or(r, |u| u.union(r)));
            }
            let mut cost = side_index + 4.0 * delta.abs() / half + 6.0 * turn_sum / PI;
            if over > 0.0 {
                cost += OVER_TURN_COST + OVER_TURN_WEIGHT * over / limit.max(EPS);
            }
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

/// A copy of `m` moved by `d` to the left of its direction, y down. It has the same vertices,
/// so `along` can map an arc length of `m` to it.
fn parallel(m: &Measured, d: f64) -> Measured {
    let n = m.pts.len();
    let pts: Vec<Point> = (0..n)
        .map(|i| {
            let t = m.pts[(i + 1).min(n - 1)] - m.pts[i.saturating_sub(1)];
            let t = t / t.hypot().max(EPS);
            m.pts[i] + kurbo::Vec2::new(t.y, -t.x) * d
        })
        .collect();
    let mut cum = Vec::with_capacity(n);
    let mut c = 0.0;
    for i in 0..n {
        if i > 0 {
            c += pts[i - 1].distance(pts[i]);
        }
        cum.push(c);
    }
    Measured { pts, cum }
}

/// The arc length on `base`, a parallel copy of `m`, at the point beside arc length `s` of `m`.
fn along(m: &Measured, base: &Measured, s: f64) -> f64 {
    let n = m.pts.len();
    let i = m.cum.partition_point(|&c| c <= s).clamp(1, n - 1);
    let seg = m.cum[i] - m.cum[i - 1];
    let t = if seg > 0.0 {
        ((s - m.cum[i - 1]) / seg).clamp(0.0, 1.0)
    } else {
        0.0
    };
    base.cum[i - 1] + t * (base.cum[i] - base.cum[i - 1])
}

/// A placed glyph and its transform.
type Ink<'a> = (Affine, &'a ShapedGlyph);

/// True if the ink of two neighbor glyphs comes too close or moves too far apart, in any
/// profile band where both have ink. `over` is true if they turn more than `max_glyph_turn`.
fn squeezed((ta, a): Ink, (tb, b): Ink, size: f64, over: bool) -> bool {
    let dir = |t: Affine| {
        let c = t.as_coeffs();
        kurbo::Vec2::new(c[0], c[1])
    };
    let u = (dir(ta) + dir(tb)).normalize();
    a.profile
        .iter()
        .zip(&b.profile)
        .enumerate()
        .any(|(i, ends)| {
            let (Some((_, right)), Some((left, _))) = ends else {
                return false;
            };
            let y = band_middle(i, size);
            let straight = (b.x + left) - (a.x + right);
            let gap = (tb * Point::new(*left, y) - ta * Point::new(*right, y)).dot(u);
            // The ink never overlaps more than in a straight run.
            let mut min = straight.min(0.0);
            if straight > WORD_SPACE * size {
                min = min.max(WORD_GAP * straight);
            } else if over {
                min = min.max(straight - SQUEEZE * size);
            }
            gap < min || gap > straight + SPREAD * size
        })
}
