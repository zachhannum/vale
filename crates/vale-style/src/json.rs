//! The JSON form of a layer style.
//!
//! A constant property is a bare value: hex text for a color, a number for a size. A property that is an expression is an object with `expr` and `fallback`. The reader ignores unknown keys.

use std::fmt;

use serde_json::{Map, Value as Json, json};

use crate::expr::{Filter, FromValue, Property};
use crate::{Color, LabelStyle, LayerStyle, Rule, StrokeSymbolizer, Style, Symbolizer};

/// The version of the style JSON that this build writes and reads.
pub const STYLE_VERSION: u64 = 1;

/// Why a style JSON did not load.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleError {
    /// The text is not JSON.
    Json(String),
    /// The version is missing or is not one that this build reads.
    Version(Option<u64>),
    /// The JSON does not have the form of a style.
    Invalid(String),
}

impl fmt::Display for StyleError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StyleError::Json(e) => write!(f, "the style is not JSON: {e}"),
            StyleError::Version(None) => f.write_str("the style has no version"),
            StyleError::Version(Some(v)) => {
                write!(f, "style version {v} is newer than version {STYLE_VERSION}")
            }
            StyleError::Invalid(e) => write!(f, "the style is not valid: {e}"),
        }
    }
}

impl std::error::Error for StyleError {}

fn invalid(what: impl Into<String>) -> StyleError {
    StyleError::Invalid(what.into())
}

impl LayerStyle {
    /// The JSON text. The same style gives the same text.
    pub fn to_json(&self) -> String {
        let rules: Vec<Json> = self.style.rules.iter().map(rule_json).collect();
        let l = &self.label;
        let label = json!({
            "enabled": l.enabled,
            "field": l.field,
            "priority_field": l.priority_field,
            "size": l.size,
            "color": l.color.to_hex(),
            "italic": l.italic,
        });
        json!({ "version": STYLE_VERSION, "rules": rules, "label": label }).to_string()
    }

    pub fn from_json(text: &str) -> Result<LayerStyle, StyleError> {
        let root: Json = serde_json::from_str(text).map_err(|e| StyleError::Json(e.to_string()))?;
        let root = root.as_object().ok_or_else(|| invalid("not an object"))?;
        match root.get("version").and_then(Json::as_u64) {
            Some(STYLE_VERSION) => {}
            v => return Err(StyleError::Version(v)),
        }
        let rules = root
            .get("rules")
            .and_then(Json::as_array)
            .ok_or_else(|| invalid("no rules"))?
            .iter()
            .map(read_rule)
            .collect::<Result<_, _>>()?;
        let label = read_label(object(root, "label")?)?;
        Ok(LayerStyle {
            style: Style { rules },
            label,
        })
    }
}

fn rule_json(r: &Rule) -> Json {
    let mut m = Map::new();
    m.insert("name".into(), json!(r.name));
    let filter = r.filter.source().map(|s| s.text()).unwrap_or("");
    m.insert("filter".into(), json!(filter));
    if let Some(v) = r.min_scale {
        m.insert("min_scale".into(), json!(v));
    }
    if let Some(v) = r.max_scale {
        m.insert("max_scale".into(), json!(v));
    }
    let symbolizers: Vec<Json> = r.symbolizers.iter().map(symbolizer_json).collect();
    m.insert("symbolizers".into(), Json::Array(symbolizers));
    Json::Object(m)
}

fn symbolizer_json(s: &Symbolizer) -> Json {
    match s {
        Symbolizer::Fill { color } => json!({ "type": "fill", "color": color_json(color) }),
        Symbolizer::Stroke(s) => {
            let mut m = stroke_json(s);
            m.insert("type".into(), json!("stroke"));
            Json::Object(m)
        }
        Symbolizer::Marker {
            radius,
            fill,
            stroke,
        } => json!({
            "type": "marker",
            "radius": number_json(radius),
            "fill": fill.as_ref().map(color_json),
            "stroke": stroke.as_ref().map(|s| Json::Object(stroke_json(s))),
        }),
    }
}

fn stroke_json(s: &StrokeSymbolizer) -> Map<String, Json> {
    let mut m = Map::new();
    m.insert("color".into(), color_json(&s.color));
    m.insert("width".into(), number_json(&s.width));
    m
}

fn property_json<T: FromValue + Clone>(p: &Property<T>, fallback: Json) -> Json {
    match p.source() {
        None => fallback,
        Some(s) => json!({ "expr": s.text(), "fallback": fallback }),
    }
}

fn color_json(p: &Property<Color>) -> Json {
    property_json(p, json!(p.fallback.to_hex()))
}

fn number_json(p: &Property<f64>) -> Json {
    property_json(p, json!(p.fallback))
}

fn object<'a>(m: &'a Map<String, Json>, key: &str) -> Result<&'a Map<String, Json>, StyleError> {
    m.get(key)
        .and_then(Json::as_object)
        .ok_or_else(|| invalid(format!("no {key}")))
}

fn optional_text(m: &Map<String, Json>, key: &str) -> Option<String> {
    m.get(key).and_then(Json::as_str).map(str::to_string)
}

fn read_rule(j: &Json) -> Result<Rule, StyleError> {
    let m = j
        .as_object()
        .ok_or_else(|| invalid("a rule is not an object"))?;
    let symbolizers = m
        .get("symbolizers")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("a rule has no symbolizers"))?
        .iter()
        .map(read_symbolizer)
        .collect::<Result<_, _>>()?;
    Ok(Rule {
        name: optional_text(m, "name").unwrap_or_default(),
        filter: Filter::new(m.get("filter").and_then(Json::as_str).unwrap_or("")),
        min_scale: m.get("min_scale").and_then(Json::as_f64),
        max_scale: m.get("max_scale").and_then(Json::as_f64),
        symbolizers,
    })
}

fn read_symbolizer(j: &Json) -> Result<Symbolizer, StyleError> {
    let m = j
        .as_object()
        .ok_or_else(|| invalid("a symbolizer is not an object"))?;
    match m.get("type").and_then(Json::as_str) {
        Some("fill") => Ok(Symbolizer::Fill {
            color: read_color(m.get("color"))?,
        }),
        Some("stroke") => Ok(Symbolizer::Stroke(read_stroke(m)?)),
        Some("marker") => Ok(Symbolizer::Marker {
            radius: read_number(m.get("radius"))?,
            fill: match m.get("fill") {
                None | Some(Json::Null) => None,
                c => Some(read_color(c)?),
            },
            stroke: match m.get("stroke") {
                None | Some(Json::Null) => None,
                Some(Json::Object(s)) => Some(read_stroke(s)?),
                Some(_) => return Err(invalid("a marker stroke is not an object")),
            },
        }),
        Some(t) => Err(invalid(format!("unknown symbolizer {t}"))),
        None => Err(invalid("a symbolizer has no type")),
    }
}

fn read_stroke(m: &Map<String, Json>) -> Result<StrokeSymbolizer, StyleError> {
    Ok(StrokeSymbolizer {
        color: read_color(m.get("color"))?,
        width: read_number(m.get("width"))?,
    })
}

/// Reads a bare value or an object with `expr` and `fallback`.
fn read_property<T: FromValue + Clone>(
    j: Option<&Json>,
    constant: impl Fn(&Json) -> Option<T>,
) -> Result<Property<T>, StyleError> {
    let j = j.ok_or_else(|| invalid("a property is missing"))?;
    if let Some(m) = j.as_object() {
        let fallback = m
            .get("fallback")
            .and_then(&constant)
            .ok_or_else(|| invalid("a property has no valid fallback"))?;
        let expr = m
            .get("expr")
            .and_then(Json::as_str)
            .ok_or_else(|| invalid("a property has no expr"))?;
        return Ok(Property::expression(expr, fallback));
    }
    constant(j)
        .map(Property::constant)
        .ok_or_else(|| invalid(format!("{j} is not a valid value")))
}

fn read_color(j: Option<&Json>) -> Result<Property<Color>, StyleError> {
    read_property(j, |v| v.as_str().and_then(Color::from_hex))
}

fn read_number(j: Option<&Json>) -> Result<Property<f64>, StyleError> {
    read_property(j, Json::as_f64)
}

fn read_label(m: &Map<String, Json>) -> Result<LabelStyle, StyleError> {
    let color = m
        .get("color")
        .and_then(Json::as_str)
        .and_then(Color::from_hex)
        .ok_or_else(|| invalid("the label has no valid color"))?;
    Ok(LabelStyle {
        enabled: m.get("enabled").and_then(Json::as_bool).unwrap_or(false),
        field: optional_text(m, "field"),
        priority_field: optional_text(m, "priority_field"),
        size: m.get("size").and_then(Json::as_f64).unwrap_or(11.0),
        color,
        italic: m.get("italic").and_then(Json::as_bool).unwrap_or(false),
    })
}
