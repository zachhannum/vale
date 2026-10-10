//! The rivers of the globe. A worker thread computes the channel map, so the
//! brush does not wait for it.

use std::sync::{Arc, mpsc};

use vale_terrain::{ChannelMap, CoarseHeights, Heightmap, TexelRect, channel_map};

/// The coarse heights of one request, with the number of the request.
type Job = (u64, CoarseHeights);
type Done = (u64, ChannelMap);

pub struct Rivers {
    /// The coarse copy of the heightmap that the flow comes from.
    coarse: CoarseHeights,
    /// A request computes the channel map on the thread that asks, so the
    /// result is ready at the next poll.
    pub sync: bool,
    /// The path to the worker. `None`: the worker did not start yet.
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Sender<Done>,
    results: mpsc::Receiver<Done>,
    /// The number of the newest request, and of the newest result.
    sent: u64,
    received: u64,
}

/// Computes the channel map of the newest job until the sender is gone.
fn work(jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    while let Ok(mut job) = jobs.recv() {
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        if done.send((job.0, channel_map(&job.1))).is_err() {
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
            jobs: None,
            done,
            results,
            sent: 0,
            received: 0,
        }
    }

    /// Reads the whole heightmap again.
    pub fn rebuild(&mut self, map: &Heightmap) {
        self.coarse = CoarseHeights::new(map);
    }

    /// Reads the texels of one rectangle again.
    pub fn update(&mut self, map: &Heightmap, face: usize, rect: TexelRect) {
        self.coarse.update(map, face, rect);
    }

    /// Asks for the channel map of the heights as they are now.
    pub fn request(&mut self) {
        self.sent += 1;
        let job = (self.sent, self.coarse.clone());
        if self.sync {
            let _ = self.done.send((job.0, channel_map(&job.1)));
            return;
        }
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

    /// The newest channel map that arrived after the last poll. It can be
    /// older than the newest request.
    pub fn poll(&mut self) -> Option<Arc<ChannelMap>> {
        let (number, map) = self.results.try_iter().last()?;
        self.received = number;
        Some(Arc::new(map))
    }

    /// True until the channel map of the newest request arrives.
    pub fn pending(&self) -> bool {
        self.received < self.sent
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::meters_to_level;

    use super::*;

    /// Polls until the newest result arrives. Returns that result.
    fn wait(rivers: &mut Rivers) -> Arc<ChannelMap> {
        let mut newest = None;
        for _ in 0..4000 {
            newest = rivers.poll().or(newest);
            if !rivers.pending() {
                return newest.expect("a result arrived");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the worker gave no result");
    }

    #[test]
    fn the_worker_gives_the_newest_result() {
        const N: usize = 64;
        let mut map = Heightmap::new(N, meters_to_level(-100.0));
        let mut rivers = Rivers::new(&map, false);
        assert!(!rivers.pending());
        assert!(rivers.poll().is_none());
        // The first request has no land. The second request has a hill.
        rivers.request();
        for y in 0..N {
            for x in 0..N {
                let from_edge = x.min(y).min(N - 1 - x).min(N - 1 - y);
                map.set(0, x, y, meters_to_level(from_edge as f64 * 100.0 - 400.0));
            }
        }
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: N,
            y1: N,
        };
        rivers.update(&map, 0, rect);
        rivers.request();
        assert!(rivers.pending());
        assert!(wait(&mut rivers).has_rivers());
        assert!(rivers.poll().is_none());

        // The whole heightmap is sea again.
        rivers.rebuild(&Heightmap::new(N, meters_to_level(-100.0)));
        rivers.request();
        assert!(!wait(&mut rivers).has_rivers());
    }

    #[test]
    fn a_sync_request_is_ready_at_the_next_poll() {
        let map = Heightmap::new(64, meters_to_level(-100.0));
        let mut rivers = Rivers::new(&map, true);
        rivers.request();
        assert!(rivers.poll().is_some());
        assert!(!rivers.pending());
    }
}
