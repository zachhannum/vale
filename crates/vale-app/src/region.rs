//! A region on the globe, and the projection that the app suggests for it.
//!
//! A region is a range of longitude and latitude. It sets the projection and
//! the first view of a new map frame, and nothing keeps it after that.

use kurbo::Rect;
use vale_sphere::{LonLat, Projection, ProjectionKind, ProjectionSpec};

use crate::atlas::fit_scale;

/// A range of longitude and latitude, in degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Region {
    /// The west edge, in -180..180. The east edge is `west + width`, which
    /// can pass 180.
    pub west: f64,
    pub width: f64,
    pub south: f64,
    pub north: f64,
    /// The center of the projection and of the first view.
    pub center: LonLat,
    /// The latitude of the pole in the region, if any: 90 or -90.
    pub pole: Option<f64>,
    /// The region holds the whole disc of the globe view.
    pub holds_globe: bool,
}

impl Region {
    /// A region with its center at the middle of the range.
    pub fn new(west: f64, width: f64, south: f64, north: f64) -> Region {
        let west = wrap(west);
        Region {
            west,
            width,
            south,
            north,
            center: [wrap(west + width / 2.0), (south + north) / 2.0],
            pole: None,
            holds_globe: false,
        }
    }

    /// The region of all longitudes around a pole, from the pole to `edge`.
    /// The center is the pole, at the longitude `lon`.
    pub fn around_pole(pole: f64, edge: f64, lon: f64) -> Region {
        let (south, north) = if pole > 0.0 {
            (edge, 90.0)
        } else {
            (-90.0, edge)
        };
        Region {
            center: [wrap(lon), pole],
            pole: Some(pole),
            ..Region::new(lon - 180.0, 360.0, south, north)
        }
    }

    /// The region of points on the globe. `pole` is the latitude of a pole
    /// inside the region. `view` is the center of the globe view, and it is
    /// the center of a region that holds the globe or a pole. `None` when
    /// fewer than two points are on the globe.
    pub fn from_points(
        points: &[LonLat],
        pole: Option<f64>,
        holds_globe: bool,
        view: LonLat,
    ) -> Option<Region> {
        if points.len() < 2 {
            return None;
        }
        let (mut south, mut north) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in points {
            south = south.min(p[1]);
            north = north.max(p[1]);
        }
        let mut region = if let Some(pole) = pole {
            let edge = if pole > 0.0 { south } else { north };
            Region::around_pole(pole, edge, view[0])
        } else {
            let mut lons: Vec<f64> = points.iter().map(|p| wrap(p[0])).collect();
            lons.sort_by(f64::total_cmp);
            // The largest gap between neighbors, with the gap across the
            // antimeridian, is outside the region.
            let mut gap = lons[0] + 360.0 - lons[lons.len() - 1];
            let mut west = lons[0];
            for w in lons.windows(2) {
                if w[1] - w[0] > gap {
                    gap = w[1] - w[0];
                    west = w[1];
                }
            }
            Region::new(west, 360.0 - gap, south, north)
        };
        if holds_globe {
            region.holds_globe = true;
            region.center = view;
        }
        Some(region)
    }

    /// The range of latitude, in degrees.
    pub fn span(&self) -> f64 {
        self.north - self.south
    }

    /// The width in degrees of a great circle at the middle latitude.
    pub fn ground_width(&self) -> f64 {
        self.width * ((self.south + self.north) / 2.0).to_radians().cos()
    }

    /// Points along the four edges, `steps` on each edge.
    pub fn boundary(&self, steps: usize) -> Vec<LonLat> {
        let (w, e, s, n) = (self.west, self.west + self.width, self.south, self.north);
        let steps = steps.max(1);
        let mut points = Vec::with_capacity(4 * steps);
        for i in 0..steps {
            let t = i as f64 / steps as f64;
            points.push([w + (e - w) * t, s]);
            points.push([e, s + (n - s) * t]);
            points.push([e - (e - w) * t, n]);
            points.push([w, n - (n - s) * t]);
        }
        points.iter().map(|p| [wrap(p[0]), p[1]]).collect()
    }
}

/// A row of the suggestion table.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionKind {
    Globe,
    World,
    Polar,
    Equator,
    Hemisphere,
    Tall,
    LargeMidLatitude,
    MidLatitude,
}

impl RegionKind {
    /// The rows in the order that `classify` tries them.
    pub const ALL: [RegionKind; 8] = [
        RegionKind::Globe,
        RegionKind::World,
        RegionKind::Polar,
        RegionKind::Equator,
        RegionKind::Hemisphere,
        RegionKind::Tall,
        RegionKind::LargeMidLatitude,
        RegionKind::MidLatitude,
    ];

    pub fn projection(self) -> ProjectionKind {
        match self {
            RegionKind::Globe => ProjectionKind::Orthographic,
            RegionKind::World => ProjectionKind::EqualEarth,
            RegionKind::Polar => ProjectionKind::PolarStereographic,
            RegionKind::Equator => ProjectionKind::Mercator,
            RegionKind::Hemisphere => ProjectionKind::LambertAzimuthal,
            RegionKind::Tall => ProjectionKind::TransverseMercator,
            RegionKind::LargeMidLatitude => ProjectionKind::AlbersEqualArea,
            RegionKind::MidLatitude => ProjectionKind::LambertConformal,
        }
    }

    /// The reason for the suggestion, in one line.
    pub fn reason(self) -> &'static str {
        match self {
            RegionKind::Globe => "Suggested for a view of the globe.",
            RegionKind::World => "Suggested for the whole world.",
            RegionKind::Polar => "Suggested for a polar region.",
            RegionKind::Equator => "Suggested for a region on the equator.",
            RegionKind::Hemisphere => "Suggested for a hemisphere or continent.",
            RegionKind::Tall => "Suggested for a region taller than wide.",
            RegionKind::LargeMidLatitude => "Suggested for a large mid-latitude region.",
            RegionKind::MidLatitude => "Suggested for a wide mid-latitude region.",
        }
    }
}

/// The row of the suggestion table for a region. The first row that matches
/// wins.
pub fn classify(region: &Region) -> RegionKind {
    let span = region.span();
    let ground = region.ground_width();
    let polar = match region.pole {
        Some(pole) if pole > 0.0 => region.south >= 45.0,
        Some(_) => region.north <= -45.0,
        None => region.south >= 60.0 || region.north <= -60.0,
    };
    if region.holds_globe {
        RegionKind::Globe
    } else if region.width >= 270.0 && span >= 120.0 {
        RegionKind::World
    } else if polar {
        RegionKind::Polar
    } else if region.south >= -30.0 && region.north <= 30.0 && ground >= span {
        RegionKind::Equator
    } else if span >= 60.0 || ground >= 60.0 {
        RegionKind::Hemisphere
    } else if span > ground {
        RegionKind::Tall
    } else if ground >= 25.0 {
        RegionKind::LargeMidLatitude
    } else {
        RegionKind::MidLatitude
    }
}

/// The suggested projection of a region, centered on the region. A conic
/// projection has its standard parallels at one sixth and five sixths of the
/// range of latitude.
pub fn suggest(region: &Region) -> (RegionKind, ProjectionSpec) {
    let kind = classify(region);
    let [lon, lat] = region.center;
    let mut spec = ProjectionSpec::new(kind.projection(), lon, lat);
    if spec.kind.is_conic() {
        let span = region.span();
        spec.lat0 = (region.south + region.north) / 2.0;
        spec.lat1 = region.south + span / 6.0;
        spec.lat2 = region.south + span * 5.0 / 6.0;
    }
    (kind, spec.normalized())
}

/// The center and the scale of a frame of `width_mm` by `height_mm` that
/// shows the region in `projection`. A view of the globe or of the world
/// shows the whole projection.
pub fn fit(
    projection: &Projection,
    region: &Region,
    kind: RegionKind,
    width_mm: f64,
    height_mm: f64,
) -> (LonLat, f64) {
    let center = region.center;
    let whole = matches!(kind, RegionKind::Globe | RegionKind::World);
    let extent = projection.forward(center).filter(|_| !whole).and_then(|c| {
        let (mut dx, mut dy) = (0.0f64, 0.0f64);
        for p in region.boundary(64) {
            let q = projection.forward(p)?;
            dx = dx.max((q.x - c.x).abs());
            dy = dy.max((q.y - c.y).abs());
        }
        (dx > 0.0 && dy > 0.0).then(|| Rect::new(-dx, -dy, dx, dy))
    });
    let bounds = extent.unwrap_or_else(|| projection.bounds());
    (center, fit_scale(bounds, width_mm, height_mm))
}

fn wrap(lon: f64) -> f64 {
    let w = (lon + 180.0).rem_euclid(360.0) - 180.0;
    if w == -180.0 && lon > 0.0 { 180.0 } else { w }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(region: Region) -> RegionKind {
        classify(&region)
    }

    /// One region for each row of the table.
    fn example(row: RegionKind) -> Region {
        match row {
            RegionKind::Globe => Region {
                holds_globe: true,
                ..Region::new(-90.0, 180.0, -90.0, 90.0)
            },
            RegionKind::World => Region::new(-180.0, 360.0, -80.0, 80.0),
            // Greenland.
            RegionKind::Polar => Region::around_pole(90.0, 60.0, -40.0),
            // Indonesia.
            RegionKind::Equator => Region::new(95.0, 46.0, -11.0, 6.0),
            // Africa.
            RegionKind::Hemisphere => Region::new(-18.0, 70.0, -35.0, 37.0),
            // Chile.
            RegionKind::Tall => Region::new(-76.0, 10.0, -56.0, -17.0),
            // The contiguous United States.
            RegionKind::LargeMidLatitude => Region::new(-125.0, 58.0, 24.0, 50.0),
            // Spain.
            RegionKind::MidLatitude => Region::new(-9.5, 13.0, 36.0, 44.0),
        }
    }

    #[test]
    fn each_row_of_the_table_has_a_region() {
        let table = [
            (RegionKind::Globe, "ortho"),
            (RegionKind::World, "eqearth"),
            (RegionKind::Polar, "stere"),
            (RegionKind::Equator, "merc"),
            (RegionKind::Hemisphere, "laea"),
            (RegionKind::Tall, "tmerc"),
            (RegionKind::LargeMidLatitude, "aea"),
            (RegionKind::MidLatitude, "lcc"),
        ];
        for (row, proj) in table {
            let (kind, spec) = suggest(&example(row));
            assert_eq!(kind, row);
            assert_eq!(spec.kind.proj_name(), proj, "{row:?}");
            assert!(spec.to_proj().contains(&format!("+proj={proj}")), "{row:?}");
        }
        assert_eq!(RegionKind::ALL.len(), table.len());
    }

    #[test]
    fn a_box_around_the_globe_suggests_orthographic() {
        let mut region = Region::new(-30.0, 60.0, 10.0, 50.0);
        region.holds_globe = true;
        region.center = [20.0, 30.0];
        let (kind, spec) = suggest(&region);
        assert_eq!(kind, RegionKind::Globe);
        assert_eq!(spec.kind, ProjectionKind::Orthographic);
        assert_eq!((spec.lon0, spec.lat0), (20.0, 30.0));
    }

    #[test]
    fn the_whole_world_suggests_equal_earth() {
        assert_eq!(kind(example(RegionKind::World)), RegionKind::World);
        assert_eq!(
            kind(Region::new(-150.0, 280.0, -60.0, 70.0)),
            RegionKind::World
        );
        // A pole box that reaches 30 degrees past the equator.
        assert_eq!(
            kind(Region::around_pole(90.0, -35.0, 0.0)),
            RegionKind::World
        );
    }

    #[test]
    fn a_polar_region_suggests_polar_stereographic() {
        for region in [
            Region::around_pole(90.0, 60.0, 10.0),
            Region::around_pole(-90.0, -50.0, 10.0),
            // Svalbard, with no pole inside.
            Region::new(10.0, 25.0, 76.0, 81.0),
            Region::new(-60.0, 40.0, -80.0, -62.0),
        ] {
            let (kind, spec) = suggest(&region);
            assert_eq!(kind, RegionKind::Polar, "{region:?}");
            assert_eq!(spec.kind, ProjectionKind::PolarStereographic);
            assert_eq!(spec.lat0, region.center[1].signum() * 90.0);
            assert!((spec.lon0 - region.center[0]).abs() < 1e-9);
        }
    }

    #[test]
    fn a_region_on_the_equator_suggests_mercator() {
        assert_eq!(kind(example(RegionKind::Equator)), RegionKind::Equator);
        // A wide band of the Pacific.
        assert_eq!(
            kind(Region::new(150.0, 100.0, -20.0, 20.0)),
            RegionKind::Equator
        );
    }

    #[test]
    fn a_continent_suggests_lambert_azimuthal_at_its_center() {
        let region = example(RegionKind::Hemisphere);
        let (kind, spec) = suggest(&region);
        assert_eq!(kind, RegionKind::Hemisphere);
        assert_eq!(spec.kind, ProjectionKind::LambertAzimuthal);
        assert!((spec.lon0 - 17.0).abs() < 1e-9);
        assert!((spec.lat0 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_region_taller_than_wide_suggests_transverse_mercator() {
        let (kind, spec) = suggest(&example(RegionKind::Tall));
        assert_eq!(kind, RegionKind::Tall);
        assert_eq!(spec.kind, ProjectionKind::TransverseMercator);
        assert!((spec.lon0 - -71.0).abs() < 1e-9);
    }

    #[test]
    fn a_large_mid_latitude_region_suggests_albers() {
        let (kind, spec) = suggest(&example(RegionKind::LargeMidLatitude));
        assert_eq!(kind, RegionKind::LargeMidLatitude);
        assert_eq!(spec.kind, ProjectionKind::AlbersEqualArea);
    }

    #[test]
    fn a_wide_mid_latitude_region_suggests_lambert_conformal() {
        let (kind, spec) = suggest(&example(RegionKind::MidLatitude));
        assert_eq!(kind, RegionKind::MidLatitude);
        assert_eq!(spec.kind, ProjectionKind::LambertConformal);
    }

    #[test]
    fn the_rules_apply_in_the_order_of_the_table() {
        // A tall strip on the equator is taller than wide.
        assert_eq!(kind(Region::new(10.0, 8.0, -20.0, 20.0)), RegionKind::Tall);
        // An Arctic cap to 30 north is a hemisphere.
        assert_eq!(
            kind(Region::around_pole(90.0, 30.0, 0.0)),
            RegionKind::Hemisphere
        );
        // A wide band on the equator is not a hemisphere.
        assert_eq!(
            kind(Region::new(-40.0, 120.0, -25.0, 25.0)),
            RegionKind::Equator
        );
    }

    #[test]
    fn the_center_of_the_projection_is_the_center_of_the_region() {
        let (_, spec) = suggest(&Region::new(170.0, 20.0, 40.0, 48.0));
        assert_eq!(spec.lon0.abs(), 180.0);
        assert_eq!(spec.lat0, 44.0);
        let (_, spec) = suggest(&Region::new(-10.0, 20.0, 40.0, 48.0));
        assert_eq!((spec.lon0, spec.lat0), (0.0, 44.0));
    }

    #[test]
    fn a_conic_suggestion_has_its_parallels_at_one_sixth_and_five_sixths() {
        let cases = [
            (Region::new(0.0, 20.0, 40.0, 52.0), 42.0, 50.0),
            (Region::new(0.0, 60.0, 30.0, 60.0), 35.0, 55.0),
            (Region::new(0.0, 20.0, -52.0, -40.0), -50.0, -42.0),
        ];
        for (region, lat1, lat2) in cases {
            let (_, spec) = suggest(&region);
            assert!(spec.kind.is_conic(), "{region:?}");
            assert!((spec.lat1 - lat1).abs() < 1e-9);
            assert!((spec.lat2 - lat2).abs() < 1e-9);
            let back = ProjectionSpec::from_proj(&spec.to_proj()).unwrap();
            assert!((back.lat1 - lat1).abs() < 1e-9);
            assert!((back.lat2 - lat2).abs() < 1e-9);
        }
    }

    #[test]
    fn points_across_the_antimeridian_make_a_narrow_region() {
        let points = [[170.0, 10.0], [-175.0, 20.0], [178.0, 15.0]];
        let region = Region::from_points(&points, None, false, [0.0, 0.0]).unwrap();
        assert_eq!(region.west, 170.0);
        assert!((region.width - 15.0).abs() < 1e-9);
        assert_eq!((region.south, region.north), (10.0, 20.0));
        assert!((region.center[0] - 177.5).abs() < 1e-9);
    }

    #[test]
    fn points_around_a_pole_make_all_longitudes() {
        let points = [[0.0, 70.0], [90.0, 80.0]];
        let region = Region::from_points(&points, Some(90.0), false, [30.0, 60.0]).unwrap();
        assert_eq!(region.width, 360.0);
        assert_eq!((region.south, region.north), (70.0, 90.0));
        assert_eq!(region.center, [30.0, 90.0]);
        assert!(Region::from_points(&points[..1], None, false, [0.0, 0.0]).is_none());
    }

    #[test]
    fn the_fit_shows_the_whole_region() {
        let region = example(RegionKind::MidLatitude);
        let (kind, spec) = suggest(&region);
        let projection = Projection::new(spec, 6_371_000.0).unwrap();
        let (center, scale) = fit(&projection, &region, kind, 200.0, 150.0);
        assert_eq!(center, region.center);
        let c = projection.forward(center).unwrap();
        for p in region.boundary(16) {
            let q = projection.forward(p).unwrap();
            assert!((q.x - c.x).abs() * 1000.0 / scale <= 100.0 + 1e-6);
            assert!((q.y - c.y).abs() * 1000.0 / scale <= 75.0 + 1e-6);
        }
    }
}
