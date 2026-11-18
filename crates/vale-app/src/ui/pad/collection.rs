//! The collection menu of the Layers panel and the Collections page of a
//! layer.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, LayerId, Project};

use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{list, popover, theme};
use crate::ui::AppState;

/// The name of the row that opens the collection menu.
pub const COLLECTION_MENU: &str = "Collection";
pub const ALL_LAYERS: &str = "All layers";
pub const NEW_COLLECTION: &str = "New collection…";
/// The row of a layer panel that opens the Collections page, and the title
/// of the page.
pub const COLLECTIONS: &str = "Collections";
/// The name of the name field of a collection.
pub const COLLECTION_NAME: &str = "Collection name";
pub const NO_COLLECTIONS: &str = "No collections.";
pub const EMPTY_COLLECTION: &str = "No layers in this collection.";

/// The width of the collection menu.
const MENU_WIDTH: f32 = 300.0;

/// The name of the menu row of a collection.
pub fn collection_row(name: &str) -> String {
    format!("Collection {name}")
}

/// The row at the top of the Layers panel that shows the shown collection
/// and opens the collection menu.
pub(super) fn menu_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let project = &state.doc.project;
    let text = project
        .shown_collection()
        .and_then(|id| project.collection(id))
        .map_or(ALL_LAYERS.to_owned(), |c| c.name.clone());
    let open = state.pad.collections_menu;
    let resp = widgets::text_row_as(ui, controls, COLLECTION_MENU, &text, 0.0, open, true);
    let row = resp.rect;
    let chevron = Rect::from_center_size(
        pos2(row.right() - widgets::ICON / 2.0 - 8.0, row.center().y),
        vec2(18.0, 18.0),
    );
    icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
    if resp.clicked() {
        state.pad.collections_menu = !open;
        state.pad.collection_list = list::ListState::default();
    }
    if state.pad.collections_menu {
        let shown = popover::show(ui.ctx(), "pad-collections", row, MENU_WIDTH, |ui| {
            menu(ui, state, controls);
        });
        state.pad.collections_rect = Some(shown.rect);
        // A tap outside closes the menu, but not while a name field is open.
        if shown.closed && state.pad.collection_list.rename.is_none() {
            state.pad.collections_menu = false;
        }
    }
}

/// The contents of the collection menu: "All layers", the collections,
/// and "New collection…".
fn menu(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let shown = state.doc.project.shown_collection();
    // All layers is a list of its own, with no gestures.
    let all = list::Row {
        label: ALL_LAYERS.to_owned(),
        on: shown.is_none(),
        ..list::Row::new((), ALL_LAYERS)
    };
    let mut gestures = list::ListState::default();
    let rows = [all];
    let events = list::show(
        ui,
        controls,
        &mut gestures,
        list::Spec::default(),
        &rows,
        |ui, controls, cell, _| {
            let look = list::Look {
                text: ALL_LAYERS,
                check: Some(shown.is_none()),
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    if events.contains(&list::Event::Tap(())) {
        state.show_collection(None);
        state.pad.collections_menu = false;
    }

    let rows: Vec<list::Row<CollectionId>> = (state.doc.project.collections().iter())
        .map(|c| list::Row {
            label: collection_row(&c.name),
            on: shown == Some(c.id),
            rename: true,
            delete: true,
            ..list::Row::new(c.id, c.name.clone())
        })
        .collect();
    let list = &mut state.pad.collection_list;
    let renamed = list.rename.as_ref().map(|r| r.0);
    let project = &state.doc.project;
    let valid = |name: &str| project.collection_name_is_free(name, renamed);
    let spec = list::Spec {
        field: COLLECTION_NAME,
        valid: &valid,
        ..list::Spec::default()
    };
    let events = list::show(ui, controls, list, spec, &rows, |ui, controls, cell, i| {
        let look = list::Look {
            text: &rows[i].name,
            check: Some(rows[i].on),
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(id) => {
                state.show_collection(Some(id));
                state.pad.collections_menu = false;
            }
            list::Event::Rename(id, name) => {
                state.rename_collection(id, &name);
            }
            list::Event::Delete(id) => {
                state.delete_collection(id);
            }
            _ => {}
        }
    }

    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    let line = Rect::from_center_size(line.center(), vec2(line.width(), 1.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
    if widgets::text_row(ui, controls, NEW_COLLECTION, 0.0, false, true).clicked() {
        // The row of the new collection is its name field.
        let name = state.doc.project.free_collection_name();
        if let Some(id) = state.add_collection(&name) {
            let list = &mut state.pad.collection_list;
            list.shut();
            list.rename = Some((id, name));
        }
    }
}

/// The value of the Collections row of a layer: "None", the name of its
/// one collection, or the number of its collections.
pub fn row_value(project: &Project, layer: LayerId) -> String {
    match project.collections_of(layer).as_slice() {
        [] => "None".to_owned(),
        [one] => one.name.clone(),
        many => format!("{} collections", many.len()),
    }
}

/// The name of the switch of a collection on the Collections page.
pub fn switch_name(name: &str) -> String {
    format!("In {name}")
}

/// The row of a layer panel that opens the Collections page.
pub(super) fn open_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    let value = row_value(&state.doc.project, layer);
    if widgets::open_row(ui, controls, COLLECTIONS, &value).clicked() {
        state.pad.page = Some(super::Page::Collections);
    }
}

/// The Collections page of a layer: a switch for each collection.
pub(super) fn page_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let switches: Vec<(CollectionId, String, bool)> = (state.doc.project.collections().iter())
        .map(|c| (c.id, c.name.clone(), c.layers.contains(&layer)))
        .collect();
    if switches.is_empty() {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let at = pos2(rect.left(), rect.center().y);
        let font = theme::body();
        (ui.painter()).text(at, Align2::LEFT_CENTER, NO_COLLECTIONS, font, theme::MUTE);
        return;
    }
    for (id, name, mut on) in switches {
        if widgets::switch_row(ui, controls, &switch_name(&name), &name, &mut on) {
            state.set_in_collection(id, layer, on);
        }
    }
}
