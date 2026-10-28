use vale_store::GeometryKind;
use vale_style::expr::Property;
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

fn set_width(style: &mut LayerStyle, rule: usize, w: f64) {
    if let Symbolizer::Stroke(s) = &mut style.style.rules[rule].symbolizers[0] {
        s.width = Property::constant(w);
    }
}

fn set_color(style: &mut LayerStyle, rule: usize, c: Color) {
    if let Symbolizer::Stroke(s) = &mut style.style.rules[rule].symbolizers[0] {
        s.color = Property::constant(c);
    }
}

#[test]
fn one_width_change_is_one_override() {
    let base = two_rules();
    let mut edited = base.clone();
    set_width(&mut edited, 1, 3.0);
    let o = Overrides::between(&base, &edited);
    assert_eq!(
        o.pointers().collect::<Vec<_>>(),
        ["/rules/Minor~1small/symbolizers/0/width"]
    );
    assert_eq!(base.with_overrides(&o), edited);
    assert_eq!(Overrides::from_json(&o.to_json()), o);
}

#[test]
fn equal_styles_have_no_overrides() {
    assert!(Overrides::between(&two_rules(), &two_rules()).is_empty());
}

#[test]
fn an_override_stays_on_its_rule_when_the_rules_move() {
    let base = two_rules();
    let mut edited = base.clone();
    set_width(&mut edited, 0, 4.0);
    let o = Overrides::between(&base, &edited);
    let mut moved = base.clone();
    moved.style.move_rule(0, false);
    let out = moved.with_overrides(&o);
    assert_eq!(out.style.rules[1].name, "Major");
    assert_eq!(width(&out, 1), 4.0);
    assert_eq!(width(&out, 0), 0.5);
}

#[test]
fn an_override_of_a_deleted_rule_does_nothing() {
    let base = two_rules();
    let mut edited = base.clone();
    set_width(&mut edited, 0, 4.0);
    let o = Overrides::between(&base, &edited);
    let mut less = base.clone();
    less.style.rules.remove(0);
    assert_eq!(less.with_overrides(&o), less);
}

#[test]
fn a_change_of_the_base_keeps_each_override() {
    let base = two_rules();
    let mut edited = base.clone();
    set_width(&mut edited, 0, 4.0);
    let o = Overrides::between(&base, &edited);
    let mut changed = base.clone();
    set_color(&mut changed, 0, RED);
    set_width(&mut changed, 1, 1.5);
    let out = changed.with_overrides(&o);
    assert_eq!(width(&out, 0), 4.0);
    assert_eq!(width(&out, 1), 1.5);
    assert_eq!(out.style.rules[0].stroke().unwrap().color, RED);
}

#[test]
fn a_merge_keeps_an_override_with_the_value_of_the_base() {
    let base = two_rules();
    let mut edited = base.clone();
    set_width(&mut edited, 0, 4.0);
    let o = Overrides::between(&base, &edited);
    // The map sets the width back to the value of the base.
    let back = base.clone();
    let o = o.merged(&base, &back);
    assert_eq!(o.len(), 1);
    // A later change of the base does not change the map.
    let mut changed = base.clone();
    set_width(&mut changed, 0, 6.0);
    assert_eq!(width(&changed.with_overrides(&o), 0), 2.0);
}

#[test]
fn a_label_property_and_a_new_scale_limit_are_overrides() {
    let base = two_rules();
    let mut edited = base.clone();
    edited.label.size = 20.0;
    edited.style.rules[0].min_scale = Some(5000.0);
    let o = Overrides::between(&base, &edited);
    assert_eq!(o.len(), 2);
    assert_eq!(base.with_overrides(&o), edited);
    // A limit that the map clears again is gone.
    let o = o.merged(&base, &base);
    assert_eq!(base.with_overrides(&o), base);
}

#[test]
fn a_new_symbolizer_list_is_one_override() {
    let base = two_rules();
    let mut edited = base.clone();
    edited.style.rules[0].symbolizers.push(stroke(RED, 1.0));
    let o = Overrides::between(&base, &edited);
    assert_eq!(
        o.pointers().collect::<Vec<_>>(),
        ["/rules/Major/symbolizers"]
    );
    assert_eq!(base.with_overrides(&o), edited);
}

#[test]
fn bad_text_has_no_overrides() {
    assert!(Overrides::from_json("[1]").is_empty());
    assert!(Overrides::from_json("nope").is_empty());
}
