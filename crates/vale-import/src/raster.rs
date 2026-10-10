//! The greyscale raster reader.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

use crate::ImportError;

/// The decoder allocates at most this many bytes.
///
/// A TIFF image needs its decoded size two times, and a world heightmap in
/// 16-bit RGBA is 1 GiB.
const MAX_ALLOC: u64 = 4 * 1024 * 1024 * 1024;

/// A greyscale image with 16-bit levels, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Greyscale {
    pub width: usize,
    pub height: usize,
    pub levels: Vec<u16>,
}

/// Reads a PNG or TIFF image. The format comes from the content, not from the file name.
///
/// A 16-bit grey level stays as it is. An 8-bit level scales to the full range.
/// A color image becomes its luma, and alpha is ignored. A float sample maps
/// 0.0 to 0 and 1.0 to 65,535.
pub fn read_bytes(name: &str, bytes: &[u8]) -> Result<Greyscale, ImportError> {
    read(name, Cursor::new(bytes))
}

/// Reads a PNG or TIFF file. The name in an error is the file stem.
pub fn read_file(path: &Path) -> Result<Greyscale, ImportError> {
    let file = File::open(path).map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    read(&name, BufReader::new(file))
}

fn read<R: BufRead + Seek>(name: &str, reader: R) -> Result<Greyscale, ImportError> {
    let parse = |message: String| ImportError::Parse {
        name: name.to_string(),
        message,
    };
    let mut reader = ImageReader::new(reader)
        .with_guessed_format()
        .map_err(|e| parse(e.to_string()))?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Tiff)) {
        return Err(ImportError::NotRaster {
            name: name.to_string(),
        });
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| parse(e.to_string()))?;
    let (width, height) = (image.width() as usize, image.height() as usize);
    let levels = levels(image).ok_or_else(|| parse("the pixel format is not supported".into()))?;
    Ok(Greyscale {
        width,
        height,
        levels,
    })
}

/// Returns `None` for a pixel format that this reader does not know.
fn levels(image: DynamicImage) -> Option<Vec<u16>> {
    Some(match image {
        DynamicImage::ImageLuma16(b) => b.into_raw(),
        DynamicImage::ImageLumaA16(b) => b.as_raw().iter().step_by(2).copied().collect(),
        DynamicImage::ImageLuma8(b) => b.as_raw().iter().map(|&v| widen(v)).collect(),
        DynamicImage::ImageLumaA8(b) => b.as_raw().iter().step_by(2).map(|&v| widen(v)).collect(),
        DynamicImage::ImageRgb16(b) => color(b.as_raw(), 3, |v| v),
        DynamicImage::ImageRgba16(b) => color(b.as_raw(), 4, |v| v),
        DynamicImage::ImageRgb8(b) => color(b.as_raw(), 3, widen),
        DynamicImage::ImageRgba8(b) => color(b.as_raw(), 4, widen),
        DynamicImage::ImageRgb32F(b) => float_color(b.as_raw(), 3),
        DynamicImage::ImageRgba32F(b) => float_color(b.as_raw(), 4),
        _ => return None,
    })
}

/// Scales an 8-bit level to the 16-bit range.
fn widen(v: u8) -> u16 {
    u16::from(v) * 257
}

/// The Rec. 709 luma weights, in parts of 10,000.
const LUMA: [u32; 3] = [2126, 7152, 722];

/// Returns the luma of each pixel. A grey pixel keeps its level exactly.
fn color<T: Copy>(samples: &[T], channels: usize, level: impl Fn(T) -> u16) -> Vec<u16> {
    samples
        .chunks_exact(channels)
        .map(|p| {
            let sum: u32 = (0..3).map(|i| LUMA[i] * u32::from(level(p[i]))).sum();
            ((sum + 5000) / 10_000) as u16
        })
        .collect()
}

fn float_color(samples: &[f32], channels: usize) -> Vec<u16> {
    samples
        .chunks_exact(channels)
        .map(|p| {
            let luma: f32 = (0..3).map(|i| LUMA[i] as f32 / 10_000.0 * p[i]).sum();
            // A NaN sample becomes 0.
            (luma.clamp(0.0, 1.0) * 65_535.0).round() as u16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use image::{ImageBuffer, Luma, LumaA, Rgb, Rgba};
    use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype};

    /// Levels that are not multiples of 256 or 257, in a 3 x 2 image.
    const LEVELS: [u16; 6] = [0, 1, 12345, 65534, 65535, 40001];

    fn encode(image: DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn grey16(format: ImageFormat) -> Vec<u8> {
        let b = ImageBuffer::<Luma<u16>, _>::from_raw(3, 2, LEVELS.to_vec()).unwrap();
        encode(DynamicImage::ImageLuma16(b), format)
    }

    fn tiff<C>(compression: Compression, width: u32, height: u32, data: &[C::Inner]) -> Vec<u8>
    where
        C: colortype::ColorType,
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut out = Cursor::new(Vec::new());
        let mut encoder = TiffEncoder::new(&mut out)
            .unwrap()
            .with_compression(compression);
        encoder.write_image::<C>(width, height, data).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_16_bit_is_exact() {
        let g = read_bytes("g", &grey16(ImageFormat::Png)).unwrap();
        assert_eq!((g.width, g.height), (3, 2));
        assert_eq!(g.levels, LEVELS);
    }

    #[test]
    fn tiff_16_bit_is_exact() {
        let g = read_bytes("g", &grey16(ImageFormat::Tiff)).unwrap();
        assert_eq!((g.width, g.height), (3, 2));
        assert_eq!(g.levels, LEVELS);
        for (label, compression) in [
            ("none", Compression::Uncompressed),
            ("lzw", Compression::Lzw),
            ("deflate", Compression::Deflate(DeflateLevel::Balanced)),
            ("packbits", Compression::Packbits),
        ] {
            let bytes = tiff::<colortype::Gray16>(compression, 3, 2, &LEVELS);
            let g = read_bytes("g", &bytes).unwrap();
            assert_eq!((g.width, g.height), (3, 2), "{label}");
            assert_eq!(g.levels, LEVELS, "{label}");
        }
    }

    #[test]
    fn png_8_bit_scales_to_full_range() {
        let b = ImageBuffer::<Luma<u8>, _>::from_raw(3, 1, vec![0, 255, 128]).unwrap();
        let bytes = encode(DynamicImage::ImageLuma8(b), ImageFormat::Png);
        assert_eq!(read_bytes("g", &bytes).unwrap().levels, [0, 65535, 32896]);
    }

    #[test]
    fn color_becomes_luma() {
        let grey: Vec<u16> = LEVELS.iter().flat_map(|&v| [v, v, v]).collect();
        let b = ImageBuffer::<Rgb<u16>, _>::from_raw(3, 2, grey).unwrap();
        let bytes = encode(DynamicImage::ImageRgb16(b), ImageFormat::Png);
        assert_eq!(read_bytes("c", &bytes).unwrap().levels, LEVELS);

        let grey: Vec<u8> = [0u8, 255, 128].iter().flat_map(|&v| [v, v, v]).collect();
        let b = ImageBuffer::<Rgb<u8>, _>::from_raw(3, 1, grey).unwrap();
        let bytes = encode(DynamicImage::ImageRgb8(b), ImageFormat::Png);
        assert_eq!(read_bytes("c", &bytes).unwrap().levels, [0, 65535, 32896]);

        let b = ImageBuffer::<Rgb<u16>, _>::from_raw(1, 1, vec![10000, 20000, 30000]).unwrap();
        let bytes = encode(DynamicImage::ImageRgb16(b), ImageFormat::Png);
        assert_eq!(read_bytes("c", &bytes).unwrap().levels, [18596]);
    }

    #[test]
    fn alpha_is_ignored() {
        let alphas = [0u16, 1, 30000, 65535, 77, 500];
        let rgba: Vec<u16> = LEVELS
            .iter()
            .zip(alphas)
            .flat_map(|(&v, a)| [v, v, v, a])
            .collect();
        let b = ImageBuffer::<Rgba<u16>, _>::from_raw(3, 2, rgba).unwrap();
        let bytes = encode(DynamicImage::ImageRgba16(b), ImageFormat::Png);
        assert_eq!(read_bytes("a", &bytes).unwrap().levels, LEVELS);

        let la: Vec<u16> = LEVELS
            .iter()
            .zip(alphas)
            .flat_map(|(&v, a)| [v, a])
            .collect();
        let b = ImageBuffer::<LumaA<u16>, _>::from_raw(3, 2, la).unwrap();
        let bytes = encode(DynamicImage::ImageLumaA16(b), ImageFormat::Png);
        assert_eq!(read_bytes("a", &bytes).unwrap().levels, LEVELS);
    }

    #[test]
    fn float_color_tiff_maps_and_clamps() {
        let samples: Vec<f32> = [0.0f32, 1.0, 0.5, -2.0, 7.0, f32::NAN]
            .iter()
            .flat_map(|&v| [v, v, v])
            .collect();
        let bytes = tiff::<colortype::RGB32Float>(Compression::Uncompressed, 3, 2, &samples);
        let g = read_bytes("f", &bytes).unwrap();
        assert_eq!(g.levels, [0, 65535, 32768, 0, 65535, 0]);
    }

    #[test]
    fn unsupported_tiff_is_an_error() {
        let float = tiff::<colortype::Gray32Float>(Compression::Uncompressed, 2, 1, &[0.0, 1.0]);
        let signed = tiff::<colortype::GrayI16>(Compression::Uncompressed, 2, 1, &[-5, 5]);
        for bytes in [float, signed] {
            match read_bytes("dem", &bytes) {
                Err(e @ ImportError::Parse { .. }) => assert!(e.to_string().contains("dem")),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn errors() {
        match read_bytes("notes", b"these bytes are not an image") {
            Err(e @ ImportError::NotRaster { .. }) => {
                assert_eq!(e.to_string(), "notes is not a PNG or TIFF image")
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            read_bytes("empty", b""),
            Err(ImportError::NotRaster { .. })
        ));
        // A JPEG signature.
        assert!(matches!(
            read_bytes(
                "photo",
                &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F']
            ),
            Err(ImportError::NotRaster { .. })
        ));
        let png = grey16(ImageFormat::Png);
        let tiff = grey16(ImageFormat::Tiff);
        for bytes in [&png, &tiff] {
            for len in [8, 20, bytes.len() / 2, bytes.len() - 1] {
                match read_bytes("cut", &bytes[..len]) {
                    Err(e @ ImportError::Parse { .. }) => assert!(e.to_string().contains("cut")),
                    // The image crate writes the TIFF directory after the pixels.
                    Ok(g) => assert_eq!(g.levels, LEVELS),
                    other => panic!("{len}: {other:?}"),
                }
            }
        }
        assert!(matches!(
            read_bytes("cut", &png[..png.len() / 2]),
            Err(ImportError::Parse { .. })
        ));
    }

    #[test]
    fn files() {
        let p = Path::new("/no/such/file.png");
        match read_file(p) {
            Err(e @ ImportError::Io { .. }) => assert!(e.to_string().contains("/no/such/file")),
            other => panic!("{other:?}"),
        }

        let path =
            std::env::temp_dir().join(format!("vale-import-raster-{}.heights", std::process::id()));
        std::fs::write(&path, grey16(ImageFormat::Png)).unwrap();
        let result = read_file(&path);
        std::fs::remove_file(&path).unwrap();
        let g = result.unwrap();
        assert_eq!((g.width, g.height), (3, 2));
        assert_eq!(g.levels, LEVELS);
    }

    /// A world heightmap of 16,384 x 8,192 pixels at 16 bits fits in the limits.
    #[test]
    #[ignore = "uses 1 GB of memory"]
    fn world_size_tiff_decodes() {
        let (w, h) = (16_384u32, 8_192u32);
        let levels: Vec<u16> = (0..w as usize * h as usize)
            .map(|i| (i % 65_521) as u16)
            .collect();
        let bytes = tiff::<colortype::Gray16>(Compression::Uncompressed, w, h, &levels);
        let g = read_bytes("world", &bytes).unwrap();
        assert_eq!((g.width, g.height), (w as usize, h as usize));
        assert!(g.levels == levels);
    }
}
