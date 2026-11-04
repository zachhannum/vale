//! Classes of the values of a field or an expression, and a rule for each
//! class. Numbers have three methods. Text gives a class for each common
//! value and one class for the other values.

use std::collections::HashMap;

use vale_store::{GeometryKind, Layer, Value};
use vale_style::expr::{Context, Expr, Filter, Property};
use vale_style::{Color, Rule, Stroke};
use vale_terrain::Rgb;

/// The methods that find the edges of number classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Classes of the same width.
    EqualIntervals,
    /// Classes with the same number of values.
    Quantiles,
    /// Edges in the largest gaps between the values (Jenks).
    NaturalBreaks,
}

impl Method {
    pub const ALL: [Method; 3] = [
        Method::EqualIntervals,
        Method::Quantiles,
        Method::NaturalBreaks,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Method::EqualIntervals => "Equal intervals",
            Method::Quantiles => "Quantiles",
            Method::NaturalBreaks => "Natural breaks",
        }
    }
}

/// The smallest and the largest number of number classes.
pub const CLASSES: (usize, usize) = (2, 12);
pub const DEFAULT_CLASSES: usize = 5;
/// The most text values that get a class of their own.
pub const MAX_CATEGORIES: usize = 12;
/// The name of the rule of the text values that have no class.
pub const OTHER_VALUES: &str = "Other values";
/// The name of the rule of the features with no number.
pub const NO_VALUE: &str = "No value";
/// The colors of the rules of `OTHER_VALUES` and `NO_VALUE`.
const OTHER_COLOR: Color = Color::rgb(180, 180, 180);
/// The largest number of values that natural breaks reads. A larger set
/// gives evenly spaced values of the sorted set.
const JENKS_VALUES: usize = 1000;

/// The value of an expression for each feature of a layer.
pub fn values(layer: &Layer, expr: &Expr) -> Vec<Value> {
    layer
        .features
        .iter()
        .map(|f| {
            expr.eval(&Context {
                attributes: &f.attributes,
                scale: 0.0,
                geometry: f.geometry.kind(),
            })
        })
        .collect()
}

/// The values as numbers, sorted, and the count of the null values. `None`
/// if a value is not a number or null, or if no value is a number.
pub fn numbers(values: &[Value]) -> Option<(Vec<f64>, usize)> {
    let mut out = Vec::with_capacity(values.len());
    let mut nulls = 0;
    for v in values {
        match v {
            Value::Number(n) if n.is_finite() => out.push(*n),
            Value::Number(_) | Value::Null => nulls += 1,
            _ => return None,
        }
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by(f64::total_cmp);
    Some((out, nulls))
}

/// The inner edges of `n` classes of sorted values, ascending and with no
/// repeats. Equal values stay in one class, so there can be fewer than
/// `n - 1` edges. A value equal to an edge is in the class above the edge.
pub fn breaks(sorted: &[f64], method: Method, n: usize) -> Vec<f64> {
    let mut edges = match method {
        Method::EqualIntervals => equal_intervals(sorted, n),
        Method::Quantiles => quantiles(sorted, n),
        Method::NaturalBreaks => natural_breaks(sorted, n),
    };
    edges.dedup();
    edges
}

fn equal_intervals(sorted: &[f64], n: usize) -> Vec<f64> {
    let (Some(&min), Some(&max)) = (sorted.first(), sorted.last()) else {
        return Vec::new();
    };
    if n < 2 || max <= min {
        return Vec::new();
    }
    let width = (max - min) / n as f64;
    let exp = width.log10().floor() as i32 - 1;
    (1..n)
        .map(|k| round_to(min + width * k as f64, exp))
        .filter(|e| *e > min && *e <= max)
        .collect()
}

fn quantiles(sorted: &[f64], n: usize) -> Vec<f64> {
    let len = sorted.len();
    if n < 2 || len < 2 {
        return Vec::new();
    }
    // The indexes `i` where a class can start: the value differs from the one before.
    let starts: Vec<usize> = (1..len).filter(|&i| sorted[i - 1] < sorted[i]).collect();
    let mut out = Vec::new();
    let mut last = 0;
    for k in 1..n {
        let i = k * len / n;
        let Some(&j) = starts
            .iter()
            .filter(|&&j| j > last)
            .min_by_key(|&&j| (j as isize - i as isize).unsigned_abs())
        else {
            break;
        };
        last = j;
        out.push(roundest(sorted[j - 1], sorted[j]));
    }
    out
}

fn natural_breaks(sorted: &[f64], n: usize) -> Vec<f64> {
    let values: Vec<f64> = if sorted.len() > JENKS_VALUES {
        let last = sorted.len() - 1;
        (0..JENKS_VALUES)
            .map(|k| sorted[k * last / (JENKS_VALUES - 1)])
            .collect()
    } else {
        sorted.to_vec()
    };
    let len = values.len();
    let n = n.min(len);
    if n < 2 {
        return Vec::new();
    }
    // Prefix sums of the values and of their squares.
    let mut s1 = vec![0.0; len + 1];
    let mut s2 = vec![0.0; len + 1];
    for (i, v) in values.iter().enumerate() {
        s1[i + 1] = s1[i] + v;
        s2[i + 1] = s2[i] + v * v;
    }
    // The sum of the squared deviations of values `a..b`.
    let cost = |a: usize, b: usize| {
        let m = (b - a) as f64;
        let s = s1[b] - s1[a];
        (s2[b] - s2[a] - s * s / m).max(0.0)
    };
    // best[c][j]: the least cost of values `0..j` in `c + 1` classes, and
    // the start of the last class.
    let mut best = vec![vec![(f64::INFINITY, 0usize); len + 1]; n];
    for j in 1..=len {
        best[0][j] = (cost(0, j), 0);
    }
    for c in 1..n {
        for j in c + 1..=len {
            let mut b = (f64::INFINITY, c);
            for i in c..j {
                let total = best[c - 1][i].0 + cost(i, j);
                if total < b.0 {
                    b = (total, i);
                }
            }
            best[c][j] = b;
        }
    }
    let mut starts = Vec::with_capacity(n - 1);
    let mut j = len;
    for c in (1..n).rev() {
        let i = best[c][j].1;
        starts.push(i);
        j = i;
    }
    starts.reverse();
    starts
        .into_iter()
        .filter(|&i| values[i - 1] < values[i])
        .map(|i| roundest(values[i - 1], values[i]))
        .collect()
}

/// The roundest number above `lo` and not above `hi`.
fn roundest(lo: f64, hi: f64) -> f64 {
    let top = lo.abs().max(hi.abs()).max(f64::MIN_POSITIVE);
    let first = top.log10().ceil() as i32;
    for exp in (first - 15..=first).rev() {
        let k = (scaled(lo, exp)).floor() + 1.0;
        let c = unscaled(k, exp);
        if c > lo && c <= hi {
            return c;
        }
    }
    hi
}

/// `value` rounded to a multiple of `10^exp`.
fn round_to(value: f64, exp: i32) -> f64 {
    unscaled(scaled(value, exp).round(), exp)
}

/// `value / 10^exp`.
fn scaled(value: f64, exp: i32) -> f64 {
    if exp >= 0 {
        value / 10f64.powi(exp)
    } else {
        value * 10f64.powi(-exp)
    }
}

/// `k * 10^exp`, with a division for a negative `exp`, so that the result
/// is the nearest number to the decimal value.
fn unscaled(k: f64, exp: i32) -> f64 {
    if exp >= 0 {
        k * 10f64.powi(exp)
    } else {
        k / 10f64.powi(-exp)
    }
}

/// The common values that are not null or blank, most common first, then
/// in text order, at most `max`.
pub fn categories(values: &[Value], max: usize) -> Vec<Value> {
    let mut counts: HashMap<String, (usize, &Value)> = HashMap::new();
    for v in values {
        let key = match v {
            Value::Null => continue,
            Value::Text(t) if t.trim().is_empty() => continue,
            Value::Text(t) => t.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => number_text(*n),
        };
        counts.entry(key).or_insert((0, v)).0 += 1;
    }
    let mut list: Vec<(String, usize, &Value)> =
        counts.into_iter().map(|(k, (n, v))| (k, n, v)).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.into_iter()
        .take(max)
        .map(|(_, _, v)| v.clone())
        .collect()
}

/// A class: the name and the filter of its rule. The rule of an extra
/// class takes the values that the other classes do not take.
#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: String,
    pub filter: String,
    pub extra: bool,
}

/// The classes of numbers with these inner edges, and the class
/// `NO_VALUE` if some features have no number. `left` is the left side of
/// each condition. The first class has no lower limit, and the last class
/// has no upper limit.
pub fn number_classes(left: &str, edges: &[f64], nulls: bool) -> Vec<Class> {
    let mut out = Vec::new();
    if edges.is_empty() {
        out.push(Class {
            name: "All values".to_owned(),
            filter: format!("{left} is not null"),
            extra: false,
        });
    }
    for (k, pair) in edges.windows(2).enumerate() {
        if k == 0 {
            out.push(Class {
                name: format!("< {}", number_text(pair[0])),
                filter: format!("{left} < {}", number_text(pair[0])),
                extra: false,
            });
        }
        let (a, b) = (number_text(pair[0]), number_text(pair[1]));
        out.push(Class {
            name: format!("{a} – {b}"),
            filter: format!("{left} >= {a} and {left} < {b}"),
            extra: false,
        });
    }
    if let [only] = edges {
        out.push(Class {
            name: format!("< {}", number_text(*only)),
            filter: format!("{left} < {}", number_text(*only)),
            extra: false,
        });
    }
    if let Some(last) = edges.last() {
        out.push(Class {
            name: format!("≥ {}", number_text(*last)),
            filter: format!("{left} >= {}", number_text(*last)),
            extra: false,
        });
    }
    if nulls {
        out.push(Class {
            name: NO_VALUE.to_owned(),
            filter: format!("{left} is null"),
            extra: true,
        });
    }
    out
}

/// A class for each value, and the class `OTHER_VALUES` for the other
/// values, the blank text, and the features with no value.
pub fn category_classes(left: &str, values: &[Value]) -> Vec<Class> {
    let mut out: Vec<Class> = values
        .iter()
        .map(|v| Class {
            name: match v {
                Value::Bool(true) => "True".to_owned(),
                Value::Bool(false) => "False".to_owned(),
                Value::Number(n) => number_text(*n),
                Value::Text(t) => t.clone(),
                Value::Null => String::new(),
            },
            filter: format!("{left} = {}", literal(v)),
            extra: false,
        })
        .collect();
    let list: Vec<String> = values.iter().map(literal).collect();
    let filter = if list.is_empty() {
        String::new()
    } else {
        format!("coalesce({left}, '') not in ({})", list.join(", "))
    };
    out.push(Class {
        name: OTHER_VALUES.to_owned(),
        filter,
        extra: true,
    });
    out
}

/// A value as the filter syntax writes it.
fn literal(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(*n),
        Value::Text(t) => format!("'{}'", t.replace('\'', "''")),
    }
}

/// The left side of a condition on the expression `text`: the text as it
/// is if it is a field name or one function call, or else in brackets.
pub fn left_side(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let plain = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    let quoted = text.len() >= 2
        && text.starts_with('"')
        && text.ends_with('"')
        && !text[1..text.len() - 1].replace("\"\"", "").contains('"');
    if plain || quoted || one_call(text) {
        text.to_owned()
    } else {
        format!("({text})")
    }
}

/// True if the text is a name and brackets that close at its end.
fn one_call(text: &str) -> bool {
    let Some(open) = text.find('(') else {
        return false;
    };
    let name = &text[..open];
    let mut chars = name.chars();
    if !(chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_'))
    {
        return false;
    }
    let (mut depth, mut quote) = (0i32, None);
    for (i, c) in text[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return open + i + 1 == text.len();
                }
            }
            _ => {}
        }
    }
    false
}

/// A number as the filter syntax writes it.
pub fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// The classes of the values of the expression `text` on the features of
/// a layer, with `n` classes of numbers. `None` if the text is not an
/// expression or if no feature has a value.
pub fn classes_of(layer: &Layer, text: &str, method: Method, n: usize) -> Option<Vec<Class>> {
    let expr = Expr::parse(text).ok()?;
    let all = values(layer, &expr);
    let left = left_side(text);
    let classes = match numbers(&all) {
        Some((sorted, nulls)) => number_classes(&left, &breaks(&sorted, method, n), nulls > 0),
        None => {
            let common = categories(&all, MAX_CATEGORIES);
            if common.is_empty() {
                return None;
            }
            category_classes(&left, &common)
        }
    };
    Some(classes)
}

/// The colors of `n` classes from a ramp. A qualitative ramp gives its
/// colors in order, and another ramp gives steps from its first color to
/// its last color.
pub fn class_colors(ramp: &[Rgb], n: usize, qualitative: bool, reverse: bool) -> Vec<Color> {
    let mut rgb = if qualitative {
        crate::ramps::cycle(ramp, n)
    } else {
        crate::ramps::sample(ramp, n)
    };
    if reverse {
        rgb.reverse();
    }
    rgb.iter().map(|c| Color::rgb(c[0], c[1], c[2])).collect()
}

/// The property that steps from the first class to the last class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    None,
    /// The stroke width of a line or a polygon, in points.
    Width(f64, f64),
    /// The size of a point marker or of a point symbol, in points.
    Size(f64, f64),
}

/// A rule for each class, with the symbolizers of `template` and the color
/// of the class. `colors` has a color for each class that is not extra.
pub fn make_rules(
    template: &Rule,
    kind: GeometryKind,
    classes: &[Class],
    colors: &[Color],
    step: Step,
) -> Vec<Rule> {
    let steps = classes.iter().filter(|c| !c.extra).count();
    let mut names: Vec<String> = Vec::new();
    let mut colors = colors.iter();
    let mut k = 0;
    classes
        .iter()
        .map(|class| {
            let name = (1..)
                .map(|i| match i {
                    1 => class.name.clone(),
                    _ => format!("{} {i}", class.name),
                })
                .find(|n| !names.contains(n))
                .unwrap_or_default();
            names.push(name.clone());
            let mut rule = Rule::new(name, template.symbolizers.clone());
            rule.filter = Filter::new(&class.filter);
            let color = if class.extra {
                OTHER_COLOR
            } else {
                *colors.next().unwrap_or(&OTHER_COLOR)
            };
            set_color(&mut rule, kind, color);
            if !class.extra {
                let t = if steps < 2 {
                    0.0
                } else {
                    k as f64 / (steps - 1) as f64
                };
                set_step(&mut rule, step, t);
                k += 1;
            }
            rule
        })
        .collect()
}

/// Sets the color of a class: the stroke of a line, the fill of a polygon
/// or a marker, or the tint of a point symbol.
fn set_color(rule: &mut Rule, kind: GeometryKind, color: Color) {
    match kind {
        GeometryKind::Line => {
            if let Some(mut s) = rule.stroke() {
                s.color = color;
                rule.set_stroke(Some(s));
            }
        }
        GeometryKind::Polygon => rule.set_fill(Some(color)),
        GeometryKind::Point => {
            if let Some(mut s) = rule.symbol().cloned() {
                s.tint = Some(Property::constant(color));
                rule.set_symbol(Some(s));
            } else {
                rule.set_fill(Some(color));
            }
        }
    }
}

/// Sets the stepped property at `t` from the first class (0) to the last (1).
fn set_step(rule: &mut Rule, step: Step, t: f64) {
    match step {
        Step::None => {}
        Step::Width(a, b) => {
            let width = a + (b - a) * t;
            let stroke = match rule.stroke() {
                Some(mut s) => {
                    s.width = width;
                    s
                }
                None => Stroke::solid(Color::rgb(90, 90, 90), width),
            };
            rule.set_stroke(Some(stroke));
        }
        Step::Size(a, b) => {
            let size = a + (b - a) * t;
            if let Some(mut s) = rule.symbol().cloned() {
                s.size = Property::constant(size);
                rule.set_symbol(Some(s));
            } else if let Some(radius) = rule.symbolizers.iter_mut().find_map(|s| match s {
                vale_style::Symbolizer::Marker { radius, .. } => Some(radius),
                _ => None,
            }) {
                *radius = Property::constant(size / 2.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three groups with large gaps between them.
    const GROUPS: [f64; 9] = [1.0, 2.0, 3.0, 10.0, 11.0, 12.0, 20.0, 21.0, 22.0];

    #[test]
    fn equal_intervals_have_the_same_width() {
        assert_eq!(breaks(&GROUPS, Method::EqualIntervals, 3), vec![8.0, 15.0]);
        let tenths = [0.0, 0.25, 1.0];
        assert_eq!(
            breaks(&tenths, Method::EqualIntervals, 4),
            vec![0.25, 0.5, 0.75]
        );
    }

    #[test]
    fn quantiles_have_the_same_count() {
        assert_eq!(breaks(&GROUPS, Method::Quantiles, 3), vec![10.0, 20.0]);
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(breaks(&ten, Method::Quantiles, 5), vec![3.0, 5.0, 7.0, 9.0]);
        let skew = [1.0, 2.0, 3.0, 4.0, 5.0, 100.0];
        assert_eq!(breaks(&skew, Method::Quantiles, 2), vec![4.0]);
    }

    #[test]
    fn natural_breaks_are_in_the_largest_gaps() {
        assert_eq!(breaks(&GROUPS, Method::NaturalBreaks, 3), vec![10.0, 20.0]);
        let skew = [1.0, 2.0, 3.0, 4.0, 5.0, 100.0];
        assert_eq!(breaks(&skew, Method::NaturalBreaks, 2), vec![100.0]);
        let gaps = [0.11, 0.12, 0.13, 0.52, 0.53];
        assert_eq!(breaks(&gaps, Method::NaturalBreaks, 2), vec![0.2]);
    }

    #[test]
    fn equal_values_stay_in_one_class() {
        let ties = [1.0, 1.0, 1.0, 1.0, 2.0, 3.0];
        assert_eq!(breaks(&ties, Method::Quantiles, 3), vec![2.0, 3.0]);
        let one = [4.0, 4.0, 4.0];
        for method in Method::ALL {
            assert!(breaks(&one, method, 3).is_empty(), "{method:?}");
        }
    }

    #[test]
    fn number_classes_have_open_ends_and_conditions() {
        let classes = number_classes("abs(x)", &[10.0, 20.0], true);
        let filters: Vec<&str> = classes.iter().map(|c| c.filter.as_str()).collect();
        assert_eq!(
            filters,
            [
                "abs(x) < 10",
                "abs(x) >= 10 and abs(x) < 20",
                "abs(x) >= 20",
                "abs(x) is null",
            ]
        );
        let names: Vec<&str> = classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["< 10", "10 – 20", "≥ 20", NO_VALUE]);
        for f in filters {
            assert!(Filter::new(f).error().is_none(), "{f}");
        }
    }

    #[test]
    fn text_gives_a_class_for_each_value_and_one_for_the_others() {
        let values: Vec<Value> = ["lake", "forest", "lake", "", "it's"]
            .iter()
            .map(|t| Value::Text((*t).to_owned()))
            .chain([Value::Null])
            .collect();
        let common = categories(&values, MAX_CATEGORIES);
        let classes = category_classes("kind", &common);
        let names: Vec<&str> = classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["lake", "forest", "it's", OTHER_VALUES]);
        assert_eq!(classes[2].filter, "kind = 'it''s'");
        assert_eq!(
            classes[3].filter,
            "coalesce(kind, '') not in ('lake', 'forest', 'it''s')"
        );
        let attrs = |v: Value| std::collections::BTreeMap::from([("kind".to_owned(), v)]);
        let other = Filter::new(&classes[3].filter);
        for (v, inside) in [
            (Value::Null, true),
            (Value::Text(String::new()), true),
            (Value::Text("sea".into()), true),
            (Value::Text("lake".into()), false),
        ] {
            let a = attrs(v.clone());
            let ctx = Context {
                attributes: &a,
                scale: 0.0,
                geometry: GeometryKind::Polygon,
            };
            assert_eq!(other.matches(&ctx), inside, "{v:?}");
        }
    }

    #[test]
    fn the_left_side_of_an_expression_has_brackets() {
        assert_eq!(left_side("kind"), "kind");
        assert_eq!(left_side("\"land use\""), "\"land use\"");
        assert_eq!(left_side("area('km2')"), "area('km2')");
        assert_eq!(left_side("a * 2"), "(a * 2)");
        assert_eq!(left_side("f(a) + g(b)"), "(f(a) + g(b))");
    }

    #[test]
    fn rules_take_the_colors_and_the_steps() {
        let template = vale_style::LayerStyle::default_for(GeometryKind::Line, 0, &[])
            .style
            .rules[0]
            .clone();
        let classes = number_classes("x", &[1.0, 2.0], true);
        let colors = [
            Color::rgb(1, 0, 0),
            Color::rgb(2, 0, 0),
            Color::rgb(3, 0, 0),
        ];
        let rules = make_rules(
            &template,
            GeometryKind::Line,
            &classes,
            &colors,
            Step::Width(1.0, 3.0),
        );
        let strokes: Vec<Stroke> = rules.iter().filter_map(Rule::stroke).collect();
        let widths: Vec<f64> = strokes.iter().map(|s| s.width).collect();
        assert_eq!(widths[..3], [1.0, 2.0, 3.0]);
        assert_eq!(strokes[1].color, colors[1]);
        assert_eq!(strokes[3].color, OTHER_COLOR);
        assert_eq!(rules[3].name, NO_VALUE);
    }
}
