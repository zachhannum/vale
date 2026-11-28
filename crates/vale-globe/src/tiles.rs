//! The sizes of the tiles of the vector layers and of their atlas.

/// The texels along one side of a tile, with no gutter.
pub const TILE: usize = 256;
/// The texels along one side of the picture of a tile: the tile and a
/// gutter of one texel on each side, so that a filter has no seam.
pub const TILE_PX: usize = TILE + 2;

/// The slots of the atlas along x and along y. 360 slots of 258 squared
/// RGBA texels take 96 MB.
pub const SLOT_COLS: u32 = 20;
pub const SLOT_ROWS: u32 = 18;
pub const SLOTS: u32 = SLOT_COLS * SLOT_ROWS;
pub const ATLAS: [u32; 2] = [SLOT_COLS * TILE_PX as u32, SLOT_ROWS * TILE_PX as u32];
/// The size of the page table of a layer: an entry for each tile of each
/// level.
pub const PAGE_SIZE: [u32; 2] = [1024, 512];
