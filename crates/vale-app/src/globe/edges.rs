//! The screen-space lines of the color stops. A pass finds the places where
//! the shown height crosses the value of a stop between two pixel centers,
//! and a second pass gives each pixel the nearest of them. The layer pass
//! then draws each line at its weight in pixels from the band edge.
//!
//! A pixel keeps one place as a 32-bit payload, relative to the pixel. The
//! layout matches `edges.wgsl`.

use super::preview::LineUniform;

/// The largest half weight of a line in pixels. A larger weight is cut to it,
/// so that the offset of a place fits in the payload.
pub const MAX_HALF: f32 = 29.0;
/// The payload of a pixel with no place.
pub const EMPTY: u32 = u32::MAX;
/// The steps of the crossing between two pixel centers.
pub const T_STEPS: u32 = 64;

const OFFSET_BITS: u32 = 6;
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;
const AXIS_SHIFT: u32 = 2 * OFFSET_BITS;
const T_SHIFT: u32 = AXIS_SHIFT + 1;
const T_MASK: u32 = (1 << 7) - 1;
const STOP_SHIFT: u32 = T_SHIFT + 7;
const STOP_MASK: u32 = (1 << 6) - 1;

/// The largest offset of a place from a pixel, on each axis.
pub const MAX_OFFSET: i32 = (1 << (OFFSET_BITS - 1)) - 1;

/// One place where the height crosses the value of a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed {
    /// The offset of the first pixel of the pair from the pixel that keeps
    /// the place.
    pub base: [i32; 2],
    /// The second pixel is the next pixel in x (false) or in y (true).
    pub axis: bool,
    /// The place of the crossing from the first pixel center, in steps of
    /// 1 / `T_STEPS` of a pixel.
    pub t: u32,
    /// The index of the stop.
    pub stop: u32,
}

/// The half weight of the widest line, cut to `MAX_HALF`. It is 0 when no
/// line is on.
pub fn max_half(lines: &[LineUniform]) -> f32 {
    lines
        .iter()
        .map(|line| line[0][3])
        .fold(0.0_f32, f32::max)
        .min(MAX_HALF)
}

/// The distance in pixels, on each axis, inside which a pixel looks for the
/// nearest place. `None` when no line is on.
pub fn radius(max_half: f32) -> Option<u32> {
    (max_half > 0.0).then(|| max_half.min(MAX_HALF).ceil() as u32 + 2)
}

/// The payload of a place. `None` when an offset or a field does not fit.
pub fn pack(seed: Seed) -> Option<u32> {
    let fits = |v: i32| (-MAX_OFFSET - 1..=MAX_OFFSET).contains(&v);
    if !fits(seed.base[0]) || !fits(seed.base[1]) || seed.t > T_STEPS || seed.stop > STOP_MASK {
        return None;
    }
    let bx = seed.base[0] as u32 & OFFSET_MASK;
    let by = seed.base[1] as u32 & OFFSET_MASK;
    Some(
        bx | by << OFFSET_BITS
            | u32::from(seed.axis) << AXIS_SHIFT
            | seed.t << T_SHIFT
            | seed.stop << STOP_SHIFT,
    )
}

/// The place in a payload. `None` for `EMPTY`.
pub fn unpack(payload: u32) -> Option<Seed> {
    if payload == EMPTY {
        return None;
    }
    let signed = |v: u32| ((v << (32 - OFFSET_BITS)) as i32) >> (32 - OFFSET_BITS);
    Some(Seed {
        base: [
            signed(payload & OFFSET_MASK),
            signed(payload >> OFFSET_BITS & OFFSET_MASK),
        ],
        axis: payload >> AXIS_SHIFT & 1 == 1,
        t: payload >> T_SHIFT & T_MASK,
        stop: payload >> STOP_SHIFT & STOP_MASK,
    })
}

/// The payload of a place kept by a pixel, for a pixel at `step` from it.
/// `None` when the new offset does not fit.
pub fn rebase(payload: u32, step: [i32; 2]) -> Option<u32> {
    let mut seed = unpack(payload)?;
    seed.base = [seed.base[0] - step[0], seed.base[1] - step[1]];
    pack(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(half: f32) -> LineUniform {
        [[0.0, 0.0, 0.0, half], [0.5, 0.0, 0.0, 0.0]]
    }

    #[test]
    fn the_widest_line_sets_the_radius() {
        let lines = [line(0.0), line(1.5), line(3.2), line(0.0)];
        assert_eq!(max_half(&lines), 3.2);
        assert_eq!(radius(max_half(&lines)), Some(6));
    }

    #[test]
    fn no_line_has_no_radius() {
        assert_eq!(max_half(&[line(0.0), line(0.0)]), 0.0);
        assert_eq!(radius(0.0), None);
    }

    #[test]
    fn a_wide_line_is_cut_so_its_offsets_fit() {
        assert_eq!(max_half(&[line(80.0)]), MAX_HALF);
        let r = radius(80.0).unwrap() as i32;
        assert!(r <= MAX_OFFSET);
        // A pixel at the radius reaches a place one pixel back on either side.
        for base in [[-r, r - 1], [r - 1, -r]] {
            let seed = Seed {
                base,
                axis: false,
                t: 0,
                stop: 0,
            };
            assert!(pack(seed).is_some());
        }
    }

    #[test]
    fn a_payload_keeps_each_field() {
        for base in [[0, 0], [-32, 31], [31, -32], [-1, 5]] {
            for axis in [false, true] {
                for t in [0, 1, 37, T_STEPS] {
                    for stop in [0, 7, 32, 63] {
                        let seed = Seed {
                            base,
                            axis,
                            t,
                            stop,
                        };
                        let payload = pack(seed).unwrap();
                        assert_ne!(payload, EMPTY);
                        assert_eq!(unpack(payload), Some(seed));
                    }
                }
            }
        }
    }

    #[test]
    fn a_field_that_does_not_fit_has_no_payload() {
        let seed = Seed {
            base: [0, 0],
            axis: false,
            t: 0,
            stop: 0,
        };
        assert!(
            pack(Seed {
                base: [32, 0],
                ..seed
            })
            .is_none()
        );
        assert!(
            pack(Seed {
                base: [0, -33],
                ..seed
            })
            .is_none()
        );
        assert!(
            pack(Seed {
                t: T_STEPS + 1,
                ..seed
            })
            .is_none()
        );
        assert!(pack(Seed { stop: 64, ..seed }).is_none());
        assert_eq!(unpack(EMPTY), None);
    }

    #[test]
    fn rebase_moves_the_offset_to_the_new_pixel() {
        let seed = Seed {
            base: [2, -3],
            axis: true,
            t: 20,
            stop: 4,
        };
        let moved = rebase(pack(seed).unwrap(), [1, -1]).unwrap();
        assert_eq!(
            unpack(moved),
            Some(Seed {
                base: [1, -2],
                ..seed
            })
        );
        assert_eq!(rebase(EMPTY, [1, 0]), None);
        let far = Seed {
            base: [MAX_OFFSET, 0],
            ..seed
        };
        assert_eq!(rebase(pack(far).unwrap(), [-1, 0]), None);
    }
}
