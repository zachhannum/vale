//! The small picture of the globe for the list of projects. The CPU makes
//! it from the heightmaps of the visible raster layers.

use eframe::egui::{Pos2, Rect, Vec2};
use vale_store::Thumbnail;
use vale_terrain::Heightmap;

use super::view::GlobeView;

/// The width and the height of the picture, in pixels.
pub const SIZE: u32 = 256;

/// The globe of the view at zoom 1. `layers` has the heightmap and the
/// opacity of each visible raster layer, from the bottom. Each layer has the
/// step colors of its bands, over the layers below it and the plain ground.
/// The pixels off the globe are transparent.
pub fn render(layers: &[(&Heightmap, f32)], view: &GlobeView, size: u32) -> Thumbnail {
    let view = GlobeView { zoom: 1.0, ..*view };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(size as f32));
    let radius = view.radius(rect) as f32;
    let bands: Vec<_> = layers.iter().map(|(map, _)| map.bands.bands()).collect();
    let ground = crate::ui::pad::theme::GROUND;
    let ground = [ground.r(), ground.g(), ground.b()].map(f32::from);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let pos = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let from_center = (pos - rect.center()).length();
            let Some(dir) = view.unproject(rect, pos) else {
                rgba.extend([0; 4]);
                continue;
            };
            let mut color = ground;
            for ((map, opacity), bands) in layers.iter().zip(&bands) {
                let value = map.bands.value(map.sample(dir));
                let band = bands.iter().find(|b| value < b.max).or(bands.last());
                let top = band.map_or([128; 3], |b| b.color).map(f32::from);
                let opacity = opacity.clamp(0.0, 1.0);
                color = std::array::from_fn(|c| color[c] + (top[c] - color[c]) * opacity);
            }
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
    use vale_terrain::meters_to_level;

    #[test]
    fn the_picture_is_a_disc_with_the_color_of_the_ground() {
        let pixel = |t: &Thumbnail, x: u32, y: u32| {
            let at = ((y * t.width + x) * 4) as usize;
            [t.rgba[at], t.rgba[at + 1], t.rgba[at + 2], t.rgba[at + 3]]
        };
        let view = GlobeView::centered(0.0, 0.0);
        let (low, high) = (meters_to_level(-3000.0), meters_to_level(2000.0));
        let (low, high) = (Heightmap::new(64, low), Heightmap::new(64, high));
        let sea = render(&[(&low, 1.0)], &view, 64);
        let land = render(&[(&high, 1.0)], &view, 64);
        assert_eq!(sea.rgba.len(), 64 * 64 * 4);
        assert_eq!(pixel(&sea, 0, 0)[3], 0);
        assert_eq!(pixel(&sea, 32, 32)[3], 255);
        let (sea, land) = (pixel(&sea, 32, 32), pixel(&land, 32, 32));
        assert!(sea[2] > sea[0], "the sea is blue: {sea:?}");
        assert_ne!(sea, land);
    }

    #[test]
    fn the_picture_of_the_globe_blends_the_raster_layers() {
        let middle = |t: &Thumbnail| {
            let at = ((32 * t.width + 32) * 4) as usize;
            [t.rgba[at], t.rgba[at + 1], t.rgba[at + 2]].map(i32::from)
        };
        let view = GlobeView::centered(0.0, 0.0);
        let low = Heightmap::new(64, meters_to_level(-3000.0));
        let high = Heightmap::new(64, meters_to_level(2000.0));
        let ground = crate::ui::pad::theme::GROUND;
        let ground = [ground.r(), ground.g(), ground.b()].map(i32::from);
        let near = |a: [i32; 3], b: [i32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 2);
        let mean = |a: [i32; 3], b: [i32; 3]| std::array::from_fn(|c| (a[c] + b[c]) / 2);

        // A world with no visible raster layer has the plain ground.
        assert!(near(middle(&render(&[], &view, 64)), ground));
        let sea = middle(&render(&[(&low, 1.0)], &view, 64));
        let land = middle(&render(&[(&high, 1.0)], &view, 64));
        // The top layer hides the layer below it.
        assert_eq!(
            middle(&render(&[(&low, 1.0), (&high, 1.0)], &view, 64)),
            land
        );
        assert_eq!(
            middle(&render(&[(&high, 1.0), (&low, 1.0)], &view, 64)),
            sea
        );
        // At one half, the picture is between the two layers.
        let half = middle(&render(&[(&low, 1.0), (&high, 0.5)], &view, 64));
        assert!(near(half, mean(sea, land)), "{half:?}");
        assert_eq!(
            middle(&render(&[(&low, 1.0), (&high, 0.0)], &view, 64)),
            sea
        );
        // One layer at one half shows the ground.
        let half = middle(&render(&[(&high, 0.5)], &view, 64));
        assert!(near(half, mean(ground, land)), "{half:?}");
    }
}
