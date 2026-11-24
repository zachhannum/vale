use std::collections::BTreeMap;

use vale_store::{BlendMode, GeometryKind, LayerId};
use vale_style::expr::{Context, Filter, Property, Value};
use vale_style::{
    Color, DashPattern, DashPreset, Extent, HAIRLINE, Hatch, ImageSymbolizer, LayerStyle,
    MarkerShape, Mask, Paint, PointSymbolizer, Rule, STYLE_VERSION, ScatterSymbolizer, Stroke,
    StrokeKind, StrokeSymbolizer, Style, StyleError, Symbolizer, Weights, extent_of,
    scale_denominator,
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
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
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
        mask: None,
    }
}

fn width_from_flow() -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.style.rules[0].symbolizers = vec![Symbolizer::Stroke(StrokeSymbolizer {
        color: Property::constant(BLUE),
        width: Property::expression("flow * 0.5", 1.0),
        kind: StrokeKind::Solid,
        apply_mask: true,
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
        mask: None,
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
        let constant: Vec<Paint> = style
            .style
            .constant_paints(1.0)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
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
    assert_eq!(STYLE_VERSION, 10);
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
    rules.label.font = Some("Noto Serif".into());
    styles.push(rules);
    for style in styles {
        let json = style.to_json();
        let back = LayerStyle::from_json(&json).unwrap();
        assert_eq!(back, style);
        assert_eq!(back.to_json(), json);
    }
}

#[test]
fn a_mask_round_trips() {
    let mut style = LayerStyle::default_for(GeometryKind::Polygon, 0, &[]);
    for mask in [
        Mask::land(LayerId(3)),
        Mask::Raster {
            layer: LayerId(4),
            min: None,
            max: Some(-20.0),
            offset: -1.5,
        },
        Mask::Polygons { layer: LayerId(7) },
        Mask::markers(LayerId(8)),
        Mask::Markers {
            layer: LayerId(9),
            gap: 0.0,
        },
    ] {
        style.style.mask = Some(mask);
        let back = LayerStyle::from_json(&style.to_json()).unwrap();
        assert_eq!(back.style.mask, Some(mask));
    }
}

#[test]
fn a_style_of_version_5_has_no_mask() {
    let mut root: serde_json::Value =
        serde_json::from_str(&LayerStyle::default_for(GeometryKind::Polygon, 0, &[]).to_json())
            .unwrap();
    root["version"] = 5.into();
    let style = LayerStyle::from_json(&root.to_string()).unwrap();
    assert_eq!(style.style.mask, None);
}

#[test]
fn a_raster_mask_without_an_offset_has_an_offset_of_0() {
    let mut style = LayerStyle::default_for(GeometryKind::Polygon, 0, &[]);
    style.style.mask = Some(Mask::land(LayerId(3)));
    let json = style.to_json();
    assert!(!json.contains("offset"));
    let back = LayerStyle::from_json(&json).unwrap();
    assert_eq!(back.style.mask, Some(Mask::land(LayerId(3))));
}

#[test]
fn a_markers_mask_has_its_layer() {
    let mask = Mask::markers(LayerId(5));
    assert_eq!(mask.layer(), LayerId(5));
    assert!(!mask.contains(0.0));
}

#[test]
fn a_markers_mask_without_a_gap_reads_the_default_gap() {
    let mut root: serde_json::Value =
        serde_json::from_str(&LayerStyle::default_for(GeometryKind::Line, 0, &[]).to_json())
            .unwrap();
    root["mask"] = serde_json::json!({ "kind": "markers", "layer": 2 });
    let style = LayerStyle::from_json(&root.to_string()).unwrap();
    assert_eq!(
        style.style.mask,
        Some(Mask::Markers {
            layer: LayerId(2),
            gap: Mask::DEFAULT_GAP
        })
    );
}

#[test]
fn a_raster_mask_holds_its_range() {
    let land = Mask::land(LayerId(1));
    assert!(land.contains(0.0));
    assert!(land.contains(4000.0));
    assert!(!land.contains(-0.5));
    let shelf = Mask::Raster {
        layer: LayerId(1),
        min: Some(-200.0),
        max: Some(0.0),
        offset: 0.0,
    };
    assert!(shelf.contains(-200.0));
    assert!(!shelf.contains(0.0));
    assert!(!Mask::Polygons { layer: LayerId(1) }.contains(0.0));
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
        m.insert("version".into(), 11.into());
    });
    assert_eq!(
        LayerStyle::from_json(&newer),
        Err(StyleError::Version(Some(11)))
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
        names: Vec::new(),
        lines: false,
        smoothing: vale_store::Smoothing {
            on: false,
            feature_size: 3.5,
        },
        hillshade: vale_store::Hillshade {
            on: true,
            azimuth: 90.0,
            color: [5, 6, 7],
            ..Default::default()
        },
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
fn raster_band_names_survive_json() {
    let named = vale_style::RasterSymbolizer {
        names: vec!["Sea \"deep\"".into(), String::new()],
        ..colors()
    };
    let style = LayerStyle::raster(named.clone());
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!(back.raster_symbolizer(), Some(&named));
}

#[test]
fn a_raster_style_without_names_reads_and_writes_unchanged() {
    let json = LayerStyle::raster(colors()).to_json();
    assert!(!json.contains("names"));
    let back = LayerStyle::from_json(&json).unwrap();
    assert!(back.raster_symbolizer().unwrap().names.is_empty());
    assert_eq!(back.to_json(), json);
    // Names that are all empty are no names.
    let blank = vale_style::RasterSymbolizer {
        names: vec![String::new(); 2],
        ..colors()
    };
    assert_eq!(LayerStyle::raster(blank).to_json(), json);
}

#[test]
fn band_names_of_the_wrong_length_are_dropped() {
    let named = vale_style::RasterSymbolizer {
        names: vec!["Sea".into(), "Land".into()],
        ..colors()
    };
    let json = LayerStyle::raster(named).to_json();
    assert!(json.contains("\"names\":[\"Sea\",\"Land\"]"));
    for bad in ["[\"Sea\"]", "[\"Sea\",7]", "\"Sea\""] {
        let text = json.replace("[\"Sea\",\"Land\"]", bad);
        let back = LayerStyle::from_json(&text).unwrap();
        assert!(back.raster_symbolizer().unwrap().names.is_empty(), "{bad}");
    }
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
        StrokeKind::Dashed(DashPattern::new(&[6.5, 1.5], false).unwrap()),
        StrokeKind::Dashed(DashPattern::DASH_DOT),
        StrokeKind::Dashed(
            DashPattern::new(&[3.0, 1.0, 0.0, 1.0, 2.0, 1.0, 0.0, 4.0], true).unwrap(),
        ),
        StrokeKind::Brush,
        StrokeKind::taper(),
        StrokeKind::Taper { start: 0.1 },
        StrokeKind::flow(),
        StrokeKind::Flow { min: 0.35 },
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
    assert_eq!(read(",\"kind\":\"flow\""), StrokeKind::Flow { min: 0.25 });
    assert_eq!(read(",\"kind\":\"dashed\""), StrokeKind::dashed());
    let widths = |l: &[f64]| StrokeKind::Dashed(DashPattern::new(l, false).unwrap());
    assert_eq!(
        read(",\"kind\":\"dashed\",\"gap\":3.0"),
        widths(&[2.0, 1.5])
    );
    assert_eq!(
        read(",\"kind\":\"dashed\",\"dash\":6.0,\"gap\":3.0"),
        widths(&[3.0, 1.5])
    );
    assert_eq!(
        read(",\"kind\":\"dashed\",\"pattern\":[6.0,2.5,0.0,2.5],\"round\":true"),
        StrokeKind::Dashed(DashPattern::DASH_DOT)
    );
    assert_eq!(
        read(",\"kind\":\"dashed\",\"pattern\":[6.0,0.0]"),
        StrokeKind::dashed()
    );
    assert_eq!(
        read(",\"kind\":\"dashed\",\"pattern\":[6.0,2.0,1.0]"),
        StrokeKind::dashed()
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
        r##"{"label":{"color":"#2864a0","enabled":false,"font":null,"italic":true,"priority_field":null,"size":11.0,"text":""},"##,
        r##""rules":[{"filter":"","name":"Default","symbolizers":[{"color":"#0000ff","type":"stroke","width":2.0}]}],"version":10}"##,
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
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
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
        Some(vec![(
            Paint::Symbol {
                symbol: "pine".into(),
                size: 8.0,
                rotation: 0.0,
                tint: None,
            },
            false
        )])
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
    assert!(matches!(paints[0].0, Paint::Scatter { spacing: 6.0, .. }));
}

fn paint_with(s: Symbolizer, size: f64) -> Paint {
    point_paints(s, &tree_field(size, 0.0)).remove(0)
}

fn fill(color: Color) -> Symbolizer {
    Symbolizer::Fill {
        color: Property::constant(color),
        apply_mask: true,
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
        shape: MarkerShape::Circle,
        fill: None,
        hatch: None,
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

#[test]
fn a_raster_style_of_version_2_reads_the_default_lines_smoothing_and_hillshade() {
    let text = LayerStyle::raster(colors()).to_json();
    let mut root: serde_json::Value = serde_json::from_str(&text).unwrap();
    root["version"] = 2.into();
    let raster = &mut root["rules"][0]["symbolizers"][0];
    for key in ["lines", "smoothing", "hillshade"] {
        raster.as_object_mut().unwrap().remove(key);
    }
    let old = root.to_string();
    assert_eq!(vale_style::style_version(&old), Some(2));
    let style = LayerStyle::from_json(&old).unwrap();
    let r = style.raster_symbolizer().unwrap();
    assert!(r.lines);
    assert_eq!(r.smoothing, vale_store::Smoothing::default());
    assert_eq!(r.hillshade, vale_store::Hillshade::default());
    assert_eq!(
        vale_style::style_version(&style.to_json()),
        Some(STYLE_VERSION)
    );
}

fn marker_rule(style: &LayerStyle) -> &Rule {
    &style.style.rules[0]
}

#[test]
fn an_old_marker_reads_as_a_circle_with_no_hatch() {
    let json = LayerStyle::default_for(GeometryKind::Point, 0, &[]).to_json();
    let mut root: serde_json::Value = serde_json::from_str(&json).unwrap();
    root["version"] = 4.into();
    let marker = root["rules"][0]["symbolizers"][0].as_object_mut().unwrap();
    assert_eq!(marker["shape"], "circle");
    assert!(marker["hatch"].is_null());
    marker.remove("shape");
    marker.remove("hatch");
    let style = LayerStyle::from_json(&root.to_string()).unwrap();
    let rule = marker_rule(&style);
    assert_eq!(rule.marker_shape(), Some(MarkerShape::Circle));
    assert_eq!(rule.marker_hatch(), None);
    assert_eq!(rule.marker_fill_kind(), Some(vale_store::FillKind::Solid));
}

#[test]
fn a_square_with_a_cross_hatch_round_trips() {
    let mut style = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    let rule = &mut style.style.rules[0];
    rule.set_marker_shape(MarkerShape::Square);
    rule.set_marker_hatch(Some(Hatch {
        cross: true,
        angle: 30.0,
        spacing: 3.5,
        line_width: 0.25,
    }));
    let json = style.to_json();
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    let marker = &root["rules"][0]["symbolizers"][0];
    assert_eq!(marker["shape"], "square");
    assert_eq!(marker["hatch"]["kind"], "cross_hatch");
    assert_eq!(LayerStyle::from_json(&json).unwrap(), style);
}

#[test]
fn an_unknown_marker_shape_is_an_error() {
    let json = LayerStyle::default_for(GeometryKind::Point, 0, &[])
        .to_json()
        .replace("\"shape\":\"circle\"", "\"shape\":\"hexagon\"");
    assert!(matches!(
        LayerStyle::from_json(&json),
        Err(StyleError::Invalid(_))
    ));
}

#[test]
fn the_extent_of_a_square_marker_reaches_its_corners() {
    let square = Paint::Marker {
        radius: 5.0,
        shape: MarkerShape::Square,
        fill: Some(RED),
        hatch: None,
        stroke: None,
    };
    let e = extent_of(&[square]);
    assert!((e.marker_radius - 5.0 * std::f64::consts::SQRT_2).abs() < 1e-9);
}

#[test]
fn each_outline_fits_its_reach() {
    for shape in MarkerShape::ALL {
        let outline = shape.unit_outline();
        assert!(outline.len() >= 3);
        let far = outline.iter().map(|(x, y)| x.hypot(*y)).fold(0.0, f64::max);
        assert!((far - shape.reach()).abs() < 1e-9, "{shape:?}");
        assert_eq!(MarkerShape::from_id(shape.id()), Some(shape));
    }
    let triangle = MarkerShape::Triangle.unit_outline();
    assert_eq!(triangle[0], (0.0, -1.0));
}

#[test]
fn the_marker_fill_kind_changes_the_fill_and_the_hatch() {
    use vale_store::FillKind;
    let mut style = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    let rule = &mut style.style.rules[0];
    let color = rule.fill().unwrap();
    rule.set_marker_fill_kind(FillKind::Hatch);
    assert_eq!(rule.marker_fill_kind(), Some(FillKind::Hatch));
    assert_eq!(rule.marker_hatch(), Some(Hatch::default()));
    assert_eq!(rule.fill(), Some(color));
    rule.set_marker_hatch(Some(Hatch {
        spacing: 4.0,
        ..Hatch::default()
    }));
    rule.set_marker_fill_kind(FillKind::CrossHatch);
    let hatch = rule.marker_hatch().unwrap();
    assert!(hatch.cross);
    assert_eq!(hatch.spacing, 4.0);
    rule.set_marker_fill_kind(FillKind::Solid);
    assert_eq!(rule.marker_hatch(), None);
    assert_eq!(rule.fill(), Some(color));
    rule.set_marker_fill_kind(FillKind::None);
    assert_eq!(rule.fill(), None);
    assert_eq!(rule.marker_fill_kind(), Some(FillKind::None));
    rule.set_marker_fill_kind(FillKind::Hatch);
    assert!(rule.fill().is_some());
    assert_eq!(rule.marker_hatch(), Some(Hatch::default()));

    let mut lines = Rule::new("Lines", vec![]);
    assert_eq!(lines.marker_fill_kind(), None);
    lines.set_marker_fill_kind(FillKind::Hatch);
    assert!(lines.symbolizers.is_empty());
}

#[test]
fn label_class_rename_changes_text_filter_and_priority() {
    let mut c = vale_style::LabelClass::new("Cities", GeometryKind::Point, 0).unwrap();
    c.text = "concat(name, ' ' || pop)".to_owned();
    c.filter = "pop > 1000".to_owned();
    c.priority_field = Some("pop".to_owned());
    c.rename_field("pop", "people");
    assert_eq!(c.text, "concat(name, ' ' || people)");
    assert_eq!(c.filter, "people > 1000");
    assert_eq!(c.priority_field.as_deref(), Some("people"));
}

#[test]
fn layer_style_rename_changes_rules_and_label() {
    let fields = vec!["name".to_owned()];
    let mut s = LayerStyle::default_for(GeometryKind::Point, 0, &fields);
    s.label.priority_field = Some("pop".to_owned());
    s.label.text = "concat(name, ' ' || pop)".to_owned();
    let rule = &mut s.style.rules[0];
    rule.filter = Filter::new("pop > 10");
    rule.symbolizers.push(Symbolizer::Marker {
        radius: Property::expression("sqrt(pop)", 2.0),
        shape: MarkerShape::Circle,
        fill: None,
        hatch: None,
        stroke: None,
    });
    s.rename_field("pop", "people");
    s.rename_field("name", "title");
    let rule = &s.style.rules[0];
    assert_eq!(rule.filter.source().unwrap().text(), "people > 10");
    let Some(Symbolizer::Marker { radius, .. }) = rule.symbolizers.last() else {
        panic!("not a marker");
    };
    assert_eq!(radius.source().unwrap().text(), "sqrt(people)");
    assert_eq!(s.label.priority_field.as_deref(), Some("people"));
    assert_eq!(s.label.text, "concat(title, ' ' || people)");
}

#[test]
fn a_version_6_label_field_reads_as_text() {
    let v6 = concat!(
        r##"{"label":{"color":"#2864a0","enabled":true,"field":"Name 2","italic":true,"priority_field":null,"size":11.0},"##,
        r##""rules":[{"filter":"","name":"Default","symbolizers":[{"color":"#0000ff","type":"stroke","width":2.0}]}],"version":6}"##,
    );
    let style = LayerStyle::from_json(v6).unwrap();
    assert_eq!(style.label.text, "\"Name 2\"");
    let plain = LayerStyle::from_json(&v6.replace("Name 2", "name")).unwrap();
    assert_eq!(plain.label.text, "name");
    let none = LayerStyle::from_json(&v6.replace("\"Name 2\"", "null")).unwrap();
    assert_eq!(none.label.text, "");
}

#[test]
fn label_text_round_trips() {
    let mut s = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    s.label.text = "concat(name, ' (' || round(elevation) || ' m)')".to_owned();
    let json = s.to_json();
    assert!(json.contains("\"version\":10"));
    assert_eq!(LayerStyle::from_json(&json).unwrap(), s);
}

fn masked_polygons(fill_mask: bool, stroke_mask: bool) -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Polygon, 0, &[]);
    style.style.rules = vec![Rule::new("r", vec![fill(RED), stroke(BLUE, 1.0)])];
    style.style.rules[0].set_fill_applies_mask(fill_mask);
    style.style.rules[0].set_stroke_applies_mask(stroke_mask);
    style.style.mask = Some(Mask::land(LayerId(3)));
    style
}

fn polygon(attrs: &BTreeMap<String, Value>) -> Context<'_> {
    Context {
        attributes: attrs,
        scale: 1.0,
        geometry: GeometryKind::Polygon,
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
    }
}

#[test]
fn apply_mask_off_round_trips() {
    let style = masked_polygons(false, false);
    let json = style.to_json();
    assert_eq!(json.matches("\"apply_mask\":false").count(), 2);
    let back = LayerStyle::from_json(&json).unwrap();
    assert_eq!(back, style);
    assert_eq!(back.style.rules[0].fill_applies_mask(), Some(false));
    assert_eq!(back.style.rules[0].stroke_applies_mask(), Some(false));
}

#[test]
fn apply_mask_on_writes_no_key() {
    let json = masked_polygons(true, true).to_json();
    assert!(!json.contains("apply_mask"));
}

#[test]
fn a_style_of_version_7_applies_the_mask() {
    let mut root: serde_json::Value =
        serde_json::from_str(&masked_polygons(true, true).to_json()).unwrap();
    root["version"] = 7.into();
    let style = LayerStyle::from_json(&root.to_string()).unwrap();
    assert_eq!(style.style.rules[0].fill_applies_mask(), Some(true));
    assert_eq!(style.style.rules[0].stroke_applies_mask(), Some(true));
}

#[test]
fn constant_paints_flag_the_paints_that_the_mask_cuts() {
    let flags = |s: &Style| -> Vec<bool> {
        s.constant_paints(1.0)
            .unwrap()
            .into_iter()
            .map(|(_, cut)| cut)
            .collect()
    };
    let mut style = masked_polygons(true, false).style;
    style.rules[0]
        .symbolizers
        .push(Symbolizer::Scatter(ScatterSymbolizer::new(
            "tree", 6.0, 10.0,
        )));
    assert_eq!(flags(&style), [true, false, true]);
    style.mask = None;
    assert_eq!(flags(&style), [false, false, false]);
}

#[test]
fn a_label_is_in_the_mask_with_a_masked_fill_or_no_fill() {
    let attrs = BTreeMap::new();
    let ctx = polygon(&attrs);
    assert!(masked_polygons(true, false).style.label_in_mask(&ctx));
    assert!(!masked_polygons(false, true).style.label_in_mask(&ctx));
    let mut no_fill = masked_polygons(true, true).style;
    no_fill.rules[0].set_fill(None);
    assert!(no_fill.label_in_mask(&ctx));
}

#[test]
fn set_fill_and_set_stroke_keep_apply_mask() {
    let mut rule = masked_polygons(false, false).style.rules.remove(0);
    rule.set_fill(Some(BLUE));
    rule.set_stroke(Some(Stroke::solid(RED, 3.0)));
    assert_eq!(rule.fill(), Some(BLUE));
    assert_eq!(rule.fill_applies_mask(), Some(false));
    assert_eq!(rule.stroke_applies_mask(), Some(false));
    rule.set_fill(None);
    assert_eq!(rule.fill_applies_mask(), None);
    rule.set_fill(Some(RED));
    assert_eq!(rule.fill_applies_mask(), Some(true));
}

#[test]
fn a_width_expression_stays_after_an_edit_of_the_stroke() {
    let mut rule = Rule::new(
        "River",
        vec![Symbolizer::Stroke(StrokeSymbolizer::constant(
            Stroke::solid(Color::rgb(0, 0, 200), 1.0),
        ))],
    );
    assert_eq!(rule.stroke_width_expression(), None);
    rule.set_stroke_width_expression(Some("sqrt(flow('Height')) / 100"));
    assert_eq!(
        rule.stroke_width_expression(),
        Some("sqrt(flow('Height')) / 100")
    );
    // A new color, width, or line type keeps the expression, and the width is its fallback.
    let mut stroke = rule.stroke().unwrap();
    stroke.width = 2.0;
    stroke.kind = StrokeKind::taper();
    rule.set_stroke(Some(stroke));
    assert_eq!(
        rule.stroke_width_expression(),
        Some("sqrt(flow('Height')) / 100")
    );
    assert_eq!(rule.stroke().unwrap().width, 2.0);
    assert_eq!(rule.stroke().unwrap().kind, StrokeKind::taper());
    rule.set_stroke_width_expression(None);
    assert_eq!(rule.stroke_width_expression(), None);
    assert_eq!(rule.stroke().unwrap().width, 2.0);
}

#[test]
fn a_flow_width_follows_the_square_root_of_the_area() {
    let stroke = Stroke {
        color: BLUE,
        width: 8.0,
        kind: StrokeKind::Flow { min: 0.25 },
    };
    assert_eq!(stroke.width_at(0.0, (0.0, 4.0)), 2.0);
    assert_eq!(stroke.width_at(4.0, (0.0, 4.0)), 8.0);
    // A quarter of the largest area gets half of the part of the width that grows.
    assert_eq!(stroke.width_at(1.0, (0.0, 4.0)), 2.0 + 0.5 * 6.0);
    // More than the largest area, and no largest area.
    assert_eq!(stroke.width_at(9.0, (0.0, 4.0)), 8.0);
    assert_eq!(stroke.width_at(1.0, (0.0, 0.0)), 2.0);
    // The width grows from the floor: a river at the floor has the smallest width.
    assert_eq!(stroke.width_at(1.0, (1.0, 5.0)), 2.0);
    assert_eq!(stroke.width_at(0.5, (1.0, 5.0)), 2.0);
    assert_eq!(stroke.width_at(2.0, (1.0, 5.0)), 2.0 + 0.5 * 6.0);
    assert_eq!(stroke.width_at(1.0, (2.0, 1.0)), 2.0);
    // Another kind keeps its width.
    assert_eq!(Stroke::solid(BLUE, 3.0).width_at(1.0, (0.0, 4.0)), 3.0);
    assert!(StrokeKind::flow().along_line() && StrokeKind::taper().along_line());
    assert!(!StrokeKind::Solid.along_line());
}

#[test]
fn the_height_layer_of_flow_round_trips_and_follows_a_rename() {
    let mut style = line_with(StrokeKind::flow());
    assert!(!style.to_json().contains("flow_height"));
    style.flow_height = Some("Ridges".into());
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!(back.flow_height.as_deref(), Some("Ridges"));
    assert_eq!(back, style);
    style.rename_raster("Ridges", "Highlands");
    assert_eq!(style.flow_height.as_deref(), Some("Highlands"));
    style.rename_raster("Other", "Name");
    assert_eq!(style.flow_height.as_deref(), Some("Highlands"));
}

#[test]
fn the_label_weights_of_a_layer_round_trip_with_defaults_for_missing_keys() {
    let mut style = line_with(StrokeKind::Solid);
    assert_eq!(
        style.weights,
        Weights {
            point: 100.0,
            line: 30.0,
            fill: 0.0,
            outline: 0.0
        }
    );
    assert!(!style.to_json().contains("weights"));
    style.weights = Weights {
        point: 40.0,
        line: 100.0,
        fill: 15.0,
        outline: 60.0,
    };
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!(back.weights, style.weights);
    assert_eq!(back, style);

    // A missing key reads as its default.
    let mut json: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    json["weights"] = serde_json::json!({ "line": 0.0, "outline": 5.0 });
    let back = LayerStyle::from_json(&json.to_string()).unwrap();
    let want = Weights {
        line: 0.0,
        outline: 5.0,
        ..Weights::default()
    };
    assert_eq!(back.weights, want);
    assert!(want.free(GeometryKind::Line) && !want.free(GeometryKind::Polygon));
}

#[test]
fn a_dash_pattern_knows_its_preset_and_keeps_valid() {
    for p in DashPreset::ALL {
        assert_eq!(p.pattern().preset(), Some(p));
    }
    let dash_dot = DashPattern::DASH_DOT;
    let custom = DashPattern::parse("5, 2.5 0 2.5", true).unwrap();
    assert_eq!(custom.lengths(), &[5.0, 2.5, 0.0, 2.5]);
    assert_eq!(custom.preset(), None);
    assert_eq!(DashPattern::parse("6 2.5 0 2.5", true), Some(dash_dot));
    assert_eq!(dash_dot.text(), "6 2.5 0 2.5");
    assert_eq!(DashPattern::parse(&dash_dot.text(), true), Some(dash_dot));
    // An odd count repeats, as in SVG.
    let odd = DashPattern::parse("3 1 0.5", false).unwrap();
    assert_eq!(odd.lengths(), &[3.0, 1.0, 0.5, 3.0, 1.0, 0.5]);
    for bad in ["", "3 0", "3 -1", "3 x", "0", &["1"; 18].join(" ")] {
        assert_eq!(DashPattern::parse(bad, false), None, "{bad:?}");
    }
    assert!(DashPattern::parse(&["1"; 16].join(" "), false).is_some());
    let flat = DashPattern::DASHED.with_lengths(&dash_dot);
    assert_eq!(flat.lengths(), dash_dot.lengths());
    assert!(!flat.round);
    assert_eq!(DashPattern::DASH_DOT.scaled(2.0), vec![12.0, 5.0, 0.0, 5.0]);
    // Round ends take one width from a dash and give it to the next gap. A dot keeps its gap.
    assert_eq!(DashPattern::DASH_DOT.cut(2.0), vec![10.0, 7.0, 0.0, 5.0]);
    let short = DashPattern::parse("0.5 2", true).unwrap();
    assert_eq!(short.cut(2.0), vec![0.0, 5.0]);
    assert_eq!(DashPattern::DASHED.cut(2.0), vec![8.0, 4.0], "flat ends");
    assert!(DashPattern::DASH_DOT.has_dots());
    assert!(!DashPattern::DASHED.has_dots());
}

#[test]
fn the_opacity_and_the_blend_mode_of_a_vector_layer_round_trip() {
    let mut style = LayerStyle::default_for(GeometryKind::Polygon, 0, &[]);
    assert_eq!((style.opacity, style.blend), (1.0, BlendMode::Normal));
    style.opacity = 0.5;
    style.blend = BlendMode::Multiply;
    let json: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    assert_eq!(
        (json["opacity"].as_f64(), json["blend"].as_str()),
        (Some(0.5), Some("multiply"))
    );
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!((back.opacity, back.blend), (0.5, BlendMode::Multiply));
    assert_eq!(back, style);

    // A value out of range gets the nearest end, and an unknown mode is Normal.
    let mut json = json;
    json["opacity"] = 1.5.into();
    json["blend"] = "plus".into();
    let back = LayerStyle::from_json(&json.to_string()).unwrap();
    assert_eq!((back.opacity, back.blend), (1.0, BlendMode::Normal));
}

#[test]
fn a_style_with_no_opacity_and_no_blend_mode_reads_as_opaque_and_normal() {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.opacity = 0.25;
    style.blend = BlendMode::Screen;
    let mut json: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    // A style of version 9 has no opacity and no blend mode.
    json["version"] = 9.into();
    let root = json.as_object_mut().unwrap();
    root.remove("opacity");
    root.remove("blend");
    let back = LayerStyle::from_json(&json.to_string()).unwrap();
    assert_eq!((back.opacity, back.blend), (1.0, BlendMode::Normal));

    // The defaults are not in the JSON.
    let plain = LayerStyle::default_for(GeometryKind::Line, 0, &[]).to_json();
    assert!(!plain.contains("opacity") && !plain.contains("blend"));
}

#[test]
fn the_reference_scale_round_trips() {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    assert_eq!(style.reference_scale, None);
    assert!(!style.to_json().contains("reference_scale"));
    style.reference_scale = Some(5_000_000.0);
    let back = LayerStyle::from_json(&style.to_json()).unwrap();
    assert_eq!(back.reference_scale, Some(5_000_000.0));
    assert_eq!(back, style);

    // A value that is not a positive number reads as none.
    let mut json: serde_json::Value = serde_json::from_str(&style.to_json()).unwrap();
    json["reference_scale"] = (-1.0).into();
    let back = LayerStyle::from_json(&json.to_string()).unwrap();
    assert_eq!(back.reference_scale, None);
}

#[test]
fn the_size_factor_follows_the_reference_scale_in_its_range() {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    assert_eq!(style.size_factor(1_000.0), 1.0);
    style.reference_scale = Some(1_000_000.0);
    assert_eq!(style.size_factor(1_000_000.0), 1.0);
    assert_eq!(style.size_factor(500_000.0), 2.0);
    assert_eq!(style.size_factor(4_000_000.0), 0.25);
    assert_eq!(style.size_factor(1.0), 16.0);
    assert_eq!(style.size_factor(1e12), 1.0 / 64.0);
}

#[test]
fn a_scaled_paint_scales_each_size() {
    let red = Color::rgb(255, 0, 0);
    let stroke = Paint::Stroke(Stroke::solid(red, 2.0));
    assert_eq!(
        stroke.clone().scaled(1.5),
        Paint::Stroke(Stroke::solid(red, 3.0))
    );
    // A stroke gets no thinner than a hairline, and a thinner stroke keeps its width.
    assert_eq!(
        stroke.scaled(0.01),
        Paint::Stroke(Stroke::solid(red, HAIRLINE))
    );
    assert_eq!(
        Paint::Stroke(Stroke::solid(red, 0.1)).scaled(0.5),
        Paint::Stroke(Stroke::solid(red, 0.1))
    );

    let marker = Paint::Marker {
        radius: 4.0,
        shape: MarkerShape::Circle,
        fill: None,
        hatch: Some(Hatch {
            cross: false,
            angle: 45.0,
            spacing: 3.0,
            line_width: 1.0,
        }),
        stroke: Some(Stroke::solid(red, 1.0)),
    };
    let Paint::Marker {
        radius,
        hatch: Some(h),
        stroke: Some(s),
        ..
    } = marker.scaled(2.0)
    else {
        panic!("a marker");
    };
    assert_eq!(
        (radius, h.spacing, h.line_width, h.angle, s.width),
        (8.0, 6.0, 2.0, 45.0, 2.0)
    );

    let symbol = Paint::Symbol {
        symbol: "tree".into(),
        size: 10.0,
        rotation: 30.0,
        tint: None,
    };
    let Paint::Symbol { size, rotation, .. } = symbol.scaled(0.5) else {
        panic!("a symbol");
    };
    assert_eq!((size, rotation), (5.0, 30.0));
}
