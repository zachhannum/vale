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
pub const TOOLS: usize = 6;
/// The number of cells of the card of the Measure tool.
pub const MEASURE_CELLS: usize = 5;
/// The height of one of the two sliders at the left edge.
pub const SLIDER: f32 = 140.0;
/// Below this height, a slider at the left edge is too short to use.
pub const SLIDER_MIN: f32 = 110.0;
pub const BRUSH_PANEL_WIDTH: f32 = 340.0;
pub const RIGHT_PANEL_WIDTH: f32 = 320.0;
pub const MENU_WIDTH: f32 = 240.0;
/// The width of the menu of the worlds.
pub const WORLDS_WIDTH: f32 = 300.0;
/// The width of the menu of the More cell.
pub const MORE_WIDTH: f32 = 180.0;
pub const SHEET_HEIGHT: f32 = 400.0;
pub const READOUT_HEIGHT: f32 = 32.0;
/// The width of the actions bar: five buttons and a divider.
pub const ACTIONS_WIDTH: f32 = 5.0 * TOUCH + 5.0 * PAD + 9.0 + 2.0 * PAD;
/// The width of the actions bar with no Measure button.
const SHORT_ACTIONS_WIDTH: f32 = ACTIONS_WIDTH - TOUCH - PAD;

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
    /// The distance that the sheet is pulled down from its open place.
    pub sheet_offset: f32,
    /// The Measure tool is the tool.
    pub measure: bool,
    /// The layout has the Measure button.
    pub measure_button: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rects {
    pub class: WidthClass,
    pub screen: Rect,
    pub bands: Bands,
    pub workspace: Rect,
    /// `None`: the workspace menu holds the views.
    pub view: Option<Rect>,
    pub actions: Rect,
    /// The card of the Measure button, below the actions bar. `None`: the
    /// button is in the actions bar, or the layout has no Measure button.
    pub measure: Option<Rect>,
    /// The top-left corner of the workspace menu.
    pub menu: Pos2,
    pub tools: Rect,
    /// The number of tool cells that show. With fewer than `TOOLS`, the strip scrolls.
    pub tool_cells: usize,
    pub brush_card: BrushCard,
    /// The card of the two sliders and the Brush button, or the card of the
    /// Measure tool.
    pub brush: Option<Rect>,
    /// The number of cells of the card of the Measure tool that show. With
    /// fewer than `MEASURE_CELLS`, the card scrolls. Zero: the card is not
    /// the card of the Measure tool.
    pub measure_cells: usize,
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
    pub readout: Option<Rect>,
}

/// The height of a card that holds `cells` tool cells.
fn strip_height(cells: usize) -> f32 {
    cells as f32 * CELL.1 + 2.0 * PAD
}

pub fn layout(input: &Input) -> Rects {
    let Input { screen, bands, .. } = *input;
    let class = WidthClass::of(screen.width());
    let left = screen.left() + bands.left + MARGIN;
    let right = screen.right() - bands.right - MARGIN;
    let top = screen.top() + bands.top + 8.0;
    let bottom = screen.bottom() - bands.bottom;
    let column_top = top + ROW + GAP;

    let workspace = Rect::from_min_size(pos2(left, top), vec2(input.workspace_width, ROW));
    // The view switch is at the middle of the screen. With no room there, it
    // moves to the middle of the space between the workspace card and the
    // actions bar. The flag is true if the top row has room for the cards.
    let place_view = |actions_left: f32| {
        let (low, high) = (workspace.right() + GAP, actions_left - GAP);
        let width = input.view_width;
        if class == WidthClass::Compact {
            return (None, low <= high);
        }
        let fits = high - low >= width;
        let x = if fits {
            screen
                .center()
                .x
                .clamp(low + width / 2.0, high - width / 2.0)
        } else {
            (low + high) / 2.0
        };
        let rect = Rect::from_center_size(pos2(x, top + ROW / 2.0), vec2(width, ROW));
        (Some(rect), fits)
    };
    // With no room in the top row, the Measure button has a card below the
    // actions bar.
    let (actions_width, view, measure) = match place_view(right - ACTIONS_WIDTH) {
        (view, true) if input.measure_button => (ACTIONS_WIDTH, view, None),
        _ if input.measure_button => {
            let (view, _) = place_view(right - SHORT_ACTIONS_WIDTH);
            let card = Rect::from_min_size(pos2(right - ROW, column_top), vec2(ROW, ROW));
            (SHORT_ACTIONS_WIDTH, view, Some(card))
        }
        _ => {
            let view = (class != WidthClass::Compact).then(|| {
                let center = pos2(screen.center().x, top + ROW / 2.0);
                Rect::from_center_size(center, vec2(input.view_width, ROW))
            });
            (SHORT_ACTIONS_WIDTH, view, None)
        }
    };
    let actions = Rect::from_min_max(pos2(right - actions_width, top), pos2(right, top + ROW));

    let (sheet, sheet_travel) = if class == WidthClass::Compact {
        let height = SHEET_HEIGHT.min(screen.bottom() - column_top);
        let travel = (height - bands.bottom - ROW).max(0.0);
        let offset = input.sheet_offset.clamp(0.0, travel);
        let sheet_top = screen.bottom() - height + offset;
        let rect = Rect::from_min_max(pos2(screen.left(), sheet_top), screen.max);
        (input.panel.then_some(rect), travel)
    } else {
        (None, 0.0)
    };

    let readout = match class {
        _ if sheet.is_some() => None,
        WidthClass::Compact => Some(Rect::from_min_max(
            pos2(left, bottom - 6.0 - READOUT_HEIGHT),
            pos2(right, bottom - 6.0),
        )),
        _ => Some(Rect::from_min_max(
            pos2(left, bottom - 8.0 - READOUT_HEIGHT),
            pos2(right, bottom - 8.0),
        )),
    };

    // The left column ends above the thing that is below it.
    let limit = match (sheet, readout, class) {
        (Some(sheet), _, _) => sheet.top() - GAP,
        (None, Some(readout), WidthClass::Compact) => readout.top() - 8.0,
        _ => bottom - MARGIN,
    };
    let button = strip_height(1);
    let room = limit - column_top - strip_height(TOOLS) - GAP;
    let slider = ((room - button) / 2.0).min(SLIDER);
    let (tool_cells, brush_card) = if slider >= SLIDER_MIN {
        (TOOLS, BrushCard::Full(slider))
    } else if room >= button {
        (TOOLS, BrushCard::Button)
    } else if sheet.is_some() && room + GAP >= 0.0 {
        (TOOLS, BrushCard::Hidden)
    } else {
        let cells = ((limit - column_top - GAP - button - 2.0 * PAD) / CELL.1).floor();
        if cells >= 1.0 {
            (cells as usize, BrushCard::Button)
        } else {
            let cells = ((limit - column_top - 2.0 * PAD) / CELL.1).floor().max(1.0);
            (cells as usize, BrushCard::Hidden)
        }
    };
    let width = CELL.0 + 2.0 * PAD;
    let tools = Rect::from_min_size(
        pos2(left, column_top),
        vec2(width, strip_height(tool_cells)),
    );
    let brush_top = tools.bottom() + GAP;
    let brush = match brush_card {
        BrushCard::Full(slider) => Some(2.0 * slider + button),
        BrushCard::Button => Some(button),
        BrushCard::Hidden => None,
    }
    .map(|height| Rect::from_min_size(pos2(left, brush_top), vec2(width, height)));

    // The card of the Measure tool goes below the tool strip. With no room
    // there, it goes to the right of the strip.
    let mut brush_panel = pos2(tools.right() + GAP, column_top);
    let (brush, measure_cells) = match brush {
        Some(_) if input.measure && limit - brush_top >= strip_height(MEASURE_CELLS) => {
            let size = vec2(width, strip_height(MEASURE_CELLS));
            (
                Some(Rect::from_min_size(pos2(left, brush_top), size)),
                MEASURE_CELLS,
            )
        }
        Some(_) if input.measure => {
            let cells = ((limit - column_top - 2.0 * PAD) / CELL.1).floor();
            let cells = (cells.max(1.0) as usize).min(MEASURE_CELLS);
            let size = vec2(width, strip_height(cells));
            let card = Rect::from_min_size(brush_panel, size);
            brush_panel.x = card.right() + GAP;
            (Some(card), cells)
        }
        brush => (brush, 0),
    };

    let mut right_panel = pos2(right - RIGHT_PANEL_WIDTH, column_top);
    if let Some(measure) = measure
        && class != WidthClass::Compact
    {
        right_panel.y = measure.bottom() + GAP;
    }
    let one_panel = class == WidthClass::Compact
        || brush_panel.x + BRUSH_PANEL_WIDTH + PANEL_GAP > right_panel.x;
    let panel_bottom = readout.map_or(bottom, |r| r.top()) - 8.0;

    Rects {
        class,
        screen,
        bands,
        workspace,
        view,
        actions,
        measure,
        menu: pos2(left, top + ROW + PAD),
        tools,
        tool_cells,
        brush_card,
        brush,
        measure_cells,
        brush_panel,
        right_panel,
        panel_bottom,
        one_panel,
        sheet,
        sheet_travel,
        readout,
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
            measure: false,
            measure_button: true,
        }
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
        assert_eq!(r.workspace, rect(16.0, 32.0, 230.0, 52.0));
        assert_eq!(r.view, Some(rect(527.0, 32.0, 140.0, 52.0)));
        assert_eq!(r.actions, rect(921.0, 32.0, 257.0, 52.0));
        assert_eq!(r.tools, rect(16.0, 96.0, 64.0, 320.0));
        assert_eq!(r.brush_card, BrushCard::Full(140.0));
        assert_eq!(r.brush, Some(rect(16.0, 428.0, 64.0, 340.0)));
        assert_eq!(r.brush_panel, pos2(92.0, 96.0));
        assert_eq!(r.right_panel, pos2(858.0, 96.0));
        assert!(!r.one_panel);
        assert_eq!(r.sheet, None);
        let readout = r.readout.unwrap();
        assert_eq!((readout.top(), readout.bottom()), (774.0, 806.0));
    }

    #[test]
    fn the_portrait_board_has_the_measured_places() {
        let r = layout(&board(834.0, 1194.0));
        assert_eq!(r.class, WidthClass::Medium);
        assert_eq!(r.view.unwrap().center().x, 417.0);
        assert_eq!(r.actions, rect(561.0, 32.0, 257.0, 52.0));
        assert_eq!(r.right_panel, pos2(498.0, 96.0));
        assert_eq!(r.brush, Some(rect(16.0, 428.0, 64.0, 340.0)));
        assert!(!r.one_panel);
    }

    #[test]
    fn the_half_board_has_the_measured_places() {
        let r = layout(&board(597.0, 834.0));
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.view, None);
        assert_eq!(r.menu, pos2(16.0, 88.0));
        assert_eq!(r.brush, Some(rect(16.0, 428.0, 64.0, 340.0)));
        let readout = r.readout.unwrap();
        assert_eq!(
            (readout.left(), readout.top(), readout.bottom()),
            (16.0, 776.0, 808.0)
        );
        assert!(r.one_panel);
    }

    #[test]
    fn the_narrow_board_shows_a_panel_as_a_sheet() {
        let mut input = board(375.0, 834.0);
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.sheet, Some(rect(0.0, 434.0, 375.0, 400.0)));
        assert_eq!(r.readout, None);
        assert_eq!(r.tools, rect(16.0, 96.0, 64.0, 320.0));
        assert_eq!(r.brush_card, BrushCard::Hidden);

        input.sheet_offset = 1000.0;
        let r = layout(&input);
        let sheet = r.sheet.unwrap();
        // The title row is above the bottom band.
        assert_eq!(sheet.top(), 834.0 - 20.0 - 52.0);
        assert_eq!(r.brush_card, BrushCard::Full(131.0));
    }

    #[test]
    fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
        let card = |height: f32| layout(&board(1024.0, height)).brush_card;
        assert_eq!(card(834.0), BrushCard::Full(140.0));
        assert_eq!(card(804.0), BrushCard::Full(140.0));
        assert_eq!(card(768.0), BrushCard::Full(122.0));
        assert_eq!(card(744.0), BrushCard::Full(110.0));
        assert_eq!(card(743.0), BrushCard::Button);
        assert_eq!(card(600.0), BrushCard::Button);
    }

    #[test]
    fn a_very_short_window_scrolls_the_tool_strip() {
        let r = layout(&board(1024.0, 480.0));
        assert_eq!(r.brush_card, BrushCard::Button);
        assert!(r.tool_cells < TOOLS);
        assert!(r.brush.unwrap().bottom() <= 480.0 - 20.0 - MARGIN);
    }

    #[test]
    fn the_cards_stay_out_of_the_safe_area() {
        let mut input = board(1194.0, 834.0);
        let safe = Rect::from_min_max(pos2(30.0, 40.0), pos2(1164.0, 800.0));
        input.bands = Bands::new(input.screen, safe);
        let r = layout(&input);
        assert_eq!(r.workspace.min, pos2(46.0, 48.0));
        assert_eq!(r.actions.right(), 1148.0);
        assert_eq!(r.readout.unwrap().bottom(), 792.0);
    }

    #[test]
    fn the_card_of_the_measure_tool_holds_its_cells() {
        let measure = |width: f32, height: f32| {
            let mut input = board(width, height);
            input.measure = true;
            layout(&input)
        };
        // Below the tool strip.
        for (width, height) in [(1194.0, 834.0), (1024.0, 744.0), (375.0, 834.0)] {
            let r = measure(width, height);
            assert_eq!(r.measure_cells, MEASURE_CELLS, "{width}x{height}");
            let card = r.brush.unwrap();
            assert_eq!(card.left(), r.tools.left());
            assert_eq!(card.top(), r.tools.bottom() + GAP);
            assert_eq!(card.height(), strip_height(MEASURE_CELLS));
        }
        // At the right of the tool strip.
        let r = measure(1024.0, 700.0);
        assert_eq!(r.measure_cells, MEASURE_CELLS);
        let card = r.brush.unwrap();
        assert_eq!(card.min, pos2(r.tools.right() + GAP, r.tools.top()));
        assert_eq!(r.brush_panel.x, card.right() + GAP);
        // A short screen scrolls the card.
        let r = measure(852.0, 393.0);
        assert_eq!(r.measure_cells, 4);
        assert!(r.brush.unwrap().bottom() <= 393.0 - 20.0 - MARGIN);
        // The card is not the card of the Measure tool for another tool.
        assert_eq!(layout(&board(1194.0, 834.0)).measure_cells, 0);
    }

    #[test]
    fn the_measure_button_has_a_card_when_the_top_row_has_no_room() {
        // The iPad in landscape and in portrait has the button in the top row.
        for (width, height) in [(1194.0, 834.0), (834.0, 1194.0)] {
            let r = layout(&board(width, height));
            assert_eq!(r.measure, None);
            assert_eq!(r.actions.width(), ACTIONS_WIDTH);
            let view = r.view.unwrap();
            assert!(view.left() >= r.workspace.right() + GAP);
            assert!(view.right() + GAP <= r.actions.left());
        }
        // A wide view switch moves to the space between the cards.
        let mut input = board(834.0, 1194.0);
        input.workspace_width = 147.0;
        input.view_width = 340.0;
        let r = layout(&input);
        assert_eq!(r.measure, None);
        assert!(r.view.unwrap().right() + GAP <= r.actions.left());
        // An iPhone and a narrow split view have the card below the actions.
        for (width, height) in [(393.0, 852.0), (375.0, 834.0)] {
            let mut input = board(width, height);
            input.workspace_width = 94.0;
            let r = layout(&input);
            let card = r.measure.unwrap();
            assert_eq!(card.right(), r.actions.right());
            assert_eq!(card.top(), r.tools.top());
            assert!(r.workspace.right() + GAP <= r.actions.left());
        }
        // With no room for the view switch, the panels go below the card.
        let mut input = board(852.0, 393.0);
        input.view_width = 340.0;
        let r = layout(&input);
        let card = r.measure.unwrap();
        assert_eq!(r.right_panel.y, card.bottom() + GAP);
        // A layout with no Measure button has the short actions bar.
        input.measure_button = false;
        let r = layout(&input);
        assert_eq!((r.measure, r.actions.width()), (None, SHORT_ACTIONS_WIDTH));
    }
}
