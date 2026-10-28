use std::collections::BTreeMap;

use vale_store::GeometryKind;
use vale_style::expr::{Context, Filter, Property, Value};
use vale_style::{
    Color, Extent, ImageSymbolizer, LayerStyle, Paint, PointSymbolizer, Rule, STYLE_VERSION,
    ScatterSymbolizer, Stroke, StrokeKind, StrokeSymbolizer, Style, StyleError, Symbolizer,
    extent_of, scale_denominator,
};

fn river(kind: &str, flow: f64) -> BTreeMap<String, Value> {
    let mut a = BTreeMap::new();
    a.insert("kind".into(), Value::Text(kind.into()));
    a.insert("flow".into(), Value::Number(flow));
    a
}

fn paints(style: &Style, attrs: &BTreeMap<String, Value>, scale: f64) -> Vec<Paint> {
    let ctx = Context {
        attributes: attrs,
        scale,
        geometry: GeometryKind::Line,
    };
    let mut out = Vec::new();
    style.paints(&ctx, &mut out);
    out
}

fn stroke(color: Color, width: f64) -> Symbolizer {
    Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke::solid(color, width)))
}

const BLUE: Color = Color::rgb(0, 0, 255);
const RED: Color = Color::rgb(255, 0, 0);

fn two_rules() -> Style {
    let mut major = Rule::new("Major", vec![stroke(BLUE, 2.0)]);
    major.filter = Filter::new("kind = 'major'");
    let mut minor = Rule::new("Minor", vec![stroke(RED, 0.5)]);
    minor.filter = Filter::new("kind = 'minor'");
    minor.max_scale = Some(1_000_000.0);
    Style {
        rules: vec![major, minor],
    }
}

fn width_from_flow() -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.style.rules[0].symbolizers = vec![Symbolizer::Stroke(StrokeSymbolizer {
        color: Property::constant(BLUE),
        width: Property::expression("flow * 0.5", 1.0),
        kind: StrokeKind::Solid,
    })];
    style
}

#[test]
fn rules_draw_by_filter_and_scale_range() {
    let style = two_rules();
    let major = river("major", 1.0);
    let minor = river("minor", 1.0);
    assert_eq!(
        paints(&style, &major, 500_000.0),
        vec![Paint::Stroke(Stroke::solid(BLUE, 2.0))]
    );
    assert_eq!(
        paints(&style, &minor, 500_000.0),
        vec![Paint::Stroke(Stroke::solid(RED, 0.5))]
    );
    assert!(paints(&style, &minor, 1_000_000.0).is_empty());
    assert!(paints(&style, &river("other", 1.0), 500_000.0).is_empty());
    assert_eq!(style.visible(2_000_000.0).count(), 1);
}

#[test]
fn each_matching_rule_draws_in_list_order() {
    let mut style = Style {
        rules: vec![
            Rule::new("Casing", vec![stroke(RED, 3.0)]),
            Rule::new("Core", vec![stroke(BLUE, 1.0)]),
        ],
    };
    let got = paints(&style, &river("major", 1.0), 1.0);
    assert_eq!(got.len(), 2);
    assert!(matches!(got[1], Paint::Stroke(s) if s.color == BLUE));
    style.rules[0].min_scale = Some(10.0);
    assert_eq!(paints(&style, &river("major", 1.0), 1.0).len(), 1);
}

#[test]
fn a_line_width_comes_from_the_flow_attribute() {
    let style = width_from_flow();
    let width = |flow| match paints(&style.style, &river("major", flow), 1.0).remove(0) {
        Paint::Stroke(s) => s.width,
        p => panic!("{p:?}"),
    };
    assert_eq!(width(4.0), 2.0);
    assert_eq!(width(1.0), 0.5);
    let mut nothing = BTreeMap::new();
    nothing.insert("kind".into(), Value::Text("x".into()));
    assert!(matches!(
        paints(&style.style, &nothing, 1.0)[0],
        Paint::Stroke(s) if s.width == 1.0
    ));
}

#[test]
fn constant_paints_only_for_constant_rules() {
    for kind in [
        GeometryKind::Point,
        GeometryKind::Line,
        GeometryKind::Polygon,
    ] {
        let style = LayerStyle::default_for(kind, 2, &[]);
        assert!(style.is_simple());
        let constant = style.style.constant_paints(1.0).unwrap();
        assert_eq!(constant, paints(&style.style, &river("x", 1.0), 1.0));
    }
    assert!(two_rules().constant_paints(1.0).is_none());
    assert!(width_from_flow().style.constant_paints(1.0).is_none());
    assert!(!width_from_flow().is_simple());
}

#[test]
fn simple_controls_edit_the_first_rule() {
    let mut point = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    assert_eq!(point.point_radius(), 2.5);
    point.set_point_radius(4.0);
    point.set_fill(None);
    assert_eq!(point.point_radius(), 4.0);
    assert_eq!(point.fill(), None);
    assert_eq!(point.stroke().unwrap().width, 1.0);

    let mut polygon = LayerStyle::default_for(GeometryKind::Polygon, 0, &[]);
    polygon.set_fill(Some(RED));
    polygon.set_stroke(None);
    assert_eq!(polygon.fill(), Some(RED));
    assert_eq!(polygon.stroke(), None);
    assert_eq!(polygon.point_radius(), 0.0);
    polygon.set_stroke(Some(Stroke::solid(BLUE, 2.0)));
    assert_eq!(polygon.style.rules[0].symbolizers.len(), 2);
    assert!(matches!(
        polygon.style.rules[0].symbolizers[0],
        Symbolizer::Fill { .. }
    ));
}

#[test]
fn the_scale_denominator_of_a_view() {
    let one_to_one = scale_denominator(0.0254 / 72.0);
    assert!((one_to_one - 1.0).abs() < 1e-9);
    assert!((scale_denominator(352.777_777) - 1_000_000.0).abs() < 1.0);
}

#[test]
fn the_json_has_a_version() {
    let json = LayerStyle::default_for(GeometryKind::Line, 0, &[]).to_json();
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(root["version"], STYLE_VERSION);
    assert_eq!(STYLE_VERSION, 2);
}

#[test]
fn json_round_trips() {
    let fields = vec!["name".to_string()];
    let mut styles: Vec<LayerStyle> = [
        GeometryKind::Point,
        GeometryKind::Line,
        GeometryKind::Polygon,
    ]
    .into_iter()
    .map(|k| LayerStyle::default_for(k, 1, &fields))
    .collect();
    let mut rules = width_from_flow();
    rules.style.rules.extend(two_rules().rules);
    rules.style.rules[1].filter = Filter::new("kind = ");
    rules.label.priority_field = Some("flow".into());
    styles.push(rules);
    for style in styles {
        let json = style.to_json();
        let back = LayerStyle::from_json(&json).unwrap();
        assert_eq!(back, style);
        assert_eq!(back.to_json(), json);
    }
}

#[test]
fn a_wrong_expression_stays_in_the_json() {
    let mut style = width_from_flow();
    style.style.rules[0].filter = Filter::new("flow >");
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    let filter = &back.style.rules[0].filter;
    assert!(filter.error().is_some());
    assert_eq!(filter.source().unwrap().text(), "flow >");
}

#[test]
fn json_errors() {
    let good = LayerStyle::default_for(GeometryKind::Line, 0, &[]).to_json();
    let edit = |f: &dyn Fn(&mut serde_json::Map<String, serde_json::Value>)| {
        let mut root: serde_json::Value = serde_json::from_str(&good).unwrap();
        f(root.as_object_mut().unwrap());
        root.to_string()
    };
    let newer = edit(&|m| {
        m.insert("version".into(), 3.into());
    });
    assert_eq!(
        LayerStyle::from_json(&newer),
        Err(StyleError::Version(Some(3)))
    );
    let none = edit(&|m| {
        m.remove("version");
    });
    assert_eq!(LayerStyle::from_json(&none), Err(StyleError::Version(None)));
    assert!(matches!(
        LayerStyle::from_json("{"),
        Err(StyleError::Json(_))
    ));
    let unknown = good.replace("\"type\":\"stroke\"", "\"type\":\"glow\"");
    assert!(matches!(
        LayerStyle::from_json(&unknown),
        Err(StyleError::Invalid(_))
    ));
    let extra = edit(&|m| {
        m.insert("later".into(), true.into());
    });
    assert!(LayerStyle::from_json(&extra).is_ok());
}

fn colors() -> vale_style::RasterSymbolizer {
    use vale_store::{RasterDisplay, Stop, StopLine};
    let line = StopLine {
        on: true,
        weight: 1.5,
        color: [10, 20, 30],
        darker: true,
        darken: 0.25,
    };
    vale_style::RasterSymbolizer {
        display: RasterDisplay::Grey,
        opacity: 0.375,
        blend: vale_store::BlendMode::Overlay,
        stops: vec![
            Stop {
                value: -100.0,
                line: StopLine::default(),
            },
            Stop { value: 0.0, line },
            Stop {
                value: 250.5,
                line: StopLine::default(),
            },
        ],
        colors: vec![[1, 2, 3], [200, 210, 220]],
    }
}

#[test]
fn a_raster_style_round_trips() {
    let style = LayerStyle::raster(colors());
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!(back, style);
    assert_eq!(back.raster_symbolizer(), Some(&colors()));
}

#[test]
fn a_raster_symbolizer_draws_no_paint() {
    let style = LayerStyle::raster(colors());
    assert_eq!(style.style.constant_paints(1.0), Some(Vec::new()));
    assert!(paints(&style.style, &river("x", 1.0), 1.0).is_empty());
}

#[test]
fn set_raster_symbolizer_replaces_the_first() {
    let mut style = LayerStyle::raster(colors());
    let mut other = colors();
    other.opacity = 1.0;
    style.set_raster_symbolizer(other.clone());
    assert_eq!(style.style.rules.len(), 1);
    assert_eq!(style.raster_symbolizer(), Some(&other));
    let mut line = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    line.set_raster_symbolizer(other.clone());
    assert_eq!(line.style.rules.len(), 2);
    assert_eq!(line.raster_symbolizer(), Some(&other));
}

#[test]
fn wrong_raster_stops_do_not_load() {
    let mut unsorted = colors();
    unsorted.stops.swap(0, 1);
    let mut short = colors();
    short.colors.pop();
    for bad in [unsorted, short] {
        let json = LayerStyle::raster(bad).to_json();
        assert!(matches!(
            LayerStyle::from_json(&json),
            Err(StyleError::Invalid(_))
        ));
    }
}

#[test]
fn the_default_image_style_is_opaque_and_normal() {
    let style = LayerStyle::image();
    assert_eq!(
        style.image_symbolizer(),
        Some(&ImageSymbolizer {
            opacity: 1.0,
            blend: vale_store::BlendMode::Normal,
        })
    );
    assert_eq!(style.raster_symbolizer(), None);
    assert_eq!(style.style.constant_paints(1.0), Some(Vec::new()));
    assert!(!style.label.enabled);
}

#[test]
fn an_image_style_round_trips() {
    let mut style = LayerStyle::image();
    style.style.rules[0].symbolizers[0] = Symbolizer::Image(ImageSymbolizer {
        opacity: 0.5,
        blend: vale_store::BlendMode::Overlay,
    });
    let json = style.to_json();
    assert!(json.contains("\"type\":\"image\""));
    let back = LayerStyle::from_json(&json).unwrap();
    assert_eq!(back, style);
    assert_eq!(back.image_symbolizer().unwrap().opacity, 0.5);
}

#[test]
fn a_raster_style_without_a_blend_mode_blends_as_normal() {
    let json = LayerStyle::raster(colors()).to_json();
    assert!(json.contains("\"blend\":\"overlay\""));
    let old = json
        .replace(",\"blend\":\"overlay\"", "")
        .replace("\"blend\":\"overlay\",", "");
    assert!(!old.contains("blend"));
    let back = LayerStyle::from_json(&old).unwrap();
    assert_eq!(
        back.raster_symbolizer().unwrap().blend,
        vale_store::BlendMode::Normal
    );
    let unknown = json.replace("overlay", "glow");
    let back = LayerStyle::from_json(&unknown).unwrap();
    assert_eq!(
        back.raster_symbolizer().unwrap().blend,
        vale_store::BlendMode::Normal
    );
}

fn line_with(kind: StrokeKind) -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.style.rules[0].set_stroke(Some(Stroke {
        color: BLUE,
        width: 2.0,
        kind,
    }));
    style
}

#[test]
fn each_stroke_kind_round_trips() {
    for kind in [
        StrokeKind::Solid,
        StrokeKind::dashed(),
        StrokeKind::Dashed {
            dash: 6.5,
            gap: 1.5,
        },
        StrokeKind::Brush,
        StrokeKind::taper(),
        StrokeKind::Taper { start: 0.1 },
    ] {
        let style = line_with(kind);
        let json = style.to_json();
        let back = LayerStyle::from_json(&json).unwrap();
        assert_eq!(back, style);
        assert_eq!(back.style.rules[0].stroke().unwrap().kind, kind);
        assert_eq!(back.to_json(), json);
    }
}

#[test]
fn a_stroke_without_kind_reads_as_solid() {
    let base = line_with(StrokeKind::Solid).to_json();
    let read = |extra: &str| {
        let json = base.replace("\"width\":2.0", &format!("\"width\":2.0{extra}"));
        assert!(extra.is_empty() || json != base);
        let style = LayerStyle::from_json(&json).unwrap();
        style.style.rules[0].stroke().unwrap().kind
    };
    assert_eq!(read(""), StrokeKind::Solid);
    assert_eq!(read(",\"kind\":\"wavy\""), StrokeKind::Solid);
    assert_eq!(read(",\"kind\":\"dashed\""), StrokeKind::dashed());
    assert_eq!(
        read(",\"kind\":\"dashed\",\"gap\":3.0"),
        StrokeKind::Dashed {
            dash: 4.0,
            gap: 3.0
        }
    );
    assert_eq!(
        read(",\"kind\":\"taper\""),
        StrokeKind::Taper { start: 0.25 }
    );
    assert_eq!(read(",\"kind\":\"brush\""), StrokeKind::Brush);
}

#[test]
fn solid_json_is_unchanged() {
    assert_eq!(Stroke::solid(BLUE, 2.0).kind, StrokeKind::default());
    let json = line_with(StrokeKind::Solid).to_json();
    let expected = concat!(
        r##"{"label":{"color":"#2864a0","enabled":false,"field":null,"italic":true,"priority_field":null,"size":11.0},"##,
        r##""rules":[{"filter":"","name":"Default","symbolizers":[{"color":"#0000ff","type":"stroke","width":2.0}]}],"version":2}"##,
    );
    assert_eq!(json, expected);
}

#[test]
fn set_stroke_keeps_the_kind() {
    let mut rule = Rule::new("r", vec![stroke(BLUE, 1.0)]);
    for kind in [StrokeKind::dashed(), StrokeKind::Brush, StrokeKind::taper()] {
        let s = Stroke {
            color: RED,
            width: 1.5,
            kind,
        };
        rule.set_stroke(Some(s));
        assert_eq!(rule.stroke(), Some(s));
        assert_eq!(rule.symbolizers.len(), 1);
    }
}

fn tree_field(size: f64, angle: f64) -> BTreeMap<String, Value> {
    let mut a = BTreeMap::new();
    a.insert("size".into(), Value::Number(size));
    a.insert("angle".into(), Value::Number(angle));
    a.insert("tint".into(), Value::Text("#ff0000".into()));
    a
}

fn symbol_rule(symbolizer: Symbolizer) -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    style.style.rules[0].symbolizers = vec![symbolizer];
    style
}

fn symbol_styles() -> Vec<LayerStyle> {
    let constant = PointSymbolizer::constant("pine", 8.0);
    let tinted = PointSymbolizer {
        tint: Some(Property::constant(RED)),
        rotation: Property::constant(30.0),
        ..PointSymbolizer::constant("mountain", 12.0)
    };
    let from_attributes = PointSymbolizer {
        symbol: "tree".into(),
        size: Property::expression("size * 2", 6.0),
        rotation: Property::expression("angle", 0.0),
        tint: Some(Property::expression("tint", BLUE)),
    };
    let scatter = ScatterSymbolizer::new("tree", 6.0, 10.0);
    let scatter_from_attributes = ScatterSymbolizer {
        size: Property::expression("size", 4.0),
        spacing: Property::expression("size * 3", 12.0),
        size_jitter: 0.35,
        rotation_jitter: 15.0,
        tint: Some(Property::constant(BLUE)),
        ..ScatterSymbolizer::new("marsh", 5.0, 5.0)
    };
    vec![
        symbol_rule(Symbolizer::Symbol(constant)),
        symbol_rule(Symbolizer::Symbol(tinted)),
        symbol_rule(Symbolizer::Symbol(from_attributes)),
        symbol_rule(Symbolizer::Scatter(scatter)),
        symbol_rule(Symbolizer::Scatter(scatter_from_attributes)),
    ]
}

#[test]
fn symbols_and_scatters_round_trip() {
    for style in symbol_styles() {
        let json = style.to_json();
        let back = LayerStyle::from_json(&json).unwrap();
        assert_eq!(back, style);
        assert_eq!(back.to_json(), json);
    }
}

#[test]
fn symbol_json_has_its_form() {
    let style = symbol_rule(Symbolizer::Symbol(PointSymbolizer {
        rotation: Property::expression("angle", 0.0),
        ..PointSymbolizer::constant("pine", 8.0)
    }));
    let root: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    let s = &root["rules"][0]["symbolizers"][0];
    assert_eq!(s["type"], "symbol");
    assert_eq!(s["symbol"], "pine");
    assert_eq!(s["size"], 8.0);
    assert_eq!(s["rotation"]["expr"], "angle");
    assert!(s["tint"].is_null());
    let style = symbol_rule(Symbolizer::Scatter(ScatterSymbolizer {
        rotation_jitter: 15.0,
        tint: Some(Property::constant(RED)),
        ..ScatterSymbolizer::new("tree", 6.0, 10.0)
    }));
    let root: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    let s = &root["rules"][0]["symbolizers"][0];
    assert_eq!(s["type"], "scatter");
    assert_eq!(s["spacing"], 10.0);
    assert_eq!(s["size_jitter"], 0.2);
    assert_eq!(s["rotation_jitter"], 15.0);
    assert_eq!(s["tint"], "#ff0000");
}

#[test]
fn version_1_still_reads() {
    let v1 = concat!(
        r##"{"label":{"color":"#2864a0","enabled":false,"field":null,"italic":true,"priority_field":null,"size":11.0},"##,
        r##""rules":[{"filter":"","name":"Default","symbolizers":[{"color":"#0000ff","type":"stroke","width":2.0}]}],"version":1}"##,
    );
    let style = LayerStyle::from_json(v1).unwrap();
    assert_eq!(style, line_with(StrokeKind::Solid));
    let zero = v1.replace("\"version\":1", "\"version\":0");
    assert_eq!(
        LayerStyle::from_json(&zero),
        Err(StyleError::Version(Some(0)))
    );
}

fn point_paints(symbolizer: Symbolizer, attrs: &BTreeMap<String, Value>) -> Vec<Paint> {
    let ctx = Context {
        attributes: attrs,
        scale: 1.0,
        geometry: GeometryKind::Point,
    };
    let mut out = Vec::new();
    symbol_rule(symbolizer).style.paints(&ctx, &mut out);
    out
}

#[test]
fn a_symbol_takes_size_and_rotation_from_attributes() {
    let styles = symbol_styles();
    let symbolizer = styles[2].style.rules[0].symbolizers[0].clone();
    assert!(!symbolizer.is_constant());
    assert_eq!(
        point_paints(symbolizer.clone(), &tree_field(5.0, 45.0)),
        vec![Paint::Symbol {
            symbol: "tree".into(),
            size: 10.0,
            rotation: 45.0,
            tint: Some(RED),
        }]
    );
    assert_eq!(
        point_paints(symbolizer, &BTreeMap::new()),
        vec![Paint::Symbol {
            symbol: "tree".into(),
            size: 6.0,
            rotation: 0.0,
            tint: Some(BLUE),
        }]
    );
    let constant = &styles[0].style;
    assert!(constant.rules[0].symbolizers[0].is_constant());
    assert_eq!(
        constant.constant_paints(1.0),
        Some(vec![Paint::Symbol {
            symbol: "pine".into(),
            size: 8.0,
            rotation: 0.0,
            tint: None,
        }])
    );
}

#[test]
fn a_scatter_keeps_spacing_at_least_size() {
    let scatter = |size: f64, spacing: f64, size_jitter: f64, rotation_jitter: f64| {
        Symbolizer::Scatter(ScatterSymbolizer {
            size_jitter,
            rotation_jitter,
            ..ScatterSymbolizer::new("tree", size, spacing)
        })
    };
    let paint = |s: Symbolizer| point_paints(s, &BTreeMap::new()).remove(0);
    assert_eq!(
        paint(scatter(6.0, 2.0, 0.9, 400.0)),
        Paint::Scatter {
            symbol: "tree".into(),
            size: 6.0,
            spacing: 6.0,
            size_jitter: 0.5,
            rotation_jitter: 180.0,
            tint: None,
        }
    );
    assert_eq!(
        paint(scatter(6.0, 9.0, -1.0, -5.0)),
        Paint::Scatter {
            symbol: "tree".into(),
            size: 6.0,
            spacing: 9.0,
            size_jitter: 0.0,
            rotation_jitter: 0.0,
            tint: None,
        }
    );
    let from_attributes = symbol_styles()[4].style.rules[0].symbolizers[0].clone();
    let Paint::Scatter { size, spacing, .. } = paint_with(from_attributes, 4.0) else {
        panic!("a scatter")
    };
    assert_eq!((size, spacing), (4.0, 12.0));
    let constant = symbol_rule(scatter(6.0, 2.0, 0.2, 0.0));
    let Some(paints) = constant.style.constant_paints(1.0) else {
        panic!("constant")
    };
    assert!(matches!(paints[0], Paint::Scatter { spacing: 6.0, .. }));
}

fn paint_with(s: Symbolizer, size: f64) -> Paint {
    point_paints(s, &tree_field(size, 0.0)).remove(0)
}

fn fill(color: Color) -> Symbolizer {
    Symbolizer::Fill {
        color: Property::constant(color),
    }
}

#[test]
fn set_scatter_goes_after_the_fill_and_before_the_stroke() {
    let tree = ScatterSymbolizer::new("tree", 6.0, 10.0);
    let mut rule = Rule::new("r", vec![fill(RED), stroke(BLUE, 1.0), stroke(RED, 0.5)]);
    rule.set_scatter(Some(tree.clone()));
    assert!(matches!(rule.symbolizers[0], Symbolizer::Fill { .. }));
    assert!(matches!(rule.symbolizers[1], Symbolizer::Scatter(_)));
    assert!(matches!(rule.symbolizers[2], Symbolizer::Stroke(_)));
    assert_eq!(rule.scatter(), Some(&tree));
    let pine = ScatterSymbolizer::new("pine", 4.0, 4.0);
    rule.set_scatter(Some(pine.clone()));
    assert_eq!(rule.symbolizers.len(), 4);
    assert_eq!(rule.scatter(), Some(&pine));
    rule.set_scatter(None);
    assert_eq!(rule.scatter(), None);
    assert_eq!(rule.symbolizers.len(), 3);

    let mut strokes = Rule::new("r", vec![stroke(BLUE, 1.0)]);
    strokes.set_scatter(Some(tree.clone()));
    assert!(matches!(strokes.symbolizers[0], Symbolizer::Scatter(_)));
    let mut fill_only = Rule::new("r", vec![fill(RED)]);
    fill_only.set_scatter(Some(tree));
    assert!(matches!(fill_only.symbolizers[1], Symbolizer::Scatter(_)));
}

#[test]
fn set_symbol_replaces_a_marker() {
    let mut style = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    let rule = &mut style.style.rules[0];
    rule.symbolizers.push(stroke(BLUE, 1.0));
    let pine = PointSymbolizer::constant("pine", 8.0);
    rule.set_symbol(Some(pine.clone()));
    assert_eq!(rule.symbolizers.len(), 2);
    assert!(matches!(rule.symbolizers[0], Symbolizer::Symbol(_)));
    assert_eq!(rule.symbol(), Some(&pine));
    let palm = PointSymbolizer::constant("palm", 10.0);
    rule.set_symbol(Some(palm.clone()));
    assert_eq!(rule.symbol(), Some(&palm));
    assert_eq!(rule.symbolizers.len(), 2);
    rule.set_symbol(None);
    assert_eq!(rule.symbol(), None);
    assert_eq!(rule.symbolizers.len(), 1);
    assert!(matches!(rule.symbolizers[0], Symbolizer::Stroke(_)));
    rule.set_symbol(Some(pine));
    assert!(matches!(rule.symbolizers[1], Symbolizer::Symbol(_)));
}

#[test]
fn the_extent_of_symbols() {
    let symbol = Paint::Symbol {
        symbol: "pine".into(),
        size: 10.0,
        rotation: 45.0,
        tint: None,
    };
    let marker = Paint::Marker {
        radius: 3.0,
        fill: None,
        stroke: Some(Stroke::solid(RED, 1.5)),
    };
    let scatter = Paint::Scatter {
        symbol: "tree".into(),
        size: 40.0,
        spacing: 40.0,
        size_jitter: 0.2,
        rotation_jitter: 0.0,
        tint: None,
    };
    assert_eq!(
        extent_of(&[marker, symbol, scatter.clone()]),
        Extent {
            stroke_width: 1.5,
            marker_radius: 5.0,
        }
    );
    assert_eq!(extent_of(&[scatter]), Extent::default());
}

#[test]
fn add_rule_tints_a_copied_symbol() {
    let mut style = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    style.style.rules[0].set_symbol(Some(PointSymbolizer {
        tint: Some(Property::constant(Color::rgb(0x2f, 0x5a, 0x2f))),
        ..PointSymbolizer::constant("pine", 8.0)
    }));
    style.add_rule(GeometryKind::Point);
    style.add_rule(GeometryKind::Point);
    let tints: Vec<_> = style
        .style
        .rules
        .iter()
        .map(|r| {
            let s = r.symbol().unwrap();
            assert_eq!(s.symbol, "pine");
            s.tint.as_ref().unwrap().fallback
        })
        .collect();
    assert_ne!(tints[0], tints[1]);
    assert_ne!(tints[1], tints[2]);
    assert_ne!(tints[0], tints[2]);

    let mut plain = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    plain.style.rules[0].set_symbol(Some(PointSymbolizer::constant("city", 8.0)));
    plain.add_rule(GeometryKind::Point);
    assert_eq!(
        plain.style.rules[1].symbol(),
        Some(&PointSymbolizer::constant("city", 8.0))
    );
}
