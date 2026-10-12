//! Vale app library: the map pipeline and the UI.

pub mod app;
pub mod cli;
pub mod document;
pub mod files;
pub mod furniture;
pub mod globe;
pub mod headless;
pub mod labels;
pub mod pen;
pub mod pick;
pub mod pipeline;
pub mod projects;
pub mod session;
pub mod ui;
pub mod view;
pub mod world;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    // The layers of a file take the styles of the sample.
    let result = ui::AppState::new(document::Document::sample()).and_then(|mut state| {
        state.file_buttons = false;
        state.projects = projects::default_dir();
        state.globe.set_face_size(globe::WINDOW_FACE_SIZE);
        state.show_projects();
        app::run_window(state, (1280.0, 800.0), None, None)
    });
    if let Err(err) = result {
        eprintln!("vale-app: {err:#}");
    }
}
