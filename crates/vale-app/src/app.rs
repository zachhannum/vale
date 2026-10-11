//! The eframe wrapper and file dialogs.

use eframe::egui;

use crate::files::PickQueue;
use crate::session::Session;
#[cfg(not(target_os = "ios"))]
use crate::ui::ExportFormat;
use crate::ui::{self, Action, AppState};

struct ValeApp {
    state: AppState,
    smoke_frames: Option<u64>,
    closing: bool,
    session: Option<Session>,
    /// The files that the file picker of the system gave.
    picked: PickQueue,
    #[cfg(target_os = "ios")]
    picker: Option<crate::files::uikit::Picker>,
}

impl eframe::App for ValeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::draw(ui, &mut self.state);

        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        let paths = dropped.into_iter().map(|f| f.path().to_path_buf());
        let (heightmaps, paths): (Vec<_>, Vec<_>) = paths.partition(|p| ui::is_heightmap_path(p));
        if !paths.is_empty() {
            self.state.run_action(Action::OpenFiles(paths));
        }
        // One image fills the globe, so the last one wins.
        if let Some(path) = heightmaps.into_iter().next_back() {
            self.state.run_action(Action::ImportHeightmap(path));
        }

        for path in self.picked.take() {
            self.state.run_action(Action::ImportHeightmap(path));
        }

        for action in std::mem::take(&mut self.state.actions) {
            match action {
                #[cfg(target_os = "ios")]
                Action::ImportHeightmapDialog => {
                    if let Some(picker) = &self.picker {
                        picker.pick_heightmap();
                    }
                }
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
                #[cfg(not(target_os = "ios"))]
                Action::ImportHeightmapDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("Heightmap", &["png", "tif", "tiff"])
                        .pick_file();
                    if let Some(path) = picked {
                        self.state.run_action(Action::ImportHeightmap(path));
                    }
                }
                other => self.state.run_action(other),
            }
        }

        if let Some(session) = &mut self.session {
            match session.tick(&self.state.doc.project, std::time::Instant::now()) {
                Ok(Some(wait)) => ui.ctx().request_repaint_after(wait),
                Ok(None) => {}
                Err(e) => self.state.status = format!("The project did not save: {e}"),
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

    fn on_exit(&mut self) {
        if let Some(session) = &mut self.session
            && let Err(e) = session.flush(&self.state.doc.project)
        {
            eprintln!("vale-app: the project did not save: {e}");
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
    session: Option<Session>,
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
        session,
        picked: PickQueue::default(),
        #[cfg(target_os = "ios")]
        picker: None,
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
                    let (picked, ctx) = (app.picked.clone(), cc.egui_ctx.clone());
                    app.picker = handle.ui_view_controller.and_then(|controller| {
                        let wake = Box::new(move || ctx.request_repaint());
                        crate::files::uikit::Picker::new(controller, picked, wake)
                    });
                }
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
