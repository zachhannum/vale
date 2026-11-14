//! The fallbacks of a label that does not fit: the text, the style, and the placement that each
//! one tries.

use crate::model::{Fallback, Fallbacks, LabelClass, Placement, PolygonMode};
use crate::text::{Fonts, ShapedText};

/// The space between the baselines of stacked lines, as a multiple of the font size.
const LINE_SPACING: f64 = 1.2;

/// The most font sizes of [`Fallback::SmallerFont`], the most widths of
/// [`Fallback::Compress`], and the most letters that [`Fallback::Truncate`] removes.
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
            let mut narrow = vec![1.0];
            if fb.compress_with_size {
                narrow.extend(widths(fb));
            }
            let all = std::iter::once(class.style.size).chain(sizes(class.style.size, fb));
            for (i, size) in all.enumerate() {
                let mut c = class.clone();
                c.style.size = size;
                for &w in &narrow[usize::from(i == 0)..] {
                    let shaped = fonts.shape(text, &c.style).compress(w);
                    push(c.clone(), shaped);
                }
            }
        }
        Fallback::Compress => {
            for w in widths(fb) {
                let shaped = fonts.shape(text, &class.style).compress(w);
                push(class.clone(), shaped);
            }
        }
        Fallback::Truncate => {
            for short in truncations(text, &fb.truncation_marker) {
                let shaped = fonts.shape(&short, &class.style);
                push(class.clone(), shaped);
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
            let pieces = pieces(text, &fb.stack_separators);
            let limits = (fb.min_line_chars, fb.max_line_chars);
            for n in 2..=fb.max_lines.min(pieces.len()) {
                let Some(lines) = split_lines(&pieces, n, limits) else {
                    continue;
                };
                let lines: Vec<ShapedText> =
                    lines.iter().map(|l| fonts.shape(l, &c.style)).collect();
                let shaped = ShapedText::stack(lines, LINE_SPACING * c.style.size, fb.stack_align);
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

/// The widths below the full width, one step apart, down to the smallest width.
pub(crate) fn widths(fb: &Fallbacks) -> Vec<f64> {
    let min = fb.min_width;
    let mut out = Vec::new();
    if !(min > 0.0 && min < 1.0) {
        return out;
    }
    if fb.width_step <= 0.0 || !fb.width_step.is_finite() {
        out.push(min);
        return out;
    }
    let mut w = 1.0 - fb.width_step;
    while w >= min - 1e-9 && out.len() < MAX_STEPS {
        out.push(w);
        w -= fb.width_step;
    }
    out
}

/// Whether `c` is a vowel, which truncation removes first.
fn is_vowel(c: char) -> bool {
    let lower = c.to_lowercase().next().unwrap_or(c);
    "aeiouàáâãäåæèéêëìíîïòóôõöøùúûüœ".contains(lower)
}

/// The texts of each step of truncation, from the longest to the shortest. Each step removes
/// one letter from the longest word: the last vowel, or the last letter if no vowel is left. The
/// first letter stays, and each short word ends with `marker`.
pub(crate) fn truncations(text: &str, marker: &str) -> Vec<String> {
    let mut words: Vec<(Vec<char>, bool)> = text
        .split_whitespace()
        .map(|w| (w.chars().collect(), false))
        .collect();
    let mut out = Vec::new();
    let mut done = vec![false; words.len()];
    while out.len() < MAX_STEPS {
        let Some(i) = (0..words.len())
            .filter(|&i| !done[i])
            .max_by(|&a, &b| words[a].0.len().cmp(&words[b].0.len()).then(b.cmp(&a)))
        else {
            break;
        };
        let letters = &mut words[i].0;
        let pick = |f: &dyn Fn(char) -> bool| (1..letters.len()).rev().find(|&k| f(letters[k]));
        let Some(k) = pick(&|c| is_vowel(c)).or_else(|| pick(&|c| c.is_alphabetic())) else {
            done[i] = true;
            continue;
        };
        letters.remove(k);
        words[i].1 = true;
        let words: Vec<String> = words
            .iter()
            .map(|(w, short)| {
                let mut s: String = w.iter().collect();
                if *short {
                    s.push_str(marker);
                }
                s
            })
            .collect();
        out.push(words.join(" "));
    }
    out
}

/// One part of a text that a stack can put on a new line.
pub(crate) struct Piece {
    pub text: String,
    /// A space separates the piece from the next piece on the same line.
    pub space_after: bool,
}

/// The pieces of `text`, with a break after each character of `separators`. A space at a break
/// goes away, and another separator stays at the end of its piece.
pub(crate) fn pieces(text: &str, separators: &str) -> Vec<Piece> {
    let break_at_space = separators.chars().any(char::is_whitespace);
    let mut out = Vec::new();
    let mut cur = String::new();
    let end = |cur: &mut String, space_after: bool, out: &mut Vec<Piece>| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(Piece {
                text: t.to_string(),
                space_after,
            });
        } else if let Some(last) = out.last_mut() {
            last.space_after |= space_after;
        }
        cur.clear();
    };
    for c in text.trim().chars() {
        if c.is_whitespace() {
            if break_at_space {
                end(&mut cur, true, &mut out);
            } else if !cur.ends_with(' ') {
                cur.push(' ');
            }
        } else {
            cur.push(c);
            if separators.contains(c) {
                end(&mut cur, false, &mut out);
            }
        }
    }
    end(&mut cur, false, &mut out);
    out
}

/// The pieces on `n` lines, with the longest line as short as possible, or `None` if no split
/// gives each line from `limits.0` to `limits.1` characters.
pub(crate) fn split_lines(
    pieces: &[Piece],
    n: usize,
    limits: (usize, usize),
) -> Option<Vec<String>> {
    let m = pieces.len();
    if n < 2 || m < n {
        return None;
    }
    let line = |a: usize, b: usize| {
        let mut s = String::new();
        for (k, p) in pieces[a..b].iter().enumerate() {
            if k > 0 && pieces[a + k - 1].space_after {
                s.push(' ');
            }
            s.push_str(&p.text);
        }
        s
    };
    let len = |a: usize, b: usize| {
        let l = line(a, b).chars().count();
        (limits.0..=limits.1).contains(&l).then_some(l)
    };
    // `best[k][i]` is the longest line and the end of the first line, for the pieces from `i`
    // on `k` lines.
    let mut best = vec![vec![(usize::MAX, 0); m + 1]; n + 1];
    for (i, b) in best[1].iter_mut().enumerate().take(m) {
        if let Some(l) = len(i, m) {
            *b = (l, m);
        }
    }
    for k in 2..=n {
        for i in 0..m {
            for end in i + 1..m {
                let rest = best[k - 1][end].0;
                if rest == usize::MAX {
                    continue;
                }
                let Some(first) = len(i, end) else { continue };
                let worst = first.max(rest);
                if worst < best[k][i].0 {
                    best[k][i] = (worst, end);
                }
            }
        }
    }
    if best[n][0].0 == usize::MAX {
        return None;
    }
    let mut lines = Vec::with_capacity(n);
    let mut i = 0;
    for k in (1..=n).rev() {
        let end = best[k][i].1;
        lines.push(line(i, end));
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

    fn split(
        text: &str,
        separators: &str,
        n: usize,
        limits: (usize, usize),
    ) -> Option<Vec<String>> {
        split_lines(&pieces(text, separators), n, limits)
    }

    #[test]
    fn splits_into_balanced_lines() {
        assert_eq!(
            split("Gulf of Saint Lawrence", " ", 2, (1, 64)).unwrap(),
            vec!["Gulf of Saint", "Lawrence"]
        );
        assert_eq!(
            split("Bay of Bengal", " ", 3, (1, 64)).unwrap(),
            vec!["Bay", "of", "Bengal"]
        );
        assert_eq!(split("Tasmania", " ", 2, (1, 64)), None);
    }

    #[test]
    fn a_visible_separator_stays_at_the_end_of_the_line() {
        assert_eq!(
            split("Stratford-upon-Avon", "-", 2, (1, 64)).unwrap(),
            vec!["Stratford-", "upon-Avon"]
        );
        assert_eq!(
            split("Port of Spain-Town", "-", 2, (1, 64)).unwrap(),
            vec!["Port of Spain-", "Town"]
        );
        assert_eq!(split("Stratford-upon-Avon", " ", 2, (1, 64)), None);
    }

    #[test]
    fn each_line_keeps_inside_the_character_limits() {
        // The best split puts "of" alone, which is under the limit.
        assert_eq!(split("Bay of Bengal", " ", 3, (3, 64)), None);
        assert_eq!(
            split("Bay of Bengal", " ", 2, (3, 64)).unwrap(),
            vec!["Bay of", "Bengal"]
        );
        assert_eq!(split("Gulf of Saint Lawrence", " ", 2, (1, 10)), None);
        assert_eq!(
            split("Gulf of Saint Lawrence", " ", 3, (1, 10)).unwrap(),
            vec!["Gulf", "of Saint", "Lawrence"]
        );
    }

    #[test]
    fn truncates_the_longest_word_vowels_first() {
        let steps = truncations("Big Lakeshore", ".");
        assert_eq!(steps[0], "Big Lakeshor.");
        assert_eq!(steps[1], "Big Lakeshr.");
        assert_eq!(steps[2], "Big Lakshr.");
        assert_eq!(steps[3], "Big Lkshr.");
        assert_eq!(steps[4], "Big Lksh.");
        // The first letter of each word stays.
        let last = steps.last().unwrap();
        assert_eq!(last, "B. L.");
        assert!(truncations("A", ".").is_empty());
    }

    #[test]
    fn narrows_down_to_the_smallest_width() {
        let fb = Fallbacks {
            min_width: 0.8,
            width_step: 0.1,
            ..Fallbacks::default()
        };
        let w = widths(&fb);
        assert_eq!(w.len(), 2);
        assert!((w[0] - 0.9).abs() < 1e-9 && (w[1] - 0.8).abs() < 1e-9);
    }

    #[test]
    fn truncation_follows_the_last_text_fallback() {
        use Fallback::*;
        assert_eq!(
            Fallbacks::normalized_order(&[Truncate, SmallerFont, Leader, Stack]),
            vec![SmallerFont, Leader, Stack, Truncate]
        );
        assert_eq!(
            Fallbacks::normalized_order(&[Stack, Truncate, Leader]),
            vec![Stack, Truncate, Leader]
        );
        assert_eq!(
            Fallbacks::normalized_order(&[Stack, Leader, Truncate, Stack]),
            vec![Stack, Leader, Truncate]
        );
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
