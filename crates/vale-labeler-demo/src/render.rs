//! Drawing and PNG output.

use std::path::Path;

use anyhow::Context;
use kurbo::{BezPath, Circle, Rect, Shape};
use vale_labeler::Labeling;
use vale_render::{Color, DisplayList, FillRule, GlyphRun, Item};
use vello_cpu::Pixmap;

use crate::scene::Scene;

pub struct RenderOptions {
    pub debug_boxes: bool,
}

/// The water color, also used by tests.
pub const WATER: [u8; 3] = [214, 232, 242];

fn rgb(c: [u8; 3]) -> Color {
    Color::rgb(c[0], c[1], c[2])
}

fn fill(path: BezPath, color: Color, rule: FillRule) -> Item {
    Item::Fill { path, color, rule }
}

fn stroke(path: BezPath, color: Color, width: f64, round: bool) -> Item {
    Item::Stroke {
        path,
        color,
        width,
        round,
    }
}

/// The drawing of a scene and its labels, in page space.
pub fn display_list(scene: &Scene, labeling: &Labeling, options: &RenderOptions) -> DisplayList {
    let mut list = DisplayList::new(f64::from(scene.width), f64::from(scene.height));

    list.push(fill(
        Rect::new(0.0, 0.0, f64::from(scene.width), f64::from(scene.height)).to_path(0.1),
        rgb(WATER),
        FillRule::NonZero,
    ));

    for path in &scene.land {
        list.push(fill(path.clone(), rgb([244, 241, 232]), FillRule::EvenOdd));
    }
    for path in &scene.land {
        list.push(stroke(path.clone(), rgb([150, 160, 165]), 0.6, false));
    }

    for (path, width) in &scene.rivers {
        list.push(stroke(path.clone(), rgb([90, 150, 200]), *width, true));
    }

    for (position, radius, capital) in &scene.places {
        list.push(fill(
            Circle::new(*position, *radius).to_path(0.1),
            rgb([60, 60, 60]),
            FillRule::NonZero,
        ));
        if *capital {
            list.push(stroke(
                Circle::new(*position, *radius + 0.5).to_path(0.1),
                rgb([255, 255, 255]),
                1.0,
                false,
            ));
        }
    }

    for label in &labeling.placed {
        list.push(stroke(
            label.outlines(),
            Color::rgba(255, 255, 255, 217),
            2.4,
            true,
        ));
    }

    for label in &labeling.placed {
        let color = if label.class == 1 {
            rgb([40, 100, 160])
        } else {
            rgb([30, 30, 30])
        };
        for run in &label.glyph_runs {
            list.push(Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
                text: label.text.clone(),
                color,
            }));
        }
    }

    if options.debug_boxes {
        for label in &labeling.placed {
            for b in &label.boxes {
                let c = b.corners();
                let mut path = BezPath::new();
                path.move_to(c[0]);
                for p in &c[1..] {
                    path.line_to(*p);
                }
                path.close_path();
                list.push(stroke(path, rgb([255, 0, 0]), 0.5, false));
            }
        }
    }
    list
}

pub fn render(scene: &Scene, labeling: &Labeling, options: &RenderOptions) -> Pixmap {
    vale_render::render_pixmap(&display_list(scene, labeling, options), 1.0)
        .expect("the demo scene has a valid page size")
}

pub fn write_png(pixmap: Pixmap, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = pixmap.into_png().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}
