//! The erode brush of the globe. A worker thread computes the steps of a
//! stroke, so the brush does not wait for them.

use std::sync::{Arc, mpsc};

use std::f64::consts::FRAC_PI_2;

use vale_terrain::{
    CoarseHeights, ErodeBrush, ErodeGrid, ErodeStep, Erosion, FlowMap, OuterFlow, WINDOW_CELLS,
    Window, WindowHeights,
};

use super::math::V3;

/// The radius of the brush is this number of cells of its window or more, if
/// the texels of the heightmap are small enough. With fewer cells, the
/// valleys of the erosion are too wide for the brush.
const BRUSH_RADIUS_CELLS: f64 = 16.0;

/// A window moves when the brush of its stroke is nearer to its side than
/// this number of brush radii.
const BRUSH_WINDOW_ROOM: f64 = 3.0;

/// The window of the cells of a brush around `center`. `radius` is the
/// radius of the brush, in radians. The cell is the largest that keeps
/// `BRUSH_RADIUS_CELLS` cells in the radius, and it is not larger than a
/// cell of the coarse copy.
pub fn brush_window(center: V3, radius: f64, face_size: usize) -> Window {
    let n = face_size;
    let texels = radius * n as f64 / FRAC_PI_2;
    let global = n / CoarseHeights::size_for(n);
    let mut cell = 1;
    while 2 * cell <= global && texels / (2 * cell) as f64 >= BRUSH_RADIUS_CELLS {
        cell *= 2;
    }
    let cells = WINDOW_CELLS.min(n / cell);
    let mut window = Window::centered(center, n, cell, cells);
    // The window ends less than half a face past the face edge.
    let step = cell as i64;
    let past = (n as i64 / 2 - 1) / step * step;
    let last = n as i64 + past - (cells * cell) as i64;
    window.x0 = window.x0.clamp(-past, last);
    window.y0 = window.y0.clamp(-past, last);
    window
}

/// Whether a brush at `center` is too near to the side of the window of its
/// stroke, or outside the window. A brush in the middle half of the
/// window is not too near.
pub fn near_side(window: &Window, center: V3, radius: f64) -> bool {
    let Some((u, v)) = window.place(center) else {
        return true;
    };
    let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
    let cells = window.cells as f64;
    let room = (BRUSH_WINDOW_ROOM * radius / cell).min(cells / 4.0);
    u.min(v).min(cells - u).min(cells - v) < room
}

/// The window of the cells of an erode stroke with a brush at `center`, or
/// `None` for a stroke on the cells of the view. `view` is the window of the
/// view, and a far view has none. The stroke has the window of the brush if
/// a cell of that window is smaller than a cell of the view.
pub fn erode_window(
    view: Option<Window>,
    center: V3,
    radius: f64,
    face_size: usize,
) -> Option<Window> {
    let window = brush_window(center, radius, face_size);
    let global = face_size / CoarseHeights::size_for(face_size);
    let cell = view.map_or(global, |view| view.cell);
    (window.cell < cell).then_some(window)
}

/// The cells that a stroke lowers.
pub enum Ground {
    /// The cells of the coarse copy of a heightmap with this face size.
    Global {
        heights: CoarseHeights,
        face_size: usize,
    },
    /// The cells of a window. `global` gives the rivers that come into the
    /// window from outside.
    Window {
        heights: WindowHeights,
        global: CoarseHeights,
    },
    /// The cells of a window that no river comes into from outside.
    Alone(WindowHeights),
    /// The cells of a window in the ground of a view. The water of that
    /// ground comes into the window from outside. `view` has the cells of
    /// the window of the view. Without it, the ground of the view is the
    /// cells of `global`.
    Nested {
        heights: WindowHeights,
        view: Option<WindowHeights>,
        global: CoarseHeights,
    },
    /// The cells of a window in the ground of the view of the stroke. The
    /// stroke began on `Nested` ground.
    Follow(WindowHeights),
}

/// The erosion of a stroke.
struct Work {
    erosion: Erosion,
    /// The flow of the ground of the view at the start of the stroke.
    /// `Some`: its water comes into the window of the stroke.
    outer: Option<OuterFlow>,
}

impl Ground {
    fn begin(&self) -> Work {
        let outer = match self {
            Ground::Nested {
                view: Some(view),
                global,
                ..
            } => Some(OuterFlow::window(view, &FlowMap::new(global))),
            Ground::Nested {
                heights,
                view: None,
                global,
            } => Some(OuterFlow::global(global, heights.window().face_size)),
            _ => None,
        };
        let erosion = match (self, &outer) {
            (Ground::Global { heights, face_size }, _) => Erosion::global(heights, *face_size),
            (Ground::Window { heights, global }, _) => {
                Erosion::window(heights, &FlowMap::new(global))
            }
            (Ground::Nested { heights, .. }, Some(outer)) => Erosion::window_in(heights, outer),
            (
                Ground::Alone(heights) | Ground::Follow(heights) | Ground::Nested { heights, .. },
                _,
            ) => Erosion::window_alone(heights),
        };
        Work { erosion, outer }
    }

    /// Gives this ground to a stroke that goes on. If the stroke has the
    /// cells of a window, and this ground is `Alone` or `Follow`, the cells
    /// of the stroke keep their heights. On `Follow` ground, the water of
    /// the view of the stroke still comes in.
    fn move_work(&self, work: &mut Work) {
        let window = matches!(work.erosion.grid(), ErodeGrid::Window(_));
        match self {
            Ground::Alone(heights) if window => {
                work.erosion = work.erosion.moved(heights, None);
                work.outer = None;
            }
            Ground::Follow(heights) if window => {
                work.erosion = work.erosion.moved(heights, work.outer.as_ref());
            }
            _ => *work = self.begin(),
        }
    }
}

/// One request, with the number of its stroke.
enum Job {
    Begin(u64, Ground),
    /// The stroke goes on with this ground.
    Move(u64, Ground),
    /// The brushes, the rate, and the uplift of one step.
    Step(u64, Vec<ErodeBrush>, f64, f64),
}

struct Done(u64, ErodeStep);

pub struct Eroder {
    /// A request computes its result on the thread that asks, so the result
    /// is ready at the next poll. A change applies from the next stroke.
    sync: bool,
    /// The erosion of a stroke that began in the sync mode.
    work: Option<Work>,
    /// The path to the worker. `None`: the worker did not start yet.
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Sender<Done>,
    results: mpsc::Receiver<Done>,
    /// The number of the stroke of the last `begin`.
    stroke: u64,
    /// The number of steps of the stroke that did not arrive yet.
    waiting: usize,
}

/// The number of steps that lower the cells after an uplift.
const UPLIFT_STEPS: usize = 4;

/// Without an uplift, lowers the cells under the brushes one time. With an
/// uplift, raises those cells by `uplift` levels at full effect and then
/// lowers them `UPLIFT_STEPS` times.
fn step(erosion: &mut Erosion, brushes: &[ErodeBrush], rate: f64, uplift: f64) -> ErodeStep {
    if uplift.is_nan() || uplift <= 0.0 {
        return erosion.step(brushes, rate);
    }
    erosion.raise(brushes, uplift);
    erosion.steps(brushes, rate, UPLIFT_STEPS)
}

/// Computes each job in the order of the requests until the sender is gone.
/// The erosion keeps the heights of its stroke from one step to the next.
fn work(jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    let mut work: Option<(u64, Work)> = None;
    for job in jobs {
        match job {
            Job::Begin(stroke, ground) => work = Some((stroke, ground.begin())),
            Job::Move(stroke, ground) => {
                if let Some((_, work)) = work.as_mut().filter(|(id, _)| *id == stroke) {
                    ground.move_work(work);
                }
            }
            Job::Step(stroke, brushes, rate, uplift) => {
                let Some((_, work)) = work.as_mut().filter(|(id, _)| *id == stroke) else {
                    continue;
                };
                let step = step(&mut work.erosion, &brushes, rate, uplift);
                if done.send(Done(stroke, step)).is_err() {
                    return;
                }
            }
        }
    }
}

impl Eroder {
    pub fn new(sync: bool) -> Eroder {
        let (done, results) = mpsc::channel();
        Eroder {
            sync,
            work: None,
            jobs: None,
            done,
            results,
            stroke: 0,
            waiting: 0,
        }
    }

    pub fn set_sync(&mut self, sync: bool) {
        self.sync = sync;
    }

    fn send(&mut self, job: Job) {
        let jobs = self.jobs.get_or_insert_with(|| {
            let (jobs, receiver) = mpsc::channel();
            let done = self.done.clone();
            std::thread::Builder::new()
                .name("vale-erode".to_string())
                .spawn(move || work(receiver, done))
                .expect("the system can start a thread");
            jobs
        });
        let _ = jobs.send(job);
    }

    /// Stops the stroke. A step of a request before this call does not
    /// arrive.
    pub fn cancel(&mut self) {
        self.stroke += 1;
        self.waiting = 0;
        self.work = None;
    }

    /// Starts a stroke on this ground. A step of the stroke before it does
    /// not arrive.
    pub fn begin(&mut self, ground: Ground) {
        self.cancel();
        if self.sync {
            self.work = Some(ground.begin());
        } else {
            self.send(Job::Begin(self.stroke, ground));
        }
    }

    /// Gives the stroke new ground. The steps of the requests after this
    /// call have the cells of that ground. If the old ground is a window
    /// and the new ground is `Alone` or `Follow`, a cell of the old window
    /// keeps the height that the stroke gave it. On `Follow` ground, the
    /// water of the view of the stroke still comes in.
    pub fn move_to(&mut self, ground: Ground) {
        match &mut self.work {
            Some(work) => ground.move_work(work),
            None => self.send(Job::Move(self.stroke, ground)),
        }
    }

    /// Asks for one step of the stroke. The step starts from the heights
    /// that the step before it left. Before the step, each brush raises its
    /// cells by `uplift` levels at full effect.
    pub fn request(&mut self, brushes: Vec<ErodeBrush>, rate: f64, uplift: f64) {
        self.waiting += 1;
        match &mut self.work {
            Some(work) => {
                let step = step(&mut work.erosion, &brushes, rate, uplift);
                let _ = self.done.send(Done(self.stroke, step));
            }
            None => self.send(Job::Step(self.stroke, brushes, rate, uplift)),
        }
    }

    /// The oldest step of the stroke that arrived after the last poll.
    pub fn poll(&mut self) -> Option<Arc<ErodeStep>> {
        while let Ok(Done(stroke, step)) = self.results.try_recv() {
            if stroke == self.stroke && self.waiting > 0 {
                self.waiting -= 1;
                return Some(Arc::new(step));
            }
        }
        None
    }

    /// True until each step that the stroke asked for arrives.
    pub fn pending(&self) -> bool {
        self.waiting > 0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::{Heightmap, WINDOW_CELLS, Window, meters_to_level};

    use super::*;
    use crate::globe::math::lonlat_to_dir;

    const N: usize = 1024;

    /// A hill on face 0 of a heightmap in the sea.
    fn hill() -> Heightmap {
        let mut map = Heightmap::new(N, meters_to_level(-100.0));
        for y in 0..N {
            for x in 0..N {
                let from_edge = x.min(y).min(N - 1 - x).min(N - 1 - y) as f64;
                let meters = from_edge * 6400.0 / N as f64 - 400.0;
                map.set(0, x, y, meters_to_level(meters));
            }
        }
        map
    }

    fn global(map: &Heightmap) -> Ground {
        Ground::Global {
            heights: CoarseHeights::new(map),
            face_size: N,
        }
    }

    /// A brush on the side of the hill, or on the sea at the other side of
    /// the world.
    fn brush(on_land: bool) -> Vec<ErodeBrush> {
        let lon = if on_land { 20.0 } else { 180.0 };
        vec![ErodeBrush {
            center: lonlat_to_dir(lon, 0.0),
            radius: 0.1,
            hardness: 0.5,
            flow: 1.0,
        }]
    }

    /// Polls until a step arrives.
    fn wait(eroder: &mut Eroder) -> Arc<ErodeStep> {
        for _ in 0..4000 {
            if let Some(step) = eroder.poll() {
                return step;
            }
            assert!(eroder.pending());
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the worker gave no result");
    }

    #[test]
    fn the_worker_gives_each_step_of_the_stroke() {
        let mut eroder = Eroder::new(false);
        assert!(!eroder.pending());
        assert!(eroder.poll().is_none());
        eroder.begin(global(&hill()));
        eroder.request(brush(true), 1.0, 0.0);
        // The request returns before the worker has the flow of the world.
        assert!(eroder.pending());
        assert!(eroder.poll().is_none());
        let first = wait(&mut eroder);
        assert!(!eroder.pending());
        assert!(!first.is_empty());
        assert!(eroder.poll().is_none());

        // The second step starts from the heights that the first step left.
        eroder.request(brush(true), 1.0, 0.0);
        let second = wait(&mut eroder);
        assert!(!second.is_empty());
        assert!(first.drops() != second.drops());
    }

    #[test]
    fn a_sync_request_is_ready_at_the_next_poll() {
        let map = hill();
        let mut eroder = Eroder::new(true);
        eroder.begin(global(&map));
        eroder.request(brush(true), 1.0, 0.0);
        assert!(eroder.pending());
        assert!(!eroder.poll().unwrap().is_empty());
        assert!(!eroder.pending());

        // A window has its own cells.
        let window = Window::centered(lonlat_to_dir(20.0, 0.0), N, 1, WINDOW_CELLS);
        eroder.begin(Ground::Window {
            heights: WindowHeights::new(&map, window),
            global: CoarseHeights::new(&map),
        });
        eroder.request(brush(true), 1.0, 0.0);
        let step = eroder.poll().unwrap();
        assert!(!step.is_empty());
        assert_eq!(step.grid(), vale_terrain::ErodeGrid::Window(window));
    }

    #[test]
    fn a_request_with_uplift_raises_the_cells_before_the_step() {
        // The sea floor is flat, so a step without uplift lowers nothing.
        let map = Heightmap::new(N, meters_to_level(-100.0));
        let mut eroder = Eroder::new(true);
        eroder.begin(global(&map));
        eroder.request(brush(true), 1.0, 0.0);
        assert!(eroder.poll().unwrap().is_empty());
        eroder.request(brush(true), 1.0, 5000.0);
        assert!(!eroder.poll().unwrap().is_empty());
    }

    #[test]
    fn the_window_of_a_brush_follows_the_radius_of_the_brush() {
        let center = lonlat_to_dir(10.0, 5.0);
        let texel = FRAC_PI_2 / 8192.0;
        let cases = [
            (10.0, 1),
            (31.0, 1),
            (33.0, 2),
            (100.0, 4),
            (255.0, 8),
            (600.0, 16),
        ];
        for (radius, cell) in cases {
            let window = brush_window(center, radius * texel, 8192);
            assert_eq!((window.cell, window.cells), (cell, WINDOW_CELLS));
            let (u, v) = window.place(center).unwrap();
            assert!((u - 256.0).abs() < 1.0 && (v - 256.0).abs() < 1.0);
            assert!(!near_side(&window, center, radius * texel));
            let side = window.cell_dir((2 * radius as usize / cell).min(100), 256);
            assert!(near_side(&window, side, radius * texel));
            assert!(near_side(
                &window,
                lonlat_to_dir(180.0, 0.0),
                radius * texel
            ));
        }
        // The window stays less than half a face past the edge of its face.
        let corner = brush_window(lonlat_to_dir(44.0, 34.0), 0.001, N);
        let (first, last) = (-(N as i64 / 2), (N + N / 2) as i64);
        for x0 in [corner.x0, corner.y0] {
            assert!(x0 > first && x0 + (corner.cells * corner.cell) as i64 <= last);
        }
    }

    #[test]
    fn a_stroke_that_moves_keeps_its_heights_in_the_new_window() {
        let map = Heightmap::new(N, meters_to_level(-100.0));
        let ground = |lon: f64| {
            let window = brush_window(lonlat_to_dir(lon, 0.0), 0.02, N);
            Ground::Alone(WindowHeights::new(&map, window))
        };
        let at = |lon: f64| {
            vec![ErodeBrush {
                center: lonlat_to_dir(lon, 0.0),
                radius: 0.02,
                hardness: 0.5,
                flow: 1.0,
            }]
        };
        for sync in [true, false] {
            let mut eroder = Eroder::new(sync);
            eroder.begin(ground(0.0));
            eroder.request(at(8.0), 1.0, 5000.0);
            assert!(!wait(&mut eroder).is_empty());
            // The hill at 8 degrees is in the new window, so a step without
            // uplift lowers it there.
            eroder.move_to(ground(15.0));
            eroder.request(at(8.0), 1.0, 0.0);
            let step = wait(&mut eroder);
            assert!(!step.is_empty());
            let ErodeGrid::Window(window) = step.grid() else {
                panic!("the step has no window");
            };
            assert!(!near_side(&window, lonlat_to_dir(15.0, 0.0), 0.02));
        }
    }

    #[test]
    fn an_erode_stroke_has_the_window_of_the_brush_if_its_cells_are_smaller() {
        let n = 8192;
        let center = lonlat_to_dir(10.0, 5.0);
        let texel = FRAC_PI_2 / n as f64;
        // A cell of the global map is 16 texels wide.
        assert_eq!(n / CoarseHeights::size_for(n), 16);
        let near = Window::centered(center, n, 4, WINDOW_CELLS);
        // The view, the radius of the brush in texels, and the cell of the
        // window of the brush.
        let cases = [
            (None, 100.0, Some(4)),
            (None, 255.0, Some(8)),
            (None, 257.0, None),
            (None, 600.0, None),
            (Some(near), 31.0, Some(1)),
            (Some(near), 63.0, Some(2)),
            (Some(near), 65.0, None),
            (Some(near), 600.0, None),
        ];
        for (view, radius, cell) in cases {
            let window = erode_window(view, center, radius * texel, n);
            assert_eq!(window.map(|window| window.cell), cell, "{radius}");
            if let Some(window) = window {
                assert_eq!(window, brush_window(center, radius * texel, n));
            }
        }
    }

    /// The sum of the drops of a step.
    fn sum(step: &ErodeStep) -> u64 {
        step.drops().iter().map(|&drop| u64::from(drop)).sum()
    }

    #[test]
    fn a_stroke_on_the_window_of_the_brush_keeps_its_heights_and_the_water_of_the_view() {
        let map = hill();
        let radius = 0.02;
        let window = |lon: f64| brush_window(lonlat_to_dir(lon, 0.0), radius, N);
        let heights = |lon: f64| WindowHeights::new(&map, window(lon));
        // The window of a near view has cells of 2 texels.
        let near = Window::centered(lonlat_to_dir(30.0, 0.0), N, 2, WINDOW_CELLS);
        let nested = |lon: f64, view: Option<Window>| Ground::Nested {
            heights: heights(lon),
            view: view.map(|view| WindowHeights::new(&map, view)),
            global: CoarseHeights::new(&map),
        };
        let at = |lon: f64| {
            vec![ErodeBrush {
                center: lonlat_to_dir(lon, 0.0),
                radius,
                hardness: 0.5,
                flow: 1.0,
            }]
        };
        let grid = |lon: f64| ErodeGrid::Window(window(lon));
        assert_eq!(window(30.0).cell, 1);
        for (sync, view) in [(true, None), (true, Some(near)), (false, Some(near))] {
            // The first step of a stroke in the second window.
            let mut eroder = Eroder::new(sync);
            eroder.begin(nested(34.0, view));
            eroder.request(at(30.0), 1.0, 0.0);
            let fresh = sum(&wait(&mut eroder));

            // A stroke that moves from the first window to the second one.
            // The result is the sums of the drops of 2 steps after the move:
            // a step at the place of the stroke, and a weak step on new
            // ground near the high side of the window.
            let mut moved = |ground: Ground| {
                eroder.begin(nested(30.0, view));
                eroder.request(at(30.0), 1.0e6, 0.0);
                let first = wait(&mut eroder);
                assert!(!first.is_empty());
                assert_eq!(first.grid(), grid(30.0));
                eroder.move_to(ground);
                eroder.request(at(30.0), 1.0, 0.0);
                let again = wait(&mut eroder);
                assert_eq!(again.grid(), grid(34.0));
                eroder.request(at(18.0), 0.02, 0.0);
                (sum(&again), sum(&wait(&mut eroder)))
            };
            let (again, wet) = moved(Ground::Follow(heights(34.0)));
            let (_, dry) = moved(Ground::Alone(heights(34.0)));
            eprintln!("fresh {fresh}, again {again}, wet {wet}, dry {dry}");
            // The first step left the ground under the brush flat, so the
            // step after the move lowers it less than a first step.
            assert!(fresh > 0 && 4 * again < fresh, "{again} {fresh}");
            // The water of the view makes the step on new ground stronger.
            assert!(4 * wet > 5 * dry, "{wet} {dry}");
        }
    }

    /// Run with `--release --ignored --nocapture` for the time of the start
    /// of a stroke on the window of a brush, and the time of one step.
    #[test]
    #[ignore]
    fn step_time_on_the_window_of_a_brush() {
        let n = 8192;
        let mut map = Heightmap::new(n, meters_to_level(-100.0));
        for y in 0..n {
            for x in 0..n {
                let from_edge = x.min(y).min(n - 1 - x).min(n - 1 - y) as f64;
                let waves = ((x / 40) as f64).sin() + ((y / 56) as f64).cos();
                let meters = from_edge * 6400.0 / n as f64 - 400.0 + 60.0 * waves;
                map.set(0, x, y, meters_to_level(meters));
            }
        }
        let center = lonlat_to_dir(20.0, 0.0);
        let radius = 20.0 * FRAC_PI_2 / n as f64;
        let view = Window::centered(center, n, 4, WINDOW_CELLS);
        let window = erode_window(Some(view), center, radius, n).unwrap();
        assert_eq!((window.cell, window.cells), (1, WINDOW_CELLS));
        let ground = Ground::Nested {
            heights: WindowHeights::new(&map, window),
            view: Some(WindowHeights::new(&map, view)),
            global: CoarseHeights::new(&map),
        };
        let brushes = [ErodeBrush {
            center,
            radius,
            hardness: 0.5,
            flow: 1.0,
        }];
        let Ground::Nested {
            heights,
            view,
            global,
        } = &ground
        else {
            unreachable!();
        };
        let start = std::time::Instant::now();
        let flow = FlowMap::new(global);
        eprintln!("the flow of the global map {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let outer = OuterFlow::window(view.as_ref().unwrap(), &flow);
        eprintln!("the flow of the view {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let mut erosion = Erosion::window_in(heights, &outer);
        eprintln!("the erosion of the window {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let mut work = ground.begin();
        eprintln!("the start of the stroke {:?}", start.elapsed());
        for _ in 0..4 {
            let start = std::time::Instant::now();
            let step = erosion.step(&brushes, 1.0);
            let lowered = step.drops().iter().filter(|&&drop| drop > 0).count();
            eprintln!("lowered cells {lowered}, step {:?}", start.elapsed());
            assert!(lowered > 0);
        }
        let moved = brush_window(lonlat_to_dir(20.5, 0.0), radius, n);
        let heights = WindowHeights::new(&map, moved);
        let start = std::time::Instant::now();
        Ground::Follow(heights).move_work(&mut work);
        eprintln!("the move of the window {:?}", start.elapsed());
    }

    #[test]
    fn a_step_of_an_old_stroke_does_not_arrive() {
        let map = hill();
        let mut eroder = Eroder::new(false);
        eroder.begin(global(&map));
        eroder.request(brush(true), 1.0, 0.0);
        // The brush of the new stroke is on the sea, so its step is empty.
        eroder.begin(global(&map));
        assert!(!eroder.pending());
        eroder.request(brush(false), 1.0, 0.0);
        assert!(wait(&mut eroder).is_empty());
        assert!(!eroder.pending());
        assert!(eroder.poll().is_none());

        eroder.request(brush(true), 1.0, 0.0);
        eroder.cancel();
        assert!(!eroder.pending());
        // The worker ends the step after the wait.
        std::thread::sleep(Duration::from_millis(300));
        assert!(eroder.poll().is_none());
    }
}
