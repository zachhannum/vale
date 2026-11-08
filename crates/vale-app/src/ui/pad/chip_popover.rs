//! The popover of a field chip: the unit of a measure, the format of the
//! field, the value when the field is empty, Wrap in function, Cut, Copy,
//! and Remove field. The popover of a page value: its format, Cut, Copy,
//! and Remove value.

use eframe::egui::{self, Align2, Color32, Rect, Sense, pos2, vec2};
use vale_store::FieldType;
use vale_style::expr::editor::{Case, Empty, FieldFormat, Piece, Subject, Template};
use vale_style::expr::{Category, FUNCTIONS, Function};

use super::geometry::TOUCH;
use super::popover;
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::expr_help::{ExprHelp, page_label};
use crate::ui::token_field::paint::{BAD, FUNCTION_TEXT};

/// The row of the unit of a measure.
pub const UNIT_ROW: &str = "Unit";
pub const DECIMAL_PLACES: &str = "Decimal places";
pub const THOUSANDS: &str = "Thousands separator";
pub const CASE: &str = "Case";
/// The cases of a text field, in the order of `Case`.
pub const CASES: [&str; 4] = ["As is", "ABC", "abc", "Abc"];
pub const TRIM: &str = "Trim spaces";
pub const WHEN_EMPTY: &str = "When empty";
pub const WRAP: &str = "Wrap in function…";
pub const CUT_FIELD: &str = "Cut field";
pub const COPY_FIELD: &str = "Copy field";
pub const REMOVE_FIELD: &str = "Remove field";
/// The choices of When empty.
pub const NOTHING: &str = "None";
pub const HIDE_GROUP: &str = "Hide group";
pub const EMPTY_TEXT: &str = "Text…";
/// The field of the text when the field is empty.
pub const EMPTY_TEXT_FIELD: &str = "Text";
/// The rows of the popover of a page value.
pub const FORMAT: &str = "Format";
pub const SCALE_FORMATS: [&str; 2] = ["Fraction", "Relative"];
pub const PAGE_UNIT: &str = "Page unit";
pub const PAGE_UNITS: [&str; 2] = ["Centimeters", "Inches"];
pub const DATE_FORMATS: [&str; 2] = ["Long", "ISO"];
pub const CUT_VALUE: &str = "Cut value";
pub const COPY_VALUE: &str = "Copy value";
pub const REMOVE_VALUE: &str = "Remove value";
/// The back button of a page of the popover.
pub const BACK: &str = "Back to field";
/// The most decimal places that the stepper gives.
const MAX_PLACES: i32 = 15;

/// The choice of When empty that uses field `name`.
pub fn use_field(name: &str) -> String {
    format!("Use field: {name}")
}

/// The name of the row of function `f` on the Wrap page.
pub fn wrap_name(f: &Function) -> String {
    format!("Wrap in {}", f.name)
}

/// A page of the popover.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    Empty,
    Wrap,
}

/// What the user did in the popover.
pub enum Action {
    /// A new format, with true for one step and false while typing.
    Format(FieldFormat, bool),
    /// New options of a page value.
    Options(Vec<String>),
    Page(Page),
    /// A new subject, such as a measure with another unit.
    Subject(Subject),
    Wrap(&'static Function),
    Cut,
    Copy,
    Remove,
}

/// The field of a popover.
pub struct Chip<'a> {
    pub subject: &'a Subject,
    pub format: &'a FieldFormat,
    /// The field is in a group, so a field with no value hides the group.
    pub in_group: bool,
}

/// The value of When empty in its row.
fn empty_value(format: &FieldFormat, in_group: bool) -> String {
    match &format.empty {
        Empty::None if in_group => HIDE_GROUP.to_owned(),
        Empty::None => NOTHING.to_owned(),
        Empty::Text(t) => format!("“{t}”"),
        Empty::Field(f) => use_field(f),
    }
}

/// The contents of the popover of `chip` on `page`. `text` holds the text
/// of the field of Text… while the user types it.
pub fn contents(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    page: Page,
    chip: &Chip,
    help: &ExprHelp,
    text: &mut String,
) -> Option<Action> {
    match page {
        Page::Main => main_page(ui, controls, chip, help),
        Page::Empty => empty_page(ui, controls, chip, help, text),
        Page::Wrap => wrap_page(ui, controls, chip),
    }
}

/// The title row of the popover: the name of the field, and its type and
/// its value in the feature of the preview.
fn header(ui: &mut egui::Ui, chip: &Chip, help: &ExprHelp) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let y = row.center().y + 2.0;
    let name_font = egui::FontId::proportional(16.0);
    let painter = ui.painter();
    painter.text(
        pos2(row.left(), y),
        Align2::LEFT_CENTER,
        chip.subject.label(),
        name_font,
        theme::TEXT,
    );
    let kind = help.subject_type(chip.subject).map_or("", FieldType::label);
    let sample = match chip.subject {
        Subject::Field(name) => help.sample(name).and_then(|v| v.label()),
        // A measure shows its value in the format of the chip.
        s => {
            let piece = Piece::Field {
                subject: s.clone(),
                format: chip.format.clone(),
            };
            help.eval(&Template::new(vec![piece]).to_text())
                .and_then(|v| v.label())
        }
    };
    let note = match sample {
        Some(s) if !kind.is_empty() => format!("{kind} · {s}"),
        Some(s) => s,
        None => kind.to_owned(),
    };
    painter.text(
        pos2(row.right(), y),
        Align2::RIGHT_CENTER,
        note,
        theme::body(),
        theme::MUTE,
    );
}

/// A row with a back button and the name of the page.
fn back_row(ui: &mut egui::Ui, controls: &mut Controls, title: &str) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let button = Rect::from_min_size(
        pos2(row.left() - 8.0, row.center().y - TOUCH / 2.0),
        vec2(TOUCH, TOUCH),
    );
    let back = widgets::icon_button_at(
        ui,
        controls,
        button,
        BACK,
        super::icons::Icon::Back,
        false,
        true,
    );
    let at = pos2(button.right() + 4.0, row.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, title, theme::title(), theme::TEXT);
    back.clicked()
}

/// A row with a text in a color that does one thing when the user taps it.
fn action_row(ui: &mut egui::Ui, controls: &mut Controls, name: &str, color: Color32) -> bool {
    popover::divider(ui);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = widgets::hit(ui, controls, row, name, Sense::click(), true, None);
    if resp.is_pointer_button_down_on() {
        let back = row.expand2(vec2(popover::PAD, 0.0));
        ui.painter().rect_filled(back, 0.0, theme::raise());
    }
    let at = pos2(row.left(), row.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, name, theme::body(), color);
    resp.clicked()
}

/// The Decimal places stepper: minus, the count, and plus. No count means
/// no rounding.
fn places_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    places: Option<i32>,
) -> Option<Option<i32>> {
    popover::divider(ui);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let painter = ui.painter();
    painter.text(
        pos2(row.left(), row.center().y),
        Align2::LEFT_CENTER,
        DECIMAL_PLACES,
        theme::body(),
        theme::TEXT,
    );
    let track = Rect::from_min_size(
        pos2(row.right() - 124.0, row.center().y - 18.0),
        vec2(124.0, 36.0),
    );
    painter.rect_filled(track, theme::CONTROL_RADIUS, Color32::from_black_alpha(77));
    let value = places.map_or("–".to_owned(), |p| p.to_string());
    painter.text(
        track.center(),
        Align2::CENTER_CENTER,
        value,
        theme::body(),
        theme::TEXT,
    );
    let minus = Rect::from_center_size(
        pos2(track.left() + 22.0, track.center().y),
        vec2(TOUCH, TOUCH),
    );
    let plus = Rect::from_center_size(
        pos2(track.right() - 22.0, track.center().y),
        vec2(TOUCH, TOUCH),
    );
    let mut out = None;
    let down = places.is_some();
    let up = places.is_none_or(|p| p < MAX_PLACES);
    for (rect, sign, enabled) in [(minus, "−", down), (plus, "+", up)] {
        let name = match sign {
            "−" => format!("Decrease {DECIMAL_PLACES}"),
            _ => format!("Increase {DECIMAL_PLACES}"),
        };
        let resp = widgets::hit(ui, controls, rect, &name, Sense::click(), enabled, None);
        let color = widgets::fade(theme::TEXT, enabled);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            sign,
            theme::title(),
            color,
        );
        if resp.clicked() {
            out = Some(match (sign, places) {
                ("−", Some(0)) => None,
                ("−", Some(p)) => Some(p - 1),
                (_, None) => Some(0),
                (_, Some(p)) => Some(p + 1),
            });
        }
    }
    out
}

fn main_page(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    chip: &Chip,
    help: &ExprHelp,
) -> Option<Action> {
    header(ui, chip, help);
    let mut out = None;
    if let Subject::Measure(m, u) = chip.subject {
        popover::divider(ui);
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let at = pos2(row.left(), row.center().y);
        ui.painter().text(
            at,
            Align2::LEFT_CENTER,
            UNIT_ROW,
            theme::body(),
            theme::TEXT,
        );
        let labels: Vec<&str> = m.units().iter().map(|u| u.label()).collect();
        let w = widgets::small_segments_width(ui, &labels);
        let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 18.0), vec2(w, 36.0));
        let unit = u.unwrap_or(m.default_unit());
        let active = m.units().iter().position(|x| *x == unit).unwrap_or(0);
        if let Some(i) = widgets::small_segments(ui, controls, seg, &labels, active) {
            out = Some(Action::Subject(Subject::Measure(*m, Some(m.units()[i]))));
        }
    }
    let format = chip.format;
    let kind = help.subject_type(chip.subject);
    if matches!(kind, Some(FieldType::Integer | FieldType::Real)) {
        if let Some(places) = places_row(ui, controls, format.places) {
            out = Some(Action::Format(
                FieldFormat {
                    places,
                    ..format.clone()
                },
                true,
            ));
        }
        popover::divider(ui);
        let mut on = format.thousands;
        if widgets::switch_row(ui, controls, THOUSANDS, THOUSANDS, &mut on) {
            out = Some(Action::Format(
                FieldFormat {
                    thousands: on,
                    ..format.clone()
                },
                true,
            ));
        }
    }
    if matches!(kind, Some(FieldType::Text) | None) {
        let cases = [Case::AsIs, Case::Upper, Case::Lower, Case::Title];
        let active = cases.iter().position(|c| *c == format.case).unwrap_or(0);
        if let Some(i) = segment_row(ui, controls, CASE, &CASES, active) {
            out = Some(Action::Format(
                FieldFormat {
                    case: cases[i],
                    ..format.clone()
                },
                true,
            ));
        }
        popover::divider(ui);
        let mut on = format.trim;
        if widgets::switch_row(ui, controls, TRIM, TRIM, &mut on) {
            out = Some(Action::Format(
                FieldFormat {
                    trim: on,
                    ..format.clone()
                },
                true,
            ));
        }
    }
    popover::divider(ui);
    let value = empty_value(format, chip.in_group);
    if widgets::open_row(ui, controls, WHEN_EMPTY, &value).clicked() {
        out = Some(Action::Page(Page::Empty));
    }
    if action_row(ui, controls, WRAP, theme::ACCENT) {
        out = Some(Action::Page(Page::Wrap));
    }
    if action_row(ui, controls, CUT_FIELD, theme::ACCENT) {
        out = Some(Action::Cut);
    }
    if action_row(ui, controls, COPY_FIELD, theme::ACCENT) {
        out = Some(Action::Copy);
    }
    if action_row(ui, controls, REMOVE_FIELD, BAD) {
        out = Some(Action::Remove);
    }
    out
}

/// A row with a name and small segments of `choices` at its right.
fn segment_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    choices: &[&str],
    active: usize,
) -> Option<usize> {
    popover::divider(ui);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let at = pos2(row.left(), row.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    let w = widgets::small_segments_width(ui, choices);
    let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 18.0), vec2(w, 36.0));
    widgets::small_segments(ui, controls, seg, choices, active)
}

/// The contents of the popover of page value `name` with `options`: the
/// name and the value of the page, the format of the scale and of the
/// date, Cut, Copy, and Remove value.
pub fn page_contents(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    (name, options): (&'static str, &[String]),
    help: &ExprHelp,
) -> Option<Action> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let y = row.center().y + 2.0;
    let piece = Piece::Page {
        name,
        options: options.to_vec(),
    };
    let sample = help.eval(&Template::new(vec![piece]).to_text());
    let painter = ui.painter();
    let name_font = egui::FontId::proportional(16.0);
    let at = pos2(row.left(), y);
    painter.text(
        at,
        Align2::LEFT_CENTER,
        page_label(name),
        name_font,
        theme::TEXT,
    );
    if let Some(s) = sample.and_then(|v| v.label()) {
        let at = pos2(row.right(), y);
        painter.text(at, Align2::RIGHT_CENTER, s, theme::body(), theme::MUTE);
    }
    let mut out = None;
    let option = |i: usize| options.get(i).map_or("", String::as_str);
    match name {
        "scale" => {
            let relative = option(0) == "relative";
            let active = usize::from(relative);
            if let Some(i) = segment_row(ui, controls, FORMAT, &SCALE_FORMATS, active) {
                out = Some(match i {
                    0 => vec!["fraction".to_owned()],
                    _ => vec!["relative".to_owned(), "cm".to_owned()],
                });
            }
            if relative {
                let active = usize::from(option(1) == "in");
                if let Some(i) = segment_row(ui, controls, PAGE_UNIT, &PAGE_UNITS, active) {
                    let unit = if i == 0 { "cm" } else { "in" };
                    out = Some(vec!["relative".to_owned(), unit.to_owned()]);
                }
            }
        }
        "date" => {
            let active = usize::from(option(0) == "iso");
            if let Some(i) = segment_row(ui, controls, FORMAT, &DATE_FORMATS, active) {
                let format = if i == 0 { "long" } else { "iso" };
                out = Some(vec![format.to_owned()]);
            }
        }
        _ => {}
    }
    let mut out = out.map(Action::Options);
    if action_row(ui, controls, CUT_VALUE, theme::ACCENT) {
        out = Some(Action::Cut);
    }
    if action_row(ui, controls, COPY_VALUE, theme::ACCENT) {
        out = Some(Action::Copy);
    }
    if action_row(ui, controls, REMOVE_VALUE, BAD) {
        out = Some(Action::Remove);
    }
    out
}

fn empty_page(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    chip: &Chip,
    help: &ExprHelp,
    text: &mut String,
) -> Option<Action> {
    let mut out = None;
    if back_row(ui, controls, WHEN_EMPTY) {
        out = Some(Action::Page(Page::Main));
    }
    let format = chip.format;
    let with = |empty: Empty, step: bool| {
        Some(Action::Format(
            FieldFormat {
                empty,
                ..format.clone()
            },
            step,
        ))
    };
    let none = if chip.in_group { HIDE_GROUP } else { NOTHING };
    let is_text = matches!(format.empty, Empty::Text(_));
    if widgets::option_row(ui, controls, none, none, &[], format.empty == Empty::None).clicked() {
        out = with(Empty::None, true);
    }
    if widgets::option_row(ui, controls, EMPTY_TEXT, EMPTY_TEXT, &[], is_text).clicked() && !is_text
    {
        text.clear();
        out = with(Empty::Text(String::new()), true);
    }
    if is_text {
        let field = widgets::text_field(
            ui,
            controls,
            EMPTY_TEXT_FIELD,
            EMPTY_TEXT_FIELD,
            None,
            text,
            true,
        );
        if field.changed() {
            out = with(Empty::Text(text.clone()), false);
        }
    }
    let scroll = egui::ScrollArea::vertical()
        .max_height(5.0 * PANEL_ROW)
        .id_salt("pad-chip-empty");
    scroll.show(ui, |ui| {
        for other in (help.fields.iter()).filter(|f| chip.subject.field() != Some(f.as_str())) {
            let name = use_field(other);
            let on = format.empty == Empty::Field(other.clone());
            if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
                out = with(Empty::Field(other.clone()), true);
            }
        }
    });
    out
}

fn wrap_page(ui: &mut egui::Ui, controls: &mut Controls, chip: &Chip) -> Option<Action> {
    let mut out = None;
    if back_row(ui, controls, &chip.subject.label()) {
        out = Some(Action::Page(Page::Main));
    }
    let categories = [
        Category::Text,
        Category::Math,
        Category::Logic,
        Category::Convert,
    ];
    let scroll = egui::ScrollArea::vertical()
        .max_height(6.0 * TOUCH)
        .id_salt("pad-chip-wrap");
    scroll.show(ui, |ui| {
        for category in categories {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
            let at = pos2(row.left(), row.bottom() - 4.0);
            let font = egui::FontId::proportional(12.0);
            ui.painter()
                .text(at, Align2::LEFT_BOTTOM, category.name(), font, theme::MUTE);
            for f in FUNCTIONS.iter().filter(|f| f.category == category) {
                let (row, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
                let resp =
                    widgets::hit(ui, controls, row, &wrap_name(f), Sense::click(), true, None);
                if resp.is_pointer_button_down_on() {
                    ui.painter().rect_filled(row, 8.0, theme::raise());
                }
                let painter = ui.painter();
                let y = row.center().y;
                painter.text(
                    pos2(row.left() + 4.0, y),
                    Align2::LEFT_CENTER,
                    f.name,
                    theme::mono(),
                    FUNCTION_TEXT,
                );
                painter.text(
                    pos2(row.left() + 120.0, y),
                    Align2::LEFT_CENTER,
                    f.hint,
                    theme::small(),
                    theme::MUTE,
                );
                if resp.clicked() {
                    out = Some(Action::Wrap(f));
                }
            }
        }
    });
    out
}
