//! The Select tool of the Atlas workspace. It selects the features of the
//! selectable layers in the maps of the frames of the open page. The
//! selection is the selection of the World workspace.

use std::collections::HashSet;

use eframe::egui::{self, Pos2, Rect};
use vale_store::{Geometry, LayerId, PageRecord};

use super::shown::{Shown, each_frame};
use super::{AppState, AtlasSelection, AtlasTool, PagePlace, frame_at, open_page};
use crate::globe::math::lonlat_to_dir;
use crate::globe::select::{self, FeatureRef, Shape};
use crate::globe::vector;

/// The screen shape of a geometry in a frame, cut at the frame.
fn shape(shown: &Shown, geometry: &Geometry) -> Shape {
    let mut out = Shape::default();
    match geometry {
        Geometry::Points(points) => {
            for (i, p) in points.iter().enumerate() {
                out.points.extend(shown.at(*p).map(|pos| (i, pos)));
            }
        }
        Geometry::Lines(lines) => {
            for line in lines {
                out.lines.extend(shown.lines(line));
                let ends = line
                    .first()
                    .into_iter()
                    .chain(line.last().filter(|_| line.len() > 1));
                out.ends.extend(ends.filter_map(|p| shown.at(*p)));
            }
        }
        Geometry::Polygons(polygons) => {
            for polygon in polygons {
                out.lines.extend(shown.outline(polygon));
                let rings: Vec<_> = polygon
                    .iter()
                    .enumerate()
                    .map(|(i, ring)| vector::ring(ring, i > 0))
                    .filter(|r| !r.is_empty())
                    .collect();
                if !rings.is_empty() {
                    out.polygons.push(rings);
                }
            }
        }
    }
    out
}

/// Calls `f` with each frame of the page, and with the shapes in that frame
/// of the features of `layers` that its map shows and that `keep` keeps.
/// The layers are in the draw order, top first, and so are the shapes.
fn each_shapes(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    layers: &[LayerId],
    keep: impl Fn(FeatureRef) -> bool,
    mut f: impl FnMut(&Shown, Vec<(FeatureRef, Shape)>),
) {
    let mut features = Vec::new();
    for &layer in layers {
        let Some(record) = state.doc.layer(layer) else {
            continue;
        };
        for feature in &record.features {
            let found = FeatureRef {
                layer,
                uuid: feature.uuid(),
            };
            if keep(found) {
                features.push((found, feature.geometry.clone()));
            }
        }
    }
    if features.is_empty() {
        return;
    }
    let shown: HashSet<_> = (page.frames.iter())
        .flat_map(|frame| layers.iter().map(move |&layer| (frame.map, layer)))
        .filter(|&(map, layer)| state.doc.shown(map, layer))
        .collect();
    each_frame(state, page, place, |frame| {
        let map = frame.frame.map;
        let shapes: Vec<_> = (features.iter())
            .filter(|(r, _)| shown.contains(&(map, r.layer)))
            .map(|(r, g)| (*r, shape(frame, g)))
            .collect();
        if !shapes.is_empty() {
            f(frame, shapes);
        }
    });
}

fn dir_at(shown: &Shown, pos: Pos2) -> Option<crate::globe::math::V3> {
    shown.place_at(pos).map(|p| lonlat_to_dir(p[0], p[1]))
}

/// A tap of the Select tool. A tap in a frame selects the frame and acts
/// on the feature under the tap in the map of the frame, as a tap on the
/// globe does. With `add`, it adds the feature or takes it out. A tap
/// outside the frames clears the selection, and with `add` it does
/// nothing.
pub(crate) fn tap(state: &mut AppState, page: &PageRecord, place: PagePlace, pos: Pos2, add: bool) {
    let Some(id) = frame_at(page, place.to_mm(pos)) else {
        if !add {
            state.atlas.selection = AtlasSelection::None;
            state.set_feature_selection(Vec::new());
        }
        return;
    };
    state.atlas.selection = AtlasSelection::Frame(id);
    let reach = state.atlas.pointer.reach();
    let mut hit = None;
    let layers = state.selectable_layers();
    each_shapes(
        state,
        page,
        place,
        &layers,
        |_| true,
        |shown, shapes| {
            if shown.frame.id == id {
                hit = select::hit_shapes(&shapes, pos, dir_at(shown, pos), reach);
            }
        },
    );
    state.select_tap(hit, add);
}

/// A box of the Select tool from `start`. It selects the features that the
/// box meets in each frame, and with `add` it adds them to the selection.
/// The frame under `start` becomes the selected frame.
pub(crate) fn select_box(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    start: Pos2,
    rect: Rect,
    add: bool,
) {
    let center = rect.center();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let layers = state.selectable_layers();
    each_shapes(
        state,
        page,
        place,
        &layers,
        |_| true,
        |shown, shapes| {
            let met = select::box_shapes(&shapes, rect, dir_at(shown, center));
            found.extend(met.into_iter().filter(|f| seen.insert(*f)));
        },
    );
    if let Some(id) = frame_at(page, place.to_mm(start)) {
        state.atlas.selection = AtlasSelection::Frame(id);
    }
    if add {
        let mut all = state.feature_selection.clone();
        found.retain(|f| !all.contains(f));
        all.extend(found);
        found = all;
    }
    state.set_feature_selection(found);
}

/// The selection on the screen, in each frame whose map shows its layer,
/// cut at the frames.
pub fn highlight(state: &mut AppState, place: PagePlace) -> Shape {
    let selected: HashSet<FeatureRef> = state.feature_selection.iter().copied().collect();
    highlight_of(state, place, selected)
}

/// The shapes of `selected` on the screen, cut at the frames.
fn highlight_of(state: &mut AppState, place: PagePlace, selected: HashSet<FeatureRef>) -> Shape {
    let mut out = Shape::default();
    if selected.is_empty() {
        return out;
    }
    let page = open_page(state).clone();
    let layers: Vec<LayerId> = (state.selection_counts().into_iter())
        .map(|(id, _)| id)
        .collect();
    let keep = |f| selected.contains(&f);
    each_shapes(state, &page, place, &layers, keep, |_, shapes| {
        for (_, shape) in shapes {
            out.points.extend(shape.points);
            out.lines.extend(shape.lines);
            out.ends.extend(shape.ends);
        }
    });
    out
}

/// Draws the selection with the Select tool.
pub(crate) fn paint(ui: &egui::Ui, state: &mut AppState, place: PagePlace, area: Rect) {
    if state.atlas.tool != AtlasTool::Select {
        return;
    }
    let painter = ui.painter_at(area);
    let key = state.key_feature().into_iter().collect();
    crate::ui::select::paint_key(&painter, &highlight_of(state, place, key));
    let shape = highlight(state, place);
    crate::ui::select::paint_shape(&painter, shape);
}
