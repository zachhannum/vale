//! The grid sizes that fit in the memory that the system gives the app.

use isotherm_fields::cells;
use isotherm_sim::memory::{model_bytes, snapshot_bytes};
pub use vale_shell::memory::{available, set_test_available};

/// The face sizes of the quality setting.
pub const QUALITIES: [usize; 4] = [32, 64, 128, 256];

/// The memory that stays free for the terrain, the globe, and the rest of
/// the app.
pub const RESERVE: u64 = 256 << 20;

/// The budget where the system gives no limit.
pub const FIXED_BUDGET: u64 = 1 << 30;

/// The largest face size that the CPU model runs at a speed for a live
/// view. A model year takes about 1.4 s at 32 and 9.8 s at 64 on a desktop
/// CPU.
#[cfg(target_os = "ios")]
pub const CPU_MAX: usize = 64;
#[cfg(not(target_os = "ios"))]
pub const CPU_MAX: usize = 128;

/// The memory that a run of face size `n` needs at its peak: the model, one
/// copy of the state during a save, and the temperature layer of the globe,
/// with its levels in the main memory and on the GPU.
pub fn run_bytes(face_size: usize) -> u64 {
    let overlay = cells(face_size) as u64 * 2 * 2;
    model_bytes(face_size) + snapshot_bytes(face_size) + overlay
}

/// The face sizes whose run fits in `available` with `RESERVE` left.
/// `None`: the fixed budget. The smallest size is always offered.
pub fn offered(available: Option<u64>) -> Vec<usize> {
    let budget = available.unwrap_or(FIXED_BUDGET);
    QUALITIES
        .into_iter()
        .enumerate()
        .filter(|&(i, n)| i == 0 || run_bytes(n) + RESERVE <= budget)
        .map(|(_, n)| n)
        .collect()
}

/// The face sizes of the quality setting: the sizes that fit and that the
/// CPU model runs.
pub fn qualities(available: Option<u64>) -> Vec<usize> {
    offered(available)
        .into_iter()
        .filter(|&n| n <= CPU_MAX)
        .collect()
}

/// The face size of a new run: the grid of the live view.
pub fn default_quality(offered: &[usize]) -> usize {
    offered.first().copied().unwrap_or(QUALITIES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_holds_the_model_and_a_snapshot() {
        for n in QUALITIES {
            assert!(run_bytes(n) >= model_bytes(n) + snapshot_bytes(n));
        }
    }

    #[test]
    fn the_offered_sizes_grow_with_the_memory() {
        assert_eq!(offered(Some(0)), vec![32]);
        assert_eq!(offered(Some(RESERVE + run_bytes(64))), vec![32, 64]);
        assert_eq!(offered(None), QUALITIES.to_vec());
        let mut last = 0;
        for mb in (0..2048).step_by(64) {
            let sizes = offered(Some(mb << 20));
            assert_eq!(sizes[..], QUALITIES[..sizes.len()]);
            assert!(sizes.len() >= last);
            last = sizes.len();
        }
    }

    #[test]
    fn the_memory_of_a_run_is_the_table_of_the_design() {
        // The table "Memory of a run" in `docs/isotherm/DESIGN.md`, in MB of
        // 2^20 bytes.
        let table = [(32, 6.6), (64, 26.3), (128, 105.1), (256, 420.4)];
        for (n, mb) in table {
            let got = run_bytes(n) as f64 / f64::from(1 << 20);
            assert!((got - mb).abs() < 0.05, "{n}: {got:.2} MB");
        }
    }

    #[test]
    fn the_quality_setting_keeps_to_the_cpu_model() {
        assert!(qualities(None).iter().all(|&n| n <= CPU_MAX));
        assert_eq!(default_quality(&qualities(None)), 32);
    }
}
