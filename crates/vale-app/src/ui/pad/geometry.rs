//! The places of the cards of the iPad layout. All sizes are in points.

use eframe::egui::{CornerRadius, Pos2, Rect, pos2, vec2};

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
pub const CELL: (f32, f32) = (52.0, 52.0);
/// The cells of the tool strip of the globe: Sculpt, Terrain, Draw, Select,
/// Move, and Measure.
pub const TOOLS: usize = 6;
/// The number of cells of the card of the Measure tool.
pub const MEASURE_CELLS: usize = 3;
/// The number of cells of the card of the Vertex tool.
pub const VERTEX_CELLS: usize = 5;
/// The number of cells of the card of the Select tool.
pub const SELECT_CELLS: usize = 5;
/// Below this length, a slider of the row layout is too short to use.
pub const SLIDER_MIN: f32 = 110.0;
/// The space from the dock to the bottom band.
pub const DOCK_FOOT: f32 = 8.0;
pub const MENU_WIDTH: f32 = 240.0;
/// The width of the menu of the worlds.
pub const WORLDS_WIDTH: f32 = 300.0;
/// The width of the flyout of the brush modes.
pub const MODES_WIDTH: f32 = 200.0;
/// The height of an open sheet, about half the height of a phone.
pub const SHEET_HEIGHT: f32 = 452.0;
/// The space between two cards of the row layout.
pub const ROW_GAP: f32 = 8.0;
/// The height of the text of the readout: the place and the scale bar.
pub const READOUT_HEIGHT: f32 = 40.0;
/// The width of the tool column, without the band at the left edge.
pub const COLUMN: f32 = 56.0;
/// The space from the edge of the tool column to its cells.
pub const COLUMN_INSET: f32 = 6.0;
/// The side of a cell of the tool column.
pub const COLUMN_CELL: f32 = TOUCH;
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
/// The space from the tool column to the readout.
pub const READOUT_GAP: f32 = 16.0;
/// The width of the actions bar of the World workspace: Undo, Redo, a
/// divider, the Dock button, and the Panels button. A compact screen has
/// no Dock button.
pub const ACTIONS_WIDTH: f32 = 4.0 * TOUCH + 4.0 * PAD + 9.0 + 2.0 * PAD;
/// The width of a button in a row of buttons, with its space.
pub const BUTTON_STEP: f32 = TOUCH + PAD;
/// The space from the status bar to the top row.
pub const TOP_GAP: f32 = 4.0;
/// The width of the Panels menu, without the band at the right edge.
pub const PANELS_MENU_WIDTH: f32 = 360.0;

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
    /// 1,000 points or more. The globe and the page fit beside the dock.
    Wide,
    /// From 600 points. The dock floats over the canvas.
    Medium,
    /// Less than 600 points. The workspace switch is a menu button, and a
    /// panel is a sheet.
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

/// The contents of the card below the tool strip of the row layout, and
/// the sliders of the tool column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushCard {
    /// The two sliders, each with this length, and the Brush button. In
    /// the tool column, the length of the track of each slider.
    Full(f32),
    /// The Brush button alone. The Brush panel holds the two sliders.
    Button,
    /// An open sheet covers the place of the card.
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub screen: Rect,
    pub bands: Bands,
    /// The width of the workspace switch.
    pub workspace_width: f32,
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
    /// The tool strip and its card are rows at the bottom of a compact
    /// screen.
    pub row: bool,
    /// The contents of the tool column, outside the row layout.
    pub column: ColumnInput,
    /// The width of the dock that the user chose.
    pub dock_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rects {
    pub class: WidthClass,
    pub screen: Rect,
    pub bands: Bands,
    pub workspace: Rect,
    pub actions: Rect,
    /// The top-left corner of the workspace menu.
    pub menu: Pos2,
    /// The tool strip of the row layout, or else the tool column: the
    /// tools, a divider, and the controls of the tool.
    pub tools: Rect,
    /// The height of the contents of the tool column. If it is more than
    /// the height of `tools`, the column scrolls.
    pub column_content: f32,
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
    /// The left end of the space at the right of the tool strip and of
    /// its cards.
    pub column_right: f32,
    /// The room of the dock: from the top of the column to the bottom of
    /// the screen, at the right edge. In the compact width, the room of a
    /// card at the left.
    pub dock: Rect,
    /// A card at the left does not go below this line.
    pub panel_bottom: f32,
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
    /// The place of the readout at the lower left of the canvas: the row
    /// of the view buttons above the text of the place.
    pub readout: Option<Rect>,
}

/// The corners of a card that touches a side edge of the screen. Only the
/// inner corners are round.
pub fn edge_radius(rect: Rect, screen: Rect) -> CornerRadius {
    let r = crate::ui::pad::theme::CARD_RADIUS as u8;
    let left = if rect.left() <= screen.left() { 0 } else { r };
    let right = if rect.right() >= screen.right() { 0 } else { r };
    CornerRadius {
        nw: left,
        sw: left,
        ne: right,
        se: right,
    }
}

/// The part of a card that holds its controls: a card that touches a side
/// edge of the screen leaves out the band of the system at that edge.
pub fn edge_content(rect: Rect, screen: Rect, bands: Bands) -> Rect {
    let mut inner = rect;
    if rect.left() <= screen.left() {
        inner.min.x += bands.left;
    }
    if rect.right() >= screen.right() {
        inner.max.x -= bands.right;
    }
    inner
}

/// The height of a card that holds `cells` tool cells.
pub fn strip_height(cells: usize) -> f32 {
    cells as f32 * CELL.1 + 2.0 * PAD
}

/// The width of a row that holds `cells` tool cells.
pub fn strip_width(cells: usize) -> f32 {
    cells as f32 * CELL.0 + 2.0 * PAD
}

pub fn layout(input: &Input) -> Rects {
    let Input { screen, bands, .. } = *input;
    let class = WidthClass::of(screen.width());
    let left = screen.left() + bands.left + MARGIN;
    let right = screen.right() - bands.right - MARGIN;
    let top = screen.top() + bands.top + TOP_GAP;
    let bottom = screen.bottom() - bands.bottom;
    let column_top = top + ROW + GAP;

    // The cards at the ends of the top row and the tool column touch the
    // side edges of the screen.
    let edge = screen.left();
    let workspace = Rect::from_min_size(
        pos2(edge, top),
        vec2(bands.left + input.workspace_width, ROW),
    );
    let actions_width = match class {
        WidthClass::Compact => ACTIONS_WIDTH - BUTTON_STEP,
        _ => ACTIONS_WIDTH,
    };
    let actions = Rect::from_min_max(
        pos2(screen.right() - bands.right - actions_width, top),
        pos2(screen.right(), top + ROW),
    );

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
    let row = input.row && class == WidthClass::Compact;
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
            actions,
            menu: pos2(left, top + ROW + PAD),
            tools,
            column_content: tools.height(),
            tool_cells,
            brush_card,
            brush,
            card_cells,
            column_right: left,
            dock: Rect::from_min_max(pos2(left, column_top), pos2(right, bottom - DOCK_FOOT)),
            panel_bottom: readout.map_or(bottom, |r| r.top()) - 8.0,
            sheet,
            sheet_travel,
            sheet_rise,
            row,
            column_top,
            readout,
        };
    }

    // The tool column: the tools, a divider, the cells and the sliders of
    // the tool, and the Brush button. It ends above an open sheet.
    let limit = sheet.map_or(bottom - DOCK_FOOT, |sheet| sheet.top() - GAP);
    let room = limit - column_top;
    let c = input.column;
    let context = c.cells > 0 || c.sliders > 0 || c.brush_button;
    let fixed = 2.0 * COLUMN_INSET
        + COLUMN_CELL * (c.tools + c.cells + usize::from(c.brush_button)) as f32
        + if context { COLUMN_DIVIDER } else { 0.0 };
    // The sliders become shorter, and then the Brush panel holds them.
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
    let height = column_content
        .min(room)
        .max(COLUMN_CELL + 2.0 * COLUMN_INSET);
    let tools = Rect::from_min_size(pos2(edge, column_top), vec2(bands.left + COLUMN, height));
    let brush_card = if slider > 0.0 {
        BrushCard::Full(slider)
    } else {
        BrushCard::Button
    };
    let column_right = tools.right() + GAP;

    // The dock touches the right edge, and it leaves the left column free.
    let most = screen.right() - column_right;
    let width = (input.dock_width + bands.right).min(most);
    let dock = match class {
        WidthClass::Compact => Rect::from_min_max(
            pos2((right - input.dock_width).max(column_right), column_top),
            pos2(right, bottom - DOCK_FOOT),
        ),
        _ => Rect::from_min_max(
            pos2(screen.right() - width, column_top),
            pos2(screen.right(), bottom - DOCK_FOOT),
        ),
    };
    // The readout is at the lower left of the canvas, at the right of the
    // tool column, with the buttons above the text. An open sheet covers
    // it.
    let readout = sheet.is_none().then(|| {
        Rect::from_min_max(
            pos2(
                tools.right() + READOUT_GAP,
                bottom - DOCK_FOOT - ROW - READOUT_HEIGHT,
            ),
            pos2(right, bottom - DOCK_FOOT),
        )
    });

    Rects {
        class,
        screen,
        bands,
        workspace,
        actions,
        menu: pos2(left, top + ROW + PAD),
        tools,
        column_content,
        tool_cells: c.tools,
        brush_card,
        brush: None,
        card_cells: 0,
        column_right,
        dock,
        panel_bottom: bottom - DOCK_FOOT,
        sheet,
        sheet_travel,
        sheet_rise,
        row,
        column_top,
        readout,
    }
}

/// The places of the row layout: the tool strip is a row above the home
/// indicator, the card of the tool is a row above it, and the readout is
/// above the card, with the row of the view buttons above its text.
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
                pos2(self.left, above - ROW - READOUT_HEIGHT),
                pos2(self.right, above),
            )
        });
        (tools, tool_cells, brush_card, brush, card_cells, readout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tools of the globe with the two sliders of the brush.
    fn brush() -> ColumnInput {
        ColumnInput {
            tools: TOOLS,
            cells: 0,
            sliders: 2,
            brush_button: true,
        }
    }

    fn board(width: f32, height: f32) -> Input {
        Input {
            screen: Rect::from_min_size(Pos2::ZERO, vec2(width, height)),
            bands: Bands::MIN,
            workspace_width: 149.0,
            panel: false,
            sheet_offset: 0.0,
            cells: 0,
            strip: true,
            extra: 0,
            tools: TOOLS,
            row: false,
            column: brush(),
            dock_width: 340.0,
        }
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn each_cell_of_the_tool_makes_the_column_longer() {
        let mut input = board(1194.0, 834.0);
        let without = layout(&input);
        input.column.cells = 1;
        let with = layout(&input);
        assert_eq!(with.tools.top(), without.tools.top());
        assert_eq!(with.column_content, without.column_content + COLUMN_CELL);
        assert_eq!(with.brush, None);
        // A tool with no controls has no divider.
        input.column = ColumnInput {
            tools: TOOLS,
            ..ColumnInput::default()
        };
        let r = layout(&input);
        let tools = 2.0 * COLUMN_INSET + TOOLS as f32 * COLUMN_CELL;
        assert_eq!(r.column_content, tools);
        assert_eq!(r.tools.height(), tools);
        input.column.cells = 1;
        let r = layout(&input);
        assert_eq!(r.column_content, tools + COLUMN_CELL + COLUMN_DIVIDER);
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
        assert_eq!(r.workspace, rect(0.0, 28.0, 149.0, 52.0));
        assert_eq!(r.actions, rect(985.0, 28.0, 209.0, 52.0));
        // The tools, the divider, the two sliders, and the Brush button.
        let content = 12.0 + 7.0 * 44.0 + 13.0 + 2.0 * (TRACK + SLIDER_TEXT);
        assert_eq!(r.tools, rect(0.0, 92.0, COLUMN, content));
        assert_eq!(r.brush_card, BrushCard::Full(TRACK));
        assert_eq!(r.brush, None);
        assert_eq!(r.column_right, 68.0);
        assert_eq!(r.dock, rect(854.0, 92.0, 340.0, 714.0));
        assert_eq!(r.sheet, None);
        assert_eq!(r.readout, Some(rect(72.0, 714.0, 1106.0, 92.0)));
    }

    #[test]
    fn the_portrait_board_has_the_measured_places() {
        let r = layout(&board(834.0, 1194.0));
        assert_eq!(r.class, WidthClass::Medium);
        assert_eq!(r.actions, rect(625.0, 28.0, 209.0, 52.0));
        assert_eq!(r.dock.left(), 494.0);
        assert_eq!(r.tools.width(), COLUMN);
        assert_eq!(r.brush_card, BrushCard::Full(TRACK));
    }

    #[test]
    fn the_half_board_has_the_measured_places() {
        let r = layout(&board(597.0, 834.0));
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.menu, pos2(16.0, 84.0));
        assert_eq!(r.tools.min, pos2(0.0, 92.0));
        let readout = r.readout.unwrap();
        assert_eq!((readout.left(), readout.bottom()), (72.0, 806.0));
    }

    #[test]
    fn the_narrow_board_shows_a_panel_as_a_sheet() {
        let mut input = board(375.0, 834.0);
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.sheet, Some(rect(0.0, 382.0, 375.0, 452.0)));
        assert_eq!(r.readout, None);
        // The column ends above the open sheet, and it scrolls.
        assert_eq!(r.tools.bottom(), 382.0 - GAP);
        assert!(r.column_content > r.tools.height());
        assert_eq!(r.brush_card, BrushCard::Button);

        input.sheet_offset = 1000.0;
        let r = layout(&input);
        let sheet = r.sheet.unwrap();
        // The title row is above the bottom band.
        assert_eq!(sheet.top(), 834.0 - 20.0 - 52.0);
        assert_eq!(r.brush_card, BrushCard::Full(118.5));
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
        input.row = true;
        input
    }

    #[test]
    fn the_iphone_portrait_board_has_the_measured_places() {
        let r = layout(&phone());
        assert_eq!(r.class, WidthClass::Compact);
        assert!(r.row);
        assert_eq!(r.workspace.top(), 63.0);
        // The strip is a row above the home indicator.
        assert_eq!(r.tools, rect(36.5, 750.0, 320.0, 60.0));
        assert_eq!(r.tool_cells, TOOLS);
        // The two sliders and the Brush button are a row above the strip.
        assert_eq!(r.brush, Some(rect(36.5, 682.0, 320.0, 60.0)));
        assert_eq!(r.brush_card, BrushCard::Full(130.0));
        // The readout is above the slider row: the view buttons, and the
        // text below them.
        let readout = r.readout.unwrap();
        assert_eq!((readout.top(), readout.bottom()), (582.0, 674.0));
        assert_eq!(readout.height(), ROW + READOUT_HEIGHT);
    }

    #[test]
    fn deselect_moves_the_sliders_of_the_row_to_the_brush_panel() {
        let mut input = phone();
        input.extra = 1;
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Button);
        let card = r.brush.unwrap();
        assert_eq!(card.width(), strip_width(2));
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
        assert_eq!(r.sheet_rise, 273.0);
        input.sheet_offset = -1000.0;
        let r = layout(&input);
        assert_eq!(r.sheet.unwrap().top(), r.column_top);
        assert_eq!(r.column_top, 127.0);
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
        assert_eq!(r.tools.width(), COLUMN);
    }

    #[test]
    fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
        let slider = |height: f32| match layout(&board(1024.0, height)).brush_card {
            BrushCard::Full(track) => track,
            _ => 0.0,
        };
        assert_eq!(slider(1194.0), TRACK);
        assert_eq!(slider(834.0), TRACK);
        assert_eq!(slider(744.0), 101.5);
        assert_eq!(slider(721.0), TRACK_MIN);
        // The Brush panel holds the sliders.
        assert_eq!(slider(720.0), 0.0);
        let r = layout(&board(1024.0, 720.0));
        assert_eq!(r.tools.height(), r.column_content);
        // Then the column scrolls.
        let r = layout(&board(1024.0, 300.0));
        assert!(r.column_content > r.tools.height());
        assert_eq!(r.tools.bottom(), 300.0 - 20.0 - DOCK_FOOT);
    }

    #[test]
    fn the_small_boards_have_the_sliders_in_the_column() {
        for (width, height) in [(1133.0, 744.0), (744.0, 1133.0), (1024.0, 768.0)] {
            let r = layout(&board(width, height));
            let BrushCard::Full(track) = r.brush_card else {
                panic!("{width}x{height}: no sliders");
            };
            assert!(track >= TRACK_MIN, "{width}x{height}");
            assert_eq!(r.tools.height(), r.column_content, "{width}x{height}");
        }
    }

    #[test]
    fn the_cards_stay_out_of_the_safe_area() {
        let mut input = board(1194.0, 834.0);
        let safe = Rect::from_min_max(pos2(30.0, 40.0), pos2(1164.0, 800.0));
        input.bands = Bands::new(input.screen, safe);
        let r = layout(&input);
        // The cards at the edges reach the edges, and their controls stay
        // out of the bands.
        assert_eq!(r.workspace.min, pos2(0.0, 44.0));
        assert_eq!(r.actions.right(), 1194.0);
        let content = |rect| edge_content(rect, r.screen, r.bands);
        assert_eq!(content(r.workspace).left(), 30.0);
        assert_eq!(content(r.actions).right(), 1164.0);
        assert_eq!(content(r.tools).left(), 30.0);
        assert_eq!(content(r.tools).width(), COLUMN);
        assert_eq!(r.readout.unwrap().bottom(), 792.0);
        assert_eq!(r.readout.unwrap().left(), 30.0 + COLUMN + READOUT_GAP);
    }

    #[test]
    fn the_chrome_touches_the_side_edges_and_sits_4_points_below_the_status_bar() {
        for (width, height) in [(1194.0, 834.0), (834.0, 1194.0), (597.0, 834.0)] {
            let r = layout(&board(width, height));
            let size = format!("{width}x{height}");
            assert_eq!(r.workspace.top(), Bands::MIN.top + TOP_GAP, "{size}");
            assert_eq!(r.actions.top(), r.workspace.top(), "{size}");
            assert_eq!(r.workspace.left(), 0.0, "{size}");
            assert_eq!(r.actions.right(), width, "{size}");
            assert_eq!(r.tools.left(), 0.0, "{size}");
            // Only the inner corners are round.
            let left = edge_radius(r.tools, r.screen);
            assert_eq!((left.nw, left.sw), (0, 0), "{size}");
            assert!(left.ne > 0 && left.se > 0, "{size}");
            let right = edge_radius(r.actions, r.screen);
            assert_eq!((right.ne, right.se), (0, 0), "{size}");
            assert!(right.nw > 0 && right.sw > 0, "{size}");
        }
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
            assert_eq!(r.tools.height(), r.column_content, "{width}x{height}");
            assert_eq!(r.brush, None, "{width}x{height}");
            assert_eq!(r.column_right, r.tools.right() + GAP, "{width}x{height}");
        }
        // A short screen scrolls the column.
        let r = measure(1024.0, 400.0);
        assert!(r.column_content > r.tools.height());
        assert_eq!(r.tools.bottom(), 400.0 - 20.0 - DOCK_FOOT);
    }
}
