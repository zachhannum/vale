//! The tests of the tiles of the full texels that the frames and the view
//! of a globe need.

use std::sync::Arc;

use eframe::egui;
use vale_sphere::{Point, Projection};
use vale_store::{BlendMode, Hillshade, RasterDisplay, RasterId, Smoothing};
use vale_terrain::Heightmap;

use super::detail::{DETAIL_IN_FLIGHT, IDLE_FRAMES, VIEW, tile_of, tile_size};
use super::flat::{DEFAULT_SPEC, FlatMesh};
use super::view::GlobeView;
use super::{FrameDraw, FrameItem, FramePlace, Globe, Look, RasterLook, StackItem, WorldView};

const RASTER: RasterId = RasterId(7);
const FACE: usize = 8192;

/// A globe with one visible raster layer of 8,192 texels in a face,
/// with no smoothing. `memory`: the whole heightmap is in the main
/// memory. Else the project file holds its tiles.
fn globe(memory: bool) -> Globe {
    let mut globe = Globe::with_face_size(64);
    let map = Heightmap::new(FACE, 1000);
    if memory {
        assert!(globe.add_raster(RASTER, map, true));
    } else {
        let reduced = super::reduce_map(&map);
        assert!(globe.add_reduced(RASTER, reduced, FACE, true));
    }
    globe.set_stack(vec![StackItem::Raster {
        id: RASTER,
        opacity: 1.0,
        display: RasterDisplay::Grey,
        smoothing: Smoothing {
            on: false,
            ..Smoothing::default()
        },
        hillshade: Hillshade::default(),
        rivers: false,
        lines: false,
        blend: BlendMode::Normal,
    }]);
    globe
}

/// The look of the raster layer, as the globe draws it.
fn raster_look() -> RasterLook {
    RasterLook {
        look: Look {
            opacity: 1.0,
            blend: BlendMode::Normal,
            display: RasterDisplay::Grey,
        },
        lines: false,
        smoothing: Smoothing {
            on: false,
            ..Smoothing::default()
        },
        hillshade: Hillshade::default(),
        bands: None,
    }
}

/// A frame of `size` pixels with `pixels_per_radian` pixels in one unit
/// of the projection, centered on the point `map_center` of the map.
fn frame(id: u32, size: u32, pixels_per_radian: f32, map_center: [f32; 2]) -> FrameDraw {
    let projection = Projection::new(DEFAULT_SPEC, 1.0).unwrap();
    FrameDraw {
        id,
        place: FramePlace {
            center: [size as f32 / 2.0; 2],
            map_center,
            units_to_pixels: pixels_per_radian,
            rotation: 0.0,
            points_per_radian: f64::from(pixels_per_radian),
            pixels_per_page_point: 1.0,
        },
        rect: [0, 0, size, size],
        mesh: Arc::new(FlatMesh::new(&projection)),
        mask: None,
        layers: vec![FrameItem::Raster(RASTER, raster_look())],
        mask_looks: Vec::new(),
    }
}

#[test]
fn a_zoomed_frame_needs_the_full_texels_under_it_only() {
    let mut globe = globe(true);
    globe.want_frames(&[frame(1, 500, 20000.0, [0.0, 0.0])]);
    assert_eq!(globe.frame_detail_level(1, RASTER), Some(0));
    let tiles = globe.frame_detail_tiles(1, RASTER);
    // The middle of the map is the middle of face 0.
    let middle = tile_of([1.0, 0.0, 0.0], FACE as u32, tile_size(FACE as u32));
    assert!(tiles.contains(&middle), "{tiles:?}");
    // The frame is on the corner of four tiles, and each side has one
    // tile more.
    assert!(tiles.len() <= 16, "{tiles:?}");
    for (face, column, row) in tiles {
        assert_eq!(face, middle.0);
        assert!(column.abs_diff(middle.1) <= 2 && row.abs_diff(middle.2) <= 2);
    }
}

#[test]
fn a_frame_of_the_whole_world_reads_the_copy() {
    let mut globe = globe(true);
    let world = frame(1, 1000, 160.0, [0.0, 0.0]);
    globe.want_frames(std::slice::from_ref(&world));
    assert_eq!(globe.frame_detail_level(1, RASTER), None);
    assert!(globe.frame_detail_tiles(1, RASTER).is_empty());
    assert_eq!(globe.detail_bytes(), 0);
    assert!(globe.frame_ready(&world));
}

#[test]
fn a_frame_over_the_budget_takes_a_coarser_level() {
    let mut globe = globe(true);
    let big = frame(1, 1400, 3000.0, [0.0, 0.0]);
    globe.want_frames(std::slice::from_ref(&big));
    assert_eq!(globe.frame_detail_level(1, RASTER), Some(0));
    assert!(globe.frame_fits(1));
    let budget = 20 << 20;
    let mut globe = self::globe(true);
    globe.set_detail_budget(Some(budget));
    globe.want_frames(std::slice::from_ref(&big));
    assert_eq!(globe.frame_detail_level(1, RASTER), Some(1));
    assert!(!globe.frame_fits(1));
    for _ in 0..10 {
        globe.want_frames(std::slice::from_ref(&big));
    }
    assert!(globe.frame_ready(&big));
    assert!(globe.detail_bytes() <= budget, "{}", globe.detail_bytes());
}

#[test]
fn a_tile_that_no_frame_needs_any_more_is_cancelled() {
    let mut globe = globe(false);
    let here = frame(1, 500, 20000.0, [0.0, 0.0]);
    globe.want_frames(std::slice::from_ref(&here));
    let reqs = globe.detail_requests();
    assert!(!reqs.is_empty() && reqs.len() <= DETAIL_IN_FLIGHT);
    assert!(!globe.frame_ready(&here));
    assert!(reqs.iter().all(|r| globe.wants_detail(r)));

    // The frame moves far away.
    let there = frame(1, 500, 20000.0, [1.0, 0.5]);
    globe.want_frames(std::slice::from_ref(&there));
    assert!(reqs.iter().all(|r| !globe.wants_detail(r)));
    globe.give_detail_tile(reqs[0], None);
    assert!(!globe.frame_ready(&there));

    // The tiles of the frame arrive.
    for _ in 0..10 {
        for req in globe.detail_requests() {
            globe.give_detail_tile(req, None);
        }
        globe.want_frames(std::slice::from_ref(&there));
    }
    assert!(globe.frame_ready(&there));
    assert!(globe.detail_requests().is_empty());
}

/// Puts the view of `globe` on a canvas of 800 by 800 points, centered
/// on the middle of face 0, with `zoom`.
fn look(globe: &mut Globe, world_view: WorldView, zoom: f64, lon: f64) {
    globe.rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(800.0));
    globe.world_view = world_view;
    globe.view = GlobeView::centered(lon, 0.0);
    globe.view.zoom = zoom;
    globe.flat.center = Point::new(lon.to_radians(), 0.0);
    globe.flat.zoom = zoom;
}

/// Gives the globe each tile that the project file holds, until the
/// view has its tiles.
fn give_tiles(globe: &mut Globe) {
    for _ in 0..20 {
        for req in globe.detail_requests() {
            globe.give_detail_tile(req, None);
        }
        globe.want_view(1.0);
    }
    assert!(globe.view_details_ready());
}

#[test]
fn a_zoomed_globe_needs_the_full_texels_under_the_view_only() {
    let mut globe = globe(true);
    look(&mut globe, WorldView::Globe, 40.0, 0.0);
    globe.want_view(1.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
    assert!(globe.view_fits());
    let tiles = globe.details.tiles(VIEW, RASTER);
    let middle = tile_of([1.0, 0.0, 0.0], FACE as u32, tile_size(FACE as u32));
    assert!(tiles.contains(&middle), "{tiles:?}");
    for (face, column, row) in tiles {
        assert_eq!(face, middle.0);
        assert!(column.abs_diff(middle.1) <= 4 && row.abs_diff(middle.2) <= 4);
    }
    // The tiles come from the main memory.
    assert!(globe.view_details_ready());
}

#[test]
fn a_zoomed_flat_view_needs_the_full_texels_under_the_view_only() {
    let mut globe = globe(true);
    look(&mut globe, WorldView::Flat, 40.0, 0.0);
    globe.want_view(1.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
    let tiles = globe.details.tiles(VIEW, RASTER);
    let middle = tile_of([1.0, 0.0, 0.0], FACE as u32, tile_size(FACE as u32));
    assert!(tiles.contains(&middle), "{tiles:?}");
    assert!(tiles.len() <= 81, "{tiles:?}");
}

#[test]
fn a_view_of_the_whole_world_reads_the_copy() {
    for world_view in [WorldView::Globe, WorldView::Flat] {
        let mut globe = globe(true);
        look(&mut globe, world_view, 1.0, 0.0);
        globe.want_view(1.0);
        assert_eq!(globe.view_detail_level(RASTER), None);
        assert_eq!(globe.detail_bytes(), 0);
        assert!(globe.view_details_ready());
    }
}

#[test]
fn a_view_with_more_pixels_needs_finer_texels() {
    let mut globe = globe(true);
    look(&mut globe, WorldView::Globe, 6.0, 0.0);
    globe.want_view(1.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(1));
    globe.want_view(2.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
}

#[test]
fn a_view_over_the_budget_takes_a_coarser_level() {
    let mut globe = globe(true);
    look(&mut globe, WorldView::Globe, 6.0, 0.0);
    globe.want_view(2.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
    assert!(globe.view_fits());
    let budget = 12 << 20;
    let mut globe = self::globe(true);
    globe.set_detail_budget(Some(budget));
    look(&mut globe, WorldView::Globe, 6.0, 0.0);
    globe.want_view(2.0);
    assert_eq!(globe.view_detail_level(RASTER), Some(1));
    assert!(!globe.view_fits());
    assert!(globe.detail_bytes() <= budget, "{}", globe.detail_bytes());
}

#[test]
fn the_frames_and_the_view_keep_their_needs() {
    let mut globe = globe(true);
    look(&mut globe, WorldView::Globe, 40.0, 0.0);
    let far = frame(1, 500, 20000.0, [1.0, 0.5]);
    globe.want_frames(std::slice::from_ref(&far));
    globe.want_view(1.0);
    globe.want_frames(std::slice::from_ref(&far));
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
    assert_eq!(globe.frame_detail_level(1, RASTER), Some(0));
    assert!(globe.view_details_ready());
    assert!(globe.frame_ready(&far));

    // The frames stop asking, and the view keeps its needs.
    for _ in 0..=IDLE_FRAMES {
        globe.details.idle_frame();
        globe.want_view(1.0);
    }
    assert_eq!(globe.frame_detail_level(1, RASTER), None);
    assert_eq!(globe.view_detail_level(RASTER), Some(0));
    assert!(globe.view_details_ready());

    // The view stops asking too, and each map goes away.
    for _ in 0..=IDLE_FRAMES {
        globe.details.idle_frame();
    }
    assert_eq!(globe.view_detail_level(RASTER), None);
    assert_eq!(globe.detail_bytes(), 0);
}

#[test]
fn a_layer_below_an_opaque_layer_needs_no_tiles() {
    let mut globe = globe(true);
    let top = RasterId(8);
    assert!(globe.add_raster(top, Heightmap::new(FACE, 2000), true));
    let mut stack = globe.stack.clone().unwrap();
    let mut item = stack[0];
    if let StackItem::Raster { id, .. } = &mut item {
        *id = top;
    }
    stack.push(item);
    globe.set_stack(stack);
    look(&mut globe, WorldView::Globe, 40.0, 0.0);
    globe.want_view(1.0);
    assert_eq!(globe.view_detail_level(top), Some(0));
    assert_eq!(globe.view_detail_level(RASTER), None);
}

#[test]
fn a_small_move_keeps_the_tiles_of_the_view() {
    let mut globe = globe(false);
    look(&mut globe, WorldView::Globe, 40.0, 0.0);
    globe.want_view(1.0);
    assert!(!globe.view_details_ready());
    assert_eq!(globe.details.source(VIEW, RASTER), None);
    give_tiles(&mut globe);
    let key = globe.details.source(VIEW, RASTER);
    assert!(key.is_some());

    // A move of less than one tile asks for new tiles of the margin,
    // and the view keeps its map.
    look(&mut globe, WorldView::Globe, 40.0, 2.0);
    globe.want_view(1.0);
    assert!(!globe.view_details_ready());
    assert_eq!(globe.details.source(VIEW, RASTER), key);

    // A long move draws the copy until the tiles are there.
    look(&mut globe, WorldView::Globe, 40.0, 20.0);
    globe.want_view(1.0);
    assert_eq!(globe.details.source(VIEW, RASTER), None);
    give_tiles(&mut globe);
    assert!(globe.details.source(VIEW, RASTER).is_some());
}
