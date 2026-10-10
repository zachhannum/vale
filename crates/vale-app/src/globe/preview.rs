//! The preview of the heightmap: stepped tints, a smooth ramp, or greyscale.

use vale_terrain::{Bands, MAX_BANDS, Rgb, meters_to_level};

/// How the globe shows the heightmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    /// Shows the plain heights and no colors.
    pub greyscale: bool,
    /// Cuts the colors at the band limits. Without it, the ramp is smooth.
    pub levels: bool,
    pub graticule: bool,
    /// Draws the rivers over the colors.
    pub rivers: bool,
    /// The factor of the width of a river line on screen.
    pub river_width: f32,
    /// The elevation panel is open.
    pub panel: bool,
    /// The band limit that the panel edits, as an index into the limits.
    pub selected: Option<usize>,
}

impl Default for Preview {
    fn default() -> Preview {
        Preview {
            greyscale: false,
            levels: true,
            graticule: true,
            rivers: true,
            river_width: 1.0,
            panel: false,
            selected: None,
        }
    }
}

impl Preview {
    /// The mode number of `globe.wgsl`.
    pub fn mode(&self) -> f32 {
        match (self.greyscale, self.levels) {
            (true, _) => 0.0,
            (false, true) => 1.0,
            (false, false) => 2.0,
        }
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
    let entry = |color: Rgb, meters: f64| {
        let [r, g, b] = color.map(|c| f32::from(c) / 255.0);
        [r, g, b, f32::from(meters_to_level(meters)) / 65535.0]
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
    fn the_mode_follows_the_switches() {
        let mut preview = Preview::default();
        assert_eq!(preview.mode(), 1.0);
        preview.levels = false;
        assert_eq!(preview.mode(), 2.0);
        preview.greyscale = true;
        assert_eq!(preview.mode(), 0.0);
    }
}
