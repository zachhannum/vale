use std::io::Cursor;

use isotherm_import::{Samples, Scale, face_size_for, read_heightmap, read_image, to_terrain};
use isotherm_world::{DEFAULT_HIGH_M, DEFAULT_LOW_M, Terrain};
use tiff::encoder::{TiffEncoder, colortype};
use tiff::tags::Tag;
use vale_terrain::math::lonlat_to_dir;

const NORTH: [f64; 3] = [0.0, 0.0, 1.0];
const SOUTH: [f64; 3] = [0.0, 0.0, -1.0];

/// The samples of an image whose north half has `north` and whose south
/// half has `south`, row by row.
fn halves<T: Copy>(width: usize, height: usize, north: T, south: T) -> Vec<T> {
    (0..width * height)
        .map(|i| if i / width < height / 2 { north } else { south })
        .collect()
}

fn png(
    width: usize,
    height: usize,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
    encoder.set_color(color);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

fn png16(width: usize, height: usize, levels: &[u16]) -> Vec<u8> {
    let data: Vec<u8> = levels.iter().flat_map(|l| l.to_be_bytes()).collect();
    png(
        width,
        height,
        png::ColorType::Grayscale,
        png::BitDepth::Sixteen,
        &data,
    )
}

fn meters(terrain: &Terrain, d: [f64; 3]) -> f64 {
    terrain.meters(terrain.heights.sample(d))
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not near {b}");
}

#[test]
fn an_8_bit_grey_png_spans_black_to_white() {
    let data = halves(64, 32, 255u8, 0);
    let bytes = png(
        64,
        32,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &data,
    );
    let samples = read_image("grey", &bytes).unwrap();
    assert!(!samples.has_units());
    let terrain = to_terrain(&samples, Scale::default(), 16).unwrap();
    assert_eq!(terrain.heights.face_size(), 16);
    assert_eq!(
        (terrain.low_m, terrain.high_m),
        (DEFAULT_LOW_M, DEFAULT_HIGH_M)
    );
    near(meters(&terrain, NORTH), DEFAULT_HIGH_M, 0.5);
    near(meters(&terrain, SOUTH), DEFAULT_LOW_M, 0.5);
}

#[test]
fn a_16_bit_grey_png_keeps_its_levels() {
    let levels = halves(64, 32, 32768 + 1000, 32768 - 3000);
    let terrain = read_heightmap("grey", &png16(64, 32, &levels), Scale::SIGNED_16, 16).unwrap();
    assert_eq!(terrain.heights.sample(NORTH), 32768 + 1000);
    assert_eq!(meters(&terrain, NORTH), 1000.0);
    assert_eq!(meters(&terrain, SOUTH), -3000.0);
    assert_eq!(terrain.sea_level_m, 0.0);
}

#[test]
fn an_rgb_png_reads_as_its_luma() {
    let data: Vec<u8> = halves(64, 32, [255u8; 3], [0; 3]).concat();
    let bytes = png(64, 32, png::ColorType::Rgb, png::BitDepth::Eight, &data);
    let samples = read_image("color", &bytes).unwrap();
    assert!(!samples.has_units());
    let scale = Scale {
        black_m: -100.0,
        white_m: 300.0,
    };
    let terrain = to_terrain(&samples, scale, 16).unwrap();
    near(meters(&terrain, NORTH), 300.0, 0.01);
    near(meters(&terrain, SOUTH), -100.0, 0.01);
}

#[test]
fn a_16_bit_tiff_keeps_its_levels() {
    let levels = halves(64, 32, 50000u16, 7);
    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out).unwrap();
    encoder
        .write_image::<colortype::Gray16>(64, 32, &levels)
        .unwrap();
    let samples = read_image("tiff", out.get_ref()).unwrap();
    assert_eq!(
        samples,
        Samples::Levels {
            width: 64,
            height: 32,
            levels
        }
    );
    let terrain = to_terrain(&samples, Scale::default(), 8).unwrap();
    assert_eq!(terrain.heights.sample(NORTH), 50000);
    assert_eq!(terrain.heights.sample(SOUTH), 7);
}

#[test]
fn a_float_geotiff_gives_meters() {
    let data = halves(64, 32, 2500.5f32, -4200.0);
    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out).unwrap();
    encoder
        .write_image::<colortype::Gray32Float>(64, 32, &data)
        .unwrap();
    let samples = read_image("float", out.get_ref()).unwrap();
    assert!(samples.has_units());
    // The scale does not apply to heights in meters.
    let terrain = to_terrain(&samples, Scale::default(), 16).unwrap();
    assert_eq!((terrain.low_m, terrain.high_m), (-4200.0, 2500.5));
    assert_eq!(terrain.sea_level_m, 0.0);
    near(meters(&terrain, NORTH), 2500.5, 0.01);
    near(meters(&terrain, SOUTH), -4200.0, 0.01);
}

#[test]
fn a_signed_16_bit_tiff_gives_meters() {
    let data = halves(64, 32, 900i16, -1100);
    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out).unwrap();
    encoder
        .write_image::<colortype::GrayI16>(64, 32, &data)
        .unwrap();
    let terrain = read_heightmap("signed", out.get_ref(), Scale::default(), 16).unwrap();
    assert_eq!((terrain.low_m, terrain.high_m), (-1100.0, 900.0));
    near(meters(&terrain, NORTH), 900.0, 0.01);
}

#[test]
fn the_gdal_offset_and_scale_of_a_16_bit_geotiff_give_meters() {
    let levels = halves(64, 32, 20000u16, 4000);
    let xml = "<GDALMetadata>\n  \
         <Item name=\"OFFSET\" sample=\"0\" role=\"offset\">-6000</Item>\n  \
         <Item name=\"SCALE\" sample=\"0\" role=\"scale\">0.5</Item>\n\
         </GDALMetadata>\n";
    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out).unwrap();
    let mut image = encoder.new_image::<colortype::Gray16>(64, 32).unwrap();
    image.encoder().write_tag(Tag::Unknown(42112), xml).unwrap();
    image.write_data(&levels).unwrap();
    let samples = read_image("geotiff", out.get_ref()).unwrap();
    assert!(samples.has_units());
    let terrain = to_terrain(&samples, Scale::default(), 16).unwrap();
    assert_eq!((terrain.low_m, terrain.high_m), (-4000.0, 4000.0));
    near(meters(&terrain, NORTH), 4000.0, 0.01);
    near(meters(&terrain, SOUTH), -4000.0, 0.01);
}

#[test]
fn an_image_of_any_size_fills_the_cube() {
    for (w, h) in [(300, 150), (301, 151), (100, 77)] {
        let levels = halves(w, h, 60000u16, 1000);
        let terrain = read_heightmap("odd", &png16(w, h, &levels), Scale::default(), 32).unwrap();
        assert_eq!(terrain.heights.sample(NORTH), 60000, "{w} x {h}");
        assert_eq!(terrain.heights.sample(SOUTH), 1000, "{w} x {h}");
    }
    assert_eq!(face_size_for(300), 256);
}

#[test]
fn a_white_pixel_reads_the_height_of_white() {
    let levels = vec![65535u16; 64 * 32];
    let scale = Scale {
        black_m: -500.0,
        white_m: 1500.0,
    };
    let terrain = read_heightmap("white", &png16(64, 32, &levels), scale, 16).unwrap();
    assert_eq!((terrain.low_m, terrain.high_m), (-500.0, 1500.0));
    assert_eq!(meters(&terrain, lonlat_to_dir(10.0, 20.0)), 1500.0);
    let below = Scale {
        black_m: 2.0,
        white_m: 1.0,
    };
    assert!(read_heightmap("white", &png16(64, 32, &levels), below, 16).is_err());
}

#[test]
fn the_sample_earth_has_mountains_and_oceans() {
    let bytes = include_bytes!("../../isotherm-app/data/earth.png");
    let samples = read_image("earth", bytes).unwrap();
    assert!(!samples.has_units());
    let face_size = face_size_for(samples.width());
    let terrain = to_terrain(&samples, Scale::SIGNED_16, face_size).unwrap();
    assert_eq!(terrain.sea_level_m, 0.0);
    let himalaya = meters(&terrain, lonlat_to_dir(87.0, 28.0));
    let pacific = meters(&terrain, lonlat_to_dir(-160.0, 0.0));
    assert!(himalaya > 4000.0, "the Himalaya is at {himalaya} m");
    assert!(pacific < -3000.0, "the Pacific is at {pacific} m");
}
