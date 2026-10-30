//! The font dialog of the desktop: a search field and the font families of
//! the system. On iOS, the font picker of the system takes its place.

use super::{Action, AppState, FontTarget};
use crate::fonts::DEFAULT_FAMILY;
use eframe::egui;

pub const TITLE: &str = "Font";
pub const SEARCH: &str = "Search";
pub const NO_MATCH: &str = "No fonts match.";
pub const CANCEL: &str = "Cancel";

/// The open font dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct FontDialog {
    /// The labels that get the font.
    pub target: FontTarget,
    pub search: String,
    /// The search field has had the focus.
    focused: bool,
}

impl FontDialog {
    pub fn new(target: FontTarget) -> FontDialog {
        FontDialog {
            target,
            search: String::new(),
            focused: false,
        }
    }
}

/// The families that contain `search`, without regard to case.
pub fn matches<'a>(families: &'a [String], search: &str) -> Vec<&'a String> {
    let search = search.trim().to_lowercase();
    (families.iter())
        .filter(|f| f.to_lowercase().contains(&search))
        .collect()
}

/// Shows the font dialog if it is open.
pub fn show(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut dialog) = state.font_dialog.take() else {
        return;
    };
    let Some(current) = state.font_of(dialog.target) else {
        return;
    };
    let current = current.as_deref().unwrap_or(DEFAULT_FAMILY);
    let mut picked = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("font-dialog")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading(TITLE);
        ui.add_space(6.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut dialog.search)
                .hint_text(SEARCH)
                .desired_width(f32::INFINITY),
        );
        field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, SEARCH));
        if !dialog.focused {
            field.request_focus();
            dialog.focused = true;
        }
        ui.add_space(4.0);
        let found = matches(&state.font_families, &dialog.search);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if found.is_empty() {
                    ui.label(NO_MATCH);
                }
                for family in found {
                    let on = family == current;
                    if ui.selectable_label(on, family.as_str()).clicked() {
                        picked = Some(family.clone());
                    }
                }
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            close |= ui.button(CANCEL).clicked();
        });
    });
    if let Some(family) = picked {
        state.run_action(Action::SetFont(dialog.target, family));
    } else if !close && !modal.should_close() {
        state.font_dialog = Some(dialog);
    }
}
