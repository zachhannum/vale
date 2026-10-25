//! The attribute schema of a vector layer: the list of the fields in the
//! panel of the layer, the page of a new field, and the page of a field.

use eframe::egui::{self, Rect, pos2, vec2};
use vale_store::{Field, FieldType, LayerId, Value};

use super::icons::{self, Icon};
use super::widgets::{self, Controls, ICON};
use super::{Page, raster, theme};
use crate::ui::AppState;
use crate::ui::point_form::can_take_focus;

/// The heading of the list of the fields.
pub const FIELDS: &str = "Fields";
/// The row of the panel of a vector layer that opens the page of a new
/// field.
pub const ADD_FIELD: &str = "Add field…";
/// The title of the page of a new field.
pub const NEW_FIELD: &str = "New field";
/// The field of the name on the page of a field.
pub const FIELD_NAME: &str = "Field name";
/// The field of the default value on the page of a field.
pub const DEFAULT_VALUE: &str = "Default value";
/// The button of the page of a new field that adds the field.
pub const ADD: &str = "Add";
/// The button of the page of a field that removes the field.
pub const DELETE_FIELD: &str = "Delete field";
/// The choices of the default value of a true or false field.
pub const NO_DEFAULT: &str = "None";
const BOOL_CHOICES: [&str; 3] = [NO_DEFAULT, "True", "False"];
const BOOL_TEXTS: [&str; 3] = ["", "true", "false"];

/// The texts of the page of a field while the user edits them.
pub struct Draft {
    layer: LayerId,
    page: Page,
    name: String,
    kind: FieldType,
    default: String,
    /// The name takes the focus in the next frame that it can.
    focus: bool,
}

/// The value of a default text. A blank text gives `Null`, and a text that
/// is not a value of the type gives `None`.
fn default_value(kind: FieldType, text: &str) -> Option<Value> {
    let text = text.trim();
    if text.is_empty() {
        Some(Value::Null)
    } else {
        kind.parse(text)
    }
}

/// The draft of the page, or a new draft from the field. A new field starts
/// with the focus in its name, so the keyboard opens.
fn take_draft(state: &mut AppState, id: LayerId, page: Page, field: Option<&Field>) -> Draft {
    match state.pad.field.take() {
        Some(draft) if draft.layer == id && draft.page == page => draft,
        _ => Draft {
            layer: id,
            page,
            name: field.map_or_else(String::new, |f| f.name.clone()),
            kind: field.map_or(FieldType::Text, |f| f.kind),
            default: field.and_then(|f| f.default.label()).unwrap_or_default(),
            focus: field.is_none(),
        },
    }
}

/// The list of the fields in the panel of a vector layer: one row for each
/// field, with its type, and the row that adds a field.
pub fn list(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let fields: Vec<(String, FieldType)> = layer
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.kind))
        .collect();
    raster::heading(ui, FIELDS);
    for (i, (name, kind)) in fields.iter().enumerate() {
        if widgets::open_row(ui, controls, name, kind.label()).clicked() {
            state.pad.page = Some(Page::Field(i));
        }
    }
    let add = widgets::text_row(ui, controls, ADD_FIELD, 0.0, false, true);
    let plus = Rect::from_center_size(
        pos2(add.rect.right() - ICON / 2.0, add.rect.center().y),
        vec2(ICON, ICON),
    );
    icons::paint(ui.painter(), plus, Icon::Plus, ICON, theme::MUTE);
    if add.clicked() {
        state.pad.page = Some(Page::NewField);
    }
}

/// One option row for each type. Returns the type that the user chose.
fn type_rows(ui: &mut egui::Ui, controls: &mut Controls, kind: FieldType) -> Option<FieldType> {
    raster::heading(ui, "Type");
    let mut chosen = None;
    for option in FieldType::ALL {
        let label = option.label();
        let on = option == kind;
        if widgets::option_row(ui, controls, label, label, &[], on).clicked() && !on {
            chosen = Some(option);
        }
    }
    chosen
}

/// The row of the default value: a text field, or for a true or false field
/// a choice of None, True, and False. Returns true if the user ends an edit
/// of the default value, and true if the text field has the focus.
fn default_row(ui: &mut egui::Ui, controls: &mut Controls, draft: &mut Draft) -> (bool, bool) {
    ui.add_space(8.0);
    if draft.kind == FieldType::Bool {
        let active = BOOL_TEXTS.iter().position(|t| t == &draft.default.trim());
        let chosen = widgets::choice_row(ui, controls, "Default", &BOOL_CHOICES, active);
        if let Some(i) = chosen {
            draft.default = BOOL_TEXTS[i].to_string();
        }
        return (chosen.is_some(), false);
    }
    let valid = default_value(draft.kind, &draft.default).is_some();
    let resp = widgets::text_field(
        ui,
        controls,
        DEFAULT_VALUE,
        "Default",
        None,
        &mut draft.default,
        valid,
    );
    (resp.lost_focus(), resp.has_focus())
}

/// The page of a new field: its name, its type, and its default value. Add,
/// and Return in the name, add the field and go back to the layer.
pub fn new_field_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
) {
    let mut draft = take_draft(state, id, Page::NewField, None);
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let free = layer.name_free(draft.name.trim(), None);
    ui.spacing_mut().item_spacing.y = 0.0;
    let name = widgets::text_field(
        ui,
        controls,
        FIELD_NAME,
        "Name",
        None,
        &mut draft.name,
        free,
    );
    if draft.focus && can_take_focus(ui.ctx()) {
        name.request_focus();
        draft.focus = false;
    }
    let enter = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if let Some(kind) = type_rows(ui, controls, draft.kind) {
        draft.kind = kind;
        if default_value(kind, &draft.default).is_none() {
            draft.default.clear();
        }
    }
    default_row(ui, controls, &mut draft);
    let value = default_value(draft.kind, &draft.default);
    let can_add = free && value.is_some();
    ui.add_space(12.0);
    let add = widgets::button_row(ui, controls, ADD, can_add).clicked();
    if let Some(default) = value.filter(|_| can_add && (add || enter)) {
        let field = Field {
            name: draft.name.trim().to_string(),
            kind: draft.kind,
            default,
        };
        if state.edit_schema(id, |doc| doc.project.add_field(id, field)) {
            state.pad.page = None;
            return;
        }
    }
    state.pad.field = Some(draft);
}

/// The page of the field `index`: its name, its type, and its default value,
/// and the button that deletes it. Each change applies at once and is one
/// undo step. The name and the default value apply at the end of their
/// edit.
pub fn field_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    index: usize,
) {
    let Some(field) = state
        .doc
        .layer(id)
        .and_then(|layer| layer.fields.get(index))
        .cloned()
    else {
        return;
    };
    let mut draft = take_draft(state, id, Page::Field(index), Some(&field));
    let layer = state.doc.layer(id).expect("checked above");
    let new_name = draft.name.trim().to_string();
    let free = new_name == field.name || layer.name_free(&new_name, Some(&field.name));
    ui.spacing_mut().item_spacing.y = 0.0;

    let name = widgets::text_field(
        ui,
        controls,
        FIELD_NAME,
        "Name",
        None,
        &mut draft.name,
        free,
    );
    if name.lost_focus() {
        if free && new_name != field.name {
            state.edit_schema(id, |doc| doc.rename_field(id, &field.name, &new_name));
        }
    } else if !name.has_focus() {
        // The name shows the field again after an undo and after a name
        // that is not free.
        draft.name = field.name.clone();
    }
    // The other changes find the field by its name after a rename.
    let current = state
        .doc
        .layer(id)
        .and_then(|layer| layer.fields.get(index))
        .map_or(field.name.clone(), |f| f.name.clone());

    if let Some(kind) = type_rows(ui, controls, field.kind) {
        state.edit_schema(id, |doc| doc.project.set_field_type(id, &current, kind));
    }
    // The type and the default value come from the field, so they follow
    // a change of the type and an undo.
    let now = state
        .doc
        .layer(id)
        .and_then(|layer| layer.fields.get(index))
        .cloned()
        .unwrap_or(field);
    draft.kind = now.kind;
    let (ended, editing) = default_row(ui, controls, &mut draft);
    if ended && let Some(value) = default_value(now.kind, &draft.default) {
        state.edit_schema(id, |doc| {
            doc.project.set_field_default(id, &now.name, value)
        });
    } else if !editing {
        draft.default = now.default.label().unwrap_or_default();
    }
    ui.add_space(12.0);
    if widgets::button_row_in(ui, controls, DELETE_FIELD, true, theme::WARN).clicked() {
        state.edit_schema(id, |doc| doc.project.remove_field(id, &now.name));
        state.pad.page = None;
        return;
    }
    state.pad.field = Some(draft);
}
