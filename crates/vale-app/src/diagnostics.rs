//! Numbers about the speed of the app, as a text that the user copies.
//!
//! The parts of a frame record their times here, from any thread. The
//! report adds the state of the open page and of the detail maps.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ui::{AppState, Layout};

/// The samples that each span keeps.
const KEPT: usize = 240;

/// A part of a frame whose time the app keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    /// The CPU time of one UI frame.
    Ui,
    /// The compose of a map frame.
    Compose,
    /// The choice of the tiles of the detail maps.
    DetailChoice,
    /// The requests and the tiles of the detail maps.
    DetailPump,
    /// The CPU time that the globe takes to send its work to the GPU.
    GpuPrepare,
}

impl Span {
    const ALL: [Span; 5] = [
        Span::Ui,
        Span::Compose,
        Span::DetailChoice,
        Span::DetailPump,
        Span::GpuPrepare,
    ];

    fn name(self) -> &'static str {
        match self {
            Span::Ui => "UI frame (CPU)",
            Span::Compose => "Map frame compose",
            Span::DetailChoice => "Detail tile choice",
            Span::DetailPump => "Detail tile pump",
            Span::GpuPrepare => "GPU prepare (CPU)",
        }
    }
}

/// A number of events that the app counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    DetailSent,
    DetailReceived,
    DetailFailed,
}

impl Count {
    const ALL: [Count; 3] = [
        Count::DetailSent,
        Count::DetailReceived,
        Count::DetailFailed,
    ];

    fn name(self) -> &'static str {
        match self {
            Count::DetailSent => "Detail tiles asked",
            Count::DetailReceived => "Detail tiles received",
            Count::DetailFailed => "Detail tiles failed",
        }
    }
}

struct Store {
    times: [VecDeque<f64>; Span::ALL.len()],
    counts: [u64; Count::ALL.len()],
    since: Option<Instant>,
}

static STORE: Mutex<Store> = Mutex::new(Store {
    times: [const { VecDeque::new() }; Span::ALL.len()],
    counts: [0; Count::ALL.len()],
    since: None,
});

fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Records one time of a span.
pub fn record(span: Span, time: Duration) {
    let mut s = store();
    s.since.get_or_insert_with(Instant::now);
    let times = &mut s.times[span as usize];
    times.push_back(time.as_secs_f64() * 1e3);
    if times.len() > KEPT {
        times.pop_front();
    }
}

/// Runs `f` and records its time.
pub fn timed<T>(span: Span, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    record(span, start.elapsed());
    out
}

/// Adds `n` to a count.
pub fn count(count: Count, n: u64) {
    let mut s = store();
    s.since.get_or_insert_with(Instant::now);
    s.counts[count as usize] += n;
}

/// Clears the times and the counts.
pub fn reset() {
    let mut s = store();
    s.times.iter_mut().for_each(VecDeque::clear);
    s.counts = [0; Count::ALL.len()];
    s.since = Some(Instant::now());
}

/// The mean, the worst, and the number of the kept times of a span, in
/// milliseconds.
pub fn summary(span: Span) -> Option<(f64, f64, usize)> {
    let s = store();
    let times = &s.times[span as usize];
    let worst = times.iter().copied().reduce(f64::max)?;
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    Some((mean, worst, times.len()))
}

/// The value of a count since the last reset.
pub fn total(count: Count) -> u64 {
    store().counts[count as usize]
}

/// The lines of the times and the counts.
fn timing_lines(out: &mut String) {
    for span in Span::ALL {
        let _ = match summary(span) {
            Some((mean, worst, n)) => writeln!(
                out,
                "{}: mean {mean:.2} ms, worst {worst:.2} ms, {n} times",
                span.name()
            ),
            None => writeln!(out, "{}: no times", span.name()),
        };
    }
    let secs = store().since.map_or(0.0, |t| t.elapsed().as_secs_f64());
    for count in Count::ALL {
        let _ = writeln!(out, "{}: {} in {secs:.1} s", count.name(), total(count));
    }
}

/// The report that the user copies: the device, the times, and the state
/// of the open page and of the detail maps.
pub fn report(ctx: &eframe::egui::Context, state: &AppState) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Vale {} diagnostics", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "OS: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "{}", state.globe.stats.gpu_line());
    let screen = ctx.content_rect().size();
    let layout = match state.layout {
        Layout::Desktop => "desktop",
        Layout::Pad => "pad",
    };
    let _ = writeln!(
        out,
        "Screen: {:.0} x {:.0} pt at {:.1}x, {layout} layout, {:?} workspace",
        screen.x,
        screen.y,
        ctx.pixels_per_point(),
        state.workspace,
    );
    let _ = writeln!(out, "{}", state.globe.stats.frame_time_line());
    timing_lines(&mut out);

    let globe = &state.globe;
    let _ = writeln!(
        out,
        "Detail maps: {:.1} MB, {} tiles in flight",
        globe.detail_bytes() as f64 / 1e6,
        globe.details_in_flight()
    );
    let project = &state.doc.project;
    for raster in project.rasters() {
        let name = project
            .raster_layer(raster.id)
            .map_or("", |l| l.name.as_str());
        let _ = writeln!(
            out,
            "Raster layer {} \"{name}\": face {}, {:?}",
            raster.id.0, raster.face_size, raster.display
        );
    }
    let atlas = &project.atlas;
    if let Some(page) = atlas.page(state.atlas.page) {
        let number = atlas.pages.iter().position(|p| p.id == page.id);
        let _ = writeln!(
            out,
            "Page {} of {}: {} frames",
            number.map_or(0, |n| n + 1),
            atlas.pages.len(),
            page.frames.len()
        );
        for frame in &page.frames {
            let map = atlas.map(frame.map).map_or("", |m| m.name.as_str());
            let _ = writeln!(
                out,
                "Frame {} (map \"{map}\"): 1:{:.0}, center {:.4}, {:.4}, fits {}, ready {}",
                frame.id.0,
                frame.scale,
                frame.center[0],
                frame.center[1],
                globe.frame_fits(frame.id.0),
                globe.frame_details_ready(frame.id.0),
            );
            for raster in project.rasters() {
                if let Some(level) = globe.frame_detail_level(frame.id.0, raster.id) {
                    let tiles = globe.frame_detail_tiles(frame.id.0, raster.id).len();
                    let _ = writeln!(
                        out,
                        "  Raster layer {}: level {level}, {tiles} tiles",
                        raster.id.0
                    );
                }
            }
        }
    }
    if !state.status.is_empty() {
        let _ = writeln!(out, "Status: {}", state.status);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The store is global, so one test checks it.
    #[test]
    fn the_store_keeps_times_and_counts_until_a_reset() {
        reset();
        record(Span::Compose, Duration::from_millis(2));
        record(Span::Compose, Duration::from_millis(6));
        let (mean, worst, n) = summary(Span::Compose).unwrap();
        assert!((mean - 4.0).abs() < 1e-9 && (worst - 6.0).abs() < 1e-9 && n == 2);
        for _ in 0..KEPT + 10 {
            record(Span::Ui, Duration::from_millis(1));
        }
        assert_eq!(summary(Span::Ui).unwrap().2, KEPT);
        count(Count::DetailSent, 3);
        count(Count::DetailSent, 2);
        assert_eq!(total(Count::DetailSent), 5);
        let mut lines = String::new();
        timing_lines(&mut lines);
        assert!(lines.contains("Map frame compose: mean 4.00 ms, worst 6.00 ms, 2 times"));
        assert!(lines.contains("Detail tiles asked: 5 in "));
        reset();
        assert_eq!(summary(Span::Compose), None);
        assert_eq!(total(Count::DetailSent), 0);
    }
}
