//! The region of a box on the screen of the world workspace.

use eframe::egui::{Pos2, Rect, pos2};

use super::math::{dir_to_lonlat, lonlat_to_dir};
use super::{Globe, WorldView};
use crate::region::Region;

/// The points across each side of the box that make the region.
const SAMPLES: usize = 33;

/// The region of the places in the screen box `rect`. A point of the box off
/// the globe is not in the region. `None` when fewer than two points of the
/// box are on the globe.
pub fn region_of_box(globe: &Globe, rect: Rect) -> Option<Region> {
    let mut points = Vec::with_capacity(SAMPLES * SAMPLES);
    for i in 0..SAMPLES {
        for j in 0..SAMPLES {
            let t = |k: usize| k as f32 / (SAMPLES - 1) as f32;
            let pos = pos2(
                rect.min.x + rect.width() * t(i),
                rect.min.y + rect.height() * t(j),
            );
            if let Some(dir) = globe.unproject(pos) {
                let (lon, lat) = dir_to_lonlat(dir);
                points.push([lon, lat]);
            }
        }
    }
    let inside = |pos: Option<Pos2>| pos.is_some_and(|p| rect.contains(p));
    let pole = [90.0, -90.0]
        .into_iter()
        .find(|lat| inside(globe.project(lonlat_to_dir(0.0, *lat))));
    let holds_globe = globe.world_view == WorldView::Globe && {
        let r = globe.view.radius(globe.rect) as f32;
        rect.contains_rect(Rect::from_center_size(
            globe.rect.center(),
            eframe::egui::vec2(2.0 * r, 2.0 * r),
        ))
    };
    let view = globe
        .unproject(globe.rect.center())
        .map_or([0.0, 0.0], |d| {
            let (lon, lat) = dir_to_lonlat(d);
            [lon, lat]
        });
    Region::from_points(&points, pole, holds_globe, view)
}
