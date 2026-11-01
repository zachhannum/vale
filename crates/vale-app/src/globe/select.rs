//! The hit test of the Select tool. It works in screen points, on the globe
//! and on the flat map. A tap hits the nearest point or line in reach, else
//! the top polygon under it. A box meets a feature if a shown vertex is in
//! the box, a shown edge crosses it, or the polygon holds the box.

use eframe::egui::{Pos2, Rect};
use vale_store::{Geometry, Layer, LayerId, Uuid};

use super::Globe;
use super::brush::slerp;
use super::math::{V3, angle, lonlat_to_dir};
use super::vector;
use crate::measure::TAP_SLOP;

/// The largest angle between two sampled points of an edge, in radians.
const SAMPLE: f64 = 0.0175;

/// One feature of one layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureRef {
    pub layer: LayerId,
    pub uuid: Uuid,
}

/// The thing that taps or drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Finger,
    Pen,
    Mouse,
}

impl Pointer {
    /// The distance in points at which a tap hits a point or a line.
    pub fn reach(self) -> f32 {
        match self {
            Pointer::Finger => 22.0,
            Pointer::Pen => 12.0,
            Pointer::Mouse => 8.0,
        }
    }
}

/// A feature on screen.
#[derive(Default)]
pub struct Shape {
    /// The shown vertices of the points, with their index in the feature.
    pub points: Vec<(usize, Pos2)>,
    /// The shown runs of the lines and of the outlines of the polygons.
    pub lines: Vec<Vec<Pos2>>,
    /// The rings of each polygon on the sphere, for the inside test.
    pub polygons: Vec<Vec<Vec<V3>>>,
}

/// The screen shape of a geometry.
pub fn shape(globe: &Globe, geometry: &Geometry) -> Shape {
    let mut out = Shape::default();
    match geometry {
        Geometry::Points(points) => {
            for (i, p) in points.iter().enumerate() {
                if let Some(pos) = globe.project(lonlat_to_dir(p[0], p[1])) {
                    out.points.push((i, pos));
                }
            }
        }
        Geometry::Lines(lines) => {
            for line in lines {
                runs(globe, &vector::line(line), false, &mut out.lines);
            }
        }
        Geometry::Polygons(polygons) => {
            for polygon in polygons {
                let rings: Vec<_> = polygon
                    .iter()
                    .enumerate()
                    .map(|(i, ring)| vector::ring(ring, i > 0))
                    .filter(|r| !r.is_empty())
                    .collect();
                for ring in &rings {
                    runs(globe, ring, true, &mut out.lines);
                }
                if !rings.is_empty() {
                    out.polygons.push(rings);
                }
            }
        }
    }
    out
}

/// Samples the edges of a path on screen, and breaks the run where the path
/// goes out of view or across the edge of the flat map.
fn runs(globe: &Globe, path: &[V3], closed: bool, out: &mut Vec<Vec<Pos2>>) {
    let n = path.len();
    let edges = if closed && n > 2 {
        n
    } else {
        n.saturating_sub(1)
    };
    let jump = globe.rect.width() / 2.0;
    let mut run: Vec<Pos2> = Vec::new();
    let mut push = |pos: Option<Pos2>, run: &mut Vec<Pos2>| match pos {
        Some(pos) if run.last().is_none_or(|last| last.distance(pos) < jump) => run.push(pos),
        _ => {
            if run.len() > 1 {
                out.push(std::mem::take(run));
            }
            run.clear();
            if let Some(pos) = pos {
                run.push(pos);
            }
        }
    };
    if n == 1 {
        push(globe.project(path[0]), &mut run);
    }
    for i in 0..edges {
        let (a, b) = (path[i], path[(i + 1) % n]);
        let steps = (angle(a, b) / SAMPLE).ceil().max(1.0) as usize;
        let start = if i == 0 { 0 } else { 1 };
        for s in start..=steps {
            let dir = slerp(a, b, s as f64 / steps as f64);
            push(globe.project(dir), &mut run);
        }
    }
    push(None, &mut run);
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len = ab.length_sq();
    let t = if len > 0.0 {
        ((p - a).dot(ab) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

impl Shape {
    /// The distance from a screen position to the nearest point, line, or
    /// outline.
    fn distance(&self, at: Pos2) -> f32 {
        let points = self.points.iter().map(|(_, p)| p.distance(at));
        let lines = self.lines.iter().flat_map(|run| {
            run.windows(2)
                .map(move |w| segment_distance(at, w[0], w[1]))
        });
        points.chain(lines).fold(f32::INFINITY, f32::min)
    }

    fn inside(&self, dir: Option<V3>) -> bool {
        dir.is_some_and(|d| self.polygons.iter().any(|rings| vector::contains(rings, d)))
    }

    fn meets(&self, globe: &Globe, rect: Rect) -> bool {
        self.points.iter().any(|(_, p)| rect.contains(*p))
            || self
                .lines
                .iter()
                .any(|run| run.windows(2).any(|w| crosses(rect, w[0], w[1])))
            || self.inside(globe.unproject(rect.center()))
    }
}

/// A segment crosses or lies in a rectangle.
fn crosses(rect: Rect, a: Pos2, b: Pos2) -> bool {
    if rect.contains(a) || rect.contains(b) {
        return true;
    }
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    (0..4).any(|i| segments_cross(a, b, corners[i], corners[(i + 1) % 4]))
}

fn segments_cross(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let side = |p: Pos2, q: Pos2, r: Pos2| (q - p).x * (r - p).y - (q - p).y * (r - p).x;
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0
}

/// The feature that a tap at `at` hits. The layers are in the draw order,
/// top first. The nearest point or line in reach wins. Else the top polygon
/// under the tap wins.
pub fn hit(globe: &Globe, layers: &[&Layer], at: Pos2, reach: f32) -> Option<FeatureRef> {
    let dir = globe.unproject(at);
    let mut nearest: Option<(f32, FeatureRef)> = None;
    let mut under = None;
    for layer in layers.iter().filter(|l| l.visible) {
        for feature in &layer.features {
            let shape = shape(globe, &feature.geometry);
            let found = FeatureRef {
                layer: layer.id,
                uuid: feature.uuid(),
            };
            let d = shape.distance(at);
            if d <= reach && nearest.is_none_or(|(best, _)| d < best) {
                nearest = Some((d, found));
            }
            if under.is_none() && shape.inside(dir) {
                under = Some(found);
            }
        }
    }
    nearest.map(|(_, f)| f).or(under)
}

/// The features that a box meets, in the draw order, top first.
pub fn in_box(globe: &Globe, layers: &[&Layer], rect: Rect) -> Vec<FeatureRef> {
    let mut out = Vec::new();
    for layer in layers.iter().filter(|l| l.visible) {
        for feature in &layer.features {
            if shape(globe, &feature.geometry).meets(globe, rect) {
                out.push(FeatureRef {
                    layer: layer.id,
                    uuid: feature.uuid(),
                });
            }
        }
    }
    out
}

/// The index of the vertex of a point feature in reach of `at`.
pub fn hit_vertex(globe: &Globe, layer: &Layer, uuid: Uuid, at: Pos2, reach: f32) -> Option<usize> {
    let feature = layer.features.iter().find(|f| f.uuid() == uuid)?;
    if !matches!(feature.geometry, Geometry::Points(_)) {
        return None;
    }
    let shape = shape(globe, &feature.geometry);
    let near = shape.points.iter().map(|&(i, p)| (i, p.distance(at)));
    near.filter(|&(_, d)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A rectangle from two corners.
pub fn drag_rect(a: Pos2, b: Pos2) -> Rect {
    Rect::from_two_pos(a, b)
}

/// What a press of the Select tool did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectEvent {
    Tap {
        pos: Pos2,
        pointer: Pointer,
    },
    /// A pen or the mouse moved past the slop. `pos` is where it went down.
    DragStart {
        pos: Pos2,
        pointer: Pointer,
    },
    Drag {
        pos: Pos2,
    },
    DragEnd {
        pos: Pos2,
    },
    /// The drag ends with no effect.
    Cancel,
}

#[derive(Debug)]
struct Press {
    id: u64,
    start: Pos2,
    pointer: Pointer,
    dragging: bool,
    broken: bool,
}

/// One press of the Select tool. A tap of any pointer selects. A drag of a
/// pen or the mouse is a box or a move, and a drag of a finger is left to
/// the view. A second finger ends the press. A finger while a pen is down
/// is a palm.
#[derive(Debug, Default)]
pub struct SelectPress {
    press: Option<Press>,
    pub events: Vec<SelectEvent>,
}

impl SelectPress {
    pub fn down(&mut self, id: u64, pos: Pos2, pointer: Pointer) {
        match &mut self.press {
            Some(p) if p.pointer == Pointer::Pen && pointer == Pointer::Finger => {}
            Some(p) => {
                if p.dragging && !p.broken {
                    self.events.push(SelectEvent::Cancel);
                }
                p.broken = true;
            }
            None => {
                self.press = Some(Press {
                    id,
                    start: pos,
                    pointer,
                    dragging: false,
                    broken: false,
                });
            }
        }
    }

    pub fn moved(&mut self, id: u64, pos: Pos2) {
        let Some(p) = self.press.as_mut().filter(|p| p.id == id && !p.broken) else {
            return;
        };
        if !p.dragging && p.start.distance(pos) > TAP_SLOP {
            if p.pointer == Pointer::Finger {
                p.broken = true;
                return;
            }
            p.dragging = true;
            let (pos, pointer) = (p.start, p.pointer);
            self.events.push(SelectEvent::DragStart { pos, pointer });
        }
        if p.dragging {
            self.events.push(SelectEvent::Drag { pos });
        }
    }

    pub fn up(&mut self, id: u64, pos: Pos2) {
        self.moved(id, pos);
        let Some(p) = self.press.take_if(|p| p.id == id) else {
            return;
        };
        match (p.broken, p.dragging) {
            (true, _) => {}
            (false, true) => self.events.push(SelectEvent::DragEnd { pos }),
            (false, false) => self.events.push(SelectEvent::Tap {
                pos,
                pointer: p.pointer,
            }),
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(p) = self.press.take_if(|p| p.id == id)
            && p.dragging
            && !p.broken
        {
            self.events.push(SelectEvent::Cancel);
        }
    }

    pub fn is_down(&self) -> bool {
        self.press.is_some()
    }

    /// True while the mouse makes a press.
    pub fn mouse(&self) -> bool {
        self.press
            .as_ref()
            .is_some_and(|p| p.pointer == Pointer::Mouse)
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Vec2, vec2};
    use vale_store::GeometryKind;

    use super::super::snap::tests::layer;
    use super::super::view::GlobeView;
    use super::super::{Globe, WorldView};
    use super::*;

    fn globe(view: WorldView) -> Globe {
        Globe {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0)),
            view: GlobeView::centered(0.0, 0.0),
            world_view: view,
            ..Globe::default()
        }
    }

    fn at(globe: &Globe, lon: f64, lat: f64) -> Pos2 {
        globe.project(lonlat_to_dir(lon, lat)).unwrap()
    }

    fn layers() -> [Layer; 3] {
        [
            layer(
                1,
                GeometryKind::Point,
                vec![Geometry::Points(vec![[0.0, 0.0]])],
            ),
            layer(
                2,
                GeometryKind::Line,
                vec![Geometry::Lines(vec![vec![[-10.0, 20.0], [10.0, 20.0]]])],
            ),
            layer(
                3,
                GeometryKind::Polygon,
                vec![Geometry::Polygons(vec![vec![vec![
                    [-30.0, -40.0],
                    [30.0, -40.0],
                    [30.0, 10.0],
                    [-30.0, 10.0],
                ]]])],
            ),
        ]
    }

    #[test]
    fn the_reach_of_a_finger_is_a_44_point_target() {
        assert_eq!(Pointer::Finger.reach() * 2.0, 44.0);
        assert!(Pointer::Pen.reach() < Pointer::Finger.reach());
        assert!(Pointer::Mouse.reach() < Pointer::Pen.reach());
    }

    #[test]
    fn a_tap_hits_points_lines_and_polygons_in_both_views() {
        let layers = layers();
        let refs: Vec<&Layer> = layers.iter().collect();
        for view in [WorldView::Globe, WorldView::Flat] {
            let globe = globe(view);
            let id = |f: Option<FeatureRef>| f.map(|f| f.layer.0);
            let point = at(&globe, 0.0, 0.0);
            // The point wins over the polygon under it.
            assert_eq!(id(hit(&globe, &refs, point, 8.0)), Some(1), "{view:?}");
            // A finger reaches a point that a pen does not reach.
            let off = point + vec2(18.0, 0.0);
            assert_eq!(
                id(hit(&globe, &refs, off, Pointer::Finger.reach())),
                Some(1)
            );
            assert_eq!(id(hit(&globe, &refs, off, Pointer::Pen.reach())), Some(3));
            let line = at(&globe, 3.0, 20.0) + vec2(0.0, 5.0);
            assert_eq!(id(hit(&globe, &refs, line, 8.0)), Some(2), "{view:?}");
            let inside = at(&globe, -20.0, -30.0);
            assert_eq!(id(hit(&globe, &refs, inside, 8.0)), Some(3), "{view:?}");
            let empty = at(&globe, 60.0, 50.0);
            assert_eq!(hit(&globe, &refs, empty, 22.0), None, "{view:?}");
        }
    }

    #[test]
    fn a_point_on_the_far_side_of_the_globe_is_not_hit() {
        let far = layer(
            1,
            GeometryKind::Point,
            vec![Geometry::Points(vec![[180.0, 0.0]])],
        );
        let globe = globe(WorldView::Globe);
        let center = globe.rect.center();
        assert_eq!(hit(&globe, &[&far], center, 1000.0), None);
        assert!(in_box(&globe, &[&far], globe.rect).is_empty());
    }

    #[test]
    fn a_box_meets_a_crossing_line_and_a_polygon_that_holds_it() {
        let layers = layers();
        let refs: Vec<&Layer> = layers.iter().collect();
        for view in [WorldView::Globe, WorldView::Flat] {
            let globe = globe(view);
            let ids = |rect| -> Vec<u32> {
                in_box(&globe, &refs, rect)
                    .iter()
                    .map(|f| f.layer.0)
                    .collect()
            };
            // The box crosses the line between its vertices.
            let line = drag_rect(at(&globe, -2.0, 25.0), at(&globe, 2.0, 15.0));
            assert_eq!(ids(line), [2], "{view:?}");
            // The box is inside the polygon, and holds no vertex.
            let inside = drag_rect(at(&globe, -10.0, -10.0), at(&globe, -5.0, -15.0));
            assert_eq!(ids(inside), [3], "{view:?}");
            let all = drag_rect(at(&globe, -40.0, 30.0), at(&globe, 40.0, -50.0));
            assert_eq!(ids(all), [1, 2, 3], "{view:?}");
        }
    }

    #[test]
    fn a_vertex_hit_finds_the_vertex_of_a_point() {
        let layers = layers();
        let globe = globe(WorldView::Globe);
        let uuid = layers[0].features[0].uuid();
        let point = at(&globe, 0.0, 0.0);
        assert_eq!(hit_vertex(&globe, &layers[0], uuid, point, 8.0), Some(0));
        let far = point + vec2(30.0, 0.0);
        assert_eq!(hit_vertex(&globe, &layers[0], uuid, far, 22.0), None);
        let line = layers[1].features[0].uuid();
        assert_eq!(hit_vertex(&globe, &layers[1], line, point, 1000.0), None);
    }

    #[test]
    fn a_finger_taps_and_a_pen_drags() {
        let mut press = SelectPress::default();
        let a = Pos2::new(10.0, 10.0);
        let b = Pos2::new(60.0, 10.0);
        press.down(1, a, Pointer::Finger);
        press.up(1, a);
        let tap = SelectEvent::Tap {
            pos: a,
            pointer: Pointer::Finger,
        };
        assert_eq!(std::mem::take(&mut press.events), [tap]);
        // A finger drag is left to the view.
        press.down(1, a, Pointer::Finger);
        press.moved(1, b);
        press.up(1, b);
        assert!(press.events.is_empty());
        press.down(2, a, Pointer::Pen);
        // A palm does not end the pen.
        press.down(3, b, Pointer::Finger);
        press.moved(2, b);
        press.up(2, b);
        let start = SelectEvent::DragStart {
            pos: a,
            pointer: Pointer::Pen,
        };
        let drag = SelectEvent::Drag { pos: b };
        let end = SelectEvent::DragEnd { pos: b };
        assert_eq!(press.events, [start, drag, drag, end]);
        assert!(!press.is_down());
    }
}
