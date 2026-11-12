use std::collections::BTreeMap;
use std::f64::consts::PI;

use vale_store::Geometry;
use vale_style::expr::{Context, Expr, Filter, Value};

const EARTH: f64 = 6371.0;
const SMALL: f64 = 500.0;

fn ctx<'a>(shape: &'a Geometry, radius_km: f64) -> Context<'a> {
    static NONE: BTreeMap<String, Value> = BTreeMap::new();
    Context {
        attributes: &NONE,
        scale: 1e6,
        geometry: shape.kind(),
        shape: Some(shape),
        radius_km,
        page: None,
        rasters: None,
        index: 0,
    }
}

fn eval(src: &str, shape: &Geometry, radius_km: f64) -> Value {
    Expr::parse(src)
        .unwrap_or_else(|e| panic!("{src}: {e}"))
        .eval(&ctx(shape, radius_km))
}

fn number(src: &str, shape: &Geometry, radius_km: f64) -> f64 {
    match eval(src, shape, radius_km) {
        Value::Number(n) => n,
        v => panic!("{src}: {v:?}"),
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

fn polygon(rings: Vec<Vec<[f64; 2]>>) -> Geometry {
    Geometry::Polygons(vec![rings])
}

fn octant() -> Geometry {
    polygon(vec![vec![[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]]])
}

/// A box between two meridians and two parallels, with the parallels in short steps.
fn lat_lon_box(lon: [f64; 2], lat: [f64; 2]) -> Geometry {
    let n = 2000;
    let step = |i: usize| lon[0] + (lon[1] - lon[0]) * i as f64 / n as f64;
    let mut ring: Vec<[f64; 2]> = (0..=n).map(|i| [step(i), lat[0]]).collect();
    ring.extend((0..=n).rev().map(|i| [step(i), lat[1]]));
    polygon(vec![ring])
}

#[test]
fn the_area_of_a_polygon_is_its_spherical_area() {
    for r in [EARTH, SMALL] {
        let a = number("area()", &octant(), r);
        assert!(close(a, PI * r * r / 2.0, 1e-3), "{a}");
        let (lon, lat): ([f64; 2], [f64; 2]) = ([10.0, 40.0], [20.0, 35.0]);
        let exact = r
            * r
            * (lon[1] - lon[0]).to_radians()
            * (lat[1].to_radians().sin() - lat[0].to_radians().sin());
        let a = number("area('km2')", &lat_lon_box(lon, lat), r);
        assert!(close(a, exact, 1e-3), "{a} {exact}");
    }
}

#[test]
fn each_unit_gives_the_right_value() {
    let km2 = PI * EARTH * EARTH / 2.0;
    for (unit, per_km2) in [
        ("km2", 1.0),
        ("mi2", 0.386_102_158_6),
        ("ha", 100.0),
        ("acre", 247.105_381_5),
        ("MI2", 0.386_102_158_6),
    ] {
        let a = number(&format!("area('{unit}')"), &octant(), EARTH);
        assert!(close(a, km2 * per_km2, 1e-9), "{unit}: {a}");
    }
    let km = 3.0 * PI * EARTH / 2.0;
    let line = Geometry::Lines(vec![vec![[0.0, 0.0], [90.0, 0.0]]]);
    for (unit, per_km) in [("km", 1.0), ("mi", 0.621_371_192_2), ("m", 1000.0)] {
        let p = number(&format!("perimeter('{unit}')"), &octant(), EARTH);
        assert!(close(p, km * per_km, 1e-9), "{unit}: {p}");
        let l = number(&format!("line_length('{unit}')"), &line, EARTH);
        assert!(close(l, km / 3.0 * per_km, 1e-9), "{unit}: {l}");
    }
    assert!(close(number("perimeter()", &octant(), EARTH), km, 1e-9));
    assert!(close(number("line_length()", &line, EARTH), km / 3.0, 1e-9));
}

#[test]
fn a_measure_on_the_wrong_geometry_type_is_null() {
    let point = Geometry::Points(vec![[1.0, 2.0]]);
    let line = Geometry::Lines(vec![vec![[0.0, 0.0], [1.0, 0.0]]]);
    for (src, shape) in [
        ("area()", &point),
        ("area()", &line),
        ("perimeter()", &point),
        ("perimeter()", &line),
        ("line_length()", &point),
        ("line_length()", &octant()),
        ("@parts", &point),
        ("area('feet')", &octant()),
        ("area(5)", &octant()),
        ("line_length('km2')", &line),
    ] {
        assert_eq!(eval(src, shape, EARTH), Value::Null, "{src}");
    }
    // Without the geometry, each measure is null.
    let mut c = ctx(&point, EARTH);
    c.shape = None;
    for src in ["area()", "@lat", "@lon", "@parts", "@vertices"] {
        assert_eq!(Expr::parse(src).unwrap().eval(&c), Value::Null, "{src}");
    }
}

#[test]
fn the_position_is_the_center_of_the_feature() {
    let point = Geometry::Points(vec![[12.5, -30.0]]);
    assert!(close(number("@lon", &point, EARTH), 12.5, 1e-9));
    assert!(close(number("@lat", &point, EARTH), -30.0, 1e-9));
    let line = Geometry::Lines(vec![vec![[0.0, 0.0], [40.0, 0.0]]]);
    assert!(close(number("@lon", &line, EARTH), 20.0, 1e-9));
    assert!(number("@lat", &line, EARTH).abs() < 1e-9);
    // The exact center of a box between two meridians and two parallels.
    let (half, lat) = (10f64.to_radians(), [40f64.to_radians(), 60f64.to_radians()]);
    let x = 2.0
        * half.sin()
        * ((lat[1] - lat[0]) / 2.0 + ((2.0 * lat[1]).sin() - (2.0 * lat[0]).sin()) / 4.0);
    let z = 2.0 * half * (lat[1].sin().powi(2) - lat[0].sin().powi(2)) / 2.0;
    let exact = z.atan2(x).to_degrees();
    let shape = lat_lon_box([-10.0, 10.0], [40.0, 60.0]);
    assert!(number("@lon", &shape, EARTH).abs() < 1e-9);
    let lat = number("@lat", &shape, EARTH);
    assert!((lat - exact).abs() < 1e-4, "{lat} {exact}");
}

#[test]
fn parts_and_vertices_count_the_geometry() {
    let lines = Geometry::Lines(vec![
        vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
        vec![[5.0, 5.0], [6.0, 5.0]],
    ]);
    assert_eq!(eval("@parts", &lines, EARTH), Value::Number(2.0));
    assert_eq!(eval("@vertices", &lines, EARTH), Value::Number(5.0));
    // A ring that repeats its first position counts it once.
    let closed = polygon(vec![
        vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ],
        vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0]],
    ]);
    assert_eq!(eval("@parts", &closed, EARTH), Value::Number(1.0));
    assert_eq!(eval("@vertices", &closed, EARTH), Value::Number(7.0));
    let points = Geometry::Points(vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(eval("@vertices", &points, EARTH), Value::Number(2.0));
}

#[test]
fn a_filter_on_the_area_selects_the_large_polygons() {
    let filter = Filter::new("area('mi2') > 10000");
    let large = lat_lon_box([0.0, 5.0], [0.0, 5.0]);
    let small = lat_lon_box([0.0, 1.0], [0.0, 1.0]);
    assert!(filter.matches(&ctx(&large, EARTH)));
    assert!(!filter.matches(&ctx(&small, EARTH)));
    // On a smaller world, the same box is small.
    assert!(!filter.matches(&ctx(&large, SMALL)));
}

#[test]
fn the_variables_print_as_they_parse() {
    for src in [
        "@lat",
        "@lon",
        "@parts",
        "@vertices",
        "area('mi2')",
        "line_length()",
    ] {
        assert_eq!(Expr::parse(src).unwrap().to_string(), src);
    }
    assert_eq!(Expr::parse("@LAT").unwrap().to_string(), "@lat");
}
