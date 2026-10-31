//! The border band around a map frame, its stroke, and the labels of its
//! reference grid.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vale_labeler::Fonts;
use vale_render::{FillRule, Item};
use vale_sphere::{Projection, graticule_in};
use vale_store::{BorderStyle, FrameRecord, GridColumns};

use crate::atlas::{FrameTarget, PT_PER_MM, frame_transform, interval_at_scale};
use crate::edge_labels::{inside, target_rings};
use crate::furniture::{Painted, TextInk, rgb};
use crate::pipeline::{frame_area, graticule_window};

/// The inner and the outer edge of the band, in target points. The inner
/// edge is the bounding box of the shape, moved out by the distance of each
/// side. A side that moves past the opposite side stops at the middle.
pub fn band_rects(frame: &FrameRecord, target: FrameTarget) -> (Rect, Rect) {
    let b = &frame.border;
    let area = frame_area(frame, target);
    let [top, right, bottom, left] = b.distances_mm.map(|d| d * target.pts_per_mm);
    let (mut x0, mut x1) = (area.x0 - left, area.x1 + right);
    let (mut y0, mut y1) = (area.y0 - top, area.y1 + bottom);
    if x0 > x1 {
        (x0, x1) = ((x0 + x1) / 2.0, (x0 + x1) / 2.0);
    }
    if y0 > y1 {
        (y0, y1) = ((y0 + y1) / 2.0, (y0 + y1) / 2.0);
    }
    let inner = Rect::new(x0, y0, x1, y1);
    let w = b.width_mm * target.pts_per_mm;
    (inner, inner.inflate(w, w))
}

/// How far the band, its stroke, and the grid labels reach out of the
/// bounding box of the shape, in page points. 0 when the border is off.
pub fn reach(frame: &FrameRecord) -> f64 {
    let b = &frame.border;
    if !b.on {
        return 0.0;
    }
    let distance = b.distances_mm.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let band = ((distance + b.width_mm) * PT_PER_MM).max(0.0);
    let stroke = if b.stroke { b.stroke_width / 2.0 } else { 0.0 };
    let labels = if b.grid { b.grid_text.size } else { 0.0 };
    band + stroke.max(labels)
}

/// The name of column `i`, from 0: A to Z, then AA, AB.
pub fn column_name(i: u32) -> String {
    let mut name = Vec::new();
    let mut n = i + 1;
    while n > 0 {
        n -= 1;
        name.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

/// The band, its stroke, and the grid labels of a frame, in target points.
/// The stroke of a graduated band also goes across the band at each change
/// of color.
pub fn items(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let b = &frame.border;
    if !b.on {
        return Vec::new();
    }
    let (inner, outer) = band_rects(frame, target);
    // The shape covers the band where it crosses it, with a gap.
    let shape = target_rings(frame, target);
    let rings = grown(&shape, b.gap_mm * target.pts_per_mm);
    let reach = rings.iter().flatten().fold(outer, |r, p| r.union_pt(*p));
    let mut clip = reach.inflate(1.0, 1.0).to_path(0.1);
    clip.extend(shape_path(&rings));
    let mut out = vec![Item::PushClip {
        path: clip,
        rule: FillRule::EvenOdd,
    }];
    let mut edges = outer.to_path(0.1);
    edges.extend(inner.to_path(0.1));
    let lines = Lines::new(projection, frame, target, outer);
    match b.style {
        BorderStyle::Solid => out.push(Item::Fill {
            path: edges.clone(),
            color: rgb(b.color),
            rule: FillRule::EvenOdd,
        }),
        BorderStyle::Graduated => {
            for side in sides(inner, outer) {
                let pieces = lines.pieces(&side, None);
                // A line across the band where two pieces meet.
                for piece in &pieces[..pieces.len().saturating_sub(1)] {
                    let r = piece.rect;
                    let (from, to) = if side.horizontal {
                        ((r.x1, r.y0), (r.x1, r.y1))
                    } else {
                        ((r.x0, r.y1), (r.x1, r.y1))
                    };
                    edges.move_to(from);
                    edges.line_to(to);
                }
                for piece in pieces {
                    let first = piece.cell.is_none_or(|[x, y]| (x + y).rem_euclid(2) == 0);
                    out.push(Item::Fill {
                        path: piece.rect.to_path(0.1),
                        color: rgb(if first { b.color } else { b.color2 }),
                        rule: FillRule::NonZero,
                    });
                }
            }
        }
    }
    let unit = target.pts_per_mm / PT_PER_MM;
    if b.stroke {
        out.push(Item::Stroke {
            path: edges,
            color: rgb(b.stroke_color),
            width: b.stroke_width * unit,
            round: false,
        });
    }
    out.push(Item::PopClip);
    if b.grid {
        let area = frame_area(frame, target);
        let labels = grid_labels(
            frame,
            &lines,
            (inner, outer, area),
            unit,
            (&shape, &rings),
            fonts,
        );
        out.extend(labels.into_items());
    }
    out
}

/// The rings of a shape, grown by `d` target points: the outer ring moves
/// out, and each hole moves in.
fn grown(rings: &[Vec<Point>], d: f64) -> Vec<Vec<Point>> {
    if d <= 0.0 {
        return rings.to_vec();
    }
    let area = |ring: &[Point]| {
        let n = ring.len();
        (0..n)
            .map(|i| ring[i].to_vec2().cross(ring[(i + 1) % n].to_vec2()))
            .sum::<f64>()
            .abs()
    };
    (rings.iter().enumerate())
        .map(|(i, ring)| {
            let (a, b) = (offset_ring(ring, d), offset_ring(ring, -d));
            let a_larger = area(&a) > area(&b);
            if a_larger == (i == 0) { a } else { b }
        })
        .collect()
}

/// `ring` with each edge moved by `d` along its normal, with mitered
/// corners. A miter is at most 4 times `d` long.
fn offset_ring(ring: &[Point], d: f64) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::new();
    for p in ring {
        if pts.last().is_none_or(|q| q.distance(*p) > 1e-9) {
            pts.push(*p);
        }
    }
    while pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= 1e-9 {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    let normal = |v: Vec2| Vec2::new(v.y, -v.x) / v.hypot();
    (0..n)
        .map(|i| {
            let (prev, p, next) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (n1, n2) = (normal(p - prev), normal(next - p));
            let bisector = n1 + n2;
            let len2 = bisector.hypot2();
            if len2 < 1e-9 {
                return p + n1 * d;
            }
            let miter = bisector * (2.0 / len2);
            let miter = if miter.hypot() > 4.0 {
                miter * (4.0 / miter.hypot())
            } else {
                miter
            };
            p + miter * d
        })
        .collect()
}

/// The westmost cell and the northmost cell of the graticule in the shape.
fn origin(lines: &Lines, shape: &[Vec<Point>], area: Rect) -> Option<(i64, i64)> {
    const N: usize = 48;
    let mut points: Vec<Point> = shape.iter().flatten().copied().collect();
    for i in 0..=N {
        for j in 0..=N {
            let t = |k: usize| k as f64 / N as f64;
            let p = Point::new(
                area.x0 + area.width() * t(i),
                area.y0 + area.height() * t(j),
            );
            if inside(p, shape) {
                points.push(p);
            }
        }
    }
    let cells: Vec<[i64; 2]> = points.iter().filter_map(|p| lines.cell(*p)).collect();
    let west = cells.iter().map(|[x, _]| *x).min()?;
    let north = cells.iter().map(|[_, y]| *y).max()?;
    Some((west, north))
}

/// A closed path through rings of target points.
fn shape_path(rings: &[Vec<Point>]) -> BezPath {
    let mut path = BezPath::new();
    for ring in rings.iter().filter(|r| !r.is_empty()) {
        path.move_to(ring[0]);
        for p in &ring[1..] {
            path.line_to(*p);
        }
        path.close_path();
    }
    path
}

/// One side of the band. The top and the bottom side hold the corners.
struct Side {
    /// True for the top and the bottom side.
    horizontal: bool,
    /// The extent along the side.
    lo: f64,
    hi: f64,
    /// The extent across the side.
    a: f64,
    b: f64,
}

fn sides(inner: Rect, outer: Rect) -> [Side; 4] {
    let side = |horizontal, lo, hi, a, b| Side {
        horizontal,
        lo,
        hi,
        a,
        b,
    };
    [
        side(true, outer.x0, outer.x1, outer.y0, inner.y0),
        side(true, outer.x0, outer.x1, inner.y1, outer.y1),
        side(false, inner.y0, inner.y1, outer.x0, inner.x0),
        side(false, inner.y0, inner.y1, inner.x1, outer.x1),
    ]
}

/// A part of a side between two graticule lines.
struct Piece {
    rect: Rect,
    /// The middle of the piece along the middle of the band.
    at: Point,
    /// The graticule cell at `at`: the index of the interval of the
    /// longitude and of the latitude. `None` off the sphere.
    cell: Option<[i64; 2]>,
}

/// The graticule lines of a frame in target points, over the band.
struct Lines<'a> {
    projection: &'a Projection,
    vt: crate::view::ViewTransform,
    center_lon: f64,
    interval: f64,
    /// Each line, with true for a meridian.
    lines: Vec<(bool, Vec<Point>)>,
}

impl<'a> Lines<'a> {
    fn new(
        projection: &'a Projection,
        frame: &FrameRecord,
        target: FrameTarget,
        outer: Rect,
    ) -> Lines<'a> {
        let interval = interval_at_scale(projection, frame);
        let vt = frame_transform(projection, frame, target);
        let window = graticule_window(projection, &vt, outer, frame.center[0], interval);
        let mut lines = Vec::new();
        for line in graticule_in(interval, window) {
            let meridian = line.len() >= 2 && line[0][0] == line[1][0];
            for part in projection.project_line(&line) {
                lines.push((meridian, part.iter().map(|p| vt.to_page(*p)).collect()));
            }
        }
        Lines {
            projection,
            vt,
            center_lon: frame.center[0],
            interval,
            lines,
        }
    }

    /// The cell of the graticule at `p`.
    fn cell(&self, p: Point) -> Option<[i64; 2]> {
        let [lon, lat] = self.projection.inverse(self.vt.from_page(p))?;
        let lon = self.center_lon + vale_sphere::wrap180(lon - self.center_lon);
        let index = |v: f64| (v / self.interval).floor() as i64;
        Some([index(lon), index(lat)])
    }

    /// The pieces of a side between the lines where they meet the middle
    /// of the band. `meridians` keeps only the meridians or only the
    /// parallels.
    fn pieces(&self, side: &Side, meridians: Option<bool>) -> Vec<Piece> {
        let c = (side.a + side.b) / 2.0;
        let mut cuts = vec![side.lo, side.hi];
        let lines = (self.lines.iter()).filter(|(m, _)| meridians.is_none_or(|want| *m == want));
        for (_, line) in lines {
            for seg in line.windows(2) {
                let (p, q) = if side.horizontal {
                    ((seg[0].x, seg[0].y), (seg[1].x, seg[1].y))
                } else {
                    ((seg[0].y, seg[0].x), (seg[1].y, seg[1].x))
                };
                if (p.1 > c) != (q.1 > c) {
                    let s = p.0 + (c - p.1) / (q.1 - p.1) * (q.0 - p.0);
                    if s > side.lo && s < side.hi {
                        cuts.push(s);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
        let mut out: Vec<Piece> = Vec::new();
        for s in cuts.windows(2) {
            let mid = (s[0] + s[1]) / 2.0;
            let (rect, at) = if side.horizontal {
                (Rect::new(s[0], side.a, s[1], side.b), Point::new(mid, c))
            } else {
                (Rect::new(side.a, s[0], side.b, s[1]), Point::new(c, mid))
            };
            // Off the sphere, the color changes at each line.
            let cell = self.cell(at).or_else(|| {
                let [x, y] = out.last()?.cell?;
                Some([x + 1, y])
            });
            out.push(Piece { rect, at, cell });
        }
        out
    }
}

/// A label at the middle of each interval of the meridians on the top and
/// the bottom side, and of the parallels on the left and the right side.
/// The columns go from the west, and the rows go from the north, both from
/// the first cell of the shape. Only the part of an interval beside the box
/// of the shape has a label. A part too short for its label has no label,
/// and neither has a label that the grown shape covers.
fn grid_labels(
    frame: &FrameRecord,
    lines: &Lines,
    (inner, outer, area): (Rect, Rect, Rect),
    unit: f64,
    (shape, grown): (&[Vec<Point>], &[Vec<Point>]),
    fonts: &mut Fonts,
) -> Painted {
    let b = &frame.border;
    let mut out = Painted::default();
    let Some((west, north)) = origin(lines, shape, area) else {
        return out;
    };
    let ink = TextInk::new(&b.grid_text, unit, fonts);
    let [top, bottom, left, right] = sides(inner, outer);
    // The pieces of a side, cut to the box of the shape.
    let beside = |side: &Side, meridians: bool| -> Vec<(Piece, f64)> {
        let (lo, hi) = if side.horizontal {
            (area.x0, area.x1)
        } else {
            (area.y0, area.y1)
        };
        let mut out = Vec::new();
        for p in lines.pieces(side, Some(meridians)) {
            let (r, at) = (p.rect, p.at);
            let (a, z) = if side.horizontal {
                (r.x0, r.x1)
            } else {
                (r.y0, r.y1)
            };
            let (a, z) = (a.max(lo), z.min(hi));
            if a >= z {
                continue;
            }
            let mid = (a + z) / 2.0;
            let at = if side.horizontal {
                Point::new(mid, at.y)
            } else {
                Point::new(at.x, mid)
            };
            if let Some(cell) = lines.cell(at) {
                let piece = Piece {
                    rect: r,
                    at,
                    cell: Some(cell),
                };
                out.push((piece, z - a));
            }
        }
        out
    };
    let name = |i: i64, letters: bool| {
        let i = i.max(0) as u32;
        if letters {
            column_name(i)
        } else {
            (i + 1).to_string()
        }
    };
    let letters = b.grid_columns == GridColumns::Letters;
    let mut put = |text: &str, piece: &Piece, along: f64| {
        let m = fonts.measure(&ink.case.apply(text), &ink.style);
        if m.advance > along {
            return;
        }
        let half = Vec2::new(m.advance / 2.0, m.ascent / 2.0);
        let corners = [-1.0, 1.0]
            .map(|sx| [-1.0, 1.0].map(|sy| piece.at + Vec2::new(sx * half.x, sy * half.y)));
        if inside(piece.at, grown) || corners.iter().flatten().any(|c| inside(*c, grown)) {
            return;
        }
        let baseline = piece.at.y + (m.ascent - m.descent.abs()) / 2.0;
        let to = Affine::translate((piece.at.x - m.advance / 2.0, baseline));
        ink.line(text, to, fonts, &mut out);
    };
    for side in [&top, &bottom] {
        for (p, along) in beside(side, true) {
            if let Some([x, _]) = p.cell {
                put(&name(x - west, letters), &p, along);
            }
        }
    }
    for side in [&left, &right] {
        for (p, along) in beside(side, false) {
            if let Some([_, y]) = p.cell {
                put(&name(north - y, !letters), &p, along);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_after_z_have_two_letters() {
        let names: Vec<String> = [0, 1, 25, 26, 27, 51, 52].map(column_name).into();
        assert_eq!(names, ["A", "B", "Z", "AA", "AB", "AZ", "BA"]);
    }
}
