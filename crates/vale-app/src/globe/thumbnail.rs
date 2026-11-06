//! The small picture of the globe for the list of projects. The CPU makes
//! it from the heightmaps of the visible raster layers and the shapes of the
//! visible vector layers.

use eframe::egui::{Mesh, Pos2, Rect, Vec2};
use vale_store::Thumbnail;
use vale_terrain::Heightmap;

use super::view::GlobeView;

/// The width and the height of the picture, in pixels.
pub const SIZE: u32 = 256;

/// One visible layer of the picture.
pub enum Layer<'a> {
    /// The heightmap and the opacity of a raster layer.
    Raster(&'a Heightmap, f32),
    /// The meshes of the shapes of a vector layer, in the pixels of the
    /// picture.
    Vector(Vec<Mesh>),
}

/// The globe of the view at zoom 1. `layers` has the visible layers, from
/// the bottom. A raster layer has the step colors of its bands, over the
/// layers below it and the plain ground. The pixels off the globe are
/// transparent.
pub fn render(layers: &[Layer], view: &GlobeView, size: u32) -> Thumbnail {
    let view = GlobeView { zoom: 1.0, ..*view };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(size as f32));
    let radius = view.radius(rect) as f32;
    // The step colors of each raster layer, or the pixels of each vector
    // layer.
    enum Paint<'a> {
        Raster(&'a Heightmap, f32, Vec<vale_terrain::Band>),
        Vector(Vec<[f32; 4]>),
    }
    let paints: Vec<_> = (layers.iter())
        .map(|layer| match layer {
            Layer::Raster(map, opacity) => Paint::Raster(map, *opacity, map.bands.bands()),
            Layer::Vector(meshes) => Paint::Vector(fill(meshes, size)),
        })
        .collect();
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
            for paint in &paints {
                match paint {
                    Paint::Raster(map, opacity, bands) => {
                        let value = map.bands.value(map.sample(dir));
                        let band = bands.iter().find(|b| value < b.max).or(bands.last());
                        let top = band.map_or([128; 3], |b| b.color).map(f32::from);
                        let opacity = opacity.clamp(0.0, 1.0);
                        color = std::array::from_fn(|c| color[c] + (top[c] - color[c]) * opacity);
                    }
                    Paint::Vector(pixels) => {
                        let top = pixels[(y * size + x) as usize];
                        color = std::array::from_fn(|c| top[c] + color[c] * (1.0 - top[3]));
                    }
                }
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

/// The premultiplied colors of `meshes` in a picture of `size` by `size`
/// pixels, from 0 to 255. Each pixel takes the color of each triangle over
/// its center, with the colors of the corners mixed.
fn fill(meshes: &[Mesh], size: u32) -> Vec<[f32; 4]> {
    let n = size as usize;
    let mut pixels = vec![[0.0f32; 4]; n * n];
    for mesh in meshes {
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
            let area =
                (b.pos - a.pos).x * (c.pos - a.pos).y - (b.pos - a.pos).y * (c.pos - a.pos).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let lo = a.pos.min(b.pos).min(c.pos);
            let hi = a.pos.max(b.pos).max(c.pos);
            let (x0, y0) = (lo.x.max(0.0) as usize, lo.y.max(0.0) as usize);
            let (x1, y1) = (
                (hi.x.ceil().max(0.0) as usize).min(n),
                (hi.y.ceil().max(0.0) as usize).min(n),
            );
            let edge =
                |p: Pos2, q: Pos2, r: Pos2| ((q - p).x * (r - p).y - (q - p).y * (r - p).x) / area;
            let colors = [a, b, c].map(|v| v.color.to_array().map(f32::from));
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w = [
                        edge(b.pos, c.pos, p),
                        edge(c.pos, a.pos, p),
                        edge(a.pos, b.pos, p),
                    ];
                    if w.iter().any(|w| *w < 0.0) {
                        continue;
                    }
                    let top: [f32; 4] = std::array::from_fn(|i| {
                        w[0] * colors[0][i] + w[1] * colors[1][i] + w[2] * colors[2][i]
                    });
                    let pixel = &mut pixels[y * n + x];
                    let keep = 1.0 - top[3] / 255.0;
                    *pixel = std::array::from_fn(|i| top[i] + pixel[i] * keep);
                }
            }
        }
    }
    for pixel in &mut pixels {
        pixel[3] /= 255.0;
    }
    pixels
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
        let sea = render(&[Layer::Raster(&low, 1.0)], &view, 64);
        let land = render(&[Layer::Raster(&high, 1.0)], &view, 64);
        assert_eq!(sea.rgba.len(), 64 * 64 * 4);
        assert_eq!(pixel(&sea, 0, 0)[3], 0);
        assert_eq!(pixel(&sea, 32, 32)[3], 255);
        let (sea, land) = (pixel(&sea, 32, 32), pixel(&land, 32, 32));
        assert!(sea[2] > sea[0], "the sea is blue: {sea:?}");
        assert_ne!(sea, land);
    }

    /// A red square of `side` pixels around the middle of a picture of 64
    /// pixels, with the alpha `alpha`.
    fn square(side: f32, alpha: u8) -> Layer<'static> {
        let color = eframe::egui::Color32::from_rgba_unmultiplied(255, 0, 0, alpha);
        let rect = Rect::from_center_size(Pos2::new(32.0, 32.0), Vec2::splat(side));
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, color);
        Layer::Vector(vec![mesh])
    }

    #[test]
    fn the_picture_shows_the_vector_layers_in_the_draw_order() {
        let middle = |t: &Thumbnail| {
            let at = ((32 * t.width + 32) * 4) as usize;
            [t.rgba[at], t.rgba[at + 1], t.rgba[at + 2]].map(i32::from)
        };
        let view = GlobeView::centered(0.0, 0.0);
        let low = Heightmap::new(64, meters_to_level(-3000.0));
        let sea = middle(&render(&[Layer::Raster(&low, 1.0)], &view, 64));
        let red = render(&[Layer::Raster(&low, 1.0), square(20.0, 255)], &view, 64);
        let [r, g, b] = middle(&red);
        assert!(r > 200 && g < 20 && b < 20, "{:?}", [r, g, b]);
        // The square covers only the middle.
        let at = ((4 * 64 + 32) * 4) as usize;
        let sea_edge = render(&[Layer::Raster(&low, 1.0)], &view, 64);
        assert_eq!(red.rgba[at..at + 4], sea_edge.rgba[at..at + 4]);
        // A raster layer above the vector layer hides it.
        let hidden = render(&[square(20.0, 255), Layer::Raster(&low, 1.0)], &view, 64);
        assert_eq!(middle(&hidden), sea);
        // A translucent fill shows the layer below it.
        let half = middle(&render(
            &[Layer::Raster(&low, 1.0), square(20.0, 128)],
            &view,
            64,
        ));
        assert!(half[0] > sea[0] + 60 && half[0] < r - 60, "{half:?}");
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
        let sea = middle(&render(&[Layer::Raster(&low, 1.0)], &view, 64));
        let land = middle(&render(&[Layer::Raster(&high, 1.0)], &view, 64));
        // The top layer hides the layer below it.
        assert_eq!(
            middle(&render(
                &[Layer::Raster(&low, 1.0), Layer::Raster(&high, 1.0)],
                &view,
                64
            )),
            land
        );
        assert_eq!(
            middle(&render(
                &[Layer::Raster(&high, 1.0), Layer::Raster(&low, 1.0)],
                &view,
                64
            )),
            sea
        );
        // At one half, the picture is between the two layers.
        let half = middle(&render(
            &[Layer::Raster(&low, 1.0), Layer::Raster(&high, 0.5)],
            &view,
            64,
        ));
        assert!(near(half, mean(sea, land)), "{half:?}");
        assert_eq!(
            middle(&render(
                &[Layer::Raster(&low, 1.0), Layer::Raster(&high, 0.0)],
                &view,
                64
            )),
            sea
        );
        // One layer at one half shows the ground.
        let half = middle(&render(&[Layer::Raster(&high, 0.5)], &view, 64));
        assert!(near(half, mean(ground, land)), "{half:?}");
    }
}
