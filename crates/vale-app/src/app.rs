//! The eframe wrapper and file dialogs.

use eframe::egui;

use crate::ui::{self, Action, AppState, ExportFormat};

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
                Action::OpenDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("GeoJSON", &["geojson", "json"])
                        .pick_files();
                    if let Some(paths) = picked {
                        self.state.run_action(Action::OpenFiles(paths));
                    }
                }
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
    eframe::run_native("Vale", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}
