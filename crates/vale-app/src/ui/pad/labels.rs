//! The Labels tab of the panel of a frame in the Atlas workspace: the label
//! classes of the map of the frame, and the panel of one class with its
//! pages.

use eframe::egui::{self, Align2, Rect, pos2, vec2};
use vale_labeler::Face;
use vale_store::{FrameId, FurnitureText, GeometryKind, LayerId, MapId};
use vale_style::expr::Filter;
use vale_style::{
    AmbiguousLabels, LabelClass, LabelFallback, LabelPlacement, LabelPosition, LineAnchor,
    LineOrientation,
};

use super::atlas::{FRAME_TABS, named_tabs};
use super::filter;
use super::geometry::TOUCH;
use super::label_text;
use super::list::{self, ListState};
use super::number::Number;
use super::text_style::{self, TextEdit, TextEvent, TextSpec};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Head, PageButtons, PageParts, Panel, theme};
use crate::fonts;
use crate::ui::atlas::{ClassColor, ClassPage, ClassSegment};
use crate::ui::expr_help::{self, ExprHelp};
use crate::ui::{Action, AppState, FontTarget};

pub const ADD_CLASS: &str = "Add class";
pub const DELETE_CLASS: &str = "Delete class";
/// The header button that deletes the selected classes.
pub const DELETE_CLASSES: &str = "Delete classes";
pub const NO_LABEL_LAYERS: &str = "This map has no vector layers.";
pub const NO_CLASSES: &str = "No layer of this map has labels.";
pub const CLASS_NAME: &str = "Class name";
pub const LAYER: &str = "Layer";
pub const TEXT: &str = "Text";
/// The row of the face of the font, and the title of its page.
pub const STYLE: &str = "Style";
/// The name of the size slider. Its label is "Size".
pub const LABEL_SIZE: &str = "Label size";
pub const CASE: &str = "Case";
pub const LETTER_SPACING: &str = "Letter spacing";
pub const COLOR: &str = "Color";
pub const HALO: &str = "Halo";
pub const HALO_COLOR: &str = "Halo color";
pub const HALO_WIDTH: &str = "Halo width";
pub const PLACEMENT: &str = "Placement";
/// The segments of the panel of a class: the name of each control and its
/// label.
pub const SEGMENTS: [(&str, &str); 3] = [
    ("Text segment", "Text"),
    ("Placement segment", "Placement"),
    ("Conflicts segment", "Conflicts"),
];
/// The row of the positions of a point class, and the title of its page.
pub const POSITIONS: &str = "Positions";
pub const OFFSET: &str = "Offset";
pub const REPEAT: &str = "Repeat";
pub const REPEAT_INTERVAL: &str = "Repeat interval";
pub const CURVE_LIMIT: &str = "Curve limit";
pub const SMOOTHING: &str = "Smoothing";
pub const SPREAD: &str = "Spread";
pub const FOLLOW_PARALLELS: &str = "Follow parallels";
pub const UPSIDE_DOWN: &str = "Allow upside-down text";
pub const CENTERED: &str = "Center on point";
pub const CENTER_SHIFT: &str = "Center shift";
pub const MAX_OFFSET: &str = "Largest offset";
pub const OUTLINE_OFFSET: &str = "Measure from symbol shape";
/// The row of the angle field of a point class, and the title of its page.
pub const ANGLE_FIELD: &str = "Angle field";
/// The value of the angle field row when the class has no angle field.
pub const NO_ANGLE_FIELD: &str = "None";
/// The row of the ambiguous labels of a point class, and the title of its page.
pub const AMBIGUOUS: &str = "Ambiguous labels";
/// The row of the zones of a polygon class, and the title of its page.
pub const ZONES: &str = "Zones";
/// The headings of the zones page.
pub const INSIDE: &str = "Inside";
pub const OUTSIDE: &str = "Outside";
/// The row of the leader line, and the title of its page.
pub const LEADER_LINE: &str = "Leader line";
/// The headings of the leader line page.
pub const LEADER_START: &str = "Start";
pub const LEADER_END: &str = "End";
pub const SHOW_LINE: &str = "Show line";
/// The switch that gives the line the color of the text.
pub const MATCH_TEXT: &str = "Match text";
/// The switch that gives the halo of the line the halo color of the label.
pub const MATCH_HALO: &str = "Match label halo";
pub const LINE_WIDTH: &str = "Width";
pub const DASHED: &str = "Dashed";
pub const DASH: &str = "Dash";
pub const DASH_GAP: &str = "Gap";
pub const END_SIZE: &str = "End size";
pub const LABEL_GAP: &str = "Gap at label";
pub const ANCHOR_GAP: &str = "Gap at anchor";
pub const OVER_HOLES: &str = "Label over holes";
pub const AVOID_POLYGONS: &str = "Avoid other polygons";
pub const LARGEST_PART: &str = "Largest part only";
pub const REPEAT_LABEL: &str = "Repeat label";
pub const REPEAT_DISTANCE: &str = "Repeat distance";
pub const OVERRUN: &str = "Overrun";
pub const MIN_AREA: &str = "Minimum area";
pub const ORIENTATION: &str = "Orientation";
pub const LINE_POSITION: &str = "Position";
pub const ANCHOR_DISTANCE: &str = "Distance";
pub const ANCHOR_TOLERANCE: &str = "Tolerance";
pub const ANCHOR_PERCENT: &str = "Percent of length";
pub const BY_DIRECTION: &str = "Side by line direction";
pub const FOLLOW_DIRECTION: &str = "Follow line direction";
pub const SPREAD_FILL: &str = "Spread to fill";
pub const MERGE_LINES: &str = "Join connected lines";
pub const MIN_LENGTH: &str = "Minimum length";
/// The text of a blocked zone.
pub const ZONE_OFF: &str = "Off";
/// The names of the zones inside a polygon, row by row from the top left.
pub const INSIDE_ZONES: [&str; 9] = [
    "top left",
    "top",
    "top right",
    "left",
    "center",
    "right",
    "bottom left",
    "bottom",
    "bottom right",
];
/// The names of the zones outside a polygon, row by row from the top left.
pub const OUTSIDE_ZONES: [&str; 8] = [
    "top left",
    "top",
    "top right",
    "left",
    "right",
    "bottom left",
    "bottom",
    "bottom right",
];
/// The highest rank that a tap on a zone gives.
pub const MAX_ZONE_RANK: u8 = 3;
pub const BUFFER: &str = "Buffer";
pub const REMOVE_DUPLICATES: &str = "Remove duplicates";
pub const DUPLICATE_DISTANCE: &str = "Duplicate distance";
/// The row of the fallbacks, and the title of its page.
pub const FALLBACKS: &str = "Fallbacks";
/// The hint of the Fallbacks page of a line class.
pub const LINE_FALLBACKS: &str = "Line labels do not stack or take a leader line.";
pub const MIN_SIZE: &str = "Smallest size";
pub const SIZE_STEP: &str = "Size step";
pub const MIN_WIDTH: &str = "Smallest width";
pub const WIDTH_STEP: &str = "Width step";
pub const MAX_LINES: &str = "Lines";
pub const BREAK_AT_SPACES: &str = "Break at spaces";
/// The field of the separators of the fallback "Stacked lines" other than
/// the space. Its label is "Others".
pub const OTHER_SEPARATORS: &str = "Other separators";
pub const OTHERS: &str = "Others";
pub const SEPARATORS_HINT: &str = "Characters where a line can break.";
pub const MIN_LINE_CHARS: &str = "Fewest characters";
pub const MAX_LINE_CHARS: &str = "Most characters";
pub const ALIGNMENT: &str = "Alignment";
/// The field of the marker of the fallback "Truncation". Its label is
/// "Marker".
pub const TRUNCATION_MARKER: &str = "Truncation marker";
pub const MARKER: &str = "Marker";
pub const LEADER_DISTANCE: &str = "Leader distance";
/// The row of the abbreviations, and the title of its page.
pub const ABBREVIATIONS: &str = "Abbreviations";
pub const ADD_ABBREVIATION: &str = "Add abbreviation";
pub const NO_ABBREVIATIONS: &str = "This class has no abbreviations.";
/// The headings of the columns of the Abbreviations page. The field of a
/// row has the heading and the number of the row as its name, for example
/// "Word 1".
pub const WORD: &str = "Word";
pub const SHORT_FORM: &str = "Short form";
/// The start of the name of the Delete button behind a row of the
/// Abbreviations page. The number of the row follows.
pub const DELETE_ABBREVIATION: &str = "Delete abbreviation";
/// The start of the name of a row of the Abbreviations page. The number
/// of the row follows.
pub const ABBREVIATION: &str = "Abbreviation";
pub const MOVE_UP: &str = "Move up";
pub const MOVE_DOWN: &str = "Move down";
/// The smallest and the largest size of a label, in points.
pub const SIZES: (f64, f64) = (4.0, 72.0);
/// The smallest and the largest letter spacing, as fractions of the size.
pub const SPACINGS: (f64, f64) = (-0.1, 1.0);
/// The thinnest and the widest halo, in points.
pub const HALO_WIDTHS: (f64, f64) = (0.1, 8.0);
/// The smallest and the largest offset, in points.
pub const OFFSETS: (f64, f64) = (0.0, 20.0);
/// The smallest and the largest shift of a centered label, in points.
pub const CENTER_SHIFTS: (f64, f64) = (0.0, 6.0);
/// The shortest and the longest repeat interval, in points.
pub const REPEAT_INTERVALS: (f64, f64) = (50.0, 2000.0);
/// The smallest and the largest curve limit, in degrees per letter.
pub const CURVE_LIMITS: (f64, f64) = (5.0, 60.0);
/// The smallest and the largest smoothing, in multiples of the size.
pub const SMOOTHINGS: (f64, f64) = (0.0, 5.0);
/// The smallest and the largest spread, in multiples of the size.
pub const SPREADS: (f64, f64) = (0.0, 4.0);
/// The shortest and the longest distance between repeated polygon labels, in points.
pub const POLYGON_REPEAT_DISTANCES: (f64, f64) = (50.0, 2000.0);
/// The smallest and the largest overrun, in points.
pub const OVERRUNS: (f64, f64) = (0.0, 20.0);
/// The smallest and the largest overrun of a line label, in points.
pub const LINE_OVERRUNS: (f64, f64) = (0.0, 50.0);
/// The smallest and the largest spread of a line label, in multiples of the size.
pub const LINE_SPREADS: (f64, f64) = (0.0, 2.0);
/// The smallest and the largest anchor distance and tolerance, in points.
pub const ANCHOR_DISTANCES: (f64, f64) = (0.0, 1000.0);
/// The smallest and the largest anchor distance and tolerance, in percent of the length.
pub const ANCHOR_PERCENTS: (f64, f64) = (0.0, 100.0);
/// The smallest and the largest minimum length of a line, in points.
pub const MIN_LENGTHS: (f64, f64) = (0.0, 1000.0);
/// The smallest and the largest minimum area, in square points.
pub const MIN_AREAS: (f64, f64) = (0.0, 10000.0);
/// The thinnest and the widest leader line, in points.
pub const LINE_WIDTHS: (f64, f64) = (0.25, 5.0);
/// The shortest and the longest dash and gap of a leader line, in points.
pub const DASHES: (f64, f64) = (0.5, 20.0);
/// The thinnest and the widest halo of a leader line, in points on each side.
pub const LINE_HALO_WIDTHS: (f64, f64) = (0.25, 5.0);
/// The smallest and the largest end mark, in points.
pub const END_SIZES: (f64, f64) = (1.0, 12.0);
/// The smallest and the largest gap at each end of a leader line, in points.
pub const LEADER_GAPS: (f64, f64) = (0.0, 10.0);
/// The smallest and the largest buffer, in points.
pub const BUFFERS: (f64, f64) = (0.0, 10.0);
/// The smallest and the largest duplicate distance, in points.
pub const DUPLICATE_DISTANCES: (f64, f64) = (0.0, 2000.0);
/// The smallest and the largest smallest size of the fallback "Smaller font", in points.
pub const MIN_SIZES: (f64, f64) = (2.0, 72.0);
/// The smallest and the largest size step, in points.
pub const SIZE_STEPS: (f64, f64) = (0.25, 4.0);
/// The smallest and the largest smallest width of the fallback "Narrow letters", in percent.
pub const MIN_WIDTHS: (f64, f64) = (50.0, 95.0);
/// The width steps of the fallback "Narrow letters", in percent.
pub const WIDTH_STEPS: [f64; 4] = [1.0, 2.0, 5.0, 10.0];
/// The fewest and the most lines of the fallback "Stacked lines".
pub const LINE_COUNTS: (f64, f64) = (2.0, 4.0);
/// The fewest and the most characters on a stacked line.
pub const LINE_CHARS: (f64, f64) = (1.0, 60.0);
/// The smallest and the largest leader distance, in points.
pub const LEADER_DISTANCES: (f64, f64) = (0.0, 50.0);

/// The name of the feature type of a layer on the Layer page.
fn kind_name(kind: GeometryKind) -> &'static str {
    match kind {
        GeometryKind::Point => "Points",
        GeometryKind::Line => "Lines",
        GeometryKind::Polygon => "Polygons",
    }
}

fn layer_name(state: &AppState, layer: LayerId) -> String {
    state
        .doc
        .layer(layer)
        .map_or_else(String::new, |l| l.name.clone())
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).color(theme::MUTE));
    ui.add_space(4.0);
}

/// The open label class of the Labels tab of the selected frame, with the
/// map of the frame. An open class that is gone closes.
pub(super) fn open_class(state: &mut AppState) -> Option<(MapId, LayerId, usize)> {
    let (layer, i) = state.atlas.open_class()?;
    let map = state.atlas.frame().and_then(|f| {
        let frame = state.doc.project.atlas.frame(f)?;
        Some(frame.map)
    });
    let found = map.filter(|m| {
        let fits = state.doc.layer(layer).and_then(|l| l.geometry()).is_some();
        fits && i < state.doc.label_classes(Some(*m), layer).len()
    });
    match found {
        Some(map) => Some((map, layer, i)),
        None => {
            state.atlas.label = None;
            None
        }
    }
}

fn show_class(state: &mut AppState, frame: FrameId, layer: LayerId, i: usize) {
    state.atlas.label = Some((frame, layer, i));
    state.atlas.label_page = None;
    // The panels of all classes have one key.
    state.pad.scroll_top = Some(CLASS_KEY);
    state.atlas.label_segment = ClassSegment::Text;
    state.atlas.label_color = None;
    state.atlas.position_list = ListState::default();
    state.atlas.fallback_list = ListState::default();
    state.atlas.abbreviation_list = ListState::default();
}

/// The label classes of the map of frame `id` in priority order, and the
/// button that adds a class. A tap on a class opens its panel.
pub(super) fn tab(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(map) = state.doc.project.atlas.frame(id).map(|f| f.map) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    if state.label_layers().is_empty() {
        state.atlas.label_list = ListState::default();
        hint(ui, NO_LABEL_LAYERS);
        return;
    }
    // A class with the name of its layer shows the name one time.
    let list: Vec<(LayerId, usize, String, String)> = (state.label_class_list(map).into_iter())
        .map(|(layer, i, c)| {
            let mut of = layer_name(state, layer);
            if of == c.name {
                of.clear();
            }
            (layer, i, c.name, of)
        })
        .collect();
    let mut open = None;
    if list.is_empty() {
        state.atlas.label_list = ListState::default();
        hint(ui, NO_CLASSES);
    } else {
        open = class_rows(ui, state, controls, map, &list);
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, ADD_CLASS, true).clicked() {
        open = state.add_label_class(map);
    }
    if let Some((layer, i)) = open {
        show_class(state, id, layer, i);
    }
}

/// The rows of the class list: the layer, the place in the classes of the
/// layer, the name of the class, and the name of the layer or an empty
/// text. A swipe to the right selects a class, a swipe to the left shows
/// Rename and Delete, and a hold lifts a class to a new place in the
/// order. Returns the class that the user taps.
fn class_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    list: &[(LayerId, usize, String, String)],
) -> Option<(LayerId, usize)> {
    let rows: Vec<list::Row<(MapId, LayerId, usize)>> = (list.iter())
        .map(|(layer, i, name, _)| list::Row {
            rename: true,
            delete: true,
            ..list::Row::new((map, *layer, *i), name.clone())
        })
        .collect();
    let spec = list::Spec {
        select: true,
        reorder: true,
        field: CLASS_NAME,
        ..list::Spec::default()
    };
    let classes = &mut state.atlas.label_list;
    let events = list::show(
        ui,
        controls,
        classes,
        spec,
        &rows,
        |ui, controls, cell, k| {
            let of = &list[k].3;
            let look = list::Look {
                text: &rows[k].name,
                note: (!of.is_empty()).then_some(of.as_str()),
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    let mut open = None;
    for event in events {
        match event {
            list::Event::Tap((_, layer, i)) => open = Some((layer, i)),
            list::Event::Rename((_, layer, i), name) => {
                state.label_class_step(map, layer, i, |c| c.name = name);
            }
            list::Event::Delete((_, layer, i)) => state.delete_label_class(map, layer, i),
            list::Event::Move { from, to, .. } => state.reorder_label_classes(map, from, to),
            _ => {}
        }
    }
    open
}

/// The header buttons of the selected classes of the Labels tab of frame
/// `id`.
pub(super) fn class_group_buttons(state: &AppState, id: FrameId) -> Option<PageButtons> {
    let map = state.doc.project.atlas.frame(id)?.map;
    let picked = state.atlas.label_list.picked.iter();
    let picked = picked.filter(|k| k.0 == map).count();
    let count = state.label_class_list(map).len();
    let buttons = list::group_buttons(picked, count, &[], Some(DELETE_CLASSES))?;
    Some(PageButtons {
        buttons,
        act: Box::new(move |state, name| class_group(state, map, name)),
    })
}

/// Does the action of a header button of the selected classes of `map`.
fn class_group(state: &mut AppState, map: MapId, name: &'static str) {
    let keys: Vec<(MapId, LayerId, usize)> = (state.label_class_list(map).into_iter())
        .map(|(layer, i, _)| (map, layer, i))
        .collect();
    let classes = &mut state.atlas.label_list;
    if let Some(list::Group::Delete(keys)) =
        list::group_action(classes, &keys, name, Some(DELETE_CLASSES))
    {
        let keys: Vec<(LayerId, usize)> = keys.iter().map(|k| (k.1, k.2)).collect();
        state.delete_label_classes(map, &keys);
    }
}

/// The key of the panel of a label class.
const CLASS_KEY: &str = "pad-atlas-class";

/// The panel of the open label class of frame `id`, or of one of its pages,
/// in the pane of the Labels panel.
pub(super) fn class_head(state: &mut AppState, id: FrameId) -> Option<Head> {
    let (map, layer, i) = open_class(state)?;
    let kind = state.doc.layer(layer)?.geometry()?;
    let class = state.doc.label_classes(Some(map), layer).get(i)?.clone();
    let page = state.atlas.label_page;
    let (key, title, back) = match page {
        Some(ClassPage::Layer) => ("pad-atlas-class-layer", LAYER, class.name.as_str()),
        Some(ClassPage::Placement) => ("pad-atlas-placement", PLACEMENT, &*class.name),
        Some(ClassPage::Orientation) => ("pad-atlas-orientation", ORIENTATION, &*class.name),
        Some(ClassPage::LinePosition) => ("pad-atlas-line-position", LINE_POSITION, &*class.name),
        Some(ClassPage::Style) => ("pad-atlas-class-style", STYLE, &*class.name),
        Some(ClassPage::Case) => ("pad-atlas-class-case", CASE, &*class.name),
        Some(ClassPage::Positions) => ("pad-atlas-positions", POSITIONS, &*class.name),
        Some(ClassPage::AngleField) => ("pad-atlas-angle-field", ANGLE_FIELD, &*class.name),
        Some(ClassPage::Ambiguous) => ("pad-atlas-ambiguous", AMBIGUOUS, &*class.name),
        Some(ClassPage::Zones) => ("pad-atlas-zones", ZONES, &*class.name),
        Some(ClassPage::LeaderLine) => ("pad-atlas-leader", LEADER_LINE, &*class.name),
        Some(ClassPage::Fallbacks) => ("pad-atlas-fallbacks", FALLBACKS, &*class.name),
        Some(ClassPage::Abbreviations) => ("pad-atlas-abbreviations", ABBREVIATIONS, FALLBACKS),
        None => (CLASS_KEY, class.name.as_str(), FRAME_TABS[3]),
    };
    let parts = PageParts::new(key, title, Some(back));
    let back_to = move |state: &mut AppState| match page {
        Some(ClassPage::Abbreviations) => state.atlas.label_page = Some(ClassPage::Fallbacks),
        Some(_) => state.atlas.label_page = None,
        None => state.atlas.label = None,
    };
    let top = move |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if page.is_none() {
            segments(ui, state, controls);
        }
    };
    let segment = state.atlas.label_segment;
    let body = move |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let at = (id, map, layer, i);
        match page {
            Some(ClassPage::Layer) => layer_page(ui, state, controls, at),
            Some(ClassPage::Placement) => placement_page(ui, state, controls, at, kind, &class),
            Some(ClassPage::Orientation) => orientation_page(ui, state, controls, at, &class),
            Some(ClassPage::LinePosition) => line_position_page(ui, state, controls, at, &class),
            Some(ClassPage::Style) => style_page(ui, state, controls, at, &class),
            Some(ClassPage::Case) => case_page(ui, state, controls, at, &class),
            Some(ClassPage::Positions) => positions_page(ui, state, controls, at, &class),
            Some(ClassPage::AngleField) => angle_field_page(ui, state, controls, at, &class),
            Some(ClassPage::Ambiguous) => ambiguous_page(ui, state, controls, at, &class),
            Some(ClassPage::Zones) => zones_page(ui, state, controls, at, &class),
            Some(ClassPage::LeaderLine) => leader_page(ui, state, controls, at, &class),
            Some(ClassPage::Fallbacks) => fallbacks_page(ui, state, controls, at, kind, &class),
            Some(ClassPage::Abbreviations) => abbreviations_page(ui, state, controls, at, &class),
            None => {
                match segment {
                    ClassSegment::Text => text_segment(ui, state, controls, at, &class),
                    ClassSegment::Placement => {
                        placement_segment(ui, state, controls, at, kind, &class);
                    }
                    ClassSegment::Conflicts => {
                        conflicts_segment(ui, state, controls, at, kind, &class);
                    }
                }
                class_buttons(ui, state, controls, at);
            }
        }
    };
    Some(parts.head(Panel::Labels, back_to, top, body))
}

/// The text segment of the panel of a label class shows, so the dock is
/// wider.
pub(super) fn wide(state: &AppState) -> bool {
    state.pad.shows(Panel::Labels)
        && state.atlas.open_class().is_some()
        && state.atlas.label_page.is_none()
        && state.atlas.label_segment == ClassSegment::Text
}

/// The segments at the top of the panel of a class.
fn segments(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let items = SEGMENTS.map(|(name, label)| (name, label, true));
    let active = state.atlas.label_segment as usize;
    let all = [
        ClassSegment::Text,
        ClassSegment::Placement,
        ClassSegment::Conflicts,
    ];
    if let Some(chosen) = named_tabs(ui, controls, &items, active) {
        state.atlas.label_segment = all[chosen];
        state.atlas.label_color = None;
        widgets::close_keypad(ui.ctx());
    }
    ui.add_space(6.0);
}

/// The place of a label class: the frame of the panel, the map, the layer,
/// and the place of the class in the classes of the layer.
type ClassAt = (FrameId, MapId, LayerId, usize);

fn filter_ok(text: &str) -> bool {
    Filter::new(text).error().is_none()
}

/// A color row of a class and its color picker while it is open, with an
/// opacity strip. Returns the new color.
pub(super) fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    named: (&str, ClassColor),
    c: vale_style::Color,
) -> Option<vale_style::Color> {
    picker_rows(ui, state, controls, named, c, true)
}

/// A color row and its color picker while it is open. `alpha`: the picker
/// has an opacity strip. Without it, the color stays opaque.
pub(super) fn picker_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (name, which): (&str, ClassColor),
    c: vale_style::Color,
    alpha: bool,
) -> Option<vale_style::Color> {
    let open = state.atlas.label_color == Some(which);
    if widgets::color_row(ui, controls, name, [c.r, c.g, c.b, c.a], open).clicked() {
        state.atlas.label_color = (!open).then_some(which);
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.label_color != Some(which) {
        return None;
    }
    ui.add_space(4.0);
    state.atlas.label_color_shown = true;
    let mut rgba = [c.r, c.g, c.b, c.a];
    let changed = if alpha {
        widgets::color_picker_alpha(ui, controls, name, &mut rgba)
    } else {
        let mut rgb = [c.r, c.g, c.b];
        let changed = widgets::color_picker(ui, controls, name, &mut rgb);
        rgba = widgets::opaque(rgb);
        changed
    };
    changed.then(|| {
        let [r, g, b, a] = rgba;
        vale_style::Color { r, g, b, a }
    })
}

/// The face of a class.
fn face(class: &LabelClass) -> Face {
    Face {
        weight: class.weight,
        italic: class.italic,
    }
}

/// The faces of the font family of `font`. A family with no list of faces
/// gives the regular face.
pub(super) fn faces_of(state: &mut AppState, font: Option<&str>) -> Vec<Face> {
    let family = fonts::family(font, &mut state.fonts).to_owned();
    let faces = state.fonts.faces(&family);
    if faces.is_empty() {
        vec![Face {
            weight: 400.0,
            italic: false,
        }]
    } else {
        faces
    }
}

/// The Text segment of the panel of a class.
fn text_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let mut name = class.name.clone();
    if widgets::text_field(ui, controls, CLASS_NAME, "Name", None, &mut name, true).changed() {
        state.edit_label_class(map, layer, i, |c| c.name = name);
    }
    let layer_name = layer_name(state, layer);
    if widgets::open_row(ui, controls, LAYER, &layer_name).clicked() {
        state.atlas.label_page = Some(ClassPage::Layer);
    }

    let help = ExprHelp::for_layer(state, layer);
    let names = (TEXT, TEXT);
    let mut look = label_text::Look {
        class,
        fonts: &mut state.fonts,
    };
    let edit = label_text::editor(ui, controls, names, &class.text, &help, Some(&mut look));
    if let Some(text) = edit.text {
        if edit.step {
            state.label_class_step(map, layer, i, |c| c.text = text);
        } else {
            state.edit_label_class(map, layer, i, |c| c.text = text);
        }
    }

    ui.add_space(6.0);
    let none = filter::NO_CLASS_CONDITION;
    let count = expr_help::matches(state, layer, &class.filter);
    let salt = (map, layer, i);
    let edit = filter::editor(ui, controls, salt, &class.filter, &help, count, none);
    if let Some(text) = edit.text.filter(|t| filter_ok(t)) {
        if edit.step {
            state.label_class_step(map, layer, i, |c| c.filter = text);
        } else {
            state.edit_label_class(map, layer, i, |c| c.filter = text);
        }
    }
    ui.add_space(6.0);

    let spec = TextSpec {
        size_name: LABEL_SIZE,
        size_step: 1.0,
        sizes: SIZES,
        spacings: SPACINGS,
        halo_widths: HALO_WIDTHS,
        colors: (ClassColor::Text, ClassColor::Halo),
    };
    let style = text_style(class);
    match text_style::section(ui, state, controls, &style, spec) {
        Some(TextEvent::PickFont) => {
            let target = FontTarget::Class(map, layer, i);
            state.actions.push(Action::PickFont(target));
        }
        Some(TextEvent::OpenFaces) => state.atlas.label_page = Some(ClassPage::Style),
        Some(TextEvent::OpenCases) => state.atlas.label_page = Some(ClassPage::Case),
        Some(TextEvent::Edit(edit)) => {
            let apply = |c: &mut LabelClass| apply_text(&edit, c);
            if edit.step() {
                state.label_class_step(map, layer, i, apply);
            } else {
                state.edit_label_class(map, layer, i, apply);
            }
        }
        None => {}
    }
}

/// The text settings of a class as a text style.
fn text_style(class: &LabelClass) -> FurnitureText {
    let rgba = |c: vale_style::Color| [c.r, c.g, c.b, c.a];
    FurnitureText {
        font: class.font.clone(),
        weight: class.weight,
        italic: class.italic,
        size: class.size,
        color: rgba(class.color),
        case: class.case,
        letter_spacing: class.letter_spacing,
        halo: class.halo,
        halo_color: rgba(class.halo_color),
        halo_width: class.halo_width,
    }
}

/// Applies a change of the Text section to a class.
fn apply_text(edit: &TextEdit, c: &mut LabelClass) {
    let mut style = text_style(c);
    edit.apply(&mut style);
    let color = |[r, g, b, a]: [u8; 4]| vale_style::Color { r, g, b, a };
    c.font = style.font;
    c.weight = style.weight;
    c.italic = style.italic;
    c.size = style.size;
    c.color = color(style.color);
    c.case = style.case;
    c.letter_spacing = style.letter_spacing;
    c.halo = style.halo;
    c.halo_color = color(style.halo_color);
    c.halo_width = style.halo_width;
}

/// True if a class of `kind` with `placement` can have straight text that follows the parallels.
pub fn can_follow_parallels(kind: GeometryKind, placement: LabelPlacement) -> bool {
    match kind {
        GeometryKind::Point => true,
        GeometryKind::Polygon => {
            matches!(
                placement,
                LabelPlacement::BestFit | LabelPlacement::Horizontal
            )
        }
        GeometryKind::Line => false,
    }
}

/// True if the text of a class follows the parallels.
pub fn follows_parallels(kind: GeometryKind, class: &LabelClass) -> bool {
    (kind == GeometryKind::Polygon && class.placement == LabelPlacement::Parallel)
        || (class.graticule && can_follow_parallels(kind, class.placement))
}

/// The switches of a class whose text can follow the parallels.
fn parallel_switches(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    kind: GeometryKind,
    class: &LabelClass,
) {
    if can_follow_parallels(kind, class.placement) {
        let mut on = class.graticule;
        if widgets::switch_row(ui, controls, FOLLOW_PARALLELS, FOLLOW_PARALLELS, &mut on) {
            state.label_class_step(map, layer, i, |c| c.graticule = on);
        }
    }
    let turned = kind == GeometryKind::Point && class.angle_field.is_some();
    if follows_parallels(kind, class) || turned {
        let mut on = class.no_flip;
        if widgets::switch_row(ui, controls, UPSIDE_DOWN, UPSIDE_DOWN, &mut on) {
            state.label_class_step(map, layer, i, |c| c.no_flip = on);
        }
    }
}

/// The Placement segment of the panel of a class. A point class has the
/// point options. A line class has its offset and the line
/// options. A polygon class has its spread, and a
/// class that is not horizontal has its curve limit. A point class and a
/// polygon class can follow the parallels.
fn placement_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    if widgets::open_row(ui, controls, PLACEMENT, class.placement.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Placement);
    }
    if kind == GeometryKind::Polygon {
        let spread = Number::new(SPREAD, class.spread, SPREADS.0..=SPREADS.1)
            .step(0.1)
            .unit("×")
            .default(vale_style::SPREAD)
            .slider(ui, controls);
        if let Some(spread) = spread {
            state.edit_label_class(map, layer, i, |c| c.spread = spread);
        }
        if class.placement != LabelPlacement::Horizontal {
            curve_limit(ui, state, controls, (map, layer, i), class);
        }
        parallel_switches(ui, state, controls, (map, layer, i), kind, class);
        polygon_options(ui, state, controls, (map, layer, i), class);
        return;
    }
    if kind == GeometryKind::Point {
        point_options(ui, state, controls, (map, layer, i), class);
        parallel_switches(ui, state, controls, (map, layer, i), kind, class);
        return;
    }
    let offset = Number::new(OFFSET, class.offset, OFFSETS.0..=OFFSETS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(offset) = offset {
        state.edit_label_class(map, layer, i, |c| c.offset = offset);
    }

    line_options(ui, state, controls, (map, layer, i), class);
}

/// The options of a point class: the center switch and the center shift,
/// or the positions, the offsets, and the outline switch; then the angle
/// field.
fn point_options(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    let mut centered = class.centered;
    if widgets::switch_row(ui, controls, CENTERED, CENTERED, &mut centered) {
        state.label_class_step(map, layer, i, |c| c.centered = centered);
    }
    if class.centered {
        let (lo, hi) = CENTER_SHIFTS;
        let shift = Number::new(CENTER_SHIFT, class.center_shift, lo..=hi)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(shift) = shift {
            state.edit_label_class(map, layer, i, |c| c.center_shift = shift);
        }
    } else {
        let on = class.positions.iter().filter(|r| r.on).count();
        let value = format!("{on} of {}", class.positions.len());
        if widgets::open_row(ui, controls, POSITIONS, &value).clicked() {
            state.atlas.label_page = Some(ClassPage::Positions);
        }
        let offset = Number::new(OFFSET, class.offset, OFFSETS.0..=OFFSETS.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(offset) = offset {
            state.edit_label_class(map, layer, i, |c| {
                c.offset = offset;
                c.max_offset = c.max_offset.max(offset);
            });
        }
        let max = class.max_offset.max(class.offset);
        let largest = Number::new(MAX_OFFSET, max, class.offset..=OFFSETS.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(largest) = largest {
            state.edit_label_class(map, layer, i, |c| c.max_offset = largest);
        }
        let mut outline = class.offset_from_outline;
        if widgets::switch_row(ui, controls, OUTLINE_OFFSET, OUTLINE_OFFSET, &mut outline) {
            state.label_class_step(map, layer, i, |c| c.offset_from_outline = outline);
        }
    }
    let field = class.angle_field.as_deref().unwrap_or(NO_ANGLE_FIELD);
    if widgets::open_row(ui, controls, ANGLE_FIELD, field).clicked() {
        state.atlas.label_page = Some(ClassPage::AngleField);
    }
}

/// The options of a line class after its offset: the orientation, the
/// position and its numbers, the direction switches, the repeat, the
/// overrun, the spread, the join, the minimum length, the curve limit, and
/// the smoothing.
fn line_options(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    type Set = fn(&mut LabelClass, bool);
    let switch = |ui: &mut egui::Ui,
                  state: &mut AppState,
                  controls: &mut Controls,
                  name: &str,
                  mut on: bool,
                  set: Set| {
        if widgets::switch_row(ui, controls, name, name, &mut on) {
            state.label_class_step(map, layer, i, |c| set(c, on));
        }
    };
    if widgets::open_row(ui, controls, ORIENTATION, class.orientation.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Orientation);
    }
    if widgets::open_row(ui, controls, LINE_POSITION, class.line_anchor.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::LinePosition);
    }
    let best = class.line_anchor == LineAnchor::Best;
    if !best {
        let ((lo, hi), unit, step) = if class.anchor_percent {
            (ANCHOR_PERCENTS, "%", 1.0)
        } else {
            (ANCHOR_DISTANCES, "pt", 5.0)
        };
        let distance = Number::new(ANCHOR_DISTANCE, class.anchor_distance, lo..=hi)
            .step(step)
            .unit(unit)
            .stepper(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.anchor_distance = distance);
        }
        let tolerance = Number::new(ANCHOR_TOLERANCE, class.anchor_tolerance, lo..=hi)
            .step(step)
            .unit(unit)
            .stepper(ui, controls);
        if let Some(tolerance) = tolerance {
            state.edit_label_class(map, layer, i, |c| c.anchor_tolerance = tolerance);
        }
        switch(
            ui,
            state,
            controls,
            ANCHOR_PERCENT,
            class.anchor_percent,
            |c, on| {
                c.anchor_percent = on;
            },
        );
    }
    switch(
        ui,
        state,
        controls,
        BY_DIRECTION,
        class.by_direction,
        |c, on| {
            c.by_direction = on;
        },
    );
    switch(
        ui,
        state,
        controls,
        FOLLOW_DIRECTION,
        class.follow_direction,
        |c, on| {
            c.follow_direction = on;
        },
    );
    if best {
        switch(ui, state, controls, REPEAT, class.repeat, |c, on| {
            c.repeat = on
        });
        if class.repeat {
            let (lo, hi) = REPEAT_INTERVALS;
            let distance = Number::new(REPEAT_INTERVAL, class.repeat_distance, lo..=hi)
                .nice()
                .step(10.0)
                .unit("pt")
                .stepper(ui, controls);
            if let Some(distance) = distance {
                state.edit_label_class(map, layer, i, |c| c.repeat_distance = distance);
            }
        }
    }
    let overrun = Number::new(OVERRUN, class.overrun, LINE_OVERRUNS.0..=LINE_OVERRUNS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(overrun) = overrun {
        state.edit_label_class(map, layer, i, |c| c.overrun = overrun);
    }
    let along = matches!(
        class.orientation,
        LineOrientation::Curved | LineOrientation::Straight
    );
    if along {
        if !class.spread_fill {
            let (lo, hi) = LINE_SPREADS;
            let spread = Number::new(SPREAD, class.line_spread, lo..=hi)
                .step(0.1)
                .unit("×")
                .stepper(ui, controls);
            if let Some(spread) = spread {
                state.edit_label_class(map, layer, i, |c| c.line_spread = spread);
            }
        }
        switch(
            ui,
            state,
            controls,
            SPREAD_FILL,
            class.spread_fill,
            |c, on| {
                c.spread_fill = on;
            },
        );
    }
    switch(
        ui,
        state,
        controls,
        MERGE_LINES,
        class.merge_lines,
        |c, on| {
            c.merge_lines = on;
        },
    );
    let length = Number::new(MIN_LENGTH, class.min_length, MIN_LENGTHS.0..=MIN_LENGTHS.1)
        .step(5.0)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(length) = length {
        state.edit_label_class(map, layer, i, |c| c.min_length = length);
    }
    if class.orientation == LineOrientation::Curved {
        curve_limit(ui, state, controls, (map, layer, i), class);
    }
    if along {
        let smoothing = Number::new(SMOOTHING, class.smoothing, SMOOTHINGS.0..=SMOOTHINGS.1)
            .step(0.1)
            .unit("×")
            .default(vale_style::SMOOTHING)
            .slider(ui, controls);
        if let Some(smoothing) = smoothing {
            state.edit_label_class(map, layer, i, |c| c.smoothing = smoothing);
        }
    }
}

/// The name of a zone button: the section and the zone, for example
/// "Inside top left".
pub fn zone_name(inside: bool, k: usize) -> String {
    if inside {
        format!("{INSIDE} {}", INSIDE_ZONES[k])
    } else {
        format!("{OUTSIDE} {}", OUTSIDE_ZONES[k])
    }
}

/// The rank of a zone after a tap: 1, 2, 3, then off, then 1 again.
pub fn next_rank(rank: u8) -> u8 {
    match rank {
        0 => 1,
        r if r < MAX_ZONE_RANK => r + 1,
        _ => 0,
    }
}

/// True if a label of the class can go outside its polygon.
fn has_outside(class: &LabelClass) -> bool {
    class.zones_outside.iter().any(|&r| r > 0)
}

/// The zones, the leader start, the switches, and the numbers of a polygon
/// class.
fn polygon_options(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    let on = (class.zones_inside.iter())
        .chain(&class.zones_outside)
        .filter(|&&r| r > 0)
        .count();
    let all = class.zones_inside.len() + class.zones_outside.len();
    if widgets::open_row(ui, controls, ZONES, &format!("{on} of {all}")).clicked() {
        state.atlas.label_page = Some(ClassPage::Zones);
    }
    let value = if class.leader.show {
        class.anchor.name()
    } else {
        ZONE_OFF
    };
    if has_outside(class) && widgets::open_row(ui, controls, LEADER_LINE, value).clicked() {
        state.atlas.label_page = Some(ClassPage::LeaderLine);
    }
    type Set = fn(&mut LabelClass, bool);
    let switches: [(&str, bool, Set); 3] = [
        (OVER_HOLES, class.over_holes, |c, on| c.over_holes = on),
        (AVOID_POLYGONS, class.avoid_polygons, |c, on| {
            c.avoid_polygons = on;
        }),
        (LARGEST_PART, class.largest_part, |c, on| {
            c.largest_part = on
        }),
    ];
    for (name, mut on, set) in switches {
        if widgets::switch_row(ui, controls, name, name, &mut on) {
            state.label_class_step(map, layer, i, |c| set(c, on));
        }
    }
    let mut repeat = class.polygon_repeat;
    if widgets::switch_row(ui, controls, REPEAT_LABEL, REPEAT_LABEL, &mut repeat) {
        state.label_class_step(map, layer, i, |c| c.polygon_repeat = repeat);
    }
    if class.polygon_repeat {
        let (lo, hi) = POLYGON_REPEAT_DISTANCES;
        let distance = Number::new(REPEAT_DISTANCE, class.polygon_repeat_distance, lo..=hi)
            .nice()
            .step(10.0)
            .unit("pt")
            .default(vale_style::POLYGON_REPEAT_DISTANCE)
            .slider(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.polygon_repeat_distance = distance);
        }
    }
    let overrun = Number::new(OVERRUN, class.overrun, OVERRUNS.0..=OVERRUNS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(overrun) = overrun {
        state.edit_label_class(map, layer, i, |c| c.overrun = overrun);
    }
    let area = Number::new(MIN_AREA, class.min_area, MIN_AREAS.0..=MIN_AREAS.1)
        .step(50.0)
        .unit("pt²")
        .stepper(ui, controls);
    if let Some(area) = area {
        state.edit_label_class(map, layer, i, |c| c.min_area = area);
    }
}

/// The side of a zone button.
const ZONE_BUTTON: f32 = 56.0;
/// The space between two zone buttons.
const ZONE_GAP: f32 = 6.0;

/// A 3×3 grid of the zones of one section. `ranks` runs row by row from the
/// top left, and the outside grid has no center. Returns the place of the
/// zone that the user taps.
fn zone_grid(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    inside: bool,
    ranks: &[u8],
) -> Option<usize> {
    let side = 3.0 * ZONE_BUTTON + 2.0 * ZONE_GAP;
    let size = vec2(ui.available_width(), side + 8.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let grid = Rect::from_center_size(rect.center(), vec2(side, side));
    let mut tapped = None;
    for (k, &rank) in ranks.iter().enumerate() {
        // The outside grid skips the center.
        let cell = if inside || k < 4 { k } else { k + 1 };
        let (x, y) = ((cell % 3) as f32, (cell / 3) as f32);
        let min = grid.min + vec2(x, y) * (ZONE_BUTTON + ZONE_GAP);
        let button = Rect::from_min_size(min, vec2(ZONE_BUTTON, ZONE_BUTTON));
        let name = zone_name(inside, k);
        let resp = widgets::tap_area(ui, controls, button, &name, true);
        if resp.clicked() {
            tapped = Some(k);
        }
        let pressed = resp.is_pointer_button_down_on();
        let (fill, text, color) = match rank {
            0 => (theme::track(), ZONE_OFF.to_owned(), theme::MUTE),
            n => (theme::raise(), n.to_string(), theme::TEXT),
        };
        let fill = if pressed { theme::divider() } else { fill };
        let painter = ui.painter();
        painter.rect_filled(button, theme::CONTROL_RADIUS, fill);
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            text,
            theme::body(),
            color,
        );
    }
    if !inside {
        let center = Rect::from_center_size(grid.center(), vec2(ZONE_BUTTON, ZONE_BUTTON));
        ui.painter().rect_stroke(
            center,
            theme::CONTROL_RADIUS,
            egui::Stroke::new(1.0, theme::divider()),
            egui::StrokeKind::Inside,
        );
    }
    tapped
}

/// The zones inside and outside the polygons of a class, each with its
/// rank. A tap on a zone gives it the next rank.
fn zones_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    super::raster::heading(ui, INSIDE);
    if let Some(k) = zone_grid(ui, controls, true, &class.zones_inside) {
        let rank = next_rank(class.zones_inside[k]);
        state.label_class_step(map, layer, i, |c| c.zones_inside[k] = rank);
    }
    super::raster::heading(ui, OUTSIDE);
    if let Some(k) = zone_grid(ui, controls, false, &class.zones_outside) {
        let rank = next_rank(class.zones_outside[k]);
        state.label_class_step(map, layer, i, |c| c.zones_outside[k] = rank);
    }
}

/// The leader line of a label outside its polygon: where it starts, and how
/// it looks.
fn leader_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    use vale_style::{LeaderAnchor, LeaderEnd};
    let style = class.leader;
    // A change that is one undo step.
    let step = |state: &mut AppState, f: &dyn Fn(&mut vale_style::LeaderStyle)| {
        state.label_class_step(map, layer, i, |c| f(&mut c.leader));
    };
    // A change of a number, which joins the undo step of the drag or the keypad.
    let edit = |state: &mut AppState, f: &dyn Fn(&mut vale_style::LeaderStyle)| {
        state.edit_label_class(map, layer, i, |c| f(&mut c.leader));
    };
    super::raster::heading(ui, LEADER_START);
    for anchor in LeaderAnchor::ALL {
        let name = anchor.name();
        let on = anchor == class.anchor;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.anchor = anchor);
        }
    }
    ui.add_space(6.0);
    let mut show = style.show;
    if widgets::switch_row(ui, controls, SHOW_LINE, SHOW_LINE, &mut show) {
        step(state, &|l| l.show = show);
    }
    if !style.show {
        return;
    }
    let mut matched = style.color.is_none();
    if widgets::switch_row(ui, controls, MATCH_TEXT, MATCH_TEXT, &mut matched) {
        let color = (!matched).then_some(class.color);
        step(state, &|l| l.color = color);
    }
    if let Some(color) = style.color {
        let which = (COLOR, ClassColor::LeaderLine);
        if let Some(c) = color_rows(ui, state, controls, which, color) {
            edit(state, &|l| l.color = Some(c));
        }
    }
    let width = Number::new(LINE_WIDTH, style.width, LINE_WIDTHS.0..=LINE_WIDTHS.1)
        .step(0.25)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(width) = width {
        edit(state, &|l| l.width = width);
    }
    let mut dashed = style.dashed;
    if widgets::switch_row(ui, controls, DASHED, DASHED, &mut dashed) {
        step(state, &|l| l.dashed = dashed);
    }
    if style.dashed {
        let dash = Number::new(DASH, style.dash, DASHES.0..=DASHES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(dash) = dash {
            edit(state, &|l| l.dash = dash);
        }
        let gap = Number::new(DASH_GAP, style.dash_gap, DASHES.0..=DASHES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(gap) = gap {
            edit(state, &|l| l.dash_gap = gap);
        }
    }
    let mut halo = style.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        step(state, &|l| l.halo = halo);
    }
    if style.halo {
        let mut matched = style.halo_color.is_none();
        if widgets::switch_row(ui, controls, MATCH_HALO, MATCH_HALO, &mut matched) {
            let color = (!matched).then_some(class.halo_color);
            step(state, &|l| l.halo_color = color);
        }
        if let Some(color) = style.halo_color {
            let which = (HALO_COLOR, ClassColor::LeaderHalo);
            if let Some(c) = color_rows(ui, state, controls, which, color) {
                edit(state, &|l| l.halo_color = Some(c));
            }
        }
        let (lo, hi) = LINE_HALO_WIDTHS;
        let width = Number::new(HALO_WIDTH, style.halo_width, lo..=hi)
            .step(0.25)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(width) = width {
            edit(state, &|l| l.halo_width = width);
        }
    }
    super::raster::heading(ui, LEADER_END);
    for end in LeaderEnd::ALL {
        let name = end.name();
        if widgets::option_row(ui, controls, name, name, &[], end == style.end).clicked() {
            step(state, &|l| l.end = end);
        }
    }
    if style.end != LeaderEnd::None {
        let size = Number::new(END_SIZE, style.end_size, END_SIZES.0..=END_SIZES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(size) = size {
            edit(state, &|l| l.end_size = size);
        }
    }
    ui.add_space(6.0);
    let (lo, hi) = LEADER_GAPS;
    let gap = Number::new(LABEL_GAP, style.label_gap, lo..=hi)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(gap) = gap {
        edit(state, &|l| l.label_gap = gap);
    }
    let gap = Number::new(ANCHOR_GAP, style.anchor_gap, lo..=hi)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(gap) = gap {
        edit(state, &|l| l.anchor_gap = gap);
    }
}

/// The curve limit of a class, in degrees.
fn curve_limit(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    let (lo, hi) = CURVE_LIMITS;
    let degrees = Number::new(CURVE_LIMIT, class.curve_limit.to_degrees(), lo..=hi)
        .step(1.0)
        .unit("°")
        .stepper(ui, controls);
    if let Some(degrees) = degrees {
        let limit = degrees.to_radians();
        state.edit_label_class(map, layer, i, |c| c.curve_limit = limit);
    }
}

/// The Conflicts segment of the panel of a class: the buffer, the
/// distance between labels with the same text, and the fallbacks.
fn conflicts_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    let buffer = Number::new(BUFFER, class.buffer, BUFFERS.0..=BUFFERS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(buffer) = buffer {
        state.edit_label_class(map, layer, i, |c| c.buffer = buffer);
    }
    let mut remove = class.remove_duplicates;
    if widgets::switch_row(
        ui,
        controls,
        REMOVE_DUPLICATES,
        REMOVE_DUPLICATES,
        &mut remove,
    ) {
        state.label_class_step(map, layer, i, |c| c.remove_duplicates = remove);
    }
    if class.remove_duplicates {
        let (lo, hi) = DUPLICATE_DISTANCES;
        let distance = Number::new(DUPLICATE_DISTANCE, class.duplicate_distance, lo..=hi)
            .step(10.0)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.duplicate_distance = distance);
        }
    }
    let fits = |r: &&vale_style::FallbackRank| fallback_applies(kind, r.fallback);
    let all = class.fallbacks.iter().filter(fits).count();
    let on = class.fallbacks.iter().filter(fits).filter(|r| r.on).count();
    let value = match on {
        0 => ZONE_OFF.to_owned(),
        on => format!("{on} of {all}"),
    };
    if widgets::open_row(ui, controls, FALLBACKS, &value).clicked() {
        state.atlas.label_page = Some(ClassPage::Fallbacks);
    }
    if kind == GeometryKind::Point
        && widgets::open_row(ui, controls, AMBIGUOUS, class.ambiguous.name()).clicked()
    {
        state.atlas.label_page = Some(ClassPage::Ambiguous);
    }
}

/// True if a class of `kind` can use `fallback`. A line label does not
/// stack and has no leader line.
pub fn fallback_applies(kind: GeometryKind, fallback: LabelFallback) -> bool {
    kind != GeometryKind::Line || !matches!(fallback, LabelFallback::Stack | LabelFallback::Leader)
}

/// Moves the item at `from` to `to`.
fn move_item<T>(list: &mut Vec<T>, from: usize, to: usize) {
    if from < list.len() && to < list.len() {
        let moved = list.remove(from);
        list.insert(to, moved);
    }
}

/// The ranked fallbacks of a class that its feature type can use, and the
/// options of the fallbacks, in a section for each fallback. A tap on a row
/// turns its fallback on or off, and a hold lifts a row to a new place in
/// the rank. Truncation stays the last fallback that changes the text.
fn fallbacks_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    at: ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    let (_, map, layer, i) = at;
    // The places of the rows in the list of the class.
    let shown: Vec<usize> = (0..class.fallbacks.len())
        .filter(|&k| fallback_applies(kind, class.fallbacks[k].fallback))
        .collect();
    let rows: Vec<(LabelFallback, &str, bool)> = (shown.iter())
        .map(|&k| {
            let r = &class.fallbacks[k];
            (r.fallback, r.fallback.name(), r.on)
        })
        .collect();
    for event in rank_list(ui, controls, &mut state.atlas.fallback_list, &rows) {
        match event {
            list::Event::Tap(f) => {
                state.label_class_step(map, layer, i, |c| {
                    if let Some(r) = c.fallbacks.iter_mut().find(|r| r.fallback == f) {
                        r.on = !r.on;
                    }
                });
            }
            list::Event::Move { from, to, .. } => {
                let (from, to) = (shown[from], shown[to]);
                state.label_class_step(map, layer, i, |c| {
                    move_item(&mut c.fallbacks, from, to);
                    vale_style::FallbackRank::normalize(&mut c.fallbacks);
                });
            }
            _ => {}
        }
    }
    if kind == GeometryKind::Line {
        hint(ui, LINE_FALLBACKS);
    }
    smaller_font_section(ui, state, controls, at, class);
    narrow_letters_section(ui, state, controls, at, class);
    if fallback_applies(kind, LabelFallback::Stack) {
        stacked_lines_section(ui, state, controls, at, class);
    }
    super::raster::heading(ui, LabelFallback::Abbreviate.name());
    let count = class.abbreviations.len().to_string();
    if widgets::open_row(ui, controls, ABBREVIATIONS, &count).clicked() {
        state.atlas.label_page = Some(ClassPage::Abbreviations);
    }
    super::raster::heading(ui, LabelFallback::Truncate.name());
    let mut marker = class.truncation_marker.clone();
    if short_field_row(ui, controls, (TRUNCATION_MARKER, MARKER), &mut marker).changed() {
        state.edit_label_class(map, layer, i, |c| c.truncation_marker = marker);
    }
    // Only a point label has a leader line distance. A polygon sets its
    // leader line on the Leader line page.
    if kind == GeometryKind::Point {
        super::raster::heading(ui, LabelFallback::Leader.name());
        let (lo, hi) = LEADER_DISTANCES;
        let distance = Number::new(LEADER_DISTANCE, class.leader_distance, lo..=hi)
            .step(1.0)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.leader_distance = distance);
        }
    }
}

/// The options of the fallback "Smaller font".
fn smaller_font_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    super::raster::heading(ui, LabelFallback::SmallerFont.name());
    let size = Number::new(MIN_SIZE, class.min_size, MIN_SIZES.0..=MIN_SIZES.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(size) = size {
        state.edit_label_class(map, layer, i, |c| c.min_size = size);
    }
    let step = Number::new(SIZE_STEP, class.size_step, SIZE_STEPS.0..=SIZE_STEPS.1)
        .step(0.25)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(step) = step {
        state.edit_label_class(map, layer, i, |c| c.size_step = step);
    }
}

/// The options of the fallback "Narrow letters".
fn narrow_letters_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    super::raster::heading(ui, LabelFallback::Compress.name());
    let width = Number::new(MIN_WIDTH, class.min_width, MIN_WIDTHS.0..=MIN_WIDTHS.1)
        .step(5.0)
        .unit("%")
        .stepper(ui, controls);
    if let Some(width) = width {
        state.edit_label_class(map, layer, i, |c| c.min_width = width);
    }
    let (lo, hi) = (WIDTH_STEPS[0], WIDTH_STEPS[WIDTH_STEPS.len() - 1]);
    let step = Number::new(WIDTH_STEP, class.width_step, lo..=hi)
        .choices(&WIDTH_STEPS)
        .unit("%")
        .stepper(ui, controls);
    if let Some(step) = step {
        state.edit_label_class(map, layer, i, |c| c.width_step = step);
    }
}

/// The options of the fallback "Stacked lines". The separators are a switch
/// for the space, which a field does not show, and a field for the other
/// characters. The class stores the space first.
fn stacked_lines_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    super::raster::heading(ui, LabelFallback::Stack.name());
    let (lo, hi) = LINE_COUNTS;
    let lines = Number::new(MAX_LINES, f64::from(class.max_lines), lo..=hi)
        .step(1.0)
        .stepper(ui, controls);
    if let Some(lines) = lines {
        let lines = lines.round() as u32;
        state.edit_label_class(map, layer, i, |c| c.max_lines = lines);
    }
    let mut spaces = class.stack_separators.contains(' ');
    let mut others: String = class.stack_separators.replace(' ', "");
    let name = BREAK_AT_SPACES;
    let switched = widgets::switch_row(ui, controls, name, name, &mut spaces);
    let typed = short_field_row(ui, controls, (OTHER_SEPARATORS, OTHERS), &mut others).changed();
    if switched || typed {
        let text = separators(spaces, &others);
        if switched {
            state.label_class_step(map, layer, i, |c| c.stack_separators = text);
        } else {
            state.edit_label_class(map, layer, i, |c| c.stack_separators = text);
        }
    }
    hint(ui, SEPARATORS_HINT);
    let (lo, hi) = LINE_CHARS;
    let fewest = Number::new(MIN_LINE_CHARS, f64::from(class.min_line_chars), lo..=hi)
        .step(1.0)
        .stepper(ui, controls);
    if let Some(fewest) = fewest {
        let fewest = fewest.round() as u32;
        state.edit_label_class(map, layer, i, |c| {
            c.min_line_chars = fewest;
            c.max_line_chars = c.max_line_chars.max(fewest);
        });
    }
    let most = Number::new(MAX_LINE_CHARS, f64::from(class.max_line_chars), lo..=hi)
        .step(1.0)
        .stepper(ui, controls);
    if let Some(most) = most {
        let most = most.round() as u32;
        state.edit_label_class(map, layer, i, |c| {
            c.max_line_chars = most;
            c.min_line_chars = c.min_line_chars.min(most);
        });
    }
    let names = vale_style::StackAlign::ALL.map(|a| a.name());
    let active = (vale_style::StackAlign::ALL.iter())
        .position(|a| *a == class.stack_align)
        .unwrap_or(1);
    if let Some(k) = segment_row(ui, controls, ALIGNMENT, &names, active) {
        let align = vale_style::StackAlign::ALL[k];
        state.label_class_step(map, layer, i, |c| c.stack_align = align);
    }
}

/// The separators of the fallback "Stacked lines": a space if `spaces`,
/// then the characters of `others` that are not spaces.
pub fn separators(spaces: bool, others: &str) -> String {
    let space = if spaces { " " } else { "" };
    let others = others.chars().filter(|c| !c.is_whitespace());
    space.chars().chain(others).collect()
}

/// The width of a short field at the right of a row, the width of a
/// stepper, so that the two line up.
const SHORT_FIELD: f32 = 2.0 * TOUCH + 84.0;

/// A row with `label` at the left and a short field for one line of text at
/// the right. `name` is the name of the field.
fn short_field_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    (name, label): (&str, &str),
    text: &mut String,
) -> egui::Response {
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), egui::Sense::hover());
    let at = pos2(row.left(), row.center().y);
    (ui.painter()).text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    let field = Rect::from_min_size(
        pos2(row.right() - SHORT_FIELD, row.center().y - TOUCH / 2.0),
        vec2(SHORT_FIELD, TOUCH),
    );
    widgets::field_at(ui, controls, field, name, None, text, true, theme::body())
}

/// A row with `label` at the left and a small segmented control of
/// `choices` at the right. Returns the choice that the user tapped.
fn segment_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    choices: &[&str],
    active: usize,
) -> Option<usize> {
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), egui::Sense::hover());
    let at = pos2(row.left(), row.center().y);
    (ui.painter()).text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    let w = widgets::small_segments_width(ui, choices);
    let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 18.0), vec2(w, 36.0));
    widgets::small_segments(ui, controls, seg, choices, active)
}

/// The space between two fields of a row of the Abbreviations page.
const FIELD_GAP: f32 = 8.0;

/// The words and the short forms of a class, one pair on each row. A
/// swipe to the left on a row shows its Delete button.
fn abbreviations_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let pairs = &class.abbreviations;
    if pairs.is_empty() {
        state.atlas.abbreviation_list = ListState::default();
        hint(ui, NO_ABBREVIATIONS);
    } else {
        let (head, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 28.0), egui::Sense::hover());
        let (word, short) = abbreviation_columns(head);
        for (column, text) in [(word, WORD), (short, SHORT_FORM)] {
            let at = pos2(column.left() + 12.0, column.center().y);
            ui.painter()
                .text(at, Align2::LEFT_CENTER, text, theme::small(), theme::MUTE);
        }
    }
    let rows: Vec<list::Row<usize>> = (0..pairs.len())
        .map(|k| list::Row {
            label: format!("{ABBREVIATION} {}", k + 1),
            delete: true,
            ..list::Row::new(k, format!("abbreviation {}", k + 1))
        })
        .collect();
    let mut edit = None;
    let rows_state = &mut state.atlas.abbreviation_list;
    let spec = list::Spec::default();
    let events = list::show(
        ui,
        controls,
        rows_state,
        spec,
        &rows,
        |ui, controls, cell, k| {
            let (word_rect, short_rect) = abbreviation_columns(cell.rect);
            let (mut long, mut short) = pairs[k].clone();
            if !cell.live {
                // The fields take no touch while a swipe moves the row.
                for (rect, text) in [(word_rect, &long), (short_rect, &short)] {
                    let back = rect.shrink(widgets::HIGHLIGHT_INSET);
                    (cell.painter).rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
                    let at = pos2(rect.left() + 12.0, rect.center().y);
                    (cell.painter).text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
                }
                return None;
            }
            let n = k + 1;
            let font = theme::body();
            let name = format!("{WORD} {n}");
            let word = widgets::field_at(
                ui,
                controls,
                word_rect,
                &name,
                None,
                &mut long,
                true,
                font.clone(),
            );
            let name = format!("{SHORT_FORM} {n}");
            let shorter = widgets::field_at(
                ui, controls, short_rect, &name, None, &mut short, true, font,
            );
            if word.changed() || shorter.changed() {
                edit = Some((k, long, short));
            }
            None
        },
    );
    if let Some((k, long, short)) = edit {
        state.edit_label_class(map, layer, i, |c| {
            if let Some(pair) = c.abbreviations.get_mut(k) {
                *pair = (long, short);
            }
        });
    }
    for event in events {
        if let list::Event::Delete(k) = event {
            widgets::close_keypad(ui.ctx());
            state.label_class_step(map, layer, i, |c| {
                if k < c.abbreviations.len() {
                    c.abbreviations.remove(k);
                }
            });
        }
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, ADD_ABBREVIATION, true).clicked() {
        state.label_class_step(map, layer, i, |c| {
            c.abbreviations.push((String::new(), String::new()));
        });
    }
}

/// The field of the word and the field of the short form of a row of the
/// Abbreviations page. The space at the right of the fields takes the
/// swipe of the row.
fn abbreviation_columns(row: Rect) -> (Rect, Rect) {
    let right = row.right() + 8.0 - TOUCH;
    let width = (right - row.left() - 2.0 * FIELD_GAP).max(2.0 * TOUCH);
    let word_width = (width * 0.55).round();
    let field = |left: f32, width: f32| {
        Rect::from_min_size(pos2(left, row.center().y - TOUCH / 2.0), vec2(width, TOUCH))
    };
    let word = field(row.left(), word_width);
    let short = field(word.right() + FIELD_GAP, width - word_width);
    (word, short)
}

/// The buttons at the bottom of each segment of the panel of a class.
fn class_buttons(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
) {
    ui.add_space(6.0);
    let delete = widgets::button_row_in(ui, controls, DELETE_CLASS, true, theme::WARN);
    if delete.clicked() {
        state.delete_label_class(map, layer, i);
        state.atlas.label = None;
    }
}

/// The faces of the font family of the class. A tap sets the weight and the
/// italic of the class.
fn style_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    if let Some(f) = text_style::face_rows(ui, state, controls, class.font.as_deref(), face(class))
    {
        state.label_class_step(map, layer, i, |c| {
            c.weight = f.weight;
            c.italic = f.italic;
        });
        state.atlas.label_page = None;
    }
}

/// The cases of the text.
fn case_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    if let Some(case) = text_style::case_rows(ui, controls, class.case) {
        state.label_class_step(map, layer, i, |c| c.case = case);
        state.atlas.label_page = None;
    }
}

/// The point layers and the line layers. A tap moves the class to the
/// layer.
fn layer_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, map, layer, i): ClassAt,
) {
    for (to, kind) in state.label_layers() {
        let name = layer_name(state, to);
        let lines = [kind_name(kind).to_owned()];
        let row = ui.push_id(("class-layer", to.0), |ui| {
            widgets::option_row(ui, controls, &name, &name, &lines, to == layer)
        });
        if row.inner.clicked() {
            if let Some(j) = state.move_label_class(map, layer, i, to) {
                state.atlas.label = Some((id, to, j));
            }
            state.atlas.label_page = None;
        }
    }
}

/// The placements of the feature type of the layer of the class.
fn placement_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    for &placement in LabelPlacement::choices(kind) {
        let name = placement.name();
        let on = placement == class.placement;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.placement = placement);
            state.atlas.label_page = None;
        }
    }
}

/// The angle fields of a point class: "None" and each numeric field of the
/// layer.
fn angle_field_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let fields = state
        .doc
        .layer(layer)
        .map(|l| super::rules::numeric_fields(&l.fields))
        .unwrap_or_default();
    let choices = std::iter::once(None).chain(fields.into_iter().map(Some));
    for field in choices {
        let name = field.as_deref().unwrap_or(NO_ANGLE_FIELD).to_owned();
        let on = field == class.angle_field;
        let row = ui.push_id(("angle-field", &name), |ui| {
            widgets::option_row(ui, controls, &name, &name, &[], on)
        });
        if row.inner.clicked() {
            state.label_class_step(map, layer, i, |c| c.angle_field = field);
            state.atlas.label_page = None;
        }
    }
}

/// The choices for a point label that is nearer to another point than to
/// its own point.
fn ambiguous_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    for ambiguous in AmbiguousLabels::ALL {
        let name = ambiguous.name();
        let on = ambiguous == class.ambiguous;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.ambiguous = ambiguous);
            state.atlas.label_page = None;
        }
    }
}

/// The orientations of a line class.
fn orientation_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    for orientation in LineOrientation::ALL {
        let name = orientation.name();
        let on = orientation == class.orientation;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.orientation = orientation);
            state.atlas.label_page = None;
        }
    }
}

/// The positions along the line of a line class.
fn line_position_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    for anchor in LineAnchor::ALL {
        let name = anchor.name();
        let on = anchor == class.line_anchor;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.line_anchor = anchor);
            state.atlas.label_page = None;
        }
    }
}

/// The size of a cell of the diagram of the positions.
const CELL: f32 = 30.0;

/// The column and the row of a position in the diagram of the positions.
fn cell(p: LabelPosition) -> (f32, f32) {
    match p {
        LabelPosition::TopLeft => (0.0, 0.0),
        LabelPosition::Top => (1.0, 0.0),
        LabelPosition::TopRight => (2.0, 0.0),
        LabelPosition::Left => (0.0, 1.0),
        LabelPosition::Right => (2.0, 1.0),
        LabelPosition::BottomLeft => (0.0, 2.0),
        LabelPosition::Bottom => (1.0, 2.0),
        LabelPosition::BottomRight => (2.0, 2.0),
    }
}

/// The rank of each row among the rows that are on, from 1. A row that is
/// off has no rank.
fn ranks(on: impl IntoIterator<Item = bool>) -> Vec<Option<usize>> {
    let mut n = 0;
    (on.into_iter())
        .map(|on| {
            on.then(|| {
                n += 1;
                n
            })
        })
        .collect()
}

/// A diagram of the positions around a point, with the rank of each
/// position that is on.
fn diagram(ui: &mut egui::Ui, positions: &[vale_style::PositionRank]) {
    let size = vec2(ui.available_width(), 3.0 * CELL + 16.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let grid = Rect::from_center_size(rect.center(), vec2(3.0 * CELL, 3.0 * CELL));
    let painter = ui.painter();
    let at = |(x, y): (f32, f32)| {
        let min = grid.min + vec2(x * CELL, y * CELL);
        Rect::from_min_size(min, vec2(CELL, CELL)).shrink(2.0)
    };
    painter.circle_filled(at((1.0, 1.0)).center(), 4.0, theme::TEXT);
    for (r, rank) in positions.iter().zip(ranks(positions.iter().map(|r| r.on))) {
        let square = at(cell(r.position));
        let (fill, text, color) = match rank {
            Some(n) => (theme::raise(), n.to_string(), theme::TEXT),
            None => (theme::track(), "–".to_owned(), theme::MUTE),
        };
        painter.rect_filled(square, 4.0, fill);
        painter.text(
            square.center(),
            Align2::CENTER_CENTER,
            text,
            theme::small(),
            color,
        );
    }
}

/// The ranked positions of a point class, best first, under a diagram of
/// the ranks.
fn positions_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let list = &class.positions;
    diagram(ui, list);
    let rows: Vec<(LabelPosition, &str, bool)> = (list.iter())
        .map(|r| (r.position, r.position.name(), r.on))
        .collect();
    for event in rank_list(ui, controls, &mut state.atlas.position_list, &rows) {
        match event {
            list::Event::Tap(p) => {
                state.label_class_step(map, layer, i, |c| {
                    if let Some(r) = c.positions.iter_mut().find(|r| r.position == p) {
                        r.on = !r.on;
                    }
                });
            }
            list::Event::Move { from, to, .. } => {
                state.label_class_step(map, layer, i, |c| move_item(&mut c.positions, from, to));
            }
            _ => {}
        }
    }
}

/// A ranked list of rows, each with a key, a name, and a switch, best
/// first. A tap on a row turns it on or off, and a hold lifts a row to a
/// new place in the rank.
fn rank_list<K>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    state: &mut ListState<K>,
    items: &[(K, &str, bool)],
) -> Vec<list::Event<K>>
where
    K: Copy + PartialEq + std::hash::Hash + std::fmt::Debug + Send + Sync + 'static,
{
    let rank = ranks(items.iter().map(|r| r.2));
    let rows: Vec<list::Row<K>> = (items.iter())
        .map(|&(key, name, _)| list::Row::new(key, name))
        .collect();
    let spec = list::Spec {
        reorder: true,
        ..list::Spec::default()
    };
    list::show(ui, controls, state, spec, &rows, |ui, _, cell, k| {
        let (_, name, on) = items[k];
        let (rect, painter) = (cell.rect, &cell.painter);
        let y = rect.center().y;
        if let Some(n) = rank[k] {
            let at = pos2(rect.left(), y);
            painter.text(at, Align2::LEFT_CENTER, n, theme::mono(), theme::MUTE);
        }
        let at = pos2(rect.left() + 28.0, y);
        painter.text(at, Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
        let id = ui.id().with(("label-switch", name));
        widgets::paint_switch(ui, id, pos2(rect.right(), y), on);
        None
    })
}

/// Closes the color picker of the label class when it did not show in this
/// frame, with the keypad of its field.
pub(super) fn color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.label_color_shown);
    if !shown && state.atlas.label_color.take().is_some() {
        widgets::close_keypad(ctx);
    }
}
