//! The realism report: statistics of a square patch of heights that real
//! terrain has in known ranges. A test compares the report of a painted
//! patch with the report of a real region, for example a GEBCO patch.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_8, PI};

use crate::cube::{ELEV_MAX, ELEV_MIN};
use crate::heightmap::Heightmap;
use crate::math::{V3, add, cross, length, normalize, scale};

/// The upper edges of the bins of the slope histogram, in degrees. The
/// first bin starts at 0.
pub const SLOPE_BINS_DEG: [f64; 7] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 90.0];

/// The shortest wavelength of the report, in cells.
pub const MIN_WAVELENGTH_CELLS: f64 = 4.0;

/// The half width of the wedges of wavenumber direction of the anisotropy.
pub const WEDGE_HALF_WIDTH: f64 = FRAC_PI_8;

/// The reference text of a flank of the East Pacific Rise with seamounts,
/// from GEBCO.
pub const PACIFIC_FLANK: &str = include_str!("../data/gebco/pacific-flank.txt");

/// The reference text of the slow Mid-Atlantic Ridge and its flanks, from
/// GEBCO.
pub const ATLANTIC_RIDGE: &str = include_str!("../data/gebco/atlantic-ridge.txt");

/// The reference text of the fast East Pacific Rise and its flanks, from
/// GEBCO.
pub const EPR_RIDGE: &str = include_str!("../data/gebco/epr-ridge.txt");

/// The reference text of the passive margin of the eastern United States,
/// with its shelf, slope, and rise, from GEBCO.
pub const US_EAST_MARGIN: &str = include_str!("../data/gebco/us-east-margin.txt");

/// The reference text of the Hawaiian Islands, from Hawaii to Kauai, with
/// their moat, from GEBCO.
pub const HAWAII: &str = include_str!("../data/gebco/hawaii.txt");

/// A square grid of heights in meters on a plane. Row 0 is the south edge,
/// and each row runs from west to east. `size` is a power of two.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub size: usize,
    pub cell_m: f64,
    pub heights: Vec<f32>,
}

impl Patch {
    /// A patch on the plane that touches a sphere of radius `radius_km` at
    /// `center`, a unit vector. The x axis points east and the y axis points
    /// north. At an exact pole, the x axis is the direction of +Y. `height`
    /// gives the height in meters at a unit vector.
    pub fn from_fn(
        center: V3,
        radius_km: f64,
        size: usize,
        cell_m: f64,
        mut height: impl FnMut(V3) -> f64,
    ) -> Patch {
        assert!(size.is_power_of_two(), "the patch size is a power of two");
        let center = normalize(center);
        let east = cross([0.0, 0.0, 1.0], center);
        let east = if length(east) < 1e-12 {
            [0.0, 1.0, 0.0]
        } else {
            normalize(east)
        };
        let north = cross(center, east);
        let radius_m = radius_km * 1000.0;
        let half = (size as f64 - 1.0) * 0.5;
        let mut heights = Vec::with_capacity(size * size);
        for j in 0..size {
            let y = (j as f64 - half) * cell_m / radius_m;
            for i in 0..size {
                let x = (i as f64 - half) * cell_m / radius_m;
                let d = normalize(add(center, add(scale(east, x), scale(north, y))));
                heights.push(height(d) as f32);
            }
        }
        Patch {
            size,
            cell_m,
            heights,
        }
    }

    /// A patch of a heightmap, with bilinear reads. See `from_fn`.
    pub fn from_heightmap(
        map: &Heightmap,
        center: V3,
        radius_km: f64,
        size: usize,
        cell_m: f64,
    ) -> Patch {
        Patch::from_fn(center, radius_km, size, cell_m, |d| {
            ELEV_MIN + map.level_between(d, true) / 65535.0 * (ELEV_MAX - ELEV_MIN)
        })
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        f64::from(self.heights[j * self.size + i])
    }
}

/// The realism report of a patch.
#[derive(Clone, Debug, PartialEq)]
pub struct Realism {
    /// The slope of the line through log power against log wavenumber of
    /// the radially averaged power spectrum, in the band. Rough relief has a
    /// slope near -2, and smooth relief has a steeper slope.
    pub spectral_slope: f64,
    /// The power with the wavenumber across the direction, over the power
    /// with the wavenumber along it, in wedges of `WEDGE_HALF_WIDTH`. The
    /// ratio is the geometric mean of the ratios of the bins of the band, so
    /// each octave has the same weight. Relief that is elongated
    /// along the direction, such as abyssal hills along the fabric, gives a
    /// ratio above 1.
    pub anisotropy: f64,
    /// The RMS height after the removal of the best-fit plane, in meters.
    pub rms_m: f64,
    /// The fraction of the inner cells with a slope angle in each bin of
    /// `SLOPE_BINS_DEG`.
    pub slope_histogram: [f64; 7],
}

/// The realism report of a patch. `band_m` is the shortest and the longest
/// wavelength of the spectrum, in meters. The shortest wavelength is
/// `MIN_WAVELENGTH_CELLS` cells or more. `direction` is an azimuth in
/// radians, clockwise from the y axis (north), so that the x axis (east)
/// is at π/2.
pub fn realism(patch: &Patch, band_m: (f64, f64), direction: f64) -> Realism {
    let n = patch.size;
    assert!(
        n.is_power_of_two() && n >= 8,
        "the patch size is a power of two"
    );
    assert_eq!(patch.heights.len(), n * n);
    let residual = remove_plane(patch);
    let rms_m = (residual.iter().map(|z| z * z).sum::<f64>() / (n * n) as f64).sqrt();

    // The Hann window and the 2D FFT.
    let hann: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())
        .collect();
    let mut grid: Vec<[f64; 2]> = (0..n * n)
        .map(|k| [residual[k] * hann[k % n] * hann[k / n], 0.0])
        .collect();
    fft_2d(&mut grid, n, false);

    let extent = n as f64 * patch.cell_m;
    let k_max = 1.0 / band_m.0.max(MIN_WAVELENGTH_CELLS * patch.cell_m);
    let k_min = 1.0 / band_m.1;
    // Bins of 1/8 octave of wavenumber.
    const BINS_PER_OCTAVE: f64 = 8.0;
    let bin_of = |k: f64| ((k / k_min).log2() * BINS_PER_OCTAVE).floor() as usize;
    let bins = bin_of(k_max) + 1;
    let mut sums = vec![(0.0, 0.0, 0usize); bins];
    // The power along and across the direction in each bin.
    let mut wedges = vec![(0.0, 0.0); bins];
    let signed = |i: usize| {
        if i < n / 2 {
            i as f64
        } else {
            i as f64 - n as f64
        }
    };
    for row in 0..n {
        let ky = signed(row) / extent;
        for column in 0..n {
            let kx = signed(column) / extent;
            let k = kx.hypot(ky);
            if k < k_min || k > k_max {
                continue;
            }
            let [re, im] = grid[row * n + column];
            let power = re * re + im * im;
            let bin = bin_of(k);
            let sum = &mut sums[bin];
            sum.0 += k;
            sum.1 += power;
            sum.2 += 1;
            // The angle between the wavenumber and the direction, in 0..π/2.
            let off = (kx.atan2(ky) - direction).rem_euclid(PI);
            let off = off.min(PI - off);
            if off <= WEDGE_HALF_WIDTH {
                wedges[bin].0 += power;
            } else if off >= FRAC_PI_2 - WEDGE_HALF_WIDTH {
                wedges[bin].1 += power;
            }
        }
    }
    let points: Vec<(f64, f64)> = sums
        .iter()
        .filter(|s| s.2 > 0 && s.1 > 0.0)
        .map(|s| ((s.0 / s.2 as f64).ln(), (s.1 / s.2 as f64).ln()))
        .collect();
    Realism {
        spectral_slope: line_slope(&points),
        anisotropy: geometric_mean_ratio(&wedges),
        rms_m,
        slope_histogram: slope_histogram(patch),
    }
}

/// The geometric mean of `across / along` of the bins that have both.
fn geometric_mean_ratio(wedges: &[(f64, f64)]) -> f64 {
    let logs: Vec<f64> = wedges
        .iter()
        .filter(|w| w.0 > 0.0 && w.1 > 0.0)
        .map(|w| (w.1 / w.0).ln())
        .collect();
    (logs.iter().sum::<f64>() / logs.len() as f64).exp()
}

/// The heights less the best-fit plane, by least squares.
fn remove_plane(patch: &Patch) -> Vec<f64> {
    let n = patch.size;
    let mid = (n as f64 - 1.0) * 0.5;
    let (mut mean, mut sx, mut sy) = (0.0, 0.0, 0.0);
    for j in 0..n {
        for i in 0..n {
            let z = patch.at(i, j);
            mean += z;
            sx += (i as f64 - mid) * z;
            sy += (j as f64 - mid) * z;
        }
    }
    // The sum of (i - mid)² over the grid.
    let sxx = (n * n) as f64 * ((n * n) as f64 - 1.0) / 12.0;
    let (mean, gx, gy) = (mean / (n * n) as f64, sx / sxx, sy / sxx);
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            out.push(patch.at(i, j) - mean - gx * (i as f64 - mid) - gy * (j as f64 - mid));
        }
    }
    out
}

fn slope_histogram(patch: &Patch) -> [f64; 7] {
    let n = patch.size;
    let mut counts = [0usize; 7];
    for j in 1..n - 1 {
        for i in 1..n - 1 {
            let dx = (patch.at(i + 1, j) - patch.at(i - 1, j)) / (2.0 * patch.cell_m);
            let dy = (patch.at(i, j + 1) - patch.at(i, j - 1)) / (2.0 * patch.cell_m);
            let deg = dx.hypot(dy).atan().to_degrees();
            let bin = SLOPE_BINS_DEG
                .iter()
                .position(|&top| deg < top)
                .unwrap_or(6);
            counts[bin] += 1;
        }
    }
    let total = ((n - 2) * (n - 2)) as f64;
    counts.map(|c| c as f64 / total)
}

/// The slope of the least-squares line through the points.
fn line_slope(points: &[(f64, f64)]) -> f64 {
    let count = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / count;
    let my = points.iter().map(|p| p.1).sum::<f64>() / count;
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = points.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    sxy / sxx
}

/// An FFT in place of a power-of-two length, radix 2. The inverse is not
/// scaled.
fn fft(data: &mut [[f64; 2]], inverse: bool) {
    let n = data.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let step = sign * 2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (step * k as f64).sin_cos();
                let a = data[start + k];
                let b = data[start + k + len / 2];
                let t = [b[0] * c - b[1] * s, b[0] * s + b[1] * c];
                data[start + k] = [a[0] + t[0], a[1] + t[1]];
                data[start + k + len / 2] = [a[0] - t[0], a[1] - t[1]];
            }
        }
        len <<= 1;
    }
}

/// A 2D FFT in place of a square row-major grid of side `n`.
fn fft_2d(grid: &mut [[f64; 2]], n: usize, inverse: bool) {
    for row in grid.chunks_mut(n) {
        fft(row, inverse);
    }
    let mut column = vec![[0.0; 2]; n];
    for i in 0..n {
        for j in 0..n {
            column[j] = grid[j * n + i];
        }
        fft(&mut column, inverse);
        for j in 0..n {
            grid[j * n + i] = column[j];
        }
    }
}

/// The realism report of a real region, with the patch and the band that
/// made it. The text form has one `key = value` line for each field, and
/// `#` starts a comment.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    /// The name of the region.
    pub region: String,
    /// The longitude and the latitude of the patch center, in degrees.
    pub center: (f64, f64),
    /// The cells on a side of the patch.
    pub size: usize,
    pub cell_m: f64,
    /// The shortest and the longest wavelength of the spectrum, in meters.
    pub band_m: (f64, f64),
    /// The trend of the fabric, in degrees clockwise from north.
    pub fabric_deg: f64,
    pub realism: Realism,
}

impl Reference {
    /// The fabric as the `direction` of `realism`.
    pub fn direction(&self) -> f64 {
        self.fabric_deg.to_radians()
    }

    pub fn parse(text: &str) -> Result<Reference, String> {
        let mut values = std::collections::HashMap::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("no `=` in the line `{line}`"))?;
            values.insert(key.trim(), value.trim());
        }
        let get = |key: &str| {
            values
                .get(key)
                .copied()
                .ok_or_else(|| format!("no value for `{key}`"))
        };
        let numbers = |key: &str| -> Result<Vec<f64>, String> {
            get(key)?
                .split(',')
                .map(|v| v.trim().parse::<f64>().map_err(|e| format!("`{key}`: {e}")))
                .collect()
        };
        let number = |key: &str| -> Result<f64, String> {
            match numbers(key)?[..] {
                [v] => Ok(v),
                _ => Err(format!("`{key}` has one value")),
            }
        };
        let pair = |key: &str| -> Result<(f64, f64), String> {
            match numbers(key)?[..] {
                [a, b] => Ok((a, b)),
                _ => Err(format!("`{key}` has two values")),
            }
        };
        let histogram: [f64; 7] = numbers("slope_histogram")?
            .try_into()
            .map_err(|_| "`slope_histogram` has 7 values".to_string())?;
        Ok(Reference {
            region: get("region")?.to_string(),
            center: pair("center")?,
            size: get("size")?.parse().map_err(|e| format!("`size`: {e}"))?,
            cell_m: number("cell_m")?,
            band_m: pair("band_m")?,
            fabric_deg: number("fabric_deg")?,
            realism: Realism {
                spectral_slope: number("spectral_slope")?,
                anisotropy: number("anisotropy")?,
                rms_m: number("rms_m")?,
                slope_histogram: histogram,
            },
        })
    }

    /// The text form, with no comments.
    pub fn to_text(&self) -> String {
        let r = &self.realism;
        let histogram: Vec<String> = r
            .slope_histogram
            .iter()
            .map(|f| format!("{f:.5}"))
            .collect();
        format!(
            "region = {}\ncenter = {}, {}\nsize = {}\ncell_m = {}\nband_m = {}, {}\n\
             fabric_deg = {}\nspectral_slope = {:.4}\nanisotropy = {:.4}\nrms_m = {:.2}\n\
             slope_histogram = {}\n",
            self.region,
            self.center.0,
            self.center.1,
            self.size,
            self.cell_m,
            self.band_m.0,
            self.band_m.1,
            self.fabric_deg,
            r.spectral_slope,
            r.anisotropy,
            r.rms_m,
            histogram.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A random number in 0..1 from a seed that it advances.
    fn random(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A surface with power `k^slope` at the wavenumber `k`, where the x
    /// part of the wavenumber is `stretch` times larger.
    fn fractal(n: usize, cell_m: f64, slope: f64, stretch: f64, seed: u64) -> Patch {
        let mut seed = seed;
        let mut grid = vec![[0.0; 2]; n * n];
        let signed = |i: usize| {
            if i < n / 2 {
                i as f64
            } else {
                i as f64 - n as f64
            }
        };
        for row in 0..n {
            for column in 0..n {
                let k = (signed(column) * stretch).hypot(signed(row));
                if k == 0.0 {
                    continue;
                }
                let amplitude = k.powf(slope * 0.5);
                let phase = random(&mut seed) * 2.0 * PI;
                grid[row * n + column] = [amplitude * phase.cos(), amplitude * phase.sin()];
            }
        }
        fft_2d(&mut grid, n, true);
        let peak = grid.iter().map(|c| c[0].abs()).fold(0.0, f64::max);
        Patch {
            size: n,
            cell_m,
            heights: grid.iter().map(|c| (c[0] / peak * 1000.0) as f32).collect(),
        }
    }

    fn plane(n: usize, cell_m: f64, gx: f64, gy: f64) -> Patch {
        let heights = (0..n * n)
            .map(|k| ((k % n) as f64 * cell_m * gx + (k / n) as f64 * cell_m * gy - 3000.0) as f32)
            .collect();
        Patch {
            size: n,
            cell_m,
            heights,
        }
    }

    #[test]
    fn realism_fft_inverts() {
        let mut data: Vec<[f64; 2]> = (0..16).map(|i| [i as f64, (i * i) as f64]).collect();
        let original = data.clone();
        fft(&mut data, false);
        fft(&mut data, true);
        for (a, b) in data.iter().zip(&original) {
            assert!((a[0] / 16.0 - b[0]).abs() < 1e-9 && (a[1] / 16.0 - b[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn realism_spectral_slope_of_a_fractal() {
        for slope in [-2.5, -3.0, -4.0] {
            let patch = fractal(256, 100.0, slope, 1.0, 11);
            let r = realism(&patch, (400.0, 6400.0), 0.0);
            assert!(
                (r.spectral_slope - slope).abs() < 0.15,
                "{slope}: {}",
                r.spectral_slope
            );
            assert!((r.anisotropy - 1.0).abs() < 0.25, "{}", r.anisotropy);
        }
    }

    #[test]
    fn realism_band_starts_at_4_cells() {
        let patch = fractal(128, 100.0, -3.0, 1.0, 5);
        let clamped = realism(&patch, (10.0, 3200.0), 0.0);
        let at_4_cells = realism(&patch, (400.0, 3200.0), 0.0);
        assert_eq!(clamped, at_4_cells);
    }

    #[test]
    fn realism_anisotropy_along_the_stretch() {
        // The power falls faster along the x part of the wavenumber, so the
        // relief is smooth in x: hills along x, to the east.
        let patch = fractal(256, 100.0, -3.0, 3.0, 3);
        let east = realism(&patch, (400.0, 12800.0), FRAC_PI_2);
        assert!(east.anisotropy > 2.0, "{}", east.anisotropy);
        let north = realism(&patch, (400.0, 12800.0), 0.0);
        assert!(north.anisotropy < 0.5, "{}", north.anisotropy);
        assert!((north.anisotropy * east.anisotropy - 1.0).abs() < 1e-9);
    }

    #[test]
    fn realism_rms_of_a_tilted_plane() {
        let r = realism(&plane(64, 500.0, 0.02, -0.01), (2000.0, 16000.0), 0.0);
        assert!(r.rms_m < 1e-3, "{}", r.rms_m);
    }

    #[test]
    fn realism_slope_histogram_of_a_plane() {
        let g = 5f64.to_radians().tan();
        let r = realism(&plane(64, 100.0, g * 0.6, g * 0.8), (400.0, 6400.0), 0.0);
        assert!(
            (r.slope_histogram[3] - 1.0).abs() < 1e-12,
            "{:?}",
            r.slope_histogram
        );
        assert!((r.slope_histogram.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn realism_patch_from_a_flat_heightmap() {
        let level = crate::cube::meters_to_level(-3000.0);
        let map = Heightmap::new(64, level);
        let pole = Patch::from_heightmap(&map, [0.0, 0.0, 1.0], 6371.0, 16, 1000.0);
        let equator = Patch::from_heightmap(&map, [1.0, 0.0, 0.0], 6371.0, 16, 1000.0);
        for patch in [pole, equator] {
            assert!(patch.heights.iter().all(|&h| (h + 3000.0).abs() < 0.2));
        }
    }

    #[test]
    fn realism_patch_axes_point_east_and_north() {
        let center = crate::math::lonlat_to_dir(30.0, 10.0);
        let patch = Patch::from_fn(center, 6371.0, 8, 10_000.0, |d| {
            let (lon, lat) = crate::math::dir_to_lonlat(d);
            lon * 1000.0 + lat
        });
        let at = |i: usize, j: usize| patch.heights[j * 8 + i];
        assert!(at(7, 3) - at(0, 3) > 300.0);
        assert!(at(3, 7) - at(3, 0) > 0.3 && at(3, 7) - at(3, 0) < 1.0);
    }

    #[test]
    fn realism_pacific_flank_parses() {
        let reference = Reference::parse(PACIFIC_FLANK).unwrap();
        assert!(reference.size.is_power_of_two());
        assert!(reference.band_m.0 >= MIN_WAVELENGTH_CELLS * reference.cell_m);
        assert!(reference.realism.anisotropy > 1.0);
    }

    #[test]
    fn realism_reference_round_trips() {
        let reference = Reference {
            region: "Test flank".to_string(),
            center: (-105.35, 11.6),
            size: 512,
            cell_m: 460.0,
            band_m: (1840.0, 60000.0),
            fabric_deg: -5.0,
            realism: Realism {
                spectral_slope: -2.8125,
                anisotropy: 2.5,
                rms_m: 120.25,
                slope_histogram: [0.1, 0.2, 0.3, 0.2, 0.15, 0.05, 0.0],
            },
        };
        let text = format!("# A comment\n\n{}", reference.to_text());
        assert_eq!(Reference::parse(&text), Ok(reference));
        assert!(Reference::parse("region = x").is_err());
    }
}
