//! The size of the meshes that the vector layers add to one frame. egui
//! puts the meshes of a frame in one vertex buffer and one index buffer,
//! and a GPU buffer has a size limit. A layer whose shapes do not fit in
//! the rest of the frame does not draw, and its row shows an error.

use eframe::egui::{Context, Id, Shape};
use vale_store::LayerId;

/// The most vertices of the vector layers in one frame. egui doubles a
/// buffer when it grows, so a buffer stays under twice the size of its
/// meshes. 4 million vertices of 20 bytes and 24 million indices of 4
/// bytes keep each buffer under 256 MB, with space for the rest of the UI.
pub const MAX_VERTICES: usize = 4_000_000;
pub const MAX_INDICES: usize = 24_000_000;

/// The note in the row of a layer that does not fit in the frame.
pub const TOO_LARGE: &str = "Too large to show";

/// The warning in the panel of a layer that does not fit in the frame.
pub const TOO_LARGE_ABOUT: &str =
    "This layer has too much detail to show in this view. Zoom in or out, or hide other layers.";

/// The vertices and the indices of a mesh.
pub type Cost = (usize, usize);

#[derive(Clone)]
struct Budget {
    pass: u64,
    used: Cost,
    limit: Cost,
    /// The layers that did not draw in this pass and in the last pass.
    dropped: Vec<LayerId>,
    last: Vec<LayerId>,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            pass: 0,
            used: (0, 0),
            limit: (MAX_VERTICES, MAX_INDICES),
            dropped: Vec::new(),
            last: Vec::new(),
        }
    }
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut Budget) -> R) -> R {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let b = d.get_temp_mut_or_insert_with(Id::new("vale-mesh-budget"), Budget::default);
        if b.pass != pass {
            b.pass = pass;
            b.used = (0, 0);
            b.last = std::mem::take(&mut b.dropped);
        }
        f(b)
    })
}

/// Takes `cost` from the frame for a draw of `layer`. False: the draw does
/// not fit, and the layer does not draw.
pub fn spend(ctx: &Context, layer: LayerId, cost: Cost) -> bool {
    let (fits, new) = with(ctx, |b| {
        let used = (b.used.0 + cost.0, b.used.1 + cost.1);
        if used.0 <= b.limit.0 && used.1 <= b.limit.1 {
            b.used = used;
            return (true, false);
        }
        let new = !b.last.contains(&layer) && !b.dropped.contains(&layer);
        b.dropped.push(layer);
        (false, new)
    });
    if new {
        // The row of the layer shows the error in the next frame.
        ctx.request_repaint();
    }
    fits
}

/// True if `layer` did not fit in the last frame.
pub fn too_large(ctx: &Context, layer: LayerId) -> bool {
    with(ctx, |b| {
        b.last.contains(&layer) || b.dropped.contains(&layer)
    })
}

/// The vertices and the indices that the last pass with a draw spent, for
/// the tests.
pub fn last_used(ctx: &Context) -> Cost {
    let budget = ctx.data(|d| d.get_temp::<Budget>(Id::new("vale-mesh-budget")));
    budget.map_or((0, 0), |b| b.used)
}

/// Sets the most vertices and indices of a frame, for the tests.
pub fn set_limit(ctx: &Context, limit: Cost) {
    with(ctx, |b| b.limit = limit);
}

/// An upper bound of the vertices and the indices of the mesh of `shape`.
/// A fill of a path has up to 2 vertices and 9 indices for each point, and
/// a stroke up to 6 vertices and 24 indices, with the feathering and the
/// sharp joins.
pub fn cost(shape: &Shape) -> Cost {
    let path = |points: usize, fill: bool, stroke: bool| {
        let (v, i) = (
            usize::from(fill) * 2 + usize::from(stroke) * 6,
            usize::from(fill) * 9 + usize::from(stroke) * 24,
        );
        (points * v, points * i)
    };
    // The points of a circle, as `epaint::Path::add_circle` makes them.
    let circle = |radius: f32| match radius {
        r if r <= 2.0 => 8,
        r if r <= 5.0 => 16,
        r if r < 18.0 => 32,
        r if r < 50.0 => 64,
        _ => 128,
    };
    match shape {
        Shape::Noop | Shape::Callback(_) => (0, 0),
        Shape::Vec(shapes) => shapes.iter().map(cost).fold((0, 0), add),
        Shape::Mesh(mesh) => (mesh.vertices.len(), mesh.indices.len()),
        Shape::Path(p) => path(p.points.len(), p.fill.a() > 0, p.stroke.width > 0.0),
        Shape::LineSegment { .. } => path(2, false, true),
        Shape::Circle(c) => path(circle(c.radius), c.fill.a() > 0, c.stroke.width > 0.0),
        Shape::Ellipse(e) => path(circle(e.radius.max_elem()), true, true),
        Shape::Text(t) => {
            let glyphs = t.galley.job.text.len();
            (glyphs * 4, glyphs * 6)
        }
        _ => path(128, true, true),
    }
}

/// The sum of two costs.
pub fn add(a: Cost, b: Cost) -> Cost {
    (a.0 + b.0, a.1 + b.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn a_draw_that_does_not_fit_leaves_its_layer_out() {
        let ctx = Context::default();
        set_limit(&ctx, (100, 1000));
        let (a, b) = (LayerId(1), LayerId(2));
        assert!(spend(&ctx, a, (60, 10)));
        assert!(!spend(&ctx, b, (60, 10)));
        assert!(!too_large(&ctx, a));
        assert!(too_large(&ctx, b));
    }

    #[test]
    fn the_cost_of_a_shape_is_at_least_its_mesh() {
        let red = egui::Color32::RED;
        let stroke = egui::Stroke::new(2.0, red);
        let points: Vec<egui::Pos2> = (0..50)
            .map(|i| egui::pos2(i as f32 * 3.0, (i % 7) as f32 * 5.0))
            .collect();
        let shapes = [
            Shape::line(points.clone(), stroke),
            Shape::closed_line(points.clone(), egui::Stroke::new(0.5, red)),
            Shape::convex_polygon(points.clone(), red, stroke),
            Shape::circle_filled(egui::pos2(10.0, 10.0), 3.0, red),
            Shape::circle_stroke(egui::pos2(10.0, 10.0), 30.0, stroke),
            Shape::line_segment([egui::pos2(0.0, 0.0), egui::pos2(9.0, 9.0)], stroke),
        ];
        let ctx = Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        for shape in shapes {
            let want = cost(&shape);
            let clipped = egui::epaint::ClippedShape {
                clip_rect: egui::Rect::EVERYTHING,
                shape,
            };
            for ppp in [1.0, 2.0] {
                for primitive in ctx.tessellate(vec![clipped.clone()], ppp) {
                    let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                        continue;
                    };
                    assert!(
                        mesh.vertices.len() <= want.0,
                        "{} > {}",
                        mesh.vertices.len(),
                        want.0
                    );
                    assert!(
                        mesh.indices.len() <= want.1,
                        "{} > {}",
                        mesh.indices.len(),
                        want.1
                    );
                }
            }
        }
    }
}
