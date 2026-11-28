use isotherm_import::{PNG_SEA_LEVEL, heightmap_png};
use vale_terrain::level_to_meters;

/// A 16-bit grey PNG whose north half is at `north` meters and whose south
/// half is at `south` meters.
fn halves(width: u32, height: u32, north: i32, south: i32) -> Vec<u8> {
    let mut data = Vec::new();
    for row in 0..height {
        let meters = if row < height / 2 { north } else { south };
        let level = (meters + PNG_SEA_LEVEL) as u16;
        for _ in 0..width {
            data.extend_from_slice(&level.to_be_bytes());
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    out
}

#[test]
fn high_level_is_land_and_low_level_is_sea() {
    let bytes = halves(64, 32, 1000, -3000);
    let heights = heightmap_png(&bytes, 16).unwrap();
    assert_eq!(heights.face_size(), 16);
    let north = level_to_meters(heights.sample([0.0, 0.0, 1.0]));
    let south = level_to_meters(heights.sample([0.0, 0.0, -1.0]));
    assert!((north - 1000.0).abs() < 1.0, "north is {north} m");
    assert!((south + 3000.0).abs() < 1.0, "south is {south} m");
}

#[test]
fn eight_bit_png_is_an_error() {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 4, 2);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0; 8]).unwrap();
    writer.finish().unwrap();
    assert!(heightmap_png(&out, 4).is_err());
}
