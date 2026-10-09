//! The heightmap of a Vale world: a 16-bit cube map in tiles, and the brush
//! that paints it.
//!
//! With no features, the crate has no dependencies. It works in sphere
//! directions and texels, and it knows nothing about the screen. The `gpu`
//! feature adds a copy of the heightmap in a wgpu texture, and the same brush
//! as a shader.

mod cube;
#[cfg(feature = "gpu")]
mod gpu;
mod heightmap;
pub mod math;

pub use cube::{
    ELEV_MAX, ELEV_MIN, FACES, face_dir, face_of, level_to_meters, meters_to_level, unwarp, warp,
};
#[cfg(feature = "gpu")]
pub use gpu::{GpuHeightmap, Readback, STAMP_SLOTS};
pub use heightmap::{Heightmap, MAX_BRUSH_RADIUS, Mode, Stamp, StampPlan, TILE_SIZE, TexelRect};
