//! Where a file from another tool sits on the globe.

/// The edges of a rectangle in degrees of longitude and latitude.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Extents {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

impl Extents {
    /// The whole globe.
    pub const GLOBE: Extents = Extents {
        west: -180.0,
        east: 180.0,
        south: -90.0,
        north: 90.0,
    };

    /// The extents that put two control points at their places. The file maps
    /// linearly to longitude and latitude. `None` if the points share an x or
    /// a y, or if the result is not valid.
    pub fn from_points(a: ControlPoint, b: ControlPoint) -> Option<Extents> {
        const MIN: f64 = 1e-9;
        if (b.x - a.x).abs() < MIN || (b.y - a.y).abs() < MIN {
            return None;
        }
        let lon_span = (b.lon - a.lon) / (b.x - a.x);
        let west = a.lon - a.x * lon_span;
        let lat_span = (a.lat - b.lat) / (b.y - a.y);
        let north = a.lat + a.y * lat_span;
        let extents = Extents {
            west,
            east: west + lon_span,
            south: north - lat_span,
            north,
        };
        let finite = [extents.west, extents.east, extents.south, extents.north]
            .iter()
            .all(|n| n.is_finite());
        (finite && extents.west < extents.east && extents.south < extents.north).then_some(extents)
    }

    /// The edges are finite, the rectangle is not more than 360 degrees wide,
    /// and it is between the poles.
    pub fn is_valid(&self) -> bool {
        let edges = [self.west, self.east, self.south, self.north];
        edges.iter().all(|n| n.is_finite())
            && self.west < self.east
            && self.east - self.west <= 360.0
            && -90.0 <= self.south
            && self.south < self.north
            && self.north <= 90.0
    }

    /// The longitude and the latitude of a place in the file. `x` and `y` are
    /// fractions of the width and the height, from the top left.
    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        [
            self.west + x * (self.east - self.west),
            self.north - y * (self.north - self.south),
        ]
    }
}

/// How a file gets its place on the globe.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum PlaceMethod {
    /// The file covers the whole globe.
    #[default]
    Globe,
    /// The file covers the extents.
    Extents,
    /// Two control points give the extents.
    Points,
}

impl PlaceMethod {
    const ALL: [PlaceMethod; 3] = [
        PlaceMethod::Globe,
        PlaceMethod::Extents,
        PlaceMethod::Points,
    ];
}

/// A known place in the file. `x` and `y` are fractions of the width and the
/// height of the file, from the top left.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ControlPoint {
    pub x: f64,
    pub y: f64,
    pub lon: f64,
    pub lat: f64,
}

/// Where a file from another tool sits on the globe.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Placement {
    pub method: PlaceMethod,
    /// Where the file is on the globe.
    pub extents: Extents,
    /// The control points of the method `Points`.
    pub points: [ControlPoint; 2],
    /// The width and the height of the file, in pixels or in the units of the
    /// vector file.
    pub size: [f64; 2],
}

/// The count of numbers in the encoding of a placement, after the method.
pub(crate) const VALUES: usize = 14;

impl Placement {
    /// A file of this size on the whole globe.
    pub fn globe(size: [f64; 2]) -> Placement {
        let extents = Extents::GLOBE;
        let point = |x, y| {
            let [lon, lat] = extents.at(x, y);
            ControlPoint { x, y, lon, lat }
        };
        Placement {
            method: PlaceMethod::Globe,
            extents,
            points: [point(0.25, 0.25), point(0.75, 0.75)],
            size,
        }
    }

    /// The index of the method and the numbers of the placement.
    pub(crate) fn values(&self) -> (u8, [f64; VALUES]) {
        let method = PlaceMethod::ALL.iter().position(|m| *m == self.method);
        let e = self.extents;
        let [a, b] = self.points;
        let values = [
            e.west,
            e.east,
            e.south,
            e.north,
            a.x,
            a.y,
            a.lon,
            a.lat,
            b.x,
            b.y,
            b.lon,
            b.lat,
            self.size[0],
            self.size[1],
        ];
        (method.expect("a method") as u8, values)
    }

    /// The placement of [`Placement::values`].
    pub(crate) fn from_values(method: u8, v: [f64; VALUES]) -> Option<Placement> {
        let point = |i: usize| ControlPoint {
            x: v[i],
            y: v[i + 1],
            lon: v[i + 2],
            lat: v[i + 3],
        };
        Some(Placement {
            method: *PlaceMethod::ALL.get(method as usize)?,
            extents: Extents {
                west: v[0],
                east: v[1],
                south: v[2],
                north: v[3],
            },
            points: [point(4), point(8)],
            size: [v[12], v[13]],
        })
    }

    /// The text of the column: the method and the numbers, with spaces
    /// between them.
    pub(crate) fn to_text(self) -> String {
        let (method, values) = self.values();
        let mut text = method.to_string();
        for n in values {
            text.push(' ');
            text.push_str(&n.to_string());
        }
        text
    }

    /// The placement of [`Placement::to_text`].
    pub(crate) fn from_text(text: &str) -> Option<Placement> {
        let mut words = text.split(' ');
        let method = words.next()?.parse().ok()?;
        let mut values = [0.0; VALUES];
        for value in &mut values {
            *value = words.next()?.parse().ok()?;
        }
        if words.next().is_some() {
            return None;
        }
        Placement::from_values(method, values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64, lon: f64, lat: f64) -> ControlPoint {
        ControlPoint { x, y, lon, lat }
    }

    #[test]
    fn two_points_give_the_extents() {
        let a = point(0.25, 0.25, -90.0, 45.0);
        let b = point(0.75, 0.75, 90.0, -45.0);
        assert_eq!(Extents::from_points(a, b), Some(Extents::GLOBE));
        // The order of the points does not matter.
        assert_eq!(Extents::from_points(b, a), Some(Extents::GLOBE));
        let a = point(0.0, 0.0, 10.0, 50.0);
        let b = point(0.5, 0.5, 20.0, 40.0);
        let expected = Extents {
            west: 10.0,
            east: 30.0,
            south: 30.0,
            north: 50.0,
        };
        assert_eq!(Extents::from_points(a, b), Some(expected));
    }

    #[test]
    fn points_with_one_x_or_one_y_give_no_extents() {
        let a = point(0.5, 0.25, 0.0, 10.0);
        assert_eq!(Extents::from_points(a, point(0.5, 0.75, 10.0, 0.0)), None);
        assert_eq!(Extents::from_points(a, point(0.75, 0.25, 10.0, 0.0)), None);
        // A file that is upside down or mirrored gives no extents.
        assert_eq!(Extents::from_points(a, point(0.75, 0.75, -10.0, 0.0)), None);
        assert_eq!(Extents::from_points(a, point(0.75, 0.75, 10.0, 20.0)), None);
        let far = point(0.75, 0.75, f64::MAX, 0.0);
        assert_eq!(
            Extents::from_points(point(0.25, 0.25, -f64::MAX, 10.0), far),
            None
        );
    }

    #[test]
    fn valid_extents_are_between_the_poles_and_not_wider_than_the_globe() {
        assert!(Extents::GLOBE.is_valid());
        let small = Extents {
            west: 170.0,
            east: 190.0,
            south: -10.0,
            north: 10.0,
        };
        assert!(small.is_valid());
        let invalid = [
            Extents {
                east: 170.0,
                ..small
            },
            Extents {
                east: 531.0,
                ..small
            },
            Extents {
                south: 10.0,
                ..small
            },
            Extents {
                north: 91.0,
                ..small
            },
            Extents {
                south: -91.0,
                ..small
            },
            Extents {
                west: f64::NAN,
                ..small
            },
            Extents {
                east: f64::INFINITY,
                ..small
            },
        ];
        for extents in invalid {
            assert!(!extents.is_valid(), "{extents:?}");
        }
    }

    #[test]
    fn the_globe_placement_has_points_on_the_globe() {
        let placement = Placement::globe([2048.0, 1024.0]);
        assert_eq!(placement.points[0], point(0.25, 0.25, -90.0, 45.0));
        assert_eq!(placement.points[1], point(0.75, 0.75, 90.0, -45.0));
        let extents = Extents::from_points(placement.points[0], placement.points[1]);
        assert_eq!(extents, Some(placement.extents));
    }

    #[test]
    fn the_text_keeps_the_placement() {
        let mut placement = Placement::globe([4000.5, 2000.0]);
        placement.method = PlaceMethod::Points;
        placement.points[1].lon = 1.0 / 3.0;
        placement.extents.west = -0.1;
        let text = placement.to_text();
        assert_eq!(Placement::from_text(&text), Some(placement));
        assert_eq!(Placement::from_text("3 0 0 0 0 0 0 0 0 0 0 0 0 0 0"), None);
        assert_eq!(Placement::from_text("0 1 2"), None);
        assert_eq!(Placement::from_text(&format!("{text} 1")), None);
    }
}
