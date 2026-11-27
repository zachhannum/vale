//! Sea modes painted at the scale of a GEBCO reference region, measured by
//! the realism report.

use std::path::Path;

use vale_terrain::math::{V3, add, angle, cross, lonlat_to_dir, normalize, scale};
use vale_terrain::{
    ALEUTIAN_TRENCH, ATLANTIC_RIDGE, AbyssalHills, AbyssalStroke, EPR_RIDGE, Erosion, HAWAII,
    HOTSPOT_HEIGHT_M, HOTSPOT_RADIUS_KM, HOTSPOT_SPEED_MM, Heightmap, Hotspot, HotspotLine,
    LINE_SPACING, Limits, Margin, MarginStroke, Mode, PACIFIC_FLANK, Patch, Rain, Realism,
    Reference, Ridge, RidgeLine, SeamountStroke, Seamounts, Stamp, Trench, TrenchLine,
    TrenchMargin, TrenchSide, US_EAST_MARGIN, WindowHeights, erode_hotspot, erode_margin,
    hotspot_window, margin_window, meters_to_level, realism,
};

/// The radius of the test world. A texel of the heightmap is near a GEBCO
/// cell of 460 m.
const WORLD_KM: f64 = 1000.0;
const FACE_SIZE: usize = 3328;

/// The distance of the ridge axis east of the middle of the patch, in
/// kilometers.
const AXIS_KM: f64 = 160.0;

/// The seed of the ridge.
const RIDGE_SEED: u32 = 7;

/// The depth of the ground past the ridge, in meters.
const BASE_M: f64 = 3550.0;

/// The settings of an ocean ridge on a world with a radius of `world_km`
/// at the largest brush radius, with a full spreading rate in mm/yr. The
/// other settings are the defaults of the brush.
fn ridge_spec(world_km: f64, spreading_mm: f64, crest_m: f64) -> Ridge {
    Ridge {
        radius: vale_terrain::MAX_RIDGE_RADIUS,
        crest: meters_to_level(crest_m),
        segment: 400.0 / world_km,
        offset: 100.0 / world_km,
        spread: 0.5 * spreading_mm / world_km,
        magma: 0.0,
        world_km,
        seed: RIDGE_SEED,
        limits: Limits::default(),
        hills: 1.0,
        fit: true,
        scale: 1.0,
    }
}

/// The crest depth and the spreading rate of the East Pacific Rise near
/// 11.6 N, with no offsets.
fn pacific_ridge() -> Ridge {
    Ridge {
        offset: 0.0,
        ..ridge_spec(WORLD_KM, 110.0, -2600.0)
    }
}

/// A heightmap at the scale of a reference region and the place of its
/// patch: the middle of face 0.
struct Painted {
    map: Heightmap,
    center: V3,
    /// The radius of the world, in kilometers.
    world_km: f64,
}

impl Painted {
    /// Flat ground at a depth in meters.
    fn new(depth_m: f64) -> Painted {
        Painted {
            map: Heightmap::new(FACE_SIZE, meters_to_level(-depth_m)),
            center: lonlat_to_dir(0.0, 0.0),
            world_km: WORLD_KM,
        }
    }

    /// Flat ground at a depth in meters on a world of the size of the
    /// Earth, with texels near 920 m.
    fn earth(depth_m: f64) -> Painted {
        Painted {
            map: Heightmap::new(EARTH_FACE_SIZE, meters_to_level(-depth_m)),
            center: lonlat_to_dir(0.0, 0.0),
            world_km: EARTH_KM,
        }
    }

    /// The patch of the reference, at the place of the patch.
    fn patch(&self, reference: &Reference) -> Patch {
        Patch::from_heightmap(
            &self.map,
            self.center,
            self.world_km,
            reference.size,
            reference.cell_m,
        )
    }

    /// An ocean ridge along a meridian `east_km` east of the middle of the
    /// patch, from latitude `-lat` to `lat` in degrees, so the fabric runs
    /// north. The line is long, so the patch is far from the taper at its
    /// ends.
    fn ridge(&mut self, spec: Ridge, east_km: f64, lat: f64) {
        let lon = east_km / self.world_km * 180.0 / std::f64::consts::PI;
        let mut line = RidgeLine::new(spec, self.map.face_size());
        for i in 0..=16 {
            let lat = -lat + 2.0 * lat * f64::from(i) / 16.0;
            line.push(lonlat_to_dir(lon, lat), 1.0);
        }
        self.map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            self.map.ridge(&plan);
            from = plan.end;
        }
        self.map.end_stroke();
    }

    /// Land at `land_m` outside a circle around `center` with a radius in
    /// kilometers, a rough shelf at 150 m for about `shelf_km` in front of
    /// the land, with bays and spurs as a user paints them, both up to `land_km` from the middle of the patch, and a
    /// margin stroke in rows over the patch, with a brush radius in
    /// kilometers, and its erosion.
    #[allow(clippy::too_many_arguments)]
    fn margin(
        &mut self,
        spec: Margin,
        center: V3,
        radius_km: f64,
        shelf_km: f64,
        land_m: f64,
        land_km: f64,
        brush_km: f64,
    ) {
        let n = self.map.face_size();
        let half = (land_km / self.world_km / (std::f64::consts::FRAC_PI_2 / n as f64)) as usize;
        let land = meters_to_level(land_m);
        let shelf = meters_to_level(-150.0);
        let arc = radius_km / self.world_km;
        let tau = std::f64::consts::TAU;
        for y in n / 2 - half..n / 2 + half {
            for x in n / 2 - half..n / 2 + half {
                let d = self.map.texel_dir(0, x, y);
                let (lon, lat) = (d[1].atan2(d[0]).to_degrees(), d[2].asin().to_degrees());
                let wobble = 10.0 * (tau * lat / 1.3).sin()
                    + 5.0 * (tau * lon / 0.6 + 1.0).sin()
                    + 2.0 * (tau * (lat + lon) / 0.25).sin();
                let shelf_arc = arc - (shelf_km + wobble) / self.world_km;
                match angle(d, center) {
                    a if a > arc => self.map.set(0, x, y, land),
                    a if a > shelf_arc => self.map.set(0, x, y, shelf),
                    _ => {}
                }
            }
        }
        let brush = brush_km / self.world_km;
        let mut stroke = MarginStroke::new(spec, n);
        let reach = 320.0 / self.world_km;
        let rows = (2.0 * reach / (0.6 * brush)).ceil() as usize;
        let mut plans = Vec::new();
        for row in 0..=rows {
            let y = -reach + 2.0 * reach * row as f64 / rows as f64;
            for column in 0..=rows {
                let x = -reach + 2.0 * reach * column as f64 / rows as f64;
                let p = offset(offset(self.center, std::f64::consts::FRAC_PI_2, x), 0.0, y);
                plans.push(stroke.stamp(&self.map, p, brush, 0.7, 1.0));
            }
        }
        // The erosion of the stroke, at the default Erosion and Branching
        // settings of the brush.
        let window = margin_window(&plans, n);
        let mut erosion = Erosion::window_alone(&WindowHeights::new(&self.map, window));
        erosion.set_branching(spec.seed, 0.5);
        let step = erode_margin(&mut erosion, &plans, 0.5);
        self.map.begin_stroke();
        for plan in &plans {
            self.map.margin(plan);
        }
        self.map.erode(&step);
        self.map.end_stroke();
    }

    /// The ridge of `pacific_ridge`, so the patch has the depths of the
    /// Pacific reference patch.
    fn ridge_flank(&mut self) {
        self.ridge(pacific_ridge(), AXIS_KM, 30.0);
    }

    /// A seamounts stroke in rows over the patch.
    fn seamounts(&mut self, spec: Seamounts) {
        let mut stroke = SeamountStroke::new(spec, FACE_SIZE);
        self.map.begin_stroke();
        for row in 0..5 {
            let lat = -8.0 + 4.0 * f64::from(row);
            for column in 0..9 {
                let lon = -8.0 + 2.0 * f64::from(column);
                let plan = stroke.stamp(&self.map, lonlat_to_dir(lon, lat), 0.06, 0.6, 1.0);
                self.map.seamounts(&plan);
            }
        }
        self.map.end_stroke();
    }
}

/// The side of the box of `relief`, in cells: 21 km.
const RELIEF_BOX: usize = 47;

/// The percentiles of `relief`.
const RELIEF_PERCENTILES: [f64; 5] = [1.0, 50.0, 90.0, 99.0, 99.9];

/// The local relief of the GEBCO patch, from `relief` in
/// `manual_seamounts_next_to_the_pacific_flank`.
const REFERENCE_RELIEF: [f64; 6] = [-176.0, -3.0, 70.0, 282.0, 825.0, 1394.0];

/// The local relief of a patch of `n` by `n` heights: each height minus the
/// mean of the box of `RELIEF_BOX` cells around it, for the cells whose box
/// is inside the patch. The numbers are the percentiles of
/// `RELIEF_PERCENTILES` and the largest relief, in meters.
fn relief(heights: &[f32], n: usize) -> [f64; 6] {
    // A table of the sums of the heights below and left of each corner.
    let mut sums = vec![0.0; (n + 1) * (n + 1)];
    for y in 0..n {
        for x in 0..n {
            sums[(y + 1) * (n + 1) + x + 1] = f64::from(heights[y * n + x])
                + sums[y * (n + 1) + x + 1]
                + sums[(y + 1) * (n + 1) + x]
                - sums[y * (n + 1) + x];
        }
    }
    let half = RELIEF_BOX / 2;
    let mut values = Vec::new();
    for y in half..n - half {
        for x in half..n - half {
            let (x0, y0, x1, y1) = (x - half, y - half, x + half + 1, y + half + 1);
            let sum = sums[y1 * (n + 1) + x1] - sums[y0 * (n + 1) + x1] - sums[y1 * (n + 1) + x0]
                + sums[y0 * (n + 1) + x0];
            values.push(f64::from(heights[y * n + x]) - sum / (RELIEF_BOX * RELIEF_BOX) as f64);
        }
    }
    values.sort_by(f64::total_cmp);
    let at = |p: f64| values[((p / 100.0 * values.len() as f64) as usize).min(values.len() - 1)];
    let mut out = [0.0; 6];
    for (o, p) in out.iter_mut().zip(RELIEF_PERCENTILES) {
        *o = at(p);
    }
    out[5] = values[values.len() - 1];
    out
}

fn print_relief(name: &str, relief: &[f64; 6]) {
    eprintln!(
        "  relief {name}: p1 {:.0}, p50 {:.0}, p90 {:.0}, p99 {:.0}, p99.9 {:.0}, max {:.0} m",
        relief[0], relief[1], relief[2], relief[3], relief[4], relief[5]
    );
}

/// The realism report and the local relief of a painted patch, with the
/// fabric along `direction`, next to the values of the reference region.
fn compare(painted: &Painted, reference: &Reference, direction: f64) -> (Realism, [f64; 6]) {
    let patch = painted.patch(reference);
    let report = realism(&patch, reference.band_m, direction);
    let r = &reference.realism;
    eprintln!("{}:", reference.region);
    // The mean depth, and the means of five bands of columns from west to
    // east.
    let n = patch.size;
    let mean = |columns: std::ops::Range<usize>| {
        let width = columns.len();
        let sum: f64 = (0..n)
            .flat_map(|y| columns.clone().map(move |x| y * n + x))
            .map(|k| f64::from(patch.heights[k]))
            .sum();
        sum / (n * width) as f64
    };
    let bands: Vec<String> = (0..5)
        .map(|b| format!("{:.0}", mean(b * n / 5..(b + 1) * n / 5)))
        .collect();
    eprintln!("  mean {:.0} m, columns {}", mean(0..n), bands.join(", "));
    eprintln!(
        "  spectral slope {:.3}, reference {:.3}",
        report.spectral_slope, r.spectral_slope
    );
    eprintln!(
        "  anisotropy {:.3}, reference {:.3}",
        report.anisotropy, r.anisotropy
    );
    eprintln!("  RMS {:.1} m, reference {:.1} m", report.rms_m, r.rms_m);
    eprintln!(
        "  slopes {:.3?}\n  reference {:.3?}",
        report.slope_histogram, r.slope_histogram
    );
    let local = relief(&patch.heights, n);
    print_relief("painted", &local);
    (report, local)
}

fn flank_spec() -> Seamounts {
    Seamounts {
        density: 4.0,
        height_m: 430.0,
        age_myr: 4.0,
        world_km: WORLD_KM,
        seed: 5,
        limits: Limits::default(),
    }
}

/// The mean depth of the reference patch, in meters.
const REFERENCE_DEPTH_M: f64 = 3286.0;

/// An abyssal hills stroke north over the patch, on flat floor at the mean
/// depth of the reference, then the seamounts of `flank_spec`. The stroke
/// sets the fabric, so it runs north. The second stamp covers the patch
/// with the full hills.
fn abyssal_flank() -> Painted {
    let mut painted = Painted::new(REFERENCE_DEPTH_M);
    let spec = AbyssalHills {
        rate: 110.0,
        hills: 1.0,
        fit: true,
        scale: 1.0,
        world_km: WORLD_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    paint_abyssal(&mut painted.map, spec, &meridian(24.0), 0.18);
    painted.seamounts(flank_spec());
    painted
}

/// Points north along the meridian of longitude 0, from latitude `-lat`
/// to `lat` in degrees.
fn meridian(lat: f64) -> Vec<V3> {
    (0..=16)
        .map(|i| lonlat_to_dir(0.0, -lat + 2.0 * lat * f64::from(i) / 16.0))
        .collect()
}

/// Paints an abyssal hills stroke through `points` with a radius in
/// radians, at full flow.
fn paint_abyssal(map: &mut Heightmap, spec: AbyssalHills, points: &[V3], radius: f64) {
    let mut stroke = AbyssalStroke::new(spec, map.face_size());
    map.begin_stroke();
    for plan in stroke.stamps(points, radius, 0.95, 1.0) {
        map.abyssal(&plan);
    }
    map.end_stroke();
}

/// Asserts that a realism report and a local relief are near those of the
/// reference.
fn assert_near(report: &Realism, local: &[f64; 6], reference: &Reference) {
    let r = &reference.realism;
    print_relief("reference", &REFERENCE_RELIEF);
    // The body of the local relief is near the reference, and the
    // seamounts stand above the faults.
    let (p99, p999) = (local[3], local[4]);
    assert!(
        p99 > 0.7 * REFERENCE_RELIEF[3] && p99 < 1.4 * REFERENCE_RELIEF[3],
        "a 99th percentile of the relief of {p99} m"
    );
    assert!(
        p999 > 0.6 * REFERENCE_RELIEF[4],
        "a 99.9th percentile of the relief of {p999} m"
    );
    assert!(
        (report.spectral_slope - r.spectral_slope).abs() < 0.4,
        "a spectral slope of {}",
        report.spectral_slope
    );
    assert!(
        report.anisotropy > 0.5 * r.anisotropy && report.anisotropy < 2.5 * r.anisotropy,
        "an anisotropy of {}",
        report.anisotropy
    );
    assert!(
        report.rms_m > 0.6 * r.rms_m && report.rms_m < 1.6 * r.rms_m,
        "an RMS of {} m",
        report.rms_m
    );
    // The hills cover the floor, so little of it is flat. Most of the
    // floor is gentle, and a little of it is steep.
    let flat = report.slope_histogram[0];
    let gentle: f64 = report.slope_histogram[..4].iter().sum();
    let steep: f64 = report.slope_histogram[4..].iter().sum();
    assert!(flat < 0.5, "{flat} of the slopes below 1 degree");
    assert!(gentle > 0.8, "{gentle} of the slopes below 8 degrees");
    assert!(steep > 0.005, "{steep} of the slopes above 8 degrees");
    let apart: f64 = (report.slope_histogram.iter())
        .zip(&r.slope_histogram)
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(apart < 0.5, "a slope histogram {apart} from the reference");
}

#[test]
fn abyssal_hills_with_seamounts_are_near_the_pacific_flank() {
    let reference = Reference::parse(PACIFIC_FLANK).unwrap();
    let (report, local) = compare(&abyssal_flank(), &reference, 0.0);
    assert_near(&report, &local, &reference);
}

fn pacific_flank() -> Painted {
    let mut painted = Painted::new(BASE_M);
    painted.ridge_flank();
    painted.seamounts(flank_spec());
    painted
}

#[test]
fn seamounts_on_a_ridge_flank_are_near_the_pacific_flank() {
    let reference = Reference::parse(PACIFIC_FLANK).unwrap();
    let (report, local) = compare(&pacific_flank(), &reference, 0.0);
    assert_near(&report, &local, &reference);
}

/// The faces of an Earth with texels near 920 m.
const EARTH_FACE_SIZE: usize = 10752;

/// The brush radius of the ridges on an Earth: 637 km. The hills start to
/// fade at 0.8 brush radii from the line, past the edge of the patch, and
/// the taper at an end of the line is 1.2 brush radii long.
const EARTH_RIDGE_RADIUS: f64 = 0.1;

/// A ridge on an Earth with its line through the middle of the patch, from
/// 12 S to 12 N, so the patch is past the taper at each end. The ground past
/// the ridge is at `base_m`, the depth of floor of the age at the brush
/// radius.
fn earth_ridge(spec: Ridge, base_m: f64) -> Painted {
    let mut painted = Painted::earth(base_m);
    let spec = Ridge {
        radius: EARTH_RIDGE_RADIUS,
        ..spec
    };
    painted.ridge(spec, 0.0, 12.0);
    painted
}

/// A slow ridge at the default settings of the brush, at the scale of the
/// Atlantic reference.
fn slow_ridge() -> Painted {
    earth_ridge(ridge_spec(EARTH_KM, 25.0, -1500.0), 5050.0)
}

/// A fast ridge with the spreading rate and the crest depth of the East
/// Pacific Rise near 11 N, at the scale of its reference.
fn fast_ridge() -> Painted {
    earth_ridge(ridge_spec(EARTH_KM, 110.0, -2600.0), 3840.0)
}

/// The limits of a realism report around the values of a reference: the
/// largest difference of the spectral slope, the smallest and the largest
/// ratio of the anisotropy and of the RMS height, and the largest sum of
/// the differences of the slope histogram.
struct Within {
    slope: f64,
    anisotropy: (f64, f64),
    rms: (f64, f64),
    histogram: f64,
}

/// Asserts that a realism report is within limits around a reference.
fn assert_within(report: &Realism, reference: &Reference, within: &Within) {
    let r = &reference.realism;
    let name = &reference.region;
    assert!(
        (report.spectral_slope - r.spectral_slope).abs() < within.slope,
        "{name}: a spectral slope of {}",
        report.spectral_slope
    );
    let ratio = report.anisotropy / r.anisotropy;
    assert!(
        ratio > within.anisotropy.0 && ratio < within.anisotropy.1,
        "{name}: an anisotropy of {}",
        report.anisotropy
    );
    let ratio = report.rms_m / r.rms_m;
    assert!(
        ratio > within.rms.0 && ratio < within.rms.1,
        "{name}: an RMS of {} m",
        report.rms_m
    );
    let apart: f64 = (report.slope_histogram.iter())
        .zip(&r.slope_histogram)
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(
        apart < within.histogram,
        "{name}: a slope histogram {apart} from the reference"
    );
}

const RIDGE_LIMITS: Within = Within {
    slope: 0.4,
    anisotropy: (0.5, 2.0),
    rms: (0.7, 1.4),
    histogram: 0.4,
};

/// The RMS of the relief shorter than about 20 km of a patch of `n` by `n`
/// heights with cells of `cell_m`, in the columns from `km.0` to `km.1`
/// kilometers east or west of the middle.
fn short_relief(heights: &[f32], n: usize, cell_m: f64, km: (f64, f64)) -> f64 {
    let box_cells = (20_000.0 / cell_m) as usize | 1;
    let half = box_cells / 2;
    let mut sums = vec![0.0; (n + 1) * (n + 1)];
    for y in 0..n {
        for x in 0..n {
            sums[(y + 1) * (n + 1) + x + 1] = f64::from(heights[y * n + x])
                + sums[y * (n + 1) + x + 1]
                + sums[(y + 1) * (n + 1) + x]
                - sums[y * (n + 1) + x];
        }
    }
    let (mut sum, mut count) = (0.0, 0);
    for y in half..n - half {
        for x in half..n - half {
            let from_middle = (x as f64 - 0.5 * n as f64).abs() * cell_m / 1000.0;
            if from_middle < km.0 || from_middle >= km.1 {
                continue;
            }
            let (x0, y0, x1, y1) = (x - half, y - half, x + half + 1, y + half + 1);
            let mean =
                (sums[y1 * (n + 1) + x1] - sums[y0 * (n + 1) + x1] - sums[y1 * (n + 1) + x0]
                    + sums[y0 * (n + 1) + x0])
                    / (box_cells * box_cells) as f64;
            let d = f64::from(heights[y * n + x]) - mean;
            sum += d * d;
            count += 1;
        }
    }
    (sum / f64::from(count)).sqrt()
}

#[test]
fn a_slow_ridge_is_near_the_mid_atlantic_ridge() {
    let reference = Reference::parse(ATLANTIC_RIDGE).unwrap();
    let painted = slow_ridge();
    let (report, _) = compare(&painted, &reference, 0.0);
    assert_within(&report, &reference, &RIDGE_LIMITS);
    // As on the real flank, the hills get lower away from the axis, so no
    // area far out stands out.
    let patch = painted.patch(&reference);
    let relief = |km| short_relief(&patch.heights, patch.size, patch.cell_m, km);
    let (near, far) = (relief((0.0, 150.0)), relief((350.0, 450.0)));
    eprintln!("  short relief: {near:.0} m near the axis, {far:.0} m far from it");
    assert!(
        far < 0.85 * near,
        "{near} m near the axis, {far} m far from it"
    );
}

#[test]
fn a_fast_ridge_is_near_the_east_pacific_rise() {
    let reference = Reference::parse(EPR_RIDGE).unwrap();
    let (report, _) = compare(&fast_ridge(), &reference, 0.0);
    assert_within(&report, &reference, &RIDGE_LIMITS);
}

/// The brush radius of the margin test, in kilometers.
const MARGIN_BRUSH_KM: f64 = 150.0;

/// The direction at a distance in radians from `center` at an azimuth in
/// radians from north.
fn offset(center: V3, azimuth: f64, distance: f64) -> V3 {
    let east = normalize(cross([0.0, 0.0, 1.0], center));
    let north = cross(center, east);
    let toward = add(scale(north, azimuth.cos()), scale(east, azimuth.sin()));
    normalize(add(
        scale(center, distance.cos()),
        scale(toward, distance.sin()),
    ))
}

/// A margin at the default settings on a rough shelf between land at 20 m
/// and a floor at 4,600 m, on a world of the size of the Earth, at the
/// scale of the margin of the eastern United States. As there, the shelf
/// break is an arc around a place to the southeast, near the middle of the
/// patch.
fn earth_margin() -> Painted {
    let mut painted = Painted::earth(4600.0);
    let spec = Margin {
        shelf_m: -130.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    let to_center = 530.0 / EARTH_KM;
    let center = offset(painted.center, 137f64.to_radians(), to_center);
    painted.margin(spec, center, 700.0, 120.0, 20.0, 900.0, MARGIN_BRUSH_KM);
    painted
}

const MARGIN_LIMITS: Within = Within {
    slope: 0.5,
    anisotropy: (0.5, 2.0),
    rms: (0.7, 1.4),
    histogram: 0.4,
};

#[test]
fn a_margin_is_near_the_margin_of_the_eastern_united_states() {
    let reference = Reference::parse(US_EAST_MARGIN).unwrap();
    let (report, _) = compare(&earth_margin(), &reference, reference.direction());
    assert_within(&report, &reference, &MARGIN_LIMITS);
}

/// The test stroke of a margin beside its reference patch, from
/// `scripts/gebco-reference.sh us-east-margin`.
#[test]
#[ignore = "manual"]
fn manual_margin_next_to_its_reference() {
    let reference = Reference::parse(US_EAST_MARGIN).unwrap();
    let gebco = gebco_patch("us-east-margin");
    save_side_by_side("us-east-margin", &earth_margin(), &reference, &gebco);
}

/// The length of the hotspot test stroke, in kilometers.
const HOTSPOT_KM: f64 = 1200.0;

/// A hotspot track at the default settings on a floor at 5,400 m, on a
/// world of the size of the Earth, at the scale of the Hawaiian Islands. The
/// stroke runs 1,200 km from the west-northwest to its end at the place of
/// Kilauea in the reference patch: 210 km east and 150 km south of the
/// middle. The erosion is at 50%.
fn earth_hotspot() -> Painted {
    let mut painted = Painted::earth(5400.0);
    let km = |d: f64| d / EARTH_KM;
    let end = offset(painted.center, std::f64::consts::FRAC_PI_2, km(210.0));
    let end = offset(end, std::f64::consts::PI, km(150.0));
    let start = offset(end, -60f64.to_radians(), km(HOTSPOT_KM));
    paint_hotspot(&mut painted, RIDGE_SEED, start, end, HOTSPOT_KM, 8);
    painted
}

/// Paints abyssal hills in `2 * reach + 1` strokes 150 km apart around the
/// middle, and then a hotspot track at the default settings from `start` to
/// `end`, `length_km` long, with the erosion at 50%.
fn paint_hotspot(painted: &mut Painted, seed: u32, start: V3, end: V3, length_km: f64, reach: i32) {
    let n = painted.map.face_size();
    let km = |d: f64| d / EARTH_KM;
    // The old Pacific floor around the islands has abyssal hills. In the
    // reference patch, their trend is near N75W.
    let hills = AbyssalHills {
        rate: 100.0,
        hills: 0.3,
        fit: true,
        scale: 1.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    let trend = -75f64.to_radians();
    let fabric: Vec<V3> = (-reach..=reach)
        .map(|i| offset(painted.center, trend, km(150.0 * f64::from(i))))
        .collect();
    paint_abyssal(&mut painted.map, hills, &fabric, 0.15);
    let spec = Hotspot {
        radius: HOTSPOT_RADIUS_KM / EARTH_KM,
        height_m: HOTSPOT_HEIGHT_M,
        speed_mm: HOTSPOT_SPEED_MM,
        reefs: false,
        age_myr: 90.0,
        world_km: EARTH_KM,
        seed,
        limits: Limits::default(),
    };
    let mut line = HotspotLine::new(spec, n);
    let count = (km(length_km) / (LINE_SPACING * spec.radius)).ceil() as usize;
    let tangent = cross(normalize(cross(start, end)), start);
    for i in 0..=count {
        let (sin, cos) = (km(length_km) * i as f64 / count as f64).sin_cos();
        line.push(add(scale(start, cos), scale(tangent, sin)), 1.0);
    }
    line.finish(&painted.map);
    let window = hotspot_window(&line, n);
    let mut erosion = Erosion::window_alone(&WindowHeights::new(&painted.map, window));
    erosion.set_branching(spec.seed, 0.5);
    // The trade wind comes from the northeast.
    erosion.set_rain(Rain {
        wind: 45.0,
        shadow: 0.6,
    });
    let step = erode_hotspot(&mut erosion, &line, 0.5);
    painted.map.begin_stroke();
    let mut from = 0;
    while from < line.len() {
        let plan = line.plan(from);
        painted.map.hotspot(&plan);
        from = plan.end;
    }
    painted.map.erode(&step);
    painted.map.end_stroke();
}

const HOTSPOT_LIMITS: Within = MARGIN_LIMITS;

#[test]
fn a_hotspot_track_is_near_the_hawaiian_islands() {
    let reference = Reference::parse(HAWAII).unwrap();
    let (report, _) = compare(&earth_hotspot(), &reference, reference.direction());
    assert_within(&report, &reference, &HOTSPOT_LIMITS);
}

/// The test stroke of a hotspot track beside its reference patch, from
/// `scripts/gebco-reference.sh hawaii`.
#[test]
#[ignore = "manual"]
fn manual_hotspot_next_to_its_reference() {
    let reference = Reference::parse(HAWAII).unwrap();
    let gebco = gebco_patch("hawaii");
    let painted = earth_hotspot();
    compare(&painted, &reference, reference.direction());
    save_side_by_side("hawaii", &painted, &reference, &gebco);
}

/// The length of the long hotspot test stroke, in kilometers.
const LONG_HOTSPOT_KM: f64 = 3000.0;

/// Long hotspot tracks at the default settings for three seeds, each in a
/// square of 3,584 km with north up, so the clusters, the long gaps, and the
/// uneven sizes along the chain show.
#[test]
#[ignore = "manual"]
fn manual_long_hotspot_tracks() {
    let km = |d: f64| d / EARTH_KM;
    let (size, cell_m) = (1024, 3500.0);
    for seed in [1, 2, 3] {
        let mut painted = Painted::earth(5400.0);
        // The chain runs from the west-northwest to its end east of the
        // middle.
        let end = offset(
            painted.center,
            120f64.to_radians(),
            km(0.5 * LONG_HOTSPOT_KM),
        );
        let start = offset(end, -60f64.to_radians(), km(LONG_HOTSPOT_KM));
        paint_hotspot(&mut painted, seed, start, end, LONG_HOTSPOT_KM, 12);
        let patch = Patch::from_heightmap(&painted.map, painted.center, EARTH_KM, size, cell_m);
        let mut image = image::RgbImage::new(size as u32, size as u32);
        draw(&mut image, &patch.heights, size, cell_m, [0, 0]);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = root.join(format!("target/realism/hotspot-long-{seed}.png"));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        image.save(&out).unwrap();
        eprintln!("wrote {}", out.display());
    }
}

/// The azimuth of the Aleutian Trench near 172 W, in degrees.
const TRENCH_AZIMUTH: f64 = 76.0;

/// The distance of the trench axis north of the middle of the patch, in
/// kilometers, as in the reference patch.
const TRENCH_NORTH_KM: f64 = 60.0;

/// The radius of the curve of the trench line, in kilometers. The arc
/// bulges toward the incoming plate, as the Aleutian arc does.
const TRENCH_CURVE_KM: f64 = 2500.0;

/// A wavy line: the sum of octaves of value noise, each with a length and
/// a height in kilometers or meters and a seed, at a distance `km` along
/// it.
fn wave(km: f64, octaves: &[(f64, f64, f64)]) -> f64 {
    let value = |i: f64, seed: f64| {
        let h = ((i * 12.9898 + seed * 78.233).sin() * 43758.5453).fract();
        2.0 * h.abs() - 1.0
    };
    octaves
        .iter()
        .map(|&(length, height, seed)| {
            let x = km / length;
            let (i, t) = (x.floor(), x - x.floor());
            let (a, b) = (value(i, seed), value(i + 1.0, seed));
            height * (a + (b - a) * t * t * (3.0 - 2.0 * t))
        })
        .sum()
}

/// Paints a noise stamp at `center` with a radius in kilometers, a height
/// in meters, and a feature size in kilometers. With a direction `down`,
/// the noise has sharp valleys along it, as gullies down a slope.
fn noise_stamp(
    map: &mut Heightmap,
    center: V3,
    radius_km: f64,
    height_m: f64,
    size_km: f64,
    down: Option<V3>,
) {
    let per_m = 65535.0 / 12000.0;
    map.begin_stroke();
    map.stamp(&Stamp {
        center,
        radius: radius_km / EARTH_KM,
        hardness: if down.is_some() { 0.5 } else { 0.7 },
        flow: 1.0,
        mode: Mode::Noise,
        level: 0,
        strength: height_m * per_m,
        lower: height_m * per_m,
        talus: 0.0,
        scale: size_km / EARTH_KM,
        seed: RIDGE_SEED + size_km as u32,
        motion: down.map_or([0.0; 3], |d| scale(d, 1e-3)),
        ridged: down.is_some(),
        invert: down.is_some(),
        relief: 0.0,
        grain: if down.is_some() { 1.5 } else { 0.0 },
        channel: vale_terrain::ChannelShape::default(),
        limits: Limits::default(),
    });
    map.end_stroke();
}

/// A trench at the scale of the Aleutian Trench near 172 W on a world of
/// the size of the Earth, as a user paints it. The incoming plate is old
/// floor under sediment at 3,900 m, with low abyssal hills, a fracture
/// zone, and a few seamounts. The overriding plate has a broad terrace, a
/// steep, uneven slope up to a platform with islands, and the slope down to
/// the sea behind the arc. The trench line is a gentle curve. The real
/// floor is 1,300 m deeper, but the heightmap ends at 6,000 m.
fn earth_trench() -> Painted {
    use vale_terrain::math::dot;
    let floor_m = 3900.0;
    let per_m = 65535.0 / 12000.0;
    let mut painted = Painted::earth(floor_m);
    let n = painted.map.face_size();
    let azimuth = TRENCH_AZIMUTH.to_radians();
    let across = azimuth - std::f64::consts::FRAC_PI_2;
    let through = offset(painted.center, across, TRENCH_NORTH_KM / EARTH_KM);
    // The trench line is a small circle around `pole`, on the side of the
    // overriding plate.
    let rho = TRENCH_CURVE_KM / EARTH_KM;
    let pole = offset(through, across, rho);
    let out = normalize(add(through, scale(pole, -dot(through, pole))));
    let ahead = normalize(cross(pole, out));
    let toward = offset(through, azimuth, 1e-3);
    let ahead = if dot(ahead, toward) < 0.0 {
        scale(ahead, -1.0)
    } else {
        ahead
    };
    let on_arc = |km: f64| {
        let theta = km / EARTH_KM / rho.sin();
        normalize(add(
            scale(pole, rho.cos()),
            add(
                scale(out, rho.sin() * theta.cos()),
                scale(ahead, rho.sin() * theta.sin()),
            ),
        ))
    };
    // The distance of a place past the axis toward the overriding plate,
    // and along the axis, in kilometers.
    let place = |p: V3| {
        let past = (rho - angle(p, pole)) * EARTH_KM;
        let flat = add(p, scale(pole, -dot(p, pole)));
        let along = dot(flat, ahead).atan2(dot(flat, out)) * rho.sin() * EARTH_KM;
        (past, along)
    };
    // Low abyssal hills under sediment. As south of the Aleutians, they
    // run near the trench.
    let line: Vec<V3> = (-20..=20)
        .map(|i| {
            offset(
                painted.center,
                80f64.to_radians(),
                f64::from(i) * 40.0 / EARTH_KM,
            )
        })
        .collect();
    let hills = AbyssalHills {
        rate: 60.0,
        hills: 0.15,
        fit: true,
        scale: 1.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    paint_abyssal(&mut painted.map, hills, &line, 0.06);
    // Broad, low swells of the sediment.
    for (height, size) in [(700.0, 80.0), (300.0, 30.0), (70.0, 7.0)] {
        let center = offset(painted.center, 0.0, -150.0 / EARTH_KM);
        noise_stamp(&mut painted.map, center, 320.0, height, size, None);
    }
    // A few seamounts on the incoming plate: a short chain and some alone,
    // as east and north of the middle of the patch in kilometers and a
    // stamp radius in kilometers.
    let spec = Seamounts {
        density: 12.0,
        height_m: 450.0,
        // The Pacific floor south of the Aleutian Trench.
        age_myr: 55.0,
        world_km: EARTH_KM,
        seed: 5,
        limits: Limits::default(),
    };
    let mut stroke = SeamountStroke::new(spec, n);
    painted.map.begin_stroke();
    for (east, north, radius) in [
        (40.0, -215.0, 40.0),
        (60.0, -170.0, 45.0),
        (75.0, -125.0, 40.0),
        (90.0, -85.0, 35.0),
        (-150.0, -60.0, 70.0),
        (170.0, -40.0, 50.0),
        (-80.0, -180.0, 60.0),
    ] {
        let east = offset(painted.center, std::f64::consts::FRAC_PI_2, east / EARTH_KM);
        let p = offset(east, 0.0, north / EARTH_KM);
        let plan = stroke.stamp(&painted.map, p, radius / EARTH_KM, 0.6, 1.0);
        painted.map.seamounts(&plan);
    }
    painted.map.end_stroke();
    // Small knolls spread thinly over the incoming plate.
    let spec = Seamounts {
        density: 3.0,
        height_m: 250.0,
        seed: 6,
        ..spec
    };
    let mut stroke = SeamountStroke::new(spec, n);
    painted.map.begin_stroke();
    let center = offset(painted.center, 0.0, -130.0 / EARTH_KM);
    let plan = stroke.stamp(&painted.map, center, 220.0 / EARTH_KM, 0.8, 1.0);
    painted.map.seamounts(&plan);
    painted.map.end_stroke();
    // The overriding plate has no abyssal hills: a terrace near 1,400 m
    // above the floor from about 50 km past the axis, a steep slope from an
    // uneven break near 92 km up to a platform near sea level, and a slope
    // down to 2,800 m behind the arc past 185 km.
    let half = (450.0 / EARTH_KM / (std::f64::consts::FRAC_PI_2 / n as f64)) as usize;
    let step = |km: f64, from: f64, to: f64| {
        let t = ((km - from) / (to - from)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let floor = f64::from(meters_to_level(-floor_m));
    let east = normalize(cross([0.0, 0.0, 1.0], painted.center));
    let north = cross(painted.center, east);
    let bump = |q: f64| (1.0 - q * q).max(0.0).powi(2);
    for y in n / 2 - half..n / 2 + half {
        for x in n / 2 - half..n / 2 + half {
            let (past, along) = place(painted.map.texel_dir(0, x, y));
            let old = f64::from(painted.map.get(0, x as i64, y as i64));
            let plate = step(past, -10.0, 20.0);
            let brk = 92.0
                + wave(
                    along,
                    &[
                        (150.0, 14.0, 1.0),
                        (60.0, 8.0, 2.0),
                        (25.0, 5.0, 0.3),
                        (10.0, 2.5, 6.1),
                        (4.0, 1.2, 7.7),
                    ],
                );
            let top = 4150.0
                + wave(
                    along,
                    &[(90.0, 120.0, 0.7), (31.0, 80.0, 2.9), (13.0, 60.0, 1.3)],
                );
            let back = 185.0
                + wave(
                    along,
                    &[(110.0, 10.0, 0.4), (37.0, 4.0, 1.9), (17.0, 2.0, 2.4)],
                );
            let foot = 8.0
                + wave(
                    along,
                    &[(70.0, 5.0, 2.2), (23.0, 3.0, 0.1), (41.0, 2.0, 1.7)],
                );
            let terrace = 1400.0
                + wave(
                    along,
                    &[(80.0, 350.0, 3.3), (35.0, 200.0, 4.1), (15.0, 80.0, 5.2)],
                );
            let lift = terrace * step(past, foot, foot + 40.0)
                + (top - terrace) * step(past, brk, brk + 15.0)
                - (top - 1100.0) * step(past, back, back + 22.0);
            // A fracture zone runs north to the trench 80 km west of the
            // middle, as the Amlia Fracture Zone does: a ridge that grows
            // toward the trench, a trough on its east side, and older,
            // deeper floor east of it.
            let d = painted.map.texel_dir(0, x, y);
            let north_km = dot(d, north).asin() * EARTH_KM;
            let west_km = -dot(d, east).asin() * EARTH_KM - 80.0
                + wave(north_km, &[(70.0, 2.0, 8.8), (25.0, 0.8, 9.1)]);
            let ridge = (150.0
                + 450.0 * step(north_km, -130.0, -20.0)
                + wave(north_km, &[(50.0, 250.0, 3.7), (17.0, 120.0, 4.4)]))
            .max(0.0);
            let fracture = ridge * bump(west_km / 6.0)
                - 300.0 * bump((west_km + 16.0) / 13.0)
                - 200.0 * step(-west_km, 0.0, 30.0);
            let old = old + fracture * per_m;
            let level = old + plate * (floor - old) + lift * per_m;
            painted
                .map
                .set(0, x, y, level.round().clamp(0.0, 65535.0) as u16);
        }
    }
    // Gullies and canyons on the slope up to the shelf, as a noise brush
    // with sharp valleys down the slope paints them, and islands and passes
    // on the shelf.
    for i in -8..=8 {
        let on_axis = on_arc(f64::from(i) * 40.0);
        let center = offset(on_axis, across, 105.0 / EARTH_KM);
        let down = normalize(add(pole, scale(center, -dot(center, pole))));
        noise_stamp(&mut painted.map, center, 45.0, 250.0, 5.0, Some(down));
        let center = offset(on_axis, across, 160.0 / EARTH_KM);
        noise_stamp(&mut painted.map, center, 50.0, 500.0, 18.0, None);
        let center = offset(on_axis, across, 70.0 / EARTH_KM);
        noise_stamp(&mut painted.map, center, 50.0, 400.0, 25.0, None);
    }
    let spec = Trench {
        radius: 350.0 / EARTH_KM,
        depth_m: 1900.0,
        age_myr: 55.0,
        side: TrenchSide::Left,
        margin: TrenchMargin::Accretionary,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    let mut trench = TrenchLine::new(spec, n);
    // The points of the app, a quarter of the brush radius apart.
    for i in -30..=30 {
        trench.push(on_arc(f64::from(i) * 0.25 * spec.radius * EARTH_KM), 1.0);
    }
    painted.map.begin_stroke();
    let mut from = 0;
    while from < trench.len() {
        let plan = trench.plan(from);
        painted.map.trench(&plan);
        from = plan.end;
    }
    painted.map.end_stroke();
    painted
}

const TRENCH_LIMITS: Within = Within {
    slope: 0.5,
    anisotropy: (0.8, 1.6),
    rms: (0.78, 1.25),
    histogram: 0.2,
};

#[test]
fn a_trench_is_near_the_aleutian_trench() {
    let reference = Reference::parse(ALEUTIAN_TRENCH).unwrap();
    let (report, _) = compare(&earth_trench(), &reference, reference.direction());
    assert_within(&report, &reference, &TRENCH_LIMITS);
}

/// The test stroke of a trench beside its reference patch, from
/// `scripts/gebco-reference.sh aleutian-trench`.
#[test]
#[ignore = "manual"]
fn manual_trench_next_to_its_reference() {
    let reference = Reference::parse(ALEUTIAN_TRENCH).unwrap();
    let gebco = gebco_patch("aleutian-trench");
    let painted = earth_trench();
    compare(&painted, &reference, reference.direction());
    save_side_by_side("aleutian-trench", &painted, &reference, &gebco);
}

/// The patch of a reference region from `scripts/gebco-reference.sh`.
fn gebco_patch(name: &str) -> Vec<f32> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = root.join(format!("target/gebco/{name}.f32"));
    let bytes =
        std::fs::read(&raw).unwrap_or_else(|_| panic!("run scripts/gebco-reference.sh {name}"));
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect()
}

/// Saves a painted patch and the reference patch side by side, with the
/// same tints and hillshade, to `target/realism/NAME.png`.
fn save_side_by_side(name: &str, painted: &Painted, reference: &Reference, gebco: &[f32]) {
    let ours = painted.patch(reference).heights;
    let n = reference.size;
    let mut image = image::RgbImage::new(2 * n as u32, n as u32);
    for (i, heights) in [&ours[..], gebco].into_iter().enumerate() {
        draw(&mut image, heights, n, reference.cell_m, [i * n, 0]);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join(format!("target/realism/{name}.png"));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
    // The painted heights, as the reference patch is saved.
    let bytes: Vec<u8> = ours.iter().flat_map(|h| h.to_le_bytes()).collect();
    std::fs::write(out.with_extension("f32"), bytes).unwrap();
}

/// The test strokes of a slow ridge and a fast ridge beside their reference
/// patches. The reference patches come from `scripts/gebco-reference.sh
/// atlantic-ridge` and `scripts/gebco-reference.sh epr-ridge`.
#[test]
#[ignore = "manual"]
fn manual_ridges_next_to_their_references() {
    for (name, text, paint) in [
        (
            "atlantic-ridge",
            ATLANTIC_RIDGE,
            slow_ridge as fn() -> Painted,
        ),
        ("epr-ridge", EPR_RIDGE, fast_ridge),
    ] {
        let reference = Reference::parse(text).unwrap();
        let gebco = gebco_patch(name);
        let painted = paint();
        compare(&painted, &reference, 0.0);
        save_side_by_side(name, &painted, &reference, &gebco);
    }
}

/// The painted patch and the reference patch side by side, with the same
/// tints and hillshade. The reference patch comes from
/// `scripts/gebco-reference.sh pacific-flank`.
#[test]
#[ignore = "manual"]
fn manual_seamounts_next_to_the_pacific_flank() {
    let reference = Reference::parse(PACIFIC_FLANK).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = root.join("target/gebco/pacific-flank.f32");
    let bytes = std::fs::read(&raw).expect("run scripts/gebco-reference.sh pacific-flank");
    let gebco: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect();
    print_relief("GEBCO", &relief(&gebco, reference.size));
    let mut faults = Painted::new(BASE_M);
    faults.ridge_flank();
    print_relief(
        "faults",
        &relief(&faults.patch(&reference).heights, reference.size),
    );
    let painted = pacific_flank();
    compare(&painted, &reference, 0.0);
    let ours = painted.patch(&reference).heights;
    let n = reference.size;
    let mut image = image::RgbImage::new(2 * n as u32, n as u32);
    for (i, heights) in [&ours, &gebco].into_iter().enumerate() {
        draw(&mut image, heights, n, reference.cell_m, [i * n, 0]);
    }
    let out = root.join("target/realism/pacific-flank.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
    // The abyssal hills brush beside the same reference.
    let painted = abyssal_flank();
    compare(&painted, &reference, 0.0);
    let ours = painted.patch(&reference).heights;
    for (i, heights) in [&ours, &gebco].into_iter().enumerate() {
        draw(&mut image, heights, n, reference.cell_m, [i * n, 0]);
    }
    let out = root.join("target/realism/abyssal-flank.png");
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
}

/// Draws a patch with row 0 at the south into `image` from column `x0` and
/// row `y0`, with tints by depth and a hillshade from the northwest. The
/// tints go from 4,500 m to 1,500 m, and from 5,200 m to 0 m on a patch
/// with a shelf.
fn draw(image: &mut image::RgbImage, heights: &[f32], n: usize, cell_m: f64, [x0, y0]: [usize; 2]) {
    let at = |x: usize, y: usize| f64::from(heights[y.min(n - 1) * n + x.min(n - 1)]);
    let shelf = heights.iter().any(|&h| h > -500.0);
    let (deep, span) = if shelf {
        (5200.0, 5200.0)
    } else {
        (4500.0, 3000.0)
    };
    for y in 0..n {
        for x in 0..n {
            let z = at(x, y);
            let dx = (at(x + 1, y) - at(x.saturating_sub(1), y)) / (2.0 * cell_m);
            let dy = (at(x, y + 1) - at(x, y.saturating_sub(1))) / (2.0 * cell_m);
            // A light from the northwest, 45 degrees up.
            let shade = ((dx - dy) * 0.707 + 1.0) / (1.0 + dx * dx + dy * dy).sqrt() * 0.707;
            let t = ((z + deep) / span).clamp(0.0, 1.0);
            let tint = [30.0 + 140.0 * t, 60.0 + 140.0 * t, 130.0 + 100.0 * t];
            let k = (0.35 + 0.9 * shade).clamp(0.0, 1.3);
            let pixel = tint.map(|c| (c * k).clamp(0.0, 255.0) as u8);
            image.put_pixel((x0 + x) as u32, (y0 + n - 1 - y) as u32, image::Rgb(pixel));
        }
    }
}

/// The radius of an Earth, in kilometers.
const EARTH_KM: f64 = 6371.0;

/// The side of the patch of the coarse maps, in kilometers.
const COARSE_PATCH_KM: f64 = 312.0;

/// The flank of an ocean ridge on an Earth with faces of `n` texels, with
/// the hills fit to the map or at their real size. The axis is a meridian
/// east of the patch, as in `ridge_flank`.
fn coarse_ridge(n: usize, fit: bool) -> Heightmap {
    let mut map = Heightmap::new(n, meters_to_level(-BASE_M));
    let spec = Ridge {
        radius: 0.06,
        crest: meters_to_level(-2600.0),
        segment: 400.0 / EARTH_KM,
        offset: 0.0,
        spread: 55.0 / EARTH_KM,
        magma: 0.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
        hills: 1.0,
        fit,
        scale: 1.0,
    };
    let mut line = RidgeLine::new(spec, n);
    let lon = 200.0 / EARTH_KM * 180.0 / std::f64::consts::PI;
    for i in 0..=16 {
        line.push(lonlat_to_dir(lon, -10.0 + 20.0 * f64::from(i) / 16.0), 1.0);
    }
    map.begin_stroke();
    let mut from = 0;
    while from < line.len() {
        let plan = line.plan(from);
        map.ridge(&plan);
        from = plan.end;
    }
    map.end_stroke();
    map
}

/// An abyssal hills stroke north over the patch on flat floor, on an Earth
/// with faces of `n` texels.
fn coarse_abyssal(n: usize, fit: bool) -> Heightmap {
    let mut map = Heightmap::new(n, meters_to_level(-REFERENCE_DEPTH_M));
    let spec = AbyssalHills {
        rate: 110.0,
        hills: 1.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
        fit,
        scale: 1.0,
    };
    paint_abyssal(&mut map, spec, &meridian(6.0), 0.04);
    map
}

/// The patch of a coarse map, with `size` cells.
fn coarse_patch(map: &Heightmap, size: usize) -> Patch {
    let cell_m = COARSE_PATCH_KM * 1000.0 / size as f64;
    Patch::from_heightmap(map, lonlat_to_dir(0.0, 0.0), EARTH_KM, size, cell_m)
}

/// The anisotropy and the part of the slopes above 2 degrees of the patch
/// of a coarse map, sampled at the texel of the map.
fn coarse_fabric(map: &Heightmap) -> (f64, f64) {
    let texel_km = EARTH_KM * std::f64::consts::FRAC_PI_2 / map.face_size() as f64;
    let size = ((COARSE_PATCH_KM / texel_km) as usize).next_power_of_two() / 2;
    let patch = coarse_patch(map, size);
    let report = realism(&patch, (4.0 * patch.cell_m, 150_000.0), 0.0);
    let steep: f64 = report.slope_histogram[2..].iter().sum();
    (report.anisotropy, steep)
}

/// On an Earth with faces of 4,096 texels, a texel is 2.4 km, wider than a
/// scarp. With the fit, the fabric of the hills shows at the texel of the
/// map, as clearly as at the real scale. At the real size, it is faint.
#[test]
fn fit_hills_show_on_a_coarse_map() {
    for (name, paint) in [
        ("ridge", coarse_ridge as fn(usize, bool) -> Heightmap),
        ("abyssal", coarse_abyssal),
    ] {
        let (on, off) = (
            coarse_fabric(&paint(4096, true)),
            coarse_fabric(&paint(4096, false)),
        );
        eprintln!(
            "{name} at 4096: fit anisotropy {:.2}, slopes above 2 degrees {:.3}; \
             real size {:.2}, {:.3}",
            on.0, on.1, off.0, off.1
        );
        assert!(on.0 > 2.0, "{name}: an anisotropy of {}", on.0);
        assert!(
            on.1 > 0.05,
            "{name}: {} of the slopes above 2 degrees",
            on.1
        );
        assert!(
            off.1 < 0.5 * on.1,
            "{name}: {} of the slopes above 2 degrees",
            off.1
        );
    }
}

/// The patches of the coarse maps with the fit off and on, at 4,096 and
/// 8,192 texels on a face: the ridge flank, then the abyssal hills.
#[test]
#[ignore = "manual"]
fn manual_fit_on_coarse_maps() {
    let size = 256;
    let mut image = image::RgbImage::new(4 * size as u32, 2 * size as u32);
    let cell_m = COARSE_PATCH_KM * 1000.0 / size as f64;
    for (row, n) in [4096, 8192].into_iter().enumerate() {
        let maps = [
            coarse_ridge(n, false),
            coarse_ridge(n, true),
            coarse_abyssal(n, false),
            coarse_abyssal(n, true),
        ];
        for (column, map) in maps.iter().enumerate() {
            let (anisotropy, steep) = coarse_fabric(map);
            eprintln!("{n} column {column}: anisotropy {anisotropy:.2}, steep {steep:.3}");
            let heights = coarse_patch(map, size).heights;
            draw(
                &mut image,
                &heights,
                size,
                cell_m,
                [column * size, row * size],
            );
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/realism/fit.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
}

/// The abyssal hills at the scale of the reference patch, at 1, 2, 4, and
/// 8 times their real size, side by side.
#[test]
#[ignore = "manual"]
fn manual_abyssal_hills_at_each_scale() {
    let reference = Reference::parse(PACIFIC_FLANK).unwrap();
    let n = reference.size;
    let scales = [1.0, 2.0, 4.0, 8.0];
    let mut image = image::RgbImage::new((scales.len() * n) as u32, n as u32);
    for (i, scale) in scales.into_iter().enumerate() {
        let mut painted = Painted::new(REFERENCE_DEPTH_M);
        let spec = AbyssalHills {
            rate: 110.0,
            hills: 1.0,
            world_km: WORLD_KM,
            seed: RIDGE_SEED,
            limits: Limits::default(),
            fit: true,
            scale,
        };
        paint_abyssal(&mut painted.map, spec, &meridian(24.0), 0.18);
        let heights = painted.patch(&reference).heights;
        draw(&mut image, &heights, n, reference.cell_m, [i * n, 0]);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/realism/scale.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
}

/// An S-shaped stroke and a tight arc of abyssal hills on flat floor, at
/// the scale of the reference patch and on an Earth with faces of 8,192
/// texels with the fit to the map, so the faults show how they bend.
#[test]
#[ignore = "manual"]
fn manual_abyssal_hills_on_curved_strokes() {
    let size = 512;
    let mut image = image::RgbImage::new(2 * size as u32, 2 * size as u32);
    // The world radius, the face size, and the side of the patch in km.
    let maps = [(WORLD_KM, FACE_SIZE, 235.0), (EARTH_KM, 8192, 625.0)];
    for (row, (km, n, side)) in maps.into_iter().enumerate() {
        // A place `(x, y)` in parts of the side of the patch, as a unit
        // direction.
        let at = |x: f64, y: f64| {
            let k = side / km * 180.0 / std::f64::consts::PI;
            lonlat_to_dir(x * k, y * k)
        };
        let s_curve: Vec<V3> = (0..=48)
            .map(|i| {
                let y = -0.6 + 1.2 * f64::from(i) / 48.0;
                at(0.18 * (std::f64::consts::TAU * y / 0.8).sin(), y)
            })
            .collect();
        let arc: Vec<V3> = (0..=54)
            .map(|i| {
                let t = (-135.0 + 270.0 * f64::from(i) / 54.0_f64).to_radians();
                at(0.22 * t.sin(), 0.22 * t.cos() - 0.05)
            })
            .collect();
        for (column, line) in [s_curve, arc].iter().enumerate() {
            let mut map = Heightmap::new(n, meters_to_level(-REFERENCE_DEPTH_M));
            let spec = AbyssalHills {
                rate: 110.0,
                hills: 1.0,
                world_km: km,
                seed: RIDGE_SEED,
                limits: Limits::default(),
                fit: true,
                scale: 1.0,
            };
            paint_abyssal(&mut map, spec, line, 0.1 * side / km);
            let cell_m = side * 1000.0 / size as f64;
            let patch = Patch::from_heightmap(&map, lonlat_to_dir(0.0, 0.0), km, size, cell_m);
            draw(
                &mut image,
                &patch.heights,
                size,
                cell_m,
                [column * size, row * size],
            );
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/realism/abyssal-curve.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
}

/// A margin on a rough shelf with bays and spurs, painted with a stroke
/// along the shelf, in `target/realism/ragged-coast.png`.
#[test]
#[ignore = "manual"]
fn manual_margin_on_a_ragged_coast() {
    let n = 2048;
    let mut map = Heightmap::new(n, meters_to_level(-4600.0));
    let center = lonlat_to_dir(0.0, 0.0);
    let half = 800;
    for y in n / 2 - half..n / 2 + half {
        for x in n / 2 - half..n / 2 + half {
            let d = map.texel_dir(0, x, y);
            let (lon, lat) = (d[1].atan2(d[0]).to_degrees(), d[2].asin().to_degrees());
            // A coast along a meridian, and a shelf with bays and spurs of
            // several sizes.
            let edge =
                1.2 * (lat * 0.9).sin() + 0.6 * (lat * 2.3 + 1.0).sin() + 0.3 * (lat * 5.1).sin();
            if lon < -3.5 {
                map.set(0, x, y, meters_to_level(200.0));
            } else if lon < edge - 1.0 {
                map.set(0, x, y, meters_to_level(-150.0));
            }
        }
    }
    let spec = Margin {
        shelf_m: -130.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    let mut stroke = MarginStroke::new(spec, n);
    map.begin_stroke();
    for row in 0..=14 {
        for i in 0..=60 {
            let p = lonlat_to_dir(-3.5 + 0.5 * f64::from(row), -6.0 + 0.2 * f64::from(i));
            let plan = stroke.stamp(&map, p, 0.012, 0.6, 1.0);
            map.margin(&plan);
        }
    }
    map.end_stroke();
    let size = 512;
    let patch = Patch::from_heightmap(&map, center, EARTH_KM, size, 2000.0);
    let mut image = image::RgbImage::new(size as u32, size as u32);
    draw(&mut image, &patch.heights, size, 2000.0, [0, 0]);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/realism/ragged-coast.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
}

/// A close view of the slope of a margin on a face of 16,384 texels, with
/// texels near 610 m, in `target/realism/margin-close.f32`: 512 by 512
/// heights in cells of 600 m, row 0 at the south.
#[test]
#[ignore = "manual"]
fn manual_margin_close_up() {
    let n = 16384;
    let mut map = Heightmap::new(n, meters_to_level(-4600.0));
    let half = 600;
    for y in n / 2 - half..n / 2 + half {
        for x in n / 2 - half..n / 2 + half {
            let d = map.texel_dir(0, x, y);
            let (lon, lat) = (d[1].atan2(d[0]).to_degrees(), d[2].asin().to_degrees());
            let coast = -1.0 + 0.3 * (lat * 1.7).sin();
            if lon < coast {
                map.set(0, x, y, meters_to_level(50.0));
            } else if lon < coast + 1.0 {
                map.set(0, x, y, meters_to_level(-150.0));
            }
        }
    }
    let spec = Margin {
        shelf_m: -130.0,
        world_km: EARTH_KM,
        seed: RIDGE_SEED,
        limits: Limits::default(),
    };
    let mut stroke = MarginStroke::new(spec, n);
    let mut plans = Vec::new();
    for row in 0..=16 {
        for i in 0..=40 {
            let p = lonlat_to_dir(-1.4 + 0.2 * f64::from(row), -2.0 + 0.1 * f64::from(i));
            plans.push(stroke.stamp(&map, p, 0.006, 0.7, 1.0));
        }
    }
    let window = margin_window(&plans, n);
    let mut erosion = Erosion::window_alone(&WindowHeights::new(&map, window));
    erosion.set_branching(RIDGE_SEED, 0.7);
    let started = std::time::Instant::now();
    let step = erode_margin(&mut erosion, &plans, 0.5);
    eprintln!(
        "erosion of {} cells of {} texels: {:?}",
        window.cells,
        window.cell,
        started.elapsed()
    );
    map.begin_stroke();
    for plan in &plans {
        map.margin(plan);
    }
    map.erode(&step);
    map.end_stroke();
    let patch = Patch::from_heightmap(&map, lonlat_to_dir(0.6, 0.0), EARTH_KM, 512, 600.0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/realism/margin-close.f32");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let bytes: Vec<u8> = patch.heights.iter().flat_map(|h| h.to_le_bytes()).collect();
    std::fs::write(&out, bytes).unwrap();
    let mut image = image::RgbImage::new(512, 512);
    draw(&mut image, &patch.heights, 512, 600.0, [0, 0]);
    image.save(out.with_extension("png")).unwrap();
}
