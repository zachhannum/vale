//! The heightmap of a Vale world: a 16-bit cube map in tiles, and the brush
//! that paints it. The heightmap owns the band limits and the color ramp of
//! the elevation preview.
//!
//! With no features, the crate has no dependencies. It works in sphere
//! directions and texels, and it knows nothing about the screen. The `gpu`
//! feature adds a copy of the heightmap in a wgpu texture, and the same brush
//! as a shader.

mod bands;
mod cube;
mod flow;
#[cfg(feature = "gpu")]
mod gpu;
mod heightmap;
pub mod math;

pub use bands::{Band, Bands, MAX_BANDS, MIN_BAND, Ramp, Rgb, SEA_LEVEL};
pub use cube::{
    ELEV_MAX, ELEV_MIN, FACES, face_dir, face_of, level_to_meters, meters_to_level, unwarp, warp,
};
pub use flow::{
    CHANNEL_SCALE, ChannelMap, CoarseHeights, FLOW_SIZE, FlowMap, RIVER_MIN_AREA, RIVER_OCTAVES,
    channel_map,
};
#[cfg(feature = "gpu")]
pub use gpu::{GROUP_STAMPS, GpuHeightmap, Readback, STAMP_SLOTS};
pub use heightmap::{Heightmap, MAX_BRUSH_RADIUS, Mode, Stamp, StampPlan, TILE_SIZE, TexelRect};
