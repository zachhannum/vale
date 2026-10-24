//! The colors of a raster layer in the style of the layer. The style is the
//! stored truth. The raster record holds a copy of the colors for the editors
//! and the globe.

use vale_store::{LayerId, Project, Raster, RasterId};
use vale_style::{LayerStyle, RasterSymbolizer};

use crate::session::{bands_of, record_stops};

/// The colors of a raster record.
pub fn symbolizer_of(r: &Raster) -> RasterSymbolizer {
    RasterSymbolizer {
        display: r.display,
        opacity: r.opacity,
        stops: r.stops.clone(),
        colors: r.colors.clone(),
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

/// Puts the colors of the style of each raster layer into its record.
pub fn load_raster_styles(p: &mut Project) {
    for (layer, raster) in raster_layers(p) {
        let Some(style) = parsed(p, layer) else {
            continue;
        };
        let (Some(colors), Some(record)) = (style.raster_symbolizer(), p.raster_mut(raster)) else {
            continue;
        };
        record.display = colors.display;
        record.opacity = colors.opacity;
        record.stops.clone_from(&colors.stops);
        record.colors.clone_from(&colors.colors);
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
