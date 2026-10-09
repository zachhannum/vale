//! Natural Earth GeoJSON reader.

use std::path::Path;

use anyhow::{Context, anyhow};
use serde_json::Value;

/// A populated place.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub scalerank: i64,
    pub pop_max: f64,
    pub capital: bool,
}

/// A named river, possibly in several parts. Coordinates are `[lon, lat]`.
#[derive(Clone, Debug, PartialEq)]
pub struct River {
    pub name: String,
    pub scalerank: i64,
    pub parts: Vec<Vec<[f64; 2]>>,
}

/// A polygon, outer ring first. Coordinates are `[lon, lat]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub rings: Vec<Vec<[f64; 2]>>,
}

fn read_features(path: &Path) -> anyhow::Result<Vec<Value>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut doc: Value =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    if doc.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
        return Err(anyhow!(
            "{} is not a GeoJSON FeatureCollection",
            path.display()
        ));
    }
    match doc.get_mut("features").map(Value::take) {
        Some(Value::Array(features)) => Ok(features),
        _ => Err(anyhow!("{} has no features array", path.display())),
    }
}

fn position(v: &Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

fn line(v: &Value) -> Option<Vec<[f64; 2]>> {
    v.as_array()?.iter().map(position).collect()
}

fn lines(v: &Value) -> Option<Vec<Vec<[f64; 2]>>> {
    v.as_array()?.iter().map(line).collect()
}

fn geometry(feature: &Value) -> Option<(&str, &Value)> {
    let g = feature.get("geometry")?;
    Some((g.get("type")?.as_str()?, g.get("coordinates")?))
}

fn non_empty_name(feature: &Value) -> Option<String> {
    let name = feature.get("properties")?.get("name")?.as_str()?;
    (!name.is_empty()).then(|| name.to_string())
}

fn scalerank(feature: &Value) -> i64 {
    feature
        .get("properties")
        .and_then(|p| p.get("scalerank"))
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(10)
}

pub fn load_places(path: &Path) -> anyhow::Result<Vec<Place>> {
    let mut out = Vec::new();
    for f in read_features(path)? {
        let Some(("Point", coords)) = geometry(&f) else {
            continue;
        };
        let (Some([lon, lat]), Some(name)) = (position(coords), non_empty_name(&f)) else {
            continue;
        };
        let props = f.get("properties");
        let pop_max = props
            .and_then(|p| p.get("pop_max"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let capital = props
            .and_then(|p| p.get("featurecla"))
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("Admin-0 capital"));
        out.push(Place {
            name,
            lon,
            lat,
            scalerank: scalerank(&f),
            pop_max,
            capital,
        });
    }
    Ok(out)
}

pub fn load_rivers(path: &Path) -> anyhow::Result<Vec<River>> {
    let mut out = Vec::new();
    for f in read_features(path)? {
        let Some(name) = non_empty_name(&f) else {
            continue;
        };
        let parts = match geometry(&f) {
            Some(("LineString", c)) => line(c).map(|l| vec![l]),
            Some(("MultiLineString", c)) => lines(c),
            _ => None,
        };
        if let Some(parts) = parts {
            out.push(River {
                name,
                scalerank: scalerank(&f),
                parts,
            });
        }
    }
    Ok(out)
}

pub fn load_polygons(path: &Path) -> anyhow::Result<Vec<Polygon>> {
    let mut out = Vec::new();
    for f in read_features(path)? {
        match geometry(&f) {
            Some(("Polygon", c)) => {
                if let Some(rings) = lines(c) {
                    out.push(Polygon { rings });
                }
            }
            Some(("MultiPolygon", c)) => {
                if let Some(members) = c.as_array() {
                    out.extend(
                        members
                            .iter()
                            .filter_map(lines)
                            .map(|rings| Polygon { rings }),
                    );
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Try to join two parts end to end. Returns the joined part.
fn try_join(a: &[[f64; 2]], b: &[[f64; 2]]) -> Option<Vec<[f64; 2]>> {
    let (a_first, a_last) = (a.first()?, a.last()?);
    let (b_first, b_last) = (b.first()?, b.last()?);
    let mut out: Vec<[f64; 2]>;
    if a_last == b_first {
        out = a.to_vec();
        out.extend_from_slice(&b[1..]);
    } else if a_last == b_last {
        out = a.to_vec();
        out.extend(b.iter().rev().skip(1));
    } else if a_first == b_last {
        out = b.to_vec();
        out.extend_from_slice(&a[1..]);
    } else if a_first == b_first {
        out = b.iter().rev().copied().collect();
        out.extend_from_slice(&a[1..]);
    } else {
        return None;
    }
    Some(out)
}

/// Merge rivers with the same name and join parts that share an end point.
pub fn join_rivers(rivers: Vec<River>) -> Vec<River> {
    let mut out: Vec<River> = Vec::new();
    for r in rivers {
        match out.iter_mut().find(|o| o.name == r.name) {
            Some(o) => {
                o.scalerank = o.scalerank.min(r.scalerank);
                o.parts.extend(r.parts);
            }
            None => out.push(r),
        }
    }
    for river in &mut out {
        let mut parts = std::mem::take(&mut river.parts);
        'again: loop {
            for i in 0..parts.len() {
                for j in (i + 1)..parts.len() {
                    if let Some(joined) = try_join(&parts[i], &parts[j]) {
                        parts[i] = joined;
                        parts.remove(j);
                        continue 'again;
                    }
                }
            }
            break;
        }
        parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
        river.parts = parts;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ne(name: &str) -> std::path::PathBuf {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/natural-earth/110m/"
        ))
        .join(name)
    }

    #[test]
    fn places() {
        let p = load_places(&ne("ne_110m_populated_places_simple.geojson")).unwrap();
        assert_eq!(p.len(), 243);
        let t = p.iter().find(|p| p.name == "Tokyo").unwrap();
        assert!(t.capital);
        assert!(t.pop_max > 1e7);
    }

    #[test]
    fn rivers() {
        let r = load_rivers(&ne("ne_110m_rivers_lake_centerlines.geojson")).unwrap();
        assert_eq!(r.len(), 13);
        assert!(r.iter().all(|r| r.parts.len() == 1));
        assert!(r.iter().any(|r| r.name == "Donau"));
    }

    #[test]
    fn polygons() {
        let p = load_polygons(&ne("ne_110m_land.geojson")).unwrap();
        assert_eq!(p.len(), 127);
    }

    #[test]
    fn missing_path_errors() {
        let path = Path::new("/nonexistent/dir/nothing.geojson");
        let e = load_places(path).unwrap_err();
        assert!(format!("{e:#}").contains("/nonexistent/dir/nothing.geojson"));
    }

    fn river(name: &str, rank: i64, parts: Vec<Vec<[f64; 2]>>) -> River {
        River {
            name: name.into(),
            scalerank: rank,
            parts,
        }
    }

    #[test]
    fn join() {
        let a1 = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let a2 = vec![[2.0, 0.0], [3.0, 0.0]];
        // Reversed: its last point touches the joined end (3,0).
        let a3 = vec![[5.0, 0.0], [4.0, 0.0], [3.0, 0.0]];
        let b = vec![[0.0, 5.0], [1.0, 5.0]];
        let c1 = vec![[0.0, 9.0], [1.0, 9.0]];
        let c2 = vec![[5.0, 9.0], [6.0, 9.0], [7.0, 9.0]];
        let out = join_rivers(vec![
            river("A", 3, vec![a1]),
            river("B", 1, vec![b]),
            river("A", 2, vec![a2]),
            river("C", 4, vec![c1, c2]),
            river("A", 5, vec![a3]),
        ]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].name, "A");
        assert_eq!(out[0].scalerank, 2);
        assert_eq!(out[0].parts.len(), 1);
        assert_eq!(out[0].parts[0].len(), 3 + 2 + 3 - 2);
        assert_eq!(out[1].name, "B");
        assert_eq!(out[1].parts.len(), 1);
        assert_eq!(out[2].name, "C");
        assert_eq!(out[2].parts.len(), 2);
        assert_eq!(out[2].parts[0].len(), 3);

        // Two-part reversed join, point count is the sum minus one.
        let out = join_rivers(vec![river(
            "A",
            1,
            vec![vec![[0.0, 0.0], [1.0, 0.0]], vec![[2.0, 0.0], [1.0, 0.0]]],
        )]);
        assert_eq!(out[0].parts.len(), 1);
        assert_eq!(out[0].parts[0].len(), 3);
    }
}
