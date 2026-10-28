//! The eframe wrapper and file dialogs.

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;

use crate::files::{ExportQueue, PickQueue};
#[cfg(not(target_os = "ios"))]
use crate::projects::Location;
#[cfg(not(target_os = "ios"))]
use crate::session::EXTENSION;
use crate::session::Session;
use crate::ui::ExportFormat;
use crate::ui::{self, Action, AppState};

/// What a file from the file picker of the system is for.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
enum PickFor {
    #[default]
    Heightmap,
    /// The colors of the file go into the ramps of this raster layer.
    Ramp(vale_store::RasterId),
    /// The image of the form of a new raster layer.
    RasterImage,
}

struct ValeApp {
    /// The observer of the life cycle of the app has the state too.
    state: Rc<RefCell<AppState>>,
    smoke_frames: Option<u64>,
    closing: bool,
    /// The files that the file picker of the system gave.
    picked: PickQueue,
    /// What the picked file is for.
    pick_for: PickFor,
    /// The ends of the hand-offs of export files to the Files app.
    exports: ExportQueue,
    /// The export file that the Files app shows.
    sharing: Option<std::path::PathBuf>,
    #[cfg(target_os = "ios")]
    picker: Option<crate::files::uikit::Picker>,
    #[cfg(target_os = "ios")]
    _lifecycle: Option<crate::lifecycle::Observer>,
}

impl eframe::App for ValeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = &mut *self.state.borrow_mut();
        state.poll_system();
        ui::draw(ui, state);

        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        let paths = dropped.into_iter().map(|f| f.path().to_path_buf());
        // The start screen has no world that takes a file.
        let paths = paths.filter(|_| state.screen == ui::Screen::Editor);
        let (heightmaps, paths): (Vec<_>, Vec<_>) = paths.partition(|p| ui::is_heightmap_path(p));
        if !paths.is_empty() {
            state.run_action(Action::OpenFiles(paths));
        }
        // One image fills the globe, so the last one wins. The form of a
        // new raster layer takes it when the form reads a file.
        if let Some(path) = heightmaps.into_iter().next_back() {
            let form = state.raster_form.as_ref();
            if form.is_some_and(|form| form.from_file) {
                state.run_action(Action::RasterImage(path));
            } else {
                state.run_action(Action::ImportHeightmap(path));
            }
        }

        for path in self.picked.take() {
            match std::mem::take(&mut self.pick_for) {
                PickFor::Heightmap => state.run_action(Action::ImportHeightmap(path)),
                PickFor::Ramp(id) => state.run_action(Action::ImportRamp(id, path)),
                PickFor::RasterImage => state.run_action(Action::RasterImage(path)),
            }
        }

        for event in self.exports.take() {
            if let Some(path) = self.sharing.take() {
                state.end_share(event, &path);
            }
        }
        #[cfg(target_os = "ios")]
        {
            let waits = if self.sharing.is_none() {
                state.share.take()
            } else {
                None
            };
            if let Some(path) = waits {
                match &self.picker {
                    Some(picker) if picker.export_file(&path) => self.sharing = Some(path),
                    // Another picker shows. A later frame asks again.
                    Some(_) => {
                        state.share = Some(path);
                        let wait = std::time::Duration::from_millis(500);
                        ui.ctx().request_repaint_after(wait);
                    }
                    None => state.end_share(crate::files::ExportEvent::Cancelled, &path),
                }
            }
        }

        for action in std::mem::take(&mut state.actions) {
            match action {
                #[cfg(target_os = "ios")]
                Action::ImportHeightmapDialog => {
                    if let Some(picker) = &self.picker
                        && picker.pick_heightmap()
                    {
                        self.pick_for = PickFor::Heightmap;
                    }
                }
                #[cfg(target_os = "ios")]
                Action::RasterImageDialog => {
                    if let Some(picker) = &self.picker
                        && picker.pick_heightmap()
                    {
                        self.pick_for = PickFor::RasterImage;
                    }
                }
                #[cfg(target_os = "ios")]
                Action::ImportRampDialog(id) => {
                    if let Some(picker) = &self.picker
                        && picker.pick_image()
                    {
                        self.pick_for = PickFor::Ramp(id);
                    }
                }
                // The export goes to a temporary file. The Files app moves it
                // after the export is complete.
                #[cfg(target_os = "ios")]
                Action::ExportHeightmapDialog | Action::ExportHeightmapPngDialog
                    if self.sharing.is_none() =>
                {
                    crate::files::clear_exports();
                    let ext = if action == Action::ExportHeightmapDialog {
                        "tif"
                    } else {
                        "png"
                    };
                    let path = crate::files::export_dir().join(state.export_name(ext));
                    state.run_action(Action::ExportHeightmap(path));
                }
                #[cfg(target_os = "ios")]
                Action::ExportDialog(format) if self.sharing.is_none() => {
                    crate::files::clear_exports();
                    let (index, _) = crate::ui::atlas::page_index(state);
                    let (name, atlas) = match format {
                        ExportFormat::Png => (format!("page-{}.png", index + 1), false),
                        ExportFormat::Pdf => (format!("page-{}.pdf", index + 1), false),
                        ExportFormat::AtlasPdf => ("atlas.pdf".to_string(), true),
                    };
                    let path = crate::files::export_dir().join(name);
                    state.run_action(if atlas {
                        Action::ExportAtlas(path)
                    } else {
                        Action::Export(path)
                    });
                }
                #[cfg(not(target_os = "ios"))]
                Action::ExportHeightmapDialog | Action::ExportHeightmapPngDialog => {
                    let png = action == Action::ExportHeightmapPngDialog;
                    let dialog = rfd::FileDialog::new().set_file_name(state.export_name(if png {
                        "png"
                    } else {
                        "tif"
                    }));
                    let picked = if png {
                        dialog.add_filter("PNG", &["png"])
                    } else {
                        dialog.add_filter("GeoTIFF", &["tif", "tiff"])
                    }
                    .save_file();
                    if let Some(path) = picked {
                        state.run_action(Action::ExportHeightmap(path));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::OpenDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("GeoJSON", &["geojson", "json"])
                        .pick_files();
                    if let Some(paths) = picked {
                        state.run_action(Action::OpenFiles(paths));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::ExportDialog(format) => {
                    let (index, _) = crate::ui::atlas::page_index(state);
                    let (name, atlas) = match format {
                        ExportFormat::Png => (format!("page-{}.png", index + 1), false),
                        ExportFormat::Pdf => (format!("page-{}.pdf", index + 1), false),
                        ExportFormat::AtlasPdf => ("atlas.pdf".to_string(), true),
                    };
                    if let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() {
                        state.run_action(if atlas {
                            Action::ExportAtlas(path)
                        } else {
                            Action::Export(path)
                        });
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::ImportHeightmapDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("Heightmap", &["png", "tif", "tiff"])
                        .pick_file();
                    if let Some(path) = picked {
                        state.run_action(Action::ImportHeightmap(path));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::RasterImageDialog => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "tif", "tiff"])
                        .pick_file();
                    if let Some(path) = picked {
                        state.run_action(Action::RasterImage(path));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::ImportRampDialog(id) => {
                    let picked = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg"])
                        .pick_file();
                    if let Some(path) = picked {
                        state.run_action(Action::ImportRamp(id, path));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                Action::OpenProjectDialog => {
                    let mut dialog =
                        rfd::FileDialog::new().add_filter("Vale project", &[EXTENSION]);
                    if let Some(dir) = state.projects.dir(Location::Local) {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(path) = dialog.pick_file() {
                        state.run_action(Action::OpenWorld(path));
                    }
                }
                other => state.run_action(other),
            }
        }

        let save = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        if ui.ctx().input_mut(|i| i.consume_shortcut(&save)) {
            state.run_action(Action::Save);
        }

        // One drag of a control is one save, and thus one undo step.
        let hold = ui.ctx().input(|i| i.pointer.any_down());
        if let Some(wait) = state.save_tick(hold, std::time::Instant::now()) {
            ui.ctx().request_repaint_after(wait);
        }

        if let Some(n) = self.smoke_frames {
            ui.ctx().request_repaint();
            if state.frames >= n && !self.closing {
                self.closing = true;
                println!("smoke: {} frames", state.frames);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn on_exit(&mut self) {
        let state = &mut *self.state.borrow_mut();
        state.save_resume();
        if !state.save_now() {
            eprintln!("vale-app: {}", state.status);
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

/// Opens the window and runs until it closes. With `restore`, the window
/// opens on the project and the view of the last run.
pub fn run_window(
    state: AppState,
    size: (f64, f64),
    smoke_frames: Option<u64>,
    session: Option<Session>,
    restore: bool,
) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([size.0 as f32, size.1 as f32])
            .with_title("Vale"),
        ..Default::default()
    };
    let mut state = state;
    state.session = session;
    let app = ValeApp {
        state: Rc::new(RefCell::new(state)),
        smoke_frames,
        closing: false,
        picked: PickQueue::default(),
        pick_for: PickFor::default(),
        exports: ExportQueue::default(),
        sharing: None,
        #[cfg(target_os = "ios")]
        picker: None,
        #[cfg(target_os = "ios")]
        _lifecycle: None,
    };
    eframe::run_native(
        "Vale",
        options,
        Box::new(|cc| {
            #[cfg_attr(not(target_os = "ios"), expect(unused_mut))]
            let mut app = app;
            {
                let state = &mut *app.state.borrow_mut();
                if let Some(render_state) = &cc.wgpu_render_state {
                    state.globe.attach(render_state);
                }
                // The renderer gives the largest face size, so it comes first.
                if restore {
                    state.restore();
                }
                // The renderer can take away a heightmap that is too large.
                state.sync_globe();
                if state.session.is_some() {
                    state.note_lost_rasters();
                }
                if cfg!(target_os = "ios") {
                    apply_touch_style(&cc.egui_ctx);
                }
            }
            let ctx = cc.egui_ctx.clone();
            let system = app.state.borrow().system.clone();
            system.set_wake(Box::new(move || ctx.request_repaint()));
            #[cfg(target_os = "ios")]
            {
                use eframe::wgpu::rwh::{HasWindowHandle as _, RawWindowHandle};
                let handle = cc.window_handle().map(|handle| handle.as_raw());
                if let Ok(RawWindowHandle::UiKit(handle)) = handle {
                    let queue = crate::pen::PenQueue::default();
                    app.state.borrow_mut().globe.pen.queue = Some(queue.clone());
                    crate::pen::uikit::install(handle.ui_view, cc.egui_ctx.clone(), queue);
                    let (picked, ctx) = (app.picked.clone(), cc.egui_ctx.clone());
                    let exports = app.exports.clone();
                    app.picker = handle.ui_view_controller.and_then(|controller| {
                        let wake = move || ctx.request_repaint();
                        crate::files::uikit::Picker::new(controller, picked, exports, wake)
                    });
                }
                let shared = app.state.clone();
                // A frame can hold the state. The frame then saves.
                let save = Box::new(move || {
                    if let Ok(mut state) = shared.try_borrow_mut()
                        && let Err(e) = state.save_waiting()
                    {
                        state.status = format!("Could not save project: {e}");
                    }
                });
                app._lifecycle = crate::lifecycle::Observer::new(save);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
