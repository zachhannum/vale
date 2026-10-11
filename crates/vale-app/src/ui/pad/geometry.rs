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
pub const TOOLS: usize = 7;
/// The height of one of the two sliders at the left edge.
pub const SLIDER: f32 = 140.0;
/// Below this height, a slider at the left edge is too short to use.
pub const SLIDER_MIN: f32 = 110.0;
pub const BRUSH_PANEL_WIDTH: f32 = 340.0;
pub const RIGHT_PANEL_WIDTH: f32 = 320.0;
pub const MENU_WIDTH: f32 = 240.0;
pub const SHEET_HEIGHT: f32 = 400.0;
pub const READOUT_HEIGHT: f32 = 32.0;
/// The width of the actions bar: four buttons and a divider.
pub const ACTIONS_WIDTH: f32 = 4.0 * TOUCH + 4.0 * PAD + 9.0 + 2.0 * PAD;

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
    /// The top-left corner of the workspace menu.
    pub menu: Pos2,
    pub tools: Rect,
    /// The number of tool cells that show. With fewer than `TOOLS`, the strip scrolls.
    pub tool_cells: usize,
    pub brush_card: BrushCard,
    /// The card of the two sliders and the Brush button.
    pub brush: Option<Rect>,
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
    let actions = Rect::from_min_max(pos2(right - ACTIONS_WIDTH, top), pos2(right, top + ROW));
    let view = (class != WidthClass::Compact).then(|| {
        let center = pos2(screen.center().x, top + ROW / 2.0);
        Rect::from_center_size(center, vec2(input.view_width, ROW))
    });

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

    let brush_panel = pos2(tools.right() + GAP, column_top);
    let right_panel = pos2(right - RIGHT_PANEL_WIDTH, column_top);
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
        menu: pos2(left, top + ROW + PAD),
        tools,
        tool_cells,
        brush_card,
        brush,
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
        assert_eq!(r.actions, rect(969.0, 32.0, 209.0, 52.0));
        assert_eq!(r.tools, rect(16.0, 96.0, 64.0, 372.0));
        assert_eq!(r.brush_card, BrushCard::Full(129.0));
        assert_eq!(r.brush, Some(rect(16.0, 480.0, 64.0, 318.0)));
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
        assert_eq!(r.actions, rect(609.0, 32.0, 209.0, 52.0));
        assert_eq!(r.right_panel, pos2(498.0, 96.0));
        assert_eq!(r.brush, Some(rect(16.0, 480.0, 64.0, 340.0)));
        assert!(!r.one_panel);
    }

    #[test]
    fn the_half_board_has_the_measured_places() {
        let r = layout(&board(597.0, 834.0));
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.view, None);
        assert_eq!(r.menu, pos2(16.0, 88.0));
        assert_eq!(r.brush_card, BrushCard::Full(114.0));
        assert_eq!(r.brush, Some(rect(16.0, 480.0, 64.0, 288.0)));
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
        // The strip scrolls above the sheet.
        assert_eq!(r.tools, rect(16.0, 96.0, 64.0, 216.0));
        assert_eq!(r.tool_cells, 4);
        assert_eq!(r.brush_card, BrushCard::Button);

        input.sheet_offset = 1000.0;
        let r = layout(&input);
        let sheet = r.sheet.unwrap();
        // The title row is above the bottom band.
        assert_eq!(sheet.top(), 834.0 - 20.0 - 52.0);
        assert_eq!((r.tool_cells, r.brush_card), (TOOLS, BrushCard::Button));
    }

    #[test]
    fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
        let card = |height: f32| layout(&board(1024.0, height)).brush_card;
        assert_eq!(card(1024.0), BrushCard::Full(140.0));
        assert_eq!(card(856.0), BrushCard::Full(140.0));
        assert_eq!(card(834.0), BrushCard::Full(129.0));
        assert_eq!(card(796.0), BrushCard::Full(110.0));
        assert_eq!(card(795.0), BrushCard::Button);
        assert_eq!(card(768.0), BrushCard::Button);
        assert_eq!(card(744.0), BrushCard::Button);
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
}
