//! The lakes of the filled pits. The flood of the flow fills each pit to its
//! rim, and a large pit shows that water as a lake.

use crate::cube::FACES;
use crate::flow::{Grid, NONE, OUT, cell_past_edge};

/// A pit shows as a lake if its cells cover this solid angle or more, in
/// steradians.
pub const LAKE_MIN_AREA: f64 = 5.0e-5;

/// The lake level of each cell, or 0 for no lake. `levels` are the cells
/// before the flood, and `filled` the cells after it. A lake cell is a land
/// cell that the flood raised, in a group of such cells that covers
/// `min_area` or more. It gets the level of the rim of its pit. A land cell
/// beside a lake gets the lowest level of the lakes beside it, so the shore
/// of a blend of the cells is at the full level.
pub(crate) fn lake_levels(
    grid: &impl Grid,
    levels: &[u16],
    filled: &[u16],
    sea: u16,
    min_area: f64,
) -> Vec<u16> {
    let count = levels.len();
    let raised = |i: usize| levels[i] >= sea && filled[i] > levels[i];
    let mut lakes = vec![0u16; count];
    let mut seen = vec![false; count];
    let mut group = Vec::new();
    for start in 0..count {
        if seen[start] || !raised(start) {
            continue;
        }
        seen[start] = true;
        group.clear();
        group.push(start as u32);
        let mut next = 0;
        while next < group.len() {
            let i = group[next] as usize;
            next += 1;
            for j in grid.neighbors(i) {
                if j == NONE || j == OUT {
                    continue;
                }
                let j = j as usize;
                if !seen[j] && raised(j) {
                    seen[j] = true;
                    group.push(j as u32);
                }
            }
        }
        let area: f64 = group.iter().map(|&i| grid.solid_angle(i as usize)).sum();
        if area >= min_area {
            for &i in &group {
                lakes[i as usize] = filled[i as usize];
            }
        }
    }
    // A land cell beside a lake is at or above the level of that lake, or
    // the flood would have raised it or drained the lake through it.
    let mut shore = lakes.clone();
    for i in (0..count).filter(|&i| lakes[i] == 0 && levels[i] >= sea) {
        let beside = grid.neighbors(i).into_iter().filter_map(|j| {
            let level = *lakes.get(j as usize)?;
            (level > 0).then_some(level)
        });
        if let Some(level) = beside.min() {
            shore[i] = level;
        }
    }
    shore
}

/// The cells of each face with a border of one cell from the faces next to
/// it, face by face, in row order. `cells` has `m` by `m` cells on each face.
pub(crate) fn bordered(m: usize, cells: &[u16]) -> Vec<u16> {
    let side = m + 2;
    let mut out = Vec::with_capacity(FACES * side * side);
    for face in 0..FACES {
        for y in -1..=m as i64 {
            for x in -1..=m as i64 {
                let (face, x, y) = if (0..m as i64).contains(&x) && (0..m as i64).contains(&y) {
                    (face, x as usize, y as usize)
                } else {
                    cell_past_edge(m, face, x, y)
                };
                out.push(cells[(face * m + y) * m + x]);
            }
        }
    }
    out
}

/// The blend of the lake levels of the 4 cells around a place. `lakes` is
/// the bordered layout of `bordered`. `x` and `y` are in cells, and a cell
/// center is at a whole number. `globe.wgsl` of the app has the same steps.
pub(crate) fn lake_at(lakes: &[u16], m: usize, face: usize, x: f64, y: f64) -> f64 {
    let side = m + 2;
    let cells = &lakes[face * side * side..(face + 1) * side * side];
    let (x, y) = (x + 1.0, y + 1.0);
    let last = (side - 1) as f64;
    let (x0, y0) = (x.floor(), y.floor());
    let at = |x: f64, y: f64| {
        let (x, y) = (x.clamp(0.0, last) as usize, y.clamp(0.0, last) as usize);
        f64::from(cells[y * side + x])
    };
    let (tx, ty) = (x - x0, y - y0);
    let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1.0, y0) * tx;
    let bottom = at(x0, y0 + 1.0) * (1.0 - tx) + at(x0 + 1.0, y0 + 1.0) * tx;
    top * (1.0 - ty) + bottom * ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::flow::tests::{OCEAN, WORLD_KM, from_cells};
    use crate::flow::{
        CoarseHeights, CubeGrid, FlowMap, cell_at, cell_dir, drain, sea, solid_angles,
    };
    use crate::heightmap::Heightmap;
    use crate::math::{angle, lonlat_to_dir};

    /// Land on the middle of face 0, at 800 m, with a round pit at 200 m.
    /// The pit has `radius` cells around the cell `(c, c)`. A notch at 500 m
    /// goes from the pit toward the coast along the row `c`, and past it the
    /// ground goes down to 100 m.
    fn bowl(m: usize, radius: f64) -> CoarseHeights {
        let c = m / 2;
        from_cells(m, move |face, x, y| {
            let inside = (m / 4..3 * m / 4).contains(&x) && (m / 4..3 * m / 4).contains(&y);
            if face != 0 || !inside {
                return OCEAN;
            }
            let (dx, dy) = (x as f64 - c as f64, y as f64 - c as f64);
            let notch = (radius.ceil() as usize) + c;
            if (dx * dx + dy * dy).sqrt() < radius {
                200.0
            } else if y == c && x == notch {
                500.0
            } else if y == c && x > notch {
                100.0
            } else {
                800.0
            }
        })
    }

    fn in_bowl(m: usize, radius: f64, x: usize, y: usize) -> bool {
        let c = (m / 2) as f64;
        ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt() < radius
    }

    #[test]
    fn a_pit_fills_to_its_rim_as_a_lake() {
        let (m, radius) = (64, 5.0);
        let flow = FlowMap::new(&bowl(m, radius));
        let rim = meters_to_level(500.0);
        for y in 0..m {
            for x in 0..m {
                if in_bowl(m, radius, x, y) {
                    assert_eq!(flow.lake(0, x, y), rim, "{x} {y}");
                }
            }
        }
        // The ground around the pit has no lake, and other faces have none.
        assert_eq!(flow.lake(0, m / 2 - 8, m / 2), 0);
        assert_eq!(flow.lake(1, m / 2, m / 2), 0);
    }

    #[test]
    fn the_cells_beside_a_lake_get_its_level() {
        let (m, radius) = (64, 5.0);
        let flow = FlowMap::new(&bowl(m, radius));
        let rim = meters_to_level(500.0);
        let c = m / 2;
        // The cells in a ring around the pit.
        for y in 0..m {
            for x in 0..m {
                let d = ((x as f64 - c as f64).powi(2) + (y as f64 - c as f64).powi(2)).sqrt();
                if d >= radius && d < radius + 0.9 {
                    assert_eq!(flow.lake(0, x, y), rim, "{x} {y}");
                }
            }
        }
        // The notch is the outlet. The cell after it has no lake.
        let notch = c + 5;
        assert_eq!(flow.lake(0, notch, c), rim);
        assert_eq!(flow.lake(0, notch + 1, c), 0);
    }

    #[test]
    fn the_lake_bytes_hold_the_lake_levels_in_order() {
        let channels = FlowMap::new(&bowl(64, 5.0)).channels(WORLD_KM);
        assert!(channels.has_lakes());
        assert_eq!(channels.lake_layers(), 1);
        let bytes = channels.lake_bytes();
        assert_eq!(bytes.len(), 128 * 128 * 4);
        for (i, &level) in channels.lakes().iter().enumerate() {
            assert_eq!(u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]), level);
        }
    }

    #[test]
    fn a_larger_lake_area_hides_a_lake() {
        let heights = bowl(64, 5.0);
        let c = 32;
        assert_eq!(FlowMap::new(&heights).lake(0, c, c), meters_to_level(500.0));
        // The pit covers about 0.05 steradians.
        let flow = FlowMap::with_lake_area(&heights, 0.2);
        assert_eq!(flow.lake(0, c, c), 0);
    }

    #[test]
    fn a_small_pit_has_no_lake() {
        // A cell at the middle of a face covers about 9.4e-6 steradians.
        let m = 512;
        let small = FlowMap::new(&bowl(m, 1.0));
        let c = m / 2;
        assert_eq!(small.lake(0, c, c), 0);
        let large = FlowMap::new(&bowl(m, 3.0));
        assert_eq!(large.lake(0, c, c), meters_to_level(500.0));
    }

    /// The heights of a pit at 200 m inside land at 800 m, centered at `at`.
    fn round_pit(m: usize, at: [f64; 3]) -> CoarseHeights {
        CoarseHeights::from_fn(m, |d| {
            let from = angle(d, at);
            let meters = if from < 0.06 {
                200.0
            } else if from < 0.3 {
                800.0
            } else {
                OCEAN
            };
            meters_to_level(meters)
        })
    }

    #[test]
    fn a_lake_across_a_face_edge_is_one_lake() {
        let m = 64;
        let at = lonlat_to_dir(45.0, 0.0);
        let heights = round_pit(m, at);
        let grid = CubeGrid {
            m,
            solid: solid_angles(m),
        };
        let flow = drain(&grid, &heights.levels, &[], sea());
        let raised: Vec<usize> = (0..heights.levels.len())
            .filter(|&i| flow.filled[i] > heights.levels[i])
            .collect();
        let area = |face: Option<usize>| -> f64 {
            let on = |i: usize| face.is_none_or(|face| i / (m * m) == face);
            raised
                .iter()
                .filter(|&&i| on(i))
                .map(|&i| grid.solid_angle(i))
                .sum()
        };
        let (face_a, ..) = cell_at(m, lonlat_to_dir(44.0, 0.0));
        let (face_b, ..) = cell_at(m, lonlat_to_dir(46.0, 0.0));
        assert_ne!(face_a, face_b);
        // Each half is below the limit, and the whole lake is above it.
        let limit = 0.75 * area(None);
        assert!(area(Some(face_a)) < limit && area(Some(face_b)) < limit);
        let lakes = lake_levels(&grid, &heights.levels, &flow.filled, sea(), limit);
        let rim = meters_to_level(800.0);
        for i in raised {
            assert_eq!(lakes[i], rim);
        }
    }

    /// The texels of each face within `reach` texels of the place `at` on
    /// the plane of the face.
    fn texels_near(n: usize, at: [f64; 3], reach: f64) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        for face in 0..crate::cube::FACES {
            let (axis, sign) = (face / 2, if face % 2 == 0 { 1.0 } else { -1.0 });
            let depth = at[axis] * sign;
            if depth < 0.5 {
                continue;
            }
            let texel = |a: f64| (crate::cube::warp(a / depth) + 1.0) * 0.5 * n as f64;
            let (u, v) = (texel(at[(axis + 1) % 3]), texel(at[(axis + 2) % 3]));
            let range =
                |c: f64| ((c - reach).max(0.0) as usize)..((c + reach).min(n as f64) as usize);
            for y in range(v) {
                for x in range(u) {
                    out.push((face, x, y));
                }
            }
        }
        out
    }

    /// A heightmap with 4 texels for each cell of the coarse copy. Land at
    /// 800 m on the middle of face 0 has a round bowl that goes down to
    /// 200 m. The bowl has no outlet, so its rim is at 800 m.
    fn fine_bowl(at: [f64; 3]) -> Heightmap {
        let n = 2048;
        let mut map = Heightmap::new(n, meters_to_level(OCEAN));
        for (face, x, y) in texels_near(n, at, 420.0) {
            let from = angle(cell_dir(n, face, x, y), at);
            if from < 0.3 {
                let t = (from / 0.05).min(1.0);
                map.set(face, x, y, meters_to_level(200.0 + 600.0 * t * t));
            }
        }
        map
    }

    /// Each texel near the bowl is below the lake level of the blend if and
    /// only if it is below the rim.
    fn assert_shore(at: [f64; 3]) {
        let map = fine_bowl(at);
        let channels = FlowMap::new(&CoarseHeights::new(&map)).channels(WORLD_KM);
        let rim = meters_to_level(800.0);
        let n = map.face_size();
        let mut wet = 0;
        for (face, x, y) in texels_near(n, at, 100.0) {
            let d = cell_dir(n, face, x, y);
            if angle(d, at) > 0.07 {
                continue;
            }
            let level = map.get(face, x as i64, y as i64);
            let lake = f64::from(level) < channels.lake_level(d);
            assert_eq!(lake, level < rim, "face {face} texel {x} {y}");
            wet += usize::from(lake);
        }
        assert!(wet > 1000, "{wet}");
    }

    #[test]
    fn the_shore_follows_the_fine_heights() {
        assert_shore(lonlat_to_dir(10.0, 15.0));
    }

    #[test]
    fn a_lake_level_has_no_seam_at_a_face_edge() {
        assert_shore(lonlat_to_dir(45.0, 0.0));
        // The two sides of the edge get the same level.
        let at = lonlat_to_dir(45.0, 0.0);
        let channels = FlowMap::new(&CoarseHeights::new(&fine_bowl(at))).channels(WORLD_KM);
        for lat in [-2.0, 0.0, 2.0] {
            let a = channels.lake_level(lonlat_to_dir(44.999, lat));
            let b = channels.lake_level(lonlat_to_dir(45.001, lat));
            assert!((a - b).abs() <= 1.0, "{a} {b}");
        }
    }
}
