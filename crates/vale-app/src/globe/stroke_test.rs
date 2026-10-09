//! The stroke test: a fixed stroke that gives the numbers of the debug panel.

use std::time::Instant;

use vale_terrain::Mode;

use super::brush::{BrushSettings, SIZE_POINTS, slerp};
use super::math::{V3, lonlat_to_dir};

/// One stroke of the test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub mode: Mode,
    pub size_points: f32,
    /// True: the pen moves 24 times as fast, so that one frame makes more
    /// stamps than one pass holds.
    pub fast: bool,
}

const fn part(mode: Mode, size_points: f32, fast: bool) -> Part {
    Part {
        mode,
        size_points,
        fast,
    }
}

/// The strokes of the test: slow strokes with the largest brush, and then
/// fast strokes with the smallest brush.
pub const PARTS: [Part; 4] = [
    part(Mode::Raise, *SIZE_POINTS.end(), false),
    part(Mode::Smooth, *SIZE_POINTS.end(), false),
    part(Mode::Raise, *SIZE_POINTS.start(), true),
    part(Mode::Smooth, *SIZE_POINTS.start(), true),
];
/// The length of a slow stroke, in seconds.
pub const SECONDS: f64 = 2.0;
/// The length of a fast stroke, as a part of the length of a slow stroke.
const FAST_TIME: f64 = 0.25;
/// The number of times that a fast stroke goes along the arc.
const FAST_CROSSINGS: usize = 6;
/// Pen samples in one second.
const RATE: f64 = 120.0;
/// The ends of the stroke, as longitude and latitude. The arc is 60 degrees
/// long, and it crosses the face edge at the longitude of 45 degrees.
const ENDS: [(f64, f64); 2] = [(13.0, 20.0), (77.0, 20.0)];

pub struct StrokeTest {
    /// The brush from before the test.
    pub saved: BrushSettings,
    /// The length of a slow stroke, in seconds.
    seconds: f64,
    /// The number of samples in the stroke.
    samples: usize,
    /// The number of times that the stroke goes along the arc.
    crossings: usize,
    /// The number of strokes that are complete.
    strokes: usize,
    started: Option<Instant>,
    sent: usize,
}

impl StrokeTest {
    pub fn new(saved: BrushSettings, seconds: f64) -> StrokeTest {
        StrokeTest {
            saved,
            seconds,
            samples: 0,
            crossings: 1,
            strokes: 0,
            started: None,
            sent: 0,
        }
    }

    /// True between the first and the last sample of a stroke.
    pub fn drawing(&self) -> bool {
        self.started.is_some()
    }

    /// The next stroke, or `None` after the last stroke.
    pub fn next(&self) -> Option<Part> {
        PARTS.get(self.strokes).copied()
    }

    /// The brush of a stroke.
    pub fn brush(&self, part: Part) -> BrushSettings {
        BrushSettings {
            mode: part.mode,
            size_points: part.size_points,
            lock: None,
            ..self.saved
        }
    }

    pub fn begin(&mut self, now: Instant, part: Part) {
        let (time, crossings) = match part.fast {
            true => (FAST_TIME, FAST_CROSSINGS),
            false => (1.0, 1),
        };
        self.samples = ((self.seconds * time * RATE).round() as usize).max(2);
        self.crossings = crossings;
        self.started = Some(now);
        self.sent = 0;
    }

    /// The place of each sample that the pen made up to `now`.
    pub fn due(&mut self, now: Instant) -> Vec<V3> {
        let Some(started) = self.started else {
            return Vec::new();
        };
        let elapsed = now.duration_since(started).as_secs_f64();
        let due = ((elapsed * RATE) as usize + 1).min(self.samples);
        let [from, to] = ENDS.map(|(lon, lat)| lonlat_to_dir(lon, lat));
        let place = |i: usize| {
            // The pen goes along the arc and back.
            let along = i as f64 / (self.samples - 1) as f64 * self.crossings as f64;
            let (crossing, part) = (along.floor(), along.fract());
            let back = crossing as usize % 2 == 1;
            slerp(from, to, if back { 1.0 - part } else { part })
        };
        let out = (self.sent..due).map(place).collect();
        self.sent = self.sent.max(due);
        out
    }

    /// Ends the stroke after its last sample. Returns `true` if it ended.
    pub fn end_if_complete(&mut self) -> bool {
        let complete = self.drawing() && self.sent == self.samples;
        if complete {
            self.started = None;
            self.strokes += 1;
        }
        complete
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::face_of;

    use super::*;
    use crate::globe::math::angle;

    #[test]
    fn a_stroke_gives_its_samples_in_time() {
        let mut test = StrokeTest::new(BrushSettings::default(), 1.0);
        let t = Instant::now();
        assert!(test.due(t).is_empty());
        assert_eq!(test.next(), Some(PARTS[0]));
        test.begin(t, PARTS[0]);
        assert_eq!(test.due(t).len(), 1);
        assert_eq!(test.due(t + Duration::from_millis(500)).len(), 60);
        assert!(!test.end_if_complete());
        let rest = test.due(t + Duration::from_secs(3));
        assert_eq!(rest.len(), 59);
        assert!(test.end_if_complete());
        assert!(!test.drawing());
        assert_eq!(test.next(), Some(PARTS[1]));
        assert_eq!(test.brush(PARTS[1]).size_points, 160.0);
        assert_eq!(test.brush(PARTS[1]).mode, Mode::Smooth);
    }

    #[test]
    fn a_fast_stroke_goes_along_the_arc_six_times() {
        let mut test = StrokeTest::new(BrushSettings::default(), 2.0);
        let t = Instant::now();
        assert_eq!(test.brush(PARTS[2]).size_points, 4.0);
        test.begin(t, PARTS[2]);
        let places = test.due(t + Duration::from_secs(1));
        assert_eq!(places.len(), 60);
        let length: f64 = places.windows(2).map(|w| angle(w[0], w[1])).sum();
        // The samples do not fall on the ends of the arc, so the path is a
        // little shorter than six arcs.
        assert!((330.0..=360.0).contains(&length.to_degrees()), "{length}");
        assert!(angle(places[0], places[59]) < 1e-6);
        assert!(test.end_if_complete());
    }

    #[test]
    fn the_stroke_crosses_a_face_edge() {
        let mut test = StrokeTest::new(BrushSettings::default(), 1.0);
        let t = Instant::now();
        test.begin(t, PARTS[0]);
        let places = test.due(t + Duration::from_secs(1));
        let (first, last) = (places[0], places[places.len() - 1]);
        assert!((angle(first, last).to_degrees() - 60.0).abs() < 1.0);
        assert_ne!(face_of(first).0, face_of(last).0);
    }
}
