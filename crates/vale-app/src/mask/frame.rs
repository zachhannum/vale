//! The raster masks of the layers in the map frames.

use vale_store::{FrameRecord, LayerId};
use vale_style::Mask;

use crate::atlas::FrameTarget;
use crate::document::Document;
use crate::globe::RasterMask;
use crate::mask::PageMask;

/// The raster mask of each layer that the map of `frame` draws with a
/// raster mask in its style in that map.
pub fn frame_raster_masks(doc: &Document, frame: &FrameRecord) -> Vec<(LayerId, RasterMask)> {
    (doc.frame.entries.iter())
        .filter(|entry| doc.shown(frame.map, entry.layer))
        .filter_map(|entry| {
            let style = doc.map_entry(Some(frame.map), entry.layer)?;
            let Mask::Raster { layer, min, max } = style.style.style.mask? else {
                return None;
            };
            let raster = doc.project.layer(layer)?.raster()?;
            let mask = RasterMask {
                raster,
                min,
                max,
                smoothing: doc.project.raster(raster)?.smoothing,
            };
            Some((entry.layer, mask))
        })
        .collect()
}

/// The page mask of the cover of a raster mask, from
/// `Globe::capture_frame_mask`: one byte for each pixel, row by row. The
/// outline is where the cover is one half. `at` maps page millimeters to
/// the pixels.
pub fn cover_mask(cover: &[u8], width: usize, height: usize, at: FrameTarget) -> PageMask {
    let grid: Vec<f32> = cover.iter().map(|&c| f32::from(c) / 255.0).collect();
    PageMask::from_grid(&grid, width, height, 0.5, f32::INFINITY, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Point;

    #[test]
    fn the_outline_of_a_cover_is_where_it_is_one_half() {
        // 4 × 1 pixels, with the edge in the middle of the second pixel.
        let cover = [255, 128, 0, 0];
        let mask = cover_mask(&cover, 4, 1, FrameTarget::MM);
        assert_eq!(mask.rings().len(), 1);
        let right = (mask.rings()[0].iter()).fold(f64::NEG_INFINITY, |m, p| m.max(p.x));
        assert!((right - 1.5).abs() < 0.01, "{right}");
        let top = (mask.rings()[0].iter()).fold(f64::INFINITY, |m, p: &Point| m.min(p.y));
        assert!(top >= 0.0, "{top}");
    }
}
