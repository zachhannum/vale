//! The face size of the copies of the raster layers that the globe shows.
//!
//! A copy has the texels of `Heightmap::read_reduced`, so its face is the
//! face of the layer divided by a power of two.

use crate::cube::FACES;
use crate::heightmap::TILE_SIZE;
use crate::smooth::pyramid_bytes;

/// The largest face sizes of a copy, from the first choice to the last.
const CAPS: [u32; 3] = [2048, 1024, 512];

/// The smallest power of two that divides a face of `face_size` to `cap`
/// texels or less along one side.
pub fn reduction_factor(face_size: u32, cap: u32) -> u32 {
    assert!(cap > 0, "the cap must not be zero");
    let mut factor = 1;
    while face_size.div_ceil(factor) > cap {
        factor *= 2;
    }
    factor
}

/// The face size of the copy of a layer: the largest size that is `cap` or
/// less and that divides `face_size` by a power of two. A face that is not
/// larger than `cap` keeps its size.
pub fn reduced_face_size(face_size: u32, cap: u32) -> u32 {
    face_size.div_ceil(reduction_factor(face_size, cap))
}

/// An estimate of the memory of the copy of a layer on the GPU, in bytes:
/// its tiles and its smooth levels. The layer has `face_size` texels along
/// one side of a face, and `tiles` of its tiles of `TILE_SIZE` hold memory.
///
/// The estimate is for tiles that are next to each other. Then
/// `factor * factor` tiles of the layer go to one tile of the copy. The
/// smooth levels take memory for each texel.
pub fn reduced_bytes(face_size: u32, tiles: usize, cap: u32) -> u64 {
    let factor = u64::from(reduction_factor(face_size, cap));
    let per_side = u64::from(reduced_face_size(face_size, cap)).div_ceil(TILE_SIZE as u64);
    let all = FACES as u64 * per_side * per_side;
    let tiles = (tiles as u64).div_ceil(factor * factor).min(all);
    let levels = pyramid_bytes(reduced_face_size(face_size, cap));
    tiles * (TILE_SIZE * TILE_SIZE * size_of::<u16>()) as u64 + levels
}

/// The largest face size of the copies of the layers that the globe shows:
/// 2048, 1024, or 512. It is the first of them at which the sum of
/// `reduced_bytes` is `budget_bytes` or less, and 512 if none is.
///
/// Each item of `layers` is the face size of one layer and the number of its
/// tiles that hold memory.
pub fn reduced_cap(layers: &[(u32, usize)], budget_bytes: u64) -> u32 {
    let fits = |cap: &u32| {
        let bytes = layers
            .iter()
            .map(|&(face, tiles)| reduced_bytes(face, tiles, *cap));
        bytes.sum::<u64>() <= budget_bytes
    };
    CAPS.into_iter().find(fits).unwrap_or(CAPS[CAPS.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE_BYTES: u64 = 256 * 256 * 2;

    #[test]
    fn a_face_smaller_than_the_cap_keeps_its_size() {
        assert_eq!(reduced_face_size(1024, 2048), 1024);
        assert_eq!(reduced_face_size(2048, 2048), 2048);
        assert_eq!(reduction_factor(2048, 2048), 1);
        assert_eq!(reduced_face_size(300, 512), 300);
    }

    #[test]
    fn a_large_face_goes_down_by_a_power_of_two() {
        assert_eq!(reduced_face_size(8192, 2048), 2048);
        assert_eq!(reduced_face_size(4096, 2048), 2048);
        assert_eq!(reduced_face_size(8192, 512), 512);
        assert_eq!(reduction_factor(8192, 512), 16);
        // 6000 / 4 is 1500, and 6000 / 2 is more than the cap.
        assert_eq!(reduced_face_size(6000, 2048), 1500);
        assert_eq!(reduced_face_size(4097, 2048), 1025);
    }

    /// The smooth levels of a face of 2048, 1024, and 512 texels.
    const LEVELS_2048: u64 = 16_777_152;
    const LEVELS_1024: u64 = 4_194_240;
    const LEVELS_512: u64 = 1_048_512;

    #[test]
    fn the_bytes_of_a_copy_come_from_its_tiles_and_its_levels() {
        // The levels 1 to 9 of a face of 2048 have 1024 to 4 texels along
        // one side.
        let texels: u64 = (2..=10).map(|j| 4u64.pow(j)).sum();
        assert_eq!(pyramid_bytes(2048), 6 * 2 * texels);
        assert_eq!(pyramid_bytes(2048), LEVELS_2048);
        assert_eq!(pyramid_bytes(1024), LEVELS_1024);
        assert_eq!(pyramid_bytes(512), LEVELS_512);
        // A layer of 8192 with memory in each tile has 6 * 32 * 32 tiles.
        assert_eq!(
            reduced_bytes(8192, 6144, 2048),
            384 * TILE_BYTES + LEVELS_2048
        );
        assert_eq!(
            reduced_bytes(8192, 6144, 1024),
            96 * TILE_BYTES + LEVELS_1024
        );
        assert_eq!(reduced_bytes(8192, 6144, 512), 24 * TILE_BYTES + LEVELS_512);
        assert_eq!(reduced_bytes(8192, 0, 2048), LEVELS_2048);
        assert_eq!(reduced_bytes(8192, 17, 2048), 2 * TILE_BYTES + LEVELS_2048);
        // A face of 1024 keeps its tiles at a cap of 2048.
        assert_eq!(reduced_bytes(1024, 40, 2048), 40 * TILE_BYTES + LEVELS_1024);
    }

    #[test]
    fn the_cap_goes_down_as_the_budget_fills() {
        let layer = (8192, 6144);
        let one = 384 * TILE_BYTES + LEVELS_2048;
        assert_eq!(reduced_cap(&[], 0), 2048);
        assert_eq!(reduced_cap(&[layer], one), 2048);
        assert_eq!(reduced_cap(&[layer; 2], 2 * one), 2048);
        assert_eq!(reduced_cap(&[layer; 3], 2 * one), 1024);
        assert_eq!(reduced_cap(&[layer; 8], 2 * one), 1024);
        assert_eq!(reduced_cap(&[layer; 9], 2 * one), 512);
        // Nothing fits, and no layer is left out.
        assert_eq!(reduced_cap(&[layer; 100], one), 512);
        // Layers with little paint leave the budget free.
        assert_eq!(reduced_cap(&[(8192, 160); 3], one), 2048);
        assert_eq!(reduced_cap(&[(8192, 160); 4], one), 1024);
    }
}
