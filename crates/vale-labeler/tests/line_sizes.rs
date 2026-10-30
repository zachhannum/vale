//! Line labels on hand-drawn rivers hold across font sizes.

use std::f64::consts::PI;
use vale_labeler::*;

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    for name in ["NotoSans-Regular.ttf", "NotoSans-Italic.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.register(std::fs::read(path).unwrap()).unwrap();
    }
    f
}

/// A fixed pseudo-random sequence.
struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 = (self.0)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// About normal, with a standard deviation of 0.58.
    fn normal(&mut self) -> f64 {
        (0..4).map(|_| self.uniform()).sum::<f64>() - 2.0
    }
}

/// A river of `len` pt along a line at `rot` radians: a sum of sine waves of
/// (amplitude, wavelength, phase), with a point every 1 to 3 pt and pen jitter.
fn river(len: f64, waves: &[(f64, f64, f64)], jitter: f64, seed: u64, rot: f64) -> Vec<Point> {
    let mut rng = Rng(seed);
    let (sin, cos) = rot.sin_cos();
    let mut pts = Vec::new();
    let mut x = 0.0;
    while x <= len {
        let y: f64 = (waves.iter())
            .map(|&(a, l, ph)| a * (2.0 * PI * x / l + ph).sin())
            .sum();
        let (px, py) = (
            x - len / 2.0 + jitter * rng.normal(),
            y + jitter * rng.normal(),
        );
        pts.push(Point::new(
            400.0 + cos * px - sin * py,
            300.0 + sin * px + cos * py,
        ));
        x += 1.0 + 2.0 * rng.uniform();
    }
    pts
}

/// The default river class of the app.
fn class(size: f64) -> LabelClass {
    LabelClass {
        name: "rivers".into(),
        style: TextStyle {
            italic: true,
            letter_spacing: 0.5,
            ..TextStyle::new("Noto Sans", size)
        },
        priority: 0,
        margin: 1.0,
        placement: Placement::Line(LinePlacement {
            sides: vec![LineSide::Above, LineSide::Below],
            offset: 4.0,
            repeat_distance: Some(600.0),
            max_glyph_turn: 0.30,
            smoothing: 1.5,
        }),
        duplicate_distance: Some(300.0),
    }
}

/// The outline of each glyph of `label` in page space, as closed polygons.
fn glyph_polygons(label: &PlacedLabel) -> Vec<Vec<Vec<Point>>> {
    let mut out = Vec::new();
    for run in &label.glyph_runs {
        for g in &run.glyphs {
            let Some(path) = glyph_outline(&run.font, &run.normalized_coords, run.font_size, g.id)
            else {
                continue;
            };
            let mut polys: Vec<Vec<Point>> = Vec::new();
            kurbo::flatten(g.transform * path, 0.02, |el| match el {
                kurbo::PathEl::MoveTo(p) => polys.push(vec![p]),
                kurbo::PathEl::LineTo(p) => polys.last_mut().unwrap().push(p),
                _ => {}
            });
            out.push(polys);
        }
    }
    out
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let side = |p: Point, q: Point, r: Point| (q - p).cross(r - p);
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// True if the outlines of two glyphs of `label` cross.
fn glyphs_overlap(label: &PlacedLabel) -> bool {
    let glyphs = glyph_polygons(label);
    let edges = |polys: &Vec<Vec<Point>>| -> Vec<(Point, Point)> {
        polys
            .iter()
            .flat_map(|p| (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()])))
            .collect()
    };
    let edges: Vec<Vec<(Point, Point)>> = glyphs.iter().map(edges).collect();
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            for &(a, b) in &edges[i] {
                if edges[j].iter().any(|&(c, d)| segments_cross(a, b, c, d)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether the first label of `path` is placed, at each size from 6 to 24 pt in steps of 0.5 pt.
/// Panics if a placed label has glyphs that overlap.
fn sweep(fonts: &mut Fonts, path: &[Point], text: &str) -> Vec<bool> {
    (12..=48)
        .map(|i| {
            let input = LabelInput {
                bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
                classes: vec![class(i as f64 * 0.5)],
                features: vec![Feature {
                    id: 1,
                    class: 0,
                    text: text.into(),
                    priority: 0.0,
                    geometry: Geometry::Line {
                        path: path.to_vec(),
                        stroke_width: 1.0,
                    },
                }],
                obstacles: vec![],
                options: Options::default(),
            };
            let out = place_labels(fonts, &input);
            for label in &out.placed {
                assert!(
                    !glyphs_overlap(label),
                    "{text} at {} pt has glyphs that overlap",
                    i as f64 * 0.5
                );
            }
            out.placed.iter().any(|p| p.repeat == 0)
        })
        .collect()
}

#[test]
fn line_labels_hold_across_font_sizes() {
    let mut fonts = fonts();
    let meander = [(25.0, 130.0, 0.0), (6.0, 45.0, 1.0)];
    let lines = [
        ("meander", river(400.0, &meander, 0.0, 2, -0.2), 0.85),
        (
            "meander with jitter",
            river(400.0, &meander, 0.35, 2, -0.2),
            0.85,
        ),
        (
            "tight",
            river(300.0, &[(18.0, 80.0, 0.5)], 0.35, 3, 0.4),
            0.6,
        ),
        // "Long Water River" at 12 pt once drew "Wa" over "ter" on this line.
        (
            "long meander with jitter",
            river(
                500.0,
                &[(30.0, 260.0, 0.0), (8.0, 70.0, 2.0), (2.0, 18.0, 0.0)],
                0.35,
                4,
                0.0,
            ),
            0.95,
        ),
    ];
    for (line, path, min_rate) in &lines {
        let mut placed = 0;
        let mut total = 0;
        for text in ["Ash", "Silverrun", "River Mere", "Long Water River"] {
            let row = sweep(&mut fonts, path, text);
            let flips = row.windows(2).filter(|w| w[0] && !w[1]).count();
            assert!(
                flips <= 1,
                "{line}, {text}: {flips} on-off flips in {row:?}"
            );
            placed += row.iter().filter(|p| **p).count();
            total += row.len();
        }
        let rate = placed as f64 / total as f64;
        assert!(rate >= *min_rate, "{line}: {rate:.2} placed");
    }
}
