//! Line label candidates: text that follows a smoothed copy of the line.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::geom::{Measured, clip_polyline_to_rect, smooth, wrap_angle};
use crate::model::{
    CandidateKind, LabelClass, LineAnchor, LineOrientation, LinePlacement, LineSide, UnplacedReason,
};
use crate::polygon::glyph_boxes;
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
    let size = class.style.size;
    let clipped = clip_polyline_to_rect(path, bounds);
    let visible: f64 = clipped
        .iter()
        .map(|p| p.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>())
        .sum();
    if visible < lp.min_length {
        return vec![Err(UnplacedReason::BelowMinimumLength)];
    }
    let pieces: Vec<Measured> = clipped
        .iter()
        .map(|p| Measured::new(smooth(p, 0.25 * size, lp.smoothing * size)))
        .filter(|m| m.pts.len() >= 2)
        .collect();
    let overrun = lp.overrun.max(0.0);
    let span = span(lp, shaped);
    let need = span + 2.0 * end_inset(overrun, size);

    // Slots as (piece index, start, end).
    let mut slot_ranges: Vec<(usize, f64, f64)> = Vec::new();
    match lp.anchor {
        LineAnchor::Best => {
            let fits = |m: &Measured| m.length() >= need;
            match lp.repeat_distance {
                Some(r) if r > 0.0 && r.is_finite() => {
                    for (pi, m) in pieces.iter().enumerate().filter(|(_, m)| fits(m)) {
                        let len = m.length();
                        let max_n = (len / need.max(size)).floor().max(1.0);
                        let n = (len / r).floor().clamp(1.0, max_n) as usize;
                        let w = len / n as f64;
                        for k in 0..n {
                            slot_ranges.push((pi, k as f64 * w, (k + 1) as f64 * w));
                        }
                    }
                }
                _ => {
                    let best = (0..pieces.len())
                        .filter(|&i| fits(&pieces[i]))
                        .max_by(|&a, &b| pieces[a].length().total_cmp(&pieces[b].length()));
                    if let Some(best) = best {
                        slot_ranges.push((best, 0.0, pieces[best].length()));
                    }
                }
            }
        }
        LineAnchor::FromStart | LineAnchor::BeforeStart => {
            if let Some(m) = pieces.first() {
                slot_ranges.push((0, 0.0, m.length()));
            }
        }
        LineAnchor::FromEnd | LineAnchor::AfterEnd => {
            if let Some(m) = pieces.last() {
                slot_ranges.push((pieces.len() - 1, 0.0, m.length()));
            }
        }
    }
    if slot_ranges.is_empty() {
        return vec![Err(UnplacedReason::LineTooShort)];
    }

    slot_ranges
        .into_iter()
        .map(|(pi, a, b)| {
            let slot = Slot {
                m: &pieces[pi],
                a,
                b,
                stroke_width,
            };
            slot_candidates(&slot, class, lp, shaped)
        })
        .collect()
}

/// The inset of a label from the ends of its line. A negative inset lets the label go past
/// the ends.
fn end_inset(overrun: f64, size: f64) -> f64 {
    if overrun > 0.0 { -overrun } else { 0.5 * size }
}

/// The length of the line under the label, with no spread.
fn span(lp: &LinePlacement, shaped: &ShapedText) -> f64 {
    match lp.orientation {
        LineOrientation::Perpendicular => shaped.cap_height,
        _ => shaped.advance,
    }
}

/// A part of a smoothed line that holds one label.
struct Slot<'a> {
    m: &'a Measured,
    a: f64,
    b: f64,
    stroke_width: f64,
}

/// `m` with straight extensions of length `e` at both ends.
fn extend(m: &Measured, e: f64) -> Measured {
    let n = m.pts.len();
    if e <= 0.0 || n < 2 {
        return Measured::new(m.pts.clone());
    }
    let unit = |v: kurbo::Vec2| v / v.hypot().max(EPS);
    let head = m.pts[0] - unit(m.pts[1] - m.pts[0]) * e;
    let tail = m.pts[n - 1] + unit(m.pts[n - 1] - m.pts[n - 2]) * e;
    let mut pts = Vec::with_capacity(n + 2);
    pts.push(head);
    pts.extend_from_slice(&m.pts);
    pts.push(tail);
    Measured::new(pts)
}

fn flip(side: LineSide) -> LineSide {
    match side {
        LineSide::Above => LineSide::Below,
        LineSide::Below => LineSide::Above,
        LineSide::Centered => LineSide::Centered,
    }
}

fn slot_candidates(
    slot: &Slot,
    class: &LabelClass,
    lp: &LinePlacement,
    shaped: &ShapedText,
) -> Result<Vec<Candidate>, UnplacedReason> {
    let size = class.style.size;
    let (a, b) = (slot.a, slot.b);
    let len = slot.m.length();
    let overrun = lp.overrun.max(0.0);
    let inset = |at_end: bool| {
        if at_end {
            end_inset(overrun, size)
        } else {
            0.5 * size
        }
    };
    let lo = a + inset(a <= EPS);
    let hi = b - inset(b >= len - EPS);

    // Spread letters fill the slot, up to the limit of the class.
    let spreads = matches!(
        lp.orientation,
        LineOrientation::Curved | LineOrientation::Straight
    ) && matches!(
        lp.anchor,
        LineAnchor::Best | LineAnchor::FromStart | LineAnchor::FromEnd
    );
    let gaps = shaped.glyph_count().saturating_sub(1);
    let extra = if spreads && gaps > 0 {
        let room = ((hi - lo - shaped.advance) / gaps as f64).max(0.0);
        if lp.spread_fill {
            room
        } else {
            room.min(lp.max_spread.max(0.0) * size)
        }
    } else {
        0.0
    };
    let spread_text;
    let shaped = if extra > 0.0 {
        spread_text = shaped.spread(extra);
        &spread_text
    } else {
        shaped
    };
    let span = span(lp, shaped);

    // Label starts along the slot, each with the cost of its distance from the best start.
    let step = STEP * size;
    let around = |target: f64, tolerance: f64| -> Vec<(f64, f64)> {
        let mut out = vec![(target, 0.0)];
        let mut k = 1.0;
        while k * step <= tolerance + EPS {
            let c = 4.0 * k * step / tolerance.max(EPS);
            out.push((target + k * step, c));
            out.push((target - k * step, c));
            k += 1.0;
        }
        out
    };
    let unit = if lp.anchor_percent { len / 100.0 } else { 1.0 };
    let distance = lp.anchor_distance.max(0.0) * unit;
    let tolerance = lp.anchor_tolerance.max(0.0) * unit;
    let half = 0.5 * (b - a);
    let starts = match lp.anchor {
        LineAnchor::Best => around(0.5 * (a + b) - span / 2.0, half)
            .into_iter()
            .map(|(s, _)| {
                (
                    s,
                    4.0 * (s + span / 2.0 - 0.5 * (a + b)).abs() / half.max(EPS),
                )
            })
            .collect(),
        LineAnchor::FromStart => around(distance, tolerance),
        LineAnchor::FromEnd => around(len - distance - span, tolerance),
        LineAnchor::BeforeStart => around(-distance - span, tolerance),
        LineAnchor::AfterEnd => around(len + distance, tolerance),
    };
    let on_line = matches!(
        lp.anchor,
        LineAnchor::Best | LineAnchor::FromStart | LineAnchor::FromEnd
    );

    // The line with room for the labels past its ends.
    let e = if on_line {
        overrun
    } else {
        distance + tolerance + span + size
    };
    let m = extend(slot.m, e);

    let mut out: Vec<Candidate> = Vec::new();
    let mut curved = Curved::default();
    let mut rejected_for_turn = false;
    for (s0, pos_cost) in starts {
        if on_line && (s0 < lo - EPS || s0 + span > hi + EPS) {
            continue;
        }
        let s0x = s0 + e;
        if s0x < -EPS || s0x + span > m.length() + EPS {
            continue;
        }
        let mut seen: Vec<LineSide> = Vec::new();
        for &side in &lp.sides {
            if seen.contains(&side) {
                continue;
            }
            let cost = seen.len() as f64 + pos_cost;
            seen.push(side);
            let at = At {
                m: &m,
                s0: s0x,
                side,
                stroke_width: slot.stroke_width,
            };
            let made = match lp.orientation {
                LineOrientation::Curved => curved.candidate(&at, class, lp, shaped),
                LineOrientation::Straight | LineOrientation::Horizontal => {
                    Ok(Some(rigid(&at, class, lp, shaped)))
                }
                LineOrientation::Perpendicular => Ok(Some(perpendicular(&at, class, lp, shaped))),
            };
            match made {
                Ok(None) => {}
                Ok(Some((transforms, extra_cost))) => {
                    let (boxes, bounds) = glyph_boxes(shaped, &transforms, class.margin);
                    out.push(Candidate {
                        leader: None,
                        kind: CandidateKind::Line { side, start: s0 },
                        cost: cost + extra_cost,
                        transforms,
                        boxes,
                        bounds,
                    });
                }
                Err(()) => rejected_for_turn = true,
            }
        }
    }
    if out.is_empty() && rejected_for_turn {
        return Err(UnplacedReason::TooCurved);
    }
    Ok(out)
}

/// One label start and side on the extended line.
struct At<'a> {
    m: &'a Measured,
    s0: f64,
    side: LineSide,
    stroke_width: f64,
}

impl At<'_> {
    /// The side on the page, with the text reading forward if `forward`.
    fn page_side(&self, lp: &LinePlacement, forward: bool) -> LineSide {
        if lp.by_direction && !forward {
            flip(self.side)
        } else {
            self.side
        }
    }

    /// The gap between the middle of the line and the text.
    fn gap(&self, lp: &LinePlacement) -> f64 {
        self.stroke_width / 2.0 + lp.offset
    }
}

/// Glyph transforms and their extra cost, `None` for text that does not fit on the line, or an
/// error for text that turns too much.
type Made = Result<Option<(Vec<Affine>, f64)>, ()>;

/// The baselines of curved text, by side and reading direction.
#[derive(Default)]
struct Curved {
    baselines: Vec<((LineSide, bool), Measured)>,
}

impl Curved {
    fn candidate(
        &mut self,
        at: &At,
        class: &LabelClass,
        lp: &LinePlacement,
        shaped: &ShapedText,
    ) -> Made {
        let size = class.style.size;
        let adv = shaped.advance;
        let cap = shaped.cap_height;
        let m = at.m;
        let s0 = at.s0;
        let p0 = m.point_at(s0);
        let p1 = m.point_at(s0 + adv);
        let forward = lp.follow_direction || p1.x >= p0.x;
        let side = at.page_side(lp, forward);
        let limit = lp.max_glyph_turn;
        let turn_cap = TURN_CAP * limit;

        // The glyphs follow the baseline, a parallel copy of the line, so the letter spacing
        // stays the same on both sides of a bend.
        let d = match side {
            LineSide::Above => at.gap(lp),
            LineSide::Centered => -cap / 2.0,
            LineSide::Below => -(at.gap(lp) + cap),
        };
        let base = match self
            .baselines
            .iter()
            .position(|(k, _)| *k == (side, forward))
        {
            Some(i) => &self.baselines[i].1,
            None => {
                self.baselines
                    .push(((side, forward), parallel(m, if forward { d } else { -d })));
                &self.baselines.last().unwrap().1
            }
        };
        let start = along(m, base, s0 + adv / 2.0) - adv / 2.0;
        if start < 0.0 || start + adv > base.length() {
            return Ok(None);
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
        for w in angles.windows(2) {
            let t = wrap_angle(w[1] - w[0]).abs();
            if t > turn_cap {
                return Err(());
            }
            turn_sum += t;
            over = f64::max(over, t - limit);
        }

        let mut transforms = Vec::with_capacity(ends.len());
        // The last glyph with ink, and its angle.
        let mut prev: Option<(Ink, f64)> = None;
        // Shrunk ink boxes, to find letters that are not neighbors and overlap.
        let mut inks: Vec<OrientedBox> = Vec::new();
        for ((_, g), ((pa, _), th)) in shaped.glyphs().zip(ends.iter().zip(&angles)) {
            let tf = Affine::translate(pa.to_vec2())
                * Affine::rotate(*th)
                * Affine::translate((0.0, g.y));
            if let Some(bb) = g.bbox {
                let ink = (tf, g);
                if let Some((p, pth)) = prev {
                    let over = wrap_angle(th - pth).abs() > limit;
                    if squeezed(p, ink, size, over) {
                        return Err(());
                    }
                }
                prev = Some((ink, *th));
                let k = (SHRINK * size).min(0.4 * bb.width()).min(0.4 * bb.height());
                let shrunk = OrientedBox::from_local_rect(
                    Rect::new(bb.x0 + k, bb.y0 + k, bb.x1 - k, bb.y1 - k),
                    tf,
                );
                let n = inks.len();
                if n >= 2 && inks[..n - 1].iter().any(|o| o.intersects(&shrunk)) {
                    return Err(());
                }
                inks.push(shrunk);
            }
            transforms.push(tf);
        }
        let mut cost = 6.0 * turn_sum / PI;
        if over > 0.0 {
            cost += OVER_TURN_COST + OVER_TURN_WEIGHT * over / limit.max(EPS);
        }
        Ok(Some((transforms, cost)))
    }
}

/// Straight text along the chord of the line under the label, or horizontal text, beside the
/// line with no part of the line under it.
fn rigid(
    at: &At,
    class: &LabelClass,
    lp: &LinePlacement,
    shaped: &ShapedText,
) -> (Vec<Affine>, f64) {
    let size = class.style.size;
    let adv = shaped.advance;
    let cap = shaped.cap_height;
    let m = at.m;
    let p0 = m.point_at(at.s0);
    let p1 = m.point_at(at.s0 + adv);
    let line_forward = p1.x >= p0.x;
    let (u, c, forward) = match lp.orientation {
        LineOrientation::Horizontal => (
            kurbo::Vec2::new(1.0, 0.0),
            m.point_at(at.s0 + adv / 2.0),
            line_forward,
        ),
        _ => {
            let t = p1 - p0;
            let t = if t.hypot() > EPS {
                t / t.hypot()
            } else {
                kurbo::Vec2::new(1.0, 0.0)
            };
            let forward = lp.follow_direction || line_forward;
            (if forward { t } else { -t }, p0.midpoint(p1), forward)
        }
    };
    // The up direction of the text.
    let up = kurbo::Vec2::new(u.y, -u.x);

    // The line under the text, as heights above `c`.
    let (mut top, mut bottom) = (0.0_f64, 0.0_f64);
    let mid = at.s0 + adv / 2.0;
    let reach = adv / 2.0 + size;
    let mut s = mid - reach;
    while s <= mid + reach + EPS {
        let q = m.point_at(s) - c;
        if q.dot(u).abs() <= adv / 2.0 + 0.5 * size {
            top = top.max(q.dot(up));
            bottom = bottom.min(q.dot(up));
        }
        s += 0.25 * size;
    }
    let (rise, lift) = match at.page_side(lp, forward) {
        LineSide::Above => (top + at.gap(lp), top),
        LineSide::Below => (bottom - at.gap(lp) - cap, -bottom),
        LineSide::Centered => (-cap / 2.0, 0.0),
    };
    let start = c + up * rise - u * (adv / 2.0);
    let th = u.y.atan2(u.x);
    let transforms = shaped
        .glyphs()
        .map(|(_, g)| {
            Affine::translate((start + u * g.x).to_vec2())
                * Affine::rotate(th)
                * Affine::translate((0.0, g.y))
        })
        .collect();
    (transforms, lift / size)
}

/// Straight text at a right angle to the line, beside it or across it.
fn perpendicular(
    at: &At,
    class: &LabelClass,
    lp: &LinePlacement,
    shaped: &ShapedText,
) -> (Vec<Affine>, f64) {
    let size = class.style.size;
    let adv = shaped.advance;
    let cap = shaped.cap_height;
    let m = at.m;
    let mid = at.s0 + cap / 2.0;
    let c = m.point_at(mid);
    let t = m.point_at(mid + 0.5 * size) - m.point_at(mid - 0.5 * size);
    let t = if t.hypot() > EPS {
        t / t.hypot()
    } else {
        kurbo::Vec2::new(1.0, 0.0)
    };
    let left = kurbo::Vec2::new(t.y, -t.x);
    let line_forward = t.x >= 0.0;
    let above = if lp.by_direction || line_forward {
        left
    } else {
        -left
    };
    let n = match at.side {
        LineSide::Below => -above,
        _ => above,
    };
    // The text reads up the page or to the right.
    let upright = |v: kurbo::Vec2| v.x > EPS || (v.x.abs() <= EPS && v.y < 0.0);
    let u = if upright(n) { n } else { -n };
    let up = kurbo::Vec2::new(u.y, -u.x);
    let along = match at.side {
        LineSide::Centered => -adv / 2.0,
        _ if u == n => at.gap(lp),
        _ => -(at.gap(lp) + adv),
    };
    let start = c + u * along - up * (cap / 2.0);
    let th = u.y.atan2(u.x);
    let transforms = shaped
        .glyphs()
        .map(|(_, g)| {
            Affine::translate((start + u * g.x).to_vec2())
                * Affine::rotate(th)
                * Affine::translate((0.0, g.y))
        })
        .collect();
    (transforms, 0.0)
}

/// A copy of `m` moved by `d` to the left of its direction, y down. It has the same vertices,
/// so `along` can map an arc length of `m` to it.
pub(crate) fn parallel(m: &Measured, d: f64) -> Measured {
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
pub(crate) fn along(m: &Measured, base: &Measured, s: f64) -> f64 {
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
