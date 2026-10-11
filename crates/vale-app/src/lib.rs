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
pub mod session;
pub mod ui;
pub mod view;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    let mut doc = document::Document::sample();
    // `HOME` is the container of the app.
    let session = std::env::var_os("HOME").and_then(|home| {
        let path =
            std::path::Path::new(&home).join("Library/Application Support/Vale/project.gpkg");
        session::Session::open(&path, &mut doc)
            .inspect_err(|err| eprintln!("vale-app: {}: {err}", path.display()))
            .ok()
    });
    let result = ui::AppState::new(doc).and_then(|mut state| {
        state.file_buttons = false;
        state.globe.set_face_size(globe::WINDOW_FACE_SIZE);
        app::run_window(state, (1280.0, 800.0), None, session)
    });
    if let Err(err) = result {
        eprintln!("vale-app: {err:#}");
    }
}
