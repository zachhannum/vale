//! The pages of the collections in the Layers panel: the management page,
//! the page of a collection, and "Add layers…".

use eframe::egui;
use vale_store::{CollectionId, LayerId, Project};

use super::collection::{
    COLLECTION_NAME, COLLECTIONS, EMPTY_COLLECTION, NEW_COLLECTION, layer_row,
};
use super::geometry::TOUCH;
use super::widgets::{self, Controls};
use super::{LayerRef, list, theme};
use crate::ui::AppState;

/// The row of the collection popover that opens the management page.
pub const MANAGE_COLLECTIONS: &str = "Manage collections";
pub const ADD_LAYERS: &str = "Add layers…";
/// The title of the page of "Add layers…".
pub const ADD_LAYERS_TITLE: &str = "Add layers";
pub const DELETE_COLLECTION: &str = "Delete collection";
/// The hint of the search field of each page.
pub const SEARCH: &str = "Search";
pub const NO_MATCH: &str = "No match.";

/// A page of the collections.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ManagePage {
    Manage,
    Collection(CollectionId),
    AddLayers(CollectionId),
}

/// The state of the pages of the collections.
#[derive(Default)]
pub struct ManageState {
    /// The page that the Layers panel shows in place of the cards.
    pub page: Option<ManagePage>,
    /// The text of the search field of the management page.
    pub search: String,
    pub collections: list::ListState<CollectionId>,
    /// The rows of the layers of the page of a collection.
    pub members: list::ListState<LayerId>,
    /// The text of the search field of "Add layers…".
    pub add_search: String,
    pub add: list::ListState<LayerId>,
}

impl ManageState {
    /// Opens a page, with its search field empty and its rows shut.
    pub fn open(&mut self, page: ManagePage) {
        match page {
            ManagePage::Manage => {
                self.search.clear();
                self.collections = list::ListState::default();
            }
            ManagePage::Collection(_) => {
                self.members = list::ListState::default();
            }
            ManagePage::AddLayers(_) => {
                self.add_search.clear();
                self.add = list::ListState::default();
            }
        }
        self.page = Some(page);
    }
}

/// The name of the row of a collection on the management page.
pub fn collection_entry(name: &str) -> String {
    format!("{name} collection")
}

/// The name of the row of a layer on the page of "Add layers…".
pub fn add_row(name: &str) -> String {
    format!("Include {name}")
}

fn holds(text: &str, search: &str) -> bool {
    text.to_lowercase().contains(&search.trim().to_lowercase())
}

/// The names of the collections of a layer, separated by commas.
pub fn collections_line(project: &Project, layer: LayerId) -> String {
    let names: Vec<&str> = (project.collections_of(layer).iter())
        .map(|c| c.name.as_str())
        .collect();
    names.join(", ")
}

/// The collections of the management page whose names hold `search`,
/// in the order of the collection popover.
pub fn collection_rows(project: &Project, search: &str) -> Vec<CollectionId> {
    (super::collection::popover_order(project).into_iter())
        .map(|(id, _)| id)
        .filter(|id| {
            project
                .collection(*id)
                .is_some_and(|c| holds(&c.name, search))
        })
        .collect()
}

/// The layers of "Add layers…" whose names hold `search`, by name. Each
/// has true if the collection holds it, and true if it can leave the
/// collection.
pub fn add_rows(project: &Project, id: CollectionId, search: &str) -> Vec<(LayerId, bool, bool)> {
    let held = project
        .collection(id)
        .map(|c| c.layers.clone())
        .unwrap_or_default();
    let mut rows: Vec<(String, LayerId)> = (project.layers().iter())
        .filter(|l| holds(&l.name, search))
        .map(|l| (l.name.to_lowercase(), l.id))
        .collect();
    rows.sort();
    (rows.into_iter())
        .map(|(_, l)| {
            let on = held.contains(&l);
            (l, on, project.can_leave_collection(id, l))
        })
        .collect()
}

/// Closes a page whose collection is gone.
fn check(state: &mut AppState) {
    let project = &state.doc.project;
    let gone = |id: CollectionId| project.collection(id).is_none();
    match state.pad.manage.page {
        Some(ManagePage::Collection(id) | ManagePage::AddLayers(id)) if gone(id) => {
            state.pad.manage.page = Some(ManagePage::Manage);
        }
        _ => {}
    }
}

/// The key, the title, and the back text of the open page.
pub(super) fn head(state: &mut AppState) -> Option<(&'static str, String, String)> {
    check(state);
    let project = &state.doc.project;
    let name = |id: CollectionId| {
        project
            .collection(id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    Some(match state.pad.manage.page? {
        ManagePage::Manage => ("pad-manage", COLLECTIONS.to_owned(), "Layers".to_owned()),
        ManagePage::Collection(id) => ("pad-collection", name(id), COLLECTIONS.to_owned()),
        ManagePage::AddLayers(id) => ("pad-add-layers", ADD_LAYERS_TITLE.to_owned(), name(id)),
    })
}

/// The back text of the panel of a layer that a page of the collections
/// opened.
pub(super) fn layer_back(state: &AppState) -> Option<String> {
    match state.pad.manage.page? {
        ManagePage::Collection(id) => Some(state.doc.project.collection(id)?.name.clone()),
        _ => Some(COLLECTIONS.to_owned()),
    }
}

/// Goes back one page.
pub(super) fn back(state: &mut AppState) {
    let manage = &mut state.pad.manage;
    match manage.page {
        Some(ManagePage::AddLayers(id)) => manage.page = Some(ManagePage::Collection(id)),
        Some(ManagePage::Collection(_)) => manage.page = Some(ManagePage::Manage),
        _ => manage.page = None,
    }
}

/// Opens the panel of a layer from a page of the collections.
fn open_layer(state: &mut AppState, layer: LayerId) {
    let Some(l) = state.doc.project.layer(layer) else {
        return;
    };
    let layer = LayerRef::of(l);
    state.select(Some(layer));
    state.pad.page = None;
    state.pad.layer = Some(layer);
}

/// The body of the open page.
pub(super) fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    match state.pad.manage.page {
        Some(ManagePage::Manage) => manage_body(ui, state, controls),
        Some(ManagePage::Collection(id)) => collection_body(ui, state, controls, id),
        Some(ManagePage::AddLayers(id)) => add_body(ui, state, controls, id),
        None => {}
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).color(theme::MUTE));
    ui.add_space(8.0);
}

/// The management page: the search field, a row with the count of each
/// collection, with Rename and Delete on a swipe, and "New collection…".
fn manage_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    super::filter::search_field(ui, controls, SEARCH, &mut state.pad.manage.search);
    let project = &state.doc.project;
    let world = project.world_collection();
    let shown = collection_rows(project, &state.pad.manage.search);
    let counts: Vec<String> = (shown.iter())
        .map(|id| {
            project
                .collection(*id)
                .map_or(0, |c| c.layers.len())
                .to_string()
        })
        .collect();
    let rows: Vec<list::Row<CollectionId>> = (shown.iter())
        .filter_map(|id| {
            let c = project.collection(*id)?;
            Some(list::Row {
                label: collection_entry(&c.name),
                rename: c.id != world,
                delete: c.id != world,
                trailing: TOUCH,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    if rows.is_empty() {
        hint(ui, NO_MATCH);
    }
    let list = &mut state.pad.manage.collections;
    let renamed = list.rename.as_ref().map(|r| r.0);
    let valid = |name: &str| project.collection_name_is_free(name, renamed);
    let spec = list::Spec {
        field: COLLECTION_NAME,
        valid: &valid,
        ..list::Spec::default()
    };
    let events = list::show(ui, controls, list, spec, &rows, |ui, controls, cell, i| {
        let look = list::Look {
            text: &rows[i].name,
            note: Some(&counts[i]),
            open: true,
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(id) | list::Event::Open(id) => {
                state.pad.manage.open(ManagePage::Collection(id));
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
    if widgets::button_row(ui, controls, NEW_COLLECTION, true).clicked() {
        // The row of the new collection is its name field.
        let name = state.doc.project.free_collection_name();
        if let Some(id) = state.add_collection(&name) {
            let manage = &mut state.pad.manage;
            manage.search.clear();
            manage.collections.shut();
            manage.collections.rename = Some((id, name));
        }
    }
}

/// The page of a collection: the rows of its layers from the top with
/// Remove on a swipe, "Add layers…", and "Delete collection".
fn collection_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: CollectionId,
) {
    let project = &state.doc.project;
    if project.collection(id).is_none() {
        return;
    }
    let world = id == project.world_collection();

    let project = &state.doc.project;
    let selected = state.selected_layer;
    let mut shown = Vec::new();
    let mut rows = Vec::new();
    for layer in (project.collection(id).into_iter()).flat_map(|c| c.layers.iter().rev()) {
        let Some(l) = project.layer(*layer) else {
            continue;
        };
        shown.push((super::layer_icon(l), l.visible));
        let remove = project.can_leave_collection(id, l.id);
        rows.push(list::Row {
            label: layer_row(&l.name),
            on: selected == Some(LayerRef::of(l)),
            actions: if remove { &[list::REMOVE] } else { &[] },
            trailing: 2.0 * TOUCH,
            ..list::Row::new(l.id, l.name.clone())
        });
    }
    if rows.is_empty() {
        hint(ui, EMPTY_COLLECTION);
    }
    let list = &mut state.pad.manage.members;
    let events = list::show(
        ui,
        controls,
        list,
        list::Spec::default(),
        &rows,
        |ui, controls, cell, i| {
            let look = list::Look {
                icon: Some(shown[i].0),
                text: &rows[i].name,
                eye: Some(shown[i].1),
                open: true,
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    let visible = |key: LayerId| {
        let i = rows.iter().position(|r| r.key == key)?;
        Some(shown[i].1)
    };
    for event in events {
        match event {
            list::Event::Tap(key) => {
                if let Some(l) = state.doc.project.layer(key) {
                    state.select(Some(LayerRef::of(l)));
                }
            }
            list::Event::Open(key) => open_layer(state, key),
            list::Event::Control(key, list::EYE) => {
                if let Some(visible) = visible(key) {
                    state.set_layer_visible(key, !visible);
                }
            }
            list::Event::Action(key, list::REMOVE) => {
                state.set_in_collection(id, key, false);
            }
            _ => {}
        }
    }

    if widgets::button_row(ui, controls, ADD_LAYERS, true).clicked() {
        state.pad.manage.open(ManagePage::AddLayers(id));
    }
    if !world
        && widgets::button_row_in(ui, controls, DELETE_COLLECTION, true, theme::WARN).clicked()
        && state.delete_collection(id)
    {
        state.pad.manage.page = Some(ManagePage::Manage);
    }
}

/// The page of "Add layers…": the search field, and a row for each layer
/// of the project with its collections on a second line. A check puts the
/// layer in the collection and keeps its other collections.
fn add_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: CollectionId) {
    super::filter::search_field(ui, controls, SEARCH, &mut state.pad.manage.add_search);
    let project = &state.doc.project;
    let shown = add_rows(project, id, &state.pad.manage.add_search);
    let mut lines = Vec::new();
    let mut icons = Vec::new();
    let mut rows = Vec::new();
    for (layer, _, free) in &shown {
        let Some(l) = project.layer(*layer) else {
            continue;
        };
        lines.push(collections_line(project, l.id));
        icons.push(super::layer_icon(l));
        rows.push(list::Row {
            label: add_row(&l.name),
            enabled: *free,
            height: list::TWO_LINE_ROW,
            ..list::Row::new(l.id, l.name.clone())
        });
    }
    if rows.is_empty() {
        hint(ui, NO_MATCH);
    }
    let list = &mut state.pad.manage.add;
    let events = list::show(
        ui,
        controls,
        list,
        list::Spec::default(),
        &rows,
        |ui, controls, cell, i| {
            let look = list::Look {
                icon: Some(icons[i]),
                text: &rows[i].name,
                detail: Some(&lines[i]),
                check: Some(shown[i].1),
                dim: !shown[i].2,
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    for event in events {
        if let list::Event::Tap(key) = event {
            let on = shown.iter().any(|(l, on, _)| *on && *l == key);
            state.set_in_collection(id, key, !on);
        }
    }
}
