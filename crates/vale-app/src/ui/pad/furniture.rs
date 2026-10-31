//! The panels of the items of furniture of a page: text, a legend, a scale
//! bar, and a north arrow.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_labeler::Face;
use vale_store::{
    DivisionSpacing, FillKind, FillPattern, Furniture, FurnitureId, FurnitureRecord, FurnitureText,
    LayerId, ScaleBarItem, ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, TextAlign,
    TickFrequency, UnitLabelStyle, UnitPosition,
};

use super::atlas::{keypad_below, panel_width};
use super::geometry::{Rects, TOUCH};
use super::icons::{self, Icon};
use super::labels::value_slider;
use super::raster::heading;
use super::text_style::{self, TextEvent, TextSpec, rgb_rows};
use super::widgets::{self, Controls, HIGHLIGHT_INSET, Keys, PANEL_ROW};
use super::{Card, CardTitle, show_card, theme};
use crate::ui::atlas::{
    self, AtlasSelection, BarPage, BarPick, ClassColor, ItemKind, ItemPage, TextPart,
};
use crate::ui::{Action, AppState, FontTarget};

pub use super::fonts::FONT;
pub use super::labels::{
    CASE, COLOR, HALO, HALO_COLOR, HALO_WIDTH, LETTER_SPACING, OPACITY, STYLE,
};

/// The field of the text of a text item.
pub const ITEM_TEXT: &str = "Item text";
/// The size slider of a text part.
pub const SIZE: &str = "Size";
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
/// The heading of the Text section on a page with other rows.
pub const TEXT: &str = "Text";
pub const ARROW_COLOR: &str = "Arrow color";
pub const HIDE_VARIES: &str = "Hide where north varies";
/// The heading of the Text section of a north arrow.
pub const LETTER: &str = "Letter";
pub const NONE: &str = "None";
pub const CUSTOM_LABEL: &str = "Label";
pub const SECOND_LABEL: &str = "Second label";
pub const GAP: &str = "Gap";
pub const DIVISIONS: &str = "Divisions";
pub const SUBDIVISIONS: &str = "Subdivisions";
pub const BEFORE_ZERO: &str = "Before zero";
pub const AUTO_VALUE: &str = "Automatic value";
pub const DIVISION_VALUE: &str = "Division value";
pub const SECOND_VALUE: &str = "Second value";
pub const BAR_HEIGHT: &str = "Bar height";
pub const FILL: &str = "Fill";
pub const FILL_1: &str = "Fill 1";
pub const FILL_2: &str = "Fill 2";
pub const OUTLINE: &str = "Outline";
pub const LINE_COLOR: &str = "Line color";
pub const LINE_WIDTH: &str = "Line width";
pub const BACKGROUND: &str = "Background";
pub const BACKGROUND_COLOR: &str = "Background color";
pub const ANGLE: &str = "Angle";
pub const SPACING: &str = "Spacing";
pub const POSITION: &str = "Position";
/// The choices of the position of the marks and the numbers, in the order
/// of `ScalePosition::ALL`.
pub const POSITIONS: [&str; 3] = ["Above", "Below", "Center"];
pub const DIVISION_HEIGHT: &str = "Division height";
pub const SUBDIVISION_HEIGHT: &str = "Subdivision height";
pub const THOUSANDS: &str = "Thousands separator";
pub const SHOW_ZERO: &str = "Show zero";
/// The decimals that each number picks.
pub const AUTO_DECIMALS: &str = "Automatic";

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

/// The key of the card of a page.
fn page_key(page: &ItemPage) -> &'static str {
    match page {
        ItemPage::Frame => "pad-atlas-item-frame",
        ItemPage::Face(_) => "pad-atlas-item-face",
        ItemPage::Case(_) => "pad-atlas-item-case",
        ItemPage::Text(_) => "pad-atlas-item-text",
        ItemPage::Entry(..) => "pad-atlas-item-entry",
        ItemPage::Bar(p) => match p {
            BarPage::Style => "pad-atlas-bar-style",
            BarPage::Units => "pad-atlas-bar-units",
            BarPage::Divisions => "pad-atlas-bar-divisions",
            BarPage::Bar => "pad-atlas-bar-bar",
            BarPage::Marks => "pad-atlas-bar-marks",
            BarPage::Numbers => "pad-atlas-bar-numbers",
        },
        ItemPage::Pick(_) => "pad-atlas-bar-pick",
        ItemPage::Fill(_) => "pad-atlas-bar-fill",
    }
}

/// The title of a page of the panel of an item of `kind`.
fn page_title(page: Option<&ItemPage>, kind: ItemKind) -> &'static str {
    match page {
        None => kind.name(),
        Some(ItemPage::Frame) => MAP_FRAME,
        Some(ItemPage::Face(_)) => STYLE,
        Some(ItemPage::Case(_)) => CASE,
        Some(ItemPage::Text(TextPart::Title)) => TITLE,
        Some(ItemPage::Text(_)) => ENTRIES,
        Some(ItemPage::Entry(..)) => ENTRY,
        Some(ItemPage::Bar(p)) => p.name(),
        Some(ItemPage::Pick(p)) => p.name(),
        Some(ItemPage::Fill(false)) => FILL_1,
        Some(ItemPage::Fill(true)) => FILL_2,
    }
}

/// Opens a page of the panel, or the top of the panel. An open color
/// picker closes.
fn go(ui: &egui::Ui, state: &mut AppState, page: Option<ItemPage>) {
    state.atlas.item_page = page;
    state.atlas.label_color = None;
    widgets::close_keypad(ui.ctx());
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
        Some(p) => {
            let back = page_title(p.parent().as_ref(), kind);
            let title = match (&rec.item, p) {
                (Furniture::ScaleBar(bar), ItemPage::Fill(_)) if fills(bar.kind).len() == 1 => FILL,
                _ => page_title(Some(p), kind),
            };
            (page_key(p), title, Some(back))
        }
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
    let parent = page.as_ref().and_then(ItemPage::parent);
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.item_page = parent.clone();
            state.atlas.label_color = None;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        match page {
            None => body(ui, state, controls, &rec),
            Some(ItemPage::Frame) => frame_page(ui, state, controls, &rec),
            Some(ItemPage::Face(part)) => face_page(ui, state, controls, &rec, part),
            Some(ItemPage::Case(part)) => case_page(ui, state, controls, &rec, part),
            Some(ItemPage::Text(part)) => legend_page(ui, state, controls, &rec, part),
            Some(ItemPage::Entry(layer, rule)) => {
                entry_page(ui, state, controls, &rec, (layer, rule));
            }
            Some(ItemPage::Bar(p)) => {
                if let Furniture::ScaleBar(bar) = &rec.item {
                    bar_page(ui, state, controls, rec.id, bar, p);
                }
            }
            Some(ItemPage::Pick(p)) => {
                if let Furniture::ScaleBar(bar) = &rec.item {
                    pick_page(ui, state, controls, rec.id, bar, p);
                }
            }
            Some(ItemPage::Fill(second)) => {
                if let Furniture::ScaleBar(bar) = &rec.item {
                    fill_page(ui, state, controls, rec.id, bar, second);
                }
            }
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// An empty state.
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
            text_rows(ui, state, controls, id, TextPart::Text, &item.style);
            let active = ALIGNS.iter().position(|a| *a == item.align);
            if let Some(i) = widgets::choice_row(ui, controls, ALIGNMENT, &ALIGNMENTS, active) {
                atlas::item_step(state, id, |r| match &mut r.item {
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
            let title = fit(ui, TITLE, &item.title);
            if widgets::open_row(ui, controls, TITLE, &title).clicked() {
                go(ui, state, Some(ItemPage::Text(TextPart::Title)));
            }
            if widgets::open_row(ui, controls, ENTRIES, "").clicked() {
                go(ui, state, Some(ItemPage::Text(TextPart::Entries)));
            }
        }
        Furniture::ScaleBar(bar) => {
            let style = BarPage::Style.name();
            if widgets::open_row(ui, controls, style, bar.kind.label()).clicked() {
                go(ui, state, Some(ItemPage::Bar(BarPage::Style)));
            }
            frame_row(ui, state, controls, rec);
            for page in &BarPage::ALL[1..] {
                let value = match page {
                    BarPage::Units => units_text(bar),
                    BarPage::Divisions => bar.divisions.to_string(),
                    _ => String::new(),
                };
                if widgets::open_row(ui, controls, page.name(), &value).clicked() {
                    go(ui, state, Some(ItemPage::Bar(*page)));
                }
            }
        }
        Furniture::NorthArrow(item) => {
            frame_row(ui, state, controls, rec);
            let c = rgb_rows(
                ui,
                state,
                controls,
                ARROW_COLOR,
                ClassColor::Arrow,
                item.color,
            );
            if let Some(c) = c {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::NorthArrow(n) => {
                        n.color = c;
                        true
                    }
                    _ => false,
                });
            }
            let mut hide = item.hide_if_varies;
            if widgets::switch_row(ui, controls, HIDE_VARIES, HIDE_VARIES, &mut hide) {
                atlas::item_step(state, id, |r| match &mut r.item {
                    Furniture::NorthArrow(n) => {
                        n.hide_if_varies = hide;
                        true
                    }
                    _ => false,
                });
            }
            heading(ui, LETTER);
            text_rows(
                ui,
                state,
                controls,
                id,
                TextPart::Letter,
                &item.letter_style,
            );
        }
    }
    ui.add_space(6.0);
    let kind = ItemKind::of(&rec.item);
    let delete = widgets::button_row_in(ui, controls, delete_name(kind), true, theme::WARN);
    if delete.clicked() {
        atlas::delete_item(state);
    }
}

/// `text`, cut with "…" to fit at the right of a row named `name`.
fn fit(ui: &egui::Ui, name: &str, text: &str) -> String {
    let width = |t: &str| {
        let galley = ui
            .painter()
            .layout_no_wrap(t.to_string(), theme::body(), theme::TEXT);
        galley.size().x
    };
    let room = ui.available_width() - width(name) - widgets::ICON - 32.0;
    let text = text.lines().next().unwrap_or_default();
    if width(text) <= room {
        return text.to_string();
    }
    let mut cut: String = text.to_string();
    while !cut.is_empty() && width(&format!("{cut}…")) > room {
        cut.pop();
    }
    if cut.is_empty() {
        String::new()
    } else {
        format!("{}…", cut.trim_end())
    }
}

/// The units of a scale bar for the Units row, for example "km, mi".
fn units_text(bar: &ScaleBarItem) -> String {
    match bar.second_unit {
        Some(u) => format!("{}, {}", bar.unit.abbreviation(), u.abbreviation()),
        None => bar.unit.abbreviation().to_string(),
    }
}

/// The Text section of a text part of an item.
fn text_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    part: TextPart,
    style: &FurnitureText,
) {
    let range = |r: std::ops::RangeInclusive<f64>| (*r.start(), *r.end());
    let spec = TextSpec {
        size_name: SIZE,
        size_step: 0.5,
        sizes: range(FurnitureText::SIZES),
        spacings: range(FurnitureText::LETTER_SPACINGS),
        halo_widths: range(FurnitureText::HALO_WIDTHS),
        colors: (ClassColor::ItemText(part), ClassColor::ItemHalo(part)),
    };
    match text_style::section(ui, state, controls, style, spec) {
        Some(TextEvent::PickFont) => {
            let target = FontTarget::Item(id, part);
            state.actions.push(Action::PickFont(target));
        }
        Some(TextEvent::OpenFaces) => go(ui, state, Some(ItemPage::Face(part))),
        Some(TextEvent::OpenCases) => go(ui, state, Some(ItemPage::Case(part))),
        Some(TextEvent::Edit(edit)) => {
            atlas::edit_text(state, id, part, edit.step(), |s| edit.apply(s));
        }
        None => {}
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
        go(ui, state, Some(ItemPage::Frame));
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
            atlas::item_step(state, rec.id, |r| {
                r.item.set_frame(Some(frame));
                true
            });
            go(ui, state, None);
        }
    }
}

/// The faces of the font of a text part. A tap sets the face and goes back.
fn face_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
    part: TextPart,
) {
    let Some(style) = part.of(&rec.item).cloned() else {
        return;
    };
    let current = Face {
        weight: style.weight,
        italic: style.italic,
    };
    let font = style.font.as_deref();
    if let Some(f) = text_style::face_rows(ui, state, controls, font, current) {
        atlas::edit_text(state, rec.id, part, true, |s| {
            s.weight = f.weight;
            s.italic = f.italic;
        });
        go(ui, state, part.page());
    }
}

/// The cases of a text part. A tap sets the case and goes back.
fn case_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
    part: TextPart,
) {
    let Some(style) = part.of(&rec.item) else {
        return;
    };
    if let Some(case) = text_style::case_rows(ui, controls, style.case) {
        atlas::edit_text(state, rec.id, part, true, |s| s.case = case);
        go(ui, state, part.page());
    }
}

/// The Title page and the Entries page of a legend.
fn legend_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rec: &FurnitureRecord,
    part: TextPart,
) {
    let Furniture::Legend(item) = &rec.item else {
        return;
    };
    let id = rec.id;
    if part == TextPart::Title {
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
        ui.add_space(6.0);
        text_rows(ui, state, controls, id, part, &item.title_style);
        return;
    }
    entry_rows(ui, state, controls, rec);
    ui.add_space(6.0);
    let reset = widgets::button_row(ui, controls, RESET_ENTRIES, !item.entries.is_empty());
    if reset.clicked() {
        atlas::item_step(state, id, |r| match &mut r.item {
            Furniture::Legend(l) => {
                l.entries.clear();
                true
            }
            _ => false,
        });
    }
    heading(ui, TEXT);
    text_rows(ui, state, controls, id, part, &item.entry_style);
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
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
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
            go(ui, state, Some(ItemPage::Entry(key.0, key.1)));
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
}

/// A slider for a length in millimeters. Returns the new length, to a
/// tenth.
fn mm_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
) -> Option<f64> {
    let text = format!("{value:.1} mm");
    let range = (*range.start(), *range.end());
    let v = value_slider(ui, controls, (name, name), value, range, &text)?;
    Some((v * 10.0).round() / 10.0)
}

/// A slider for a whole number.
fn count_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    value: u32,
    range: std::ops::RangeInclusive<u32>,
) -> Option<u32> {
    let lo = f64::from(*range.start());
    let hi = f64::from(*range.end());
    let text = value.to_string();
    let v = value_slider(
        ui,
        controls,
        (name, name),
        f64::from(value),
        (lo, hi),
        &text,
    )?;
    let v = v.round() as u32;
    (v != value).then_some(v)
}

/// A number with no zeros at the end of its decimals, for example "2.5".
fn number_text(v: f64) -> String {
    let text = format!("{v:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_string()
}

/// A division value that the user typed: a number more than zero.
fn parse_value(text: &str) -> Option<f64> {
    let v: f64 = text.trim().replace(',', "").parse().ok()?;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// The name of a position of the marks or the numbers.
pub fn position_name(p: ScalePosition) -> &'static str {
    let i = ScalePosition::ALL.iter().position(|q| *q == p).unwrap_or(0);
    POSITIONS[i]
}

/// The position choice row of the marks or the numbers.
fn position_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    position: ScalePosition,
) -> Option<ScalePosition> {
    let active = ScalePosition::ALL.iter().position(|p| *p == position);
    let chosen = widgets::choice_row(ui, controls, POSITION, &POSITIONS, active)?;
    Some(ScalePosition::ALL[chosen])
}

/// A row that opens a page of choices of a scale bar, with the current
/// choice at the right.
fn pick_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    pick: BarPick,
    value: &str,
) {
    let value = fit(ui, pick.name(), value);
    if widgets::open_row(ui, controls, pick.name(), &value).clicked() {
        go(ui, state, Some(ItemPage::Pick(pick)));
    }
}

/// A page of the panel of a scale bar.
fn bar_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
    page: BarPage,
) {
    match page {
        BarPage::Style => style_gallery(ui, state, controls, id, bar),
        BarPage::Units => units_page(ui, state, controls, id, bar),
        BarPage::Divisions => divisions_page(ui, state, controls, id, bar),
        BarPage::Bar => bar_rows(ui, state, controls, id, bar),
        BarPage::Marks => {
            pick_row(
                ui,
                state,
                controls,
                BarPick::MarkFrequency,
                bar.mark_frequency.label(),
            );
            if let Some(p) = position_row(ui, controls, bar.mark_position) {
                atlas::edit_bar(state, id, true, |b| b.mark_position = p);
            }
            let range = ScaleBarItem::MARK_HEIGHTS;
            let h = bar.division_mark_height;
            if let Some(v) = mm_slider(ui, controls, DIVISION_HEIGHT, h, range.clone()) {
                atlas::edit_bar(state, id, false, |b| b.division_mark_height = v);
            }
            let h = bar.subdivision_mark_height;
            if let Some(v) = mm_slider(ui, controls, SUBDIVISION_HEIGHT, h, range) {
                atlas::edit_bar(state, id, false, |b| b.subdivision_mark_height = v);
            }
        }
        BarPage::Numbers => numbers_page(ui, state, controls, id, bar),
    }
}

/// The height of a row of the style gallery, and the width of its
/// thumbnail.
const GALLERY_ROW: f32 = 92.0;
const GALLERY_THUMB: f32 = 200.0;

/// The style gallery: one row for each kind of scale bar, with a thumbnail
/// of the bar in that kind. A tap sets the kind and goes back.
fn style_gallery(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
) {
    let ctx = ui.ctx().clone();
    for kind in ScaleBarKind::ALL {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), GALLERY_ROW), Sense::hover());
        let name = kind.label();
        let on = bar.kind == kind;
        let resp = widgets::tap_area(ui, controls, rect, name, true);
        if on || resp.is_pointer_button_down_on() {
            let back = rect.shrink2(vec2(0.0, HIGHLIGHT_INSET));
            ui.painter()
                .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
        }
        let left = rect.left() + 38.0;
        let title_y = rect.top() + 22.0;
        if on {
            let mark = Rect::from_center_size(pos2(rect.left() + 18.0, title_y), vec2(18.0, 18.0));
            icons::paint(ui.painter(), mark, Icon::Tick, 18.0, theme::ACCENT);
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        ui.painter().text(
            pos2(left, title_y),
            Align2::LEFT_CENTER,
            name,
            theme::body(),
            color,
        );
        let width = GALLERY_THUMB.min(rect.right() - 12.0 - left);
        let (w, h) = atlas::PREVIEW_MM;
        let height = width * (h / w) as f32;
        let paper = Rect::from_min_size(pos2(left, rect.top() + 40.0), vec2(width, height));
        ui.painter()
            .rect_filled(paper.expand(4.0), 6.0, egui::Color32::WHITE);
        if let Some(texture) = atlas::bar_preview(&ctx, state, bar, kind, width) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(texture, paper, uv, egui::Color32::WHITE);
        }
        if resp.clicked() {
            atlas::edit_bar(state, id, true, |b| b.kind = kind);
            go(ui, state, None);
        }
    }
}

fn units_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
) {
    pick_row(ui, state, controls, BarPick::Unit, bar.unit.label());
    let second = bar.second_unit.map_or(NONE, ScaleUnit::label);
    pick_row(ui, state, controls, BarPick::SecondUnit, second);
    pick_row(
        ui,
        state,
        controls,
        BarPick::UnitLabel,
        bar.unit_label_style.label(),
    );
    if bar.unit_label_style == UnitLabelStyle::Custom {
        let mut text = bar.unit_label.clone();
        let label = CUSTOM_LABEL;
        if widgets::text_field(ui, controls, label, label, None, &mut text, true).changed() {
            atlas::edit_bar(state, id, false, |b| b.unit_label = text);
        }
        if bar.second_unit.is_some() {
            let mut text = bar.second_unit_label.clone();
            let label = SECOND_LABEL;
            if widgets::text_field(ui, controls, label, label, None, &mut text, true).changed() {
                atlas::edit_bar(state, id, false, |b| b.second_unit_label = text);
            }
        }
    }
    pick_row(
        ui,
        state,
        controls,
        BarPick::UnitPosition,
        bar.unit_position.label(),
    );
    if let Some(v) = mm_slider(ui, controls, GAP, bar.unit_gap, ScaleBarItem::GAPS) {
        atlas::edit_bar(state, id, false, |b| b.unit_gap = v);
    }
    heading(ui, TEXT);
    text_rows(ui, state, controls, id, TextPart::Units, &bar.unit_style);
}

fn divisions_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
) {
    let n = count_slider(
        ui,
        controls,
        DIVISIONS,
        bar.divisions,
        ScaleBarItem::DIVISIONS,
    );
    if let Some(n) = n {
        atlas::edit_bar(state, id, false, |b| b.divisions = n);
    }
    let all = DivisionSpacing::ALL;
    let labels = all.map(DivisionSpacing::label);
    let active = all.iter().position(|s| *s == bar.spacing);
    if let Some(i) = widgets::choice_row(ui, controls, SPACING, &labels, active) {
        atlas::edit_bar(state, id, true, |b| b.spacing = all[i]);
    }
    let n = count_slider(
        ui,
        controls,
        SUBDIVISIONS,
        bar.subdivisions,
        ScaleBarItem::SUBDIVISIONS,
    );
    if let Some(n) = n {
        atlas::edit_bar(state, id, false, |b| b.subdivisions = n);
    }
    let mut before = bar.before_zero;
    if widgets::switch_row(ui, controls, BEFORE_ZERO, BEFORE_ZERO, &mut before) {
        atlas::edit_bar(state, id, true, |b| b.before_zero = before);
    }
    pick_row(ui, state, controls, BarPick::Fit, bar.fit.label());
    // The fit that adjusts the value always picks the value.
    if bar.fit == ScaleFit::AdjustDivisionValue {
        return;
    }
    let mut auto = bar.division_value.is_none();
    if widgets::switch_row(ui, controls, AUTO_VALUE, AUTO_VALUE, &mut auto) {
        // The values start at the values that the bar draws.
        let drawn = atlas::bar_rows(state, id);
        let value = |i: usize| drawn.get(i).map(|r| r.value);
        atlas::edit_bar(state, id, true, |b| {
            if auto {
                b.division_value = None;
                b.second_division_value = None;
            } else {
                b.division_value = Some(value(0).unwrap_or(1.0));
                b.second_division_value = b.second_unit.and(value(1));
            }
        });
    }
    if auto {
        return;
    }
    let drawn = atlas::bar_rows(state, id);
    let units = [
        (Some(bar.unit), bar.division_value, DIVISION_VALUE),
        (bar.second_unit, bar.second_division_value, SECOND_VALUE),
    ];
    for (i, (unit, value, name)) in units.into_iter().enumerate() {
        let Some(unit) = unit else {
            continue;
        };
        let shown = value.or_else(|| drawn.get(i).map(|r| r.value));
        let shown = shown.map_or_else(String::new, number_text);
        let typed = widgets::key_field(
            ui,
            controls,
            name,
            name,
            unit.abbreviation(),
            &shown,
            Keys::Number,
            |t| parse_value(t).is_some(),
        );
        if let Some(v) = typed.as_deref().and_then(parse_value) {
            atlas::edit_bar(state, id, true, |b| {
                if i == 0 {
                    b.division_value = Some(v);
                } else {
                    b.second_division_value = Some(v);
                }
            });
        }
        keypad_below(ui, controls, name);
    }
}

/// The Bar page: the height, the fills, and the outline. A row that the
/// kind does not draw does not show.
fn bar_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
) {
    let kind = bar.kind;
    if kind != ScaleBarKind::Line {
        let h = bar.bar_height;
        if let Some(v) = mm_slider(ui, controls, BAR_HEIGHT, h, ScaleBarItem::BAR_HEIGHTS) {
            atlas::edit_bar(state, id, false, |b| b.bar_height = v);
        }
    }
    for &(name, second) in fills(kind) {
        let fill = if second { &bar.fill2 } else { &bar.fill1 };
        let value = fit(ui, name, fill.kind.label());
        if widgets::open_row(ui, controls, name, &value).clicked() {
            go(ui, state, Some(ItemPage::Fill(second)));
        }
    }
    let line = matches!(kind, ScaleBarKind::SteppedLine | ScaleBarKind::Line);
    let name = if line { LINE_COLOR } else { OUTLINE };
    if let Some(c) = rgb_rows(ui, state, controls, name, ClassColor::Outline, bar.outline) {
        atlas::edit_bar(state, id, false, |b| b.outline = c);
    }
    let range = (
        *ScaleBarItem::LINE_WIDTHS.start(),
        *ScaleBarItem::LINE_WIDTHS.end(),
    );
    let text = super::labels::points(bar.line_width);
    let names = (LINE_WIDTH, LINE_WIDTH);
    if let Some(v) = value_slider(ui, controls, names, bar.line_width, range, &text) {
        let v = (v * 10.0).round() / 10.0;
        atlas::edit_bar(state, id, false, |b| b.line_width = v);
    }
}

/// The fills that a kind of scale bar draws: the name of the row, and
/// whether it is the second fill.
fn fills(kind: ScaleBarKind) -> &'static [(&'static str, bool)] {
    match kind {
        ScaleBarKind::Alternating | ScaleBarKind::DoubleAlternating => {
            &[(FILL_1, false), (FILL_2, true)]
        }
        ScaleBarKind::SingleDivision => &[(FILL, false)],
        ScaleBarKind::Hollow => &[(FILL, true)],
        ScaleBarKind::SteppedLine | ScaleBarKind::Line => &[],
    }
}

/// Sets the kind of a fill. A solid fill that becomes a hatch keeps its
/// color as the background, and its lines take the color of the outline.
pub fn set_kind(f: &mut FillPattern, kind: FillKind, outline: vale_store::Rgb) {
    if f.kind == FillKind::Solid && kind.hatched() {
        f.background = Some(f.color);
        f.color = outline;
    }
    f.kind = kind;
}

/// The page of a fill of a scale bar: the kind, the color, and for a hatch
/// the background, the angle, the spacing, and the line width.
fn fill_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
    second: bool,
) {
    let fill = if second { &bar.fill2 } else { &bar.fill1 };
    for kind in FillKind::ALL {
        let name = kind.label();
        if widgets::option_row(ui, controls, name, name, &[], fill.kind == kind).clicked() {
            let outline = bar.outline;
            atlas::edit_fill(state, id, second, true, |f| set_kind(f, kind, outline));
        }
    }
    if fill.kind == FillKind::None {
        return;
    }
    ui.add_space(6.0);
    let which = if second {
        ClassColor::Fill2
    } else {
        ClassColor::Fill1
    };
    let name = if fill.kind.hatched() {
        LINE_COLOR
    } else {
        COLOR
    };
    if let Some(c) = rgb_rows(ui, state, controls, name, which, fill.color) {
        atlas::edit_fill(state, id, second, false, |f| f.color = c);
    }
    if !fill.kind.hatched() {
        return;
    }
    let mut on = fill.background.is_some();
    if widgets::switch_row(ui, controls, BACKGROUND, BACKGROUND, &mut on) {
        let color = on.then_some([0xff, 0xff, 0xff]);
        atlas::edit_fill(state, id, second, true, |f| f.background = color);
    }
    if let Some(back) = fill.background {
        let which = ClassColor::Background(second);
        if let Some(c) = rgb_rows(ui, state, controls, BACKGROUND_COLOR, which, back) {
            atlas::edit_fill(state, id, second, false, |f| f.background = Some(c));
        }
    }
    let range = |r: std::ops::RangeInclusive<f64>| (*r.start(), *r.end());
    let text = format!("{:.0}°", fill.angle);
    let names = (ANGLE, ANGLE);
    let angles = range(FillPattern::ANGLES);
    if let Some(v) = value_slider(ui, controls, names, fill.angle, angles, &text) {
        let v = (v / 5.0).round() * 5.0;
        atlas::edit_fill(state, id, second, false, |f| f.angle = v);
    }
    if let Some(v) = mm_slider(ui, controls, SPACING, fill.spacing, FillPattern::SPACINGS) {
        let v = v.max(*FillPattern::SPACINGS.start());
        atlas::edit_fill(state, id, second, false, |f| f.spacing = v);
    }
    let text = super::labels::points(fill.line_width);
    let names = (LINE_WIDTH, LINE_WIDTH);
    let widths = range(FillPattern::LINE_WIDTHS);
    if let Some(v) = value_slider(ui, controls, names, fill.line_width, widths, &text) {
        let v = ((v * 20.0).round() / 20.0).max(widths.0);
        atlas::edit_fill(state, id, second, false, |f| f.line_width = v);
    }
}

fn numbers_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
) {
    pick_row(
        ui,
        state,
        controls,
        BarPick::NumberFrequency,
        bar.number_frequency.label(),
    );
    if let Some(p) = position_row(ui, controls, bar.number_position) {
        atlas::edit_bar(state, id, true, |b| b.number_position = p);
    }
    if let Some(v) = mm_slider(ui, controls, GAP, bar.number_gap, ScaleBarItem::GAPS) {
        atlas::edit_bar(state, id, false, |b| b.number_gap = v);
    }
    let decimals = bar
        .decimals
        .map_or(AUTO_DECIMALS.to_string(), |d| d.to_string());
    pick_row(ui, state, controls, BarPick::Decimals, &decimals);
    let mut on = bar.thousands;
    if widgets::switch_row(ui, controls, THOUSANDS, THOUSANDS, &mut on) {
        atlas::edit_bar(state, id, true, |b| b.thousands = on);
    }
    let mut on = bar.zero_label;
    if widgets::switch_row(ui, controls, SHOW_ZERO, SHOW_ZERO, &mut on) {
        atlas::edit_bar(state, id, true, |b| b.zero_label = on);
    }
    heading(ui, TEXT);
    text_rows(
        ui,
        state,
        controls,
        id,
        TextPart::Numbers,
        &bar.number_style,
    );
}

/// The names of the choices of a page of choices, and the one that is on.
fn choices(bar: &ScaleBarItem, pick: BarPick) -> (Vec<String>, Option<usize>) {
    fn of<T: PartialEq + Copy>(
        all: &[T],
        label: fn(T) -> &'static str,
        on: T,
    ) -> (Vec<String>, Option<usize>) {
        let names = all.iter().map(|c| label(*c).to_string()).collect();
        (names, all.iter().position(|c| *c == on))
    }
    match pick {
        BarPick::Unit => of(&ScaleUnit::ALL, ScaleUnit::label, bar.unit),
        BarPick::SecondUnit => {
            let mut names = vec![NONE.to_string()];
            let others = ScaleUnit::ALL.into_iter().filter(|u| *u != bar.unit);
            let others: Vec<ScaleUnit> = others.collect();
            names.extend(others.iter().map(|u| u.label().to_string()));
            let on = match bar.second_unit {
                None => Some(0),
                Some(u) => others.iter().position(|o| *o == u).map(|i| i + 1),
            };
            (names, on)
        }
        BarPick::UnitLabel => of(
            &UnitLabelStyle::ALL,
            UnitLabelStyle::label,
            bar.unit_label_style,
        ),
        BarPick::UnitPosition => of(&UnitPosition::ALL, UnitPosition::label, bar.unit_position),
        BarPick::Fit => of(&ScaleFit::ALL, ScaleFit::label, bar.fit),
        BarPick::MarkFrequency => of(
            &TickFrequency::ALL,
            TickFrequency::label,
            bar.mark_frequency,
        ),
        BarPick::NumberFrequency => of(
            &TickFrequency::ALL,
            TickFrequency::label,
            bar.number_frequency,
        ),
        BarPick::Decimals => {
            let mut names = vec![AUTO_DECIMALS.to_string()];
            names.extend(ScaleBarItem::DECIMALS.map(|d| d.to_string()));
            let on = bar.decimals.map_or(0, |d| d as usize + 1);
            (names, Some(on))
        }
    }
}

/// Sets choice `i` of a page of choices.
fn choose(b: &mut ScaleBarItem, pick: BarPick, i: usize) {
    match pick {
        BarPick::Unit => {
            b.unit = ScaleUnit::ALL[i];
            if b.second_unit == Some(b.unit) {
                b.second_unit = None;
            }
        }
        BarPick::SecondUnit => {
            let others: Vec<ScaleUnit> = ScaleUnit::ALL
                .into_iter()
                .filter(|u| *u != b.unit)
                .collect();
            b.second_unit = i.checked_sub(1).map(|i| others[i]);
            b.second_division_value = None;
        }
        BarPick::UnitLabel => b.unit_label_style = UnitLabelStyle::ALL[i],
        BarPick::UnitPosition => b.unit_position = UnitPosition::ALL[i],
        BarPick::Fit => b.fit = ScaleFit::ALL[i],
        BarPick::MarkFrequency => b.mark_frequency = TickFrequency::ALL[i],
        BarPick::NumberFrequency => b.number_frequency = TickFrequency::ALL[i],
        BarPick::Decimals => b.decimals = i.checked_sub(1).map(|d| d as u32),
    }
}

/// A page of choices of a scale bar. A tap sets the choice and goes back.
fn pick_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: FurnitureId,
    bar: &ScaleBarItem,
    pick: BarPick,
) {
    let (names, on) = choices(bar, pick);
    for (i, name) in names.iter().enumerate() {
        if widgets::option_row(ui, controls, name, name, &[], on == Some(i)).clicked() {
            atlas::edit_bar(state, id, true, |b| choose(b, pick, i));
            go(ui, state, Some(ItemPage::Bar(pick.parent())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_has_no_zeros_at_the_end() {
        assert_eq!(number_text(250.0), "250");
        assert_eq!(number_text(2.5), "2.5");
        assert_eq!(number_text(0.125), "0.125");
    }

    #[test]
    fn a_division_value_is_more_than_zero() {
        assert_eq!(parse_value("1,000"), Some(1000.0));
        assert_eq!(parse_value("0"), None);
        assert_eq!(parse_value("-2"), None);
        assert_eq!(parse_value("x"), None);
    }

    #[test]
    fn each_choice_of_each_page_sets_the_bar() {
        let picks = [
            BarPick::Unit,
            BarPick::SecondUnit,
            BarPick::UnitLabel,
            BarPick::UnitPosition,
            BarPick::Fit,
            BarPick::MarkFrequency,
            BarPick::NumberFrequency,
            BarPick::Decimals,
        ];
        for pick in picks {
            let (names, _) = choices(&ScaleBarItem::default(), pick);
            for i in 0..names.len() {
                let mut bar = ScaleBarItem::default();
                choose(&mut bar, pick, i);
                assert_eq!(choices(&bar, pick).1, Some(i), "{pick:?} {i}");
            }
        }
    }
}
