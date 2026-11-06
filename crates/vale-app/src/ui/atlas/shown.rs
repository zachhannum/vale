//! The frames of the open page on the screen, for the overlays of the tools
//! that work on the world: Measure and Select.

use eframe::egui::{Pos2, pos2};
use kurbo::Point;
use vale_sphere::{LonLat, Projection};
use vale_store::{FrameRecord, PageRecord};

use super::{AppState, PagePlace, math};
use crate::view::ViewTransform;

fn screen(p: Point) -> Pos2 {
    pos2(p.x as f32, p.y as f32)
}

/// A frame of the page on the screen: the frame, the frames above it, its
/// projection, and its transform.
pub(super) struct Shown<'a> {
    pub frame: &'a FrameRecord,
    above: &'a [FrameRecord],
    projection: &'a Projection,
    vt: ViewTransform,
    place: PagePlace,
}

impl Shown<'_> {
    /// True if the frame shows the screen position: it is in the shape,
    /// and no frame above covers it.
    pub fn shows(&self, pos: Pos2) -> bool {
        let p = self.place.to_mm(pos);
        math::contains(&self.frame.shape, p)
            && !self.above.iter().any(|f| math::contains(&f.shape, p))
    }

    /// The screen position of a place, if the frame shows it.
    pub fn at(&self, p: LonLat) -> Option<Pos2> {
        let pos = screen(self.vt.to_page(self.projection.forward(p)?));
        self.shows(pos).then_some(pos)
    }

    /// The place at a screen position, if the frame shows it.
    pub fn place_at(&self, pos: Pos2) -> Option<LonLat> {
        let q = Point::new(f64::from(pos.x), f64::from(pos.y));
        self.shows(pos)
            .then(|| self.projection.inverse(self.vt.from_page(q)))
            .flatten()
    }

    /// The parts on screen of a path of places that the frame shows.
    pub fn lines(&self, path: &[LonLat]) -> Vec<Vec<Pos2>> {
        self.clipped(self.projection.project_line(path))
    }

    /// The parts on screen of the outline of a polygon that the frame
    /// shows.
    pub fn outline(&self, rings: &[Vec<LonLat>]) -> Vec<Vec<Pos2>> {
        self.clipped(self.projection.project_outline_of(rings))
    }

    /// The parts of projected lines that the frame shows, on screen.
    fn clipped(&self, parts: Vec<Vec<Point>>) -> Vec<Vec<Pos2>> {
        let mut edges = vec![&self.frame.shape];
        edges.extend(self.above.iter().map(|f| &f.shape));
        let rings: Vec<Vec<Pos2>> = edges
            .iter()
            .flat_map(|shape| shape.rings())
            .map(|ring| {
                let at = |p: &[f64; 2]| self.place.to_screen(Point::new(p[0], p[1]));
                ring.iter().map(at).collect()
            })
            .collect();
        let mut out = Vec::new();
        for part in parts {
            let line: Vec<Pos2> = part.iter().map(|p| screen(self.vt.to_page(*p))).collect();
            out.extend(clip(&line, &rings, |p| self.shows(p)));
        }
        out
    }
}

/// Calls `f` with each frame of the page, from the bottom.
pub(super) fn each_frame(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    mut f: impl FnMut(&Shown),
) {
    for (i, frame) in page.frames.iter().enumerate() {
        let Ok(projection) = state.pipeline.projection(&state.doc, frame.map) else {
            continue;
        };
        let vt = math::frame_transform(projection, frame, place.target());
        f(&Shown {
            frame,
            above: &page.frames[i + 1..],
            projection,
            vt,
            place,
        });
    }
}

/// The point where the segment `a`–`b` crosses the segment `c`–`d`, as a
/// fraction of `a`–`b`.
fn crossing(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> Option<f32> {
    let (r, s) = (b - a, d - c);
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let ca = c - a;
    let t = (ca.x * s.y - ca.y * s.x) / den;
    let u = (ca.x * r.y - ca.y * r.x) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// The parts of a screen line where `inside` is true. `rings` are the
/// edges where `inside` changes.
fn clip(line: &[Pos2], rings: &[Vec<Pos2>], inside: impl Fn(Pos2) -> bool) -> Vec<Vec<Pos2>> {
    let mut out = Vec::new();
    let mut part: Vec<Pos2> = Vec::new();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut ts = vec![0.0, 1.0];
        for ring in rings {
            for i in 0..ring.len() {
                let (c, d) = (ring[i], ring[(i + 1) % ring.len()]);
                ts.extend(crossing(a, b, c, d));
            }
        }
        ts.sort_by(f32::total_cmp);
        for t in ts.windows(2) {
            if t[1] - t[0] < 1e-6 {
                continue;
            }
            if inside(a.lerp(b, (t[0] + t[1]) / 2.0)) {
                let (p0, p1) = (a.lerp(b, t[0]), a.lerp(b, t[1]));
                if part.last().is_none_or(|last| last.distance(p0) > 1e-3) {
                    if part.len() > 1 {
                        out.push(std::mem::take(&mut part));
                    }
                    part = vec![p0];
                }
                part.push(p1);
            } else if part.len() > 1 {
                out.push(std::mem::take(&mut part));
            } else {
                part.clear();
            }
        }
    }
    if part.len() > 1 {
        out.push(part);
    }
    out
}
