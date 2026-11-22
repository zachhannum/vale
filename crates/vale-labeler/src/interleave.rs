//! Labels that interleave. A label with gaps between its letters slides along its path until
//! a gap holds the letters of a label that crosses it.

use crate::collide::OrientedBox;
use crate::engine::{Candidate, Instance, State};
use crate::geom::wrap_angle;
use crate::improve::{Dup, conflicts};
use crate::model::CandidateKind;
use crate::polygon::glyph_boxes;
use crate::text::ShapedText;
use kurbo::{Affine, Rect};

/// The slides that a search tries in each direction, out to one letter pitch.
const STEPS: usize = 16;

/// The most candidates of one label that slide, cheapest first.
const MAX_AIMED: usize = 16;

/// The cost of a slide by one letter pitch.
const SLIDE_COST: f64 = 0.2;

/// The cost of each letter of a label on a letter of a label that it does not conflict with,
/// because one of the two is a background label.
pub(crate) const ON_LETTER: f64 = 4.0;

/// The mean distance between the origins of two neighbor glyphs.
pub(crate) fn pitch(transforms: &[Affine]) -> f64 {
    let n = transforms.len();
    if n < 2 {
        return 0.0;
    }
    let sum: f64 = transforms
        .windows(2)
        .map(|w| (w[1].translation() - w[0].translation()).hypot())
        .sum();
    sum / (n - 1) as f64
}

/// The glyphs moved by `s` along their path. A glyph moves toward the next glyph, or back
/// toward the previous glyph, and turns toward the angle of that glyph. The first and the
/// last glyph continue the path in a straight line.
pub(crate) fn slide(transforms: &[Affine], s: f64) -> Vec<Affine> {
    let n = transforms.len();
    if n < 2 || s == 0.0 {
        return transforms.to_vec();
    }
    let angle = |t: &Affine| {
        let c = t.as_coeffs();
        c[1].atan2(c[0])
    };
    (0..n)
        .map(|k| {
            let (neighbor, a, b) = match (s > 0.0, k) {
                (true, k) if k + 1 < n => (Some(k + 1), k, k + 1),
                (true, k) => (None, k - 1, k),
                (false, 0) => (None, 0, 1),
                (false, k) => (Some(k - 1), k - 1, k),
            };
            let tf = transforms[k];
            let d = transforms[b].translation() - transforms[a].translation();
            let len = d.hypot();
            if len <= 0.0 {
                return tf;
            }
            let t = s / len;
            let own = angle(&tf);
            let turn = neighbor.map_or(0.0, |j| {
                wrap_angle(angle(&transforms[j]) - own) * t.abs().min(1.0)
            });
            let [a0, b0, c0, d0, _, _] = tf.as_coeffs();
            let linear = Affine::rotate(turn) * Affine::new([a0, b0, c0, d0, 0.0, 0.0]);
            Affine::translate(tf.translation() + d * t) * linear
        })
        .collect()
}

/// How far the glyphs can slide back and forward, up to one letter pitch, while `fits` holds
/// for the slid glyphs. The search assumes that a glyph that fits at a slide also fits at each
/// smaller slide in the same direction.
pub(crate) fn room(transforms: &[Affine], fits: impl Fn(&[Affine]) -> bool) -> [f64; 2] {
    let step = pitch(transforms) / STEPS as f64;
    let mut out = [0.0; 2];
    if step <= 0.0 {
        return out;
    }
    for (side, sign) in [-1.0, 1.0].into_iter().enumerate() {
        let (mut lo, mut hi) = (0, STEPS);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if fits(&slide(transforms, sign * mid as f64 * step)) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        out[side] = lo as f64 * step;
    }
    out
}

/// The glyphs slid as little as they can, within `room`, so that no glyph box touches
/// `aims`, with the slide, the boxes, and their bounds.
fn clear(
    shaped: &ShapedText,
    transforms: &[Affine],
    margin: f64,
    room: [f64; 2],
    aims: &[OrientedBox],
) -> Option<(f64, Vec<Affine>, Vec<OrientedBox>, Rect)> {
    let step = pitch(transforms) / STEPS as f64;
    if step <= 0.0 {
        return None;
    }
    for i in 1..=STEPS {
        for s in [i as f64 * step, -(i as f64) * step] {
            if s > room[1] || -s > room[0] {
                continue;
            }
            let slid = slide(transforms, s);
            let (boxes, bounds) = glyph_boxes(shaped, &slid, margin);
            if !boxes.iter().any(|b| aims.iter().any(|a| a.intersects(b))) {
                return Some((s, slid, boxes, bounds));
            }
        }
    }
    None
}

/// What the labeler knows about the label of an instance.
pub(crate) struct Text<'a> {
    pub shaped: &'a ShapedText,
    pub margin: f64,
}

/// `c` slid by `s`, with the cost of the slide. `obstacles` gives the cost of the obstacles
/// under a set of boxes, or `None` if a forbidden obstacle blocks them.
fn slid(
    c: &Candidate,
    (s, transforms, boxes, bounds): (f64, Vec<Affine>, Vec<OrientedBox>, Rect),
    obstacles: &mut impl FnMut(&[OrientedBox]) -> Option<f64>,
) -> Option<Candidate> {
    let before = obstacles(&c.boxes)?;
    let after = obstacles(&boxes)?;
    let kind = match c.kind {
        CandidateKind::Line { side, start } => CandidateKind::Line {
            side,
            start: start + s,
        },
        k => k,
    };
    let p = pitch(&c.transforms).max(f64::EPSILON);
    Some(Candidate {
        kind,
        cost: c.cost - before + after + SLIDE_COST * s.abs() / p,
        transforms,
        boxes,
        bounds,
        leader: None,
        slide: [c.slide[0] + s, c.slide[1] - s],
    })
}

/// New candidates where labels interleave, from the placed set in `state`. A candidate that
/// hits the letters of a placed label slides until a gap holds them, and the placed label
/// slides until one of its gaps holds the letter in the way. `obstacles(i, boxes)` gives the
/// cost of the obstacles under `boxes` for instance `i`. The new candidates of each instance
/// are in the order of their cost.
pub(crate) fn aim(
    instances: &[Instance],
    state: &mut State,
    texts: &[Text],
    mut obstacles: impl FnMut(usize, &[OrientedBox]) -> Option<f64>,
) -> Vec<Vec<Candidate>> {
    let mut out: Vec<Vec<Candidate>> = (0..instances.len()).map(|_| Vec::new()).collect();
    let mut hits: Vec<usize> = Vec::new();
    for (i, inst) in instances.iter().enumerate() {
        let last = state.chosen[i].map_or(inst.candidates.len(), |ci| ci + 1);
        for c in inst.candidates[..last].iter().take(MAX_AIMED) {
            hits.clear();
            state.letters(instances, &c.boxes, i, &mut hits);
            if hits.is_empty() {
                continue;
            }
            let aims: Vec<OrientedBox> = hits
                .iter()
                .flat_map(|&j| state.boxes(instances, j).iter().copied())
                .collect();
            let t = &texts[i];
            if c.slide != [0.0; 2]
                && let Some(found) = clear(t.shaped, &c.transforms, t.margin, c.slide, &aims)
                && let Some(new) = slid(c, found, &mut |b| obstacles(i, b))
            {
                out[i].push(new);
            }
            for &j in &hits {
                let cj = state.chosen_candidate(instances, j);
                let tj = &texts[j];
                if cj.slide != [0.0; 2]
                    && let Some(found) =
                        clear(tj.shaped, &cj.transforms, tj.margin, cj.slide, &c.boxes)
                    && let Some(new) = slid(cj, found, &mut |b| obstacles(j, b))
                {
                    out[j].push(new);
                }
            }
        }
    }
    for cands in &mut out {
        cands.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    }
    out
}

/// The number of glyph boxes in `boxes` of instance `own` that touch a glyph box of a placed
/// label that `own` does not conflict with.
pub(crate) fn on_letters(
    instances: &[Instance],
    state: &mut State,
    boxes: &[OrientedBox],
    own: usize,
) -> usize {
    let mut hits: Vec<usize> = Vec::new();
    state.letters(instances, boxes, own, &mut hits);
    hits.retain(|&j| instances[j].background != instances[own].background);
    boxes
        .iter()
        .filter(|b| {
            hits.iter()
                .any(|&j| state.boxes(instances, j).iter().any(|o| o.intersects(b)))
        })
        .count()
}

/// The rounds of [`settle`].
const ROUNDS: usize = 3;

/// Moves labels to cheaper candidates, in priority order, after new candidates came in. A
/// label takes a cheaper candidate where it is free. Where up to two placed labels block it,
/// they move to their cheapest free candidates, and the move stays if the sum of the costs
/// falls. An unplaced label costs more than any placed one. Each move lowers the energy, so
/// the placement keeps what the search found.
pub(crate) fn settle(instances: &[Instance], state: &mut State, order: &[usize], dup: &Dup) {
    let mut k: Vec<usize> = Vec::new();
    let mut blockers: Vec<(usize, usize)> = Vec::new();
    for _ in 0..ROUNDS {
        let mut moved = false;
        for &i in order {
            let current = state.chosen[i];
            let last = current.unwrap_or(instances[i].candidates.len());
            let cost_of = |inst: usize, c: Option<usize>| {
                c.map_or(f64::INFINITY, |c| instances[inst].candidates[c].cost)
            };
            for c in 0..last {
                conflicts(instances, state, dup, i, c, &mut k);
                if k.len() > 2 {
                    continue;
                }
                blockers.clear();
                for &b in &k {
                    blockers.push((b, state.chosen[b].expect("conflicts are placed")));
                }
                state.remove(instances, i);
                for &(b, _) in &blockers {
                    state.remove(instances, b);
                }
                state.insert(instances, i, c);
                let mut delta = instances[i].candidates[c].cost - cost_of(i, current);
                let mut placed = true;
                for &(b, old) in &blockers {
                    let next = (0..instances[b].candidates.len()).find(|&cb| {
                        cb != old && {
                            conflicts(instances, state, dup, b, cb, &mut k);
                            k.is_empty()
                        }
                    });
                    match next {
                        Some(cb) => {
                            state.insert(instances, b, cb);
                            delta += instances[b].candidates[cb].cost - cost_of(b, Some(old));
                        }
                        None => placed = false,
                    }
                }
                if placed && delta < 0.0 {
                    moved = true;
                    break;
                }
                state.remove(instances, i);
                for &(b, old) in &blockers {
                    state.remove(instances, b);
                    state.insert(instances, b, old);
                }
                if let Some(old) = current {
                    state.insert(instances, i, old);
                }
            }
        }
        if !moved {
            break;
        }
    }
}
