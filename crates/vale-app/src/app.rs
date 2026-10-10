//! The eframe wrapper and file dialogs.

use eframe::egui;

#[cfg(not(target_os = "ios"))]
use crate::ui::ExportFormat;
use crate::ui::{self, Action, AppState};

struct ValeApp {
    state: AppState,
    smoke_frames: Option<u64>,
    closing: bool,
}

impl eframe::App for ValeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::draw(ui, &mut self.state);

        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        let paths: Vec<_> = dropped
            .into_iter()
            .map(|f| f.path().to_path_buf())
            .collect();
        if !paths.is_empty() {
            self.state.run_action(Action::OpenFiles(paths));
        }

        for action in std::mem::take(&mut self.state.actions) {
            match action {
                #[cfg(not(target_os = "ios"))]
                Action::OpenDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("GeoJSON", &["geojson", "json"])
                        .pick_files();
                    if let Some(paths) = picked {
                        self.state.run_action(Action::OpenFiles(paths));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::ExportDialog(format) => {
                    let name = match format {
                        ExportFormat::Png => "map.png",
                        ExportFormat::Pdf => "map.pdf",
                    };
                    if let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() {
                        self.state.run_action(Action::Export(path));
                    }
                }
                other => self.state.run_action(other),
            }
        }

        if let Some(n) = self.smoke_frames {
            ui.ctx().request_repaint();
            if self.state.frames >= n && !self.closing {
                self.closing = true;
                println!("smoke: {} frames", self.state.frames);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// Makes the controls large enough for a finger.
pub fn apply_touch_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.interact_size = egui::vec2(48.0, 38.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.icon_width = 22.0;
        for (text_style, font) in &mut style.text_styles {
            font.size = match text_style {
                egui::TextStyle::Heading => 19.0,
                egui::TextStyle::Small => 12.0,
                egui::TextStyle::Monospace => 13.0,
                _ => 15.0,
            };
        }
    });
}

/// Opens the window and runs until it closes.
pub fn run_window(
    state: AppState,
    size: (f64, f64),
    smoke_frames: Option<u64>,
) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([size.0 as f32, size.1 as f32])
            .with_title("Vale"),
        ..Default::default()
    };
    let app = ValeApp {
        state,
        smoke_frames,
        closing: false,
    };
    eframe::run_native(
        "Vale",
        options,
        Box::new(|cc| {
            let mut app = app;
            if let Some(render_state) = &cc.wgpu_render_state {
                app.state.globe.attach(render_state);
            }
            if cfg!(target_os = "ios") {
                apply_touch_style(&cc.egui_ctx);
            }
            #[cfg(target_os = "ios")]
            {
                use eframe::wgpu::rwh::{HasWindowHandle as _, RawWindowHandle};
                let handle = cc.window_handle().map(|handle| handle.as_raw());
                if let Ok(RawWindowHandle::UiKit(handle)) = handle {
                    let queue = crate::pen::PenQueue::default();
                    app.state.globe.pen.queue = Some(queue.clone());
                    crate::pen::uikit::install(handle.ui_view, cc.egui_ctx.clone(), queue);
                }
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
