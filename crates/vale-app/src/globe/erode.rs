//! The erode brush of the globe. A worker thread computes the steps of a
//! stroke, so the brush does not wait for them.

use std::sync::{Arc, mpsc};

use std::f64::consts::FRAC_PI_2;

use vale_terrain::{
    CoarseHeights, ErodeBrush, ErodeGrid, ErodeStep, Erosion, FlowMap, WINDOW_CELLS, Window,
    WindowHeights,
};

use super::math::V3;

/// The radius of the brush of an uplift stroke is this number of cells or
/// more, if the texels of the heightmap are small enough. With fewer cells,
/// the valleys of the erosion are too wide for the range.
const UPLIFT_RADIUS_CELLS: f64 = 16.0;

/// A window moves when the brush of an uplift stroke is nearer to its side
/// than this number of brush radii.
const UPLIFT_WINDOW_ROOM: f64 = 3.0;

/// The window of the cells of an uplift stroke around `center`. `radius` is
/// the radius of the brush, in radians. The cell is the largest that keeps
/// `UPLIFT_RADIUS_CELLS` cells in the radius, and it is not larger than a
/// cell of the coarse copy.
pub fn uplift_window(center: V3, radius: f64, face_size: usize) -> Window {
    let n = face_size;
    let texels = radius * n as f64 / FRAC_PI_2;
    let global = n / CoarseHeights::size_for(n);
    let mut cell = 1;
    while 2 * cell <= global && texels / (2 * cell) as f64 >= UPLIFT_RADIUS_CELLS {
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
/// uplift stroke, or outside the window. A brush in the middle half of the
/// window is not too near.
pub fn near_side(window: &Window, center: V3, radius: f64) -> bool {
    let Some((u, v)) = window.place(center) else {
        return true;
    };
    let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
    let cells = window.cells as f64;
    let room = (UPLIFT_WINDOW_ROOM * radius / cell).min(cells / 4.0);
    u.min(v).min(cells - u).min(cells - v) < room
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
}

impl Ground {
    fn erosion(&self) -> Erosion {
        match self {
            Ground::Global { heights, face_size } => Erosion::global(heights, *face_size),
            Ground::Window { heights, global } => Erosion::window(heights, &FlowMap::new(global)),
            Ground::Alone(heights) => Erosion::window_alone(heights),
        }
    }
    /// The erosion of a stroke that goes on from `from` on this ground. If
    /// both have the cells of a window with no rivers from outside, the
    /// cells of `from` keep their heights.
    fn moved(&self, from: &Erosion) -> Erosion {
        match (self, from.grid()) {
            (Ground::Alone(heights), ErodeGrid::Window(_)) => from.moved(heights),
            _ => self.erosion(),
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
    erosion: Option<Erosion>,
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
    let mut erosion: Option<(u64, Erosion)> = None;
    for job in jobs {
        match job {
            Job::Begin(stroke, ground) => erosion = Some((stroke, ground.erosion())),
            Job::Move(stroke, ground) => {
                if let Some((_, erosion)) = erosion.as_mut().filter(|(id, _)| *id == stroke) {
                    *erosion = ground.moved(erosion);
                }
            }
            Job::Step(stroke, brushes, rate, uplift) => {
                let Some((_, erosion)) = erosion.as_mut().filter(|(id, _)| *id == stroke) else {
                    continue;
                };
                let step = step(erosion, &brushes, rate, uplift);
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
            erosion: None,
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
        self.erosion = None;
    }

    /// Starts a stroke on this ground. A step of the stroke before it does
    /// not arrive.
    pub fn begin(&mut self, ground: Ground) {
        self.cancel();
        if self.sync {
            self.erosion = Some(ground.erosion());
        } else {
            self.send(Job::Begin(self.stroke, ground));
        }
    }

    /// Gives the stroke new ground. The steps of the requests after this
    /// call have the cells of that ground. If the old ground is a window
    /// and the new ground is a window with no rivers from outside, a cell of
    /// the old window keeps the height that the stroke gave it.
    pub fn move_to(&mut self, ground: Ground) {
        match &mut self.erosion {
            Some(erosion) => *erosion = ground.moved(erosion),
            None => self.send(Job::Move(self.stroke, ground)),
        }
    }

    /// Asks for one step of the stroke. The step starts from the heights
    /// that the step before it left. Before the step, each brush raises its
    /// cells by `uplift` levels at full effect.
    pub fn request(&mut self, brushes: Vec<ErodeBrush>, rate: f64, uplift: f64) {
        self.waiting += 1;
        match &mut self.erosion {
            Some(erosion) => {
                let step = step(erosion, &brushes, rate, uplift);
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
    fn the_window_of_an_uplift_stroke_follows_the_radius_of_the_brush() {
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
            let window = uplift_window(center, radius * texel, 8192);
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
        let corner = uplift_window(lonlat_to_dir(44.0, 34.0), 0.001, N);
        let (first, last) = (-(N as i64 / 2), (N + N / 2) as i64);
        for x0 in [corner.x0, corner.y0] {
            assert!(x0 > first && x0 + (corner.cells * corner.cell) as i64 <= last);
        }
    }

    #[test]
    fn a_stroke_that_moves_keeps_its_heights_in_the_new_window() {
        let map = Heightmap::new(N, meters_to_level(-100.0));
        let ground = |lon: f64| {
            let window = uplift_window(lonlat_to_dir(lon, 0.0), 0.02, N);
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
