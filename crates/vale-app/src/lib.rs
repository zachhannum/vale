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
pub mod world;

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_app_main() {
    // `HOME` is the container of the app.
    let path = std::env::var_os("HOME").map(|home| {
        std::path::Path::new(&home).join("Library/Application Support/Vale/project.gpkg")
    });
    // With no project file, the app starts with an empty world and the form
    // of a new world. The layers of a file take the styles of the sample.
    let first_start = path.as_ref().is_some_and(|path| !path.exists());
    let mut doc = if first_start {
        document::Document::empty()
    } else {
        document::Document::sample()
    };
    let session = path.and_then(|path| {
        session::Session::open(&path, &mut doc)
            .inspect_err(|err| eprintln!("vale-app: {}: {err}", path.display()))
            .ok()
    });
    let result = ui::AppState::new(doc).and_then(|mut state| {
        state.file_buttons = false;
        if first_start {
            state.world_form = Some(ui::world_form::WorldForm::new_world());
        }
        state.globe.set_face_size(globe::WINDOW_FACE_SIZE);
        // A session that cannot read its heightmap must not save over it.
        let session = session.and_then(|session| match session.load_map() {
            Ok(map) => {
                map.into_iter().for_each(|map| state.globe.set_map(map));
                Some(session)
            }
            Err(err) => {
                eprintln!("vale-app: the heightmap of the project: {err}");
                None
            }
        });
        app::run_window(state, (1280.0, 800.0), None, session)
    });
    if let Err(err) = result {
        eprintln!("vale-app: {err:#}");
    }
}
