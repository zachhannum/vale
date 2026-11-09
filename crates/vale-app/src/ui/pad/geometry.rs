//! The places of the cards of the iPad layout. All sizes are in points.

use eframe::egui::{Pos2, Rect, pos2, vec2};

/// The space from a card to the edge of the screen.
pub const MARGIN: f32 = 16.0;
/// The space between two cards in a column.
pub const GAP: f32 = 12.0;
/// The space between two panels in a row.
pub const PANEL_GAP: f32 = 8.0;
/// The padding of a bar, a strip, and a segmented list.
pub const PAD: f32 = 4.0;
/// The smallest side of a control.
pub const TOUCH: f32 = 44.0;
/// The height of the top row and of the title row of a panel.
pub const ROW: f32 = TOUCH + 2.0 * PAD;
/// The size of one cell of the tool strip.
pub const CELL: (f32, f32) = (56.0, 52.0);
/// The cells of the tool strip of the globe: Sculpt, Terrain, Draw, Select,
/// Move, and Measure.
pub const TOOLS: usize = 6;
/// The number of cells of the card of the Measure tool.
pub const MEASURE_CELLS: usize = 3;
/// The number of cells of the card of the Vertex tool.
pub const VERTEX_CELLS: usize = 5;
/// The number of cells of the card of the Select tool.
pub const SELECT_CELLS: usize = 5;
/// The height of one of the two sliders at the left edge.
pub const SLIDER: f32 = 140.0;
/// Below this height, a slider at the left edge is too short to use.
pub const SLIDER_MIN: f32 = 110.0;
pub const BRUSH_PANEL_WIDTH: f32 = 340.0;
pub const RIGHT_PANEL_WIDTH: f32 = 320.0;
pub const MENU_WIDTH: f32 = 240.0;
/// The width of the menu of the worlds.
pub const WORLDS_WIDTH: f32 = 300.0;
/// The width of the flyout of the brush modes.
pub const MODES_WIDTH: f32 = 200.0;
/// The height of an open sheet, about half the height of a phone.
pub const SHEET_HEIGHT: f32 = 452.0;
/// The space between two cards of the row layout.
pub const ROW_GAP: f32 = 8.0;
pub const READOUT_HEIGHT: f32 = 32.0;
/// The width of the actions card of the compact layout: Undo, Redo, a
/// divider, and the Panels button.
pub const NARROW_ACTIONS_WIDTH: f32 = 3.0 * TOUCH + 3.0 * PAD + 9.0 + 2.0 * PAD;

/// The height of the top bar.
pub const BAR: f32 = 44.0;
/// The width of the space of a divider in a row of buttons.
pub const DIVIDER: f32 = 9.0;
/// The name of the world becomes shorter down to this width.
pub const NAME_MIN: f32 = 80.0;
/// The space from the bar to a card below it.
pub const BAR_GAP: f32 = 8.0;
/// The width of the tool column.
pub const COLUMN: f32 = 56.0;
/// The space from the edge of the tool column to its cells.
pub const COLUMN_INSET: f32 = 6.0;
/// The side of a cell of the tool column.
pub const COLUMN_CELL: f32 = 44.0;
/// The height of the divider between the tools and the controls of the
/// tool.
pub const COLUMN_DIVIDER: f32 = 13.0;
/// The space of a vertical slider of the tool column, above and below its
/// track, for its value and its name.
pub const SLIDER_TEXT: f32 = 44.0;
/// The length of the track of a vertical slider of the tool column.
pub const TRACK: f32 = 120.0;
/// Below this length, the sliders go to the Brush panel.
pub const TRACK_MIN: f32 = 90.0;
/// The size of the readout at the lower left of the canvas.
pub const READOUT_SIZE: (f32, f32) = (160.0, 48.0);
/// The space from the tool column to the readout.
pub const READOUT_GAP: f32 = 16.0;
/// The space from the right edge of the canvas to a floating panel.
pub const FLOAT_MARGIN: f32 = 12.0;

/// The bands at the screen edges that the system uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bands {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Bands {
    /// The status bar is 24 points high, and the home indicator is 20 points high.
    pub const MIN: Bands = Bands {
        top: 24.0,
        bottom: 20.0,
        left: 0.0,
        right: 0.0,
    };

    /// The bands of a screen with this safe area.
    pub fn new(screen: Rect, safe: Rect) -> Bands {
        Bands {
            top: (safe.top() - screen.top()).max(Bands::MIN.top),
            bottom: (screen.bottom() - safe.bottom()).max(Bands::MIN.bottom),
            left: (safe.left() - screen.left()).max(0.0),
            right: (screen.right() - safe.right()).max(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidthClass {
    /// 1,000 points or more. The workspace switch shows all workspaces.
    Wide,
    /// From 600 points. The workspace switch is a menu button.
    Medium,
    /// Less than 600 points. The menu holds the views, and a panel is a sheet.
    Compact,
}

impl WidthClass {
    pub fn of(width: f32) -> WidthClass {
        if width >= 1000.0 {
            WidthClass::Wide
        } else if width >= 600.0 {
            WidthClass::Medium
        } else {
            WidthClass::Compact
        }
    }
}

/// The contents of the card below the tool strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushCard {
    /// The two sliders, each with this height, and the Brush button.
    Full(f32),
    /// The Brush button alone. The Brush panel holds the two sliders.
    Button,
    /// An open sheet covers the place of the card.
    Hidden,
}

/// The contents of the tool column.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColumnInput {
    /// The number of tool cells.
    pub tools: usize,
    /// The number of cells of the controls of the tool: switches and
    /// commands.
    pub cells: usize,
    /// The number of vertical sliders.
    pub sliders: usize,
    /// The column ends with the Brush button.
    pub brush_button: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub screen: Rect,
    pub bands: Bands,
    /// The width of the workspace switch, which follows from its text.
    pub workspace_width: f32,
    /// The width of the view switch.
    pub view_width: f32,
    /// A panel is open. On a compact screen, it is a sheet.
    pub panel: bool,
    /// The distance that the sheet is pulled down from its open place. A
    /// negative distance pulls it up.
    pub sheet_offset: f32,
    /// The number of cells of the card of the tool, for a tool with a card
    /// of cells, such as the Measure tool. Zero: the tool has no such card.
    pub cells: usize,
    /// The tool has a context strip: the card below the tool strip.
    pub strip: bool,
    /// The cells that the context strip has for each tool, such as
    /// Deselect. A tool with no context strip then has a strip of these
    /// cells alone.
    pub extra: usize,
    /// The number of cells of the tool strip.
    pub tools: usize,
    /// The width of the dock. Zero: the dock is closed, or the screen is
    /// narrower than 1,000 points.
    pub dock_width: f32,
    /// The width of the name button of the world at its full length.
    pub name_width: f32,
    pub column: ColumnInput,
}

impl Input {
    /// The input of a screen with no cards, with the sizes of the bar of
    /// the World workspace.
    pub fn new(screen: Rect, bands: Bands) -> Input {
        Input {
            screen,
            bands,
            workspace_width: 0.0,
            view_width: 0.0,
            panel: false,
            sheet_offset: 0.0,
            cells: 0,
            strip: false,
            extra: 0,
            tools: TOOLS,
            dock_width: 0.0,
            name_width: NAME_MIN,
            column: ColumnInput {
                tools: TOOLS,
                ..ColumnInput::default()
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rects {
    pub class: WidthClass,
    pub screen: Rect,
    pub bands: Bands,
    /// The card of the workspace switch. From 600 points, the name button of
    /// the world, at the center of the canvas.
    pub workspace: Rect,
    /// The view control. `None`: the workspace menu holds the views.
    pub view: Option<Rect>,
    /// The card of the actions. From 600 points, the panel buttons and the
    /// Dock button in the dock zone of the bar.
    pub actions: Rect,
    /// The top-left corner of the workspace menu, or of the menu of the
    /// worlds.
    pub menu: Pos2,
    /// The top band of the system. From 600 points, the bar fills it.
    pub band: Rect,
    /// The top bar. On a compact screen, the place of the top row.
    pub bar: Rect,
    /// Back, at the left end of the bar.
    pub back: Rect,
    pub undo: Rect,
    pub redo: Rect,
    /// The space below the bar, from the left edge to the dock.
    pub canvas: Rect,
    /// The open dock.
    pub dock: Option<Rect>,
    /// The right part of the bar that holds the panel buttons. It is above
    /// the dock while the dock is open.
    pub dock_zone: Rect,
    /// The Dock button. Below 1,000 points, the bar has no Dock button.
    pub dock_button: Option<Rect>,
    /// The two workspace buttons, between Back and Undo.
    pub tabs: Rect,
    /// The globe fits this rectangle.
    pub fit: Rect,
    /// The page of the atlas fits this rectangle.
    pub free: Rect,
    /// The tool column at the left edge. Its height follows its contents,
    /// down to the space that the screen has.
    pub column: Rect,
    /// The height of the contents of the tool column. With a height more
    /// than the height of `column`, the column scrolls.
    pub column_content: f32,
    /// The length of the track of each vertical slider of the column. Zero:
    /// the Brush panel holds the sliders.
    pub slider: f32,
    pub tools: Rect,
    /// The number of tool cells that show. With fewer than `Input::tools`, the strip scrolls.
    pub tool_cells: usize,
    pub brush_card: BrushCard,
    /// The card of the two sliders and the Brush button, or the card of the
    /// Measure tool.
    pub brush: Option<Rect>,
    /// The number of cells of the card of cells that show. With fewer than
    /// the cells of the tool and the extra cells, the card scrolls. Zero:
    /// the card is not a card of cells.
    pub card_cells: usize,
    /// The top-left corner of the Brush panel.
    pub brush_panel: Pos2,
    /// The top-left corner of the Layers and Toolbox panel.
    pub right_panel: Pos2,
    /// A panel does not go below this line.
    pub panel_bottom: f32,
    /// True: the Brush panel and the right panel do not fit side by side.
    pub one_panel: bool,
    /// The sheet of a compact screen, at its present place.
    pub sheet: Option<Rect>,
    /// The largest `sheet_offset`. At this offset, the sheet shows its title row only.
    pub sheet_travel: f32,
    /// The smallest `sheet_offset` is minus this distance. At that offset,
    /// the sheet has the full height.
    pub sheet_rise: f32,
    /// The tool strip and its card are rows at the bottom.
    pub row: bool,
    /// The top of the space below the top row.
    pub column_top: f32,
    pub readout: Option<Rect>,
}

/// The right edge of a floating panel at the right.
pub fn float_right(rects: &Rects) -> f32 {
    let edge = rects.screen.right() - rects.bands.right;
    match rects.class {
        WidthClass::Compact => edge - MARGIN,
        _ => rects.canvas.right().min(edge) - FLOAT_MARGIN,
    }
}

/// The top of a floating panel at the right from `left` to `right`: the top
/// of the right panel, or below the view control if the panel would cover
/// it.
pub fn float_top(rects: &Rects, left: f32, right: f32) -> f32 {
    match rects.view {
        Some(view)
            if rects.class != WidthClass::Compact
                && left < view.right() + GAP
                && right > view.left() - GAP =>
        {
            rects.right_panel.y.max(view.bottom() + GAP)
        }
        _ => rects.right_panel.y,
    }
}

/// The height of a card that holds `cells` tool cells.
pub fn strip_height(cells: usize) -> f32 {
    cells as f32 * CELL.1 + 2.0 * PAD
}

/// The width of a row that holds `cells` tool cells.
pub fn strip_width(cells: usize) -> f32 {
    cells as f32 * CELL.0 + 2.0 * PAD
}

/// The places of the parts of the bar, the canvas, and the tool column from
/// 600 points.
struct Bar {
    band: Rect,
    bar: Rect,
    back: Rect,
    name: Rect,
    undo: Rect,
    redo: Rect,
    canvas: Rect,
    dock: Option<Rect>,
    dock_zone: Rect,
    actions: Rect,
    dock_button: Option<Rect>,
    tabs: Rect,
    view: Rect,
    fit: Rect,
    free: Rect,
    column: Rect,
    column_content: f32,
    slider: f32,
    readout: Rect,
    right_panel: Pos2,
}

fn bar_layout(input: &Input, class: WidthClass) -> Bar {
    let Input { screen, bands, .. } = *input;
    let band = Rect::from_min_max(screen.min, pos2(screen.right(), screen.top() + bands.top));
    let bar = Rect::from_min_max(
        pos2(screen.left(), band.bottom()),
        pos2(screen.right(), band.bottom() + BAR),
    );
    let bottom = screen.bottom() - bands.bottom;
    let dock_width = if class == WidthClass::Wide {
        input.dock_width.max(0.0)
    } else {
        0.0
    };
    let canvas_right = if dock_width > 0.0 {
        screen.right() - bands.right - dock_width
    } else {
        screen.right()
    };
    let canvas = Rect::from_min_max(
        pos2(screen.left(), bar.bottom()),
        pos2(canvas_right, screen.bottom()),
    );
    let dock = (dock_width > 0.0)
        .then(|| Rect::from_min_max(pos2(canvas_right, bar.bottom()), screen.max));

    // The dock zone: the Panels button, a divider, and the Dock button.
    let button = |x: f32| Rect::from_min_size(pos2(x, bar.top()), vec2(TOUCH, BAR));
    let wide = class == WidthClass::Wide;
    let buttons_width = TOUCH + if wide { DIVIDER + TOUCH } else { 0.0 };
    let right = screen.right() - bands.right - PAD;
    let actions = Rect::from_min_max(
        pos2(right - buttons_width, bar.top()),
        pos2(right, bar.bottom()),
    );
    let dock_button = wide.then(|| button(right - TOUCH));
    let zone_left = match dock {
        Some(dock) => dock.left().min(actions.left() - PAD),
        None => actions.left() - PAD,
    };
    let dock_zone = Rect::from_min_max(pos2(zone_left, bar.top()), bar.max);

    // The left group: Back, the two workspace buttons, a divider, Undo, and
    // Redo.
    let back = button(screen.left() + bands.left + PAD);
    let tabs = Rect::from_min_size(pos2(back.right(), bar.top()), vec2(2.0 * TOUCH, BAR));
    let undo = button(tabs.right() + DIVIDER);
    let redo = button(undo.right());

    // The name of the world, at the center of the canvas. It becomes
    // shorter down to `NAME_MIN`, and a name that does not fit at the center
    // moves to the side with more room.
    let center = canvas.center().x;
    let (start, end) = (redo.right() + PAD, actions.left() - PAD);
    let half = (center - start).min(end - center);
    let name_width = input.name_width.min(2.0 * half).max(NAME_MIN);
    let name_left = (center - name_width / 2.0).min(end - name_width).max(start);
    let name = Rect::from_min_size(pos2(name_left, bar.top()), vec2(name_width, BAR));

    // The view control, at the top center of the canvas.
    let view = Rect::from_center_size(
        pos2(center, bar.bottom() + BAR_GAP + ROW / 2.0),
        vec2(input.view_width, ROW),
    );
    let fit = Rect::from_min_max(
        pos2(canvas.left(), view.bottom() + BAR_GAP),
        pos2(canvas.right(), bottom),
    );

    // The tool column: the tools, a divider, the cells and the sliders of
    // the tool, and the Brush button.
    let c = input.column;
    let top = bar.bottom() + BAR_GAP;
    let room = bottom - BAR_GAP - top;
    let context = c.cells > 0 || c.sliders > 0 || c.brush_button;
    let fixed = 2.0 * COLUMN_INSET
        + COLUMN_CELL * (c.tools + c.cells + usize::from(c.brush_button)) as f32
        + if context { COLUMN_DIVIDER } else { 0.0 };
    let slider = if c.sliders == 0 {
        0.0
    } else {
        let track = ((room - fixed) / c.sliders as f32 - SLIDER_TEXT).min(TRACK);
        if track >= TRACK_MIN { track } else { 0.0 }
    };
    let column_content = fixed
        + if slider > 0.0 {
            c.sliders as f32 * (slider + SLIDER_TEXT)
        } else {
            0.0
        };
    let column = Rect::from_min_size(
        pos2(screen.left() + bands.left, top),
        vec2(COLUMN, column_content.min(room)),
    );

    // The page keeps the same space at the left and at the right, so it is
    // at the center of the canvas.
    let side = column.right() + GAP - canvas.left();
    let free = Rect::from_min_max(
        pos2(canvas.left() + side, view.bottom() + GAP),
        pos2(
            (canvas.right() - side).max(canvas.left() + side),
            bottom - GAP,
        ),
    );
    let readout = Rect::from_min_max(
        pos2(
            column.right() + READOUT_GAP,
            bottom - BAR_GAP - READOUT_SIZE.1,
        ),
        pos2(
            column.right() + READOUT_GAP + READOUT_SIZE.0,
            bottom - BAR_GAP,
        ),
    );
    // A floating panel that would cover the view control goes below it.
    let float_right = canvas.right().min(screen.right() - bands.right);
    let float_left = float_right - FLOAT_MARGIN - RIGHT_PANEL_WIDTH;
    let float_top = if float_left < view.right() + GAP {
        view.bottom() + GAP
    } else {
        bar.bottom() + BAR_GAP
    };
    let right_panel = pos2(float_left, float_top);
    Bar {
        band,
        bar,
        back,
        name,
        undo,
        redo,
        canvas,
        dock,
        dock_zone,
        actions,
        dock_button,
        tabs,
        view,
        fit,
        free,
        column,
        column_content,
        slider,
        readout,
        right_panel,
    }
}

pub fn layout(input: &Input) -> Rects {
    let Input { screen, bands, .. } = *input;
    let class = WidthClass::of(screen.width());
    let left = screen.left() + bands.left + MARGIN;
    let right = screen.right() - bands.right - MARGIN;
    let bottom = screen.bottom() - bands.bottom;
    let parts = (class != WidthClass::Compact).then(|| bar_layout(input, class));
    let top = screen.top() + bands.top + 8.0;
    let column_top = match &parts {
        Some(parts) => parts.bar.bottom() + BAR_GAP,
        None => top + ROW + GAP,
    };

    let (workspace, actions, view, menu) = match &parts {
        Some(parts) => (
            parts.name,
            parts.actions,
            Some(parts.view),
            pos2(
                (parts.name.center().x - WORLDS_WIDTH / 2.0)
                    .min(right - WORLDS_WIDTH)
                    .max(left),
                parts.bar.bottom() + PAD,
            ),
        ),
        None => {
            let workspace = Rect::from_min_size(pos2(left, top), vec2(input.workspace_width, ROW));
            let actions = Rect::from_min_max(
                pos2(right - NARROW_ACTIONS_WIDTH, top),
                pos2(right, top + ROW),
            );
            (workspace, actions, None, pos2(left, top + ROW + PAD))
        }
    };
    let band = Rect::from_min_max(screen.min, pos2(screen.right(), screen.top() + bands.top));
    let bar = parts.as_ref().map_or(
        Rect::from_min_max(pos2(screen.left(), top), pos2(screen.right(), top + ROW)),
        |p| p.bar,
    );
    let extras = |rects: Rects| -> Rects {
        match &parts {
            Some(p) => Rects {
                band: p.band,
                bar: p.bar,
                back: p.back,
                undo: p.undo,
                redo: p.redo,
                canvas: p.canvas,
                dock: p.dock,
                dock_zone: p.dock_zone,
                dock_button: p.dock_button,
                tabs: p.tabs,
                fit: p.fit,
                free: p.free,
                column: p.column,
                column_content: p.column_content,
                slider: p.slider,
                ..rects
            },
            None => rects,
        }
    };

    let (sheet, sheet_travel, sheet_rise) = if class == WidthClass::Compact {
        let full = screen.bottom() - column_top;
        let height = SHEET_HEIGHT.min(full);
        let travel = (height - bands.bottom - ROW).max(0.0);
        let rise = full - height;
        let offset = input.sheet_offset.clamp(-rise, travel);
        let sheet_top = screen.bottom() - height + offset;
        let rect = Rect::from_min_max(pos2(screen.left(), sheet_top), screen.max);
        (input.panel.then_some(rect), travel, rise)
    } else {
        (None, 0.0, 0.0)
    };
    let row = class == WidthClass::Compact;
    let blank = Rect::NOTHING;
    if row {
        let rows = Rows {
            input,
            left,
            right,
            bottom,
            sheet,
            title_row: input.sheet_offset >= sheet_travel,
        };
        let (tools, tool_cells, brush_card, brush, card_cells, readout) = rows.layout();
        return Rects {
            class,
            screen,
            bands,
            workspace,
            view,
            actions,
            menu,
            band,
            bar,
            back: blank,
            undo: blank,
            redo: blank,
            canvas: screen,
            dock: None,
            dock_zone: actions,
            dock_button: None,
            tabs: blank,
            fit: screen,
            free: screen,
            column: tools,
            column_content: tools.height(),
            slider: 0.0,
            tools,
            tool_cells,
            brush_card,
            brush,
            card_cells,
            brush_panel: pos2(left, column_top),
            right_panel: pos2(left, column_top),
            panel_bottom: readout.map_or(bottom, |r| r.top()) - 8.0,
            one_panel: true,
            sheet,
            sheet_travel,
            sheet_rise,
            row,
            column_top,
            readout,
        };
    }

    // From 600 points, the tool column holds the tools and the controls of
    // the tool, and the Brush panel opens next to it.
    let parts = parts.as_ref().expect("a bar from 600 points");
    let tools = parts.column;
    let brush_card = if parts.slider > 0.0 {
        BrushCard::Full(parts.slider)
    } else {
        BrushCard::Button
    };
    let mut brush_panel = pos2(tools.right() + GAP, column_top);
    // A Brush panel that would cover the view control goes below it.
    let view = parts.view;
    if brush_panel.x + BRUSH_PANEL_WIDTH + GAP > view.left() && brush_panel.x < view.right() + GAP {
        brush_panel.y = brush_panel.y.max(view.bottom() + GAP);
    }
    let right_panel = parts.right_panel;
    let one_panel = brush_panel.x + BRUSH_PANEL_WIDTH + PANEL_GAP > right_panel.x;
    extras(Rects {
        class,
        screen,
        bands,
        workspace,
        view: Some(view),
        actions,
        menu,
        band,
        bar,
        back: blank,
        undo: blank,
        redo: blank,
        canvas: screen,
        dock: None,
        dock_zone: actions,
        dock_button: None,
        tabs: blank,
        fit: screen,
        free: screen,
        column: tools,
        column_content: tools.height(),
        slider: 0.0,
        tools,
        tool_cells: input.column.tools,
        brush_card,
        brush: None,
        card_cells: 0,
        brush_panel,
        right_panel,
        panel_bottom: bottom - 8.0,
        one_panel,
        sheet: None,
        sheet_travel,
        sheet_rise,
        row: false,
        column_top,
        readout: Some(parts.readout),
    })
}

/// The places of the row layout: the tool strip is a row above the home
/// indicator, the card of the tool is a row above it, and the readout is
/// above the card.
struct Rows<'a> {
    input: &'a Input,
    left: f32,
    right: f32,
    bottom: f32,
    sheet: Option<Rect>,
    /// The sheet shows its title row only.
    title_row: bool,
}

type RowPlaces = (Rect, usize, BrushCard, Option<Rect>, usize, Option<Rect>);

impl Rows<'_> {
    fn layout(&self) -> RowPlaces {
        let input = self.input;
        // A row can go as near the edges as `GAP`, so the six cells fit on
        // a screen 375 points wide.
        let room = self.right - self.left + 2.0 * (MARGIN - GAP);
        let fit = |cells: usize| {
            let most = ((room - 2.0 * PAD) / CELL.0).floor().max(1.0) as usize;
            cells.min(most)
        };
        let center = (self.left + self.right) / 2.0;
        let height = strip_height(1);
        // An open sheet covers the rows. A sheet at its title row has the
        // rows above it.
        let floor = match self.sheet {
            Some(sheet) if self.title_row => sheet.top() - ROW_GAP,
            Some(_) => {
                let none = Rect::from_center_size(pos2(center, self.bottom), vec2(0.0, 0.0));
                return (none, 0, BrushCard::Hidden, None, 0, None);
            }
            None => self.bottom - ROW_GAP,
        };
        let tool_cells = fit(input.tools);
        let width = strip_width(tool_cells);
        let tools = Rect::from_min_size(
            pos2(center - width / 2.0, floor - height),
            vec2(width, height),
        );
        // The card has the width of the strip. A card that is shorter is
        // at the right, so the Brush button is above the right end of the
        // strip.
        let card = |cells: usize| {
            let w = strip_width(cells);
            Rect::from_min_size(
                pos2(tools.right() - w, tools.top() - ROW_GAP - height),
                vec2(w, height),
            )
        };
        let button = 1 + input.extra;
        let slider = (width - strip_width(button)) / 2.0;
        let brush_card = if slider >= SLIDER_MIN {
            BrushCard::Full(slider)
        } else {
            BrushCard::Button
        };
        let (brush, card_cells) = if input.cells > 0 {
            let all = input.cells + input.extra;
            (Some(card(fit(all))), fit(all))
        } else if !input.strip && input.extra == 0 {
            (None, 0)
        } else if !input.strip {
            (Some(card(input.extra)), 0)
        } else {
            match brush_card {
                BrushCard::Full(_) => (Some(card(tool_cells)), 0),
                _ => (Some(card(button)), 0),
            }
        };
        let above = brush.unwrap_or(tools).top() - ROW_GAP;
        let readout = self.sheet.is_none().then(|| {
            Rect::from_min_max(
                pos2(self.left, above - READOUT_HEIGHT),
                pos2(self.right, above),
            )
        });
        (tools, tool_cells, brush_card, brush, card_cells, readout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(width: f32, height: f32) -> Input {
        Input {
            screen: Rect::from_min_size(Pos2::ZERO, vec2(width, height)),
            bands: Bands::MIN,
            workspace_width: 230.0,
            view_width: 140.0,
            panel: false,
            sheet_offset: 0.0,
            cells: 0,
            strip: true,
            extra: 0,
            tools: TOOLS,
            dock_width: 0.0,
            name_width: 120.0,
            column: ColumnInput {
                tools: TOOLS,
                cells: 0,
                sliders: 2,
                brush_button: true,
            },
        }
    }

    #[test]
    fn each_cell_of_the_tool_makes_the_column_longer() {
        let mut input = board(1194.0, 834.0);
        let without = layout(&input);
        input.column.cells = 1;
        let with = layout(&input);
        assert_eq!(with.column.top(), without.column.top());
        assert_eq!(with.column_content, without.column_content + COLUMN_CELL);
        assert_eq!(with.tools, with.column);
        assert_eq!(with.brush, None);
        // A tool with no controls has no divider.
        input.column = ColumnInput {
            tools: TOOLS,
            ..ColumnInput::default()
        };
        let r = layout(&input);
        assert_eq!(
            r.column_content,
            2.0 * COLUMN_INSET + TOOLS as f32 * COLUMN_CELL
        );
        input.column.cells = 1;
        let r = layout(&input);
        assert_eq!(
            r.column_content,
            2.0 * COLUMN_INSET + (TOOLS + 1) as f32 * COLUMN_CELL + COLUMN_DIVIDER
        );
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn the_width_classes_change_at_1000_and_at_600_points() {
        assert_eq!(WidthClass::of(1194.0), WidthClass::Wide);
        assert_eq!(WidthClass::of(1000.0), WidthClass::Wide);
        assert_eq!(WidthClass::of(999.0), WidthClass::Medium);
        assert_eq!(WidthClass::of(600.0), WidthClass::Medium);
        assert_eq!(WidthClass::of(599.0), WidthClass::Compact);
    }

    #[test]
    fn the_landscape_board_has_the_measured_places() {
        let r = layout(&board(1194.0, 834.0));
        assert_eq!(r.class, WidthClass::Wide);
        // The bar is below the band of the system and fills the width.
        assert_eq!(r.band, rect(0.0, 0.0, 1194.0, 24.0));
        assert_eq!(r.bar, rect(0.0, 24.0, 1194.0, BAR));
        assert_eq!(r.back, rect(4.0, 24.0, TOUCH, BAR));
        assert_eq!(r.tabs, rect(48.0, 24.0, 2.0 * TOUCH, BAR));
        assert_eq!(r.undo.left(), r.tabs.right() + DIVIDER);
        assert_eq!(r.redo.left(), r.undo.right());
        // The name of the world is at the center of the canvas.
        assert_eq!(r.workspace, rect(537.0, 24.0, 120.0, BAR));
        // The Panels button, a divider, and the Dock button.
        assert_eq!(r.actions, rect(1093.0, 24.0, 97.0, BAR));
        assert_eq!(r.dock_button, Some(rect(1146.0, 24.0, TOUCH, BAR)));
        assert_eq!(r.dock, None);
        assert_eq!(r.canvas, rect(0.0, 68.0, 1194.0, 766.0));
        assert_eq!(r.view, Some(rect(527.0, 76.0, 140.0, 52.0)));
        assert_eq!(
            r.fit,
            Rect::from_min_max(pos2(0.0, 136.0), pos2(1194.0, 814.0))
        );
        // The tool column is at the left edge, 8 points below the bar.
        assert_eq!(r.column, rect(0.0, 76.0, COLUMN, 661.0));
        assert_eq!(r.slider, TRACK);
        assert_eq!(r.column_top, 76.0);
        assert_eq!(r.readout, Some(rect(72.0, 758.0, 160.0, 48.0)));
        assert_eq!(r.right_panel, pos2(862.0, 76.0));
        assert_eq!(r.menu, pos2(597.0 - WORLDS_WIDTH / 2.0, 72.0));
        assert_eq!(r.panel_bottom, 806.0);
        assert_eq!(r.sheet, None);
    }

    #[test]
    fn the_name_and_the_view_control_share_the_center_of_the_canvas() {
        for width in [1194.0, 1366.0, 1024.0] {
            for dock in [0.0, 320.0, 480.0] {
                let mut input = board(width, 834.0);
                input.dock_width = dock;
                let r = layout(&input);
                let center = r.canvas.center().x;
                // A name that does not fit at the center moves to the right
                // of Redo.
                if center - r.redo.right() - PAD >= r.workspace.width() / 2.0 {
                    assert_eq!(r.workspace.center().x, center, "{width} {dock}");
                } else {
                    assert_eq!(r.workspace.left(), r.redo.right() + PAD, "{width} {dock}");
                }
                assert_eq!(r.view.unwrap().center().x, center, "{width} {dock}");
                assert_eq!(r.canvas.right(), width - dock, "{width} {dock}");
                assert_eq!(r.fit.right(), r.canvas.right(), "{width} {dock}");
                assert!(r.free.right() <= r.canvas.right(), "{width} {dock}");
            }
        }
    }

    #[test]
    fn the_open_dock_is_below_the_bar_and_its_zone_is_above_it() {
        let mut input = board(1194.0, 834.0);
        input.dock_width = 320.0;
        let r = layout(&input);
        let dock = r.dock.unwrap();
        assert_eq!(
            dock,
            Rect::from_min_max(pos2(874.0, 68.0), pos2(1194.0, 834.0))
        );
        assert_eq!(
            r.dock_zone,
            Rect::from_min_max(pos2(874.0, 24.0), pos2(1194.0, 68.0))
        );
        assert!(r.actions.left() >= dock.left());
        assert_eq!(r.right_panel.x, 874.0 - FLOAT_MARGIN - RIGHT_PANEL_WIDTH);
        // With the dock closed, the dock zone is the right end of the bar.
        input.dock_width = 0.0;
        let r = layout(&input);
        assert_eq!(r.dock_zone.left(), r.actions.left() - PAD);
        assert_eq!(r.dock_zone.right(), 1194.0);
    }

    #[test]
    fn the_globe_and_the_page_fit_below_the_view_control() {
        for (width, height) in [(1194.0, 834.0), (834.0, 1194.0), (1133.0, 744.0)] {
            let r = layout(&board(width, height));
            let view = r.view.unwrap();
            assert!(r.fit.top() >= view.bottom(), "{width}x{height}");
            assert!(r.free.top() >= view.bottom(), "{width}x{height}");
            assert!(r.free.left() >= r.column.right(), "{width}x{height}");
            assert_eq!(r.free.center().x, r.canvas.center().x, "{width}x{height}");
            assert_eq!(r.fit.bottom(), height - 20.0, "{width}x{height}");
        }
    }

    #[test]
    fn the_portrait_board_has_the_measured_places() {
        let r = layout(&board(834.0, 1194.0));
        assert_eq!(r.class, WidthClass::Medium);
        assert_eq!(r.view.unwrap().center().x, 417.0);
        // Below 1,000 points, the bar has no Dock button and no dock.
        assert_eq!(r.dock_button, None);
        assert_eq!(r.actions, rect(786.0, 24.0, TOUCH, BAR));
        let mut input = board(834.0, 1194.0);
        input.dock_width = 320.0;
        let r = layout(&input);
        assert_eq!(r.dock, None);
        assert_eq!(r.canvas.right(), 834.0);
        assert_eq!(r.workspace.center().x, 417.0);
        assert_eq!(r.right_panel, pos2(502.0, 76.0));
    }

    #[test]
    fn the_name_of_the_world_becomes_shorter_down_to_80_points() {
        let mut input = board(1000.0, 834.0);
        input.name_width = 1000.0;
        let r = layout(&input);
        assert!(r.workspace.width() < 1000.0);
        assert!(r.redo.right() < r.workspace.left());
        assert!(r.workspace.right() < r.actions.left());
        assert_eq!(r.workspace.center().x, r.canvas.center().x);
        // A short name keeps its width, and the name is never narrower than
        // 80 points.
        input.name_width = 40.0;
        assert_eq!(layout(&input).workspace.width(), NAME_MIN);
        for width in [600.0, 700.0, 999.0] {
            for dock in [0.0, 320.0] {
                let mut input = board(width, 834.0);
                input.name_width = 1000.0;
                input.dock_width = dock;
                let r = layout(&input);
                assert!(r.workspace.width() >= NAME_MIN, "{width}");
                assert!(r.redo.right() <= r.workspace.left(), "{width}");
                assert!(r.workspace.right() <= r.actions.left(), "{width}");
            }
        }
    }

    /// An iPhone 16 in portrait, with its safe area.
    fn phone() -> Input {
        let mut input = board(393.0, 852.0);
        input.bands = Bands {
            top: 59.0,
            bottom: 34.0,
            left: 0.0,
            right: 0.0,
        };
        input
    }

    #[test]
    fn the_iphone_portrait_board_has_the_measured_places() {
        let r = layout(&phone());
        assert_eq!(r.class, WidthClass::Compact);
        assert!(r.row);
        assert_eq!(r.view, None);
        assert_eq!(r.workspace.top(), 67.0);
        // The strip is a row above the home indicator.
        assert_eq!(r.tools, rect(24.5, 750.0, 344.0, 60.0));
        assert_eq!(r.tool_cells, TOOLS);
        // The two sliders and the Brush button are a row above the strip.
        assert_eq!(r.brush, Some(rect(24.5, 682.0, 344.0, 60.0)));
        assert_eq!(r.brush_card, BrushCard::Full(140.0));
        // The readout is above the slider row.
        let readout = r.readout.unwrap();
        assert_eq!((readout.top(), readout.bottom()), (642.0, 674.0));
        assert!(r.one_panel);
    }

    #[test]
    fn deselect_makes_the_sliders_of_the_row_shorter() {
        let mut input = phone();
        input.extra = 1;
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Full(112.0));
        assert_eq!(r.brush, Some(rect(24.5, 682.0, 344.0, 60.0)));
        // With a second extra cell, the sliders go to the Brush panel.
        input.extra = 2;
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Button);
        let card = r.brush.unwrap();
        assert_eq!(card.width(), strip_width(3));
        assert_eq!(card.right(), r.tools.right());
        // The Move tool has a row of Deselect alone.
        input.strip = false;
        input.extra = 1;
        let r = layout(&input);
        assert_eq!(r.brush.unwrap().width(), strip_width(1));
        input.extra = 0;
        let r = layout(&input);
        assert_eq!(r.brush, None);
        // With no card, the readout is above the strip.
        assert_eq!(r.readout.unwrap().bottom(), r.tools.top() - ROW_GAP);
    }

    #[test]
    fn a_card_of_cells_is_a_row_at_the_right_of_the_strip() {
        let mut input = phone();
        input.cells = MEASURE_CELLS;
        input.extra = 1;
        let r = layout(&input);
        assert_eq!(r.card_cells, MEASURE_CELLS + 1);
        let card = r.brush.unwrap();
        assert_eq!(card.size(), vec2(strip_width(4), 60.0));
        assert_eq!(card.right(), r.tools.right());
        assert_eq!(card.bottom(), r.tools.top() - ROW_GAP);
        // A card with more cells than the row holds scrolls.
        input.cells = 9;
        assert_eq!(layout(&input).card_cells, 6);
    }

    #[test]
    fn a_strip_with_more_cells_than_the_row_holds_scrolls() {
        let mut input = phone();
        input.tools = TOOLS + 1;
        let r = layout(&input);
        assert_eq!(r.tool_cells, TOOLS);
        assert_eq!(r.tools.width(), strip_width(TOOLS));
    }

    #[test]
    fn the_sheet_opens_at_half_height_and_has_a_full_height_and_a_title_row() {
        let mut input = phone();
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.sheet, Some(rect(0.0, 400.0, 393.0, 452.0)));
        // The open sheet covers the rows and the readout.
        assert_eq!((r.tool_cells, r.brush, r.readout), (0, None, None));
        // At the full height, the sheet is below the top row.
        assert_eq!(r.sheet_rise, 269.0);
        input.sheet_offset = -1000.0;
        let r = layout(&input);
        assert_eq!(r.sheet.unwrap().top(), r.column_top);
        assert_eq!(r.column_top, 131.0);
        // At its title row, the sheet has the rows above it.
        input.sheet_offset = 1000.0;
        let r = layout(&input);
        let sheet = r.sheet.unwrap();
        assert_eq!(sheet.top(), 852.0 - 34.0 - ROW);
        assert_eq!(r.tool_cells, TOOLS);
        assert_eq!(r.tools.bottom(), sheet.top() - ROW_GAP);
        assert_eq!(r.brush.unwrap().bottom(), r.tools.top() - ROW_GAP);
        assert_eq!(r.readout, None);
    }

    #[test]
    fn the_iphone_landscape_board_has_no_rows() {
        let mut input = phone();
        input.screen = Rect::from_min_size(Pos2::ZERO, vec2(852.0, 393.0));
        let r = layout(&input);
        assert_eq!(r.class, WidthClass::Medium);
        assert!(!r.row);
        assert_eq!(r.tools, r.column);
        assert_eq!(r.tools.width(), COLUMN);
    }

    #[test]
    fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
        let slider = |height: f32| layout(&board(1024.0, height)).slider;
        assert_eq!(slider(1194.0), TRACK);
        assert_eq!(slider(834.0), TRACK);
        assert_eq!(slider(744.0), 109.5);
        assert_eq!(slider(705.0), TRACK_MIN);
        // The Brush panel holds the sliders.
        assert_eq!(slider(704.0), 0.0);
        let r = layout(&board(1024.0, 704.0));
        assert_eq!(r.column.height(), r.column_content);
        // Then the column scrolls.
        let r = layout(&board(1024.0, 300.0));
        assert!(r.column_content > r.column.height());
        assert_eq!(r.column.bottom(), 300.0 - 20.0 - BAR_GAP);
    }

    #[test]
    fn the_raise_column_fits_the_small_boards() {
        for (width, height) in [(1133.0, 744.0), (744.0, 1133.0), (1024.0, 768.0)] {
            let r = layout(&board(width, height));
            assert!(r.slider >= TRACK_MIN, "{width}x{height}");
            assert_eq!(r.column.height(), r.column_content, "{width}x{height}");
            assert_eq!(r.column.width(), COLUMN, "{width}x{height}");
        }
    }

    #[test]
    fn the_bar_and_the_column_stay_out_of_the_safe_area() {
        let mut input = board(1194.0, 834.0);
        let safe = Rect::from_min_max(pos2(30.0, 40.0), pos2(1164.0, 800.0));
        input.bands = Bands::new(input.screen, safe);
        let r = layout(&input);
        assert_eq!(r.band, rect(0.0, 0.0, 1194.0, 40.0));
        assert_eq!(r.bar, rect(0.0, 40.0, 1194.0, BAR));
        assert_eq!(r.back.left(), 34.0);
        assert_eq!(r.actions.right(), 1160.0);
        assert_eq!(r.column.left(), 30.0);
        assert_eq!(r.readout.unwrap().bottom(), 792.0);
    }

    #[test]
    fn the_column_holds_the_cells_of_the_measure_tool() {
        let measure = |width: f32, height: f32| {
            let mut input = board(width, height);
            input.column = ColumnInput {
                tools: TOOLS,
                cells: MEASURE_CELLS,
                ..ColumnInput::default()
            };
            layout(&input)
        };
        for (width, height) in [(1194.0, 834.0), (1024.0, 700.0)] {
            let r = measure(width, height);
            assert_eq!(r.column.height(), r.column_content, "{width}x{height}");
            assert_eq!(r.brush, None, "{width}x{height}");
            assert_eq!(r.brush_panel.x, r.column.right() + GAP, "{width}x{height}");
        }
        // A short screen scrolls the column.
        let r = measure(1024.0, 400.0);
        assert!(r.column_content > r.column.height());
        assert_eq!(r.column.bottom(), 400.0 - 20.0 - BAR_GAP);
    }
}
