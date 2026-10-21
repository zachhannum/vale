//! The measures of the line of a color stop, for the tests of the globe and
//! of the flat view.

#![allow(dead_code)]

use egui_kittest::Harness;
use vale_app::globe::math::V3;
use vale_app::ui::AppState;
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Heightmap, Stop, StopLine, meters_to_level};

pub type App = Harness<'static, AppState>;

/// The weight of the line of a stop, in points.
pub const LINE_WEIGHT: f32 = 3.0;

/// The colors of a measured line: red everywhere, with green on land.
pub const MEASURE_COLORS: [[u8; 3]; 4] = [[255, 0, 0], [255, 0, 0], [255, 255, 0], [255, 255, 0]];

/// Gives a range of heights one stop at each end and one at sea level. The
/// colors go from the lowest value to the highest, with two at sea level.
pub fn three_stops(bands: &mut Bands, colors: [[u8; 3]; 4]) {
    let stop = |value, below, above| Stop {
        value,
        below,
        above,
        line: StopLine::default(),
    };
    let (min, max) = (bands.min(), bands.max());
    let stops = [
        stop(min, colors[0], colors[0]),
        stop(0.0, colors[1], colors[2]),
        stop(max, colors[3], colors[3]),
    ];
    *bands = Bands::from_range_parts(min, max, true, &stops).unwrap();
}

/// Shows each raster layer, and a globe with no layers, with this display.
pub fn show_as(state: &mut AppState, display: RasterDisplay) {
    state.globe.preview.display = display;
    let ids: Vec<RasterId> = state.doc.project.rasters().iter().map(|r| r.id).collect();
    for id in ids {
        state.set_raster_display(id, display);
    }
}

/// The frame without the pointer, cut to the canvas.
pub fn canvas_image(h: &mut App) -> image::RgbaImage {
    h.remove_cursor();
    h.run_steps(2);
    let rect = h.state().globe.rect * h.ctx.pixels_per_point();
    let (x, y) = (rect.min.x.ceil() as u32, rect.min.y.ceil() as u32);
    let (w, h_) = (rect.width().floor() as u32, rect.height().floor() as u32);
    let frame = h.render().unwrap();
    image::imageops::crop_imm(&frame, x, y, w - 1, h_ - 1).to_image()
}

/// The measures of a line in one column of the canvas, in pixels from the
/// top of the canvas image.
#[derive(Debug)]
pub struct LineColumn {
    /// The dark part of the column.
    pub weight: f32,
    /// The middle of the dark part.
    pub center: f32,
    /// The band edge, from the land color of the image with no line.
    pub edge: f32,
}

/// Measures the coast line in each column of the
/// middle third of the canvas. The coast goes across the canvas, with land
/// above it.
pub fn measure_line(h: &mut App, name: &str) -> Vec<LineColumn> {
    set_coast_line(h, false);
    let plain = canvas_image(h);
    set_coast_line(h, true);
    let lined = canvas_image(h);
    if std::env::var_os("VALE_LINE_IMAGES").is_some() {
        lined.save(format!("/tmp/line-{name}.png")).unwrap();
    }
    let ink = |x, y| {
        let a = f32::from(plain.get_pixel(x, y).0[0]).max(1.0);
        let b = f32::from(lined.get_pixel(x, y).0[0]);
        (a - b).max(0.0) / a
    };
    let height = lined.height();
    (lined.width() / 3..2 * lined.width() / 3)
        .map(|x| {
            let dark: Vec<u32> = (0..height).filter(|&y| ink(x, y) > 0.02).collect();
            let (first, last) = match (dark.first(), dark.last()) {
                (Some(&a), Some(&b)) => (a, b),
                _ => (height / 2, height / 2),
            };
            let rows = first.saturating_sub(8)..(last + 9).min(height);
            let weight: f32 = rows.clone().map(|y| ink(x, y)).sum();
            let moment: f32 = rows.clone().map(|y| ink(x, y) * (y as f32 + 0.5)).sum();
            // The shade of the globe darkens red and green alike.
            let land: f32 = rows
                .clone()
                .map(|y| {
                    let [r, g, _, _] = plain.get_pixel(x, y).0;
                    f32::from(g) / f32::from(r).max(1.0)
                })
                .sum();
            LineColumn {
                weight,
                center: moment / weight.max(1e-6),
                edge: rows.start as f32 + land,
            }
        })
        .collect()
}

/// Turns the line of the stop at sea level on or off.
pub fn set_coast_line(h: &mut App, on: bool) {
    let bands = &mut h.state_mut().globe.map.bands;
    let sea = bands.sea().unwrap();
    let line = StopLine {
        on,
        weight: LINE_WEIGHT,
        color: [0; 3],
        ..StopLine::default()
    };
    assert!(bands.set_line(sea, line));
}

/// The heights of an even slope, in meters, at a distance along an axis.
pub fn even_slope(along: f64) -> f64 {
    3000.0 * along
}

/// A slope down to a flat sea just below sea level.
pub fn flat_sea(along: f64) -> f64 {
    if along < 0.0 { -5.0 } else { 3000.0 * along }
}

/// A slope up to flat land just above sea level.
pub fn flat_land(along: f64) -> f64 {
    if along > 0.0 { 5.0 } else { 3000.0 * along }
}

/// Gives the highest stop a line. No height crosses it, so the image with
/// no coast line draws the band edges in the same way as the image with it.
pub fn set_top_line(h: &mut App) {
    let bands = &mut h.state_mut().globe.map.bands;
    let top = bands.stops().len() - 1;
    let line = StopLine {
        on: true,
        weight: LINE_WEIGHT,
        color: [0; 3],
        ..StopLine::default()
    };
    assert!(bands.set_line(top, line));
}

/// Sets each texel to `height` of its direction, in meters.
pub fn set_heights(map: &mut Heightmap, height: impl Fn(V3) -> f64) {
    let n = map.face_size();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let level = meters_to_level(height(map.texel_dir(face, x, y)));
                map.set(face, x, y, level);
            }
        }
    }
}

/// Shows the brush layer in bands with the colors of a measured line, with
/// `height` of each direction in meters on a face of `face_size` texels.
/// Only the stop at sea level and the highest stop have a line.
pub fn measured_scene(h: &mut App, face_size: usize, height: impl Fn(V3) -> f64) {
    show_as(h.state_mut(), RasterDisplay::Bands);
    let globe = &mut h.state_mut().globe;
    globe.set_face_size(face_size);
    globe.preview.graticule = false;
    globe.preview.rivers = false;
    three_stops(&mut globe.map.bands, MEASURE_COLORS);
    set_heights(&mut globe.map, height);
    set_coast_line(h, true);
    set_top_line(h);
}

/// The measures of the line at one place on the coast, in pixels.
#[derive(Debug)]
pub struct Across {
    /// A place near the band edge.
    pub at: [f64; 2],
    /// The width of the line, along the normal of the edge.
    pub weight: f64,
    /// The middle of the line from the band edge, along the normal.
    pub center: f64,
}

/// The value of an image at a place, from the 4 pixel centers around it.
pub fn sample(values: &[f64], width: usize, p: [f64; 2]) -> f64 {
    let (x, y) = (p[0] - 0.5, p[1] - 0.5);
    let (i, j) = (x.floor() as usize, y.floor() as usize);
    let (s, t) = (x - x.floor(), y - y.floor());
    let v = |a: usize, b: usize| values[b * width + a];
    let top = v(i, j) * (1.0 - s) + v(i + 1, j) * s;
    let bottom = v(i, j + 1) * (1.0 - s) + v(i + 1, j + 1) * s;
    top * (1.0 - t) + bottom * t
}

/// Measures the coast line across the straight parts of the band edge in
/// the middle third of the canvas, on the ground. A part is straight where the normals of
/// the edge within 3 points turn by less than 10 degrees.
pub fn measure_across(h: &mut App, name: &str) -> Vec<Across> {
    let scale = f64::from(h.ctx.pixels_per_point());
    set_coast_line(h, false);
    let plain = canvas_image(h);
    set_coast_line(h, true);
    let lined = canvas_image(h);
    if std::env::var_os("VALE_LINE_IMAGES").is_some() {
        lined.save(format!("/tmp/line-{name}.png")).unwrap();
    }
    let (w, ht) = (plain.width() as usize, plain.height() as usize);
    let mut land = Vec::with_capacity(w * ht);
    let mut ink = Vec::with_capacity(w * ht);
    // The colors of the measure have no blue, and the canvas around them
    // has some.
    let ground: Vec<bool> = plain.pixels().map(|p| p.0[2] < 30).collect();
    for (a, b) in plain.pixels().zip(lined.pixels()) {
        // The shade of the globe darkens red and green alike.
        let red = f64::from(a.0[0]).max(1.0);
        land.push(f64::from(a.0[1]) / red);
        ink.push(((red - f64::from(b.0[0])) / red).max(0.0));
    }
    let reach = f64::from(LINE_WEIGHT) * scale / 2.0 + 3.0;
    let margin = reach as usize + 3;
    // The band edge between each two pixel centers, with its normal toward
    // the land.
    let mut edge: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for y in margin..ht - margin {
        for x in w / 3..2 * w / 3 {
            let here = land[y * w + x];
            for (dx, dy) in [(1, 0), (0, 1)] {
                let next = land[(y + dy) * w + x + dx];
                let around = (y - 1..=y + 2).all(|j| (x - 1..=x + 2).all(|i| ground[j * w + i]));
                if (here < 0.5) == (next < 0.5) || !around {
                    continue;
                }
                let t = (0.5 - here) / (next - here);
                let at = [
                    x as f64 + 0.5 + t * dx as f64,
                    y as f64 + 0.5 + t * dy as f64,
                ];
                let v = |i: usize, j: usize| land[j * w + i];
                let gx = v(x + 1, y - 1) + 2.0 * v(x + 1, y) + v(x + 1, y + 1)
                    - v(x - 1, y - 1)
                    - 2.0 * v(x - 1, y)
                    - v(x - 1, y + 1);
                let gy = v(x - 1, y + 1) + 2.0 * v(x, y + 1) + v(x + 1, y + 1)
                    - v(x - 1, y - 1)
                    - 2.0 * v(x, y - 1)
                    - v(x + 1, y - 1);
                let n = gx.hypot(gy).max(1e-9);
                edge.push((at, [gx / n, gy / n]));
            }
        }
    }
    let near = 3.0 * scale;
    edge.iter()
        .filter(|(at, n)| {
            edge.iter().all(|(b, m)| {
                let d = (at[0] - b[0]).hypot(at[1] - b[1]);
                d > near || n[0] * m[0] + n[1] * m[1] > 10f64.to_radians().cos()
            })
        })
        .map(|&(at, n)| {
            // The sums along the normal. For a straight edge, they give the
            // place of the edge and of the line with no error from the
            // blend between pixel centers.
            let (mut land_sum, mut ink_sum, mut moment) = (0.0, 0.0, 0.0);
            let step = 0.01;
            let mut s = -reach;
            while s <= reach {
                let p = [at[0] + s * n[0], at[1] + s * n[1]];
                let k = sample(&ink, w, p);
                land_sum += sample(&land, w, p) * step;
                ink_sum += k * step;
                moment += k * s * step;
                s += step;
            }
            let edge = reach - land_sum;
            let weight = ink_sum;
            let center = moment / ink_sum.max(1e-9) - edge;
            Across { at, weight, center }
        })
        .collect()
}

/// Makes sure that each measure has the weight of the line in pixels, with
/// its middle on the band edge, within 0.25 px. There must be at least
/// `least` measures.
pub fn assert_across(found: &[Across], scale: f64, least: usize, what: &str) {
    assert!(found.len() >= least, "{what}: {} places", found.len());
    let weight = f64::from(LINE_WEIGHT) * scale;
    for a in found {
        let ok = (a.weight - weight).abs() < 0.25 && a.center.abs() < 0.25;
        assert!(ok, "{what}: {a:?}, not a weight of {weight}");
    }
}
