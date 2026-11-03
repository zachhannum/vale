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
use crate::model::{CandidateKind, LabelClass, PolygonMode, PolygonPlacement, UnplacedReason};
use crate::text::ShapedText;
use kurbo::{Affine, Point, Rect};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::f64::consts::{PI, SQRT_2};

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

/// The most horizontal positions.
const SITES: usize = 8;

const TURN_CAP: f64 = 1.6;
const OVER_TURN_COST: f64 = 20.0;
const OVER_TURN_WEIGHT: f64 = 10.0;

pub(crate) fn candidates(
    rings: &[Vec<Point>],
    class: &LabelClass,
    pp: &PolygonPlacement,
    shaped: &ShapedText,
    bounds: Rect,
    parallels: &Parallels,
) -> Result<Vec<Candidate>, UnplacedReason> {
    let size = class.style.size;
    let grid = Grid::new(rings, bounds, size).ok_or(UnplacedReason::OutOfBounds)?;
    let ranked = grid.ranked();
    let center = ranked[0];
    let dmax = grid.edge_distance(center);
    let half = 0.5 * shaped.cap_height + PAD * size;
    if dmax < half {
        return Err(UnplacedReason::AreaTooSmall);
    }
    let axis = grid.axis(center, half);
    let long = axis
        .as_ref()
        .is_some_and(|m| m.length() >= LONG * 2.0 * dmax);

    let mut out = Vec::new();
    if pp.mode == PolygonMode::Parallel {
        along_parallels(
            &grid, &ranked, parallels, class, pp, shaped, dmax, half, &mut out,
        );
    } else if pp.mode != PolygonMode::Horizontal
        && let Some(m) = &axis
    {
        let base = if long || pp.mode == PolygonMode::Curved {
            0.0
        } else {
            UNSUITED
        };
        curved(&grid, m, class, pp, shaped, base, &mut out);
    }
    let base = match pp.mode {
        PolygonMode::Auto if long => UNSUITED,
        PolygonMode::Curved | PolygonMode::Parallel => FALLBACK,
        _ => 0.0,
    };
    let follow = (pp.graticule || pp.mode == PolygonMode::Parallel).then_some(parallels);
    horizontal(
        &grid, &ranked, follow, class, pp, shaped, base, dmax, half, &mut out,
    );
    if out.is_empty() {
        return Err(UnplacedReason::AreaTooSmall);
    }
    Ok(out)
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

/// Horizontal candidates at the cells farthest from the edge, the visual center first. The
/// letters spread to fill a part of the width of the shape at each cell.
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
    let mut sites: Vec<Point> = Vec::new();
    for &i in ranked
        .iter()
        .take_while(|&&i| grid.edge_distance(i) >= half)
    {
        if sites.len() == SITES {
            break;
        }
        let c = grid.point(i);
        if sites.iter().any(|s| s.distance(c) < apart) {
            continue;
        }
        sites.push(c);
        let rank = sites.len() - 1;
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
/// costs least.
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
    runs.sort_by(|a, b| (b.2 - b.1).total_cmp(&(a.2 - a.1)));
    runs.truncate(LATITUDES);
    let Some(room) = runs.first().map(|r| r.2 - r.1) else {
        return;
    };

    let adv = shaped.advance;
    let gaps = shaped.glyph_count().saturating_sub(1);
    for (curve, lo, hi) in runs {
        let len = hi - lo;
        if len < adv {
            continue;
        }
        let room_cost = 3.0 * (1.0 - len / room);
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
                let cost = room_cost + 0.5 * fi as f64 + 0.5 * k.abs() + turns;
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

/// The visible part of a polygon as a grid of cells. A border of cells outside the shape
/// surrounds the grid.
struct Grid {
    origin: Point,
    cell: f64,
    w: usize,
    h: usize,
    /// Cells of the largest part.
    inside: Vec<bool>,
    /// The distance from each cell to the nearest cell outside, in cells.
    dist: Vec<f64>,
}

impl Grid {
    fn new(rings: &[Vec<Point>], bounds: Rect, size: f64) -> Option<Grid> {
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
        let w = (clip.width() / cell).ceil() as usize + 2;
        let h = (clip.height() / cell).ceil() as usize + 2;
        let origin = Point::new(clip.x0 - cell, clip.y0 - cell);

        // Even-odd scanlines through the cell centers.
        let mut fill = vec![false; w * h];
        let mut xs: Vec<f64> = Vec::new();
        for j in 1..h - 1 {
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

        // Keep the largest 4-connected part.
        let mut part = vec![0u32; w * h];
        let mut best = (0, 0usize);
        let mut stack = Vec::new();
        let mut id = 0;
        for s in 0..w * h {
            if !fill[s] || part[s] != 0 {
                continue;
            }
            id += 1;
            let mut count = 0;
            part[s] = id;
            stack.push(s);
            while let Some(c) = stack.pop() {
                count += 1;
                for n in [c - 1, c + 1, c - w, c + w] {
                    if fill[n] && part[n] == 0 {
                        part[n] = id;
                        stack.push(n);
                    }
                }
            }
            if count > best.1 {
                best = (id, count);
            }
        }
        if best.1 == 0 {
            return None;
        }
        let inside: Vec<bool> = part.iter().map(|&p| p == best.0).collect();

        // Two-pass chamfer distance.
        let mut dist: Vec<f64> = inside
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
        Some(Grid {
            origin,
            cell,
            w,
            h,
            inside,
            dist,
        })
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
    fn ranked(&self) -> Vec<usize> {
        let (mut sum, mut n) = (kurbo::Vec2::ZERO, 0.0_f64);
        for i in (0..self.inside.len()).filter(|&i| self.inside[i]) {
            sum += self.point(i).to_vec2();
            n += 1.0;
        }
        let centroid = (sum / n.max(1.0)).to_point();
        let mut cells: Vec<usize> = (0..self.inside.len()).filter(|&i| self.inside[i]).collect();
        cells.sort_by(|&a, &b| {
            self.dist[b].total_cmp(&self.dist[a]).then_with(|| {
                let da = self.point(a).distance_squared(centroid);
                da.total_cmp(&self.point(b).distance_squared(centroid))
            })
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
    fn longest_run(&self, curve: &Measured, pad: f64) -> Option<(f64, f64)> {
        let step = 0.5 * self.cell;
        let n = (curve.length() / step).floor() as usize;
        let mut best: Option<(f64, f64)> = None;
        let mut open: Option<f64> = None;
        for k in 0..=n + 1 {
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
        best.filter(|(a, b)| b > a)
    }

    /// True if `p` is in the shape and at least `pad` from its edge.
    fn holds(&self, p: Point, pad: f64) -> bool {
        self.index(p)
            .is_some_and(|i| self.inside[i] && self.edge_distance(i) >= pad)
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
                if !self.holds(p, pad) {
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
