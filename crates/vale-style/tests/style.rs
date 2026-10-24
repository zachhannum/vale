use std::collections::BTreeMap;

use vale_store::GeometryKind;
use vale_style::expr::{Context, Filter, Property, Value};
use vale_style::{
    Color, LayerStyle, Paint, Rule, STYLE_VERSION, Stroke, StrokeSymbolizer, Style, StyleError,
    Symbolizer, scale_denominator,
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
    Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke { color, width }))
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
        vec![Paint::Stroke(Stroke {
            color: BLUE,
            width: 2.0
        })]
    );
    assert_eq!(
        paints(&style, &minor, 500_000.0),
        vec![Paint::Stroke(Stroke {
            color: RED,
            width: 0.5
        })]
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
    let width = |flow| match paints(&style.style, &river("major", flow), 1.0)[0] {
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
    polygon.set_stroke(Some(Stroke {
        color: BLUE,
        width: 2.0,
    }));
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
    assert_eq!(STYLE_VERSION, 1);
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
        m.insert("version".into(), 2.into());
    });
    assert_eq!(
        LayerStyle::from_json(&newer),
        Err(StyleError::Version(Some(2)))
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
