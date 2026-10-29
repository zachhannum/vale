//! The Measure tool in the map frames of the open page. The path is on the
//! world, so each frame of the page shows the part of the path in its map.

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, pos2};
use kurbo::Point;
use vale_sphere::measure::densify_great_circle;
use vale_sphere::{LonLat, Projection};
use vale_store::{FrameRecord, FrameShape, MapId, PageRecord};

use super::{AppState, PagePlace, frame_at, math, open_page};
use crate::globe::brush::slerp;
use crate::globe::math::{dir_to_lonlat, lonlat_to_dir};
use crate::globe::nav::on_canvas;
use crate::globe::snap::{LayerTargets, Targets};
use crate::measure::{DENSIFY_DEG, MeasurePoint, close_reach};
use crate::ui::globe::press_input;
use crate::ui::pad::theme;
use crate::view::ViewTransform;

fn screen(p: Point) -> Pos2 {
    pos2(p.x as f32, p.y as f32)
}

/// A frame of the page on the screen: its shape, the shapes of the frames
/// above it, its projection, and its transform.
struct Shown<'a> {
    shape: &'a FrameShape,
    above: &'a [FrameRecord],
    projection: &'a Projection,
    vt: ViewTransform,
    place: PagePlace,
}

impl Shown<'_> {
    /// True if the frame shows the screen position: it is in the shape,
    /// and no frame above covers it.
    fn shows(&self, pos: Pos2) -> bool {
        let p = self.place.to_mm(pos);
        math::contains(self.shape, p) && !self.above.iter().any(|f| math::contains(&f.shape, p))
    }

    /// The screen position of a place, if the frame shows it.
    fn at(&self, p: LonLat) -> Option<Pos2> {
        let pos = screen(self.vt.to_page(self.projection.forward(p)?));
        self.shows(pos).then_some(pos)
    }

    /// The parts on screen of a path of places that the frame shows.
    fn lines(&self, path: &[LonLat]) -> Vec<Vec<Pos2>> {
        let mut edges = vec![self.shape];
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
        for part in self.projection.project_line(path) {
            let line: Vec<Pos2> = part.iter().map(|p| screen(self.vt.to_page(*p))).collect();
            out.extend(clip(&line, &rings, |p| self.shows(p)));
        }
        out
    }
}

/// Calls `f` with each frame of the page, from the bottom.
fn each_frame(
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
            shape: &frame.shape,
            above: &page.frames[i + 1..],
            projection,
            vt,
            place,
        });
    }
}

/// The snap targets of the vector layers that a map shows.
fn map_targets(state: &mut AppState, map: MapId) -> Option<Targets> {
    let mut targets = Vec::new();
    for layer in state.doc.project.layers() {
        if layer.geometry().is_none() || !state.doc.shown(map, layer.id) {
            continue;
        }
        let print = LayerTargets::fingerprint(layer);
        let built = match state.layer_targets.get(&layer.id) {
            Some((old, built)) if *old == print => built.clone(),
            _ => {
                let built = Arc::new(LayerTargets::build(layer.id, layer));
                state.layer_targets.insert(layer.id, (print, built.clone()));
                built
            }
        };
        targets.push(built);
    }
    (!targets.is_empty()).then(|| Targets::new(targets, Vec::new()))
}

/// The point of the Measure tool under a screen position in the top frame
/// there, snapped to the vector layers of the map of the frame.
fn point_at(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    at: Pos2,
) -> Option<MeasurePoint> {
    let id = frame_at(page, place.to_mm(at))?;
    let frame = page.frames.iter().find(|f| f.id == id)?;
    let projection = state.pipeline.projection(&state.doc, frame.map).ok()?;
    let vt = math::frame_transform(projection, frame, place.target());
    let q = Point::new(f64::from(at.x), f64::from(at.y));
    let pos = projection.inverse(vt.from_page(q))?;
    // Screen points per radian: points per projected meter, projected
    // meters per ground meter, and ground meters per radian.
    let local = projection.local_scale(pos).unwrap_or(1.0);
    let scale = vt.view.scale * local * projection.radius_m();
    let mut point = MeasurePoint { pos, exact: false };
    if !state.globe.snap.on {
        return Some(point);
    }
    let targets = map_targets(state, frame.map)?;
    let dir = lonlat_to_dir(pos[0], pos[1]);
    if let Some(snap) = targets.snap(dir, scale, &state.globe.snap, None) {
        point = match snap.exact {
            Some(exact) => MeasurePoint {
                pos: exact,
                exact: true,
            },
            None => {
                let (lon, lat) = dir_to_lonlat(snap.dir);
                MeasurePoint {
                    pos: [lon, lat],
                    exact: false,
                }
            }
        };
    }
    Some(point)
}

/// Reads the taps of the Measure tool on the page. A tap in a frame adds a
/// point, and a tap on the first point in any frame closes the path. A tap
/// outside the frames does nothing.
pub(super) fn input(
    ui: &egui::Ui,
    state: &mut AppState,
    resp: &egui::Response,
    page: &PageRecord,
    place: PagePlace,
) {
    let touching = ui.input(|i| i.any_touches());
    let area = resp.rect;
    let on = |pos| on_canvas(ui, area, pos);
    let press = &mut state.atlas.measure_press;
    let Some(at) = press_input(ui, resp, press, on, false, touching) else {
        return;
    };
    let mut firsts = Vec::new();
    if let Some(first) = state.measure.points.first().map(|p| p.pos) {
        each_frame(state, page, place, |shown| firsts.extend(shown.at(first)));
    }
    let point = point_at(state, page, place, at);
    let nearest = |_: LonLat| {
        let d = |p: &&Pos2| p.distance(at);
        firsts.iter().min_by(|a, b| d(a).total_cmp(&d(b))).copied()
    };
    let reach = close_reach(state.globe.snap.distance);
    state.measure.tap(at, point, nearest, reach);
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

/// The middle of the great-circle arc from `a` to `b`.
fn middle(a: LonLat, b: LonLat) -> LonLat {
    let (a, b) = (lonlat_to_dir(a[0], a[1]), lonlat_to_dir(b[0], b[1]));
    let (lon, lat) = dir_to_lonlat(slerp(a, b, 0.5));
    [lon, lat]
}

/// The parts on screen of each segment of the path, in all the frames of
/// the page.
pub fn measure_lines(state: &mut AppState, place: PagePlace) -> Vec<Vec<Vec<Pos2>>> {
    let page = open_page(state).clone();
    let segments = state.measure.segments();
    let paths: Vec<_> = segments
        .iter()
        .map(|&(a, b)| densify_great_circle(&[a, b], DENSIFY_DEG, false))
        .collect();
    let mut lines = vec![Vec::new(); paths.len()];
    each_frame(state, &page, place, |shown| {
        for (i, path) in paths.iter().enumerate() {
            lines[i].extend(shown.lines(path));
        }
    });
    lines
}

/// The length on screen of a set of lines.
fn screen_length(lines: &[Vec<Pos2>]) -> f32 {
    let pairs = lines.iter().flat_map(|l| l.windows(2));
    pairs.map(|w| w[0].distance(w[1])).sum()
}

/// Draws the path of the Measure tool in each frame of the page: the arcs,
/// the points, the length of each segment, and a dashed line from the last
/// point to the pointer.
pub(super) fn overlay(ui: &egui::Ui, state: &mut AppState, place: PagePlace, area: Rect) {
    if state.atlas.tool != super::AtlasTool::Measure || state.measure.is_empty() {
        return;
    }
    let page = open_page(state).clone();
    let world = state.doc.project.world.clone();
    let measure = state.measure.clone();
    let segments = measure.segments();
    let lengths = measure.summary(world.radius_km).segments;
    let paths: Vec<_> = segments
        .iter()
        .map(|&(a, b)| densify_great_circle(&[a, b], DENSIFY_DEG, false))
        .collect();
    let down = ui.input(|i| i.pointer.any_down() || i.any_touches());
    let cursor = match (measure.closed, measure.points.last(), state.cursor_lonlat) {
        (false, Some(last), Some(cursor)) if !down => Some((last.pos, cursor)),
        _ => None,
    };
    let mut lines = Vec::new();
    let mut dashes = Vec::new();
    // The middle of a segment in a frame, and the length of the segment on
    // screen in that frame.
    let mut labels: Vec<(Pos2, f32, f64)> = Vec::new();
    let mut dots = Vec::new();
    each_frame(state, &page, place, |shown| {
        for (((a, b), path), km) in segments.iter().zip(&paths).zip(&lengths) {
            let parts = shown.lines(path);
            if let Some(mid) = shown.at(middle(*a, *b)) {
                labels.push((mid, screen_length(&parts), *km));
            }
            lines.extend(parts);
        }
        if let Some((a, b)) = cursor {
            let path = densify_great_circle(&[a, b], DENSIFY_DEG, false);
            dashes.extend(shown.lines(&path));
        }
        for (i, point) in measure.points.iter().enumerate() {
            dots.extend(shown.at(point.pos).map(|p| (i, p)));
        }
    });
    let painter = ui.painter_at(area);
    let dash = egui::Stroke::new(1.5, Color32::from_white_alpha(200));
    for part in &dashes {
        painter.extend(egui::Shape::dashed_line(part, dash, 6.0, 4.0));
    }
    let shadow = egui::Stroke::new(4.5, Color32::from_black_alpha(110));
    let line = egui::Stroke::new(2.5, theme::ACCENT);
    for part in &lines {
        painter.line(part.clone(), shadow);
    }
    for part in &lines {
        painter.line(part.clone(), line);
    }
    for (mid, on_screen, km) in labels {
        let text = crate::world::distance(km, world.units);
        let font = egui::FontId::proportional(12.0);
        let galley = painter.layout_no_wrap(text, font, theme::TEXT);
        let pill = Rect::from_center_size(mid, galley.size() + egui::vec2(12.0, 4.0));
        if on_screen < pill.width() + 8.0 {
            continue;
        }
        let fill = Color32::from_black_alpha(190);
        painter.rect_filled(pill, pill.height() / 2.0, fill);
        painter.galley(pill.center() - galley.size() / 2.0, galley, theme::TEXT);
    }
    let edge = egui::Stroke::new(1.0, Color32::from_black_alpha(160));
    for (i, pos) in dots {
        if i == 0 {
            painter.circle_stroke(pos, 8.0, egui::Stroke::new(2.0, Color32::WHITE));
        }
        painter.circle(pos, 4.0, Color32::WHITE, edge);
    }
}
