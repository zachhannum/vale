//! The rivers of the globe. A worker thread computes the channel map and the
//! window of small rivers, so the brush does not wait for them.

use std::f64::consts::FRAC_PI_2;
use std::sync::{Arc, mpsc};

use vale_terrain::{
    ChannelMap, ChannelWindow, CoarseHeights, FlowMap, Heightmap, TexelRect, WINDOW_CELLS,
    WINDOW_MARGIN, Window, WindowHeights, window_channels,
};

use super::math::V3;

/// The side of the window is this number of times the span of the view, at
/// least.
const WINDOW_ROOM: f64 = 1.5;

/// The first texel of a window is a multiple of this number of cells, so a
/// small move of the view gives the same window.
const WINDOW_SNAP: usize = WINDOW_CELLS / 4;

/// The window of small rivers for a view, or `None` for a view that the
/// channel map of the whole world serves. `middle` is the place at the middle
/// of the view, and `span` is the angle that the view covers, in radians. The
/// view keeps the window `current` while its cells have the right size and
/// the view is inside it.
pub fn choose_window(
    middle: V3,
    span: f64,
    face_size: usize,
    current: Option<Window>,
) -> Option<Window> {
    if !(span > 0.0 && span.is_finite()) {
        return None;
    }
    let n = face_size as f64;
    // The side of a window with cells of one texel, in radians.
    let side = WINDOW_CELLS as f64 * FRAC_PI_2 / n;
    let need = (WINDOW_ROOM * span / side).ceil();
    let global = face_size / CoarseHeights::size_for(face_size);
    if need >= global as f64 {
        return None;
    }
    let cell = (need as usize).max(1).next_power_of_two();
    if cell >= global {
        return None;
    }
    // Half of the span, in cells.
    let half = span / 2.0 * n / (cell as f64 * FRAC_PI_2);
    let keeps = |window: &Window| {
        let margin = WINDOW_MARGIN / 2.0;
        let inside = |p: f64| p - half >= margin && p + half <= window.cells as f64 - margin;
        let same = window.cell == cell && window.face_size == face_size;
        same && window
            .place(middle)
            .is_some_and(|(u, v)| inside(u) && inside(v))
    };
    if let Some(window) = current.filter(keeps) {
        return Some(window);
    }
    let mut window = Window::centered(middle, face_size, cell, WINDOW_CELLS);
    let snap = (WINDOW_SNAP * cell) as i64;
    // The window ends less than half a face past the face edge.
    let past = (face_size as i64 / 2 - 1) / snap * snap;
    let last = face_size as i64 + past - (WINDOW_CELLS * cell) as i64;
    let first = |x: i64| ((x as f64 / snap as f64).round() as i64 * snap).clamp(-past, last);
    window.x0 = first(window.x0);
    window.y0 = first(window.y0);
    Some(window)
}

/// The size of a cell of a window on the ground, in radians, at the middle
/// of a face.
pub fn cell_angle(window: &Window) -> f64 {
    window.cell as f64 * FRAC_PI_2 / window.face_size as f64
}

/// One request, with its number.
enum Job {
    Global(u64, CoarseHeights),
    Window(u64, WindowHeights),
}

enum Done {
    Global(u64, ChannelMap),
    Window(u64, ChannelWindow),
}

/// The numbers of the newest request and of the newest result of one kind.
#[derive(Default)]
struct Count {
    sent: u64,
    received: u64,
}

impl Count {
    fn pending(&self) -> bool {
        self.received < self.sent
    }
}

pub struct Rivers {
    /// The coarse copy of the heightmap that the flow comes from.
    coarse: CoarseHeights,
    /// A request computes its result on the thread that asks, so the result
    /// is ready at the next poll.
    sync: bool,
    /// The flow of the newest request in the sync mode.
    flow: Option<FlowMap>,
    /// The sync mode has a flow, or the worker has one or gets one before
    /// the next window.
    has_flow: bool,
    /// The path to the worker. `None`: the worker did not start yet.
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Sender<Done>,
    results: mpsc::Receiver<Done>,
    global: Count,
    window: Count,
    /// The results that arrived and that no poll took.
    channels: Option<ChannelMap>,
    channel_window: Option<ChannelWindow>,
}

/// Computes the newest job of each kind until the sender is gone. A window
/// job runs after the global jobs, with the flow of the newest one.
fn work(jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    let mut flow = None;
    let (mut global, mut window) = (None, None);
    loop {
        if global.is_none() && window.is_none() {
            match jobs.recv() {
                Ok(Job::Global(number, heights)) => global = Some((number, heights)),
                Ok(Job::Window(number, heights)) => window = Some((number, heights)),
                Err(_) => return,
            }
        }
        for job in jobs.try_iter() {
            match job {
                Job::Global(number, heights) => global = Some((number, heights)),
                Job::Window(number, heights) => window = Some((number, heights)),
            }
        }
        let result = if let Some((number, heights)) = global.take() {
            let new = flow.insert(FlowMap::new(&heights));
            Done::Global(number, new.channels())
        } else if let Some((number, heights)) = window.take() {
            // A window job comes after a global job, so the flow is there.
            let Some(flow) = &flow else { continue };
            Done::Window(number, window_channels(&heights, flow))
        } else {
            continue;
        };
        if done.send(result).is_err() {
            return;
        }
    }
}

impl Rivers {
    pub fn new(map: &Heightmap, sync: bool) -> Rivers {
        let (done, results) = mpsc::channel();
        Rivers {
            coarse: CoarseHeights::new(map),
            sync,
            flow: None,
            has_flow: false,
            jobs: None,
            done,
            results,
            global: Count::default(),
            window: Count::default(),
            channels: None,
            channel_window: None,
        }
    }

    pub fn sync(&self) -> bool {
        self.sync
    }

    pub fn set_sync(&mut self, sync: bool) {
        if sync != self.sync {
            self.sync = sync;
            self.has_flow = false;
        }
    }

    /// The coarse copy of the heightmap, with each change up to the last
    /// `update`.
    pub fn coarse(&self) -> &CoarseHeights {
        &self.coarse
    }

    /// Reads the whole heightmap again.
    pub fn rebuild(&mut self, map: &Heightmap) {
        self.coarse = CoarseHeights::new(map);
    }

    /// Reads the texels of one rectangle again.
    pub fn update(&mut self, map: &Heightmap, face: usize, rect: TexelRect) {
        self.coarse.update(map, face, rect);
    }

    fn send(&mut self, job: Job) {
        let jobs = self.jobs.get_or_insert_with(|| {
            let (jobs, receiver) = mpsc::channel();
            let done = self.done.clone();
            std::thread::Builder::new()
                .name("vale-rivers".to_string())
                .spawn(move || work(receiver, done))
                .expect("the system can start a thread");
            jobs
        });
        let _ = jobs.send(job);
    }

    /// Asks for the channel map of the heights as they are now.
    pub fn request(&mut self) {
        self.global.sent += 1;
        self.has_flow = true;
        let (number, heights) = (self.global.sent, self.coarse.clone());
        if self.sync {
            let flow = self.flow.insert(FlowMap::new(&heights));
            let _ = self.done.send(Done::Global(number, flow.channels()));
        } else {
            self.send(Job::Global(number, heights));
        }
    }

    /// Asks for the small rivers of a window of the heightmap as it is now.
    /// The result of a window request before this one does not arrive.
    pub fn request_window(&mut self, map: &Heightmap, window: Window) {
        if !self.has_flow {
            self.request();
        }
        self.window.sent += 1;
        let (number, heights) = (self.window.sent, WindowHeights::new(map, window));
        match &self.flow {
            Some(flow) if self.sync => {
                let _ = self
                    .done
                    .send(Done::Window(number, window_channels(&heights, flow)));
            }
            _ => self.send(Job::Window(number, heights)),
        }
    }

    /// Stops the wait for a window. The result of a window request before
    /// this call does not arrive.
    pub fn drop_window(&mut self) {
        self.window.sent += 1;
        self.window.received = self.window.sent;
        self.channel_window = None;
    }

    fn read(&mut self) {
        for result in self.results.try_iter() {
            match result {
                Done::Global(number, channels) => {
                    self.global.received = number;
                    self.channels = Some(channels);
                }
                Done::Window(number, window) if number == self.window.sent => {
                    self.window.received = number;
                    self.channel_window = Some(window);
                }
                Done::Window(..) => {}
            }
        }
    }

    /// The newest channel map that arrived after the last poll. It can be
    /// older than the newest request.
    pub fn poll(&mut self) -> Option<Arc<ChannelMap>> {
        self.read();
        self.channels.take().map(Arc::new)
    }

    /// The window of the newest window request, if it arrived after the last
    /// poll.
    pub fn poll_window(&mut self) -> Option<Arc<ChannelWindow>> {
        self.read();
        self.channel_window.take().map(Arc::new)
    }

    /// True until the results of the newest requests arrive.
    pub fn pending(&self) -> bool {
        self.global.pending() || self.window.pending()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::meters_to_level;

    use super::*;
    use crate::globe::math::{angle, lonlat_to_dir};

    /// Polls until the newest results arrive. Returns the newest of each
    /// kind.
    fn wait(rivers: &mut Rivers) -> (Option<Arc<ChannelMap>>, Option<Arc<ChannelWindow>>) {
        let (mut channels, mut window) = (None, None);
        for _ in 0..4000 {
            channels = rivers.poll().or(channels);
            window = rivers.poll_window().or(window);
            if !rivers.pending() {
                return (channels, window);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the worker gave no result");
    }

    /// A hill on face 0 of a heightmap in the sea.
    fn hill(n: usize) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(-100.0));
        for y in 0..n {
            for x in 0..n {
                let from_edge = x.min(y).min(n - 1 - x).min(n - 1 - y) as f64;
                let meters = from_edge * 6400.0 / n as f64 - 400.0;
                map.set(0, x, y, meters_to_level(meters));
            }
        }
        map
    }

    #[test]
    fn the_worker_gives_the_newest_result() {
        const N: usize = 64;
        let map = Heightmap::new(N, meters_to_level(-100.0));
        let mut rivers = Rivers::new(&map, false);
        assert!(!rivers.pending());
        assert!(rivers.poll().is_none());
        // The first request has no land. The second request has a hill.
        rivers.request();
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: N,
            y1: N,
        };
        rivers.update(&hill(N), 0, rect);
        rivers.request();
        assert!(rivers.pending());
        assert!(wait(&mut rivers).0.unwrap().has_rivers());
        assert!(rivers.poll().is_none());

        // The whole heightmap is sea again.
        rivers.rebuild(&map);
        rivers.request();
        assert!(!wait(&mut rivers).0.unwrap().has_rivers());
    }

    #[test]
    fn a_sync_request_is_ready_at_the_next_poll() {
        let map = hill(1024);
        let mut rivers = Rivers::new(&map, true);
        let window = Window::centered(lonlat_to_dir(0.0, 0.0), 1024, 1, WINDOW_CELLS);
        // The window request asks for the flow of the world first.
        rivers.request_window(&map, window);
        assert!(rivers.poll().unwrap().has_rivers());
        let channels = rivers.poll_window().unwrap();
        assert!(!rivers.pending());
        assert_eq!(channels.window(), window);
        assert!(channels.has_rivers());
    }

    #[test]
    fn the_worker_gives_the_window_of_the_newest_request() {
        let map = hill(1024);
        let mut rivers = Rivers::new(&map, false);
        let at = |lon: f64| Window::centered(lonlat_to_dir(lon, 0.0), 1024, 1, WINDOW_CELLS);
        rivers.request_window(&map, at(-10.0));
        rivers.request_window(&map, at(10.0));
        assert!(rivers.pending());
        let (channels, window) = wait(&mut rivers);
        assert!(channels.unwrap().has_rivers());
        let window = window.unwrap();
        assert_eq!(window.window(), at(10.0));
        assert!(window.has_rivers());
        assert!(rivers.poll_window().is_none());
    }

    #[test]
    fn a_window_that_the_view_left_does_not_arrive() {
        let map = hill(1024);
        let mut rivers = Rivers::new(&map, false);
        let window = Window::centered(lonlat_to_dir(0.0, 0.0), 1024, 1, WINDOW_CELLS);
        rivers.request_window(&map, window);
        rivers.drop_window();
        let (channels, window) = wait(&mut rivers);
        assert!(channels.is_some());
        assert!(window.is_none());
        // The worker ends the window job after the wait.
        std::thread::sleep(Duration::from_millis(50));
        assert!(rivers.poll_window().is_none());
    }

    const N: usize = 8192;
    /// The span of a view that needs cells of one texel at the face size `N`.
    const NEAR: f64 = 0.06;

    #[test]
    fn a_far_view_has_no_window() {
        let middle = lonlat_to_dir(20.0, 30.0);
        assert_eq!(choose_window(middle, 3.0, N, None), None);
        // The cells of this window are the cells of the channel map.
        assert_eq!(choose_window(middle, 0.6, N, None), None);
        assert_eq!(choose_window(middle, 0.5, N, None).unwrap().cell, 8);
        // A cell of the channel map is one texel at this face size.
        assert_eq!(choose_window(middle, 0.01, 256, None), None);
        assert_eq!(choose_window(middle, 0.0, N, None), None);
    }

    #[test]
    fn a_near_view_has_a_window_of_fine_cells() {
        let middle = lonlat_to_dir(20.0, 30.0);
        let window = choose_window(middle, NEAR, N, None).unwrap();
        assert_eq!((window.cell, window.cells), (1, WINDOW_CELLS));
        assert_eq!(window.face_size, N);
        // The window has the view around its middle.
        let (u, v) = window.place(middle).unwrap();
        let half = NEAR / 2.0 / cell_angle(&window);
        for p in [u, v] {
            assert!(p - half > 8.0 && p + half < 504.0, "{p}");
        }
        // The cells grow with the span.
        assert_eq!(choose_window(middle, 0.1, N, None).unwrap().cell, 2);
        assert_eq!(choose_window(middle, 0.2, N, None).unwrap().cell, 4);
    }

    #[test]
    fn a_small_pan_keeps_the_window() {
        let window = choose_window(lonlat_to_dir(20.0, 30.0), NEAR, N, None).unwrap();
        for (lon, lat) in [(20.3, 30.0), (19.8, 30.2), (20.0, 29.7)] {
            let middle = lonlat_to_dir(lon, lat);
            assert_eq!(choose_window(middle, NEAR, N, Some(window)), Some(window));
            // A view with no window gets the same one from a place near it.
            let fresh = choose_window(middle, NEAR, N, None).unwrap();
            assert!((fresh.x0 - window.x0).abs() <= 128 && (fresh.y0 - window.y0).abs() <= 128);
        }
        // A small zoom keeps it also.
        let middle = lonlat_to_dir(20.0, 30.0);
        assert_eq!(
            choose_window(middle, NEAR * 0.8, N, Some(window)),
            Some(window)
        );
    }

    #[test]
    fn a_large_pan_moves_the_window() {
        let from = lonlat_to_dir(20.0, 30.0);
        let window = choose_window(from, NEAR, N, None).unwrap();
        let to = lonlat_to_dir(24.0, 30.0);
        let moved = choose_window(to, NEAR, N, Some(window)).unwrap();
        assert_ne!(moved, window);
        assert_eq!(moved.cell, 1);
        let (u, v) = moved.place(to).unwrap();
        assert!((u - 256.0).abs() <= 65.0 && (v - 256.0).abs() <= 65.0);
        // A zoom to another cell size gives another window at the same place.
        let wide = choose_window(from, 0.2, N, Some(window)).unwrap();
        assert_eq!(wide.cell, 4);
        assert!(angle(wide.dir(256.0, 256.0), from) < 0.06);
    }

    #[test]
    fn the_window_stays_within_half_a_face_of_the_edge() {
        for n in [1024usize, 2048, 8192] {
            let global = n / CoarseHeights::size_for(n);
            for step in 0..400 {
                // Places along a face edge and across a cube corner.
                let t = f64::from(step) / 400.0;
                let lon = 45.0 + (t * 37.0).sin() * 3.0;
                let lat = -50.0 + 100.0 * t;
                let mut cell = 1;
                while cell < global {
                    let span = cell as f64 * WINDOW_CELLS as f64 * FRAC_PI_2 / n as f64 / 1.6;
                    let window = choose_window(lonlat_to_dir(lon, lat), span, n, None).unwrap();
                    assert_eq!(window.cell, cell);
                    let (half, side) = (n as i64 / 2, (cell * WINDOW_CELLS) as i64);
                    for first in [window.x0, window.y0] {
                        assert_eq!(first % cell as i64, 0);
                        assert!(
                            first > -half && first + side < n as i64 + half,
                            "{window:?}"
                        );
                    }
                    cell *= 2;
                }
            }
        }
    }
}
