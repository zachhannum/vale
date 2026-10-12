//! The small picture of the globe for the list of projects. The CPU makes
//! it from the heightmap.

use eframe::egui::{Pos2, Rect, Vec2};
use vale_store::Thumbnail;
use vale_terrain::{Heightmap, level_to_meters};

use super::view::GlobeView;

/// The width and the height of the picture, in pixels.
pub const SIZE: u32 = 256;

/// The globe of the view at zoom 1, with the step colors of the bands. The
/// pixels off the globe are transparent.
pub fn render(map: &Heightmap, view: &GlobeView, size: u32) -> Thumbnail {
    let view = GlobeView { zoom: 1.0, ..*view };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(size as f32));
    let radius = view.radius(rect) as f32;
    let bands = map.bands.bands();
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let pos = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let from_center = (pos - rect.center()).length();
            let Some(dir) = view.unproject(rect, pos) else {
                rgba.extend([0; 4]);
                continue;
            };
            let meters = level_to_meters(map.sample(dir));
            let band = bands.iter().find(|b| meters < b.max).or(bands.last());
            let color = band.map_or([128; 3], |b| b.color);
            // The globe is darker at its edge, as a lit ball is.
            let height = (1.0 - (from_center / radius).powi(2)).max(0.0).sqrt();
            let light = 0.6 + 0.4 * height;
            let alpha = (radius - from_center).clamp(0.0, 1.0);
            rgba.extend(color.map(|c| (f32::from(c) * light) as u8));
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
    use vale_terrain::meters_to_level;

    #[test]
    fn the_picture_is_a_disc_with_the_color_of_the_ground() {
        let pixel = |t: &Thumbnail, x: u32, y: u32| {
            let at = ((y * t.width + x) * 4) as usize;
            [t.rgba[at], t.rgba[at + 1], t.rgba[at + 2], t.rgba[at + 3]]
        };
        let view = GlobeView::centered(0.0, 0.0);
        let sea = render(&Heightmap::new(64, meters_to_level(-3000.0)), &view, 64);
        let land = render(&Heightmap::new(64, meters_to_level(2000.0)), &view, 64);
        assert_eq!(sea.rgba.len(), 64 * 64 * 4);
        assert_eq!(pixel(&sea, 0, 0)[3], 0);
        assert_eq!(pixel(&sea, 32, 32)[3], 255);
        let (sea, land) = (pixel(&sea, 32, 32), pixel(&land, 32, 32));
        assert!(sea[2] > sea[0], "the sea is blue: {sea:?}");
        assert_ne!(sea, land);
    }
}
