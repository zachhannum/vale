//! The map frames that the renderer draws, and the looks of their layers.

use std::sync::Arc;

use vale_store::{BlendMode, Hillshade, ImageId, RasterDisplay, RasterId, Smoothing};
use vale_terrain::Bands;

use crate::flat;
use crate::gpu::Mask;

/// Where a map frame draws the flat map in its target of pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePlace {
    /// The pixel of the target that shows the center of the map.
    pub center: [f32; 2],
    /// The center of the map, in units of the projection of radius 1.
    pub map_center: [f32; 2],
    /// Pixels in one unit of the projection.
    pub units_to_pixels: f32,
    /// The turn of the map, in radians clockwise.
    pub rotation: f64,
    /// Points of the page in one radian of the world.
    pub points_per_radian: f64,
    /// Pixels in one point of the page.
    pub pixels_per_page_point: f32,
}

/// The most runs of raster layers and image layers in a map frame. Each run
/// has a target of its own.
pub const MAX_RUNS: usize = 64;

/// A map frame to draw.
#[derive(Clone)]
pub struct FrameDraw {
    pub id: u32,
    pub place: FramePlace,
    /// The left, the top, the width, and the height of the target on the
    /// screen, in pixels.
    pub rect: [u32; 4],
    pub mesh: Arc<flat::FlatMesh>,
    /// A shape that cuts the layers. The mask has the size of the target.
    pub mask: Option<Arc<Mask>>,
    /// The layers that the map draws, from the bottom.
    pub layers: Vec<FrameItem>,
    /// The looks in the map of raster layers that the map does not draw.
    /// The raster masks read them.
    pub mask_looks: Vec<(RasterId, RasterLook)>,
}

impl FrameDraw {
    /// The raster layers that the frame draws, from the bottom, each with
    /// its look.
    pub fn rasters(&self) -> impl Iterator<Item = (RasterId, &RasterLook)> {
        self.layers.iter().filter_map(|item| match item {
            FrameItem::Raster(id, look) => Some((*id, look)),
            _ => None,
        })
    }

    /// The image layers that the frame draws, from the bottom.
    pub fn images(&self) -> impl Iterator<Item = ImageId> {
        self.layers.iter().filter_map(|item| match item {
            FrameItem::Image(id, _) => Some(*id),
            _ => None,
        })
    }

    /// The runs of raster layers and image layers between the runs of
    /// vector layers, from the bottom. The runs above the first `MAX_RUNS`
    /// join the last run.
    pub fn runs(&self) -> Vec<Vec<&FrameItem>> {
        let mut runs: Vec<Vec<&FrameItem>> = Vec::new();
        let split = self.layers.split(|item| *item == FrameItem::Cut);
        for run in split.filter(|run| !run.is_empty()) {
            match runs.len() {
                MAX_RUNS => runs[MAX_RUNS - 1].extend(run),
                _ => runs.push(run.iter().collect()),
            }
        }
        runs
    }

    /// The look of a raster layer that the frame draws.
    pub fn raster_look(&self, id: RasterId) -> Option<&RasterLook> {
        self.rasters()
            .find(|(of, _)| *of == id)
            .map(|(_, look)| look)
    }
}

/// One item of the layers of a map frame, with its look in the map.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameItem {
    Raster(RasterId, RasterLook),
    Image(ImageId, Look),
    /// The place of a run of vector layers.
    Cut,
}

/// The look of a raster layer in a map.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterLook {
    pub look: Look,
    pub lines: bool,
    pub smoothing: Smoothing,
    pub hillshade: Hillshade,
    /// The bands of the map. `None`: the bands of the layer.
    pub bands: Option<Bands>,
}

/// The look of a raster layer or an image layer in a map. An image layer
/// has no display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub opacity: f32,
    pub blend: BlendMode,
    pub display: RasterDisplay,
}
