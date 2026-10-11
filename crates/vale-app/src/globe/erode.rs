//! The erode brush of the globe. A worker thread computes the steps of a
//! stroke, so the brush does not wait for them.

use std::sync::{Arc, mpsc};

use vale_terrain::{CoarseHeights, ErodeBrush, ErodeStep, Erosion, FlowMap, WindowHeights};

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
}

impl Ground {
    fn erosion(&self) -> Erosion {
        match self {
            Ground::Global { heights, face_size } => Erosion::global(heights, *face_size),
            Ground::Window { heights, global } => Erosion::window(heights, &FlowMap::new(global)),
        }
    }
}

/// One request, with the number of its stroke.
enum Job {
    Begin(u64, Ground),
    /// The brushes and the rate of one step.
    Step(u64, Vec<ErodeBrush>, f64),
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

/// Computes each job in the order of the requests until the sender is gone.
/// The erosion keeps the heights of its stroke from one step to the next.
fn work(jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    let mut erosion: Option<(u64, Erosion)> = None;
    for job in jobs {
        match job {
            Job::Begin(stroke, ground) => erosion = Some((stroke, ground.erosion())),
            Job::Step(stroke, brushes, rate) => {
                let Some((_, erosion)) = erosion.as_mut().filter(|(id, _)| *id == stroke) else {
                    continue;
                };
                if done
                    .send(Done(stroke, erosion.step(&brushes, rate)))
                    .is_err()
                {
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

    /// Asks for one step of the stroke. The step starts from the heights
    /// that the step before it left.
    pub fn request(&mut self, brushes: Vec<ErodeBrush>, rate: f64) {
        self.waiting += 1;
        match &mut self.erosion {
            Some(erosion) => {
                let step = erosion.step(&brushes, rate);
                let _ = self.done.send(Done(self.stroke, step));
            }
            None => self.send(Job::Step(self.stroke, brushes, rate)),
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
        eroder.request(brush(true), 1.0);
        // The request returns before the worker has the flow of the world.
        assert!(eroder.pending());
        assert!(eroder.poll().is_none());
        let first = wait(&mut eroder);
        assert!(!eroder.pending());
        assert!(!first.is_empty());
        assert!(eroder.poll().is_none());

        // The second step starts from the heights that the first step left.
        eroder.request(brush(true), 1.0);
        let second = wait(&mut eroder);
        assert!(!second.is_empty());
        assert!(first.drops() != second.drops());
    }

    #[test]
    fn a_sync_request_is_ready_at_the_next_poll() {
        let map = hill();
        let mut eroder = Eroder::new(true);
        eroder.begin(global(&map));
        eroder.request(brush(true), 1.0);
        assert!(eroder.pending());
        assert!(!eroder.poll().unwrap().is_empty());
        assert!(!eroder.pending());

        // A window has its own cells.
        let window = Window::centered(lonlat_to_dir(20.0, 0.0), N, 1, WINDOW_CELLS);
        eroder.begin(Ground::Window {
            heights: WindowHeights::new(&map, window),
            global: CoarseHeights::new(&map),
        });
        eroder.request(brush(true), 1.0);
        let step = eroder.poll().unwrap();
        assert!(!step.is_empty());
        assert_eq!(step.grid(), vale_terrain::ErodeGrid::Window(window));
    }

    #[test]
    fn a_step_of_an_old_stroke_does_not_arrive() {
        let map = hill();
        let mut eroder = Eroder::new(false);
        eroder.begin(global(&map));
        eroder.request(brush(true), 1.0);
        // The brush of the new stroke is on the sea, so its step is empty.
        eroder.begin(global(&map));
        assert!(!eroder.pending());
        eroder.request(brush(false), 1.0);
        assert!(wait(&mut eroder).is_empty());
        assert!(!eroder.pending());
        assert!(eroder.poll().is_none());

        eroder.request(brush(true), 1.0);
        eroder.cancel();
        assert!(!eroder.pending());
        // The worker ends the step after the wait.
        std::thread::sleep(Duration::from_millis(300));
        assert!(eroder.poll().is_none());
    }
}
