use vale_store::GeometryKind;
use vale_style::{Color, LayerStyle, Overrides, Rule, Stroke, StrokeSymbolizer, Symbolizer};

const BLUE: Color = Color::rgb(0, 0, 255);
const RED: Color = Color::rgb(255, 0, 0);

fn stroke(color: Color, width: f64) -> Symbolizer {
    Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke::solid(color, width)))
}

fn two_rules() -> LayerStyle {
    let mut style = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
    style.style.rules = vec![
        Rule::new("Major", vec![stroke(BLUE, 2.0)]),
        Rule::new("Minor/small", vec![stroke(RED, 0.5)]),
    ];
    style
}

fn width(style: &LayerStyle, rule: usize) -> f64 {
    match &style.style.rules[rule].symbolizers[0] {
        Symbolizer::Stroke(s) => s.width.fallback,
        _ => panic!("not a stroke"),
    }
}

/// The overrides of the text of a file of version 34 or older.
fn overrides(text: &str) -> Overrides {
    let o = Overrides::from_json(text);
    assert!(!o.is_empty(), "{text}");
    o
}

#[test]
fn an_override_sets_one_property() {
    let base = two_rules();
    let o = overrides(r#"{"/rules/Minor~1small/symbolizers/0/width":3.0}"#);
    let out = base.with_overrides(&o);
    assert_eq!(width(&out, 1), 3.0);
    assert_eq!(width(&out, 0), 2.0);
    assert_eq!(out.style.rules[1].stroke().unwrap().color, RED);
}

#[test]
fn an_override_stays_on_its_rule_when_the_rules_move() {
    let mut moved = two_rules();
    moved.style.move_rule(0, false);
    let o = overrides(r#"{"/rules/Major/symbolizers/0/width":4.0}"#);
    let out = moved.with_overrides(&o);
    assert_eq!(out.style.rules[1].name, "Major");
    assert_eq!(width(&out, 1), 4.0);
    assert_eq!(width(&out, 0), 0.5);
}

#[test]
fn an_override_of_a_deleted_rule_does_nothing() {
    let mut less = two_rules();
    less.style.rules.remove(0);
    let o = overrides(r#"{"/rules/Major/symbolizers/0/width":4.0}"#);
    assert_eq!(less.with_overrides(&o), less);
}

#[test]
fn a_null_override_clears_a_property() {
    let mut base = two_rules();
    base.style.rules[0].min_scale = Some(5000.0);
    let o = overrides(r#"{"/rules/Major/min_scale":null,"/label/size":20.0}"#);
    let out = base.with_overrides(&o);
    assert_eq!(out.style.rules[0].min_scale, None);
    assert_eq!(out.label.size, 20.0);
}

#[test]
fn bad_text_has_no_overrides() {
    assert!(Overrides::from_json("[1]").is_empty());
    assert!(Overrides::from_json("nope").is_empty());
    let base = two_rules();
    assert_eq!(base.with_overrides(&Overrides::from_json("nope")), base);
    // A value that does not fit the style gives the style.
    let o = overrides(r#"{"/rules/Major/symbolizers/0/width":"wide"}"#);
    assert_eq!(base.with_overrides(&o), base);
}

fn raster() -> LayerStyle {
    use vale_store::{RasterDisplay, Stop};
    let stops = [0.0, 10.0, 20.0].map(|value| Stop {
        value,
        line: Default::default(),
    });
    LayerStyle::raster(vale_style::RasterSymbolizer {
        display: RasterDisplay::Bands,
        opacity: 1.0,
        blend: vale_store::BlendMode::Normal,
        stops: stops.to_vec(),
        colors: vec![[10, 10, 10], [200, 200, 200]],
        lines: true,
        smoothing: Default::default(),
        hillshade: Default::default(),
    })
}

fn raster_mut(style: &mut LayerStyle) -> &mut vale_style::RasterSymbolizer {
    let mut s = style.style.rules[0].symbolizers.iter_mut();
    s.find_map(|s| match s {
        Symbolizer::Raster(r) => Some(r),
        _ => None,
    })
    .unwrap()
}

#[test]
fn a_band_override_sets_the_stops_and_the_colors() {
    let base = raster();
    let o = overrides(
        r##"{"/rules/Colors/symbolizers/0/colors":["#0a0a0a","#ff0000"],
            "/rules/Colors/symbolizers/0/stops/1/value":15.0}"##,
    );
    let out = base.with_overrides(&o);
    let r = out.raster_symbolizer().unwrap();
    assert_eq!(r.colors, [[10, 10, 10], [255, 0, 0]]);
    let values: Vec<f64> = r.stops.iter().map(|s| s.value).collect();
    assert_eq!(values, [0.0, 15.0, 20.0]);
}

#[test]
fn a_hillshade_override_sets_one_value() {
    let base = raster();
    let o = overrides(r#"{"/rules/Colors/symbolizers/0/hillshade/azimuth":90.0}"#);
    let mut expected = base.clone();
    raster_mut(&mut expected).hillshade.azimuth = 90.0;
    assert_eq!(base.with_overrides(&o), expected);
}
