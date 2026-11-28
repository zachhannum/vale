//! The memory that the system still gives the app, and the memory that a
//! whole heightmap needs.

pub use vale_shell::memory::{available, set_test_available};
use vale_terrain::FACES;

/// The bytes of one texel of a whole heightmap, as an iPhone measured: 2 in
/// the main memory, 2 on the GPU with a third more for the coarser levels,
/// and about 1.6 more that neither of them counts.
const BYTES_PER_TEXEL: f64 = 6.25;

/// The memory that stays free for the rest of the app after a heightmap
/// loads.
pub const RESERVE: u64 = 256 << 20;

/// The memory that a whole heightmap with all its tiles needs, in the main
/// memory and on the GPU together.
pub fn heightmap_bytes(face_size: usize) -> u64 {
    let texels = (FACES * face_size * face_size) as f64;
    (texels * BYTES_PER_TEXEL) as u64
}

/// True if a whole heightmap fits in `available` bytes and leaves
/// `RESERVE`. With no limit, each heightmap fits.
pub fn heightmap_fits(face_size: usize, available: Option<u64>) -> bool {
    available.is_none_or(|bytes| heightmap_bytes(face_size) + RESERVE <= bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heightmap_of_face_8192_needs_what_an_iphone_measured() {
        // The footprint grew by about 2,400 MB when the layer loaded.
        let mb = heightmap_bytes(8192) >> 20;
        assert!((2300..2500).contains(&mb), "{mb}");
    }

    #[test]
    fn a_heightmap_fits_only_with_the_reserve_left() {
        let need = heightmap_bytes(4096);
        assert!(heightmap_fits(4096, None));
        assert!(heightmap_fits(4096, Some(need + RESERVE)));
        assert!(!heightmap_fits(4096, Some(need + RESERVE - 1)));
        assert!(!heightmap_fits(8192, Some(2 << 30)));
    }
}
