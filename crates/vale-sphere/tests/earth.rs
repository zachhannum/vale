//! Tests on Natural Earth 110m land.

use serde_json::Value;
use vale_sphere::{LonLat, Point, Projection, ProjectionKind, ProjectionSpec};

const RADIUS: f64 = 6_371_000.0;

/// Polygons as lists of rings.
fn land() -> Vec<Vec<Vec<LonLat>>> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/natural-earth/110m/ne_110m_land.geojson"
    ))
    .unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let ring = |r: &Value| -> Vec<LonLat> {
        r.as_array()
            .unwrap()
            .iter()
            .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
            .collect()
    };
    let mut out = Vec::new();
    for f in v["features"].as_array().unwrap() {
        let g = &f["geometry"];
        let c = &g["coordinates"];
        match g["type"].as_str().unwrap() {
            "Polygon" => out.push(c.as_array().unwrap().iter().map(ring).collect()),
            "MultiPolygon" => {
                for poly in c.as_array().unwrap() {
                    out.push(poly.as_array().unwrap().iter().map(ring).collect());
                }
            }
            _ => {}
        }
    }
    out
}

fn shoelace(r: &[Point]) -> f64 {
    let n = r.len();
    (0..n)
        .map(|i| r[i].x * r[(i + 1) % n].y - r[(i + 1) % n].x * r[i].y)
        .sum::<f64>()
        / 2.0
}

fn fraction(kind: ProjectionKind, lon0: f64, lat0: f64) -> f64 {
    let p = Projection::new(ProjectionSpec { kind, lon0, lat0 }, RADIUS).unwrap();
    let mut total = 0.0;
    for poly in land() {
        let rings = p.project_polygon(&poly);
        total += rings.iter().map(|r| shoelace(r)).sum::<f64>().abs();
    }
    total / (4.0 * std::f64::consts::PI * RADIUS * RADIUS)
}

#[test]
fn land_area_is_stable() {
    for (kind, lon0, lat0) in [
        (ProjectionKind::EqualEarth, 0.0, 0.0),
        (ProjectionKind::EqualEarth, 150.0, 0.0),
        (ProjectionKind::LambertAzimuthal, -100.0, 40.0),
        (ProjectionKind::LambertAzimuthal, 20.0, -30.0),
    ] {
        let f = fraction(kind, lon0, lat0);
        assert!(f > 0.283 && f < 0.294, "{kind:?} {lon0} {lat0}: {f}");
    }
}

#[test]
fn orthographic_land_fraction() {
    let f = fraction(ProjectionKind::Orthographic, 20.0, 30.0);
    assert!(f > 0.10 && f < 0.14, "{f}");
}

#[test]
fn everything_is_finite_and_inside_bounds() {
    let polys = land();
    for kind in ProjectionKind::ALL {
        for (lon0, lat0) in [(0.0, 0.0), (150.0, 0.0), (20.0, 30.0), (-100.0, -70.0)] {
            let p = Projection::new(ProjectionSpec { kind, lon0, lat0 }, RADIUS).unwrap();
            let b = p.bounds();
            let grown = b.inflate(0.001 * b.width(), 0.001 * b.height());
            for poly in &polys {
                let all = p
                    .project_polygon(poly)
                    .into_iter()
                    .chain(p.project_outline_of(poly));
                for ring in all {
                    for q in ring {
                        assert!(q.x.is_finite() && q.y.is_finite());
                        assert!(
                            q.x >= grown.x0
                                && q.x <= grown.x1
                                && q.y >= grown.y0
                                && q.y <= grown.y1,
                            "{kind:?} {lon0} {lat0}: {q:?} outside {grown:?}"
                        );
                    }
                }
            }
        }
    }
}
