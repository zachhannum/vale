//! The label classes of a map.

use serde_json::{Map, Value as Json, json};
use vale_store::GeometryKind;

use crate::expr::{field_ref, title_case};
use crate::{Color, LabelStyle, StyleError};

/// The version of the label class JSON that this build writes.
pub const CLASSES_VERSION: u64 = 1;

/// Where a label goes relative to its feature.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LabelPlacement {
    /// The best of the positions around a point.
    Around,
    Right,
    Left,
    Above,
    Below,
    /// Along a line, on the side that fits.
    AboveOrBelow,
    LineAbove,
    LineBelow,
    OnLine,
}

const POINT_PLACEMENTS: [LabelPlacement; 5] = [
    LabelPlacement::Around,
    LabelPlacement::Right,
    LabelPlacement::Left,
    LabelPlacement::Above,
    LabelPlacement::Below,
];

const LINE_PLACEMENTS: [LabelPlacement; 4] = [
    LabelPlacement::AboveOrBelow,
    LabelPlacement::LineAbove,
    LabelPlacement::LineBelow,
    LabelPlacement::OnLine,
];

const ALL_PLACEMENTS: [LabelPlacement; 9] = [
    LabelPlacement::Around,
    LabelPlacement::Right,
    LabelPlacement::Left,
    LabelPlacement::Above,
    LabelPlacement::Below,
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
            LabelPlacement::Right => "Right",
            LabelPlacement::Left => "Left",
            LabelPlacement::Above => "Above",
            LabelPlacement::Below => "Below",
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
            LabelPlacement::Right => "right",
            LabelPlacement::Left => "left",
            LabelPlacement::Above => "above",
            LabelPlacement::Below => "below",
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

/// How the case of the label text changes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextCase {
    Normal,
    Upper,
    Lower,
    Title,
    /// Small capitals for the lowercase letters. The labeler shapes them.
    SmallCaps,
    /// Small capitals for all letters. The labeler shapes them.
    AllSmallCaps,
}

impl TextCase {
    pub const ALL: [TextCase; 6] = [
        TextCase::Normal,
        TextCase::Upper,
        TextCase::Lower,
        TextCase::Title,
        TextCase::SmallCaps,
        TextCase::AllSmallCaps,
    ];

    /// The UI text.
    pub fn name(self) -> &'static str {
        match self {
            TextCase::Normal => "Normal",
            TextCase::Upper => "Uppercase",
            TextCase::Lower => "Lowercase",
            TextCase::Title => "Title case",
            TextCase::SmallCaps => "Small caps",
            TextCase::AllSmallCaps => "All small caps",
        }
    }

    /// The text in this case. The small caps cases give the text unchanged.
    pub fn apply(self, text: &str) -> String {
        match self {
            TextCase::Upper => text.to_uppercase(),
            TextCase::Lower => text.to_lowercase(),
            TextCase::Title => title_case(text),
            TextCase::Normal | TextCase::SmallCaps | TextCase::AllSmallCaps => text.to_owned(),
        }
    }

    /// The JSON name.
    fn key(self) -> &'static str {
        match self {
            TextCase::Normal => "normal",
            TextCase::Upper => "upper",
            TextCase::Lower => "lower",
            TextCase::Title => "title",
            TextCase::SmallCaps => "small_caps",
            TextCase::AllSmallCaps => "all_small_caps",
        }
    }

    fn from_key(key: &str) -> Option<TextCase> {
        TextCase::ALL.into_iter().find(|c| c.key() == key)
    }
}

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
            placement: LabelPlacement::default_for(kind)?,
        })
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
            placement,
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
                "case": c.case.key(),
                "letter_spacing": c.letter_spacing,
                "opacity": c.opacity,
                "halo": c.halo,
                "halo_color": c.halo_color.to_hex(),
                "halo_width": c.halo_width,
                "priority": c.priority,
                "priority_field": c.priority_field,
                "placement": c.placement.key(),
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
    let placement = text(m, "placement")
        .as_deref()
        .and_then(LabelPlacement::from_key)
        .ok_or_else(|| invalid("a label class has no valid placement"))?;
    let priority = m
        .get("priority")
        .and_then(Json::as_i64)
        .and_then(|p| i32::try_from(p).ok())
        .unwrap_or(0);
    let number = |key: &str, fallback: f64| m.get(key).and_then(Json::as_f64).unwrap_or(fallback);
    Ok(LabelClass {
        name: text(m, "name").unwrap_or_default(),
        text: text(m, "text").unwrap_or_default(),
        filter: text(m, "filter").unwrap_or_default(),
        font: text(m, "font"),
        size: number("size", 11.0),
        color,
        italic: m.get("italic").and_then(Json::as_bool).unwrap_or(false),
        weight: number("weight", 400.0) as f32,
        case: text(m, "case")
            .as_deref()
            .and_then(TextCase::from_key)
            .unwrap_or(TextCase::Normal),
        letter_spacing: number("letter_spacing", 0.0),
        opacity: number("opacity", 1.0),
        halo: m.get("halo").and_then(Json::as_bool).unwrap_or(true),
        halo_color: m
            .get("halo_color")
            .and_then(Json::as_str)
            .and_then(Color::from_hex)
            .unwrap_or(HALO_COLOR),
        halo_width: number("halo_width", HALO_WIDTH),
        priority,
        priority_field: text(m, "priority_field"),
        placement,
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
        a.placement = LabelPlacement::Below;
        let b = LabelClass::new("Rivers", GeometryKind::Line, -1).unwrap();
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

        let line = LabelClass::from_style("D", &style("old name"), GeometryKind::Line, 0).unwrap();
        assert_eq!(line.text, "\"old name\"");
        assert_eq!(line.placement, LabelPlacement::AboveOrBelow);
        assert_eq!(line.letter_spacing, 0.5 / 13.0);
        let new = LabelClass::new("R", GeometryKind::Line, 0).unwrap();
        assert_eq!(new.letter_spacing * new.size, 0.5);
        assert_eq!(new.halo_width, 2.2);
        let point = LabelClass::new("T", GeometryKind::Point, 0).unwrap();
        assert_eq!(point.letter_spacing, 0.0);
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
            assert_eq!(TextCase::from_key(c.key()), Some(c));
        }
        assert_eq!(TextCase::AllSmallCaps.key(), "all_small_caps");
        assert_eq!(TextCase::Title.name(), "Title case");
    }

    #[test]
    fn the_placements_match_the_feature_type() {
        use GeometryKind::{Line, Point, Polygon};
        assert_eq!(LabelPlacement::choices(Point).len(), 5);
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
    }
}
