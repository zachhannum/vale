//! Point symbols as egui meshes, for the globe and for the symbol previews.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::{Arc, Mutex, Weak};

use eframe::egui::{self, Pos2, Rect};
use kurbo::{BezPath, PathEl};
use vale_render::{Color, FillRule, Symbol};
use vale_sphere::{LonLat, ScatterPoint};
use vale_store::{Feature, LayerId};
use vale_style::scale_denominator;

use super::math::lonlat_to_dir;
use super::view::GlobeView;
use crate::scatter::{self, ScatterCache, ScatterFill};

/// The flattening tolerance in symbol units.
const TOLERANCE: f64 = 0.002;

/// The radius of the dot that stands for a missing symbol, in symbol units.
const MISSING_DOT: f32 = 1.0 / 6.0;

/// The color of a missing symbol with no tint.
const MISSING: Color = Color::rgb(80, 80, 80);

/// The triangles of a symbol in symbol units, with a color for each vertex.
#[derive(Clone, Debug, Default)]
pub struct SymbolMesh {
    pub vertices: Vec<[f32; 2]>,
    pub colors: Vec<Color>,
    pub indices: Vec<u32>,
}

impl SymbolMesh {
    /// The fills and the strokes of a symbol, in drawing order.
    pub fn new(symbol: &Symbol) -> SymbolMesh {
        let mut mesh = SymbolMesh::default();
        for p in symbol.paths() {
            if let Some((color, rule)) = p.fill {
                let (points, indices) = fill(&loops(&p.path), rule);
                mesh.push(&points, &indices, color);
            }
            if let Some((color, width)) = p.stroke {
                let (points, indices) = stroke(&p.path, width);
                mesh.push(&points, &indices, color);
            }
        }
        mesh
    }

    /// The mesh of a symbol, from a cache that is shared by all callers. A
    /// symbol that replaces another symbol gets a new mesh.
    pub fn of(symbol: &Arc<Symbol>) -> Arc<SymbolMesh> {
        type Cache = HashMap<usize, (Weak<Symbol>, Arc<SymbolMesh>)>;
        static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
        let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(HashMap::new);
        let key = Arc::as_ptr(symbol) as usize;
        if let Some((weak, mesh)) = cache.get(&key)
            && weak.upgrade().is_some_and(|s| Arc::ptr_eq(&s, symbol))
        {
            return mesh.clone();
        }
        if cache.len() > 256 {
            cache.retain(|_, (weak, _)| weak.strong_count() > 0);
        }
        let mesh = Arc::new(SymbolMesh::new(symbol));
        cache.insert(key, (Arc::downgrade(symbol), mesh.clone()));
        mesh
    }

    fn push(&mut self, points: &[[f64; 2]], indices: &[u32], color: Color) {
        let base = self.vertices.len() as u32;
        self.vertices
            .extend(points.iter().map(|p| [p[0] as f32, p[1] as f32]));
        self.colors.extend(std::iter::repeat_n(color, points.len()));
        self.indices.extend(indices.iter().map(|i| i + base));
    }

    /// Whether a triangle of the mesh covers a point in symbol units.
    pub fn covers(&self, p: [f32; 2]) -> bool {
        self.indices.as_chunks::<3>().0.iter().any(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| self.vertices[t[k] as usize]);
            let side = |u: [f32; 2], v: [f32; 2]| {
                (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
            };
            let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
            let neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
            let pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
            !(neg && pos)
        })
    }

    /// Adds the symbol to `out`, `size` points big at `center`, turned
    /// `rotation` degrees clockwise. A tint replaces the colors and keeps
    /// their alpha.
    pub fn append(
        &self,
        out: &mut egui::Mesh,
        center: Pos2,
        size: f32,
        rotation: f32,
        tint: Option<Color>,
    ) {
        let (sin, cos) = rotation.to_radians().sin_cos();
        let base = out.vertices.len() as u32;
        let vertices = self.vertices.iter().zip(&self.colors);
        out.vertices.extend(vertices.map(|(v, c)| {
            let (x, y) = (v[0] * size, v[1] * size);
            egui::epaint::Vertex {
                pos: center + egui::vec2(x * cos - y * sin, x * sin + y * cos),
                uv: egui::epaint::WHITE_UV,
                color: color32(tinted(*c, tint)),
            }
        }));
        out.indices.extend(self.indices.iter().map(|i| i + base));
    }
}

/// Adds the dot that stands for a missing symbol to `out`.
pub fn append_missing(out: &mut egui::Mesh, center: Pos2, size: f32, tint: Option<Color>) {
    const N: u32 = 12;
    let color = color32(tint.unwrap_or(MISSING));
    let radius = MISSING_DOT * size;
    let base = out.vertices.len() as u32;
    out.colored_vertex(center, color);
    for i in 0..N {
        let a = i as f32 / N as f32 * std::f32::consts::TAU;
        out.colored_vertex(center + radius * egui::vec2(a.cos(), a.sin()), color);
        out.add_triangle(base, base + 1 + i, base + 1 + (i + 1) % N);
    }
}

/// Draws a symbol `size` points big at `center`, turned `rotation` degrees
/// clockwise. A tint replaces the colors and keeps their alpha.
pub fn paint_symbol(
    painter: &egui::Painter,
    symbol: &Arc<Symbol>,
    center: Pos2,
    size: f32,
    rotation: f32,
    tint: Option<Color>,
) {
    let mut mesh = egui::Mesh::default();
    SymbolMesh::of(symbol).append(&mut mesh, center, size, rotation, tint);
    painter.add(mesh);
}

fn color32(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

fn tinted(color: Color, tint: Option<Color>) -> Color {
    match tint {
        None => color,
        Some(t) => {
            let a = (u16::from(t.a) * u16::from(color.a) + 127) / 255;
            Color::rgba(t.r, t.g, t.b, a as u8)
        }
    }
}

/// The closed loops of the flattened subpaths of a path.
fn loops(path: &BezPath) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    kurbo::flatten(path, TOLERANCE, |el| match el {
        PathEl::MoveTo(p) => out.push(vec![[p.x, p.y]]),
        PathEl::LineTo(p) => {
            if let Some(l) = out.last_mut()
                && l.last() != Some(&[p.x, p.y])
            {
                l.push([p.x, p.y]);
            }
        }
        _ => {}
    });
    for l in &mut out {
        if l.len() > 1 && l.first() == l.last() {
            l.pop();
        }
    }
    out.retain(|l| l.len() > 2);
    out
}

/// The polylines of the flattened subpaths of a path, and whether each one
/// is closed.
fn polylines(path: &BezPath) -> Vec<(Vec<[f64; 2]>, bool)> {
    let mut out: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
    kurbo::flatten(path, TOLERANCE, |el| match el {
        PathEl::MoveTo(p) => out.push((vec![[p.x, p.y]], false)),
        PathEl::LineTo(p) => {
            if let Some((l, _)) = out.last_mut() {
                l.push([p.x, p.y]);
            }
        }
        PathEl::ClosePath => {
            if let Some((l, closed)) = out.last_mut() {
                *closed = true;
                if l.len() > 1 && l.first() == l.last() {
                    l.pop();
                }
            }
        }
        _ => {}
    });
    out
}

/// Twice the signed area of a closed path.
fn area2(path: &[[f64; 2]]) -> f64 {
    let n = path.len();
    (0..n)
        .map(|i| {
            let (a, b) = (path[i], path[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The winding number of the loops at a point.
fn winding(loops: &[Vec<[f64; 2]>], p: [f64; 2]) -> i32 {
    let mut w = 0;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
            if a[1] <= p[1] && b[1] > p[1] && cross > 0.0 {
                w += 1;
            } else if a[1] > p[1] && b[1] <= p[1] && cross < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// Two points beside the middle of each edge of a loop, the longest edge
/// first: one on the left of the edge and one on the right.
fn beside(l: &[[f64; 2]]) -> Vec<([f64; 2], [f64; 2])> {
    let n = l.len();
    let edge = |k: usize| (l[k], l[(k + 1) % n]);
    let len = |k: usize| {
        let (a, b) = edge(k);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    let mut order: Vec<usize> = (0..n).filter(|&k| len(k) > 0.0).collect();
    order.sort_by(|&i, &j| len(j).total_cmp(&len(i)));
    order
        .into_iter()
        .map(|k| {
            let (a, b) = edge(k);
            let e = 1e-6 / len(k);
            let (nx, ny) = (-(b[1] - a[1]) * e, (b[0] - a[0]) * e);
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            ([m[0] + nx, m[1] + ny], [m[0] - nx, m[1] - ny])
        })
        .collect()
}

/// The triangles of the fill of closed loops with a fill rule. A loop with
/// the fill on one side only is an outer edge or the edge of a hole.
fn fill(loops: &[Vec<[f64; 2]>], rule: FillRule) -> (Vec<[f64; 2]>, Vec<u32>) {
    let filled = |p: [f64; 2]| {
        let w = winding(loops, p);
        match rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    // Each loop as an outer edge or a hole, with a point inside it.
    let mut outers = Vec::new();
    let mut holes = Vec::new();
    for l in loops {
        // The fill is on one side of the loop at an edge that no other loop
        // touches.
        let ccw = area2(l) > 0.0;
        let sides = beside(l).into_iter().map(|(left, right)| {
            let (inner, outer) = if ccw { (left, right) } else { (right, left) };
            (inner, filled(inner), filled(outer))
        });
        let mut first = None;
        let mut found = None;
        for side in sides {
            first.get_or_insert(side);
            if side.1 != side.2 {
                found = Some(side);
                break;
            }
        }
        let Some((inner, filled_in, filled_out)) = found.or(first) else {
            continue;
        };
        match (filled_in, filled_out) {
            (false, true) => holes.push((l, inner)),
            (true, _) => outers.push(l),
            (false, false) => {}
        }
    }
    let inside = |outer: &[[f64; 2]], p| winding(&[outer.to_vec()], p) != 0;
    let mut assigned: Vec<Vec<&Vec<[f64; 2]>>> = vec![Vec::new(); outers.len()];
    for (hole, p) in holes {
        let smallest = (0..outers.len())
            .filter(|&o| inside(outers[o], p))
            .min_by(|&a, &b| area2(outers[a]).abs().total_cmp(&area2(outers[b]).abs()));
        if let Some(o) = smallest {
            assigned[o].push(hole);
        }
    }
    let (mut points, mut indices) = (Vec::new(), Vec::new());
    let mut earcut = earcut::Earcut::new();
    let mut triangles: Vec<u32> = Vec::new();
    for (outer, holes) in outers.into_iter().zip(assigned) {
        let base = points.len();
        points.extend_from_slice(outer);
        let mut starts = Vec::new();
        for hole in holes {
            starts.push((points.len() - base) as u32);
            points.extend_from_slice(hole);
        }
        earcut.earcut(points[base..].iter().copied(), &starts, &mut triangles);
        indices.extend(triangles.iter().map(|i| i + base as u32));
    }
    (points, indices)
}

/// The triangles of a stroke of `width`: a quad for each edge and a round
/// join at each corner.
fn stroke(path: &BezPath, width: f64) -> (Vec<[f64; 2]>, Vec<u32>) {
    const N: usize = 8;
    let half = width / 2.0;
    let (mut points, mut indices) = (Vec::new(), Vec::new());
    let disc = |c: [f64; 2], points: &mut Vec<[f64; 2]>, indices: &mut Vec<u32>| {
        let base = points.len() as u32;
        points.push(c);
        for i in 0..N {
            let a = i as f64 / N as f64 * TAU;
            points.push([c[0] + half * a.cos(), c[1] + half * a.sin()]);
            let (j, k) = (i as u32 + 1, (i + 1) as u32 % N as u32 + 1);
            indices.extend([base, base + j, base + k]);
        }
    };
    for (line, closed) in polylines(path) {
        let n = line.len();
        let edges = if closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            let (a, b) = (line[i], line[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.hypot(dy);
            if len == 0.0 {
                continue;
            }
            let (nx, ny) = (-dy / len * half, dx / len * half);
            let base = points.len() as u32;
            points.extend([
                [a[0] + nx, a[1] + ny],
                [b[0] + nx, b[1] + ny],
                [b[0] - nx, b[1] - ny],
                [a[0] - nx, a[1] - ny],
            ]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let corners = if closed { 0..n } else { 1..n.saturating_sub(1) };
        for i in corners {
            disc(line[i], &mut points, &mut indices);
        }
    }
    (points, indices)
}

/// The nominal scale denominator of the globe at the center of the disc. It
/// changes with the zoom and not with a pan or a turn.
pub fn globe_scale(view: &GlobeView, rect: Rect, radius_m: f64) -> f64 {
    scale_denominator(radius_m / view.radius(rect))
}

/// The places on the globe under a grid of points of `rect`.
pub fn view_places(view: &GlobeView, rect: Rect) -> Vec<LonLat> {
    const N: usize = 9;
    let t = |k: usize| k as f32 / (N - 1) as f32;
    let mut out = Vec::with_capacity(N * N);
    for i in 0..N {
        for j in 0..N {
            let pos = rect.min + egui::vec2(rect.width() * t(i), rect.height() * t(j));
            if let Some(d) = view.unproject(rect, pos) {
                let (lon, lat) = super::math::dir_to_lonlat(d);
                out.push([lon, lat]);
            }
        }
    }
    out
}

/// One symbol of a scatter fill on the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Instance {
    pub lonlat: LonLat,
    pub pos: Pos2,
    /// The longer side in points.
    pub size: f32,
    /// Degrees clockwise.
    pub rotation: f32,
}

/// The scatter of one polygon feature as the globe draws it.
pub struct GlobeScatter<'a> {
    pub cache: &'a mut ScatterCache,
    pub view: &'a GlobeView,
    pub rect: Rect,
    /// The sphere radius in meters.
    pub radius_m: f64,
    /// The places that the view shows, from `view_places`.
    pub places: &'a [LonLat],
    /// The symbols of the layer so far. The layer draws at most
    /// `scatter::MAX_POINTS`.
    pub count: usize,
}

impl GlobeScatter<'_> {
    /// The symbols of feature `index` of a layer that are on the near side
    /// of the globe and on the screen.
    pub fn instances(
        &mut self,
        layer: LayerId,
        index: usize,
        feature: &Feature,
        fill: &ScatterFill,
    ) -> Vec<Instance> {
        let scale = globe_scale(self.view, self.rect, self.radius_m);
        let points: Arc<Vec<ScatterPoint>> = self.cache.get(
            layer,
            index,
            feature,
            fill,
            scale,
            self.radius_m,
            self.places,
        );
        let reach = fill.size as f32;
        let shown = self.rect.expand(reach);
        let mut out = Vec::new();
        for p in points.iter() {
            if self.count >= scatter::MAX_POINTS {
                break;
            }
            let d = lonlat_to_dir(p.lonlat[0], p.lonlat[1]);
            let Some(pos) = self.view.project(self.rect, d) else {
                continue;
            };
            if !shown.contains(pos) {
                continue;
            }
            self.count += 1;
            out.push(Instance {
                lonlat: p.lonlat,
                pos,
                size: (fill.size * p.scale) as f32,
                rotation: p.rotation as f32,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_render::{Symbol, Symbols};

    /// The test symbols in `tests/data`.
    const FIXTURES: [&str; 9] = [
        "tree", "pine", "palm", "mountain", "hill", "marsh", "city", "capital", "castle",
    ];

    fn symbol(name: &str) -> Symbol {
        let path = format!("{}/tests/data/{name}.svg", env!("CARGO_MANIFEST_DIR"));
        Symbol::from_svg(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn mesh(name: &str) -> SymbolMesh {
        SymbolMesh::new(&symbol(name))
    }

    /// A point of the 32 by 32 box of the castle file in symbol units. The
    /// castle is 18 wide and 25 tall, with its middle at 16, 17.5.
    fn castle_point(x: f32, y: f32) -> [f32; 2] {
        [(x - 16.0) / 25.0, (y - 17.5) / 25.0]
    }

    #[test]
    fn the_castle_has_its_door_and_its_window() {
        let castle = mesh("castle");
        assert!(castle.covers(castle_point(9.0, 20.0)));
        assert!(castle.covers(castle_point(23.0, 28.0)));
        // The door and the window are holes.
        assert!(!castle.covers(castle_point(16.0, 28.0)));
        assert!(!castle.covers(castle_point(16.0, 14.0)));
        // The gap between two merlons.
        assert!(!castle.covers(castle_point(12.5, 6.0)));
    }

    #[test]
    fn each_test_symbol_has_triangles() {
        for name in FIXTURES {
            assert!(!mesh(name).indices.is_empty(), "{name}");
        }
    }

    #[test]
    fn a_tree_covers_its_trunk_and_its_crown() {
        let tree = mesh("tree");
        assert!(tree.covers([0.0, 0.4]));
        assert!(tree.covers([0.0, -0.2]));
        assert!(!tree.covers([-0.4, 0.4]));
    }

    #[test]
    fn a_replaced_symbol_gets_a_new_mesh() {
        let mut symbols = Symbols::default();
        symbols.insert("tree", symbol("tree"));
        let first = SymbolMesh::of(symbols.get("tree").unwrap());
        assert!(Arc::ptr_eq(
            &first,
            &SymbolMesh::of(symbols.get("tree").unwrap())
        ));
        symbols.insert("tree", symbol("castle"));
        let second = SymbolMesh::of(symbols.get("tree").unwrap());
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(second.indices, mesh("castle").indices);
    }

    #[test]
    fn append_scales_turns_and_tints() {
        let tree = mesh("tree");
        let mut out = egui::Mesh::default();
        let red = Color::rgb(255, 0, 0);
        tree.append(&mut out, Pos2::new(100.0, 50.0), 20.0, 90.0, Some(red));
        assert_eq!(out.vertices.len(), tree.vertices.len());
        let (min, max) = out.vertices.iter().fold(
            (Pos2::new(f32::MAX, f32::MAX), Pos2::new(f32::MIN, f32::MIN)),
            |(a, b), v| (a.min(v.pos), b.max(v.pos)),
        );
        // A turned tree is 20 points wide.
        assert!((max.x - min.x - 20.0).abs() < 0.1, "{min:?} {max:?}");
        assert!(out.vertices.iter().all(|v| v.color == egui::Color32::RED));
    }
}
