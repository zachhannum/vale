//! The border band around a map frame, its stroke, and the labels of its
//! reference grid.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vale_labeler::Fonts;
use vale_render::{FillRule, Item};
use vale_sphere::{Projection, graticule_in};
use vale_store::{BorderStyle, FrameRecord};

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
    // The shape covers the band where it crosses it.
    let rings = target_rings(frame, target);
    let mut clip = outer
        .inflate(1.0, 1.0)
        .union(frame_area(frame, target))
        .to_path(0.1);
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
        let labels = grid_labels(frame, &lines, inner, outer, unit, &rings, fonts);
        out.extend(labels.into_items());
    }
    out
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

/// A letter at the middle of each interval of the meridians on the top and
/// the bottom side, and a number at the middle of each interval of the
/// parallels on the left and the right side. The letters go from the west,
/// and the numbers go from the north. A piece too short for its label has
/// no label, and neither has a label that the shape covers.
fn grid_labels(
    frame: &FrameRecord,
    lines: &Lines,
    inner: Rect,
    outer: Rect,
    unit: f64,
    rings: &[Vec<Point>],
    fonts: &mut Fonts,
) -> Painted {
    let ink = TextInk::new(&frame.border.grid_text, unit, fonts);
    let [top, bottom, left, right] = sides(inner, outer);
    let on_sphere = |side: &Side, meridians: bool| {
        let pieces = lines.pieces(side, Some(meridians));
        pieces
            .into_iter()
            .filter(|p| p.cell.is_some() && lines.cell(p.at).is_some())
    };
    let columns: Vec<Piece> = on_sphere(&top, true)
        .chain(on_sphere(&bottom, true))
        .collect();
    let rows: Vec<Piece> = on_sphere(&left, false)
        .chain(on_sphere(&right, false))
        .collect();
    let west = columns.iter().filter_map(|p| p.cell).map(|[x, _]| x).min();
    let north = rows.iter().filter_map(|p| p.cell).map(|[_, y]| y).max();
    let mut out = Painted::default();
    let mut put = |text: &str, piece: &Piece, along: f64| {
        let m = fonts.measure(&ink.case.apply(text), &ink.style);
        if m.advance > along {
            return;
        }
        // A label that the shape covers is not drawn.
        let half = Vec2::new(m.advance / 2.0, m.ascent / 2.0);
        let corners = [-1.0, 1.0]
            .map(|sx| [-1.0, 1.0].map(|sy| piece.at + Vec2::new(sx * half.x, sy * half.y)));
        if inside(piece.at, rings) || corners.iter().flatten().any(|c| inside(*c, rings)) {
            return;
        }
        let baseline = piece.at.y + (m.ascent - m.descent.abs()) / 2.0;
        let to = Affine::translate((piece.at.x - m.advance / 2.0, baseline));
        ink.line(text, to, fonts, &mut out);
    };
    for p in &columns {
        if let (Some([x, _]), Some(west)) = (p.cell, west) {
            put(&column_name((x - west) as u32), p, p.rect.width());
        }
    }
    for p in &rows {
        if let (Some([_, y]), Some(north)) = (p.cell, north) {
            put(&(north - y + 1).to_string(), p, p.rect.height());
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
