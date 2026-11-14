use std::collections::BTreeMap;

use vale_store::Geometry;
use vale_style::expr::{Context, Expr, Filter, RasterValues, Stat, Value, rename_raster};

/// A raster layer "Height" with fixed stats, and a layer "Rain" that has no value yet.
struct Fake;

impl RasterValues for Fake {
    fn value(&self, layer: &str, stat: Stat, index: usize, shape: &Geometry) -> Option<f64> {
        if layer != "Height" {
            return None;
        }
        let base = 100.0 * index as f64;
        Some(match (shape, stat) {
            (Geometry::Points(_), _) => base + 2500.0,
            (_, Stat::Mean) => base + 2000.0,
            (_, Stat::Min) => base + 1000.0,
            (_, Stat::Max) => base + 3456.4,
        })
    }

    fn has_layer(&self, layer: &str) -> bool {
        layer == "Height" || layer == "Rain"
    }

    fn flow(&self, layer: &str, index: usize, _shape: &Geometry) -> Option<f64> {
        (layer == "Height").then_some(1e-3 * (index + 1) as f64)
    }
}

fn attributes() -> BTreeMap<String, Value> {
    BTreeMap::from([("name".to_owned(), Value::Text("Mount Ash".into()))])
}

fn ctx<'a>(
    attributes: &'a BTreeMap<String, Value>,
    shape: &'a Geometry,
    rasters: Option<&'a dyn RasterValues>,
) -> Context<'a> {
    Context {
        attributes,
        scale: 1e6,
        geometry: shape.kind(),
        shape: Some(shape),
        radius_km: 6371.0,
        page: None,
        rasters,
        index: 0,
    }
}

fn line() -> Geometry {
    Geometry::Lines(vec![vec![[0.0, 0.0], [1.0, 0.0]]])
}

fn point() -> Geometry {
    Geometry::Points(vec![[1.0, 2.0]])
}

fn eval(src: &str, shape: &Geometry) -> Value {
    let a = attributes();
    let expr = Expr::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    expr.eval(&ctx(&a, shape, Some(&Fake)))
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

#[test]
fn point_gives_the_value_at_the_point() {
    assert_eq!(eval("raster('Height')", &point()), n(2500.0));
    assert_eq!(eval("raster('Height', 'max')", &point()), n(2500.0));
}

#[test]
fn stat_is_mean_if_not_given() {
    assert_eq!(eval("raster('Height')", &line()), n(2000.0));
    assert_eq!(eval("raster('Height', 'mean')", &line()), n(2000.0));
}

#[test]
fn explicit_min_and_max() {
    assert_eq!(eval("raster('Height', 'min')", &line()), n(1000.0));
    assert_eq!(eval("raster('Height', 'max')", &line()), n(3456.4));
}

#[test]
fn stat_has_any_case() {
    assert_eq!(eval("raster('Height', 'MAX')", &line()), n(3456.4));
    assert_eq!(eval("raster('Height', 'Min')", &line()), n(1000.0));
    assert_eq!(Stat::parse("MeAn"), Some(Stat::Mean));
}

#[test]
fn bad_stat_is_null() {
    assert_eq!(eval("raster('Height', 'median')", &line()), Value::Null);
    assert_eq!(eval("raster('Height', 1)", &line()), Value::Null);
    assert_eq!(eval("raster('Height', 'median')", &point()), Value::Null);
}

#[test]
fn unknown_layer_is_null() {
    assert_eq!(eval("raster('Depth')", &line()), Value::Null);
    assert_eq!(eval("raster('Rain')", &line()), Value::Null);
}

#[test]
fn no_rasters_is_null() {
    let a = attributes();
    let shape = line();
    let expr = Expr::parse("raster('Height')").unwrap();
    assert_eq!(expr.eval(&ctx(&a, &shape, None)), Value::Null);
}

#[test]
fn no_shape_is_null() {
    let a = attributes();
    let shape = line();
    let mut c = ctx(&a, &shape, Some(&Fake));
    c.shape = None;
    assert_eq!(
        Expr::parse("raster('Height')").unwrap().eval(&c),
        Value::Null
    );
}

#[test]
fn name_that_is_not_text_is_null() {
    assert_eq!(eval("raster(1)", &line()), Value::Null);
    assert_eq!(eval("raster(null)", &line()), Value::Null);
}

#[test]
fn index_picks_the_feature() {
    let a = attributes();
    let shape = line();
    let mut c = ctx(&a, &shape, Some(&Fake));
    c.index = 2;
    assert_eq!(Expr::parse("raster('Height')").unwrap().eval(&c), n(2200.0));
}

#[test]
fn filter_compares_the_value() {
    let a = attributes();
    let shape = line();
    let c = ctx(&a, &shape, Some(&Fake));
    assert!(Filter::new("raster('Height', 'max') > 3000").matches(&c));
    assert!(!Filter::new("raster('Height') > 3000").matches(&c));
    assert!(!Filter::new("raster('Depth') > 3000").matches(&c));
}

#[test]
fn label_text_with_the_value() {
    assert_eq!(
        eval(
            "name || ' (' || to_text(round(raster('Height', 'max'))) || ' m)'",
            &line()
        ),
        Value::Text("Mount Ash (3456 m)".into())
    );
}

#[test]
fn raster_layers_lists_the_literal_names() {
    let expr = Expr::parse(
        "raster('Rain') > 2 and raster('Height', 'max') > raster('Height') or raster(name) > 1",
    )
    .unwrap();
    assert_eq!(expr.raster_layers(), ["Height", "Rain"]);
    assert!(
        Expr::parse("area() > 1")
            .unwrap()
            .raster_layers()
            .is_empty()
    );
}

#[test]
fn stat_codes_and_labels() {
    let codes: Vec<_> = Stat::ALL.iter().map(|s| s.code()).collect();
    assert_eq!(codes, ["mean", "min", "max"]);
    let labels: Vec<_> = Stat::ALL.iter().map(|s| s.label()).collect();
    assert_eq!(labels, ["Mean", "Min", "Max"]);
    assert_eq!(Stat::default(), Stat::Mean);
    assert!(Fake.has_layer("Rain"));
}

#[test]
fn rename_changes_only_raster_names() {
    assert_eq!(
        rename_raster(
            "raster('Height') > 1 and RASTER( 'Height', 'max') > 2 and name = 'Height'",
            "Height",
            "Elevation's"
        ),
        "raster('Elevation''s') > 1 and RASTER( 'Elevation''s', 'max') > 2 and name = 'Height'"
    );
    assert_eq!(
        rename_raster("raster('Rain') + raster('Height' || 'x')", "Height", "H"),
        "raster('Rain') + raster('Height' || 'x')"
    );
    assert_eq!(
        rename_raster("raster('Height", "Height", "H"),
        "raster('Height"
    );
}

#[test]
fn flow_gives_the_drained_area_in_square_kilometers() {
    let km2 = 1e-3 * 6371.0 * 6371.0;
    assert_eq!(eval("flow('Height')", &line()), n(km2));
    assert_eq!(eval("FLOW('Height')", &line()), n(km2));
    let a = attributes();
    let shape = line();
    let mut c = ctx(&a, &shape, Some(&Fake));
    c.index = 2;
    assert_eq!(
        Expr::parse("flow('Height')").unwrap().eval(&c),
        n(3.0 * km2)
    );
}

#[test]
fn flow_is_null_off_a_line_and_for_an_unknown_layer() {
    let polygon = Geometry::Polygons(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]]]);
    assert_eq!(eval("flow('Height')", &point()), Value::Null);
    assert_eq!(eval("flow('Height')", &polygon), Value::Null);
    assert_eq!(eval("flow('Rain')", &line()), Value::Null);
    assert_eq!(eval("flow(1)", &line()), Value::Null);
    let a = attributes();
    let shape = line();
    let expr = Expr::parse("flow('Height')").unwrap();
    assert_eq!(expr.eval(&ctx(&a, &shape, None)), Value::Null);
}

#[test]
fn flow_names_a_raster_layer() {
    let expr = Expr::parse("sqrt(flow('Height')) / 20 + raster('Rain')").unwrap();
    assert_eq!(expr.raster_layers(), ["Height", "Rain"]);
    assert_eq!(
        rename_raster("clamp(0.5, sqrt(flow('Height')) / 20, 6)", "Height", "Land"),
        "clamp(0.5, sqrt(flow('Land')) / 20, 6)"
    );
}
