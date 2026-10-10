//! The numbers of the brush debug panel.

use std::collections::VecDeque;
use std::time::Instant;

use vale_terrain::Mode;

/// The number of strokes that the stats keep.
pub const KEPT_STROKES: usize = 8;

/// The number of frame times that the stats keep.
pub const KEPT_FRAMES: usize = 240;
/// The longest time between two frames that counts as a frame time.
const IDLE_MS: f64 = 100.0;

/// What the stroke delay covers.
pub const DELAY_NOTE: &str = "The delay is the time from the frame that read the pen to the end \
     of the GPU work. The time from the pen to egui is not included. The time \
     to show the frame is not included.";

/// The last, the mean, and the worst of a list of values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Series {
    pub count: u32,
    pub last: f64,
    pub worst: f64,
    sum: f64,
}

impl Series {
    pub fn add(&mut self, value: f64) {
        self.count += 1;
        self.last = value;
        self.worst = if self.count == 1 {
            value
        } else {
            self.worst.max(value)
        };
        self.sum += value;
    }

    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / f64::from(self.count)
        }
    }
}

/// The numbers of one stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStats {
    pub id: u64,
    pub mode: Mode,
    pub size_points: f32,
    /// The time from the frame that read a pen sample to the end of the GPU
    /// work of its stamps, in milliseconds.
    pub delay_ms: Series,
    /// The time from one frame of the stroke to the next, in milliseconds.
    pub frame_ms: Series,
    /// The stamps and the texels that one frame sent to the GPU.
    pub stamps: Series,
    pub texels: Series,
    /// The passes that the stamps of one frame made on the GPU.
    pub passes: Series,
    last_frame: Option<Instant>,
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Raise => "raise",
        Mode::Lower => "lower",
        Mode::Smooth => "smooth",
        Mode::Flatten => "flatten",
    }
}

impl StrokeStats {
    pub fn title(&self) -> String {
        format!(
            "Stroke {}: {}, {:.0} pt",
            self.id,
            mode_name(self.mode),
            self.size_points
        )
    }

    pub fn delay_line(&self) -> String {
        let d = &self.delay_ms;
        if d.count == 0 {
            return "Stroke delay: no samples".to_string();
        }
        format!(
            "Stroke delay: last {:.2} ms, mean {:.2} ms, worst {:.2} ms, {} samples",
            d.last,
            d.mean(),
            d.worst,
            d.count
        )
    }

    pub fn frame_line(&self) -> String {
        let f = &self.frame_ms;
        format!(
            "Frame interval: mean {:.2} ms, worst {:.2} ms",
            f.mean(),
            f.worst
        )
    }

    pub fn stamps_line(&self) -> String {
        let s = &self.stamps;
        format!(
            "Stamps in a frame: mean {:.1}, worst {:.0}",
            s.mean(),
            s.worst
        )
    }

    pub fn passes_line(&self) -> String {
        let p = &self.passes;
        format!(
            "Passes in a frame: mean {:.1}, worst {:.0}",
            p.mean(),
            p.worst
        )
    }

    pub fn texels_line(&self) -> String {
        let t = &self.texels;
        format!(
            "Texels in a frame: mean {:.2} M, worst {:.2} M",
            t.mean() / 1e6,
            t.worst / 1e6
        )
    }
}

#[derive(Default)]
pub struct Stats {
    strokes: VecDeque<StrokeStats>,
    /// The stamps that wait for a later frame.
    pub backlog: usize,
    pub face_size: u32,
    /// The name and the backend of the GPU adapter.
    pub adapter: String,
    /// The time from one frame to the next, in milliseconds, for the last
    /// frames.
    frames: VecDeque<f64>,
    last_frame: Option<Instant>,
}

impl Stats {
    pub fn new(face_size: u32) -> Stats {
        Stats {
            face_size,
            ..Stats::default()
        }
    }

    pub fn begin_stroke(&mut self, id: u64, mode: Mode, size_points: f32) {
        if self.strokes.len() == KEPT_STROKES {
            self.strokes.pop_front();
        }
        self.strokes.push_back(StrokeStats {
            id,
            mode,
            size_points,
            delay_ms: Series::default(),
            frame_ms: Series::default(),
            stamps: Series::default(),
            texels: Series::default(),
            passes: Series::default(),
            last_frame: None,
        });
    }

    fn stroke(&mut self, id: u64) -> Option<&mut StrokeStats> {
        self.strokes.iter_mut().find(|s| s.id == id)
    }

    /// Records one frame of a stroke.
    pub fn frame(&mut self, id: u64, now: Instant) {
        let Some(stroke) = self.stroke(id) else {
            return;
        };
        if let Some(last) = stroke.last_frame.replace(now) {
            stroke
                .frame_ms
                .add(now.duration_since(last).as_secs_f64() * 1000.0);
        }
    }

    /// Records the stamps that one frame sent to the GPU.
    pub fn stamps(&mut self, id: u64, stamps: usize, texels: u64) {
        if let Some(stroke) = self.stroke(id) {
            stroke.stamps.add(stamps as f64);
            stroke.texels.add(texels as f64);
        }
    }

    /// Records the passes that the stamps of one frame made.
    pub fn passes(&mut self, id: u64, passes: u32) {
        if let Some(stroke) = self.stroke(id) {
            stroke.passes.add(f64::from(passes));
        }
    }

    /// Records the end of the GPU work for the stamps of one pen sample.
    pub fn delay(&mut self, id: u64, sample: Instant, done: Instant) {
        if let Some(stroke) = self.stroke(id) {
            stroke
                .delay_ms
                .add(done.duration_since(sample).as_secs_f64() * 1000.0);
        }
    }

    pub fn last(&self) -> Option<&StrokeStats> {
        self.strokes.back()
    }

    /// The worst delay of the kept strokes, in milliseconds.
    pub fn worst_delay_ms(&self) -> Option<f64> {
        let with_samples = self.strokes.iter().filter(|s| s.delay_ms.count > 0);
        with_samples.map(|s| s.delay_ms.worst).reduce(f64::max)
    }

    /// Counts one frame of the UI. A time above `IDLE_MS` is a pause of the
    /// UI and is not a frame time.
    pub fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last_frame.replace(now) {
            let ms = now.duration_since(last).as_secs_f64() * 1e3;
            if ms < IDLE_MS {
                self.frames.push_back(ms);
            }
            if self.frames.len() > KEPT_FRAMES {
                self.frames.pop_front();
            }
        }
    }

    /// The mean and the worst frame time in milliseconds.
    pub fn frame_time(&self) -> Option<(f64, f64)> {
        let worst = self.frames.iter().copied().reduce(f64::max)?;
        Some((
            self.frames.iter().sum::<f64>() / self.frames.len() as f64,
            worst,
        ))
    }

    pub fn frame_time_line(&self) -> String {
        match self.frame_time() {
            Some((mean, worst)) => format!(
                "Frame time of the last {} frames: mean {mean:.2} ms, worst {worst:.2} ms",
                self.frames.len()
            ),
            None => "Frame time: no frames".to_string(),
        }
    }

    pub fn gpu_line(&self) -> String {
        format!("GPU: {}, face {}", self.adapter, self.face_size)
    }

    pub fn worst_line(&self) -> String {
        match self.worst_delay_ms() {
            Some(worst) => format!(
                "Worst delay of the last {} strokes: {worst:.2} ms",
                self.strokes.len()
            ),
            None => "Worst delay: no samples".to_string(),
        }
    }

    /// The numbers of the last `strokes` strokes, as lines of text.
    pub fn report(&self, strokes: usize) -> String {
        let mut out = self.gpu_line();
        out.push('\n');
        let skip = self.strokes.len().saturating_sub(strokes);
        for stroke in self.strokes.iter().skip(skip) {
            for line in [
                stroke.title(),
                stroke.delay_line(),
                stroke.frame_line(),
                stroke.stamps_line(),
                stroke.passes_line(),
                stroke.texels_line(),
            ] {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out.push_str(&self.worst_line());
        out.push('\n');
        out.push_str(&format!("Backlog: {} stamps\n", self.backlog));
        out
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_series_keeps_the_last_the_mean_and_the_worst() {
        let mut s = Series::default();
        assert_eq!(s.mean(), 0.0);
        for v in [2.0, 6.0, 1.0] {
            s.add(v);
        }
        assert_eq!((s.count, s.last, s.worst, s.mean()), (3, 1.0, 6.0, 3.0));
    }

    #[test]
    fn a_stroke_gets_its_delays_and_its_frame_intervals() {
        let mut stats = Stats::default();
        let t = Instant::now();
        let ms = Duration::from_millis;
        stats.begin_stroke(1, Mode::Raise, 160.0);
        stats.begin_stroke(2, Mode::Smooth, 160.0);
        stats.frame(2, t);
        stats.frame(2, t + ms(16));
        stats.frame(2, t + ms(48));
        stats.stamps(2, 3, 30_000_000);
        stats.stamps(2, 1, 10_000_000);
        stats.passes(2, 3);
        stats.passes(2, 2);
        // A delay can arrive after the next stroke starts.
        stats.delay(1, t, t + ms(4));
        stats.delay(2, t, t + ms(2));
        stats.delay(2, t + ms(16), t + ms(22));
        // A stroke that the stats do not keep.
        stats.delay(9, t, t + ms(500));

        let last = stats.last().unwrap();
        assert_eq!(last.id, 2);
        assert_eq!(last.delay_ms.count, 2);
        assert!((last.delay_ms.last - 6.0).abs() < 1e-9);
        assert!((last.delay_ms.mean() - 4.0).abs() < 1e-9);
        assert!((last.frame_ms.mean() - 24.0).abs() < 1e-9);
        assert!((last.frame_ms.worst - 32.0).abs() < 1e-9);
        assert_eq!((last.stamps.mean(), last.stamps.worst), (2.0, 3.0));
        assert_eq!(last.texels.worst, 30_000_000.0);
        assert_eq!(last.passes_line(), "Passes in a frame: mean 2.5, worst 3");
        assert!((stats.worst_delay_ms().unwrap() - 6.0).abs() < 1e-9);
        assert_eq!(
            last.delay_line(),
            "Stroke delay: last 6.00 ms, mean 4.00 ms, worst 6.00 ms, 2 samples"
        );
    }

    #[test]
    fn the_worst_delay_covers_eight_strokes() {
        let mut stats = Stats::default();
        assert_eq!(stats.worst_delay_ms(), None);
        let t = Instant::now();
        for id in 0..=KEPT_STROKES as u64 {
            stats.begin_stroke(id, Mode::Raise, 36.0);
            // The first stroke is the slowest.
            let ms = if id == 0 { 90 } else { 10 + id };
            stats.delay(id, t, t + Duration::from_millis(ms));
        }
        // The first stroke is not kept.
        assert!((stats.worst_delay_ms().unwrap() - 18.0).abs() < 1e-9);
        assert_eq!(stats.report(2).matches("Stroke delay").count(), 2);
        assert!(stats.report(2).contains("Stroke 8: raise, 36 pt"));
    }
}
