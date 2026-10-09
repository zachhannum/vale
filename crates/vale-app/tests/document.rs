use std::path::Path;

use vale_app::document::Document;
use vale_app::view::View;
use vale_import::geojson::read_str;
use vale_sphere::{ProjectionKind, ProjectionSpec};

fn imported(name: &str, text: &str) -> vale_import::Imported {
    read_str(name, text).unwrap()
}

const POINT: &str = r#"{"type":"Point","coordinates":[1,2]}"#;
const POLY: &str = r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#;

#[test]
fn sample_layers() {
    let d = Document::sample();
    let got: Vec<_> = d
        .frame
        .entries
        .iter()
        .map(|e| {
            let l = d.layer(e.layer).unwrap();
            (l.name.clone(), l.features.len(), e.visible)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("Land".to_string(), 127, true),
            ("Rivers".to_string(), 13, true),
            ("Places".to_string(), 243, true)
        ]
    );
    assert_eq!(d.project.world.name, "Earth (sample)");
}

#[test]
fn polygons_stay_under_points() {
    let mut d = Document::empty();
    let p = d.add_imported(imported("pts", POINT), None)[0];
    let g = d.add_imported(imported("poly", POLY), None)[0];
    assert_eq!(d.frame.entries[0].layer, g);
    assert_eq!(d.frame.entries[1].layer, p);
}

#[test]
fn move_and_remove() {
    let mut d = Document::sample();
    let ids: Vec<_> = d.frame.entries.iter().map(|e| e.layer).collect();
    assert!(!d.move_entry(ids[0], false));
    assert!(!d.move_entry(ids[2], true));
    assert!(d.move_entry(ids[0], true));
    let order: Vec<_> = d.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(order, [ids[1], ids[0], ids[2]]);
    assert!(d.move_entry(ids[0], false));
    let order: Vec<_> = d.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(order, ids);
    assert!(d.remove_layer(ids[1]));
    assert!(d.entry(ids[1]).is_none());
    assert!(d.layer(ids[1]).is_none());
    assert_eq!(d.frame.entries.len(), 2);
}

#[test]
fn radius_rescales_view() {
    let mut d = Document::sample();
    d.frame.view = Some(View {
        center: kurbo::Point::new(1000.0, 2000.0),
        scale: 1e-4,
    });
    d.set_radius_km(3000.0);
    let v = d.frame.view.unwrap();
    let k = 3000.0 / 6371.0;
    assert!((v.center.x - 1000.0 * k).abs() < 1e-9);
    assert!((v.center.y - 2000.0 * k).abs() < 1e-9);
    assert!((v.scale - 1e-4 / k).abs() < 1e-15);
    d.set_radius_km(1.0);
    assert_eq!(d.project.world.radius_km, 100.0);
}

#[test]
fn projection_resets_view() {
    let mut d = Document::sample();
    let view = View {
        center: kurbo::Point::new(1.0, 2.0),
        scale: 1e-4,
    };
    d.frame.view = Some(view);
    d.set_projection(ProjectionSpec::default());
    assert_eq!(d.frame.view, Some(view));
    d.set_projection(ProjectionSpec {
        kind: ProjectionKind::Mercator,
        lon0: 0.0,
        lat0: 0.0,
    });
    assert_eq!(d.frame.view, None);
}

#[test]
fn open_missing_changes_nothing() {
    let mut d = Document::sample();
    let before = d.clone();
    assert!(d.open_geojson(Path::new("/no/such/file.geojson")).is_err());
    assert_eq!(d, before);
}
