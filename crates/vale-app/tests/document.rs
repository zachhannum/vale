use std::path::Path;

use vale_app::document::Document;
use vale_app::view::View;
use vale_import::geojson::read_str;
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::GeometryKind;

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
            (l.name.clone(), l.features.len(), l.visible)
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
fn a_new_layer_goes_on_top_with_the_first_free_name() {
    let mut d = Document::sample();
    let a = d.add_layer(GeometryKind::Line);
    let b = d.add_layer(GeometryKind::Polygon);
    let c = d.add_layer(GeometryKind::Line);
    let names: Vec<&str> = d.project.layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Land",
            "Rivers",
            "Places",
            "Lines 1",
            "Polygons 1",
            "Lines 2"
        ]
    );
    assert!(d.remove_layer(a));
    let again = d.add_layer(GeometryKind::Line);
    assert_eq!(d.layer(again).unwrap().name, "Lines 1");
    assert!(d.layer(b).unwrap().features.is_empty());
    assert!(d.visible(c));
}

#[test]
fn the_entries_follow_the_order_of_the_project_and_keep_their_styles() {
    let mut d = Document::sample();
    let ids: Vec<_> = d.project.layers().iter().map(|l| l.id).collect();
    let style = d.entry(ids[0]).unwrap().style.clone();
    assert!(d.move_layer(ids[0], 2));
    let order: Vec<_> = d.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(order, [ids[1], ids[2], ids[0]]);
    let layers: Vec<_> = d.project.layers().iter().map(|l| l.id).collect();
    assert_eq!(order, layers);
    assert_eq!(d.entry(ids[0]).unwrap().style, style);

    // An undo gives the document an older project.
    let mut older = d.project.clone();
    older.move_layer(ids[0], 0);
    d.set_project(older);
    let order: Vec<_> = d.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(order, ids);
    assert_eq!(d.entry(ids[0]).unwrap().style, style);
}

#[test]
fn rename_and_hide() {
    let mut d = Document::sample();
    let land = d.project.layers()[0].id;
    assert!(d.rename_layer(land, "  Continents "));
    assert_eq!(d.layer(land).unwrap().name, "Continents");
    assert!(!d.rename_layer(land, "  "));
    assert_eq!(d.layer(land).unwrap().name, "Continents");
    assert!(d.set_visible(land, false));
    assert!(!d.visible(land));
}

#[test]
fn radius_rescales_view() {
    let mut d = Document::sample();
    d.frame.view = Some(View {
        center: kurbo::Point::new(1000.0, 2000.0),
        scale: 1e-4,
        rotation: 0.0,
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
        rotation: 0.0,
    };
    d.frame.view = Some(view);
    d.set_projection(d.first_map(), ProjectionSpec::default());
    assert_eq!(d.frame.view, Some(view));
    d.set_projection(
        d.first_map(),
        ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0),
    );
    assert_eq!(d.frame.view, None);
}

#[test]
fn open_missing_changes_nothing() {
    let mut d = Document::sample();
    let before = d.clone();
    assert!(d.open_geojson(Path::new("/no/such/file.geojson")).is_err());
    assert_eq!(d, before);
}

#[test]
fn a_raster_rename_changes_each_expression_that_reads_the_raster() {
    use vale_style::expr::Filter;
    let mut d = Document::sample();
    let layer_of = |d: &Document, name: &str| {
        (d.project.layers().iter())
            .find(|l| l.name == name)
            .unwrap()
            .id
    };
    for name in ["Height", "Rain"] {
        d.project
            .add_raster(name.into(), 64, "m".into(), 0.0, 4000.0);
    }
    let places = layer_of(&d, "Places");
    let style = &mut d.entry_mut(places).unwrap().style;
    style.style.rules[0].filter = Filter::new("raster('Height') > raster('Rain')");
    style.label.text = "name || raster('Height', 'max')".into();
    let map = d.project.atlas.add_map();
    let globe = d.entry(places).unwrap().style.to_json();
    let record = d.project.atlas.map_mut(map).unwrap();
    record.styles.insert(places, globe);
    let filter = |d: &Document| {
        let style = &d.entry(places).unwrap().style;
        style.style.rules[0]
            .filter
            .source()
            .unwrap()
            .text()
            .to_owned()
    };

    let height = layer_of(&d, "Height");
    assert!(d.rename_layer(height, "Elevation"));
    assert_eq!(filter(&d), "raster('Elevation') > raster('Rain')");
    let style = &d.entry(places).unwrap().style;
    assert_eq!(style.label.text, "name || raster('Elevation', 'max')");
    let in_map = d.project.atlas.map(map).unwrap().styles[&places].clone();
    assert!(in_map.contains("raster('Elevation')"), "{in_map}");

    // A name that another raster layer has changes no expression.
    assert!(d.rename_layer(height, "Rain"));
    assert_eq!(filter(&d), "raster('Elevation') > raster('Rain')");
}

#[test]
fn a_field_rename_changes_each_expression_of_the_layer() {
    use vale_app::document::Target;
    use vale_style::LabelClass;
    use vale_style::expr::Filter;
    let mut d = Document::sample();
    let places = (d.project.layers().iter())
        .find(|l| l.name == "Places")
        .unwrap()
        .id;
    let style = &mut d.entry_mut(places).unwrap().style;
    style.style.rules[0].filter = Filter::new("pop_max > 10");
    style.label.text = "concat(name, ' ' || pop_max)".into();
    let globe = style.clone();

    // A map style, with a style and a label class of the layer.
    let first = d.project.atlas.add_map();
    let mut class = LabelClass::new("Big", GeometryKind::Point, 0).unwrap();
    class.text = "upper(name)".into();
    class.filter = "pop_max > 5".into();
    assert!(d.set_label_classes(first, places, std::slice::from_ref(&class)));
    let record = d.project.atlas.map_mut(first).unwrap();
    record.styles.insert(places, globe.to_json());
    let shared = d.save_as_map_style(first).unwrap();
    // A map with its own label class.
    let second = d.project.atlas.add_map();
    class.text = "name || ' ' || pop_max".into();
    assert!(d.set_label_classes(second, places, &[class]));

    assert!(d.rename_field(places, "pop_max", "people"));
    assert!(d.rename_field(places, "name", "title"));
    let style = &d.entry(places).unwrap().style;
    let filter =
        |s: &vale_style::LayerStyle| s.style.rules[0].filter.source().unwrap().text().to_owned();
    assert_eq!(filter(style), "people > 10");
    assert_eq!(style.label.text, "concat(title, ' ' || people)");
    assert_eq!(style.label.priority_field.as_deref(), Some("people"));
    let in_style = d.map_style(Target::Style(shared), places).unwrap();
    assert_eq!(filter(&in_style), "people > 10");
    let classes = d.label_classes_in(Target::Style(shared), places);
    assert_eq!(classes[0].text, "upper(title)");
    assert_eq!(classes[0].filter, "people > 5");
    let classes = d.label_classes(Some(second), places);
    assert_eq!(classes[0].text, "title || ' ' || people");
}
