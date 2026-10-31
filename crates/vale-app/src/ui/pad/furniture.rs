//! The panels of the items of furniture of a page: text, a legend, a scale
//! bar, and a north arrow.

use eframe::egui::{self, Align2, Rect, pos2, vec2};
use vale_labeler::Face;
use vale_store::{Furniture, FurnitureId, FurnitureRecord, FurnitureText, LayerId, TextAlign};

use super::atlas::panel_width;
use super::geometry::{Rects, TOUCH};
use super::icons::Icon;
use super::labels::{faces_of, value_slider};
use super::raster::heading;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Card, CardTitle, note_text, show_card, theme};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::atlas::{self, AtlasSelection, ItemKind, ItemPage};
use crate::ui::{Action, AppState, FontTarget, Note};

/// The field of the text of a text item.
pub const ITEM_TEXT: &str = "Item text";
pub const FONT: &str = "Font";
pub const STYLE: &str = "Style";
pub const SIZE: &str = "Size";
pub const COLOR: &str = "Color";
pub const ALIGNMENT: &str = "Alignment";
/// The choices of the alignment, in the order of `ALIGNS`.
pub const ALIGNMENTS: [&str; 3] = ["Left", "Center", "Right"];
const ALIGNS: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];
/// The row that opens the page of the frames, and the title of the page.
pub const MAP_FRAME: &str = "Map frame";
pub const NO_FRAME: &str = "None";
pub const NO_FRAMES: &str = "The page has no map frames.";
pub const TITLE: &str = "Title";
pub const ENTRIES: &str = "Entries";
pub const NO_ENTRIES: &str = "The map shows no vector layers.";
pub const RESET_ENTRIES: &str = "Reset entries";
pub const ENTRY: &str = "Entry";
pub const ENTRY_NAME: &str = "Entry name";
pub const ENTRY_HINT: &str = "Clear the name to use the default name.";
pub const SCALE: &str = "Scale";
pub const SCALE_HINT: &str = "Measured at the center of the frame.";
pub const HIDE_VARIES: &str = "Hide where north varies";
pub const NORTH_VARIES: &str = "North varies";

/// The button that deletes an item of a kind.
pub fn delete_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Text => "Delete text",
        ItemKind::Legend => "Delete legend",
        ItemKind::ScaleBar => "Delete scale bar",
        ItemKind::NorthArrow => "Delete north arrow",
    }
}

fn key_of(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Text => "pad-atlas-text",
        ItemKind::Legend => "pad-atlas-legend",
        ItemKind::ScaleBar => "pad-atlas-scale-bar",
        ItemKind::NorthArrow => "pad-atlas-north-arrow",
    }
}

/// The panel of an item, or of a page of the panel.
pub fn panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: FurnitureId,
) -> Option<Rect> {
    let rec = state.doc.project.atlas.furniture(id)?.clone();
    let kind = ItemKind::of(&rec.item);
    let page = state.atlas.item_page.clone();
    let (key, title, back) = match &page {
        None => (key_of(kind), kind.name(), None),
        Some(ItemPage::Frame) => ("pad-atlas-item-frame", MAP_FRAME, Some(kind.name())),
        Some(ItemPage::Style) => ("pad-atlas-item-style", STYLE, Some(kind.name())),
        Some(ItemPage::Entry(..)) => ("pad-atlas-item-entry", ENTRY, Some(kind.name())),
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head: CardTitle {
            title,
            back,
            action: None,
        },
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.item_page = None;
            state.atlas.item_color = false;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| match page {
        None => body(ui, state, controls, &rec),
        Some(ItemPage::Frame) => frame_page(ui, state, controls, &rec),
        Some(ItemPage::Style) => style_page(ui, state, controls, &rec),
        Some(ItemPage::Entry(layer, rule)) => entry_page(ui, state, controls, &rec, (layer, rule)),
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// Closes the color picker of the item when it did not show in this frame,
/// with the keypad of its field.
pub fn color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.item_color_shown);
    if !shown && std::mem::take(&mut state.atlas.item_color) {
        widgets::close_keypad(ctx);
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).color(theme::MUTE));
    ui.add_space(4.0);
}

fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, rec: &FurnitureRecord) {
    let id = rec.id;
    match &rec.item {
        Furniture::Text(item) => {
            let mut text = item.text.clone();
            if widgets::text_area(ui, controls, ITEM_TEXT, &mut text, 3).changed() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Text(i) => {
                        i.text = text;
                        true
                    }
                    _ => false,
                });
            }
            ui.add_space(6.0);
            style_rows(ui, state, controls, id, &item.style, true);
            let active = ALIGNS.iter().position(|a| *a == item.align);
            if let Some(i) = widgets::choice_row(ui, controls, ALIGNMENT, &ALIGNMENTS, active) {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Text(t) => {
                        t.align = ALIGNS[i];
                        true
                    }
                    _ => false,
                });
            }
        }
        Furniture::Legend(item) => {
            frame_row(ui, state, controls, rec);
            let mut title = item.title.clone();
            if widgets::text_field(ui, controls, TITLE, TITLE, None, &mut title, true).changed() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Legend(l) => {
                        l.title = title;
                        true
                    }
                    _ => false,
                });
            }
            style_rows(ui, state, controls, id, &item.style, false);
            heading(ui, ENTRIES);
            entry_rows(ui, state, controls, rec);
            ui.add_space(6.0);
            let reset = widgets::button_row(ui, controls, RESET_ENTRIES, !item.entries.is_empty());
            if reset.clicked() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Legend(l) => {
                        l.entries.clear();
                        true
                    }
                    _ => false,
                });
            }
        }
        Furniture::ScaleBar(item) => {
            frame_row(ui, state, controls, rec);
            style_rows(ui, state, controls, id, &item.style, false);
            let text = atlas::item_scale_text(state, item.frame);
            widgets::value_row(ui, SCALE, text.as_deref().unwrap_or("—"));
            hint(ui, SCALE_HINT);
        }
        Furniture::NorthArrow(item) => {
            frame_row(ui, state, controls, rec);
            color_rows(ui, state, controls, id, item.color);
            let mut hide = item.hide_if_varies;
            if widgets::switch_row(ui, controls, HIDE_VARIES, HIDE_VARIES, &mut hide) {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::NorthArrow(n) => {
                        n.hide_if_varies = hide;
                        true
                    }
                    _ => false,
                });
            }
            let variation = atlas::item_variation(state, item.frame);
            let text = variation.map_or_else(|| "—".to_string(), |v| format!("{v:.1}°"));
            widgets::value_row(ui, NORTH_VARIES, &text);
        }
    }
    ui.add_space(6.0);
    let kind = ItemKind::of(&rec.item);
    let delete = widgets::button_row_in(ui, controls, delete_name(kind), true, theme::WARN);
    if delete.clicked() {
        atlas::delete_item(state);
    }
}

/// The font, the style, the size, and the color of the text of an item. The
/// font and the style are for a text item only.
fn style_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    style: &FurnitureText,
    font: bool,
) {
    if font {
        let family = style.font.as_deref().unwrap_or(DEFAULT_FAMILY);
        if widgets::open_row(ui, controls, FONT, family).clicked() {
            state.actions.push(Action::PickFont(FontTarget::Item(id)));
        }
        let warning = fonts::missing(style.font.as_deref(), &state.font_families);
        let mut warning = warning.map(|t| Note::new(t, true, None));
        note_text(ui, None, &mut warning, None);
        let face = Face {
            weight: style.weight,
            italic: style.italic,
        };
        if widgets::open_row(ui, controls, STYLE, &face.name()).clicked() {
            state.atlas.item_page = Some(ItemPage::Style);
            state.atlas.item_color = false;
        }
    }
    let sizes = (*FurnitureText::SIZES.start(), *FurnitureText::SIZES.end());
    let text = format!("{} pt", (style.size * 2.0).round() / 2.0);
    if let Some(size) = value_slider(ui, controls, (SIZE, SIZE), style.size, sizes, &text) {
        let size = (size * 2.0).round() / 2.0;
        atlas::edit_style(state, id, |s| s.size = size);
    }
    color_rows(ui, state, controls, id, style.color);
}

/// The color row of an item and its color picker while it is open.
fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    color: [u8; 3],
) {
    let open = state.atlas.item_color;
    if widgets::color_row(ui, controls, COLOR, color, open).clicked() {
        state.atlas.item_color = !open;
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.item_color {
        ui.add_space(4.0);
        state.atlas.item_color_shown = true;
        let mut rgb = color;
        if widgets::color_picker(ui, controls, COLOR, &mut rgb) {
            atlas::set_item_color(state, id, rgb);
        }
    }
}

/// The name of the frame of an item, or "None".
fn frame_name(state: &AppState, rec: &FurnitureRecord) -> String {
    let names = atlas::frame_names(state);
    let found = names
        .into_iter()
        .find(|(f, _)| Some(*f) == rec.item.frame());
    found.map_or_else(|| NO_FRAME.to_string(), |(_, name)| name)
}

/// The row that opens the page of the frames of the page.
fn frame_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
) {
    let name = frame_name(state, rec);
    if widgets::open_row(ui, controls, MAP_FRAME, &name).clicked() {
        state.atlas.item_page = Some(ItemPage::Frame);
        state.atlas.item_color = false;
    }
}

/// The frames of the page. A tap links the item to the frame and goes back.
fn frame_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
) {
    let names = atlas::frame_names(state);
    if names.is_empty() {
        hint(ui, NO_FRAMES);
        return;
    }
    for (frame, name) in names {
        let on = rec.item.frame() == Some(frame);
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
            atlas::set_item_frame(state, rec.id, Some(frame));
            state.atlas.item_page = None;
        }
    }
}

/// The faces of the font of a text item. A tap sets the face and goes back.
fn style_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
) {
    let Some(style) = atlas::item_style(&rec.item).cloned() else {
        return;
    };
    for f in faces_of(state, style.font.as_deref()) {
        let name = f.name();
        let on = f.weight == style.weight && f.italic == style.italic;
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
            atlas::edit_style(state, rec.id, |s| {
                s.weight = f.weight;
                s.italic = f.italic;
            });
            state.atlas.item_page = None;
        }
    }
}

/// The names of the rows of a legend: the text of each row, with a number
/// after a text that an earlier row has too.
pub fn entry_names(labels: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let n = labels[..i].iter().filter(|l| *l == label).count();
        out.push(match n {
            0 => label.clone(),
            n => format!("{label} {}", n + 1),
        });
    }
    out
}

/// One row for each entry of a legend: a tap on the name opens the page of
/// its name, and the eye hides or shows the entry.
fn entry_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
) {
    let Furniture::Legend(legend) = &rec.item else {
        return;
    };
    let rows = atlas::legend_rows(state, legend);
    if rows.is_empty() {
        let text = if legend.frame.is_some() {
            NO_ENTRIES
        } else {
            NO_FRAMES
        };
        hint(ui, text);
        return;
    }
    let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    for (row, name) in rows.iter().zip(entry_names(&labels)) {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), egui::Sense::hover());
        let center = pos2(rect.right() + 8.0 - TOUCH / 2.0, rect.center().y);
        let eye = Rect::from_center_size(center, vec2(TOUCH, TOUCH));
        let text = Rect::from_min_max(rect.min, pos2(eye.left() - 4.0, rect.max.y));
        let tap = widgets::tap_area(ui, controls, text, &name, true);
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(text));
        let color = widgets::fade(theme::TEXT, !row.hidden);
        painter.text(
            text.left_center(),
            Align2::LEFT_CENTER,
            &row.label,
            theme::body(),
            color,
        );
        let icon = if row.hidden { Icon::EyeOff } else { Icon::Eye };
        let eye_name = widgets::eye_name(&name, !row.hidden);
        let eye = widgets::icon_button_at(ui, controls, eye, &eye_name, icon, false, true);
        let key = (row.layer, row.rule.clone());
        if eye.clicked() {
            let hidden = !row.hidden;
            atlas::edit_entry(state, rec.id, key.clone(), |e| e.hidden = hidden);
        }
        if tap.clicked() {
            state.atlas.entry_name = row.label.clone();
            state.atlas.item_page = Some(ItemPage::Entry(key.0, key.1));
            state.atlas.item_color = false;
        }
    }
}

/// The page of the name of an entry of a legend. An empty name, or the
/// default name, clears the name of the entry.
fn entry_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
    key: (LayerId, String),
) {
    let Furniture::Legend(legend) = &rec.item else {
        return;
    };
    let mut plain = legend.clone();
    if let Some(e) = plain.entries.get_mut(&key) {
        e.name = None;
    }
    let rows = atlas::legend_rows(state, &plain);
    let default = rows.iter().find(|r| (r.layer, &r.rule) == (key.0, &key.1));
    let default = default.map(|r| r.label.clone()).unwrap_or_default();
    let mut text = std::mem::take(&mut state.atlas.entry_name);
    let changed = widgets::text_field(ui, controls, ENTRY_NAME, "Name", None, &mut text, true);
    if changed.changed() {
        let typed = text.trim();
        let name = (!typed.is_empty() && typed != default).then(|| typed.to_string());
        atlas::edit_entry(state, rec.id, key, |e| e.name = name);
    }
    state.atlas.entry_name = text;
    hint(ui, ENTRY_HINT);
}
