//! The colors of a legend image: a row of blocks of color, from left to
//! right, or a column of blocks, from bottom to top.

use crate::bands::Rgb;

/// The largest distance between two colors of one block, in RGB units.
const SAME: f64 = 10.0;
/// The least width of a block, as a part of the width of the run that
/// holds the middle pixel. A narrower run is an edge, a border, or a gap.
const MIN_WIDTH: f64 = 0.4;

/// The colors of the blocks of a legend image, in order. `pixels` has
/// `width * height` colors, row by row from the top. An image that is
/// taller than it is wide is a column of blocks, and its order goes up.
///
/// The colors come from the middle half of the image across the blocks,
/// so a label above or below the blocks does not count. Edges, borders,
/// gaps, and a margin at the two ends in the color of the four corners
/// are not blocks.
pub fn swatches(width: usize, height: usize, pixels: &[Rgb]) -> Vec<Rgb> {
    if width == 0 || height == 0 || pixels.len() != width * height {
        return Vec::new();
    }
    let tall = height > width;
    let (along, across) = if tall {
        (height, width)
    } else {
        (width, height)
    };
    let at = |i: usize, j: usize| {
        if tall {
            pixels[(height - 1 - i) * width + j]
        } else {
            pixels[j * width + i]
        }
    };

    let (from, to) = (across / 4, across - across / 4);
    let line: Vec<Rgb> = (0..along)
        .map(|i| median((from..to).map(|j| at(i, j))))
        .collect();

    // Runs of columns of one color.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut sum = [0.0; 3];
    for (i, &color) in line.iter().enumerate() {
        let count = (i - start) as f64;
        let mean = sum.map(|s| s / count.max(1.0));
        if i > start && distance(color, mean) > SAME {
            runs.push((start, i));
            start = i;
            sum = [0.0; 3];
        }
        for (s, c) in sum.iter_mut().zip(color) {
            *s += f64::from(c);
        }
    }
    runs.push((start, along));

    let mut widths: Vec<usize> = runs.iter().map(|(a, b)| b - a).collect();
    widths.sort_unstable();
    let mut sum = 0;
    let middle = widths.iter().find(|&&w| {
        sum += w;
        2 * sum >= along
    });
    let least = (*middle.unwrap_or(&along) as f64 * MIN_WIDTH).max(2.0);
    runs.retain(|(a, b)| (b - a) as f64 >= least);

    // A run keeps its inner columns, away from the blur of its edges.
    let mut colors: Vec<Rgb> = runs
        .iter()
        .map(|&(a, b)| {
            let trim = if b - a > 4 { 1 } else { 0 };
            median(line[a + trim..b - trim].iter().copied())
        })
        .collect();

    let corners = [
        at(0, 0),
        at(0, across - 1),
        at(along - 1, 0),
        at(along - 1, across - 1),
    ];
    let back = corners[0];
    let margin = |color: Rgb| distance(color, back.map(f64::from)) <= SAME;
    if corners.iter().all(|&c| margin(c)) {
        if colors.last().is_some_and(|&c| margin(c)) {
            colors.pop();
        }
        if colors.first().is_some_and(|&c| margin(c)) {
            colors.remove(0);
        }
    }
    colors
}

/// The median of each channel.
fn median(colors: impl Iterator<Item = Rgb>) -> Rgb {
    let mut channels: [Vec<u8>; 3] = Default::default();
    for color in colors {
        for (channel, c) in channels.iter_mut().zip(color) {
            channel.push(c);
        }
    }
    channels.map(|mut channel| {
        channel.sort_unstable();
        channel.get(channel.len() / 2).copied().unwrap_or_default()
    })
}

fn distance(a: Rgb, b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&a, b)| (f64::from(a) - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = [255; 3];
    const BLOCKS: [Rgb; 5] = [
        [16, 44, 92],
        [190, 222, 240],
        [108, 164, 100],
        [222, 202, 148],
        [150, 108, 76],
    ];

    /// A legend image. Each item is a width and a color, from the left. The
    /// rows of `label` at the bottom are black.
    fn image(runs: &[(usize, Rgb)], height: usize, label: usize) -> (usize, Vec<Rgb>) {
        let row: Vec<Rgb> = runs
            .iter()
            .flat_map(|&(w, c)| std::iter::repeat_n(c, w))
            .collect();
        let width = row.len();
        let mut pixels = Vec::new();
        for y in 0..height {
            if y >= height - label {
                pixels.extend(std::iter::repeat_n([0; 3], width));
            } else {
                pixels.extend(&row);
            }
        }
        (width, pixels)
    }

    #[test]
    fn blocks_that_touch_give_their_colors() {
        let runs: Vec<_> = BLOCKS.iter().map(|&c| (40, c)).collect();
        let (width, pixels) = image(&runs, 20, 0);
        assert_eq!(swatches(width, 20, &pixels), BLOCKS);
    }

    #[test]
    fn gaps_borders_and_a_margin_are_not_blocks() {
        let mut runs = vec![(30, WHITE)];
        for &c in &BLOCKS {
            runs.extend([(1, [0; 3]), (40, c), (1, [0; 3]), (8, WHITE)]);
        }
        runs.push((22, WHITE));
        let (width, pixels) = image(&runs, 30, 0);
        assert_eq!(swatches(width, 30, &pixels), BLOCKS);
    }

    #[test]
    fn a_white_block_inside_stays() {
        let colors = [BLOCKS[0], WHITE, BLOCKS[1]];
        let runs: Vec<_> = colors.iter().map(|&c| (30, c)).collect();
        let (width, pixels) = image(&runs, 10, 0);
        assert_eq!(swatches(width, 10, &pixels), colors);
    }

    #[test]
    fn noise_and_blurred_edges_do_not_split_a_block() {
        let mut runs = Vec::new();
        for w in BLOCKS.windows(2) {
            runs.push((40, w[0]));
            let mid: Rgb =
                std::array::from_fn(|c| ((u16::from(w[0][c]) + u16::from(w[1][c])) / 2) as u8);
            runs.push((1, mid));
        }
        runs.push((40, BLOCKS[4]));
        let (width, mut pixels) = image(&runs, 20, 0);
        for (i, p) in pixels.iter_mut().enumerate() {
            let jitter = (i * 7919 % 13) as i16 - 6;
            *p = p.map(|c| (i16::from(c) + jitter).clamp(0, 255) as u8);
        }
        let found = swatches(width, 20, &pixels);
        assert_eq!(found.len(), BLOCKS.len());
        for (a, b) in found.iter().zip(BLOCKS) {
            assert!(distance(*a, b.map(f64::from)) < 8.0, "{a:?} {b:?}");
        }
    }

    #[test]
    fn a_label_below_the_blocks_does_not_count() {
        let runs: Vec<_> = BLOCKS.iter().map(|&c| (40, c)).collect();
        let (width, pixels) = image(&runs, 40, 9);
        assert_eq!(swatches(width, 40, &pixels), BLOCKS);
    }

    #[test]
    fn a_column_of_blocks_goes_up() {
        let (height, width) = (BLOCKS.len() * 30, 10);
        let pixels: Vec<Rgb> = (0..height)
            .flat_map(|y| std::iter::repeat_n(BLOCKS[BLOCKS.len() - 1 - y / 30], width))
            .collect();
        assert_eq!(swatches(width, height, &pixels), BLOCKS);
    }

    #[test]
    fn close_colors_are_two_blocks() {
        let colors: Vec<Rgb> = (0..6).map(|i| [100 + 12 * i, 120, 140]).collect();
        let runs: Vec<_> = colors.iter().map(|&c| (25, c)).collect();
        let (width, pixels) = image(&runs, 10, 0);
        assert_eq!(swatches(width, 10, &pixels), colors);
    }

    #[test]
    fn an_empty_image_has_no_colors() {
        assert!(swatches(0, 0, &[]).is_empty());
        assert!(swatches(2, 2, &[WHITE]).is_empty());
    }
}
