//! A prototype that tests the riskiest part of the Vale design: pen drawing
//! on a globe, in egui, on desktop and on iPad.
//!
//! The crate does not use the other Vale crates. `README.md` in this crate
//! lists what the prototype tests and what it leaves out.

pub mod app;
pub mod cube;
pub mod gpu;
#[cfg(not(target_os = "ios"))]
pub mod headless;
pub mod lines;
pub mod math;
pub mod view;

use eframe::egui;

/// Opens the window and runs until it closes.
pub fn run(face_size: usize) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 820.0])
            .with_title("Vale globe prototype"),
        ..Default::default()
    };
    eframe::run_native(
        "Vale globe prototype",
        options,
        Box::new(move |cc| {
            let format = cc
                .wgpu_render_state
                .as_ref()
                .map(|state| state.target_format)
                .ok_or("the prototype needs the wgpu renderer")?;
            app::apply_touch_style(&cc.egui_ctx);
            Ok(Box::new(app::ProtoApp::new(format, face_size)))
        }),
    )
}

/// The entry point that `ios/main.m` calls.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn vale_globe_proto_main() {
    if let Err(err) = run(app::DEFAULT_FACE_SIZE) {
        eprintln!("vale-globe-proto: {err}");
    }
}
