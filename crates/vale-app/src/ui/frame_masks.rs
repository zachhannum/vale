//! The raster masks of the map frames on the screen.

use std::collections::HashMap;
use std::sync::mpsc;

use vale_store::{FrameId, LayerId};

use crate::atlas::FrameTarget;
use crate::globe::gpu::Captured;
use crate::mask::frame::cover_mask;
use crate::pipeline::Pipeline;

/// The raster mask of each masked layer of each map frame on the screen,
/// from the GPU. A mask stays until its new capture arrives.
#[derive(Default)]
pub struct FrameMasks {
    entries: HashMap<(FrameId, LayerId), Entry>,
    /// The number of the last mask that arrived.
    count: u64,
}

#[derive(Default)]
struct Entry {
    /// The key of the mask that the pipeline has. 0: none.
    key: u64,
    pending: Option<Pending>,
    /// The number of the mask that the pipeline has.
    number: u64,
}

struct Pending {
    key: u64,
    receiver: mpsc::Receiver<Result<Captured, String>>,
    /// Page millimeters to the pixels of the capture.
    at: FrameTarget,
}

impl FrameMasks {
    /// Sets the masks whose captures arrived in `pipeline`. Returns `true`
    /// while a capture waits.
    pub fn poll(&mut self, pipeline: &mut Pipeline) -> bool {
        let mut waits = false;
        for (&(frame, layer), entry) in &mut self.entries {
            let Some(pending) = &entry.pending else {
                continue;
            };
            let captured = match pending.receiver.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    waits = true;
                    continue;
                }
                Ok(Ok(captured)) => Some(captured),
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => None,
            };
            let pending = entry.pending.take().expect("a capture waits");
            let Some(captured) = captured else {
                continue;
            };
            let [w, h] = captured.size.map(|s| s as usize);
            if captured.rgba.len() != w * h {
                continue;
            }
            let mask = cover_mask(&captured.rgba, w, h, pending.at);
            pipeline.set_page_mask(frame, layer, Some(mask));
            self.count += 1;
            entry.key = pending.key;
            entry.number = self.count;
        }
        waits
    }

    /// A number that changes when a mask of frame `frame` changes.
    pub fn number(&self, frame: FrameId) -> u64 {
        (self.entries.iter())
            .filter(|((f, _), _)| *f == frame)
            .map(|(_, e)| e.number)
            .max()
            .unwrap_or(0)
    }

    /// True if the mask of layer `layer` in frame `frame` needs a capture
    /// for `key`. A mask waits for its capture before it gets another.
    pub fn wants(&self, frame: FrameId, layer: LayerId, key: u64) -> bool {
        self.entries
            .get(&(frame, layer))
            .is_none_or(|e| e.pending.is_none() && e.key != key)
    }

    /// Waits for the capture of the mask of layer `layer` in frame `frame`
    /// for `key`. `at` maps page millimeters to the pixels of the capture.
    pub fn start(
        &mut self,
        frame: FrameId,
        layer: LayerId,
        key: u64,
        receiver: mpsc::Receiver<Result<Captured, String>>,
        at: FrameTarget,
    ) {
        let entry = self.entries.entry((frame, layer)).or_default();
        entry.pending = Some(Pending { key, receiver, at });
    }

    /// Keeps only the masks that `keep` gives, and removes the others from
    /// `pipeline`.
    pub fn retain(&mut self, pipeline: &mut Pipeline, keep: impl Fn(FrameId, LayerId) -> bool) {
        self.entries.retain(|&(frame, layer), _| {
            let kept = keep(frame, layer);
            if !kept {
                pipeline.set_page_mask(frame, layer, None);
            }
            kept
        });
    }
}
