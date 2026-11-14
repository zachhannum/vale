//! The distortion of the map of a frame: the scale error at the corners of
//! the frame, and Tissot circles.
//!
//! A scale error is relative to the scale of the frame. A projection has
//! that scale on its standard lines.

use kurbo::Point;
use vale_sphere::{LonLat, Projection, measure::small_circle};
use vale_store::FrameRecord;

use crate::atlas::{self as math, FrameTarget};
use crate::view::ViewTransform;

/// The places of a frame at which the editor prints the scale error: the
/// four corners of the box of the shape, and its center.
pub const PLACES: [&str; 5] = [
    "Top left",
    "Top right",
    "Center",
    "Bottom left",
    "Bottom right",
];

/// The number of circles across the shorter side of a frame.
const ACROSS: f64 = 4.0;
/// The radius of the circle at the center of a frame, as a part of the
/// space between two circles.
const RADIUS: f64 = 0.3;
/// The number of points of a circle.
const POINTS: usize = 48;

/// The distortion at one place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleError {
    /// The scale factor that differs most from 1, less 1.
    pub scale: f64,
    /// The area scale, less 1.
    pub area: f64,
}

/// The distortion of a projection at a place.
pub fn scale_error(projection: &Projection, p: LonLat) -> Option<ScaleError> {
    let [a, b] = projection.distortion(p)?;
    let scale = if (a - 1.0).abs() >= (b - 1.0).abs() {
        a
    } else {
        b
    };
    Some(ScaleError {
        scale: scale - 1.0,
        area: a * b - 1.0,
    })
}

/// The page points of `PLACES`, in millimeters.
fn places(frame: &FrameRecord) -> [Point; 5] {
    let b = math::shape_bbox(&frame.shape);
    [
        Point::new(b.x0, b.y0),
        Point::new(b.x1, b.y0),
        b.center(),
        Point::new(b.x0, b.y1),
        Point::new(b.x1, b.y1),
    ]
}

fn lonlat_at(projection: &Projection, vt: &ViewTransform, p: Point) -> Option<LonLat> {
    projection.inverse(vt.from_page(p))
}

/// The distortion at each of `PLACES`. `None`: the place is not on the
/// globe.
pub fn frame_errors(projection: &Projection, frame: &FrameRecord) -> [Option<ScaleError>; 5] {
    let vt = math::frame_transform(projection, frame, FrameTarget::MM);
    places(frame).map(|p| lonlat_at(projection, &vt, p).and_then(|ll| scale_error(projection, ll)))
}

/// An error as a percent with a sign, for example "+0.4%" and "−12%".
pub fn percent_text(error: f64) -> String {
    let percent = error * 100.0;
    let digits = if percent.abs() >= 10.0 { 0 } else { 1 };
    let text = format!("{:.*}", digits, percent.abs());
    if text.chars().all(|c| c == '0' || c == '.') {
        return format!("{text}%");
    }
    let sign = if percent < 0.0 { '\u{2212}' } else { '+' };
    format!("{sign}{text}%")
}

/// Tissot circles in a frame, in page millimeters. The circles are equal
/// on the sphere, and their centers are on a grid in the box of the shape.
/// A circle with its center outside the shape, or across a cut of the
/// map, is left out.
pub fn tissot_circles(projection: &Projection, frame: &FrameRecord) -> Vec<Vec<Point>> {
    let vt = math::frame_transform(projection, frame, FrameTarget::MM);
    let b = math::shape_bbox(&frame.shape);
    let space = b.width().min(b.height()) / ACROSS;
    if space <= 0.0 {
        return Vec::new();
    }
    let Some([a, c]) = lonlat_at(projection, &vt, b.center())
        .or_else(|| projection.inverse(vt.view.center))
        .and_then(|ll| projection.distortion(ll))
    else {
        return Vec::new();
    };
    // The ground meters of one page millimeter at the center.
    let ground = frame.scale / 1000.0 / (a * c).sqrt();
    let radius = (RADIUS * space * ground / projection.radius_m())
        .to_degrees()
        .min(20.0);
    let cols = (b.width() / space).round().max(1.0) as usize;
    let rows = (b.height() / space).round().max(1.0) as usize;
    let mut circles = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let p = Point::new(
                b.x0 + (col as f64 + 0.5) * b.width() / cols as f64,
                b.y0 + (row as f64 + 0.5) * b.height() / rows as f64,
            );
            if !math::contains(&frame.shape, p) {
                continue;
            }
            let Some(center) = lonlat_at(projection, &vt, p) else {
                continue;
            };
            // A point on the edge of the clip rectangle does not project.
            // The edge runs north from the center of an azimuthal map, so
            // the circle leaves out the point.
            let ring: Vec<Point> = small_circle(center, radius, POINTS)
                .into_iter()
                .filter_map(|q| projection.forward(q).map(|x| vt.to_page(x)))
                .collect();
            // A circle across a cut of the map has points on both sides.
            let whole = ring.iter().all(|q| (*q - p).hypot() < 2.0 * space);
            if ring.len() + 2 >= POINTS && whole {
                circles.push(ring);
            }
        }
    }
    circles
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_sphere::{ProjectionKind, ProjectionSpec};
    use vale_store::{Border, FrameId, FrameShape, Graticule, MapId, Neatline};

    fn frame(center: LonLat, scale: f64) -> FrameRecord {
        FrameRecord {
            id: FrameId(1),
            map: MapId(1),
            shape: FrameShape::Rectangle([10.0, 10.0, 210.0, 160.0]),
            center,
            scale,
            rotation: 0.0,
            graticule: Graticule::default(),
            neatline: Neatline::default(),
            border: Border::default(),
        }
    }

    fn projection(kind: ProjectionKind, lon0: f64, lat0: f64) -> Projection {
        Projection::new(ProjectionSpec::new(kind, lon0, lat0), 6_371_000.0).unwrap()
    }

    #[test]
    fn a_mercator_frame_north_of_the_equator_is_larger_at_the_top() {
        let p = projection(ProjectionKind::Mercator, 0.0, 0.0);
        let errors = frame_errors(&p, &frame([0.0, 45.0], 3e7)).map(Option::unwrap);
        let top = errors[0].scale;
        assert!(top > errors[2].scale && errors[2].scale > errors[3].scale);
        assert!((errors[0].scale - errors[1].scale).abs() < 1e-9);
        // Mercator keeps shape, so the area grows with the square of the
        // scale.
        let area = (1.0 + top).powi(2) - 1.0;
        assert!((errors[0].area - area).abs() < 1e-6);
    }

    #[test]
    fn an_equal_area_frame_has_no_area_error() {
        let p = projection(ProjectionKind::LambertAzimuthal, 20.0, 50.0);
        for error in frame_errors(&p, &frame([20.0, 50.0], 4e7)) {
            assert!(error.unwrap().area.abs() < 1e-6);
        }
    }

    #[test]
    fn a_corner_off_the_globe_has_no_error() {
        let p = projection(ProjectionKind::Orthographic, 0.0, 0.0);
        let errors = frame_errors(&p, &frame([0.0, 0.0], 1e8));
        assert!(errors[0].is_none() && errors[2].is_some());
    }

    #[test]
    fn the_circles_are_round_where_the_projection_keeps_shape() {
        let p = projection(ProjectionKind::Stereographic, 10.0, 40.0);
        let f = frame([10.0, 40.0], 2e7);
        let circles = tissot_circles(&p, &f);
        // 200 by 150 mm with 37.5 mm between the circles.
        assert_eq!(circles.len(), 5 * 4);
        for ring in &circles {
            let sum = ring.iter().fold(kurbo::Vec2::ZERO, |s, q| s + q.to_vec2());
            let center = (sum / ring.len() as f64).to_point();
            let radii: Vec<f64> = ring.iter().map(|q| (*q - center).hypot()).collect();
            let (lo, hi) = radii
                .iter()
                .fold((f64::MAX, 0.0f64), |(l, h), r| (l.min(*r), h.max(*r)));
            assert!(hi / lo < 1.01, "{lo} {hi}");
        }
    }

    #[test]
    fn the_circles_stretch_where_the_projection_does() {
        let p = projection(ProjectionKind::EqualEarth, 0.0, 0.0);
        let circles = tissot_circles(&p, &frame([0.0, 0.0], 1.2e8));
        let width = |ring: &Vec<Point>| {
            let xs = ring.iter().map(|q| q.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        };
        let height = |ring: &Vec<Point>| {
            let ys = ring.iter().map(|q| q.y);
            ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min)
        };
        // A circle at a high latitude is wider than tall.
        let top = circles.first().unwrap();
        assert!(width(top) > 1.2 * height(top));
    }

    #[test]
    fn an_error_prints_as_a_percent_with_a_sign() {
        assert_eq!(percent_text(0.004), "+0.4%");
        assert_eq!(percent_text(-0.0031), "\u{2212}0.3%");
        assert_eq!(percent_text(0.00001), "0.0%");
        assert_eq!(percent_text(-0.00001), "0.0%");
        assert_eq!(percent_text(1.234), "+123%");
    }
}
