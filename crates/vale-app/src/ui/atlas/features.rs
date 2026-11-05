//! The Select tool of the Atlas workspace. It selects the features of the
//! selected vector layer in the maps of the frames of the open page. The
//! selection is the selection of the World workspace.

use std::collections::HashSet;

use eframe::egui::{self, Pos2, Rect};
use vale_store::{Geometry, PageRecord, Uuid};

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

/// Calls `f` with each frame of the page whose map shows the selected
/// vector layer, and with the shapes in that frame of the features of the
/// layer that `keep` keeps, top first.
fn each_shapes(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    keep: impl Fn(Uuid) -> bool,
    mut f: impl FnMut(&Shown, Vec<(FeatureRef, Shape)>),
) {
    let Some(layer) = state.selected_vector() else {
        return;
    };
    let Some(record) = state.doc.layer(layer) else {
        return;
    };
    let features: Vec<_> = (record.features.iter())
        .filter(|feature| keep(feature.uuid()))
        .map(|feature| {
            let found = FeatureRef {
                layer,
                uuid: feature.uuid(),
            };
            (found, feature.geometry.clone())
        })
        .collect();
    if features.is_empty() {
        return;
    }
    let maps: HashSet<_> = (page.frames.iter())
        .map(|frame| frame.map)
        .filter(|map| state.doc.shown(*map, layer))
        .collect();
    each_frame(state, page, place, |shown| {
        if !maps.contains(&shown.frame.map) {
            return;
        }
        let shapes = features.iter().map(|(r, g)| (*r, shape(shown, g)));
        f(shown, shapes.collect());
    });
}

fn dir_at(shown: &Shown, pos: Pos2) -> Option<crate::globe::math::V3> {
    shown.place_at(pos).map(|p| lonlat_to_dir(p[0], p[1]))
}

/// A tap of the Select tool. A tap in a frame selects the frame and the
/// feature under the tap in the map of the frame, or no feature. A tap
/// outside the frames clears the selection.
pub(crate) fn tap(state: &mut AppState, page: &PageRecord, place: PagePlace, pos: Pos2) {
    let Some(id) = frame_at(page, place.to_mm(pos)) else {
        state.atlas.selection = AtlasSelection::None;
        state.set_feature_selection(Vec::new());
        return;
    };
    state.atlas.selection = AtlasSelection::Frame(id);
    let reach = state.atlas.pointer.reach();
    let mut hit = None;
    each_shapes(
        state,
        page,
        place,
        |_| true,
        |shown, shapes| {
            if shown.frame.id == id {
                hit = select::hit_shapes(&shapes, pos, dir_at(shown, pos), reach);
            }
        },
    );
    state.set_feature_selection(hit.into_iter().collect());
}

/// A box of the Select tool from `start`. It selects the features that the
/// box meets in each frame. The frame under `start` becomes the selected
/// frame.
pub(crate) fn select_box(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    start: Pos2,
    rect: Rect,
) {
    let center = rect.center();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    each_shapes(
        state,
        page,
        place,
        |_| true,
        |shown, shapes| {
            let met = select::box_shapes(&shapes, rect, dir_at(shown, center));
            found.extend(met.into_iter().filter(|f| seen.insert(f.uuid)));
        },
    );
    if let Some(id) = frame_at(page, place.to_mm(start)) {
        state.atlas.selection = AtlasSelection::Frame(id);
    }
    state.set_feature_selection(found);
}

/// The selection on the screen, in each frame whose map shows its layer,
/// cut at the frames.
pub fn highlight(state: &mut AppState, place: PagePlace) -> Shape {
    let mut out = Shape::default();
    let selected: HashSet<Uuid> = state.feature_selection.iter().map(|f| f.uuid).collect();
    if selected.is_empty() {
        return out;
    }
    let page = open_page(state).clone();
    let keep = |uuid| selected.contains(&uuid);
    each_shapes(state, &page, place, keep, |_, shapes| {
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
    let shape = highlight(state, place);
    crate::ui::select::paint_shape(&ui.painter_at(area), shape);
}
