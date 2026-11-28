//! The Isotherm app: the workspaces, the open world, and the globe.

pub mod app;
pub mod build;
pub mod cli;
pub mod document;
pub mod explore;
pub mod globe;
#[cfg(not(target_os = "ios"))]
pub mod headless;
pub mod memory;
pub mod resume;
pub mod sim;
pub mod thumbnail;
pub mod worlds;

use eframe::egui;

/// Opens the window and runs until it closes.
pub fn run(args: cli::Args) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("Isotherm"),
        ..Default::default()
    };
    eframe::run_native(
        "Isotherm",
        options,
        Box::new(move |cc| {
            let state = worlds::state_dir();
            let mut app = app::IsothermApp::from_args(&args, state.as_deref())?;
            app.data_dir = worlds::default_dir();
            app.set_threaded();
            if let Some(render_state) = &cc.wgpu_render_state {
                app.globe.attach(render_state);
            }
            app::setup(&cc.egui_ctx);
            #[cfg(target_os = "ios")]
            {
                use eframe::wgpu::rwh::{HasWindowHandle as _, RawWindowHandle};
                let handle = cc.window_handle().map(|handle| handle.as_raw());
                if let Ok(RawWindowHandle::UiKit(handle)) = handle {
                    let (picked, ctx) = (app.picked.clone(), cc.egui_ctx.clone());
                    let exports = vale_shell::picker::ExportQueue::default();
                    app.picker = handle.ui_view_controller.and_then(|controller| {
                        let wake = move || ctx.request_repaint();
                        vale_shell::picker::Picker::new(controller, picked, exports, wake)
                    });
                }
                // The system stops the frames in the background, and the run
                // goes on when they come back. The save runs at two
                // notifications, so a frame that holds the run at the first
                // one does not stop the save.
                let run = app.run.clone();
                let save = Box::new(move || {
                    if let Ok(mut run) = run.try_borrow_mut()
                        && let Err(e) = run.save()
                    {
                        eprintln!("isotherm-app: {e}");
                    }
                });
                app.lifecycle = vale_shell::lifecycle::Observer::new(save);
            }
            Ok(Box::new(app))
        }),
    )
}

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn isotherm_app_main() {
    if let Err(err) = run(cli::Args::none()) {
        eprintln!("isotherm-app: {err}");
    }
}
