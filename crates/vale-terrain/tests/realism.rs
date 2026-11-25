//! Sea modes painted at the scale of a GEBCO reference region, measured by
//! the realism report.

use std::path::Path;

use vale_terrain::math::{V3, lonlat_to_dir};
use vale_terrain::{
    Heightmap, Limits, PACIFIC_FLANK, Patch, Realism, Reference, Ridge, RidgeLine, SeamountStroke,
    Seamounts, meters_to_level, realism,
};

/// The radius of the test world. A texel of the heightmap is near a GEBCO
/// cell of 460 m.
const WORLD_KM: f64 = 1000.0;
const FACE_SIZE: usize = 3328;

/// The distance of the ridge axis east of the middle of the patch, in
/// kilometers, and its longitude in degrees.
const AXIS_KM: f64 = 160.0;
const AXIS_LON: f64 = AXIS_KM / WORLD_KM * 180.0 / std::f64::consts::PI;

/// The seed of the ridge.
const RIDGE_SEED: u32 = 7;

/// The depth of the ground past the ridge, in meters.
const BASE_M: f64 = 3550.0;

/// A heightmap at the scale of a reference region and the place of its
/// patch: the middle of face 0.
struct Painted {
    map: Heightmap,
    center: V3,
}

impl Painted {
    /// Flat ground at a depth in meters.
    fn new(depth_m: f64) -> Painted {
        Painted {
            map: Heightmap::new(FACE_SIZE, meters_to_level(-depth_m)),
            center: lonlat_to_dir(0.0, 0.0),
        }
    }

    /// The patch of the reference, at the place of the patch.
    fn patch(&self, reference: &Reference) -> Patch {
        Patch::from_heightmap(
            &self.map,
            self.center,
            WORLD_KM,
            reference.size,
            reference.cell_m,
        )
    }

    /// An ocean ridge along a meridian east of the patch, so the patch is
    /// on its west flank and the fabric runs north. The line is long, so the
    /// patch is far from the taper at its ends. The crest depth and the
    /// spreading rate are those of the East Pacific Rise near 11.6 N. The
    /// axis is `AXIS_KM` east of the east edge of the patch, so the patch
    /// has the depths of the reference patch.
    fn ridge_flank(&mut self) {
        let spec = Ridge {
            radius: 0.24,
            crest: meters_to_level(-2600.0),
            segment: 400.0 / WORLD_KM,
            offset: 0.0,
            spread: 55.0 / WORLD_KM,
            magma: 0.0,
            world_km: WORLD_KM,
            seed: RIDGE_SEED,
            limits: Limits::default(),
            hills: 1.0,
        };
        let mut line = RidgeLine::new(spec, FACE_SIZE);
        for i in 0..=16 {
            let lat = -30.0 + 60.0 * f64::from(i) / 16.0;
            line.push(lonlat_to_dir(AXIS_LON, lat), 1.0);
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

/// The realism report of a painted patch, with the fabric along `direction`,
/// next to the values of the reference region.
fn compare(painted: &Painted, reference: &Reference, direction: f64) -> Realism {
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
    report
}

fn flank_spec() -> Seamounts {
    Seamounts {
        density: 15.0,
        height_m: 250.0,
        world_km: WORLD_KM,
        seed: 5,
        limits: Limits::default(),
    }
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
    let report = compare(&pacific_flank(), &reference, 0.0);
    let r = &reference.realism;
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
    let painted = pacific_flank();
    compare(&painted, &reference, 0.0);
    let ours = painted.patch(&reference).heights;
    let n = reference.size;
    let mut image = image::RgbImage::new(2 * n as u32, n as u32);
    for (i, heights) in [&ours, &gebco].into_iter().enumerate() {
        draw(&mut image, heights, n, reference.cell_m, i * n);
    }
    let out = root.join("target/realism/pacific-flank.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image.save(&out).unwrap();
    eprintln!("wrote {}", out.display());
}

/// Draws a patch with row 0 at the south into `image` from column `x0`,
/// with tints by depth and a hillshade from the northwest.
fn draw(image: &mut image::RgbImage, heights: &[f32], n: usize, cell_m: f64, x0: usize) {
    let at = |x: usize, y: usize| f64::from(heights[y.min(n - 1) * n + x.min(n - 1)]);
    for y in 0..n {
        for x in 0..n {
            let z = at(x, y);
            let dx = (at(x + 1, y) - at(x.saturating_sub(1), y)) / (2.0 * cell_m);
            let dy = (at(x, y + 1) - at(x, y.saturating_sub(1))) / (2.0 * cell_m);
            // A light from the northwest, 45 degrees up.
            let shade = ((dx - dy) * 0.707 + 1.0) / (1.0 + dx * dx + dy * dy).sqrt() * 0.707;
            let t = ((z + 4500.0) / 3000.0).clamp(0.0, 1.0);
            let tint = [30.0 + 140.0 * t, 60.0 + 140.0 * t, 130.0 + 100.0 * t];
            let k = (0.35 + 0.9 * shade).clamp(0.0, 1.3);
            let pixel = tint.map(|c| (c * k).clamp(0.0, 255.0) as u8);
            image.put_pixel((x0 + x) as u32, (n - 1 - y) as u32, image::Rgb(pixel));
        }
    }
}
