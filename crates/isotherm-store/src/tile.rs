//! The texels of one heightmap tile as a compressed blob.
//!
//! The first byte of a blob is the codec. Codec 1 is a deflate stream of the
//! low bytes and then the high bytes of the differences. The difference of a
//! texel is from the texel to its left, and for the first column from the
//! texel above. Ground is smooth, so most differences are small.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const DELTA_DEFLATE: u8 = 1;

/// The blob of a tile of `side` by `side` texels, row by row.
pub fn encode(texels: &[u16], side: usize) -> Vec<u8> {
    let count = side * side;
    assert_eq!(texels.len(), count, "the tile size");
    let mut planes = vec![0u8; 2 * count];
    let (low, high) = planes.split_at_mut(count);
    for (i, &texel) in texels.iter().enumerate() {
        let [l, h] = texel.wrapping_sub(before(texels, i, side)).to_le_bytes();
        (low[i], high[i]) = (l, h);
    }
    let mut encoder = DeflateEncoder::new(vec![DELTA_DEFLATE], Compression::fast());
    encoder
        .write_all(&planes)
        .and_then(|()| encoder.finish())
        .expect("a write to memory")
}

/// The texels of a blob of a tile of `side` by `side` texels. `None`: the
/// blob is not such a tile.
pub fn decode(blob: &[u8], side: usize) -> Option<Vec<u16>> {
    let count = side * side;
    let (&codec, stream) = blob.split_first()?;
    if codec != DELTA_DEFLATE {
        return None;
    }
    let mut planes = Vec::with_capacity(2 * count);
    // One more byte than a tile has, so that a longer stream fails.
    DeflateDecoder::new(stream)
        .take(2 * count as u64 + 1)
        .read_to_end(&mut planes)
        .ok()?;
    if planes.len() != 2 * count {
        return None;
    }
    let (low, high) = planes.split_at(count);
    let mut texels = vec![0u16; count];
    for i in 0..count {
        let delta = u16::from_le_bytes([low[i], high[i]]);
        texels[i] = delta.wrapping_add(before(&texels, i, side));
    }
    Some(texels)
}

/// The texel that predicts texel `i`.
fn before(texels: &[u16], i: usize, side: usize) -> u16 {
    if !i.is_multiple_of(side) {
        texels[i - 1]
    } else if i > 0 {
        texels[i - side]
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: u32, count: usize) -> Vec<u16> {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 16) as u16
            })
            .collect()
    }

    #[test]
    fn a_tile_is_the_same_after_encode_and_decode() {
        for side in [1, 16, 256] {
            let count = side * side;
            let slope: Vec<u16> = (0..count)
                .map(|i| (30_000 + 7 * (i % side) + 3 * (i / side)) as u16)
                .collect();
            for texels in [
                vec![0; count],
                vec![u16::MAX; count],
                slope,
                noise(1, count),
            ] {
                assert_eq!(decode(&encode(&texels, side), side), Some(texels));
            }
        }
    }

    #[test]
    fn a_blob_that_is_not_a_tile_fails() {
        let blob = encode(&noise(2, 256), 16);
        assert_eq!(decode(&[], 16), None);
        assert_eq!(decode(&[2], 16), None);
        assert_eq!(decode(&blob[..blob.len() / 2], 16), None);
        assert_eq!(decode(&blob, 8), None);
        assert_eq!(decode(&blob, 32), None);
    }
}
