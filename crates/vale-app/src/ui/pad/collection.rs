//! The collection menu of the Layers panel and the Collections page of a
//! layer.

use eframe::egui::{self, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, LayerId, Project};

use super::icons::{self, Icon};
use super::widgets::{self, Controls};
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
pub const EMPTY_COLLECTION: &str = "No layers in this collection.";

/// The width of the collection menu.
const MENU_WIDTH: f32 = 300.0;

/// The name of the menu row of a collection.
pub fn collection_row(name: &str) -> String {
    format!("Collection {name}")
}

/// True if the globe checks each collection.
pub fn checks_all(project: &Project) -> bool {
    let checked = project.globe_collections();
    (project.collections().iter()).all(|c| checked.contains(&c.id))
}

/// The value of the row of the collection menu: "All layers" if the globe
/// checks each collection, or else the names of the checked collections
/// from the top.
pub fn menu_value(project: &Project) -> String {
    if checks_all(project) {
        return ALL_LAYERS.to_owned();
    }
    let names: Vec<&str> = (project.globe_collections().iter().rev())
        .filter_map(|id| project.collection(*id))
        .map(|c| c.name.as_str())
        .collect();
    names.join(", ")
}

/// The row at the top of the Layers panel that shows the checked
/// collections and opens the collection menu.
pub(super) fn menu_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let text = menu_value(&state.doc.project);
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
    let all_on = checks_all(&state.doc.project);
    // All layers is a list of its own, with no gestures.
    let all = list::Row {
        label: ALL_LAYERS.to_owned(),
        on: all_on,
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
                check: Some(all_on),
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    if events.contains(&list::Event::Tap(())) {
        state.show_collection(None);
        state.pad.collections_menu = false;
    }

    let project = &state.doc.project;
    let (checked, world) = (project.globe_collections(), project.world_collection());
    let rows: Vec<list::Row<CollectionId>> = (project.collections().iter())
        .map(|c| list::Row {
            label: collection_row(&c.name),
            on: checked.contains(&c.id),
            rename: true,
            delete: c.id != world,
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

/// The value of the Collections row of a layer: the name of its one
/// collection, or the number of its collections.
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

/// The Collections page of a layer: a switch for each collection. The
/// switch of World is dim while the layer is only in World.
pub(super) fn page_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let project = &state.doc.project;
    let switches: Vec<(CollectionId, String, bool, bool)> = (project.collections().iter())
        .map(|c| {
            let on = c.layers.contains(&layer);
            let free = project.can_leave_collection(c.id, layer);
            (c.id, c.name.clone(), on, free)
        })
        .collect();
    for (id, name, mut on, free) in switches {
        let switch = switch_name(&name);
        if widgets::switch_row_enabled(ui, controls, &switch, &name, &mut on, free) {
            state.set_in_collection(id, layer, on);
        }
    }
}
