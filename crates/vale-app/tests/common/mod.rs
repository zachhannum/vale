//! The test symbols in `tests/data`, and a heightmap with a valley.

// Each test file uses a part of the helpers.
#![allow(dead_code)]

use vale_app::document::Document;

/// Adds the test symbols `names` to the project of `doc`, as a user adds an
/// SVG file.
pub fn add_symbols(doc: &mut Document, names: &[&str]) {
    for name in names {
        let added = doc.add_symbol(name, svg(name)).unwrap();
        assert_eq!(added, *name);
    }
}

/// The text of the test symbol `name`.
pub fn svg(name: &str) -> String {
    let path = format!("{}/tests/data/{name}.svg", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// The height in meters of a valley along the meridian at 0.35° east. It
/// goes down to the sea at 40° south, and its head is at latitude `head`.
pub fn valley_meters(lonlat: [f64; 2], head: f64) -> f64 {
    let [lon, lat] = lonlat;
    if lon.abs() > 3.0 || !(-40.0..head).contains(&lat) {
        return -1000.0;
    }
    100.0 + 30.0 * (lat + 40.0) + 300.0 * (lon - 0.35).abs()
}

/// Puts the valley with its head at `head` into `map`. Only the texels that
/// change get a new level.
pub fn paint_valley(map: &mut vale_terrain::Heightmap, head: f64) {
    use vale_terrain::math::dir_to_lonlat;
    let n = map.face_size();
    for face in 0..vale_terrain::FACES {
        for y in 0..n {
            for x in 0..n {
                let (lon, lat) = dir_to_lonlat(map.texel_dir(face, x, y));
                let level =
                    vale_terrain::Elevation::default().level(valley_meters([lon, lat], head));
                if map.get(face, x as i64, y as i64) != level {
                    map.set(face, x, y, level);
                }
            }
        }
    }
}

/// The height in meters of the valley of `valley_meters` with its head at
/// the equator, and a side valley along 20.35° S that falls into it from
/// the east.
pub fn confluence_meters(lonlat: [f64; 2]) -> f64 {
    let [lon, lat] = lonlat;
    let main = valley_meters(lonlat, 0.0);
    let main = (main > 0.0).then_some(main);
    let side = (0.35..14.0).contains(&lon) && (lat + 20.35).abs() <= 3.0;
    let side =
        side.then(|| 100.0 + 30.0 * 19.65 + 15.0 * (lon - 0.35) + 300.0 * (lat + 20.35).abs());
    match (main, side) {
        (Some(a), Some(b)) => a.min(b),
        (Some(h), None) | (None, Some(h)) => h,
        (None, None) => -1000.0,
    }
}

/// Puts the valley and the side valley of `confluence_meters` into `map`.
pub fn paint_confluence(map: &mut vale_terrain::Heightmap) {
    use vale_terrain::math::dir_to_lonlat;
    let n = map.face_size();
    for face in 0..vale_terrain::FACES {
        for y in 0..n {
            for x in 0..n {
                let (lon, lat) = dir_to_lonlat(map.texel_dir(face, x, y));
                let level = vale_terrain::Elevation::default().level(confluence_meters([lon, lat]));
                map.set(face, x, y, level);
            }
        }
    }
}
