//! The iPad layout of the Atlas workspace: the page on the canvas, the top
//! row with the page controls, the tool strip, and the panel of the
//! selection.

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use vale_sphere::{INTERVALS, ProjectionKind, interval_text};
use vale_store::{
    Border, BorderStyle, FrameShape, Graticule, GridColumns, LayerId, LayoutGrid, MapId, Neatline,
    PageUnits, RasterId,
};

use super::geometry::{self, GAP, Input, MARGIN, PAD, ROW, Rects, TOUCH, WidthClass};
use super::icons::Icon;
use super::number::Number;
use super::text_style;
use super::widgets::{self, BODY_PAD, Controls, Keys, PANEL_ROW, keypad_below};
use super::{Card, CardTitle, GRATICULE, WORKSPACES, WORLDS, WORLDS_LEAD, WORLDS_ROW};
use super::{NO_LAYERS, layer_icon, rule, save_mark, show_card, theme, toggle_worlds, worlds_menu};
use crate::atlas::{self as math, PAGE_PRESETS};
use crate::ui::atlas::{self, AtlasSelection, AtlasTool, ClassColor, FrameTab};
use crate::ui::{Action, AppState, ExportFormat, Workspace};
use crate::ui::{region, select};

/// The name of the cell of the Frame tool. Its label is "Frame", as the
/// label of the Frame tab.
pub const FRAME_TOOL: &str = "Frame tool";
pub const VERTEX: &str = "Vertex";
pub const MOVE: &str = "Move";
pub const PREVIOUS_PAGE: &str = "Previous page";
pub const NEXT_PAGE: &str = "Next page";
/// The button of the top row that opens the Add menu.
pub const ADD: &str = "Add";
/// The row of the Add menu that adds a map frame.
pub const MAP_FRAME: &str = "Map frame";
/// The rows of the Add menu that other tasks add.
/// The tabs of the panel of a frame.
pub const FRAME_TABS: [&str; 4] = ["Frame", "Projection", "Layers", "Labels"];
pub const MAP_NAME: &str = "Map name";
pub const MAP_STYLE: &str = "Map style";
/// The value of the Map style row when the map has no map style.
pub const NO_MAP_STYLE: &str = "None";
/// The name of the row of the Map style page that unlinks the map style.
pub const NO_MAP_STYLE_ROW: &str = "No map style";
pub const MAP_STYLE_NAME: &str = "Map style name";
pub const SAVE_AS_MAP_STYLE: &str = "Save as map style…";
pub const DELETE_MAP_STYLE: &str = "Delete map style";
pub const UPDATE_MAP_STYLE: &str = "Update map style";
pub const RESET_TO_MAP_STYLE: &str = "Reset to map style";
/// The mark of a map, and of a layer in a map, with local changes.
pub const EDITED: &str = "Edited";
/// The value of the Map style row of a map with local changes.
pub fn edited_value(name: &str) -> String {
    format!("{name} ({})", EDITED.to_lowercase())
}
/// The name of the row of a map style in the Map style page.
pub fn map_style_row(name: &str) -> String {
    format!("Map style {name}")
}
pub const COPY_GLOBE_STYLE: &str = "Copy globe style";
pub const SHAPE: &str = "Shape";
pub const RECTANGLE: &str = "Rectangle";
pub const POLYGON: &str = "Polygon";
pub const ADD_HOLE: &str = "Add hole";
pub const SCALE: &str = "Scale";
pub const LONGITUDE: &str = "Longitude";
pub const LATITUDE: &str = "Latitude";
pub const ROTATION: &str = "Rotation";
pub const AUTO_INTERVAL: &str = "Auto interval";
pub const INTERVAL: &str = "Interval";
pub const GRATICULE_COLOR: &str = "Graticule color";
pub const GRATICULE_WIDTH: &str = "Graticule width";
pub const LABEL_FORMAT: &str = "Label format";
/// The choices of the label format, in the order of `CoordFormat::ALL`.
pub const LABEL_FORMATS: [&str; 2] = ["DMS", "Decimal"];
pub const NEATLINE: &str = "Neatline";
pub const NEATLINE_COLOR: &str = "Neatline color";
pub const NEATLINE_WIDTH: &str = "Neatline width";
pub const BORDER: &str = "Border";
pub const BORDER_STYLE: &str = "Border style";
/// The choices of the border style, in the order of `BorderStyle::ALL`.
pub const BORDER_STYLES: [&str; 2] = ["Solid", "Graduated"];
pub const BORDER_COLOR: &str = "Border color";
pub const SECOND_COLOR: &str = "Second color";
pub const BORDER_WIDTH: &str = "Border width";
pub const BORDER_DISTANCE: &str = "Border distance";
pub const DISTANCE_BY_SIDE: &str = "Distance by side";
pub const BORDER_GAP: &str = "Border gap";
pub const GRID_COLUMNS: &str = "Columns";
/// The choices of the labels of the columns, in the order of
/// `GridColumns::ALL`.
pub const GRID_COLUMN_NAMES: [&str; 2] = ["Letters", "Numbers"];
/// The distance of each side of the border: top, right, bottom, left.
pub const SIDE_DISTANCES: [&str; 4] = [
    "Top distance",
    "Right distance",
    "Bottom distance",
    "Left distance",
];
pub const BORDER_STROKE: &str = "Border stroke";
pub const STROKE_COLOR: &str = "Stroke color";
pub const STROKE_WIDTH: &str = "Stroke width";
pub const REFERENCE_GRID: &str = "Reference grid";
pub const PAPER_COLOR: &str = "Paper color";
pub const SNAP_TO_LAYOUT: &str = "Snap to layout";
pub const LAYOUT_GRID: &str = "Layout grid";
pub const SHOW_GRID: &str = "Show layout grid";
/// The Layers button of the top row. It opens the Layers tab of the
/// selected frame, or the list of the map frames.
pub const MAP_LAYERS: &str = "Map layers";
pub const MAP_FRAMES: &str = "Map frames";
pub const NO_FRAMES: &str = "Add a map frame to see its layers.";
pub const GRID_SPACING: &str = "Grid spacing";
pub const GRID_COLOR: &str = "Grid color";
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

/// The width of the actions card in the wide layout: six buttons and a
/// divider. Each narrower layout moves one more button into the workspace
/// menu.
const ACTIONS_WIDTH: f32 = 6.0 * TOUCH + 6.0 * PAD + 9.0 + 2.0 * PAD;
const ADD_WIDTH: f32 = 220.0;
const TOOL_CELLS: usize = 5;
/// The smallest space at each side of the text of a tab.
pub const TAB_PAD: f32 = 12.0;

fn text_width(ctx: &egui::Context, text: &str) -> f32 {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), theme::body(), theme::TEXT));
    galley.size().x.ceil()
}

/// The width of the panel of the selection. The tabs of a frame fit in it.
pub(super) fn panel_width(ctx: &egui::Context) -> f32 {
    let text: f32 = FRAME_TABS.iter().map(|t| text_width(ctx, t)).sum();
    let tabs = text + 2.0 * TAB_PAD * FRAME_TABS.len() as f32 + 2.0 * PAD;
    geometry::RIGHT_PANEL_WIDTH.max(tabs + 2.0 * BODY_PAD)
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
    // The inspector is the inspector of the World workspace. It shares the
    // panel place with the panel of the selection, and it wins while it is
    // open.
    let attributes = !measuring && state.pad.is_open(super::Panel::Attributes);
    let input = Input {
        screen,
        bands: geometry::Bands::new(screen, ctx.content_rect()),
        workspace_width,
        view_width: page_width,
        panel: if measuring {
            state.pad.brush
        } else {
            state.atlas.selection != AtlasSelection::None || attributes || state.atlas.frames
        },
        sheet_offset: state.pad.sheet_offset,
        cells: if measuring {
            geometry::MEASURE_CELLS
        } else {
            0
        },
        strip: false,
        extra: 0,
        tools: TOOL_CELLS,
        row: false,
    };
    let mut rects = geometry::layout(&input);
    // Below 1,000 points, the switch of the layout grid is in the
    // workspace menu, and on a compact screen the Add button is there too.
    let hidden = match class {
        WidthClass::Wide => 0.0,
        WidthClass::Medium => 1.0,
        WidthClass::Compact => 2.0,
    };
    rects.actions.min.x = rects.actions.max.x - ACTIONS_WIDTH + hidden * (TOUCH + PAD);
    rects.right_panel.x += geometry::RIGHT_PANEL_WIDTH - panel_width(&ctx);
    if !measuring {
        rects.brush = None;
    }
    super::table::check(state);
    super::table::widen(state, &mut rects);
    state.sync_selection_form(attributes);
    crate::ui::select::drop_focus(&ctx, state);
    state.pad.sheet_offset = input
        .sheet_offset
        .clamp(-rects.sheet_rise, rects.sheet_travel);
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
        AtlasSelection::None if !attributes && !state.atlas.frames => None,
        selection => Some((selection, state.atlas.tab, attributes)),
    };
    // A frame, a page, or an item that a tool other than Select selects
    // closes the inspector, so that its panel shows.
    let before = state.atlas.shown.map(|(selection, _, _)| selection);
    let after = shown.map(|(selection, _, _)| selection);
    if attributes
        && state.atlas.tool != AtlasTool::Select
        && after.is_some_and(|s| s != AtlasSelection::None)
        && before != after
    {
        state.pad.close(super::Panel::Attributes);
    }
    let attributes = state.pad.is_open(super::Panel::Attributes) && !measuring;
    let shown = shown.map(|(selection, tab, _)| (selection, tab, attributes));
    if state.atlas.shown != shown {
        widgets::close_keypad(&ctx);
        state.atlas.shown = shown;
        state.atlas.item_page = None;
        state.atlas.label_color = None;
        state.pad.sheet_offset = 0.0;
    }

    top_row(&ctx, state, &mut controls, &rects, page_card);
    tool_strip(&ctx, state, &mut controls, &rects);
    frame_card(&ctx, state, &mut controls, &rects, page_card);
    if measuring {
        super::cell_card(&ctx, state, &mut controls, &rects);
        if state.pad.brush {
            super::show_panel(&ctx, state, &mut controls, &rects, super::Panel::Brush);
        }
    } else if attributes {
        super::show_panel(&ctx, state, &mut controls, &rects, super::Panel::Attributes);
        state.atlas.panel_rect = state.pad.panels.last().map(|&(_, rect)| rect);
    } else if state.atlas.selection != AtlasSelection::None {
        selection_panel(&ctx, state, &mut controls, &rects);
    } else if state.atlas.frames {
        frames_panel(&ctx, state, &mut controls, &rects);
    }
    graticule_color_frame(&ctx, state);
    neatline_color_frame(&ctx, state);
    super::labels::color_frame(&ctx, state);
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
            let mut x = 2.0 * step + 9.0 + PAD;
            if rects.class == WidthClass::Wide {
                let grid = at(x);
                x += step;
                let on = state.doc.project.atlas.grid.on;
                let icon = Icon::LayoutGrid;
                if widgets::icon_button_at(ui, controls, grid, SHOW_GRID, icon, on, true).clicked()
                {
                    atlas::edit_grid(state, |g| g.on = !on);
                }
            }
            if rects.class != WidthClass::Compact {
                let open = state.atlas.add;
                let add = at(x);
                x += step;
                if widgets::icon_button_at(ui, controls, add, ADD, Icon::Plus, open, true).clicked()
                {
                    state.atlas.add = !open;
                }
            }
            // The inspector and the layers share the panel place.
            let inspecting = state.pad.is_open(super::Panel::Attributes);
            let layers = at(x);
            x += step;
            let open = atlas::layers_open(state) && !inspecting;
            if widgets::icon_button_at(ui, controls, layers, MAP_LAYERS, Icon::Layers, open, true)
                .clicked()
            {
                state.pad.close(super::Panel::Attributes);
                if !(inspecting && atlas::layers_open(state)) {
                    atlas::toggle_layers(state);
                }
            }
            let inspector = at(x);
            let name = crate::ui::select::ATTRIBUTES;
            let open = state.pad.is_open(super::Panel::Attributes);
            if widgets::icon_button_at(ui, controls, inspector, name, Icon::Attributes, open, true)
                .clicked()
            {
                if open {
                    state.pad.close(super::Panel::Attributes);
                } else {
                    state.pad.open(super::Panel::Attributes);
                    state.atlas.frames = false;
                }
            }
        },
    );
}

/// The context strip: the card below the tool strip while a frame, an item
/// of furniture, or a feature is selected. With the Frame tool, it holds
/// the switch that snaps a frame to the layout of the page, "Delete
/// selection", and Deselect. With the Vertex tool, it holds the switch,
/// "Delete vertex", and Deselect. With the Select tool and the Move tool,
/// it holds Deselect. With no
/// room below the strip, it goes to the right of the strip, below the page
/// card if the page card is there. It ends above the sheet, and it scrolls
/// if it is too long.
fn frame_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    page_card: Rect,
) {
    let tool = state.atlas.tool;
    let framed = state.atlas.frame().is_some();
    let cells = match tool {
        AtlasTool::Vertex if framed => 3,
        AtlasTool::Frame if atlas::has_selection(state) => 2 + usize::from(framed),
        AtlasTool::Move if atlas::has_selection(state) => 1,
        AtlasTool::Select => {
            1 + usize::from(atlas::has_selection(state) || !state.feature_selection.is_empty())
        }
        _ => {
            state.pad.selectable = false;
            return;
        }
    };
    let mut selectable_cell = None;
    let mut size = vec2(rects.tools.width(), geometry::strip_height(cells));
    let limit = rects.sheet.map_or(rects.panel_bottom, |s| s.top() - GAP);
    let below = pos2(rects.tools.left(), rects.tools.bottom() + GAP);
    let min = if rects.tool_cells < TOOL_CELLS || below.y + size.y > limit {
        let top = match rects.view {
            None => page_card.bottom() + GAP,
            Some(_) => rects.tools.top(),
        };
        pos2(rects.tools.right() + GAP, top)
    } else {
        below
    };
    let room = limit - min.y;
    let scroll = size.y > room;
    if scroll {
        size.y = room.max(geometry::strip_height(1));
    }
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-frame-card",
        Rect::from_min_size(min, size),
        Order::Middle,
        theme::CARD_RADIUS.into(),
        |ui| {
            let mut ui = super::padded(ui);
            let mut cells = |ui: &mut egui::Ui| {
                let on = state.atlas.snap;
                let label = super::SNAP;
                if framed
                    && matches!(tool, AtlasTool::Frame | AtlasTool::Vertex)
                    && widgets::tool_cell(
                        ui,
                        controls,
                        SNAP_TO_LAYOUT,
                        label,
                        Icon::Magnet,
                        on,
                        true,
                    )
                    .clicked()
                {
                    state.atlas.snap = !on;
                }
                if tool == AtlasTool::Select {
                    selectable_cell = Some(super::selectable::cell(ui, state, controls));
                    if cells < 2 {
                        return;
                    }
                }
                if tool == AtlasTool::Vertex {
                    let (name, can) = (super::DELETE_VERTEX, atlas::can_delete_vertices(state));
                    if widgets::tool_cell(ui, controls, name, "Delete", Icon::Delete, false, can)
                        .clicked()
                    {
                        atlas::delete_vertices(state);
                    }
                } else if tool == AtlasTool::Frame {
                    let name = select::DELETE;
                    if widgets::tool_cell(ui, controls, name, "Delete", Icon::Trash, false, true)
                        .clicked()
                    {
                        atlas::delete_selection(state);
                    }
                }
                let name = super::DESELECT;
                if widgets::tool_cell(ui, controls, name, name, Icon::Close, false, true).clicked()
                {
                    if matches!(tool, AtlasTool::Vertex | AtlasTool::Select) {
                        atlas::deselect(state);
                    } else {
                        atlas::clear_selection(state);
                    }
                }
            };
            if scroll {
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut ui, cells);
            } else {
                cells(&mut ui);
            }
        },
    );
    match selectable_cell {
        Some(cell) if state.pad.selectable => {
            super::selectable::menu(ctx, state, controls, rects, cell);
        }
        _ => state.pad.selectable = false,
    }
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
            let mut cells = |ui: &mut egui::Ui| {
                for (name, label, icon, tool) in [
                    (FRAME_TOOL, "Frame", Icon::Frame, AtlasTool::Frame),
                    (select::TOOL, select::TOOL, Icon::Select, AtlasTool::Select),
                    (VERTEX, VERTEX, Icon::Vertex, AtlasTool::Vertex),
                    (MOVE, MOVE, Icon::Move, AtlasTool::Move),
                    (
                        super::MEASURE,
                        super::MEASURE,
                        Icon::Measure,
                        AtlasTool::Measure,
                    ),
                ] {
                    let active = state.atlas.tool == tool;
                    if widgets::tool_cell(ui, controls, name, label, icon, active, true).clicked() {
                        state.atlas.tool = tool;
                    }
                }
            };
            if rects.tool_cells < TOOL_CELLS {
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut ui, cells);
            } else {
                cells(&mut ui);
            }
        },
    );
}

fn readout(ctx: &egui::Context, ui: &egui::Ui, state: &AppState, rects: &Rects) {
    let Some(band) = rects.readout else {
        return;
    };
    let text = state
        .measure_readout()
        .unwrap_or_else(|| readout_text(state));
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
    let rows = 2 + atlas::ItemKind::ALL.len();
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
        if widgets::text_row(ui, controls, region::MAP_FROM_REGION, 0.0, false, true).clicked() {
            region::start(state);
        }
        for kind in atlas::ItemKind::ALL {
            if widgets::text_row(ui, controls, kind.name(), 0.0, false, true).clicked() {
                atlas::add_item(state, kind);
                state.atlas.add = false;
            }
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
    let rows = if compact { 5.0 } else { 3.0 };
    let rules = 1.0;
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
        rule(ui);
        if compact && widgets::text_row(ui, controls, WORLDS_ROW, 0.0, false, true).clicked() {
            state.pad.menu = false;
            toggle_worlds(state);
        }
        let on = state.doc.project.atlas.grid.on;
        if widgets::text_row(ui, controls, SHOW_GRID, 0.0, on, true).clicked() {
            state.pad.menu = false;
            atlas::edit_grid(state, |g| g.on = !on);
        }
        if compact && widgets::text_row(ui, controls, ADD, 0.0, false, true).clicked() {
            state.pad.menu = false;
            state.atlas.add = true;
        }
    });
}

/// The list of the map frames of the open page. A row selects its frame
/// and opens its Layers tab.
fn frames_panel(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let head = CardTitle {
        title: MAP_FRAMES,
        back: None,
        action: None,
    };
    let card = Card {
        key: "pad-atlas-frames",
        place: (rects.right_panel, panel_width(ctx)),
        head,
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.close {
            state.atlas.frames = false;
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        let frames = atlas::frame_names(state);
        if frames.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(NO_FRAMES).color(theme::MUTE));
        }
        for (id, name) in frames {
            if widgets::open_row(ui, controls, &name, "").clicked() {
                atlas::open_frame_layers(state, id);
            }
        }
    };
    let rect = show_card(ctx, state, controls, rects, card, on_title, top, body);
    state.atlas.panel_rect = Some(rect);
}

/// The panel of the selected frame or of the open page.
fn selection_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let place = (rects.right_panel, panel_width(ctx));
    // A drag of a label class moves the class, and not the panel.
    let scroll = match state.atlas.label_drag {
        Some(_) => egui::scroll_area::ScrollSource::NONE,
        None => egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertices.clear();
        }
    };
    let class_open = super::labels::open_class(state).is_some();
    let rect = match state.atlas.selection {
        AtlasSelection::None => return,
        AtlasSelection::Item(id) => {
            let Some(rect) = super::furniture::panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
        AtlasSelection::Frame(id) if class_open => {
            let Some(rect) = super::labels::class_panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
        AtlasSelection::Frame(id) if state.atlas.open_map_styles().is_some() => {
            let Some(rect) = map_style_panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
        AtlasSelection::Frame(id) if state.atlas.open_grid().is_some() => {
            let Some(rect) = super::grid_labels::panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
        AtlasSelection::Frame(id) if state.atlas.open_edge().is_some() => {
            let Some(rect) = super::edge_labels::panel(ctx, state, controls, rects, id) else {
                return;
            };
            rect
        }
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
                FrameTab::Labels => "pad-atlas-labels",
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
                    FrameTab::Labels => super::labels::tab(ui, state, controls, id),
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
    let named: Vec<_> = items.iter().map(|&(l, e)| (l, l, e)).collect();
    named_tabs(ui, controls, &named, active)
}

/// `tabs` with a name for each tab that differs from its label. Each item is
/// a name, a label, and the enabled state.
pub(super) fn named_tabs(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    items: &[(&str, &str, bool)],
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
        .map(|(_, label, _)| text_width(&ctx, label) + 2.0 * TAB_PAD)
        .collect();
    let room = row.width() - 2.0 * PAD;
    let rest = room - widths.iter().sum::<f32>();
    let n = items.len() as f32;
    // In a row that is too narrow, the tabs shrink in proportion.
    let shrink = (room / (room - rest)).min(1.0);
    let mut left = row.left() + PAD;
    let mut chosen = None;
    for (index, &(name, label, enabled)) in items.iter().enumerate() {
        let width = widths[index] * shrink + rest.max(0.0) / n;
        let rect = Rect::from_min_size(pos2(left, row.top() + PAD), vec2(width, TOUCH));
        left += width;
        let resp = widgets::tap_area(ui, controls, rect, name, enabled);
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
        (FRAME_TABS[3], true),
    ];
    let active = state.atlas.tab as usize;
    match tabs(ui, controls, &items, active) {
        Some(0) => state.atlas.tab = FrameTab::Frame,
        Some(1) => state.atlas.tab = FrameTab::Projection,
        Some(2) => state.atlas.tab = FrameTab::Layers,
        Some(3) => state.atlas.tab = FrameTab::Labels,
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
    map_style_rows(ui, state, controls, id, frame.map);

    let polygon = matches!(frame.shape, FrameShape::Polygon(_));
    let shapes = [RECTANGLE, POLYGON];
    if let Some(chosen) =
        widgets::choice_row(ui, controls, SHAPE, &shapes, Some(usize::from(polygon)))
    {
        atlas::set_polygon(state, id, chosen == 1);
    }
    if polygon && widgets::button_row(ui, controls, ADD_HOLE, true).clicked() {
        atlas::add_hole(state, id);
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
    let mut neatline = frame.neatline.on;
    if widgets::switch_row(ui, controls, NEATLINE, NEATLINE, &mut neatline) {
        atlas::set_neatline(state, id, neatline);
    }
    if frame.neatline.on {
        neatline_rows(ui, state, controls, &frame);
    }
    let mut border = frame.border.on;
    if widgets::switch_row(ui, controls, BORDER, BORDER, &mut border) {
        atlas::set_border(state, id, border);
    }
    if frame.border.on {
        border_rows(ui, state, controls, &frame);
    }
    let mut grid = frame.border.grid;
    if widgets::switch_row(ui, controls, REFERENCE_GRID, REFERENCE_GRID, &mut grid) {
        atlas::set_border_grid(state, id, grid);
    }
    if frame.border.grid {
        let active = GridColumns::ALL
            .iter()
            .position(|c| *c == frame.border.grid_columns);
        if let Some(i) = widgets::choice_row(ui, controls, GRID_COLUMNS, &GRID_COLUMN_NAMES, active)
        {
            atlas::set_grid_columns(state, id, GridColumns::ALL[i]);
        }
        let summary = super::grid_labels::summary(&frame.border.grid_text);
        if widgets::open_row(ui, controls, super::grid_labels::GRID_LABELS, &summary).clicked() {
            state.atlas.grid = Some(id);
            state.atlas.grid_page = None;
            state.atlas.label_color = None;
        }
    }
}

/// The Map style row of a map, which opens the Map style page, and the
/// buttons of a map with local changes.
fn map_style_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::FrameId,
    map: MapId,
) {
    let name = (state.doc.project.atlas.style_of(map)).map(|s| s.name.clone());
    let edited = !state.doc.local_changes(map).is_empty();
    let value = match &name {
        Some(name) if edited => edited_value(name),
        Some(name) => name.clone(),
        None => NO_MAP_STYLE.to_owned(),
    };
    if widgets::open_row(ui, controls, MAP_STYLE, &value).clicked() {
        state.atlas.map_styles = Some(id);
        state.atlas.map_style_swipe = None;
        state.atlas.map_style_name = None;
    }
    if edited {
        if widgets::button_row(ui, controls, UPDATE_MAP_STYLE, true).clicked() {
            state.update_map_style(map);
        }
        if widgets::button_row(ui, controls, RESET_TO_MAP_STYLE, true).clicked() {
            state.reset_to_map_style(map);
        }
    }
    ui.add_space(6.0);
}

/// The Map style page of frame `id`: "None" and each map style, with a
/// check on the map style of the map. A tap links the map style, and a
/// swipe to the left shows the Delete button of a map style. Below them are
/// the name of the linked map style and "Save as map style…".
pub(super) fn map_style_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: vale_store::FrameId,
) -> Option<Rect> {
    let map = state.doc.project.atlas.frame(id)?.map;
    let card = Card {
        key: "pad-atlas-map-style",
        place: (rects.right_panel, panel_width(ctx)),
        head: CardTitle {
            title: MAP_STYLE,
            back: Some(FRAME_TABS[0]),
            action: None,
        },
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.map_styles = None;
            state.atlas.map_style_swipe = None;
            state.atlas.map_style_name = None;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertices.clear();
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        map_style_body(ui, state, controls, map);
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

fn map_style_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, map: MapId) {
    use super::swipe;
    ui.spacing_mut().item_spacing.y = 0.0;
    let doc_atlas = &state.doc.project.atlas;
    let linked = doc_atlas.map(map).and_then(|m| m.style);
    let styles: Vec<_> = (doc_atlas.map_styles.iter())
        .map(|s| (s.id, s.name.clone()))
        .collect();
    // The first row is None, and it has no Delete button.
    let size = vec2(ui.available_width(), (styles.len() + 1) as f32 * PANEL_ROW);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let place = |i: usize| {
        let top = area.top() + i as f32 * PANEL_ROW;
        Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW))
    };
    swipe::shut_outside(ui, &mut state.atlas.map_style_swipe, |style| {
        styles
            .iter()
            .position(|s| s.0 == style)
            .map(|i| place(i + 1))
    });
    let (name, title, on) = (NO_MAP_STYLE_ROW, NO_MAP_STYLE, linked.is_none());
    let none = widgets::swipe_option_row(ui, controls, place(0), name, title, on, 0.0);
    if none.body.dragged() {
        ui.scroll_with_delta(vec2(0.0, none.body.drag_delta().y));
    }
    if none.body.clicked() {
        state.atlas.map_style_swipe = None;
        state.set_map_style_link(map, None);
    }
    for (i, (style, name)) in styles.iter().enumerate() {
        let style = *style;
        if let Some((_, text)) = state.atlas.map_style_name.as_mut().filter(|r| r.0 == style) {
            // The row is the name field while the user renames the map
            // style. The name changes when the field loses the focus.
            let taken = (styles.iter()).any(|(id, n)| *id != style && n == text.trim());
            let valid = !text.trim().is_empty() && !taken;
            let field = ui
                .push_id(("map style", style.0), |ui| {
                    let (rect, font) = (place(i + 1), theme::body());
                    widgets::field_at(ui, controls, rect, MAP_STYLE_NAME, None, text, valid, font)
                })
                .inner;
            if field.lost_focus() {
                let text = state.atlas.map_style_name.take().map(|r| r.1);
                atlas::rename_map_style(state, style, &text.unwrap_or_default());
            } else if !field.has_focus() {
                widgets::focus_all(ui, &field, name);
            }
            continue;
        }
        let open = state.atlas.map_style_swipe.filter(|s| s.row == style);
        let shift = swipe::shift(ui, state.atlas.map_style_swipe, style);
        let on = linked == Some(style);
        let row = ui
            .push_id(("map style", style.0), |ui| {
                let title = map_style_row(name);
                widgets::swipe_option_row(ui, controls, place(i + 1), &title, name, on, shift)
            })
            .inner;
        if row.delete {
            state.atlas.map_style_swipe = None;
            state.atlas.map_style_name = None;
            state.delete_map_style(style);
            continue;
        }
        if row.rename {
            state.atlas.map_style_swipe = None;
            state.atlas.map_style_name = Some((style, name.clone()));
            continue;
        }
        let width = widgets::RENAME_WIDTH;
        if swipe::drag(
            ui,
            &mut state.atlas.map_style_swipe,
            style,
            &row.body,
            width,
        ) {
            continue;
        }
        if row.body.dragged() {
            ui.scroll_with_delta(vec2(0.0, row.body.drag_delta().y));
        }
        if row.body.clicked() {
            if open.is_some() {
                state.atlas.map_style_swipe = None;
            } else if !on {
                state.atlas.map_style_name = None;
                state.set_map_style_link(map, Some(style));
            }
        }
    }
    ui.add_space(14.0);
    if widgets::button_row(ui, controls, SAVE_AS_MAP_STYLE, true).clicked() {
        state.atlas.map_style_swipe = None;
        // The row of the new map style is its name field.
        let made = state.save_as_map_style(map);
        let name =
            made.and_then(|id| Some((id, state.doc.project.atlas.map_style(id)?.name.clone())));
        state.atlas.map_style_name = name;
    }
}

/// The rows of the graticule of a frame: the interval, the color, the
/// width, and the edge labels. The interval stepper shows the interval
/// that the frame draws, and its buttons set the interval by hand. A tap on the color opens a color
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
    let range = INTERVALS[0]..=INTERVALS[last];
    let interval = Number::new(INTERVAL, INTERVALS[index], range)
        .choices(&INTERVALS)
        .unit("°")
        .text(interval_text)
        .stepper(ui, controls);
    if let Some(chosen) = interval
        && g.interval != Some(chosen)
    {
        atlas::set_graticule_interval(state, id, Some(chosen));
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
        if widgets::color_picker_alpha(ui, controls, GRATICULE_COLOR, &mut color) {
            atlas::set_graticule_color(state, id, color);
        }
        state.atlas.graticule_shown = true;
    }

    if let Some(w) = Number::new(GRATICULE_WIDTH, g.width, Graticule::WIDTHS)
        .step(0.1)
        .unit("pt")
        .stepper(ui, controls)
    {
        atlas::set_graticule_width(state, id, w);
    }

    let summary = super::edge_labels::summary(g);
    if widgets::open_row(ui, controls, super::edge_labels::EDGE_LABELS, summary).clicked() {
        state.atlas.edge = Some(id);
        state.atlas.edge_styles = false;
        state.atlas.graticule_color = false;
    }
}

/// The rows of the neatline of a frame: the color and the width. A tap on the
/// color opens a color picker below it.
fn neatline_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    frame: &vale_store::FrameRecord,
) {
    let id = frame.id;
    let b = &frame.neatline;
    let open = state.atlas.neatline_color;
    let row = widgets::color_row(ui, controls, NEATLINE_COLOR, b.color, open);
    if row.clicked() {
        state.atlas.neatline_color = !open;
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.neatline_color {
        ui.add_space(4.0);
        let mut color = b.color;
        if widgets::color_picker_alpha(ui, controls, NEATLINE_COLOR, &mut color) {
            atlas::set_neatline_color(state, id, color);
        }
        state.atlas.neatline_shown = true;
    }

    if let Some(w) = Number::new(NEATLINE_WIDTH, b.width, Neatline::WIDTHS)
        .nice()
        .step(0.05)
        .unit("pt")
        .stepper(ui, controls)
    {
        atlas::set_neatline_width(state, id, w);
    }
}

/// The rows of the border of a frame: the style, the colors, the width,
/// the distance, and the stroke.
fn border_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    frame: &vale_store::FrameRecord,
) {
    let (id, b) = (frame.id, &frame.border);
    let active = BorderStyle::ALL.iter().position(|s| *s == b.style);
    if let Some(i) = widgets::choice_row(ui, controls, BORDER_STYLE, &BORDER_STYLES, active) {
        atlas::set_border_style(state, id, BorderStyle::ALL[i]);
    }
    let first = ClassColor::Border(false);
    if let Some(c) = text_style::rgba_rows(ui, state, controls, BORDER_COLOR, first, b.color) {
        atlas::set_border_color(state, id, false, c);
    }
    if b.style == BorderStyle::Graduated {
        let second = ClassColor::Border(true);
        if let Some(c) = text_style::rgba_rows(ui, state, controls, SECOND_COLOR, second, b.color2)
        {
            atlas::set_border_color(state, id, true, c);
        }
    }
    let mm = |name, value, range, step| Number::new(name, value, range).step(step).unit("mm");
    if let Some(w) = mm(BORDER_WIDTH, b.width_mm, Border::WIDTHS, 0.05)
        .nice()
        .stepper(ui, controls)
    {
        atlas::set_border_width(state, id, w);
    }
    let distance = |name, d| mm(name, d, Border::DISTANCES, 0.5);
    let mut by_side = atlas::border_by_side(state, b);
    if !by_side {
        let d = b.distances_mm[0];
        if let Some(d) = distance(BORDER_DISTANCE, d).stepper(ui, controls) {
            atlas::set_border_distance(state, id, None, d);
        }
    }
    if widgets::switch_row(
        ui,
        controls,
        DISTANCE_BY_SIDE,
        DISTANCE_BY_SIDE,
        &mut by_side,
    ) {
        atlas::set_border_by_side(state, id, by_side);
    }
    if atlas::border_by_side(state, b) {
        for (i, name) in SIDE_DISTANCES.into_iter().enumerate() {
            if let Some(d) = distance(name, b.distances_mm[i]).stepper(ui, controls) {
                atlas::set_border_distance(state, id, Some(i), d);
            }
        }
    }

    if let Some(g) = mm(BORDER_GAP, b.gap_mm, Border::GAPS, 0.25).stepper(ui, controls) {
        atlas::set_border_gap(state, id, g);
    }

    let mut stroke = b.stroke;
    if widgets::switch_row(ui, controls, BORDER_STROKE, BORDER_STROKE, &mut stroke) {
        atlas::set_border_stroke(state, id, stroke);
    }
    if b.stroke {
        let which = ClassColor::BorderStroke;
        if let Some(c) =
            text_style::rgba_rows(ui, state, controls, STROKE_COLOR, which, b.stroke_color)
        {
            atlas::set_border_stroke_color(state, id, c);
        }
        if let Some(w) = Number::new(STROKE_WIDTH, b.stroke_width, Border::STROKE_WIDTHS)
            .nice()
            .step(0.05)
            .unit("pt")
            .stepper(ui, controls)
        {
            atlas::set_border_stroke_width(state, id, w);
        }
    }
}

/// Closes the color picker of the neatline when it did not show in this
/// frame, with the keypad of its field.
fn neatline_color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.neatline_shown);
    if !shown && std::mem::take(&mut state.atlas.neatline_color) {
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
    let edited = state.doc.local_changes(map);
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
                    state.selected_vector() == Some(layer),
                    true,
                    false,
                    0.0,
                    edited.contains(&layer).then_some(EDITED),
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
    if let Some(layer) = open {
        state.select_vector(layer);
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
    let rules = state.doc.map_style(map, layer)?.style.rules;
    let page = match state.atlas.layer_page {
        Some(
            Page::Rule(i)
            | Page::Condition(i)
            | Page::Symbols(i)
            | Page::MarkerShape(i)
            | Page::MarkerFill(i),
        ) if i >= rules.len() => None,
        Some(
            Page::Rule(_)
            | Page::Condition(_)
            | Page::Symbols(_)
            | Page::MarkerShape(_)
            | Page::MarkerFill(_),
        ) => state.atlas.layer_page,
        Some(Page::Classify | Page::Ramps | Page::RampLabels(_) | Page::Mask) => {
            state.atlas.layer_page
        }
        Some(Page::Table) => state.atlas.layer_page,
        _ => None,
    };
    state.atlas.layer_page = page;
    let rule_name = match page {
        Some(
            Page::Rule(i)
            | Page::Condition(i)
            | Page::Symbols(i)
            | Page::MarkerShape(i)
            | Page::MarkerFill(i),
        ) => rules[i].name.clone(),
        _ => String::new(),
    };
    let (key, title, back) = match page {
        Some(Page::Condition(_)) => (
            "pad-atlas-condition",
            super::filter::ADD_CONDITION,
            rule_name.as_str(),
        ),
        Some(Page::Symbols(_)) => ("pad-atlas-symbols", super::rules::SYMBOLS, &*rule_name),
        Some(Page::MarkerShape(_)) => (
            "pad-atlas-marker-shape",
            super::rules::MARKER_SHAPE,
            &*rule_name,
        ),
        Some(Page::MarkerFill(_)) => ("pad-atlas-marker-fill", super::rules::FILL, &*rule_name),
        Some(Page::Classify) => ("pad-atlas-classify", super::classify::TITLE, name.as_str()),
        Some(Page::Mask) => ("pad-atlas-mask", super::rules::MASK, name.as_str()),
        Some(Page::Ramps) => (
            "pad-atlas-classify-ramps",
            super::raster::COLOR_RAMPS,
            super::classify::TITLE,
        ),
        Some(Page::RampLabels(_)) => (
            "pad-atlas-classify-ramp-labels",
            super::raster::LABELS,
            super::raster::COLOR_RAMPS,
        ),
        Some(Page::Table) => ("pad-atlas-table", super::table::TITLE, name.as_str()),
        Some(_) => ("pad-atlas-rule", rule_name.as_str(), name.as_str()),
        None => ("pad-atlas-layer", name.as_str(), FRAME_TABS[2]),
    };
    let head = CardTitle {
        title,
        back: Some(back),
        action: None,
    };
    // The panel is wider while it shows the table.
    let right = rects.screen.right() - rects.bands.right - MARGIN;
    let card = Card {
        key,
        place: (rects.right_panel, right - rects.right_panel.x),
        head,
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            state.atlas.layer_page = match page {
                Some(
                    Page::Condition(i)
                    | Page::Symbols(i)
                    | Page::MarkerShape(i)
                    | Page::MarkerFill(i),
                ) => Some(Page::Rule(i)),
                Some(Page::Ramps) => Some(Page::Classify),
                Some(Page::RampLabels(_)) => Some(Page::Ramps),
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
            state.atlas.vertices.clear();
        }
    };
    let table = page == Some(Page::Table);
    // The table fills the page, and its rows have their own scroll. The
    // attributes are not a part of the style of the map.
    let top = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if table {
            let size = ui.available_size();
            ui.allocate_ui(size, |ui| super::table::page(ui, state, controls));
        }
    };
    let features = state.doc.layer(layer)?.features.len().to_string();
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if table {
            return;
        }
        state.edit_map_style(map, layer, |state| match page {
            Some(Page::Rule(i)) => super::rules::rule_body(ui, state, controls, layer, i),
            Some(Page::Condition(i)) => {
                super::filter::condition_body(ui, state, controls, layer, i);
            }
            Some(Page::Symbols(i)) => super::rules::symbols_body(ui, state, controls, layer, i),
            Some(Page::MarkerShape(i)) => {
                super::rules::shape_body(ui, state, controls, layer, i);
            }
            Some(Page::MarkerFill(i)) => {
                super::rules::marker_fill_body(ui, state, controls, layer, i);
            }
            Some(Page::Classify) => super::classify::body(ui, state, controls, layer),
            Some(Page::Mask) => super::rules::mask_body(ui, state, controls, layer),
            Some(Page::Ramps) => super::raster::ramps_body(
                ui,
                state,
                controls,
                super::raster::RampUser::Classify(layer),
            ),
            Some(Page::RampLabels(i)) => super::raster::labels_body(ui, state, controls, i),
            _ => super::rules::section(ui, state, controls, layer),
        });
        if page.is_none() {
            style_source_rows(ui, state, controls, map, layer);
            ui.add_space(8.0);
            if widgets::open_row(ui, controls, super::table::FEATURES, &features).clicked() {
                state.open_table(layer);
            }
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// The rows that set the style of a layer in a map from its map style or
/// from the globe style.
fn style_source_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    layer: LayerId,
) {
    ui.add_space(8.0);
    if state.doc.local_changes(map).contains(&layer)
        && widgets::button_row(ui, controls, RESET_TO_MAP_STYLE, true).clicked()
    {
        state.reset_layer_to_map_style(map, layer);
    }
    let differs = state.differs_from_globe(map, layer);
    if widgets::button_row(ui, controls, COPY_GLOBE_STYLE, differs).clicked() {
        state.copy_globe_style(map, layer);
    }
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
    use super::raster::{BLEND, COLOR_RAMPS, COLOR_STOPS, HILLSHADE, LABELS};
    let map = state.doc.project.atlas.frame(id)?.map;
    let name = state.doc.layer(layer)?.name.clone();
    let page = state.atlas.layer_page.filter(|p| {
        matches!(
            p,
            Page::Stops | Page::Ramps | Page::RampLabels(_) | Page::Hillshade | Page::Blend
        )
    });
    state.atlas.layer_page = page;
    let (key, title, back) = match page {
        Some(Page::Ramps) => ("pad-atlas-ramps", COLOR_RAMPS, COLOR_STOPS),
        Some(Page::RampLabels(_)) => ("pad-atlas-ramp-labels", LABELS, COLOR_RAMPS),
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
                Some(Page::RampLabels(_)) => Some(Page::Ramps),
                Some(_) => None,
                None => {
                    state.atlas.layer = None;
                    None
                }
            };
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertices.clear();
        }
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        state.edit_map_raster(map, layer, raster, |state| match state.pad.page {
            Some(Page::Stops) => super::raster::stops_body(ui, state, controls, raster),
            Some(Page::Ramps) => super::raster::ramps_body(
                ui,
                state,
                controls,
                super::raster::RampUser::Raster(raster),
            ),
            Some(Page::RampLabels(i)) => super::raster::labels_body(ui, state, controls, i),
            Some(Page::Hillshade) => {
                super::raster::hillshade_body(ui, state, controls, raster);
            }
            Some(Page::Blend) => super::raster::blend_body(ui, state, controls, raster),
            _ => {
                let Some(opacity) = state.doc.project.raster(raster).map(|r| r.opacity) else {
                    return;
                };
                if let Some(v) = Number::new(super::OPACITY, f64::from(opacity), 0.0..=1.0)
                    .label("Opacity")
                    .percent()
                    .slider(ui, controls)
                {
                    state.set_raster_opacity(raster, v as f32);
                }
                super::raster::style(ui, state, controls, raster);
            }
        });
        if page.is_none() {
            style_source_rows(ui, state, controls, map, layer);
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let rect = show_card(ctx, state, controls, rects, card, on_title, top, body);
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
            state.atlas.vertices.clear();
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
            if let Some(v) = Number::new(super::OPACITY, f64::from(look.opacity), 0.0..=1.0)
                .label("Opacity")
                .percent()
                .slider(ui, controls)
            {
                look.opacity = v as f32;
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
            style_source_rows(ui, state, controls, map, layer);
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
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
    if let Some(reason) = atlas::suggestion_reason(state, id) {
        let text = egui::RichText::new(reason).color(theme::MUTE);
        ui.add(egui::Label::new(text).truncate());
        ui.add_space(4.0);
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

/// The switch of the layout grid, and its spacing, color, and opacity while
/// it is on.
fn grid_rows(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, units: PageUnits) {
    let grid = state.doc.project.atlas.grid.clone();
    let mut on = grid.on;
    if widgets::switch_row(ui, controls, LAYOUT_GRID, LAYOUT_GRID, &mut on) {
        atlas::edit_grid(state, |g| g.on = on);
    }
    if !grid.on {
        return;
    }
    let parse = move |text: &str| {
        atlas::parse_length(text, units).filter(|mm| LayoutGrid::SPACINGS.contains(mm))
    };
    let shown = length_text(grid.spacing_mm, units);
    let suffix = unit_name(units);
    let name = GRID_SPACING;
    let typed = widgets::key_field(
        ui,
        controls,
        name,
        name,
        suffix,
        &shown,
        Keys::Number,
        |t| parse(t).is_some(),
    );
    if let Some(mm) = typed.as_deref().and_then(parse) {
        atlas::edit_grid(state, |g| g.spacing_mm = mm);
    }
    keypad_below(ui, controls, GRID_SPACING);
    let which = ClassColor::LayoutGrid;
    if let Some(c) = text_style::rgba_rows(ui, state, controls, GRID_COLOR, which, grid.color) {
        atlas::edit_grid(state, |g| g.color = c);
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
    let paper = atlas::open_page(state).paper;
    let which = ClassColor::Paper;
    if let Some(c) = text_style::rgb_rows(ui, state, controls, PAPER_COLOR, which, paper) {
        atlas::set_paper(state, c);
    }
    grid_rows(ui, state, controls, units);

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
