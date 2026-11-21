//! Weighted obstacles: the shapes that labels avoid, with a cost or a ban for each.

use crate::collide::{GridIndex, OrientedBox};
use crate::model::{Obstacle, Weight};
use kurbo::{Point, Rect};

enum Shape {
    Box(OrientedBox),
    Disc(Point, f64),
    /// A segment and its half width.
    Segment(Point, Point, f64),
    /// The inside of a polygon of `areas`.
    Area(usize),
}

struct Part {
    shape: Shape,
    weight: Weight,
    /// The parts with one key add their cost one time.
    key: u32,
    /// The feature that owns the part. Its own labels ignore it.
    owner: Option<usize>,
}

struct Area {
    rings: Vec<Vec<Point>>,
    bbox: Rect,
}

/// All obstacles of one run, in a grid.
pub(crate) struct Obstacles {
    parts: Vec<Part>,
    areas: Vec<Area>,
    grid: GridIndex,
    keys: u32,
}

impl Obstacles {
    pub fn new(bounds: Rect) -> Self {
        Obstacles {
            parts: Vec::new(),
            areas: Vec::new(),
            grid: GridIndex::new(bounds, 32.0),
            keys: 0,
        }
    }

    fn key(&mut self) -> u32 {
        self.keys += 1;
        self.keys
    }

    fn push(&mut self, shape: Shape, weight: Weight, key: u32, owner: Option<usize>) {
        if weight == Weight::FREE {
            return;
        }
        let aabb = match &shape {
            Shape::Box(b) => b.aabb(),
            Shape::Disc(c, r) => Rect::new(c.x - r, c.y - r, c.x + r, c.y + r),
            Shape::Segment(a, b, hw) => Rect::from_points(*a, *b).inflate(*hw, *hw),
            Shape::Area(i) => self.areas[*i].bbox,
        };
        self.grid.insert(self.parts.len() as u32, aabb);
        self.parts.push(Part {
            shape,
            weight,
            key,
            owner,
        });
    }

    /// Adds an obstacle of the input.
    pub fn add(&mut self, o: &Obstacle) {
        match o {
            Obstacle::Point {
                position,
                radius,
                weight,
            } => {
                let key = self.key();
                self.push(Shape::Disc(*position, *radius), *weight, key, None);
            }
            Obstacle::Line {
                path,
                width,
                weight,
            } => {
                let key = self.key();
                self.add_line(path, width * 0.5, *weight, key, None);
            }
            Obstacle::Polygon {
                rings,
                inside,
                outline,
            } => {
                if let [r] = rings.as_slice()
                    && inside == outline
                    && let Some(rect) = axis_rect(r)
                {
                    let key = self.key();
                    self.push(Shape::Box(OrientedBox::from_rect(rect)), *inside, key, None);
                } else {
                    self.add_polygon(rings, *inside, *outline, None);
                }
            }
        }
    }

    pub fn add_box(&mut self, b: OrientedBox, weight: Weight, owner: Option<usize>) {
        let key = self.key();
        self.push(Shape::Box(b), weight, key, owner);
    }

    /// Adds a line. Pass the same `key` for each line of one feature.
    pub fn add_line(
        &mut self,
        path: &[Point],
        half_width: f64,
        weight: Weight,
        key: u32,
        owner: Option<usize>,
    ) {
        for w in path.windows(2) {
            self.push(Shape::Segment(w[0], w[1], half_width), weight, key, owner);
        }
    }

    /// A new key for [`Obstacles::add_line`].
    pub fn line_key(&mut self) -> u32 {
        self.key()
    }

    pub fn add_polygon(
        &mut self,
        rings: &[Vec<Point>],
        inside: Weight,
        outline: Weight,
        owner: Option<usize>,
    ) {
        let mut pts = rings.iter().flatten();
        let Some(first) = pts.next() else { return };
        let bbox = pts.fold(Rect::from_points(*first, *first), |b, p| b.union_pt(*p));
        if inside != Weight::FREE {
            self.areas.push(Area {
                rings: rings.to_vec(),
                bbox,
            });
            let key = self.key();
            self.push(Shape::Area(self.areas.len() - 1), inside, key, owner);
        }
        let key = self.key();
        for ring in rings {
            let n = ring.len();
            for k in 0..n {
                self.push(
                    Shape::Segment(ring[k], ring[(k + 1) % n], 0.0),
                    outline,
                    key,
                    owner,
                );
            }
        }
    }

    /// The cost of a label with `boxes` for feature `own`, or `None` if a forbidden obstacle
    /// blocks it. `hits` and `keys` are scratch space.
    pub fn cost(
        &self,
        own: usize,
        boxes: &[OrientedBox],
        hits: &mut Vec<u32>,
        keys: &mut Vec<u32>,
    ) -> Option<f64> {
        let Some(aabb) = boxes.iter().map(|b| b.aabb()).reduce(|a, b| a.union(b)) else {
            return Some(0.0);
        };
        self.grid.query(aabb, hits);
        keys.clear();
        let mut cost = 0.0;
        for &h in hits.iter() {
            let p = &self.parts[h as usize];
            if p.owner == Some(own) || keys.contains(&p.key) || !self.hit(p, boxes, aabb) {
                continue;
            }
            match p.weight {
                Weight::Forbidden => return None,
                Weight::Cost(c) => {
                    keys.push(p.key);
                    cost += c;
                }
            }
        }
        Some(cost)
    }

    /// True if the segment `a`-`b` crosses a forbidden obstacle that `own` does not own.
    pub fn blocks_segment(&self, own: usize, a: Point, b: Point, hits: &mut Vec<u32>) -> bool {
        self.grid.query(Rect::from_points(a, b), hits);
        hits.iter().any(|&h| {
            let p = &self.parts[h as usize];
            p.weight == Weight::Forbidden
                && p.owner != Some(own)
                && match &p.shape {
                    Shape::Box(o) => o.intersects_segment(a, b),
                    Shape::Disc(c, r) => segment_distance(a, b, *c) < *r,
                    Shape::Segment(s, e, hw) => segments_near(a, b, *s, *e, *hw),
                    Shape::Area(i) => {
                        let area = &self.areas[*i];
                        in_rings(&area.rings, a)
                            || in_rings(&area.rings, b)
                            || edges(&area.rings).any(|(s, e)| segments_near(a, b, s, e, 0.0))
                    }
                }
        })
    }

    fn hit(&self, p: &Part, boxes: &[OrientedBox], aabb: Rect) -> bool {
        match &p.shape {
            Shape::Box(o) => boxes.iter().any(|b| o.intersects(b)),
            Shape::Disc(c, r) => boxes.iter().any(|b| b.distance_to_point(*c) < *r),
            Shape::Segment(s, e, hw) => boxes.iter().any(|b| {
                if *hw > 0.0 {
                    b.inflate(*hw).intersects_segment(*s, *e)
                } else {
                    b.intersects_segment(*s, *e)
                }
            }),
            Shape::Area(i) => {
                let area = &self.areas[*i];
                boxes.iter().any(|b| in_rings(&area.rings, b.center))
                    || edges(&area.rings).any(|(s, e)| {
                        let r = Rect::from_points(s, e);
                        r.x0 <= aabb.x1
                            && r.x1 >= aabb.x0
                            && r.y0 <= aabb.y1
                            && r.y1 >= aabb.y0
                            && boxes.iter().any(|b| b.intersects_segment(s, e))
                    })
            }
        }
    }
}

/// The rectangle of a ring of four corners with axis-aligned sides.
fn axis_rect(r: &[Point]) -> Option<Rect> {
    let r = match r {
        [a, .., z] if a == z => &r[..r.len() - 1],
        _ => r,
    };
    let [a, b, c, d] = r else { return None };
    let rect = Rect::from_points(*a, *c);
    let corners = [
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x1, rect.y1),
        Point::new(rect.x0, rect.y1),
    ];
    [a, b, c, d]
        .iter()
        .all(|p| corners.contains(p))
        .then_some(rect)
        .filter(|_| (a.x == b.x) != (a.y == b.y))
}

fn edges(rings: &[Vec<Point>]) -> impl Iterator<Item = (Point, Point)> + '_ {
    rings.iter().flat_map(|r| {
        let n = r.len();
        (0..n).map(move |k| (r[k], r[(k + 1) % n]))
    })
}

/// Even-odd point in polygon.
fn in_rings(rings: &[Vec<Point>], p: Point) -> bool {
    let mut inside = false;
    for (a, b) in edges(rings) {
        if (a.y <= p.y) != (b.y <= p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

fn segment_distance(a: Point, b: Point, p: Point) -> f64 {
    let d = b - a;
    let len2 = d.hypot2();
    let t = if len2 > 0.0 {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t).distance(p)
}

/// True if the segments `a`-`b` and `c`-`d` cross or come nearer than `gap`.
fn segments_near(a: Point, b: Point, c: Point, d: Point, gap: f64) -> bool {
    let cross = |o: Point, p: Point, q: Point| (p - o).cross(q - o);
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    if (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) {
        return true;
    }
    [
        segment_distance(c, d, a),
        segment_distance(c, d, b),
        segment_distance(a, b, c),
        segment_distance(a, b, d),
    ]
    .into_iter()
    .any(|x| x < gap.max(1e-9))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(x: f64, y: f64) -> Vec<OrientedBox> {
        vec![OrientedBox::from_rect(Rect::new(x, y, x + 10.0, y + 4.0))]
    }

    #[test]
    fn polygon_inside_and_outline() {
        let mut o = Obstacles::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let ring = vec![
            Point::new(50.0, 50.0),
            Point::new(150.0, 50.0),
            Point::new(100.0, 150.0),
        ];
        o.add_polygon(&[ring], Weight::Cost(1.0), Weight::Forbidden, None);
        let (mut h, mut k) = (Vec::new(), Vec::new());
        assert_eq!(o.cost(0, &label(95.0, 70.0), &mut h, &mut k), Some(1.0));
        assert_eq!(o.cost(0, &label(45.0, 48.0), &mut h, &mut k), None);
        assert_eq!(o.cost(0, &label(10.0, 10.0), &mut h, &mut k), Some(0.0));
    }

    #[test]
    fn rect_polygon_is_a_box() {
        let mut o = Obstacles::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        o.add(&Obstacle::rect(
            Rect::new(20.0, 20.0, 40.0, 40.0),
            Weight::Forbidden,
        ));
        assert!(matches!(o.parts[0].shape, Shape::Box(_)));
        let (mut h, mut k) = (Vec::new(), Vec::new());
        // Touching edges do not intersect.
        assert_eq!(o.cost(0, &label(40.0, 25.0), &mut h, &mut k), Some(0.0));
        assert_eq!(o.cost(0, &label(35.0, 25.0), &mut h, &mut k), None);
    }

    #[test]
    fn wide_line_and_owner() {
        let mut o = Obstacles::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let key = o.line_key();
        let path = [Point::new(0.0, 100.0), Point::new(200.0, 100.0)];
        o.add_line(&path, 3.0, Weight::Cost(2.0), key, Some(7));
        let (mut h, mut k) = (Vec::new(), Vec::new());
        // 2 units above the line, inside its half width.
        assert_eq!(o.cost(0, &label(50.0, 94.0), &mut h, &mut k), Some(2.0));
        assert_eq!(o.cost(0, &label(50.0, 90.0), &mut h, &mut k), Some(0.0));
        assert_eq!(o.cost(7, &label(50.0, 98.0), &mut h, &mut k), Some(0.0));
    }

    #[test]
    fn point_disc() {
        let mut o = Obstacles::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        o.add(&Obstacle::Point {
            position: Point::new(100.0, 100.0),
            radius: 5.0,
            weight: Weight::Forbidden,
        });
        let (mut h, mut k) = (Vec::new(), Vec::new());
        assert_eq!(o.cost(0, &label(103.0, 98.0), &mut h, &mut k), None);
        assert_eq!(o.cost(0, &label(106.0, 98.0), &mut h, &mut k), Some(0.0));
        assert!(o.blocks_segment(0, Point::new(90.0, 100.0), Point::new(110.0, 100.0), &mut h));
    }
}
