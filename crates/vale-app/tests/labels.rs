mod common;

use clap::Parser;
use kurbo::{Point, Rect, Shape};
use vale_app::cli::{Args, apply_view};
use vale_app::document::{Document, LayerEntry};
use vale_app::furniture::{nice, scale_bar};
use vale_app::pick::pick;
use vale_app::pipeline::{Composed, PageFeature, PageLayer, Pipeline, Quality, Selection, fonts};
use vale_render::Item;
use vale_store::{Feature, Geometry, GeometryKind, LayerId, Units, Value};
use vale_style::{LayerStyle, PointSymbolizer, Rule, ScatterSymbolizer, Style, Symbolizer};

const SIZE: (f64, f64) = (1280.0, 800.0);
const REGULAR_LEN: usize = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf").len();

fn compose_with(doc: &Document, quality: Quality) -> Composed {
    let mut f = fonts().unwrap();
    Pipeline::new()
        .compose(doc, SIZE, &mut f, quality, None)
        .unwrap()
}

fn compose(doc: &Document) -> Composed {
    compose_with(doc, Quality::Final)
}

fn texts(c: &Composed) -> Vec<String> {
    c.list
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs(g) => Some(g.text.clone()),
            _ => None,
        })
        .collect()
}

fn layer_id(doc: &Document, name: &str) -> LayerId {
    doc.project
        .layers()
        .iter()
        .find(|l| l.name == name)
        .unwrap()
        .id
}

fn place_named(doc: &Document, layer: &str, name: &str) -> [f64; 2] {
    let l = doc.layer(layer_id(doc, layer)).unwrap();
    let f = l
        .features
        .iter()
        .find(|f| f.attributes.get("name") == Some(&Value::Text(name.into())))
        .unwrap();
    match &f.geometry {
        Geometry::Points(p) => p[0],
        _ => panic!("not a point"),
    }
}

#[test]
fn world_labels() {
    let doc = Document::sample();
    let c = compose(&doc);
    assert!(c.labels.placed >= 60, "placed {}", c.labels.placed);
    assert!(texts(&c).iter().any(|t| t == "Tokyo"));
    for u in &c.labels.unplaced {
        assert!(u.layer == "Rivers" || u.layer == "Places", "{u:?}");
        assert!(!u.reason.is_empty());
    }
}

#[test]
fn labels_off_and_hidden() {
    let mut doc = Document::sample();
    doc.frame.labels = false;
    let c = compose(&doc);
    assert_eq!(c.labels.placed, 0);
    // Only the scale bar text remains.
    assert!(texts(&c).iter().all(|t| t.ends_with(" km")));

    let mut doc = Document::sample();
    let id = layer_id(&doc, "Places");
    doc.set_visible(id, false);
    assert!(!texts(&compose(&doc)).iter().any(|t| t == "Tokyo"));
}

#[test]
fn rivers_off_is_upright() {
    let mut doc = Document::sample();
    let c = compose(&doc);
    assert!(texts(&c).iter().any(|t| t == "Nile" || t == "Amazon"));
    let id = layer_id(&doc, "Rivers");
    doc.entry_mut(id).unwrap().style.label.enabled = false;
    let c = compose(&doc);
    for i in &c.list.items {
        if let Item::Glyphs(g) = i {
            assert_eq!(g.font.data.as_ref().len(), REGULAR_LEN, "{}", g.text);
        }
    }
}

#[test]
fn other_label_field() {
    let mut doc = Document::sample();
    let id = layer_id(&doc, "Places");
    doc.entry_mut(id).unwrap().style.label.field = Some("adm0name".into());
    assert!(texts(&compose(&doc)).iter().any(|t| t == "Japan"));
}

#[test]
fn quality_and_determinism() {
    let doc = Document::sample();
    let i = compose_with(&doc, Quality::Interactive);
    assert!(i.labels.placed > 0);
    let a = compose_with(&doc, Quality::Final);
    let mut f = fonts().unwrap();
    let mut p = Pipeline::new();
    let b = p.compose(&doc, SIZE, &mut f, Quality::Final, None).unwrap();
    let c = p.compose(&doc, SIZE, &mut f, Quality::Final, None).unwrap();
    assert_eq!(b.list, c.list);
    assert!(a.labels.placed > 0);
}

#[test]
fn zoomed() {
    let mut doc = Document::sample();
    let args = Args::try_parse_from(["vale-app", "--look-at", "10,50", "--zoom", "6"]).unwrap();
    let mut p = Pipeline::new();
    apply_view(&args, &mut doc, &mut p, SIZE).unwrap();
    let mut f = fonts().unwrap();
    let c = p.compose(&doc, SIZE, &mut f, Quality::Final, None).unwrap();
    assert!(c.labels.placed >= 10, "placed {}", c.labels.placed);
    let page = Rect::new(0.0, 0.0, SIZE.0, SIZE.1);
    for i in &c.list.items {
        if let Item::Glyphs(g) = i {
            let t = g.glyphs[0].1.translation();
            assert!(page.contains(Point::new(t.x, t.y)), "{} at {t:?}", g.text);
        }
    }
}

#[test]
fn many_points() {
    let mut json = String::from(r#"{"type":"FeatureCollection","features":["#);
    for i in 0..5000 {
        let lon = -170.0 + (i % 100) as f64 * 3.4;
        let lat = -80.0 + (i / 100) as f64 * 3.2;
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"type":"Feature","properties":{{"name":"P{i}"}},"geometry":{{"type":"Point","coordinates":[{lon},{lat}]}}}}"#
        ));
    }
    json.push_str("]}");
    let imported = vale_import::geojson::read_str("Many", &json).unwrap();
    let mut doc = Document::empty();
    doc.add_imported(imported, None);
    let t = std::time::Instant::now();
    let c = compose(&doc);
    assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
    assert!(c.labels.placed >= 1);
}

#[test]
fn scale_bar_world() {
    let doc = Document::sample();
    let c = compose(&doc);
    let page = Rect::new(0.0, 0.0, SIZE.0, SIZE.1);
    let mut f = fonts().unwrap();
    let (items, rect) = scale_bar(page, c.km_per_point, Units::Metric, &mut f);
    assert!(!items.is_empty());
    assert!(page.contains_rect(rect));
    let text = items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs(g) => Some(g.text.clone()),
            _ => None,
        })
        .unwrap();
    let n: u64 = text.strip_suffix(" km").unwrap().parse().unwrap();
    assert!(n >= 1);
    // No label glyph lies inside the bar.
    let labels_only = {
        let mut d = doc.clone();
        d.frame.graticule = false;
        compose(&d)
    };
    for i in &labels_only.list.items {
        if let Item::Glyphs(g) = i {
            if g.text.ends_with(" km") {
                continue;
            }
            for (_, t) in &g.glyphs {
                let o = t.translation();
                assert!(!rect.contains(Point::new(o.x, o.y)), "{}", g.text);
            }
        }
    }

    let (items, _) = scale_bar(page, 0.004, Units::Metric, &mut f);
    let text = items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs(g) => Some(g.text.clone()),
            _ => None,
        })
        .unwrap();
    assert!(text.ends_with(" m") && !text.ends_with(" km"), "{text}");
    let (items, rect) = scale_bar(
        Rect::new(0.0, 0.0, 200.0, 100.0),
        10.0,
        Units::Metric,
        &mut f,
    );
    assert!(items.is_empty());
    assert_eq!(rect, Rect::ZERO);
}

#[test]
fn scale_bar_follows_radius() {
    let mut small = Document::sample();
    small.set_radius_km(1000.0);
    let a = compose(&Document::sample());
    let b = compose(&small);
    assert!(b.km_per_point < a.km_per_point);
    let bar = |c: &Composed| {
        texts(c)
            .into_iter()
            .find(|t| t.ends_with(" km"))
            .unwrap()
            .trim_end_matches(" km")
            .parse::<u64>()
            .unwrap()
    };
    assert!(bar(&b) < bar(&a));
}

#[test]
fn the_scale_bar_uses_the_display_units() {
    let mut doc = Document::sample();
    doc.project.world.units = Units::Imperial;
    let c = compose(&doc);
    let text = texts(&c).into_iter().find(|t| t.ends_with(" mi")).unwrap();
    let miles: f64 = text.trim_end_matches(" mi").parse().unwrap();
    assert_eq!(nice(miles), miles);
    assert!(texts(&c).iter().all(|t| !t.ends_with(" km")));

    // The bar is as long on the page as its distance on the ground.
    let page = Rect::new(0.0, 0.0, SIZE.0, SIZE.1);
    let mut f = fonts().unwrap();
    let (items, _) = scale_bar(page, c.km_per_point, Units::Imperial, &mut f);
    let length = items
        .iter()
        .find_map(|i| match i {
            Item::Stroke { path, .. } => Some(path.bounding_box().width()),
            _ => None,
        })
        .unwrap();
    let km = miles * 1.609_344;
    assert!((length * c.km_per_point - km).abs() < km * 1e-9);

    let (items, _) = scale_bar(page, 0.0004, Units::Imperial, &mut f);
    let text = items.iter().find_map(|i| match i {
        Item::Glyphs(g) => Some(g.text.clone()),
        _ => None,
    });
    assert_eq!(text.as_deref(), Some("100 ft"));
}

#[test]
fn nice_values() {
    assert_eq!(nice(140.0 * 31.4), 2000.0);
    assert_eq!(nice(0.7), 0.5);
    assert_eq!(nice(1.0), 1.0);
}

#[test]
fn picking() {
    let mut doc = Document::sample();
    let mut p = Pipeline::new();
    let name_of = |doc: &Document, s: Selection| {
        let l = doc.layer(s.layer).unwrap();
        (
            l.name.clone(),
            l.features[s.feature].attributes.get("name").cloned(),
        )
    };
    let tokyo = place_named(&doc, "Places", "Tokyo");
    let at = p.lonlat_to_page(&doc, SIZE, tokyo).unwrap().unwrap();
    let hit = p.pick(&doc, SIZE, at).unwrap().unwrap();
    assert_eq!(
        name_of(&doc, hit),
        ("Places".to_string(), Some(Value::Text("Tokyo".into())))
    );
    let land = |doc: &Document, ll: [f64; 2], p: &mut Pipeline| {
        let at = p.lonlat_to_page(doc, SIZE, ll).unwrap().unwrap();
        p.pick(doc, SIZE, at).unwrap()
    };
    let hit = land(&doc, [25.0, 25.0], &mut p).unwrap();
    assert_eq!(doc.layer(hit.layer).unwrap().name, "Land");
    assert_eq!(land(&doc, [-27.0, -22.0], &mut p), None);

    let moscow = place_named(&doc, "Places", "Moscow");
    let id = layer_id(&doc, "Places");
    doc.set_visible(id, false);
    let hit = land(&doc, moscow, &mut p).unwrap();
    assert_eq!(doc.layer(hit.layer).unwrap().name, "Land");
}

fn feature(g: Geometry) -> Feature {
    Feature::new(g, Default::default())
}

fn pf(
    index: usize,
    points: Vec<Point>,
    lines: Vec<Vec<Point>>,
    rings: Vec<Vec<Point>>,
) -> PageFeature {
    let mut b = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in points
        .iter()
        .chain(lines.iter().flatten())
        .chain(rings.iter().flatten())
    {
        b = Rect::new(b.x0.min(p.x), b.y0.min(p.y), b.x1.max(p.x), b.y1.max(p.y));
    }
    PageFeature {
        index,
        points,
        lines,
        rings,
        bbox: b,
    }
}

fn hand_made(
    kind: GeometryKind,
    geometry: Geometry,
    f: PageFeature,
) -> (vale_store::Layer, LayerEntry, PageFeature) {
    let mut project = vale_store::Project::new(vale_store::World::default());
    let id = project.add_layer("T".into(), kind, None, vec![feature(geometry)]);
    let layer = project.layer(id).unwrap().clone();
    let entry = LayerEntry {
        layer: id,
        style: LayerStyle::default_for(kind, 0, &[]),
    };
    (layer, entry, f)
}

fn hit(kind: GeometryKind, geometry: Geometry, f: PageFeature, at: Point) -> bool {
    let (layer, entry, f) = hand_made(kind, geometry, f);
    let layers = [PageLayer {
        entry: &entry,
        layer: &layer,
        kind,
        features: vec![f],
        scale: 1.0e7,
    }];
    pick(&layers, at).is_some()
}

#[test]
fn pick_hand_made() {
    let pt = || pf(0, vec![Point::new(100.0, 100.0)], vec![], vec![]);
    let g = || Geometry::Points(vec![[0.0, 0.0]]);
    assert!(hit(
        GeometryKind::Point,
        g(),
        pt(),
        Point::new(104.0, 100.0)
    ));
    assert!(!hit(
        GeometryKind::Point,
        g(),
        pt(),
        Point::new(120.0, 100.0)
    ));

    let line = || {
        pf(
            0,
            vec![],
            vec![vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)]],
            vec![],
        )
    };
    let g = || Geometry::Lines(vec![vec![[0.0, 0.0], [1.0, 0.0]]]);
    assert!(hit(GeometryKind::Line, g(), line(), Point::new(50.0, 3.0)));
    assert!(!hit(
        GeometryKind::Line,
        g(),
        line(),
        Point::new(50.0, 10.0)
    ));

    let sq = |a: f64, b: f64| {
        vec![
            Point::new(a, a),
            Point::new(b, a),
            Point::new(b, b),
            Point::new(a, b),
        ]
    };
    let poly = || pf(0, vec![], vec![], vec![sq(0.0, 100.0), sq(30.0, 70.0)]);
    let g = || Geometry::Polygons(vec![]);
    assert!(hit(
        GeometryKind::Polygon,
        g(),
        poly(),
        Point::new(10.0, 50.0)
    ));
    assert!(!hit(
        GeometryKind::Polygon,
        g(),
        poly(),
        Point::new(50.0, 50.0)
    ));
}

/// Adds a layer of GeoJSON features and returns its ID.
fn add_layer(doc: &mut Document, name: &str, features: &[String]) -> LayerId {
    let text = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );
    let imported = vale_import::geojson::read_str(name, &text).unwrap();
    doc.add_imported(imported, None)[0]
}

fn point_at(name: Option<&str>, [lon, lat]: [f64; 2]) -> String {
    let props = name.map_or(String::new(), |n| format!(r#""name":"{n}""#));
    format!(
        r#"{{"type":"Feature","properties":{{{props}}},"geometry":{{"type":"Point","coordinates":[{lon},{lat}]}}}}"#
    )
}

/// A document with one city, in a view that shows 10 degrees.
fn city_doc(p: &mut Pipeline) -> Document {
    let mut doc = Document::empty();
    doc.frame.graticule = false;
    add_layer(&mut doc, "Cities", &[point_at(Some("Oakton"), [0.0, 0.0])]);
    let at = |doc: &Document, p: &mut Pipeline, lon: f64| {
        p.project_lonlat(doc, [lon, 0.0]).unwrap().unwrap()
    };
    let (a, b) = (at(&doc, p, -5.0), at(&doc, p, 5.0));
    doc.frame.view = Some(vale_app::view::View {
        center: at(&doc, p, 0.0),
        scale: SIZE.0 / (b.x - a.x),
        rotation: 0.0,
    });
    doc
}

/// The box of the halo of each label.
fn halos(c: &Composed) -> Vec<Rect> {
    c.list
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Stroke { path, color, .. } if (color.r, color.a) == (255, 217) => {
                Some(path.bounding_box())
            }
            _ => None,
        })
        .collect()
}

fn compose_in(doc: &Document, p: &mut Pipeline) -> Composed {
    let mut f = fonts().unwrap();
    p.compose(doc, SIZE, &mut f, Quality::Final, None).unwrap()
}

#[test]
fn labels_avoid_point_symbols() {
    let mut p = Pipeline::new();
    let mut doc = city_doc(&mut p);
    let alone = compose_in(&doc, &mut p);
    assert_eq!(oakton(&alone), 1);
    let label = halos(&alone)[0];

    // A castle on another layer, with no labels, where the label was.
    let place = p
        .page_to_lonlat(&doc, SIZE, label.center())
        .unwrap()
        .unwrap();
    common::add_symbols(&mut doc, &["castle"]);
    let castles = add_layer(&mut doc, "Castles", &[point_at(None, place)]);
    let entry = doc.entry_mut(castles).unwrap();
    entry.style.label.enabled = false;
    entry.style.style = Style {
        rules: vec![Rule::new(
            "Castle",
            vec![Symbolizer::Symbol(PointSymbolizer::constant(
                "castle", 14.0,
            ))],
        )],
    };
    let c = compose_in(&doc, &mut p);
    let castle = c.symbols.iter().find(|s| s.layer == castles).unwrap();
    assert!(castle.bounds.overlaps(label));
    assert_eq!(oakton(&c), 1);
    let moved = halos(&c)[0];
    assert!(
        !moved.overlaps(castle.bounds),
        "{moved:?} {:?}",
        castle.bounds
    );

    // A city in a dense forest still has its label, over the trees.
    let mut doc = city_doc(&mut p);
    common::add_symbols(&mut doc, &["tree"]);
    let forest = add_layer(
        &mut doc,
        "Forest",
        &[
            r#"{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[-4,-3],[4,-3],[4,3],[-4,3],[-4,-3]]]}}"#
                .to_string(),
        ],
    );
    doc.entry_mut(forest).unwrap().style.style = Style {
        rules: vec![Rule::new(
            "Trees",
            vec![Symbolizer::Scatter(ScatterSymbolizer::new(
                "tree", 10.0, 10.0,
            ))],
        )],
    };
    // The forest draws under the cities.
    doc.move_layer(forest, 0);
    let c = compose_in(&doc, &mut p);
    let trees: Vec<Rect> = c
        .symbols
        .iter()
        .filter(|s| s.layer == forest)
        .map(|s| s.bounds)
        .collect();
    assert!(trees.len() > 1000, "{}", trees.len());
    assert_eq!(oakton(&c), 1);
}

/// The count of the labels of the city. The labels come before the scale bar.
fn oakton(c: &Composed) -> usize {
    texts(c).iter().filter(|t| *t == "Oakton").count()
}
