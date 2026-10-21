//! The zoom test: the view zooms from the whole world to a close view and
//! back, and the test measures the time of each frame.

use std::time::Instant;

/// The length of the test, in seconds.
pub const SECONDS: f64 = 4.0;
/// The zoom at the middle of the test, as a factor of the start zoom.
pub const DEPTH: f64 = 40.0;

pub struct ZoomTest {
    seconds: f64,
    start: Option<Instant>,
    last: Option<Instant>,
    /// The zoom of the view before the test.
    pub saved: f64,
    /// The time of each frame, in milliseconds.
    frames: Vec<f64>,
}

impl ZoomTest {
    pub fn new(saved: f64, seconds: f64) -> Self {
        Self {
            seconds,
            start: None,
            last: None,
            saved,
            frames: Vec::new(),
        }
    }

    /// The zoom at the frame that starts at `now`, or `None` after the end.
    /// The zoom goes up and down on a geometric path, so each frame zooms
    /// by the same factor.
    pub fn zoom(&mut self, now: Instant) -> Option<f64> {
        let start = *self.start.get_or_insert(now);
        if let Some(last) = self.last.replace(now) {
            self.frames
                .push(now.saturating_duration_since(last).as_secs_f64() * 1e3);
        }
        let t = now.saturating_duration_since(start).as_secs_f64() / self.seconds;
        if t >= 1.0 {
            return None;
        }
        let up = 1.0 - (2.0 * t - 1.0).abs();
        Some(self.saved * DEPTH.powf(up))
    }

    /// The text with the frame times.
    pub fn report(&self) -> String {
        let mut sorted = self.frames.clone();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        if n == 0 {
            return "No frames".to_string();
        }
        let mean = sorted.iter().sum::<f64>() / n as f64;
        let p95 = sorted[(n * 95 / 100).min(n - 1)];
        let worst = sorted[n - 1];
        format!("{n} frames, mean {mean:.1} ms, 95 % {p95:.1} ms, worst {worst:.1} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_zoom_goes_to_the_depth_and_back() {
        let mut test = ZoomTest::new(2.0, 1.0);
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        assert_eq!(test.zoom(at(0.0)), Some(2.0));
        let middle = test.zoom(at(0.5)).unwrap();
        assert!((middle - 2.0 * DEPTH).abs() < 1e-9, "{middle}");
        let quarter = test.zoom(at(0.25)).unwrap();
        assert!((quarter - 2.0 * DEPTH.sqrt()).abs() < 1e-9, "{quarter}");
        assert_eq!(test.zoom(at(1.0)), None);
        assert!(test.report().starts_with("3 frames"), "{}", test.report());
    }
}
