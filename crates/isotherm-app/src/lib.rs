//! The Isotherm app: the workspace switch and the globe with the sample
//! world.

pub mod app;
pub mod build;
pub mod cli;
pub mod globe;
#[cfg(not(target_os = "ios"))]
pub mod headless;

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
        Box::new(|cc| {
            let mut app = app::IsothermApp::new()?;
            app.workspace = args.workspace;
            if let Some(render_state) = &cc.wgpu_render_state {
                app.globe.attach(render_state);
            }
            app::setup(&cc.egui_ctx);
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
