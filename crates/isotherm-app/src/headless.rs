//! Runs the app without a window, for tests and screenshots.

use eframe::egui;
use egui_kittest::Harness;

use crate::app::{IsothermApp, Layout, setup};
use crate::cli::Args;

/// The pixels for each point of a screenshot.
pub const PIXELS_PER_POINT: f32 = 2.0;

/// A harness of the pad layout with no renderer. The globe draws nothing.
pub fn harness(size: (f32, f32)) -> Harness<'static, IsothermApp> {
    harness_with(
        size,
        IsothermApp::sample(Layout::Pad).expect("the sample world reads"),
    )
}

/// A harness with no renderer that runs `app`.
pub fn harness_with(size: (f32, f32), app: IsothermApp) -> Harness<'static, IsothermApp> {
    Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .build_ui_state(draw, app)
}

/// A harness of the pad layout that renders on the GPU, with the globe.
pub fn gpu_harness(
    size: (f32, f32),
    safe_area: Option<egui::SafeAreaInsets>,
) -> Harness<'static, IsothermApp> {
    let mut app = IsothermApp::sample(Layout::Pad).expect("the sample world reads");
    app.safe_area = safe_area;
    gpu_harness_with(size, app)
}

/// A harness that renders `app` on the GPU.
pub fn gpu_harness_with(size: (f32, f32), mut app: IsothermApp) -> Harness<'static, IsothermApp> {
    // Each frame of an offscreen render draws all passes.
    vale_globe::compile::wait_in_frames();
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    app.globe.attach(&render_state);
    Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(PIXELS_PER_POINT)
        .renderer(vale_shell::headless::ScreenRenderer::new(render_state))
        .build_ui_state(draw, app)
}

fn draw(ui: &mut egui::Ui, app: &mut IsothermApp) {
    if ui.ctx().cumulative_frame_nr() == 0 {
        setup(ui.ctx());
    }
    app.draw(ui);
}

/// Renders a harness to an image. The texels of the globe go to the GPU in
/// the first render, so the second render shows them.
pub fn render(harness: &mut Harness<'_, IsothermApp>) -> Result<image::RgbaImage, String> {
    harness.run_steps(4);
    let render = |h: &mut Harness<'_, IsothermApp>| {
        h.render()
            .map_err(|e| format!("cannot render the UI (no GPU adapter?): {e}"))
    };
    render(harness)?;
    harness.step();
    render(harness)
}

/// Renders the UI that the arguments ask for to an image.
pub fn screenshot(args: &Args) -> Result<image::RgbaImage, String> {
    let app = IsothermApp::from_args(args, None)?;
    let mut harness = gpu_harness_with(args.size, app);
    render(&mut harness)
}
