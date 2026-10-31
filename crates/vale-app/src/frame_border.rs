//! The border band around a map frame and the labels of its reference grid.

use kurbo::{Affine, Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{FillRule, Item};
use vale_sphere::{Projection, graticule_in};
use vale_store::{BorderStyle, FrameRecord};

use crate::atlas::{FrameTarget, PT_PER_MM, frame_transform, interval_at_scale};
use crate::furniture::{Painted, TextInk, rgb};
use crate::pipeline::{frame_area, graticule_window};

/// The inner and the outer edge of the band, in target points. The inner
/// edge is the bounding box of the shape, grown by the distance.
pub fn band_rects(frame: &FrameRecord, target: FrameTarget) -> (Rect, Rect) {
    let b = &frame.border;
    let inner = frame_area(frame, target).inflate(
        b.distance_mm * target.pts_per_mm,
        b.distance_mm * target.pts_per_mm,
    );
    let w = b.width_mm * target.pts_per_mm;
    (inner, inner.inflate(w, w))
}

/// How far the band and the grid labels reach out of the bounding box of
/// the shape, in page points. 0 when both are off.
pub fn reach(frame: &FrameRecord) -> f64 {
    let (b, g) = (&frame.border, &frame.grid);
    if !b.on && !g.on {
        return 0.0;
    }
    let band = (b.distance_mm + b.width_mm) * PT_PER_MM;
    if g.on { band + g.text.size } else { band }
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

/// The band and the grid labels of a frame, in target points. The band
/// comes first.
pub fn items(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let mut out = Vec::new();
    let (inner, outer) = band_rects(frame, target);
    let b = &frame.border;
    if b.on {
        match b.style {
            BorderStyle::Solid => {
                let mut path = outer.to_path(0.1);
                path.extend(inner.to_path(0.1));
                out.push(Item::Fill {
                    path,
                    color: rgb(b.color),
                    rule: FillRule::EvenOdd,
                });
            }
            BorderStyle::Graduated => {
                for (rect, first) in graduated(projection, frame, target, inner, outer) {
                    out.push(Item::Fill {
                        path: rect.to_path(0.1),
                        color: rgb(if first { b.color } else { b.color2 }),
                        rule: FillRule::NonZero,
                    });
                }
            }
        }
    }
    if frame.grid.on {
        out.extend(grid_labels(frame, target, inner, outer, fonts).into_items());
    }
    out
}

/// The pieces of a graduated band, each with `true` for the first color.
/// The top and the bottom side hold the corners.
fn graduated(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
    inner: Rect,
    outer: Rect,
) -> Vec<(Rect, bool)> {
    let interval = interval_at_scale(projection, frame);
    let vt = frame_transform(projection, frame, target);
    let window = graticule_window(projection, &vt, outer, frame.center[0], interval);
    let lines: Vec<Vec<Point>> = (graticule_in(interval, window).iter())
        .flat_map(|line| projection.project_line(line))
        .map(|part| part.iter().map(|p| vt.to_page(*p)).collect())
        .collect();
    // The cells of the graticule alternate in a checkerboard, so the color
    // changes at each line, a meridian or a parallel.
    let parity = |p: Point| {
        let [lon, lat] = projection.inverse(vt.from_page(p))?;
        let lon = frame.center[0] + vale_sphere::wrap180(lon - frame.center[0]);
        let cell = (lon / interval).floor() + (lat / interval).floor();
        Some(cell.rem_euclid(2.0) < 0.5)
    };
    let sides = [
        (true, outer.x0, outer.x1, outer.y0, inner.y0),
        (true, outer.x0, outer.x1, inner.y1, outer.y1),
        (false, inner.y0, inner.y1, outer.x0, inner.x0),
        (false, inner.y0, inner.y1, inner.x1, outer.x1),
    ];
    let mut out = Vec::new();
    for (horizontal, lo, hi, a, b) in sides {
        let c = (a + b) / 2.0;
        let point = |s: f64| {
            if horizontal {
                Point::new(s, c)
            } else {
                Point::new(c, s)
            }
        };
        let mut cuts = vec![lo, hi];
        for line in &lines {
            for seg in line.windows(2) {
                let (p, q) = if horizontal {
                    ((seg[0].x, seg[0].y), (seg[1].x, seg[1].y))
                } else {
                    ((seg[0].y, seg[0].x), (seg[1].y, seg[1].x))
                };
                if (p.1 > c) != (q.1 > c) {
                    let s = p.0 + (c - p.1) / (q.1 - p.1) * (q.0 - p.0);
                    if s > lo && s < hi {
                        cuts.push(s);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
        let mut first = true;
        for (i, s) in cuts.windows(2).enumerate() {
            first = match parity(point((s[0] + s[1]) / 2.0)) {
                Some(f) => f,
                None if i == 0 => true,
                None => !first,
            };
            let rect = if horizontal {
                Rect::new(s[0], a, s[1], b)
            } else {
                Rect::new(a, s[0], b, s[1])
            };
            out.push((rect, first));
        }
    }
    out
}

/// The column labels at the top and the bottom, and the row labels at the
/// left and the right, each at the middle of the band.
fn grid_labels(
    frame: &FrameRecord,
    target: FrameTarget,
    inner: Rect,
    outer: Rect,
    fonts: &mut Fonts,
) -> Painted {
    let g = &frame.grid;
    let area = frame_area(frame, target);
    let ink = TextInk::new(&g.text, target.pts_per_mm / PT_PER_MM, fonts);
    let mut out = Painted::default();
    let mut put = |text: &str, at: Point, out: &mut Painted| {
        let m = fonts.measure(&ink.case.apply(text), &ink.style);
        let baseline = at.y + (m.ascent - m.descent.abs()) / 2.0;
        let to = Affine::translate((at.x - m.advance / 2.0, baseline));
        ink.line(text, to, fonts, out);
    };
    let (top, bottom) = ((outer.y0 + inner.y0) / 2.0, (inner.y1 + outer.y1) / 2.0);
    let (left, right) = ((outer.x0 + inner.x0) / 2.0, (inner.x1 + outer.x1) / 2.0);
    let column = area.width() / f64::from(g.columns);
    for i in 0..g.columns {
        let x = area.x0 + column * (f64::from(i) + 0.5);
        let name = column_name(i);
        put(&name, Point::new(x, top), &mut out);
        put(&name, Point::new(x, bottom), &mut out);
    }
    let row = area.height() / f64::from(g.rows);
    for i in 0..g.rows {
        let y = area.y0 + row * (f64::from(i) + 0.5);
        let name = (i + 1).to_string();
        put(&name, Point::new(left, y), &mut out);
        put(&name, Point::new(right, y), &mut out);
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
