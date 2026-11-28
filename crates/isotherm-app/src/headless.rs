//! Runs the app without a window, for tests and screenshots.

use eframe::egui;
use egui_kittest::Harness;

use crate::app::{IsothermApp, setup};

/// The pixels for each point of a screenshot.
pub const PIXELS_PER_POINT: f32 = 2.0;

/// A harness with no renderer. The globe draws nothing.
pub fn harness(size: (f32, f32)) -> Harness<'static, IsothermApp> {
    let app = IsothermApp::new().expect("the sample world reads");
    Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .build_ui_state(draw, app)
}

/// A harness that renders on the GPU, with the globe.
pub fn gpu_harness(
    size: (f32, f32),
    safe_area: Option<egui::SafeAreaInsets>,
) -> Harness<'static, IsothermApp> {
    // Each frame of an offscreen render draws all passes.
    vale_globe::compile::wait_in_frames();
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut app = IsothermApp::new().expect("the sample world reads");
    app.globe.attach(&render_state);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(PIXELS_PER_POINT)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(
            render_state,
        ))
        .build_ui_state(draw, app);
    // The renderer of the harness writes only the safe area to the image,
    // so the app takes the safe area and the image keeps the whole screen.
    harness.state_mut().safe_area = safe_area;
    harness
}

fn draw(ui: &mut egui::Ui, app: &mut IsothermApp) {
    if ui.ctx().cumulative_frame_nr() == 0 {
        setup(ui.ctx());
    }
    app.draw(ui);
}

/// Renders the UI to an image. The texels of the globe go to the GPU in
/// the first render, so the second render shows them.
pub fn screenshot(
    size: (f32, f32),
    safe_area: Option<egui::SafeAreaInsets>,
) -> Result<image::RgbaImage, String> {
    let mut harness = gpu_harness(size, safe_area);
    harness.run_steps(4);
    let render = |h: &mut Harness<'_, IsothermApp>| {
        h.render()
            .map_err(|e| format!("cannot render the UI (no GPU adapter?): {e}"))
    };
    render(&mut harness)?;
    harness.step();
    render(&mut harness)
}
