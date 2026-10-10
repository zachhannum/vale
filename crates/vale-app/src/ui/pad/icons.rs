//! The icons of the iPad layout. An icon is a stroke path in a box of 24 units.

use eframe::egui::{self, Color32, Pos2, Rect, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Line,
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
    Raster,
}

impl Icon {
    /// The path, in the syntax of an SVG path without arcs.
    fn path(self) -> &'static str {
        match self {
            Icon::Raise => "M3 18c4 0 5-11 9-11s5 11 9 11",
            Icon::Lower => "M3 7c4 0 5 11 9 11s5-11 9-11",
            Icon::Smooth => "M3 12c3-4 6-4 9 0s6 4 9 0",
            Icon::Flatten => "M4 12h16",
            Icon::Line => "M4 20l4-1 11-11-3-3L5 16z",
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
            Icon::Back => "M15 6l-6 6 6 6",
            Icon::Down => "M6 9l6 6 6-6",
            Icon::Open => "M9 6l6 6-6 6",
            Icon::Tick => "M5 12l5 5 9-10",
            Icon::Eye => "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z",
            Icon::Raster => "M4 4h16v16H4zM4 12h16M12 4v16",
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
            Icon::Eye => &[(12.0, 12.0, 3.0, false)],
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
    fn each_icon_stays_in_its_box() {
        for icon in [
            Icon::Raise,
            Icon::Lower,
            Icon::Smooth,
            Icon::Flatten,
            Icon::Line,
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
            Icon::Raster,
        ] {
            let lines = parse(icon.path());
            assert!(!lines.is_empty(), "{icon:?}");
            for p in lines.iter().flatten() {
                assert!(
                    (0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y),
                    "{icon:?} {p:?}"
                );
            }
        }
    }
}
