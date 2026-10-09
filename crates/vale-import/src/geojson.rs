//! The GeoJSON reader.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value as Json;
use vale_store::{Feature, Geometry, GeometryKind, LonLat, Value};

use crate::ImportError;

/// One layer read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedLayer {
    pub name: String,
    pub kind: GeometryKind,
    pub features: Vec<Feature>,
}

/// The result of reading one file.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub layers: Vec<ImportedLayer>,
    /// The count of features that could not be used.
    pub skipped: usize,
}

/// Read GeoJSON text. `name` names the layers.
pub fn read_str(name: &str, text: &str) -> Result<Imported, ImportError> {
    let json: Json = serde_json::from_str(text).map_err(|e| ImportError::Parse {
        name: name.to_string(),
        message: e.to_string(),
    })?;
    let not_geojson = || ImportError::NotGeoJson {
        name: name.to_string(),
    };
    let ty = json
        .get("type")
        .and_then(Json::as_str)
        .ok_or_else(not_geojson)?;

    let mut skipped = 0usize;
    let mut buckets: [Vec<Feature>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut add = |geom: Option<Geometry>, attributes, skipped: &mut usize| match geom {
        Some(g) => {
            let slot = match g.kind() {
                GeometryKind::Polygon => 0,
                GeometryKind::Line => 1,
                GeometryKind::Point => 2,
            };
            buckets[slot].push(Feature {
                geometry: g,
                attributes,
            });
        }
        None => *skipped += 1,
    };

    match ty {
        "FeatureCollection" => {
            let features = json
                .get("features")
                .and_then(Json::as_array)
                .ok_or_else(not_geojson)?;
            for f in features {
                let (g, a) = read_feature(f);
                add(g, a, &mut skipped);
            }
        }
        "Feature" => {
            let (g, a) = read_feature(&json);
            add(g, a, &mut skipped);
        }
        "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon"
        | "GeometryCollection" => {
            add(read_geometry(&json), BTreeMap::new(), &mut skipped);
        }
        _ => return Err(not_geojson()),
    }

    let kinds = [
        (GeometryKind::Polygon, "polygons"),
        (GeometryKind::Line, "lines"),
        (GeometryKind::Point, "points"),
    ];
    let used = buckets.iter().filter(|b| !b.is_empty()).count();
    let mut layers = Vec::new();
    for (bucket, (kind, suffix)) in buckets.into_iter().zip(kinds) {
        if bucket.is_empty() {
            continue;
        }
        layers.push(ImportedLayer {
            name: if used > 1 {
                format!("{name} ({suffix})")
            } else {
                name.to_string()
            },
            kind,
            features: bucket,
        });
    }
    if layers.is_empty() {
        return Err(ImportError::Empty {
            name: name.to_string(),
        });
    }
    Ok(Imported { layers, skipped })
}

/// Read a GeoJSON file. The layer name is the file stem.
pub fn read_file(path: &Path) -> Result<Imported, ImportError> {
    let text = std::fs::read_to_string(path).map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "layer".to_string());
    read_str(&name, &text)
}

fn read_feature(f: &Json) -> (Option<Geometry>, BTreeMap<String, Value>) {
    let geometry = f.get("geometry").and_then(read_geometry);
    let mut attributes = BTreeMap::new();
    if let Some(props) = f.get("properties").and_then(Json::as_object) {
        for (k, v) in props {
            attributes.insert(k.clone(), to_value(v));
        }
    }
    (geometry, attributes)
}

fn to_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n.as_f64().map(Value::Number).unwrap_or(Value::Null),
        Json::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

fn position(v: &Json) -> Option<LonLat> {
    let a = v.as_array()?;
    let lon = a.first()?.as_f64()?;
    let lat = a.get(1)?.as_f64()?;
    if !lon.is_finite() || !lat.is_finite() {
        return None;
    }
    Some([lon, lat.clamp(-90.0, 90.0)])
}

fn positions(v: &Json) -> Option<Vec<LonLat>> {
    v.as_array()?.iter().map(position).collect()
}

fn lines(v: &Json) -> Option<Vec<Vec<LonLat>>> {
    let mut out = Vec::new();
    for l in v.as_array()? {
        let p = positions(l)?;
        if p.len() >= 2 {
            out.push(p);
        }
    }
    Some(out)
}

fn polygon(v: &Json) -> Option<Vec<Vec<LonLat>>> {
    let mut rings = Vec::new();
    for r in v.as_array()? {
        let p = positions(r)?;
        if p.len() >= 3 {
            rings.push(p);
        }
    }
    Some(rings)
}

/// Returns `None` when the geometry is unusable.
fn read_geometry(g: &Json) -> Option<Geometry> {
    let ty = g.get("type")?.as_str()?;
    let coords = g.get("coordinates");
    match ty {
        "Point" => Some(Geometry::Points(vec![position(coords?)?])),
        "MultiPoint" => {
            let p = positions(coords?)?;
            (!p.is_empty()).then_some(Geometry::Points(p))
        }
        "LineString" => {
            let p = positions(coords?)?;
            (p.len() >= 2).then(|| Geometry::Lines(vec![p]))
        }
        "MultiLineString" => {
            let l = lines(coords?)?;
            (!l.is_empty()).then_some(Geometry::Lines(l))
        }
        "Polygon" => {
            let rings = polygon(coords?)?;
            // The outer ring must survive for the polygon to count.
            let ok = coords?.as_array()?.first().and_then(positions)?.len() >= 3;
            (ok && !rings.is_empty()).then(|| Geometry::Polygons(vec![rings]))
        }
        "MultiPolygon" => {
            let mut out = Vec::new();
            for poly in coords?.as_array()? {
                let rings = polygon(poly)?;
                let outer_ok = poly.as_array()?.first().and_then(positions)?.len() >= 3;
                if outer_ok && !rings.is_empty() {
                    out.push(rings);
                }
            }
            (!out.is_empty()).then_some(Geometry::Polygons(out))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(f: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/natural-earth/110m")
            .join(f)
    }

    #[test]
    fn natural_earth() {
        let land = read_file(&data("ne_110m_land.geojson")).unwrap();
        assert_eq!(land.skipped, 0);
        assert_eq!(land.layers.len(), 1);
        assert_eq!(land.layers[0].kind, GeometryKind::Polygon);
        assert_eq!(land.layers[0].features.len(), 127);
        let rivers = read_file(&data("ne_110m_rivers_lake_centerlines.geojson")).unwrap();
        assert_eq!(rivers.skipped, 0);
        assert_eq!(rivers.layers.len(), 1);
        assert_eq!(rivers.layers[0].kind, GeometryKind::Line);
        assert_eq!(rivers.layers[0].features.len(), 13);
        let places = read_file(&data("ne_110m_populated_places_simple.geojson")).unwrap();
        assert_eq!(places.skipped, 0);
        assert_eq!(places.layers.len(), 1);
        assert_eq!(places.layers[0].kind, GeometryKind::Point);
        assert_eq!(places.layers[0].features.len(), 243);
        let tokyo = places.layers[0]
            .features
            .iter()
            .find(|f| f.attributes.get("name") == Some(&Value::Text("Tokyo".into())))
            .unwrap();
        assert!(tokyo.attributes["pop_max"].as_f64().unwrap() > 1e7);
    }

    #[test]
    fn mixed_collection() {
        let t = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[1,2]}},
          {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]}},
          {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}}]}"#;
        let i = read_str("x", t).unwrap();
        let names: Vec<_> = i.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["x (polygons)", "x (lines)", "x (points)"]);
        assert_eq!(i.skipped, 0);
    }

    #[test]
    fn bare_and_single() {
        let p = read_str("p", r#"{"type":"Point","coordinates":[3,4]}"#).unwrap();
        assert_eq!(p.layers.len(), 1);
        assert_eq!(p.layers[0].features.len(), 1);
        let f = read_str(
            "f",
            r#"{"type":"Feature","properties":{"a":1},"geometry":{"type":"Point","coordinates":[3,4]}}"#,
        )
        .unwrap();
        assert_eq!(f.layers[0].features.len(), 1);
        assert_eq!(f.layers[0].name, "f");
    }

    #[test]
    fn multipoint() {
        let i = read_str(
            "m",
            r#"{"type":"MultiPoint","coordinates":[[0,0],[1,1],[2,2]]}"#,
        )
        .unwrap();
        assert_eq!(i.layers[0].features.len(), 1);
        match &i.layers[0].features[0].geometry {
            Geometry::Points(p) => assert_eq!(p.len(), 3),
            _ => panic!(),
        }
    }

    #[test]
    fn skipped_and_empty() {
        let t = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{},"geometry":null},
          {"type":"Feature","properties":{},"geometry":{"type":"GeometryCollection","geometries":[]}}]}"#;
        assert!(matches!(read_str("e", t), Err(ImportError::Empty { .. })));
        let t2 = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{},"geometry":null},
          {"type":"Feature","properties":{},"geometry":{"type":"GeometryCollection","geometries":[]}},
          {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
        assert_eq!(read_str("e", t2).unwrap().skipped, 2);
    }

    #[test]
    fn errors() {
        assert!(matches!(
            read_str("t", r#"{"type":"Topology"}"#),
            Err(ImportError::NotGeoJson { .. })
        ));
        assert!(matches!(read_str("t", "{"), Err(ImportError::Parse { .. })));
        let p = Path::new("/no/such/file.geojson");
        match read_file(p) {
            Err(e @ ImportError::Io { .. }) => assert!(e.to_string().contains("/no/such/file")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn positions_and_properties() {
        let i = read_str(
            "c",
            r#"{"type":"FeatureCollection","features":[
              {"type":"Feature","properties":{"l":[1,2],"n":null,"b":true},"geometry":{"type":"Point","coordinates":[190,95,7]}},
              {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":["a",1]}}]}"#,
        )
        .unwrap();
        assert_eq!(i.skipped, 1);
        let f = &i.layers[0].features[0];
        assert_eq!(f.geometry, Geometry::Points(vec![[190.0, 90.0]]));
        assert_eq!(f.attributes["l"], Value::Text("[1,2]".into()));
        assert_eq!(f.attributes["n"], Value::Null);
        assert_eq!(f.attributes["b"], Value::Bool(true));
    }
}
