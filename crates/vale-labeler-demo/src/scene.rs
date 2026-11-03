//! Presets and label input construction.

use std::path::Path;

use anyhow::{Context, bail};
use kurbo::{BezPath, Point, Rect};
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, LinePlacement, Options, Placement,
    PointPlacement, PolygonMode, PolygonPlacement, TextStyle,
};

use crate::data::{Area, join_rivers, load_areas, load_places, load_polygons, load_rivers};
use crate::project::{Frame, PageMap, Projection};

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    World,
    Europe,
    /// Europe in a conic projection. The country names follow the parallels.
    Conic,
}

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::World => "world",
            Preset::Europe => "europe",
            Preset::Conic => "conic",
        }
    }
}

pub struct Scene {
    pub width: u16,
    pub height: u16,
    /// One closed path per polygon, holes included.
    pub land: Vec<BezPath>,
    /// The outlines of the countries.
    pub borders: Vec<BezPath>,
    /// Path and stroke width.
    pub rivers: Vec<(BezPath, f64)>,
    /// Position, symbol radius, capital.
    pub places: Vec<(Point, f64, bool)>,
    /// The parallels and meridians that are drawn.
    pub graticule: Vec<BezPath>,
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
        Preset::Conic => (
            "110m",
            "110m",
            Projection::Lcc {
                lon0: 10.0,
                lat0: 52.0,
                lat1: 35.0,
                lat2: 65.0,
            },
            (-12.0, 42.0, 34.0, 66.0),
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
    // The area names are optional, so the 50m set works without them.
    let areas = |kind: &str, name: &str, rank: &str| -> anyhow::Result<Vec<Area>> {
        if file(kind).exists() {
            load_areas(&file(kind), name, rank).context(kind.to_string())
        } else {
            Ok(Vec::new())
        }
    };
    let countries = areas("admin_0_countries", "NAME", "LABELRANK")?;
    let seas = areas("geography_marine_polys", "name", "scalerank")?;

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

    let mut classes = vec![
        LabelClass {
            name: "capitals".into(),
            style: TextStyle::new("Noto Sans", 13.0),
            priority: 3,
            margin: 1.0,
            placement: Placement::Point(PointPlacement::default()),
            duplicate_distance: Some(400.0),
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
            duplicate_distance: Some(400.0),
        },
        LabelClass {
            name: "cities".into(),
            style: TextStyle::new("Noto Sans", 10.0),
            priority: 1,
            margin: 1.0,
            placement: Placement::Point(PointPlacement::default()),
            duplicate_distance: Some(400.0),
        },
        LabelClass {
            name: "countries".into(),
            style: TextStyle {
                letter_spacing: 1.0,
                ..TextStyle::new("Noto Sans", 11.0)
            },
            priority: 2,
            margin: 2.0,
            placement: Placement::Polygon(PolygonPlacement::default()),
            duplicate_distance: None,
        },
        LabelClass {
            name: "seas".into(),
            style: TextStyle {
                italic: true,
                letter_spacing: 1.5,
                ..TextStyle::new("Noto Sans", 13.0)
            },
            priority: 4,
            margin: 2.0,
            placement: Placement::Polygon(PolygonPlacement {
                max_spread: 1.5,
                ..Default::default()
            }),
            duplicate_distance: None,
        },
    ];

    let mut parallels = Vec::new();
    let mut graticule = Vec::new();
    if preset == Preset::Conic {
        classes[3].placement = Placement::Polygon(PolygonPlacement {
            mode: PolygonMode::Parallel,
            max_spread: 0.8,
            graticule: true,
            ..Default::default()
        });
        parallels = parallel_lines(&map, page, projection);
        graticule = graticule_lines(&map, projection);
    }

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

    let rings = |area: &Area| -> Vec<Vec<Point>> {
        area.parts
            .iter()
            .flat_map(|p| &p.rings)
            .filter_map(|r| map.ring_to_page(r))
            .filter(|r| r.len() >= 3)
            .collect()
    };
    let mut borders = Vec::new();
    for (index, country) in countries.iter().enumerate() {
        let rings = rings(country);
        for ring in &rings {
            let mut path = open_path(ring);
            path.close_path();
            borders.push(path);
        }
        features.push(Feature {
            id: 2_000_000 + index as u64,
            class: 3,
            text: country.name.to_uppercase(),
            priority: -(country.rank as f64),
            geometry: Geometry::Polygon { rings },
        });
    }
    for (index, sea) in seas.iter().enumerate() {
        // Natural Earth writes some ocean names in capitals and some not.
        let text = if sea.class == "ocean" {
            sea.name.to_uppercase()
        } else {
            sea.name.clone()
        };
        features.push(Feature {
            id: 3_000_000 + index as u64,
            class: 4,
            text,
            priority: -(sea.rank as f64),
            geometry: Geometry::Polygon { rings: rings(sea) },
        });
    }

    Ok(Scene {
        width,
        height,
        land,
        borders,
        rivers: scene_rivers,
        places: scene_places,
        graticule,
        input: LabelInput {
            bounds: page,
            classes,
            features,
            obstacles: Vec::new(),
            parallels,
            options: Options { seed, improve },
        },
    })
}

/// The longitude of the central meridian, where the projection has one.
fn central_meridian(projection: Projection) -> f64 {
    match projection {
        Projection::EqualEarth { lon0 }
        | Projection::Laea { lon0, .. }
        | Projection::Lcc { lon0, .. } => lon0,
    }
}

/// The page-space points of a parallel from west to east, in steps of `step` degrees. The
/// line stops just short of the antimeridian of the projection.
fn parallel(map: &PageMap, projection: Projection, lat: f64, step: f64) -> Vec<Vec<Point>> {
    let lon0 = central_meridian(projection);
    let n = (359.8 / step).floor() as usize;
    let line: Vec<[f64; 2]> = (0..=n)
        .map(|i| [lon0 - 179.9 + i as f64 * step, lat])
        .collect();
    map.line_to_page(&line)
}

/// One parallel each degree, in the parts that are on the page or near it.
fn parallel_lines(map: &PageMap, page: Rect, projection: Projection) -> Vec<Vec<Point>> {
    let near = page.inflate(50.0, 50.0);
    let mut out = Vec::new();
    for lat in -60..90 {
        for line in parallel(map, projection, f64::from(lat), 0.25) {
            let mut part: Vec<Point> = Vec::new();
            for p in line {
                if near.contains(p) {
                    part.push(p);
                } else if !part.is_empty() {
                    out.push(std::mem::take(&mut part));
                }
            }
            if !part.is_empty() {
                out.push(part);
            }
        }
    }
    out.retain(|l| l.len() >= 2);
    out
}

/// The parallels and meridians each 10 degrees.
fn graticule_lines(map: &PageMap, projection: Projection) -> Vec<BezPath> {
    let mut lines = Vec::new();
    for lat in (-50..90).step_by(10) {
        lines.extend(parallel(map, projection, f64::from(lat), 0.5));
    }
    for lon in (-180..180).step_by(10) {
        let line: Vec<[f64; 2]> = (0..=140)
            .map(|i| [f64::from(lon), -60.0 + f64::from(i)])
            .collect();
        lines.extend(map.line_to_page(&line));
    }
    lines.iter().map(|l| open_path(l)).collect()
}
