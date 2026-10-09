//! Projections and page fit.

use kurbo::Point;

/// A spherical projection. Angles in degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Projection {
    EqualEarth { lon0: f64 },
    Laea { lon0: f64, lat0: f64 },
}

fn wrap_lon(lon: f64, lon0: f64) -> f64 {
    let mut d = (lon - lon0) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d.to_radians()
}

impl Projection {
    /// Project onto the unit sphere plane, y up. `None` when the point cannot be shown.
    pub fn forward(&self, lon: f64, lat: f64) -> Option<(f64, f64)> {
        match *self {
            Projection::EqualEarth { lon0 } => {
                const A1: f64 = 1.340264;
                const A2: f64 = -0.081106;
                const A3: f64 = 0.000893;
                const A4: f64 = 0.003796;
                let l = wrap_lon(lon, lon0);
                let phi = lat.to_radians();
                let th = (3f64.sqrt() / 2.0 * phi.sin()).asin();
                let t2 = th * th;
                let t6 = t2 * t2 * t2;
                let x = 2.0 * 3f64.sqrt() * l * th.cos()
                    / (3.0 * (9.0 * A4 * t6 * t2 + 7.0 * A3 * t6 + 3.0 * A2 * t2 + A1));
                let y = th * (A1 + A2 * t2 + t6 * (A3 + A4 * t2));
                Some((x, y))
            }
            Projection::Laea { lon0, lat0 } => {
                let l = wrap_lon(lon, lon0);
                let phi = lat.to_radians();
                let phi0 = lat0.to_radians();
                let c = phi0.sin() * phi.sin() + phi0.cos() * phi.cos() * l.cos();
                if c < -0.5 {
                    return None;
                }
                let k = (2.0 / (1.0 + c)).sqrt();
                let x = k * phi.cos() * l.sin();
                let y = k * (phi0.cos() * phi.sin() - phi0.sin() * phi.cos() * l.cos());
                Some((x, y))
            }
        }
    }
}

/// The geographic window and page that a map is fitted into.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frame {
    pub projection: Projection,
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
    pub page_width: f64,
    pub page_height: f64,
    pub margin: f64,
}

/// Maps longitude and latitude to page space (y down).
#[derive(Copy, Clone, Debug)]
pub struct PageMap {
    projection: Projection,
    scale: f64,
    offset_x: f64,
    offset_y: f64,
    page_width: f64,
}

impl PageMap {
    pub fn new(frame: &Frame) -> Self {
        const N: usize = 40;
        let mut min = (f64::INFINITY, f64::INFINITY);
        let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for i in 0..=N {
            let t = i as f64 / N as f64;
            let lon = frame.west + t * (frame.east - frame.west);
            let lat = frame.south + t * (frame.north - frame.south);
            let samples = [
                (lon, frame.south),
                (lon, frame.north),
                (frame.west, lat),
                (frame.east, lat),
            ];
            for (lo, la) in samples {
                if let Some((x, y)) = frame.projection.forward(lo, la) {
                    min = (min.0.min(x), min.1.min(y));
                    max = (max.0.max(x), max.1.max(y));
                }
            }
        }
        if min.0 > max.0 {
            min = (-1.0, -1.0);
            max = (1.0, 1.0);
        }
        let avail_w = (frame.page_width - 2.0 * frame.margin).max(1e-9);
        let avail_h = (frame.page_height - 2.0 * frame.margin).max(1e-9);
        let bw = (max.0 - min.0).max(1e-12);
        let bh = (max.1 - min.1).max(1e-12);
        let scale = (avail_w / bw).min(avail_h / bh);
        let cx = (min.0 + max.0) / 2.0;
        let cy = (min.1 + max.1) / 2.0;
        PageMap {
            projection: frame.projection,
            scale,
            offset_x: frame.page_width / 2.0 - cx * scale,
            offset_y: frame.page_height / 2.0 + cy * scale,
            page_width: frame.page_width,
        }
    }

    pub fn to_page(&self, lon: f64, lat: f64) -> Option<Point> {
        let (x, y) = self.projection.forward(lon, lat)?;
        Some(Point::new(
            self.offset_x + x * self.scale,
            self.offset_y - y * self.scale,
        ))
    }

    /// Project a line, splitting where a point is `None` or where the line jumps
    /// more than half the page width in x (a seam crossing).
    pub fn line_to_page(&self, line: &[[f64; 2]]) -> Vec<Vec<Point>> {
        let mut out: Vec<Vec<Point>> = Vec::new();
        let mut cur: Vec<Point> = Vec::new();
        for p in line {
            match self.to_page(p[0], p[1]) {
                None => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                Some(pt) => {
                    if let Some(prev) = cur.last()
                        && (pt.x - prev.x).abs() > self.page_width / 2.0
                    {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.push(pt);
                }
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out.retain(|l| l.len() >= 2);
        out
    }

    /// Project a ring. `None` when any point cannot be projected.
    pub fn ring_to_page(&self, ring: &[[f64; 2]]) -> Option<Vec<Point>> {
        ring.iter().map(|p| self.to_page(p[0], p[1])).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EE: Projection = Projection::EqualEarth { lon0: 0.0 };

    #[test]
    fn equal_earth() {
        let (x, y) = EE.forward(0.0, 0.0).unwrap();
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12);
        assert!((EE.forward(180.0, 0.0).unwrap().0 - 2.70663).abs() < 1e-4);
        assert!((EE.forward(0.0, 90.0).unwrap().1 - 1.31736).abs() < 1e-4);
        let a = EE.forward(40.0, 30.0).unwrap();
        let b = EE.forward(-40.0, -30.0).unwrap();
        assert!((a.0 + b.0).abs() < 1e-12 && (a.1 + b.1).abs() < 1e-12);
    }

    #[test]
    fn laea() {
        let p = Projection::Laea {
            lon0: 15.0,
            lat0: 52.0,
        };
        let (x, y) = p.forward(15.0, 52.0).unwrap();
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12);
        assert!(p.forward(15.0 - 180.0, -52.0).is_none());
        let q = Projection::Laea {
            lon0: 0.0,
            lat0: 0.0,
        };
        assert!((q.forward(90.0, 0.0).unwrap().0 - 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn page_map_world() {
        let f = Frame {
            projection: EE,
            west: -180.0,
            east: 180.0,
            south: -90.0,
            north: 90.0,
            page_width: 1800.0,
            page_height: 900.0,
            margin: 20.0,
        };
        let m = PageMap::new(&f);
        for (lo, la) in [
            (-180.0, -90.0),
            (180.0, -90.0),
            (-180.0, 90.0),
            (180.0, 90.0),
            (-180.0, 0.0),
            (180.0, 0.0),
        ] {
            let p = m.to_page(lo, la).unwrap();
            assert!(p.x >= 0.0 && p.x <= 1800.0 && p.y >= 0.0 && p.y <= 900.0);
        }
        let c = m.to_page(0.0, 0.0).unwrap();
        assert!((c.x - 900.0).abs() < 1e-6 && (c.y - 450.0).abs() < 1e-6);
        assert!(m.to_page(0.0, 50.0).unwrap().y < c.y);
    }
}
