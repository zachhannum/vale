//! The small picture of the globe for the start screen. The CPU makes it
//! from the heightmap, in the colors of the globe.

use eframe::egui::{Pos2, Rect, Vec2};
use isotherm_store::Thumbnail;
use isotherm_world::Terrain;
use vale_globe::view::GlobeView;

/// The width and the height of the picture, in pixels.
pub const SIZE: u32 = 256;

/// The globe of the view at zoom 1, with the step colors of the bands of
/// the terrain. The pixels off the globe are transparent.
pub fn render(terrain: &Terrain, view: &GlobeView, size: u32) -> Thumbnail {
    let view = GlobeView { zoom: 1.0, ..*view };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(size as f32));
    let radius = view.radius(rect) as f32;
    let bands = terrain.bands();
    let steps = bands.bands();
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let pos = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let from_center = (pos - rect.center()).length();
            let Some(dir) = view.unproject(rect, pos) else {
                rgba.extend([0; 4]);
                continue;
            };
            let value = bands.value(terrain.heights.sample(dir));
            let band = steps.iter().find(|b| value < b.max).or(steps.last());
            let color = band.map_or([128; 3], |b| b.color).map(f32::from);
            // The globe is darker at its edge, as a lit ball is.
            let height = (1.0 - (from_center / radius).powi(2)).max(0.0).sqrt();
            let light = 0.6 + 0.4 * height;
            let alpha = (radius - from_center).clamp(0.0, 1.0);
            rgba.extend(color.map(|c| (c * light) as u8));
            rgba.push((alpha * 255.0) as u8);
        }
    }
    Thumbnail {
        width: size,
        height: size,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn the_picture_is_a_disc_and_the_sea_is_bluer_than_the_land() {
        let world = Document::sample().unwrap().world;
        // Africa and the Atlantic at the middle of the globe.
        let view = GlobeView::centered(0.0, 0.0);
        let picture = render(&world.terrain, &view, 64);
        assert_eq!(picture.rgba.len(), 64 * 64 * 4);
        let pixel = |x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            [0, 1, 2, 3].map(|c| i32::from(picture.rgba[i + c]))
        };
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(32, 32)[3], 255);
        assert_eq!(pixel(63, 63)[3], 0);
        let blue = |p: [i32; 4]| p[2] - p[0];
        // A point in the Sahara, and a point in the Gulf of Guinea.
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(64.0));
        let at = |lon: f64, lat: f64| {
            let d = vale_terrain::math::lonlat_to_dir(lon, lat);
            let p = view.project(rect, d).unwrap();
            pixel(p.x as u32, p.y as u32)
        };
        assert!(blue(at(-5.0, 0.0)) > blue(at(15.0, 22.0)) + 20);
    }
}
