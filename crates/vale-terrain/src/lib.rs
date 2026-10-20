//! The heightmap of a Vale world: a 16-bit cube map in tiles, and the brush
//! that paints it. The heightmap owns the color stops of the elevation
//! preview.
//!
//! With no features, the crate has no dependencies. It works in sphere
//! directions and texels, and it knows nothing about the screen. The `gpu`
//! feature adds a copy of the heightmap in wgpu textures, and the same brush
//! as a shader.

mod bands;
mod cube;
mod equirect;
mod erode;
mod flow;
#[cfg(feature = "gpu")]
mod gpu;
mod heightmap;
pub mod math;
mod reduce;
#[cfg(feature = "gpu")]
mod tiles;

pub use bands::{Band, Bands, MAX_BANDS, MIN_BAND, Rgb, SEA_LEVEL, Side, Stop};
pub use cube::{
    ELEV_MAX, ELEV_MIN, FACES, FaceSizeValues, face_dir, face_of, face_size_values,
    level_to_meters, meters_to_level, unwarp, warp,
};
pub use equirect::{Equirect, EquirectGrid};
pub use erode::{ErodeBrush, ErodeGrid, ErodeStep, Erosion, OuterFlow};
pub use flow::{
    CHANNEL_SCALE, ChannelMap, ChannelWindow, CoarseHeights, FLOW_SIZE, FlowMap, RIDGE_REACH,
    RIVER_MIN_AREA, RIVER_MIN_CELLS, RIVER_OCTAVES, WINDOW_CELLS, WINDOW_MARGIN, Window,
    WindowHeights, channel_map, window_channels,
};
#[cfg(feature = "gpu")]
pub use gpu::{GROUP_STAMPS, GpuHeightmap, Readback, STAMP_SLOTS};
pub use heightmap::{
    Heightmap, Limits, MAX_BRUSH_RADIUS, Mode, Stamp, StampPlan, TILE_SIZE, TexelRect,
    UNDO_MEMORY_LIMIT, talus_levels,
};
pub use reduce::{reduced_bytes, reduced_cap, reduced_face_size, reduction_factor};
#[cfg(feature = "gpu")]
pub use tiles::{GpuTiles, TILES_WGSL, tiles_layout};
