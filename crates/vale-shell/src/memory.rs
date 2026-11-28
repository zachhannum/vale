//! The memory that the system still gives the app.

use std::cell::Cell;

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
