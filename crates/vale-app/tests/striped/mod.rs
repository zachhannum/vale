//! A project file with a raster layer that is not selected and that a map
//! frame zooms in on.

use std::path::Path;

use vale_app::document::Document;
use vale_app::session::Session;
use vale_app::ui::{AppState, LayerRef, Layout, RasterSpec, Workspace};
use vale_store::{FrameId, RasterDisplay, RasterId};
use vale_terrain::math::lonlat_to_dir;

/// Texels along one side of a face of the layer.
pub const FACE: usize = 8192;

/// The levels of the high stripes and of the low stripes.
pub const HIGH: u16 = 50000;
pub const LOW: u16 = 15000;

/// The center of the frame, away from the lines of the graticule.
pub const CENTER: [f64; 2] = [10.2, 20.3];

/// An Atlas workspace with a project file at `path`. A grey height layer
/// of `FACE` texels has stripes of two texels around `CENTER`, which a
/// copy with a quarter of the face makes one grey. The stripes change
/// places in each band of 64 rows of texels. The file holds
/// its tiles, and the main memory does not. The brush paints another
/// layer, which the frame does not show. The frame of the page shows
/// `CENTER` at `scale`. At a scale of 400,000, a texel is about 3
/// millimeters on the page.
pub fn project(path: &Path, scale: f64) -> (AppState, RasterId, FrameId) {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.session = Some(Session::open(path, &mut state.doc).unwrap());
    state.set_layout(Layout::Pad);
    state.workspace = Workspace::Atlas;
    vale_app::ui::atlas::add_frame(&mut state).expect("a page takes a frame");
    let height = state
        .add_raster(RasterSpec::height("Height", FACE))
        .unwrap();
    state.set_raster_display(height, RasterDisplay::Grey);
    state.set_raster_smoothing(height, false);
    state.set_raster_hillshade(height, false);

    let (face, a, b) = vale_terrain::face_of(lonlat_to_dir(CENTER[0], CENTER[1]));
    let texel = |s: f64| ((vale_terrain::warp(s) + 1.0) / 2.0 * FACE as f64) as usize;
    let (cx, cy) = (texel(a), texel(b));
    let map = state.globe.raster_map_mut(height).expect("the brush layer");
    for y in cy - 600..cy + 600 {
        for x in cx - 600..cx + 600 {
            let level = if (x / 2 + y / 64) % 2 == 0 { HIGH } else { LOW };
            map.set(face, x, y, level);
        }
    }
    state.save_waiting().unwrap();

    let other = state.add_raster(RasterSpec::height("Other", FACE)).unwrap();
    state.select(Some(LayerRef::Raster(other)));
    state.pump_layers(true);
    assert_eq!(state.globe.raster(), Some(other));
    assert!(
        state.globe.raster_map(height).is_none(),
        "in the main memory"
    );
    let map = vale_app::ui::atlas::open_page(&state).frames[0].map;
    let layer = state.doc.project.raster_layer(other).unwrap().id;
    assert!(state.doc.set_shown(map, layer, false));

    let id = vale_app::ui::atlas::open_page(&state).frames[0].id;
    let frame = state.doc.project.atlas.frame_mut(id).unwrap();
    frame.center = CENTER;
    frame.scale = scale;
    state.touch();
    (state, height, id)
}
