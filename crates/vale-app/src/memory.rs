//! The memory that the system still gives the app, and the memory that a
//! whole heightmap needs.

use std::cell::Cell;

use vale_terrain::FACES;

/// The bytes of one texel of a whole heightmap: 2 in the main memory, and 2
/// on the GPU with a third more for the coarser levels.
const BYTES_PER_TEXEL: f64 = 2.0 + 2.0 * 4.0 / 3.0;

/// The memory that stays free for the rest of the app after a heightmap
/// loads.
pub const RESERVE: u64 = 512 << 20;

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

thread_local! {
    static TEST_AVAILABLE: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Sets the memory that `available` gives on this thread, for a test.
#[doc(hidden)]
pub fn set_test_available(bytes: Option<u64>) {
    TEST_AVAILABLE.set(bytes);
}

/// The memory that the system still gives the app before it stops the app,
/// in bytes. `None`: the system has no such limit.
pub fn available() -> Option<u64> {
    if let Some(bytes) = TEST_AVAILABLE.get() {
        return Some(bytes);
    }
    system_available()
}

#[cfg(target_os = "ios")]
fn system_available() -> Option<u64> {
    unsafe extern "C" {
        fn os_proc_available_memory() -> usize;
    }
    // SAFETY: the function has no arguments. It returns 0 when the process
    // has no limit.
    let bytes = unsafe { os_proc_available_memory() };
    (bytes > 0).then_some(bytes as u64)
}

#[cfg(not(target_os = "ios"))]
fn system_available() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heightmap_of_face_8192_needs_about_2_gb() {
        let gb = heightmap_bytes(8192) as f64 / f64::from(1 << 30);
        assert!((1.7..1.9).contains(&gb), "{gb}");
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
