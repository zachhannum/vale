//! The iPad layout of the Atlas workspace: the page on the canvas, the top
//! row with the page controls, the tool strip, and the panel of the
//! selection.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use vale_sphere::{INTERVALS, ProjectionKind, interval_text};
use vale_store::{Border, FrameShape, Graticule, LayerId, MapId, PageUnits, RasterId};

use super::geometry::{self, GAP, Input, MARGIN, PAD, ROW, Rects, TOUCH, WidthClass};
use super::icons::Icon;
use super::widgets::{self, BODY_PAD, Controls, Keys, PANEL_ROW};
use super::{Card, CardTitle, GRATICULE, WORKSPACES, WORLDS, WORLDS_LEAD, WORLDS_ROW};
use super::{NO_LAYERS, layer_icon, rule, save_mark, show_card, theme, toggle_worlds, worlds_menu};
use crate::atlas::{self as math, PAGE_PRESETS};
use crate::ui::atlas::{self, AtlasSelection, AtlasTool, FrameTab};
use crate::ui::{Action, AppState, ExportFormat, Workspace};

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
/// The tabs of the panel of a frame. The last one comes in another task.
pub const FRAME_TABS: [&str; 4] = ["Frame", "Projection", "Layers", "Labels"];
pub const MAP_NAME: &str = "Map name";
pub const MAP_STYLE_HINT: &str = "Changes apply to this map only.";
pub const STYLE_FOR_THIS_MAP: &str = "Style for this map";
pub const REMOVE_OVERRIDES: &str = "Remove overrides";
pub const SHAPE: &str = "Shape";
pub const RECTANGLE: &str = "Rectangle";
pub const POLYGON: &str = "Polygon";
pub const ADD_HOLE: &str = "Add hole";
pub const DELETE_FRAME: &str = "Delete frame";
pub const REMOVE_VERTEX: &str = "Remove vertex";
pub const SCALE: &str = "Scale";
pub const LONGITUDE: &str = "Longitude";
pub const LATITUDE: &str = "Latitude";
pub const ROTATION: &str = "Rotation";
pub const AUTO_INTERVAL: &str = "Auto interval";
pub const INTERVAL: &str = "Interval";
pub const GRATICULE_COLOR: &str = "Graticule color";
pub const GRATICULE_WIDTH: &str = "Graticule width";
pub const BORDER: &str = "Border";
pub const BORDER_COLOR: &str = "Border color";
pub const BORDER_WIDTH: &str = "Border width";
pub const CENTER_ON_FRAME: &str = "Center on frame";
pub const PROJECTION: &str = "Map projection";
pub const CENTER_LONGITUDE: &str = "Center longitude";
pub const CENTER_LATITUDE: &str = "Center latitude";
pub const FIRST_PARALLEL: &str = "First parallel";
pub const SECOND_PARALLEL: &str = "Second parallel";
pub const OBLIQUE: &str = "Oblique";
pub const AZIMUTH: &str = "Azimuth";
pub const PROJECTION_NAME: &str = "Projection name";
pub const SAVE_PROJECTION: &str = "Save projection";
pub const DELETE_PROJECTION: &str = "Delete projection";
/// The name of the row of a custom projection.
pub fn custom_row(name: &str) -> String {
    format!("Custom projection {name}")
}
pub const PAGE_NAME: &str = "Page name";
pub const SIZE: &str = "Size";
pub const ORIENTATIONS: [&str; 2] = ["Portrait", "Landscape"];
pub const WIDTH: &str = "Width";
pub const HEIGHT: &str = "Height";
pub const UNIT_NAMES: [&str; 2] = ["mm", "in"];
pub const ADD_PAGE: &str = "Add page";
pub const DELETE_PAGE: &str = "Delete page";
pub const EXPORT_PAGE_PNG: &str = "Export page PNG…";
pub const EXPORT_PAGE_PDF: &str = "Export page PDF…";
pub const EXPORT_ATLAS_PDF: &str = "Export atlas PDF…";
pub const CANCEL_EXPORT: &str = "Cancel export";

/// The width of the actions card with no Measure button: three buttons and
/// a divider.
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
    // While the Measure tool is the tool, the Brush panel of its snapping
    // settings is the panel, and the panel of the selection does not show.
    let measuring = state.measuring();
    let input = Input {
        screen,
        bands: geometry::Bands::new(screen, ctx.content_rect()),
        workspace_width,
        view_width: page_width,
        panel: if measuring {
            state.pad.brush
        } else {
            state.atlas.selection != AtlasSelection::None
        },
        sheet_offset: state.pad.sheet_offset,
        measure: measuring,
        measure_button: true,
        actions_width: ACTIONS_WIDTH,
        tools: TOOL_CELLS,
    };
    let mut rects = geometry::layout(&input);
    rects.right_panel.x += geometry::RIGHT_PANEL_WIDTH - panel_width(&ctx);
    if !measuring {
        rects.brush = None;
    }
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
    if measuring {
        super::measure_card(&ctx, state, &mut controls, &rects);
        if state.pad.brush {
            super::show_panel(&ctx, state, &mut controls, &rects, super::Panel::Brush);
        }
    } else if state.atlas.selection != AtlasSelection::None {
        selection_panel(&ctx, state, &mut controls, &rects);
    }
    graticule_color_frame(&ctx, state);
    border_color_frame(&ctx, state);
    super::rules::picker_frame(&ctx, state);
    raster_cards(&ctx, state, &mut controls, &rects);
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
        state.globe.backdrop(),
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
        state.globe.backdrop(),
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
        state.globe.backdrop(),
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
            if rects.measure.is_none() {
                super::measure_button(ui, state, controls, at(3.0 * step + 9.0 + PAD));
            }
        },
    );
    super::measure_button_card(ctx, state, controls, rects);
}

fn tool_strip(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        state.globe.backdrop(),
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
    let text = if state.measuring() {
        state.measure_readout()
    } else {
        readout_text(state)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::mono(), theme::MUTE);
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
        state.globe.backdrop(),
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
        AtlasSelection::Frame(id) if open_layer(state).is_some() => {
            let Some(rect) = layer_panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
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
                FrameTab::Layers => "pad-atlas-layers",
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
                    FrameTab::Layers => layers_tab(ui, state, controls, id),
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
        (FRAME_TABS[2], true),
        (FRAME_TABS[3], false),
    ];
    let active = state.atlas.tab as usize;
    match tabs(ui, controls, &items, active) {
        Some(0) => state.atlas.tab = FrameTab::Frame,
        Some(1) => state.atlas.tab = FrameTab::Projection,
        Some(2) => state.atlas.tab = FrameTab::Layers,
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
    let mut name = map.map_or_else(String::new, |m| m.name.clone());
    if widgets::text_field(ui, controls, MAP_NAME, "Name", None, &mut name, true).changed() {
        atlas::rename_map(state, id, &name);
    }

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
    let mut graticule = frame.graticule.on;
    if widgets::switch_row(ui, controls, GRATICULE, GRATICULE, &mut graticule) {
        atlas::set_graticule(state, id, graticule);
    }
    if frame.graticule.on {
        graticule_rows(ui, state, controls, &frame);
    }
    let mut border = frame.border.on;
    if widgets::switch_row(ui, controls, BORDER, BORDER, &mut border) {
        atlas::set_border(state, id, border);
    }
    if frame.border.on {
        border_rows(ui, state, controls, &frame);
    }

    ui.add_space(6.0);
    let delete = widgets::button_row_in(ui, controls, DELETE_FRAME, true, theme::WARN);
    if delete.clicked() {
        atlas::delete_frame(state);
    }
}

/// The rows of the graticule of a frame: the interval, the color, and the
/// width. The interval slider shows the interval that the frame draws, and
/// a drag on it sets the interval by hand. A tap on the color opens a color
/// picker below it.
fn graticule_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    frame: &vale_store::FrameRecord,
) {
    let id = frame.id;
    let g = &frame.graticule;
    let Some((drawn, auto)) = atlas::graticule_intervals(state, id) else {
        return;
    };
    let mut automatic = g.interval.is_none();
    if widgets::switch_row(ui, controls, AUTO_INTERVAL, AUTO_INTERVAL, &mut automatic) {
        atlas::set_graticule_interval(state, id, (!automatic).then_some(auto));
    }
    let last = INTERVALS.len() - 1;
    let index = INTERVALS
        .iter()
        .position(|i| *i >= drawn * (1.0 - 1e-9))
        .unwrap_or(last);
    let mut fraction = index as f64 / last as f64;
    let text = interval_text(INTERVALS[index]);
    if widgets::row_slider(ui, controls, INTERVAL, INTERVAL, &mut fraction, &text) {
        let chosen = INTERVALS[(fraction * last as f64).round() as usize];
        if g.interval != Some(chosen) {
            atlas::set_graticule_interval(state, id, Some(chosen));
        }
    }

    let open = state.atlas.graticule_color;
    let row = widgets::color_row(ui, controls, GRATICULE_COLOR, g.color, open);
    if row.clicked() {
        state.atlas.graticule_color = !open;
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.graticule_color {
        ui.add_space(4.0);
        let mut color = g.color;
        if widgets::color_picker(ui, controls, GRATICULE_COLOR, &mut color) {
            atlas::set_graticule_color(state, id, color);
        }
        state.atlas.graticule_shown = true;
    }

    let (lo, hi) = (*Graticule::WIDTHS.start(), *Graticule::WIDTHS.end());
    let mut fraction = (g.width - lo) / (hi - lo);
    let text = format!("{} pt", (g.width * 10.0).round() / 10.0);
    let name = GRATICULE_WIDTH;
    if widgets::row_slider(ui, controls, name, name, &mut fraction, &text) {
        atlas::set_graticule_width(state, id, lo + fraction * (hi - lo));
    }
}

/// The rows of the border of a frame: the color and the width. A tap on the
/// color opens a color picker below it.
fn border_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    frame: &vale_store::FrameRecord,
) {
    let id = frame.id;
    let b = &frame.border;
    let open = state.atlas.border_color;
    let row = widgets::color_row(ui, controls, BORDER_COLOR, b.color, open);
    if row.clicked() {
        state.atlas.border_color = !open;
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.border_color {
        ui.add_space(4.0);
        let mut color = b.color;
        if widgets::color_picker(ui, controls, BORDER_COLOR, &mut color) {
            atlas::set_border_color(state, id, color);
        }
        state.atlas.border_shown = true;
    }

    let (lo, hi) = (*Border::WIDTHS.start(), *Border::WIDTHS.end());
    let mut fraction = (b.width - lo) / (hi - lo);
    let text = format!("{} pt", (b.width * 4.0).round() / 4.0);
    if widgets::row_slider(
        ui,
        controls,
        BORDER_WIDTH,
        BORDER_WIDTH,
        &mut fraction,
        &text,
    ) {
        atlas::set_border_width(state, id, lo + fraction * (hi - lo));
    }
}

/// Closes the color picker of the border when it did not show in this
/// frame, with the keypad of its field.
fn border_color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.border_shown);
    if !shown && std::mem::take(&mut state.atlas.border_color) {
        widgets::close_keypad(ctx);
    }
}

/// Closes the color picker of the graticule when it did not show in this
/// frame, with the keypad of its field.
fn graticule_color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.graticule_shown);
    if !shown && std::mem::take(&mut state.atlas.graticule_color) {
        widgets::close_keypad(ctx);
    }
}

/// The layers that the map of a frame can draw, in the order of the Layers
/// panel. The eye of a row turns the layer on or off in the map.
fn layers_tab(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::FrameId,
) {
    let Some(map) = state.doc.project.atlas.frame(id).map(|f| f.map) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    let rows: Vec<_> = (state.doc.project.layers().iter())
        .map(|l| (l.id, layer_icon(l), l.name.clone()))
        .collect();
    if rows.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_LAYERS).color(theme::MUTE));
        return;
    }
    let mut open = None;
    for &(layer, icon, ref name) in &rows {
        let shown = state.doc.shown(map, layer);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), egui::Sense::hover());
        let row = ui
            .push_id(layer.0, |ui| {
                widgets::layer_row(
                    ui,
                    controls,
                    rect,
                    icon,
                    name,
                    Some(shown),
                    false,
                    true,
                    false,
                    0.0,
                )
            })
            .inner;
        if row.eye {
            state.set_map_shown(map, layer, !shown);
        }
        if row.open || row.body.is_some_and(|b| b.clicked()) {
            open = Some(layer);
        }
    }
    // The layers that have style overrides in this map.
    let changed: Vec<_> = (rows.iter())
        .filter_map(|(layer, _, name)| {
            let n = state.doc.overrides(map, *layer).len();
            (n > 0).then(|| (*layer, name.clone(), n))
        })
        .collect();
    if !changed.is_empty() {
        ui.add_space(8.0);
        super::raster::heading(ui, STYLE_FOR_THIS_MAP);
        for (layer, name, n) in changed {
            let value = if n == 1 {
                "1 change".to_string()
            } else {
                format!("{n} changes")
            };
            let tapped = ui
                .push_id(("changed", layer.0), |ui| {
                    widgets::open_row(ui, controls, &name, &value).clicked()
                })
                .inner;
            if tapped {
                open = Some(layer);
            }
        }
    }
    if let Some(layer) = open {
        state.atlas.layer = Some((id, layer));
        state.atlas.layer_page = None;
        state.pad.rules = Default::default();
        state.pad.style = Default::default();
    }
}

/// The layer whose panel the Layers tab of the selected frame shows.
fn open_layer(state: &AppState) -> Option<vale_store::LayerId> {
    let layer = state.atlas.open_layer()?;
    state.doc.layer_style(layer)?;
    Some(layer)
}

/// The panel of a layer in the Layers tab of frame `id`: the switch between
/// the style of the layer and the style of the map of the frame, and the
/// style section with its pages.
fn layer_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: vale_store::FrameId,
) -> Option<Rect> {
    use super::Page;
    let layer = open_layer(state)?;
    if state.doc.entry(layer).is_none() {
        if let Some(raster) = state.doc.layer(layer).and_then(|l| l.raster()) {
            return raster_panel(ctx, state, controls, rects, id, layer, raster);
        }
        return look_panel(ctx, state, controls, rects, id, layer);
    }
    let map = state.doc.project.atlas.frame(id)?.map;
    let name = state.doc.layer(layer)?.name.clone();
    let rules = &state.doc.entry(layer)?.style.style.rules;
    let page = match state.atlas.layer_page {
        Some(Page::Rule(i) | Page::Condition(i) | Page::Symbols(i)) if i >= rules.len() => None,
        Some(Page::Rule(_) | Page::Condition(_) | Page::Symbols(_)) => state.atlas.layer_page,
        _ => None,
    };
    state.atlas.layer_page = page;
    let rule_name = match page {
        Some(Page::Rule(i) | Page::Condition(i) | Page::Symbols(i)) => rules[i].name.clone(),
        _ => String::new(),
    };
    let (key, title, back) = match page {
        Some(Page::Condition(_)) => (
            "pad-atlas-condition",
            super::filter::ADD_CONDITION,
            rule_name.as_str(),
        ),
        Some(Page::Symbols(_)) => ("pad-atlas-symbols", super::rules::SYMBOLS, &*rule_name),
        Some(_) => ("pad-atlas-rule", rule_name.as_str(), name.as_str()),
        None => ("pad-atlas-layer", name.as_str(), FRAME_TABS[2]),
    };
    let head = CardTitle {
        title,
        back: Some(back),
        action: None,
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head,
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.layer_page = match page {
                Some(Page::Condition(i) | Page::Symbols(i)) => Some(Page::Rule(i)),
                Some(_) => None,
                None => {
                    state.atlas.layer = None;
                    None
                }
            };
            state.pad.rules = Default::default();
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        state.edit_map_style(map, layer, |state| match page {
            Some(Page::Rule(i)) => super::rules::rule_body(ui, state, controls, layer, i),
            Some(Page::Condition(i)) => {
                super::filter::condition_body(ui, state, controls, layer, i);
            }
            Some(Page::Symbols(i)) => super::rules::symbols_body(ui, state, controls, layer, i),
            _ => super::rules::section(ui, state, controls, layer),
        });
        if page.is_none() {
            ui.add_space(8.0);
            let any = !state.doc.overrides(map, layer).is_empty();
            if widgets::button_row(ui, controls, REMOVE_OVERRIDES, any).clicked() {
                state.clear_overrides(map, layer);
            }
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, map_hint, body,
    ))
}

/// The hint at the top of the panel of a layer.
fn map_hint(ui: &mut egui::Ui, _: &mut AppState, _: &mut Controls) {
    ui.label(egui::RichText::new(MAP_STYLE_HINT).color(theme::MUTE));
    ui.add_space(4.0);
}

/// The open raster layer of the Layers tab, with the map of its frame.
fn open_raster(state: &AppState) -> Option<(MapId, LayerId, RasterId)> {
    let layer = open_layer(state)?;
    let raster = state.doc.layer(layer)?.raster()?;
    let frame = state.atlas.frame()?;
    let map = state.doc.project.atlas.frame(frame)?.map;
    Some((map, layer, raster))
}

/// The cards of the raster editors of the open raster layer: the band, the
/// reset, the ramp, and the shade color.
fn raster_cards(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some((map, layer, raster)) = open_raster(state) else {
        return;
    };
    state.edit_map_raster(map, layer, raster, |state| {
        super::raster::band_card(ctx, state, controls, rects);
        super::raster::shade_color_frame(ctx, state);
        super::raster::reset_card(ctx, state, controls, rects);
        super::raster::ramp_card(ctx, state, controls, rects);
    });
}

/// The panel of a raster layer in the Layers tab of frame `id`: the switch,
/// the opacity, and the style of the layer, with the pages of the color
/// stops, the color ramps, the hillshade, and the blend mode.
fn raster_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: vale_store::FrameId,
    layer: LayerId,
    raster: RasterId,
) -> Option<Rect> {
    use super::Page;
    use super::raster::{BLEND, COLOR_RAMPS, COLOR_STOPS, HILLSHADE};
    let map = state.doc.project.atlas.frame(id)?.map;
    let name = state.doc.layer(layer)?.name.clone();
    let page = state
        .atlas
        .layer_page
        .filter(|p| matches!(p, Page::Stops | Page::Ramps | Page::Hillshade | Page::Blend));
    state.atlas.layer_page = page;
    let (key, title, back) = match page {
        Some(Page::Ramps) => ("pad-atlas-ramps", COLOR_RAMPS, COLOR_STOPS),
        Some(Page::Stops) => ("pad-atlas-stops", COLOR_STOPS, name.as_str()),
        Some(Page::Hillshade) => ("pad-atlas-hillshade", HILLSHADE, name.as_str()),
        Some(_) => ("pad-atlas-blend", BLEND, name.as_str()),
        None => ("pad-atlas-raster", name.as_str(), FRAME_TABS[2]),
    };
    let head = CardTitle {
        title,
        back: Some(back),
        action: None,
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head,
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.layer_page = match page {
                Some(Page::Ramps) => Some(Page::Stops),
                Some(_) => None,
                None => {
                    state.atlas.layer = None;
                    None
                }
            };
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        state.edit_map_raster(map, layer, raster, |state| match state.pad.page {
            Some(Page::Stops) => super::raster::stops_body(ui, state, controls, raster),
            Some(Page::Ramps) => super::raster::ramps_body(ui, state, controls, raster),
            Some(Page::Hillshade) => {
                super::raster::hillshade_body(ui, state, controls, raster);
            }
            Some(Page::Blend) => super::raster::blend_body(ui, state, controls, raster),
            _ => {
                let Some(opacity) = state.doc.project.raster(raster).map(|r| r.opacity) else {
                    return;
                };
                let mut fraction = f64::from(opacity);
                let text = format!("{:.0}%", fraction * 100.0);
                let name = super::OPACITY;
                if widgets::row_slider(ui, controls, name, "Opacity", &mut fraction, &text) {
                    // The steps are whole percents.
                    let opacity = ((fraction * 100.0).round() / 100.0) as f32;
                    state.set_raster_opacity(raster, opacity);
                }
                super::raster::style(ui, state, controls, raster);
            }
        });
        if page.is_none() {
            ui.add_space(8.0);
            let any = !state.doc.overrides(map, layer).is_empty();
            if widgets::button_row(ui, controls, REMOVE_OVERRIDES, any).clicked() {
                state.clear_overrides(map, layer);
            }
        }
    };
    let rect = show_card(ctx, state, controls, rects, card, on_title, map_hint, body);
    // The cards of the editors take their place from the panel.
    state.pad.panels.push((super::Panel::Layers, rect));
    Some(rect)
}

/// The panel of an image layer in the Layers tab of frame `id`: the
/// switch, the opacity, and the blend mode, with its page.
fn look_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: vale_store::FrameId,
    layer: vale_store::LayerId,
) -> Option<Rect> {
    use super::Page;
    use super::raster::{BLEND, SHOW_AS, blend_rows};
    let map = state.doc.project.atlas.frame(id)?.map;
    let l = state.doc.layer(layer)?;
    let (name, raster) = (l.name.clone(), l.raster().is_some());
    let page = state.atlas.layer_page.filter(|p| *p == Page::Blend);
    state.atlas.layer_page = page;
    let (key, title, back) = match page {
        Some(_) => ("pad-atlas-blend", BLEND, name.as_str()),
        None => ("pad-atlas-look", name.as_str(), FRAME_TABS[2]),
    };
    let head = CardTitle {
        title,
        back: Some(back),
        action: None,
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head,
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            if page.is_none() {
                state.atlas.layer = None;
            }
            state.atlas.layer_page = None;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let scope = Some(map);
        let Some(mut look) = state.look(scope, layer) else {
            return;
        };
        let old = look;
        if page.is_some() {
            if let Some(mode) = blend_rows(ui, controls, look.blend) {
                look.blend = mode;
            }
        } else {
            let mut fraction = f64::from(look.opacity);
            let text = format!("{:.0}%", fraction * 100.0);
            if widgets::row_slider(
                ui,
                controls,
                super::OPACITY,
                "Opacity",
                &mut fraction,
                &text,
            ) {
                // The steps are whole percents.
                look.opacity = ((fraction * 100.0).round() / 100.0) as f32;
            }
            if raster {
                let all = vale_store::RasterDisplay::ALL;
                let active = all.iter().position(|d| *d == look.display);
                let displays = &crate::ui::elevation::DISPLAYS;
                if let Some(i) = widgets::choice_row(ui, controls, SHOW_AS, displays, active) {
                    look.display = all[i];
                }
            }
            let label = crate::ui::elevation::blend_label(look.blend);
            if widgets::open_row(ui, controls, BLEND, label).clicked() {
                state.atlas.layer_page = Some(Page::Blend);
            }
        }
        if look != old {
            state.set_look(scope, layer, look);
        }
        if page.is_none() {
            ui.add_space(8.0);
            let any = !state.doc.overrides(map, layer).is_empty();
            if widgets::button_row(ui, controls, REMOVE_OVERRIDES, any).clicked() {
                state.clear_overrides(map, layer);
            }
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, map_hint, body,
    ))
}

fn projection_tab(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::FrameId,
) {
    let Some(map) = state.doc.project.atlas.frame(id).map(|f| f.map) else {
        return;
    };
    let spec = state.doc.projection(map);
    let current = atlas::current_preset(state, id);
    let shown = current
        .clone()
        .unwrap_or_else(|| spec.kind.name().to_string());
    if widgets::open_row(ui, controls, PROJECTION, &shown).clicked() {
        state.atlas.kinds = !state.atlas.kinds;
    }
    if state.atlas.kinds {
        for kind in ProjectionKind::ALL {
            let on = spec.kind == kind && current.is_none();
            let row = widgets::option_row(ui, controls, kind.name(), kind.name(), &[], on);
            if row.clicked() {
                atlas::set_projection_kind(state, id, kind);
                state.atlas.kinds = false;
            }
        }
        let names: Vec<String> = state.doc.presets().iter().map(|p| p.name.clone()).collect();
        for name in names {
            let on = current.as_deref() == Some(name.as_str());
            let row = widgets::option_row(ui, controls, &custom_row(&name), &name, &[], on);
            if row.clicked() {
                atlas::apply_preset(state, id, &name);
                state.atlas.kinds = false;
            }
        }
    }
    ui.add_space(6.0);

    let keys = Keys::Number;
    let degrees = |text: &str| math::parse_degrees(text);
    let spec = state.doc.projection(map);
    let shown = math::lon_text(spec.lon0);
    let typed = widgets::key_field(
        ui,
        controls,
        CENTER_LONGITUDE,
        CENTER_LONGITUDE,
        "",
        &shown,
        keys,
        |t| degrees(t).is_some(),
    );
    if let Some(lon) = typed.as_deref().and_then(degrees) {
        atlas::set_projection_longitude(state, id, lon);
    }
    keypad_below(ui, controls, CENTER_LONGITUDE);
    let has_latitude = spec.kind.has_origin_latitude() || spec.azimuth.is_some();
    if has_latitude && spec.kind != ProjectionKind::PolarStereographic {
        let shown = math::lat_text(spec.lat0);
        let valid = |t: &str| degrees(t).is_some_and(|v| v.abs() <= 90.0);
        let typed = widgets::key_field(
            ui,
            controls,
            CENTER_LATITUDE,
            CENTER_LATITUDE,
            "",
            &shown,
            keys,
            valid,
        );
        if let Some(lat) = typed.as_deref().and_then(degrees) {
            atlas::set_projection_latitude(state, id, lat);
        }
        keypad_below(ui, controls, CENTER_LATITUDE);
    }
    if spec.kind == ProjectionKind::PolarStereographic {
        let pole = ["South pole", "North pole"];
        let north = usize::from(spec.lat0 >= 0.0);
        if let Some(chosen) = widgets::choice_row(ui, controls, "Pole", &pole, Some(north)) {
            atlas::set_projection_latitude(state, id, if chosen == 1 { 90.0 } else { -90.0 });
        }
    }
    if spec.kind.is_conic() {
        let valid = |t: &str| degrees(t).is_some_and(|v| v.abs() <= 89.0);
        for (name, value, first) in [
            (FIRST_PARALLEL, spec.lat1, true),
            (SECOND_PARALLEL, spec.lat2, false),
        ] {
            let shown = math::lat_text(value);
            let typed = widgets::key_field(ui, controls, name, name, "", &shown, keys, valid);
            if let Some(v) = typed.as_deref().and_then(degrees) {
                let (a, b) = if first {
                    (v, spec.lat2)
                } else {
                    (spec.lat1, v)
                };
                atlas::set_parallels(state, id, a, b);
            }
            keypad_below(ui, controls, name);
        }
    }
    if spec.kind.can_tilt() {
        let mut oblique = spec.azimuth.is_some();
        if widgets::switch_row(ui, controls, OBLIQUE, OBLIQUE, &mut oblique) {
            atlas::set_oblique(state, id, oblique);
        }
        if let Some(azimuth) = spec.azimuth {
            let mut angle = azimuth as f32;
            if widgets::compass_row(ui, controls, AZIMUTH, AZIMUTH, &mut angle) {
                atlas::set_azimuth(state, id, f64::from(angle));
            }
        }
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, CENTER_ON_FRAME, true).clicked() {
        atlas::center_projection(state, id);
    }

    ui.add_space(6.0);
    let mut name = state.atlas.preset_name.clone();
    let hint = Some("Name");
    if widgets::text_field(ui, controls, PROJECTION_NAME, "Name", hint, &mut name, true).changed() {
        state.atlas.preset_name = name;
    }
    let named = !state.atlas.preset_name.trim().is_empty();
    if widgets::button_row(ui, controls, SAVE_PROJECTION, named).clicked() {
        atlas::save_preset(state, id);
    }
    let saved = state
        .doc
        .presets()
        .iter()
        .any(|p| p.name == state.atlas.preset_name);
    if widgets::button_row(ui, controls, DELETE_PROJECTION, saved).clicked() {
        atlas::delete_preset(state);
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
    let ready = state.atlas_export.is_none();
    if widgets::button_row(ui, controls, EXPORT_PAGE_PNG, ready).clicked() {
        state.actions.push(Action::ExportDialog(ExportFormat::Png));
    }
    if widgets::button_row(ui, controls, EXPORT_PAGE_PDF, ready).clicked() {
        state.actions.push(Action::ExportDialog(ExportFormat::Pdf));
    }
    if widgets::button_row(ui, controls, EXPORT_ATLAS_PDF, ready).clicked() {
        state
            .actions
            .push(Action::ExportDialog(ExportFormat::AtlasPdf));
    }
    if let Some(progress) = state.atlas_export_progress() {
        ui.label(progress);
        if widgets::button_row_in(ui, controls, CANCEL_EXPORT, true, theme::WARN).clicked() {
            state.actions.push(Action::CancelExport);
        }
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
