//! Pen input that does not come through egui, and the numbers of the pen in
//! the debug panel.

use std::sync::{Arc, Mutex};

use eframe::egui::Pos2;

#[cfg(target_os = "ios")]
pub mod uikit;

/// The touch id of the pen that the queue brings.
pub const QUEUE_PEN: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenPhase {
    Down,
    Move,
    Up,
    Cancel,
    /// The pen moves above the screen.
    Hover,
    HoverEnd,
}

/// One sample of the pen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenEvent {
    pub phase: PenPhase,
    /// The position in the view, in points.
    pub pos: [f32; 2],
    /// From 0 to 1.
    pub force: f32,
    /// The angle from the screen to the pen, in radians.
    pub altitude: Option<f32>,
    /// The direction that the pen points to, in radians.
    pub azimuth: Option<f32>,
    /// The height of a pen that hovers, from 0 to 1.
    pub height: Option<f32>,
    /// The time of the sample, in seconds.
    pub time: f64,
}

/// The pen events that wait for the canvas.
#[derive(Clone, Default)]
pub struct PenQueue(Arc<Mutex<Vec<PenEvent>>>);

impl PenQueue {
    pub fn push(&self, events: impl IntoIterator<Item = PenEvent>) {
        if let Ok(mut queue) = self.0.lock() {
            queue.extend(events);
        }
    }

    pub fn take(&self) -> Vec<PenEvent> {
        self.0
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }
}

/// The samples of one stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Run {
    samples: u32,
    first: f64,
    last: f64,
}

impl Run {
    fn rate(&self) -> f64 {
        let seconds = self.last - self.first;
        if self.samples < 2 || seconds <= 0.0 {
            0.0
        } else {
            f64::from(self.samples - 1) / seconds
        }
    }
}

/// What the pen delivers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PenStats {
    /// The force of the last sample, or `None` for a mouse.
    pub force: Option<f32>,
    /// The altitude and the azimuth of the last sample, in radians.
    pub tilt: Option<(f32, f32)>,
    pub hover_seen: bool,
    pub hover_height: Option<f32>,
    run: Option<Run>,
    last: Option<Run>,
}

impl PenStats {
    /// Records one sample of a stroke. `time` is in seconds.
    pub fn sample(&mut self, time: f64, force: Option<f32>, tilt: Option<(f32, f32)>) {
        let run = self.run.get_or_insert(Run {
            samples: 0,
            first: time,
            last: time,
        });
        run.samples += 1;
        run.last = time;
        self.force = force;
        self.tilt = tilt.or(self.tilt);
    }

    /// Records the end of a stroke.
    pub fn up(&mut self) {
        self.last = self.run.take().or(self.last);
    }

    pub fn hover(&mut self, event: &PenEvent) {
        self.hover_seen = true;
        self.hover_height = event.height;
        self.tilt = event.altitude.zip(event.azimuth).or(self.tilt);
    }

    /// The samples per second of the last stroke.
    pub fn rate(&self) -> Option<f64> {
        self.last.map(|run| run.rate())
    }

    pub fn stroke_line(&self) -> String {
        match self.last {
            Some(run) => format!(
                "Last stroke: {} samples, {:.0} per second",
                run.samples,
                run.rate()
            ),
            None => "Last stroke: none".to_string(),
        }
    }

    pub fn force_line(&self) -> String {
        match self.force {
            Some(force) => format!("Pen force: {force:.2}"),
            None => "Pen force: none".to_string(),
        }
    }

    pub fn tilt_line(&self) -> String {
        match self.tilt {
            Some((altitude, azimuth)) => format!(
                "Tilt: {:.0}° above the screen, direction {:.0}°",
                altitude.to_degrees(),
                azimuth.to_degrees()
            ),
            None => "Tilt: none".to_string(),
        }
    }

    pub fn hover_line(&self) -> String {
        match (self.hover_seen, self.hover_height) {
            (false, _) => "Hover: not seen".to_string(),
            (true, Some(height)) => format!("Hover: seen, height {height:.2}"),
            (true, None) => "Hover: seen".to_string(),
        }
    }
}

/// The pen of the canvas.
#[derive(Default)]
pub struct Pen {
    /// The queue of the pen layer below egui. With a queue, the canvas does
    /// not read the pen from egui.
    pub queue: Option<PenQueue>,
    /// The position of a pen that hovers, in egui points.
    pub hover: Option<Pos2>,
    pub stats: PenStats,
}

impl Pen {
    pub fn source_line(&self) -> &'static str {
        match self.queue {
            Some(_) => "Pen source: UIKit",
            None => "Pen source: egui",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_is_the_samples_per_second_of_the_last_stroke() {
        let mut stats = PenStats::default();
        assert_eq!(stats.rate(), None);
        assert_eq!(stats.stroke_line(), "Last stroke: none");
        for i in 0..=480 {
            stats.sample(100.0 + f64::from(i) / 240.0, Some(0.5), Some((1.0, 2.0)));
        }
        // The stroke is not complete.
        assert_eq!(stats.rate(), None);
        stats.up();
        assert!((stats.rate().unwrap() - 240.0).abs() < 1e-6);
        assert_eq!(
            stats.stroke_line(),
            "Last stroke: 481 samples, 240 per second"
        );
        assert_eq!(stats.force_line(), "Pen force: 0.50");
        assert_eq!(
            stats.tilt_line(),
            "Tilt: 57° above the screen, direction 115°"
        );
        // A stroke of one sample has no rate.
        stats.sample(200.0, None, None);
        stats.up();
        assert_eq!(stats.rate(), Some(0.0));
        assert_eq!(stats.force_line(), "Pen force: none");
    }

    #[test]
    fn the_queue_gives_each_event_one_time() {
        let queue = PenQueue::default();
        let event = PenEvent {
            phase: PenPhase::Hover,
            pos: [10.0, 20.0],
            force: 0.0,
            altitude: None,
            azimuth: None,
            height: Some(0.4),
            time: 1.0,
        };
        queue.clone().push([event, event]);
        assert_eq!(queue.take().len(), 2);
        assert!(queue.take().is_empty());

        let mut stats = PenStats::default();
        assert_eq!(stats.hover_line(), "Hover: not seen");
        stats.hover(&event);
        assert_eq!(stats.hover_line(), "Hover: seen, height 0.40");
    }
}
