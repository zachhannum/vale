//! Graticule lines.

use crate::frame::LonLat;

/// Meridians and parallels every `step_deg` degrees, as two-point lines. Meridians come first.
pub fn graticule(step_deg: f64) -> Vec<Vec<LonLat>> {
    let mut out = Vec::new();
    let n = (180.0 / step_deg).ceil() as i64;
    for k in -n..n {
        let lon = k as f64 * step_deg;
        if (-180.0..180.0).contains(&lon) {
            out.push(vec![[lon, -90.0], [lon, 90.0]]);
        }
    }
    let m = (90.0 / step_deg).ceil() as i64;
    for k in -m..=m {
        let lat = k as f64 * step_deg;
        if lat > -90.0 && lat < 90.0 {
            out.push(vec![[-180.0, lat], [180.0, lat]]);
        }
    }
    out
}

/// The smallest of 1, 2, 5, 10, 15, 30 that is at least `min_step_deg`, or 30.
pub fn nice_step(min_step_deg: f64) -> f64 {
    [1.0, 2.0, 5.0, 10.0, 15.0, 30.0]
        .into_iter()
        .find(|s| *s >= min_step_deg)
        .unwrap_or(30.0)
}

/// Great-circle distance in kilometers (haversine).
pub fn distance_km(a: LonLat, b: LonLat, radius_km: f64) -> f64 {
    let (la, lb) = (a[1].to_radians(), b[1].to_radians());
    let dlat = lb - la;
    let dlon = (b[0] - a[0]).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * radius_km * h.sqrt().min(1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts() {
        let g = graticule(30.0);
        assert_eq!(g.iter().filter(|l| l[0][1] == -90.0).count(), 12);
        assert_eq!(g.iter().filter(|l| l[0][1] != -90.0).count(), 5);
        assert_eq!(g.len(), 17);
        assert_eq!(g[0][0][0], -180.0);
    }

    #[test]
    fn steps_and_distance() {
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(40.0), 30.0);
        assert!((distance_km([0.0, 0.0], [90.0, 0.0], 6371.0) - 10007.5).abs() < 0.5);
    }
}
