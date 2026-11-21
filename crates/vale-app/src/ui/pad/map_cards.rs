//! The row "Add collection…" of the Layers tab of the Atlas workspace,
//! below the collections of the map.

use eframe::egui;
use vale_store::{CollectionId, MapId};

use super::collection::collection_row;
use super::widgets::{self, Controls};
use super::{list, popover};
use crate::ui::AppState;

pub const ADD_COLLECTION: &str = "Add collection…";
/// The desktop button that takes a collection out of a map.
pub const REMOVE_COLLECTION: &str = "Remove collection";
/// The hint of the desktop menu "Add collection…" when the map has each
/// collection.
pub const ALL_COLLECTIONS: &str = "The map has each collection.";

/// The width of the popover of "Add collection…".
const MENU_WIDTH: f32 = 300.0;

/// The collections of a map from the top of its stack, then the others in
/// the order of creation, each with true if the map has it.
pub fn popover_order(state: &AppState, map: MapId) -> Vec<(CollectionId, bool)> {
    let project = &state.doc.project;
    let stack = project.map_stack(map);
    let top_first = stack.iter().rev().map(|id| (*id, true));
    let others = (project.collections().iter())
        .filter(|c| !stack.contains(&c.id))
        .map(|c| (c.id, false));
    top_first.chain(others).collect()
}

/// The row "Add collection…" and its popover. The popover lists each
/// collection with a checkmark on each collection of the map. A tap puts a
/// collection on top of the map or takes it out, as one undo step. The
/// last collection of a map is dim and does not change.
pub fn add_collection(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
) {
    let button = widgets::button_row(ui, controls, ADD_COLLECTION, true);
    if button.clicked() {
        state.atlas.map_cards.adding = !state.atlas.map_cards.adding;
    }
    if !state.atlas.map_cards.adding {
        return;
    }
    let order = popover_order(state, map);
    let last = order.iter().filter(|(_, on)| *on).count() == 1;
    let rows: Vec<list::Row<CollectionId>> = (order.iter())
        .filter_map(|(id, _)| {
            let c = state.doc.project.collection(*id)?;
            Some(list::Row {
                label: collection_row(&c.name),
                select: false,
                lift: false,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    let mut tapped = None;
    let shown = popover::show(
        ui.ctx(),
        "pad-map-collections",
        button.rect,
        MENU_WIDTH,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut gestures = list::ListState::default();
            let spec = list::Spec::default();
            let events = list::show(
                ui,
                controls,
                &mut gestures,
                spec,
                &rows,
                |ui, controls, cell, i| {
                    let on = order[i].1;
                    let look = list::Look {
                        text: &rows[i].name,
                        check: Some(on),
                        dim: on && last,
                        ..list::Look::default()
                    };
                    list::draw(ui, controls, cell, &look)
                },
            );
            for event in events {
                if let list::Event::Tap(id) = event {
                    tapped = Some(id);
                }
            }
        },
    );
    if let Some(id) = tapped {
        match order.iter().any(|(key, on)| *on && *key == id) {
            true => state.remove_map_collection(map, id),
            false => state.add_map_collection(map, id),
        };
    } else if shown.closed && !button.clicked() {
        state.atlas.map_cards.adding = false;
    }
}
