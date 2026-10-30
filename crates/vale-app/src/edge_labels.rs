//! The graticule labels at the edge of a map frame.

use kurbo::{Affine, Point, Rect, Vec2};
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    PointPosition, TextStyle, place_labels,
};
use vale_render::{Color, GlyphRun, Item};
use vale_sphere::{Projection, graticule_in};
use vale_store::FrameRecord;

use crate::atlas::{FrameTarget, PT_PER_MM, coord_text, frame_transform, graticule_interval};
use crate::pipeline::{frame_area, graticule_window};

/// The text size in page points.
const SIZE: f64 = 7.0;
/// The distance from the edge to the start of the text, in page points.
const GAP: f64 = 2.0;
/// The distance from the line to the baseline, in page points.
const RISE: f64 = 1.5;
/// The number of steps of a quarter of the text size that a label can move
/// along its line, away from the edge.
const SLIDES: usize = 8;
/// The space that two labels keep between them, in page points.
const MARGIN: f64 = 1.0;
/// A line that meets the edge at less than this angle gets no label.
const MIN_ANGLE_DEG: f64 = 20.0;

/// One label at the edge of a frame.
#[derive(Clone, Debug)]
pub struct EdgeLabel {
    pub text: String,
    /// Where the line meets the edge, in target points.
    pub at: Point,
    /// The corners of the box of the text, in target points.
    pub quad: [Point; 4],
    pub items: Vec<Item>,
}

/// The labels where the graticule lines of `frame` meet the edge of its
/// shape. Each label is inside the shape, along its line, and it reads from
/// left to right. A label that overlaps another label or leaves the shape is
/// not in the list. The labels come from the whole shape, so each part of a
/// frame has the same ones.
pub fn edge_labels(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
    fonts: &mut Fonts,
) -> Vec<EdgeLabel> {
    let Some(interval) = graticule_interval(projection, frame) else {
        return Vec::new();
    };
    let vt = frame_transform(projection, frame, target);
    let area = frame_area(frame, target);
    let window = graticule_window(projection, &vt, area, frame.center[0], interval);
    let unit = target.pts_per_mm / PT_PER_MM;
    let rings: Vec<Vec<Point>> = (frame.shape.rings().iter())
        .map(|r| {
            let points = r.iter().map(|&[x, y]| target.to_target(Point::new(x, y)));
            points.collect()
        })
        .collect();
    let format = frame.graticule.format;
    let style = TextStyle::new(crate::fonts::DEFAULT_FAMILY, SIZE * unit);
    let [r, g, b] = frame.graticule.color;
    let ink = Color::rgb(r / 2, g / 2, b / 2);

    let mut candidates = Vec::new();
    for line in graticule_in(interval, window) {
        let meridian = line.len() >= 2 && line[0][0] == line[1][0];
        let text = if meridian {
            coord_text(line[0][0], interval, format, 'E', 'W')
        } else {
            coord_text(line[0][1], interval, format, 'N', 'S')
        };
        for part in projection.project_line(&line) {
            let pts: Vec<Point> = part.iter().map(|p| vt.to_page(*p)).collect();
            for (at, dir) in crossings(&pts, &rings) {
                candidates.push((text.clone(), at, dir));
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
    candidates.sort_by(|a, b| corner_distance(b.1).total_cmp(&corner_distance(a.1)));

    let mut placed: Vec<EdgeLabel> = Vec::new();
    for (text, at, inward) in candidates {
        let metrics = fonts.measure(&text, &style);
        if metrics.glyph_count == 0 {
            continue;
        }
        let width = metrics.advance;
        // The reading direction runs to the right, or up for a vertical line.
        let reads_inward = inward.x > 1e-6 || (inward.x.abs() <= 1e-6 && inward.y < 0.0);
        let u = if reads_inward { inward } else { -inward };
        let up = Vec2::new(u.y, -u.x);
        let (ascent, descent) = (metrics.ascent, metrics.descent.abs());
        let box_at = |gap: f64| {
            let start = if reads_inward {
                at + inward * gap
            } else {
                at + inward * (gap + width)
            };
            let origin = start + up * RISE * unit;
            let quad = [
                origin - up * descent,
                origin + u * width - up * descent,
                origin + u * width + up * ascent,
                origin + up * ascent,
            ];
            (origin, quad)
        };
        // A line that meets the edge at a slant needs more room, so the
        // label moves along the line until its box is inside the shape.
        let fits = |quad: &[Point; 4]| {
            quad.iter().all(|q| inside(*q, &rings)) && !crosses_rings(quad, &rings)
        };
        let Some((origin, quad)) = (0..=SLIDES)
            .map(|k| box_at((GAP + k as f64 * SIZE / 4.0) * unit))
            .find(|(_, quad)| fits(quad))
        else {
            continue;
        };
        let grown = grow(&quad, MARGIN * unit);
        if placed.iter().any(|p| quads_overlap(&grown, &p.quad)) {
            continue;
        }
        let rotation = Affine::translate(origin.to_vec2()) * Affine::rotate(u.y.atan2(u.x));
        let items = glyph_items(&text, width, &style, rotation, ink, fonts);
        if items.is_empty() {
            continue;
        }
        placed.push(EdgeLabel {
            text,
            at,
            quad,
            items,
        });
    }
    placed
}

/// The axis-aligned box of the corners of a label.
pub fn quad_bounds(quad: &[Point; 4]) -> Rect {
    let mut r = Rect::from_points(quad[0], quad[1]);
    for q in &quad[2..] {
        r = r.union_pt(*q);
    }
    r
}

/// Each place where `line` crosses an edge of `rings`, with the direction of
/// the line there that points into the shape.
fn crossings(line: &[Point], rings: &[Vec<Point>]) -> Vec<(Point, Vec2)> {
    let min_sin = MIN_ANGLE_DEG.to_radians().sin();
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
                let probe = 0.5;
                if inside(at + dir * probe, rings) {
                    out.push((at, dir));
                } else if inside(at - dir * probe, rings) {
                    out.push((at, -dir));
                }
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
fn inside(p: Point, rings: &[Vec<Point>]) -> bool {
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

/// The glyphs of `text` with the baseline start at the origin of `to`.
fn glyph_items(
    text: &str,
    width: f64,
    style: &TextStyle,
    to: Affine,
    color: Color,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let room = 4.0 * style.size + width;
    let input = LabelInput {
        bounds: Rect::new(-room, -room, room, room),
        classes: vec![LabelClass {
            name: "graticule".to_string(),
            style: style.clone(),
            priority: 0,
            margin: 0.0,
            placement: Placement::Point(PointPlacement {
                positions: vec![PointPosition::TopRight],
                offset: 0.0,
            }),
            duplicate_distance: None,
        }],
        features: vec![Feature {
            id: 0,
            class: 0,
            text: text.to_string(),
            priority: 0.0,
            geometry: Geometry::Point {
                position: Point::ZERO,
                symbol_radius: 0.0,
            },
        }],
        obstacles: vec![],
        options: Options {
            seed: 1,
            improve: false,
        },
    };
    let labeling = place_labels(fonts, &input);
    let Some(label) = labeling.placed.first() else {
        return vec![];
    };
    let Some(start) = (label.glyph_runs.iter())
        .flat_map(|r| r.glyphs.first())
        .map(|g| g.transform.translation())
        .next()
    else {
        return vec![];
    };
    let to = to * Affine::translate(-start);
    (label.glyph_runs.iter())
        .map(|run| {
            Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: (run.glyphs.iter())
                    .map(|g| (g.id, to * g.transform))
                    .collect(),
                text: label.text.clone(),
                color,
            })
        })
        .collect()
}
