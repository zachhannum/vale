//! The parts of an app that Vale and Isotherm share: the theme, the touch
//! widgets, and the glass cards of the pad layout, the file picker, the
//! shader compiles on worker threads, the screenshots without a window, the files of a sync
//! service, the working copy of a project, the state of the next start, and
//! the iOS life cycle.

pub mod backdrop;
pub mod compile;
pub mod files;
#[cfg(not(target_os = "ios"))]
pub mod headless;
#[cfg(target_os = "ios")]
pub mod lifecycle;
pub mod pad;
pub mod picker;
pub mod resume;
pub mod workcopy;
