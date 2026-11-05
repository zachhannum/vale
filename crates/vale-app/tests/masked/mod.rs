//! A polygon with a mask, and the shapes that a painter draws of it.

#![allow(dead_code)]

use eframe::egui::{self, Pos2};
use vale_app::document::Document;
use vale_app::globe::math::V3;
use vale_app::ui::AppState;
use vale_import::geojson;
use vale_style::{Color, Stroke};

pub const MASK_FILL: Color = Color::rgb(200, 10, 10);
pub const MASK_STROKE: Color = Color::rgb(10, 200, 10);

/// A document with a polygon from 0 to 40 degrees east and north, with a
/// red fill and a green stroke, and a hidden polygon from 10 degrees west
/// to 20 degrees east as its mask.
pub fn masked_square() -> (Document, vale_store::LayerId) {
    let mut doc = Document::empty();
    let square = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let text = format!(
            r#"{{"type":"Polygon","coordinates":[[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}],[{x0},{y0}]]]}}"#
        );
        geojson::read_str("square", &text).unwrap()
    };
    let id = doc.add_imported(square(0.0, 0.0, 40.0, 40.0), None)[0];
    let mask = doc.add_imported(square(-10.0, -10.0, 20.0, 50.0), None)[0];
    doc.set_visible(mask, false);
    let style = &mut doc.entry_mut(id).unwrap().style;
    style.set_fill(Some(MASK_FILL));
    style.set_stroke(Some(Stroke::solid(MASK_STROKE, 2.0)));
    style.style.mask = Some(vale_style::Mask::Polygons { layer: mask });
    (doc, id)
}

/// The screen positions of the fill and of the stroke of the shapes that
/// `paint` gives, in the colors of `masked_square`.
pub fn painted(paint: impl FnMut(&egui::Painter)) -> (Vec<Pos2>, Vec<Vec<Pos2>>) {
    let mut paint = paint;
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 800.0));
        paint(&egui::Painter::new(ui.ctx().clone(), ui.layer_id(), rect));
    });
    out.textures_delta.clear();
    let (fill, stroke) = (MASK_FILL, MASK_STROKE);
    let as32 = |c: Color| egui::Color32::from_rgb(c.r, c.g, c.b);
    let (mut fills, mut strokes) = (Vec::new(), Vec::new());
    for clipped in out.shapes {
        match clipped.shape {
            egui::Shape::Mesh(mesh) => fills.extend(
                (mesh.vertices.iter())
                    .filter(|v| v.color == as32(fill))
                    .map(|v| v.pos),
            ),
            egui::Shape::Path(path)
                if path.stroke.color == egui::epaint::ColorMode::Solid(as32(stroke)) =>
            {
                strokes.push(path.points.clone())
            }
            _ => {}
        }
    }
    (fills, strokes)
}

/// The longitude of each screen position, from a camera.
pub fn lons(points: &[Pos2], unproject: impl Fn(Pos2) -> Option<V3>) -> Vec<f64> {
    (points.iter())
        .map(|&p| {
            let d = unproject(p).expect("on the globe");
            d[1].atan2(d[0]).to_degrees()
        })
        .collect()
}

/// Checks that the fill and the stroke of `masked_square` stop at 20
/// degrees east, and that no step of the stroke goes along the cut.
pub fn assert_masked(
    fills: &[Pos2],
    strokes: &[Vec<Pos2>],
    unproject: impl Fn(Pos2) -> Option<V3>,
) {
    let range = |l: &[f64]| {
        (
            l.iter().copied().fold(f64::INFINITY, f64::min),
            l.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let (west, east) = range(&lons(fills, &unproject));
    assert!(
        west > -0.3 && (east - 20.0).abs() < 0.3,
        "fill {west} {east}"
    );
    let all: Vec<Pos2> = strokes.iter().flatten().copied().collect();
    let (west, east) = range(&lons(&all, &unproject));
    assert!(
        west > -0.3 && (east - 20.0).abs() < 0.3,
        "stroke {west} {east}"
    );
    for s in strokes {
        let l = lons(s, &unproject);
        for (w, p) in l.windows(2).zip(s.windows(2)) {
            let along = (w[0] - 20.0).abs() < 0.3 && (w[1] - 20.0).abs() < 0.3;
            assert!(
                !(along && p[0].distance(p[1]) > 2.0),
                "a stroke along the cut: {p:?}"
            );
        }
    }
}

/// The vector layer of `raster_masked`, and the layer of its raster.
pub struct RasterMasked {
    pub layer: vale_store::LayerId,
    pub raster: vale_store::LayerId,
}

/// Adds a hidden polygon layer from 60 degrees west to 80 degrees east and
/// from 40 degrees south to 80 degrees north, with a red fill and the mask
/// of the values of the raster layer `raster` from 0 up.
pub fn raster_masked(state: &mut AppState, raster: vale_store::RasterId) -> RasterMasked {
    let text =
        r#"{"type":"Polygon","coordinates":[[[-60,-40],[80,-40],[80,80],[-60,80],[-60,-40]]]}"#;
    let doc = &mut state.doc;
    let layer = doc.add_imported(geojson::read_str("land", text).unwrap(), None)[0];
    let of = |l: &&vale_store::Layer| l.kind == vale_store::LayerKind::Raster(raster);
    let raster = doc.project.layers().iter().find(of).unwrap().id;
    let style = &mut doc.entry_mut(layer).unwrap().style;
    style.set_fill(Some(MASK_FILL));
    style.set_stroke(None);
    style.style.mask = Some(vale_style::Mask::Raster {
        layer: raster,
        min: Some(0.0),
        max: None,
    });
    doc.set_visible(layer, false);
    state.sync_globe();
    RasterMasked { layer, raster }
}

/// Shows or hides a layer.
pub fn show(state: &mut AppState, layer: vale_store::LayerId, on: bool) {
    state.doc.set_visible(layer, on);
    state.sync_globe();
}

/// The color of the fill of `raster_masked`.
pub fn red(p: &image::Rgba<u8>) -> bool {
    let [r, g, b, _] = p.0;
    r > 150 && g < 70 && b < 70
}

/// What a pixel of a raster of two bands shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Sea,
    Land,
    Water,
    /// A mix of the others.
    Mixed,
}

/// What a pixel shows, for a sea band of grey 40 and a light land band. A
/// pixel is land or sea only if most of it is, as `red` asks of the fill.
pub fn ground(p: &image::Rgba<u8>) -> Ground {
    let [r, g, b, _] = p.0.map(i32::from);
    if b > r + 60 && b > g + 30 {
        Ground::Water
    } else if r.max(g).max(b) < 50 {
        Ground::Sea
    } else if r.max(g).max(b) > 95 && (r - b).abs() < 60 {
        Ground::Land
    } else {
        Ground::Mixed
    }
}

/// The match of the fill of `raster_masked` with the land of the raster.
#[derive(Debug, Default)]
pub struct Agreement {
    /// The land and sea pixels that the test keeps, away from each other.
    pub compared: usize,
    /// The land pixels of `compared`.
    pub land: usize,
    /// The compared pixels with red on the sea or with no red on the land.
    pub wrong: usize,
}

/// Compares the red pixels of `masked` with the land pixels of `plain`, at
/// the pixels that `inside` keeps. A pixel next to a pixel of another kind
/// is in the band of the coast, and it does not count.
pub fn agreement(
    plain: &image::RgbaImage,
    masked: &image::RgbaImage,
    inside: impl Fn(u32, u32) -> bool,
) -> Agreement {
    let (w, h) = plain.dimensions();
    let mut out = Agreement::default();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let kind = ground(plain.get_pixel(x, y));
            if !matches!(kind, Ground::Sea | Ground::Land) || !inside(x, y) {
                continue;
            }
            let mut near = (0..9).map(|i| (x + i % 3 - 1, y + i / 3 - 1));
            if near.any(|(a, b)| ground(plain.get_pixel(a, b)) != kind) {
                continue;
            }
            out.compared += 1;
            out.land += usize::from(kind == Ground::Land);
            out.wrong += usize::from(red(masked.get_pixel(x, y)) != (kind == Ground::Land));
        }
    }
    out
}

/// Keeps the pixels of a canvas image of `globe` that are well inside the
/// polygon of `raster_masked`.
pub fn inside_polygon(globe: &vale_app::globe::Globe) -> impl Fn(u32, u32) -> bool + '_ {
    let min = globe.rect.min.ceil();
    move |x, y| {
        let pos = min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5);
        globe.unproject(pos).is_some_and(|d| {
            let (lon, lat) = vale_terrain::math::dir_to_lonlat(d);
            (-50.0..70.0).contains(&lon) && (-30.0..60.0).contains(&lat)
        })
    }
}

/// The water pixels of `plain` well below the level of a lake, and the red
/// pixels of `masked` among them.
pub fn lake_red(
    globe: &vale_app::globe::Globe,
    plain: &image::RgbaImage,
    masked: &image::RgbaImage,
) -> (usize, usize) {
    let channels = globe.channels().unwrap();
    let margin = f64::from(vale_terrain::meters_to_level(150.0))
        - f64::from(vale_terrain::meters_to_level(0.0));
    let min = globe.rect.min.ceil();
    let (mut wet, mut red_wet) = (0, 0);
    for (x, y, p) in plain.enumerate_pixels() {
        let pos = min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5);
        let Some(dir) = globe.unproject(pos) else {
            continue;
        };
        let level = f64::from(globe.map.sample(dir));
        if level < channels.lake_level(dir) - margin && ground(p) == Ground::Water {
            wet += 1;
            red_wet += usize::from(red(masked.get_pixel(x, y)));
        }
    }
    (wet, red_wet)
}

/// Saves the images of a test of a raster mask when the variable
/// `VALE_SCREENSHOTS` is set.
pub fn save_images(name: &str, plain: &image::RgbaImage, masked: &image::RgbaImage) {
    if std::env::var_os("VALE_SCREENSHOTS").is_none() {
        return;
    }
    std::fs::create_dir_all("../../target/app").unwrap();
    plain
        .save(format!("../../target/app/{name}-plain.png"))
        .unwrap();
    masked.save(format!("../../target/app/{name}.png")).unwrap();
}

/// Adds ripples of 300 meters about 0.1 radians apart to the heights, so
/// that a smoothing moves the coast.
pub fn ripple(map: &mut vale_terrain::Heightmap) {
    let n = map.face_size();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let d = map.texel_dir(face, x, y);
                let wave = (60.0 * d[0]).sin() * (60.0 * d[1]).sin() * (60.0 * d[2]).sin();
                let meters = vale_terrain::level_to_meters(map.get(face, x as i64, y as i64));
                map.set(
                    face,
                    x,
                    y,
                    vale_terrain::meters_to_level(meters + 300.0 * wave),
                );
            }
        }
    }
}

/// The clip operators in the content of a PDF: the nonzero ones and the
/// even-odd ones.
pub fn pdf_clips(pdf: &[u8]) -> (usize, usize) {
    use std::io::Read;
    let mut text = String::new();
    let mut rest = pdf;
    while let Some(at) = rest.windows(6).position(|w| w == b"stream") {
        let mut start = at + 6;
        while matches!(rest.get(start), Some(b'\r' | b'\n')) {
            start += 1;
        }
        let body = &rest[start..];
        let end = body.windows(9).position(|w| w == b"endstream").unwrap_or(0);
        let mut out = Vec::new();
        match flate2::read::ZlibDecoder::new(&body[..end]).read_to_end(&mut out) {
            Ok(_) => text.push_str(&String::from_utf8_lossy(&out)),
            Err(_) => text.push_str(&String::from_utf8_lossy(&body[..end])),
        }
        text.push('\n');
        rest = &body[end + 9..];
    }
    let words: Vec<&str> = text.split_ascii_whitespace().collect();
    let count = |op: &str| words.iter().filter(|w| **w == op).count();
    (count("W"), count("W*"))
}
