//! Presets and label input construction.

use std::path::Path;

use anyhow::{Context, bail};
use kurbo::{BezPath, Point, Rect};
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, LinePlacement, Options, Placement,
    PointPlacement, TextStyle,
};

use crate::data::{join_rivers, load_places, load_polygons, load_rivers};
use crate::project::{Frame, PageMap, Projection};

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    World,
    Europe,
}

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::World => "world",
            Preset::Europe => "europe",
        }
    }
}

pub struct Scene {
    pub width: u16,
    pub height: u16,
    /// One closed path per polygon, holes included.
    pub land: Vec<BezPath>,
    /// Path and stroke width.
    pub rivers: Vec<(BezPath, f64)>,
    /// Position, symbol radius, capital.
    pub places: Vec<(Point, f64, bool)>,
    pub input: LabelInput,
}

/// A font registry with both bundled Noto Sans files.
pub fn fonts() -> anyhow::Result<Fonts> {
    let mut fonts = Fonts::new();
    fonts.register(include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf").to_vec())?;
    fonts.register(include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf").to_vec())?;
    Ok(fonts)
}

fn open_path(points: &[Point]) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(*p);
        } else {
            path.line_to(*p);
        }
    }
    path
}

pub fn build(preset: Preset, data_dir: &Path, seed: u64, improve: bool) -> anyhow::Result<Scene> {
    let (dir, res, projection, extent, width, height, margin) = match preset {
        Preset::World => (
            "110m",
            "110m",
            Projection::EqualEarth { lon0: 0.0 },
            (-180.0, 180.0, -90.0, 90.0),
            1800u16,
            900u16,
            20.0,
        ),
        Preset::Europe => (
            "50m",
            "50m",
            Projection::Laea {
                lon0: 15.0,
                lat0: 52.0,
            },
            (-12.0, 42.0, 35.0, 62.0),
            1600u16,
            1200u16,
            0.0,
        ),
    };
    let base = data_dir.join(dir);
    let file = |kind: &str| base.join(format!("ne_{res}_{kind}.geojson"));
    for kind in ["populated_places_simple", "rivers_lake_centerlines", "land"] {
        if !file(kind).exists() {
            if preset == Preset::Europe {
                bail!(
                    "missing {}. Run scripts/fetch-natural-earth.sh 50m",
                    file(kind).display()
                );
            }
            bail!("missing {}", file(kind).display());
        }
    }
    let places = load_places(&file("populated_places_simple")).context("places")?;
    let rivers = join_rivers(load_rivers(&file("rivers_lake_centerlines")).context("rivers")?);
    let polygons = load_polygons(&file("land")).context("land")?;

    let map = PageMap::new(&Frame {
        projection,
        west: extent.0,
        east: extent.1,
        south: extent.2,
        north: extent.3,
        page_width: f64::from(width),
        page_height: f64::from(height),
        margin,
    });
    let page = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));

    let mut land = Vec::new();
    for polygon in &polygons {
        let mut path = BezPath::new();
        for ring in &polygon.rings {
            let Some(points) = map.ring_to_page(ring) else {
                continue;
            };
            if points.len() < 3 {
                continue;
            }
            path.extend(open_path(&points));
            path.close_path();
        }
        if !path.elements().is_empty() {
            land.push(path);
        }
    }

    let classes = vec![
        LabelClass {
            name: "capitals".into(),
            style: TextStyle::new("Noto Sans", 13.0),
            priority: 3,
            margin: 1.0,
            placement: Placement::Point(PointPlacement::default()),
        },
        LabelClass {
            name: "rivers".into(),
            style: TextStyle {
                italic: true,
                letter_spacing: 0.5,
                ..TextStyle::new("Noto Sans", 12.0)
            },
            priority: 2,
            margin: 1.0,
            placement: Placement::Line(LinePlacement {
                repeat_distance: Some(700.0),
                offset: 4.0,
                ..Default::default()
            }),
        },
        LabelClass {
            name: "cities".into(),
            style: TextStyle::new("Noto Sans", 10.0),
            priority: 1,
            margin: 1.0,
            placement: Placement::Point(PointPlacement::default()),
        },
    ];

    let mut features = Vec::new();
    let mut scene_places = Vec::new();
    for (index, place) in places.iter().enumerate() {
        let Some(position) = map.to_page(place.lon, place.lat) else {
            continue;
        };
        if !page.contains(position) {
            continue;
        }
        let radius = if place.capital { 3.0 } else { 2.0 };
        scene_places.push((position, radius, place.capital));
        features.push(Feature {
            id: index as u64,
            class: if place.capital { 0 } else { 2 },
            text: place.name.clone(),
            priority: place.pop_max,
            geometry: Geometry::Point {
                position,
                symbol_radius: radius,
            },
        });
    }

    let mut scene_rivers = Vec::new();
    let mut counter = 0u64;
    for river in &rivers {
        for part in &river.parts {
            for path in map.line_to_page(part) {
                if path.len() < 2 {
                    continue;
                }
                scene_rivers.push((open_path(&path), 1.2));
                features.push(Feature {
                    id: 1_000_000 + counter,
                    class: 1,
                    text: river.name.clone(),
                    priority: -(river.scalerank as f64),
                    geometry: Geometry::Line {
                        path,
                        stroke_width: 1.2,
                    },
                });
                counter += 1;
            }
        }
    }

    Ok(Scene {
        width,
        height,
        land,
        rivers: scene_rivers,
        places: scene_places,
        input: LabelInput {
            bounds: page,
            classes,
            features,
            obstacles: Vec::new(),
            options: Options {
                seed,
                improve,
                duplicate_distance: Some(400.0),
            },
        },
    })
}
