//! The pixels of one raster tile as a compressed blob.
//!
//! The first byte of a blob is the codec. Codec 1 is a deflate stream of the
//! low bytes and then the high bytes of the differences. The difference of a
//! pixel is from the pixel to its left, and for the first column from the
//! pixel above. Ground is smooth, so most differences are small.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

/// The side of a tile, in pixels.
pub const TILE_SIZE: usize = 256;

const PIXELS: usize = TILE_SIZE * TILE_SIZE;
const DELTA_DEFLATE: u8 = 1;

/// The blob of a tile of `TILE_SIZE` by `TILE_SIZE` pixels, row by row.
pub fn encode(pixels: &[u16]) -> Vec<u8> {
    assert_eq!(pixels.len(), PIXELS, "the tile size");
    let mut planes = vec![0u8; 2 * PIXELS];
    let (low, high) = planes.split_at_mut(PIXELS);
    for (i, &pixel) in pixels.iter().enumerate() {
        let [l, h] = pixel.wrapping_sub(before(pixels, i)).to_le_bytes();
        (low[i], high[i]) = (l, h);
    }
    let mut encoder = DeflateEncoder::new(vec![DELTA_DEFLATE], Compression::fast());
    encoder
        .write_all(&planes)
        .and_then(|()| encoder.finish())
        .expect("a write to memory")
}

/// The pixels of a blob. `None`: the blob is not a tile.
pub fn decode(blob: &[u8]) -> Option<Vec<u16>> {
    let (&codec, stream) = blob.split_first()?;
    if codec != DELTA_DEFLATE {
        return None;
    }
    let mut planes = Vec::with_capacity(2 * PIXELS);
    // One more byte than a tile has, so that a longer stream fails.
    DeflateDecoder::new(stream)
        .take(2 * PIXELS as u64 + 1)
        .read_to_end(&mut planes)
        .ok()?;
    if planes.len() != 2 * PIXELS {
        return None;
    }
    let (low, high) = planes.split_at(PIXELS);
    let mut pixels = vec![0u16; PIXELS];
    for i in 0..PIXELS {
        let delta = u16::from_le_bytes([low[i], high[i]]);
        pixels[i] = delta.wrapping_add(before(&pixels, i));
    }
    Some(pixels)
}

/// The pixel that predicts pixel `i`.
fn before(pixels: &[u16], i: usize) -> u16 {
    if !i.is_multiple_of(TILE_SIZE) {
        pixels[i - 1]
    } else if i > 0 {
        pixels[i - TILE_SIZE]
    } else {
        0
    }
}

/// `f` of each item, in order, on the threads of the machine.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if threads < 2 || items.len() < 2 {
        return items.iter().map(f).collect();
    }
    let f = &f;
    std::thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(items.len().div_ceil(threads))
            .map(|chunk| scope.spawn(move || chunk.iter().map(f).collect::<Vec<R>>()))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a worker thread"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixels with no pattern.
    fn noise(seed: u32) -> Vec<u16> {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..PIXELS)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 16) as u16
            })
            .collect()
    }

    #[test]
    fn a_tile_is_the_same_after_encode_and_decode() {
        let slope: Vec<u16> = (0..PIXELS)
            .map(|i| (30_000 + 7 * (i % TILE_SIZE) + 3 * (i / TILE_SIZE)) as u16)
            .collect();
        let mut ends = vec![0u16; PIXELS];
        ends.iter_mut().step_by(2).for_each(|p| *p = u16::MAX);
        for pixels in [
            vec![0; PIXELS],
            vec![u16::MAX; PIXELS],
            ends,
            slope,
            noise(1),
        ] {
            assert_eq!(decode(&encode(&pixels)), Some(pixels));
        }
    }

    #[test]
    fn smooth_ground_is_small() {
        let slope: Vec<u16> = (0..PIXELS)
            .map(|i| (30_000 + 7 * (i % TILE_SIZE) + 3 * (i / TILE_SIZE)) as u16)
            .collect();
        assert!(encode(&slope).len() < PIXELS / 20);
        assert!(encode(&vec![19_114; PIXELS]).len() < 1024);
    }

    #[test]
    fn a_blob_that_is_not_a_tile_fails() {
        let blob = encode(&noise(2));
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[2]), None);
        assert_eq!(decode(&blob[..blob.len() / 2]), None);
        let mut other = blob.clone();
        other[0] = 0;
        assert_eq!(decode(&other), None);

        // A stream of a different length.
        for len in [2 * PIXELS - 1, 2 * PIXELS + 1] {
            let mut encoder = DeflateEncoder::new(vec![DELTA_DEFLATE], Compression::fast());
            encoder.write_all(&vec![0; len]).unwrap();
            assert_eq!(decode(&encoder.finish().unwrap()), None);
        }
    }

    #[test]
    fn parallel_map_keeps_the_order() {
        let items: Vec<u32> = (0..1000).collect();
        let doubled = parallel_map(&items, |n| n * 2);
        assert!(doubled.iter().enumerate().all(|(i, &n)| n == 2 * i as u32));
        assert_eq!(parallel_map(&[] as &[u32], |n| *n), [0u32; 0]);
    }
}
