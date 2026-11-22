//! The colors of a raster layer in the style of the layer. The style is the
//! stored truth. The raster record holds a copy of the colors for the editors
//! and the globe. The record of an image holds a copy of its opacity and its
//! blend mode in the same way.

use vale_store::{ImageId, LayerId, LayerKind, MapId, Project, Raster, RasterDisplay, RasterId};
use vale_style::{ImageSymbolizer, LayerStyle, RasterSymbolizer, Symbolizer, style_version};

use crate::document::Document;
use crate::globe::{FrameItem, Look, MAX_RUNS, RasterLook};
use crate::session::{bands_of, put_bands, record_stops};

/// The colors of a raster record.
pub fn symbolizer_of(r: &Raster) -> RasterSymbolizer {
    RasterSymbolizer {
        display: r.display,
        opacity: r.opacity,
        blend: r.blend,
        stops: r.stops.clone(),
        colors: r.colors.clone(),
        names: r.names.clone(),
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
    record.names.clone_from(&s.names);
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
        if record.stops != stops || record.colors != bands.colors() || record.names != bands.names()
        {
            put_bands(record, &bands);
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

/// The look of a raster layer in a map: its style in the map, or else its
/// record. `bands` is `None` with the colors of the record.
fn raster_look(doc: &Document, map: MapId, layer: LayerId, raster: RasterId) -> Option<RasterLook> {
    let base = doc.project.raster(raster)?;
    let style = doc.map_style(map, layer);
    let own = style.as_ref().and_then(|s| s.raster_symbolizer());
    let record = symbolizer_of(base);
    let s = own.unwrap_or(&record);
    // The bands keep the range of the raster.
    let bands = (s.stops != base.stops || s.colors != base.colors).then(|| {
        let mut r = base.clone();
        r.stops.clone_from(&s.stops);
        r.colors.clone_from(&s.colors);
        r.names.clone_from(&s.names);
        bands_of(&r)
    });
    let mut hillshade = s.hillshade;
    hillshade.on &= crate::ui::is_height(base);
    Some(RasterLook {
        look: Look {
            opacity: s.opacity,
            blend: s.blend,
            display: s.display,
        },
        lines: s.lines,
        smoothing: s.smoothing,
        hillshade,
        bands,
    })
}

/// The look of an image layer in a map: its style in the map, or else its
/// record.
fn image_look(doc: &Document, map: MapId, layer: LayerId, image: ImageId) -> Option<Look> {
    let base = doc.project.image(image)?;
    let style = doc.map_style(map, layer);
    let (opacity, blend) = match style.as_ref().and_then(|s| s.image_symbolizer()) {
        Some(s) => (s.opacity, s.blend),
        None => (base.opacity, base.blend),
    };
    Some(Look {
        opacity,
        blend,
        display: RasterDisplay::default(),
    })
}

/// Each layer that a map draws, from the bottom, with its index in
/// `Document::map_layers`. A raster layer and an image layer have their
/// look in the map, and a vector layer is a `FrameItem::Cut`.
fn indexed_items(doc: &Document, map: MapId) -> Vec<(usize, FrameItem)> {
    let ids = doc.map_layers(map).into_iter().enumerate();
    ids.filter_map(|(index, id)| {
        let layer = doc.project.layer(id)?;
        let item = match layer.kind {
            LayerKind::Raster(r) => FrameItem::Raster(r, raster_look(doc, map, id, r)?),
            LayerKind::Image(i) => FrameItem::Image(i, image_look(doc, map, id, i)?),
            LayerKind::Vector(_) => FrameItem::Cut,
        };
        Some((index, item))
    })
    .collect()
}

/// The layers that a map draws, from the bottom, as `Document::map_layers`
/// gives them. Each raster layer and image layer has its look in the map.
/// Each run of vector layers is one `FrameItem::Cut`.
pub fn frame_items(doc: &Document, map: MapId) -> Vec<FrameItem> {
    let mut items: Vec<FrameItem> = Vec::new();
    for (_, item) in indexed_items(doc, map) {
        if item != FrameItem::Cut || items.last() != Some(&FrameItem::Cut) {
            items.push(item);
        }
    }
    items
}

/// The start of each run of raster layers and image layers of a map, as
/// an index in `Document::map_layers`, for at most `MAX_RUNS` runs. The
/// runs are the runs of `FrameDraw::runs` for `frame_items`.
pub fn frame_cuts(doc: &Document, map: MapId) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut picture = false;
    for (index, item) in indexed_items(doc, map) {
        let starts = item != FrameItem::Cut && !picture;
        if starts && cuts.len() < MAX_RUNS {
            cuts.push(index);
        }
        picture = item != FrameItem::Cut;
    }
    cuts
}

/// The looks in a map of the raster layers that the map does not draw.
pub fn hidden_looks(doc: &Document, map: MapId) -> Vec<(RasterId, RasterLook)> {
    let drawn = doc.map_layers(map);
    let hidden = (doc.project.layers().iter()).filter(|l| !drawn.contains(&l.id));
    hidden
        .filter_map(|l| Some((l.raster()?, l.id)))
        .filter_map(|(r, id)| Some((r, raster_look(doc, map, id, r)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use vale_store::GeometryKind;

    use super::*;

    #[test]
    fn the_frame_items_and_the_cuts_follow_the_map_and_fold_each_run_of_vector_layers() {
        let mut doc = Document::empty();
        let map = doc.project.atlas.add_map();
        let p = &mut doc.project;
        let vector = |p: &mut Project, name: &str| {
            p.add_layer(name.into(), GeometryKind::Line, None, Vec::new())
        };
        let (v0, v1, v2) = (vector(p, "v0"), vector(p, "v1"), vector(p, "v2"));
        let r0 = p.add_raster("r0".into(), 64, "m".into(), 0.0, 1.0);
        let r1 = p.add_raster("r1".into(), 64, "m".into(), 0.0, 1.0);
        let image = p.add_image("i".into(), 4, 4);
        let layer = |p: &Project, r| p.raster_layer(r).unwrap().id;
        let (l0, l1) = (layer(p, r0), layer(p, r1));
        let li = p
            .layers()
            .iter()
            .find(|l| l.image() == Some(image))
            .unwrap()
            .id;
        let a = p.add_collection("A").unwrap();
        let b = p.add_collection("B").unwrap();
        for l in [v0, v1, l0, v2] {
            assert!(p.set_in_collection(a, l, true));
        }
        for l in [li, v2, l1] {
            assert!(p.set_in_collection(b, l, true));
        }
        assert!(p.set_map_stack(map, vec![a, b]));

        let kinds = |doc: &Document| -> Vec<String> {
            (frame_items(doc, map).iter())
                .map(|item| match item {
                    FrameItem::Raster(r, _) => format!("r{}", r.0),
                    FrameItem::Image(..) => "i".into(),
                    FrameItem::Cut => "cut".into(),
                })
                .collect()
        };
        let (n0, n1) = (format!("r{}", r0.0), format!("r{}", r1.0));
        // v2 is in both collections, at its place in B.
        // The first layer of each run of raster layers and image layers.
        let starts = |doc: &Document| -> Vec<LayerId> {
            let ids = doc.map_layers(map);
            frame_cuts(doc, map).iter().map(|&i| ids[i]).collect()
        };
        assert_eq!(kinds(&doc), ["cut", n0.as_str(), "i", "cut", n1.as_str()]);
        assert_eq!(starts(&doc), [l0, l1]);
        assert!(doc.project.set_map_stack(map, vec![b, a]));
        assert_eq!(kinds(&doc), ["i", n1.as_str(), "cut", n0.as_str(), "cut"]);
        assert_eq!(starts(&doc), [li, l0]);
        assert!(doc.set_shown(map, l0, false));
        assert_eq!(kinds(&doc), ["i", n1.as_str(), "cut"]);
        assert_eq!(starts(&doc), [li]);
        assert_eq!(hidden_looks(&doc, map).first().map(|(r, _)| *r), Some(r0));
    }
}
