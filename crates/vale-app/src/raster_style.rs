//! The colors of a raster layer in the style of the layer. The style is the
//! stored truth. The raster record holds a copy of the colors for the editors
//! and the globe. The record of an image holds a copy of its opacity and its
//! blend mode in the same way.

use vale_store::{ImageId, LayerId, MapId, Project, Raster, RasterDisplay, RasterId};
use vale_style::{ImageSymbolizer, LayerStyle, RasterSymbolizer, Symbolizer, style_version};

use crate::document::Document;
use crate::globe::{Look, RasterLook};
use crate::session::{bands_of, record_stops};

/// The colors of a raster record.
pub fn symbolizer_of(r: &Raster) -> RasterSymbolizer {
    RasterSymbolizer {
        display: r.display,
        opacity: r.opacity,
        blend: r.blend,
        stops: r.stops.clone(),
        colors: r.colors.clone(),
        lines: r.lines,
        smoothing: r.smoothing,
        hillshade: r.hillshade,
    }
}

/// Puts the style of a raster into its record. `all` false keeps the band
/// lines, the smoothing, and the hillshade of the record, because a style
/// of version 2 has none.
pub fn apply_symbolizer(record: &mut Raster, s: &RasterSymbolizer, all: bool) {
    record.display = s.display;
    record.opacity = s.opacity;
    record.blend = s.blend;
    record.stops.clone_from(&s.stops);
    record.colors.clone_from(&s.colors);
    if all {
        record.lines = s.lines;
        record.smoothing = s.smoothing;
        record.hillshade = s.hillshade;
    }
}

/// Each raster layer of the project, with its raster.
fn raster_layers(p: &Project) -> Vec<(LayerId, RasterId)> {
    let layers = p.layers().iter();
    layers.filter_map(|l| Some((l.id, l.raster()?))).collect()
}

/// The style of a layer, if it parses.
fn parsed(p: &Project, id: LayerId) -> Option<LayerStyle> {
    let text = p.layer(id)?.style.as_deref()?;
    LayerStyle::from_json(text).ok()
}

/// Puts the colors of each raster record into the style of its layer. A
/// record with no stops keeps the style. Returns `true` if a style changed.
pub fn store_raster_styles(p: &mut Project) -> bool {
    let mut changed = false;
    for (layer, raster) in raster_layers(p) {
        let Some(record) = p.raster(raster).filter(|r| !r.stops.is_empty()) else {
            continue;
        };
        let colors = symbolizer_of(record);
        let mut style = match parsed(p, layer) {
            Some(style) if style.raster_symbolizer() == Some(&colors) => continue,
            Some(style) => style,
            None => LayerStyle::raster(colors.clone()),
        };
        style.set_raster_symbolizer(colors);
        let json = style.to_json();
        if p.layer(layer).and_then(|l| l.style.as_deref()) != Some(json.as_str()) {
            changed |= p.set_layer_style(layer, Some(json));
        }
    }
    changed
}

/// Puts the style of each raster layer into its record.
pub fn load_raster_styles(p: &mut Project) {
    for (layer, raster) in raster_layers(p) {
        let Some(style) = parsed(p, layer) else {
            continue;
        };
        let text = p.layer(layer).and_then(|l| l.style.as_deref());
        let all = text.and_then(style_version).is_some_and(|v| v >= 3);
        let (Some(colors), Some(record)) = (style.raster_symbolizer(), p.raster_mut(raster)) else {
            continue;
        };
        apply_symbolizer(record, colors, all);
    }
}

/// Gives each raster layer with no colors in its style the colors of its
/// record. A record with no stops, or with stops that give no bands, gets
/// the default bands of its range, so the globe draws it as before. Returns
/// `true` if the project changed. The records must have the colors of the
/// styles, because each record goes into its style.
pub fn migrate(p: &mut Project) -> bool {
    let mut changed = false;
    for (layer, raster) in raster_layers(p) {
        let styled = parsed(p, layer).is_some_and(|s| s.raster_symbolizer().is_some());
        let Some(record) = p.raster_mut(raster).filter(|_| !styled) else {
            continue;
        };
        let bands = bands_of(record);
        let stops = record_stops(bands.stops());
        if record.stops != stops || record.colors != bands.colors() {
            record.stops = stops;
            record.colors = bands.colors().to_vec();
            changed = true;
        }
    }
    store_raster_styles(p) || changed
}

/// Each image layer of the project, with its image.
fn image_layers(p: &Project) -> Vec<(LayerId, ImageId)> {
    let layers = p.layers().iter();
    layers.filter_map(|l| Some((l.id, l.image()?))).collect()
}

/// Puts the opacity and the blend mode of each image record into the style
/// of its layer. Returns `true` if a style changed.
pub fn store_image_styles(p: &mut Project) -> bool {
    let mut changed = false;
    for (layer, image) in image_layers(p) {
        let Some(record) = p.image(image) else {
            continue;
        };
        let look = ImageSymbolizer {
            opacity: record.opacity,
            blend: record.blend,
        };
        let mut style = match parsed(p, layer) {
            Some(style) if style.image_symbolizer() == Some(&look) => continue,
            Some(style) if style.image_symbolizer().is_some() => style,
            _ => LayerStyle::image(),
        };
        let symbolizers = style
            .style
            .rules
            .iter_mut()
            .flat_map(|r| &mut r.symbolizers);
        for symbolizer in symbolizers {
            if let Symbolizer::Image(first) = symbolizer {
                *first = look;
                break;
            }
        }
        let json = style.to_json();
        if p.layer(layer).and_then(|l| l.style.as_deref()) != Some(json.as_str()) {
            changed |= p.set_layer_style(layer, Some(json));
        }
    }
    changed
}

/// Puts the opacity and the blend mode of the style of each image layer
/// into its record.
pub fn load_image_styles(p: &mut Project) {
    for (layer, image) in image_layers(p) {
        let Some(style) = parsed(p, layer) else {
            continue;
        };
        let Some(&look) = style.image_symbolizer() else {
            continue;
        };
        p.set_image_opacity(image, look.opacity);
        p.set_image_blend(image, look.blend);
    }
}

/// The raster looks and the image looks that a map overrides.
pub type MapLooks = (Vec<(RasterId, RasterLook)>, Vec<(ImageId, Look)>);

/// The looks of the raster layers and the image layers whose style a map
/// overrides.
pub fn map_looks(doc: &Document, map: MapId) -> MapLooks {
    let (mut rasters, mut images) = (Vec::new(), Vec::new());
    let Some(record) = doc.project.atlas.map(map) else {
        return (rasters, images);
    };
    for &layer in record.overrides.keys() {
        let (Some(l), Some(style)) = (doc.project.layer(layer), doc.map_style(map, layer)) else {
            continue;
        };
        if let (Some(raster), Some(s)) = (l.raster(), style.raster_symbolizer()) {
            let Some(base) = doc.project.raster(raster) else {
                continue;
            };
            // The bands keep the range of the raster.
            let bands = (s.stops != base.stops || s.colors != base.colors).then(|| {
                let mut r = base.clone();
                r.stops.clone_from(&s.stops);
                r.colors.clone_from(&s.colors);
                bands_of(&r)
            });
            let mut hillshade = s.hillshade;
            hillshade.on &= crate::ui::is_height(base);
            let look = RasterLook {
                look: Look {
                    opacity: s.opacity,
                    blend: s.blend,
                    display: s.display,
                },
                lines: s.lines,
                smoothing: s.smoothing,
                hillshade,
                bands,
            };
            rasters.push((raster, look));
        } else if let (Some(image), Some(s)) = (l.image(), style.image_symbolizer()) {
            let look = Look {
                opacity: s.opacity,
                blend: s.blend,
                display: RasterDisplay::default(),
            };
            images.push((image, look));
        }
    }
    (rasters, images)
}
