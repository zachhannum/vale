//! The math of pages and map frames: views, gestures, shapes, and text.
//!
//! A page coordinate is in millimeters from the top-left corner of the page,
//! with y down.

use kurbo::{Point, Rect, Vec2};
use vale_sphere::{Projection, auto_interval};
use vale_store::{Border, FrameId, FrameRecord, FrameShape, Graticule, MapRecord, PageRecord};

use crate::view::{View, ViewTransform};

/// Points per millimeter in a PDF and in a PNG at scale 1.
pub const PT_PER_MM: f64 = 72.0 / 25.4;

/// The smallest scale and the largest scale, as N of 1:N.
pub const SCALE_RANGE: (f64, f64) = (1e3, 1e10);

/// The space between the edge of the page and a new frame, in millimeters.
pub const MARGIN_MM: f64 = 10.0;

/// The smallest width and height of a rectangle, in millimeters.
pub const MIN_SIZE_MM: f64 = 5.0;

/// Where a page is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FrameTarget {
    /// The top-left corner of the page, in target points.
    pub origin: Point,
    pub pts_per_mm: f64,
}

impl FrameTarget {
    /// Page millimeters, with the page at 0, 0.
    pub const MM: FrameTarget = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: 1.0,
    };

    /// A page point in millimeters to target points.
    pub fn to_target(&self, p: Point) -> Point {
        self.origin + p.to_vec2() * self.pts_per_mm
    }

    /// Target points to a page point in millimeters.
    pub fn from_target(&self, q: Point) -> Point {
        ((q - self.origin) / self.pts_per_mm).to_point()
    }
}

fn bbox_of(points: &[[f64; 2]]) -> Rect {
    let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &[x, y] in points {
        r = Rect::new(r.x0.min(x), r.y0.min(y), r.x1.max(x), r.y1.max(y));
    }
    if points.is_empty() { Rect::ZERO } else { r }
}

/// The box of ring 0 of a shape, in millimeters.
pub fn shape_bbox(shape: &FrameShape) -> Rect {
    match shape {
        FrameShape::Rectangle([x0, y0, x1, y1]) => Rect::new(*x0, *y0, *x1, *y1).abs(),
        FrameShape::Polygon(rings) => rings.first().map_or(Rect::ZERO, |r| bbox_of(r)),
    }
}

/// The point of a frame that shows its center: the middle of the box of
/// ring 0.
pub fn anchor(shape: &FrameShape) -> Point {
    shape_bbox(shape).center()
}

/// The view of a frame in a target. The width and the height are the size of
/// the box of the shape.
pub fn frame_transform(
    projection: &Projection,
    frame: &FrameRecord,
    target: FrameTarget,
) -> ViewTransform {
    let spec = projection.spec();
    let center = projection
        .forward(frame.center)
        .or_else(|| projection.forward([spec.lon0, spec.lat0]))
        .unwrap_or(Point::ZERO);
    let bbox = shape_bbox(&frame.shape);
    ViewTransform {
        view: View {
            center,
            scale: target.pts_per_mm * 1000.0 / frame.scale,
            rotation: frame.rotation.to_radians(),
        },
        width: bbox.width() * target.pts_per_mm,
        height: bbox.height() * target.pts_per_mm,
        origin: target.to_target(anchor(&frame.shape)),
    }
}

/// The projected center that keeps `p` under the page point `at`.
fn center_keeping(vt: ViewTransform, at: Point, p: Point) -> Point {
    let probe = ViewTransform {
        view: View {
            center: p,
            ..vt.view
        },
        ..vt
    };
    p - (probe.from_page(at) - p)
}

/// Sets the center of a frame to a projected point. Returns false if the
/// point is not on the globe.
fn set_center(frame: &mut FrameRecord, projection: &Projection, c: Point) -> bool {
    match projection.inverse(c) {
        Some(lonlat) => {
            frame.center = lonlat;
            true
        }
        None => false,
    }
}

/// Moves the map in a frame by `d` page millimeters. Returns false if the
/// new center is not on the globe.
pub fn pan(frame: &mut FrameRecord, projection: &Projection, d: Vec2) -> bool {
    let vt = frame_transform(projection, frame, FrameTarget::MM);
    let view = vt.view.panned(d.x, d.y);
    set_center(frame, projection, view.center)
}

/// Multiplies the size of the map in a frame by `factor`. The place under
/// the page point `at` stays. Returns false if nothing changed.
pub fn zoom(frame: &mut FrameRecord, projection: &Projection, at: Point, factor: f64) -> bool {
    let vt = frame_transform(projection, frame, FrameTarget::MM);
    let p = vt.from_page(at);
    let scale = (frame.scale / factor).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
    if scale == frame.scale {
        return false;
    }
    let mut next = vt;
    next.view.scale = 1000.0 / scale;
    let mut zoomed = frame.clone();
    zoomed.scale = scale;
    let moved = set_center(&mut zoomed, projection, center_keeping(next, at, p));
    if moved {
        *frame = zoomed;
    }
    moved
}

/// Turns the map in a frame clockwise by `degrees` about the page point
/// `at`. Returns false if nothing changed.
pub fn rotate(frame: &mut FrameRecord, projection: &Projection, at: Point, degrees: f64) -> bool {
    if degrees.rem_euclid(360.0) == 0.0 {
        return false;
    }
    let vt = frame_transform(projection, frame, FrameTarget::MM);
    let p = vt.from_page(at);
    let rotation = (frame.rotation + degrees).rem_euclid(360.0);
    let mut next = vt;
    next.view.rotation = rotation.to_radians();
    let mut turned = frame.clone();
    turned.rotation = rotation;
    let moved = set_center(&mut turned, projection, center_keeping(next, at, p));
    if moved {
        *frame = turned;
    }
    moved
}

/// The smallest scale of 1, 2, or 5 times a power of ten at which `bounds`
/// in projected meters fits in a box of millimeters.
pub fn fit_scale(bounds: Rect, width_mm: f64, height_mm: f64) -> f64 {
    let need = (bounds.width() * 1000.0 / width_mm.max(1e-9))
        .max(bounds.height() * 1000.0 / height_mm.max(1e-9));
    if !need.is_finite() || need <= SCALE_RANGE.0 {
        return SCALE_RANGE.0;
    }
    let power = 10f64.powf(need.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|n| *n >= need * (1.0 - 1e-12))
        .unwrap_or(10.0 * power);
    nice.min(SCALE_RANGE.1)
}

/// A size of paper, in portrait.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PagePreset {
    pub name: &'static str,
    pub width_mm: f64,
    pub height_mm: f64,
}

const fn preset(name: &'static str, width_mm: f64, height_mm: f64) -> PagePreset {
    PagePreset {
        name,
        width_mm,
        height_mm,
    }
}

pub const PAGE_PRESETS: [PagePreset; 8] = [
    preset("A5", 148.0, 210.0),
    preset("A4", 210.0, 297.0),
    preset("A3", 297.0, 420.0),
    preset("A2", 420.0, 594.0),
    preset("Letter", 215.9, 279.4),
    preset("Legal", 215.9, 355.6),
    preset("Tabloid", 279.4, 431.8),
    preset("12 × 16 in", 304.8, 406.4),
];

/// The name of the preset of a page size in either orientation, or "Custom".
pub fn preset_of(width_mm: f64, height_mm: f64) -> &'static str {
    let near = |a: f64, b: f64| (a - b).abs() < 0.05;
    let found = PAGE_PRESETS.iter().find(|p| {
        (near(p.width_mm, width_mm) && near(p.height_mm, height_mm))
            || (near(p.width_mm, height_mm) && near(p.height_mm, width_mm))
    });
    found.map_or("Custom", |p| p.name)
}

/// A scale as text, for example "1:90,000,000".
pub fn scale_text(scale: f64) -> String {
    let digits = format!("{:.0}", scale.round().max(0.0));
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("1:{grouped}")
}

/// Reads a scale such as "90000000", "1:90,000,000", or "1 : 90 000 000".
pub fn parse_scale(text: &str) -> Option<f64> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();
    let number = text.strip_prefix("1:").unwrap_or(&text);
    let n: f64 = number.parse().ok()?;
    (n.is_finite() && n > 0.0).then_some(n)
}

fn degrees_text(value: f64, positive: char, negative: char) -> String {
    let text = format!("{:.2}", value.abs());
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match value {
        v if text == "0" || v == 0.0 => "0°".to_string(),
        v if v > 0.0 => format!("{text}° {positive}"),
        _ => format!("{text}° {negative}"),
    }
}

/// A longitude as text, for example "12.5° W".
pub fn lon_text(lon: f64) -> String {
    degrees_text(lon, 'E', 'W')
}

/// A latitude as text, for example "40° N".
pub fn lat_text(lat: f64) -> String {
    degrees_text(lat, 'N', 'S')
}

/// Reads degrees such as "-12.5", "12.5°", "12.5 W", or "40° n". N and E
/// are positive, and S and W are negative.
pub fn parse_degrees(text: &str) -> Option<f64> {
    let text = text.trim();
    let (number, sign) = match text.chars().last()?.to_ascii_uppercase() {
        'N' | 'E' => (&text[..text.len() - 1], 1.0),
        'S' | 'W' => (&text[..text.len() - 1], -1.0),
        _ => (text, 1.0),
    };
    let number = number.trim().trim_end_matches('°').trim();
    let n: f64 = number.parse().ok()?;
    n.is_finite().then_some(sign * n)
}

/// True if a page point is inside a shape. The fill rule is even-odd, so a
/// point in a hole is outside.
pub fn contains(shape: &FrameShape, p: Point) -> bool {
    let mut inside = false;
    for ring in shape.rings() {
        let n = ring.len();
        for i in 0..n {
            let [ax, ay] = ring[i];
            let [bx, by] = ring[(i + 1) % n];
            if (ay > p.y) != (by > p.y) && p.x < ax + (p.y - ay) / (by - ay) * (bx - ax) {
                inside = !inside;
            }
        }
    }
    inside
}

/// A vertex of a polygon: the ring and the place in the ring.
pub type Vertex = (usize, usize);

/// A place on a selected shape that a drag changes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    /// A handle of a rectangle, clockwise from the top-left corner: 0
    /// top-left, 1 top, 2 top-right, 3 right, 4 bottom-right, 5 bottom, 6
    /// bottom-left, 7 left.
    Resize(usize),
    /// A vertex of a polygon.
    Vertex(Vertex),
    /// The middle of the edge from a vertex of a polygon to the next vertex.
    Edge(Vertex),
}

/// The handles of a shape and their page points.
pub fn handles(shape: &FrameShape) -> Vec<(Handle, Point)> {
    match shape {
        FrameShape::Rectangle(_) => {
            let b = shape_bbox(shape);
            let c = b.center();
            let points = [
                (b.x0, b.y0),
                (c.x, b.y0),
                (b.x1, b.y0),
                (b.x1, c.y),
                (b.x1, b.y1),
                (c.x, b.y1),
                (b.x0, b.y1),
                (b.x0, c.y),
            ];
            let points = points.into_iter().map(|(x, y)| Point::new(x, y));
            points
                .enumerate()
                .map(|(i, p)| (Handle::Resize(i), p))
                .collect()
        }
        FrameShape::Polygon(rings) => {
            let mut out = Vec::new();
            for (r, ring) in rings.iter().enumerate() {
                for (i, &[x, y]) in ring.iter().enumerate() {
                    out.push((Handle::Vertex((r, i)), Point::new(x, y)));
                }
            }
            for (r, ring) in rings.iter().enumerate() {
                for i in 0..ring.len() {
                    let [ax, ay] = ring[i];
                    let [bx, by] = ring[(i + 1) % ring.len()];
                    let middle = Point::new((ax + bx) / 2.0, (ay + by) / 2.0);
                    out.push((Handle::Edge((r, i)), middle));
                }
            }
            out
        }
    }
}

fn vertex_mut(shape: &mut FrameShape, (r, i): Vertex) -> Option<&mut [f64; 2]> {
    match shape {
        FrameShape::Polygon(rings) => rings.get_mut(r)?.get_mut(i),
        FrameShape::Rectangle(_) => None,
    }
}

/// Moves a vertex of a polygon to a page point. Returns false if the
/// shape has no such vertex.
pub fn move_vertex(shape: &mut FrameShape, v: Vertex, to: Point) -> bool {
    match vertex_mut(shape, v) {
        Some(p) => {
            *p = [to.x, to.y];
            true
        }
        None => false,
    }
}

/// Puts a vertex at the middle of the edge from vertex `v` to the next
/// vertex. Returns the new vertex.
pub fn insert_vertex(shape: &mut FrameShape, (r, i): Vertex) -> Option<Vertex> {
    let FrameShape::Polygon(rings) = shape else {
        return None;
    };
    let ring = rings.get_mut(r)?;
    let [ax, ay] = *ring.get(i)?;
    let [bx, by] = ring[(i + 1) % ring.len()];
    ring.insert(i + 1, [(ax + bx) / 2.0, (ay + by) / 2.0]);
    Some((r, i + 1))
}

/// Removes a vertex of a polygon. A ring keeps at least 3 vertices: the
/// outer ring refuses, and a hole of 3 vertices goes away. Returns false if
/// nothing changed.
pub fn remove_vertex(shape: &mut FrameShape, (r, i): Vertex) -> bool {
    let FrameShape::Polygon(rings) = shape else {
        return false;
    };
    let Some(ring) = rings.get_mut(r) else {
        return false;
    };
    if i >= ring.len() {
        return false;
    }
    if ring.len() > 3 {
        ring.remove(i);
        true
    } else if r > 0 {
        rings.remove(r);
        true
    } else {
        false
    }
}

/// Adds a square hole at the anchor of a shape. Its side is half of the
/// short side of the box of the shape. A rectangle becomes a polygon.
/// Returns the ring of the hole.
pub fn add_hole(shape: &mut FrameShape) -> usize {
    let b = shape_bbox(shape);
    let c = b.center();
    let h = b.width().min(b.height()) / 4.0;
    let hole = vec![
        [c.x - h, c.y - h],
        [c.x - h, c.y + h],
        [c.x + h, c.y + h],
        [c.x + h, c.y - h],
    ];
    let mut rings = shape.rings();
    rings.push(hole);
    *shape = FrameShape::Polygon(rings);
    shape.rings().len() - 1
}

/// The shape as a polygon. A rectangle gives its 4 corners.
pub fn to_polygon(shape: &FrameShape) -> FrameShape {
    FrameShape::Polygon(shape.rings())
}

/// The shape as a rectangle: the box of ring 0.
pub fn to_rectangle(shape: &FrameShape) -> FrameShape {
    let b = shape_bbox(shape);
    FrameShape::Rectangle([b.x0, b.y0, b.x1, b.y1])
}

/// Moves a resize handle of a rectangle to a page point. The rectangle keeps
/// a width and a height of at least `MIN_SIZE_MM`. Returns false if the
/// shape is not a rectangle or the handle does not exist.
pub fn resize(shape: &mut FrameShape, handle: usize, to: Point) -> bool {
    let FrameShape::Rectangle(r) = shape else {
        return false;
    };
    if handle >= 8 {
        return false;
    }
    let b = Rect::new(r[0], r[1], r[2], r[3]).abs();
    let (mut x0, mut y0, mut x1, mut y1) = (b.x0, b.y0, b.x1, b.y1);
    if matches!(handle, 0 | 6 | 7) {
        x0 = to.x.min(x1 - MIN_SIZE_MM);
    }
    if matches!(handle, 2..=4) {
        x1 = to.x.max(x0 + MIN_SIZE_MM);
    }
    if matches!(handle, 0..=2) {
        y0 = to.y.min(y1 - MIN_SIZE_MM);
    }
    if matches!(handle, 4..=6) {
        y1 = to.y.max(y0 + MIN_SIZE_MM);
    }
    *r = [x0, y0, x1, y1];
    true
}

/// Moves a shape by `d` page millimeters.
pub fn move_shape(shape: &mut FrameShape, d: Vec2) {
    match shape {
        FrameShape::Rectangle(r) => {
            *r = [r[0] + d.x, r[1] + d.y, r[2] + d.x, r[3] + d.y];
        }
        FrameShape::Polygon(rings) => {
            for p in rings.iter_mut().flatten() {
                *p = [p[0] + d.x, p[1] + d.y];
            }
        }
    }
}

/// A new frame of a map for a page. The first frame of a page fills the
/// page inside the margin. A later frame is one third of the width and of
/// the height, at the bottom-right corner inside the margin. The center is
/// the center of the projection, and the scale fits the projection in the
/// frame. The ID is a placeholder.
pub fn new_frame(page: &PageRecord, map: &MapRecord, projection: &Projection) -> FrameRecord {
    let spec = projection.spec();
    let (w, h) = (page.width_mm, page.height_mm);
    let inner = Rect::new(MARGIN_MM, MARGIN_MM, w - MARGIN_MM, h - MARGIN_MM);
    let rect = if page.frames.is_empty() {
        inner
    } else {
        let (fw, fh) = (inner.width() / 3.0, inner.height() / 3.0);
        Rect::new(inner.x1 - fw, inner.y1 - fh, inner.x1, inner.y1)
    };
    FrameRecord {
        id: FrameId(0),
        map: map.id,
        shape: FrameShape::Rectangle([rect.x0, rect.y0, rect.x1, rect.y1]),
        center: [spec.lon0, spec.lat0],
        scale: fit_scale(projection.bounds(), rect.width(), rect.height()),
        rotation: 0.0,
        graticule: Graticule::default(),
        border: Border::default(),
    }
}

/// Degrees of latitude per millimeter of paper at the center of a frame.
fn degrees_per_mm(projection: &Projection, frame: &FrameRecord) -> f64 {
    let local = projection
        .local_scale(frame.center)
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);
    frame.scale / 1000.0 / local / projection.radius_m().to_radians()
}

/// The graticule interval of a frame from its scale, in degrees. The lines
/// are at least 25 mm apart on the paper.
pub fn auto_graticule_interval(projection: &Projection, frame: &FrameRecord) -> f64 {
    auto_interval(25.0 * degrees_per_mm(projection, frame))
}

/// The graticule interval that a frame draws, in degrees. `None` when the
/// graticule is off. A set interval is never less than the interval that
/// puts the lines 1.5 mm apart.
pub fn graticule_interval(projection: &Projection, frame: &FrameRecord) -> Option<f64> {
    let g = &frame.graticule;
    if !g.on {
        return None;
    }
    let deg_per_mm = degrees_per_mm(projection, frame);
    Some(match g.interval {
        Some(i) => i.max(auto_interval(1.5 * deg_per_mm)),
        None => auto_interval(25.0 * deg_per_mm),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use vale_sphere::{ProjectionKind, ProjectionSpec};
    use vale_store::{MapId, PageId, PageUnits};

    use super::*;

    fn projection(kind: ProjectionKind, radius_m: f64) -> Projection {
        let spec = ProjectionSpec::new(kind, 0.0, 0.0);
        Projection::new(spec, radius_m).unwrap()
    }

    fn frame(scale: f64) -> FrameRecord {
        FrameRecord {
            id: FrameId(0),
            map: MapId(0),
            shape: FrameShape::Rectangle([20.0, 30.0, 220.0, 150.0]),
            center: [10.0, 20.0],
            scale,
            rotation: 30.0,
            graticule: Graticule {
                on: false,
                ..Graticule::default()
            },
            border: Border::default(),
        }
    }

    fn page(frames: usize) -> PageRecord {
        PageRecord {
            id: PageId(0),
            name: String::new(),
            width_mm: 297.0,
            height_mm: 210.0,
            units: PageUnits::Mm,
            frames: (0..frames).map(|_| frame(1e7)).collect(),
        }
    }

    fn map() -> MapRecord {
        MapRecord {
            id: MapId(0),
            name: "Map".to_string(),
            projection: "+proj=eqearth +lon_0=15".to_string(),
            hidden: BTreeSet::new(),
            overrides: Default::default(),
        }
    }

    fn page_point(p: &Projection, f: &FrameRecord, lonlat: [f64; 2]) -> Point {
        let vt = frame_transform(p, f, FrameTarget::MM);
        vt.to_page(p.forward(lonlat).unwrap())
    }

    fn lonlat_at(p: &Projection, f: &FrameRecord, at: Point) -> [f64; 2] {
        let vt = frame_transform(p, f, FrameTarget::MM);
        p.inverse(vt.from_page(at)).unwrap()
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn the_scale_is_true_for_any_radius() {
        let n = 10_000_000.0;
        for radius_m in [1_000_000.0, 10_000_000.0] {
            for kind in [ProjectionKind::Equirectangular, ProjectionKind::Mercator] {
                let p = projection(kind, radius_m);
                let mut f = frame(n);
                f.center = [0.0, 0.0];
                f.rotation = 0.0;
                let a = page_point(&p, &f, [0.0, 0.0]);
                let b = page_point(&p, &f, [1.0, 0.0]);
                let want = 2.0 * std::f64::consts::PI * radius_m / 360.0 * 1000.0 / n;
                let got = (b - a).hypot();
                assert!(
                    (got / want - 1.0).abs() < 1e-6,
                    "{kind:?} {radius_m}: {got} {want}"
                );
            }
        }
    }

    #[test]
    fn the_center_is_at_the_anchor_in_any_target() {
        let p = projection(ProjectionKind::EqualEarth, 6_371_000.0);
        let f = frame(5e7);
        let target = FrameTarget {
            origin: Point::new(5.0, 7.0),
            pts_per_mm: PT_PER_MM,
        };
        let vt = frame_transform(&p, &f, target);
        let c = vt.to_page(p.forward(f.center).unwrap());
        assert!((c - target.to_target(Point::new(120.0, 90.0))).hypot() < 1e-6);
        assert!((vt.width - 200.0 * PT_PER_MM).abs() < 1e-9);
    }

    #[test]
    fn zoom_and_rotate_keep_the_place_under_the_gesture() {
        let p = projection(ProjectionKind::EqualEarth, 6_371_000.0);
        let at = Point::new(60.0, 50.0);
        let mut f = frame(5e7);
        let before = lonlat_at(&p, &f, at);
        assert!(zoom(&mut f, &p, at, 2.0));
        assert_eq!(f.scale, 2.5e7);
        assert!(near(lonlat_at(&p, &f, at), before));
        assert!(rotate(&mut f, &p, at, 345.0));
        assert_eq!(f.rotation, 15.0);
        assert!(near(lonlat_at(&p, &f, at), before));
        assert!(!rotate(&mut f, &p, at, 360.0));
        f.scale = 2e3;
        assert!(zoom(&mut f, &p, at, 10.0));
        assert_eq!(f.scale, SCALE_RANGE.0);
        assert!(!zoom(&mut f, &p, at, 2.0));
    }

    #[test]
    fn pan_moves_the_center_with_the_map() {
        let p = projection(ProjectionKind::EqualEarth, 6_371_000.0);
        let mut f = frame(5e7);
        let anchor = anchor(&f.shape);
        let start = f.center;
        let d = Vec2::new(12.0, -8.0);
        assert!(pan(&mut f, &p, d));
        assert_ne!(f.center, start);
        // The old center is now under the anchor moved by d.
        assert!(near(lonlat_at(&p, &f, anchor + d), start));
    }

    #[test]
    fn fit_scale_is_a_nice_number_that_fits() {
        let bounds = Rect::new(0.0, 0.0, 3.4e7, 1.6e7);
        let n = fit_scale(bounds, 277.0, 190.0);
        assert_eq!(n, 2e8);
        assert!(bounds.width() * 1000.0 / n <= 277.0);
        assert_eq!(fit_scale(Rect::new(0.0, 0.0, 1.0, 1.0), 100.0, 100.0), 1e3);
    }

    #[test]
    fn presets_and_text() {
        assert_eq!(preset_of(297.0, 210.0), "A4");
        assert_eq!(preset_of(215.9, 279.4), "Letter");
        assert_eq!(preset_of(100.0, 100.0), "Custom");
        assert_eq!(scale_text(90_000_000.0), "1:90,000,000");
        assert_eq!(scale_text(500.0), "1:500");
        assert_eq!(parse_scale("1:90,000,000"), Some(9e7));
        assert_eq!(parse_scale(" 1 : 90 000 000 "), Some(9e7));
        assert_eq!(parse_scale("90000000"), Some(9e7));
        assert_eq!(parse_scale("0"), None);
        assert_eq!(parse_scale("a"), None);
        assert_eq!(lon_text(-12.5), "12.5° W");
        assert_eq!(lat_text(40.0), "40° N");
        assert_eq!(lat_text(0.0), "0°");
        assert_eq!(parse_degrees("12.5 W"), Some(-12.5));
        assert_eq!(parse_degrees("40° n"), Some(40.0));
        assert_eq!(parse_degrees("-3"), Some(-3.0));
        assert_eq!(parse_degrees(lon_text(-12.5).as_str()), Some(-12.5));
        assert_eq!(parse_degrees("x"), None);
    }

    fn notched() -> FrameShape {
        FrameShape::Polygon(vec![
            vec![
                [0.0, 0.0],
                [100.0, 0.0],
                [100.0, 100.0],
                [50.0, 50.0],
                [0.0, 100.0],
            ],
            vec![[20.0, 10.0], [30.0, 10.0], [30.0, 20.0], [20.0, 20.0]],
        ])
    }

    #[test]
    fn a_hole_and_a_notch_are_outside() {
        let s = notched();
        assert!(contains(&s, Point::new(80.0, 30.0)));
        assert!(!contains(&s, Point::new(25.0, 15.0)));
        assert!(!contains(&s, Point::new(50.0, 90.0)));
        assert!(!contains(&s, Point::new(150.0, 50.0)));
        let r = FrameShape::Rectangle([10.0, 10.0, 20.0, 20.0]);
        assert!(contains(&r, Point::new(15.0, 15.0)));
        assert!(!contains(&r, Point::new(25.0, 15.0)));
    }

    #[test]
    fn vertices_move_come_and_go() {
        let mut s = notched();
        assert!(move_vertex(&mut s, (0, 3), Point::new(50.0, 60.0)));
        assert_eq!(s.rings()[0][3], [50.0, 60.0]);
        assert_eq!(insert_vertex(&mut s, (0, 4)), Some((0, 5)));
        assert_eq!(s.rings()[0][5], [0.0, 50.0]);
        assert!(remove_vertex(&mut s, (0, 5)));
        assert_eq!(s.rings()[0].len(), 5);
        // A hole of 3 vertices goes away.
        assert!(remove_vertex(&mut s, (1, 0)));
        assert!(remove_vertex(&mut s, (1, 0)));
        assert_eq!(s.rings().len(), 1);
        // The outer ring keeps 3 vertices.
        assert!(remove_vertex(&mut s, (0, 0)));
        assert!(remove_vertex(&mut s, (0, 0)));
        assert!(!remove_vertex(&mut s, (0, 0)));
        assert_eq!(s.rings()[0].len(), 3);
        assert!(!move_vertex(&mut s, (4, 0), Point::ZERO));
    }

    #[test]
    fn shapes_change_kind_and_get_holes() {
        let r = FrameShape::Rectangle([10.0, 20.0, 50.0, 100.0]);
        let p = to_polygon(&r);
        assert_eq!(p.rings(), r.rings());
        assert!(matches!(p, FrameShape::Polygon(_)));
        assert_eq!(
            to_rectangle(&notched()),
            FrameShape::Rectangle([0.0, 0.0, 100.0, 100.0])
        );
        let mut s = r.clone();
        assert_eq!(add_hole(&mut s), 1);
        let hole = bbox_of(&s.rings()[1]);
        assert_eq!(hole, Rect::new(20.0, 50.0, 40.0, 70.0));
        assert!(!contains(&s, Point::new(30.0, 60.0)));
    }

    #[test]
    fn a_rectangle_resizes_by_its_handles_down_to_the_minimum() {
        let mut s = FrameShape::Rectangle([10.0, 20.0, 50.0, 100.0]);
        assert!(resize(&mut s, 4, Point::new(70.0, 120.0)));
        assert_eq!(s, FrameShape::Rectangle([10.0, 20.0, 70.0, 120.0]));
        assert!(resize(&mut s, 7, Point::new(200.0, 0.0)));
        assert_eq!(s, FrameShape::Rectangle([65.0, 20.0, 70.0, 120.0]));
        assert!(resize(&mut s, 1, Point::new(0.0, 500.0)));
        assert_eq!(s, FrameShape::Rectangle([65.0, 115.0, 70.0, 120.0]));
        assert!(!resize(&mut s, 8, Point::ZERO));
        move_shape(&mut s, Vec2::new(1.0, -2.0));
        assert_eq!(s, FrameShape::Rectangle([66.0, 113.0, 71.0, 118.0]));
        let hs = handles(&s);
        assert_eq!(hs.len(), 8);
        assert_eq!(hs[3], (Handle::Resize(3), Point::new(71.0, 115.5)));
    }

    #[test]
    fn a_polygon_has_vertex_and_edge_handles() {
        let hs = handles(&notched());
        assert_eq!(hs.len(), 18);
        assert_eq!(hs[0], (Handle::Vertex((0, 0)), Point::ZERO));
        assert!(hs.contains(&(Handle::Edge((0, 4)), Point::new(0.0, 50.0))));
    }

    #[test]
    fn a_new_frame_fills_the_page_then_takes_a_third() {
        let spec = ProjectionSpec::new(ProjectionKind::EqualEarth, 15.0, 0.0);
        let p = Projection::new(spec, 6_371_000.0).unwrap();
        let first = new_frame(&page(0), &map(), &p);
        assert_eq!(
            first.shape,
            FrameShape::Rectangle([10.0, 10.0, 287.0, 200.0])
        );
        assert_eq!(first.center, [15.0, 0.0]);
        assert_eq!(first.scale, fit_scale(p.bounds(), 277.0, 190.0));
        let b = p.bounds();
        assert!(b.width() * 1000.0 / first.scale <= 277.0);
        assert!(b.height() * 1000.0 / first.scale <= 190.0);
        let later = new_frame(&page(1), &map(), &p);
        let FrameShape::Rectangle([x0, y0, x1, y1]) = later.shape else {
            panic!("a rectangle");
        };
        assert!((x1 - 287.0).abs() < 1e-9 && (y1 - 200.0).abs() < 1e-9);
        assert!((x1 - x0 - 277.0 / 3.0).abs() < 1e-9 && (y1 - y0 - 190.0 / 3.0).abs() < 1e-9);
        assert!(later.scale > first.scale);
    }

    #[test]
    fn the_auto_interval_follows_the_frame_scale() {
        let p = projection(ProjectionKind::Equirectangular, 6_371_000.0);
        let mut f = frame(1e8);
        f.center = [0.0, 0.0];
        // 25 mm at 1:100,000,000 is about 22.5°.
        assert_eq!(auto_graticule_interval(&p, &f), 30.0);
        f.scale = 1e6;
        // 25 mm at 1:1,000,000 is about 13.5′.
        assert_eq!(auto_graticule_interval(&p, &f), 15.0 / 60.0);
        f.scale = 5e3;
        assert!(auto_graticule_interval(&p, &f) < 5.0 / 3600.0 + 1e-12);
    }

    #[test]
    fn the_graticule_interval_is_off_set_or_auto() {
        let p = projection(ProjectionKind::Equirectangular, 6_371_000.0);
        let mut f = frame(1e6);
        f.center = [0.0, 0.0];
        assert_eq!(graticule_interval(&p, &f), None);
        f.graticule.on = true;
        assert_eq!(
            graticule_interval(&p, &f),
            Some(auto_graticule_interval(&p, &f))
        );
        f.graticule.interval = Some(1.0);
        assert_eq!(graticule_interval(&p, &f), Some(1.0));
        f.graticule.interval = Some(1.0 / 60.0);
        assert_eq!(graticule_interval(&p, &f), Some(1.0 / 60.0));
    }

    #[test]
    fn a_fine_set_interval_coarsens_on_a_small_scale() {
        let p = projection(ProjectionKind::Equirectangular, 6_371_000.0);
        let mut f = frame(1e8);
        f.center = [0.0, 0.0];
        f.graticule.on = true;
        f.graticule.interval = Some(1.0 / 3600.0);
        // 1.5 mm at 1:100,000,000 is about 1.35°.
        assert_eq!(graticule_interval(&p, &f), Some(2.0));
    }
}
