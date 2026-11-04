//! Polygon label candidates: text that curves along the long axis of the shape, or horizontal
//! text at its visual center.
//!
//! The visible part of the polygon becomes a grid of cells. A distance field on the grid gives
//! the visual center, which is the point farthest from the edge. The long axis is the path
//! between the two cells that are farthest apart inside the shape, and a cost that grows near
//! the edge keeps it in the middle.

use crate::collide::OrientedBox;
use crate::engine::Candidate;
use crate::geom::{Measured, smooth, wrap_angle};
use crate::graticule::{Parallels, upright};
use crate::line::{along, parallel};
use crate::model::{
    CandidateKind, LabelClass, Leader, LeaderAnchor, PolygonMode, PolygonPlacement, UnplacedReason,
    Zone,
};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::f64::consts::{PI, SQRT_2};

/// The most labels of one part of a polygon.
const MAX_REPEATS: usize = 64;

/// The most cells of the grid.
const MAX_CELLS: f64 = 60_000.0;

/// The smallest cell, as a multiple of the font size.
const MIN_CELL: f64 = 0.25;

/// The ink of a label keeps this distance from the edge, as a multiple of the font size.
const PAD: f64 = 0.15;

/// A shape is long if its axis is this many times its width.
const LONG: f64 = 2.5;

/// The extra cost of the placement that does not suit the shape: horizontal text in a long
/// shape, or curved text in a compact shape.
const UNSUITED: f64 = 4.0;

/// The extra cost of a horizontal label in a curved class. It is lower than the cost of an
/// unplaced label, so the horizontal label is the fallback where no curved label fits.
const FALLBACK: f64 = 50.0;

/// The fractions of the axis that a curved label spans, best first. Zero adds no spread.
const FILLS: [f64; 4] = [0.85, 0.7, 0.55, 0.0];

/// The step between the positions of a curved label along the axis, as a fraction of its length.
const SHIFT: f64 = 0.08;

/// The most horizontal positions of each zone rank, and half the most positions along the
/// parallels.
const SITES: usize = 8;

const TURN_CAP: f64 = 1.6;
const OVER_TURN_COST: f64 = 20.0;
const OVER_TURN_WEIGHT: f64 = 10.0;

/// The label slots of a polygon, one for each instance: a part of the shape, or a region of a
/// part where the label repeats. Each slot has its candidates, or the reason that it has none.
pub(crate) fn slots(
    rings: &[Vec<Point>],
    others: &[&[Vec<Point>]],
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    bounds: Rect,
    parallels: &Parallels,
) -> Vec<Result<Vec<Candidate>, UnplacedReason>> {
    let size = class.style.size;
    // The depth that the middle of a label needs.
    let half = (0.5 * shaped.cap_height + PAD * size - pp.overrun.max(0.0)).max(0.0);
    let Some(raster) = Raster::new(rings, others, pp, bounds, size) else {
        return vec![Err(UnplacedReason::OutOfBounds)];
    };
    let mut parts = raster.parts();
    if pp.largest_part_only {
        parts.truncate(1);
    }
    let cell_area = raster.cell * raster.cell;
    parts.retain(|p| p.len() as f64 * cell_area >= pp.min_area);
    if parts.is_empty() {
        return vec![Err(UnplacedReason::BelowMinimumArea)];
    }
    let mut out = Vec::new();
    for part in parts {
        let grid = Grid::new(&raster, &part, pp.overrun);
        let regions = match pp.repeat_distance {
            Some(d) => grid.regions(d.max(size), half),
            None => Vec::new(),
        };
        if regions.len() < 2 {
            out.push(candidates(
                &grid,
                Some((&raster, rings)),
                class,
                pp,
                shaped,
                half,
                parallels,
            ));
        } else {
            for region in regions {
                let grid = Grid::new(&raster, &region, pp.overrun);
                out.push(candidates(&grid, None, class, pp, shaped, half, parallels));
            }
        }
    }
    // A part or a region where the label does not fit gets no label, if another one gets one.
    if out.len() > 1 && out.iter().any(Result::is_ok) {
        out.retain(Result::is_ok);
    }
    out
}

/// The candidates in one slot. With `whole`, the polygon and its rings, the slot also gets the
/// candidates outside the polygon.
fn candidates(
    grid: &Grid,
    whole: Option<(&Raster, &[Vec<Point>])>,
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    half: f64,
    parallels: &Parallels,
) -> Result<Vec<Candidate>, UnplacedReason> {
    let ranked = grid.ranked();
    let center = ranked[0];
    let dmax = grid.edge_distance(center);
    let mut out = Vec::new();
    if dmax >= half {
        inside(
            grid, &ranked, class, pp, shaped, dmax, half, parallels, &mut out,
        );
    }
    if let Some((raster, rings)) = whole {
        outside(grid, raster, rings, center, class, pp, shaped, &mut out);
    }
    if out.is_empty() {
        return Err(UnplacedReason::AreaTooSmall);
    }
    Ok(out)
}

/// The candidates inside the shape.
#[allow(clippy::too_many_arguments)]
fn inside(
    grid: &Grid,
    ranked: &[usize],
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    dmax: f64,
    half: f64,
    parallels: &Parallels,
    out: &mut Vec<Candidate>,
) {
    let axis = grid.axis(ranked[0], half);
    let long = axis
        .as_ref()
        .is_some_and(|m| m.length() >= LONG * 2.0 * dmax);

    if pp.mode == PolygonMode::Parallel {
        along_parallels(grid, ranked, parallels, class, pp, shaped, dmax, half, out);
    } else if pp.mode != PolygonMode::Horizontal
        && let Some(m) = &axis
    {
        let base = if long || pp.mode == PolygonMode::Curved {
            0.0
        } else {
            UNSUITED
        };
        curved(grid, m, class, pp, shaped, base, out);
    }
    let base = match pp.mode {
        PolygonMode::Auto if long => UNSUITED,
        PolygonMode::Curved | PolygonMode::Parallel => FALLBACK,
        _ => 0.0,
    };
    let follow = (pp.graticule || pp.mode == PolygonMode::Parallel).then_some(parallels);
    horizontal(
        grid, ranked, follow, class, pp, shaped, base, dmax, half, out,
    );
}

/// The extra cost of a label outside the polygon. It is more than the cost of each label inside.
const OUTSIDE: f64 = 60.0;

/// The extra cost of each rank of a zone after the first.
const ZONE_STEP: f64 = 1.0;

/// The space between a label outside the polygon and the outline, as a multiple of the font size.
const OUTSIDE_GAP: f64 = 0.3;

/// Horizontal candidates outside the polygon, one in each zone that is not blocked, each with a
/// leader line to the anchor of the class.
#[allow(clippy::too_many_arguments)]
fn outside(
    grid: &Grid,
    raster: &Raster,
    rings: &[Vec<Point>],
    center: usize,
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    out: &mut Vec<Candidate>,
) {
    let size = class.style.size;
    let ink = shaped.ink_bounds;
    let visual = grid.point(center);
    let start = match pp.anchor {
        LeaderAnchor::Center => grid.centroid(),
        LeaderAnchor::Inside | LeaderAnchor::Outline => visual,
    };
    let gap = OUTSIDE_GAP * size;
    let step = raster.cell.max(0.25 * size);
    let reach = raster.extent().width() + raster.extent().height() + ink.width() + ink.height();
    for zone in Zone::OUTSIDE {
        let rank = pp.zones.outside(zone);
        if rank == 0 {
            continue;
        }
        let (dx, dy) = zone.offset();
        let place = |t: f64| {
            let x0 = match dx {
                1 => start.x + t,
                -1 => start.x - t - ink.width(),
                _ => start.x - 0.5 * ink.width(),
            };
            let y0 = match dy {
                1 => start.y + t,
                -1 => start.y - t - ink.height(),
                _ => start.y - 0.5 * ink.height(),
            };
            Rect::new(x0, y0, x0 + ink.width(), y0 + ink.height())
        };
        let mut t = 0.0;
        while t <= reach && !raster.clear(place(t).inflate(gap, gap)) {
            t += step;
        }
        if t > reach {
            continue;
        }
        let rect = place(t);
        let anchor = match pp.anchor {
            LeaderAnchor::Center => start,
            LeaderAnchor::Inside => visual,
            LeaderAnchor::Outline => nearest_on_outline(rings, rect.center()).unwrap_or(visual),
        };
        let end = Point::new(
            anchor.x.clamp(rect.x0, rect.x1),
            anchor.y.clamp(rect.y0, rect.y1),
        );
        let origin = Point::new(rect.x0 - ink.x0, rect.y0 - ink.y0);
        let transforms: Vec<Affine> = shaped
            .glyphs()
            .map(|(_, g)| Affine::translate((origin.x + g.x, origin.y + g.y)))
            .collect();
        let (boxes, bounds) = glyph_boxes(shaped, &transforms, class.margin);
        out.push(Candidate {
            kind: CandidateKind::Polygon { curved: false },
            cost: OUTSIDE + ZONE_STEP * (rank - 1) as f64 + 0.2 * t / size,
            transforms,
            boxes,
            bounds,
            leader: Some(Leader { anchor, end }),
        });
    }
}

/// The point of the rings nearest to `p`.
fn nearest_on_outline(rings: &[Vec<Point>], p: Point) -> Option<Point> {
    let mut best: Option<(f64, Point)> = None;
    for ring in rings {
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let ab = b - a;
            let t = if ab.hypot2() > 0.0 {
                ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = a + ab * t;
            let d = q.distance_squared(p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
    }
    best.map(|(_, q)| q)
}

/// Candidates along the axis `m`, with letters spread to fill a part of it.
fn curved(
    grid: &Grid,
    m: &Measured,
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    base_cost: f64,
    out: &mut Vec<Candidate>,
) {
    let size = class.style.size;
    let adv = shaped.advance;
    let len = m.length();
    if len < adv {
        return;
    }
    let gaps = shaped.glyph_count().saturating_sub(1);
    // The baseline is below the axis by half the cap height, so the text centers on the axis.
    let base = parallel(m, -shaped.cap_height / 2.0);
    let limit = pp.max_glyph_turn;
    let mut spans: Vec<f64> = Vec::new();
    for (fi, fill) in FILLS.iter().enumerate() {
        let extra = spread(fill * len, shaped, pp.max_spread * size);
        let span = adv + extra * gaps as f64;
        if spans.iter().any(|s| (s - span).abs() < 0.5 * size) {
            continue;
        }
        spans.push(span);
        for k in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0] {
            let mid = 0.5 * len + k * SHIFT * len;
            if mid - span / 2.0 < 0.0 || mid + span / 2.0 > len {
                continue;
            }
            let start = along(m, &base, mid) - span / 2.0;
            if start < 0.0 || start + span > base.length() {
                continue;
            }
            let f = follow(&base, start, shaped, extra);
            if f.max_turn > TURN_CAP * limit {
                continue;
            }
            let turns = turn_cost(&f, limit);
            let Some(c) = grid.candidate(shaped, f.transforms, class.margin, PAD * size) else {
                continue;
            };
            let cost = base_cost + 0.5 * fi as f64 + 0.5 * k.abs() + turns;
            out.push(Candidate {
                kind: CandidateKind::Polygon { curved: true },
                cost: cost + c.cost,
                ..c
            });
        }
    }
}

/// Horizontal candidates at the cells farthest from the edge in the zones of each rank.
/// The letters spread to fill a part of the width of the shape at each cell.
#[allow(clippy::too_many_arguments)]
fn horizontal(
    grid: &Grid,
    ranked: &[usize],
    parallels: Option<&Parallels>,
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    base_cost: f64,
    dmax: f64,
    half: f64,
    out: &mut Vec<Candidate>,
) {
    let size = class.style.size;
    let ink = shaped.ink_bounds;
    let gaps = shaped.glyph_count().saturating_sub(1) as f64;
    let apart = size.max(0.25 * dmax);
    // For each rank, best first, the deepest cells in the zones of that rank, apart from each
    // other.
    let mut ranks: Vec<u8> = pp.zones.inside.iter().copied().filter(|r| *r > 0).collect();
    ranks.sort_unstable();
    ranks.dedup();
    let mut sites: Vec<Point> = Vec::new();
    let mut picks: Vec<(usize, f64, usize)> = Vec::new();
    for r in ranks {
        let mut k = 0;
        for &i in ranked
            .iter()
            .take_while(|&&i| grid.edge_distance(i) >= half)
        {
            if k == SITES {
                break;
            }
            let c = grid.point(i);
            if pp.zones.inside(grid.zone(c)) != r || sites.iter().any(|s| s.distance(c) < apart) {
                continue;
            }
            sites.push(c);
            picks.push((i, ZONE_STEP * (r - 1) as f64, k));
            k += 1;
        }
    }
    for (i, zone_cost, rank) in picks {
        let c = grid.point(i);
        let d = grid.edge_distance(i);
        // The curve through the site, and the arc length of the site on it.
        let through = parallels.map(|ps| {
            let t = ps.through(c);
            let (lo, hi) = grid.run(&t.curve, t.at, 0.0);
            let (curve, map) = upright(t.curve, lo, hi, pp.no_flip);
            (curve, map(t.at), hi - lo)
        });
        let width = match &through {
            Some((_, _, w)) => *w,
            None => grid.row_width(i),
        } - 2.0 * PAD * size;
        let mut spans: Vec<f64> = Vec::new();
        for (fi, fill) in FILLS.iter().enumerate() {
            let extra = spread(fill * width, shaped, pp.max_spread * size);
            let span = extra * gaps;
            if spans.iter().any(|s| (s - span).abs() < 0.5 * size) {
                continue;
            }
            spans.push(span);
            let transforms = if let Some((curve, at, _)) = &through {
                // The ink centers on the site, as in horizontal text.
                let base = parallel(curve, ink.center().y);
                let start = along(curve, &base, at - ink.center().x - span / 2.0);
                follow(&base, start, shaped, extra).transforms
            } else {
                let origin = Point::new(c.x - ink.center().x - span / 2.0, c.y - ink.center().y);
                shaped
                    .glyphs()
                    .enumerate()
                    .map(|(k, (_, g))| {
                        Affine::translate((origin.x + g.x + extra * k as f64, origin.y + g.y))
                    })
                    .collect()
            };
            let Some(cand) = grid.candidate(shaped, transforms, class.margin, PAD * size) else {
                continue;
            };
            out.push(Candidate {
                kind: CandidateKind::Polygon { curved: false },
                cost: base_cost
                    + zone_cost
                    + 3.0 * (1.0 - d / dmax).max(0.0)
                    + 0.1 * rank as f64
                    + 0.5 * fi as f64,
                ..cand
            });
        }
    }
}

/// The most curves through the shape that a label along the parallels tries.
const LATITUDES: usize = 4;

/// Candidates along the parallels through the shape. Curves at more than one latitude go
/// through the sites farthest from the edge, and the longest part of a curve inside the shape
/// costs least. Of two parts with the same length, the one through the deeper site costs less.
#[allow(clippy::too_many_arguments)]
fn along_parallels(
    grid: &Grid,
    ranked: &[usize],
    parallels: &Parallels,
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    dmax: f64,
    half: f64,
    out: &mut Vec<Candidate>,
) {
    let size = class.style.size;
    let center = grid.point(ranked[0]);
    // Sites: the cells farthest from the edge, apart from each other, and a row of points
    // across the parallel through the visual center.
    let apart = size.max(0.5 * dmax);
    let mut sites: Vec<Point> = Vec::new();
    for &i in ranked
        .iter()
        .take_while(|&&i| grid.edge_distance(i) >= half)
    {
        if sites.len() == 2 * SITES {
            break;
        }
        let c = grid.point(i);
        if sites.iter().all(|s| s.distance(c) >= apart) {
            sites.push(c);
        }
    }
    let t = parallels.through(center);
    let tangent = t.curve.point_at(t.at + 1.0) - t.curve.point_at(t.at - 1.0);
    let normal = kurbo::Vec2::new(tangent.y, -tangent.x).normalize();
    if normal.is_finite() {
        for k in 1..=6 {
            for sign in [1.0, -1.0] {
                let p = center + normal * (sign * k as f64 * apart);
                if grid.holds(p, half) {
                    sites.push(p);
                }
            }
        }
    }

    // The longest part inside the shape of the curve through each site.
    let mut runs: Vec<(Measured, f64, f64)> = Vec::new();
    for p in sites {
        let t = parallels.through(p);
        let Some((lo, hi)) = grid.longest_run(&t.curve, half) else {
            continue;
        };
        let mid = t.curve.point_at(0.5 * (lo + hi));
        if runs
            .iter()
            .any(|(c, a, b)| c.point_at(0.5 * (a + b)).distance(mid) < 0.5 * size)
        {
            continue;
        }
        runs.push((t.curve, lo, hi));
    }
    // A run is a whole number of steps. Runs compare by their steps, so a rounding error that
    // changes when the page moves does not change the order. Equal runs keep the site order.
    let step = 0.5 * grid.cell;
    let steps = |lo: f64, hi: f64| ((hi - lo) / step).round();
    runs.sort_by(|a, b| steps(b.1, b.2).total_cmp(&steps(a.1, a.2)));
    runs.truncate(LATITUDES);
    let Some(room) = runs.first().map(|r| steps(r.1, r.2)) else {
        return;
    };

    let adv = shaped.advance;
    let gaps = shaped.glyph_count().saturating_sub(1);
    for (rank, (curve, lo, hi)) in runs.into_iter().enumerate() {
        let len = hi - lo;
        if len < adv {
            continue;
        }
        let room_cost = 3.0 * (1.0 - steps(lo, hi) / room);
        let (curve, map) = upright(curve, lo, hi, pp.no_flip);
        let (lo, hi) = {
            let (a, b) = (map(lo), map(hi));
            (a.min(b), a.max(b))
        };
        let base = parallel(&curve, -shaped.cap_height / 2.0);
        let mut spans: Vec<f64> = Vec::new();
        for (fi, fill) in FILLS.iter().enumerate() {
            let extra = spread(fill * len, shaped, pp.max_spread * size);
            let span = adv + extra * gaps as f64;
            if spans.iter().any(|s| (s - span).abs() < 0.5 * size) {
                continue;
            }
            spans.push(span);
            for k in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0] {
                let mid = lo + 0.5 * len + k * SHIFT * len;
                if mid - span / 2.0 < lo || mid + span / 2.0 > hi {
                    continue;
                }
                let start = along(&curve, &base, mid) - span / 2.0;
                if start < 0.0 || start + span > base.length() {
                    continue;
                }
                let f = follow(&base, start, shaped, extra);
                if f.max_turn > TURN_CAP * pp.max_glyph_turn {
                    continue;
                }
                let turns = turn_cost(&f, pp.max_glyph_turn);
                let Some(c) = grid.candidate(shaped, f.transforms, class.margin, PAD * size) else {
                    continue;
                };
                let cost = room_cost + 0.1 * rank as f64 + 0.5 * fi as f64 + 0.5 * k.abs() + turns;
                out.push(Candidate {
                    kind: CandidateKind::Polygon { curved: true },
                    cost: cost + c.cost,
                    ..c
                });
            }
        }
    }
}

/// Glyphs along a baseline.
pub(crate) struct Follow {
    pub transforms: Vec<Affine>,
    /// The sum of the turns between neighbor glyphs, in radians.
    pub turn_sum: f64,
    /// The largest turn between two neighbor glyphs, in radians.
    pub max_turn: f64,
}

/// The glyphs of `shaped` along `base` from the arc length `start`, with `extra` space after
/// each glyph but the last.
pub(crate) fn follow(base: &Measured, start: f64, shaped: &ShapedText, extra: f64) -> Follow {
    let mut ends: Vec<(Point, Point)> = Vec::new();
    for (i, (_, g)) in shaped.glyphs().enumerate() {
        let sa = start + g.x + extra * i as f64;
        ends.push((base.point_at(sa), base.point_at(sa + g.advance)));
    }
    let mut angles: Vec<f64> = Vec::with_capacity(ends.len());
    for (pa, pb) in &ends {
        let th = if pa == pb {
            angles.last().copied().unwrap_or(0.0)
        } else {
            (pb.y - pa.y).atan2(pb.x - pa.x)
        };
        angles.push(th);
    }
    let (mut turn_sum, mut max_turn) = (0.0, 0.0_f64);
    for w in angles.windows(2) {
        let t = wrap_angle(w[1] - w[0]).abs();
        turn_sum += t;
        max_turn = max_turn.max(t);
    }
    let transforms = shaped
        .glyphs()
        .zip(ends.iter().zip(&angles))
        .map(|((_, g), ((pa, _), th))| {
            Affine::translate(pa.to_vec2()) * Affine::rotate(*th) * Affine::translate((0.0, g.y))
        })
        .collect();
    Follow {
        transforms,
        turn_sum,
        max_turn,
    }
}

/// The cost of the turns of `f`, with `limit` as the angle above which a turn costs more.
fn turn_cost(f: &Follow, limit: f64) -> f64 {
    let over = f.max_turn - limit;
    let mut cost = 6.0 * f.turn_sum / PI;
    if over > 0.0 {
        cost += OVER_TURN_COST + OVER_TURN_WEIGHT * over / limit.max(1e-9);
    }
    cost
}

/// One box for each glyph with ink, grown by `margin`, and the bounds of the boxes.
pub(crate) fn glyph_boxes(
    shaped: &ShapedText,
    transforms: &[Affine],
    margin: f64,
) -> (Vec<OrientedBox>, Rect) {
    let boxes: Vec<OrientedBox> = shaped
        .glyphs()
        .zip(transforms)
        .filter_map(|((_, g), tf)| Some(OrientedBox::from_local_rect(g.bbox?, *tf).inflate(margin)))
        .collect();
    let mut bounds: Option<Rect> = None;
    for b in &boxes {
        let r = b.aabb();
        bounds = Some(bounds.map_or(r, |u| u.union(r)));
    }
    (boxes, bounds.unwrap_or(Rect::ZERO))
}

/// The extra space between two letters that makes the text `len` long, at most `max`.
fn spread(len: f64, shaped: &ShapedText, max: f64) -> f64 {
    let gaps = shaped.glyph_count().saturating_sub(1);
    if gaps == 0 {
        return 0.0;
    }
    ((len - shaped.advance) / gaps as f64).clamp(0.0, max.max(0.0))
}

/// The visible polygon as a grid of cells. A border of cells outside the shape surrounds the
/// grid, as wide as the overrun of the class.
struct Raster {
    origin: Point,
    cell: f64,
    w: usize,
    h: usize,
    /// Cells inside the polygon.
    fill: Vec<bool>,
}

impl Raster {
    fn new(
        rings: &[Vec<Point>],
        others: &[&[Vec<Point>]],
        pp: &PolygonPlacement,
        bounds: Rect,
        size: f64,
    ) -> Option<Raster> {
        let mut bbox: Option<Rect> = None;
        for p in rings.iter().flatten() {
            if !p.is_finite() {
                return None;
            }
            bbox = Some(bbox.map_or(Rect::from_points(*p, *p), |b| b.union_pt(*p)));
        }
        let clip = bbox?.intersect(bounds);
        if !(clip.width() > 0.0 && clip.height() > 0.0) {
            return None;
        }
        let cell = (MIN_CELL * size).max((clip.area() / MAX_CELLS).sqrt());
        if !cell.is_finite() || cell <= 0.0 {
            return None;
        }
        let border = 1 + (pp.overrun.max(0.0) / cell).ceil() as usize;
        let w = (clip.width() / cell).ceil() as usize + 2 * border;
        let h = (clip.height() / cell).ceil() as usize + 2 * border;
        let origin = Point::new(
            clip.x0 - border as f64 * cell,
            clip.y0 - border as f64 * cell,
        );
        let mut r = Raster {
            origin,
            cell,
            w,
            h,
            fill: vec![false; w * h],
        };
        let mut fill = vec![false; w * h];
        if pp.over_holes {
            // Each outer ring fills on its own, so a hole does not cut it.
            for (k, ring) in rings.iter().enumerate() {
                let depth = rings
                    .iter()
                    .enumerate()
                    .filter(|(o, other)| {
                        *o != k && ring.first().is_some_and(|p| contains(other, *p))
                    })
                    .count();
                if depth % 2 == 0 {
                    r.scan(std::slice::from_ref(ring), clip, &mut fill);
                }
            }
        } else {
            r.scan(rings, clip, &mut fill);
        }
        // A shape smaller than a cell fills the cell at its center.
        if !fill.contains(&true)
            && let Some(i) = r.index(clip.center())
        {
            fill[i] = true;
        }
        let mut cut = vec![false; w * h];
        for other in others {
            r.scan(other, clip, &mut cut);
        }
        for (f, c) in fill.iter_mut().zip(&cut) {
            *f &= !c;
        }
        r.fill = fill;
        Some(r)
    }

    /// Sets the cells inside `rings` with the even-odd rule, by scanlines through the cell
    /// centers.
    fn scan(&self, rings: &[Vec<Point>], clip: Rect, fill: &mut [bool]) {
        let (origin, cell, w) = (self.origin, self.cell, self.w);
        let mut xs: Vec<f64> = Vec::new();
        for j in 1..self.h - 1 {
            let y = origin.y + (j as f64 + 0.5) * cell;
            if y < clip.y0 || y > clip.y1 {
                continue;
            }
            xs.clear();
            for ring in rings {
                let n = ring.len();
                for k in 0..n {
                    let (a, b) = (ring[k], ring[(k + 1) % n]);
                    if (a.y <= y) != (b.y <= y) {
                        xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for [x0, x1] in xs.as_chunks::<2>().0 {
                let x0 = x0.max(clip.x0);
                let x1 = x1.min(clip.x1);
                let i0 = ((x0 - origin.x) / cell - 0.5).ceil().max(1.0) as usize;
                let i1 = ((x1 - origin.x) / cell - 0.5).floor();
                if i1 < 1.0 {
                    continue;
                }
                for i in i0..=(i1 as usize).min(w - 2) {
                    fill[j * w + i] = true;
                }
            }
        }
    }

    fn index(&self, p: Point) -> Option<usize> {
        let i = ((p.x - self.origin.x) / self.cell).floor();
        let j = ((p.y - self.origin.y) / self.cell).floor();
        if i < 0.0 || j < 0.0 || i >= self.w as f64 || j >= self.h as f64 {
            return None;
        }
        Some(j as usize * self.w + i as usize)
    }

    /// The bounds of the grid.
    fn extent(&self) -> Rect {
        Rect::new(
            self.origin.x,
            self.origin.y,
            self.origin.x + self.w as f64 * self.cell,
            self.origin.y + self.h as f64 * self.cell,
        )
    }

    /// True if no cell that `rect` touches is inside the polygon.
    fn clear(&self, rect: Rect) -> bool {
        let e = self.extent();
        let r = rect.intersect(e);
        if !(r.width() >= 0.0 && r.height() >= 0.0) || rect.x0 > e.x1 || rect.y0 > e.y1 {
            return true;
        }
        let to =
            |v: f64, o: f64, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let (i0, i1) = (to(r.x0, e.x0, self.w), to(r.x1, e.x0, self.w));
        let (j0, j1) = (to(r.y0, e.y0, self.h), to(r.y1, e.y0, self.h));
        (j0..=j1).all(|j| (i0..=i1).all(|i| !self.fill[j * self.w + i]))
    }

    /// The 4-connected parts of the polygon, largest first. Each part is a list of cells.
    fn parts(&self) -> Vec<Vec<usize>> {
        let w = self.w;
        let mut seen = vec![false; self.fill.len()];
        let mut parts: Vec<Vec<usize>> = Vec::new();
        let mut stack = Vec::new();
        for s in 0..self.fill.len() {
            if !self.fill[s] || seen[s] {
                continue;
            }
            let mut part = Vec::new();
            seen[s] = true;
            stack.push(s);
            while let Some(c) = stack.pop() {
                part.push(c);
                for n in [c - 1, c + 1, c - w, c + w] {
                    if self.fill[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            parts.push(part);
        }
        parts.sort_by_key(|p| Reverse(p.len()));
        parts
    }
}

/// Even-odd point in one ring.
fn contains(ring: &[Point], p: Point) -> bool {
    let mut inside = false;
    let n = ring.len();
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        if (a.y <= p.y) != (b.y <= p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// A part of the polygon, or a region of a part, on the cells of its raster.
struct Grid {
    origin: Point,
    cell: f64,
    w: usize,
    h: usize,
    /// The cells of the part.
    inside: Vec<bool>,
    /// The distance from each cell to the nearest cell outside, in cells.
    dist: Vec<f64>,
    /// The distance from each cell outside to the nearest cell inside, in cells. Empty if the
    /// overrun is zero.
    out_dist: Vec<f64>,
    overrun: f64,
    /// The bounds of the cell centers of the part.
    bbox: Rect,
}

impl Grid {
    fn new(raster: &Raster, cells: &[usize], overrun: f64) -> Grid {
        let (w, h) = (raster.w, raster.h);
        let mut inside = vec![false; w * h];
        for &c in cells {
            inside[c] = true;
        }
        let dist = chamfer(&inside, w, h);
        let out_dist = if overrun > 0.0 {
            let outside: Vec<bool> = inside.iter().map(|i| !i).collect();
            chamfer(&outside, w, h)
        } else {
            Vec::new()
        };
        let mut g = Grid {
            origin: raster.origin,
            cell: raster.cell,
            w,
            h,
            inside,
            dist,
            out_dist,
            overrun: overrun.max(0.0),
            bbox: Rect::ZERO,
        };
        let first = g.point(cells[0]);
        g.bbox = cells.iter().fold(Rect::from_points(first, first), |b, &c| {
            b.union_pt(g.point(c))
        });
        g
    }

    /// The zone of `p` inside the part: the cell of a three by three grid over its bounds.
    fn zone(&self, p: Point) -> Zone {
        let b = self.bbox;
        let third = |v: f64, lo: f64, len: f64| {
            if len <= 0.0 {
                1
            } else {
                ((3.0 * (v - lo) / len).floor() as usize).min(2)
            }
        };
        let col = third(p.x, b.x0, b.width());
        let row = third(p.y, b.y0, b.height());
        Zone::INSIDE[row * 3 + col]
    }

    /// The centroid of the cells of the part.
    fn centroid(&self) -> Point {
        let (mut sum, mut n) = (kurbo::Vec2::ZERO, 0.0_f64);
        for i in (0..self.inside.len()).filter(|&i| self.inside[i]) {
            sum += self.point(i).to_vec2();
            n += 1.0;
        }
        (sum / n.max(1.0)).to_point()
    }

    /// The regions of the part for labels that repeat each `distance`. The sites of the labels
    /// are cells at least `half` from the edge and `distance` apart, and each cell belongs to
    /// the nearest site.
    fn regions(&self, distance: f64, half: f64) -> Vec<Vec<usize>> {
        let mut seeds: Vec<Point> = Vec::new();
        for i in self.ranked() {
            if self.edge_distance(i) < half || seeds.len() == MAX_REPEATS {
                break;
            }
            let c = self.point(i);
            if seeds.iter().all(|s| s.distance(c) >= distance) {
                seeds.push(c);
            }
        }
        let mut regions = vec![Vec::new(); seeds.len()];
        for i in (0..self.inside.len()).filter(|&i| self.inside[i]) {
            let p = self.point(i);
            let near = (0..seeds.len()).min_by(|&a, &b| {
                seeds[a]
                    .distance_squared(p)
                    .total_cmp(&seeds[b].distance_squared(p))
            });
            if let Some(k) = near {
                regions[k].push(i);
            }
        }
        regions
    }

    fn point(&self, i: usize) -> Point {
        Point::new(
            self.origin.x + ((i % self.w) as f64 + 0.5) * self.cell,
            self.origin.y + ((i / self.w) as f64 + 0.5) * self.cell,
        )
    }

    fn index(&self, p: Point) -> Option<usize> {
        let i = ((p.x - self.origin.x) / self.cell).floor();
        let j = ((p.y - self.origin.y) / self.cell).floor();
        if i < 0.0 || j < 0.0 || i >= self.w as f64 || j >= self.h as f64 {
            return None;
        }
        Some(j as usize * self.w + i as usize)
    }

    /// The distance from the center of cell `i` to the edge, in page units.
    fn edge_distance(&self, i: usize) -> f64 {
        (self.dist[i] - 0.5).max(0.0) * self.cell
    }

    /// The width of the shape along the row of cell `i`, through the cell.
    fn row_width(&self, i: usize) -> f64 {
        let row = i - i % self.w;
        let left = (row..=i).rev().take_while(|&k| self.inside[k]).count();
        let right = (i..row + self.w).take_while(|&k| self.inside[k]).count();
        (left + right - 1) as f64 * self.cell
    }

    /// The cells by distance from the edge, farthest first. Of two cells at the same distance,
    /// the one nearer the centroid of the shape goes first.
    /// The distances are in whole cells, so the order does not change when the page moves.
    fn ranked(&self) -> Vec<usize> {
        let mut cells: Vec<usize> = (0..self.inside.len()).filter(|&i| self.inside[i]).collect();
        let w = self.w as i128;
        let n = cells.len() as i128;
        let (sx, sy) = cells
            .iter()
            .fold((0, 0), |(x, y), &i| (x + i as i128 % w, y + i as i128 / w));
        // The squared distance to the centroid, times the square of the number of cells.
        let d2 = |i: usize| {
            let (dx, dy) = (n * (i as i128 % w) - sx, n * (i as i128 / w) - sy);
            dx * dx + dy * dy
        };
        cells.sort_by(|&a, &b| {
            self.dist[b]
                .total_cmp(&self.dist[a])
                .then_with(|| d2(a).cmp(&d2(b)))
        });
        cells
    }

    /// The arc lengths where `curve` leaves the shape on each side of the arc length `at`, or
    /// comes nearer than `pad` to its edge.
    fn run(&self, curve: &Measured, at: f64, pad: f64) -> (f64, f64) {
        let step = 0.5 * self.cell;
        let len = curve.length();
        let reach = |dir: f64| {
            let mut s = at;
            while (0.0..=len).contains(&(s + dir * step))
                && self.holds(curve.point_at(s + dir * step), pad)
            {
                s += dir * step;
            }
            s
        };
        (reach(-1.0), reach(1.0))
    }

    /// The longest part of `curve` that is inside the shape and at least `pad` from its edge.
    /// It samples the curve at each half cell, but only where the curve is near the grid.
    fn longest_run(&self, curve: &Measured, pad: f64) -> Option<(f64, f64)> {
        let step = 0.5 * self.cell;
        let n = (curve.length() / step).floor() as usize;
        let mut best: Option<(f64, f64)> = None;
        for (first, last) in self.steps_near(curve, step, n) {
            let mut open: Option<f64> = None;
            // The step after `last` is off the grid or past the end, so it closes a run.
            for k in first..=last + 1 {
                let s = k as f64 * step;
                let inside = k <= n && self.holds(curve.point_at(s), pad);
                match (inside, open) {
                    (true, None) => open = Some(s),
                    (false, Some(a)) => {
                        let b = s - step;
                        if best.is_none_or(|(x, y)| b - a > y - x) {
                            best = Some((a, b));
                        }
                        open = None;
                    }
                    _ => {}
                }
            }
        }
        best.filter(|(a, b)| b > a)
    }

    /// The ranges of the steps `0..=n` of `step` along `curve` that are within one step of the
    /// grid, in order and apart. A step outside them is off the grid.
    fn steps_near(&self, curve: &Measured, step: f64, n: usize) -> Vec<(usize, usize)> {
        let size = kurbo::Size::new(self.w as f64 * self.cell, self.h as f64 * self.cell);
        let near = Rect::from_origin_size(self.origin, size).inflate(step, step);
        let mut out: Vec<(usize, usize)> = Vec::new();
        for i in 0..curve.pts.len().saturating_sub(1) {
            let (a, b) = (curve.pts[i], curve.pts[i + 1]);
            let Some((t0, t1)) = clip_segment(a, b, near) else {
                continue;
            };
            let len = curve.cum[i + 1] - curve.cum[i];
            let s0 = curve.cum[i] + t0 * len;
            let s1 = curve.cum[i] + t1 * len;
            let first = ((s0 / step).floor().max(0.0) as usize).saturating_sub(1);
            let last = ((s1 / step).ceil().max(0.0) as usize + 1).min(n);
            if first > last {
                continue;
            }
            match out.last_mut() {
                Some(prev) if first <= prev.1 + 1 => prev.1 = prev.1.max(last),
                _ => out.push((first, last)),
            }
        }
        out
    }

    /// True if `p` is in the shape and at least `pad` from its edge.
    fn holds(&self, p: Point, pad: f64) -> bool {
        self.index(p)
            .is_some_and(|i| self.inside[i] && self.edge_distance(i) >= pad)
    }

    /// True if ink at `p` keeps the distance `pad` from the edge, less the overrun.
    fn holds_ink(&self, p: Point, pad: f64) -> bool {
        let need = pad - self.overrun;
        match self.index(p) {
            Some(i) if self.inside[i] => self.edge_distance(i) >= need,
            Some(i) if !self.out_dist.is_empty() => {
                (self.out_dist[i] - 0.5).max(0.0) * self.cell <= -need
            }
            _ => false,
        }
    }

    /// A candidate with one box per glyph, or `None` if some ink leaves the shape.
    fn candidate(
        &self,
        shaped: &ShapedText,
        transforms: Vec<Affine>,
        margin: f64,
        pad: f64,
    ) -> Option<Candidate> {
        for ((_, g), tf) in shaped.glyphs().zip(&transforms) {
            let Some(bb) = g.bbox else { continue };
            for (u, v) in [0.0, 0.5, 1.0]
                .into_iter()
                .flat_map(|u| [0.0, 0.5, 1.0].map(|v| (u, v)))
            {
                let p = *tf * Point::new(bb.x0 + u * bb.width(), bb.y0 + v * bb.height());
                if !self.holds_ink(p, pad) {
                    return None;
                }
            }
        }
        let (boxes, bounds) = glyph_boxes(shaped, &transforms, margin);
        Some(Candidate {
            kind: CandidateKind::Polygon { curved: false },
            cost: 0.0,
            transforms,
            boxes,
            bounds,
            leader: None,
        })
    }

    /// The long axis through the cells at least `half` from the edge, smoothed and in reading
    /// direction. `None` if the shape is too small for a path.
    fn axis(&self, center: usize, half: f64) -> Option<Measured> {
        let open: Vec<bool> = (0..self.dist.len())
            .map(|i| self.inside[i] && self.edge_distance(i) >= half)
            .collect();
        let (from_center, _) = self.paths(&open, center, false);
        let a = farthest(&from_center)?;
        let (from_a, _) = self.paths(&open, a, false);
        let b = farthest(&from_a)?;
        let (_, parent) = self.paths(&open, a, true);
        let mut cells = vec![b];
        while let Some(&c) = cells.last()
            && c != a
        {
            cells.push(parent[c]? as usize);
        }
        let mut pts: Vec<Point> = cells.iter().map(|&c| self.point(c)).collect();
        // Left to right, or bottom to top if the axis is near vertical.
        let d = pts[pts.len() - 1] - pts[0];
        let reverse = if d.x.abs() >= 0.2 * d.hypot() {
            d.x < 0.0
        } else {
            d.y > 0.0
        };
        if reverse {
            pts.reverse();
        }
        let width = 2.0 * self.edge_distance(center);
        let m = Measured::new(smooth(&pts, self.cell, width));
        (m.pts.len() >= 2).then_some(m)
    }

    /// Shortest paths from `src` through `open` cells, with 8 neighbors. If `middle`, a step
    /// costs more near the edge.
    fn paths(&self, open: &[bool], src: usize, middle: bool) -> (Vec<f64>, Vec<Option<u32>>) {
        let n = open.len();
        let mut cost = vec![f64::INFINITY; n];
        let mut parent: Vec<Option<u32>> = vec![None; n];
        let mut heap = BinaryHeap::new();
        cost[src] = 0.0;
        heap.push(Reverse((0f64.to_bits(), src)));
        let dmax = self.dist[src].max(1.0);
        let w = self.w as isize;
        let steps = [
            (-1, 1.0),
            (1, 1.0),
            (-w, 1.0),
            (w, 1.0),
            (-w - 1, SQRT_2),
            (-w + 1, SQRT_2),
            (w - 1, SQRT_2),
            (w + 1, SQRT_2),
        ];
        while let Some(Reverse((bits, c))) = heap.pop() {
            let d = f64::from_bits(bits);
            if d > cost[c] {
                continue;
            }
            for (off, len) in steps {
                let nb = (c as isize + off) as usize;
                if !open[nb] {
                    continue;
                }
                let step = if middle {
                    len * (dmax / self.dist[nb]).powi(2)
                } else {
                    len
                };
                let nd = d + step;
                if nd < cost[nb] {
                    cost[nb] = nd;
                    parent[nb] = Some(c as u32);
                    heap.push(Reverse((nd.to_bits(), nb)));
                }
            }
        }
        (cost, parent)
    }
}

/// The distance from each cell in `mask` to the nearest cell outside it, in cells, by a
/// two-pass chamfer. The border of the grid keeps its start value.
fn chamfer(mask: &[bool], w: usize, h: usize) -> Vec<f64> {
    let mut dist: Vec<f64> = mask
        .iter()
        .map(|&i| if i { f64::INFINITY } else { 0.0 })
        .collect();
    let steps = [
        (-1, 0, 1.0),
        (-1, -1, SQRT_2),
        (0, -1, 1.0),
        (1, -1, SQRT_2),
    ];
    for j in 1..h - 1 {
        for i in 1..w - 1 {
            for (di, dj, d) in steps {
                let n = (j as isize + dj) as usize * w + (i as isize + di) as usize;
                dist[j * w + i] = dist[j * w + i].min(dist[n] + d);
            }
        }
    }
    for j in (1..h - 1).rev() {
        for i in (1..w - 1).rev() {
            for (di, dj, d) in steps {
                let n = (j as isize - dj) as usize * w + (i as isize - di) as usize;
                dist[j * w + i] = dist[j * w + i].min(dist[n] + d);
            }
        }
    }
    dist
}

/// The reached cell with the highest cost. The first cell wins a tie.
fn farthest(cost: &[f64]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, c) in cost.iter().enumerate() {
        if c.is_finite() && best.is_none_or(|b| *c > cost[b]) {
            best = Some(i);
        }
    }
    best
}

/// The part of the segment `a`-`b` inside `area`, as the parameters of its ends, by the
/// Liang-Barsky method.
fn clip_segment(a: Point, b: Point, area: Rect) -> Option<(f64, f64)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-d.x, a.x - area.x0),
        (d.x, area.x1 - a.x),
        (-d.y, a.y - area.y0),
        (d.y, area.y1 - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 <= t1).then_some((t0, t1))
}
