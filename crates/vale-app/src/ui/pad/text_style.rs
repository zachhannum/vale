//! The Text section of a label class and of a text part of an item of
//! furniture: the font, the face, the size, the case, the letter spacing,
//! the color, the opacity, and the halo.

use eframe::egui;
use vale_labeler::Face;
use vale_store::{FurnitureText, Rgb, TextCase};

use super::labels::{
    CASE, COLOR, HALO, HALO_COLOR, HALO_WIDTH, LETTER_SPACING, OPACITY, STYLE, color_rows,
    faces_of, percent, value_slider,
};
use super::note_text;
use super::widgets::{self, Controls};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::atlas::ClassColor;
use crate::ui::{AppState, Note};

/// The limits and the names of a Text section.
#[derive(Copy, Clone, Debug)]
pub struct TextSpec {
    /// The name of the size slider. Its label is "Size".
    pub size_name: &'static str,
    /// The step of the size, in points.
    pub size_step: f64,
    pub sizes: (f64, f64),
    pub spacings: (f64, f64),
    pub halo_widths: (f64, f64),
    /// The colors of the text and of the halo for the color picker.
    pub colors: (ClassColor, ClassColor),
}

/// A change of a text style.
#[derive(Clone, Debug, PartialEq)]
pub enum TextEdit {
    Size(f64),
    LetterSpacing(f64),
    Color(Rgb),
    Opacity(f64),
    Halo(bool),
    HaloColor(Rgb),
    HaloWidth(f64),
    Face(Face),
    Case(TextCase),
}

impl TextEdit {
    pub fn apply(&self, s: &mut FurnitureText) {
        match *self {
            TextEdit::Size(v) => s.size = v,
            TextEdit::LetterSpacing(v) => s.letter_spacing = v,
            TextEdit::Color(c) => s.color = c,
            TextEdit::Opacity(v) => s.opacity = v,
            TextEdit::Halo(on) => s.halo = on,
            TextEdit::HaloColor(c) => s.halo_color = c,
            TextEdit::HaloWidth(v) => s.halo_width = v,
            TextEdit::Face(f) => {
                s.weight = f.weight;
                s.italic = f.italic;
            }
            TextEdit::Case(c) => s.case = c,
        }
    }

    /// True for a change that is one undo step by itself: a tap, and not
    /// a drag.
    pub fn step(&self) -> bool {
        matches!(
            self,
            TextEdit::Halo(_) | TextEdit::Face(_) | TextEdit::Case(_)
        )
    }
}

/// What the user did in a Text section.
#[derive(Clone, Debug, PartialEq)]
pub enum TextEvent {
    /// The Font row asks for the font picker.
    PickFont,
    /// The Style row opens the page of the faces.
    OpenFaces,
    /// The Case row opens the page of the cases.
    OpenCases,
    Edit(TextEdit),
}

/// A color row and its color picker while it is open, for a color with no
/// alpha. Returns the new color.
pub fn rgb_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    name: &str,
    which: ClassColor,
    [r, g, b]: Rgb,
) -> Option<Rgb> {
    let c = vale_style::Color { r, g, b, a: 255 };
    color_rows(ui, state, controls, (name, which), c).map(|c| [c.r, c.g, c.b])
}

/// `v` to the nearest multiple of `step`.
fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// The rows of a Text section. Returns what the user did.
pub fn section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    style: &FurnitureText,
    spec: TextSpec,
) -> Option<TextEvent> {
    let mut event = None;
    let family = style.font.as_deref().unwrap_or(DEFAULT_FAMILY);
    if widgets::open_row(ui, controls, super::fonts::FONT, family).clicked() {
        event = Some(TextEvent::PickFont);
    }
    let warning = fonts::missing(style.font.as_deref(), &state.font_families);
    let mut warning = warning.map(|t| Note::new(t, true, None));
    note_text(ui, None, &mut warning, None);
    let face = Face {
        weight: style.weight,
        italic: style.italic,
    };
    if widgets::open_row(ui, controls, STYLE, &face.name()).clicked() {
        event = Some(TextEvent::OpenFaces);
    }

    let step = spec.size_step;
    let text = format!("{} pt", round_to(style.size, step));
    let names = (spec.size_name, "Size");
    if let Some(size) = value_slider(ui, controls, names, style.size, spec.sizes, &text) {
        event = Some(TextEvent::Edit(TextEdit::Size(round_to(size, step))));
    }
    if widgets::open_row(ui, controls, CASE, style.case.name()).clicked() {
        event = Some(TextEvent::OpenCases);
    }
    let names = (LETTER_SPACING, LETTER_SPACING);
    let text = percent(style.letter_spacing);
    let spacing = value_slider(
        ui,
        controls,
        names,
        style.letter_spacing,
        spec.spacings,
        &text,
    );
    if let Some(spacing) = spacing {
        let spacing = round_to(spacing, 0.01);
        event = Some(TextEvent::Edit(TextEdit::LetterSpacing(spacing)));
    }

    let (text_color, halo_color) = spec.colors;
    if let Some(c) = rgb_rows(ui, state, controls, COLOR, text_color, style.color) {
        event = Some(TextEvent::Edit(TextEdit::Color(c)));
    }
    let text = percent(style.opacity);
    let names = (OPACITY, OPACITY);
    if let Some(opacity) = value_slider(ui, controls, names, style.opacity, (0.0, 1.0), &text) {
        event = Some(TextEvent::Edit(TextEdit::Opacity(round_to(opacity, 0.01))));
    }

    let mut halo = style.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        event = Some(TextEvent::Edit(TextEdit::Halo(halo)));
    }
    if style.halo {
        let c = rgb_rows(
            ui,
            state,
            controls,
            HALO_COLOR,
            halo_color,
            style.halo_color,
        );
        if let Some(c) = c {
            event = Some(TextEvent::Edit(TextEdit::HaloColor(c)));
        }
        let names = (HALO_WIDTH, HALO_WIDTH);
        let text = format!("{:.1} pt", style.halo_width);
        let width = value_slider(
            ui,
            controls,
            names,
            style.halo_width,
            spec.halo_widths,
            &text,
        );
        if let Some(width) = width {
            event = Some(TextEvent::Edit(TextEdit::HaloWidth(round_to(width, 0.1))));
        }
    }
    event
}

/// The faces of the font family `font`, one row each, with a mark on the
/// face `current`. Returns the face of a tap.
pub fn face_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    font: Option<&str>,
    current: Face,
) -> Option<Face> {
    let mut chosen = None;
    for f in faces_of(state, font) {
        let name = f.name();
        let on = f.weight == current.weight && f.italic == current.italic;
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
            chosen = Some(f);
        }
    }
    chosen
}

/// The cases, one row each, with a mark on `current`. Returns the case of
/// a tap.
pub fn case_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    current: TextCase,
) -> Option<TextCase> {
    let mut chosen = None;
    for case in TextCase::ALL {
        let name = case.name();
        if widgets::option_row(ui, controls, name, name, &[], case == current).clicked() {
            chosen = Some(case);
        }
    }
    chosen
}
