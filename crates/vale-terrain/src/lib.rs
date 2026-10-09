//! The heightmap of a Vale world: a 16-bit cube map in tiles, and the brush
//! that paints it.
//!
//! The crate has no dependencies. It works in sphere directions and texels,
//! and it knows nothing about the screen.

mod cube;
mod heightmap;
pub mod math;

pub use cube::{
    ELEV_MAX, ELEV_MIN, FACES, face_dir, face_of, level_to_meters, meters_to_level, unwarp, warp,
};
pub use heightmap::{Heightmap, MAX_BRUSH_RADIUS, Mode, Stamp, TILE_SIZE, TexelRect};
