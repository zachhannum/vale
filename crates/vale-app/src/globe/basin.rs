//! The basin brush of the globe. A worker thread makes the basin of a
//! stroke, so the brush does not wait for it.

use std::sync::mpsc;

use vale_terrain::{BasinCut, BasinLine, Heightmap, basin};

pub struct Basiner {
    /// A request computes its result on the thread that asks, so the result
    /// is ready at the next poll.
    sync: bool,
    done: mpsc::Sender<(u64, BasinCut)>,
    results: mpsc::Receiver<(u64, BasinCut)>,
    /// The number of the last request.
    request: u64,
    /// The last request did not arrive yet.
    waiting: bool,
}

impl Basiner {
    pub fn new(sync: bool) -> Basiner {
        let (done, results) = mpsc::channel();
        Basiner {
            sync,
            done,
            results,
            request: 0,
            waiting: false,
        }
    }

    pub fn set_sync(&mut self, sync: bool) {
        self.sync = sync;
    }

    /// Asks for the basin of `line` on `map`. The result of a request before
    /// this one does not arrive.
    pub fn request(&mut self, map: Heightmap, line: BasinLine) {
        self.request += 1;
        self.waiting = true;
        let (id, done) = (self.request, self.done.clone());
        let job = move || {
            let _ = done.send((id, basin(&map, &line)));
        };
        if self.sync {
            job();
        } else {
            std::thread::Builder::new()
                .name("vale-basin".to_string())
                .spawn(job)
                .expect("the system can start a thread");
        }
    }

    /// The basin of the last request, if it arrived after the last poll.
    pub fn poll(&mut self) -> Option<BasinCut> {
        while let Ok((id, cut)) = self.results.try_recv() {
            if id == self.request && self.waiting {
                self.waiting = false;
                return Some(cut);
            }
        }
        None
    }

    /// True until the basin of the last request arrives.
    pub fn pending(&self) -> bool {
        self.waiting
    }

    /// Stops the last request. Its result does not arrive.
    pub fn cancel(&mut self) {
        self.request += 1;
        self.waiting = false;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::{BasinSpec, ChannelShape, Limits, Rain, meters_to_level};

    use super::*;
    use crate::globe::math::lonlat_to_dir;

    const N: usize = 128;

    fn line() -> BasinLine {
        let spec = BasinSpec {
            height: f64::from(meters_to_level(1500.0) - meters_to_level(0.0)),
            density: 0.02,
            seed: 7,
            rain: Rain::default(),
            channel: ChannelShape::default(),
            world_km: 6371.0,
            limits: Limits::default(),
        };
        let outline = [(-25.0, -9.0), (25.0, -9.0), (25.0, 9.0), (-25.0, 9.0)];
        let outline = outline.map(|(lon, lat)| lonlat_to_dir(lon, lat));
        let mut line = BasinLine::new(spec, &outline, N);
        for k in 0..=20 {
            line.push(lonlat_to_dir(-20.0 + 2.0 * f64::from(k), 0.0));
        }
        line
    }

    fn ground() -> Heightmap {
        Heightmap::new(N, meters_to_level(300.0))
    }

    fn wait(basiner: &mut Basiner) -> BasinCut {
        for _ in 0..2000 {
            if let Some(cut) = basiner.poll() {
                return cut;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the basin did not arrive");
    }

    #[test]
    fn a_sync_request_is_ready_at_the_next_poll() {
        let mut basiner = Basiner::new(true);
        assert!(!basiner.pending());
        assert!(basiner.poll().is_none());
        basiner.request(ground(), line());
        assert!(basiner.pending());
        let cut = basiner.poll().unwrap();
        assert!(!cut.rects.is_empty());
        assert!(!basiner.pending());
        assert!(basiner.poll().is_none());
    }

    #[test]
    fn the_worker_gives_the_basin_of_the_cpu() {
        let mut basiner = Basiner::new(false);
        basiner.request(ground(), line());
        let cut = wait(&mut basiner);
        assert!(!basiner.pending());
        let cpu = basin(&ground(), &line());
        assert!(cut.rects == cpu.rects);
    }

    #[test]
    fn the_result_of_an_old_request_does_not_arrive() {
        for sync in [true, false] {
            let mut basiner = Basiner::new(sync);
            basiner.request(ground(), line());
            basiner.cancel();
            assert!(!basiner.pending());
            std::thread::sleep(Duration::from_millis(50));
            assert!(basiner.poll().is_none());

            // A new request gives its own result, not the one before it.
            basiner.request(ground(), line());
            let short = BasinLine::new(line().spec(), line().outline(), N);
            basiner.request(ground(), short);
            assert!(wait(&mut basiner).rects.is_empty());
            assert!(!basiner.pending());
        }
    }
}
