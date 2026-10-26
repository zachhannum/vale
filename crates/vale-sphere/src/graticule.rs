//! Graticule lines.

use crate::clip::ClipRect;
use crate::frame::{LonLat, wrap180};

const SEC: f64 = 1.0 / 3600.0;
const MIN: f64 = 1.0 / 60.0;

/// Graticule intervals in degrees, ascending, from 1″ to 30°.
pub const INTERVALS: [f64; 21] = [
    SEC,
    2.0 * SEC,
    5.0 * SEC,
    10.0 * SEC,
    15.0 * SEC,
    20.0 * SEC,
    30.0 * SEC,
    MIN,
    2.0 * MIN,
    5.0 * MIN,
    10.0 * MIN,
    15.0 * MIN,
    20.0 * MIN,
    30.0 * MIN,
    1.0,
    2.0,
    5.0,
    10.0,
    15.0,
    20.0,
    30.0,
];

/// Relative tolerance for float noise in interval arithmetic.
const TOL: f64 = 1e-9;

/// The smallest interval in [`INTERVALS`] that is at least `min_deg`, clamped to the ends of the list.
pub fn auto_interval(min_deg: f64) -> f64 {
    INTERVALS
        .into_iter()
        .find(|s| *s >= min_deg * (1.0 - TOL))
        .unwrap_or(INTERVALS[INTERVALS.len() - 1])
}

/// An interval as text: whole degrees as "30°", whole minutes as "15′", else seconds as "5″".
pub fn interval_text(deg: f64) -> String {
    let whole = |v: f64| (v - v.round()).abs() < TOL * v.abs().max(1.0);
    if whole(deg) {
        format!("{}°", deg.round())
    } else if whole(deg * 60.0) {
        format!("{}′", (deg * 60.0).round())
    } else {
        format!("{}″", (deg * 3600.0 * 1000.0).round() / 1000.0)
    }
}

/// Meridians and parallels every `step_deg` degrees over the whole sphere. Meridians come first.
pub fn graticule(step_deg: f64) -> Vec<Vec<LonLat>> {
    graticule_in(step_deg, ClipRect::WORLD)
}

/// Meridians and parallels at multiples of `interval` degrees inside `window`. Meridians come first.
///
/// The longitudes of the window can extend past ±180. Output longitudes are wrapped into -180..180, so a line that
/// crosses 180° jumps there. Parallels leave out the poles. Points are at most min(1°, `interval`/4) apart.
pub fn graticule_in(interval: f64, window: ClipRect) -> Vec<Vec<LonLat>> {
    let mut out = Vec::new();
    if !interval.is_finite() || interval <= 0.0 {
        return out;
    }
    let (lon_lo, lon_hi) = (window.lon_min, window.lon_max);
    let (lat_lo, lat_hi) = (window.lat_min.max(-90.0), window.lat_max.min(90.0));
    if !(lon_hi >= lon_lo && lat_hi >= lat_lo) {
        return out;
    }
    let spacing = interval.min(4.0) / 4.0;
    let wrap = |lon: f64| {
        if (-180.0..180.0).contains(&lon) {
            lon
        } else {
            wrap180(lon)
        }
    };
    let k_range = |lo: f64, hi: f64| {
        (
            (lo / interval - TOL).ceil() as i64,
            (hi / interval + TOL).floor() as i64,
        )
    };

    // A window of 360° or more holds each meridian one time.
    let (k0, mut k1) = k_range(lon_lo, lon_hi);
    k1 = k1.min(k0 + (360.0 / interval - TOL).ceil() as i64 - 1);
    let n_lat = ((lat_hi - lat_lo) / spacing).ceil().max(1.0) as usize;
    for k in k0..=k1 {
        let lon = wrap((k as f64 * interval).clamp(lon_lo, lon_hi));
        out.push(steps(lat_lo, lat_hi, n_lat).map(|lat| [lon, lat]).collect());
    }

    let (k0, k1) = k_range(lat_lo, lat_hi);
    let n_lon = ((lon_hi - lon_lo) / spacing).ceil().max(1.0) as usize;
    for k in k0..=k1 {
        let lat = (k as f64 * interval).clamp(lat_lo, lat_hi);
        if lat.abs() < 90.0 {
            out.push(
                steps(lon_lo, lon_hi, n_lon)
                    .map(|lon| [wrap(lon), lat])
                    .collect(),
            );
        }
    }
    out
}

/// `n + 1` evenly spaced values from `a` to `b`, with both ends exact.
fn steps(a: f64, b: f64, n: usize) -> impl Iterator<Item = f64> {
    (0..=n).map(move |j| {
        if j == n {
            b
        } else {
            a + (b - a) * j as f64 / n as f64
        }
    })
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
        assert!((distance_km([0.0, 0.0], [90.0, 0.0], 6371.0) - 10007.5).abs() < 0.5);
    }

    #[test]
    fn auto_interval_picks() {
        assert!(INTERVALS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(auto_interval(7.0), 10.0);
        assert_eq!(auto_interval(30.0), 30.0);
        assert_eq!(auto_interval(40.0), 30.0);
        assert_eq!(auto_interval(1.0), 1.0);
        assert_eq!(auto_interval(0.9), 1.0);
        assert_eq!(auto_interval(0.2), 15.0 / 60.0);
        assert_eq!(auto_interval(3.0 / 3600.0), 5.0 / 3600.0);
        assert_eq!(auto_interval(0.0), INTERVALS[0]);
        assert_eq!(auto_interval(-5.0), INTERVALS[0]);
        assert_eq!(auto_interval(f64::INFINITY), 30.0);
        for i in INTERVALS {
            assert_eq!(auto_interval(i), i);
        }
    }

    #[test]
    fn interval_texts() {
        assert_eq!(interval_text(30.0), "30°");
        assert_eq!(interval_text(1.0), "1°");
        assert_eq!(interval_text(15.0 / 60.0), "15′");
        assert_eq!(interval_text(1.0 / 60.0), "1′");
        assert_eq!(interval_text(5.0 / 3600.0), "5″");
        let texts: Vec<String> = INTERVALS.iter().map(|i| interval_text(*i)).collect();
        assert_eq!(texts[0], "1″");
        assert_eq!(texts[6], "30″");
        assert_eq!(texts[7], "1′");
        assert_eq!(texts[13], "30′");
        assert_eq!(texts[14], "1°");
    }

    fn inside(p: LonLat, w: &ClipRect) -> bool {
        let e = 1e-9;
        let lon_ok = (0..3).any(|t| {
            let lon = p[0] + (t as f64 - 1.0) * 360.0;
            lon >= w.lon_min - e && lon <= w.lon_max + e
        });
        lon_ok && p[1] >= w.lat_min - e && p[1] <= w.lat_max + e
    }

    #[test]
    fn stays_inside_window() {
        for (interval, w) in [
            (
                5.0,
                ClipRect {
                    lon_min: 12.3,
                    lon_max: 47.9,
                    lat_min: -8.1,
                    lat_max: 31.0,
                },
            ),
            (
                10.0 / 3600.0,
                ClipRect {
                    lon_min: 7.0101,
                    lon_max: 7.0302,
                    lat_min: 45.0,
                    lat_max: 45.02,
                },
            ),
            (
                1.0 / 60.0,
                ClipRect {
                    lon_min: -0.1,
                    lon_max: 0.1,
                    lat_min: 89.9,
                    lat_max: 90.0,
                },
            ),
        ] {
            let g = graticule_in(interval, w);
            assert!(!g.is_empty());
            let max_gap = interval.min(4.0) / 4.0 + 1e-12;
            for line in &g {
                assert!(line.len() >= 2);
                for p in line {
                    assert!(inside(*p, &w), "{p:?} outside {w:?}");
                    assert!((-180.0..180.0).contains(&p[0]));
                }
                for s in line.windows(2) {
                    let d = wrap180(s[1][0] - s[0][0])
                        .abs()
                        .max((s[1][1] - s[0][1]).abs());
                    assert!(d <= max_gap, "{d} > {max_gap}");
                }
            }
            for line in g.iter().filter(|l| l[0][0] == l[1][0]) {
                let k = line[0][0] / interval;
                assert!(
                    (k - k.round()).abs() < 1e-6,
                    "meridian {} not on {interval}",
                    line[0][0]
                );
            }
        }
        // 10″ on 7.0101..7.0302: meridians at k/360° for k in 2524..=2530.
        let w = ClipRect {
            lon_min: 7.0101,
            lon_max: 7.0302,
            lat_min: 45.0,
            lat_max: 45.02,
        };
        let g = graticule_in(10.0 / 3600.0, w);
        assert_eq!(
            g.iter()
                .filter(|l| l[0][1] == 45.0 && l[0][0] == l[1][0])
                .count(),
            7
        );
        assert!(
            graticule_in(
                1.0,
                ClipRect {
                    lon_min: 0.0,
                    lon_max: 1.0,
                    lat_min: 90.0,
                    lat_max: 90.0
                }
            )
            .iter()
            .all(|l| l[0][0] == l[1][0])
        );
    }

    #[test]
    fn window_across_180_wraps() {
        use crate::{Projection, ProjectionKind, ProjectionSpec};
        let w = ClipRect {
            lon_min: 160.0,
            lon_max: 200.0,
            lat_min: -10.0,
            lat_max: 10.0,
        };
        let g = graticule_in(10.0, w);
        let lons: Vec<f64> = g
            .iter()
            .filter(|l| l[0][0] == l[1][0])
            .map(|l| l[0][0])
            .collect();
        assert_eq!(lons, vec![160.0, 170.0, -180.0, -170.0, -160.0]);
        assert_eq!(g.len() - lons.len(), 3);
        for kind in ProjectionKind::ALL {
            let p = Projection::new(ProjectionSpec::new(kind, 180.0, 0.0), 1.0).unwrap();
            for line in &g {
                for piece in p.project_line(line) {
                    assert!(
                        piece.iter().all(|q| q.x.is_finite() && q.y.is_finite()),
                        "{kind:?}"
                    );
                }
            }
        }
        let p = Projection::new(
            ProjectionSpec::new(ProjectionKind::Equirectangular, 180.0, 0.0),
            1.0,
        )
        .unwrap();
        assert!(g.iter().all(|l| !p.project_line(l).is_empty()));
    }

    #[test]
    fn wide_window_has_each_meridian_once() {
        let w = ClipRect {
            lon_min: -200.0,
            lon_max: 250.0,
            lat_min: -90.0,
            lat_max: 90.0,
        };
        let g = graticule_in(30.0, w);
        let mut lons: Vec<f64> = g
            .iter()
            .filter(|l| l[0][1] == -90.0)
            .map(|l| l[0][0])
            .collect();
        lons.sort_by(f64::total_cmp);
        lons.dedup();
        assert_eq!(lons.len(), 12);
        assert_eq!(g.iter().filter(|l| l[0][1] == -90.0).count(), 12);
    }
}
