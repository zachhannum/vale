//! The label classes of a map.

use serde_json::{Map, Value as Json, json};
use vale_store::GeometryKind;

use crate::expr::field_ref;
use crate::{Color, LabelStyle, StyleError};

/// The version of the label class JSON that this build writes.
pub const CLASSES_VERSION: u64 = 1;

/// Where a label goes relative to its feature.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LabelPlacement {
    /// The best of the ranked positions around a point.
    Around,
    /// Along a line, on the side that fits.
    AboveOrBelow,
    LineAbove,
    LineBelow,
    OnLine,
}

const POINT_PLACEMENTS: [LabelPlacement; 1] = [LabelPlacement::Around];

const LINE_PLACEMENTS: [LabelPlacement; 4] = [
    LabelPlacement::AboveOrBelow,
    LabelPlacement::LineAbove,
    LabelPlacement::LineBelow,
    LabelPlacement::OnLine,
];

const ALL_PLACEMENTS: [LabelPlacement; 5] = [
    LabelPlacement::Around,
    LabelPlacement::AboveOrBelow,
    LabelPlacement::LineAbove,
    LabelPlacement::LineBelow,
    LabelPlacement::OnLine,
];

impl LabelPlacement {
    /// The placements of a feature type. The first is the default.
    pub fn choices(kind: GeometryKind) -> &'static [LabelPlacement] {
        match kind {
            GeometryKind::Point => &POINT_PLACEMENTS,
            GeometryKind::Line => &LINE_PLACEMENTS,
            GeometryKind::Polygon => &[],
        }
    }

    pub fn default_for(kind: GeometryKind) -> Option<LabelPlacement> {
        LabelPlacement::choices(kind).first().copied()
    }

    /// True if a feature of `kind` can have this placement.
    pub fn fits(self, kind: GeometryKind) -> bool {
        LabelPlacement::choices(kind).contains(&self)
    }

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LabelPlacement::Around => "Around point",
            LabelPlacement::AboveOrBelow => "Above or below",
            LabelPlacement::LineAbove => "Above line",
            LabelPlacement::LineBelow => "Below line",
            LabelPlacement::OnLine => "On line",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LabelPlacement::Around => "around",
            LabelPlacement::AboveOrBelow => "above_or_below",
            LabelPlacement::LineAbove => "line_above",
            LabelPlacement::LineBelow => "line_below",
            LabelPlacement::OnLine => "on_line",
        }
    }

    fn from_key(key: &str) -> Option<LabelPlacement> {
        ALL_PLACEMENTS.into_iter().find(|p| p.key() == key)
    }
}

/// A position of a point label relative to its symbol.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LabelPosition {
    TopRight,
    Right,
    BottomRight,
    Top,
    Bottom,
    TopLeft,
    Left,
    BottomLeft,
}

impl LabelPosition {
    /// The default rank, best first. It equals the default order of the labeler.
    pub const DEFAULT_ORDER: [LabelPosition; 8] = [
        LabelPosition::TopRight,
        LabelPosition::TopLeft,
        LabelPosition::BottomRight,
        LabelPosition::BottomLeft,
        LabelPosition::Right,
        LabelPosition::Left,
        LabelPosition::Top,
        LabelPosition::Bottom,
    ];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            LabelPosition::TopRight => "Top right",
            LabelPosition::Right => "Right",
            LabelPosition::BottomRight => "Bottom right",
            LabelPosition::Top => "Top",
            LabelPosition::Bottom => "Bottom",
            LabelPosition::TopLeft => "Top left",
            LabelPosition::Left => "Left",
            LabelPosition::BottomLeft => "Bottom left",
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            LabelPosition::TopRight => "top_right",
            LabelPosition::Right => "right",
            LabelPosition::BottomRight => "bottom_right",
            LabelPosition::Top => "top",
            LabelPosition::Bottom => "bottom",
            LabelPosition::TopLeft => "top_left",
            LabelPosition::Left => "left",
            LabelPosition::BottomLeft => "bottom_left",
        }
    }

    fn from_key(key: &str) -> Option<LabelPosition> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .find(|p| p.key() == key)
    }
}

/// One position in the rank of a point class. A blocked position is off.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PositionRank {
    pub position: LabelPosition,
    pub on: bool,
}

impl PositionRank {
    /// All positions in the default order, all on.
    pub fn defaults() -> Vec<PositionRank> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .map(|position| PositionRank { position, on: true })
            .collect()
    }

    /// All positions in the default order, with only `only` on.
    fn only(only: LabelPosition) -> Vec<PositionRank> {
        LabelPosition::DEFAULT_ORDER
            .into_iter()
            .map(|position| PositionRank {
                position,
                on: position == only,
            })
            .collect()
    }
}

/// The default offset of a placement, in points.
pub fn offset_for(placement: LabelPlacement) -> f64 {
    if placement == LabelPlacement::Around {
        2.0
    } else {
        4.0
    }
}

/// The default distance between the repeats of a line label, in points.
pub const REPEAT_DISTANCE: f64 = 600.0;

/// The default curve limit, in radians between glyphs.
pub const CURVE_LIMIT: f64 = 0.30;

/// The default smoothing window, in multiples of the font size.
pub const SMOOTHING: f64 = 1.5;

/// The default buffer around a label, in points.
pub const BUFFER: f64 = 1.0;

/// The default distance between two labels with the same text, in points.
pub const DUPLICATE_DISTANCE: f64 = 300.0;

pub use vale_store::TextCase;

/// The default color of the halo.
pub const HALO_COLOR: Color = Color::rgba(255, 255, 255, 217);

/// The default width of the halo, in points.
pub const HALO_WIDTH: f64 = 2.2;

/// The default letter spacing of a feature type, as a fraction of `size`. Line labels get 0.5 pt.
fn letter_spacing_for(kind: GeometryKind, size: f64) -> f64 {
    if kind == GeometryKind::Line && size > 0.0 {
        0.5 / size
    } else {
        0.0
    }
}

/// One class of labels of a layer on a map.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelClass {
    pub name: String,
    /// An expression. Empty text gives no label.
    pub text: String,
    /// A filter. Empty text matches all features.
    pub filter: String,
    /// A font family. `None` is the default font.
    pub font: Option<String>,
    /// Points.
    pub size: f64,
    pub color: Color,
    pub italic: bool,
    /// The font weight, from 100 to 900. 400 is regular.
    pub weight: f32,
    pub case: TextCase,
    /// A fraction of `size`. 0.05 adds 5 % of the size between letters.
    pub letter_spacing: f64,
    /// The opacity of the text, from 0 to 1.
    pub opacity: f64,
    pub halo: bool,
    pub halo_color: Color,
    /// Points.
    pub halo_width: f64,
    /// Higher is placed first.
    pub priority: i32,
    /// A numeric attribute. Higher is placed first within the class.
    pub priority_field: Option<String>,
    pub placement: LabelPlacement,
    /// The distance from the feature, in points.
    pub offset: f64,
    /// The ranked positions of a point label, best first.
    pub positions: Vec<PositionRank>,
    /// Lines. True repeats the label along a long line.
    pub repeat: bool,
    /// Lines. The distance between the repeats, in points.
    pub repeat_distance: f64,
    /// Lines. The largest turn of the text between two glyphs, in radians.
    pub curve_limit: f64,
    /// Lines. The smoothing window of the line, in multiples of `size`.
    pub smoothing: f64,
    /// The margin around the label that other labels keep free, in points.
    pub buffer: f64,
    /// True keeps labels with the same text apart.
    pub remove_duplicates: bool,
    /// The smallest distance between two labels with the same text, in points.
    pub duplicate_distance: f64,
}

impl LabelClass {
    /// The class that gives the labels of `style`. `None` if the labels are off, there is no field, or the feature type has no placements.
    pub fn from_style(
        name: &str,
        style: &LabelStyle,
        kind: GeometryKind,
        priority: i32,
    ) -> Option<LabelClass> {
        if !style.enabled {
            return None;
        }
        let field = style.field.as_deref()?;
        Some(LabelClass {
            name: name.to_owned(),
            text: field_ref(field),
            filter: String::new(),
            font: style.font.clone(),
            size: style.size,
            color: style.color,
            italic: style.italic,
            weight: 400.0,
            case: TextCase::Normal,
            letter_spacing: letter_spacing_for(kind, style.size),
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority,
            priority_field: style.priority_field.clone(),
            ..LabelClass::placement_defaults(LabelPlacement::default_for(kind)?)
        })
    }

    /// A class with the default text and placement options of `placement`.
    fn placement_defaults(placement: LabelPlacement) -> LabelClass {
        LabelClass {
            name: String::new(),
            text: String::new(),
            filter: String::new(),
            font: None,
            size: 11.0,
            color: Color::rgb(0, 0, 0),
            italic: false,
            weight: 400.0,
            case: TextCase::Normal,
            letter_spacing: 0.0,
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority: 0,
            priority_field: None,
            placement,
            offset: offset_for(placement),
            positions: PositionRank::defaults(),
            repeat: true,
            repeat_distance: REPEAT_DISTANCE,
            curve_limit: CURVE_LIMIT,
            smoothing: SMOOTHING,
            buffer: BUFFER,
            remove_duplicates: true,
            duplicate_distance: DUPLICATE_DISTANCE,
        }
    }

    /// A new class that labels each feature with its `name` field. `None` if the feature type has no placements.
    pub fn new(name: &str, kind: GeometryKind, priority: i32) -> Option<LabelClass> {
        let placement = LabelPlacement::default_for(kind)?;
        let line = kind == GeometryKind::Line;
        let size = 11.0;
        Some(LabelClass {
            name: name.to_owned(),
            text: "name".to_owned(),
            filter: String::new(),
            font: None,
            size,
            color: if line {
                Color::rgb(40, 100, 160)
            } else {
                Color::rgb(0x2b, 0x2a, 0x26)
            },
            italic: line,
            weight: 400.0,
            case: TextCase::Normal,
            letter_spacing: letter_spacing_for(kind, size),
            opacity: 1.0,
            halo: true,
            halo_color: HALO_COLOR,
            halo_width: HALO_WIDTH,
            priority,
            priority_field: None,
            remove_duplicates: false,
            ..LabelClass::placement_defaults(placement)
        })
    }
}

/// The JSON text of `classes`. The same classes give the same text.
pub fn classes_to_json(classes: &[LabelClass]) -> String {
    let classes: Vec<Json> = classes
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "text": c.text,
                "filter": c.filter,
                "font": c.font,
                "size": c.size,
                "color": c.color.to_hex(),
                "italic": c.italic,
                "weight": c.weight,
                "case": c.case.id(),
                "letter_spacing": c.letter_spacing,
                "opacity": c.opacity,
                "halo": c.halo,
                "halo_color": c.halo_color.to_hex(),
                "halo_width": c.halo_width,
                "priority": c.priority,
                "priority_field": c.priority_field,
                "placement": c.placement.key(),
                "offset": c.offset,
                "positions": c
                    .positions
                    .iter()
                    .map(|r| json!({ "position": r.position.key(), "on": r.on }))
                    .collect::<Vec<_>>(),
                "repeat": c.repeat,
                "repeat_distance": c.repeat_distance,
                "curve_limit": c.curve_limit,
                "smoothing": c.smoothing,
                "buffer": c.buffer,
                "remove_duplicates": c.remove_duplicates,
                "duplicate_distance": c.duplicate_distance,
            })
        })
        .collect();
    json!({ "version": CLASSES_VERSION, "classes": classes }).to_string()
}

/// The classes of a JSON text. The reader ignores unknown keys.
pub fn classes_from_json(text: &str) -> Result<Vec<LabelClass>, StyleError> {
    let root: Json = serde_json::from_str(text).map_err(|e| StyleError::Json(e.to_string()))?;
    let root = root.as_object().ok_or_else(|| invalid("not an object"))?;
    match root.get("version").and_then(Json::as_u64) {
        Some(1..=CLASSES_VERSION) => {}
        v => return Err(StyleError::Version(v)),
    }
    root.get("classes")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("no classes"))?
        .iter()
        .map(read_class)
        .collect()
}

fn invalid(what: &str) -> StyleError {
    StyleError::Invalid(what.to_owned())
}

fn text(m: &Map<String, Json>, key: &str) -> Option<String> {
    m.get(key).and_then(Json::as_str).map(str::to_owned)
}

fn read_class(j: &Json) -> Result<LabelClass, StyleError> {
    let m = j
        .as_object()
        .ok_or_else(|| invalid("a label class is not an object"))?;
    let color = m
        .get("color")
        .and_then(Json::as_str)
        .and_then(Color::from_hex)
        .ok_or_else(|| invalid("a label class has no valid color"))?;
    let key = text(m, "placement").unwrap_or_default();
    // Older files have one side of a point as the placement.
    let (placement, side) = match key.as_str() {
        "right" => (LabelPlacement::Around, Some(LabelPosition::Right)),
        "left" => (LabelPlacement::Around, Some(LabelPosition::Left)),
        "above" => (LabelPlacement::Around, Some(LabelPosition::Top)),
        "below" => (LabelPlacement::Around, Some(LabelPosition::Bottom)),
        key => (
            LabelPlacement::from_key(key)
                .ok_or_else(|| invalid("a label class has no valid placement"))?,
            None,
        ),
    };
    let positions = match (side, m.get("positions").and_then(Json::as_array)) {
        (Some(side), _) => PositionRank::only(side),
        (None, Some(list)) => list
            .iter()
            .filter_map(|r| {
                let position = r
                    .get("position")
                    .and_then(Json::as_str)
                    .and_then(LabelPosition::from_key)?;
                let on = r.get("on").and_then(Json::as_bool).unwrap_or(true);
                Some(PositionRank { position, on })
            })
            .collect(),
        (None, None) => PositionRank::defaults(),
    };
    let priority = m
        .get("priority")
        .and_then(Json::as_i64)
        .and_then(|p| i32::try_from(p).ok())
        .unwrap_or(0);
    let number = |key: &str, fallback: f64| m.get(key).and_then(Json::as_f64).unwrap_or(fallback);
    let flag = |key: &str, fallback: bool| m.get(key).and_then(Json::as_bool).unwrap_or(fallback);
    Ok(LabelClass {
        name: text(m, "name").unwrap_or_default(),
        text: text(m, "text").unwrap_or_default(),
        filter: text(m, "filter").unwrap_or_default(),
        font: text(m, "font"),
        size: number("size", 11.0),
        color,
        italic: flag("italic", false),
        weight: number("weight", 400.0) as f32,
        case: text(m, "case")
            .as_deref()
            .and_then(TextCase::from_id)
            .unwrap_or(TextCase::Normal),
        letter_spacing: number("letter_spacing", 0.0),
        opacity: number("opacity", 1.0),
        halo: flag("halo", true),
        halo_color: m
            .get("halo_color")
            .and_then(Json::as_str)
            .and_then(Color::from_hex)
            .unwrap_or(HALO_COLOR),
        halo_width: number("halo_width", HALO_WIDTH),
        priority,
        priority_field: text(m, "priority_field"),
        placement,
        offset: number("offset", offset_for(placement)),
        positions,
        repeat: flag("repeat", true),
        repeat_distance: number("repeat_distance", REPEAT_DISTANCE),
        curve_limit: number("curve_limit", CURVE_LIMIT),
        smoothing: number("smoothing", SMOOTHING),
        buffer: number("buffer", BUFFER),
        remove_duplicates: flag("remove_duplicates", true),
        duplicate_distance: number("duplicate_distance", DUPLICATE_DISTANCE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(field: &str) -> LabelStyle {
        LabelStyle {
            enabled: true,
            field: Some(field.to_owned()),
            priority_field: Some("pop".to_owned()),
            size: 13.0,
            color: Color::rgb(10, 20, 30),
            italic: true,
            font: Some("Serif".to_owned()),
        }
    }

    #[test]
    fn a_label_class_survives_json() {
        let mut a = LabelClass::new("Towns", GeometryKind::Point, 2).unwrap();
        a.filter = "pop > 1000".to_owned();
        a.font = Some("Serif".to_owned());
        a.priority_field = Some("pop".to_owned());
        a.size = 14.5;
        a.color = Color::rgba(1, 2, 3, 128);
        a.italic = true;
        a.weight = 700.0;
        a.case = TextCase::AllSmallCaps;
        a.letter_spacing = 0.05;
        a.opacity = 0.75;
        a.halo = false;
        a.halo_color = Color::rgba(0, 0, 0, 100);
        a.halo_width = 1.5;
        a.offset = 3.5;
        a.positions = vec![
            PositionRank {
                position: LabelPosition::Left,
                on: true,
            },
            PositionRank {
                position: LabelPosition::TopRight,
                on: false,
            },
        ];
        a.repeat = false;
        a.repeat_distance = 250.0;
        a.curve_limit = 0.5;
        a.smoothing = 2.0;
        a.buffer = 3.0;
        a.remove_duplicates = false;
        a.duplicate_distance = 120.0;
        let mut b = LabelClass::new("Rivers", GeometryKind::Line, -1).unwrap();
        b.placement = LabelPlacement::LineBelow;
        let classes = vec![a, b];
        let text = classes_to_json(&classes);
        assert_eq!(classes_from_json(&text).unwrap(), classes);
        assert_eq!(classes_to_json(&classes_from_json(&text).unwrap()), text);

        let sparse = r##"{"version":1,"extra":1,"classes":[{"color":"#102030","placement":"on_line","x":2}]}"##;
        let read = classes_from_json(sparse).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].text, "");
        assert_eq!(read[0].font, None);
        assert!(read[0].halo);
        assert_eq!(read[0].halo_color, HALO_COLOR);
        assert_eq!(read[0].halo_width, 2.2);
        assert_eq!(read[0].weight, 400.0);
        assert_eq!(read[0].case, TextCase::Normal);
        assert_eq!(read[0].letter_spacing, 0.0);
        assert_eq!(read[0].opacity, 1.0);
        assert_eq!(read[0].placement, LabelPlacement::OnLine);
        assert_eq!(read[0].offset, 4.0);
        assert_eq!(read[0].positions, PositionRank::defaults());
        assert!(read[0].repeat);
        assert_eq!(read[0].repeat_distance, 600.0);
        assert_eq!(read[0].curve_limit, 0.30);
        assert_eq!(read[0].smoothing, 1.5);
        assert_eq!(read[0].buffer, 1.0);
        assert!(read[0].remove_duplicates);
        assert_eq!(read[0].duplicate_distance, 300.0);
        let point = r##"{"version":1,"classes":[{"color":"#102030","placement":"around"}]}"##;
        assert_eq!(classes_from_json(point).unwrap()[0].offset, 2.0);
        assert_eq!(
            classes_from_json(r#"{"version":9,"classes":[]}"#),
            Err(StyleError::Version(Some(9)))
        );
    }

    #[test]
    fn the_default_class_reproduces_the_label_style() {
        let c = LabelClass::from_style("Default", &style("name"), GeometryKind::Point, 5).unwrap();
        assert_eq!(c.name, "Default");
        assert_eq!(c.text, "name");
        assert_eq!(c.filter, "");
        assert_eq!(c.font.as_deref(), Some("Serif"));
        assert_eq!(c.size, 13.0);
        assert_eq!(c.color, Color::rgb(10, 20, 30));
        assert!(c.italic);
        assert_eq!(c.weight, 400.0);
        assert_eq!(c.case, TextCase::Normal);
        assert_eq!(c.letter_spacing, 0.0);
        assert_eq!(c.opacity, 1.0);
        assert!(c.halo);
        assert_eq!(c.halo_color, Color::rgba(255, 255, 255, 217));
        assert_eq!(c.halo_width, 2.2);
        assert_eq!(c.priority, 5);
        assert_eq!(c.priority_field.as_deref(), Some("pop"));
        assert_eq!(c.placement, LabelPlacement::Around);
        assert_eq!(c.offset, 2.0);
        assert_eq!(c.positions.len(), 8);
        assert!(c.positions.iter().all(|r| r.on));
        assert_eq!(c.positions[0].position, LabelPosition::TopRight);
        assert!(c.repeat);
        assert_eq!(c.repeat_distance, 600.0);
        assert_eq!(c.curve_limit, 0.30);
        assert_eq!(c.smoothing, 1.5);
        assert_eq!(c.buffer, 1.0);
        assert!(c.remove_duplicates);
        assert_eq!(c.duplicate_distance, 300.0);

        let line = LabelClass::from_style("D", &style("old name"), GeometryKind::Line, 0).unwrap();
        assert_eq!(line.text, "\"old name\"");
        assert_eq!(line.placement, LabelPlacement::AboveOrBelow);
        assert_eq!(line.letter_spacing, 0.5 / 13.0);
        assert_eq!(line.offset, 4.0);
        let new = LabelClass::new("R", GeometryKind::Line, 0).unwrap();
        assert_eq!(new.offset, 4.0);
        assert_eq!(new.repeat_distance, 600.0);
        assert_eq!(new.letter_spacing * new.size, 0.5);
        assert_eq!(new.halo_width, 2.2);
        let point = LabelClass::new("T", GeometryKind::Point, 0).unwrap();
        assert_eq!(point.letter_spacing, 0.0);
        assert!(!new.remove_duplicates);
        assert!(!point.remove_duplicates);
        assert_eq!(point.offset, 2.0);
        assert_eq!(point.positions, PositionRank::defaults());
        assert_eq!(field_ref("end"), "\"end\"");
        assert_eq!(field_ref("a\"b"), "\"a\"\"b\"");

        let mut off = style("name");
        off.enabled = false;
        assert_eq!(
            LabelClass::from_style("D", &off, GeometryKind::Point, 0),
            None
        );
        let mut none = style("name");
        none.field = None;
        assert_eq!(
            LabelClass::from_style("D", &none, GeometryKind::Point, 0),
            None
        );
        assert_eq!(
            LabelClass::from_style("D", &style("name"), GeometryKind::Polygon, 0),
            None
        );
    }

    #[test]
    fn each_case_changes_the_text() {
        let text = "the river ODE";
        assert_eq!(TextCase::Normal.apply(text), text);
        assert_eq!(TextCase::Upper.apply(text), "THE RIVER ODE");
        assert_eq!(TextCase::Lower.apply(text), "the river ode");
        assert_eq!(TextCase::Title.apply(text), "The River Ode");
        assert_eq!(TextCase::SmallCaps.apply(text), text);
        assert_eq!(TextCase::AllSmallCaps.apply(text), text);
        for c in TextCase::ALL {
            assert_eq!(TextCase::from_id(c.id()), Some(c));
        }
        assert_eq!(TextCase::AllSmallCaps.id(), "all_small_caps");
        assert_eq!(TextCase::Title.name(), "Title case");
    }

    #[test]
    fn the_placements_match_the_feature_type() {
        use GeometryKind::{Line, Point, Polygon};
        assert_eq!(LabelPlacement::choices(Point), &[LabelPlacement::Around]);
        assert_eq!(LabelPlacement::choices(Line).len(), 4);
        assert!(LabelPlacement::choices(Polygon).is_empty());
        assert_eq!(
            LabelPlacement::default_for(Point),
            Some(LabelPlacement::Around)
        );
        assert_eq!(
            LabelPlacement::default_for(Line),
            Some(LabelPlacement::AboveOrBelow)
        );
        assert_eq!(LabelPlacement::default_for(Polygon), None);
        for p in ALL_PLACEMENTS {
            assert_ne!(p.fits(Point), p.fits(Line));
            assert!(!p.fits(Polygon));
            assert_eq!(LabelPlacement::from_key(p.key()), Some(p));
        }
        assert_eq!(LabelPlacement::AboveOrBelow.name(), "Above or below");
        assert_eq!(LabelPlacement::Around.name(), "Around point");
        assert_eq!(LabelClass::new("A", Polygon, 0), None);
        for p in LabelPosition::DEFAULT_ORDER {
            assert_eq!(LabelPosition::from_key(p.key()), Some(p));
        }
        assert_eq!(LabelPosition::TopRight.name(), "Top right");
        assert_eq!(LabelPosition::BottomLeft.key(), "bottom_left");
    }

    #[test]
    fn a_point_placement_of_one_side_reads_as_a_ranked_position() {
        for (key, side) in [
            ("right", LabelPosition::Right),
            ("left", LabelPosition::Left),
            ("above", LabelPosition::Top),
            ("below", LabelPosition::Bottom),
        ] {
            let text = format!(
                r##"{{"version":1,"classes":[{{"color":"#102030","placement":"{key}"}}]}}"##
            );
            let c = &classes_from_json(&text).unwrap()[0];
            assert_eq!(c.placement, LabelPlacement::Around);
            assert_eq!(c.offset, 2.0);
            assert_eq!(c.positions.len(), 8);
            let on: Vec<_> = c.positions.iter().filter(|r| r.on).collect();
            assert_eq!(on.len(), 1);
            assert_eq!(on[0].position, side);
        }
    }
}
