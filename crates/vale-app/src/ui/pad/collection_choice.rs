//! The Collection row of a form that makes a layer, and its picker.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, Project};

use super::collection::SEARCH_COLLECTIONS;
use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::AppState;
use crate::ui::collection_choice::CollectionChoice;

/// The label of the row, and the title of the picker.
pub const COLLECTION: &str = "Collection";
/// The row of the picker that makes a collection with the new layer.
pub const NEW_COLLECTION_ROW: &str = "New collection";
/// The name of the name field of the row "New collection".
pub const NEW_COLLECTION_NAME: &str = "New collection name";

/// The space between the left edge of a row and its text. The check is at
/// the left of the text, as in an `option_row`.
const TEXT_X: f32 = 38.0;

/// The text of the picker. One picker shows at a time.
#[derive(Default)]
pub struct Picker {
    pub search: String,
    /// The name of the new collection.
    pub name: String,
}

/// The height of the picker with the rows of each collection.
pub fn picker_height(project: &Project) -> f32 {
    let rows = 1 + project.collections().len();
    // The search field has a space above it and below it, and the rows
    // have a space above them.
    8.0 + TOUCH + 4.0 + 4.0 + rows as f32 * PANEL_ROW
}

/// The name of the row of a collection in the picker.
pub fn row_name(collection: &str) -> String {
    widgets::choice_row_name(COLLECTION, collection)
}

/// The Collection row: the label and the name of the choice. True if the
/// user tapped it, and then the picker of the form opens with `open`.
pub fn row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    choice: Option<&CollectionChoice>,
) -> bool {
    let value = state.collection_choice_name(choice);
    let tapped = widgets::open_row(ui, controls, COLLECTION, &value).clicked();
    if tapped {
        open(state, choice);
    }
    tapped
}

/// Prepares the picker for a choice.
pub fn open(state: &mut AppState, choice: Option<&CollectionChoice>) {
    let name = match choice {
        Some(CollectionChoice::New(name)) => name.clone(),
        _ => state.doc.project.free_collection_name(),
    };
    state.pad.collection_picker = Picker {
        search: String::new(),
        name,
    };
}

/// The picker: the search field, the row "New collection" with its name
/// field, and a row for each collection whose name holds the search text,
/// by name. The row of the choice has a check. True if the user chose a
/// collection that exists, and the picker closes.
pub fn picker(
    ui: &mut egui::Ui,
    project: &Project,
    default: CollectionId,
    controls: &mut Controls,
    picker: &mut Picker,
    choice: &mut Option<CollectionChoice>,
) -> bool {
    ui.spacing_mut().item_spacing.y = 0.0;
    super::filter::search_field(ui, controls, SEARCH_COLLECTIONS, &mut picker.search);
    ui.add_space(4.0);

    let new = matches!(choice, Some(CollectionChoice::New(_)));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let left = rect.left() + TEXT_X + widgets::text_width(ui, NEW_COLLECTION_ROW, theme::body());
    let field = Rect::from_min_max(pos2(left + 12.0, rect.top()), rect.max);
    let label = Rect::from_min_max(rect.min, pos2(field.left(), rect.bottom()));
    let sense = Sense::click();
    let row = widgets::hit(
        ui,
        controls,
        label,
        NEW_COLLECTION_ROW,
        sense,
        true,
        Some(new),
    );
    let mark = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0));
    if new {
        icons::paint(ui.painter(), mark, Icon::Tick, 18.0, theme::ACCENT);
    }
    let at = pos2(rect.left() + TEXT_X, rect.center().y);
    let color = if new { theme::TEXT } else { theme::MUTE };
    (ui.painter()).text(
        at,
        Align2::LEFT_CENTER,
        NEW_COLLECTION_ROW,
        theme::body(),
        color,
    );
    let valid = project.collection_name_is_free(&picker.name, None);
    let font = theme::body();
    let edit = widgets::field_at(
        ui,
        controls,
        field,
        NEW_COLLECTION_NAME,
        None,
        &mut picker.name,
        valid,
        font,
    );
    if row.clicked() || edit.changed() || edit.gained_focus() {
        *choice = Some(CollectionChoice::New(picker.name.clone()));
    }

    let on = match choice {
        Some(CollectionChoice::Existing(id)) if project.collection(*id).is_some() => Some(*id),
        Some(CollectionChoice::New(_)) => None,
        _ => Some(default),
    };
    let search = picker.search.trim().to_lowercase();
    let mut rows: Vec<_> = (project.collections().iter())
        .filter(|c| c.name.to_lowercase().contains(&search))
        .collect();
    rows.sort_by_key(|c| c.name.to_lowercase());
    let mut picked = false;
    for c in rows {
        let name = row_name(&c.name);
        let checked = on == Some(c.id);
        if widgets::option_row(ui, controls, &name, &c.name, &[], checked).clicked() {
            *choice = Some(CollectionChoice::Existing(c.id));
            picked = true;
        }
    }
    picked
}

/// The plain desktop version: a combo box with each collection and "New
/// collection", and the name field of a new collection.
pub fn desktop(
    ui: &mut egui::Ui,
    state: &AppState,
    id: &str,
    choice: &mut Option<CollectionChoice>,
) {
    let project = &state.doc.project;
    let default = state.default_layer_collection();
    let text = state.collection_choice_name(choice.as_ref());
    let new = matches!(choice, Some(CollectionChoice::New(_)));
    let text = if new {
        NEW_COLLECTION_ROW.to_owned()
    } else {
        text
    };
    ui.horizontal(|ui| {
        ui.label(COLLECTION);
        egui::ComboBox::from_id_salt((id, COLLECTION))
            .selected_text(text)
            .show_ui(ui, |ui| {
                let on = match choice {
                    Some(CollectionChoice::Existing(id)) => Some(*id),
                    Some(CollectionChoice::New(_)) => None,
                    None => Some(default),
                };
                for c in project.collections() {
                    if ui.selectable_label(on == Some(c.id), &c.name).clicked() {
                        *choice = Some(CollectionChoice::Existing(c.id));
                    }
                }
                if ui.selectable_label(new, NEW_COLLECTION_ROW).clicked() && !new {
                    *choice = Some(CollectionChoice::New(project.free_collection_name()));
                }
            });
    });
    if let Some(CollectionChoice::New(name)) = choice {
        ui.horizontal(|ui| {
            let label = ui.label(NEW_COLLECTION_NAME);
            ui.text_edit_singleline(name).labelled_by(label.id);
        });
    }
}
