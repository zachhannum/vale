//! The question before a change that deletes the run.

use eframe::egui::{self, CornerRadius, Rect, pos2, vec2};
use isotherm_world::Planet;
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, BODY_PAD, Controls};

use super::{IsothermApp, MARGIN, plain_button};

pub const ASK_TITLE: &str = "Start the run again?";
pub const ASK_TEXT: &str = "This change deletes the current run.";
pub const START_AGAIN: &str = "Start again";
const CANCEL: &str = "Cancel";

/// The width of the card of the question.
const WIDTH: f32 = 340.0;

/// A change of the world that deletes the run.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Planet(Box<Planet>),
    SeaLevel(f64),
    Undo,
    Redo,
}

impl IsothermApp {
    /// The card that asks to delete the run, over a dark fill that takes
    /// each touch. "Start again" deletes the run and applies the change,
    /// and "Cancel" drops the change.
    pub(super) fn ask_card(
        &mut self,
        ctx: &egui::Context,
        controls: &mut Controls,
        screen: Rect,
        safe: Rect,
    ) {
        // The card covers each other control.
        controls.clear();
        let width = WIDTH.min(safe.width() - 2.0 * MARGIN);
        let text_width = width - 2.0 * BODY_PAD;
        let galley = ctx
            .fonts_mut(|f| f.layout(ASK_TEXT.to_owned(), theme::body(), theme::MUTE, text_width));
        let title = 24.0;
        let buttons = vale_shell::pad::metrics::TOUCH;
        let height = 20.0 + title + 6.0 + galley.size().y + 20.0 + buttons + 14.0;
        let rect = Rect::from_center_size(safe.center(), vec2(width, height));
        self.cards.push(("ask", rect));
        let radius = CornerRadius::same(theme::CARD_RADIUS as u8);
        let mut start = false;
        let mut cancel = false;
        widgets::form_card(ctx, "ask", screen, rect, radius, false, |ui| {
            let left = rect.left() + BODY_PAD;
            let top = rect.top() + 20.0;
            let label = |ui: &mut egui::Ui, rect: Rect, text: egui::RichText| {
                let layout = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
                child.add(egui::Label::new(text).wrap().selectable(false));
            };
            let heading = egui::RichText::new(ASK_TITLE)
                .font(theme::heading(ctx))
                .color(theme::TEXT);
            label(
                ui,
                Rect::from_min_size(pos2(left, top), vec2(text_width, title)),
                heading,
            );
            let at = pos2(left, top + title + 6.0);
            let text = egui::RichText::new(ASK_TEXT)
                .font(theme::body())
                .color(theme::MUTE);
            label(
                ui,
                Rect::from_min_size(at, vec2(text_width, galley.size().y)),
                text,
            );
            let y = rect.bottom() - 14.0 - buttons / 2.0;
            let right = pos2(rect.right() - BODY_PAD + 8.0, y);
            let accent = widgets::accent_button(ui, controls, right, START_AGAIN);
            start = accent.clicked();
            let right = pos2(accent.rect.left() - 8.0, y);
            cancel = plain_button(ui, controls, right, CANCEL).clicked();
        });
        // The Escape key cancels, as on macOS.
        cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if start {
            self.start_again();
        } else if cancel {
            self.ask = None;
        }
    }
}
