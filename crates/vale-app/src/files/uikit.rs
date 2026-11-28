//! The file types of the picks of Vale in the Files app.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;

use super::{ExportQueue, PickQueue};

/// The picker of the Files app, with the file types of Vale.
pub struct Picker(vale_shell::picker::Picker);

impl Picker {
    /// See [`vale_shell::picker::Picker::new`].
    pub fn new(
        controller: NonNull<c_void>,
        queue: PickQueue,
        exports: ExportQueue,
        wake: impl Fn() + Clone + 'static,
    ) -> Option<Picker> {
        vale_shell::picker::Picker::new(controller, queue, exports, wake).map(Picker)
    }

    /// Shows the PNG and TIFF files. Returns `false` if the Files app cannot
    /// show now.
    pub fn pick_heightmap(&self) -> bool {
        self.0.pick(&["public.png", "public.tiff"])
    }

    /// Shows the PNG, TIFF, and Photoshop files, for an image layer.
    pub fn pick_layer_image(&self) -> bool {
        self.0
            .pick(&["public.png", "public.tiff", "com.adobe.photoshop-image"])
    }

    /// Shows the GeoJSON and JSON files.
    pub fn pick_geojson(&self) -> bool {
        self.0.pick(&["public.geojson", "public.json"])
    }

    /// Shows the PNG and JPEG files.
    pub fn pick_image(&self) -> bool {
        self.0.pick(&["public.png", "public.jpeg"])
    }

    /// Shows the SVG files.
    pub fn pick_svg(&self) -> bool {
        self.0.pick(&["public.svg-image"])
    }

    /// See [`vale_shell::picker::Picker::export_file`].
    pub fn export_file(&self, paths: &[PathBuf]) -> bool {
        self.0.export_file(paths)
    }
}
