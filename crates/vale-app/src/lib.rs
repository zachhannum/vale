//! Vale app library: the map pipeline and the UI.

pub mod app;
pub mod cli;
pub mod document;
pub mod furniture;
pub mod headless;
pub mod labels;
pub mod pick;
pub mod pipeline;
pub mod ui;
pub mod view;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    let result = ui::AppState::new(document::Document::sample()).and_then(|mut state| {
        state.file_buttons = false;
        app::run_window(state, (1280.0, 800.0), None)
    });
    if let Err(err) = result {
        eprintln!("vale-app: {err:#}");
    }
}
