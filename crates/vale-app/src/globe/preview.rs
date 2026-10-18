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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_band_starts_where_the_band_below_ends() {
        let bands = Bands::default();
        let (uniforms, count) = band_uniforms(&bands);
        assert_eq!(count, bands.limits().len() + 1);
        assert_eq!(uniforms[0][0][3], 0.0);
        assert_eq!(uniforms[count - 1][1][3], 1.0);
        for pair in uniforms[..count].windows(2) {
            assert_eq!(pair[0][1][3], pair[1][0][3]);
        }
    }

    #[test]
    fn each_display_has_its_mode() {
        assert_eq!(mode(RasterDisplay::Grey), 0.0);
        assert_eq!(mode(RasterDisplay::Bands), 1.0);
        assert_eq!(mode(RasterDisplay::Gradient), 2.0);
    }
}
