//! The import of heightmaps into an Isotherm world: an equirectangular
//! image, or a height layer of a Vale project.

mod image;
pub mod vale_project;

use isotherm_world::{DEFAULT_HIGH_M, DEFAULT_LOW_M, Heightmap, Terrain};
use vale_terrain::Equirect;

pub use image::read_image;
pub use vale_import::ImportError;

/// The heights of black and of white in an image with no units, in meters.
/// The heights of the levels between them are linear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub black_m: f64,
    pub white_m: f64,
}

impl Scale {
    /// A 16-bit image where a level minus 32,768 is the height in meters.
    pub const SIGNED_16: Scale = Scale {
        black_m: -32768.0,
        white_m: 32767.0,
    };
}

impl Default for Scale {
    fn default() -> Scale {
        Scale {
            black_m: DEFAULT_LOW_M,
            white_m: DEFAULT_HIGH_M,
        }
    }
}

/// The pixels of an equirectangular heightmap, row by row from the north.
#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    /// Grey levels with no unit. A [`Scale`] gives their heights.
    Levels {
        width: usize,
        height: usize,
        levels: Vec<u16>,
    },
    /// Heights in meters. Level 0 is `low_m`, and level 65,535 is `high_m`.
    Meters {
        width: usize,
        height: usize,
        levels: Vec<u16>,
        low_m: f64,
        high_m: f64,
    },
}

impl Samples {
    /// True if the file gives the heights in meters, so that a [`Scale`]
    /// does not apply.
    pub fn has_units(&self) -> bool {
        matches!(self, Samples::Meters { .. })
    }

    pub fn width(&self) -> usize {
        match self {
            Samples::Levels { width, .. } | Samples::Meters { width, .. } => *width,
        }
    }

    pub fn height(&self) -> usize {
        match self {
            Samples::Levels { height, .. } | Samples::Meters { height, .. } => *height,
        }
    }
}

/// A face size for an image `width` pixels wide: the power of two from 256
/// to 2,048 that gives the equator at least as many texels as the image has
/// pixels, or the nearest end.
pub fn face_size_for(width: usize) -> usize {
    width
        .div_ceil(4)
        .max(1)
        .next_power_of_two()
        .clamp(256, 2048)
}

/// Puts the samples on a cube map with `face_size` texels on each side. The
/// levels stay as they are. `scale` gives the heights of samples with no
/// unit.
pub fn to_terrain(
    samples: &Samples,
    scale: Scale,
    face_size: usize,
) -> Result<Terrain, ImportError> {
    let error = |message: &str| ImportError::Parse {
        name: "heightmap".to_string(),
        message: message.to_string(),
    };
    let (width, height, levels, low_m, high_m) = match samples {
        Samples::Levels {
            width,
            height,
            levels,
        } => {
            let Scale { black_m, white_m } = scale;
            if !(black_m.is_finite() && white_m.is_finite()) {
                return Err(error("the height of black or white is not a number"));
            }
            if black_m >= white_m {
                return Err(error("black must be lower than white"));
            }
            (*width, *height, levels, black_m, white_m)
        }
        Samples::Meters {
            width,
            height,
            levels,
            low_m,
            high_m,
        } => (*width, *height, levels, *low_m, *high_m),
    };
    if face_size == 0 {
        return Err(error("the face size is zero"));
    }
    let src = Equirect::new(width, height, levels.clone()).ok_or(ImportError::Empty {
        name: "heightmap".to_string(),
    })?;
    let mut heights = Heightmap::new(face_size, 0);
    heights.import_equirect(&src.reduced_for(face_size));
    Ok(Terrain::new(heights, low_m, high_m))
}

/// Reads an equirectangular PNG or TIFF heightmap into a cube map with
/// `face_size` texels on each side. `scale` applies to an image with no
/// units.
pub fn read_heightmap(
    name: &str,
    bytes: &[u8],
    scale: Scale,
    face_size: usize,
) -> Result<Terrain, ImportError> {
    to_terrain(&read_image(name, bytes)?, scale, face_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_size_follows_the_width() {
        assert_eq!(face_size_for(0), 256);
        assert_eq!(face_size_for(1024), 256);
        assert_eq!(face_size_for(1500), 512);
        assert_eq!(face_size_for(4096), 1024);
        assert_eq!(face_size_for(86400), 2048);
    }

    #[test]
    fn black_above_white_is_an_error() {
        let samples = Samples::Levels {
            width: 2,
            height: 1,
            levels: vec![0, 65535],
        };
        let scale = Scale {
            black_m: 100.0,
            white_m: -100.0,
        };
        assert!(to_terrain(&samples, scale, 4).is_err());
    }
}
