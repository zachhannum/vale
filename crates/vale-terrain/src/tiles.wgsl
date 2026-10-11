// The heightmap as a pool of tiles. `tiles.rs` fills the bindings.

struct Tiles {
    // The level of each texel in a tile that has no memory.
    base: u32,
    // The number of texels along one side of a tile.
    tile: u32,
}

@group(1) @binding(0) var<uniform> tiles: Tiles;
// One texel for each tile, and one layer for each face. 0 is a tile that has
// no memory. For the other tiles, the number minus 1 holds the place of the
// tile in the pool. From the low byte to the high byte, the place is the
// column and the row in a layer, the layer, and the chunk.
@group(1) @binding(1) var tile_table: texture_2d_array<u32>;
// The chunks of the pool. A layer of a chunk holds rows of tiles.
@group(1) @binding(2) var tile_chunk_0: texture_2d_array<u32>;
@group(1) @binding(3) var tile_chunk_1: texture_2d_array<u32>;
@group(1) @binding(4) var tile_chunk_2: texture_2d_array<u32>;
@group(1) @binding(5) var tile_chunk_3: texture_2d_array<u32>;
@group(1) @binding(6) var tile_chunk_4: texture_2d_array<u32>;
@group(1) @binding(7) var tile_chunk_5: texture_2d_array<u32>;
@group(1) @binding(8) var tile_chunk_6: texture_2d_array<u32>;
@group(1) @binding(9) var tile_chunk_7: texture_2d_array<u32>;

// The level of a texel of a face. The texel is on the face.
fn tile_level(face: i32, at: vec2<i32>) -> u32 {
    let side = i32(tiles.tile);
    let entry = textureLoad(tile_table, at / side, face, 0).r;
    if entry == 0u {
        return tiles.base;
    }
    let place = entry - 1u;
    let column = i32(place & 0xffu);
    let row = i32((place >> 8u) & 0xffu);
    let layer = i32((place >> 16u) & 0xffu);
    let p = vec2<i32>(column, row) * side + at % side;
    switch place >> 24u {
        case 0u {
            return textureLoad(tile_chunk_0, p, layer, 0).r;
        }
        case 1u {
            return textureLoad(tile_chunk_1, p, layer, 0).r;
        }
        case 2u {
            return textureLoad(tile_chunk_2, p, layer, 0).r;
        }
        case 3u {
            return textureLoad(tile_chunk_3, p, layer, 0).r;
        }
        case 4u {
            return textureLoad(tile_chunk_4, p, layer, 0).r;
        }
        case 5u {
            return textureLoad(tile_chunk_5, p, layer, 0).r;
        }
        case 6u {
            return textureLoad(tile_chunk_6, p, layer, 0).r;
        }
        default {
            return textureLoad(tile_chunk_7, p, layer, 0).r;
        }
    }
}
