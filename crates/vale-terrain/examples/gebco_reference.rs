//! Computes the realism reference of a GEBCO region. It reads an ESRI ASCII
//! grid of elevations in meters, samples a square patch on the plane that
//! touches the Earth at the patch center, and prints the reference text.
//! `scripts/gebco-reference.sh` downloads the grid and runs this example.
//!
//! ```sh
//! cargo run --release -p vale-terrain --example gebco_reference -- \
//!     GRID.asc OUT_DIR NAME "REGION" LON,LAT SIZE CELL_M BAND_MAX_M FABRIC_DEG
//! ```
//!
//! It also writes the patch to `OUT_DIR/NAME.f32` (little-endian `f32`
//! meters, row 0 at the south, as in `Patch`) and to `OUT_DIR/NAME.png`
//! (16-bit gray Vale levels, north up).

use std::path::PathBuf;

use image::{ImageBuffer, Luma};
use vale_terrain::math::{dir_to_lonlat, lonlat_to_dir};
use vale_terrain::{MIN_WAVELENGTH_CELLS, Patch, Realism, Reference, meters_to_level, realism};

#[path = "shared/grid.rs"]
mod grid;

use grid::{Grid, M_PER_DEG};

const EARTH_KM: f64 = 6371.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [
        grid,
        out_dir,
        name,
        region,
        center,
        size,
        cell_m,
        band_max_m,
        fabric_deg,
    ] = &args[..]
    else {
        eprintln!(
            "usage: gebco_reference GRID.asc OUT_DIR NAME REGION LON,LAT SIZE CELL_M \
             BAND_MAX_M FABRIC_DEG"
        );
        std::process::exit(2);
    };
    let number = |s: &str| s.trim().parse::<f64>().expect("number");
    let (lon, lat) = center.split_once(',').expect("LON,LAT");
    let center = (number(lon), number(lat));
    let size: usize = size.parse().expect("size");
    let cell_m = number(cell_m);
    let band_m = (MIN_WAVELENGTH_CELLS * cell_m, number(band_max_m));
    let fabric_deg = number(fabric_deg);

    let grid = Grid::read(&std::fs::read_to_string(grid).expect("read the grid"));
    let k = ((cell_m / (grid.cell_deg * M_PER_DEG)).round() as usize).max(1);
    let patch = Patch::from_fn(
        lonlat_to_dir(center.0, center.1),
        EARTH_KM,
        size,
        cell_m,
        |d| {
            let (lon, lat) = dir_to_lonlat(d);
            // The mean of `k` by `k` points over the cell, so a cell wider
            // than a grid cell does not alias.
            let step_lat = cell_m / k as f64 / M_PER_DEG;
            let step_lon = step_lat / lat.to_radians().cos();
            let half = (k as f64 - 1.0) * 0.5;
            let mut sum = 0.0;
            for j in 0..k {
                for i in 0..k {
                    let (x, y) = (i as f64 - half, j as f64 - half);
                    sum += grid.at(lon + x * step_lon, lat + y * step_lat);
                }
            }
            sum / (k * k) as f64
        },
    );

    let reference = Reference {
        region: region.clone(),
        center,
        size,
        cell_m,
        band_m,
        fabric_deg,
        realism: realism(&patch, band_m, fabric_deg.to_radians()),
    };
    print!("{}", reference.to_text());

    // The trend of the strongest anisotropy, as a check of the fabric.
    let (best, ratio) = (0..36)
        .map(|step| {
            let deg = step as f64 * 5.0 - 90.0;
            let Realism { anisotropy, .. } = realism(&patch, band_m, deg.to_radians());
            (deg, anisotropy)
        })
        .fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    eprintln!("strongest anisotropy: {ratio:.3} at {best} degrees");

    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).expect("make the output folder");
    let bytes: Vec<u8> = patch.heights.iter().flat_map(|h| h.to_le_bytes()).collect();
    std::fs::write(out_dir.join(format!("{name}.f32")), bytes).expect("write the patch");
    let image = ImageBuffer::from_fn(size as u32, size as u32, |x, y| {
        let row = size - 1 - y as usize;
        Luma([meters_to_level(f64::from(
            patch.heights[row * size + x as usize],
        ))])
    });
    let image: ImageBuffer<Luma<u16>, Vec<u16>> = image;
    image
        .save(out_dir.join(format!("{name}.png")))
        .expect("write the picture");
}
