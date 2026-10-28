//! The image layers on the globe. Each visible image has a copy on the GPU:
//! one level of its pyramid, with the next levels as the mip chain. The
//! tiles come from the app one by one, so the main memory never holds a
//! whole level.

use vale_store::{IMAGE_TILE_SIZE, Image, ImageId, level_size};

use super::gpu::{ImageKey, ImageUpload};
use super::{Globe, StackItem};
use crate::image_layer::premultiply;

/// The most tiles that wait for the GPU. 32 tiles are 8 MB.
const QUEUE_TILES: usize = 32;

/// An image of the project.
pub(super) struct ImageLayer {
    id: ImageId,
    /// The size of level 0, in pixels.
    width: u32,
    height: u32,
    levels: u32,
    /// The copy on the GPU. `None`: the image is not visible.
    copy: Option<ImageCopy>,
}

struct ImageCopy {
    key: ImageKey,
    /// The count of the tiles that the copy got.
    next: usize,
    total: usize,
}

impl ImageCopy {
    fn loaded(&self) -> bool {
        self.next >= self.total
    }
}

/// The level of the pyramid of the copy of an image: the first level that
/// is at most four times `cap` wide and at most `limit` on each side.
pub fn display_level(width: u32, height: u32, levels: u32, cap: u32, limit: u32) -> u32 {
    let most = cap.saturating_mul(4).min(limit);
    let fits = |level: &u32| {
        let (w, h) = level_size(width, height, *level);
        w <= most && h <= limit
    };
    (0..levels).find(fits).unwrap_or(levels.saturating_sub(1))
}

/// The number of columns and of rows of the tiles of one level.
fn tile_grid(width: u32, height: u32, level: u32) -> (u32, u32) {
    let (w, h) = level_size(width, height, level);
    (w.div_ceil(IMAGE_TILE_SIZE), h.div_ceil(IMAGE_TILE_SIZE))
}

impl ImageLayer {
    /// The mip, the level, the column, and the row of the tile with this
    /// number in the copy. The tiles go mip by mip, row by row.
    fn tile_at(&self, key: &ImageKey, mut index: usize) -> Option<(u32, u32, u32, u32)> {
        for mip in 0..key.mips {
            let level = key.level + mip;
            let (columns, rows) = tile_grid(self.width, self.height, level);
            let count = (columns * rows) as usize;
            if index < count {
                let (row, column) = (index as u32 / columns, index as u32 % columns);
                return Some((mip, level, column, row));
            }
            index -= count;
        }
        None
    }
}

impl Globe {
    /// Makes the images of the globe those of the project. The globe draws
    /// an image while `set_stack` lists it.
    pub fn set_images(&mut self, images: &[Image]) {
        let same = |a: &ImageLayer, b: &Image| {
            (a.id, a.width, a.height, a.levels) == (b.id, b.width, b.height, b.levels)
        };
        self.images
            .retain(|layer| images.iter().any(|image| same(layer, image)));
        for image in images {
            if !self.images.iter().any(|layer| same(layer, image)) {
                self.images.push(ImageLayer {
                    id: image.id,
                    width: image.width,
                    height: image.height,
                    levels: image.levels.max(1),
                    copy: None,
                });
            }
        }
        self.sync_images();
    }

    /// True if the globe has the image.
    pub fn holds_image(&self, id: ImageId) -> bool {
        self.images.iter().any(|layer| layer.id == id)
    }

    /// True if the list of `set_stack` has the image.
    fn image_visible(&self, id: ImageId) -> bool {
        let stack = self.stack.iter().flatten();
        stack
            .into_iter()
            .any(|item| matches!(item, StackItem::Image { id: of, .. } if *of == id))
    }

    /// Gives each visible image a copy at the level of the cap, and takes
    /// the copy from each other image. With no renderer, no image has a
    /// copy.
    pub(super) fn sync_images(&mut self) {
        let (cap, limit) = (
            self.display_cap,
            self.max_face_size.min(u32::MAX as usize) as u32,
        );
        for at in 0..self.images.len() {
            let layer = &self.images[at];
            let shown = self.format.is_some() && self.image_visible(layer.id);
            let level =
                shown.then(|| display_level(layer.width, layer.height, layer.levels, cap, limit));
            let old = layer.copy.as_ref().map(|copy| copy.key.level);
            if old == level {
                continue;
            }
            let copy = match level {
                Some(level) => {
                    self.generations += 1;
                    let (width, height) = level_size(layer.width, layer.height, level);
                    // A texture has no mip past the size of 1 pixel.
                    let most = 32 - width.max(height).leading_zeros();
                    let mips = (layer.levels - level).min(most).max(1);
                    let key = ImageKey {
                        image: layer.id,
                        generation: self.generations,
                        level,
                        width,
                        height,
                        mips,
                    };
                    let total = (0..mips)
                        .map(|mip| {
                            let (columns, rows) = tile_grid(layer.width, layer.height, level + mip);
                            (columns * rows) as usize
                        })
                        .sum();
                    Some(ImageCopy {
                        key,
                        next: 0,
                        total,
                    })
                }
                None => None,
            };
            self.images[at].copy = copy;
        }
    }

    /// The next tile that a copy of an image waits for: the image, the
    /// level, the column, and the row. `None`: each copy has its tiles, or
    /// enough tiles wait for the GPU.
    pub fn image_request(&self) -> Option<(ImageId, u32, u32, u32)> {
        if self.link.image_queue() >= QUEUE_TILES {
            return None;
        }
        self.images.iter().find_map(|layer| {
            let copy = layer.copy.as_ref().filter(|copy| !copy.loaded())?;
            let (_, level, column, row) = layer.tile_at(&copy.key, copy.next)?;
            Some((layer.id, level, column, row))
        })
    }

    /// Gives the copy of an image the tile of `image_request`, as its
    /// width, its height, and its straight RGBA8 pixels. `None`: the tile
    /// has no pixels, and the copy keeps it transparent.
    pub fn give_image_tile(&mut self, id: ImageId, tile: Option<(u32, u32, Vec<u8>)>) {
        let Some(layer) = self.images.iter_mut().find(|layer| layer.id == id) else {
            return;
        };
        let Some(copy) = layer.copy.as_ref().filter(|copy| !copy.loaded()) else {
            return;
        };
        let key = copy.key;
        let at = layer.tile_at(&key, copy.next);
        if let (Some((mip, _, column, row)), Some((width, height, mut rgba))) = (at, tile) {
            premultiply(&mut rgba);
            self.link.push_image(ImageUpload {
                key,
                mip,
                x: column * IMAGE_TILE_SIZE,
                y: row * IMAGE_TILE_SIZE,
                width,
                height,
                rgba,
            });
        }
        if let Some(copy) = &mut layer.copy {
            copy.next += 1;
        }
    }

    /// True while a visible image waits for the tiles of its copy.
    pub(super) fn images_loading(&self) -> bool {
        let copies = self.images.iter().filter_map(|layer| layer.copy.as_ref());
        copies.into_iter().any(|copy| !copy.loaded())
    }

    /// The memory of the copies of the images on the GPU, in bytes.
    pub fn image_bytes(&self) -> u64 {
        let copies = self.images.iter().filter_map(|layer| layer.copy.as_ref());
        copies.map(|copy| copy.key.bytes()).sum()
    }

    /// The copy of each image on the GPU.
    pub(super) fn image_keys(&self) -> Vec<ImageKey> {
        let copies = self.images.iter().filter_map(|layer| layer.copy.as_ref());
        copies.map(|copy| copy.key).collect()
    }

    /// The copy that the globe draws an image from. `None`: the copy does
    /// not have each tile.
    pub(super) fn image_source(&self, id: ImageId) -> Option<ImageKey> {
        let layer = self.images.iter().find(|layer| layer.id == id)?;
        let copy = layer.copy.as_ref().filter(|copy| copy.loaded())?;
        Some(copy.key)
    }

    /// The level of the pyramid of the copy of an image on the GPU. `None`:
    /// the image is not visible.
    pub fn image_level(&self, id: ImageId) -> Option<u32> {
        let layer = self.images.iter().find(|layer| layer.id == id)?;
        Some(layer.copy.as_ref()?.key.level)
    }

    /// Takes the copy from each image, so each one gets a new copy.
    pub(super) fn reset_images(&mut self) {
        for layer in &mut self.images {
            layer.copy = None;
        }
        self.sync_images();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_of_a_large_image_is_four_times_the_cap_wide() {
        let levels = vale_store::image_levels(16384, 8192);
        assert_eq!(display_level(16384, 8192, levels, 2048, 16384), 1);
        assert_eq!(display_level(16384, 8192, levels, 1024, 16384), 2);
        // The device limit counts too.
        assert_eq!(display_level(16384, 8192, levels, 2048, 4096), 2);
        assert_eq!(display_level(300, 100, 2, 2048, 16384), 0);
    }
}
