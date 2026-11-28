//! Measures the profiles across a scarp in a GEBCO grid and prints the scarp
//! reference text. Each profile runs east along one latitude, from WEST to
//! EAST, and the scarp must drop to the east.
//! `scripts/gebco-reference.sh manihiki-scarp` downloads the grid and runs
//! this example.
//!
//! ```sh
//! cargo run --release -p vale-terrain --example gebco_profiles -- \
//!     GRID.asc SOUTH NORTH COUNT WEST EAST
//! ```

#[path = "shared/grid.rs"]
mod grid;

use grid::{Grid, M_PER_DEG};
use vale_terrain::{PROFILE_STEP_KM, ScarpReference, scarp_profile};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [grid, south, north, count, west, east] = args.as_slice() else {
        eprintln!("usage: gebco_profiles GRID.asc SOUTH NORTH COUNT WEST EAST");
        std::process::exit(2);
    };
    let number = |s: &String| s.parse::<f64>().expect("number");
    let (south, north, west, east) = (number(south), number(north), number(west), number(east));
    let count: usize = count.parse().expect("count");
    let grid = Grid::read(&std::fs::read_to_string(grid).expect("read the grid"));
    let profiles = (0..count)
        .map(|n| {
            let lat = north - (north - south) * n as f64 / (count - 1) as f64;
            let km_per_deg = M_PER_DEG * lat.to_radians().cos() / 1000.0;
            let samples = ((east - west) * km_per_deg / PROFILE_STEP_KM) as usize;
            let z: Vec<f64> = (0..=samples)
                .map(|i| grid.at(west + i as f64 * PROFILE_STEP_KM / km_per_deg, lat))
                .collect();
            scarp_profile(&z)
        })
        .collect();
    let reference = ScarpReference {
        profiles,
        spacing_km: (north - south) / (count - 1) as f64 * M_PER_DEG / 1000.0,
    };
    print!("{}", reference.lines());
}
