//! The memory of a run at each grid size, for the memory budget of each
//! device.

use isotherm_fields::{Field, Output, cells};

use crate::grid::Grid;
use crate::ops::Stencil;
use crate::{model, solver};

/// The bytes of [`isotherm_fields::Means`] with all outputs at face size `n`.
fn means_bytes(n: usize) -> u64 {
    let cells = cells(n);
    let k = Output::ALL.len();
    (k * cells * (13 * 4 + 8)
        + k * size_of::<Field>()
        + k * size_of::<Vec<f64>>()
        + k * size_of::<Output>()) as u64
}

/// The heap bytes of a model at face size `n`: the grid tables, the cell
/// data, the state, the solver, and the means of all outputs.
pub fn model_bytes(face_size: usize) -> u64 {
    let cells = cells(face_size) as u64;
    let solver = (solver::F64_PER_CELL * 8) as u64 + Stencil::BYTES_PER_CELL;
    let own = (model::F64_PER_CELL * 8 + 1) as u64;
    cells * (Grid::bytes_per_cell() + solver + own) + means_bytes(face_size)
}

/// The heap bytes of one [`crate::RunState`] at face size `n`.
pub fn snapshot_bytes(face_size: usize) -> u64 {
    cells(face_size) as u64 * 24 + means_bytes(face_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model;
    use isotherm_world::{Planet, World};

    /// The bytes of a run at each face size, as `docs/isotherm/DESIGN.md`
    /// gives them.
    const TABLE: [(usize, u64, u64); 4] = [
        (32, 5_978_834, 885_458),
        (64, 23_913_170, 3_539_666),
        (128, 95_650_514, 14_156_498),
        (256, 382_599_890, 56_623_826),
    ];

    #[test]
    fn model_bytes_counts_every_allocation() {
        for n in [32, 64, 128] {
            let world = World::flat("Flat", Planet::earth(), 16);
            let mut model = Model::new(&world, n);
            model.run(31);
            assert_eq!(model.heap_bytes(), model_bytes(n), "{n}");
            assert_eq!(model.snapshot().heap_bytes(), snapshot_bytes(n), "{n}");
        }
    }

    #[test]
    fn the_bytes_of_each_grid_size_match_the_design_table() {
        for (n, model, snapshot) in TABLE {
            assert_eq!(
                (model_bytes(n), snapshot_bytes(n)),
                (model, snapshot),
                "{n}"
            );
        }
    }
}
