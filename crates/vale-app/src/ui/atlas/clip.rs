//! Cut, Copy, and Paste of a map frame or an item of furniture between the
//! pages of the atlas. The app keeps the copy. The system clipboard gets a
//! short text, so that ⌘V sends a paste.

use eframe::egui;
use vale_store::{FrameId, FrameRecord, FrameShape, Furniture, FurnitureRecord, MapRecord};

use super::{AtlasSelection, AtlasState, open_page};
use crate::ui::AppState;
use crate::ui::pad::widgets::{COPY, CUT, PASTE};

/// The move of a paste on a page that has the same frame or item at the
/// same place, in millimeters to the right and down.
pub const NUDGE_MM: f64 = 5.0;

/// A copied frame with its map, or a copied item of furniture.
#[derive(Clone)]
pub enum Clip {
    Frame { frame: FrameRecord, map: MapRecord },
    Item(FurnitureRecord),
}

impl AtlasState {
    /// True if the app holds a copied frame or item.
    pub fn can_paste(&self) -> bool {
        self.clip.is_some()
    }
}

/// The Edit menu of the Atlas workspace on the desktop: Cut, Copy, Paste,
/// and "Delete selection".
pub fn edit_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let keys = |key| ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key));
    ui.menu_button("Edit", |ui| {
        let any = super::has_selection(state);
        let button = |label, key| egui::Button::new(label).shortcut_text(keys(key));
        if ui.add_enabled(any, button(CUT, egui::Key::X)).clicked() {
            cut(&ctx, state);
            ui.close();
        }
        if ui.add_enabled(any, button(COPY, egui::Key::C)).clicked() {
            copy(&ctx, state);
            ui.close();
        }
        let paste_on = state.atlas.can_paste();
        if ui.add_enabled(paste_on, button(PASTE, egui::Key::V)).clicked() {
            paste(state);
            ui.close();
        }
        let delete = egui::Button::new(crate::ui::select::DELETE);
        if ui.add_enabled(any, delete).clicked() {
            super::delete_selection(state);
            ui.close();
        }
    });
}

/// Cut, Copy, and Paste from the keys: ⌘X, ⌘C, and ⌘V.
pub fn keys(ui: &egui::Ui, state: &mut AppState) {
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            egui::Event::Copy => {
                copy(ui.ctx(), state);
            }
            egui::Event::Cut => {
                cut(ui.ctx(), state);
            }
            egui::Event::Paste(_) => {
                paste(state);
            }
            _ => {}
        }
    }
}

/// Copies the selected frame and its map, or the selected item. Returns
/// false if nothing is selected.
pub fn copy(ctx: &egui::Context, state: &mut AppState) -> bool {
    let atlas = &state.doc.project.atlas;
    let (clip, text) = match state.atlas.selection {
        AtlasSelection::Frame(id) => {
            let Some(frame) = atlas.frame(id) else {
                return false;
            };
            let Some(map) = atlas.map(frame.map) else {
                return false;
            };
            let text = map.name.clone();
            let clip = Clip::Frame {
                frame: frame.clone(),
                map: map.clone(),
            };
            (clip, text)
        }
        AtlasSelection::Item(id) => {
            let Some(item) = atlas.furniture(id) else {
                return false;
            };
            let text = match &item.item {
                Furniture::Text(text) => text.template.clone(),
                Furniture::Legend(_) => "Legend".to_string(),
                Furniture::ScaleBar(_) => "Scale bar".to_string(),
                Furniture::NorthArrow(_) => "North arrow".to_string(),
                Furniture::ColorKey(_) => "Color key".to_string(),
            };
            (Clip::Item(item.clone()), text)
        }
        AtlasSelection::None | AtlasSelection::Page => return false,
    };
    crate::clipboard::set(ctx, &text);
    // On iPadOS, ⌘V reads the clipboard of the window.
    #[cfg(target_os = "ios")]
    ctx.copy_text(text);
    state.atlas.clip = Some(clip);
    true
}

/// Copies the selection and deletes it. Returns false if nothing is
/// selected.
pub fn cut(ctx: &egui::Context, state: &mut AppState) -> bool {
    if !copy(ctx, state) {
        return false;
    }
    super::delete_selection(state);
    true
}

/// Puts the copied frame or item on the open page and selects it. A frame
/// gets a new copy of its map. Returns false if the app holds no copy.
pub fn paste(state: &mut AppState) -> bool {
    let Some(clip) = state.atlas.clip.clone() else {
        return false;
    };
    let page = open_page(state).clone();
    let atlas = &mut state.doc.project.atlas;
    let selection = match clip {
        Clip::Frame { mut frame, map } => {
            let taken = |shape: &FrameShape| page.frames.iter().any(|f| f.shape == *shape);
            while taken(&frame.shape) {
                frame.shape = nudge_shape(&frame.shape);
            }
            frame.map = atlas.add_map_copy(&map);
            let Some(id) = atlas.add_frame(page.id, frame) else {
                return false;
            };
            AtlasSelection::Frame(id)
        }
        Clip::Item(mut item) => {
            let kind = item.item.kind_id();
            let taken = |rect: &[f64; 4]| {
                page.furniture
                    .iter()
                    .any(|f| f.item.kind_id() == kind && f.rect == *rect)
            };
            while taken(&item.rect) {
                item.rect = nudge_rect(item.rect);
            }
            let on_page = |f: &FrameId| page.frames.iter().any(|p| p.id == *f);
            let frame = item.item.frame().filter(on_page);
            let frame = frame.or_else(|| super::largest_frame(&page));
            item.item.set_frame(frame);
            let Some(id) = atlas.add_furniture(page.id, item.rect, item.item) else {
                return false;
            };
            state.atlas.item_page = None;
            AtlasSelection::Item(id)
        }
    };
    state.atlas.selection = selection;
    state.atlas.vertices.clear();
    true
}

fn nudge_rect([x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    [x0 + NUDGE_MM, y0 + NUDGE_MM, x1 + NUDGE_MM, y1 + NUDGE_MM]
}

fn nudge_shape(shape: &FrameShape) -> FrameShape {
    match shape {
        FrameShape::Rectangle(rect) => FrameShape::Rectangle(nudge_rect(*rect)),
        FrameShape::Polygon(rings) => FrameShape::Polygon(
            rings
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|[x, y]| [x + NUDGE_MM, y + NUDGE_MM])
                        .collect()
                })
                .collect(),
        ),
    }
}
