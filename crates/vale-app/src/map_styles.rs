//! The styles of the layers in the maps and in the map styles.

use vale_store::{LayerId, Project};
use vale_style::{LayerStyle, Overrides};

/// Gives each map with no map style and each map style a style for each
/// layer that has a style in the project. Returns `true` if the project
/// changed.
pub fn fill(p: &mut Project) -> bool {
    let layers = needed(p);
    let globe: Vec<_> = (layers.into_iter())
        .filter_map(|id| Some((id, p.layer(id)?.style.clone()?)))
        .collect();
    fill_with(p, &globe)
}

/// The layers that `fill_with` needs the globe style of: each layer that
/// has no style in a map with no map style or in a map style, and each
/// layer with style overrides of an older file.
pub fn needed(p: &Project) -> Vec<LayerId> {
    let atlas = &p.atlas;
    let maps = atlas.maps.iter().filter(|m| m.style.is_none());
    let sets: Vec<_> = (maps.map(|m| &m.styles))
        .chain(atlas.map_styles.iter().map(|s| &s.styles))
        .collect();
    let legacy = atlas.maps.iter().flat_map(|m| m.legacy_overrides.keys());
    let mut out: Vec<LayerId> = legacy.copied().collect();
    let missing = (p.layers().iter()).filter(|l| sets.iter().any(|s| !s.contains_key(&l.id)));
    out.extend(missing.map(|l| l.id));
    out.sort();
    out.dedup();
    out
}

/// Gives each map with no map style and each map style a copy of the globe
/// style of each layer in `globe` that it has no style for. The style
/// overrides of an older file go first: a map gets the globe style with
/// its overrides, and the overrides go. `globe` holds the globe styles of
/// the layers that `needed` gives. Returns `true` if the project changed.
pub fn fill_with(p: &mut Project, globe: &[(LayerId, String)]) -> bool {
    let mut changed = false;
    for map in &mut p.atlas.maps {
        for (layer, text) in std::mem::take(&mut map.legacy_overrides) {
            changed = true;
            let base = globe.iter().find(|(id, _)| *id == layer);
            let Some(base) = base.and_then(|(_, t)| LayerStyle::from_json(t).ok()) else {
                continue;
            };
            let style = base.with_overrides(&Overrides::from_json(&text));
            map.styles.entry(layer).or_insert_with(|| style.to_json());
        }
    }
    let maps = p.atlas.maps.iter_mut().filter(|m| m.style.is_none());
    let sets = maps
        .map(|m| &mut m.styles)
        .chain(p.atlas.map_styles.iter_mut().map(|s| &mut s.styles));
    for styles in sets {
        for (layer, text) in globe {
            if !styles.contains_key(layer) {
                styles.insert(*layer, text.clone());
                changed = true;
            }
        }
    }
    changed
}
