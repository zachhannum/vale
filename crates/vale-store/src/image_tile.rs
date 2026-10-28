//! The pyramid of an image layer and the PNG blob of one of its tiles.
//!
//! Level 0 has the size of the image. Each level after it has half the size
//! of the level below it, rounded up, until a level is at most one tile wide.
//! A tile has `IMAGE_TILE_SIZE` by `IMAGE_TILE_SIZE` pixels, and a tile at
//! the right or the bottom edge of a level has only the pixels inside the
//! level.

use std::io::Cursor;

use crate::gpkg::{StoreError, corrupt};

/// The number of pixels along the edge of an image tile.
pub const IMAGE_TILE_SIZE: u32 = 256;

/// The number of levels of the pyramid of an image.
pub fn image_levels(width: u32, height: u32) -> u32 {
    let mut levels = 1;
    while level_size(width, height, levels - 1).0 > IMAGE_TILE_SIZE {
        levels += 1;
    }
    levels
}

/// The width and the height of one level of the pyramid of an image.
pub fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    let shrink = |n: u32| match level {
        0..32 => n.div_ceil(1 << level),
        _ => 1,
    };
    (shrink(width), shrink(height))
}

/// A PNG of `width` by `height` pixels of straight RGBA8, row by row.
pub fn encode_tile(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(
        rgba.len() as u64,
        u64::from(width) * u64::from(height) * 4,
        "4 bytes for each pixel"
    );
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    // A write to a vector does not fail.
    let mut writer = encoder.write_header().expect("a PNG header");
    writer.write_image_data(rgba).expect("the PNG pixels");
    writer.finish().expect("the end of the PNG");
    out
}

/// The width, the height, and the straight RGBA8 pixels of a tile PNG.
pub fn decode_tile(blob: &[u8]) -> Result<(u32, u32, Vec<u8>), StoreError> {
    let bad = || corrupt("the pixels of an image tile");
    let decoder = png::Decoder::new(Cursor::new(blob));
    let mut reader = decoder.read_info().map_err(|_| bad())?;
    let size = reader.output_buffer_size().ok_or_else(bad)?;
    let mut rgba = vec![0; size];
    let info = reader.next_frame(&mut rgba).map_err(|_| bad())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(bad());
    }
    rgba.truncate(info.buffer_size());
    Ok((info.width, info.height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_keeps_its_pixels_and_its_alpha() {
        let (width, height) = (5, 3);
        let rgba: Vec<u8> = (0..width * height * 4).map(|i| (i * 17) as u8).collect();
        let blob = encode_tile(width, height, &rgba);
        assert_eq!(&blob[1..4], b"PNG");
        assert_eq!(decode_tile(&blob).unwrap(), (width, height, rgba));
        assert!(decode_tile(&blob[..blob.len() / 2]).is_err());
    }

    #[test]
    fn the_pyramid_halves_each_level_until_one_tile_is_wide() {
        assert_eq!(image_levels(256, 128), 1);
        assert_eq!(image_levels(257, 128), 2);
        assert_eq!(image_levels(16_384, 8_192), 7);
        assert_eq!(level_size(16_384, 8_192, 6), (256, 128));
        assert_eq!(level_size(1000, 501, 1), (500, 251));
        assert_eq!(level_size(1000, 501, 2), (250, 126));
        assert_eq!(image_levels(1000, 501), 3);
        assert_eq!(level_size(3, 1, 40), (1, 1));
    }
}
