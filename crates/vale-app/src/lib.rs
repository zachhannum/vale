//! Vale app library: the map pipeline and the UI.

pub mod app;
pub mod atlas;
pub mod atlas_export;
pub mod classify;
pub mod cli;
pub mod clipboard;
pub mod color_key;
pub mod distortion;
pub mod document;
pub mod edge_labels;
pub mod files;
#[cfg(target_os = "ios")]
pub mod font_picker;
pub mod fonts;
pub mod frame_border;
pub mod furniture;
pub mod globe;
pub mod headless;
pub mod image_layer;
pub mod keyboard;
pub mod labels;
#[cfg(target_os = "ios")]
pub mod lifecycle;
pub mod lod;
pub mod log_file;

/// The log file reads the size of the heap from this allocator.
#[cfg(target_os = "ios")]
#[global_allocator]
static ALLOCATOR: log_file::CountingAllocator = log_file::CountingAllocator;
pub mod map_styles;
pub mod mask;
pub mod measure;
pub mod memory;
pub mod neighbors;
pub mod page_snap;
pub mod pen;
pub mod pick;
pub mod pipeline;
pub mod projects;
pub mod ramps;
pub mod raster_style;
pub mod raster_values;
pub mod region;
pub mod reserved;
pub mod resume;
pub mod rule_edit;
pub mod scale_bar;
pub mod scatter;
pub mod session;
pub mod table;
pub mod tools;
pub mod ui;
pub mod view;
pub mod workcopy;
pub mod world;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    if let Some(dir) = projects::default_dir() {
        let _ = log_file::install(&dir.join(log_file::FILE_NAME));
    }
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
        log_file::write(&format!("error: {err:#}"));
    }
}
