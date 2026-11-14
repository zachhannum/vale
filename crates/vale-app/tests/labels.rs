mod common;

use clap::Parser;
use kurbo::{Point, Rect, Shape};
use vale_app::atlas::{FrameTarget, PT_PER_MM, new_frame};
use vale_app::cli::{Args, apply_view};
use vale_app::document::{Document, LayerEntry};
use vale_app::furniture::{nice, scale_bar};
use vale_app::pick::pick;
use vale_app::pipeline::{
    Composed, FrameDetail, PageFeature, PageLayer, Pipeline, Quality, Selection, fonts,
};
use vale_render::Item;
use vale_store::{Feature, Geometry, GeometryKind, LayerId, Units, Value};
use vale_style::{LayerStyle, PointSymbolizer, Rule, ScatterSymbolizer, Style, Symbolizer};

const SIZE: (f64, f64) = (1280.0, 800.0);
const REGULAR_LEN: usize = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf").len();

fn compose_with(doc: &Document, quality: Quality) -> Composed {
    let mut f = fonts().unwrap();
    Pipeline::new().compose(doc, SIZE, &mut f, quality).unwrap()
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
    doc.entry_mut(id).unwrap().style.label.text = "adm0name".into();
    assert!(texts(&compose(&doc)).iter().any(|t| t == "Japan"));
}

#[test]
fn a_label_text_expression_gives_the_labels() {
    let mut doc = Document::sample();
    let id = layer_id(&doc, "Places");
    let label = &mut doc.entry_mut(id).unwrap().style.label;
    label.text = "concat(name, ' in ' || adm0name)".into();
    assert!(texts(&compose(&doc)).iter().any(|t| t == "Tokyo in Japan"));
    // The prefix and the suffix of a field with no value do not show.
    let label = &mut doc.entry_mut(id).unwrap().style.label;
    label.text = "concat(name, ' (' || no_field || ')')".into();
    let texts = texts(&compose(&doc));
    assert!(texts.iter().any(|t| t == "Tokyo"));
    assert!(texts.iter().all(|t| !t.contains('(')));
}

#[test]
fn quality_and_determinism() {
    let doc = Document::sample();
    let i = compose_with(&doc, Quality::Interactive);
    assert!(i.labels.placed > 0);
    let a = compose_with(&doc, Quality::Final);
    let mut f = fonts().unwrap();
    let mut p = Pipeline::new();
    let b = p.compose(&doc, SIZE, &mut f, Quality::Final).unwrap();
    let c = p.compose(&doc, SIZE, &mut f, Quality::Final).unwrap();
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
    let c = p.compose(&doc, SIZE, &mut f, Quality::Final).unwrap();
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
        entry: std::borrow::Cow::Borrowed(&entry),
        layer: &layer,
        kind,
        features: vec![f],
        scale: 1.0e7,
        radius_km: 6371.0,
        rasters: None,
    }];
    pick(&layers, at).is_some()
}

#[test]
fn a_tap_at_the_corner_of_a_square_marker_picks_the_point() {
    use vale_style::{MarkerShape, Symbolizer};
    let at = Point::new(118.0, 118.0);
    let picks = |shape: MarkerShape| {
        let pt = pf(0, vec![Point::new(100.0, 100.0)], vec![], vec![]);
        let g = Geometry::Points(vec![[0.0, 0.0]]);
        let (layer, mut entry, f) = hand_made(GeometryKind::Point, g, pt);
        for s in &mut entry.style.style.rules[0].symbolizers {
            if let Symbolizer::Marker {
                radius, shape: s, ..
            } = s
            {
                *radius = vale_style::expr::Property::constant(20.0);
                *s = shape;
            }
        }
        let layers = [PageLayer {
            entry: std::borrow::Cow::Borrowed(&entry),
            layer: &layer,
            kind: GeometryKind::Point,
            features: vec![f],
            scale: 1.0e7,
            radius_km: 6371.0,
            rasters: None,
        }];
        pick(&layers, at).is_some()
    };
    // The corner is 25.5 points from the center: inside the square, outside the circle.
    assert!(picks(MarkerShape::Square));
    assert!(!picks(MarkerShape::Circle));
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
    p.compose(doc, SIZE, &mut f, Quality::Final).unwrap()
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
        mask: None,
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
        mask: None,
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

/// The sample document with a frame of the first map. Only the places have
/// labels in the map, with the class that `edit` changes. Each place has a
/// castle symbol.
fn places_frame(edit: impl FnOnce(&mut vale_style::LabelClass)) -> (Document, Composed) {
    let mut doc = Document::sample();
    let mut p = Pipeline::new();
    let map = doc.first_map();
    let places = layer_id(&doc, "Places");
    common::add_symbols(&mut doc, &["castle"]);
    doc.entry_mut(places).unwrap().style.style = Style {
        mask: None,
        rules: vec![Rule::new(
            "Castle",
            vec![Symbolizer::Symbol(PointSymbolizer::constant("castle", 6.0))],
        )],
    };
    let ids: Vec<LayerId> = doc.frame.entries.iter().map(|e| e.layer).collect();
    for id in ids.into_iter().filter(|id| *id != places) {
        doc.set_label_classes(map, id, &[]);
    }
    let mut classes = doc.label_classes(Some(map), places);
    assert_eq!(classes.len(), 1);
    edit(&mut classes[0]);
    assert!(doc.set_label_classes(map, places, &classes));
    assert!(!doc.set_label_classes(map, places, &classes));
    let (c, _) = first_map_frame(&mut doc, &mut p);
    (doc, c)
}

/// Adds a frame of the first map to the first page, and composes it. Returns
/// the frame box in points too.
fn first_map_frame(doc: &mut Document, p: &mut Pipeline) -> (Composed, Rect) {
    let map = doc.first_map();
    let page = doc.project.atlas.pages[0].id;
    let atlas = &doc.project.atlas;
    let projection = p.projection(doc, map).unwrap();
    let frame = new_frame(atlas.page(page).unwrap(), &atlas.maps[0], projection);
    let id = doc.project.atlas.add_frame(page, frame).unwrap();
    let atlas = &doc.project.atlas;
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    let record = atlas.frame(id).unwrap();
    let c = p
        .compose_frame(
            doc,
            atlas.page(page).unwrap(),
            record,
            target,
            &mut f,
            Quality::Final,
        )
        .unwrap();
    let vale_store::FrameShape::Rectangle([x0, y0, x1, y1]) = record.shape else {
        panic!("not a rectangle");
    };
    let rect = Rect::new(x0, y0, x1, y1).scale_from_origin(PT_PER_MM);
    (c, rect)
}

#[test]
fn a_polygon_class_places_its_label_inside_the_frame() {
    use vale_style::LabelPlacement;
    for placement in [
        LabelPlacement::BestFit,
        LabelPlacement::Curved,
        LabelPlacement::Horizontal,
    ] {
        let mut doc = Document::sample();
        let mut p = Pipeline::new();
        let map = doc.first_map();
        let sea = add_layer(
            &mut doc,
            "Seas",
            &[r#"{"type":"Feature","properties":{"name":"Long Sea"},"geometry":{"type":"Polygon","coordinates":[[[-60,-8],[60,-8],[60,8],[-60,8],[-60,-8]],[[-5,-2],[5,-2],[5,2],[-5,2],[-5,-2]]]}}"#
                .to_string()],
        );
        let ids: Vec<LayerId> = doc.frame.entries.iter().map(|e| e.layer).collect();
        for id in ids.into_iter().filter(|id| *id != sea) {
            doc.set_label_classes(map, id, &[]);
        }
        let mut class = vale_style::LabelClass::new("Seas", GeometryKind::Polygon, 0).unwrap();
        class.placement = placement;
        assert!(doc.set_label_classes(map, sea, &[class]));
        let (c, frame) = first_map_frame(&mut doc, &mut p);
        let runs: Vec<&vale_render::GlyphRun> = (c.list.items.iter())
            .filter_map(|i| match i {
                Item::Glyphs(g) if g.text == "LONG SEA" => Some(g),
                _ => None,
            })
            .collect();
        assert!(!runs.is_empty(), "{placement:?} {:?}", c.labels.unplaced);
        let origins: Vec<Point> = (runs.iter())
            .flat_map(|g| g.glyphs.iter().map(|(_, t)| Point::ZERO + t.translation()))
            .collect();
        for o in &origins {
            assert!(frame.contains(*o), "{placement:?} {o:?} {frame:?}");
        }
        let ys = origins.iter().map(|o| o.y);
        let rise = ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min);
        if placement == LabelPlacement::Horizontal {
            assert!(rise < 0.01, "{rise}");
        }
    }
}

#[test]
fn the_right_position_alone_puts_point_labels_right_of_the_point() {
    use vale_style::LabelPosition;
    let (doc, c) = places_frame(|class| {
        for rank in &mut class.positions {
            rank.on = rank.position == LabelPosition::Right;
        }
        assert!(class.positions.iter().any(|r| r.on));
    });
    let places = layer_id(&doc, "Places");
    let layer = doc.layer(places).unwrap();
    let mut checked = 0;
    for item in &c.list.items {
        let Item::Glyphs(g) = item else {
            continue;
        };
        let start = g.glyphs[0].1.translation().x;
        let centers: Vec<f64> = (c.symbols.iter())
            .filter(|s| s.layer == places)
            .filter(|s| {
                layer.features[s.feature].attributes.get("name")
                    == Some(&Value::Text(g.text.clone()))
            })
            .map(|s| s.center.x)
            .collect();
        assert!(!centers.is_empty(), "{}", g.text);
        assert!(centers.iter().any(|x| start > *x), "{}", g.text);
        checked += 1;
    }
    assert!(checked >= 20, "{checked}");
}

#[test]
fn a_class_filter_and_expression_choose_the_text() {
    let (doc, c) = places_frame(|class| {
        class.text = "upper(name)".into();
        class.filter = "adm0name = 'Japan'".into();
    });
    let layer = doc.layer(layer_id(&doc, "Places")).unwrap();
    let japan: Vec<String> = (layer.features.iter())
        .filter(|f| f.attributes.get("adm0name") == Some(&Value::Text("Japan".into())))
        .filter_map(|f| f.attributes.get("name")?.label())
        .map(|n| n.to_uppercase())
        .collect();
    let texts = texts(&c);
    assert!(texts.iter().any(|t| t == "TOKYO"), "{texts:?}");
    for t in &texts {
        assert!(japan.contains(t), "{t}");
    }
}

#[test]
fn a_class_without_halo_draws_no_halo() {
    let (_, c) = places_frame(|_| {});
    assert!(!halos(&c).is_empty());
    let (_, c) = places_frame(|class| class.halo = false);
    assert!(!texts(&c).is_empty());
    assert!(halos(&c).is_empty());
}

/// The glyph runs of each label text.
fn runs(c: &Composed) -> std::collections::HashMap<String, Vec<&vale_render::GlyphRun>> {
    let mut out: std::collections::HashMap<String, Vec<&vale_render::GlyphRun>> =
        Default::default();
    for item in &c.list.items {
        if let Item::Glyphs(g) = item {
            out.entry(g.text.clone()).or_default().push(g);
        }
    }
    out
}

/// The distance from the first glyph of a label to its last glyph.
fn span(runs: &[&vale_render::GlyphRun]) -> f64 {
    let xs: Vec<f64> = (runs.iter())
        .flat_map(|g| g.glyphs.iter().map(|(_, t)| t.translation().x))
        .collect();
    let (lo, hi) = xs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    hi - lo
}

#[test]
fn small_caps_and_letter_spacing_reach_the_labeler() {
    use vale_style::TextCase;
    // With no fallbacks, each label has the size of the class.
    let no_fallbacks = |c: &mut vale_style::LabelClass| c.fallbacks.clear();
    let (_, base) = places_frame(no_fallbacks);
    let (_, caps) = places_frame(|c| {
        no_fallbacks(c);
        c.case = TextCase::AllSmallCaps;
    });
    let (_, small) = places_frame(|c| {
        no_fallbacks(c);
        c.case = TextCase::SmallCaps;
    });
    let (_, spaced) = places_frame(|c| {
        no_fallbacks(c);
        c.letter_spacing = 0.2;
    });
    // Noto Sans has small capitals, so the glyphs change and the size stays.
    let ids = |runs: &[&vale_render::GlyphRun]| -> Vec<u32> {
        runs.iter()
            .flat_map(|g| g.glyphs.iter().map(|(id, _)| *id))
            .collect()
    };
    let base = runs(&base);
    assert!(base.values().flatten().all(|g| g.font_size == 11.0));
    for case in [runs(&caps), runs(&small)] {
        let mut checked = 0;
        for (text, r) in &case {
            let Some(b) = base.get(text) else { continue };
            assert_ne!(ids(r), ids(b), "{text}");
            checked += 1;
        }
        assert!(checked >= 10, "{checked}");
    }
    // A capital stays in small caps and changes in all small caps.
    let (caps, small) = (runs(&caps), runs(&small));
    let first = |runs: &[&vale_render::GlyphRun]| ids(runs)[0];
    let mut same = 0;
    for (text, r) in &small {
        let Some(b) = base.get(text) else { continue };
        if text.starts_with(char::is_uppercase) && first(r) == first(b) {
            same += 1;
            if let Some(c) = caps.get(text) {
                assert_ne!(first(c), first(b), "{text}");
            }
        }
    }
    assert!(same >= 10, "{same}");

    let spaced = runs(&spaced);
    let mut checked = 0;
    for (text, r) in &spaced {
        let Some(b) = base.get(text) else { continue };
        if text.chars().count() < 3 || r.len() != b.len() {
            continue;
        }
        assert!(span(r) > span(b) + 1.0, "{text}");
        checked += 1;
    }
    assert!(checked >= 10, "{checked}");
}

#[test]
fn the_default_positions_match_the_labeler() {
    let ours = vale_style::LabelPosition::DEFAULT_ORDER.map(vale_app::labels::point_position);
    assert_eq!(ours, vale_labeler::PointPosition::DEFAULT_ORDER);
}

#[test]
fn a_conic_frame_gives_the_labeler_curved_parallels() {
    use vale_sphere::{ProjectionKind, ProjectionSpec};
    for follow in [true, false] {
        let mut doc = Document::sample();
        let mut p = Pipeline::new();
        let map = doc.first_map();
        let mut spec = ProjectionSpec::new(ProjectionKind::LambertConformal, 10.0, 40.0);
        spec.lat1 = 30.0;
        spec.lat2 = 50.0;
        doc.set_projection(map, spec);
        let places = layer_id(&doc, "Places");
        let mut classes = doc.label_classes(Some(map), places);
        classes[0].graticule = follow;
        doc.set_label_classes(map, places, &classes);
        let (_, rect) = first_map_frame(&mut doc, &mut p);
        let (_, frame) = doc.project.atlas.frames_of_map(map).next().unwrap();
        let target = FrameTarget {
            origin: Point::ZERO,
            pts_per_mm: PT_PER_MM,
        };
        let parallels = p.label_parallels(&doc, frame, target).unwrap();
        if !follow {
            assert!(parallels.is_empty());
            continue;
        }
        // The parts of the parallels that cross most of the frame.
        let across: Vec<Vec<Point>> = (parallels.iter())
            .map(|l| l.iter().copied().filter(|q| rect.contains(*q)).collect())
            .filter(|l: &Vec<Point>| {
                l.len() >= 3 && (l[l.len() - 1].x - l[0].x).abs() > rect.width() / 2.0
            })
            .collect();
        assert!(!across.is_empty(), "{} parallels", parallels.len());
        for l in &across {
            let (a, b) = (l[0], l[l.len() - 1]);
            assert!(a.x < b.x, "not west to east: {a:?} {b:?}");
            // A curved line leaves its chord.
            let chord = b - a;
            let bow = (l.iter())
                .map(|q| ((*q - a).cross(chord) / chord.hypot()).abs())
                .fold(0.0, f64::max);
            assert!(bow > 1.0, "straight: {bow}");
        }
    }
}

/// The text and the first glyph origin of each label, moved by `shift`.
fn label_spots(c: &Composed, shift: kurbo::Vec2) -> Vec<(String, i64, i64)> {
    let mut spots: Vec<_> = (c.list.items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(g) => {
                let o = Point::ZERO + g.glyphs.first()?.1.translation() + shift;
                Some((
                    g.text.clone(),
                    (o.x * 100.0).round() as i64,
                    (o.y * 100.0).round() as i64,
                ))
            }
            _ => None,
        })
        .collect();
    spots.sort();
    spots
}

/// The frame that `places_frame` made, composed in `part` of a target with
/// its page at `origin`.
fn compose_part(doc: &Document, origin: Point, part: Rect) -> Composed {
    let atlas = &doc.project.atlas;
    let frame = atlas.pages[0].frames.last().unwrap();
    let target = FrameTarget {
        origin,
        pts_per_mm: PT_PER_MM,
    };
    let mut f = fonts().unwrap();
    Pipeline::new()
        .compose_frame_part(
            doc,
            frame,
            target,
            part,
            &mut f,
            Quality::Final,
            FrameDetail::Full,
        )
        .unwrap()
}

#[test]
fn each_part_of_a_frame_shows_the_labels_of_the_whole_frame() {
    let (doc, whole) = places_frame(|_| {});
    let all = label_spots(&whole, kurbo::Vec2::ZERO);
    assert!(all.len() > 3, "{all:?}");
    let frame = &doc.project.atlas.pages[0].frames.last().unwrap();
    let b = vale_app::atlas::shape_bbox(&frame.shape).scale_from_origin(PT_PER_MM);
    let (w, h) = (b.width(), b.height());
    let parts = [
        Rect::new(b.x0, b.y0, b.x0 + w / 2.0, b.y1),
        Rect::new(b.x0 + w / 2.0, b.y0, b.x1, b.y1),
        Rect::new(
            b.x0 + w / 4.0,
            b.y0 + h / 4.0,
            b.x0 + w / 2.0,
            b.y0 + h / 2.0,
        ),
    ];
    for part in parts {
        let c = compose_part(&doc, Point::ZERO, part);
        assert_eq!(label_spots(&c, kurbo::Vec2::ZERO), all, "{part:?}");
    }
    // The canvas puts the top-left corner of the part at 0, 0.
    let part = parts[2];
    let shift = kurbo::Vec2::new(-part.x0, -part.y0);
    let c = compose_part(&doc, Point::ZERO + shift, part + shift);
    assert_eq!(label_spots(&c, -shift), all);
}

#[test]
fn a_label_avoids_a_symbol_outside_the_part() {
    let (doc, whole) = places_frame(|_| {});
    let castles: Vec<Rect> = whole.symbols.iter().map(|s| s.bounds).collect();
    let frame = &doc.project.atlas.pages[0].frames.last().unwrap();
    let b = vale_app::atlas::shape_bbox(&frame.shape).scale_from_origin(PT_PER_MM);
    let part = Rect::new(b.x0, b.y0, b.center().x, b.y1);
    let c = compose_part(&doc, Point::ZERO, part);
    let labels = halos(&c);
    assert_eq!(labels.len(), halos(&whole).len());
    assert!(castles.iter().any(|s| !s.overlaps(part)));
    for label in labels {
        for castle in &castles {
            assert!(!label.overlaps(*castle), "{label:?} {castle:?}");
        }
    }
}

/// A document with a polygon from 80° W to 80° E, with a label class, and
/// a hidden polygon layer west of 0°.
fn half_land() -> (Document, LayerId, LayerId) {
    let mut doc = Document::empty();
    let map = doc.first_map();
    let land = add_layer(
        &mut doc,
        "Lands",
        &[r#"{"type":"Feature","properties":{"name":"Half Land"},"geometry":{"type":"Polygon","coordinates":[[[-80,-30],[80,-30],[80,30],[-80,30],[-80,-30]]]}}"#
            .to_string()],
    );
    let mask = add_layer(
        &mut doc,
        "Mask",
        // The small square in the east makes the data, and thus the frame,
        // even about 0 degrees.
        &[r#"{"type":"Feature","properties":{},"geometry":{"type":"MultiPolygon","coordinates":[[[[-100,-50],[0,-50],[0,50],[-100,50],[-100,-50]]],[[[99,0],[100,0],[100,1],[99,1],[99,0]]]]}}"#
            .to_string()],
    );
    doc.set_visible(mask, false);
    doc.set_shown(map, mask, false);
    doc.set_label_classes(map, mask, &[]);
    let mut class = vale_style::LabelClass::new("Lands", GeometryKind::Polygon, 0).unwrap();
    class.placement = vale_style::LabelPlacement::Horizontal;
    assert!(doc.set_label_classes(map, land, &[class]));
    (doc, land, mask)
}

/// The origins of the glyphs of the label of `half_land`.
fn half_land_glyphs(c: &Composed) -> Vec<Point> {
    (c.list.items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(g) if g.text.to_uppercase() == "HALF LAND" => Some(g),
            _ => None,
        })
        .flat_map(|g| g.glyphs.iter().map(|(_, t)| Point::ZERO + t.translation()))
        .collect()
}

/// Each point is west of 0° and east of 80° W, in the first frame.
fn assert_west_half(doc: &Document, p: &mut Pipeline, points: &[Point]) {
    let record = doc.project.atlas.pages[0].frames[0].clone();
    let projection = p.projection(doc, doc.first_map()).unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let vt = vale_app::atlas::frame_transform(projection, &record, target);
    for o in points {
        let [lon, _] = projection.inverse(vt.from_page(*o)).unwrap();
        assert!(lon > -80.0 && lon < 0.0, "{lon}");
    }
}

#[test]
fn a_masked_polygon_places_its_label_inside_the_mask() {
    let (mut doc, land, mask) = half_land();
    let mut p = Pipeline::new();
    doc.entry_mut(land).unwrap().style.style.mask =
        Some(vale_style::Mask::Polygons { layer: mask });
    let (c, _) = first_map_frame(&mut doc, &mut p);
    let origins = half_land_glyphs(&c);
    assert!(!origins.is_empty(), "{:?}", c.labels.unplaced);
    // Each glyph is west of 0 degrees, over the part of the polygon inside
    // the mask. With no mask, the label is across 0 degrees.
    assert_west_half(&doc, &mut p, &origins);
}

#[test]
fn a_polygon_half_outside_a_page_mask_places_its_label_inside_the_mask() {
    use vale_app::mask::PageMask;
    let (mut doc, land, mask) = half_land();
    let mut p = Pipeline::new();
    doc.entry_mut(land).unwrap().style.style.mask = Some(vale_style::Mask::Raster {
        layer: mask,
        min: Some(0.0),
        max: None,
        offset: 0.0,
    });
    // With no page mask yet, the polygon has no label.
    let (c, _) = first_map_frame(&mut doc, &mut p);
    assert!(half_land_glyphs(&c).is_empty());

    // A page mask west of 0 degrees.
    let record = doc.project.atlas.pages[0].frames[0].clone();
    let projection = p.projection(&doc, doc.first_map()).unwrap();
    let vt = vale_app::atlas::frame_transform(projection, &record, FrameTarget::MM);
    let x = vt.to_page(projection.forward([0.0, 0.0]).unwrap()).x;
    let west = vec![
        Point::new(-1000.0, -1000.0),
        Point::new(x, -1000.0),
        Point::new(x, 1000.0),
        Point::new(-1000.0, 1000.0),
    ];
    p.set_page_mask(record.id, land, Some(PageMask::new(vec![west])));
    let page = &doc.project.atlas.pages[0];
    let mut f = fonts().unwrap();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let c = p
        .compose_frame(&doc, page, &record, target, &mut f, Quality::Final)
        .unwrap();
    let origins = half_land_glyphs(&c);
    assert!(!origins.is_empty(), "{:?}", c.labels.unplaced);
    assert_west_half(&doc, &mut p, &origins);
}
