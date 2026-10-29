//! The JSON form of a layer style.
//!
//! A constant property is a bare value: hex text for a color, a number for a size. A property that is an expression is an object with `expr` and `fallback`. The reader ignores unknown keys.

use std::fmt;

use serde_json::{Map, Value as Json, json};
use vale_store::{BlendMode, Hillshade, RasterDisplay, Rgb, Smoothing, Stop, StopLine};

use crate::expr::{Filter, FromValue, Property};
use crate::{
    Color, ImageSymbolizer, LabelStyle, LayerStyle, PointSymbolizer, RasterSymbolizer, Rule,
    ScatterSymbolizer, StrokeKind, StrokeSymbolizer, Style, Symbolizer,
};

/// The version of the style JSON that this build writes. It reads this version and each earlier version.
pub const STYLE_VERSION: u64 = 3;

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
            Some(1..=STYLE_VERSION) => {}
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
        Symbolizer::Raster(r) => raster_json(r),
        Symbolizer::Image(i) => json!({
            "type": "image",
            "opacity": f64::from(i.opacity),
            "blend": i.blend.id(),
        }),
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
        Symbolizer::Symbol(s) => json!({
            "type": "symbol",
            "symbol": s.symbol,
            "size": number_json(&s.size),
            "rotation": number_json(&s.rotation),
            "tint": s.tint.as_ref().map(color_json),
        }),
        Symbolizer::Scatter(s) => json!({
            "type": "scatter",
            "symbol": s.symbol,
            "size": number_json(&s.size),
            "spacing": number_json(&s.spacing),
            "size_jitter": s.size_jitter,
            "rotation_jitter": s.rotation_jitter,
            "tint": s.tint.as_ref().map(color_json),
        }),
    }
}

fn stroke_json(s: &StrokeSymbolizer) -> Map<String, Json> {
    let mut m = Map::new();
    m.insert("color".into(), color_json(&s.color));
    m.insert("width".into(), number_json(&s.width));
    match s.kind {
        StrokeKind::Solid => {}
        StrokeKind::Dashed { dash, gap } => {
            m.insert("kind".into(), json!("dashed"));
            m.insert("dash".into(), json!(dash));
            m.insert("gap".into(), json!(gap));
        }
        StrokeKind::Brush => {
            m.insert("kind".into(), json!("brush"));
        }
        StrokeKind::Taper { start } => {
            m.insert("kind".into(), json!("taper"));
            m.insert("start".into(), json!(start));
        }
    }
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
        Some("symbol") => Ok(Symbolizer::Symbol(PointSymbolizer {
            symbol: read_symbol_name(m)?,
            size: read_number(m.get("size"))?,
            rotation: match m.get("rotation") {
                None => Property::constant(0.0),
                r => read_number(r)?,
            },
            tint: read_tint(m)?,
        })),
        Some("scatter") => Ok(Symbolizer::Scatter(ScatterSymbolizer {
            symbol: read_symbol_name(m)?,
            size: read_number(m.get("size"))?,
            spacing: read_number(m.get("spacing"))?,
            size_jitter: m
                .get("size_jitter")
                .and_then(Json::as_f64)
                .unwrap_or(ScatterSymbolizer::SIZE_JITTER),
            rotation_jitter: m
                .get("rotation_jitter")
                .and_then(Json::as_f64)
                .unwrap_or(0.0),
            tint: read_tint(m)?,
        })),
        Some("raster") => Ok(Symbolizer::Raster(read_raster(m)?)),
        Some("image") => {
            let (opacity, blend) = read_mix(m);
            Ok(Symbolizer::Image(ImageSymbolizer { opacity, blend }))
        }
        Some(t) => Err(invalid(format!("unknown symbolizer {t}"))),
        None => Err(invalid("a symbolizer has no type")),
    }
}

fn read_symbol_name(m: &Map<String, Json>) -> Result<String, StyleError> {
    optional_text(m, "symbol").ok_or_else(|| invalid("a symbol has no name"))
}

fn read_tint(m: &Map<String, Json>) -> Result<Option<Property<Color>>, StyleError> {
    match m.get("tint") {
        None | Some(Json::Null) => Ok(None),
        c => Ok(Some(read_color(c)?)),
    }
}

fn read_stroke(m: &Map<String, Json>) -> Result<StrokeSymbolizer, StyleError> {
    Ok(StrokeSymbolizer {
        color: read_color(m.get("color"))?,
        width: read_number(m.get("width"))?,
        kind: read_kind(m),
    })
}

/// A missing or unknown kind is solid. A missing number is the default.
fn read_kind(m: &Map<String, Json>) -> StrokeKind {
    let number = |k: &str, d: f64| m.get(k).and_then(Json::as_f64).unwrap_or(d);
    match m.get("kind").and_then(Json::as_str) {
        Some("dashed") => StrokeKind::Dashed {
            dash: number("dash", StrokeKind::DASH),
            gap: number("gap", StrokeKind::GAP),
        },
        Some("brush") => StrokeKind::Brush,
        Some("taper") => StrokeKind::Taper {
            start: number("start", StrokeKind::TAPER_START),
        },
        _ => StrokeKind::Solid,
    }
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

fn rgb_json(c: Rgb) -> Json {
    json!(Color::rgb(c[0], c[1], c[2]).to_hex())
}

fn read_rgb(j: Option<&Json>) -> Option<Rgb> {
    let c = Color::from_hex(j?.as_str()?)?;
    (c.a == 255).then_some([c.r, c.g, c.b])
}

fn display_name(d: RasterDisplay) -> &'static str {
    match d {
        RasterDisplay::Bands => "bands",
        RasterDisplay::Gradient => "gradient",
        RasterDisplay::Grey => "grey",
    }
}

fn raster_json(r: &RasterSymbolizer) -> Json {
    let stops: Vec<Json> = r
        .stops
        .iter()
        .map(|s| {
            let l = &s.line;
            json!({
                "value": s.value,
                "line": {
                    "on": l.on,
                    "weight": f64::from(l.weight),
                    "color": rgb_json(l.color),
                    "darker": l.darker,
                    "darken": f64::from(l.darken),
                },
            })
        })
        .collect();
    let colors: Vec<Json> = r.colors.iter().map(|c| rgb_json(*c)).collect();
    let h = &r.hillshade;
    json!({
        "type": "raster",
        "display": display_name(r.display),
        "opacity": f64::from(r.opacity),
        "blend": r.blend.id(),
        "stops": stops,
        "colors": colors,
        "lines": r.lines,
        "smoothing": {
            "on": r.smoothing.on,
            "feature_size": f64::from(r.smoothing.feature_size),
        },
        "hillshade": {
            "on": h.on,
            "opacity": f64::from(h.opacity),
            "highlights": h.highlights,
            "azimuth": f64::from(h.azimuth),
            "altitude": f64::from(h.altitude),
            "exaggeration": f64::from(h.exaggeration),
            "color": rgb_json(h.color),
        },
    })
}

/// The smoothing of a raster. A raster of style version 2 has none, and it reads as the default.
fn read_smoothing(j: Option<&Json>) -> Smoothing {
    let d = Smoothing::default();
    let Some(m) = j.and_then(Json::as_object) else {
        return d;
    };
    let size = m
        .get("feature_size")
        .and_then(Json::as_f64)
        .map(|v| v as f32);
    Smoothing {
        on: m.get("on").and_then(Json::as_bool).unwrap_or(d.on),
        feature_size: match size {
            Some(v) if v.is_finite() => v.clamp(0.0, Smoothing::FEATURE_SIZE_MAX),
            _ => d.feature_size,
        },
    }
}

/// The hillshade of a raster. A raster of style version 2 has none, and it reads as the default.
fn read_hillshade(j: Option<&Json>) -> Hillshade {
    let d = Hillshade::default();
    let Some(m) = j.and_then(Json::as_object) else {
        return d;
    };
    let number = |k: &str, d: f32| m.get(k).and_then(Json::as_f64).map_or(d, |v| v as f32);
    let flag = |k: &str, d: bool| m.get(k).and_then(Json::as_bool).unwrap_or(d);
    Hillshade {
        on: flag("on", d.on),
        opacity: number("opacity", d.opacity),
        highlights: flag("highlights", d.highlights),
        azimuth: number("azimuth", d.azimuth),
        altitude: number("altitude", d.altitude),
        exaggeration: number("exaggeration", d.exaggeration),
        color: read_rgb(m.get("color")).unwrap_or(d.color),
    }
    .clamped()
}

/// The version of a style JSON text, or `None` if the text has none.
pub fn style_version(text: &str) -> Option<u64> {
    let root: Json = serde_json::from_str(text).ok()?;
    root.get("version").and_then(Json::as_u64)
}

fn read_line(j: Option<&Json>) -> Result<StopLine, StyleError> {
    let d = StopLine::default();
    let Some(j) = j else { return Ok(d) };
    let m = j
        .as_object()
        .ok_or_else(|| invalid("a stop line is not an object"))?;
    let number = |k: &str, d: f32| m.get(k).and_then(Json::as_f64).map_or(d, |v| v as f32);
    let flag = |k: &str, d: bool| m.get(k).and_then(Json::as_bool).unwrap_or(d);
    Ok(StopLine {
        on: flag("on", d.on),
        weight: number("weight", d.weight),
        color: match m.get("color") {
            None => d.color,
            c => read_rgb(c).ok_or_else(|| invalid("a stop line has no valid color"))?,
        },
        darker: flag("darker", d.darker),
        darken: number("darken", d.darken),
    })
}

/// The opacity and the blend mode of a raster or an image.
fn read_mix(m: &Map<String, Json>) -> (f32, BlendMode) {
    let opacity = m.get("opacity").and_then(Json::as_f64).unwrap_or(1.0) as f32;
    let opacity = if opacity.is_nan() {
        1.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    let blend = m
        .get("blend")
        .and_then(Json::as_str)
        .and_then(BlendMode::from_id)
        .unwrap_or_default();
    (opacity, blend)
}

fn read_raster(m: &Map<String, Json>) -> Result<RasterSymbolizer, StyleError> {
    let display = match m.get("display").and_then(Json::as_str) {
        Some("bands") => RasterDisplay::Bands,
        Some("gradient") => RasterDisplay::Gradient,
        Some("grey") => RasterDisplay::Grey,
        _ => return Err(invalid("a raster has no valid display")),
    };
    let (opacity, blend) = read_mix(m);
    let stops = m
        .get("stops")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("a raster has no stops"))?
        .iter()
        .map(|j| {
            let value = j
                .get("value")
                .and_then(Json::as_f64)
                .filter(|v| v.is_finite())
                .ok_or_else(|| invalid("a stop has no valid value"))?;
            Ok(Stop {
                value,
                line: read_line(j.get("line"))?,
            })
        })
        .collect::<Result<Vec<_>, StyleError>>()?;
    if stops.len() < 2 || stops.windows(2).any(|w| w[0].value >= w[1].value) {
        return Err(invalid("the stops of a raster are not in ascending order"));
    }
    let colors = m
        .get("colors")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("a raster has no colors"))?
        .iter()
        .map(|j| read_rgb(Some(j)).ok_or_else(|| invalid("a band has no valid color")))
        .collect::<Result<Vec<_>, _>>()?;
    if colors.len() + 1 != stops.len() {
        return Err(invalid("a raster needs one color fewer than stops"));
    }
    Ok(RasterSymbolizer {
        display,
        opacity,
        blend,
        stops,
        colors,
        lines: m.get("lines").and_then(Json::as_bool).unwrap_or(true),
        smoothing: read_smoothing(m.get("smoothing")),
        hillshade: read_hillshade(m.get("hillshade")),
    })
}
