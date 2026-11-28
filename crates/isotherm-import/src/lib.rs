//! The import of heightmaps into an Isotherm world.

use isotherm_world::Heightmap;
use vale_terrain::{Equirect, meters_to_level};

pub use vale_import::ImportError;

/// The PNG level of sea level. A level minus this offset is the height in meters.
pub const PNG_SEA_LEVEL: i32 = 32768;

/// Reads a 16-bit grey equirectangular PNG into a cube map with `face_size`
/// texels on each side. A level minus 32,768 is the height in meters. Heights
/// outside the range of the heightmap get the nearest end.
pub fn heightmap_png(bytes: &[u8], face_size: usize) -> Result<Heightmap, ImportError> {
    let name = "heightmap";
    let image = vale_import::raster::read_bytes(name, bytes)?;
    if image.bits != 16 {
        return Err(ImportError::Parse {
            name: name.to_string(),
            message: format!("the image has {}-bit levels, not 16-bit", image.bits),
        });
    }
    let levels = image
        .levels
        .iter()
        .map(|&level| meters_to_level(f64::from(i32::from(level) - PNG_SEA_LEVEL)))
        .collect();
    let src = Equirect::new(image.width, image.height, levels).ok_or(ImportError::Empty {
        name: name.to_string(),
    })?;
    let mut heights = Heightmap::new(face_size, meters_to_level(0.0));
    heights.import_equirect(&src.reduced_for(face_size));
    Ok(heights)
}
