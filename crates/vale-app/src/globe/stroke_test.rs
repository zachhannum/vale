//! The stroke test: a fixed stroke that gives the numbers of the debug panel.

use std::time::Instant;

use vale_terrain::Mode;

use super::brush::{BrushSettings, SIZE_POINTS, slerp};
use super::math::{V3, lonlat_to_dir};

/// The mode of each stroke of the test.
pub const MODES: [Mode; 2] = [Mode::Raise, Mode::Smooth];
/// The length of each stroke, in seconds.
pub const SECONDS: f64 = 2.0;
/// Pen samples in one second.
const RATE: f64 = 120.0;
/// The ends of the stroke, as longitude and latitude. The arc is 60 degrees
/// long, and it crosses the face edge at the longitude of 45 degrees.
const ENDS: [(f64, f64); 2] = [(13.0, 20.0), (77.0, 20.0)];

pub struct StrokeTest {
    /// The brush from before the test.
    pub saved: BrushSettings,
    /// The number of samples in one stroke.
    samples: usize,
    /// The number of strokes that are complete.
    strokes: usize,
    started: Option<Instant>,
    sent: usize,
}

impl StrokeTest {
    pub fn new(saved: BrushSettings, seconds: f64) -> StrokeTest {
        StrokeTest {
            saved,
            samples: ((seconds * RATE).round() as usize).max(2),
            strokes: 0,
            started: None,
            sent: 0,
        }
    }

    /// True between the first and the last sample of a stroke.
    pub fn drawing(&self) -> bool {
        self.started.is_some()
    }

    /// The mode of the next stroke, or `None` after the last stroke.
    pub fn next_mode(&self) -> Option<Mode> {
        MODES.get(self.strokes).copied()
    }

    /// The brush of a stroke: the largest size.
    pub fn brush(&self, mode: Mode) -> BrushSettings {
        BrushSettings {
            mode,
            size_points: *SIZE_POINTS.end(),
            ..self.saved
        }
    }

    pub fn begin(&mut self, now: Instant) {
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
        let place = |i: usize| slerp(from, to, i as f64 / (self.samples - 1) as f64);
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
        assert_eq!(test.next_mode(), Some(Mode::Raise));
        test.begin(t);
        assert_eq!(test.due(t).len(), 1);
        assert_eq!(test.due(t + Duration::from_millis(500)).len(), 60);
        assert!(!test.end_if_complete());
        let rest = test.due(t + Duration::from_secs(3));
        assert_eq!(rest.len(), 59);
        assert!(test.end_if_complete());
        assert!(!test.drawing());
        assert_eq!(test.next_mode(), Some(Mode::Smooth));
        assert_eq!(test.brush(Mode::Smooth).size_points, 160.0);
    }

    #[test]
    fn the_stroke_crosses_a_face_edge() {
        let mut test = StrokeTest::new(BrushSettings::default(), 1.0);
        let t = Instant::now();
        test.begin(t);
        let places = test.due(t + Duration::from_secs(1));
        let (first, last) = (places[0], places[places.len() - 1]);
        assert!((angle(first, last).to_degrees() - 60.0).abs() < 1.0);
        assert_ne!(face_of(first).0, face_of(last).0);
    }
}
