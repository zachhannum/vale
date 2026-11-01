//! The terrain fit of the line tool. When the pen lifts, the free runs of a
//! stroke move onto the path of least effort over the first height layer.

use vale_store::{Feature, Geometry, LonLat};
use vale_terrain::{Heightmap, RouteSpec, fit_route, is_water};

use super::line::{LineSettings, chains, dirs, join_end};
use super::math::V3;
use super::snap::Run;

/// The runs of a stroke after the terrain fit. With the fit off, or at the
/// sketch end of the slider, the runs stay as they are. A stroke that
/// starts near a line end starts at that end, so it extends that line.
pub fn fit_runs(
    map: &Heightmap,
    features: &[Feature],
    runs: &[Run],
    scale: f64,
    settings: &LineSettings,
    radius_m: f64,
) -> Vec<Run> {
    if !settings.fit || settings.terrain <= 0.0 {
        return runs.to_vec();
    }
    let spec = RouteSpec {
        radius_m,
        terrain: settings.terrain.min(1.0),
    };
    let roads = roads(features);
    let water = |d: V3| is_water(map, d);
    let mut out = runs.to_vec();
    if let Some(Run::Free(samples)) = out.first_mut()
        && let Some(&first) = samples.first()
        && let Some(end) = join_end(features, first, scale)
    {
        samples[0] = end;
    }
    for run in &mut out {
        if let Run::Free(samples) = run
            && samples.len() >= 2
            && let Some(line) = fit_route(map, samples, &spec, &water, &roads)
        {
            *samples = line;
        }
    }
    out
}

/// Each chain of the lines of a layer, as points on the unit sphere.
fn roads(features: &[Feature]) -> Vec<Vec<V3>> {
    let mut out = Vec::new();
    for f in features {
        let Geometry::Lines(parts) = &f.geometry else {
            continue;
        };
        for chain in chains(parts) {
            let ordered: Vec<Vec<LonLat>> = chain.iter().map(|&i| parts[i].clone()).collect();
            out.push(dirs(&ordered));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use vale_terrain::meters_to_level;

    use super::*;
    use crate::globe::line::tests::{SCALE, feature, free, lines, stroke};
    use crate::globe::line::{LineEdit, SIGMA, commit};
    use crate::globe::math::{angle, lonlat_to_dir};

    const N: usize = 256;
    const RADIUS: f64 = 1.0e6;

    fn flat() -> Heightmap {
        Heightmap::new(N, meters_to_level(200.0))
    }

    fn on(terrain: f64) -> LineSettings {
        LineSettings {
            fit: true,
            terrain,
            ..LineSettings::default()
        }
    }

    fn texel() -> f64 {
        FRAC_PI_2 / N as f64
    }

    fn fitted(
        map: &Heightmap,
        features: &[Feature],
        samples: Vec<V3>,
        s: &LineSettings,
    ) -> LineEdit {
        let runs = fit_runs(map, features, &free(samples), SCALE, s, RADIUS);
        commit(features, &runs, SCALE, s.smoothing).unwrap()
    }

    fn points(edit: &LineEdit) -> Vec<V3> {
        let parts = lines(edit).iter().flatten();
        parts.map(|p| lonlat_to_dir(p[0], p[1])).collect()
    }

    /// The distance from `p` to the nearest point of `line`, in radians.
    fn off(p: V3, line: &[V3]) -> f64 {
        line.iter()
            .map(|&q| angle(p, q))
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn on_flat_ground_the_fit_stays_within_the_smoothing_of_the_stroke() {
        let map = flat();
        let s = on(0.5);
        let samples = stroke([-5.0, -3.0], [5.0, 1.0], 400);
        let edit = fitted(&map, &[], samples.clone(), &s);
        let reach = SIGMA * s.smoothing / SCALE + texel();
        for p in points(&edit) {
            let d = off(p, &samples);
            assert!(d <= reach, "{} texels from the stroke", d / texel());
        }
    }

    #[test]
    fn at_the_sketch_end_the_fit_gives_the_line_of_the_sketch() {
        let map = flat();
        let old = feature(vec![vec![[0.0, 0.0], [10.0, 0.0]]]);
        let features = [old];
        let samples = stroke([10.5, 0.2], [20.0, 2.0], 60);
        let off = LineSettings::default();
        let sketch = fitted(&map, &features, samples.clone(), &on(0.0));
        assert_eq!(sketch, fitted(&map, &features, samples, &off));
    }

    #[test]
    fn a_fit_that_starts_near_a_line_end_extends_that_line() {
        let map = flat();
        let near = 6.0 / SCALE;
        let old = feature(vec![vec![[0.0, 0.0], [10.0, 0.0]]]);
        let other = feature(vec![vec![[0.0, 30.0], [10.0, 30.0]]]);
        let features = [other, old.clone()];
        let from = [10.0 + near.to_degrees(), 0.0];
        let edit = fitted(&map, &features, stroke(from, [20.0, 2.0], 40), &on(0.5));
        let LineEdit::Extend { uuid, .. } = &edit else {
            panic!("a new line");
        };
        assert_eq!(*uuid, old.uuid());
        let parts = lines(&edit);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0][..2], [[0.0, 0.0], [10.0, 0.0]]);
        assert!((parts[0][parts[0].len() - 1][0] - 20.0).abs() < 1e-6);
    }

    #[test]
    fn a_fit_follows_a_line_of_the_layer_that_runs_the_same_way() {
        let map = flat();
        let lat = (3.0 * texel()).to_degrees();
        let road: Vec<LonLat> = (-6..=6).map(|lon| [f64::from(lon), lat]).collect();
        let features = [feature(vec![road.clone()])];
        let dense = (-600..=600).map(|i| lonlat_to_dir(f64::from(i) / 100.0, lat));
        let road: Vec<V3> = dense.collect();
        let edit = fitted(
            &map,
            &features,
            stroke([-10.0, 0.0], [10.0, 0.0], 400),
            &on(0.5),
        );
        assert!(matches!(edit, LineEdit::New(_)));
        let line = points(&edit);
        for lon in [-4.0, -2.0, 0.0, 2.0, 4.0] {
            let at = lonlat_to_dir(lon, lat);
            let p = line
                .iter()
                .copied()
                .min_by(|&a, &b| angle(a, at).total_cmp(&angle(b, at)))
                .unwrap();
            let miss = off(p, &road) / texel();
            assert!(miss <= 1.0, "at {lon} degrees the fit is {miss} texels off");
        }
    }

    #[test]
    fn a_fit_across_180_degrees_has_two_parts_that_meet() {
        let map = flat();
        let edit = fitted(
            &map,
            &[],
            stroke([170.0, 10.0], [-170.0, 10.0], 400),
            &on(0.5),
        );
        let parts = lines(&edit);
        assert_eq!(parts.len(), 2);
        let (end, start) = (parts[0][parts[0].len() - 1], parts[1][0]);
        assert_eq!((end[0], start[0]), (180.0, -180.0));
        assert_eq!(end[1], start[1]);
        assert!(parts[0].iter().all(|p| p[0] >= 170.0 - 1e-9));
        assert!(parts[1].iter().all(|p| p[0] <= -170.0 + 1e-9));
    }
}
