//! Image files with colors and alpha, for the tests of the image layers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use image::RgbaImage;

/// An equirectangular image whose west half is transparent and whose east
/// half is opaque red.
pub fn half_red(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, _| {
        if x < width / 2 {
            image::Rgba([0, 0, 255, 0])
        } else {
            image::Rgba([255, 0, 0, 255])
        }
    })
}

/// An opaque image of one color.
pub fn solid(width: u32, height: u32, rgb: [u8; 3]) -> RgbaImage {
    let [r, g, b] = rgb;
    RgbaImage::from_pixel(width, height, image::Rgba([r, g, b, 255]))
}

pub fn write_png(path: &Path, image: &RgbaImage) -> PathBuf {
    image.save(path).unwrap();
    path.to_path_buf()
}

pub fn write_tiff(path: &Path, image: &RgbaImage) -> PathBuf {
    use tiff::encoder::{TiffEncoder, colortype::RGBA8};
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = TiffEncoder::new(std::io::BufWriter::new(file)).unwrap();
    encoder
        .write_image::<RGBA8>(image.width(), image.height(), image.as_raw())
        .unwrap();
    path.to_path_buf()
}

/// Writes a flattened PSD file with raw RGB and alpha channels. Photoshop
/// mixes the composite with white where the alpha is less than 1, so the
/// file has that mix.
pub fn write_psd(path: &Path, image: &RgbaImage) -> PathBuf {
    let (w, h) = (image.width(), image.height());
    let mut f = Vec::new();
    f.extend_from_slice(b"8BPS");
    f.extend_from_slice(&1u16.to_be_bytes());
    f.extend_from_slice(&[0; 6]);
    f.extend_from_slice(&4u16.to_be_bytes());
    f.extend_from_slice(&h.to_be_bytes());
    f.extend_from_slice(&w.to_be_bytes());
    f.extend_from_slice(&8u16.to_be_bytes());
    f.extend_from_slice(&3u16.to_be_bytes());
    // The color data, the image resources, and the layers: none.
    for _ in 0..3 {
        f.extend_from_slice(&0u32.to_be_bytes());
    }
    // Raw data, one plane for each channel.
    f.extend_from_slice(&0u16.to_be_bytes());
    for channel in 0..4 {
        for pixel in image.pixels() {
            let a = u32::from(pixel[3]);
            let value = match channel {
                3 => pixel[3],
                c => ((u32::from(pixel[c]) * a + 255 * (255 - a) + 127) / 255) as u8,
            };
            f.push(value);
        }
    }
    std::fs::write(path, f).unwrap();
    path.to_path_buf()
}
