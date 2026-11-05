//! The regions of the style masks, and the parts of the features inside
//! them.

pub mod frame;
mod page;
pub mod trace;

pub use page::PageMask;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use vale_store::{Geometry, LayerId, LonLat};
use vale_style::Mask;

use crate::document::Document;
use crate::globe::mask::{Region, clip_lines, clip_polygons, polygon_outlines};

/// Polygons in longitude and latitude.
pub type Polygons = Vec<Vec<Vec<LonLat>>>;

static GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// The part of a feature inside the mask of its layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaskedFeature {
    /// The part of a polygon feature: the fill and the label.
    pub polygons: Polygons,
    /// The part of a line feature, or of the outline of a polygon feature:
    /// the stroke. The edge of the mask is no part of it.
    pub lines: Vec<Vec<LonLat>>,
}

/// The parts of the features of a layer inside its mask.
#[derive(Debug)]
pub struct MaskedLayer {
    /// One for each feature of the layer. `None` for a point feature.
    pub features: Vec<Option<MaskedFeature>>,
    /// A new number for each new cut.
    pub generation: u64,
}

/// What a cut comes from.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    mask: Mask,
    features: usize,
    mask_features: usize,
}

/// The cuts of the masked layers.
#[derive(Default)]
pub struct MaskCache {
    entries: HashMap<LayerId, (Key, Arc<MaskedLayer>)>,
}

impl MaskCache {
    /// The parts of the features of layer `id` inside its mask. `None`: the
    /// layer has no mask or a raster mask, so the layer draws whole.
    pub fn layer(&mut self, doc: &Document, id: LayerId) -> Option<Arc<MaskedLayer>> {
        let Some(key) = key(doc, id) else {
            self.entries.remove(&id);
            return None;
        };
        if let Some((old, masked)) = self.entries.get(&id)
            && *old == key
        {
            return Some(masked.clone());
        }
        let layer = doc.layer(id)?;
        let region = Region::new(&region_polygons(doc, &key.mask)?);
        let features = layer
            .features
            .iter()
            .map(|f| match &f.geometry {
                Geometry::Points(_) => None,
                Geometry::Lines(lines) => Some(MaskedFeature {
                    polygons: Vec::new(),
                    lines: clip_lines(&region, lines),
                }),
                Geometry::Polygons(polygons) => Some(MaskedFeature {
                    polygons: clip_polygons(&region, polygons),
                    lines: clip_lines(&region, &polygon_outlines(polygons)),
                }),
            })
            .collect();
        let masked = Arc::new(MaskedLayer {
            features,
            generation: next_generation(),
        });
        self.entries.insert(id, (key, masked.clone()));
        Some(masked)
    }

    /// Cuts layer `id` again, and each layer that `id` masks.
    pub fn forget(&mut self, id: LayerId) {
        self.entries
            .retain(|layer, (key, _)| *layer != id && key.mask.layer() != id);
    }

    /// Keeps only the cuts of the layers that `keep` gives.
    pub fn retain(&mut self, keep: impl Fn(LayerId) -> bool) {
        self.entries.retain(|id, _| keep(*id));
    }
}

/// What the cut of layer `id` comes from. `None`: the layer draws whole.
fn key(doc: &Document, id: LayerId) -> Option<Key> {
    let mask = doc.entry(id)?.style.style.mask?;
    let layer = doc.layer(id)?;
    layer.geometry()?;
    let mask_features = match mask {
        Mask::Polygons { layer: of } => doc.layer(of)?.features.len(),
        Mask::Raster { .. } => return None,
    };
    Some(Key {
        mask,
        features: layer.features.len(),
        mask_features,
    })
}

/// The polygons of the region of a polygon mask.
fn region_polygons(doc: &Document, mask: &Mask) -> Option<Polygons> {
    match *mask {
        Mask::Polygons { layer } => Some(
            (doc.layer(layer)?.features.iter())
                .filter_map(|f| match &f.geometry {
                    Geometry::Polygons(p) => Some(p.iter().cloned()),
                    _ => None,
                })
                .flatten()
                .collect(),
        ),
        Mask::Raster { .. } => None,
    }
}
