//! The fallbacks of a label that does not fit: the text, the style, and the placement that each
//! one tries.

use crate::model::{Fallback, Fallbacks, LabelClass, Placement, PolygonMode};
use crate::text::{Fonts, ShapedText};

/// The space between the baselines of stacked lines, as a multiple of the font size.
const LINE_SPACING: f64 = 1.2;

/// The most font sizes of [`Fallback::SmallerFont`].
const MAX_STEPS: usize = 32;

/// One try of a fallback: the class with its changed style or placement, and the shaped text.
pub(crate) struct Attempt {
    pub class: LabelClass,
    pub shaped: ShapedText,
}

/// The tries of `fallback` for `text` in `class`, in order. A fallback that does not apply to
/// the class or the text gives none.
pub(crate) fn attempts(
    fonts: &mut Fonts,
    text: &str,
    class: &LabelClass,
    fallback: Fallback,
) -> Vec<Attempt> {
    let fb = &class.fallbacks;
    let mut out = Vec::new();
    let mut push = |class: LabelClass, shaped: ShapedText| {
        if shaped.glyph_count() > 0 && shaped.missing_glyphs == 0 {
            out.push(Attempt { class, shaped });
        }
    };
    match fallback {
        Fallback::SmallerFont => {
            for size in sizes(class.style.size, fb) {
                let mut c = class.clone();
                c.style.size = size;
                let shaped = fonts.shape(text, &c.style);
                push(c, shaped);
            }
        }
        Fallback::Stack => {
            let mut c = class.clone();
            match &mut c.placement {
                Placement::Point(pp) => pp.graticule = false,
                Placement::Polygon(pp) => {
                    pp.mode = PolygonMode::Horizontal;
                    pp.graticule = false;
                    pp.max_spread = 0.0;
                }
                Placement::Line(_) => return out,
            }
            for n in 2..=fb.max_lines {
                let Some(lines) = split_lines(text, n) else {
                    break;
                };
                let lines: Vec<ShapedText> =
                    lines.iter().map(|l| fonts.shape(l, &c.style)).collect();
                let shaped = ShapedText::stack(lines, LINE_SPACING * c.style.size);
                push(c.clone(), shaped);
            }
        }
        Fallback::Abbreviate => {
            if let Some(short) = abbreviate(text, &fb.abbreviations) {
                let shaped = fonts.shape(&short, &class.style);
                push(class.clone(), shaped);
            }
        }
        Fallback::Leader => {
            let mut c = class.clone();
            match &mut c.placement {
                Placement::Point(pp) => pp.offset += fb.leader_distance.max(0.0),
                Placement::Polygon(pp) => {
                    // A class that blocks every zone outside gets all of them for the fallback.
                    if pp.zones.outside.iter().all(|r| *r == 0) {
                        pp.zones.outside = [1; 8];
                    }
                    pp.repeat_distance = None;
                }
                Placement::Line(_) => return out,
            }
            let shaped = fonts.shape(text, &c.style);
            push(c, shaped);
        }
    }
    out
}

/// The font sizes below `size`, one step apart, down to the smallest size.
pub(crate) fn sizes(size: f64, fb: &Fallbacks) -> Vec<f64> {
    let mut out = Vec::new();
    if fb.size_step <= 0.0 || !fb.size_step.is_finite() {
        if fb.min_size < size && fb.min_size > 0.0 {
            out.push(fb.min_size);
        }
        return out;
    }
    let mut s = size - fb.size_step;
    while s >= fb.min_size - 1e-9 && s > 0.0 && out.len() < MAX_STEPS {
        out.push(s);
        s -= fb.size_step;
    }
    out
}

/// `text` with each word of `pairs` in its short form, or `None` if no word changes.
pub(crate) fn abbreviate(text: &str, pairs: &[(String, String)]) -> Option<String> {
    let mut changed = false;
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|w| match pairs.iter().find(|(long, _)| long == w) {
            Some((_, short)) if !short.trim().is_empty() => {
                changed = true;
                short.as_str()
            }
            _ => w,
        })
        .collect();
    changed.then(|| words.join(" "))
}

/// The words of `text` on `n` lines, with the longest line as short as possible, or `None` if
/// the text has fewer than `n` words. The length of a line is its number of characters.
pub(crate) fn split_lines(text: &str, n: usize) -> Option<Vec<String>> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let m = words.len();
    if n < 2 || m < n {
        return None;
    }
    let len = |a: usize, b: usize| {
        words[a..b].iter().map(|w| w.chars().count()).sum::<usize>() + (b - a - 1)
    };
    // `best[k][i]` is the longest line and the end of the first line, for the words from `i`
    // on `k` lines.
    let mut best = vec![vec![(usize::MAX, 0); m + 1]; n + 1];
    best[1] = (0..=m)
        .map(|i| {
            if i < m {
                (len(i, m), m)
            } else {
                (usize::MAX, m)
            }
        })
        .collect();
    for k in 2..=n {
        for i in 0..m {
            for end in i + 1..m {
                let rest = best[k - 1][end].0;
                if rest == usize::MAX {
                    continue;
                }
                let worst = len(i, end).max(rest);
                if worst < best[k][i].0 {
                    best[k][i] = (worst, end);
                }
            }
        }
    }
    let mut lines = Vec::with_capacity(n);
    let mut i = 0;
    for k in (1..=n).rev() {
        let end = best[k][i].1;
        lines.push(words[i..end].join(" "));
        i = end;
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_whole_words() {
        let pairs = vec![
            ("Mount".to_string(), "Mt.".to_string()),
            ("Saint".to_string(), "St.".to_string()),
        ];
        assert_eq!(
            abbreviate("Mount Saint Helens", &pairs).as_deref(),
            Some("Mt. St. Helens")
        );
        assert_eq!(abbreviate("Mountains of Mourne", &pairs), None);
    }

    #[test]
    fn splits_into_balanced_lines() {
        assert_eq!(
            split_lines("Gulf of Saint Lawrence", 2).unwrap(),
            vec!["Gulf of Saint", "Lawrence"]
        );
        assert_eq!(
            split_lines("Bay of Bengal", 3).unwrap(),
            vec!["Bay", "of", "Bengal"]
        );
        assert_eq!(split_lines("Tasmania", 2), None);
    }

    #[test]
    fn steps_down_to_the_smallest_size() {
        let fb = Fallbacks {
            min_size: 7.0,
            size_step: 1.5,
            ..Fallbacks::default()
        };
        assert_eq!(sizes(11.0, &fb), vec![9.5, 8.0]);
        assert!(sizes(6.0, &fb).is_empty());
    }
}
