//! A run of the climate model: inline, or on a worker thread that does one
//! batch of steps at a time.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use isotherm_fields::{Means, Output, Period};
use isotherm_sim::{Grid, Model, RunState, STEPS_PER_MONTH, STEPS_PER_YEAR};
use isotherm_world::World;

/// The steps of each frame. The speed changes only how fast a run goes,
/// not its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Speed {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Slow, Speed::Normal, Speed::Fast];

    pub fn steps(self) -> u32 {
        match self {
            Speed::Slow => 1,
            Speed::Normal => 6,
            Speed::Fast => 30,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Speed::Slow => "Slow",
            Speed::Normal => "Normal",
            Speed::Fast => "Fast",
        }
    }
}

/// What the run tells the app after a batch.
struct Report {
    step: u64,
    /// The time of one step, in milliseconds.
    ms_per_step: f64,
    /// The means, when a month ended in the batch.
    means: Option<Means>,
}

enum Job {
    Advance(u32),
    Snapshot,
}

enum Reply {
    Advanced(Report),
    Snapshot(Box<RunState>),
}

struct Worker {
    jobs: Option<Sender<Job>>,
    replies: Receiver<Reply>,
    busy: bool,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Engine {
    Inline(Box<Model>),
    Worker(Worker),
}

/// Runs `steps` steps, and gives the report.
fn advance(model: &mut Model, steps: u32) -> Report {
    let start = Instant::now();
    let before = model.step_count();
    model.run(u64::from(steps));
    let ms_per_step = start.elapsed().as_secs_f64() * 1000.0 / f64::from(steps.max(1));
    // The last step of a month closes it.
    let per_month = u64::from(STEPS_PER_MONTH);
    let closed = model.step_count() / per_month != before / per_month;
    Report {
        step: model.step_count(),
        ms_per_step,
        means: closed.then(|| model.means().clone()),
    }
}

/// A run of the model and its controls.
pub struct Simulation {
    pub running: bool,
    pub speed: Speed,
    face_size: usize,
    step: u64,
    ms_per_step: f64,
    /// The means of the last month that ended, until the app takes them.
    news: Option<Means>,
    engine: Engine,
}

impl Simulation {
    /// A new run at face size `n`. `threaded`: the steps run on a worker
    /// thread.
    pub fn new(world: &World, face_size: usize, threaded: bool) -> Simulation {
        Simulation::with_model(Model::new(world, face_size), threaded)
    }

    /// A run that continues a saved state.
    pub fn restore(world: &World, state: RunState, threaded: bool) -> Result<Simulation, String> {
        Ok(Simulation::with_model(
            Model::restore(world, state)?,
            threaded,
        ))
    }

    fn with_model(model: Model, threaded: bool) -> Simulation {
        let face_size = model.grid().n();
        let step = model.step_count();
        let news = Some(model.means().clone());
        let engine = if threaded {
            Engine::Worker(spawn(model))
        } else {
            Engine::Inline(Box::new(model))
        };
        Simulation {
            running: false,
            speed: Speed::default(),
            face_size,
            step,
            ms_per_step: 0.0,
            news,
            engine,
        }
    }

    pub fn face_size(&self) -> usize {
        self.face_size
    }

    pub fn step_count(&self) -> u64 {
        self.step
    }

    pub fn year(&self) -> u64 {
        self.step / u64::from(STEPS_PER_YEAR)
    }

    pub fn month(&self) -> usize {
        let in_year = self.step % u64::from(STEPS_PER_YEAR);
        in_year as usize / STEPS_PER_MONTH as usize
    }

    pub fn ms_per_step(&self) -> f64 {
        self.ms_per_step
    }

    /// Advances the run by the steps of one frame while it runs. A worker
    /// gets the next batch when it ends the last batch.
    pub fn frame(&mut self) {
        let steps = self.speed.steps();
        let running = self.running;
        let report = match &mut self.engine {
            Engine::Inline(model) => running.then(|| advance(model, steps)),
            Engine::Worker(worker) => {
                let report = match worker.replies.try_recv() {
                    Ok(Reply::Advanced(report)) => {
                        worker.busy = false;
                        Some(report)
                    }
                    Ok(Reply::Snapshot(_)) | Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        worker.busy = false;
                        None
                    }
                };
                if running && !worker.busy && send(worker, Job::Advance(steps)) {
                    worker.busy = true;
                }
                report
            }
        };
        if let Some(report) = report {
            self.take_report(report);
        }
    }

    fn take_report(&mut self, report: Report) {
        self.step = report.step;
        self.ms_per_step = report.ms_per_step;
        if report.means.is_some() {
            self.news = report.means;
        }
    }

    /// Runs `steps` steps and waits for them.
    pub fn run_steps(&mut self, steps: u64) {
        let mut left = steps;
        while left > 0 {
            let batch = left.min(u64::from(STEPS_PER_YEAR)) as u32;
            let report = match &mut self.engine {
                Engine::Inline(model) => Some(advance(model, batch)),
                Engine::Worker(worker) => {
                    send(worker, Job::Advance(batch));
                    loop {
                        match worker.replies.recv() {
                            Ok(Reply::Advanced(report)) if !worker.busy => break Some(report),
                            Ok(Reply::Advanced(_)) => worker.busy = false,
                            Ok(Reply::Snapshot(_)) => {}
                            Err(_) => break None,
                        }
                    }
                }
            };
            match report {
                Some(report) => self.take_report(report),
                None => return,
            }
            left -= u64::from(batch);
        }
    }

    /// The same run on a worker thread.
    pub fn threaded(self) -> Simulation {
        match self.engine {
            Engine::Inline(model) => Simulation {
                engine: Engine::Worker(spawn(*model)),
                ..self
            },
            Engine::Worker(_) => self,
        }
    }

    /// True while a worker does a batch.
    pub fn busy(&self) -> bool {
        matches!(&self.engine, Engine::Worker(w) if w.busy)
    }

    /// The means of a month that ended since the last call.
    pub fn take_means(&mut self) -> Option<Means> {
        self.news.take()
    }

    /// The state of the run. Waits for the batch of a worker to end.
    pub fn snapshot(&mut self) -> Result<RunState, String> {
        let lost = || "The model stopped.".to_owned();
        let worker = match &mut self.engine {
            Engine::Inline(model) => return Ok(model.snapshot()),
            Engine::Worker(worker) => worker,
        };
        if !send(worker, Job::Snapshot) {
            return Err(lost());
        }
        let mut reports = Vec::new();
        let state = loop {
            match worker.replies.recv().map_err(|_| lost())? {
                Reply::Advanced(report) => {
                    worker.busy = false;
                    reports.push(report);
                }
                Reply::Snapshot(state) => break *state,
            }
        };
        for report in reports {
            self.take_report(report);
        }
        Ok(state)
    }
}

/// The area-weighted global mean of the yearly mean temperature, and the
/// mean within 10° of the equator minus the mean poleward of 70°, in °C.
/// `None` before the first year ends.
pub fn summary(means: &Means) -> Option<(f64, f64)> {
    let field = means.field(Output::Temperature);
    let year = field.get(Period::Year)?;
    let grid = Grid::new(field.face_size);
    let (mut all, mut eq, mut pole) = ([0.0; 2], [0.0; 2], [0.0; 2]);
    for (i, &t) in year.iter().enumerate() {
        let (lat, _) = grid.lat_lon(i);
        let (a, t) = (grid.area(i), f64::from(t));
        let lat = lat.to_degrees().abs();
        for (sum, take) in [
            (&mut all, true),
            (&mut eq, lat < 10.0),
            (&mut pole, lat > 70.0),
        ] {
            if take {
                sum[0] += a * t;
                sum[1] += a;
            }
        }
    }
    let mean = |s: [f64; 2]| s[0] / s[1];
    Some((mean(all), mean(eq) - mean(pole)))
}

/// The run of the open world, and the world file that a save writes. The
/// app and the save of the iOS life cycle share it.
#[derive(Default)]
pub struct Run {
    pub sim: Option<Simulation>,
    pub path: Option<PathBuf>,
}

impl Run {
    /// Writes the state of the run to the world file. A run of a world with
    /// no file is not saved.
    pub fn save(&mut self) -> Result<(), String> {
        let (Some(sim), Some(path)) = (self.sim.as_mut(), &self.path) else {
            return Ok(());
        };
        let state = sim.snapshot()?;
        isotherm_store::RunFile::open(path)
            .and_then(|mut file| file.save_run(&state))
            .map_err(|e| format!("Could not save the run: {e}."))
    }
}

fn send(worker: &Worker, job: Job) -> bool {
    worker
        .jobs
        .as_ref()
        .is_some_and(|jobs| jobs.send(job).is_ok())
}

fn spawn(mut model: Model) -> Worker {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let (outbox, replies) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("isotherm-model".to_owned())
        .spawn(move || {
            for job in inbox {
                let reply = match job {
                    Job::Advance(steps) => Reply::Advanced(advance(&mut model, steps)),
                    Job::Snapshot => Reply::Snapshot(Box::new(model.snapshot())),
                };
                if outbox.send(reply).is_err() {
                    break;
                }
            }
        })
        .ok();
    Worker {
        jobs: Some(jobs),
        replies,
        busy: false,
        thread,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isotherm_world::Planet;

    fn world() -> World {
        World::flat("Sim", Planet::earth(), 8)
    }

    #[test]
    fn a_worker_and_an_inline_run_give_the_same_state() {
        let world = world();
        let mut worker = Simulation::new(&world, 4, true);
        worker.running = true;
        while worker.step_count() < 40 {
            worker.frame();
            std::thread::yield_now();
        }
        worker.running = false;
        let b = worker.snapshot().unwrap();
        let mut inline = Simulation::new(&world, 4, false);
        inline.running = true;
        while inline.step_count() < b.step {
            inline.frame();
        }
        assert_eq!(inline.snapshot().unwrap(), b);
    }

    #[test]
    fn a_paused_run_does_not_advance() {
        let mut sim = Simulation::new(&world(), 4, false);
        sim.frame();
        assert_eq!(sim.step_count(), 0);
        sim.running = true;
        sim.speed = Speed::Fast;
        sim.frame();
        assert_eq!(sim.step_count(), 30);
        assert_eq!(sim.month(), 1);
    }
}
