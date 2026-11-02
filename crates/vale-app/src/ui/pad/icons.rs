//! The icons of the iPad layout. An icon is a stroke path in a box of 24 units.

use eframe::egui::{self, Color32, Pos2, Rect, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Carve,
    Ridge,
    Deposit,
    Erode,
    Weather,
    Uplift,
    Warp,
    Push,
    Noise,
    OceanRidge,
    Range,
    Basin,
    More,
    Move,
    Undo,
    Redo,
    Layers,
    Toolbox,
    Brush,
    Close,
    Back,
    Down,
    Open,
    Tick,
    Eye,
    EyeOff,
    Raster,
    Point,
    Line,
    Polygon,
    Plus,
    Center,
    Reset,
    World,
    Grid,
    Delete,
    Magnet,
    /// The layout grid of the pages.
    LayoutGrid,
    Select,
    Reshape,
    Measure,
    /// The handle of a row that a drag moves.
    Grip,
}

impl Icon {
    /// The path, in the syntax of an SVG path without arcs.
    fn path(self) -> &'static str {
        match self {
            Icon::Raise => "M3 18c4 0 5-11 9-11s5 11 9 11",
            Icon::Lower => "M3 7c4 0 5 11 9 11s5-11 9-11",
            Icon::Smooth => "M3 12c3-4 6-4 9 0s6 4 9 0",
            Icon::Flatten => "M4 12h16",
            Icon::Carve => "M3 7c4 0 5 9 9 9s5-9 9-9M12 16v5",
            Icon::Ridge => "M3 9c2 0 3 8 5 8l4-11 4 11c2 0 3-8 5-8",
            Icon::Deposit => "M3 7c4 0 5 12 9 12s5-12 9-12M6.5 13h11M9 16.5h6",
            Icon::Erode => "M5 4c0 5 7 5 7 10v7M12 4v10M19 4c0 5-7 5-7 10",
            Icon::Weather => "M3 18l5-10 3 6 4-9 6 13",
            Icon::Uplift => "M3 20l6-8 4 5 3-4 5 7M12 9v-6M9 6l3-3 3 3",
            Icon::Warp => "M3 7h18M3 16c2-5 4 5 6 0s4-5 6 0s4 5 6 0",
            Icon::Push => "M3 19c3 0 4-8 7-8s4 8 7 8M14 6h7M18 3l3 3-3 3",
            Icon::Noise => "M2 16l3-4 2 3 3-8 2 6 2-3 3 7 2-4 3 3",
            Icon::OceanRidge => "M3 9h6v6h6v-6h6M9 4v16M15 4v16",
            Icon::Range => "M2 19l5-8 3 4 4-9 4 6 4 7M6 7l3-2M13 5l3-2",
            Icon::Basin => "M12 21V3M12 16l-7-5M12 16l7-4M12 10l-5-5M12 10l5-6M5 11l-1-5M19 12l1-5",
            Icon::More => "",
            Icon::Move => {
                "M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3\
                 M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3"
            }
            Icon::Undo => "M8 5L3 10l5 5M3 10h11c3.04 0 5.5 2.46 5.5 5.5s-2.46 5.5-5.5 5.5h-3",
            Icon::Redo => "M16 5l5 5-5 5M21 10H10c-3.04 0-5.5 2.46-5.5 5.5s2.46 5.5 5.5 5.5h3",
            Icon::Layers => "M12 4l9 5-9 5-9-5zM3 14l9 5 9-5",
            Icon::Toolbox => "M4 9h16v10H4zM9 9V6h6v3M4 14h16",
            Icon::Brush => "M5 4v16M12 4v16M19 4v16",
            Icon::Close => "M6 6l12 12M18 6L6 18",
            Icon::Delete => "M9 6h12v12H9L3 12L9 6M12 9l6 6M18 9l-6 6",
            Icon::Back => "M15 6l-6 6 6 6",
            Icon::Down => "M6 9l6 6 6-6",
            Icon::Open => "M9 6l6 6-6 6",
            Icon::Tick => "M5 12l5 5 9-10",
            Icon::Eye => "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z",
            Icon::EyeOff => "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM4 4l16 16",
            Icon::Raster => "M4 4h16v16H4zM4 12h16M12 4v16",
            Icon::Point => "",
            Icon::Line => "M4 18c4-10 8 2 16-12",
            Icon::Polygon => "M5 7l8-3 6 6-3 9-9-1z",
            Icon::Plus => "M12 5v14M5 12h14",
            Icon::Center => "M12 3v5M12 16v5M3 12h5M16 12h5",
            Icon::World => {
                "M12 3c4.97 0 9 4.03 9 9s-4.03 9-9 9-9-4.03-9-9 4.03-9 9-9z\
                 M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9zM3 12h18"
            }
            Icon::Grid => "M12 3v18M3 12h18",
            Icon::LayoutGrid => "M4 4h16v16h-16zM4 9.33h16M4 14.67h16M9.33 4v16M14.67 4v16",
            Icon::Grip => "M5 8h14M5 12h14M5 16h14",
            Icon::Select => "M5 3l14 8-6 2-2 6z",
            // A square with its top side redrawn as a curve.
            Icon::Reshape => "M6 8v12h12V8M6 8c2-6 10-6 12 0",
            Icon::Measure => "M3 17L17 3l4 4L7 21zM7 13l2 2M10 10l3 3M13 7l2 2",
            Icon::Magnet => {
                "M5 4h4v8c0 1.66 1.34 3 3 3s3-1.34 3-3V4h4v8c0 3.87-3.13 7-7 7s-7-3.13-7-7zM5 8h4M15 8h4"
            }
            Icon::Reset => {
                "M4 4v5h5M4.5 9c1.4-3 4.2-5 7.5-5 4.4 0 8 3.6 8 8s-3.6 8-8 8c-3.5 0-6.5-2.2-7.6-5.4"
            }
        }
    }

    /// Circles: center, radius, and filled or not.
    fn circles(self) -> &'static [(f32, f32, f32, bool)] {
        match self {
            Icon::Brush => &[
                (5.0, 9.0, 2.0, true),
                (12.0, 15.0, 2.0, true),
                (19.0, 8.0, 2.0, true),
            ],
            Icon::Eye | Icon::EyeOff | Icon::Point => &[(12.0, 12.0, 3.0, false)],
            Icon::Grid => &[(12.0, 12.0, 9.0, false)],
            Icon::Reshape => &[(6.0, 8.0, 1.5, true), (18.0, 8.0, 1.5, true)],
            Icon::More => &[
                (5.0, 12.0, 1.5, true),
                (12.0, 12.0, 1.5, true),
                (19.0, 12.0, 1.5, true),
            ],
            _ => &[],
        }
    }
}

/// Splits a path into commands. A command is a letter and its numbers.
fn tokens(d: &str) -> Vec<(char, Vec<f32>)> {
    let mut out: Vec<(char, Vec<f32>)> = Vec::new();
    let mut number = String::new();
    let flush = |number: &mut String, out: &mut Vec<(char, Vec<f32>)>| {
        if let (Ok(v), Some(last)) = (number.parse(), out.last_mut()) {
            last.1.push(v);
        }
        number.clear();
    };
    for c in d.chars() {
        match c {
            c if c.is_ascii_alphabetic() => {
                flush(&mut number, &mut out);
                out.push((c, Vec::new()));
            }
            '-' => {
                flush(&mut number, &mut out);
                number.push(c);
            }
            '.' if number.contains('.') => {
                flush(&mut number, &mut out);
                number.push(c);
            }
            c if c.is_ascii_digit() || c == '.' => number.push(c),
            _ => flush(&mut number, &mut out),
        }
    }
    flush(&mut number, &mut out);
    out
}

/// The points of a cubic curve after its start.
fn cubic(p: [Pos2; 4], out: &mut Vec<Pos2>) {
    const STEPS: usize = 12;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push(pos2(
            w[0] * p[0].x + w[1] * p[1].x + w[2] * p[2].x + w[3] * p[3].x,
            w[0] * p[0].y + w[1] * p[1].y + w[2] * p[2].y + w[3] * p[3].y,
        ));
    }
}

/// The lines of a path, in path units. A closed line ends at its first point.
pub fn parse(d: &str) -> Vec<Vec<Pos2>> {
    let mut lines: Vec<Vec<Pos2>> = Vec::new();
    let mut at = Pos2::ZERO;
    // The second control point of the last curve.
    let mut control: Option<Pos2> = None;
    for (command, numbers) in tokens(d) {
        let relative = command.is_ascii_lowercase();
        let base = |at: Pos2| {
            if relative {
                at.to_vec2()
            } else {
                egui::Vec2::ZERO
            }
        };
        let command = command.to_ascii_uppercase();
        let size = match command {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' => 4,
            _ => 0,
        };
        if command == 'Z' {
            if let Some(line) = lines.last_mut()
                && let Some(first) = line.first().copied()
            {
                line.push(first);
                at = first;
            }
            control = None;
            continue;
        }
        if size == 0 {
            continue;
        }
        for (index, n) in numbers.chunks_exact(size).enumerate() {
            let origin = base(at);
            let point = |i: usize| pos2(n[i], n[i + 1]) + origin;
            let mut next_control = None;
            match command {
                'M' if index == 0 => {
                    at = point(0);
                    lines.push(vec![at]);
                    continue;
                }
                'M' | 'L' => at = point(0),
                'H' => at = pos2(n[0] + origin.x, at.y),
                'V' => at = pos2(at.x, n[0] + origin.y),
                _ => {
                    let (c1, c2, end) = if command == 'C' {
                        (point(0), point(2), point(4))
                    } else {
                        let c1 = control.map_or(at, |c| at + (at - c));
                        (c1, point(0), point(2))
                    };
                    if let Some(line) = lines.last_mut() {
                        cubic([at, c1, c2, end], line);
                        // The last point is `end`. The code below adds it again.
                        line.pop();
                    }
                    next_control = Some(c2);
                    at = end;
                }
            }
            control = next_control;
            if let Some(line) = lines.last_mut() {
                line.push(at);
            }
        }
    }
    lines
}

/// Paints an icon at the center of a rectangle. `size` is the side of the icon.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, size: f32, color: Color32) {
    let scale = size / 24.0;
    let origin = rect.center() - egui::vec2(size, size) / 2.0;
    let place = |p: Pos2| origin + p.to_vec2() * scale;
    let width = 1.6 * size / 22.0;
    let stroke = egui::Stroke::new(width, color);
    for line in parse(icon.path()) {
        let points: Vec<Pos2> = line.into_iter().map(place).collect();
        // Egui has no round line ends.
        if let (Some(first), Some(last)) = (points.first(), points.last())
            && first != last
        {
            painter.circle_filled(*first, width / 2.0, color);
            painter.circle_filled(*last, width / 2.0, color);
        }
        painter.add(egui::Shape::line(points, stroke));
    }
    for &(x, y, radius, filled) in icon.circles() {
        let center = place(pos2(x, y));
        if filled {
            painter.circle_filled(center, radius * scale, color);
        } else {
            painter.circle_stroke(center, radius * scale, stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_relative_lines_and_a_close_gives_one_closed_line() {
        let lines = parse("M4 20l4-1 11-11-3-3L5 16z");
        let want = [(4, 20), (8, 19), (19, 8), (16, 5), (5, 16), (4, 20)];
        let want: Vec<Pos2> = want
            .iter()
            .map(|&(x, y)| pos2(x as f32, y as f32))
            .collect();
        assert_eq!(lines, vec![want]);
    }

    #[test]
    fn a_path_with_two_moves_gives_two_lines() {
        let lines = parse("M4 9h16v10H4zM9 9V6h6v3");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 5);
        assert_eq!(
            lines[1],
            vec![
                pos2(9.0, 9.0),
                pos2(9.0, 6.0),
                pos2(15.0, 6.0),
                pos2(15.0, 9.0)
            ]
        );
    }

    #[test]
    fn a_smooth_curve_mirrors_the_control_point_of_the_curve_before_it() {
        let lines = parse("M3 18c4 0 5-11 9-11s5 11 9 11");
        let line = &lines[0];
        assert_eq!(line.len(), 25);
        assert_eq!(line[12], pos2(12.0, 7.0));
        assert_eq!(line[24], pos2(21.0, 18.0));
        // The top of the hill is flat, so the points next to it are at the same height.
        assert!((line[11].y - line[13].y).abs() < 1e-3);
    }

    #[test]
    fn numbers_without_a_space_between_them_are_separate() {
        assert_eq!(
            tokens("s-2.46 5.5-5.5.5"),
            vec![('s', vec![-2.46, 5.5, -5.5, 0.5])]
        );
    }

    #[test]
    fn an_empty_path_gives_no_lines() {
        assert!(parse("").is_empty());
        assert_eq!(
            parse("M3 19l7-13 4 7 7 6"),
            vec![vec![
                pos2(3.0, 19.0),
                pos2(10.0, 6.0),
                pos2(14.0, 13.0),
                pos2(21.0, 19.0)
            ]]
        );
    }

    #[test]
    fn each_icon_stays_in_its_box() {
        for icon in [
            Icon::Raise,
            Icon::Lower,
            Icon::Smooth,
            Icon::Flatten,
            Icon::Carve,
            Icon::Ridge,
            Icon::Deposit,
            Icon::Erode,
            Icon::Weather,
            Icon::Uplift,
            Icon::Warp,
            Icon::Push,
            Icon::Noise,
            Icon::OceanRidge,
            Icon::Range,
            Icon::Basin,
            Icon::More,
            Icon::Move,
            Icon::Undo,
            Icon::Redo,
            Icon::Layers,
            Icon::Toolbox,
            Icon::Brush,
            Icon::Close,
            Icon::Back,
            Icon::Down,
            Icon::Open,
            Icon::Tick,
            Icon::Eye,
            Icon::EyeOff,
            Icon::Raster,
            Icon::Point,
            Icon::Line,
            Icon::Polygon,
            Icon::Plus,
            Icon::Center,
            Icon::Reset,
            Icon::World,
            Icon::Grid,
            Icon::LayoutGrid,
            Icon::Delete,
            Icon::Select,
            Icon::Reshape,
            Icon::Measure,
            Icon::Grip,
        ] {
            let lines = parse(icon.path());
            let circles = icon.circles();
            assert!(!lines.is_empty() || !circles.is_empty(), "{icon:?}");
            for p in lines.iter().flatten() {
                assert!(
                    (0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y),
                    "{icon:?} {p:?}"
                );
            }
            for &(x, y, radius, _) in circles {
                assert!(x - radius >= 0.0 && x + radius <= 24.0, "{icon:?}");
                assert!(y - radius >= 0.0 && y + radius <= 24.0, "{icon:?}");
            }
        }
    }
}
