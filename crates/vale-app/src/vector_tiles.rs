//! The render of one tile of a vector layer: a picture of a part of a cube
//! face, with the fills and the strokes of the layer.
//!
//! A tile reads only the parts of the rings and the lines near it. Each
//! ring and line splits into chunks of points with a cap around each chunk.
//! A stroke skips each chunk whose cap misses the tile. A fill puts the
//! chord from the first point to the last point of such a chunk in its
//! place: the region between the chunk and its chord is inside the cap, so
//! the fill inside the tile does not change.

use std::sync::Arc;

use kurbo::{Affine, Point};
use vale_render::{DisplayList, PreparedList, render_region};
use vale_store::{Geometry, GeometryKind, LonLat};
use vale_style::{Paint, StrokeKind, scale_denominator};
use vale_terrain::{face_dir, unwarp, warp};

use crate::globe::math::{V3, add, angle, dot, lonlat_to_dir, normalize, scale};
use crate::pipeline::{Batch, Plain, draw_plain};

/// The texels along one side of a tile, with no gutter.
pub const TILE: usize = 256;
/// The texels along one side of the picture of a tile: the tile and a
/// gutter of one texel on each side, so that a filter has no seam.
pub const TILE_PX: usize = TILE + 2;

/// The points of a chunk.
const CHUNK: usize = 64;
/// A chunk with a wider cap is always in each tile. A cap under a quarter
/// turn holds the arcs between its points.
const WIDE_CHUNK: f64 = 0.5;
/// The side of the face plane that a tile reads: a point closer to the
/// face than this does not project.
const FACE_SIDE: f64 = 0.2;
/// A point closer than this to the last point, in texels, goes.
const MIN_STEP: f64 = 0.25;
/// A longer step in texels is cut into steps of this length, because the
/// warp of the face bends the straight lines of the flat face.
const MAX_STEP: f64 = 32.0;

/// A tile of a level: its face, and its column and row among the `2^level`
/// tiles along each side of the face, in the coordinates of `place` in
/// `field.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub face: u8,
    pub level: u8,
    pub col: u32,
    pub row: u32,
}

/// The paints of a layer at one level: the batches of
/// `pipeline::style_batches` at the scale of the level.
pub struct TilePaints {
    pub(crate) batches: Vec<Batch>,
    pub pixels_per_point: f32,
}

/// True if tiles draw each paint of `batches`: fills, and solid strokes.
/// Each tile cuts its own pieces of a line, so a dash pattern would not
/// continue from one tile to the next.
pub(crate) fn tiles_draw(batches: &[Batch]) -> bool {
    batches.iter().all(|b| match &b.paint {
        Paint::Fill(_) => true,
        Paint::Stroke(s) => s.kind == StrokeKind::Solid,
        _ => false,
    })
}

/// The angle of one texel of `level`.
pub fn texel_angle(level: u8) -> f64 {
    std::f64::consts::FRAC_PI_2 / (TILE as f64 * f64::from(1u32 << level))
}

/// The scale denominator of `level` on a world of radius `radius_m`, where
/// one texel is one pixel of a screen with `pixels_per_point`.
pub fn level_scale(level: u8, radius_m: f64, pixels_per_point: f32) -> f64 {
    scale_denominator(radius_m * texel_angle(level) * f64::from(pixels_per_point))
}

/// A run of points of a path and the cap around them. The run ends at the
/// first point of the next chunk.
struct Chunk {
    start: usize,
    center: V3,
    radius: f64,
}

/// A ring or a line of a feature.
struct Path {
    feature: usize,
    /// The polygon and the ring of a polygon feature, or the line of a line
    /// feature.
    part: (usize, usize),
    closed: bool,
    chunks: std::ops::Range<usize>,
}

/// A path that a snapshot finds: its feature, its part, whether it closes,
/// and its points.
type Found<'a> = (usize, (usize, usize), bool, &'a [LonLat]);

/// The geometry of a layer with the chunks of its paths.
pub struct Snapshot {
    /// The geometry of each feature. A snapshot shares the geometry of a
    /// feature that did not change with the snapshot before it.
    geometries: Arc<Vec<Arc<Geometry>>>,
    paths: Vec<Path>,
    chunks: Vec<Chunk>,
    /// The kind of the layer.
    kind: Option<GeometryKind>,
}

impl Snapshot {
    /// The snapshot of `geometries`. The chunks build on all cores.
    pub fn new(geometries: Arc<Vec<Arc<Geometry>>>) -> Snapshot {
        let mut found: Vec<Found<'_>> = Vec::new();
        let mut kind = None;
        for (i, geometry) in geometries.iter().enumerate() {
            match &**geometry {
                Geometry::Points(_) => {}
                Geometry::Lines(lines) => {
                    kind.get_or_insert(GeometryKind::Line);
                    for (l, line) in lines.iter().enumerate() {
                        found.push((i, (l, 0), false, line));
                    }
                }
                Geometry::Polygons(polygons) => {
                    kind.get_or_insert(GeometryKind::Polygon);
                    for (p, rings) in polygons.iter().enumerate() {
                        for (r, ring) in rings.iter().enumerate() {
                            found.push((i, (p, r), true, ring));
                        }
                    }
                }
            }
        }
        found.retain(|f| f.3.len() >= 2);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let total: usize = found.iter().map(|f| f.3.len()).sum();
        // Runs of paths with about the same number of points.
        let mut runs: Vec<&[Found<'_>]> = Vec::new();
        let (mut start, mut sum) = (0, 0);
        for (k, f) in found.iter().enumerate() {
            sum += f.3.len();
            if sum * threads >= total * (runs.len() + 1) {
                runs.push(&found[start..=k]);
                start = k + 1;
            }
        }
        runs.push(&found[start..]);
        let parts: Vec<Vec<Vec<Chunk>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (runs.iter())
                .map(|run| scope.spawn(move || run.iter().map(|f| path_chunks(f.3)).collect()))
                .collect();
            (handles.into_iter())
                .map(|h| h.join().unwrap_or_default())
                .collect()
        });
        let mut paths = Vec::with_capacity(found.len());
        let mut chunks = Vec::new();
        for (f, path) in found.iter().zip(parts.into_iter().flatten()) {
            let first = chunks.len();
            chunks.extend(path);
            paths.push(Path {
                feature: f.0,
                part: f.1,
                closed: f.2,
                chunks: first..chunks.len(),
            });
        }
        Snapshot {
            geometries: geometries.clone(),
            paths,
            chunks,
            kind,
        }
    }

    /// The caps around the points that differ from `old`, in this snapshot
    /// and in `old`. A ring or a line that changed gives the caps of the
    /// chunks between its first and its last changed point. `None`: the
    /// features are not the same list, or too many points changed.
    pub fn changed_caps(&self, old: &Snapshot) -> Option<Vec<(V3, f64)>> {
        if self.geometries.len() != old.geometries.len() {
            return None;
        }
        let mut caps = Vec::new();
        for (i, (a, b)) in self
            .geometries
            .iter()
            .zip(old.geometries.iter())
            .enumerate()
        {
            if Arc::ptr_eq(a, b) || a == b {
                continue;
            }
            let (new_paths, old_paths) = (self.feature_paths(i), old.feature_paths(i));
            if new_paths.len() != old_paths.len() {
                for (snap, paths) in [(self, new_paths), (old, old_paths)] {
                    for path in paths {
                        caps.extend(snap.caps_between(path, 0, usize::MAX));
                    }
                }
                continue;
            }
            for (n, o) in new_paths.iter().zip(old_paths) {
                let (pn, po) = (self.points(n), old.points(o));
                if pn == po {
                    continue;
                }
                let head = pn.iter().zip(po).take_while(|(x, y)| x == y).count();
                let tail = (pn.iter().rev().zip(po.iter().rev()))
                    .take_while(|(x, y)| x == y)
                    .count();
                let tail = tail.min(pn.len().min(po.len()) - head.min(pn.len().min(po.len())));
                caps.extend(self.caps_between(n, head, pn.len() - tail));
                caps.extend(old.caps_between(o, head, po.len() - tail));
            }
            if caps.len() > 16_384 {
                return None;
            }
        }
        Some(caps)
    }

    /// The paths of feature `i`.
    fn feature_paths(&self, i: usize) -> &[Path] {
        let start = self.paths.partition_point(|p| p.feature < i);
        let end = self.paths.partition_point(|p| p.feature <= i);
        &self.paths[start..end]
    }

    /// The caps of the chunks of `path` that hold a point from `from` to
    /// before `to`, and the chunk before, which ends at `from`.
    fn caps_between(&self, path: &Path, from: usize, to: usize) -> impl Iterator<Item = (V3, f64)> {
        let chunks = &self.chunks[path.chunks.clone()];
        (0..chunks.len())
            .filter(move |&k| {
                let end = chunks.get(k + 1).map_or(usize::MAX, |c| c.start);
                chunks[k].start <= to && end >= from
            })
            .map(move |k| (chunks[k].center, chunks[k].radius))
    }

    fn points(&self, path: &Path) -> &[LonLat] {
        match &*self.geometries[path.feature] {
            Geometry::Lines(lines) => &lines[path.part.0],
            Geometry::Polygons(polygons) => &polygons[path.part.0][path.part.1],
            Geometry::Points(_) => &[],
        }
    }
}

/// The middle of the picture of tile `key` and the angle from it to the
/// farthest corner.
pub fn tile_cap(key: TileKey) -> (V3, f64) {
    let frame = Frame::new(key);
    (frame.center, frame.radius)
}

/// The chunks of a path of two points or more.
fn path_chunks(points: &[LonLat]) -> Vec<Chunk> {
    let mut chunks = Vec::with_capacity(points.len() / CHUNK + 1);
    let mut start = 0;
    while start < points.len() - 1 {
        let end = (start + CHUNK).min(points.len() - 1);
        let dirs: Vec<V3> = (points[start..=end].iter())
            .map(|p| lonlat_to_dir(p[0], p[1]))
            .collect();
        let center = normalize(dirs.iter().fold([0.0; 3], |a, d| add(a, *d)));
        let radius = dirs.iter().map(|d| angle(center, *d)).fold(0.0, f64::max);
        chunks.push(Chunk {
            start,
            center,
            radius: if radius.is_finite() { radius } else { 4.0 },
        });
        start = end;
    }
    chunks
}

/// The place of a tile on its face.
struct Frame {
    key: TileKey,
    axis: usize,
    sign: f64,
    normal: V3,
    center: V3,
    radius: f64,
}

impl Frame {
    fn new(key: TileKey) -> Frame {
        let face = usize::from(key.face);
        let axis = face / 2;
        let sign = if face % 2 == 0 { 1.0 } else { -1.0 };
        let mut normal = [0.0; 3];
        normal[axis] = sign;
        let side = f64::from(1u32 << key.level);
        let at = |c: f64| unwarp(c / side * 2.0 - 1.0);
        let corner = |c: u32, r: u32| face_dir(face, at(f64::from(c)), at(f64::from(r)));
        let center = face_dir(
            face,
            at(f64::from(key.col) + 0.5),
            at(f64::from(key.row) + 0.5),
        );
        let radius = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .iter()
            .map(|&(c, r)| angle(center, corner(key.col + c, key.row + r)))
            .fold(0.0, f64::max);
        Frame {
            key,
            axis,
            sign,
            normal,
            center,
            radius,
        }
    }

    /// The point of the flat face under `d`.
    fn flat(&self, d: V3) -> [f64; 2] {
        let w = d[self.axis] * self.sign;
        [d[(self.axis + 1) % 3] / w, d[(self.axis + 2) % 3] / w]
    }

    /// The pixel of the picture at a point of the flat face.
    fn pixel(&self, [a, b]: [f64; 2]) -> Point {
        let side = f64::from(1u32 << self.key.level);
        let x = ((warp(a) * 0.5 + 0.5) * side - f64::from(self.key.col)) * TILE as f64 + 1.0;
        let y = ((warp(b) * 0.5 + 0.5) * side - f64::from(self.key.row)) * TILE as f64 + 1.0;
        Point::new(x, y)
    }

    /// The flat face inside the picture and `margin` texels around it.
    fn flat_rect(&self, margin: f64) -> [f64; 4] {
        let side = f64::from(1u32 << self.key.level);
        let edge = |c: f64| unwarp(((c / side) * 2.0 - 1.0).clamp(-1.5, 1.5));
        let pad = (1.0 + margin) / TILE as f64;
        [
            edge(f64::from(self.key.col) - pad),
            edge(f64::from(self.key.row) - pad),
            edge(f64::from(self.key.col) + 1.0 + pad),
            edge(f64::from(self.key.row) + 1.0 + pad),
        ]
    }
}

/// The points of `path` near the tile, as directions. With `fill`, the
/// chunks far from the tile give their first point, and the result is one
/// ring. Else each run of near chunks is one line.
fn near_points(
    snap: &Snapshot,
    path: &Path,
    frame: &Frame,
    reach: f64,
    fill: bool,
) -> Vec<Vec<V3>> {
    let points = snap.points(path);
    let chunks = &snap.chunks[path.chunks.clone()];
    let dir = |i: usize| lonlat_to_dir(points[i][0], points[i][1]);
    let mut out: Vec<Vec<V3>> = Vec::new();
    let mut run: Vec<V3> = Vec::new();
    for (k, chunk) in chunks.iter().enumerate() {
        let end = chunks.get(k + 1).map_or(points.len() - 1, |c| c.start);
        let near = chunk.radius > WIDE_CHUNK
            || angle(chunk.center, frame.center) <= chunk.radius + frame.radius + reach;
        if near {
            run.extend((chunk.start..end).map(dir));
        } else if fill {
            run.push(dir(chunk.start));
        } else if !run.is_empty() {
            run.push(dir(chunk.start));
            out.push(std::mem::take(&mut run));
        }
    }
    let last = points.len() - 1;
    if fill {
        if !path.closed {
            run.push(dir(last));
        }
        out.push(run);
    } else if !run.is_empty() {
        run.push(dir(last));
        out.push(run);
    }
    out
}

/// The part of a ring on the side of the face plane that projects.
fn clip_ring_3d(ring: &[V3], frame: &Frame) -> Vec<V3> {
    let side = |p: V3| dot(p, frame.normal) - FACE_SIDE;
    let mut out = Vec::with_capacity(ring.len());
    for (i, &p) in ring.iter().enumerate() {
        let q = ring[(i + 1) % ring.len()];
        let (sp, sq) = (side(p), side(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push(add(scale(p, 1.0 - t), scale(q, t)));
        }
    }
    out
}

/// The runs of a line on the side of the face plane that projects.
fn clip_line_3d(line: &[V3], frame: &Frame) -> Vec<Vec<V3>> {
    let side = |p: V3| dot(p, frame.normal) - FACE_SIDE;
    let mut out = Vec::new();
    let mut run = Vec::new();
    for (i, &p) in line.iter().enumerate() {
        let sp = side(p);
        if i > 0 {
            let o = line[i - 1];
            let so = side(o);
            if (so >= 0.0) != (sp >= 0.0) {
                let t = so / (so - sp);
                run.push(add(scale(o, 1.0 - t), scale(p, t)));
                if sp < 0.0 {
                    out.push(std::mem::take(&mut run));
                }
            }
        }
        if sp >= 0.0 {
            run.push(p);
        }
    }
    if run.len() > 1 {
        out.push(run);
    }
    out.retain(|r: &Vec<V3>| r.len() > 1);
    out
}

/// A ring cut to a rectangle, by Sutherland and Hodgman.
fn clip_ring_2d(ring: Vec<[f64; 2]>, [x0, y0, x1, y1]: [f64; 4]) -> Vec<[f64; 2]> {
    let mut ring = ring;
    let edges: [(usize, f64, bool); 4] =
        [(0, x0, true), (0, x1, false), (1, y0, true), (1, y1, false)];
    for (axis, at, low) in edges {
        if ring.is_empty() {
            break;
        }
        let inside = |p: &[f64; 2]| if low { p[axis] >= at } else { p[axis] <= at };
        let mut out = Vec::with_capacity(ring.len());
        for (i, p) in ring.iter().enumerate() {
            let q = &ring[(i + 1) % ring.len()];
            if inside(p) {
                out.push(*p);
            }
            if inside(p) != inside(q) {
                let t = (at - p[axis]) / (q[axis] - p[axis]);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        ring = out;
    }
    ring
}

/// The runs of a line inside a rectangle.
fn clip_line_2d(line: &[[f64; 2]], [x0, y0, x1, y1]: [f64; 4]) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
    for w in line.windows(2) {
        let (p, q) = (w[0], w[1]);
        // Liang and Barsky.
        let d = [q[0] - p[0], q[1] - p[1]];
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut keep = true;
        for (num, den) in [
            (p[0] - x0, -d[0]),
            (x1 - p[0], d[0]),
            (p[1] - y0, -d[1]),
            (y1 - p[1], d[1]),
        ] {
            if den == 0.0 {
                if num < 0.0 {
                    keep = false;
                }
            } else {
                let t = num / den;
                if den < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !keep || t0 > t1 {
            if run.len() > 1 {
                out.push(std::mem::take(&mut run));
            }
            run.clear();
            continue;
        }
        let at = |t: f64| [p[0] + d[0] * t, p[1] + d[1] * t];
        if run.is_empty() || t0 > 0.0 {
            if run.len() > 1 {
                out.push(std::mem::take(&mut run));
            }
            run.clear();
            run.push(at(t0));
        }
        run.push(at(t1));
        if t1 < 1.0 {
            out.push(std::mem::take(&mut run));
        }
    }
    if run.len() > 1 {
        out.push(run);
    }
    out
}

/// The pixels of a path of the flat face: long steps cut into short ones,
/// and points too close to the last one left out.
fn to_pixels(frame: &Frame, flat: &[[f64; 2]], closed: bool) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(flat.len());
    let mut push = |p: Point| {
        if out.last().is_none_or(|l| l.distance(p) >= MIN_STEP) {
            out.push(p);
        }
    };
    let count = flat.len() + usize::from(closed);
    for i in 0..count {
        let q = flat[i % flat.len()];
        if i > 0 {
            let p = flat[i - 1];
            let (pp, pq) = (frame.pixel(p), frame.pixel(q));
            let steps = (pp.distance(pq) / MAX_STEP).ceil() as usize;
            for s in 1..steps {
                let t = s as f64 / steps as f64;
                push(frame.pixel([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]));
            }
        }
        push(frame.pixel(q));
    }
    out
}

/// The rings of each feature in the pixels of the tile, for the fills.
fn fill_rings(snap: &Snapshot, frame: &Frame, wanted: &[bool]) -> Vec<Vec<Vec<Point>>> {
    let rect = frame.flat_rect(2.0);
    let mut out: Vec<Vec<Vec<Point>>> = vec![Vec::new(); snap.geometries.len()];
    for path in snap.paths.iter().filter(|p| p.closed && wanted[p.feature]) {
        for ring in near_points(snap, path, frame, 0.0, true) {
            let ring = clip_ring_3d(&ring, frame);
            if ring.len() < 3 {
                continue;
            }
            let flat: Vec<[f64; 2]> = ring.iter().map(|d| frame.flat(*d)).collect();
            let flat = clip_ring_2d(flat, rect);
            if flat.len() < 3 {
                continue;
            }
            let pixels = to_pixels(frame, &flat, false);
            if pixels.len() >= 3 {
                out[path.feature].push(pixels);
            }
        }
    }
    out
}

/// The lines of each feature in the pixels of the tile, for the strokes:
/// the lines of a line feature and the rings of a polygon feature. `margin`
/// is half the widest stroke, in texels.
fn stroke_lines(
    snap: &Snapshot,
    frame: &Frame,
    wanted: &[bool],
    margin: f64,
) -> Vec<Vec<Vec<Point>>> {
    let rect = frame.flat_rect(margin + 2.0);
    let reach = (margin + 2.0) * texel_angle(frame.key.level);
    let mut out: Vec<Vec<Vec<Point>>> = vec![Vec::new(); snap.geometries.len()];
    for path in snap.paths.iter().filter(|p| wanted[p.feature]) {
        for mut run in near_points(snap, path, frame, reach, false) {
            // A ring whose chunks are all near closes.
            if path.closed && run.len() == snap.points(path).len() {
                run.push(run[0]);
            }
            for part in clip_line_3d(&run, frame) {
                let flat: Vec<[f64; 2]> = part.iter().map(|d| frame.flat(*d)).collect();
                for piece in clip_line_2d(&flat, rect) {
                    let pixels = to_pixels(frame, &piece, false);
                    if pixels.len() >= 2 {
                        out[path.feature].push(pixels);
                    }
                }
            }
        }
    }
    out
}

/// The picture of tile `key`: `TILE_PX` by `TILE_PX` premultiplied RGBA8
/// pixels, rows from the low edge of `v`.
pub fn render_tile(snap: &Snapshot, paints: &TilePaints, key: TileKey) -> Vec<u8> {
    let blank = || vec![0; TILE_PX * TILE_PX * 4];
    let Some(kind) = snap.kind else {
        return blank();
    };
    let ppp = f64::from(paints.pixels_per_point);
    let frame = Frame::new(key);
    let plains: Vec<(Option<Plain>, &Batch)> = (paints.batches.iter())
        .map(|b| (Plain::of(kind, &b.paint).map(|p| in_pixels(p, ppp)), b))
        .collect();
    let count = snap.geometries.len();
    let mut fills = vec![false; count];
    let mut strokes = vec![false; count];
    let mut margin: f64 = 0.0;
    for (plain, batch) in &plains {
        match plain {
            Some(Plain::Fill(_)) => batch.features.iter().for_each(|&i| fills[i] = true),
            Some(Plain::Stroke { width, .. }) => {
                margin = margin.max(width / 2.0);
                batch.features.iter().for_each(|&i| strokes[i] = true);
            }
            None => {}
        }
    }
    let rings = fill_rings(snap, &frame, &fills);
    let lines = stroke_lines(snap, &frame, &strokes, margin);
    let mut list = DisplayList::new(TILE_PX as f64, TILE_PX as f64);
    for (plain, batch) in &plains {
        let Some(plain) = plain else {
            continue;
        };
        let rings = batch.features.iter().map(|&i| rings[i].as_slice());
        let lines = batch.features.iter().map(|&i| lines[i].as_slice());
        draw_plain(&mut list, plain, rings, lines);
    }
    if list.items.is_empty() {
        return blank();
    }
    let side = TILE_PX as u32;
    let prepared = PreparedList::new(&list);
    match prepared.and_then(|p| render_region(&p, Affine::IDENTITY, side, side)) {
        Ok((rgba, _)) => rgba.data,
        Err(_) => blank(),
    }
}

/// A plain paint with its widths in texels.
fn in_pixels(plain: Plain, ppp: f64) -> Plain {
    match plain {
        Plain::Fill(color) => Plain::Fill(color),
        Plain::Stroke {
            color,
            width,
            round,
            dash,
        } => Plain::Stroke {
            color,
            width: width * ppp,
            round,
            dash,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::vtiles::tile_at;

    fn square(lon: f64, lat: f64, size: f64, steps: usize) -> Vec<LonLat> {
        let at = |t: usize| t as f64 * size / steps as f64;
        let mut ring = Vec::new();
        ring.extend((0..steps).map(|t| [lon + at(t), lat]));
        ring.extend((0..steps).map(|t| [lon + size, lat + at(t)]));
        ring.extend((0..steps).map(|t| [lon + size - at(t), lat + size]));
        ring.extend((0..=steps).map(|t| [lon, lat + size - at(t)]));
        ring
    }

    fn red_fill() -> TilePaints {
        TilePaints {
            batches: vec![Batch {
                order: (0, 0),
                paint: Paint::Fill(vale_render::Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
                features: vec![0],
                masked: false,
            }],
            pixels_per_point: 1.0,
        }
    }

    fn stroke(width: f64) -> TilePaints {
        TilePaints {
            batches: vec![Batch {
                order: (0, 0),
                paint: Paint::Stroke(vale_style::Stroke::solid(
                    vale_render::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    width,
                )),
                features: vec![0],
                masked: false,
            }],
            pixels_per_point: 1.0,
        }
    }

    fn alpha(rgba: &[u8], x: usize, y: usize) -> u8 {
        rgba[(y * TILE_PX + x) * 4 + 3]
    }

    /// The pixel of the picture of `key` under a longitude and a latitude.
    fn pixel_at(key: TileKey, lon: f64, lat: f64) -> (usize, usize) {
        let frame = Frame::new(key);
        let p = frame.pixel(frame.flat(lonlat_to_dir(lon, lat)));
        assert!(p.x >= 0.0 && p.y >= 0.0, "{p:?}");
        (p.x as usize, p.y as usize)
    }

    fn snap(geometry: Geometry) -> Snapshot {
        Snapshot::new(Arc::new(vec![Arc::new(geometry)]))
    }

    #[test]
    fn a_square_fills_the_texels_inside_it() {
        let s = snap(Geometry::Polygons(vec![vec![square(10.0, 10.0, 10.0, 50)]]));
        let key = tile_at(lonlat_to_dir(15.0, 15.0), 3);
        let rgba = render_tile(&s, &red_fill(), key);
        let (x, y) = pixel_at(key, 15.0, 15.0);
        assert_eq!(alpha(&rgba, x, y), 255);
        // The tile reaches 3.75 degrees around the middle of the square.
        let (x, y) = pixel_at(key, 19.0, 15.0);
        assert_eq!(alpha(&rgba, x, y), 255);
        let key = tile_at(lonlat_to_dir(22.0, 15.0), 3);
        let rgba = render_tile(&s, &red_fill(), key);
        let (x, y) = pixel_at(key, 22.0, 15.0);
        assert_eq!(alpha(&rgba, x, y), 0);
    }

    #[test]
    fn a_hole_is_clear() {
        let outer = square(0.0, 0.0, 20.0, 50);
        let mut hole = square(5.0, 5.0, 10.0, 50);
        hole.reverse();
        let s = snap(Geometry::Polygons(vec![vec![outer, hole]]));
        let key = tile_at(lonlat_to_dir(10.0, 10.0), 2);
        let rgba = render_tile(&s, &red_fill(), key);
        let (x, y) = pixel_at(key, 10.0, 10.0);
        assert_eq!(alpha(&rgba, x, y), 0);
        let (x, y) = pixel_at(key, 2.0, 10.0);
        assert_eq!(alpha(&rgba, x, y), 255);
    }

    #[test]
    fn a_ring_across_a_face_edge_fills_both_faces() {
        // The edge between two faces is at 45 degrees of longitude.
        let s = snap(Geometry::Polygons(vec![vec![square(40.0, -5.0, 10.0, 80)]]));
        for lon in [42.0, 48.0] {
            let key = tile_at(lonlat_to_dir(lon, 0.0), 2);
            let rgba = render_tile(&s, &red_fill(), key);
            let (x, y) = pixel_at(key, lon, 0.0);
            assert_eq!(alpha(&rgba, x, y), 255, "{lon}");
        }
    }

    #[test]
    fn a_far_tile_reads_only_the_chords_of_a_large_ring() {
        // A ring of many points around a tile far from its points fills
        // the whole tile.
        let s = snap(Geometry::Polygons(vec![vec![square(
            -40.0, -40.0, 80.0, 4000,
        )]]));
        let key = tile_at(lonlat_to_dir(0.0, 0.0), 6);
        let rgba = render_tile(&s, &red_fill(), key);
        assert!((0..TILE_PX).all(|i| alpha(&rgba, i, i) == 255));
    }

    #[test]
    fn a_tile_with_chunks_left_out_matches_one_with_all() {
        let mut ring = Vec::new();
        for i in 0..2000 {
            let t = i as f64 / 2000.0 * std::f64::consts::TAU;
            let r = 10.0 + 3.0 * (7.0 * t).sin();
            ring.push([r * t.cos(), r * t.sin()]);
        }
        ring.push(ring[0]);
        let s = snap(Geometry::Polygons(vec![vec![ring]]));
        let key = tile_at(lonlat_to_dir(10.0, 0.0), 4);
        let culled = render_tile(&s, &red_fill(), key);
        let all = Snapshot {
            chunks: s
                .chunks
                .iter()
                .map(|c| Chunk {
                    start: c.start,
                    center: c.center,
                    radius: 1.0,
                })
                .collect(),
            paths: s
                .paths
                .iter()
                .map(|p| Path {
                    feature: p.feature,
                    part: p.part,
                    closed: p.closed,
                    chunks: p.chunks.clone(),
                })
                .collect(),
            geometries: s.geometries.clone(),
            kind: s.kind,
        };
        let full = render_tile(&all, &red_fill(), key);
        let off = culled
            .iter()
            .zip(&full)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert!(off < 20, "{off}");
    }

    #[test]
    fn a_stroke_is_as_wide_as_its_width_in_texels() {
        let line = vec![[10.0, -5.0], [10.0, 5.0]];
        let s = snap(Geometry::Lines(vec![line]));
        let key = tile_at(lonlat_to_dir(10.0, 0.0), 5);
        let rgba = render_tile(&s, &stroke(6.0), key);
        let (x, y) = pixel_at(key, 10.0, 0.0);
        let covered = (0..TILE_PX).filter(|&i| alpha(&rgba, i, y) > 127).count();
        assert!((5..=7).contains(&covered), "{covered}");
        assert!(alpha(&rgba, x, y) > 200);
    }

    #[test]
    fn tiles_draw_no_marker() {
        let batch = |paint| Batch {
            order: (0, 0),
            paint,
            features: vec![0],
            masked: false,
        };
        assert!(tiles_draw(&red_fill().batches));
        let marker = Paint::Marker {
            shape: vale_style::MarkerShape::Circle,
            radius: 4.0,
            fill: None,
            hatch: None,
            stroke: None,
        };
        assert!(!tiles_draw(&[batch(marker)]));
        let stroke = |kind| {
            Paint::Stroke(vale_style::Stroke {
                color: vale_style::Color::rgb(0, 0, 0),
                width: 1.0,
                kind,
            })
        };
        assert!(tiles_draw(&[batch(stroke(StrokeKind::Solid))]));
        assert!(!tiles_draw(&[batch(stroke(StrokeKind::dashed()))]));
    }
}
