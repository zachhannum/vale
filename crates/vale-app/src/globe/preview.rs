//! The preview of the heightmap: stepped tints, a smooth ramp, or greyscale.

use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, MAX_BANDS, Rgb};

/// How the globe shows the heightmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    /// The display of a globe with no list of layers. A raster layer of
    /// the list has its own display.
    pub display: RasterDisplay,
    pub graticule: bool,
    /// Draws the rivers over the colors.
    pub rivers: bool,
    /// The factor of the width of a river line on screen.
    pub river_width: f32,
    /// The elevation panel is open.
    pub panel: bool,
    /// The band limit that a panel edits: the raster layer, and an index
    /// into its limits.
    pub selected: Option<(RasterId, usize)>,
}

impl Default for Preview {
    fn default() -> Preview {
        Preview {
            display: RasterDisplay::Bands,
            graticule: true,
            rivers: true,
            river_width: 1.0,
            panel: false,
            selected: None,
        }
    }
}

/// The mode number of `globe.wgsl` for a display.
pub fn mode(display: RasterDisplay) -> f32 {
    match display {
        RasterDisplay::Grey => 0.0,
        RasterDisplay::Bands => 1.0,
        RasterDisplay::Gradient => 2.0,
    }
}

/// One band in the layout of `globe.wgsl`: the color at the lower limit with
/// the limit, the color at the upper limit with the limit, and the color of
/// the step. A limit is a level divided by the highest level.
pub type BandUniform = [[f32; 4]; 3];

/// The bands for the shader, and their count.
pub fn band_uniforms(bands: &Bands) -> ([BandUniform; MAX_BANDS], usize) {
    let mut out = [[[0.0; 4]; 3]; MAX_BANDS];
    let list = bands.bands();
    let entry = |color: Rgb, value: f64| {
        let [r, g, b] = color.map(|c| f32::from(c) / 255.0);
        [r, g, b, f32::from(bands.level(value)) / 65535.0]
    };
    for (slot, band) in out.iter_mut().zip(&list) {
        *slot = [
            entry(band.bottom, band.min),
            entry(band.top, band.max),
            entry(band.color, band.min),
        ];
    }
    (out, list.len().min(MAX_BANDS))
}

/// The line of one stop in the layout of `globe.wgsl`: the color with half
/// of the weight in pixels, then the value of the stop as a limit with the
/// color of the ground at that value. The shader reads that color for the
/// peak step of the top stop only.
pub type LineUniform = [[f32; 4]; 2];

/// The lines of the stops for the shader. A line that is off has a weight of
/// 0. `pixels_per_point` turns a weight in points into pixels.
pub fn line_uniforms(bands: &Bands, pixels_per_point: f32) -> [LineUniform; MAX_BANDS + 1] {
    let mut out = [[[0.0; 4]; 2]; MAX_BANDS + 1];
    for (index, (slot, stop)) in out.iter_mut().zip(bands.stops()).enumerate() {
        let line = stop.line;
        let color = bands.line_color(index).unwrap_or(line.color);
        let [r, g, b] = color.map(|c| f32::from(c) / 255.0);
        let half = if line.on {
            0.5 * line.weight * pixels_per_point
        } else {
            0.0
        };
        let at = f32::from(bands.level(stop.value)) / 65535.0;
        let [fr, fg, fb] = stop.above.map(|c| f32::from(c) / 255.0);
        *slot = [[r, g, b, half], [at, fr, fg, fb]];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_terrain::StopLine;

    #[test]
    fn each_band_starts_where_the_band_below_ends() {
        let bands = Bands::default();
        let (uniforms, count) = band_uniforms(&bands);
        assert_eq!(count, bands.stops().len() - 1);
        assert_eq!(uniforms[0][0][3], 0.0);
        assert_eq!(uniforms[count - 1][1][3], 1.0);
        for pair in uniforms[..count].windows(2) {
            assert_eq!(pair[0][1][3], pair[1][0][3]);
        }
    }

    #[test]
    fn the_top_line_has_the_color_of_the_peak() {
        let mut bands = Bands::default();
        let last = bands.stops().len() - 1;
        bands.set_color(last, vale_terrain::Side::Above, [255, 0, 0]);
        let lines = line_uniforms(&bands, 1.0);
        assert_eq!(lines[last][1], [1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_line_has_the_value_of_its_stop_and_a_weight_in_pixels() {
        let mut bands = Bands::default();
        let sea = bands.sea().unwrap();
        let line = StopLine {
            on: true,
            weight: 1.5,
            color: [255, 0, 0],
            ..StopLine::default()
        };
        bands.set_line(sea, line);
        bands.set_line(0, StopLine { on: false, ..line });
        let lines = line_uniforms(&bands, 2.0);
        let (uniforms, count) = band_uniforms(&bands);
        assert_eq!(lines[sea][0], [1.0, 0.0, 0.0, 1.5]);
        assert_eq!(lines[sea][1][0], uniforms[sea][0][3]);
        assert_eq!(lines[0][0][3], 0.0);
        assert_eq!(lines[count][1][0], 1.0);
        assert!(lines[count + 1..].iter().all(|l| l[0][3] == 0.0));
        // A darker line has the darker color of its stop.
        let darker = StopLine {
            darker: true,
            darken: 1.0,
            ..line
        };
        bands.set_line(sea, darker);
        assert_eq!(line_uniforms(&bands, 2.0)[sea][0], [0.0, 0.0, 0.0, 1.5]);
    }

    #[test]
    fn each_display_has_its_mode() {
        assert_eq!(mode(RasterDisplay::Grey), 0.0);
        assert_eq!(mode(RasterDisplay::Bands), 1.0);
        assert_eq!(mode(RasterDisplay::Gradient), 2.0);
    }
}
