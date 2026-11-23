//! The places of the cards of the iPad layout. All sizes are in points.

use eframe::egui::{self, CornerRadius, Pos2, Rangef, Rect, pos2, vec2};

use crate::reserved::Regions;

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
/// A screen lower than this opens the sheet at half its height, such as
/// Split View on the inner screen of iPhone Duo.
pub const SHORT_SCREEN: f32 = 834.0;
/// The space between two cards of the row layout.
pub const ROW_GAP: f32 = 8.0;
/// The height of the card of the place above the readout, with its gap.
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
/// Below this length, the sliders lose their value and their name, and
/// then the Brush panel holds them.
pub const TRACK_MIN: f32 = 90.0;
/// The space of a vertical slider of the tool column with no value and no
/// name, above and below its track.
pub const SLIDER_GAP: f32 = 8.0;
/// The space from the tool column to the readout.
pub const READOUT_GAP: f32 = 16.0;
/// The width of the actions bar of the World workspace: Undo, Redo, a
/// divider, the Dock button, and the Panels button. A compact screen has
/// no Dock button.
pub const ACTIONS_WIDTH: f32 = 4.0 * TOUCH + 4.0 * PAD + 9.0 + 2.0 * PAD;
/// The width of a button in a row of buttons, with its space.
pub const BUTTON_STEP: f32 = TOUCH + PAD;
/// The width of a divider between two groups of buttons, with its space.
pub const DIVIDER_WIDTH: f32 = 9.0 + PAD;
/// The space from the status bar to the top row.
pub const TOP_GAP: f32 = 4.0;
/// The width of the Panels menu, without the band at the right edge.
pub const PANELS_MENU_WIDTH: f32 = 360.0;
/// The space from a card to the folding region.
pub const FOLD_GAP: f32 = 8.0;

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

    /// The bands of the screen of `ctx`, with its reserved regions.
    pub fn of(ctx: &egui::Context) -> Bands {
        let screen = ctx.viewport_rect();
        Bands::new(screen, ctx.content_rect()).free_camera_side(screen, regions(ctx).camera)
    }

    /// A side band that holds a camera at a top corner is free, and the
    /// layout keeps out of the camera region alone. The outer screen of
    /// iPhone Duo has such a band, the column of the status bar.
    pub fn free_camera_side(mut self, screen: Rect, camera: Option<Rect>) -> Bands {
        let Some(camera) = camera else {
            return self;
        };
        if camera.bottom() > screen.top() + self.top + TOP_GAP + ROW {
            return self;
        }
        let x = camera.center().x;
        if self.right > 0.0 && x >= screen.right() - self.right {
            self.right = 0.0;
        }
        if self.left > 0.0 && x <= screen.left() + self.left {
            self.left = 0.0;
        }
        self
    }
}

fn regions_id() -> egui::Id {
    egui::Id::new("pad-reserved-regions")
}

/// Keeps the reserved regions for the code of this frame.
pub fn set_regions(ctx: &egui::Context, regions: Regions) {
    ctx.data_mut(|d| d.insert_temp(regions_id(), regions));
}

/// The reserved regions of this frame.
pub fn regions(ctx: &egui::Context) -> Regions {
    ctx.data(|d| d.get_temp(regions_id())).unwrap_or_default()
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
    /// The number of these cells that are above the sliders.
    pub lead: usize,
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
    /// In the tool column, the sliders with no value and no name, with
    /// tracks of this length.
    Bare(f32),
    /// The Brush button alone. The Brush panel holds the two sliders.
    Button,
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
    /// The region that the system reserves around a camera at the top of
    /// the screen.
    pub camera: Option<Rect>,
    /// The folding region of a partially folded device.
    pub fold: Option<Rect>,
    /// The cards are glass. On a compact screen, the cards of the top row
    /// and the sheet then float clear of the edges of the screen.
    pub glass: bool,
    /// A field has the keyboard of the system. On a compact screen, the
    /// keyboard covers the rows, and the sheet reaches the bottom.
    pub keyboard: bool,
    /// On a compact screen, the actions card holds the View button before
    /// Undo.
    pub top_view: bool,
}

/// The space at the top of a glass sheet that holds its grabber, above the
/// title row.
pub const GRABBER_SPACE: f32 = 10.0;

/// The space between the highlight of a cell, or the track of a slider,
/// and the edge of the floating card of the rows, on each side. It is also
/// the space between the highlights of the two rows. The radius of the
/// card is the radius of the highlight plus this space.
pub const BAR_PAD: f32 = 8.0;
/// The space between a cell and its highlight.
pub const CELL_INSET: f32 = 2.0;
/// The space between the edge of the floating card and the cells inside.
pub const BAR_CELL_INSET: f32 = BAR_PAD - CELL_INSET;

/// The side of a cell of the rows of a compact screen: an icon alone, as
/// in the tool column.
pub const ROW_CELL: f32 = COLUMN_CELL;

/// The height of a row of a compact screen: a cell with `PAD` above and
/// below.
pub fn row_height() -> f32 {
    ROW_CELL + 2.0 * PAD
}

/// The columns of the rows of a compact screen. The center of the first
/// column is on the line of the first control of the sheet header, and the
/// center of the last column is on the line of its Close button. A cell of
/// the control row is above a cell of the tool strip, and a slider spans
/// whole columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The left edge of the highlight of the first cell and of the track
    /// of a slider: `BAR_PAD` inside the card.
    pub line: f32,
    /// The distance between the centers of two columns.
    pub pitch: f32,
    pub columns: usize,
    /// The columns are segments of the width, from `line`, each with a
    /// capsule as wide as the segment less a gap.
    pub segment: bool,
}

impl Grid {
    /// The columns of a row from `left` to `right`, for `columns` cells.
    pub fn new(left: f32, right: f32, columns: usize) -> Grid {
        let line = left + BAR_PAD;
        let highlight = ROW_CELL - 2.0 * CELL_INSET;
        let span = right - BAR_PAD - highlight - line;
        let pitch = match columns {
            0 | 1 => 0.0,
            n => span / (n - 1) as f32,
        };
        Grid {
            line,
            pitch,
            columns,
            segment: false,
        }
    }

    /// The center of column `i`.
    pub fn center(&self, i: usize) -> f32 {
        if self.segment {
            return self.line + (i as f32 + 0.5) * self.pitch;
        }
        self.line + ROW_CELL / 2.0 - CELL_INSET + i as f32 * self.pitch
    }

    /// The left edge of a cell of `ROW_CELL` width in column `i`. In a
    /// segment, the left edge of the segment.
    pub fn cell_left(&self, i: usize) -> f32 {
        if self.segment {
            return self.line + i as f32 * self.pitch;
        }
        self.center(i) - ROW_CELL / 2.0
    }

    /// The width that a slider over `k` columns takes in a row, with
    /// `CELL_INSET` at each side of its track.
    pub fn span_width(&self, k: usize) -> f32 {
        (k.max(1) - 1) as f32 * self.pitch + ROW_CELL
    }

    /// The space between two cells in a row. In segments, the gap between
    /// two capsules.
    pub fn spacing(&self) -> f32 {
        if self.segment {
            return SEGMENT_GAP;
        }
        self.pitch - ROW_CELL
    }

    /// The width of a cell of the row: `ROW_CELL`, or a segment less its
    /// gap.
    pub fn cell_width(&self) -> f32 {
        if self.segment {
            return self.pitch - SEGMENT_GAP;
        }
        ROW_CELL
    }
}

/// The gap between two segments of a control row of segments.
pub const SEGMENT_GAP: f32 = 4.0;

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
    /// The columns of the rows of a compact screen.
    pub grid: Option<Grid>,
    /// The columns of the control row. A row with a slider has the columns
    /// of the strip. A row of cells only, with fewer cells than the strip,
    /// has segments of the width.
    pub row_grid: Option<Grid>,
    /// The columns of the first slider and of each later slider of the
    /// control row.
    pub slider_spans: [usize; 2],
    /// The sheet of a compact screen, at its present place.
    pub sheet: Option<Rect>,
    /// The footer of the sheet of a compact screen: the rows of the tools,
    /// from the line above them to the bottom of the card.
    pub footer: Option<Rect>,
    /// The floating card of the rows of a compact screen with no sheet. A
    /// sheet grows up from it, and the rows keep their place.
    pub bar: Option<Rect>,
    /// The largest `sheet_offset`. At this offset, the sheet shows its title row only.
    pub sheet_travel: f32,
    /// The smallest `sheet_offset` is minus this distance. At that offset,
    /// the sheet has the full height.
    pub sheet_rise: f32,
    /// At this offset, the sheet is the floating card of the rows. A sheet
    /// that opens rises from it.
    pub sheet_closed: f32,
    /// The tool strip and its card are rows at the bottom.
    pub row: bool,
    /// The top of the space below the top row.
    pub column_top: f32,
    /// The place of the readout at the lower left of the canvas: the card
    /// of the place above the card of the View button and the scale bar.
    pub readout: Option<Rect>,
    /// The tool column has controls below the tools.
    pub column_context: bool,
    /// The folding region of a partially folded device.
    pub fold: Option<Rect>,
    /// The band of the tool column that holds no control, at a fold that
    /// crosses the column: from the bottom of the last control above the
    /// fold to `COLUMN_INSET` below the end of the card at the fold. A
    /// control that would be in it is below it.
    pub column_gap: Option<Rangef>,
}

/// True if the fold is a horizontal band: the hinge of the inner screen of
/// iPhone Duo in portrait.
pub fn horizontal(fold: Rect) -> bool {
    fold.width() > fold.height()
}

/// The part of `room` at the side of the fold that holds `anchor`, with
/// `FOLD_GAP` from the fold.
pub fn fold_part(anchor: Rect, fold: Option<Rect>, room: Rect) -> Rect {
    let Some(fold) = fold.filter(|f| f.intersects(room)) else {
        return room;
    };
    let mut part = room;
    if horizontal(fold) {
        if anchor.center().y < fold.center().y {
            part.max.y = part.max.y.min(fold.top() - FOLD_GAP);
        } else {
            part.min.y = part.min.y.max(fold.bottom() + FOLD_GAP);
        }
    } else if anchor.center().x < fold.center().x {
        part.max.x = part.max.x.min(fold.left() - FOLD_GAP);
    } else {
        part.min.x = part.min.x.max(fold.right() + FOLD_GAP);
    }
    part
}

/// Moves a card that crosses the fold to the side of the fold that holds
/// `anchor`, its control, inside `room`. A card that opens past its
/// control keeps that edge, so it does not cover the control. A card that
/// does not fit becomes smaller.
pub fn off_fold(rect: Rect, anchor: Rect, fold: Option<Rect>, room: Rect) -> Rect {
    let Some(f) = fold.filter(|f| f.intersects(rect)) else {
        return rect;
    };
    let part = fold_part(anchor, Some(f), room);
    let (r, a, p) = match horizontal(f) {
        true => (rect.y_range(), anchor.y_range(), part.y_range()),
        false => (rect.x_range(), anchor.x_range(), part.x_range()),
    };
    let (min, max) = if r.min >= a.max && r.min + TOUCH <= p.max {
        // The card opens after its control.
        let min = r.min.max(p.min);
        (min, r.max.min(p.max))
    } else if r.max <= a.min && r.max - TOUCH >= p.min {
        // The card opens before its control.
        let max = r.max.min(p.max);
        (r.min.max(p.min), max)
    } else {
        let min = r.min.min(p.max - r.span()).max(p.min);
        (min, (min + r.span()).min(p.max))
    };
    match horizontal(f) {
        true => Rect::from_x_y_ranges(rect.x_range(), min..=max),
        false => Rect::from_x_y_ranges(min..=max, rect.y_range()),
    }
}

/// The part of the canvas for a card at its center: the left part in
/// landscape and the upper part in portrait.
pub fn canvas_part(screen: Rect, fold: Option<Rect>) -> Rect {
    fold_part(
        Rect::from_min_size(screen.min, vec2(1.0, 1.0)),
        fold,
        screen,
    )
}

/// The height of the contents of the tool column from `top`, with sliders
/// `slider` high, and the gap of the column. A control that would be in `gap` moves below it, and so
/// does a divider just above it. The gap of the column starts at the
/// bottom of the last control above it.
fn column_height(
    top: f32,
    c: &ColumnInput,
    slider: f32,
    gap: Option<Rangef>,
) -> (f32, Option<Rangef>) {
    let mut y = top + COLUMN_INSET;
    let mut start = None;
    let mut divider = None;
    let mut add = |height: f32, is_divider: bool| {
        let at = skip_gap(y, height, gap);
        if at > y && start.is_none() {
            start = Some(divider.unwrap_or(y));
            y = match divider {
                Some(_) => at + COLUMN_DIVIDER,
                None => at,
            };
        }
        divider = is_divider.then_some(y);
        y += height;
    };
    let cell = |n: usize, add: &mut dyn FnMut(f32, bool)| {
        (0..n).for_each(|_| add(COLUMN_CELL, false));
    };
    cell(c.tools, &mut add);
    if c.cells > 0 || c.sliders > 0 || c.brush_button {
        add(COLUMN_DIVIDER, true);
    }
    let lead = c.lead.min(c.cells);
    cell(lead, &mut add);
    if slider > 0.0 {
        (0..c.sliders).for_each(|_| add(slider, false));
    }
    cell(c.cells - lead + usize::from(c.brush_button), &mut add);
    let gap = gap.zip(start).map(|(g, start)| Rangef::new(start, g.max));
    (y + COLUMN_INSET - top, gap)
}

/// The top of a control `height` high that would start at `y`: below the
/// gap if it would be in the gap.
pub fn skip_gap(y: f32, height: f32, gap: Option<Rangef>) -> f32 {
    match gap {
        Some(g) if y < g.max && y + height > g.min => g.max,
        _ => y,
    }
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
/// The space between the edge of the actions card and its first and last
/// button. On a compact screen, the last button is on the last column of
/// the rows and on the line of the Close button of the sheet.
pub fn actions_inset(class: WidthClass) -> f32 {
    match class {
        WidthClass::Compact => BAR_CELL_INSET,
        _ => PAD,
    }
}

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
    // side edges of the screen. On a compact screen with glass, the cards
    // of the top row float at the margin.
    let float = input.glass && class == WidthClass::Compact;
    let compact = 2.0 * (actions_inset(class) - PAD);
    let actions_width = match class {
        WidthClass::Compact if input.top_view => ACTIONS_WIDTH + DIVIDER_WIDTH + compact,
        WidthClass::Compact => ACTIONS_WIDTH - BUTTON_STEP + compact,
        _ => ACTIONS_WIDTH,
    };
    let (workspace, mut actions) = if float {
        (
            Rect::from_min_size(pos2(left, top), vec2(input.workspace_width, ROW)),
            Rect::from_min_size(pos2(right - actions_width, top), vec2(actions_width, ROW)),
        )
    } else {
        (
            Rect::from_min_size(
                pos2(screen.left(), top),
                vec2(bands.left + input.workspace_width, ROW),
            ),
            Rect::from_min_max(
                pos2(screen.right() - bands.right - actions_width, top),
                pos2(screen.right(), top + ROW),
            ),
        )
    };
    let mut workspace = workspace;
    // The card of the top row at the corner of the camera moves along the
    // row, out of the region of the camera.
    let row_band = Rect::from_x_y_ranges(screen.x_range(), top..=top + ROW);
    if let Some(camera) = input.camera.filter(|c| c.intersects(row_band)) {
        if camera.center().x > screen.center().x {
            let right = camera.left() - ROW_GAP;
            actions =
                Rect::from_min_size(pos2(right - actions_width, top), vec2(actions_width, ROW));
        } else {
            let left = camera.right() + ROW_GAP;
            workspace = Rect::from_min_size(pos2(left, top), vec2(input.workspace_width, ROW));
        }
    }

    let row = input.row && class == WidthClass::Compact;
    if row {
        let rows = Rows {
            input,
            left,
            right,
            bottom,
        };
        let places = rows.layout();
        let (tools, mut tool_cells, brush_card, mut brush, mut card_cells, readout) = places.rects;
        let first = brush.unwrap_or(tools);
        let card_bottom = bottom;
        // The floating card of the rows: `BAR_PAD` around the highlights
        // of the cells, so a highlight is concentric with the corners of
        // the card.
        let bar = Rect::from_min_max(
            pos2(left, first.top() + PAD - BAR_CELL_INSET),
            pos2(right, card_bottom),
        );
        // In a sheet, the line above the rows is at the top of the card.
        let mut footer = Some(bar);
        let mut bar = Some(bar);
        let mut card_bottom = card_bottom;
        // The keyboard of the system covers the rows. A sheet then reaches
        // the bottom of the screen, with no footer.
        if input.keyboard && input.panel {
            (tool_cells, brush, card_cells) = (0, None, 0);
            (footer, bar) = (None, None);
            card_bottom = screen.bottom();
        }
        // The sheet grows up from the card. It opens at about half the
        // height of the screen, rises to below the top row, and its title
        // row sits on the footer.
        let top_full = column_top;
        let top_half = match screen.height() < SHORT_SCREEN {
            true => screen.bottom() - screen.height() / 2.0,
            false => screen.bottom() - SHEET_HEIGHT,
        }
        .max(top_full);
        let grabber = if input.glass { GRABBER_SPACE } else { 0.0 };
        let top_title = footer.map_or(bottom, |f| f.top()) - grabber - ROW;
        let sheet_rise = top_half - top_full;
        let sheet_travel = (top_title - top_half).max(0.0);
        let sheet_closed = bar.map_or(sheet_travel, |b| b.top() - top_half);
        let offset = input.sheet_offset.clamp(-sheet_rise, sheet_closed);
        let sheet = input
            .panel
            .then(|| Rect::from_min_max(pos2(left, top_half + offset), pos2(right, card_bottom)));
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
            panel_bottom: bar.map_or(bottom, |b| b.top()) - 8.0,
            grid: Some(places.grid),
            row_grid: Some(places.row_grid),
            slider_spans: places.spans,
            sheet,
            footer,
            bar,
            sheet_travel,
            sheet_rise,
            sheet_closed,
            row,
            column_top,
            readout,
            column_context: false,
            fold: input.fold,
            column_gap: None,
        };
    }
    // Outside the row layout, a compact screen has the sheet at the bottom
    // edge.
    let (sheet, sheet_travel, sheet_rise) = if class == WidthClass::Compact {
        let full = screen.bottom() - column_top;
        let open = if screen.height() < SHORT_SCREEN {
            screen.height() / 2.0
        } else {
            SHEET_HEIGHT
        };
        let height = open.min(full);
        let grabber = if input.glass { GRABBER_SPACE } else { 0.0 };
        let travel = (height - grabber - ROW - bands.bottom).max(0.0);
        let rise = full - height;
        let offset = input.sheet_offset.clamp(-rise, travel);
        let sheet_top = screen.bottom() - height + offset;
        let rect = Rect::from_min_max(pos2(screen.left(), sheet_top), screen.max);
        (input.panel.then_some(rect), travel, rise)
    } else {
        (None, 0.0, 0.0)
    };

    // The tool column: the tools, a divider, the cells and the sliders of
    // the tool, and the Brush button. It ends above an open sheet.
    let limit = sheet.map_or(bottom - DOCK_FOOT, |sheet| sheet.top() - GAP);
    let room = limit - column_top;
    let c = input.column;
    let context = c.cells > 0 || c.sliders > 0 || c.brush_button;
    let fixed = 2.0 * COLUMN_INSET
        + COLUMN_CELL * (c.tools + c.cells + usize::from(c.brush_button)) as f32
        + if context { COLUMN_DIVIDER } else { 0.0 };
    // A horizontal fold makes a gap in the column, with `FOLD_GAP` and the
    // inset of the column at each side of the fold.
    let gap = input.fold.filter(|f| horizontal(*f)).map(|f| {
        let space = FOLD_GAP + COLUMN_INSET;
        Rangef::new(f.top() - space, f.bottom() + space)
    });
    let fits = |slider: f32| column_height(column_top, &c, slider, gap).0 <= room;
    // The length of the tracks of sliders with `space` above and below
    // each track. Zero: the sliders do not fit.
    let track = |space: f32| {
        let mut track = ((room - fixed) / c.sliders as f32 - space).min(TRACK);
        while gap.is_some() && track >= TRACK_MIN && !fits(track + space) {
            track -= 1.0;
        }
        if track >= TRACK_MIN { track } else { 0.0 }
    };
    // The sliders become shorter, then they lose their value and their
    // name, and then the Brush panel holds them.
    let (slider, space) = if c.sliders == 0 {
        (0.0, 0.0)
    } else if track(SLIDER_TEXT) > 0.0 {
        (track(SLIDER_TEXT), SLIDER_TEXT)
    } else {
        (track(SLIDER_GAP), SLIDER_GAP)
    };
    let extent = if slider > 0.0 { slider + space } else { 0.0 };
    let plain = fixed + c.sliders as f32 * extent;
    let (mut column_content, mut column_gap) = match gap {
        Some(_) => column_height(column_top, &c, extent, gap),
        None => (plain, None),
    };
    let mut room = room;
    // A column that scrolls ends above the fold.
    if column_content > room
        && column_gap.is_some()
        && let Some(g) = gap
    {
        room = room.min(g.min + COLUMN_INSET - column_top);
        column_content = plain;
        column_gap = None;
    }
    let height = column_content
        .min(room)
        .max(COLUMN_CELL + 2.0 * COLUMN_INSET);
    let tools = Rect::from_min_size(
        pos2(screen.left(), column_top),
        vec2(bands.left + COLUMN, height),
    );
    let brush_card = if slider == 0.0 {
        BrushCard::Button
    } else if space == SLIDER_TEXT {
        BrushCard::Full(slider)
    } else {
        BrushCard::Bare(slider)
    };
    let column_right = tools.right() + GAP;

    // The dock touches the right edge, and it leaves the left column free.
    // In landscape, it is in the part at the right of the fold.
    let mut most = screen.right() - column_right;
    if let Some(f) = input.fold.filter(|f| !horizontal(*f)) {
        most = most.min(screen.right() - f.right() - FOLD_GAP);
    }
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
    // At a horizontal fold, the dock is above the fold, near the top row.
    let mut dock = dock;
    if let Some(f) = input
        .fold
        .filter(|f| horizontal(*f) && f.top() < dock.bottom())
    {
        dock.max.y = (f.top() - FOLD_GAP).max(dock.top() + ROW);
    }
    // The readout is at the lower left of the canvas, at the right of the
    // tool column. An open sheet covers it.
    let readout = sheet.is_none().then(|| {
        Rect::from_min_max(
            pos2(
                tools.right() + READOUT_GAP,
                bottom - DOCK_FOOT - ROW - READOUT_HEIGHT,
            ),
            pos2(right, bottom - DOCK_FOOT),
        )
    });
    // In landscape, the readout is in the part at the left of the fold.
    let readout = readout.map(|r| match input.fold.filter(|f| !horizontal(*f)) {
        Some(f) => Rect::from_min_max(r.min, pos2(r.right().min(f.left() - FOLD_GAP), r.bottom())),
        None => r,
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
        grid: None,
        row_grid: None,
        slider_spans: [0, 0],
        sheet,
        footer: None,
        bar: None,
        sheet_travel,
        sheet_rise,
        sheet_closed: sheet_travel,
        row,
        column_top,
        readout,
        column_context: context,
        fold: input.fold,
        column_gap,
    }
}

/// The cells of the control row, on the columns of the strip: the fixed
/// cells and the columns of each slider. With no room for two columns for
/// each slider, the Brush panel holds the sliders.
struct RowCells {
    /// The cells that are not sliders.
    fixed: usize,
    /// The sliders have their columns in the row.
    sliders: bool,
    spans: [usize; 2],
}

fn row_cells(input: &Input, columns: usize) -> RowCells {
    let c = input.column;
    let fixed = c.cells + usize::from(c.brush_button);
    let free = columns.saturating_sub(fixed);
    if c.sliders > 0 && free >= 2 * c.sliders {
        let (base, extra) = (free / c.sliders, free % c.sliders);
        RowCells {
            fixed,
            sliders: true,
            spans: [
                base + usize::from(extra >= 1),
                base + usize::from(extra >= 2),
            ],
        }
    } else {
        RowCells {
            fixed,
            sliders: false,
            spans: [0, 0],
        }
    }
}

/// True if the row layout has a control row above the tool strip.
fn has_control_row(input: &Input) -> bool {
    let c = input.column;
    c.cells > 0 || c.sliders > 0 || c.brush_button
}

/// The places of the row layout: the tool strip is a row above the home
/// indicator, the card of the tool is a row above it, and the readout is
/// above the card, with the row of the view buttons above its text. With a
/// sheet, the rows are the footer of the sheet.
struct Rows<'a> {
    input: &'a Input,
    left: f32,
    right: f32,
    bottom: f32,
}

type RowRects = (Rect, usize, BrushCard, Option<Rect>, usize, Option<Rect>);

struct RowPlaces {
    rects: RowRects,
    grid: Grid,
    row_grid: Grid,
    spans: [usize; 2],
}

impl Rows<'_> {
    fn layout(&self) -> RowPlaces {
        let input = self.input;
        // The rows span the width inside the margins, as the readout and
        // the cards of the top row do. The cells share the columns of the
        // header of the sheet, and a column is at least `TOUCH` wide.
        let width = self.right - self.left;
        let highlight = ROW_CELL - 2.0 * CELL_INSET;
        let span = width - 2.0 * BAR_PAD - highlight;
        let most = ((span / TOUCH).floor() as usize + 1).max(1);
        let tool_cells = input.tools.min(most);
        let grid = Grid::new(self.left, self.right, tool_cells);
        let height = row_height();
        // The cells of the strip end `BAR_CELL_INSET` above the bottom of
        // the floating card, which sits on the bottom band.
        let floor = self.bottom + PAD - BAR_CELL_INSET;
        let tools = Rect::from_min_size(pos2(self.left, floor - height), vec2(width, height));
        let cells = row_cells(input, tool_cells);
        let brush_card = if cells.sliders {
            BrushCard::Full(grid.pitch)
        } else {
            BrushCard::Button
        };
        // The control row spans the width. Its highlights are `BAR_PAD`
        // above the highlights of the strip. A row of cells only, with
        // fewer cells than the strip, has segments of the width.
        let top = tools.top() + 2.0 * PAD - (BAR_PAD - 2.0 * CELL_INSET) - height;
        let card = Rect::from_min_size(pos2(self.left, top), vec2(width, height));
        let short = !cells.sliders && cells.fixed > 0 && cells.fixed < tool_cells;
        let row_grid = match short {
            false => grid,
            true => Grid {
                line: grid.line,
                pitch: (width - 2.0 * BAR_PAD) / cells.fixed as f32,
                columns: cells.fixed,
                segment: true,
            },
        };
        let (brush, card_cells) = if input.cells > 0 {
            let all = input.cells + input.extra;
            (Some(card), all.min(tool_cells))
        } else if !has_control_row(input) {
            (None, 0)
        } else {
            (Some(card), 0)
        };
        // A compact screen has no readout card: the scale is an overlay
        // below the top row, and the View button is in the top row.
        let readout = None;
        RowPlaces {
            rects: (tools, tool_cells, brush_card, brush, card_cells, readout),
            grid,
            row_grid,
            spans: cells.spans,
        }
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
            lead: 0,
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
            camera: None,
            fold: None,
            glass: false,
            keyboard: false,
            top_view: false,
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
        // The strip is inside the margins. Its cells are icons alone, and
        // they end `BAR_CELL_INSET` above the bottom band, at the bottom of
        // the floating card.
        assert_eq!(r.tools, rect(16.0, 764.0, 361.0, 52.0));
        assert_eq!(r.tool_cells, TOOLS);
        // The cells are on six columns. The first column is on the line
        // of the first button of a card, and the last on the line of the
        // last button.
        let grid = r.grid.unwrap();
        assert_eq!(grid.line, MARGIN + BAR_PAD);
        assert_eq!(grid.center(0), 44.0);
        assert_eq!(grid.center(5), 393.0 - 44.0);
        assert_eq!(grid.pitch, 61.0);
        // The two sliders and the Brush button are a row above the strip.
        // The sliders span three columns and two columns.
        assert_eq!(r.brush, Some(rect(16.0, 716.0, 361.0, 52.0)));
        assert_eq!(r.brush_card, BrushCard::Full(61.0));
        assert_eq!(r.slider_spans, [3, 2]);
        assert_eq!(r.row_grid, r.grid);
        // The rows are in one floating card, `BAR_PAD` around the
        // highlights of the cells, on the bottom band. In a sheet, the
        // line above the rows is at the top of the card. A compact screen
        // has no readout card.
        assert_eq!(r.bar, Some(rect(16.0, 714.0, 361.0, 104.0)));
        assert_eq!(r.footer, r.bar);
        assert_eq!(r.panel_bottom, 706.0);
        assert_eq!(r.readout, None);
        // With the View button before Undo, the top row is one button and
        // one divider wider. Its buttons are `BAR_CELL_INSET` inside the
        // card, so the last button is on the last column.
        let mut input = phone();
        input.top_view = true;
        input.glass = true;
        let r = layout(&input);
        let inset = actions_inset(WidthClass::Compact);
        assert_eq!(inset, BAR_CELL_INSET);
        let width = ACTIONS_WIDTH + DIVIDER_WIDTH + 2.0 * (inset - PAD);
        assert_eq!(r.actions.width(), width);
        assert_eq!(r.actions.right(), 377.0);
        assert_eq!(r.actions.right() - inset - TOUCH / 2.0, grid.center(5));
        // A tablet keeps `PAD` at the ends of the card.
        assert_eq!(actions_inset(WidthClass::Medium), PAD);
    }

    #[test]
    fn the_rows_of_a_phone_stay_in_place_when_a_sheet_opens() {
        let mut input = phone();
        let bar = layout(&input);
        input.panel = true;
        let travel = layout(&input).sheet_travel;
        for offset in [0.0, -1000.0, travel, 1000.0] {
            input.sheet_offset = offset;
            let r = layout(&input);
            assert_eq!(r.tools, bar.tools);
            assert_eq!(r.brush, bar.brush);
            assert_eq!(r.footer, bar.footer);
            assert_eq!(r.bar, bar.bar);
            assert_eq!(r.grid, bar.grid);
            let sheet = r.sheet.unwrap();
            assert_eq!((sheet.left(), sheet.right()), (16.0, 377.0));
            assert_eq!(sheet.bottom(), bar.bar.unwrap().bottom());
        }
        // At its title row, the sheet is its title row and the footer. At
        // the offset `sheet_closed`, it is the floating card.
        input.sheet_offset = travel;
        let r = layout(&input);
        assert_eq!(r.sheet.unwrap().top(), 714.0 - ROW);
        input.sheet_offset = r.sheet_closed;
        assert_eq!(layout(&input).sheet, bar.bar);
    }

    #[test]
    fn the_keyboard_hides_the_rows_and_the_sheet_reaches_the_bottom() {
        let mut input = phone();
        input.panel = true;
        input.keyboard = true;
        let r = layout(&input);
        assert_eq!((r.tool_cells, r.brush, r.card_cells), (0, None, 0));
        assert_eq!((r.bar, r.footer), (None, None));
        let sheet = r.sheet.unwrap();
        assert_eq!((sheet.left(), sheet.right()), (16.0, 377.0));
        assert_eq!(sheet.bottom(), 852.0);
        // With no sheet, the keyboard does not hide the rows.
        input.panel = false;
        let r = layout(&input);
        assert_eq!(r.tool_cells, TOOLS);
        assert!(r.bar.is_some());
    }

    #[test]
    fn deselect_moves_the_sliders_of_the_row_to_the_brush_panel() {
        let mut input = phone();
        // With Deselect, each slider spans two columns.
        input.column.cells = 1;
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Full(61.0));
        assert_eq!(r.slider_spans, [2, 2]);
        assert_eq!(r.row_grid, r.grid);
        // With two more cells, the sliders have no two columns each, and
        // the Brush panel holds them. The four cells are segments of the
        // width.
        input.column.cells = 3;
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Button);
        let row = r.row_grid.unwrap();
        assert!(row.segment);
        assert_eq!((row.columns, row.pitch), (4, 345.0 / 4.0));
        assert_eq!(
            (row.cell_left(0), row.cell_left(3) + row.cell_width()),
            (24.0, 365.0)
        );
        let card = r.brush.unwrap();
        assert_eq!(
            (card.left(), card.right()),
            (r.tools.left(), r.tools.right())
        );
        // The Move tool has a row of Deselect alone, in the middle.
        input.strip = false;
        input.column = ColumnInput {
            tools: TOOLS,
            cells: 1,
            lead: 0,
            sliders: 0,
            brush_button: false,
        };
        let r = layout(&input);
        let row = r.row_grid.unwrap();
        assert_eq!(
            (row.columns, row.center(0), row.cell_width()),
            (1, 196.5, 341.0)
        );
        input.column.cells = 0;
        let r = layout(&input);
        assert_eq!(r.brush, None);
        // With no card, the bar is the strip alone, `BAR_CELL_INSET` around
        // its cells.
        assert_eq!(r.bar.unwrap().top(), r.tools.top() + PAD - BAR_CELL_INSET);
    }

    #[test]
    fn a_card_of_cells_is_a_row_at_the_right_of_the_strip() {
        let mut input = phone();
        input.cells = MEASURE_CELLS;
        input.extra = 1;
        input.column = ColumnInput {
            tools: TOOLS,
            cells: MEASURE_CELLS + 1,
            lead: 0,
            sliders: 0,
            brush_button: false,
        };
        let r = layout(&input);
        assert_eq!(r.card_cells, MEASURE_CELLS + 1);
        let row = r.row_grid.unwrap();
        assert_eq!((row.segment, row.columns), (true, 4));
        let card = r.brush.unwrap();
        assert_eq!(card.left(), r.tools.left());
        assert_eq!(card.right(), r.tools.right());
        assert_eq!(
            card.bottom(),
            r.tools.top() + 2.0 * PAD - (BAR_PAD - 2.0 * CELL_INSET)
        );
        // A card with more cells than the row holds scrolls.
        input.cells = 9;
        input.column.cells = 10;
        let r = layout(&input);
        assert_eq!(r.card_cells, 6);
        assert_eq!(r.row_grid, r.grid);
    }

    #[test]
    fn a_short_screen_opens_the_sheet_at_half_the_room_above_the_rows() {
        // Split View on the inner screen of iPhone Duo in landscape.
        let mut input = board(475.0, 669.0);
        input.bands.bottom = 34.0;
        input.row = true;
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.sheet, Some(rect(16.0, 334.5, 443.0, 300.5)));
    }

    #[test]
    fn a_strip_with_more_cells_than_the_row_holds_scrolls() {
        let mut input = phone();
        // Seven cells fit on the columns, and the columns are narrower.
        input.tools = TOOLS + 1;
        let r = layout(&input);
        assert_eq!(r.tool_cells, TOOLS + 1);
        assert_eq!(r.grid.unwrap().pitch, 305.0 / 6.0);
        // A column is at least `TOUCH` wide, so nine cells scroll.
        input.tools = 9;
        let r = layout(&input);
        assert_eq!(r.tool_cells, 7);
        assert_eq!(r.tools.width(), 361.0);
    }

    #[test]
    fn on_a_phone_with_glass_the_top_row_and_the_sheet_float_clear_of_the_edges() {
        let mut input = phone();
        input.panel = true;
        input.glass = true;
        let r = layout(&input);
        assert_eq!(r.workspace.left(), MARGIN);
        assert_eq!(r.actions.right(), 393.0 - MARGIN);
        // The sheet is the floating card of the rows, grown up.
        assert_eq!(r.sheet, Some(rect(16.0, 400.0, 361.0, 418.0)));
        // At its title row, the grabber, the title row, and the footer are
        // above the bottom band.
        input.sheet_offset = layout(&input).sheet_travel;
        let r = layout(&input);
        let low = r.sheet.unwrap();
        let footer = r.footer.unwrap();
        assert_eq!(low.top(), footer.top() - GRABBER_SPACE - ROW);
        // A tablet keeps the cards at the edges.
        let mut tablet = board(1194.0, 834.0);
        tablet.glass = true;
        let r = layout(&tablet);
        assert_eq!((r.workspace.left(), r.actions.right()), (0.0, 1194.0));
    }

    #[test]
    fn the_sheet_opens_at_half_height_and_has_a_full_height_and_a_title_row() {
        let mut input = phone();
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.sheet, Some(rect(16.0, 400.0, 361.0, 418.0)));
        // The rows are the footer of the sheet, and the sheet covers the
        // readout. The footer starts a gap above the control row.
        assert_eq!((r.tool_cells, r.readout), (TOOLS, None));
        assert_eq!(r.tools.bottom(), 852.0 - 34.0 + PAD - BAR_CELL_INSET);
        // The highlights of the two rows are `BAR_PAD` apart.
        let brush = r.brush.unwrap();
        assert_eq!(
            brush.bottom() - PAD - CELL_INSET + BAR_PAD,
            r.tools.top() + PAD + CELL_INSET
        );
        // The line of the footer is at the top of the floating card.
        let footer = r.footer.unwrap();
        assert_eq!(footer.top(), brush.top() + PAD - BAR_CELL_INSET);
        assert_eq!(footer.bottom(), 852.0 - 34.0);
        // At the full height, the sheet is below the top row.
        assert_eq!(r.sheet_rise, 273.0);
        input.sheet_offset = -1000.0;
        let r = layout(&input);
        assert_eq!(r.sheet.unwrap().top(), r.column_top);
        assert_eq!(r.column_top, 127.0);
        // At its title row, the sheet is its title row and its footer.
        input.sheet_offset = layout(&input).sheet_travel;
        let r = layout(&input);
        let sheet = r.sheet.unwrap();
        assert_eq!(sheet.top(), r.footer.unwrap().top() - ROW);
        assert_eq!(r.tool_cells, TOOLS);
        assert_eq!(r.tools.bottom(), 852.0 - 34.0 + PAD - BAR_CELL_INSET);
        assert_eq!(r.readout, None);
        // A tool with no controls has the strip alone in the footer, and
        // the strip does not move.
        input.column = ColumnInput {
            tools: TOOLS,
            ..Default::default()
        };
        let r = layout(&input);
        assert_eq!(r.brush, None);
        assert_eq!(r.tools.bottom(), 852.0 - 34.0 + PAD - BAR_CELL_INSET);
        assert_eq!(
            r.footer.unwrap().top(),
            r.tools.top() + PAD - BAR_CELL_INSET
        );
        assert_eq!(r.sheet.unwrap().bottom(), 852.0 - 34.0);
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
    fn a_short_window_makes_the_sliders_short_then_bare_and_then_leaves_them_out() {
        let card = |height: f32| layout(&board(1024.0, height)).brush_card;
        assert_eq!(card(1194.0), BrushCard::Full(TRACK));
        assert_eq!(card(834.0), BrushCard::Full(TRACK));
        assert_eq!(card(744.0), BrushCard::Full(101.5));
        assert_eq!(card(721.0), BrushCard::Full(TRACK_MIN));
        // The sliders lose their value and their name, and the tracks are
        // longer.
        assert_eq!(card(720.0), BrushCard::Bare(TRACK));
        assert_eq!(card(690.0), BrushCard::Bare(110.5));
        assert_eq!(card(649.0), BrushCard::Bare(TRACK_MIN));
        let r = layout(&board(1024.0, 649.0));
        assert_eq!(r.tools.height(), r.column_content);
        assert_eq!(r.tools.bottom(), 649.0 - 20.0 - DOCK_FOOT);
        // The Brush panel holds the sliders.
        assert_eq!(card(648.0), BrushCard::Button);
        let r = layout(&board(1024.0, 648.0));
        assert_eq!(r.tools.height(), r.column_content);
        // Then the column scrolls.
        let r = layout(&board(1024.0, 300.0));
        assert!(r.column_content > r.tools.height());
        assert_eq!(r.tools.bottom(), 300.0 - 20.0 - DOCK_FOOT);
    }

    #[test]
    fn the_inner_screen_of_iphone_duo_in_landscape_has_bare_sliders() {
        let mut input = board(951.0, 669.0);
        input.bands.bottom = 34.0;
        input.fold = Some(rect(461.0, 0.0, 28.0, 669.0));
        let r = layout(&input);
        assert_eq!(r.brush_card, BrushCard::Bare(93.0));
        assert_eq!(r.tools.height(), r.column_content);
        assert_eq!(r.tools.bottom(), 669.0 - 34.0 - DOCK_FOOT);
        // In portrait, the sliders have their labels.
        let mut input = board(669.0, 951.0);
        input.bands.bottom = 34.0;
        input.fold = Some(rect(0.0, 461.0, 669.0, 28.0));
        assert_eq!(layout(&input).brush_card, BrushCard::Full(TRACK));
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

    /// The outer screen of iPhone Duo, with a camera region of 30 points
    /// at the top right in portrait and at the top left in landscape.
    fn duo_outer(portrait: bool) -> Input {
        let (width, height) = if portrait {
            (466.0, 678.0)
        } else {
            (678.0, 466.0)
        };
        let mut input = board(width, height);
        input.bands.bottom = 34.0;
        input.row = portrait;
        let x = if portrait { width - 57.0 } else { 27.0 };
        input.camera = Some(rect(x, 9.0, 30.0, 30.0));
        input
    }

    #[test]
    fn the_card_at_the_corner_of_the_camera_moves_out_of_its_region() {
        // Portrait: the card of the actions ends at the left of the region.
        let input = duo_outer(true);
        let camera = input.camera.unwrap();
        let mut without = input;
        without.camera = None;
        let (r, before) = (layout(&input), layout(&without));
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.actions.right(), camera.left() - ROW_GAP);
        let inset = 2.0 * (BAR_CELL_INSET - PAD);
        assert_eq!(
            r.actions.size(),
            vec2(ACTIONS_WIDTH - BUTTON_STEP + inset, ROW)
        );
        assert_eq!(r.actions.top(), before.actions.top());
        assert!(!r.actions.intersects(camera));
        assert_eq!(
            Rects {
                actions: before.actions,
                ..r
            },
            before
        );

        // Landscape: the card of the world menu starts at the right of the
        // region.
        let input = duo_outer(false);
        let camera = input.camera.unwrap();
        let mut without = input;
        without.camera = None;
        let (r, before) = (layout(&input), layout(&without));
        assert_eq!(r.class, WidthClass::Medium);
        assert_eq!(r.workspace.left(), camera.right() + ROW_GAP);
        assert_eq!(r.workspace.size(), vec2(149.0, ROW));
        assert!(!r.workspace.intersects(camera));
        let moved = Rects {
            workspace: before.workspace,
            ..r
        };
        assert_eq!(moved, before);
    }

    #[test]
    fn a_camera_below_the_top_row_moves_no_card() {
        let mut input = duo_outer(true);
        let mut without = input;
        without.camera = None;
        input.camera = Some(rect(400.0, 300.0, 30.0, 30.0));
        assert_eq!(layout(&input), layout(&without));
    }

    #[test]
    fn the_outer_screen_of_iphone_duo_opens_the_sheet_at_half_the_room_above_the_rows() {
        let mut input = duo_outer(true);
        input.panel = true;
        let r = layout(&input);
        assert_eq!(r.sheet, Some(rect(16.0, 339.0, 434.0, 305.0)));
    }

    #[test]
    fn a_side_band_with_a_camera_at_a_top_corner_is_free() {
        let screen = rect(0.0, 0.0, 466.0, 678.0);
        let bands = Bands {
            top: 24.0,
            bottom: 34.0,
            left: 0.0,
            right: 84.0,
        };
        // The outer screen of iPhone Duo in portrait.
        let camera = rect(409.0, 9.0, 30.0, 30.0);
        let free = bands.free_camera_side(screen, Some(camera));
        assert_eq!(
            free,
            Bands {
                right: 0.0,
                ..bands
            }
        );
        assert_eq!(bands.free_camera_side(screen, None), bands);
        // The Dynamic Island of an iPhone in landscape is at the middle of
        // the side band, and the band stays.
        let screen = rect(0.0, 0.0, 852.0, 393.0);
        let bands = Bands {
            left: 59.0,
            right: 59.0,
            ..Bands::MIN
        };
        let island = rect(10.0, 140.0, 37.0, 113.0);
        assert_eq!(bands.free_camera_side(screen, Some(island)), bands);
    }

    /// The inner screen of iPhone Duo, with a folding region of 28 points
    /// along the hinge.
    fn duo_folded(portrait: bool) -> Input {
        let (width, height) = if portrait {
            (669.0, 951.0)
        } else {
            (951.0, 669.0)
        };
        let mut input = board(width, height);
        input.bands.bottom = 34.0;
        input.fold = Some(if portrait {
            rect(0.0, 461.0, width, 28.0)
        } else {
            rect(461.0, 0.0, 28.0, height)
        });
        input
    }

    #[test]
    fn in_landscape_the_dock_is_at_the_right_of_the_fold() {
        let mut input = duo_folded(false);
        let fold = input.fold.unwrap();
        let mut flat = input;
        flat.fold = None;
        // A dock that fits keeps its width.
        let (r, before) = (layout(&input), layout(&flat));
        assert_eq!(r.dock, before.dock);
        assert_eq!(r.tools, before.tools);
        // A wider dock is as wide as the right part.
        input.dock_width = 600.0;
        flat.dock_width = 600.0;
        let r = layout(&input);
        assert_eq!(r.dock.left(), fold.right() + FOLD_GAP);
        assert_eq!(r.dock.right(), 951.0);
        assert!(layout(&flat).dock.left() < fold.left());
        // The readout is at the left of the fold.
        let readout = r.readout.unwrap();
        assert_eq!(readout.right(), fold.left() - FOLD_GAP);
        assert_eq!(r.column_gap, None);
    }

    #[test]
    fn in_portrait_the_tool_column_has_a_gap_at_the_fold() {
        let input = duo_folded(true);
        let fold = input.fold.unwrap();
        let mut flat = input;
        flat.fold = None;
        let (r, before) = (layout(&input), layout(&flat));
        let gap = r.column_gap.expect("a gap");
        assert_eq!(gap.max, fold.bottom() + FOLD_GAP + COLUMN_INSET);
        // The sliders keep their length, and the column is longer by the
        // gap.
        assert_eq!(r.brush_card, BrushCard::Full(TRACK));
        assert_eq!(r.column_content, before.column_content + gap.span());
        assert_eq!(r.tools.height(), r.column_content);
        assert_eq!(r.tools.top(), before.tools.top());
        // The dock ends above the fold.
        assert_eq!(r.dock.top(), before.dock.top());
        assert_eq!(r.dock.bottom(), fold.top() - FOLD_GAP);
        // The tools are above the fold. The first slider does not fit
        // there, so it moves below the fold with the divider above it, and
        // the gap starts at the bottom of the last tool.
        let top = r.tools.top() + COLUMN_INSET;
        assert_eq!(gap.min, top + TOOLS as f32 * COLUMN_CELL);
        assert!(gap.min + COLUMN_INSET < fold.top() - FOLD_GAP);
        let slider = TRACK + SLIDER_TEXT;
        let below = gap.max + COLUMN_DIVIDER + 2.0 * slider + COLUMN_CELL;
        assert_eq!(r.tools.bottom(), below + COLUMN_INSET);

        // A column that does not reach the fold has no gap.
        let mut short = input;
        short.column = ColumnInput {
            tools: TOOLS,
            ..ColumnInput::default()
        };
        let r = layout(&short);
        assert_eq!(r.column_gap, None);
        assert!(r.tools.bottom() < fold.top());

        // A column that scrolls ends above the fold.
        let mut long = input;
        long.column.cells = 12;
        let r = layout(&long);
        assert_eq!(r.column_gap, None);
        assert_eq!(r.tools.bottom(), fold.top() - FOLD_GAP);
        assert!(r.column_content > r.tools.height());
    }

    #[test]
    fn a_card_that_crosses_the_fold_moves_to_the_side_of_its_control() {
        let screen = rect(0.0, 0.0, 669.0, 951.0);
        let fold = Some(rect(0.0, 461.0, 669.0, 28.0));
        let button = rect(600.0, 28.0, 44.0, 44.0);
        // A menu below a button of the top row ends above the fold.
        let menu = rect(300.0, 84.0, 360.0, 600.0);
        let moved = off_fold(menu, button, fold, screen);
        assert_eq!(moved, rect(300.0, 84.0, 360.0, 461.0 - FOLD_GAP - 84.0));
        // A card at the side of a control below the fold starts below it.
        let cell = rect(6.0, 500.0, 44.0, 44.0);
        let card = rect(68.0, 420.0, 200.0, 100.0);
        let moved = off_fold(card, cell, fold, screen);
        assert_eq!(moved.top(), 489.0 + FOLD_GAP);
        assert_eq!(moved.size(), card.size());
        // A card off the fold does not move.
        assert_eq!(off_fold(button, button, fold, screen), button);
        assert_eq!(off_fold(menu, button, None, screen), menu);
        // In landscape, a card at the center of the canvas is in the left
        // part.
        let screen = rect(0.0, 0.0, 951.0, 669.0);
        let fold = Some(rect(461.0, 0.0, 28.0, 669.0));
        assert_eq!(
            canvas_part(screen, fold),
            rect(0.0, 0.0, 461.0 - FOLD_GAP, 669.0)
        );
    }
}
