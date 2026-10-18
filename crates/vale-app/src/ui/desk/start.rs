//! The start screen of the desktop layout: a title bar with the search and
//! the sort order, a sidebar with the places and the recent worlds, and a
//! grid of cards.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Rect, Sense, Stroke, UiBuilder, pos2, vec2,
};

use super::command::{self, Command, REVEAL};
use super::float::{self, Glass, Item};
use super::{menu, theme, title, widgets};
use crate::files::Availability;
use crate::projects::{self, Entry, Location};
use crate::ui::browser::{self, open_entry};
use crate::ui::pad::browser::{COPY, DELETE, RENAME, actions_name, open_name};
use crate::ui::pad::icons::{self, Icon};
use crate::ui::{Action, AppState, world_form};

/// The title of the screen, and the group of the places in the sidebar.
pub const TITLE: &str = "Worlds";
/// The row of the sidebar that shows the worlds of each place.
pub const ALL_WORLDS: &str = "All worlds";
/// The group of the recent worlds in the sidebar.
pub const RECENT: &str = "Recent";
/// The name of the search field.
pub const SEARCH: &str = "Search worlds";
/// The name of the button of the sort order.
pub const SORT: &str = "Sort";
/// The first row of the menu of a card.
pub const OPEN: &str = "Open";
pub const NO_WORLDS: &str = "No worlds yet. Make the first one with \"New world…\".";
pub const NO_MATCH: &str = "No worlds match.";

const SIDEBAR: f32 = 200.0;
/// The space between two cards.
const GAP: f32 = 22.0;
/// The smallest width of a card.
const CARD_MIN: f32 = 240.0;
/// The height of the text below the picture of a card.
const INFO: f32 = 70.0;
const PICTURE_RADIUS: u8 = 10;
const SEARCH_WIDTH: f32 = 200.0;
const CONTROL: f32 = 24.0;
const MENU_WIDTH: f32 = 200.0;

/// The order of the cards.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// The newest change first.
    #[default]
    LastChanged,
    Name,
}

impl Sort {
    pub const ALL: [Sort; 2] = [Sort::LastChanged, Sort::Name];

    pub fn label(self) -> &'static str {
        match self {
            Sort::LastChanged => "Last changed",
            Sort::Name => "Name",
        }
    }
}

/// The filter and the order of the cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartState {
    /// The place of the cards. `None`: each place.
    pub place: Option<Location>,
    pub search: String,
    pub sort: Sort,
}

/// The entries of one place, or of each place with `None`, whose names
/// contain the search text, in the sort order. The case of the letters does
/// not matter. An entry with no date is last in the order of the last
/// change.
pub fn visible<'a>(
    entries: &'a [Entry],
    place: Option<Location>,
    search: &str,
    sort: Sort,
) -> Vec<&'a Entry> {
    let search = search.trim().to_lowercase();
    let mut list: Vec<&Entry> = entries
        .iter()
        .filter(|e| place.is_none_or(|p| e.location == p))
        .filter(|e| e.name.to_lowercase().contains(&search))
        .collect();
    match sort {
        // `None` is less than each date.
        Sort::LastChanged => list.sort_by_key(|e| std::cmp::Reverse(e.modified)),
        Sort::Name => list.sort_by_cached_key(|e| e.name.to_lowercase()),
    }
    list
}

/// The line below the title.
pub fn count_text(count: usize) -> String {
    match count {
        1 => "1 world".to_owned(),
        n => format!("{n} worlds"),
    }
}

fn place_icon(location: Location) -> Icon {
    match location {
        Location::Local => Icon::Folder,
        Location::Cloud => Icon::Cloud,
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    menu::keys(&ctx, state);
    let available = state.projects.available();
    let start = &state.desk.start;
    let place = start.place.filter(|p| available.contains(p));
    let (search, sort) = (start.search.clone(), start.sort);
    // The cards borrow the entries while the screen changes the state.
    let entries = std::mem::take(&mut state.browser.entries);
    let shown = visible(&entries, place, &search, sort);
    let screen = ctx.content_rect();
    ui.scope_builder(UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        menu::row(ui, state);
        title_bar(ui, state, shown.len());
        sidebar(ui, state, &available, place);
        let frame = egui::Frame::NONE.fill(theme::canvas(state.translucent));
        egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| grid(ui, state, &entries, &shown));
    });
    drop(shown);
    state.browser.entries = entries;
    browser::dialogs(ui, state);
    world_form::desktop(&ctx, state);
}

fn title_bar(ui: &mut egui::Ui, state: &mut AppState, count: usize) {
    let frame = egui::Frame::NONE.fill(theme::glass(state.translucent));
    egui::Panel::top("desk-start-title")
        .exact_size(theme::BAR)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.max_rect();
            if state.desk.title_inset > 0.0 {
                title::drag(ui, rect);
            }
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, stroke);
            let left = rect.left() + 16.0 + state.desk.title_inset;
            let left = Rect::from_min_max(pos2(left, rect.top()), rect.max);
            title::place(ui, left, Layout::left_to_right(Align::Center), |ui| {
                heading(ui, count);
            });
            let right = Rect::from_min_max(rect.min, pos2(rect.right() - 10.0, rect.bottom()));
            title::place(ui, right, Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ctx = ui.ctx().clone();
                let new = Command::NewWorld;
                if widgets::primary_button(ui, new.label(), Some(&new.tip(&ctx)), true).clicked() {
                    command::run(new, &ctx, state);
                }
                sort_button(ui, state);
                search_field(ui, state);
            });
        });
}

/// The title, and below it the count of the cards.
fn heading(ui: &mut egui::Ui, count: usize) {
    let painter = ui.painter();
    let name = painter.layout_no_wrap(TITLE.to_owned(), theme::body(), theme::TEXT);
    let line = painter.layout_no_wrap(count_text(count), theme::small(), theme::MUTE);
    let width = name.size().x.max(line.size().x).ceil();
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::BAR), Sense::hover());
    let middle = rect.center().y;
    let name_rect = Rect::from_min_size(pos2(rect.left(), middle - name.size().y), name.size());
    let line_rect = Rect::from_min_size(pos2(rect.left(), middle + 1.0), line.size());
    title::label(ui, name_rect, "title", TITLE);
    title::label(ui, line_rect, "count", line.text());
    ui.painter().galley(name_rect.min, name, theme::TEXT);
    ui.painter().galley(line_rect.min, line, theme::MUTE);
}

/// The button that shows the sort order and opens the menu of the orders.
fn sort_button(ui: &mut egui::Ui, state: &mut AppState) {
    let sort = state.desk.start.sort;
    let resp = widgets::menu_button(ui, SORT, sort.label());
    let glass = Glass::of(state);
    float::menu(&resp, glass, |ui| {
        ui.set_min_width(140.0);
        for order in Sort::ALL {
            let item = Item::new(order.label()).checked(order == sort);
            if float::item(ui, item).clicked() {
                state.desk.start.sort = order;
            }
        }
    });
}

fn search_field(ui: &mut egui::Ui, state: &mut AppState) {
    let (rect, _) = ui.allocate_exact_size(vec2(SEARCH_WIDTH, CONTROL), Sense::hover());
    // The fill goes below the text of the field.
    let below = ui.painter().add(egui::Shape::Noop);
    let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(14.0, 14.0));
    let inner = Rect::from_min_max(pos2(icon.right() + 5.0, rect.top()), rect.max);
    let edit = egui::TextEdit::singleline(&mut state.desk.start.search)
        .id(egui::Id::new("desk-start-search"))
        .hint_text("Search")
        .frame(egui::Frame::NONE)
        .font(theme::label())
        .text_color(theme::TEXT)
        .vertical_align(Align::Center);
    let resp = ui.put(inner, edit);
    ui.ctx()
        .accesskit_node_builder(resp.id, |node| node.set_label(SEARCH));
    let color = if resp.has_focus() {
        theme::ACCENT
    } else {
        theme::line()
    };
    let back = egui::Shape::rect_filled(rect, theme::CONTROL_RADIUS, Color32::from_black_alpha(64));
    let edge = egui::Shape::rect_stroke(
        rect,
        theme::CONTROL_RADIUS,
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter();
    painter.set(below, egui::Shape::Vec(vec![back, edge]));
    icons::paint(painter, icon, Icon::Search, 14.0, theme::MUTE);
}

fn sidebar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    available: &[Location],
    place: Option<Location>,
) {
    let frame = egui::Frame::NONE
        .fill(theme::glass(state.translucent))
        .inner_margin(egui::Margin::symmetric(0, 6));
    egui::Panel::left("desk-start-side")
        .exact_size(SIDEBAR)
        .resizable(false)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.clip_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .vline(rect.right() - 0.5, rect.y_range(), stroke);
            ui.spacing_mut().item_spacing.y = 0.0;
            group(ui, TITLE);
            if side_row(ui, Icon::Grid, ALL_WORLDS, place.is_none()).clicked() {
                state.desk.start.place = None;
            }
            // The device comes first.
            for &location in available.iter().rev() {
                let label = location.label(false);
                let on = place == Some(location);
                if side_row(ui, place_icon(location), label, on).clicked() {
                    state.desk.start.place = Some(location);
                }
            }
            if state.browser.recent.is_empty() {
                return;
            }
            group(ui, RECENT);
            for (path, name) in state.browser.recent.clone() {
                if side_row(ui, Icon::Polygon, &name, false).clicked() {
                    state.actions.push(Action::OpenWorld(path));
                }
            }
        });
}

/// The name of a group of rows in the sidebar.
fn group(ui: &mut egui::Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let font = FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font, theme::MUTE);
    let at = pos2(rect.left() + 14.0, rect.bottom() - 2.0 - galley.size().y);
    let text = Rect::from_min_size(at, galley.size());
    title::label(ui, text, name, name);
    ui.painter().galley(at, galley, theme::MUTE);
}

/// A row of the sidebar: an icon in the accent color and a name.
fn side_row(ui: &mut egui::Ui, icon: Icon, name: &str, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(vec2(width, theme::ROW), Sense::hover());
    let rect = outer.shrink2(vec2(6.0, 0.0));
    let resp = ui.interact(rect, ui.id().with(("side", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::selected());
    } else if resp.hovered() {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::hover());
    }
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 15.5, rect.center().y), vec2(15.0, 15.0));
    icons::paint(painter, icon_rect, icon, 15.0, theme::ACCENT);
    let left = icon_rect.right() + 8.0;
    let galley = one_line(
        ui,
        name,
        theme::label(),
        theme::TEXT,
        rect.right() - 4.0 - left,
    );
    let at = pos2(left, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, theme::TEXT);
    resp
}

/// One line of text that ends with "…" if it is wider than `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

/// The status text and the cards. The grid scrolls.
fn grid(ui: &mut egui::Ui, state: &mut AppState, entries: &[Entry], shown: &[&Entry]) {
    egui::ScrollArea::vertical()
        .id_salt("desk-start-grid")
        .auto_shrink(false)
        .show(ui, |ui| {
            let margin = egui::Margin::symmetric(32, 28);
            egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let note = |ui: &mut egui::Ui, text: &str| {
                    let text = egui::RichText::new(text).font(theme::body());
                    ui.add(egui::Label::new(text.color(theme::MUTE)).wrap());
                    ui.add_space(16.0);
                };
                if !state.status.is_empty() {
                    note(ui, &state.status.clone());
                }
                if entries.is_empty() {
                    note(ui, NO_WORLDS);
                } else if shown.is_empty() {
                    note(ui, NO_MATCH);
                }
                let width = ui.available_width();
                let columns = ((width + GAP) / (CARD_MIN + GAP)).floor().max(1.0);
                let card_width = (width - (columns - 1.0) * GAP) / columns;
                let size = vec2(card_width, card_width * 0.75 + INFO);
                for row in shown.chunks(columns as usize) {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
                    for (column, entry) in row.iter().enumerate() {
                        let left = rect.left() + column as f32 * (card_width + GAP);
                        let rect = Rect::from_min_size(pos2(left, rect.top()), size);
                        ui.push_id(&entry.path, |ui| card(ui, state, rect, entry));
                    }
                    ui.add_space(GAP);
                }
            });
        });
}

/// One project: the picture, the name, the date, and the place. The actions
/// button shows while the pointer is on the card.
fn card(ui: &mut egui::Ui, state: &mut AppState, rect: Rect, entry: &Entry) {
    let resp = ui.interact(rect, ui.id().with("card"), Sense::click());
    let name = open_name(&entry.name);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let picture = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * 0.75));
    let painter = ui.painter();
    painter.rect(
        picture,
        PICTURE_RADIUS,
        Color32::from_rgb(27, 30, 35),
        Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    match entry.availability {
        Availability::Here => match state.browser.texture(ui.ctx(), entry) {
            Some(texture) => {
                let side = 0.7 * picture.width();
                let at = Rect::from_center_size(picture.center(), vec2(side, side));
                egui::Image::new(&texture).paint_at(ui, at);
            }
            // A file of an older version has no picture until it is open
            // one time.
            None => {
                let stroke = Stroke::new(1.0, theme::divider());
                let radius = 0.3 * picture.width();
                painter.circle_stroke(picture.center(), radius, stroke);
            }
        },
        Availability::InCloud | Availability::Downloading => {
            let text = match entry.availability {
                Availability::InCloud => "Not downloaded",
                _ => "Downloading…",
            };
            let center = picture.center();
            let icon = Rect::from_center_size(center - vec2(0.0, 12.0), vec2(32.0, 32.0));
            icons::paint(painter, icon, Icon::Cloud, 32.0, theme::MUTE);
            let galley = painter.layout_no_wrap(text.to_owned(), theme::label(), theme::MUTE);
            let at = pos2(center.x - galley.size().x / 2.0, icon.bottom() + 8.0);
            title::label(ui, Rect::from_min_size(at, galley.size()), "state", text);
            ui.painter().galley(at, galley, theme::MUTE);
        }
    }
    if entry.error.is_none() && !entry.conflicts.is_empty() {
        badge(ui, picture, entry.conflicts.len() + 1);
    }

    let more = Rect::from_min_size(
        pos2(picture.right() - 8.0 - theme::TOOL, picture.top() + 8.0),
        vec2(theme::TOOL, theme::TOOL),
    );
    let more = ui.interact(more, ui.id().with("more"), Sense::click());
    let actions = actions_name(&entry.name);
    more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &actions));
    let open = card_menu(&more, state, entry).is_some();
    let hovered = resp.contains_pointer() || open;
    if hovered {
        ui.painter().rect_stroke(
            picture,
            PICTURE_RADIUS,
            Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    if hovered || more.has_focus() {
        let alpha = if more.hovered() || open { 190 } else { 140 };
        let painter = ui.painter();
        painter.rect_filled(
            more.rect,
            theme::TOOL_RADIUS,
            Color32::from_black_alpha(alpha),
        );
        icons::paint(painter, more.rect, Icon::More, theme::ICON, theme::TEXT);
    }

    let left = rect.left() + 4.0;
    let width = rect.width() - 8.0;
    let top = picture.bottom() + 10.0;
    let at = pos2(left, top);
    text_line(
        ui,
        "name",
        &entry.name,
        theme::body(),
        theme::TEXT,
        at,
        width,
    );
    let at = pos2(left, top + 20.0);
    match &entry.error {
        Some(_) => {
            let text = "Cannot read the file";
            text_line(ui, "date", text, theme::label(), theme::WARN, at, width);
        }
        None => {
            if let Some(time) = entry.modified {
                let date = projects::date_text(time);
                text_line(ui, "date", &date, theme::label(), theme::MUTE, at, width);
            }
        }
    }
    let place = entry.location.label(false);
    let at = pos2(left + 18.0, top + 38.0);
    let text = text_line(
        ui,
        "place",
        place,
        theme::label(),
        theme::MUTE,
        at,
        width - 18.0,
    );
    let icon = Rect::from_center_size(pos2(left + 6.5, text.center().y), vec2(13.0, 13.0));
    icons::paint(
        ui.painter(),
        icon,
        place_icon(entry.location),
        13.0,
        theme::MUTE,
    );

    if resp.clicked() && !more.clicked() {
        open_card(state, entry);
    }
}

/// One line of text of a card, with a name for the screen reader.
fn text_line(
    ui: &egui::Ui,
    id: &str,
    text: &str,
    font: FontId,
    color: Color32,
    at: egui::Pos2,
    width: f32,
) -> Rect {
    let galley = one_line(ui, text, font, color, width);
    let rect = Rect::from_min_size(at, galley.size());
    title::label(ui, rect, id, text);
    ui.painter().galley(at, galley, color);
    rect
}

/// Opens a card. A file with an error shows the whole error.
fn open_card(state: &mut AppState, entry: &Entry) {
    match &entry.error {
        Some(error) => {
            let browser = &mut state.browser;
            browser.show_problem(entry.path.clone(), error.clone());
        }
        None => open_entry(state, entry),
    }
}

/// The count of the versions of a file that two devices changed.
fn badge(ui: &egui::Ui, picture: Rect, versions: usize) {
    let text = format!("{versions} versions");
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text.clone(), theme::small(), theme::WARN);
    let back = Rect::from_min_size(
        picture.min + vec2(10.0, 10.0),
        galley.size() + vec2(14.0, 4.0),
    );
    painter.rect_filled(back, theme::CONTROL_RADIUS, Color32::from_black_alpha(153));
    let at = back.min + vec2(7.0, 2.0);
    title::label(ui, Rect::from_min_size(at, galley.size()), "badge", &text);
    ui.painter().galley(at, galley, theme::WARN);
}

/// The menu of the actions of a card. Returns `Some` while it is open.
fn card_menu(button: &egui::Response, state: &mut AppState, entry: &Entry) -> Option<()> {
    let here = entry.availability == Availability::Here;
    let readable = entry.error.is_none() && here;
    let glass = Glass::of(state);
    let shown = float::menu(button, glass, |ui| {
        ui.set_min_width(MENU_WIDTH);
        let row = |ui: &mut egui::Ui, label: &str, enabled: bool| {
            float::item(ui, Item::new(label).enabled(enabled)).clicked()
        };
        let (path, name) = (entry.path.clone(), entry.name.clone());
        if row(ui, OPEN, true) {
            open_card(state, entry);
        }
        float::separator(ui);
        if row(ui, RENAME, readable) {
            state.browser.ask_rename(path.clone(), name.clone());
        }
        if row(ui, COPY, readable) {
            state.actions.push(Action::CopyProject(path.clone()));
        }
        if row(ui, REVEAL, here) {
            state.actions.push(Action::Reveal(path.clone()));
        }
        float::separator(ui);
        if row(ui, DELETE, true) {
            state.browser.ask_delete(path, name);
        }
    });
    shown.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn entry(name: &str, location: Location, days: Option<u64>) -> Entry {
        Entry {
            path: PathBuf::from(format!("{name}.gpkg")),
            name: name.to_owned(),
            thumbnail: None,
            modified: days.map(|d| SystemTime::UNIX_EPOCH + Duration::from_secs(d * 86_400)),
            error: None,
            location,
            availability: Availability::Here,
            conflicts: Vec::new(),
        }
    }

    fn names(list: Vec<&Entry>) -> Vec<&str> {
        list.into_iter().map(|e| e.name.as_str()).collect()
    }

    fn entries() -> Vec<Entry> {
        vec![
            entry("borea", Location::Local, Some(3)),
            entry("Aurelia", Location::Cloud, None),
            entry("Cassia", Location::Local, Some(9)),
            entry("Dune Sea", Location::Cloud, Some(5)),
        ]
    }

    #[test]
    fn the_last_change_comes_first_and_no_date_is_last() {
        let all = entries();
        let list = visible(&all, None, "", Sort::LastChanged);
        assert_eq!(names(list), ["Cassia", "Dune Sea", "borea", "Aurelia"]);
    }

    #[test]
    fn the_name_order_ignores_the_case() {
        let all = entries();
        let list = visible(&all, None, "", Sort::Name);
        assert_eq!(names(list), ["Aurelia", "borea", "Cassia", "Dune Sea"]);
    }

    #[test]
    fn a_place_shows_only_its_worlds() {
        let all = entries();
        let list = visible(&all, Some(Location::Cloud), "", Sort::Name);
        assert_eq!(names(list), ["Aurelia", "Dune Sea"]);
        let list = visible(&all, Some(Location::Local), "", Sort::Name);
        assert_eq!(names(list), ["borea", "Cassia"]);
    }

    #[test]
    fn the_search_finds_a_part_of_the_name_in_each_case() {
        let all = entries();
        assert_eq!(names(visible(&all, None, "SEA", Sort::Name)), ["Dune Sea"]);
        assert_eq!(names(visible(&all, None, " a ", Sort::Name)).len(), 4);
        assert_eq!(
            names(visible(&all, Some(Location::Local), "or", Sort::Name)),
            ["borea"]
        );
        assert!(visible(&all, None, "zz", Sort::Name).is_empty());
    }

    #[test]
    fn the_count_has_the_right_number() {
        assert_eq!(count_text(0), "0 worlds");
        assert_eq!(count_text(1), "1 world");
        assert_eq!(count_text(6), "6 worlds");
    }
}
