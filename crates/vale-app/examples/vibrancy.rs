//! A test window for the glass of macOS. It has the translucent places of
//! the desktop layout, and an opaque disc on the canvas.
//!
//! ```sh
//! cargo run -p vale-app --example vibrancy
//! ```

#[cfg(target_os = "macos")]
fn main() -> eframe::Result {
    use eframe::egui::{self, Color32};
    use vale_app::app::{apply_glass, window_options};

    struct Test {
        glass: bool,
    }

    impl eframe::App for Test {
        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
            if self.glass {
                [0.0; 4]
            } else {
                Color32::from_rgb(12, 12, 12).to_normalized_gamma_f32()
            }
        }

        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let glass = Color32::from_rgba_unmultiplied(30, 33, 38, 153);
            let canvas = Color32::from_rgba_unmultiplied(10, 12, 15, 204);
            let panel = |fill| {
                egui::Frame::NONE
                    .fill(fill)
                    .inner_margin(egui::Margin::same(12))
            };
            egui::Panel::top("bar")
                .exact_size(52.0)
                .frame(panel(glass))
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(70.0);
                        let text = if self.glass {
                            "apply_glass: Ok"
                        } else {
                            "apply_glass: failed"
                        };
                        ui.label(text);
                    });
                });
            egui::Panel::left("rail")
                .exact_size(44.0)
                .frame(panel(glass))
                .show(ui, |_| {});
            egui::Panel::right("dock")
                .exact_size(300.0)
                .frame(panel(glass))
                .show(ui, |ui| {
                    ui.label("Dock");
                });
            egui::CentralPanel::default()
                .frame(panel(canvas))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    let radius = rect.width().min(rect.height()) * 0.3;
                    let blue = Color32::from_rgb(64, 96, 140);
                    ui.painter().circle_filled(rect.center(), radius, blue);
                });
        }
    }

    eframe::run_native(
        "Vibrancy test",
        window_options((1200.0, 760.0)),
        Box::new(|cc| {
            let glass = apply_glass(cc);
            Ok(Box::new(Test { glass }))
        }),
    )
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("The test window needs macOS.");
}
