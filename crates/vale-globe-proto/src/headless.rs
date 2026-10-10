//! Runs without a window: a fixed painting for a screenshot, and a brush timer.

use std::time::Instant;

use eframe::egui;

use vale_terrain::{Heightmap, Mode, Stamp};

use crate::app::{ProtoApp, Source, Tool};
use crate::math::{V3, lonlat_to_dir, slerp};

/// Paints one stroke through places given as longitude, latitude, and flow.
fn paint(app: &mut ProtoApp, radius: f64, path: &[(f64, f64, f64)]) {
    app.begin_stroke(Source::Pen, 0.0);
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (da, db) = (lonlat_to_dir(a.0, a.1), lonlat_to_dir(b.0, b.1));
        // Short steps, as a pen gives them.
        for i in 0..=24 {
            let t = f64::from(i) / 24.0;
            let dir: V3 = slerp(da, db, t);
            app.add_sample(Some(dir), a.2 + (b.2 - a.2) * t, radius);
        }
    }
    app.end_stroke(1.0);
}

/// Paints a fixed scene. The scene puts land on the north pole and across
/// cube face edges, the two places where a flat heightmap goes wrong.
pub fn paint_demo(app: &mut ProtoApp) {
    app.tool = Tool::Paint;
    app.brush.mode = Mode::Raise;
    // A continent that crosses the face edge at longitude 45.
    for _ in 0..3 {
        paint(
            app,
            0.16,
            &[
                (5.0, 10.0, 0.3),
                (30.0, 22.0, 0.9),
                (55.0, 30.0, 1.0),
                (80.0, 18.0, 0.4),
            ],
        );
    }
    paint(app, 0.09, &[(35.0, 26.0, 1.0), (52.0, 31.0, 1.0)]);
    paint(app, 0.05, &[(40.0, 28.0, 1.0), (48.0, 30.0, 1.0)]);
    // A pressure ramp: one stroke from light to hard.
    paint(app, 0.07, &[(-40.0, 0.0, 0.05), (0.0, -12.0, 1.0)]);
    paint(app, 0.07, &[(-40.0, 0.0, 0.05), (0.0, -12.0, 1.0)]);
    // A round island on the north pole, and a ring of the same dots around it.
    for _ in 0..4 {
        paint(app, 0.10, &[(0.0, 90.0, 1.0), (0.0, 89.9, 1.0)]);
    }
    for lon in (-180..180).step_by(45) {
        for _ in 0..4 {
            let lon = f64::from(lon);
            paint(app, 0.06, &[(lon, 68.0, 1.0), (lon, 67.9, 1.0)]);
        }
    }
    // A river, drawn with jitter that the line tool must remove.
    app.tool = Tool::Line;
    app.begin_stroke(Source::Pen, 0.0);
    for i in 0..=120 {
        let t = f64::from(i) / 120.0;
        let wobble = (t * 9.0).sin() * 3.0 + if i % 2 == 0 { 0.15 } else { -0.15 };
        let dir = lonlat_to_dir(52.0 - t * 30.0, 31.0 - t * 16.0 + wobble);
        app.add_sample(Some(dir), 0.5, 0.05);
    }
    app.end_stroke(1.0);
    app.tool = Tool::Paint;
}

/// Renders the UI with the fixed scene to an image.
pub fn screenshot(face_size: usize, size: (f32, f32)) -> Result<image::RgbaImage, String> {
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut app = ProtoApp::new(render_state.target_format, face_size);
    app.view = crate::view::GlobeView::centered(25.0, 48.0);
    paint_demo(&mut app);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(2.0)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(
            render_state,
        ))
        .build_ui_state(
            |ui, app: &mut ProtoApp| {
                crate::app::apply_touch_style(ui.ctx());
                app.draw(ui);
            },
            app,
        );
    harness.run_steps(4);
    harness.render().map_err(|e| format!("cannot render: {e}"))
}

/// Times the brush on the CPU, for three brush sizes.
pub fn bench(face_size: usize) -> String {
    let mut map = Heightmap::new(face_size, 20000);
    let mut out = format!("face size {face_size}\n");
    for radius in [0.01, 0.05, 0.2] {
        let stamps = 400;
        let start = Instant::now();
        let mut texels = 0;
        for i in 0..stamps {
            texels += map.stamp(&Stamp {
                center: lonlat_to_dir(f64::from(i) * 0.9, f64::from(i) * 0.4 - 80.0),
                radius,
                hardness: 0.3,
                flow: 0.5,
                mode: Mode::Raise,
                level: 0,
                strength: 20.0,
            });
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(stamps);
        out.push_str(&format!(
            "radius {radius:.2} rad: {ms:.3} ms per stamp, {} texels per stamp\n",
            texels / stamps as usize
        ));
    }
    out
}
