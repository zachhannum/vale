//! The parts of an app that Vale and Isotherm share: the theme and the
//! touch widgets of the pad layout, the shader compiles on worker threads,
//! the files of a sync service, the working copy of a project, the state of
//! the next start, and the iOS life cycle.

pub mod compile;
pub mod files;
#[cfg(target_os = "ios")]
pub mod lifecycle;
pub mod pad;
pub mod resume;
pub mod workcopy;
