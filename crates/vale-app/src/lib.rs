//! Vale app library: the map pipeline and the UI.

pub mod app;
pub mod cli;
pub mod document;
pub mod files;
pub mod furniture;
pub mod globe;
pub mod headless;
pub mod labels;
#[cfg(target_os = "ios")]
pub mod lifecycle;
pub mod pen;
pub mod pick;
pub mod pipeline;
pub mod projects;
pub mod resume;
pub mod session;
pub mod ui;
pub mod view;
pub mod workcopy;
pub mod world;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    files::clear_exports();
    // The layers of a file take the styles of the sample.
    let result = ui::AppState::new(document::Document::sample()).and_then(|mut state| {
        state.file_buttons = false;
        state.share_exports = true;
        state.projects = projects::default_locations();
        state.projects.adopt_old_projects();
        state.files = Box::new(files::cloud::CloudFiles);
        state.recover_projects();
        files::cloud::install(state.system.clone());
        state.globe.set_face_size(globe::WINDOW_FACE_SIZE);
        state.show_projects();
        app::run_window(state, (1280.0, 800.0), None, None, true)
    });
    if let Err(err) = result {
        eprintln!("vale-app: {err:#}");
    }
}
