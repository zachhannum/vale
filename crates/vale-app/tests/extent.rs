//! The extent of a map on the globe and in the flat view.

use eframe::egui::{Pos2, Rect, Vec2};
use image::RgbaImage;
use kurbo::Point;
use vale_app::atlas::{self, FrameTarget};
use vale_app::document::Document;
use vale_app::globe::WorldView;
use vale_app::globe::math::{V3, lonlat_to_dir};
use vale_app::headless;
use vale_app::ui::AppState;
use vale_app::ui::pad::theme::ACCENT;
use vale_sphere::{Projection, ProjectionSpec};
use vale_store::{FrameShape, MapId};

/// One GPU test runs at a time, because the software renderer is slow.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const SIZE: (f64, f64) = (800.0, 600.0);
const FRAME: [f64; 4] = [20.0, 20.0, 180.0, 120.0];

/// The sample world with no vector layers shown, and a map with one frame.
fn state(proj: &str, center: [f64; 2], scale: f64) -> (AppState, MapId) {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    let ids: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in ids {
        state.doc.set_visible(id, false);
    }
    state.globe.preview.graticule = false;
    let map = state.doc.first_map();
    let spec = ProjectionSpec::from_proj(proj).unwrap();
    state.doc.set_projection(map, spec);
    let projection = projection(&state, map);
    let atlas = &mut state.doc.project.atlas;
    let mut frame = atlas::new_frame(&atlas.pages[0], &atlas.maps[0], &projection);
    frame.shape = FrameShape::Rectangle(FRAME);
    frame.center = center;
    frame.scale = scale;
    atlas.pages[0].frames = vec![frame];
    (state, map)
}

fn projection(state: &AppState, map: MapId) -> Projection {
    let radius = state.doc.project.world.radius_km * 1000.0;
    Projection::new(state.doc.projection(map), radius).unwrap()
}

/// Directions on the edges of the frame of the map.
fn frame_edge(state: &AppState, map: MapId) -> Vec<V3> {
    let projection = projection(state, map);
    let frame = &state.doc.project.atlas.pages[0].frames[0];
    let view = atlas::frame_transform(&projection, frame, FrameTarget::MM);
    let [x0, y0, x1, y1] = FRAME;
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)];
    let mut out = Vec::new();
    for pair in corners.windows(2) {
        let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
        for i in 0..12 {
            let t = (i as f64 + 0.5) / 12.0;
            let q = Point::new(ax + (bx - ax) * t, ay + (by - ay) * t);
            let ll = projection.inverse(view.from_page(q)).unwrap();
            out.push(lonlat_to_dir(ll[0], ll[1]));
        }
    }
    out
}

/// The direction of a place inside the frame, near its middle. The middle
/// itself is on the edge of the flat map in some tests.
fn frame_middle(state: &AppState, map: MapId) -> V3 {
    let projection = projection(state, map);
    let frame = &state.doc.project.atlas.pages[0].frames[0];
    let view = atlas::frame_transform(&projection, frame, FrameTarget::MM);
    let q = Point::new(
        FRAME[0] * 0.6 + FRAME[2] * 0.4,
        FRAME[1] * 0.6 + FRAME[3] * 0.4,
    );
    let ll = projection.inverse(view.from_page(q)).unwrap();
    lonlat_to_dir(ll[0], ll[1])
}

fn is_accent(img: &RgbaImage, p: Pos2) -> bool {
    let (x, y) = (p.x.round() as i64, p.y.round() as i64);
    if x < 0 || y < 0 || x >= i64::from(img.width()) || y >= i64::from(img.height()) {
        return false;
    }
    let c = img.get_pixel(x as u32, y as u32).0;
    let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
    d(c[0], ACCENT.r()) + d(c[1], ACCENT.g()) + d(c[2], ACCENT.b()) < 60
}

/// True if an accent pixel is within `r` pixels of `p`.
fn accent_near(img: &RgbaImage, p: Pos2, r: f32) -> bool {
    let n = r as i32;
    (-n..=n).any(|dy| (-n..=n).any(|dx| is_accent(img, p + Vec2::new(dx as f32, dy as f32))))
}

fn grey(img: &RgbaImage, p: Pos2) -> i32 {
    let c = img.get_pixel(p.x as u32, p.y as u32).0;
    (i32::from(c[0]) + i32::from(c[1]) + i32::from(c[2])) / 3
}

/// The screen position of a direction in the view of the state.
fn screen(state: &AppState, d: V3) -> Option<Pos2> {
    state.globe.project(d)
}

/// Renders a state with the map on the globe, and the state with no map.
fn render(state: AppState, map: MapId, view: WorldView) -> (RgbaImage, RgbaImage, AppState) {
    let (with, state) = headless::ui_png_with(state, SIZE, 1.0, |state| {
        state.globe.world_view = view;
        state.show_map_on_globe(map);
        Ok(())
    })
    .unwrap();
    let (without, state) = headless::ui_png_with(state, SIZE, 1.0, |state| {
        state.close_map_on_globe();
        Ok(())
    })
    .unwrap();
    (with, without, state)
}

/// The outline is on the edges of the frame, the middle of the frame keeps
/// its color, and the ground outside is darker.
fn check_extent(proj: &str, center: [f64; 2], scale: f64, view: WorldView, name: &str) {
    let (state, map) = state(proj, center, scale);
    let (with, without, state) = render(state, map, view);
    with.save(format!("../../target/app/extent-{name}.png"))
        .ok();
    let edge = frame_edge(&state, map);
    let hits = edge.iter().filter_map(|&d| screen(&state, d));
    let (count, near) = hits.fold((0, 0), |(n, k), p| {
        (n + 1, k + usize::from(accent_near(&with, p, 6.0)))
    });
    assert!(
        count >= edge.len() * 3 / 4,
        "{name}: {count} of {} on screen",
        edge.len()
    );
    assert!(
        near * 10 >= count * 9,
        "{name}: {near} of {count} near the outline"
    );

    let middle = screen(&state, frame_middle(&state, map)).unwrap();
    assert!(
        (grey(&with, middle) - grey(&without, middle)).abs() <= 2,
        "{name}: the inside keeps its color"
    );
    // A place of the ground outside the frame, between the middle of the
    // frame and the edge of the globe or the map.
    let rect: Rect = state.globe.rect;
    let outside = [
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(-1.0, 0.0),
    ]
    .into_iter()
    .filter_map(|dir| {
        let p = middle + dir * 250.0;
        let d = state.globe.unproject(p)?;
        let inside_frame = edge
            .iter()
            .any(|&e| vale_app::globe::math::angle(e, d) < 0.02);
        (rect.contains(p) && !inside_frame).then_some(p)
    })
    .find(|&p| grey(&without, p) > 10 && !accent_near(&with, p, 6.0))
    .expect("a place outside the frame");
    let (dark, light) = (grey(&with, outside), grey(&without, outside));
    assert!(
        dark + 5 < light,
        "{name}: the outside is darker: {dark} {light}"
    );
}

#[test]
#[ignore = "gpu"]
fn the_extent_crosses_the_antimeridian_on_the_globe() {
    let _gpu = one_gpu_test();
    check_extent(
        "+proj=eqearth +lon_0=180",
        [180.0, -15.0],
        30_000_000.0,
        WorldView::Globe,
        "antimeridian-globe",
    );
}

#[test]
#[ignore = "gpu"]
fn the_extent_crosses_the_antimeridian_in_the_flat_view() {
    let _gpu = one_gpu_test();
    check_extent(
        "+proj=eqearth +lon_0=180",
        [180.0, -15.0],
        30_000_000.0,
        WorldView::Flat,
        "antimeridian-flat",
    );
}

#[test]
#[ignore = "gpu"]
fn the_extent_goes_around_a_pole_on_the_globe() {
    let _gpu = one_gpu_test();
    check_extent(
        "+proj=laea +lat_0=90 +lon_0=0",
        [0.0, 90.0],
        40_000_000.0,
        WorldView::Globe,
        "pole-globe",
    );
}

#[test]
#[ignore = "gpu"]
fn the_extent_goes_around_a_pole_in_the_flat_view() {
    let _gpu = one_gpu_test();
    check_extent(
        "+proj=laea +lat_0=90 +lon_0=0",
        [0.0, 90.0],
        40_000_000.0,
        WorldView::Flat,
        "pole-flat",
    );
}

/// The widest run of accent pixels across the top edge of the frame, near
/// its middle.
fn line_weight(img: &RgbaImage, p: Pos2) -> usize {
    let column = |x: f32| {
        let mut best = 0;
        let mut run = 0;
        for dy in -12..=12 {
            if is_accent(img, Pos2::new(x, p.y + dy as f32)) {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
        best
    };
    (-8..=8).map(|dx| column(p.x + dx as f32)).max().unwrap()
}

#[test]
#[ignore = "gpu"]
fn the_outline_has_the_same_weight_at_each_zoom() {
    let _gpu = one_gpu_test();
    let (state, map) = state("+proj=eqearth +lon_0=0", [10.0, 45.0], 20_000_000.0);
    let mut weights = Vec::new();
    let mut state = Some(state);
    for zoom in [1.0, 2.0] {
        let (img, s) = headless::ui_png_with(state.take().unwrap(), SIZE, 1.0, |state| {
            state.show_map_on_globe(map);
            state.globe.view.zoom *= zoom;
            Ok(())
        })
        .unwrap();
        let top = frame_edge(&s, map)[6];
        let p = screen(&s, top).unwrap();
        weights.push(line_weight(&img, p));
        state = Some(s);
    }
    assert!(
        weights[0] >= 1 && weights[0].abs_diff(weights[1]) <= 1,
        "{weights:?}"
    );
}

#[test]
#[ignore = "gpu"]
fn a_frame_change_moves_the_extent() {
    let _gpu = one_gpu_test();
    let (state, map) = state("+proj=eqearth +lon_0=0", [10.0, 45.0], 20_000_000.0);
    let (before, state) = headless::ui_png_with(state, SIZE, 1.0, |state| {
        state.show_map_on_globe(map);
        Ok(())
    })
    .unwrap();
    let old_edge = frame_edge(&state, map);
    let (after, state) = headless::ui_png_with(state, SIZE, 1.0, |state| {
        state.doc.project.atlas.pages[0].frames[0].center = [30.0, 45.0];
        Ok(())
    })
    .unwrap();
    let new_edge = frame_edge(&state, map);
    let left = screen(&state, new_edge[42]).unwrap();
    let old_left = screen(&state, old_edge[42]).unwrap();
    assert!(accent_near(&after, left, 6.0));
    assert!(!accent_near(&after, old_left, 3.0));
    assert!(accent_near(&before, old_left, 6.0));
}
