//! Snapping of map frames to the layout of a page.
//!
//! All positions are page millimeters. The reach is the snap distance in
//! millimeters at the current zoom.

use kurbo::{Point, Rect, Vec2};
use vale_store::{FrameId, PageRecord};

use crate::atlas::shape_bbox;

/// A line of the layout on one axis.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Line {
    /// The x of a vertical line or the y of a horizontal line.
    pub at: f64,
    /// The extent of the target on the other axis, for the guide.
    pub from: f64,
    pub to: f64,
}

/// The snap targets of a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Targets {
    /// The vertical lines.
    pub xs: Vec<Line>,
    /// The horizontal lines.
    pub ys: Vec<Line>,
    /// The vertices of the other frames.
    pub points: Vec<Point>,
    /// The other vertices of the moving frame. A vertex lines up with them,
    /// and it does not snap onto them.
    pub align: Vec<Point>,
    /// The edges of the other frames.
    pub segments: Vec<(Point, Point)>,
    /// The spacing of the layout grid, when the grid shows.
    pub grid: Option<f64>,
    /// The page.
    pub page: Rect,
}

/// A mark that shows a snap.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Guide {
    Line(Point, Point),
    /// A vertex that a vertex snapped to.
    Point(Point),
}

/// The part of a rectangle that a drag moves.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Moving {
    /// The whole rectangle.
    Body,
    /// A resize handle, as `atlas::Handle::Resize` numbers it.
    Handle(usize),
}

const SAME: f64 = 1e-6;

/// The targets of a page for a drag of one frame. The margins come from the
/// page template.
pub fn targets(
    page: &PageRecord,
    moving: FrameId,
    margins: Option<Rect>,
    grid: Option<f64>,
) -> Targets {
    let (w, h) = (page.width_mm, page.height_mm);
    let mut t = Targets {
        grid: grid.filter(|s| *s > 0.0),
        page: Rect::new(0.0, 0.0, w, h),
        ..Targets::default()
    };
    for x in [0.0, w / 2.0, w] {
        t.xs.push(Line {
            at: x,
            from: 0.0,
            to: h,
        });
    }
    for y in [0.0, h / 2.0, h] {
        t.ys.push(Line {
            at: y,
            from: 0.0,
            to: w,
        });
    }
    if let Some(m) = margins {
        for x in [m.x0, m.x1] {
            t.xs.push(Line {
                at: x,
                from: 0.0,
                to: h,
            });
        }
        for y in [m.y0, m.y1] {
            t.ys.push(Line {
                at: y,
                from: 0.0,
                to: w,
            });
        }
    }
    for frame in page.frames.iter().filter(|f| f.id != moving) {
        let b = shape_bbox(&frame.shape);
        let c = b.center();
        for x in [b.x0, c.x, b.x1] {
            t.xs.push(Line {
                at: x,
                from: b.y0,
                to: b.y1,
            });
        }
        for y in [b.y0, c.y, b.y1] {
            t.ys.push(Line {
                at: y,
                from: b.x0,
                to: b.x1,
            });
        }
        for ring in frame.shape.rings() {
            let ring: Vec<Point> = ring.iter().map(|&[x, y]| Point::new(x, y)).collect();
            for (i, &p) in ring.iter().enumerate() {
                t.points.push(p);
                t.segments.push((p, ring[(i + 1) % ring.len()]));
            }
        }
    }
    t
}

impl Targets {
    /// The lines of the page and of the frames, and a vertical and a
    /// horizontal line through each vertex of `points` and `align`. The
    /// extent of a line through a vertex is the vertex, so its guide goes
    /// from the vertex to the moving item.
    fn lines(&self) -> (Vec<Line>, Vec<Line>) {
        let (mut xs, mut ys) = (self.xs.clone(), self.ys.clone());
        for p in self.points.iter().chain(&self.align) {
            xs.push(Line {
                at: p.x,
                from: p.y,
                to: p.y,
            });
            ys.push(Line {
                at: p.y,
                from: p.x,
                to: p.x,
            });
        }
        (xs, ys)
    }
}

/// The nearest grid line to `c`.
fn grid_line(c: f64, spacing: f64, extent: f64) -> Option<f64> {
    let at = (c / spacing).round() * spacing;
    (0.0..=extent + SAME).contains(&at).then_some(at)
}

/// The smallest move of one of `cands` onto a line within `reach`. A layout
/// line wins over a grid line at the same distance.
fn axis_delta(
    cands: &[f64],
    lines: &[Line],
    grid: Option<f64>,
    extent: f64,
    reach: f64,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut take = |d: f64, strict: bool| {
        let better = match best {
            None => true,
            Some(b) if strict => d.abs() < b.abs() - SAME,
            Some(b) => d.abs() < b.abs(),
        };
        if d.abs() <= reach && better {
            best = Some(d);
        }
    };
    for &c in cands {
        for l in lines {
            take(l.at - c, false);
        }
    }
    if let Some(s) = grid {
        for &c in cands {
            if let Some(at) = grid_line(c, s, extent) {
                take(at - c, true);
            }
        }
    }
    best
}

/// The guides of the lines at `cands`. `vertical`: the lines are x lines,
/// and `span` is the extent of the moving item on the y axis.
fn axis_guides(
    cands: &[f64],
    lines: &[Line],
    t: &Targets,
    span: (f64, f64),
    vertical: bool,
    out: &mut Vec<Guide>,
) {
    let full = if vertical {
        (t.page.y0, t.page.y1)
    } else {
        (t.page.x0, t.page.x1)
    };
    let extent = if vertical { t.page.x1 } else { t.page.y1 };
    // One guide for each position, over the extents of all its targets.
    let mut hits: Vec<(f64, f64, f64)> = Vec::new();
    let mut hit =
        |at: f64, from: f64, to: f64| match hits.iter_mut().find(|h| (h.0 - at).abs() < SAME) {
            Some(h) => (h.1, h.2) = (h.1.min(from), h.2.max(to)),
            None => hits.push((at, from, to)),
        };
    for &c in cands {
        for l in lines.iter().filter(|l| (l.at - c).abs() < SAME) {
            hit(l.at, l.from, l.to);
        }
        if let Some(at) = t.grid.and_then(|s| grid_line(c, s, extent))
            && (at - c).abs() < SAME
        {
            hit(at, full.0, full.1);
        }
    }
    for (at, from, to) in hits {
        let (a, b) = (from.min(span.0), to.max(span.1));
        out.push(if vertical {
            Guide::Line(Point::new(at, a), Point::new(at, b))
        } else {
            Guide::Line(Point::new(a, at), Point::new(b, at))
        });
    }
}

/// Snaps a rectangle that a drag moves or resizes. A move snaps the left
/// edge, the center, or the right edge, and the same on the y axis. A resize
/// snaps only the edges that the handle moves.
pub fn snap_rect(r: Rect, moving: Moving, t: &Targets, reach: f64) -> (Rect, Vec<Guide>) {
    let c = r.center();
    let (xs, ys): (Vec<f64>, Vec<f64>) = match moving {
        Moving::Body => (vec![r.x0, c.x, r.x1], vec![r.y0, c.y, r.y1]),
        Moving::Handle(h) => {
            let x = match h {
                0 | 6 | 7 => vec![r.x0],
                2..=4 => vec![r.x1],
                _ => vec![],
            };
            let y = match h {
                0..=2 => vec![r.y0],
                4..=6 => vec![r.y1],
                _ => vec![],
            };
            (x, y)
        }
    };
    let (lx, ly) = t.lines();
    let dx = axis_delta(&xs, &lx, t.grid, t.page.x1, reach);
    let dy = axis_delta(&ys, &ly, t.grid, t.page.y1, reach);
    let mut out = r;
    match moving {
        Moving::Body => out = out + Vec2::new(dx.unwrap_or(0.0), dy.unwrap_or(0.0)),
        Moving::Handle(h) => {
            if let Some(d) = dx {
                if matches!(h, 0 | 6 | 7) {
                    out.x0 += d;
                } else {
                    out.x1 += d;
                }
            }
            if let Some(d) = dy {
                if matches!(h, 0..=2) {
                    out.y0 += d;
                } else {
                    out.y1 += d;
                }
            }
        }
    }
    let mut guides = Vec::new();
    let oc = out.center();
    let x_cands: Vec<f64> = match (moving, dx) {
        (_, None) => Vec::new(),
        (Moving::Body, Some(_)) => vec![out.x0, oc.x, out.x1],
        (Moving::Handle(0 | 6 | 7), Some(_)) => vec![out.x0],
        (Moving::Handle(_), Some(_)) => vec![out.x1],
    };
    let y_cands: Vec<f64> = match (moving, dy) {
        (_, None) => Vec::new(),
        (Moving::Body, Some(_)) => vec![out.y0, oc.y, out.y1],
        (Moving::Handle(0..=2), Some(_)) => vec![out.y0],
        (Moving::Handle(_), Some(_)) => vec![out.y1],
    };
    axis_guides(&x_cands, &lx, t, (out.y0, out.y1), true, &mut guides);
    axis_guides(&y_cands, &ly, t, (out.x0, out.x1), false, &mut guides);
    (out, guides)
}

fn nearest_on_segment(p: Point, (a, b): (Point, Point)) -> Point {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 == 0.0 {
        return a;
    }
    let s = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    a + ab * s
}

/// Snaps a vertex. A vertex of another frame within reach wins. Else the
/// vertex snaps to an edge of another frame, or its x and its y snap to the
/// lines, whichever is nearer. The lines include a vertical and a
/// horizontal line through each other vertex, so the vertex lines up with
/// it.
pub fn snap_point(p: Point, t: &Targets, reach: f64) -> (Point, Vec<Guide>) {
    let vertex = t
        .points
        .iter()
        .map(|&q| (q, q.distance(p)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((q, _)) = vertex {
        return (q, vec![Guide::Point(q)]);
    }
    let (lx, ly) = t.lines();
    let dx = axis_delta(&[p.x], &lx, t.grid, t.page.x1, reach);
    let dy = axis_delta(&[p.y], &ly, t.grid, t.page.y1, reach);
    let edge = t
        .segments
        .iter()
        .map(|&s| (s, nearest_on_segment(p, s)))
        .map(|(s, q)| (s, q, q.distance(p)))
        .filter(|(_, _, d)| *d <= reach)
        .min_by(|a, b| a.2.total_cmp(&b.2));
    let axis = dx
        .into_iter()
        .chain(dy)
        .map(f64::abs)
        .fold(f64::MAX, f64::min);
    if let Some((s, q, d)) = edge
        && d < axis - SAME
    {
        return (q, vec![Guide::Line(s.0, s.1)]);
    }
    let out = Point::new(p.x + dx.unwrap_or(0.0), p.y + dy.unwrap_or(0.0));
    let mut guides = Vec::new();
    let xc: Vec<f64> = dx.map(|_| out.x).into_iter().collect();
    let yc: Vec<f64> = dy.map(|_| out.y).into_iter().collect();
    axis_guides(&xc, &lx, t, (out.y, out.y), true, &mut guides);
    axis_guides(&yc, &ly, t, (out.x, out.x), false, &mut guides);
    (out, guides)
}

/// The directions of a locked edge: horizontal, 45 degrees, vertical, and
/// 135 degrees.
const DIRECTIONS: [Vec2; 4] = [
    Vec2::new(1.0, 0.0),
    Vec2::new(
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    Vec2::new(0.0, 1.0),
    Vec2::new(
        -std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
];

/// The intersection of two lines, each as a point and a direction.
fn cross_lines(a: Point, u: Vec2, b: Point, v: Vec2) -> Option<Point> {
    let den = u.cross(v);
    if den.abs() < 1e-9 {
        return None;
    }
    Some(a + u * ((b - a).cross(v) / den))
}

/// Moves a vertex so that its edge to one of its neighbors is horizontal,
/// vertical, or at 45 degrees, whichever is nearest to the pen. On the
/// locked line, the vertex then snaps to the nearest crossing with a layout
/// line, a grid line, or a locked line of the other neighbor. `t` is `None`
/// when snapping is off.
pub fn lock_angle(
    p: Point,
    neighbors: &[Point],
    t: Option<&Targets>,
    reach: f64,
) -> (Point, Vec<Guide>) {
    let best = neighbors
        .iter()
        .flat_map(|&n| DIRECTIONS.map(|u| (n, u)))
        .map(|(n, u)| {
            let q = n + u * (p - n).dot(u);
            (n, u, q, q.distance(p))
        })
        .min_by(|a, b| a.3.total_cmp(&b.3));
    let Some((anchor, u, mut q, _)) = best else {
        return (p, Vec::new());
    };
    let mut guides = Vec::new();
    if let Some(t) = t {
        let mut crossings: Vec<(Point, Guide)> = Vec::new();
        let vertical = Vec2::new(0.0, 1.0);
        let horizontal = Vec2::new(1.0, 0.0);
        let (mut xs, mut ys) = t.lines();
        if let Some(s) = t.grid {
            let (w, h) = (t.page.x1, t.page.y1);
            xs.extend(grid_line(q.x, s, w).map(|at| Line {
                at,
                from: 0.0,
                to: h,
            }));
            ys.extend(grid_line(q.y, s, h).map(|at| Line {
                at,
                from: 0.0,
                to: w,
            }));
        }
        for l in &xs {
            if let Some(c) = cross_lines(anchor, u, Point::new(l.at, 0.0), vertical) {
                let g = Guide::Line(
                    Point::new(l.at, l.from.min(c.y)),
                    Point::new(l.at, l.to.max(c.y)),
                );
                crossings.push((c, g));
            }
        }
        for l in &ys {
            if let Some(c) = cross_lines(anchor, u, Point::new(0.0, l.at), horizontal) {
                let g = Guide::Line(
                    Point::new(l.from.min(c.x), l.at),
                    Point::new(l.to.max(c.x), l.at),
                );
                crossings.push((c, g));
            }
        }
        for &n in neighbors.iter().filter(|&&n| n != anchor) {
            for v in DIRECTIONS {
                if let Some(c) = cross_lines(anchor, u, n, v) {
                    crossings.push((c, Guide::Line(n, c)));
                }
            }
        }
        let near = crossings
            .into_iter()
            .map(|(c, g)| (c, g, c.distance(q)))
            .filter(|(_, _, d)| *d <= reach)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((c, g, _)) = near {
            q = c;
            guides.push(g);
        }
    }
    guides.insert(0, Guide::Line(anchor, q));
    (q, guides)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_store::{
        Border, FrameRecord, FrameShape, Graticule, MapId, Neatline, PageId, PageUnits,
    };

    fn frame(id: u32, shape: FrameShape) -> FrameRecord {
        FrameRecord {
            id: FrameId(id),
            map: MapId(0),
            shape,
            center: [0.0, 0.0],
            scale: 1e7,
            rotation: 0.0,
            graticule: Graticule::default(),
            neatline: Neatline::default(),
            border: Border::default(),
        }
    }

    /// A 300 by 200 page with a frame from 20, 20 to 120, 100.
    fn page() -> PageRecord {
        PageRecord {
            id: PageId(0),
            name: String::new(),
            width_mm: 300.0,
            height_mm: 200.0,
            units: PageUnits::Mm,
            paper: [0xff; 3],
            frames: vec![
                frame(0, FrameShape::Rectangle([20.0, 20.0, 120.0, 100.0])),
                frame(1, FrameShape::Rectangle([200.0, 150.0, 240.0, 180.0])),
            ],
            furniture: Vec::new(),
        }
    }

    #[test]
    fn a_moved_rect_snaps_to_the_page_center_and_another_frame() {
        let t = targets(&page(), FrameId(1), None, None);
        // The left edge is 2 mm from the page center, and the top edge is 1
        // mm from the bottom of the other frame.
        let r = Rect::new(152.0, 101.0, 192.0, 131.0);
        let (out, guides) = snap_rect(r, Moving::Body, &t, 3.0);
        assert_eq!(out, Rect::new(150.0, 100.0, 190.0, 130.0));
        assert!(guides.contains(&Guide::Line(
            Point::new(150.0, 0.0),
            Point::new(150.0, 200.0)
        )));
        // The frame edge is on the page center, so one guide spans both.
        assert!(guides.contains(&Guide::Line(
            Point::new(0.0, 100.0),
            Point::new(300.0, 100.0)
        )));
    }

    #[test]
    fn a_moved_rect_snaps_by_its_center_and_to_the_page_edges() {
        let t = targets(&page(), FrameId(1), None, None);
        // The center is 1 mm right of the center of the other frame.
        let (out, _) = snap_rect(Rect::new(51.0, 196.0, 91.0, 198.5), Moving::Body, &t, 3.0);
        assert_eq!(out, Rect::new(50.0, 197.5, 90.0, 200.0));
    }

    #[test]
    fn a_rect_out_of_reach_does_not_snap() {
        let t = targets(&page(), FrameId(1), None, None);
        let r = Rect::new(155.0, 105.0, 175.0, 125.0);
        let (out, guides) = snap_rect(r, Moving::Body, &t, 3.0);
        assert_eq!((out, guides.len()), (r, 0));
    }

    #[test]
    fn a_resize_snaps_only_the_edges_of_the_handle() {
        let t = targets(&page(), FrameId(1), None, None);
        // Handle 4 moves the right edge and the bottom edge. The left edge is
        // near the page center, and it stays.
        let r = Rect::new(151.0, 30.0, 298.0, 99.0);
        let (out, guides) = snap_rect(r, Moving::Handle(4), &t, 3.0);
        assert_eq!(out, Rect::new(151.0, 30.0, 300.0, 100.0));
        // The bottom edge is on the page center and on the other frame, and
        // one guide shows both.
        assert_eq!(guides.len(), 2);
        // Handle 1 moves only the top edge.
        let r = Rect::new(151.0, 22.0, 298.0, 99.0);
        let (out, _) = snap_rect(r, Moving::Handle(1), &t, 3.0);
        assert_eq!(out, Rect::new(151.0, 20.0, 298.0, 99.0));
    }

    #[test]
    fn a_rect_snaps_to_the_margins_of_a_template() {
        let margins = Rect::new(15.0, 15.0, 285.0, 185.0);
        let t = targets(&page(), FrameId(1), Some(margins), None);
        let r = Rect::new(240.0, 170.0, 284.0, 186.5);
        let (out, _) = snap_rect(r, Moving::Body, &t, 3.0);
        assert_eq!(out, Rect::new(241.0, 168.5, 285.0, 185.0));
    }

    #[test]
    fn a_rect_snaps_to_the_layout_grid() {
        let t = targets(&page(), FrameId(1), None, Some(25.0));
        let r = Rect::new(161.0, 113.0, 191.0, 133.0);
        let (out, guides) = snap_rect(r, Moving::Body, &t, 3.0);
        // The center snaps to the line at 175, and the center of the y axis
        // snaps to the line at 125.
        assert_eq!(out, Rect::new(160.0, 115.0, 190.0, 135.0));
        assert!(!guides.is_empty());
    }

    #[test]
    fn a_vertex_snaps_to_a_vertex_before_an_edge() {
        let t = targets(&page(), FrameId(9), None, None);
        let (out, guides) = snap_point(Point::new(121.0, 98.5), &t, 3.0);
        assert_eq!(out, Point::new(120.0, 100.0));
        assert_eq!(guides, [Guide::Point(Point::new(120.0, 100.0))]);
    }

    #[test]
    fn a_vertex_snaps_to_an_edge_of_another_frame() {
        let t = targets(&page(), FrameId(9), None, None);
        let (out, guides) = snap_point(Point::new(76.0, 21.0), &t, 3.0);
        assert_eq!(out, Point::new(76.0, 20.0));
        assert_eq!(guides.len(), 1);
    }

    #[test]
    fn a_vertex_snaps_to_the_page_edges_and_center_lines() {
        let t = targets(&page(), FrameId(9), None, None);
        let (out, guides) = snap_point(Point::new(148.0, 198.0), &t, 3.0);
        assert_eq!(out, Point::new(150.0, 200.0));
        assert_eq!(guides.len(), 2);
    }

    #[test]
    fn a_vertex_lines_up_with_the_other_vertices_of_its_frame() {
        let mut t = targets(&page(), FrameId(9), None, None);
        t.align = vec![Point::new(230.0, 40.0), Point::new(265.0, 70.0)];
        // The x lines up with the first vertex and the y with the second.
        let (out, guides) = snap_point(Point::new(231.5, 68.0), &t, 3.0);
        assert_eq!(out, Point::new(230.0, 70.0));
        // Each guide goes from the vertex that it lines up with.
        assert!(guides.contains(&Guide::Line(
            Point::new(230.0, 40.0),
            Point::new(230.0, 70.0)
        )));
        assert!(guides.contains(&Guide::Line(
            Point::new(230.0, 70.0),
            Point::new(265.0, 70.0)
        )));
    }

    #[test]
    fn a_vertex_lines_up_with_a_vertex_of_another_frame() {
        let t = targets(&page(), FrameId(9), None, None);
        // The corner of the frame at 200, 150 is far, and the vertex lines
        // up with its x.
        let (out, guides) = snap_point(Point::new(201.0, 45.0), &t, 3.0);
        assert_eq!(out, Point::new(200.0, 45.0));
        assert_eq!(guides.len(), 1);
    }

    #[test]
    fn a_locked_edge_is_horizontal_vertical_or_diagonal() {
        let n = Point::new(100.0, 100.0);
        let (q, _) = lock_angle(Point::new(140.0, 104.0), &[n], None, 3.0);
        assert_eq!(q, Point::new(140.0, 100.0));
        let (q, _) = lock_angle(Point::new(97.0, 160.0), &[n], None, 3.0);
        assert_eq!(q, Point::new(100.0, 160.0));
        let (q, _) = lock_angle(Point::new(131.0, 129.0), &[n], None, 3.0);
        assert!((q.x - 130.0).abs() < 1e-9 && (q.y - 130.0).abs() < 1e-9);
    }

    #[test]
    fn a_locked_vertex_slides_to_a_layout_line() {
        let t = targets(&page(), FrameId(9), None, None);
        let n = Point::new(100.0, 60.0);
        // The edge is horizontal, and the vertex slides to the page center.
        let (q, guides) = lock_angle(Point::new(152.0, 63.0), &[n], Some(&t), 3.0);
        assert_eq!(q, Point::new(150.0, 60.0));
        assert_eq!(guides.len(), 2);
    }
}
