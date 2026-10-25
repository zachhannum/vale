//! The iPad layout of the Atlas workspace: the page on the canvas, the top
//! row with the page controls, the tool strip, and the panel of the
//! selection.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use vale_sphere::ProjectionKind;
use vale_store::{FrameShape, PageUnits};

use super::geometry::{self, GAP, Input, MARGIN, PAD, ROW, Rects, TOUCH, WidthClass};
use super::icons::Icon;
use super::widgets::{self, BODY_PAD, Controls, Keys, PANEL_ROW};
use super::{Card, CardTitle, GRATICULE, WORKSPACES, WORLDS, WORLDS_LEAD, WORLDS_ROW};
use super::{rule, save_mark, show_card, theme, toggle_worlds, worlds_menu};
use crate::atlas::{self as math, PAGE_PRESETS};
use crate::ui::atlas::{self, AtlasSelection, AtlasTool, FrameTab};
use crate::ui::{AppState, Workspace};

pub const SELECT: &str = "Select";
pub const MOVE: &str = "Move";
pub const PREVIOUS_PAGE: &str = "Previous page";
pub const NEXT_PAGE: &str = "Next page";
/// The button of the top row that opens the Add menu.
pub const ADD: &str = "Add";
/// The row of the Add menu that adds a map frame.
pub const MAP_FRAME: &str = "Map frame";
/// The rows of the Add menu that other tasks add.
const LATER_ITEMS: [&str; 4] = ["Text", "Legend", "Scale bar", "North arrow"];
/// The tabs of the panel of a frame. The last two come in other tasks.
pub const FRAME_TABS: [&str; 4] = ["Frame", "Projection", "Layers", "Labels"];
pub const SHAPE: &str = "Shape";
pub const RECTANGLE: &str = "Rectangle";
pub const POLYGON: &str = "Polygon";
pub const ADD_HOLE: &str = "Add a hole";
pub const DELETE_FRAME: &str = "Delete the frame";
pub const REMOVE_VERTEX: &str = "Remove vertex";
pub const SCALE: &str = "Scale";
pub const LONGITUDE: &str = "Longitude";
pub const LATITUDE: &str = "Latitude";
pub const ROTATION: &str = "Rotation";
pub const CENTER_ON_FRAME: &str = "Center on the frame";
pub const PAGE_NAME: &str = "Page name";
pub const SIZE: &str = "Size";
pub const ORIENTATIONS: [&str; 2] = ["Portrait", "Landscape"];
pub const WIDTH: &str = "Width";
pub const HEIGHT: &str = "Height";
pub const UNIT_NAMES: [&str; 2] = ["mm", "in"];
pub const ADD_PAGE: &str = "Add a page";
pub const DELETE_PAGE: &str = "Delete the page";

/// The width of the actions card: three buttons and a divider.
const ACTIONS_WIDTH: f32 = 3.0 * TOUCH + 3.0 * PAD + 9.0 + 2.0 * PAD;
const ADD_WIDTH: f32 = 220.0;
const TOOL_CELLS: usize = 2;
/// The height of the keypad of a field.
const KEYPAD: f32 = 4.0 * PANEL_ROW;
/// The smallest space at each side of the text of a tab.
pub const TAB_PAD: f32 = 12.0;

fn text_width(ctx: &egui::Context, text: &str) -> f32 {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), theme::body(), theme::TEXT));
    galley.size().x.ceil()
}

/// The width of the panel of the selection. The tabs of a frame fit in it.
fn panel_width(ctx: &egui::Context) -> f32 {
    let text: f32 = FRAME_TABS.iter().map(|t| text_width(ctx, t)).sum();
    let tabs = text + 2.0 * TAB_PAD * FRAME_TABS.len() as f32 + 2.0 * PAD;
    geometry::RIGHT_PANEL_WIDTH.max(tabs + 2.0 * BODY_PAD)
}

/// The keypad of the field `name`, below the field, if it is open. The
/// panel scrolls to show the keypad when it opens.
fn keypad_below(ui: &mut egui::Ui, controls: &mut Controls, name: &str) {
    let id = egui::Id::new(("pad-atlas-keypad", name));
    let open = widgets::keypad_field(ui.ctx()).as_deref() == Some(name);
    let shown = ui.data_mut(|d| std::mem::replace(d.get_temp_mut_or_default::<bool>(id), open));
    if !open {
        return;
    }
    let top = ui.cursor().top();
    widgets::keypad(ui, controls, KEYPAD);
    if !shown {
        let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top());
        ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
    }
}

/// The name of a page in the top row and in the title of its panel.
pub fn page_title(index: usize, name: &str) -> String {
    match name.trim() {
        "" => format!("Page {}", index + 1),
        name => format!("Page {} · {name}", index + 1),
    }
}

/// A length of a page in its units, for example "297" or "8.5".
pub fn length_text(mm: f64, units: PageUnits) -> String {
    let text = match units {
        PageUnits::Mm => format!("{mm:.1}"),
        PageUnits::In => format!("{:.2}", mm / 25.4),
    };
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub fn unit_name(units: PageUnits) -> &'static str {
    match units {
        PageUnits::Mm => UNIT_NAMES[0],
        PageUnits::In => UNIT_NAMES[1],
    }
}

/// The text of the readout: the size of the open page and its place in the
/// atlas, for example "297 × 210 mm · 1 / 2".
pub fn readout_text(state: &AppState) -> String {
    let page = atlas::open_page(state);
    let (index, count) = atlas::page_index(state);
    let units = page.units;
    format!(
        "{} × {} {} · {} / {count}",
        length_text(page.width_mm, units),
        length_text(page.height_mm, units),
        unit_name(units),
        index + 1
    )
}

fn page_button_text(state: &AppState) -> String {
    let (index, _) = atlas::page_index(state);
    page_title(index, &atlas::open_page(state).name)
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let screen = ctx.viewport_rect();
    atlas::sync(state);
    let class = WidthClass::of(screen.width());
    let workspace_width = match class {
        WidthClass::Wide => WORLDS_LEAD + widgets::segments_width(ui, &WORKSPACES),
        WidthClass::Medium => WORLDS_LEAD + widgets::menu_button_width(ui, WORKSPACES[1]),
        WidthClass::Compact => widgets::menu_button_width(ui, WORKSPACES[1]),
    };
    let page_width = 2.0 * TOUCH + page_text_width(ui, state) + 2.0 * PAD;
    let input = Input {
        screen,
        bands: geometry::Bands::new(screen, ctx.content_rect()),
        workspace_width,
        view_width: page_width,
        panel: state.atlas.selection != AtlasSelection::None,
        sheet_offset: state.pad.sheet_offset,
    };
    let mut rects = geometry::layout(&input);
    rects.actions.min.x = rects.actions.max.x - ACTIONS_WIDTH;
    rects.right_panel.x += geometry::RIGHT_PANEL_WIDTH - panel_width(&ctx);
    rects.tools.max.y = rects.tools.min.y + TOOL_CELLS as f32 * geometry::CELL.1 + 2.0 * PAD;
    rects.brush = None;
    state.pad.sheet_offset = input.sheet_offset.clamp(0.0, rects.sheet_travel);
    // The compact layout has no room in the top row, so the page card is at
    // the right of the tool strip.
    let page_card = rects.view.unwrap_or_else(|| {
        let min = pos2(rects.tools.right() + GAP, rects.tools.top());
        Rect::from_min_size(min, vec2(page_width, ROW))
    });

    let mut controls = std::mem::take(&mut state.pad.controls);
    controls.clear();
    state.pad.panels.clear();
    state.pad.menu_rect = None;
    state.atlas.panel_rect = None;
    let shown = match state.atlas.selection {
        AtlasSelection::None => None,
        selection => Some((selection, state.atlas.tab)),
    };
    if state.atlas.shown != shown {
        widgets::close_keypad(&ctx);
        state.atlas.shown = shown;
        state.pad.sheet_offset = 0.0;
    }

    top_row(&ctx, state, &mut controls, &rects, page_card);
    tool_strip(&ctx, state, &mut controls, &rects);
    if state.atlas.selection != AtlasSelection::None {
        selection_panel(&ctx, state, &mut controls, &rects);
    }
    readout(&ctx, ui, state, &rects);
    state.atlas.add_rect = None;
    if state.atlas.add {
        add_menu(&ctx, state, &mut controls, &rects);
    }
    if state.pad.menu && class != WidthClass::Wide {
        workspace_menu(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.menu = false;
    }
    state.pad.worlds_rect = None;
    if state.pad.worlds.is_some() {
        worlds_menu(&ctx, state, &mut controls, &rects);
    }

    // The page fits the space between the cards. In the wide layout, the
    // panel has its own place at the right.
    let left = rects.tools.right() + GAP;
    let top = match rects.view {
        Some(_) => rects.tools.top(),
        None => page_card.bottom() + GAP,
    };
    let right = match class {
        WidthClass::Wide => rects.right_panel.x - GAP,
        _ => screen.right() - rects.bands.right - MARGIN,
    };
    let free = Rect::from_min_max(pos2(left, top), pos2(right, rects.panel_bottom));
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        atlas::canvas(ui, state, screen, free, theme::CANVAS);
    });

    state.pad.controls = controls;
    state.pad.rects = Some(rects);
}

fn page_text_width(ui: &egui::Ui, state: &AppState) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(page_button_text(state), theme::body(), theme::TEXT);
    galley.size().x.ceil() + 2.0 * 14.0
}

fn divider(ui: &egui::Ui, x: f32, card: Rect) {
    let line = Rect::from_center_size(pos2(x, card.center().y), vec2(1.0, 24.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
}

fn top_row(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    page_card: Rect,
) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-workspace",
        rects.workspace,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let mut rest = card;
            if rects.class != WidthClass::Compact {
                let button = Rect::from_min_size(card.min + vec2(PAD, PAD), vec2(TOUCH, TOUCH));
                let open = state.pad.worlds.is_some();
                let worlds =
                    widgets::icon_button_at(ui, controls, button, WORLDS, Icon::World, open, true);
                if worlds.clicked() {
                    toggle_worlds(state);
                }
                divider(ui, button.right() + 4.5, card);
                rest.min.x += WORLDS_LEAD;
            }
            let ui = &ui.new_child(egui::UiBuilder::new().max_rect(rest));
            if rects.class == WidthClass::Wide {
                let items = [(WORKSPACES[0], true), (WORKSPACES[1], true)];
                if widgets::segments(ui, controls, &items, 1) == Some(0) {
                    state.workspace = Workspace::Globe;
                }
            } else {
                let open = state.pad.menu;
                if widgets::menu_button(ui, controls, "Workspace", WORKSPACES[1], open).clicked() {
                    state.pad.menu = !open;
                    state.pad.worlds = None;
                }
            }
        },
    );

    let text = page_button_text(state);
    let (index, count) = atlas::page_index(state);
    widgets::card(
        ctx,
        None,
        "pad-pages",
        page_card,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let at = |x: f32| Rect::from_min_size(pos2(x, card.top() + PAD), vec2(TOUCH, TOUCH));
            let previous = at(card.left() + PAD);
            let next = at(card.right() - PAD - TOUCH);
            let back = widgets::icon_button_at(
                ui,
                controls,
                previous,
                PREVIOUS_PAGE,
                Icon::Back,
                false,
                index > 0,
            );
            if back.clicked() {
                atlas::go_to_page(state, -1);
            }
            let on = index + 1 < count;
            if widgets::icon_button_at(ui, controls, next, NEXT_PAGE, Icon::Open, false, on)
                .clicked()
            {
                atlas::go_to_page(state, 1);
            }
            let middle = Rect::from_min_max(
                pos2(previous.right(), previous.top()),
                pos2(next.left(), next.bottom()),
            );
            ui.painter().text(
                middle.center(),
                Align2::CENTER_CENTER,
                &text,
                theme::body(),
                theme::TEXT,
            );
        },
    );

    widgets::card(
        ctx,
        None,
        "pad-actions",
        rects.actions,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let at = |x: f32| {
                let min = pos2(card.left() + PAD + x, card.top() + PAD);
                Rect::from_min_size(min, vec2(TOUCH, TOUCH))
            };
            let step = TOUCH + PAD;
            let can_undo = state.can_undo();
            if widgets::icon_button_at(ui, controls, at(0.0), "Undo", Icon::Undo, false, can_undo)
                .clicked()
            {
                state.undo();
            }
            let can_redo = state.can_redo();
            if widgets::icon_button_at(ui, controls, at(step), "Redo", Icon::Redo, false, can_redo)
                .clicked()
            {
                state.redo();
            }
            divider(ui, card.left() + PAD + 2.0 * step + 4.5, card);
            let open = state.atlas.add;
            let add = at(2.0 * step + 9.0 + PAD);
            if widgets::icon_button_at(ui, controls, add, ADD, Icon::Plus, open, true).clicked() {
                state.atlas.add = !open;
            }
        },
    );
}

fn tool_strip(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-tools",
        rects.tools,
        Order::Middle,
        radius,
        |ui| {
            let mut ui = super::padded(ui);
            for (name, icon, tool) in [
                (SELECT, Icon::Select, AtlasTool::Select),
                (MOVE, Icon::Move, AtlasTool::Move),
            ] {
                let active = state.atlas.tool == tool;
                if widgets::tool_cell(&mut ui, controls, name, name, icon, active, true).clicked() {
                    state.atlas.tool = tool;
                }
            }
        },
    );
}

fn readout(ctx: &egui::Context, ui: &egui::Ui, state: &AppState, rects: &Rects) {
    let Some(band) = rects.readout else {
        return;
    };
    let galley = ui
        .painter()
        .layout_no_wrap(readout_text(state), theme::mono(), theme::MUTE);
    let mark = state.save_mark(std::time::Instant::now());
    let lead = if mark.is_some() {
        super::SAVE_MARK + 8.0
    } else {
        0.0
    };
    let size = vec2(lead + galley.size().x.ceil() + 24.0, band.height());
    let rect = if rects.class == WidthClass::Compact {
        Rect::from_min_size(band.min, size)
    } else {
        Rect::from_center_size(band.center(), size)
    };
    let radius = theme::CONTROL_RADIUS.into();
    widgets::card(
        ctx,
        None,
        "pad-readout",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let left = rect.left() + 12.0;
            if let Some(mark) = mark {
                let center = pos2(left + super::SAVE_MARK / 2.0, rect.center().y);
                let size = vec2(super::SAVE_MARK, super::SAVE_MARK);
                save_mark(
                    ui,
                    Rect::from_center_size(center, size),
                    mark,
                    !state.headless,
                );
            }
            let at = pos2(left + lead, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(at, galley, theme::MUTE);
        },
    );
}

/// Closes a menu when a touch starts outside it and outside its button.
fn pressed_outside(ctx: &egui::Context, menu: Rect, button: Rect) -> bool {
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    pressed.is_some_and(|pos| !menu.contains(pos) && !button.contains(pos))
}

/// An opaque card for a menu, with rows from the top.
fn menu_card(ctx: &egui::Context, id: &str, rect: Rect, contents: impl FnOnce(&mut egui::Ui)) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, id, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        contents(&mut ui);
    });
}

fn add_menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let rows = 1 + LATER_ITEMS.len();
    let size = vec2(ADD_WIDTH, 10.0 + rows as f32 * PANEL_ROW + 10.0);
    let left = (rects.actions.right() - size.x).max(rects.screen.left() + geometry::PANEL_GAP);
    let rect = Rect::from_min_size(pos2(left, rects.actions.bottom() + PAD), size);
    state.atlas.add_rect = Some(rect);
    if pressed_outside(ctx, rect, rects.actions) {
        state.atlas.add = false;
    }
    menu_card(ctx, "pad-atlas-add", rect, |ui| {
        if widgets::text_row(ui, controls, MAP_FRAME, 0.0, false, true).clicked() {
            atlas::add_frame(state);
            state.atlas.add = false;
        }
        for name in LATER_ITEMS {
            widgets::text_row(ui, controls, name, 0.0, false, false);
        }
    });
}

fn workspace_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let compact = rects.class == WidthClass::Compact;
    let rows = if compact { 3.0 } else { 2.0 };
    let rules = if compact { 1.0 } else { 0.0 };
    let size = vec2(
        geometry::MENU_WIDTH,
        10.0 + rows * PANEL_ROW + rules * 13.0 + 10.0,
    );
    let rect = Rect::from_min_size(rects.menu, size);
    state.pad.menu_rect = Some(rect);
    if pressed_outside(ctx, rect, rects.workspace) {
        state.pad.menu = false;
    }
    menu_card(ctx, "pad-menu", rect, |ui| {
        if widgets::text_row(ui, controls, WORKSPACES[0], 0.0, false, true).clicked() {
            state.workspace = Workspace::Globe;
            state.pad.menu = false;
        }
        if widgets::text_row(ui, controls, WORKSPACES[1], 0.0, true, true).clicked() {
            state.pad.menu = false;
        }
        if compact {
            rule(ui);
            if widgets::text_row(ui, controls, WORLDS_ROW, 0.0, false, true).clicked() {
                state.pad.menu = false;
                toggle_worlds(state);
            }
        }
    });
}

/// The panel of the selected frame or of the open page.
fn selection_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let place = (rects.right_panel, panel_width(ctx));
    let scroll = egui::scroll_area::ScrollSource::default();
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let rect = match state.atlas.selection {
        AtlasSelection::None => return,
        AtlasSelection::Frame(id) => {
            let Some(frame) = state.doc.project.atlas.frame(id) else {
                return;
            };
            let map = state.doc.project.atlas.map(frame.map);
            let title = map.map_or_else(String::new, |m| m.name.clone());
            let head = CardTitle {
                title: &title,
                back: None,
                action: None,
            };
            let key = match state.atlas.tab {
                FrameTab::Frame => "pad-atlas-frame",
                FrameTab::Projection => "pad-atlas-projection",
            };
            let card = Card {
                key,
                place,
                head,
                scroll,
            };
            let body =
                |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| match state
                    .atlas
                    .tab
                {
                    FrameTab::Frame => frame_tab(ui, state, controls, id),
                    FrameTab::Projection => projection_tab(ui, state, controls, id),
                };
            show_card(
                ctx, state, controls, rects, card, on_title, frame_tabs, body,
            )
        }
        AtlasSelection::Page => {
            let (index, _) = atlas::page_index(state);
            let title = format!("Page {}", index + 1);
            let head = CardTitle {
                title: &title,
                back: None,
                action: None,
            };
            let card = Card {
                key: "pad-atlas-page",
                place,
                head,
                scroll,
            };
            let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
            show_card(ctx, state, controls, rects, card, on_title, top, page_body)
        }
    };
    state.atlas.panel_rect = Some(rect);
}

/// A row of tabs. Each tab is as wide as its text and a padding, and the
/// tabs share the rest of the row. A tab that is not `enabled` does nothing
/// yet. Returns the tab that the user chose.
fn tabs(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    items: &[(&str, bool)],
    active: usize,
) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), TOUCH + 2.0 * PAD),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(row, theme::CONTROL_RADIUS, theme::track());
    let ctx = ui.ctx().clone();
    let widths: Vec<f32> = items
        .iter()
        .map(|(label, _)| text_width(&ctx, label) + 2.0 * TAB_PAD)
        .collect();
    let room = row.width() - 2.0 * PAD;
    let rest = room - widths.iter().sum::<f32>();
    let n = items.len() as f32;
    // In a row that is too narrow, the tabs shrink in proportion.
    let shrink = (room / (room - rest)).min(1.0);
    let mut left = row.left() + PAD;
    let mut chosen = None;
    for (index, &(label, enabled)) in items.iter().enumerate() {
        let width = widths[index] * shrink + rest.max(0.0) / n;
        let rect = Rect::from_min_size(pos2(left, row.top() + PAD), vec2(width, TOUCH));
        left += width;
        let resp = widgets::tap_area(ui, controls, rect, label, enabled);
        let on = index == active;
        if on || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                rect.shrink(2.0),
                theme::CONTROL_RADIUS - 2.0,
                theme::raise(),
            );
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        let color = if enabled {
            color
        } else {
            color.gamma_multiply(theme::OFF)
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::body(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

/// The tabs of the panel of a frame.
fn frame_tabs(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let items = [
        (FRAME_TABS[0], true),
        (FRAME_TABS[1], true),
        (FRAME_TABS[2], false),
        (FRAME_TABS[3], false),
    ];
    let active = state.atlas.tab as usize;
    match tabs(ui, controls, &items, active) {
        Some(0) => state.atlas.tab = FrameTab::Frame,
        Some(1) => state.atlas.tab = FrameTab::Projection,
        _ => {}
    }
    ui.add_space(6.0);
}

fn frame_tab(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::FrameId,
) {
    let Some(frame) = state.doc.project.atlas.frame(id).cloned() else {
        return;
    };
    let map = state.doc.project.atlas.map(frame.map);
    let name = map.map_or_else(String::new, |m| m.name.clone());
    widgets::value_row(ui, "Map", &name);

    let polygon = matches!(frame.shape, FrameShape::Polygon(_));
    let shapes = [RECTANGLE, POLYGON];
    if let Some(chosen) =
        widgets::choice_row(ui, controls, SHAPE, &shapes, Some(usize::from(polygon)))
    {
        atlas::set_polygon(state, id, chosen == 1);
    }
    if polygon {
        if widgets::button_row(ui, controls, ADD_HOLE, true).clicked() {
            atlas::add_hole(state, id);
        }
        let vertex = state.atlas.vertex;
        if widgets::button_row(ui, controls, REMOVE_VERTEX, vertex.is_some()).clicked()
            && let Some(v) = vertex
        {
            atlas::remove_vertex(state, v);
        }
    }

    let keys = Keys::Number;
    let shown = math::scale_text(frame.scale);
    let valid = |text: &str| math::parse_scale(text).is_some();
    if let Some(scale) = widgets::key_field(ui, controls, SCALE, SCALE, "", &shown, keys, valid)
        .as_deref()
        .and_then(math::parse_scale)
    {
        atlas::set_scale(state, id, scale);
    }
    keypad_below(ui, controls, SCALE);
    let [lon, lat] = frame.center;
    let degrees = |text: &str| math::parse_degrees(text);
    let shown = math::lon_text(lon);
    let typed = widgets::key_field(ui, controls, LONGITUDE, LONGITUDE, "", &shown, keys, |t| {
        degrees(t).is_some()
    });
    if let Some(lon) = typed.as_deref().and_then(degrees) {
        atlas::set_longitude(state, id, lon);
    }
    keypad_below(ui, controls, LONGITUDE);
    let shown = math::lat_text(lat);
    let typed = widgets::key_field(ui, controls, LATITUDE, LATITUDE, "", &shown, keys, |t| {
        degrees(t).is_some_and(|v| v.abs() <= 90.0)
    });
    if let Some(lat) = typed.as_deref().and_then(degrees) {
        atlas::set_latitude(state, id, lat);
    }
    keypad_below(ui, controls, LATITUDE);

    let mut rotation = frame.rotation as f32;
    if widgets::compass_row(ui, controls, ROTATION, ROTATION, &mut rotation) {
        atlas::set_rotation(state, id, f64::from(rotation));
    }
    let mut graticule = frame.graticule;
    if widgets::switch_row(ui, controls, GRATICULE, GRATICULE, &mut graticule) {
        atlas::set_graticule(state, id, graticule);
    }

    ui.add_space(6.0);
    let delete = widgets::button_row_in(ui, controls, DELETE_FRAME, true, theme::WARN);
    if delete.clicked() {
        atlas::delete_frame(state);
    }
}

fn projection_tab(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::FrameId,
) {
    let spec = state.doc.projection();
    for kind in ProjectionKind::ALL {
        let row = widgets::option_row(
            ui,
            controls,
            kind.name(),
            kind.name(),
            &[],
            spec.kind == kind,
        );
        if row.clicked() {
            atlas::set_projection_kind(state, kind);
        }
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, CENTER_ON_FRAME, true).clicked() {
        atlas::center_projection(state, id);
    }
}

fn page_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let page = atlas::open_page(state).clone();

    let mut name = page.name.clone();
    if widgets::text_field(ui, controls, PAGE_NAME, "Name", None, &mut name, true).changed() {
        atlas::edit_page(state, |p| p.name = name);
    }

    let preset = math::preset_of(page.width_mm, page.height_mm);
    let landscape = page.width_mm > page.height_mm;
    if widgets::open_row(ui, controls, SIZE, preset).clicked() {
        state.atlas.sizes = !state.atlas.sizes;
    }
    if state.atlas.sizes {
        for p in PAGE_PRESETS {
            if widgets::option_row(ui, controls, p.name, p.name, &[], p.name == preset).clicked() {
                atlas::set_page_preset(state, &p);
                state.atlas.sizes = false;
            }
        }
    }
    let orientation = usize::from(landscape);
    if widgets::choice_row(
        ui,
        controls,
        "Orientation",
        &ORIENTATIONS,
        Some(orientation),
    )
    .is_some()
    {
        atlas::turn_page(state);
    }

    let units = page.units;
    let parse = move |text: &str| atlas::parse_length(text, units);
    let keys = Keys::Number;
    let suffix = unit_name(units);
    let shown = length_text(page.width_mm, units);
    let typed = widgets::key_field(ui, controls, WIDTH, WIDTH, suffix, &shown, keys, |t| {
        parse(t).is_some()
    });
    if let Some(mm) = typed.as_deref().and_then(parse) {
        atlas::edit_page(state, |p| p.width_mm = mm);
    }
    keypad_below(ui, controls, WIDTH);
    let shown = length_text(page.height_mm, units);
    let typed = widgets::key_field(ui, controls, HEIGHT, HEIGHT, suffix, &shown, keys, |t| {
        parse(t).is_some()
    });
    if let Some(mm) = typed.as_deref().and_then(parse) {
        atlas::edit_page(state, |p| p.height_mm = mm);
    }
    keypad_below(ui, controls, HEIGHT);
    let active = PageUnits::ALL.iter().position(|u| *u == units);
    if let Some(chosen) = widgets::choice_row(ui, controls, "Units", &UNIT_NAMES, active) {
        let units = PageUnits::ALL[chosen];
        atlas::edit_page(state, |p| p.units = units);
    }

    ui.add_space(6.0);
    if widgets::button_row(ui, controls, ADD_PAGE, true).clicked() {
        atlas::add_page(state);
    }
    let count = state.doc.project.atlas.pages.len();
    let delete = widgets::button_row_in(ui, controls, DELETE_PAGE, count > 1, theme::WARN);
    if delete.clicked() {
        atlas::delete_page(state);
    }
}
