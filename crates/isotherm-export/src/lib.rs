//! The exports of the results.

use serde_json::{Map, Value, json};

/// An RFC 7946 feature collection with the foreign member
/// `"isotherm": {"radius_km": …}`, because RFC 7946 assumes the WGS 84 ellipsoid.
pub fn feature_collection(radius_km: f64, features: Vec<Value>) -> Value {
    json!({
        "type": "FeatureCollection",
        "isotherm": { "radius_km": radius_km },
        "features": features,
    })
}

/// A polygon feature with one ring of longitude and latitude pairs in
/// degrees. The properties get `area_km2` on a sphere of radius `radius_km`.
pub fn polygon_feature(
    ring: &[(f64, f64)],
    radius_km: f64,
    mut properties: Map<String, Value>,
) -> Value {
    let area = isotherm_vector::ring_area_km2(ring, radius_km);
    properties.insert("area_km2".to_string(), json!(area));
    let mut coordinates: Vec<[f64; 2]> = ring.iter().map(|&(lon, lat)| [lon, lat]).collect();
    if ring.first() != ring.last() {
        coordinates.push(coordinates[0]);
    }
    json!({
        "type": "Feature",
        "geometry": { "type": "Polygon", "coordinates": [coordinates] },
        "properties": properties,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_carries_the_radius_and_the_area() {
        let mut properties = Map::new();
        properties.insert("code".to_string(), json!("EF"));
        let ring = [(0.0, 0.0), (90.0, 0.0), (0.0, 90.0)];
        let feature = polygon_feature(&ring, 3000.0, properties);
        let collection = feature_collection(3000.0, vec![feature]);
        assert_eq!(collection["type"], "FeatureCollection");
        assert_eq!(collection["isotherm"]["radius_km"], 3000.0);
        let feature = &collection["features"][0];
        assert_eq!(feature["properties"]["code"], "EF");
        let area = feature["properties"]["area_km2"].as_f64().unwrap();
        let octant = std::f64::consts::PI * 3000.0 * 3000.0 / 2.0;
        assert!((area - octant).abs() < 1e-6 * octant);
        let coordinates = feature["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(coordinates.len(), 4);
        assert_eq!(coordinates[0], coordinates[3]);
    }
}
