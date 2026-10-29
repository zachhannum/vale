//! The path of the Measure tool. A measurement is temporary, so the project
//! and the undo log do not store it.

use eframe::egui::Pos2;
use vale_sphere::LonLat;
use vale_sphere::measure::{central_angle, path_length, ring_area, ring_crosses_itself};
use vale_store::Units;

use crate::world;

/// The smallest distance on screen, in points, of a tap that closes the
/// path on its first point.
pub const CLOSE_REACH: f32 = 22.0;

/// The largest angle between two drawn points of a measured path, in
/// degrees.
pub const DENSIFY_DEG: f64 = 1.0;

/// The largest move of a tap, in points. A larger move is a drag.
const TAP_SLOP: f32 = 10.0;

/// The ID of the mouse in a `TapPress`.
pub const MOUSE: u64 = u64::MAX - 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurePoint {
    pub pos: LonLat,
    /// The point snapped to a stored vertex, and `pos` is that vertex.
    pub exact: bool,
}

/// The points of the path, and whether the path is closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measure {
    pub points: Vec<MeasurePoint>,
    pub closed: bool,
}

/// The values of a path.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The length, or the perimeter of a closed path.
    pub length_km: f64,
    /// The area of a closed path that does not cross itself.
    pub area_km2: Option<f64>,
    /// The path is closed and crosses itself.
    pub crosses: bool,
    /// The length of each segment. A closed path has the segment from the
    /// last point to the first.
    pub segments: Vec<f64>,
}

impl Measure {
    /// Adds a point. After the path closes, a point starts a new path.
    pub fn add(&mut self, pos: LonLat, exact: bool) {
        if self.closed {
            self.clear();
        }
        self.points.push(MeasurePoint { pos, exact });
    }

    pub fn can_close(&self) -> bool {
        !self.closed && self.points.len() >= 3
    }

    /// Closes a path of three or more points. Returns true if it closed.
    pub fn close(&mut self) -> bool {
        let close = self.can_close();
        self.closed |= close;
        close
    }

    /// Removes the last point. A closed path opens first.
    pub fn remove_last(&mut self) {
        if self.closed {
            self.closed = false;
        } else {
            self.points.pop();
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.closed = false;
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn positions(&self) -> Vec<LonLat> {
        self.points.iter().map(|p| p.pos).collect()
    }

    /// A tap at the screen position `at`. A tap within `reach` points of
    /// the first point closes a path that can close. Each other tap adds
    /// `point`. `project` gives the screen position of a place.
    pub fn tap(
        &mut self,
        at: Pos2,
        point: Option<MeasurePoint>,
        project: impl Fn(LonLat) -> Option<Pos2>,
        reach: f32,
    ) {
        let first = self.points.first().and_then(|p| project(p.pos));
        if self.can_close() && first.is_some_and(|p| p.distance(at) <= reach) {
            self.close();
        } else if let Some(point) = point {
            self.add(point.pos, point.exact);
        }
    }

    /// The segments of the path, as the two ends of each one.
    pub fn segments(&self) -> Vec<(LonLat, LonLat)> {
        let pos = self.positions();
        let mut out: Vec<_> = pos.windows(2).map(|w| (w[0], w[1])).collect();
        if self.closed && pos.len() > 2 {
            out.push((pos[pos.len() - 1], pos[0]));
        }
        out
    }

    pub fn summary(&self, radius_km: f64) -> Summary {
        let pos = self.positions();
        let crosses = self.closed && ring_crosses_itself(&pos);
        let segments = self.segments();
        Summary {
            length_km: path_length(&pos, self.closed, radius_km),
            area_km2: (self.closed && !crosses).then(|| ring_area(&pos, radius_km)),
            crosses,
            segments: segments
                .iter()
                .map(|&(a, b)| central_angle(a, b) * radius_km)
                .collect(),
        }
    }

    /// The label of each segment on the map, in the format of the readout.
    pub fn segment_labels(&self, radius_km: f64, units: Units) -> Vec<String> {
        let summary = self.summary(radius_km);
        let length = |km: &f64| world::length(*km, units);
        summary.segments.iter().map(length).collect()
    }

    /// The text of the readout.
    pub fn readout(&self, radius_km: f64, units: Units) -> String {
        if self.is_empty() {
            return "Tap to add a point.".to_string();
        }
        let summary = self.summary(radius_km);
        let length = world::length(summary.length_km, units);
        if !self.closed {
            return format!("Length {length}");
        }
        match summary.area_km2 {
            Some(area) => format!("Perimeter {length} · Area {}", world::area(area, units)),
            None => format!("Perimeter {length} · The path crosses itself."),
        }
    }
}

/// The distance on screen of a tap that closes the path, for the reach of
/// the snap.
pub fn close_reach(snap_distance: f64) -> f32 {
    (snap_distance as f32).max(CLOSE_REACH)
}

/// One press of a pen, a finger, or the mouse that can be a tap. A second
/// finger or a move ends the tap. A finger while a pen is down is a palm.
#[derive(Debug, Default)]
pub struct TapPress {
    /// The ID, the start, and whether it is a pen.
    press: Option<(u64, Pos2, bool)>,
    broken: bool,
}

impl TapPress {
    pub fn down(&mut self, id: u64, pos: Pos2, pen: bool) {
        match self.press {
            Some((_, _, true)) if !pen => {}
            Some(_) => self.broken = true,
            None => {
                self.press = Some((id, pos, pen));
                self.broken = false;
            }
        }
    }

    pub fn moved(&mut self, id: u64, pos: Pos2) {
        if let Some((i, start, _)) = self.press
            && i == id
            && start.distance(pos) > TAP_SLOP
        {
            self.broken = true;
        }
    }

    /// Ends the press. Returns the position of a tap.
    pub fn up(&mut self, id: u64, pos: Pos2) -> Option<Pos2> {
        self.moved(id, pos);
        match self.press {
            Some((i, ..)) if i == id => {
                self.press = None;
                (!std::mem::take(&mut self.broken)).then_some(pos)
            }
            _ => None,
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if self.press.is_some_and(|(i, ..)| i == id) {
            self.press = None;
            self.broken = false;
        }
    }

    pub fn is_down(&self) -> bool {
        self.press.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use eframe::egui::pos2;

    use super::*;

    const EARTH: f64 = 6371.0;

    fn square() -> Measure {
        let mut m = Measure::default();
        for p in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
            m.add(p, false);
        }
        m
    }

    #[test]
    fn a_path_closes_with_three_points_and_reopens() {
        let mut m = Measure::default();
        m.add([0.0, 0.0], false);
        m.add([1.0, 0.0], false);
        assert!(!m.close());
        m.add([1.0, 1.0], false);
        assert!(m.close());
        assert_eq!(m.segments().len(), 3);
        m.remove_last();
        assert!(!m.closed);
        assert_eq!(m.points.len(), 3);
        m.remove_last();
        assert_eq!(m.points.len(), 2);
        m.add([1.0, 1.0], false);
        m.close();
        // A point after the close starts a new path.
        m.add([5.0, 5.0], true);
        assert_eq!(
            m.points,
            [MeasurePoint {
                pos: [5.0, 5.0],
                exact: true
            }]
        );
        assert!(!m.closed);
        m.clear();
        assert!(m.is_empty());
    }

    #[test]
    fn the_summary_has_the_great_circle_length_and_the_area() {
        let mut m = square();
        let open = m.summary(EARTH);
        assert_eq!(open.area_km2, None);
        assert_eq!(open.segments.len(), 3);
        let sum: f64 = open.segments.iter().sum();
        assert!((open.length_km - sum).abs() < 1e-9);
        m.close();
        let closed = m.summary(EARTH);
        assert_eq!(closed.segments.len(), 4);
        // The cap of a hemisphere is 2πr².
        let mut half = Measure::default();
        for lon in [0.0, 90.0, 180.0, -90.0] {
            half.add([lon, 0.0], false);
        }
        half.close();
        let area = half.summary(EARTH).area_km2.unwrap();
        assert!((area / (2.0 * PI * EARTH * EARTH) - 1.0).abs() < 1e-9);
        assert!(closed.area_km2.unwrap() > 1.0e6);
    }

    #[test]
    fn a_path_that_crosses_itself_has_no_area() {
        let mut m = Measure::default();
        for p in [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]] {
            m.add(p, false);
        }
        m.close();
        let summary = m.summary(EARTH);
        assert!(summary.crosses);
        assert_eq!(summary.area_km2, None);
        let text = m.readout(EARTH, Units::Metric);
        assert!(text.ends_with("The path crosses itself."), "{text}");
    }

    #[test]
    fn the_readout_shows_the_length_or_the_perimeter_and_the_area() {
        let mut m = Measure::default();
        assert_eq!(m.readout(EARTH, Units::Metric), "Tap to add a point.");
        m.add([0.0, 0.0], false);
        m.add([0.0, 10.0], false);
        assert_eq!(m.readout(EARTH, Units::Metric), "Length 1,112 km");
        m.add([10.0, 10.0], false);
        m.close();
        let text = m.readout(EARTH, Units::Imperial);
        assert!(text.starts_with("Perimeter "), "{text}");
        assert!(
            text.contains(" mi · Area ") && text.ends_with(" mi²"),
            "{text}"
        );
    }

    #[test]
    fn a_segment_label_has_the_format_of_the_readout() {
        let mut m = Measure::default();
        m.add([0.0, 0.0], false);
        m.add([0.0, 10.0], false);
        m.add([0.0, 10.05], false);
        let labels = m.segment_labels(EARTH, Units::Metric);
        assert_eq!(labels, ["1,112 km", "5.56 km"]);
        assert_eq!(m.readout(EARTH, Units::Metric), "Length 1,118 km");
        let miles = m.segment_labels(EARTH, Units::Imperial);
        assert_eq!(miles[0], "691 mi");
    }

    #[test]
    fn a_tap_on_the_first_point_closes_the_path() {
        let mut m = square();
        let project = |p: LonLat| Some(pos2(p[0] as f32 * 10.0, p[1] as f32 * 10.0));
        let point = Some(MeasurePoint {
            pos: [1.0, 1.0],
            exact: false,
        });
        m.tap(pos2(30.0, 0.0), point, project, CLOSE_REACH);
        assert_eq!(m.points.len(), 5);
        m.tap(pos2(5.0, 5.0), point, project, CLOSE_REACH);
        assert!(m.closed);
        assert_eq!(m.points.len(), 5);
    }

    #[test]
    fn a_press_that_moves_or_has_two_fingers_is_not_a_tap() {
        let mut press = TapPress::default();
        press.down(1, pos2(0.0, 0.0), false);
        assert_eq!(press.up(1, pos2(3.0, 0.0)), Some(pos2(3.0, 0.0)));
        press.down(1, pos2(0.0, 0.0), false);
        press.moved(1, pos2(30.0, 0.0));
        assert_eq!(press.up(1, pos2(0.0, 0.0)), None);
        press.down(1, pos2(0.0, 0.0), false);
        press.down(2, pos2(50.0, 0.0), false);
        assert_eq!(press.up(2, pos2(50.0, 0.0)), None);
        assert_eq!(press.up(1, pos2(0.0, 0.0)), None);
        // A palm next to a pen does not end the tap of the pen.
        press.down(3, pos2(0.0, 0.0), true);
        press.down(4, pos2(90.0, 0.0), false);
        assert_eq!(press.up(3, pos2(0.0, 0.0)), Some(pos2(0.0, 0.0)));
        assert!(!press.is_down());
    }
}
