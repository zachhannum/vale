//! The graticule labels at the edge of a map frame.

use kurbo::{Affine, Point, Rect, Vec2};
use vale_labeler::{Fonts, TextStyle};
use vale_render::{Color, Item};
use vale_sphere::{Projection, graticule_in};
use vale_store::{CoordFormat, EdgeLabels, EdgeOrientation, FrameRecord};

use crate::atlas::{FrameTarget, PT_PER_MM, coord_text, frame_transform, graticule_interval};
use crate::furniture;
use crate::pipeline::{frame_area, graticule_window};

/// The distance from the line to the baseline of a label along its line, in
/// page points.
const RISE: f64 = 1.5;
/// The number of steps of a quarter of the text size that a label can move
/// away from the edge.
const SLIDES: usize = 8;
/// The space that two labels keep between them, in page points.
const MARGIN: f64 = 1.0;
/// A line that meets the edge at less than this angle gets no label.
const MIN_ANGLE_DEG: f64 = 20.0;

/// The side of a frame that an edge faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// One label at the edge of a frame.
#[derive(Clone, Debug)]
pub struct EdgeLabel {
    pub text: String,
    /// Where the line meets the edge, in target points.
    pub at: Point,
    pub side: Side,
    pub outside: bool,
    /// The corners of the box of the text, in target points.
    pub quad: [Point; 4],
    /// The halo, a stroke around the text.
    pub halo: Option<Item>,
    pub glyphs: Vec<Item>,
}

/// A place where a graticule line meets the edge of a frame.
struct Crossing {
    at: Point,
    /// The direction of the line there, into the shape.
    inward: Vec2,
    /// The normal of the edge there, out of the shape.
    outward: Vec2,
    side: Side,
}

/// The labels where the graticule lines of `frame` meet the edge of its
/// shape. A label reads from left to right. A label that overlaps another
/// label, or is not on its side of the edge, is not in the list. The labels
/// come from the whole shape, so each part of a frame has the same ones.
pub fn edge_labels(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
    fonts: &mut Fonts,
) -> Vec<EdgeLabel> {
    let g = &frame.graticule;
    if !g.meridians.on && !g.parallels.on {
        return Vec::new();
    }
    let Some(interval) = graticule_interval(projection, frame) else {
        return Vec::new();
    };
    let vt = frame_transform(projection, frame, target);
    let area = frame_area(frame, target);
    let window = graticule_window(projection, &vt, area, frame.center[0], interval);
    let unit = target.pts_per_mm / PT_PER_MM;
    let rings = target_rings(frame, target);

    let mut candidates = Vec::new();
    for line in graticule_in(interval, window) {
        let meridian = line.len() >= 2 && line[0][0] == line[1][0];
        let set = if meridian { &g.meridians } else { &g.parallels };
        if !set.on {
            continue;
        }
        let text = if meridian {
            coord_text(line[0][0], interval, g.format, 'E', 'W')
        } else {
            coord_text(line[0][1], interval, g.format, 'N', 'S')
        };
        for part in projection.project_line(&line) {
            let pts: Vec<Point> = part.iter().map(|p| vt.to_page(*p)).collect();
            for crossing in crossings(&pts, &rings) {
                let edges = set.edges;
                let shown = match crossing.side {
                    Side::Top => edges.top,
                    Side::Bottom => edges.bottom,
                    Side::Left => edges.left,
                    Side::Right => edges.right,
                };
                if shown {
                    candidates.push((set, text.clone(), crossing));
                }
            }
        }
    }

    // The labels far from a corner go first, so at a corner the label that
    // has more room stays.
    let corners: Vec<Point> = rings.iter().flatten().copied().collect();
    let corner_distance = |p: Point| {
        (corners.iter())
            .map(|c| c.distance(p))
            .fold(f64::INFINITY, f64::min)
    };
    candidates.sort_by(|a, b| corner_distance(b.2.at).total_cmp(&corner_distance(a.2.at)));

    let mut placed: Vec<EdgeLabel> = Vec::new();
    for (set, text, c) in candidates {
        let style = text_style(set, unit, fonts);
        let metrics = fonts.measure(&text, &style);
        if metrics.glyph_count == 0 {
            continue;
        }
        let width = metrics.advance;
        let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
        let box_at = |gap: f64| match set.orientation {
            EdgeOrientation::Along => {
                // Away from the edge, on the side of the label.
                let away = if set.outside { -c.inward } else { c.inward };
                // The text runs to the right, or up for a vertical line.
                let forward = away.x > 1e-6 || (away.x.abs() <= 1e-6 && away.y < 0.0);
                let u = if forward { away } else { -away };
                let up = Vec2::new(u.y, -u.x);
                let start = if forward {
                    c.at + away * gap
                } else {
                    c.at + away * (gap + width)
                };
                let origin = start + up * RISE * unit;
                text_box(origin, u, width, ascent, descent)
            }
            EdgeOrientation::Horizontal => {
                let m = if set.outside { c.outward } else { -c.outward };
                // The half size of the box in the direction `m`.
                let half = m.x.abs() * width / 2.0 + m.y.abs() * (ascent + descent) / 2.0;
                let center = c.at + m * (gap + half);
                let top = center.y - (ascent + descent) / 2.0;
                let origin = Point::new(center.x - width / 2.0, top + ascent);
                text_box(origin, Vec2::new(1.0, 0.0), width, ascent, descent)
            }
        };
        // A line that meets the edge at a slant needs more room, so the
        // label moves away from the edge until its box is on its side.
        let fits = |quad: &[Point; 4]| {
            quad.iter().all(|q| inside(*q, &rings) != set.outside) && !crosses_rings(quad, &rings)
        };
        let Some((origin, u, quad)) = (0..=SLIDES)
            .map(|k| box_at((set.offset + k as f64 * set.size / 4.0) * unit))
            .find(|(_, _, quad)| fits(quad))
        else {
            continue;
        };
        let grown = grow(&quad, MARGIN * unit);
        if placed.iter().any(|p| quads_overlap(&grown, &p.quad)) {
            continue;
        }
        let to = Affine::translate(origin.to_vec2()) * Affine::rotate(u.y.atan2(u.x));
        let text_color = color(set.color, set.opacity);
        let Some(line) = furniture::glyph_line(&text, &style, text_color, to, true, fonts) else {
            continue;
        };
        let glyphs = line.glyphs;
        let halo = set.halo.then(|| Item::Stroke {
            path: line.outlines,
            color: color(set.halo_color, set.opacity),
            width: set.halo_width * unit,
            round: true,
        });
        placed.push(EdgeLabel {
            text,
            at: c.at,
            side: c.side,
            outside: set.outside,
            quad,
            halo,
            glyphs,
        });
    }
    placed
}

/// The items of the labels of one side of the edge: the halos, then the
/// glyphs.
pub fn items(labels: &[EdgeLabel], outside: bool) -> Vec<Item> {
    let labels = labels.iter().filter(|l| l.outside == outside);
    let halos = labels.clone().filter_map(|l| l.halo.clone());
    halos.chain(labels.flat_map(|l| l.glyphs.clone())).collect()
}

/// How far the labels outside a frame can reach past its shape, in page
/// points. 0 when no labels are outside.
pub fn outside_reach(frame: &FrameRecord, fonts: &mut Fonts) -> f64 {
    let g = &frame.graticule;
    let widest = match g.format {
        CoordFormat::Dms => "000°00′00″ W",
        CoordFormat::Decimal => "000.0000° W",
    };
    [&g.meridians, &g.parallels]
        .into_iter()
        .filter(|set| set.on && set.outside)
        .map(|set| {
            let style = text_style(set, 1.0, fonts);
            let m = fonts.measure(widest, &style);
            let text = m.advance.max(m.ascent + m.descent.abs());
            let slides = SLIDES as f64 * set.size / 4.0;
            set.offset + slides + RISE + text + set.halo_width
        })
        .fold(0.0, f64::max)
}

/// The rings of the shape of a frame, in target points.
pub(crate) fn target_rings(frame: &FrameRecord, target: FrameTarget) -> Vec<Vec<Point>> {
    (frame.shape.rings().iter())
        .map(|r| {
            let points = r.iter().map(|&[x, y]| target.to_target(Point::new(x, y)));
            points.collect()
        })
        .collect()
}

fn text_style(set: &EdgeLabels, unit: f64, fonts: &mut Fonts) -> TextStyle {
    TextStyle {
        weight: set.weight,
        italic: set.italic,
        ..TextStyle::new(
            crate::fonts::family(set.font.as_deref(), fonts),
            set.size * unit,
        )
    }
}

fn color([r, g, b]: [u8; 3], opacity: f64) -> Color {
    Color::rgba(r, g, b, (opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The origin, the direction, and the corners of the box of a text with its
/// baseline start at `origin`, running in the direction `u`.
fn text_box(
    origin: Point,
    u: Vec2,
    width: f64,
    ascent: f64,
    descent: f64,
) -> (Point, Vec2, [Point; 4]) {
    let up = Vec2::new(u.y, -u.x);
    let quad = [
        origin - up * descent,
        origin + u * width - up * descent,
        origin + u * width + up * ascent,
        origin + up * ascent,
    ];
    (origin, u, quad)
}

/// The axis-aligned box of the corners of a label.
pub fn quad_bounds(quad: &[Point; 4]) -> Rect {
    let mut r = Rect::from_points(quad[0], quad[1]);
    for q in &quad[2..] {
        r = r.union_pt(*q);
    }
    r
}

/// Each place where `line` crosses an edge of `rings`.
fn crossings(line: &[Point], rings: &[Vec<Point>]) -> Vec<Crossing> {
    let min_sin = MIN_ANGLE_DEG.to_radians().sin();
    let probe = 0.5;
    let mut out = Vec::new();
    for seg in line.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let d = b - a;
        if d.hypot() < 1e-9 {
            continue;
        }
        for ring in rings {
            for (i, &c) in ring.iter().enumerate() {
                let e = ring[(i + 1) % ring.len()] - c;
                let Some((t, _)) = intersect(a, d, c, e) else {
                    continue;
                };
                let sin = d.cross(e).abs() / (d.hypot() * e.hypot());
                if sin < min_sin {
                    continue;
                }
                let at = a + d * t;
                let dir = d.normalize();
                let inward = if inside(at + dir * probe, rings) {
                    dir
                } else if inside(at - dir * probe, rings) {
                    -dir
                } else {
                    continue;
                };
                let n = Vec2::new(e.y, -e.x).normalize();
                let outward = if inside(at + n * probe, rings) { -n } else { n };
                let side = if outward.x.abs() > outward.y.abs() {
                    if outward.x < 0.0 {
                        Side::Left
                    } else {
                        Side::Right
                    }
                } else if outward.y < 0.0 {
                    Side::Top
                } else {
                    Side::Bottom
                };
                out.push(Crossing {
                    at,
                    inward,
                    outward,
                    side,
                });
            }
        }
    }
    out
}

/// The parameters of the crossing of segments `a + t d` and `c + u e`, both
/// in 0..1. The end of a segment counts at its start only, so a crossing at a
/// shared point is one crossing.
fn intersect(a: Point, d: Vec2, c: Point, e: Vec2) -> Option<(f64, f64)> {
    let den = d.cross(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let ac = c - a;
    let t = ac.cross(e) / den;
    let u = ac.cross(d) / den;
    ((0.0..1.0).contains(&t) && (0.0..1.0).contains(&u)).then_some((t, u))
}

/// The even-odd rule over all rings.
pub(crate) fn inside(p: Point, rings: &[Vec<Point>]) -> bool {
    let mut odd = false;
    for ring in rings {
        for (i, &a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
    }
    odd
}

/// Whether an edge of `rings` crosses an edge of `quad`, as at a notch.
fn crosses_rings(quad: &[Point; 4], rings: &[Vec<Point>]) -> bool {
    (0..4).any(|i| {
        let (a, d) = (quad[i], quad[(i + 1) % 4] - quad[i]);
        rings.iter().any(|ring| {
            (0..ring.len()).any(|j| {
                let (c, e) = (ring[j], ring[(j + 1) % ring.len()] - ring[j]);
                intersect(a, d, c, e).is_some()
            })
        })
    })
}

/// A box with each side moved out by `by`.
fn grow(quad: &[Point; 4], by: f64) -> [Point; 4] {
    let u = (quad[1] - quad[0]).normalize();
    let up = (quad[3] - quad[0]).normalize();
    [
        quad[0] - u * by - up * by,
        quad[1] + u * by - up * by,
        quad[2] + u * by + up * by,
        quad[3] - u * by + up * by,
    ]
}

/// Whether two boxes overlap, by the separating axes of their sides.
pub fn quads_overlap(a: &[Point; 4], b: &[Point; 4]) -> bool {
    let axes = [a[1] - a[0], a[3] - a[0], b[1] - b[0], b[3] - b[0]];
    axes.iter().all(|axis| {
        let span = |q: &[Point; 4]| {
            q.iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    let v = p.to_vec2().dot(*axis);
                    (lo.min(v), hi.max(v))
                })
        };
        let (a0, a1) = span(a);
        let (b0, b1) = span(b);
        a0 < b1 && b0 < a1
    })
}
