//! The extent of a map on the GPU: the table of its projection and the data
//! of its frames.

use std::sync::Arc;

use vale_sphere::{Projection, ProjectionSpec};

/// The size of the table of projected points, in texels.
pub const TABLE_WIDTH: u32 = 1024;
pub const TABLE_HEIGHT: u32 = 512;

/// The table of projected points of a projection on the equirectangular
/// grid of `TABLE_WIDTH` by `TABLE_HEIGHT` texels: for each texel center the
/// projected point at radius 1, and 1 if the point is on the map.
pub struct Table {
    pub spec: ProjectionSpec,
    pub texels: Vec<[f32; 4]>,
}

impl Table {
    pub fn new(spec: ProjectionSpec) -> Option<Table> {
        let (w, h) = (TABLE_WIDTH as usize, TABLE_HEIGHT as usize);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let rows = h.div_ceil(threads);
        let mut texels = vec![[0.0; 4]; w * h];
        let ok = std::thread::scope(|scope| {
            let jobs: Vec<_> = texels
                .chunks_mut(rows * w)
                .enumerate()
                .map(|(job, chunk)| {
                    scope.spawn(move || {
                        let projection = Projection::new(spec, 1.0).ok()?;
                        for (i, texel) in chunk.iter_mut().enumerate() {
                            let (x, y) = (i % w, job * rows + i / w);
                            let lon = -180.0 + (x as f64 + 0.5) * 360.0 / w as f64;
                            let lat = 90.0 - (y as f64 + 0.5) * 180.0 / h as f64;
                            if let Some(p) = projection.forward([lon, lat]) {
                                *texel = [p.x as f32, p.y as f32, 1.0, 0.0];
                            }
                        }
                        Some(())
                    })
                })
                .collect();
            jobs.into_iter()
                .all(|job| job.join().ok().flatten().is_some())
        });
        ok.then_some(Table { spec, texels })
    }
}

/// The extent on the GPU: the table of its projection and the data of its
/// frames.
#[derive(Clone)]
pub struct GpuExtent {
    pub table: Arc<Table>,
    pub frames: Arc<Vec<[f32; 4]>>,
}
